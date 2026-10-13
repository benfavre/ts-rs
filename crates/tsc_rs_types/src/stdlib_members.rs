//! Declaration-based library members, adapting resolveTypeReferenceMembers /
//! resolveObjectTypeMembers from pingdotgg/ts-rust checker_p21.rs. See NOTICE.txt.
//! Native signatures retain their declaration metadata while owner parameters
//! are instantiated at the receiver, independently of method type parameters.

use super::*;
use std::sync::OnceLock;

#[derive(Default)]
struct LibraryInterface {
    type_parameters: Vec<String>,
    type_defaults: Vec<Option<Type>>,
    methods: HashMap<String, Vec<FunctionType>>,
    bases: Vec<Type>,
    properties: HashMap<String, LibraryProperty>,
    indices: Vec<LibraryIndex>,
    constructors: Vec<ConstructorType>,
}

#[derive(Clone)]
pub(super) struct LibraryProperty {
    pub(super) ty: Type,
    pub(super) readonly: bool,
}

#[derive(Clone)]
pub(super) struct LibraryIndex {
    pub(super) key: Type,
    pub(super) value: Type,
    pub(super) readonly: bool,
}

pub(super) enum LibraryReadonlyAccess {
    Property(String),
    Index,
}

type LibraryInterfaces = HashMap<String, LibraryInterface>;
type LibraryCache = Mutex<HashMap<(String, bool), Arc<LibraryInterfaces>>>;
thread_local! { static LOADING_LIBRARY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
struct LibraryLoadGuard;
impl LibraryLoadGuard {
    fn enter() -> Self {
        LOADING_LIBRARY.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}
impl Drop for LibraryLoadGuard {
    fn drop(&mut self) {
        LOADING_LIBRARY.with(|depth| depth.set(depth.get() - 1));
    }
}

static LIBRARY_SIGNATURES: OnceLock<LibraryCache> = OnceLock::new();

fn library_interfaces(path: &str, strict_null_checks: bool) -> Option<Arc<LibraryInterfaces>> {
    let key = (path.to_owned(), strict_null_checks);
    let cache = LIBRARY_SIGNATURES.get_or_init(Default::default);
    if let Some(cached) = cache.lock().ok()?.get(&key).cloned() {
        return Some(cached);
    }
    let _loading = LibraryLoadGuard::enter();
    let _environment = crate::class_return_inference::isolated_environment();
    let text = std::fs::read_to_string(path).ok()?;
    let file = tsc_rs_parser::parse(path, &text);
    let mut checker = TypeChecker::new();
    checker.enable_expression_type_spans();
    checker.set_current_file_name(path);
    checker.current_source = Some(text);
    checker.strict_null_checks = strict_null_checks;
    let mut interfaces: LibraryInterfaces = HashMap::new();
    let mut statements: Vec<_> = file.statements.iter().rev().collect();
    while let Some(statement) = statements.pop() {
        match &statement.kind {
            StmtKind::ModuleDecl(module) if matches!(&module.name, ModuleName::Ident(name) if name == "global") =>
            {
                if let Some(ModuleBody::Block(body)) = &module.body {
                    statements.extend(body.iter().rev());
                }
                continue;
            }
            StmtKind::Export(export) => {
                if let ExportDeclKind::Decl(declaration)
                | ExportDeclKind::DefaultDecl(declaration) = &export.kind
                {
                    statements.push(declaration);
                }
                continue;
            }
            _ => {}
        }
        let StmtKind::InterfaceDecl(declaration) = &statement.kind else {
            continue;
        };
        let _parameters = crate::type_params_in_resolution(declaration.type_params.as_deref());
        let interface = interfaces.entry(declaration.name.clone()).or_default();
        interface.type_parameters = declaration
            .type_params
            .iter()
            .flatten()
            .map(|parameter| parameter.name.clone())
            .collect();
        interface.type_defaults = declaration
            .type_params
            .iter()
            .flatten()
            .map(|parameter| {
                parameter
                    .default
                    .as_ref()
                    .map(|node| checker.resolve_type_node(node))
            })
            .collect();
        interface.bases.extend(
            declaration
                .extends
                .iter()
                .map(|base| checker.resolve_type_node(base)),
        );
        for member in &declaration.members {
            match &member.kind {
                TypeMemberKind::ConstructSig(_) => {
                    let node = TypeNode {
                        kind: TypeNodeKind::TypeLit(vec![member.clone()]),
                        span: member.span,
                    };
                    if let Type::ObjectType(shape) = checker.resolve_type_node(&node) {
                        interface
                            .constructors
                            .extend(shape.construct_signatures.iter().cloned());
                    }
                    continue;
                }
                TypeMemberKind::PropertySig(property) => {
                    let mut ty = property
                        .type_ann
                        .as_ref()
                        .map(|node| checker.resolve_type_node(node))
                        .unwrap_or(Type::Any);
                    if property.optional {
                        ty = Type::Optional(Arc::new(ty));
                    }
                    interface.properties.insert(
                        checker.prop_name_to_string(&property.name),
                        LibraryProperty {
                            ty,
                            readonly: property.readonly,
                        },
                    );
                    continue;
                }
                TypeMemberKind::IndexSig(index) => {
                    if let (Some(key), Some(value)) = (
                        index.params.first().and_then(|p| p.type_ann.as_ref()),
                        index.type_ann.as_ref(),
                    ) {
                        interface.indices.push(LibraryIndex {
                            key: checker.resolve_type_node(key),
                            value: checker.resolve_type_node(value),
                            readonly: index.modifiers & MOD_READONLY != 0,
                        });
                    }
                    continue;
                }
                _ => {}
            }
            let TypeMemberKind::MethodSig(method) = &member.kind else {
                continue;
            };
            let name = checker.prop_name_to_string(&method.name);
            let function = FnDecl {
                name: Some(name.clone()),
                name_span: None,
                type_params: method.type_params.clone(),
                params: method.params.clone(),
                return_type: method.return_type.clone(),
                body: None,
                modifiers: 0,
                is_generator: false,
                is_async: false,
                decorators: Vec::new(),
                span: member.span,
            };
            let mut signature = checker.resolve_fn_type(&function);
            if !strict_null_checks {
                if let Type::Union(members) = signature.return_type.as_ref() {
                    let non_nullable: Vec<_> = members
                        .iter()
                        .filter(|ty| !matches!(ty, Type::Null | Type::Undefined))
                        .cloned()
                        .collect();
                    if !non_nullable.is_empty() {
                        signature.return_type = Arc::new(Type::flatten_union(non_nullable));
                        signature.source = signature
                            .source
                            .with_resolved_return_type(&signature.return_type);
                    }
                }
            }
            interface.methods.entry(name).or_default().push(signature);
        }
    }
    let interfaces = Arc::new(interfaces);
    cache.lock().ok()?.insert(key, interfaces.clone());
    Some(interfaces)
}

impl TypeChecker {
    pub(super) fn library_constructor_signatures(&self, owner: &str) -> Vec<ConstructorType> {
        if LOADING_LIBRARY.with(|depth| depth.get() != 0)
            || self.program_declares_library_owner(owner)
        {
            return Vec::new();
        }
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return Vec::new();
        };
        let mut signatures = Vec::new();
        for file in availability.owner_declaring_libraries(owner) {
            if let Some(interfaces) = library_interfaces(&file, self.strict_null_checks) {
                if let Some(interface) = interfaces.get(owner) {
                    signatures.extend(interface.constructors.iter().cloned());
                }
            }
        }
        signatures
    }

    pub(super) fn library_interface_is_active(&self, owner: &str) -> bool {
        if LOADING_LIBRARY.with(|depth| depth.get() != 0)
            || self.program_declares_library_owner(owner)
        {
            return false;
        }
        stdlib::stdlib_member_availability(&self.compiler_options).is_some_and(|availability| {
            availability
                .owner_declaring_libraries(owner)
                .iter()
                .any(|file| {
                    library_interfaces(file, self.strict_null_checks)
                        .is_some_and(|interfaces| interfaces.contains_key(owner))
                })
        })
    }

    fn library_base_reference(base: Type) -> Option<(String, Vec<Type>)> {
        match base {
            Type::TypeReference(name, args) => Some((name, args.to_vec())),
            Type::Array(element) => Some(("Array".into(), vec![Type::clone(&element)])),
            Type::Readonly(inner) => match inner.as_ref() {
                Type::Array(element) => Some(("ReadonlyArray".into(), vec![Type::clone(element)])),
                _ => None,
            },
            _ => None,
        }
    }

    fn legacy_iterable_reference(&self, owner: &str) -> bool {
        matches!(
            owner,
            "Iterable" | "IterableIterator" | "AsyncIterable" | "AsyncIterableIterator"
        ) && !self.program_declares_library_owner(owner)
    }

    /// typeReferenceToTypeNode (nodebuilder_impl_p3.rs) elides matching
    /// defaults for the four historical iterable interfaces. Native resolved
    /// references have no parse-tree reference node, matching its nil-node path.
    pub(super) fn library_printed_argument_count(&self, owner: &str, arguments: &[Type]) -> usize {
        if !self.legacy_iterable_reference(owner) {
            return arguments.len();
        }
        let mut count = arguments.len();
        while count > 0 {
            let Some(defaulted) = self.library_type_arguments(owner, &arguments[..count - 1])
            else {
                break;
            };
            if defaulted != arguments {
                break;
            }
            count -= 1;
        }
        count
    }

    pub(super) fn library_type_arguments(
        &self,
        owner: &str,
        arguments: &[Type],
    ) -> Option<Vec<Type>> {
        if LOADING_LIBRARY.with(|depth| depth.get() != 0)
            || self.program_declares_library_owner(owner)
            || self.type_parameter_in_resolution(owner)
        {
            return None;
        }
        let availability = stdlib::stdlib_member_availability(&self.compiler_options)?;
        for file in availability.owner_declaring_libraries(owner) {
            let interfaces = library_interfaces(&file, self.strict_null_checks)?;
            let Some(interface) = interfaces.get(owner) else {
                continue;
            };
            if interface.type_defaults.len() <= arguments.len() {
                continue;
            }
            let mut result = arguments.to_vec();
            let mut substitutions: HashMap<_, _> = interface
                .type_parameters
                .iter()
                .cloned()
                .zip(arguments.iter().cloned())
                .collect();
            for (name, default) in interface
                .type_parameters
                .iter()
                .zip(&interface.type_defaults)
                .skip(arguments.len())
            {
                let Some(default) = default else {
                    break;
                };
                let value = Self::substitute(default, &substitutions);
                result.push(value.clone());
                substitutions.insert(name.clone(), value);
            }
            if result.len() > arguments.len() {
                return Some(result);
            }
        }
        None
    }

    pub(super) fn library_method_type(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
    ) -> Option<Type> {
        self.library_method_type_inner(owner, arguments, name, &mut Vec::new())
    }

    fn library_method_type_inner(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
        resolving: &mut Vec<String>,
    ) -> Option<Type> {
        if LOADING_LIBRARY.with(|depth| depth.get() != 0)
            || resolving.len() >= 12
            || resolving.iter().any(|name| name == owner)
        {
            return None;
        }
        let defaulted = self.library_type_arguments(owner, arguments);
        let arguments = defaulted.as_deref().unwrap_or(arguments);
        resolving.push(owner.to_owned());
        let result = self.library_method_type_resolved(owner, arguments, name, resolving);
        resolving.pop();
        result
    }

    fn library_method_type_resolved(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
        resolving: &mut Vec<String>,
    ) -> Option<Type> {
        let availability = stdlib::stdlib_member_availability(&self.compiler_options)?;
        let mut signatures = Vec::new();
        let mut bases = Vec::new();
        // getSignaturesOfSymbol (checker_p22.rs) retains declaration order.
        // Candidate precedence during calls is separate from member type order.
        for file in availability.owner_declaring_libraries(owner) {
            let interfaces = library_interfaces(&file, self.strict_null_checks)?;
            let Some(interface) = interfaces.get(owner) else {
                continue;
            };
            let substitutions: HashMap<_, _> = interface
                .type_parameters
                .iter()
                .cloned()
                .zip(arguments.iter().cloned())
                .collect();
            bases.extend(
                interface
                    .bases
                    .iter()
                    .map(|base| Self::substitute(base, &substitutions)),
            );
            for method in interface.methods.get(name).into_iter().flatten() {
                // A method's own parameters shadow the receiver's parameters.
                let mut substitutions = substitutions.clone();
                for parameter in &method.type_params {
                    substitutions.remove(parameter);
                }
                let mut instantiated =
                    Self::substitute(&Type::Function(method.clone()), &substitutions);
                if let Type::Function(function) = &mut instantiated {
                    let names = interface
                        .type_parameters
                        .iter()
                        .chain(&function.type_params)
                        .cloned()
                        .collect();
                    function.source = function.source.clone().with_instantiated_parameters(names);
                }
                signatures.push(instantiated);
            }
        }
        match signatures.len() {
            0 => bases.into_iter().find_map(|base| {
                let (base_name, args) = Self::library_base_reference(base)?;
                self.library_method_type_inner(&base_name, &args, name, resolving)
            }),
            1 => signatures.pop(),
            _ => Some(Type::Intersection(signatures.into())),
        }
    }

    pub(super) fn program_declares_library_owner(&self, owner: &str) -> bool {
        self.class_info.contains_key(owner)
            || self.interface_info.get(owner).is_some_and(|info| {
                // Builtin discriminator seeds have no source declaration.
                !info.decl_file.is_empty() || info.declaration_span != Span::new(0, 0)
            })
            || self.type_aliases.contains_key(owner)
    }

    /// Adapt resolveObjectTypeMembers (checker_p21.rs): instantiate own index
    /// signatures before inheriting signatures whose key domain is absent.
    pub(super) fn library_index_signatures(
        &self,
        owner: &str,
        arguments: &[Type],
    ) -> Vec<LibraryIndex> {
        self.library_index_signatures_inner(owner, arguments, &mut Vec::new())
    }

    fn library_index_signatures_inner(
        &self,
        owner: &str,
        arguments: &[Type],
        resolving: &mut Vec<String>,
    ) -> Vec<LibraryIndex> {
        if LOADING_LIBRARY.with(|depth| depth.get() != 0)
            || self.program_declares_library_owner(owner)
            || resolving.len() >= 32
            || resolving.iter().any(|name| name == owner)
        {
            return Vec::new();
        }
        let Some(availability) = stdlib::stdlib_member_availability(&self.compiler_options) else {
            return Vec::new();
        };
        let defaulted = self.library_type_arguments(owner, arguments);
        let arguments = defaulted.as_deref().unwrap_or(arguments);
        resolving.push(owner.to_owned());
        let mut indices: Vec<LibraryIndex> = Vec::new();
        let mut bases = Vec::new();
        for file in availability.owner_declaring_libraries(owner) {
            let Some(interfaces) = library_interfaces(&file, self.strict_null_checks) else {
                continue;
            };
            let Some(interface) = interfaces.get(owner) else {
                continue;
            };
            let substitutions: HashMap<_, _> = interface
                .type_parameters
                .iter()
                .cloned()
                .zip(arguments.iter().cloned())
                .collect();
            for index in &interface.indices {
                let key = Self::substitute(&index.key, &substitutions);
                if !indices.iter().any(|own| own.key == key) {
                    indices.push(LibraryIndex {
                        key,
                        value: Self::substitute(&index.value, &substitutions),
                        readonly: index.readonly,
                    });
                }
            }
            bases.extend(
                interface
                    .bases
                    .iter()
                    .map(|base| Self::substitute(base, &substitutions)),
            );
        }
        for base in bases {
            if let Some((name, args)) = Self::library_base_reference(base) {
                for index in self.library_index_signatures_inner(&name, &args, resolving) {
                    if !indices.iter().any(|own| own.key == index.key) {
                        indices.push(index);
                    }
                }
            }
        }
        resolving.pop();
        indices
    }

    pub(super) fn library_numeric_index(&self, owner: &str, arguments: &[Type]) -> Option<Type> {
        self.library_index_signatures(owner, arguments)
            .into_iter()
            .find(|index| matches!(index.key, Type::Number))
            .map(|index| index.value)
    }

    /// Named properties precede applicable index signatures, including the
    /// required first element of RegExpMatchArray in unchecked-access mode.
    pub(super) fn library_property(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
    ) -> Option<LibraryProperty> {
        self.library_property_inner(owner, arguments, name, &mut Vec::new())
    }

    fn library_property_inner(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
        resolving: &mut Vec<String>,
    ) -> Option<LibraryProperty> {
        if LOADING_LIBRARY.with(|depth| depth.get() != 0)
            || self.program_declares_library_owner(owner)
            || resolving.len() >= 32
            || resolving.iter().any(|name| name == owner)
        {
            return None;
        }
        let defaulted = self.library_type_arguments(owner, arguments);
        let arguments = defaulted.as_deref().unwrap_or(arguments);
        resolving.push(owner.to_owned());
        let result = (|| {
            let availability = stdlib::stdlib_member_availability(&self.compiler_options)?;
            let mut bases = Vec::new();
            for file in availability.owner_declaring_libraries(owner) {
                let interfaces = library_interfaces(&file, self.strict_null_checks)?;
                let Some(interface) = interfaces.get(owner) else {
                    continue;
                };
                let substitutions: HashMap<_, _> = interface
                    .type_parameters
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned())
                    .collect();
                if let Some(property) = interface.properties.get(name) {
                    return Some(LibraryProperty {
                        ty: Self::substitute(&property.ty, &substitutions),
                        readonly: property.readonly,
                    });
                }
                bases.extend(
                    interface
                        .bases
                        .iter()
                        .map(|base| Self::substitute(base, &substitutions)),
                );
            }
            bases.into_iter().find_map(|base| {
                let (name_of_base, args) = Self::library_base_reference(base)?;
                self.library_property_inner(&name_of_base, &args, name, resolving)
            })
        })();
        resolving.pop();
        result
    }

    /// Resolve library write restrictions through the same owner substitutions
    /// and heritage edges as reads. A program's own declaration shadows a base.
    pub(super) fn library_readonly_access(
        &self,
        receiver: &Type,
        index: &Type,
        depth: usize,
    ) -> Option<LibraryReadonlyAccess> {
        if depth >= 32 {
            return None;
        }
        if let Type::Union(members) = receiver {
            return members
                .iter()
                .find_map(|member| self.library_readonly_access(member, index, depth + 1));
        }
        if let Type::Intersection(members) = receiver {
            let key = match index {
                Type::StringLiteral(key) => Some(key.clone()),
                Type::NumberLiteral(key) => Some(Self::canonical_numeric_property_name(key)),
                _ => None,
            };
            let mut readonly = None;
            for member in members.iter() {
                let access = self.library_readonly_access(member, index, depth + 1);
                // createUnionOrIntersectionProperty (checker_p24.rs): a union
                // retains any readonly property, whereas an intersection's
                // property is writable if any declaration of it is writable.
                // Constituents which do not declare the property are neutral.
                if access.is_none()
                    && key
                        .as_ref()
                        .is_some_and(|key| self.type_property_presence(member, key) == Some(true))
                {
                    return None;
                }
                if access.is_some() {
                    readonly = access;
                }
            }
            return readonly;
        }
        if let Some(constraint) = self.declared_type_param_constraint(receiver) {
            if constraint != *receiver {
                return self.library_readonly_access(&constraint, index, depth + 1);
            }
        }
        if let Type::ObjectType(info) = receiver {
            let key = match index {
                Type::StringLiteral(key) => key.clone(),
                Type::NumberLiteral(key) => Self::canonical_numeric_property_name(key),
                _ => return None,
            };
            return info
                .readonly_names
                .contains(&key)
                .then_some(LibraryReadonlyAccess::Property(key));
        }
        let Type::TypeReference(name, arguments) = receiver else {
            return None;
        };
        if let Some(expanded) = self.expand_type_alias_once(name, arguments) {
            if expanded != *receiver {
                return self.library_readonly_access(&expanded, index, depth + 1);
            }
        }
        let key = match index {
            Type::StringLiteral(key) => Some(key.clone()),
            Type::NumberLiteral(key) => Some(Self::canonical_numeric_property_name(key)),
            _ => None,
        };
        if !self.program_declares_library_owner(name) {
            if let Some(key) = &key {
                if let Some(property) = self.library_property(name, arguments, key) {
                    return property
                        .readonly
                        .then(|| LibraryReadonlyAccess::Property(key.clone()));
                }
            }
            let mut indices = self.library_index_signatures(name, arguments);
            // Specific number/symbol domains take precedence over string.
            indices.sort_by_key(|index| matches!(index.key, Type::String));
            return indices
                .into_iter()
                .find(|signature| self.index_applies_to_property(index, &signature.key))
                .and_then(|signature| signature.readonly.then_some(LibraryReadonlyAccess::Index));
        }
        let (parameters, bases) = if let Some(info) = self.interface_info.get(name) {
            if key.as_ref().is_some_and(|key| {
                info.object_type
                    .properties
                    .iter()
                    .any(|(name, _)| name == key)
            }) || info
                .index_signatures
                .iter()
                .any(|(key, _)| self.index_applies_to_property(index, key))
            {
                return None;
            }
            (
                &info.type_params,
                info.extends
                    .iter()
                    .map(|(name, arguments)| Type::TypeReference(name.clone(), arguments.clone()))
                    .collect::<Vec<_>>(),
            )
        } else if let Some(info) = self.class_info.get(name) {
            if key.as_ref().is_some_and(|key| {
                info.instance_properties
                    .iter()
                    .map(|(name, _)| name)
                    .chain(info.instance_methods.iter().map(|(name, _)| name))
                    .any(|name| name == key)
            }) || info
                .index_signatures
                .iter()
                .any(|(key, _, is_static)| !is_static && self.index_applies_to_property(index, key))
            {
                return None;
            }
            (
                &info.type_params,
                info.extends
                    .iter()
                    .map(|name| {
                        Type::TypeReference(name.clone(), info.extends_type_args.clone().into())
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            return None;
        };
        let substitutions = parameters
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        bases.into_iter().find_map(|base| {
            self.library_readonly_access(&Self::substitute(&base, &substitutions), index, depth + 1)
        })
    }

    pub(super) fn report_readonly_library_write(
        &mut self,
        target: &Expr,
        deleting: bool,
    ) -> Option<bool> {
        let target = Self::skip_reference_wrappers(target);
        let (object, index, key_span) = match &target.kind {
            ExprKind::ElemAccess(access) => (
                access.object.as_ref(),
                self.infer_expr_type(&access.index),
                access.index.span,
            ),
            ExprKind::Member(member) => (
                member.object.as_ref(),
                Type::StringLiteral(member.property.to_string()),
                Span::new(
                    target.span.end - member.property.len() as u32,
                    target.span.end,
                ),
            ),
            _ => return None,
        };
        let receiver = self.infer_expr_type(object);
        let access = self.library_readonly_access(&receiver, &index, 0)?;
        match access {
            LibraryReadonlyAccess::Property(name) => {
                self.diagnostics.push(if deleting {
                    crate::diagnostics::error_delete_operand(2704, target.span)
                } else {
                    crate::diagnostics::error_cannot_assign_readonly(&name, key_span)
                });
                Some(true)
            }
            LibraryReadonlyAccess::Index => {
                self.diagnostics.push(Diagnostic {
                    code: 2542,
                    message: format!(
                        "Index signature in type '{}' only permits reading.",
                        receiver.display_string()
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(target.span),
                    related: None,
                });
                // Index writes keep their value type for assignment checking.
                Some(false)
            }
        }
    }

    pub(super) fn lookup_library_property(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
    ) -> Option<Type> {
        self.library_property(owner, arguments, name)
            .map(|property| {
                let ty = match property.ty {
                    Type::Optional(inner) if self.strict_null_checks => {
                        Type::flatten_union(vec![Type::clone(&inner), Type::Undefined])
                    }
                    Type::Optional(inner) => Type::clone(&inner),
                    ty => ty,
                };
                if self.strict_null_checks || !matches!(ty, Type::Union(_)) {
                    ty
                } else {
                    let resolved = self.remove_null_undefined(&ty);
                    if matches!(resolved, Type::Never) {
                        ty
                    } else {
                        resolved
                    }
                }
            })
    }

    pub(super) fn lookup_library_instance_method(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
    ) -> Option<Type> {
        if self.program_declares_library_owner(owner) {
            return None;
        }
        self.library_method_type(owner, arguments, name)
            .map(|method| {
                Self::substitute_this(
                    &method,
                    &Type::TypeReference(owner.to_owned(), arguments.to_vec().into()),
                )
            })
    }

    /// Primitive apparent types use their wrapper interfaces, following
    /// getApparentType in pingdotgg/ts-rust checker_p24.rs. Keep the original
    /// receiver when instantiating polymorphic `this` in a library signature.
    pub(super) fn primitive_library_owner(receiver: &Type) -> Option<&'static str> {
        match receiver {
            Type::String | Type::StringLiteral(_) => Some("String"),
            Type::Number | Type::NumberLiteral(_) => Some("Number"),
            Type::Boolean | Type::BooleanLiteral(_) => Some("Boolean"),
            Type::BigInt | Type::BigIntLiteral(_) => Some("BigInt"),
            Type::Symbol | Type::UniqueSymbol(_) => Some("Symbol"),
            _ => None,
        }
    }

    pub(super) fn lookup_primitive_member(&self, receiver: &Type, name: &str) -> Option<Type> {
        let owner = Self::primitive_library_owner(receiver)?;
        let user_declared = self.class_info.contains_key(owner)
            || self.interface_info.contains_key(owner)
            || self.type_aliases.contains_key(owner);
        (!user_declared)
            .then(|| self.library_method_type(owner, &[], name))
            .flatten()
            .map(|method| Self::substitute_this(&method, receiver))
            .or_else(|| self.builtins.lookup_instance_property(owner, name))
    }

    pub(crate) fn lookup_array_method_typed(&self, element: &Type, method: &str) -> Option<Type> {
        self.library_method_type("Array", std::slice::from_ref(element), method)
            .map(|method| Self::substitute_this(&method, &Type::Array(Arc::new(element.clone()))))
            .or_else(|| self.builtins.lookup_array_method_typed(element, method))
    }
}
