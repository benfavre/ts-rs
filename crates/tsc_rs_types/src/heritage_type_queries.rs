//! Class heritage type queries, adapted from getTypeOfNode and
//! resolveBaseTypesOfClass in pingdotgg/ts-rust checker_p35/p21.rs. See NOTICE.txt.

use super::*;
use tsc_rs_scanner::TokenKind;

impl TypeChecker {
    pub(super) fn has_class_expression_type(ty: &Type) -> bool {
        match ty {
            Type::TypeReference(name, arguments) => {
                name.starts_with("typeof __class_expression_")
                    || arguments.iter().any(Self::has_class_expression_type)
            }
            Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => {
                members.iter().any(Self::has_class_expression_type)
            }
            Type::Function(signature) => Self::has_class_expression_type(&signature.return_type),
            Type::Array(element) | Type::Readonly(element) | Type::Optional(element) => {
                Self::has_class_expression_type(element)
            }
            _ => false,
        }
    }

    /// The node builder keeps a named expression's source name while its
    /// checker identity remains unique. Anonymous mixin constructors expand
    /// their own constructor side and retain the generic base intersection.
    pub(super) fn class_expression_type_node(
        &self,
        declaration: &ClassDecl,
        info: &ClassInfo,
    ) -> Option<Type> {
        if let Some(name) = &declaration.name {
            return Some(Type::Typeof(name.clone()));
        }
        let base = self.infer_expr_type(declaration.extends.as_ref()?);
        let apparent = self.typeparam_constraint_apparent(&base)?;
        let mut signatures = self.direct_constructor_signatures(&apparent)?;
        for signature in &mut signatures {
            signature.is_abstract = info.is_abstract;
            signature.return_type = Arc::new(Type::TypeReference(
                "(Anonymous class)".into(),
                Arc::from([]),
            ));
            if info.has_explicit_constructor {
                signature.params = info
                    .constructor_params
                    .iter()
                    .map(|(name, ty, optional, rest)| {
                        (
                            if *rest {
                                format!("...{name}")
                            } else if *optional {
                                format!("?{name}")
                            } else {
                                name.clone()
                            },
                            ty.clone(),
                        )
                    })
                    .collect();
                signature.source = Default::default();
            }
        }
        let mut properties: Vec<_> = info
            .static_properties
            .iter()
            .map(|(name, ty)| (name.clone(), Arc::new(ty.clone())))
            .collect();
        properties.extend(
            info.static_methods.iter().map(|(name, signature)| {
                (name.clone(), Arc::new(Type::Function(signature.clone())))
            }),
        );
        let prefix = self.type_node_function_path.join(".");
        let prototype = if prefix.is_empty() {
            "(Anonymous class)".to_string()
        } else {
            format!("{prefix}.(Anonymous class)")
        };
        properties.push((
            "prototype".into(),
            Arc::new(Type::TypeReference(prototype, Arc::from([]))),
        ));
        Some(Type::Intersection(
            vec![
                Type::ObjectType(ObjectTypeInfo::new(ObjectTypeData {
                    properties,
                    construct_signatures: signatures,
                    method_names: info
                        .static_methods
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect(),
                    ..Default::default()
                })),
                base,
            ]
            .into(),
        ))
    }

    pub(super) fn class_heritage_span(&self, declaration: &ClassDecl) -> Span {
        let mut span = declaration.extends.as_ref().unwrap().span;
        if let Some(last) = declaration
            .extends_type_args
            .as_ref()
            .and_then(|args| args.last())
        {
            span.end = last.span.end;
            if let Some(source) = self
                .current_source
                .as_deref()
                .and_then(|source| source.get(span.end as usize..))
            {
                let mut scanner = tsc_rs_scanner::TsScanner::new(source);
                let mut token = scanner.scan();
                if token == TokenKind::Comma {
                    token = scanner.scan();
                }
                if token == TokenKind::GreaterThan {
                    span.end += scanner.text_pos() as u32;
                }
            }
        }
        span
    }

    pub(super) fn record_class_heritage_type(&mut self, declaration: &ClassDecl, value: &Type) {
        let Some(expression) = &declaration.extends else {
            return;
        };
        let arguments: Vec<_> = declaration
            .extends_type_args
            .iter()
            .flatten()
            .map(|node| self.resolve_type_node(node))
            .collect();
        let path = self.expr_to_name(expression);
        let library = self
            .standard_global_path_is_unshadowed(&path)
            .then(|| stdlib::stdlib_member_availability(&self.compiler_options))
            .flatten();
        let owner = library.as_ref().and_then(|lib| lib.value_owner(&path));
        let mut constructor = self
            .typeparam_constraint_apparent(value)
            .unwrap_or_else(|| value.clone());
        let mut seen = HashSet::new();
        while seen.insert(constructor.clone()) {
            let Type::TypeReference(name, arguments) = &constructor else {
                break;
            };
            let Some(expanded) = self.expand_type_alias_once(name, arguments) else {
                break;
            };
            constructor = expanded;
        }
        let mut signatures = owner
            .map(|owner| self.library_constructor_signatures(owner))
            .unwrap_or_default();
        if signatures.is_empty() {
            signatures = self
                .direct_constructor_signatures(&constructor)
                .unwrap_or_default();
        }
        let Some(signature) = signatures.iter().find(|signature| {
            let required = signature
                .type_params
                .iter()
                .enumerate()
                .rfind(|(index, _)| {
                    !signature
                        .type_param_defaults
                        .get(*index)
                        .is_some_and(Option::is_some)
                })
                .map_or(0, |(index, _)| index + 1);
            arguments.len() >= required && arguments.len() <= signature.type_params.len()
        }) else {
            return;
        };
        let mut substitutions = HashMap::new();
        for (index, parameter) in signature.type_params.iter().enumerate() {
            let argument = arguments
                .get(index)
                .cloned()
                .or_else(|| {
                    signature
                        .type_param_defaults
                        .get(index)
                        .and_then(Option::as_ref)
                        .map(|ty| Self::substitute(ty, &substitutions))
                })
                .unwrap_or(Type::Any);
            substitutions.insert(parameter.clone(), argument);
        }
        let mut instance = Self::substitute(&signature.return_type, &substitutions);
        if let Type::TypeReference(class, _) = &instance {
            let mut name = class.as_str();
            let mut visited = HashSet::new();
            while let Some(info) = self.class_info.get(name) {
                if !visited.insert(name) {
                    return;
                }
                let Some(base) = &info.extends else { break };
                name = base;
            }
        }
        // Class constructor signatures can use a local registry key. The value
        // retains the symbol's qualified identity, which the type query needs.
        if let (Type::TypeReference(name, _), Type::TypeReference(_, arguments)) =
            (&constructor, &instance)
        {
            if let Some(name) = name.strip_prefix("typeof ") {
                instance = Type::TypeReference(name.to_owned(), arguments.clone());
            }
        }
        if !self.valid_heritage_instance(&instance, 0) {
            self.diagnostics.push(Diagnostic {
                code: 2509,
                message: format!("Base constructor return type '{}' is not an object type or intersection of object types with statically known members.", instance.display_string_single_line()),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(expression.span),
                related: None,
            });
            return;
        }
        // TypeWriterWalker queries the ExpressionWithTypeArguments parent for
        // this span. Its child names and the ordinary expression/hover type keep
        // their value-space constructor types.
        if !matches!(instance, Type::Any | Type::Error) {
            self.record_expression_type_span(expression.span, &instance);
        }
    }

    fn valid_heritage_instance(&self, ty: &Type, depth: usize) -> bool {
        if depth >= 16 {
            return false;
        }
        match ty {
            Type::ObjectType(_)
            | Type::Object
            | Type::Array(_)
            | Type::Tuple(_)
            | Type::Function(_)
            | Type::Constructor(_)
            | Type::Any
            | Type::Error => true,
            Type::Intersection(members) => members
                .iter()
                .all(|ty| self.valid_heritage_instance(ty, depth + 1)),
            Type::Readonly(inner) => self.valid_heritage_instance(inner, depth + 1),
            Type::Mapped { constraint, .. } => self.heritage_keys_are_static(constraint, depth + 1),
            Type::TypeReference(name, args)
                if !self.program_declares_library_owner(name)
                    && stdlib::stdlib_member_availability(&self.compiler_options)
                        .is_some_and(|lib| lib.declares_global_type(name))
                    && matches!(
                        name.as_str(),
                        "Partial" | "Required" | "Readonly" | "Pick" | "Omit" | "Record"
                    ) =>
            {
                match (name.as_str(), args.as_ref()) {
                    ("Partial" | "Required" | "Readonly", [object]) => {
                        self.heritage_members_are_static(object, depth + 1)
                    }
                    ("Record", [keys, _]) => self.heritage_keys_are_static(keys, depth + 1),
                    ("Pick", [_, keys]) => self.heritage_keys_are_static(keys, depth + 1),
                    ("Omit", [object, keys]) => {
                        self.heritage_members_are_static(object, depth + 1)
                            && self.heritage_keys_are_static(keys, depth + 1)
                    }
                    _ => false,
                }
            }
            Type::TypeReference(name, _) => self
                .resolve_type_for_assignability(ty)
                .filter(|resolved| resolved != ty)
                .map_or_else(
                    || {
                        self.class_info.contains_key(name)
                            || self.interface_info.contains_key(name)
                            || self.builtins.instance_members.contains_key(name)
                            || self.library_interface_is_active(name)
                    },
                    |resolved| self.valid_heritage_instance(&resolved, depth + 1),
                ),
            Type::TypeParameter(_) => self
                .declared_type_param_constraint(ty)
                .filter(|constraint| constraint != ty)
                .is_some_and(|constraint| self.valid_heritage_instance(&constraint, depth + 1)),
            _ => false,
        }
    }

    fn heritage_members_are_static(&self, ty: &Type, depth: usize) -> bool {
        match ty {
            Type::TypeParameter(_) => false,
            Type::TypeReference(name, _)
                if self.type_parameter_in_resolution(name)
                    || self.type_param_name_is_active(name) =>
            {
                false
            }
            Type::Union(members) => members
                .iter()
                .all(|member| self.heritage_members_are_static(member, depth + 1)),
            _ => self.valid_heritage_instance(ty, depth + 1),
        }
    }

    fn heritage_keys_are_static(&self, ty: &Type, depth: usize) -> bool {
        if depth >= 16 {
            return false;
        }
        match ty {
            Type::String
            | Type::Number
            | Type::Symbol
            | Type::UniqueSymbol(_)
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::Never => true,
            Type::Union(members) | Type::Intersection(members) => members
                .iter()
                .all(|member| self.heritage_keys_are_static(member, depth + 1)),
            Type::Keyof(object) => self.heritage_members_are_static(object, depth + 1),
            Type::TypeReference(_, _) => self
                .resolve_type_for_assignability(ty)
                .filter(|resolved| resolved != ty)
                .is_some_and(|resolved| self.heritage_keys_are_static(&resolved, depth + 1)),
            _ => false,
        }
    }
}
