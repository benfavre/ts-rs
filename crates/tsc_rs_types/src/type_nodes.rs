//! Source-aware member serialization, adapted from pingdotgg/ts-rust's
//! nodebuilder_impl_p2.rs and nodebuilder_impl_p3.rs (see NOTICE.txt). The native types remain authoritative;
//! a declaration's annotation is reusable only while its resolved type survives
//! instantiation. Source provenance never participates in type identity.

use crate::{ConstructorType, FunctionType, Type, TypeChecker};
use std::sync::Arc;
use tsc_rs_ast::*;

#[derive(Debug, Clone, Default)]
pub struct SignatureSource(Option<Arc<SignatureDeclaration>>);
impl SignatureSource {
    pub(crate) fn with_original_this_parameter(mut self) -> Self {
        if let Some(source) = &mut self.0 {
            Arc::make_mut(source).original_this_parameter = true;
        }
        self
    }
}
impl PartialEq for SignatureSource {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for SignatureSource {}
impl std::hash::Hash for SignatureSource {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

#[derive(Debug, Clone)]
struct Annotation {
    ty: Type,
    text: String,
}
#[derive(Debug, Clone)]
struct ParameterDeclaration {
    name: String,
    annotation: Option<Annotation>,
}
#[derive(Debug, Clone)]
struct SignatureDeclaration {
    original_this_parameter: bool,
    parameters: Vec<ParameterDeclaration>,
    type_parameters: Vec<(String, Option<Annotation>)>,
    return_type: Option<Annotation>,
}

/// Display-only declarations. Cloning an object shares its provenance; a type
/// substitution deliberately does not rewrite the original annotation's type.
#[derive(Debug, Clone, Default)]
pub struct ObjectSource(Option<Arc<ObjectDeclaration>>);
#[derive(Debug, Clone, Default)]
struct ObjectDeclaration {
    properties: Vec<(String, PropertyDeclaration)>,
    index_readonly: bool,
}
#[derive(Debug, Clone, Default)]
struct PropertyDeclaration {
    annotation: Option<Annotation>,
    name_text: Option<String>,
    method: bool,
}
impl ObjectSource {
    fn property(&self, name: &str) -> Option<&PropertyDeclaration> {
        self.0
            .as_ref()?
            .properties
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }
    fn set_property(&mut self, name: String, declaration: PropertyDeclaration) {
        let source = Arc::make_mut(self.0.get_or_insert_with(Default::default));
        if let Some((_, value)) = source.properties.iter_mut().find(|(key, _)| key == &name) {
            *value = declaration;
        } else {
            source.properties.push((name, declaration));
        }
    }
    pub(crate) fn copy_property_from(&mut self, other: &Self, name: &str) {
        if self.0.is_some() || other.0.is_some() {
            self.set_property(
                name.to_owned(),
                other.property(name).cloned().unwrap_or_default(),
            );
        }
    }
    pub(crate) fn copy_index_from(&mut self, other: &Self) {
        if self.0.is_some() || other.0.is_some() {
            Arc::make_mut(self.0.get_or_insert_with(Default::default)).index_readonly =
                other.0.as_ref().is_some_and(|source| source.index_readonly);
        }
    }
}

impl TypeChecker {
    fn source_annotation(&self, node: &TypeNode, ty: &Type) -> Option<Annotation> {
        // JSDoc annotations are desugared nodes, not parse-tree type nodes.
        if self
            .expression_type_comments
            .iter()
            .any(|s| s.start <= node.span.start && node.span.start < s.end)
        {
            return None;
        }
        Some(Annotation {
            ty: ty.clone(),
            text: print_annotation(node, self.current_source.as_deref().unwrap_or_default())?,
        })
    }

    fn property_declaration(
        &self,
        name: &PropName,
        annotation: Option<&TypeNode>,
        properties: &[(String, Arc<Type>)],
        accessor: bool,
    ) -> PropertyDeclaration {
        let key = self.prop_name_to_string(name);
        let ty = properties
            .iter()
            .rev()
            .find(|(name, _)| name == &key)
            .map(|(_, ty)| match ty.as_ref() {
                Type::Optional(inner) => inner.as_ref(),
                ty => ty,
            });
        let name_text = if let PropName::String(_, span) = name {
            let synthetic = property_name(&key);
            synthetic.starts_with('\'').then(|| {
                let quote = self
                    .current_source
                    .as_deref()
                    .and_then(|source| source.get(span.start as usize..span.end as usize))
                    .and_then(|text| text.chars().next())
                    .filter(|ch| *ch == '"')
                    .unwrap_or('\'');
                quote_string(&key, quote)
            })
        } else {
            None
        };
        PropertyDeclaration {
            annotation: annotation.and_then(|node| {
                ty.and_then(|ty| {
                    if accessor {
                        // Getter and setter annotations can describe different types.
                        self.source_annotation(node, &self.resolve_type_node(node))
                    } else {
                        // This annotation already produced the member type.
                        // Re-resolving nested object annotations here would
                        // revisit each subtree exponentially with its depth.
                        self.source_annotation(node, ty)
                    }
                })
            }),
            name_text,
            method: false,
        }
    }

    pub(crate) fn object_declaration_source(
        &self,
        members: &[TypeMember],
        properties: &[(String, Arc<Type>)],
    ) -> ObjectSource {
        if self.expression_type_spans.is_none() {
            return ObjectSource::default();
        }
        let mut source = ObjectSource(Some(Arc::new(ObjectDeclaration::default())));
        for member in members {
            let (name, annotation, method, setter) = match &member.kind {
                TypeMemberKind::PropertySig(property) => {
                    (&property.name, property.type_ann.as_ref(), false, false)
                }
                TypeMemberKind::MethodSig(method) => (&method.name, None, true, false),
                TypeMemberKind::GetAccessorSig(accessor) => {
                    (&accessor.name, accessor.return_type.as_ref(), false, false)
                }
                TypeMemberKind::SetAccessorSig(accessor) => (
                    &accessor.name,
                    accessor.params.first().and_then(|p| p.type_ann.as_ref()),
                    false,
                    true,
                ),
                TypeMemberKind::IndexSig(index) => {
                    Arc::make_mut(source.0.as_mut().unwrap()).index_readonly =
                        index.modifiers & MOD_READONLY != 0;
                    continue;
                }
                _ => continue,
            };
            let key = self.prop_name_to_string(name);
            let mut declaration = self.property_declaration(
                name,
                annotation,
                properties,
                matches!(
                    member.kind,
                    TypeMemberKind::GetAccessorSig(_) | TypeMemberKind::SetAccessorSig(_)
                ),
            );
            declaration.method = method;
            if let Some(existing) = source.property(&key) {
                if setter && existing.annotation.is_some() || annotation.is_none() && !method {
                    declaration.annotation = existing.annotation.clone();
                }
            }
            source.set_property(key, declaration);
        }
        source
    }

    pub(crate) fn object_literal_source(
        &self,
        members: &[ObjLitProp],
        properties: &[(String, Arc<Type>)],
    ) -> ObjectSource {
        if self.expression_type_spans.is_none() {
            return ObjectSource::default();
        }
        let mut source = ObjectSource(Some(Arc::new(ObjectDeclaration::default())));
        let mut direct_methods = rustc_hash::FxHashSet::default();
        let mut direct_accessors = rustc_hash::FxHashSet::default();
        for member in members {
            let (name, annotation, method, accessor, setter) = match member {
                ObjLitProp::Property(property) => (&property.key, None, false, false, false),
                ObjLitProp::Method(method) => (&method.name, None, true, false, false),
                ObjLitProp::Get(accessor) => (
                    &accessor.name,
                    accessor.return_type.as_ref(),
                    false,
                    true,
                    false,
                ),
                ObjLitProp::Set(accessor) => (
                    &accessor.name,
                    accessor.params.first().and_then(|p| p.type_ann.as_ref()),
                    false,
                    true,
                    true,
                ),
                ObjLitProp::Shorthand(name, _) | ObjLitProp::ShorthandDefault(name, _, _) => {
                    direct_accessors.remove(name.as_str());
                    source.set_property(name.to_string(), PropertyDeclaration::default());
                    continue;
                }
                ObjLitProp::Spread(expression, _) => {
                    let mut spread = ObjectSource::default();
                    self.spread_node_source(
                        &self.infer_expr_type(expression),
                        &mut spread,
                        &mut rustc_hash::FxHashSet::default(),
                    );
                    if let Some(spread) = spread.0 {
                        for (name, declaration) in &spread.properties {
                            direct_methods.remove(name);
                            direct_accessors.remove(name);
                            source.set_property(name.clone(), declaration.clone());
                        }
                    }
                    continue;
                }
            };
            let key = self.prop_name_to_string(name);
            let mut declaration = self.property_declaration(name, annotation, properties, accessor);
            if method {
                direct_methods.insert(key.clone());
            }
            // Duplicate direct declarations share a symbol's method flag.
            // A later spread replaces that symbol for each incoming member.
            declaration.method = direct_methods.contains(&key);
            let paired_accessor = accessor && !direct_accessors.insert(key.clone());
            if !accessor {
                direct_accessors.remove(&key);
            }
            if paired_accessor {
                if let Some(existing) = source.property(&key) {
                    if setter && existing.annotation.is_some() || annotation.is_none() {
                        declaration.annotation = existing.annotation.clone();
                    }
                }
            }
            source.set_property(key, declaration);
        }
        source
    }

    fn spread_node_source(
        &self,
        ty: &Type,
        source: &mut ObjectSource,
        seen: &mut rustc_hash::FxHashSet<Type>,
    ) {
        if !seen.insert(ty.clone()) {
            return;
        }
        // Resolve once; keep a path set for recursively expanded intersections.
        let expanded = if let Type::TypeReference(name, arguments) = ty {
            self.resolve_type_reference_to_object(name, arguments)
        } else {
            None
        };
        match expanded.as_ref().unwrap_or(ty) {
            Type::ObjectType(object) => {
                for (name, _) in &object.properties {
                    let mut declaration = object
                        .node_source
                        .property(name)
                        .cloned()
                        .unwrap_or_default();
                    if object.node_source.0.is_none() {
                        declaration.method = object.method_names.contains(name);
                    }
                    source.set_property(name.clone(), declaration);
                }
            }
            Type::Intersection(members) => {
                for member in members.iter() {
                    self.spread_node_source(member, source, seen);
                }
            }
            _ => {}
        }
        seen.remove(ty);
    }

    pub(crate) fn signature_source(
        &self,
        parameters: &[Param],
        types: &[(String, Type)],
        type_parameters: Option<&[TypeParam]>,
        return_node: Option<&TypeNode>,
        return_type: &Type,
    ) -> SignatureSource {
        if self.expression_type_spans.is_none() {
            return SignatureSource::default();
        }
        let annotation = |node: &TypeNode, ty: &Type| self.source_annotation(node, ty);
        SignatureSource(Some(Arc::new(SignatureDeclaration {
            original_this_parameter: false,
            parameters: parameters
                .iter()
                .zip(types)
                .map(|(parameter, (_, ty))| ParameterDeclaration {
                    name: print_binding(
                        &parameter.name,
                        self.current_source.as_deref().unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                    annotation: parameter
                        .type_ann
                        .as_ref()
                        .and_then(|node| annotation(node, ty)),
                })
                .collect(),
            type_parameters: type_parameters
                .unwrap_or_default()
                .iter()
                .map(|parameter| {
                    (
                        parameter.name.clone(),
                        parameter
                            .constraint
                            .as_ref()
                            .and_then(|node| annotation(node, &self.resolve_type_node(node))),
                    )
                })
                .collect(),
            return_type: return_node.and_then(|node| annotation(node, return_type)),
        })))
    }

    pub(crate) fn overload_signature_source(
        &self,
        signature: &tsc_rs_symbols::OverloadSignature,
        types: &[(String, Type)],
        return_type: &Type,
    ) -> SignatureSource {
        if self.expression_type_spans.is_none() {
            return SignatureSource::default();
        }
        let annotation = |node: &TypeNode, ty: &Type| {
            Some(Annotation {
                ty: ty.clone(),
                text: print_annotation(node, self.current_source.as_deref().unwrap_or_default())?,
            })
        };
        SignatureSource(Some(Arc::new(SignatureDeclaration {
            original_this_parameter: false,
            parameters: signature
                .params
                .iter()
                .zip(types)
                .map(|(parameter, (_, ty))| ParameterDeclaration {
                    name: parameter.name.clone(),
                    annotation: parameter
                        .type_ann
                        .as_ref()
                        .and_then(|node| annotation(node, ty)),
                })
                .collect(),
            type_parameters: signature
                .type_params
                .iter()
                .map(|name| (name.clone(), None))
                .collect(),
            return_type: signature
                .return_type
                .as_ref()
                .and_then(|node| annotation(node, return_type)),
        })))
    }
}

/// The printer records provenance when writing each node, before concatenating
/// text. In particular, it never infers source ranges from the resulting string.
#[derive(Default)]
pub(crate) struct PrintedType {
    pub text: String,
    pub underline: String,
}
impl PrintedType {
    fn write(&mut self, text: &str, original: bool) {
        self.text.push_str(text);
        self.underline.extend(std::iter::repeat_n(
            if original { ' ' } else { '^' },
            text.encode_utf16().count(),
        ));
    }
    fn synthetic(&mut self, text: &str) {
        self.write(text, false);
    }
    fn original(&mut self, text: &str) {
        self.write(text, true);
    }
    fn annotation(&mut self, ty: &Type, annotation: Option<&Annotation>) {
        if let Some(annotation) = annotation.filter(|a| a.ty == *ty) {
            self.original(&annotation.text);
        } else {
            self.ty(ty);
        }
    }
    fn signature(&mut self, signature: &FunctionType, separator: &str) {
        self.signature_parts(
            &signature.params,
            &signature.return_type,
            &signature.type_params,
            &signature.type_param_constraints,
            &signature.type_param_defaults,
            &signature.source,
            separator,
        );
    }
    fn constructor(&mut self, signature: &ConstructorType, separator: &str) {
        if signature.is_abstract {
            self.synthetic("abstract ");
        }
        self.synthetic("new ");
        self.signature_parts(
            &signature.params,
            &signature.return_type,
            &signature.type_params,
            &signature.type_param_constraints,
            &signature.type_param_defaults,
            &signature.source,
            separator,
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn signature_parts(
        &mut self,
        parameters: &[(String, Type)],
        return_type: &Type,
        type_parameters: &[String],
        constraints: &[Option<Type>],
        defaults: &[Option<Type>],
        source: &SignatureSource,
        separator: &str,
    ) {
        let source = source.0.as_deref();
        if !type_parameters.is_empty() {
            self.synthetic("<");
            for (i, name) in type_parameters.iter().enumerate() {
                if i != 0 {
                    self.synthetic(", ");
                }
                let declaration = source
                    .and_then(|s| s.type_parameters.get(i))
                    .filter(|(n, _)| n == name);
                self.write(name, declaration.is_some());
                if let Some(Some(constraint)) = constraints.get(i) {
                    self.synthetic(" extends ");
                    self.annotation(
                        constraint,
                        declaration.and_then(|(_, annotation)| annotation.as_ref()),
                    );
                }
                if let Some(Some(default)) = defaults.get(i) {
                    self.synthetic(" = ");
                    self.ty(default);
                }
            }
            self.synthetic(">");
        }
        self.synthetic("(");
        for (i, (name, ty)) in parameters.iter().enumerate() {
            if i != 0 {
                self.synthetic(", ");
            }
            let (name, rest, optional) = if let Some(name) = name.strip_prefix("...") {
                (name, true, false)
            } else if let Some(name) = name.strip_prefix('?') {
                (name, false, true)
            } else {
                (name.as_str(), false, false)
            };
            let declaration = source
                .and_then(|s| s.parameters.get(i))
                .filter(|p| p.name == name || name == "_");
            if rest {
                self.synthetic("...");
            }
            if let Some(declaration) = declaration.filter(|p| !p.name.is_empty()) {
                self.original(&declaration.name);
            } else {
                self.synthetic(name);
            }
            if optional {
                self.synthetic("?");
            }
            // Serializing an inferred returned function reuses its explicit
            // receiver declaration; printing the function itself synthesizes it.
            if name == "this"
                && source.is_some_and(|s| s.original_this_parameter)
                && declaration
                    .and_then(|p| p.annotation.as_ref())
                    .is_some_and(|a| a.ty == *ty)
            {
                self.original(": ");
            } else {
                self.synthetic(": ");
            }
            self.annotation(ty, declaration.and_then(|p| p.annotation.as_ref()));
        }
        self.synthetic(")");
        self.synthetic(separator);
        self.annotation(return_type, source.and_then(|s| s.return_type.as_ref()));
    }
    fn parenthesized(&mut self, ty: &Type, parens: bool) {
        if parens {
            self.synthetic("(");
        }
        self.ty(ty);
        if parens {
            self.synthetic(")");
        }
    }
    fn ty(&mut self, ty: &Type) {
        match ty {
            Type::Function(signature) => self.signature(signature, " => "),
            Type::Constructor(signature) => self.constructor(signature, " => "),
            Type::Array(element) if matches!(element.as_ref(), Type::UniqueSymbol(_)) => {
                self.synthetic(&ty.display_string_single_line());
            }
            Type::Array(element) => {
                self.parenthesized(
                    element,
                    matches!(
                        element.as_ref(),
                        Type::Union(_)
                            | Type::Intersection(_)
                            | Type::Conditional { .. }
                            | Type::Function(_)
                            | Type::Constructor(_)
                            | Type::Readonly(_)
                    ),
                );
                self.synthetic("[]");
            }
            Type::Tuple(elements) => {
                self.synthetic("[");
                for (i, element) in elements.iter().enumerate() {
                    if i != 0 {
                        self.synthetic(", ");
                    }
                    self.ty(element);
                }
                self.synthetic("]");
            }
            Type::Union(members) => {
                for (i, member) in members
                    .iter()
                    .filter(|t| !matches!(t, Type::Null | Type::Undefined))
                    .chain(members.iter().filter(|t| matches!(t, Type::Null)))
                    .chain(members.iter().filter(|t| matches!(t, Type::Undefined)))
                    .enumerate()
                {
                    if i != 0 {
                        self.synthetic(" | ");
                    }
                    self.parenthesized(
                        member,
                        matches!(member, Type::Function(_) | Type::Constructor(_)),
                    );
                }
            }
            Type::Intersection(members)
                if members.len() > 1 && members.iter().all(|t| matches!(t, Type::Function(_))) =>
            {
                self.synthetic("{ ");
                for member in members.iter() {
                    if let Type::Function(signature) = member {
                        self.signature(signature, ": ");
                        self.synthetic("; ");
                    }
                }
                self.synthetic("}");
            }
            Type::Intersection(members) => {
                for (i, member) in members.iter().enumerate() {
                    if i != 0 {
                        self.synthetic(" & ");
                    }
                    self.parenthesized(
                        member,
                        matches!(
                            member,
                            Type::Function(_) | Type::Constructor(_) | Type::Union(_)
                        ),
                    );
                }
            }
            Type::ObjectType(object) => {
                if object.properties.is_empty() && object.index_signature.is_none() {
                    if let [signature] = object.call_signatures.as_slice() {
                        if object.construct_signatures.is_empty() {
                            self.signature(signature, " => ");
                            return;
                        }
                    }
                    if let [signature] = object.construct_signatures.as_slice() {
                        if object.call_signatures.is_empty() {
                            self.constructor(signature, " => ");
                            return;
                        }
                    }
                }
                if object.properties.is_empty()
                    && object.index_signature.is_none()
                    && object.call_signatures.is_empty()
                    && object.construct_signatures.is_empty()
                {
                    self.synthetic("{}");
                    return;
                }
                self.synthetic("{ ");
                for signature in &object.call_signatures {
                    self.signature(signature, ": ");
                    self.synthetic("; ");
                }
                for signature in &object.construct_signatures {
                    self.constructor(signature, ": ");
                    self.synthetic("; ");
                }
                if let Some((key, value)) = &object.index_signature {
                    if object
                        .node_source
                        .0
                        .as_ref()
                        .is_some_and(|source| source.index_readonly)
                    {
                        self.synthetic("readonly ");
                    }
                    self.synthetic("[");
                    self.synthetic(object.index_signature_name.as_deref().unwrap_or("x"));
                    self.synthetic(": ");
                    self.ty(key);
                    self.synthetic("]: ");
                    self.ty(value);
                    self.synthetic("; ");
                }
                for (name, ty) in &object.properties {
                    let mut optional = false;
                    let name = name
                        .strip_prefix('?')
                        .or_else(|| name.strip_suffix('?'))
                        .inspect(|_| {
                            optional = true;
                        })
                        .unwrap_or(name);
                    let ty = if let Type::Optional(inner) = ty.as_ref() {
                        optional = true;
                        inner.as_ref()
                    } else {
                        ty.as_ref()
                    };
                    let declaration = object.node_source.property(name);
                    let property = declaration
                        .and_then(|d| d.name_text.as_deref())
                        .map(str::to_owned)
                        .unwrap_or_else(|| property_name(name));
                    if if object.node_source.0.is_some() {
                        declaration.is_some_and(|declaration| declaration.method)
                    } else {
                        object.method_names.iter().any(|method| method == name)
                    } {
                        let signatures: Vec<_> = match ty {
                            Type::Function(signature) => vec![signature],
                            Type::Intersection(members) => members
                                .iter()
                                .filter_map(|m| {
                                    if let Type::Function(s) = m {
                                        Some(s)
                                    } else {
                                        None
                                    }
                                })
                                .collect(),
                            _ => Vec::new(),
                        };
                        if !signatures.is_empty() {
                            for signature in signatures {
                                self.synthetic(&property);
                                if optional {
                                    self.synthetic("?");
                                }
                                self.signature(signature, ": ");
                                self.synthetic("; ");
                            }
                            continue;
                        }
                    }
                    if object.readonly_names.iter().any(|n| n == name) {
                        self.synthetic("readonly ");
                    }
                    self.synthetic(&property);
                    if optional {
                        self.synthetic("?");
                    }
                    self.synthetic(": ");
                    self.annotation(ty, declaration.and_then(|d| d.annotation.as_ref()));
                    self.synthetic("; ");
                }
                self.synthetic("}");
            }
            Type::TypeReference(name, arguments)
                if !arguments.is_empty() && name != "Array" && name != "ReadonlyArray" =>
            {
                self.synthetic(crate::scoped_interface_display_name(name));
                self.synthetic("<");
                for (i, argument) in arguments.iter().enumerate() {
                    if i != 0 {
                        self.synthetic(", ");
                    }
                    self.ty(argument);
                }
                self.synthetic(">");
            }
            Type::Readonly(inner) | Type::Rest(inner) | Type::Keyof(inner) => {
                self.synthetic(match ty {
                    Type::Readonly(_) => "readonly ",
                    Type::Rest(_) => "...",
                    _ => "keyof ",
                });
                self.parenthesized(
                    inner,
                    matches!(ty, Type::Keyof(_))
                        && matches!(inner.as_ref(), Type::Union(_) | Type::Intersection(_)),
                );
            }
            Type::Optional(inner) => {
                self.parenthesized(
                    inner,
                    matches!(inner.as_ref(), Type::Union(_) | Type::Intersection(_)),
                );
                self.synthetic("?");
            }
            Type::IndexedAccess(object, index) => {
                self.ty(object);
                self.synthetic("[");
                self.ty(index);
                self.synthetic("]");
            }
            Type::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                self.ty(check);
                self.synthetic(" extends ");
                self.ty(extends);
                self.synthetic(" ? ");
                self.ty(true_type);
                self.synthetic(" : ");
                self.ty(false_type);
            }
            _ => self.synthetic(&ty.display_string_single_line()),
        }
    }
}

pub(crate) fn print_type(ty: &Type) -> PrintedType {
    let mut result = PrintedType::default();
    result.ty(ty);
    result
}

fn quote_string(value: &str, quote: char) -> String {
    use std::fmt::Write;
    let mut text = String::from(quote);
    let mut units = value.encode_utf16().peekable();
    while let Some(unit) = units.next() {
        match unit {
            0 => text.push_str(
                if units.peek().is_some_and(|unit| (48..=57).contains(unit)) {
                    "\\x00"
                } else {
                    "\\0"
                },
            ),
            11 => text.push_str("\\v"),
            8 => text.push_str("\\b"),
            9 => text.push_str("\\t"),
            10 => text.push_str("\\n"),
            12 => text.push_str("\\f"),
            13 => text.push_str("\\r"),
            92 => text.push_str("\\\\"),
            unit if unit == quote as u16 => {
                text.push('\\');
                text.push(quote);
            }
            32..=127 => text.push(char::from_u32(unit as u32).unwrap()),
            unit => {
                write!(text, "\\u{unit:04X}").unwrap();
            }
        }
    }
    text.push(quote);
    text
}

fn property_name(name: &str) -> String {
    let mut chars = name.chars();
    if chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        || !name.is_empty() && name.chars().all(|c| c.is_ascii_digit())
    {
        name.into()
    } else {
        format!("'{name}'")
    }
}

fn expression_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.to_string()),
        ExprKind::Member(member) => Some(format!(
            "{}.{}",
            expression_name(&member.object)?,
            member.property
        )),
        _ => None,
    }
}
fn print_binding(pattern: &Pat, source: &str) -> Option<String> {
    match &pattern.kind {
        PatKind::Ident(name) => Some(name.to_string()),
        PatKind::Assign(pattern, _) => print_binding(pattern, source),
        PatKind::Rest(pattern) => Some(format!("...{}", print_binding(pattern, source)?)),
        PatKind::Array(elements) => Some(format!(
            "[{}]",
            elements
                .iter()
                .map(|element| match element {
                    Some(ArrayPatElem::Pat(pattern)) => print_binding(pattern, source),
                    Some(ArrayPatElem::Rest(pattern)) =>
                        Some(format!("...{}", print_binding(pattern, source)?)),
                    None => Some(String::new()),
                })
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
        PatKind::Object(properties) => Some(format!(
            "{{ {} }}",
            properties
                .iter()
                .map(|property| match property {
                    ObjPatProp::KeyValue(name, pattern) => Some(format!(
                        "{}: {}",
                        print_property(name, source)?,
                        print_binding(pattern, source)?
                    )),
                    ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) =>
                        Some(name.to_string()),
                    ObjPatProp::Rest(pattern) =>
                        Some(format!("...{}", print_binding(pattern, source)?)),
                })
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        )),
    }
}
fn print_property(name: &PropName, source: &str) -> Option<String> {
    Some(match name {
        PropName::Ident(name, _) => name.to_string(),
        PropName::String(_, span) | PropName::Number(_, span) => source
            .get(span.start as usize..span.end as usize)?
            .to_string(),
        PropName::Computed(expr, _) => format!("[{}]", expression_name(expr)?),
        _ => return None,
    })
}

fn print_annotation(node: &TypeNode, source: &str) -> Option<String> {
    let print = |node: &TypeNode| print_annotation(node, source);
    let list = |nodes: &[TypeNode], separator: &str| {
        nodes
            .iter()
            .map(print)
            .collect::<Option<Vec<_>>>()
            .map(|nodes| nodes.join(separator))
    };
    Some(match &node.kind {
        TypeNodeKind::Keyword(keyword) => match keyword {
            KeywordTypeKind::Any => "any",
            KeywordTypeKind::Unknown => "unknown",
            KeywordTypeKind::Number => "number",
            KeywordTypeKind::BigInt => "bigint",
            KeywordTypeKind::String => "string",
            KeywordTypeKind::Boolean => "boolean",
            KeywordTypeKind::Void => "void",
            KeywordTypeKind::Undefined => "undefined",
            KeywordTypeKind::Null => "null",
            KeywordTypeKind::Never => "never",
            KeywordTypeKind::Object => "object",
            KeywordTypeKind::Symbol => "symbol",
            KeywordTypeKind::Intrinsic => "intrinsic",
        }
        .into(),
        TypeNodeKind::Reference(reference) => {
            let mut text = expression_name(&reference.name)?;
            if let Some(arguments) = &reference.type_args {
                text += &format!("<{}>", list(arguments, ", ")?);
            }
            text
        }
        TypeNodeKind::Array(inner) => format!("{}[]", print(inner)?),
        TypeNodeKind::Paren(inner) => format!("({})", print(inner)?),
        TypeNodeKind::Optional(inner) => format!("{}?", print(inner)?),
        TypeNodeKind::Rest(inner) => format!("...{}", print(inner)?),
        TypeNodeKind::Keyof(inner) => format!("keyof {}", print(inner)?),
        TypeNodeKind::Unique(inner) => format!("unique {}", print(inner)?),
        TypeNodeKind::Readonly(inner) => format!("readonly {}", print(inner)?),
        TypeNodeKind::TypeOperator(operator, inner) => format!(
            "{} {}",
            match operator {
                TypeOperatorKind::Keyof => "keyof",
                TypeOperatorKind::Unique => "unique",
                TypeOperatorKind::Readonly => "readonly",
            },
            print(inner)?
        ),
        TypeNodeKind::Union(members) => list(members, " | ")?,
        TypeNodeKind::Intersection(members) => list(members, " & ")?,
        TypeNodeKind::Tuple(elements) => format!(
            "[{}]",
            elements
                .iter()
                .map(|element| {
                    let mut text = if element.dotdotdot {
                        "...".into()
                    } else {
                        String::new()
                    };
                    if let Some(label) = &element.label {
                        text += label;
                        if element.optional {
                            text += "?";
                        }
                        text += ": ";
                    }
                    text += &print(&element.type_node)?;
                    if element.label.is_none() && element.optional {
                        text += "?";
                    }
                    Some(text)
                })
                .collect::<Option<Vec<_>>>()?
                .join(", ")
        ),
        TypeNodeKind::NamedTupleMember(member) => format!(
            "{}{}{}: {}",
            if member.dotdotdot { "..." } else { "" },
            member.name,
            if member.optional { "?" } else { "" },
            print(&member.type_node)?
        ),
        TypeNodeKind::Function(signature) | TypeNodeKind::Constructor(signature) => format!(
            "{}{}({}) => {}",
            if matches!(node.kind, TypeNodeKind::Constructor(_)) {
                if signature.is_abstract {
                    "abstract new "
                } else {
                    "new "
                }
            } else {
                ""
            },
            print_type_parameters(signature.type_params.as_deref(), source)?,
            print_parameters(&signature.params, source)?,
            print(&signature.return_type)?
        ),
        TypeNodeKind::TypeLit(members) => {
            if members.is_empty() {
                "{}".into()
            } else {
                format!(
                    "{{ {} }}",
                    members
                        .iter()
                        .map(|member| print_member(member, source))
                        .collect::<Option<Vec<_>>>()?
                        .join(" ")
                )
            }
        }
        TypeNodeKind::Conditional(conditional) => format!(
            "{} extends {} ? {} : {}",
            print(&conditional.check)?,
            print(&conditional.extends)?,
            print(&conditional.true_type)?,
            print(&conditional.false_type)?
        ),
        TypeNodeKind::IndexedAccess(object, index) => {
            format!("{}[{}]", print(object)?, print(index)?)
        }
        TypeNodeKind::TypeQuery(expression) => format!("typeof {}", expression_name(expression)?),
        TypeNodeKind::This => "this".into(),
        TypeNodeKind::Literal(literal) => match literal {
            LiteralTypeKind::String(value) => {
                let raw = source.get(node.span.start as usize..node.span.end as usize)?;
                let mut scanner = tsc_rs_scanner::TsScanner::new(raw);
                let value = if scanner.scan() == tsc_rs_scanner::TokenKind::StringLiteral {
                    scanner.token_value()
                } else {
                    value
                };
                quote_string(value, '"')
            }
            LiteralTypeKind::Number(number) => number.clone(),
            LiteralTypeKind::Minus(number) => format!("-{number}"),
            LiteralTypeKind::Boolean(value) => value.to_string(),
            LiteralTypeKind::Null => "null".into(),
            LiteralTypeKind::BigInt(number) => {
                format!("{}n", number.strip_suffix('n').unwrap_or(number))
            }
        },
        TypeNodeKind::Infer(name, constraint) => {
            let mut text = format!("infer {name}");
            if let Some(constraint) = constraint {
                text += &format!(" extends {}", print(constraint)?);
            }
            text
        }
        TypeNodeKind::Predicate(predicate) => {
            let mut text = format!(
                "{}{}",
                if predicate.asserts { "asserts " } else { "" },
                predicate.param_name
            );
            if let Some(ty) = &predicate.type_ann {
                text += &format!(" is {}", print(ty)?);
            }
            text
        }
        // Unsupported source-node forms fall back to semantic serialization.
        TypeNodeKind::Mapped(_)
        | TypeNodeKind::TemplateLit(_)
        | TypeNodeKind::ImportType(_)
        | TypeNodeKind::JSDocNullable(_) => return None,
    })
}
fn print_type_parameters(parameters: Option<&[TypeParam]>, source: &str) -> Option<String> {
    let Some(parameters) = parameters.filter(|p| !p.is_empty()) else {
        return Some(String::new());
    };
    Some(format!(
        "<{}>",
        parameters
            .iter()
            .map(|parameter| {
                let mut text = parameter.name.clone();
                if let Some(constraint) = &parameter.constraint {
                    text += &format!(" extends {}", print_annotation(constraint, source)?);
                }
                if let Some(default) = &parameter.default {
                    text += &format!(" = {}", print_annotation(default, source)?);
                }
                Some(text)
            })
            .collect::<Option<Vec<_>>>()?
            .join(", ")
    ))
}
fn print_parameters(parameters: &[Param], source: &str) -> Option<String> {
    parameters
        .iter()
        .map(|parameter| {
            let mut text = format!(
                "{}{}{}",
                if parameter.dotdotdot { "..." } else { "" },
                print_binding(&parameter.name, source)?,
                if parameter.optional { "?" } else { "" }
            );
            if let Some(annotation) = &parameter.type_ann {
                text += &format!(": {}", print_annotation(annotation, source)?);
            }
            Some(text)
        })
        .collect::<Option<Vec<_>>>()
        .map(|parameters| parameters.join(", "))
}
fn print_member(member: &TypeMember, source: &str) -> Option<String> {
    let signature = |params: &[Param],
                     generics: Option<&[TypeParam]>,
                     ret: Option<&TypeNode>|
     -> Option<String> {
        let mut text = format!(
            "{}({})",
            print_type_parameters(generics, source)?,
            print_parameters(params, source)?
        );
        if let Some(ret) = ret {
            text += &format!(": {}", print_annotation(ret, source)?);
        }
        Some(text)
    };
    let text = match &member.kind {
        TypeMemberKind::PropertySig(property) => {
            let mut text = format!(
                "{}{}{}",
                if property.readonly { "readonly " } else { "" },
                print_property(&property.name, source)?,
                if property.optional { "?" } else { "" }
            );
            if let Some(ty) = &property.type_ann {
                text += &format!(": {}", print_annotation(ty, source)?);
            }
            text
        }
        TypeMemberKind::MethodSig(method) => format!(
            "{}{}{}",
            print_property(&method.name, source)?,
            if method.optional { "?" } else { "" },
            signature(
                &method.params,
                method.type_params.as_deref(),
                method.return_type.as_ref()
            )?
        ),
        TypeMemberKind::CallSig(call) => signature(
            &call.params,
            call.type_params.as_deref(),
            call.return_type.as_ref(),
        )?,
        TypeMemberKind::ConstructSig(call) => format!(
            "new {}",
            signature(
                &call.params,
                call.type_params.as_deref(),
                call.return_type.as_ref()
            )?
        ),
        TypeMemberKind::IndexSig(index) => format!(
            "{}[{}]: {}",
            if index.modifiers & MOD_READONLY != 0 {
                "readonly "
            } else {
                ""
            },
            print_parameters(&index.params, source)?,
            print_annotation(index.type_ann.as_ref()?, source)?
        ),
        _ => return None,
    };
    Some(format!("{text};"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn both_substitution_paths_preserve_unchanged_member_annotations() {
        let mut source = ObjectSource::default();
        for (name, ty, text) in [
            ("a", Type::Number, "number"),
            ("b", Type::TypeParameter("T".into()), "T"),
        ] {
            source.set_property(
                name.into(),
                PropertyDeclaration {
                    annotation: Some(Annotation {
                        ty,
                        text: text.into(),
                    }),
                    ..Default::default()
                },
            );
        }
        let original = Type::ObjectType(crate::ObjectTypeInfo::new(crate::ObjectTypeData {
            properties: vec![
                ("a".into(), Arc::new(Type::Number)),
                ("b".into(), Arc::new(Type::TypeParameter("T".into()))),
            ],
            node_source: source,
            ..Default::default()
        }));
        let arguments = std::collections::HashMap::from([("T".into(), Type::String)]);
        for instantiated in [
            TypeChecker::substitute(&original, &arguments),
            crate::substitute(&original, &arguments),
        ] {
            let printed = print_type(&instantiated);
            assert_eq!(printed.text, "{ a: number; b: string; }");
            assert_eq!(printed.underline, "^^^^^      ^^^^^^^^^^^^^^");
        }
    }

    #[test]
    fn object_provenance_does_not_change_type_equality_hashing_or_hover() {
        let mut source = ObjectSource::default();
        source.set_property(
            "value".into(),
            PropertyDeclaration {
                annotation: Some(Annotation {
                    ty: Type::Number,
                    text: "number".into(),
                }),
                ..Default::default()
            },
        );
        let original = Type::ObjectType(crate::ObjectTypeInfo::new(crate::ObjectTypeData {
            properties: vec![("value".into(), Arc::new(Type::Number))],
            node_source: source,
            ..Default::default()
        }));
        let mut synthetic = original.clone();
        let Type::ObjectType(object) = &mut synthetic else {
            unreachable!()
        };
        object.node_source = ObjectSource::default();
        assert_eq!(synthetic, original);
        assert_eq!(
            HashSet::from([synthetic.clone(), original.clone()]).len(),
            1
        );
        assert_eq!(synthetic.display_string(), original.display_string());
        assert_eq!(print_type(&original).underline, "^^^^^^^^^      ^^^");
        assert_eq!(print_type(&synthetic).underline, "^^^^^^^^^^^^^^^^^^");
    }

    #[test]
    fn source_provenance_does_not_change_type_equality_or_hashing() {
        let synthetic = Type::Function(FunctionType {
            source: SignatureSource::default(),
            params: vec![("x".into(), Type::Number)],
            return_type: Arc::new(Type::Number),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            type_param_defaults: Vec::new(),
            type_predicate: None,
        });
        let mut original = synthetic.clone();
        let Type::Function(signature) = &mut original else {
            unreachable!()
        };
        signature.source = SignatureSource(Some(Arc::new(SignatureDeclaration {
            original_this_parameter: false,
            parameters: vec![ParameterDeclaration {
                name: "x".into(),
                annotation: Some(Annotation {
                    ty: Type::Number,
                    text: "number".into(),
                }),
            }],
            type_parameters: Vec::new(),
            return_type: None,
        })));
        assert_eq!(synthetic, original);
        assert_eq!(
            HashSet::from([synthetic.clone(), original.clone()]).len(),
            1
        );
        assert_eq!(synthetic.display_string(), original.display_string());
        assert_eq!(print_type(&synthetic).underline, "^^^^^^^^^^^^^^^^^^^^^");
        assert_eq!(print_type(&original).underline, "^ ^^      ^^^^^^^^^^^");
    }
}
