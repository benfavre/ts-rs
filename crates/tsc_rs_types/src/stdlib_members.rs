//! Declaration-based library members, adapting resolveTypeReferenceMembers /
//! resolveObjectTypeMembers from pingdotgg/ts-rust checker_p21.rs. See NOTICE.txt.
//! Native signatures retain their declaration metadata while owner parameters
//! are instantiated at the receiver, independently of method type parameters.

use super::*;
use std::sync::OnceLock;

#[derive(Default)]
struct LibraryInterface {
    type_parameters: Vec<String>,
    methods: HashMap<String, Vec<FunctionType>>,
    bases: Vec<Type>,
    numeric_index: Option<Type>,
}

type LibraryInterfaces = HashMap<String, LibraryInterface>;
type LibraryCache = Mutex<HashMap<(String, bool), Arc<LibraryInterfaces>>>;
static LIBRARY_SIGNATURES: OnceLock<LibraryCache> = OnceLock::new();

fn library_interfaces(path: &str, strict_null_checks: bool) -> Option<Arc<LibraryInterfaces>> {
    let key = (path.to_owned(), strict_null_checks);
    let cache = LIBRARY_SIGNATURES.get_or_init(Default::default);
    if let Some(cached) = cache.lock().ok()?.get(&key).cloned() {
        return Some(cached);
    }
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
        interface.bases.extend(
            declaration
                .extends
                .iter()
                .map(|base| checker.resolve_type_node(base)),
        );
        if let Type::ObjectType(object) = checker.resolve_type_node(&TypeNode {
            kind: TypeNodeKind::TypeLit(declaration.members.clone()),
            span: declaration.span,
        }) {
            if let Some((key, value)) = &object.index_signature {
                if matches!(key.as_ref(), Type::Number) {
                    interface.numeric_index = Some(value.as_ref().clone());
                }
            }
        }
        for member in &declaration.members {
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
        if resolving.len() >= 12 || resolving.iter().any(|name| name == owner) {
            return None;
        }
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
        // Later declaration groups precede earlier groups, preserving the
        // order within each group, just like merged interface overloads.
        for file in availability
            .owner_declaring_libraries(owner)
            .into_iter()
            .rev()
        {
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
            0 => bases.into_iter().find_map(|base| match base {
                Type::TypeReference(base_name, args) => {
                    self.library_method_type_inner(&base_name, &args, name, resolving)
                }
                _ => None,
            }),
            1 => signatures.pop(),
            _ => Some(Type::Intersection(signatures.into())),
        }
    }

    /// Numeric index signatures participate in contextual array typing without
    /// materializing an incomplete library object for structural relations.
    pub(super) fn library_numeric_index(&self, owner: &str, arguments: &[Type]) -> Option<Type> {
        self.library_numeric_index_inner(owner, arguments, &mut Vec::new())
    }

    fn library_numeric_index_inner(
        &self,
        owner: &str,
        arguments: &[Type],
        resolving: &mut Vec<String>,
    ) -> Option<Type> {
        if self.class_info.contains_key(owner)
            || self.interface_info.contains_key(owner)
            || self.type_aliases.contains_key(owner)
            || resolving.iter().any(|name| name == owner)
        {
            return None;
        }
        resolving.push(owner.to_owned());
        let result = (|| {
            let availability = stdlib::stdlib_member_availability(&self.compiler_options)?;
            let mut bases = Vec::new();
            for file in availability
                .owner_declaring_libraries(owner)
                .into_iter()
                .rev()
            {
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
                if let Some(element) = &interface.numeric_index {
                    return Some(Self::substitute(element, &substitutions));
                }
                bases.extend(
                    interface
                        .bases
                        .iter()
                        .map(|base| Self::substitute(base, &substitutions)),
                );
            }
            bases.into_iter().find_map(|base| match base {
                Type::TypeReference(name, args) => {
                    self.library_numeric_index_inner(&name, &args, resolving)
                }
                _ => None,
            })
        })();
        resolving.pop();
        result
    }

    pub(super) fn lookup_library_instance_method(
        &self,
        owner: &str,
        arguments: &[Type],
        name: &str,
    ) -> Option<Type> {
        if self.class_info.contains_key(owner)
            || self.interface_info.contains_key(owner)
            || self.type_aliases.contains_key(owner)
        {
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

    pub(crate) fn lookup_array_method_typed(&self, element: &Type, method: &str) -> Option<Type> {
        self.library_method_type("Array", std::slice::from_ref(element), method)
            .map(|method| Self::substitute_this(&method, &Type::Array(Arc::new(element.clone()))))
            .or_else(|| self.builtins.lookup_array_method_typed(element, method))
    }
}
