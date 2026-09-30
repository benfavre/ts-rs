//! Class and interface type building methods for TypeChecker.

use std::sync::Arc;

use super::*;

/// When an interface (or object type literal) declares the same method
/// name multiple times, TypeScript treats the sequence as overload
/// signatures of one callable. The natural encoding is
/// `Type::Intersection([Function(sig1), Function(sig2), ...])` —
/// matches what TS Hierarchy returns for `typeof obj.method`.
///
/// `build_interface_info` already pushes each MethodSig as its own
/// `(name, Function)` property; this post-pass collapses duplicates.
/// Non-function dupes (interface authoring error in user code) are
/// passed through as-is; the first-wins shape they had before still
/// holds for them.
pub(crate) fn merge_overloaded_function_props(
    props: Vec<(std::string::String, Arc<Type>)>,
) -> Vec<(std::string::String, Arc<Type>)> {
    let mut by_name: std::collections::HashMap<std::string::String, Vec<Arc<Type>>> =
        std::collections::HashMap::new();
    let mut order: Vec<std::string::String> = Vec::new();
    for (name, ty) in props {
        if !by_name.contains_key(&name) {
            order.push(name.clone());
        }
        let entries = by_name.entry(name).or_default();
        if !entries
            .iter()
            .any(|existing| existing.as_ref() == ty.as_ref())
        {
            entries.push(ty);
        }
    }
    let mut out = Vec::with_capacity(order.len());
    for name in order {
        let entries = by_name.remove(&name).unwrap_or_default();
        if entries.len() == 1 {
            out.push((name, entries.into_iter().next().unwrap()));
            continue;
        }
        // Multiple props with the same name: if they are all functions,
        // intersect them as overload sigs. If any non-function appears,
        // keep the first entry (matches the legacy first-wins shape).
        let all_funcs = entries.iter().all(|ty| match ty.as_ref() {
            Type::Function(_) => true,
            Type::Intersection(members) => members
                .iter()
                .all(|member| matches!(member, Type::Function(_))),
            _ => false,
        });
        if all_funcs {
            let mut funcs: Vec<Type> = Vec::new();
            for ty in entries {
                match Type::clone(&ty) {
                    Type::Intersection(members) => {
                        for member in members.iter() {
                            if !funcs.contains(member) {
                                funcs.push(member.clone());
                            }
                        }
                    }
                    function => {
                        if !funcs.contains(&function) {
                            funcs.push(function);
                        }
                    }
                }
            }
            out.push((name, Arc::new(Type::Intersection(funcs.into()))));
        } else {
            out.push((name, entries.into_iter().next().unwrap()));
        }
    }
    out
}

impl TypeChecker {
    /// Accessor signatures have stricter grammar than ordinary functions.
    pub(crate) fn check_accessor_signature_grammar(
        &mut self,
        is_getter: bool,
        name: &PropName,
        params: &[Param],
        return_type: Option<&TypeNode>,
        has_type_parameters: bool,
    ) {
        // The parser reports general parameter-list grammar first. Do not add
        // a second, accessor-specific grammar error for the same malformed list.
        let mut seen_optional = false;
        for (index, param) in params.iter().enumerate() {
            if param.dotdotdot {
                if index + 1 != params.len() || param.optional || param.initializer.is_some() {
                    return;
                }
            } else if param.optional {
                seen_optional = true;
                if param.initializer.is_some() {
                    return;
                }
            } else if seen_optional && param.initializer.is_none() {
                return;
            }
        }
        let expected = if is_getter { 0 } else { 1 };
        let has_this = params.len() == expected + 1
            && matches!(&params[0].name.kind, PatKind::Ident(name) if name == "this");
        let value_params = if has_this { &params[1..] } else { params };
        let diagnostic = if has_type_parameters {
            Some((
                1094,
                "An accessor cannot have type parameters.",
                name.span(),
            ))
        } else if value_params.len() != expected {
            Some(if is_getter {
                (
                    1054,
                    "A 'get' accessor cannot have parameters.",
                    name.span(),
                )
            } else {
                (
                    1049,
                    "A 'set' accessor must have exactly one parameter.",
                    name.span(),
                )
            })
        } else if is_getter {
            None
        } else if return_type.is_some() {
            Some((
                1095,
                "A 'set' accessor cannot have a return type annotation.",
                name.span(),
            ))
        } else {
            let param = &value_params[0];
            if param.dotdotdot {
                let rest_start = self
                    .current_source
                    .as_deref()
                    .and_then(|source| {
                        let start = param.span.start as usize;
                        let end = (param.name.span.start as usize).min(source.len());
                        source
                            .get(start..end)?
                            .rfind("...")
                            .map(|offset| param.span.start + offset as u32)
                    })
                    .unwrap_or(param.span.start);
                Some((
                    1053,
                    "A 'set' accessor cannot have rest parameter.",
                    Span::new(rest_start, rest_start + 3),
                ))
            } else if param.optional {
                let question = self
                    .current_source
                    .as_deref()
                    .and_then(|source| {
                        let start = param.name.span.end as usize;
                        let end = param
                            .type_ann
                            .as_ref()
                            .map_or(param.span.end, |ty| ty.span.start)
                            as usize;
                        source
                            .get(start..end)?
                            .rfind('?')
                            .map(|offset| start as u32 + offset as u32)
                    })
                    .unwrap_or(param.name.span.end);
                Some((
                    1051,
                    "A 'set' accessor cannot have an optional parameter.",
                    Span::new(question, question + 1),
                ))
            } else if param.initializer.is_some() {
                Some((
                    1052,
                    "A 'set' accessor parameter cannot have an initializer.",
                    name.span(),
                ))
            } else {
                None
            }
        };
        if let Some((code, message, span)) = diagnostic {
            self.diagnostics.push(Diagnostic {
                code,
                message: message.to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(span),
                related: None,
            });
        }
    }

    pub(crate) fn check_accessor_pair_modifiers(
        &mut self,
        accessor: &ClassAccessor,
        is_getter: bool,
        siblings: &[ClassMember],
    ) {
        if matches!(&accessor.name, PropName::Ident(name, _) if name == "constructor") {
            self.diagnostics.push(Diagnostic {
                code: 1341,
                message: "Class constructor may not be an accessor.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(accessor.name.span()),
                related: None,
            });
        }
        if !is_getter {
            return;
        }
        let accessor_key = |other: &ClassAccessor| match &other.name {
            PropName::Computed(expression, _) => self.computed_property_name(expression),
            name => Some(self.prop_name_to_string(name)),
        };
        let Some(key) = accessor_key(accessor) else {
            return;
        };
        let same_scope =
            |other: &ClassAccessor| other.modifiers & MOD_STATIC == accessor.modifiers & MOD_STATIC;
        // Each accessor symbol is checked once, from its first getter.
        let first_getter = siblings.iter().find_map(|member| match &member.kind {
            ClassMemberKind::GetAccessor(other)
                if same_scope(other) && accessor_key(other).as_ref() == Some(&key) =>
            {
                Some(other)
            }
            _ => None,
        });
        if first_getter.is_some_and(|first| first.name.span() != accessor.name.span()) {
            return;
        }
        let setter = siblings.iter().find_map(|member| match &member.kind {
            ClassMemberKind::SetAccessor(other)
                if same_scope(other) && accessor_key(other).as_ref() == Some(&key) =>
            {
                Some(other)
            }
            _ => None,
        });
        let Some(setter) = setter else {
            return;
        };
        let abstract_mismatch =
            accessor.modifiers & MOD_ABSTRACT != setter.modifiers & MOD_ABSTRACT;
        let visibility_mismatch = (accessor.modifiers & MOD_PROTECTED != 0
            && setter.modifiers & (MOD_PROTECTED | MOD_PRIVATE) == 0)
            || (accessor.modifiers & MOD_PRIVATE != 0 && setter.modifiers & MOD_PRIVATE == 0);
        for (applies, code, message) in [
            (
                abstract_mismatch,
                2676,
                "Accessors must both be abstract or non-abstract.",
            ),
            (
                visibility_mismatch,
                2808,
                "A get accessor must be at least as accessible as the setter",
            ),
        ] {
            if applies {
                for name in [&accessor.name, &setter.name] {
                    self.diagnostics.push(Diagnostic {
                        code,
                        message: message.to_string(),
                        category: DiagnosticCategory::Error,
                        file_name: None,
                        span: Some(name.span()),
                        related: None,
                    });
                }
            }
        }
    }

    pub(crate) fn check_getter_missing_return(&mut self, name: &PropName, body: &[Stmt]) {
        if self.ambient_depth == 0
            && !self.current_file_is_declaration()
            && crate::accessor_flow::missing_return(body)
        {
            self.diagnostics.push(Diagnostic {
                code: 2378,
                message: "A 'get' accessor must return a value.".to_string(),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(name.span()),
                related: None,
            });
        }
    }

    pub(crate) fn check_setter_value_returns(&mut self, body: &[Stmt]) {
        let mut pending: Vec<&Stmt> = body.iter().rev().collect();
        while let Some(statement) = pending.pop() {
            match &statement.kind {
                StmtKind::Return(Some(_)) => self.diagnostics.push(Diagnostic {
                    code: 2408,
                    message: "Setters cannot return a value.".to_string(),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(Span::new(statement.span.start, statement.span.start + 6)),
                    related: None,
                }),
                StmtKind::Block(body) => pending.extend(body.iter().rev()),
                StmtKind::If(branch) => {
                    if let Some(alternate) = &branch.alternate {
                        pending.push(alternate);
                    }
                    pending.push(&branch.consequent);
                }
                StmtKind::While(loop_stmt) => pending.push(&loop_stmt.body),
                StmtKind::DoWhile(loop_stmt) => pending.push(&loop_stmt.body),
                StmtKind::For(loop_stmt) => pending.push(&loop_stmt.body),
                StmtKind::ForIn(loop_stmt) => pending.push(&loop_stmt.body),
                StmtKind::ForOf(loop_stmt) => pending.push(&loop_stmt.body),
                StmtKind::Switch(switch) => {
                    for case in switch.cases.iter().rev() {
                        pending.extend(case.consequent.iter().rev());
                    }
                }
                StmtKind::Try(try_stmt) => {
                    if let Some(finalizer) = &try_stmt.finalizer {
                        pending.extend(finalizer.iter().rev());
                    }
                    if let Some(handler) = &try_stmt.handler {
                        pending.extend(handler.body.iter().rev());
                    }
                    pending.extend(try_stmt.block.iter().rev());
                }
                StmtKind::Labeled(label) => pending.push(&label.body),
                StmtKind::With(with) => pending.push(&with.body),
                // Nested function/class bodies have their own return context.
                _ => {}
            }
        }
    }

    /// Correct the declaration-sequence diagnostics for class overloads.
    ///
    /// The file-level missing-implementation pass deliberately runs before
    /// statement checking.  A bodyless method followed by a *differently*
    /// named method implementation is the one case where its provisional
    /// TS2391 must instead become TS2389 on the implementation name.  Keeping
    /// this class-local refinement here also lets computed and literal names
    /// use the same structural rule as identifier names.
    fn check_class_overload_sequences(&mut self, class_decl: &ClassDecl) {
        if self.current_file_is_declaration()
            || self.ambient_depth > 0
            || class_decl.modifiers & MOD_DECLARE != 0
        {
            return;
        }

        fn key(name: &PropName) -> Option<std::string::String> {
            match name {
                PropName::Ident(value, _)
                | PropName::String(value, _)
                | PropName::Number(value, _)
                | PropName::Private(value, _) => Some(value.to_string()),
                PropName::Computed(expr, _) => match &expr.kind {
                    ExprKind::Ident(value) | ExprKind::StrLit(value) | ExprKind::NumLit(value) => {
                        Some(value.to_string())
                    }
                    _ => None,
                },
            }
        }

        fn source_name(source: Option<&str>, name: &PropName) -> std::string::String {
            let span = name.span();
            source
                .and_then(|text| text.get(span.start as usize..span.end as usize))
                .map(str::to_string)
                .unwrap_or_else(|| match name {
                    PropName::Ident(value, _)
                    | PropName::String(value, _)
                    | PropName::Number(value, _)
                    | PropName::Private(value, _) => value.to_string(),
                    PropName::Computed(_, _) => "[computed]".to_string(),
                })
        }

        let source = self.current_source.as_deref();
        for pair in class_decl.members.windows(2) {
            let (ClassMemberKind::Method(signature), ClassMemberKind::Method(implementation)) =
                (&pair[0].kind, &pair[1].kind)
            else {
                continue;
            };
            if signature.body.is_some()
                || signature.modifiers & MOD_ABSTRACT != 0
                || implementation.body.is_none()
            {
                continue;
            }
            if key(&signature.name) == key(&implementation.name) {
                continue;
            }

            // Remove the provisional missing-implementation diagnostic that
            // was anchored at the overload signature.
            let signature_span = signature.name.span();
            self.diagnostics.retain(|diag| {
                diag.code != 2391
                    || !diag
                        .span
                        .is_some_and(|span| span.start == signature_span.start)
            });
            self.diagnostics.push(Diagnostic {
                code: 2389,
                message: format!(
                    "Function implementation name must be '{}'.",
                    source_name(source, &signature.name)
                ),
                category: DiagnosticCategory::Error,
                file_name: None,
                span: Some(implementation.name.span()),
                related: None,
            });
        }

        // The older pre-pass skips abstract classes wholesale. Non-abstract
        // overload declarations inside an abstract class still require an
        // implementation, however (an explicitly `abstract` method does not).
        if class_decl.modifiers & MOD_ABSTRACT != 0 {
            for (index, member) in class_decl.members.iter().enumerate() {
                let ClassMemberKind::Method(method) = &member.kind else {
                    continue;
                };
                if method.body.is_some() || method.modifiers & MOD_ABSTRACT != 0 {
                    continue;
                }
                let followed_by_same = class_decl.members.get(index + 1).is_some_and(|next| {
                    let ClassMemberKind::Method(next_method) = &next.kind else {
                        return false;
                    };
                    key(&method.name) == key(&next_method.name)
                        && method.modifiers & MOD_STATIC == next_method.modifiers & MOD_STATIC
                });
                if followed_by_same {
                    continue;
                }
                let span = method.name.span();
                if !self.diagnostics.iter().any(|diag| {
                    diag.code == 2391
                        && diag
                            .span
                            .is_some_and(|existing| existing.start == span.start)
                }) {
                    self.diagnostics.push(
                        super::diagnostics::error_function_implementation_missing(span),
                    );
                }
            }
        }

        self.check_class_overload_compatibility(&class_decl.members);
    }

    /// Remember each method overload set's bodied implementation (see
    /// `overload_implementations`), keyed like the callable intersection.
    fn record_overload_implementations(
        &self,
        methods: &[(String, FunctionType)],
        bodies: &[Option<Span>],
    ) {
        let file = self
            .injected_decl_file
            .as_ref()
            .or(self.current_file_name.as_ref())
            .cloned();
        for (index, (name, signature)) in methods.iter().enumerate() {
            let Some(span) = bodies[index] else {
                continue;
            };
            let overloads: Vec<FunctionType> = methods
                .iter()
                .zip(bodies)
                .filter(|((other, _), body)| other == name && body.is_none())
                .map(|((_, overload), _)| overload.clone())
                .collect();
            if overloads.len() < 2 {
                continue;
            }
            self.overload_implementations
                .lock()
                .expect("overload_implementations lock poisoned")
                .entry(overloads)
                .or_insert_with(|| (signature.clone(), file.clone(), span));
        }
    }

    pub(crate) fn build_class_info(&self, class_decl: &ClassDecl) -> ClassInfo {
        self.enter_type_resolution_type_params(
            class_decl
                .type_params
                .iter()
                .flat_map(|params| params.iter().map(|param| &param.name)),
        );
        let is_abstract = (class_decl.modifiers & MOD_ABSTRACT) != 0;
        let extends = class_decl
            .extends
            .as_ref()
            .map(|e| self.namespace_owned_type_name(&self.expr_to_name(e)));
        let extends_type_args: Vec<Type> = class_decl
            .extends_type_args
            .as_ref()
            .map(|nodes| nodes.iter().map(|n| self.resolve_type_node(n)).collect())
            .unwrap_or_default();
        let type_params: Vec<std::string::String> = class_decl
            .type_params
            .as_ref()
            .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
            .unwrap_or_default();
        let type_param_constraints = class_decl
            .type_params
            .as_ref()
            .map(|params| {
                params
                    .iter()
                    .map(|param| {
                        param
                            .constraint
                            .as_ref()
                            .map(|constraint| self.resolve_type_node(constraint))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let type_param_defaults = class_decl
            .type_params
            .as_ref()
            .map(|params| {
                params
                    .iter()
                    .map(|param| {
                        param
                            .default
                            .as_ref()
                            .map(|default| self.resolve_type_node(default))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let required_type_params = class_decl
            .type_params
            .as_ref()
            .map(|tps| tps.iter().filter(|tp| tp.default.is_none()).count())
            .unwrap_or(0);

        let mut constructor_params = Vec::new();
        let mut constructor_overloads = Vec::new();
        let mut has_explicit_constructor = false;
        let mut constructor_declaration_count = 0usize;
        let mut constructor_body_count = 0usize;
        let mut instance_properties = Vec::new();
        let mut optional_instance_properties: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut optional_static_properties: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut accessor_props: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut own_private_members: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut own_protected_members = rustc_hash::FxHashSet::default();
        let mut own_private_static_members: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut readonly_members: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut setter_names: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut abstract_instance_props: rustc_hash::FxHashSet<std::string::String> =
            rustc_hash::FxHashSet::default();
        let mut instance_methods = Vec::new();
        // Parallel to instance_methods: whether each declaration had a body
        // (overload signatures are bodyless; the implementation is bodied).
        // `Some(name span)` for a bodied declaration.
        let mut instance_method_has_body: Vec<Option<Span>> = Vec::new();
        let mut static_properties = Vec::new();
        let mut static_methods = Vec::new();
        let mut static_method_has_body: Vec<Option<Span>> = Vec::new();
        let mut static_member_modifiers = Vec::new();
        let mut instance_member_modifiers = Vec::new();
        let mut accessor_modifiers = rustc_hash::FxHashMap::default();

        for member in &class_decl.members {
            if let ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) =
                &member.kind
            {
                let key = (
                    self.prop_name_to_string(&accessor.name),
                    accessor.modifiers & MOD_STATIC != 0,
                );
                let flags = accessor_modifiers.entry(key).or_insert((None, None));
                if matches!(member.kind, ClassMemberKind::GetAccessor(_)) {
                    flags.0 = Some(accessor.modifiers);
                } else {
                    flags.1 = Some(accessor.modifiers);
                }
            }
            let declaration = match &member.kind {
                ClassMemberKind::Property(p) => Some((&p.name, p.modifiers)),
                ClassMemberKind::Method(m) => Some((&m.name, m.modifiers)),
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    Some((&a.name, a.modifiers))
                }
                _ => None,
            };
            if let Some((name, modifiers)) = declaration {
                if modifiers & MOD_STATIC == 0 {
                    let name = self.prop_name_to_string(name);
                    if !instance_member_modifiers.iter().any(|(n, _)| n == &name) {
                        instance_member_modifiers.push((name, modifiers));
                    }
                }
                // `#name` accessors are private instance members too.
                if modifiers & MOD_STATIC == 0 && matches!(name, PropName::Private(..)) {
                    own_private_members.insert(self.prop_name_to_string(name));
                }
                if modifiers & MOD_STATIC != 0 {
                    let name = self.prop_name_to_string(name);
                    if modifiers & MOD_PRIVATE != 0 || name.starts_with('#') {
                        own_private_static_members.insert(name.clone());
                    }
                    if !static_member_modifiers.iter().any(|(n, _)| n == &name) {
                        static_member_modifiers.push((name, modifiers));
                    }
                }
            }
            let protected_name = match &member.kind {
                ClassMemberKind::Property(property)
                    if property.modifiers & MOD_PROTECTED != 0
                        && property.modifiers & MOD_STATIC == 0 =>
                {
                    Some(&property.name)
                }
                ClassMemberKind::Method(method)
                    if method.modifiers & MOD_PROTECTED != 0
                        && method.modifiers & MOD_STATIC == 0 =>
                {
                    Some(&method.name)
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor)
                    if accessor.modifiers & MOD_PROTECTED != 0
                        && accessor.modifiers & MOD_STATIC == 0 =>
                {
                    Some(&accessor.name)
                }
                _ => None,
            };
            if let Some(name) = protected_name {
                own_protected_members.insert(self.prop_name_to_string(name));
            }
            match &member.kind {
                ClassMemberKind::Constructor(ctor) => {
                    has_explicit_constructor = true;
                    constructor_declaration_count += 1;
                    if ctor.body.is_some() {
                        constructor_body_count += 1;
                    }
                    let mut declaration_params = Vec::new();
                    let last_required = ctor
                        .params
                        .iter()
                        .rposition(|p| !p.optional && p.initializer.is_none() && !p.dotdotdot);
                    for (index, param) in ctor.params.iter().enumerate() {
                        if param.modifiers & MOD_PROTECTED != 0 {
                            if let PatKind::Ident(name) = &param.name.kind {
                                own_protected_members.insert(name.to_string());
                            }
                        }
                        let pname = match &param.name.kind {
                            PatKind::Ident(n) => n.to_string(),
                            _ => "_".to_string(),
                        };
                        let pty = param
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .or_else(|| {
                                param
                                    .initializer
                                    .as_ref()
                                    .map(|init| self.widen_type(&self.infer_expr_type(init)))
                            })
                            .unwrap_or(Type::Any);
                        let optional = param.optional
                            || param.initializer.is_some()
                                && last_required.is_none_or(|last| index > last);
                        let has_rest = param.dotdotdot;
                        declaration_params.push((pname.clone(), pty.clone(), optional, has_rest));
                        // Parameter properties: public/private/protected/readonly params
                        // become instance properties
                        let is_param_prop = param.modifiers
                            & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY)
                            != 0;
                        if is_param_prop {
                            if !instance_member_modifiers.iter().any(|(n, _)| n == &pname) {
                                instance_member_modifiers.push((pname.clone(), param.modifiers));
                            }
                            if (param.modifiers & MOD_PRIVATE) != 0 {
                                own_private_members.insert(pname.clone());
                            }
                            // `constructor(public two?: U)` declares an optional property.
                            if param.optional {
                                optional_instance_properties.insert(pname.clone());
                            }
                            instance_properties.push((pname, pty));
                        }
                    }
                    if ctor.body.is_none() {
                        constructor_overloads.push(
                            declaration_params
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
                                .collect(),
                        );
                    }
                    if ctor.body.is_some() || constructor_declaration_count == 1 {
                        constructor_params = declaration_params;
                    }
                }
                ClassMemberKind::Property(prop) => {
                    let name = self.prop_name_to_string(&prop.name);
                    if (prop.modifiers & tsc_rs_ast::MOD_READONLY) != 0 {
                        readonly_members.insert(name.clone());
                    }
                    let ty = prop
                        .type_ann
                        .as_ref()
                        .map(|t| {
                            if prop.modifiers & (MOD_STATIC | MOD_READONLY)
                                == (MOD_STATIC | MOD_READONLY)
                                && Self::type_node_is_unique_symbol(t)
                            {
                                let class_name =
                                    class_decl.name.as_deref().unwrap_or("<anonymous>");
                                let file = self
                                    .injected_decl_file
                                    .as_deref()
                                    .or(self.current_file_name.as_deref())
                                    .unwrap_or("");
                                Type::UniqueSymbol(format!(
                                    "{class_name}.{name}\0{file}\0{}",
                                    prop.name.span().start
                                ))
                            } else {
                                self.resolve_type_node(t)
                            }
                        })
                        .unwrap_or_else(|| {
                            let class_name = class_decl.name.as_deref().unwrap_or("<anonymous>");
                            if let Some(unique) = self.inferred_unique_symbol(
                                &format!("{class_name}.{name}"),
                                prop.name.span().start,
                                prop.initializer.as_deref(),
                                prop.modifiers & (MOD_STATIC | MOD_READONLY)
                                    == (MOD_STATIC | MOD_READONLY),
                            ) {
                                return unique;
                            }
                            // Widen literal types from initializers (3 → number, "hello" → string)
                            // Readonly scalar properties and `as const` preserve literals.
                            prop.initializer
                                .as_ref()
                                .map(|init| {
                                    let inferred = self.infer_expr_type(init);
                                    if Self::is_const_assertion_expr(init)
                                        || prop.modifiers & MOD_READONLY != 0
                                            && matches!(
                                                inferred,
                                                Type::StringLiteral(_)
                                                    | Type::NumberLiteral(_)
                                                    | Type::BooleanLiteral(_)
                                                    | Type::BigIntLiteral(_)
                                            )
                                    {
                                        inferred
                                    } else {
                                        Self::widen_nested_literals(
                                            &Self::erase_inferred_unique_symbols(&inferred),
                                        )
                                    }
                                })
                                .unwrap_or(Type::Any)
                        });
                    if (prop.modifiers & MOD_STATIC) != 0 {
                        if prop.optional {
                            optional_static_properties.insert(name.clone());
                        }
                        if (prop.modifiers & MOD_PRIVATE) != 0 || name.starts_with('#') {
                            own_private_static_members.insert(name.clone());
                        }
                        static_properties.push((name, ty));
                    } else {
                        if prop.optional {
                            optional_instance_properties.insert(name.clone());
                        }
                        if (prop.modifiers & MOD_ABSTRACT) != 0 {
                            abstract_instance_props.insert(name.clone());
                        }
                        if (prop.modifiers & MOD_ACCESSOR) != 0 {
                            accessor_props.insert(name.clone());
                        }
                        if (prop.modifiers & MOD_PRIVATE) != 0 || name.starts_with('#') {
                            own_private_members.insert(name.clone());
                        }
                        instance_properties.push((name, ty));
                    }
                }
                ClassMemberKind::Method(method) => {
                    let name = self.prop_name_to_string(&method.name);
                    let is_private = (method.modifiers & MOD_PRIVATE) != 0 || name.starts_with('#');
                    let type_params: Vec<_> = method
                        .type_params
                        .as_ref()
                        .map(|params| params.iter().map(|param| param.name.clone()).collect())
                        .unwrap_or_default();
                    self.enter_type_resolution_type_params(type_params.iter());
                    let last_required = method
                        .params
                        .iter()
                        .rposition(|p| !p.optional && p.initializer.is_none() && !p.dotdotdot);
                    let params: Vec<_> = method
                        .params
                        .iter()
                        .enumerate()
                        .map(|(index, p)| {
                            let base_name = match &p.name.kind {
                                PatKind::Ident(n) => n.to_string(),
                                _ => "_".to_string(),
                            };
                            let pname = if p.dotdotdot {
                                format!("...{}", base_name)
                            } else if p.optional
                                || p.initializer.is_some()
                                    && last_required.is_none_or(|last| index > last)
                            {
                                format!("?{}", base_name)
                            } else {
                                base_name
                            };
                            let pty = p
                                .type_ann
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .or_else(|| {
                                    p.initializer
                                        .as_ref()
                                        .map(|init| self.widen_type(&self.infer_expr_type(init)))
                                })
                                .unwrap_or(Type::Any);
                            (pname, self.declared_optional_param_type(p, pty))
                        })
                        .collect();
                    let ret = method
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .unwrap_or_else(|| {
                            if let Some(ref body) = method.body {
                                // Bind the parameters so `return a` resolves
                                // to the parameter's declared type (as the
                                // FnDecl registration path does).
                                let bindings: Vec<_> = method
                                    .params
                                    .iter()
                                    .zip(params.iter())
                                    .filter_map(|(p, (_, pty))| match &p.name.kind {
                                        PatKind::Ident(n) => Some((n.to_string(), pty.clone())),
                                        _ => None,
                                    })
                                    .collect();
                                let inferred = crate::with_infer_param_scope(bindings, || {
                                    self.infer_return_type_from_stmts(body)
                                });
                                // An async method's inferred return is a Promise; a
                                // generator method's is a (Async)Generator.
                                if method.is_generator {
                                    let owner = if method.is_async {
                                        "AsyncGenerator"
                                    } else {
                                        "Generator"
                                    };
                                    Type::TypeReference(
                                        owner.into(),
                                        vec![Type::Any, Type::Any, Type::Any].into(),
                                    )
                                } else if method.is_async {
                                    Self::wrap_async_inferred_return(inferred)
                                } else {
                                    inferred
                                }
                            } else {
                                Type::Any
                            }
                        });
                    let mut ft = FunctionType {
                        type_param_constraints: Vec::new(),
                        params,
                        return_type: Arc::new(ret),
                        type_params,
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    };
                    for param in method.type_params.as_deref().unwrap_or_default() {
                        ft.type_param_constraints
                            .push(param.constraint.as_ref().map(|t| self.resolve_type_node(t)));
                        ft.type_param_defaults
                            .push(param.default.as_ref().map(|t| self.resolve_type_node(t)));
                    }
                    self.exit_type_resolution_type_params();
                    if (method.modifiers & MOD_STATIC) != 0 {
                        if method.optional {
                            optional_static_properties.insert(name.clone());
                        }
                        if is_private {
                            own_private_static_members.insert(name.clone());
                        }
                        static_method_has_body
                            .push(method.body.as_ref().map(|_| method.name.span()));
                        static_methods.push((name, ft));
                    } else {
                        if method.optional {
                            optional_instance_properties.insert(name.clone());
                        }
                        if is_private {
                            own_private_members.insert(name.clone());
                        }
                        instance_method_has_body
                            .push(method.body.as_ref().map(|_| method.name.span()));
                        instance_methods.push((name, ft));
                    }
                }
                ClassMemberKind::GetAccessor(acc) => {
                    let name = self.prop_name_to_string(&acc.name);
                    let ty = acc
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .unwrap_or_else(|| {
                            if let Some(ref body) = acc.body {
                                self.infer_return_type_from_stmts(body)
                            } else {
                                Type::Any
                            }
                        });
                    if (acc.modifiers & MOD_STATIC) != 0 {
                        if let Some((_, existing)) =
                            static_properties.iter_mut().find(|(n, _)| n == &name)
                        {
                            *existing = ty;
                        } else {
                            static_properties.push((name, ty));
                        }
                    } else {
                        if (acc.modifiers & MOD_ABSTRACT) != 0 {
                            abstract_instance_props.insert(name.clone());
                        }
                        instance_properties.push((name, ty));
                    }
                }
                ClassMemberKind::SetAccessor(acc) => {
                    // A write-only setter still declares the member (needed so
                    // `this.x = ...` doesn't trip TS2339). Don't overwrite a
                    // getter's read type when both exist.
                    if (acc.modifiers & MOD_STATIC) != 0 {
                        let name = self.prop_name_to_string(&acc.name);
                        if !static_properties.iter().any(|(n, _)| n == &name) {
                            let ty = acc
                                .params
                                .first()
                                .and_then(|p| p.type_ann.as_ref())
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            static_properties.push((name, ty));
                        }
                    } else {
                        let name = self.prop_name_to_string(&acc.name);
                        if (acc.modifiers & MOD_ABSTRACT) != 0 {
                            abstract_instance_props.insert(name.clone());
                        }
                        if !instance_properties.iter().any(|(n, _)| n == &name) {
                            let ty = acc
                                .params
                                .first()
                                .and_then(|p| p.type_ann.as_ref())
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            instance_properties.push((name, ty));
                        }
                    }
                }
                _ => {}
            }
        }

        // A method name declared multiple times is an overload set. The
        // callable member type is the Intersection of the SIGNATURE
        // declarations — the bodied implementation is excluded (not callable
        // when signatures exist). In ambient/declare classes every declaration
        // is bodyless and all are callable. Downstream, the call paths run
        // overload selection on the Intersection (TS2769 when nothing
        // matches) instead of last-wins clobbering all signatures with the
        // implementation's lenient `(x: any)`.
        fn callable_overloads(
            methods: &[(String, FunctionType)],
            has_body: &[Option<Span>],
        ) -> rustc_hash::FxHashMap<String, Type> {
            let mut overloaded_methods: rustc_hash::FxHashMap<std::string::String, Type> =
                rustc_hash::FxHashMap::default();
            {
                let mut by_name: Vec<(&str, Vec<usize>)> = Vec::new();
                for (i, (name, _)) in methods.iter().enumerate() {
                    if let Some((_, list)) = by_name.iter_mut().find(|(n, _)| *n == name.as_str()) {
                        list.push(i);
                    } else {
                        by_name.push((name.as_str(), vec![i]));
                    }
                }
                for (name, decls) in &by_name {
                    if decls.len() < 2 {
                        continue;
                    }
                    let any_body = decls.iter().any(|&i| has_body[i].is_some());
                    let callable: Vec<Type> = decls
                        .iter()
                        .filter(|&&i| !any_body || has_body[i].is_none())
                        .map(|&i| Type::Function(methods[i].1.clone()))
                        .collect();
                    match callable.len() {
                        0 => {}
                        1 => {
                            overloaded_methods
                                .insert(name.to_string(), callable.into_iter().next().unwrap());
                        }
                        _ => {
                            overloaded_methods
                                .insert(name.to_string(), Type::Intersection(callable.into()));
                        }
                    }
                }
            }
            overloaded_methods
        }
        let overloaded_methods = callable_overloads(&instance_methods, &instance_method_has_body);
        let overloaded_static_methods =
            callable_overloads(&static_methods, &static_method_has_body);
        self.record_overload_implementations(&instance_methods, &instance_method_has_body);
        self.record_overload_implementations(&static_methods, &static_method_has_body);

        // JS field pattern post-pass: `this.x = init` assignments in the
        // constructor, method bodies, and arrow property initializers
        // declare instance properties — AFTER declared members, so explicit
        // declarations always win the name.
        // During cross-file injection the declaring file is the injected one.
        let js_file = self.current_file_is_js()
            || (self.current_file_name.is_none()
                && self.injected_decl_file.as_deref().is_some_and(|file| {
                    [".js", ".jsx", ".mjs", ".cjs"]
                        .iter()
                        .any(|extension| file.ends_with(extension))
                }));
        for member in &class_decl.members {
            match &member.kind {
                ClassMemberKind::Constructor(ctor) => {
                    if let Some(ref body) = ctor.body {
                        self.collect_this_assignments_props(body, &mut instance_properties);
                    }
                }
                ClassMemberKind::Method(method)
                    if js_file && (method.modifiers & MOD_STATIC) == 0 =>
                {
                    if let Some(ref body) = method.body {
                        self.collect_this_assignments_props(body, &mut instance_properties);
                    }
                }
                ClassMemberKind::Property(prop)
                    if js_file && (prop.modifiers & MOD_STATIC) == 0 =>
                {
                    if let Some(ref init) = prop.initializer {
                        if let ExprKind::Arrow(arrow) = &init.kind {
                            match &arrow.body {
                                ArrowBody::Block(stmts) => self.collect_this_assignments_props(
                                    stmts,
                                    &mut instance_properties,
                                ),
                                ArrowBody::Expr(_) => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let info = ClassInfo {
            member_locations: {
                let mut locations = rustc_hash::FxHashMap::default();
                let file = self
                    .injected_decl_file
                    .as_ref()
                    .or(self.current_file_name.as_ref())
                    .cloned()
                    .unwrap_or_default();
                for member in &class_decl.members {
                    let name = match &member.kind {
                        ClassMemberKind::Property(property) => &property.name,
                        ClassMemberKind::Method(method) => &method.name,
                        ClassMemberKind::GetAccessor(accessor)
                        | ClassMemberKind::SetAccessor(accessor) => &accessor.name,
                        // Parameter properties (`constructor(private x)`).
                        ClassMemberKind::Constructor(ctor) => {
                            const PROPERTY_MODIFIERS: tsc_rs_ast::ModifierFlags =
                                tsc_rs_ast::MOD_PUBLIC
                                    | tsc_rs_ast::MOD_PRIVATE
                                    | tsc_rs_ast::MOD_PROTECTED
                                    | tsc_rs_ast::MOD_READONLY;
                            for param in &ctor.params {
                                if param.modifiers & PROPERTY_MODIFIERS == 0 {
                                    continue;
                                }
                                if let PatKind::Ident(n) = &param.name.kind {
                                    locations
                                        .entry(n.to_string())
                                        .or_insert((file.clone(), param.name.span));
                                }
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    locations
                        .entry(self.prop_name_to_string(name))
                        .or_insert((file.clone(), name.span()));
                }
                locations
            },
            index_signatures: self.class_index_signature_types(class_decl),
            computed_member_origins: self.class_computed_index_members(class_decl),
            constructor_params,
            constructor_overloads: if constructor_declaration_count > 1 {
                constructor_overloads
            } else {
                Vec::new()
            },
            has_explicit_constructor,
            constructor_contextual_typing_safe: constructor_declaration_count == 1
                && constructor_body_count == 1
                && type_params.is_empty(),
            instance_properties,
            optional_instance_properties,
            optional_static_properties,
            abstract_instance_props,
            readonly_members: {
                // A getter with no matching setter is read-only.
                for m in &class_decl.members {
                    match &m.kind {
                        ClassMemberKind::SetAccessor(a) => {
                            setter_names.insert(self.prop_name_to_string(&a.name));
                        }
                        _ => {}
                    }
                }
                for m in &class_decl.members {
                    if let ClassMemberKind::GetAccessor(a) = &m.kind {
                        let n = self.prop_name_to_string(&a.name);
                        if !setter_names.contains(&n) {
                            readonly_members.insert(n);
                        }
                    }
                }
                readonly_members
            },
            accessor_props,
            extends_type_args,
            own_private_members,
            own_protected_members,
            own_private_static_members,
            instance_methods,
            overloaded_methods,
            static_properties,
            static_methods,
            static_member_modifiers,
            instance_member_modifiers,
            accessor_modifiers,
            overloaded_static_methods,
            is_abstract,
            extends,
            type_params,
            type_param_constraints,
            type_param_defaults,
            required_type_params,
        };
        self.exit_type_resolution_type_params();
        info
    }

    /// Insert a `ClassInfo` into the global map, preferring entries that carry
    /// an explicit constructor.
    ///
    /// `class_info` is keyed by the unqualified class name — two unrelated
    /// files declaring `class NextResponse` (e.g. a Next.js shim with no ctor
    /// vs the real `declare class NextResponse extends Response { constructor(...) }`)
    /// land at the same key. The original policy was first-write-wins
    /// (the `lib.rs:1325` site) or last-write-wins (sites at lines 2044/2392),
    /// which made constructor-arity checking order-dependent and produced
    /// TS2554 false positives whenever a no-ctor shim won the race against a
    /// real declaration.
    ///
    /// This helper makes the policy order-independent: a same-named entry is
    /// only replaced when the new one strictly improves on the existing one
    /// (it carries an explicit constructor and the existing entry does not).
    /// Ties fall back to first-wins.
    pub(crate) fn insert_class_info(&mut self, name: std::string::String, info: ClassInfo) {
        if let Some(existing) = self.class_info.get(name.as_str()) {
            if existing.has_explicit_constructor || !info.has_explicit_constructor {
                return;
            }
        }
        std::sync::Arc::make_mut(&mut self.class_info).insert(name, info);
    }

    pub(crate) fn build_interface_info(&self, iface_decl: &InterfaceDecl) -> InterfaceInfo {
        self.enter_type_resolution_type_params(
            iface_decl
                .type_params
                .iter()
                .flat_map(|params| params.iter().map(|param| &param.name)),
        );
        let mut props = Vec::new();
        let mut call_sigs = Vec::new();
        let mut construct_sigs = Vec::new();
        let mut index_sig = None;
        let mut index_sig_name: Option<std::string::String> = None;
        let mut optional_props = Vec::new();

        for member in &iface_decl.members {
            match &member.kind {
                TypeMemberKind::PropertySig(prop) => {
                    let name = self.prop_name_to_string(&prop.name);
                    let ty = prop
                        .type_ann
                        .as_ref()
                        .map(|t| {
                            if prop.readonly && Self::type_node_is_unique_symbol(t) {
                                let file = self
                                    .injected_decl_file
                                    .as_deref()
                                    .or(self.current_file_name.as_deref())
                                    .unwrap_or("");
                                Type::UniqueSymbol(format!(
                                    "{}.{name}\0{file}\0{}",
                                    iface_decl.name,
                                    prop.name.span().start
                                ))
                            } else {
                                self.resolve_type_node(t)
                            }
                        })
                        .unwrap_or(Type::Any);
                    if prop.optional {
                        optional_props.push(name.clone());
                    }
                    props.push((name, Arc::new(ty)));
                }
                TypeMemberKind::MethodSig(method) => {
                    let name = self.prop_name_to_string(&method.name);
                    let type_params: Vec<_> = method
                        .type_params
                        .as_ref()
                        .map(|params| params.iter().map(|param| param.name.clone()).collect())
                        .unwrap_or_default();
                    self.enter_type_resolution_type_params(type_params.iter());
                    let params: Vec<_> = method
                        .params
                        .iter()
                        .map(|p| {
                            let base_name = match &p.name.kind {
                                PatKind::Ident(n) => n.to_string(),
                                _ => "_".to_string(),
                            };
                            let pname = if p.dotdotdot {
                                format!("...{}", base_name)
                            } else if p.optional || p.initializer.is_some() {
                                format!("?{}", base_name)
                            } else {
                                base_name
                            };
                            let pty = p
                                .type_ann
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            (pname, self.declared_optional_param_type(p, pty))
                        })
                        .collect();
                    let ret = method
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .unwrap_or(Type::Any);
                    let ft = FunctionType {
                        type_param_constraints: method
                            .type_params
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .map(|p| p.constraint.as_ref().map(|t| self.resolve_type_node(t)))
                            .collect(),
                        params,
                        return_type: Arc::new(ret),
                        type_params,
                        type_param_defaults: method
                            .type_params
                            .as_deref()
                            .unwrap_or_default()
                            .iter()
                            .map(|p| p.default.as_ref().map(|t| self.resolve_type_node(t)))
                            .collect(),
                        type_predicate: None,
                    };
                    self.exit_type_resolution_type_params();
                    if method.optional {
                        optional_props.push(name.clone());
                    }
                    props.push((name, Arc::new(Type::Function(ft))));
                }
                TypeMemberKind::CallSig(cs) => {
                    let type_params: Vec<_> = cs
                        .type_params
                        .as_ref()
                        .map(|params| params.iter().map(|param| param.name.clone()).collect())
                        .unwrap_or_default();
                    self.enter_type_resolution_type_params(type_params.iter());
                    let params: Vec<_> = cs
                        .params
                        .iter()
                        .map(|p| {
                            let base_name = match &p.name.kind {
                                PatKind::Ident(n) => n.to_string(),
                                _ => "_".to_string(),
                            };
                            let pname = if p.dotdotdot {
                                format!("...{}", base_name)
                            } else if p.optional || p.initializer.is_some() {
                                format!("?{}", base_name)
                            } else {
                                base_name
                            };
                            let pty = p
                                .type_ann
                                .as_ref()
                                .map(|t| self.resolve_type_node(t))
                                .unwrap_or(Type::Any);
                            (pname, self.declared_optional_param_type(p, pty))
                        })
                        .collect();
                    let ret = cs
                        .return_type
                        .as_ref()
                        .map(|t| self.resolve_type_node(t))
                        .unwrap_or(Type::Void);
                    let ft = FunctionType {
                        type_param_constraints: Vec::new(),
                        params,
                        return_type: Arc::new(ret),
                        type_params,
                        type_param_defaults: Vec::new(),
                        type_predicate: None,
                    };
                    self.exit_type_resolution_type_params();
                    call_sigs.push(ft);
                }
                TypeMemberKind::ConstructSig(signature) => {
                    let type_params: Vec<_> = signature
                        .type_params
                        .as_ref()
                        .map(|params| params.iter().map(|param| param.name.clone()).collect())
                        .unwrap_or_default();
                    self.enter_type_resolution_type_params(type_params.iter());
                    let params = signature
                        .params
                        .iter()
                        .map(|parameter| {
                            let base_name = match &parameter.name.kind {
                                PatKind::Ident(name) => name.to_string(),
                                _ => "_".to_string(),
                            };
                            let name = if parameter.dotdotdot {
                                format!("...{base_name}")
                            } else if parameter.optional || parameter.initializer.is_some() {
                                format!("?{base_name}")
                            } else {
                                base_name
                            };
                            let ty = parameter
                                .type_ann
                                .as_ref()
                                .map(|annotation| self.resolve_type_node(annotation))
                                .unwrap_or(Type::Any);
                            (name, self.declared_optional_param_type(parameter, ty))
                        })
                        .collect();
                    let return_type = signature
                        .return_type
                        .as_ref()
                        .map(|annotation| self.resolve_type_node(annotation))
                        .unwrap_or(Type::Any);
                    let type_param_constraints = signature
                        .type_params
                        .as_ref()
                        .map(|params| {
                            params
                                .iter()
                                .map(|param| {
                                    param
                                        .constraint
                                        .as_ref()
                                        .map(|ty| self.resolve_type_node(ty))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let constructor = ConstructorType {
                        is_abstract: false,
                        params,
                        return_type: Arc::new(return_type),
                        type_param_defaults: self
                            .resolve_type_param_defaults(signature.type_params.as_deref()),
                        type_params,
                        type_param_constraints,
                    };
                    self.exit_type_resolution_type_params();
                    construct_sigs.push(constructor);
                }
                TypeMemberKind::IndexSig(is) => {
                    if let Some(first_param) = is.params.first() {
                        let key_ty = first_param
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or(Type::String);
                        let val_ty = is
                            .type_ann
                            .as_ref()
                            .map(|t| self.resolve_type_node(t))
                            .unwrap_or(Type::Any);
                        // On duplicate index signatures TypeScript keeps the
                        // FIRST declared one (and reports a duplicate error),
                        // so don't let a later duplicate overwrite it.
                        if index_sig.is_none() {
                            index_sig = Some((Arc::new(key_ty), Arc::new(val_ty)));
                            if let PatKind::Ident(name) = &first_param.name.kind {
                                index_sig_name = Some(name.to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let extends: Vec<(std::string::String, Arc<[Type]>)> = iface_decl
            .extends
            .iter()
            .map(|tn| {
                if let TypeNodeKind::Reference(ref type_ref) = tn.kind {
                    let name = self.namespace_owned_type_name(&self.expr_to_name(&type_ref.name));
                    let args: Arc<[Type]> = type_ref
                        .type_args
                        .as_ref()
                        .map(|args| args.iter().map(|a| self.resolve_type_node(a)).collect())
                        .unwrap_or_default();
                    (name, args)
                } else {
                    ("_".to_string(), Arc::from([] as [Type; 0]))
                }
            })
            .collect();
        let extends_sources = extends
            .iter()
            .map(|(name, _)| self.imported_type_sources.get(name).cloned())
            .collect();

        let type_params: Vec<std::string::String> = iface_decl
            .type_params
            .as_ref()
            .map(|tps| tps.iter().map(|tp| tp.name.clone()).collect())
            .unwrap_or_default();
        let required_type_params = iface_decl
            .type_params
            .as_ref()
            .map(|tps| tps.iter().filter(|tp| tp.default.is_none()).count())
            .unwrap_or(0);
        let type_param_constraints: Vec<Option<Type>> = iface_decl
            .type_params
            .as_ref()
            .map(|params| {
                params
                    .iter()
                    .map(|param| {
                        param
                            .constraint
                            .as_ref()
                            .map(|constraint| self.resolve_type_node(constraint))
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Merge duplicate-named function properties into a single
        // intersection-of-functions. TypeScript treats consecutive
        // same-name method signatures as overloads of one callable;
        // without this, only the first signature is consulted. Real
        // zod's `default(value)` / `default(thunk)` pair was the
        // motivating case — the value-form overload was getting hidden.
        let props = merge_overloaded_function_props(props);

        let info = InterfaceInfo {
            index_signatures: self.interface_index_signature_types(iface_decl),
            index_locations: self.interface_index_locations(iface_decl),
            declaration_span: iface_decl.span,
            decl_file: self
                .injected_decl_file
                .as_ref()
                .or(self.current_file_name.as_ref())
                .cloned()
                .unwrap_or_default(),
            is_global: !self.file_is_module_flag,
            object_type: ObjectTypeInfo {
                properties: props,
                call_signatures: call_sigs,
                construct_signatures: construct_sigs,
                index_signature: index_sig,
                index_signature_name: index_sig_name,
                method_names: iface_decl
                    .members
                    .iter()
                    .filter_map(|member| match &member.kind {
                        TypeMemberKind::MethodSig(method) => {
                            Some(self.prop_name_to_string(&method.name))
                        }
                        _ => None,
                    })
                    .collect(),
            },
            optional_props,
            member_locations: {
                let mut locations = rustc_hash::FxHashMap::default();
                let file = self
                    .injected_decl_file
                    .as_ref()
                    .or(self.current_file_name.as_ref())
                    .cloned()
                    .unwrap_or_default();
                for member in &iface_decl.members {
                    let name = match &member.kind {
                        TypeMemberKind::PropertySig(property) => &property.name,
                        TypeMemberKind::MethodSig(method) => &method.name,
                        TypeMemberKind::GetAccessorSig(accessor)
                        | TypeMemberKind::SetAccessorSig(accessor) => &accessor.name,
                        _ => continue,
                    };
                    locations
                        .entry(self.prop_name_to_string(name))
                        .or_insert((file.clone(), name.span()));
                }
                locations
            },
            member_names: iface_decl
                .members
                .iter()
                .filter_map(|member| {
                    let (name, method) = match &member.kind {
                        TypeMemberKind::PropertySig(property) => (&property.name, false),
                        TypeMemberKind::MethodSig(method) => (&method.name, true),
                        _ => return None,
                    };
                    let spelling = match name {
                        PropName::Number(value, _) => value.to_string(),
                        PropName::String(value, span) => self
                            .current_source
                            .as_ref()
                            .and_then(|source| source.get(span.start as usize..span.end as usize))
                            .filter(|text| text.starts_with(['\'', '"']))
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("'{value}'")),
                        _ => self.prop_name_to_string(name),
                    };
                    Some((self.prop_name_to_string(name), (spelling, method)))
                })
                .collect(),
            readonly_members: {
                let mut ro: rustc_hash::FxHashSet<std::string::String> =
                    rustc_hash::FxHashSet::default();
                let mut setters: rustc_hash::FxHashSet<std::string::String> =
                    rustc_hash::FxHashSet::default();
                for m in &iface_decl.members {
                    if let tsc_rs_ast::TypeMemberKind::SetAccessorSig(a) = &m.kind {
                        setters.insert(self.prop_name_to_string(&a.name));
                    }
                }
                for m in &iface_decl.members {
                    match &m.kind {
                        tsc_rs_ast::TypeMemberKind::PropertySig(p) if p.readonly => {
                            ro.insert(self.prop_name_to_string(&p.name));
                        }
                        tsc_rs_ast::TypeMemberKind::GetAccessorSig(a) => {
                            let n = self.prop_name_to_string(&a.name);
                            if !setters.contains(&n) {
                                ro.insert(n);
                            }
                        }
                        _ => {}
                    }
                }
                ro
            },
            extends,
            extends_sources,
            type_params,
            type_param_constraints,
            required_type_params,
        };
        self.exit_type_resolution_type_params();
        info
    }

    /// Get all required properties for an interface, including inherited ones.
    pub(crate) fn get_interface_required_properties(
        &self,
        iface_name: &str,
    ) -> Vec<(std::string::String, Type)> {
        let mut seen = HashSet::new();
        self.get_interface_required_properties_inner(iface_name, &mut seen)
    }

    fn get_interface_required_properties_inner(
        &self,
        iface_name: &str,
        seen: &mut HashSet<std::string::String>,
    ) -> Vec<(std::string::String, Type)> {
        if !seen.insert(iface_name.to_string()) {
            return Vec::new(); // cycle detected
        }

        let info = match self.interface_info.get(iface_name) {
            Some(info) => info.clone(),
            None => return Vec::new(),
        };

        // tsc resolveObjectTypeMembers: own members first (declaration
        // order), then inherited members not redeclared (`missing the
        // following properties from type 'I': y, x` for `I extends Foo`).
        let mut all_props: Vec<(std::string::String, Type)> = Vec::new();
        for (name, ty) in &info.object_type.properties {
            if info.optional_props.contains(name) {
                continue;
            }
            if !all_props.iter().any(|(n, _)| n == name) {
                all_props.push((name.clone(), Type::clone(&ty)));
            }
        }

        for (index, (base_name, _base_args)) in info.extends.iter().enumerate() {
            let lookup_name = self.interface_lookup_name_for_source(
                base_name,
                info.extends_sources
                    .get(index)
                    .and_then(|source| source.as_deref()),
            );
            let base_props = if !self.interface_info.contains_key(lookup_name.as_str())
                && self.class_info.contains_key(lookup_name.as_str())
            {
                // An interface extending a class inherits all its instance
                // members, private and protected ones included.
                self.class_required_instance_members(&lookup_name)
            } else {
                self.get_interface_required_properties_inner(&lookup_name, seen)
            };
            for (name, ty) in base_props {
                if !all_props.iter().any(|(n, _)| n == &name) {
                    all_props.push((name, ty));
                }
            }
        }

        all_props
    }

    /// Required instance members of a class and its base classes, own members
    /// first.
    fn class_required_instance_members(
        &self,
        class_name: &str,
    ) -> Vec<(std::string::String, Type)> {
        let mut out: Vec<(std::string::String, Type)> = Vec::new();
        let mut current = class_name.to_string();
        for _ in 0..8 {
            let Some(info) = self.class_info.get(current.as_str()) else {
                break;
            };
            for (name, ty) in &info.instance_properties {
                if info.optional_instance_properties.contains(name)
                    || matches!(ty, Type::Optional(_))
                    || out.iter().any(|(n, _)| n == name)
                {
                    continue;
                }
                out.push((name.clone(), ty.clone()));
            }
            for (name, function) in &info.instance_methods {
                if !out.iter().any(|(n, _)| n == name) {
                    out.push((name.clone(), Type::Function(function.clone())));
                }
            }
            match info.extends.clone() {
                Some(base) => current = base,
                None => break,
            }
        }
        out
    }

    /// Check that a class correctly implements all of its `implements` interfaces.
    /// Span of the class member declaring `name`, for member-anchored
    /// diagnostics (tsc anchors TS2416 at the member, not the class head).
    fn class_member_spans<'a>(
        class_decl: &'a ClassDecl,
        name: &'a str,
    ) -> impl Iterator<Item = Span> + 'a {
        class_decl.members.iter().filter_map(move |member| {
            let (prop, modifiers) = match &member.kind {
                ClassMemberKind::Property(p) => (&p.name, p.modifiers),
                ClassMemberKind::Method(m) => (&m.name, m.modifiers),
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    (&a.name, a.modifiers)
                }
                _ => return None,
            };
            (modifiers & MOD_STATIC == 0 && Self::propname_text_opt(prop).as_deref() == Some(name))
                .then(|| prop.span())
        })
    }

    fn class_member_span(class_decl: &ClassDecl, name: &str) -> Option<Span> {
        Self::class_member_spans(class_decl, name).next()
    }

    fn push_instance_member_diagnostic(
        &mut self,
        class: &ClassDecl,
        name: &str,
        diagnostic: Diagnostic,
    ) {
        let mut spans: Vec<Span> = Self::class_member_spans(class, name).collect();
        if spans.is_empty() {
            // Computed names (`[Symbol.toPrimitive]`) match by their
            // rendered property name.
            spans = class
                .members
                .iter()
                .filter_map(|member| {
                    let (prop, modifiers) = match &member.kind {
                        ClassMemberKind::Property(p) => (&p.name, p.modifiers),
                        ClassMemberKind::Method(m) => (&m.name, m.modifiers),
                        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                            (&a.name, a.modifiers)
                        }
                        _ => return None,
                    };
                    (modifiers & MOD_STATIC == 0
                        && matches!(prop, tsc_rs_ast::PropName::Computed(..))
                        && self.prop_name_to_string(prop) == name)
                        .then(|| prop.span())
                })
                .collect();
        }
        let mut spans = spans.into_iter().peekable();
        if spans.peek().is_none() {
            self.diagnostics.push(diagnostic);
        } else {
            for span in spans {
                let mut diagnostic = diagnostic.clone();
                diagnostic.span = Some(span);
                self.diagnostics.push(diagnostic);
            }
        }
    }

    pub(crate) fn propname_text_opt(n: &tsc_rs_ast::PropName) -> Option<std::string::String> {
        match n {
            tsc_rs_ast::PropName::Ident(s, _) => Some(s.to_string()),
            tsc_rs_ast::PropName::String(s, _) => Some(s.to_string()),
            _ => None,
        }
    }

    /// Keep class parameters opaque while comparing their declared members.
    /// Both sides use the same map; method-local parameters shadow it.
    fn heritage_type_parameters(&self, class: &ClassDecl) -> HashMap<String, Type> {
        let params = class.type_params.as_deref().unwrap_or_default();
        let map: HashMap<_, _> = params
            .iter()
            .map(|param| {
                let constraint = param
                    .constraint
                    .as_ref()
                    .map(|node| self.resolve_type_node(node));
                (
                    param.name.clone(),
                    Self::opaque_type_param(&param.name, constraint.as_ref()),
                )
            })
            .collect();
        for param in params {
            if let Some(constraint) = &param.constraint {
                Self::register_opaque_type_param_constraint(
                    &map[&param.name],
                    Self::substitute(&self.resolve_type_node(constraint), &map),
                );
            }
        }
        map
    }

    /// Static members conflict with the constructor's built-in properties.
    /// Ambient classes have no runtime members, and define semantics permit
    /// all of these names except `prototype`.
    pub(crate) fn check_static_property_name_conflicts(
        &mut self,
        class: &ClassDecl,
        inferred_name: Option<&str>,
    ) {
        let ambient = self.ambient_depth > 0
            || self.current_file_is_declaration()
            || class.modifiers & MOD_DECLARE != 0;
        let target = self.compiler_options.target.unwrap_or(ScriptTarget::ES2025);
        let define_fields = self
            .compiler_options
            .use_define_for_class_fields
            .unwrap_or(target >= ScriptTarget::ES2022);
        let class_name = class.name.as_deref().or(inferred_name).unwrap_or(
            if class.modifiers & MOD_DEFAULT != 0 {
                "default"
            } else {
                "(Anonymous class)"
            },
        );
        for member in &class.members {
            let name = match &member.kind {
                ClassMemberKind::Property(property) if property.modifiers & MOD_STATIC != 0 => {
                    &property.name
                }
                ClassMemberKind::Method(method) if method.modifiers & MOD_STATIC != 0 => {
                    &method.name
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor)
                    if accessor.modifiers & MOD_STATIC != 0 =>
                {
                    &accessor.name
                }
                _ => continue,
            };
            let Some(property) = self.pattern_property_name(name) else {
                continue;
            };
            let span = name.span();
            // Every class already declares a `prototype` property. Methods
            // and accessors cannot merge with that symbol, even in ambient
            // classes, while an ordinary property declaration can.
            let conflicts_with_prototype = property == "prototype"
                && match &member.kind {
                    ClassMemberKind::Property(property) => property.modifiers & MOD_ACCESSOR != 0,
                    _ => true,
                };
            if conflicts_with_prototype
                && !self
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == 2300 && diagnostic.span == Some(span))
            {
                let display = self
                    .current_source
                    .as_deref()
                    .and_then(|source| source.get(span.start as usize..span.end as usize))
                    .unwrap_or(&property);
                self.diagnostics
                    .push(error_duplicate_identifier(display, span));
            }
            if ambient {
                continue;
            }
            if property != "prototype"
                && (define_fields
                    || !matches!(
                        property.as_str(),
                        "name" | "length" | "caller" | "arguments"
                    ))
            {
                continue;
            }
            if !self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == 2699 && diagnostic.span == Some(span))
            {
                self.diagnostics.push(Diagnostic {
                    code: 2699,
                    message: format!("Static property '{property}' conflicts with built-in property 'Function.{property}' of constructor function '{class_name}'."),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(span),
                    related: None,
                });
            }
        }
    }

    fn instance_class_display(class: &ClassDecl) -> String {
        Type::TypeReference(
            class.name.as_deref().unwrap_or_default().to_string(),
            class
                .type_params
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|p| Type::TypeParameter(p.name.clone()))
                .collect::<Vec<_>>()
                .into(),
        )
        .display_string_single_line()
    }

    /// TS2416 for `extends` overrides: derived members must be assignable to
    /// the base member of the same name.
    /// Returns true when the instance side failed (a member TS2416 or the
    /// whole-class TS2415): tsc then skips the static-side TS2417 check.
    pub(crate) fn check_extends_overrides(&mut self, class_decl: &ClassDecl) -> bool {
        self.check_class_overload_sequences(class_decl);

        let class_name = match class_decl.name {
            Some(ref n) => n.clone(),
            None => return false,
        };
        let Some(ref extends) = class_decl.extends else {
            return false;
        };
        let ExprKind::Ident(ref base_name) = extends.kind else {
            return false;
        };
        let substitutions = self.heritage_type_parameters(class_decl);
        let class_display = Self::instance_class_display(class_decl);
        let base_args: Vec<_> = class_decl
            .extends_type_args
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|t| Self::substitute(&self.resolve_type_node(t), &substitutions))
            .collect();
        let (base_display, base_ty) = if self.class_info.contains_key(base_name.as_str()) {
            (
                Type::TypeReference(base_name.to_string(), base_args.clone().into())
                    .display_string_single_line(),
                self.resolve_type_reference_to_object(base_name, &base_args)
                    .unwrap_or_else(|| self.get_class_instance_type(base_name)),
            )
        } else {
            // `declare var A: { new(): A }`: the base instance type is the
            // non-generic construct signature's return type.
            let instance = match self.lookup_var(base_name) {
                Some(Type::ObjectType(info)) if base_args.is_empty() => {
                    match info.construct_signatures.as_slice() {
                        [signature] if signature.type_params.is_empty() => {
                            Some(signature.return_type.as_ref().clone())
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some(Type::TypeReference(instance_name, instance_args)) = instance else {
                return false;
            };
            if !instance_args.is_empty()
                || !self.interface_info.contains_key(instance_name.as_str())
            {
                return false;
            }
            let Some(resolved) = self.resolve_type_reference_to_object(&instance_name, &[]) else {
                return false;
            };
            (instance_name, resolved)
        };
        let base_methods = match &base_ty {
            Type::ObjectType(info) => info.method_names.clone(),
            _ => Vec::new(),
        };
        let base_props = match &base_ty {
            Type::ObjectType(info) => info.properties.clone(),
            _ => return false,
        };
        let mut class_instance_ty = self.get_class_instance_type(&class_name);
        if !substitutions.is_empty() {
            class_instance_ty = Self::substitute(&class_instance_ty, &substitutions);
        }
        let class_props = match &class_instance_ty {
            Type::ObjectType(info) => info.properties.clone(),
            _ => return false,
        };
        let mut issued_member_error = false;
        for (prop_name, derived_ty) in &class_props {
            // Only members redeclared in THIS class body are checked.
            let Some(member_span) = Self::class_member_span(class_decl, prop_name) else {
                continue;
            };
            let Some((_, base_prop_ty)) = base_props.iter().find(|(n, _)| n == prop_name) else {
                continue;
            };
            if !self.member_type_assignable(
                derived_ty,
                base_prop_ty,
                base_methods.contains(prop_name),
            ) {
                let mut diagnostic = error_property_not_assignable_to_base(
                    prop_name,
                    &class_display,
                    &base_display,
                    member_span,
                );
                if let Some(line) = self.member_type_elaboration(
                    derived_ty,
                    base_prop_ty,
                    base_methods.contains(prop_name),
                ) {
                    diagnostic.message.push_str("\n  ");
                    diagnostic.message.push_str(&line.replace('\n', "\n  "));
                    if diagnostic.related.is_none() {
                        diagnostic.related = self.last_leaf_related.take().map(|note| vec![note]);
                    }
                }
                self.push_instance_member_diagnostic(class_decl, prop_name, diagnostic);
                issued_member_error = true;
            }
        }
        // TS2415: with no member-specific error, tsc falls back to the whole
        // instance-type relation, whose first failure for redeclared private/
        // protected members is a visibility conflict.
        if !issued_member_error {
            if let Some(elaboration) =
                self.member_visibility_conflict(class_decl, &class_name, base_name)
            {
                self.diagnostics.push(error_class_incorrectly_extends(
                    &class_name,
                    base_name,
                    &elaboration,
                    class_decl.name_span.unwrap_or(class_decl.span),
                ));
                return true;
            }
        }
        issued_member_error
    }

    /// First private/protected conflict between a member redeclared in
    /// `class_decl` and the nearest ancestor declaring the same instance
    /// member, worded as tsc's propertyRelatedTo elaborations.
    fn member_visibility_conflict(
        &self,
        class_decl: &ClassDecl,
        class_name: &str,
        base_name: &str,
    ) -> Option<std::string::String> {
        #[derive(Clone, Copy, PartialEq)]
        enum Visibility {
            Private,
            Protected,
            Public,
        }
        let own = self.class_info.get(class_name)?;
        let visibility_in = |info: &ClassInfo, name: &str| -> Option<Visibility> {
            if info.own_private_members.contains(name) {
                Some(Visibility::Private)
            } else if info.own_protected_members.contains(name) {
                Some(Visibility::Protected)
            } else if info.instance_properties.iter().any(|(n, _)| n == name)
                || info.instance_methods.iter().any(|(n, _)| n == name)
            {
                Some(Visibility::Public)
            } else {
                None
            }
        };
        let mut own_members: Vec<std::string::String> = Vec::new();
        for member in &class_decl.members {
            let name = match &member.kind {
                ClassMemberKind::Property(p) if p.modifiers & MOD_STATIC == 0 => {
                    Self::propname_text_opt(&p.name)
                }
                ClassMemberKind::Method(m) if m.modifiers & MOD_STATIC == 0 => {
                    Self::propname_text_opt(&m.name)
                }
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a)
                    if a.modifiers & MOD_STATIC == 0 =>
                {
                    Self::propname_text_opt(&a.name)
                }
                ClassMemberKind::Constructor(c) => {
                    for param in &c.params {
                        if param.modifiers
                            & (tsc_rs_ast::MOD_PUBLIC
                                | tsc_rs_ast::MOD_PRIVATE
                                | tsc_rs_ast::MOD_PROTECTED
                                | tsc_rs_ast::MOD_READONLY)
                            != 0
                        {
                            if let tsc_rs_ast::PatKind::Ident(n) = &param.name.kind {
                                own_members.push(n.to_string());
                            }
                        }
                    }
                    None
                }
                _ => None,
            };
            if let Some(name) = name {
                if !name.starts_with('#') && !own_members.contains(&name) {
                    own_members.push(name);
                }
            }
        }
        for name in &own_members {
            let Some(derived) = visibility_in(own, name) else {
                continue;
            };
            // Nearest ancestor declaring the member.
            let mut ancestor = Some(base_name.to_string());
            let mut seen = 0usize;
            let mut found: Option<(std::string::String, Visibility)> = None;
            while let Some(current) = ancestor.take() {
                seen += 1;
                if seen > 64 {
                    break;
                }
                let Some(info) = self.class_info.get(current.as_str()) else {
                    break;
                };
                if let Some(visibility) = visibility_in(info, name) {
                    found = Some((current, visibility));
                    break;
                }
                ancestor = info.extends.clone();
            }
            let Some((declaring, base)) = found else {
                continue;
            };
            match (derived, base) {
                (Visibility::Private, Visibility::Private) => {
                    return Some(format!(
                        "Types have separate declarations of a private property '{name}'."
                    ));
                }
                (Visibility::Private, _) => {
                    return Some(format!(
                        "Property '{name}' is private in type '{class_name}' but not in type '{declaring}'."
                    ));
                }
                (_, Visibility::Private) => {
                    return Some(format!(
                        "Property '{name}' is private in type '{declaring}' but not in type '{class_name}'."
                    ));
                }
                (Visibility::Protected, Visibility::Public) => {
                    return Some(format!(
                        "Property '{name}' is protected in type '{class_name}' but public in type '{declaring}'."
                    ));
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn check_implements_clause(&mut self, class_decl: &ClassDecl) {
        let class_display_name = match class_decl.name {
            Some(ref n) => n.clone(),
            None => return,
        };
        let class_name = self.namespace_owned_type_name(&class_display_name);

        // Skip implements check if class extends an unresolvable base class
        // (the base class may provide the missing interface members)
        if let Some(ref extends) = class_decl.extends {
            if let ExprKind::Ident(ref base_name) = extends.kind {
                let base_name = self.namespace_owned_type_name(base_name);
                if !self.class_info.contains_key(base_name.as_str()) {
                    return; // Unresolvable base — can't verify interface
                }
            }
        }

        let substitutions = self.heritage_type_parameters(class_decl);
        let member_class_display = Self::instance_class_display(class_decl);
        let mut class_instance_ty = self.get_class_instance_type(&class_name);
        if !substitutions.is_empty() {
            class_instance_ty = Self::substitute(&class_instance_ty, &substitutions);
        }
        let class_props = match &class_instance_ty {
            Type::ObjectType(info) => info.properties.clone(),
            _ => return,
        };

        for iface_node in &class_decl.implements {
            // TS2864: primitives cannot be implemented.
            if let TypeNodeKind::Keyword(kw) = &iface_node.kind {
                let prim = match kw {
                    tsc_rs_ast::KeywordTypeKind::Number => Some("number"),
                    tsc_rs_ast::KeywordTypeKind::String => Some("string"),
                    tsc_rs_ast::KeywordTypeKind::Boolean => Some("boolean"),
                    tsc_rs_ast::KeywordTypeKind::BigInt => Some("bigint"),
                    tsc_rs_ast::KeywordTypeKind::Symbol => Some("symbol"),
                    _ => None,
                };
                if let Some(prim) = prim {
                    self.diagnostics
                        .push(crate::diagnostics::error_class_implements_primitive(
                            prim,
                            iface_node.span,
                        ));
                    continue;
                }
            }
            let iface_display_name = match &iface_node.kind {
                TypeNodeKind::Reference(type_ref) => self.expr_to_name(&type_ref.name),
                _ => continue,
            };
            let iface_name = self.namespace_owned_type_name(&iface_display_name);

            // TS2304: the implements target names nothing at all.
            if !iface_name.contains('.')
                && !self.class_info.contains_key(iface_name.as_str())
                && !self.interface_info.contains_key(iface_name.as_str())
                && !self.type_aliases.contains_key(iface_name.as_str())
                && !self.enum_info.contains_key(iface_name.as_str())
                && !Self::name_shadows_js_builtin(&iface_name)
                && self.lookup_var(&iface_name).is_none()
                // `import T = M1.I` aliases resolve through the registry.
                && !self.import_equals_names.contains(iface_name.as_str())
                && !self.imported_namespace_bindings.contains(iface_name.as_str())
                && !self.type_name_is_known(&iface_name)
            {
                self.diagnostics
                    .push(crate::diagnostics::error_cannot_find_name(
                        &iface_name,
                        iface_node.span,
                    ));
                continue;
            }

            // TS2720: implementing a CLASS (not an interface) whose members
            // the implementor lacks — tsc suggests `extends` instead.
            if self.class_info.contains_key(iface_name.as_str())
                && !self.interface_info.contains_key(iface_name.as_str())
            {
                let base_inst = self.get_class_instance_type(&iface_name);
                if let Type::ObjectType(bi) = &base_inst {
                    let missing = bi.properties.iter().any(|(n, _)| {
                        !n.ends_with('?')
                            && !class_props
                                .iter()
                                .any(|(cn, _)| cn.trim_end_matches('?') == n.trim_end_matches('?'))
                    });
                    if missing {
                        let err_span = class_decl.name_span.unwrap_or(class_decl.span);
                        self.diagnostics.push(
                            crate::diagnostics::error_class_incorrectly_implements_class(
                                &class_display_name,
                                &iface_display_name,
                                err_span,
                            ),
                        );
                        continue;
                    }
                }
            }

            // Get required properties from the interface
            let iface_args: Vec<_> = match &iface_node.kind {
                TypeNodeKind::Reference(reference) => reference
                    .type_args
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|t| Self::substitute(&self.resolve_type_node(t), &substitutions))
                    .collect(),
                _ => Vec::new(),
            };
            let iface_display_name =
                if let Some((params, body, _)) = self.type_aliases.get(&iface_name) {
                    let map: HashMap<_, _> = params
                        .iter()
                        .cloned()
                        .zip(iface_args.iter().cloned())
                        .collect();
                    Self::substitute(body, &map).display_string_single_line()
                } else {
                    Type::TypeReference(iface_display_name, iface_args.clone().into())
                        .display_string_single_line()
                };
            let mut required_props = Vec::new();
            let mut iface_methods = Vec::new();
            self.collect_heritage_members(
                &Type::TypeReference(iface_name.clone(), iface_args.into()),
                &mut required_props,
                &mut iface_methods,
                &mut HashSet::new(),
            );

            // Use name_span (class name position) if available, otherwise class span
            let err_span = class_decl.name_span.unwrap_or(class_decl.span);
            // Members the interface inherits from CLASS bases (`interface I
            // extends C`), private and protected ones included: they are part
            // of the structural type but carry no checkable type here.
            let class_base_members: Vec<std::string::String> = self
                .interface_class_base_members(&iface_name)
                .into_iter()
                .filter(|name| !required_props.iter().any(|(n, _)| n == name))
                .collect();
            // Member-level TS2416 for present-but-incompatible members; tsc
            // then skips the whole-class error.
            let mut issued_member_error = false;
            let mut missing: Vec<std::string::String> = Vec::new();
            let iface_class_base_props: Vec<(std::string::String, Type)> = self
                .interface_info
                .get(iface_name.as_str())
                .map(|info| {
                    info.extends
                        .iter()
                        .filter(|(base, _)| self.class_info.contains_key(base.as_str()))
                        .flat_map(|(base, _)| match self.get_class_instance_type(base) {
                            Type::ObjectType(base_info) => base_info
                                .properties
                                .iter()
                                .map(|(n, t)| (n.clone(), (**t).clone()))
                                .collect::<Vec<_>>(),
                            _ => Vec::new(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            // Inherited class-base members come after the interface's own
            // (tsc member order): collected apart, appended below.
            let mut base_missing: Vec<std::string::String> = Vec::new();
            for name in &class_base_members {
                match class_props.iter().find(|(n, _)| n == name) {
                    None => base_missing.push(name.clone()),
                    Some((_, class_ty)) => {
                        if let Some((_, base_ty)) =
                            iface_class_base_props.iter().find(|(n, _)| n == name)
                        {
                            if !self.member_type_assignable(class_ty, base_ty, true) {
                                let anchor =
                                    Self::class_member_span(class_decl, name).unwrap_or(err_span);
                                let mut diagnostic = error_property_not_assignable_to_base(
                                    name,
                                    &member_class_display,
                                    &iface_display_name,
                                    anchor,
                                );
                                if let Some(line) =
                                    self.member_type_elaboration(class_ty, base_ty, true)
                                {
                                    diagnostic.message.push_str("\n  ");
                                    diagnostic.message.push_str(&line.replace('\n', "\n  "));
                                    if diagnostic.related.is_none() {
                                        diagnostic.related =
                                            self.last_leaf_related.take().map(|note| vec![note]);
                                    }
                                }
                                self.push_instance_member_diagnostic(class_decl, name, diagnostic);
                                issued_member_error = true;
                            }
                        }
                    }
                }
            }
            // `implements Array<T>`: the built-in members, in lib order.
            if iface_name == "Array" && required_props.is_empty() {
                for member in ARRAY_INTERFACE_MEMBERS {
                    if !class_props.iter().any(|(n, _)| n == member) {
                        missing.push(member.to_string());
                    }
                }
            }
            for (prop_name, iface_ty) in &required_props {
                if let Some((_, class_ty)) = class_props.iter().find(|(n, _)| n == prop_name) {
                    if !self.member_type_assignable(
                        class_ty,
                        iface_ty,
                        iface_methods.contains(prop_name),
                    ) {
                        let anchor =
                            Self::class_member_span(class_decl, prop_name).unwrap_or(err_span);
                        let mut diagnostic = error_property_not_assignable_to_base(
                            prop_name,
                            &member_class_display,
                            &iface_display_name,
                            anchor,
                        );
                        if let Some(line) = self.member_type_elaboration(
                            class_ty,
                            iface_ty,
                            iface_methods.contains(prop_name),
                        ) {
                            diagnostic.message.push_str("\n  ");
                            diagnostic.message.push_str(&line.replace('\n', "\n  "));
                            if diagnostic.related.is_none() {
                                diagnostic.related =
                                    self.last_leaf_related.take().map(|note| vec![note]);
                            }
                        }
                        self.push_instance_member_diagnostic(class_decl, prop_name, diagnostic);
                        issued_member_error = true;
                    }
                } else if !matches!(iface_ty, Type::Optional(_)) {
                    missing.push(prop_name.clone());
                }
            }
            missing.extend(base_missing);
            if issued_member_error {
                continue;
            }
            // TS2559 (tsc isWeakType): every interface member is optional and
            // the class shares none of them.
            let strip = |name: &str| name.trim_end_matches('?').to_string();
            let weak_interface = missing.is_empty()
                && !required_props.is_empty()
                && class_base_members.is_empty()
                && required_props
                    .iter()
                    .all(|(_, ty)| matches!(ty, Type::Optional(_)))
                && self
                    .interface_info
                    .get(iface_name.as_str())
                    .is_some_and(|info| {
                        info.index_signatures.is_empty()
                            && info.object_type.index_signature.is_none()
                            && info.object_type.call_signatures.is_empty()
                            && info.object_type.construct_signatures.is_empty()
                    });
            if weak_interface
                && !class_props.is_empty()
                && !class_props.iter().any(|(class_name, _)| {
                    required_props
                        .iter()
                        .any(|(iface_prop, _)| strip(iface_prop) == strip(class_name))
                })
            {
                self.diagnostics
                    .push(crate::diagnostics::error_no_properties_in_common(
                        &class_display_name,
                        &iface_display_name,
                        err_span,
                    ));
                continue;
            }
            // Whole-class TS2420 with tsc's first structural failure.
            let iface_display = self
                .resolve_type_node(iface_node)
                .display_string_single_line();
            let class_display = match &class_decl.type_params {
                Some(params) if !params.is_empty() => format!(
                    "{}<{}>",
                    class_display_name,
                    params
                        .iter()
                        .map(|p| p.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => class_display_name.clone(),
            };
            let missing_class_display = self
                .memberless_class_base_display(&Type::TypeReference(
                    class_name.clone(),
                    Arc::from(Vec::<Type>::new()),
                ))
                .unwrap_or_else(|| class_display.clone());
            let mut related: Option<RelatedDiagnostic> = None;
            let elaboration = if missing.len() == 1 {
                let missing_written = self
                    .heritage_member_metadata(
                        &Type::TypeReference(iface_name.clone(), Arc::from(Vec::<Type>::new())),
                        &missing[0],
                        &mut HashSet::new(),
                    )
                    .map(|(spelling, _)| spelling)
                    .unwrap_or_else(|| missing[0].clone());
                related = self
                    .interface_info
                    .get(iface_name.as_str())
                    .and_then(|info| {
                        info.member_locations.get(&missing[0]).cloned().or_else(|| {
                            // Members inherited from a class base (`interface
                            // I extends Foo`) are declared in that class.
                            info.extends.iter().find_map(|(base, _)| {
                                self.class_info
                                    .get(base.as_str())
                                    .and_then(|class| class.member_locations.get(&missing[0]))
                                    .cloned()
                            })
                        })
                    })
                    .map(|(file, span)| RelatedDiagnostic {
                        code: 2728,
                        message: format!("'{}' is declared here.", missing_written),
                        file_name: Some(file),
                        span: Some(span),
                    });
                Some(format!(
                    "Property '{}' is missing in type '{}' but required in type '{}'.",
                    missing_written, missing_class_display, iface_display
                ))
            } else if missing.len() > 1 {
                Some(
                    crate::diagnostics::error_missing_properties(
                        &missing,
                        &missing_class_display,
                        &iface_display,
                        err_span,
                    )
                    .message,
                )
            } else {
                let mut names: Vec<(std::string::String, Type)> = required_props.clone();
                names.extend(
                    class_base_members
                        .iter()
                        .map(|name| (name.clone(), Type::Any)),
                );
                self.implements_visibility_or_index_failure(
                    &class_name,
                    &class_display,
                    &iface_name,
                    &iface_display,
                    &names,
                )
            };
            if let Some(elaboration) = elaboration {
                self.diagnostics.push(Diagnostic {
                    code: 2420,
                    message: format!(
                        "Class '{}' incorrectly implements interface '{}'.\n  {}",
                        class_display, iface_display, elaboration
                    ),
                    category: DiagnosticCategory::Error,
                    file_name: None,
                    span: Some(err_span),
                    related: related.map(|note| vec![note]),
                });
            }
        }
    }

    fn member_type_elaboration(
        &mut self,
        source: &Type,
        target: &Type,
        method: bool,
    ) -> Option<String> {
        let source = self.optional_property_type(source.clone());
        let target = self.optional_property_type(target.clone());
        let target = Self::strip_nullable_union_target(&source, &target).unwrap_or(target);
        let old_mode = self.current_elaboration_mode.swap(
            if method { RELATION_TARGET_METHOD } else { 0 },
            Ordering::Relaxed,
        );
        self.last_leaf_related = None;
        let message = self.leaf_reason_text(&source, &target);
        self.current_elaboration_mode
            .store(old_mode, Ordering::Relaxed);
        Some(message)
    }

    fn collect_heritage_members(
        &self,
        ty: &Type,
        props: &mut Vec<(String, Type)>,
        methods: &mut Vec<String>,
        seen: &mut HashSet<String>,
    ) {
        match ty {
            Type::TypeReference(name, args) if seen.insert(name.clone()) => {
                if let Some((params, body, _)) = self.type_aliases.get(name) {
                    let map: HashMap<_, _> =
                        params.iter().cloned().zip(args.iter().cloned()).collect();
                    self.collect_heritage_members(
                        &Self::substitute(body, &map),
                        props,
                        methods,
                        seen,
                    );
                } else if let Some(resolved) = self.resolve_type_reference_to_object(name, args) {
                    self.collect_heritage_members(&resolved, props, methods, seen);
                }
                seen.remove(name);
            }
            Type::Intersection(parts) => {
                for part in parts.iter() {
                    self.collect_heritage_members(part, props, methods, seen);
                }
            }
            Type::ObjectType(info) => {
                props.extend(
                    info.properties
                        .iter()
                        .map(|(n, t)| (n.clone(), t.as_ref().clone())),
                );
                methods.extend(info.method_names.iter().cloned());
            }
            _ => {}
        }
    }

    /// Instance member names an interface inherits from class bases
    /// (`interface I extends C`), walking each class's extends chain;
    /// private and protected members included.
    fn interface_class_base_members(&self, iface_name: &str) -> Vec<std::string::String> {
        let mut out: Vec<std::string::String> = Vec::new();
        let Some(info) = self.interface_info.get(iface_name) else {
            return out;
        };
        for (base, _) in &info.extends {
            let mut current = Some(base.clone());
            let mut hops = 0usize;
            while let Some(class) = current.take() {
                hops += 1;
                if hops > 64 {
                    break;
                }
                let Some(class_info) = self.class_info.get(class.as_str()) else {
                    break;
                };
                for (name, _) in &class_info.instance_properties {
                    if !out.contains(name) {
                        out.push(name.clone());
                    }
                }
                for (name, _) in &class_info.instance_methods {
                    if !out.contains(name) {
                        out.push(name.clone());
                    }
                }
                for name in class_info
                    .own_private_members
                    .iter()
                    .chain(class_info.own_protected_members.iter())
                {
                    if !out.contains(name) {
                        out.push(name.clone());
                    }
                }
                current = class_info.extends.clone();
            }
        }
        out
    }

    /// Nearest class in `class_name`'s chain that declares `member`, with
    /// its visibility (private / protected / public).
    fn member_declaring_visibility(
        &self,
        class_name: &str,
        member: &str,
    ) -> Option<(std::string::String, u8)> {
        const PRIVATE: u8 = 1;
        const PROTECTED: u8 = 2;
        const PUBLIC: u8 = 0;
        let mut current = Some(class_name.to_string());
        let mut hops = 0usize;
        while let Some(class) = current.take() {
            hops += 1;
            if hops > 64 {
                return None;
            }
            let info = self.class_info.get(class.as_str())?;
            if info.own_private_members.contains(member) {
                return Some((class, PRIVATE));
            }
            if info.own_protected_members.contains(member) {
                return Some((class, PROTECTED));
            }
            if info.instance_properties.iter().any(|(n, _)| n == member)
                || info.instance_methods.iter().any(|(n, _)| n == member)
                || info.accessor_props.contains(member)
            {
                return Some((class, PUBLIC));
            }
            current = info.extends.clone();
        }
        None
    }

    /// Elaboration for a class that has every required member of `iface`
    /// with compatible types but still fails structurally: a member that is
    /// private/protected on one side, or a missing index signature.
    fn implements_visibility_or_index_failure(
        &self,
        class_name: &str,
        class_display: &str,
        iface_name: &str,
        iface_display: &str,
        required_props: &[(std::string::String, Type)],
    ) -> Option<std::string::String> {
        // Interface members inherited from a class with private/protected
        // declarations (`interface I extends C`).
        let iface_class_bases: Vec<std::string::String> = self
            .interface_info
            .get(iface_name)
            .map(|info| {
                info.extends
                    .iter()
                    .map(|(name, _)| name.clone())
                    .filter(|name| self.class_info.contains_key(name.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        for (prop_name, _) in required_props {
            let class_side = self.member_declaring_visibility(class_name, prop_name);
            let iface_side = iface_class_bases
                .iter()
                .find_map(|base| self.member_declaring_visibility(base, prop_name))
                .filter(|(_, visibility)| *visibility != 0);
            match (class_side, iface_side) {
                (Some((declaring, 1)), Some((iface_declaring, 1))) => {
                    if declaring != iface_declaring {
                        return Some(format!(
                            "Types have separate declarations of a private property '{}'.",
                            prop_name
                        ));
                    }
                }
                (Some((declaring, 1)), _) => {
                    return Some(format!(
                        "Property '{}' is private in type '{}' but not in type '{}'.",
                        prop_name, declaring, iface_display
                    ));
                }
                (Some((declaring, 2)), None) => {
                    return Some(format!(
                        "Property '{}' is protected in type '{}' but public in type '{}'.",
                        prop_name, declaring, iface_display
                    ));
                }
                // A public class member against a private/protected one the
                // interface inherits from a class base.
                (Some((_, 0)), Some((_, 1))) => {
                    return Some(format!(
                        "Property '{}' is private in type '{}' but not in type '{}'.",
                        prop_name, iface_display, class_display
                    ));
                }
                // tsc: a protected member is only compatible with the same
                // declaration reached through derivation.
                (Some((declaring, 0 | 2)), Some((iface_declaring, 2)))
                    if declaring != iface_declaring
                        && !self.class_derives_from(class_name, &iface_declaring) =>
                {
                    return Some(format!(
                        "Property '{}' is protected but type '{}' is not a class derived from '{}'.",
                        prop_name, class_display, iface_declaring
                    ));
                }
                _ => {}
            }
        }
        // Index signatures required by the interface but absent from the
        // class (own or inherited, instance side).
        let iface_index: Vec<Type> = self
            .interface_info
            .get(iface_name)
            .map(|info| {
                info.index_signatures
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect()
            })
            .unwrap_or_default();
        if iface_index.is_empty() {
            return None;
        }
        let mut class_keys: Vec<Type> = Vec::new();
        let mut current = Some(class_name.to_string());
        let mut hops = 0usize;
        while let Some(class) = current.take() {
            hops += 1;
            if hops > 64 {
                break;
            }
            let Some(info) = self.class_info.get(class.as_str()) else {
                break;
            };
            class_keys.extend(
                info.index_signatures
                    .iter()
                    .filter(|(_, _, is_static)| !is_static)
                    .map(|(key, _, _)| key.clone()),
            );
            current = info.extends.clone();
        }
        for key in iface_index {
            let covered = class_keys.iter().any(|k| *k == key)
                || (matches!(key, Type::Number)
                    && class_keys.iter().any(|k| matches!(k, Type::String)));
            if !covered {
                return Some(format!(
                    "Index signature for type '{}' is missing in type '{}'.",
                    key.display_string_single_line(),
                    class_display
                ));
            }
        }
        None
    }

    /// Collect `this.NAME = init` assignments (recursively through nested
    /// blocks/ifs/loops and arrow initializer bodies) as instance
    /// properties — the JS field-declaration pattern.
    pub(crate) fn collect_this_assignments_props(
        &self,
        stmts: &[Stmt],
        out: &mut Vec<(std::string::String, Type)>,
    ) {
        fn walk_expr(
            checker: &crate::TypeChecker,
            e: &Expr,
            out: &mut Vec<(std::string::String, Type)>,
        ) {
            match &e.kind {
                ExprKind::Assign(assign) => {
                    if assign.op == AssignOp::Assign {
                        if let ExprKind::Member(mem) = &assign.left.kind {
                            if matches!(mem.object.kind, ExprKind::This) {
                                let name = mem.property.to_string();
                                if !name.is_empty() && !out.iter().any(|(n, _)| *n == name) {
                                    let ty = crate::TypeChecker::widen_nested_literals(
                                        &checker.infer_expr_type(&assign.right),
                                    );
                                    out.push((name, ty));
                                }
                            }
                        }
                    }
                    walk_expr(checker, &assign.right, out);
                }
                ExprKind::Arrow(arrow) => match &arrow.body {
                    ArrowBody::Block(stmts) => checker.collect_this_assignments_props(stmts, out),
                    ArrowBody::Expr(inner) => walk_expr(checker, inner, out),
                },
                _ => {}
            }
        }
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Expr(e) => walk_expr(self, e, out),
                StmtKind::If(ifs) => {
                    self.collect_this_assignments_props(std::slice::from_ref(&ifs.consequent), out);
                    if let Some(alt) = &ifs.alternate {
                        self.collect_this_assignments_props(std::slice::from_ref(alt), out);
                    }
                }
                StmtKind::Block(inner) => self.collect_this_assignments_props(inner, out),
                StmtKind::For(f) => {
                    self.collect_this_assignments_props(std::slice::from_ref(&f.body), out)
                }
                StmtKind::While(w) => {
                    self.collect_this_assignments_props(std::slice::from_ref(&w.body), out)
                }
                _ => {}
            }
        }
    }

    pub(crate) fn get_class_instance_type(&self, class_name: &str) -> Type {
        let mut seen = HashSet::new();
        self.get_class_instance_type_inner(class_name, &mut seen)
    }

    fn get_class_instance_type_inner(
        &self,
        class_name: &str,
        seen: &mut HashSet<std::string::String>,
    ) -> Type {
        if !seen.insert(class_name.to_string()) {
            // Cycle detected — return an empty object type to break recursion
            return Type::TypeReference(class_name.to_string(), Arc::from([] as [Type; 0]));
        }

        let info = match self.class_info.get(class_name) {
            Some(info) => info,
            None => {
                // Try interface_info — many TypeReference types come from interfaces.
                // Merge inherited properties from extended interfaces so that e.g.
                // Request (extends Body) includes json(), text(), etc.
                if let Some(iface) = self.interface_info.get(class_name) {
                    let mut properties = Vec::new();
                    // First, collect properties from all extended interfaces
                    let extends = iface.extends.clone();
                    for (base_name, _base_args) in &extends {
                        if let Type::ObjectType(ref base_obj) =
                            self.get_class_instance_type_inner(base_name, seen)
                        {
                            for prop in &base_obj.properties {
                                if !properties
                                    .iter()
                                    .any(|(n, _): &(std::string::String, _)| n == &prop.0)
                                {
                                    properties.push(prop.clone());
                                }
                            }
                        }
                    }
                    // Then add own properties (overriding inherited ones)
                    for prop in &iface.object_type.properties {
                        if let Some(pos) = properties.iter().position(|(n, _)| n == &prop.0) {
                            properties[pos] = prop.clone();
                        } else {
                            properties.push(prop.clone());
                        }
                    }
                    let mut method_names = iface.object_type.method_names.clone();
                    for (base_name, _) in &extends {
                        if let Type::ObjectType(ref base_obj) =
                            self.get_class_instance_type_inner(base_name, seen)
                        {
                            for name in &base_obj.method_names {
                                if !method_names.contains(name) {
                                    method_names.push(name.clone());
                                }
                            }
                        }
                    }
                    return Type::ObjectType(ObjectTypeInfo {
                        properties,
                        call_signatures: iface.object_type.call_signatures.clone(),
                        construct_signatures: iface.object_type.construct_signatures.clone(),
                        index_signature: iface.object_type.index_signature.clone(),
                        index_signature_name: iface.object_type.index_signature_name.clone(),
                        method_names,
                    });
                }
                return Type::TypeReference(class_name.to_string(), Arc::from([] as [Type; 0]));
            }
        };

        // Extract fields before recursive calls that re-borrow self
        let base_name = info.extends.clone();
        // Optional members (`x?: T`, `constructor(public x?: T)`) are
        // wrapped like interface members so assignability treats them as
        // optional.
        let inst_props: Vec<_> = info
            .instance_properties
            .iter()
            .map(|(n, t)| {
                let ty = if info.optional_instance_properties.contains(n) {
                    Type::Optional(Arc::new(t.clone()))
                } else {
                    t.clone()
                };
                (n.clone(), ty)
            })
            .collect();
        let inst_methods: Vec<_> = info
            .instance_methods
            .iter()
            .map(|(n, ft)| (n.clone(), ft.clone()))
            .collect();
        let overloaded_methods = info.overloaded_methods.clone();

        let mut properties: Vec<(std::string::String, Arc<Type>)> = Vec::new();
        let mut method_names = Vec::new();

        // Include inherited members from base class, instantiated with the
        // extends clause's type arguments (`class D<T> extends C<string>`
        // inherits `x: string`, not `x: T`).
        let extends_type_args = info.extends_type_args.clone();
        if let Some(ref bn) = base_name {
            if let Type::ObjectType(ref base_obj) = self.get_class_instance_type_inner(bn, seen) {
                method_names.extend(base_obj.method_names.iter().cloned());
                let base_map: HashMap<std::string::String, Type> = self
                    .class_info
                    .get(bn.as_str())
                    .map(|base| {
                        base.type_params
                            .iter()
                            .cloned()
                            .zip(extends_type_args.iter().cloned())
                            .collect()
                    })
                    .unwrap_or_default();
                if base_map.is_empty() {
                    properties.extend(base_obj.properties.clone());
                } else {
                    properties.extend(base_obj.properties.iter().map(|(name, ty)| {
                        (name.clone(), Arc::new(Self::substitute(ty, &base_map)))
                    }));
                }
            }
        }
        // Declaration merging: an interface of the same name contributes its
        // members to the class instance type.
        if let Some(interface) = self.interface_info.get(class_name) {
            if interface.type_params.is_empty() {
                for (name, ty) in &interface.object_type.properties {
                    if !properties.iter().any(|(n, _)| n == name) {
                        let merged = if interface.optional_props.contains(name) {
                            Arc::new(Type::Optional(Arc::clone(ty)))
                        } else {
                            Arc::clone(ty)
                        };
                        properties.push((name.clone(), merged));
                        if interface.object_type.method_names.contains(name) {
                            method_names.push(name.clone());
                        }
                    }
                }
            }
        }

        for (name, ty) in &inst_props {
            method_names.retain(|method| method != name);
            if let Some(pos) = properties.iter().position(|(n, _)| n == name) {
                properties[pos] = (name.clone(), Arc::new(ty.clone()));
            } else {
                properties.push((name.clone(), Arc::new(ty.clone())));
            }
        }

        for (name, ft) in &inst_methods {
            if !method_names.contains(name) {
                method_names.push(name.clone());
            }
            // An overloaded method's callable member type is the Intersection
            // of its signature declarations (see build_class_info); a single
            // declaration keeps its plain Function type. Repeated same-name
            // entries re-insert the same overload set — idempotent.
            let member_ty = overloaded_methods
                .get(name)
                .cloned()
                .unwrap_or_else(|| Type::Function(ft.clone()));
            if let Some(pos) = properties.iter().position(|(n, _)| n == name) {
                properties[pos] = (name.clone(), Arc::new(member_ty));
            } else {
                properties.push((name.clone(), Arc::new(member_ty)));
            }
        }

        Type::ObjectType(ObjectTypeInfo {
            properties,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_signature: None,
            index_signature_name: None,
            method_names,
        })
    }

    /// Return constructor parameter types only when the entire inheritance
    /// lookup is locally known and has one non-generic, bodied constructor.
    /// This deliberately excludes overloads and ambient/external signatures:
    /// contextual typing with an arbitrary implementation signature would be
    /// less conservative than checking those arguments without a context.
    pub(crate) fn get_contextual_constructor_params(
        &self,
        class_name: &str,
    ) -> Option<Vec<(Type, bool)>> {
        let mut cur = class_name.to_string();
        let mut seen: HashSet<std::string::String> = HashSet::new();
        let mut inherited_params = None;
        let mut is_direct_class = true;
        loop {
            if !seen.insert(cur.clone()) {
                return None;
            }
            let Some(info) = self.class_info.get(cur.as_str()) else {
                return inherited_params;
            };
            if inherited_params.is_none() {
                if !info.type_params.is_empty() {
                    return None;
                }
                if info.has_explicit_constructor {
                    if !info.constructor_contextual_typing_safe {
                        return None;
                    }
                    let params = info
                        .constructor_params
                        .iter()
                        .map(|(_, ty, _, rest)| (ty.clone(), *rest))
                        .collect();
                    // A class's own construct signature remains usable even
                    // when its heritage is invalid. An inherited signature,
                    // however, is only safe after the rest of the heritage
                    // chain has been checked for a cycle.
                    if is_direct_class {
                        return Some(params);
                    }
                    inherited_params = Some(params);
                }
            }
            match info.extends {
                Some(ref base_name) => {
                    cur = base_name.clone();
                    is_direct_class = false;
                }
                None => return Some(inherited_params.unwrap_or_default()),
            }
        }
    }

    // -------------------------------------------------------------------
    // TS2564: Strict property initialization
    // -------------------------------------------------------------------

    /// TS2564: Check that all non-optional, non-initialized class properties
    /// are definitely assigned in the constructor.
    pub(crate) fn check_strict_property_initialization(&mut self, class_decl: &ClassDecl) {
        use tsc_rs_ast::{ClassMemberKind, PropName, MOD_ABSTRACT, MOD_DECLARE, MOD_STATIC};

        // Skip ambient/declare classes
        if (class_decl.modifiers & MOD_DECLARE) != 0 {
            return;
        }

        // Collect constructor body and parameter property names
        let mut constructor_body: Option<&[Stmt]> = None;
        let mut param_property_names: HashSet<String> = HashSet::new();
        for member in &class_decl.members {
            if let ClassMemberKind::Constructor(ctor) = &member.kind {
                if let Some(ref body) = ctor.body {
                    constructor_body = Some(body.as_slice());
                }
                for param in &ctor.params {
                    let is_param_property = (param.modifiers
                        & (tsc_rs_ast::MOD_PUBLIC
                            | tsc_rs_ast::MOD_PRIVATE
                            | tsc_rs_ast::MOD_PROTECTED
                            | tsc_rs_ast::MOD_READONLY))
                        != 0;
                    if is_param_property {
                        if let tsc_rs_ast::PatKind::Ident(ref name) = param.name.kind {
                            param_property_names.insert(name.to_string());
                        }
                    }
                }
                break;
            }
        }

        // Collect assigned properties from constructor body
        let mut assigned_in_ctor: HashSet<String> = HashSet::new();
        if let Some(body) = constructor_body {
            Self::collect_this_assignments(body, &mut assigned_in_ctor);
        }

        // Check each instance property
        for member in &class_decl.members {
            if let ClassMemberKind::Property(prop) = &member.kind {
                if (prop.modifiers & MOD_STATIC) != 0 {
                    continue;
                }
                if (prop.modifiers & MOD_ABSTRACT) != 0 {
                    continue;
                }
                if (prop.modifiers & MOD_DECLARE) != 0 {
                    continue;
                }
                if prop.optional {
                    continue;
                }
                if prop.definite {
                    continue;
                }
                if prop.initializer.is_some() {
                    continue;
                }

                // tsc checks identifier, private and computed names only;
                // string/numeric literal names are exempt. Computed names
                // print as their source text (`[NS.x]`).
                let prop_name = match &prop.name {
                    PropName::Ident(name, _) => name.to_string(),
                    PropName::String(_, _) | PropName::Number(_, _) => continue,
                    PropName::Private(name, _) => format!("#{}", name),
                    PropName::Computed(_, span) => match self
                        .current_source
                        .as_deref()
                        .and_then(|source| source.get(span.start as usize..span.end as usize))
                    {
                        Some(text) => text.to_string(),
                        None => continue,
                    },
                };

                if assigned_in_ctor.contains(&prop_name) {
                    continue;
                }
                if param_property_names.contains(&prop_name) {
                    continue;
                }

                // Check if the type includes undefined/any/void/unknown
                if let Some(ref type_ann) = prop.type_ann {
                    let ty = self.resolve_type_node(type_ann);
                    if Self::type_includes_undefined(&ty) {
                        continue;
                    }
                }

                let span = prop.name.span();
                self.diagnostics
                    .push(super::diagnostics::error_property_not_definitely_assigned(
                        &prop_name, span,
                    ));
            }
        }
    }

    /// Walk constructor body statements looking for `this.propName = ...`
    fn collect_this_assignments(stmts: &[Stmt], assigned: &mut HashSet<String>) {
        use tsc_rs_ast::StmtKind;
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Expr(expr) => {
                    Self::collect_this_assign_expr(expr, assigned);
                }
                StmtKind::If(if_stmt) => {
                    Self::collect_this_assign_stmt(&if_stmt.consequent, assigned);
                    if let Some(ref alt) = if_stmt.alternate {
                        Self::collect_this_assign_stmt(alt, assigned);
                    }
                }
                StmtKind::Block(block_stmts) => {
                    Self::collect_this_assignments(block_stmts, assigned);
                }
                StmtKind::Switch(sw) => {
                    for case in &sw.cases {
                        Self::collect_this_assignments(&case.consequent, assigned);
                    }
                }
                StmtKind::Try(try_stmt) => {
                    Self::collect_this_assignments(&try_stmt.block, assigned);
                }
                StmtKind::For(for_stmt) => {
                    Self::collect_this_assign_stmt(&for_stmt.body, assigned);
                }
                StmtKind::ForIn(fo) => {
                    Self::collect_this_assign_stmt(&fo.body, assigned);
                }
                StmtKind::ForOf(fo) => {
                    Self::collect_this_assign_stmt(&fo.body, assigned);
                }
                StmtKind::While(w) => {
                    Self::collect_this_assign_stmt(&w.body, assigned);
                }
                StmtKind::DoWhile(dw) => {
                    Self::collect_this_assign_stmt(&dw.body, assigned);
                }
                StmtKind::Return(Some(expr)) => {
                    Self::collect_this_assign_expr(expr, assigned);
                }
                _ => {}
            }
        }
    }

    fn collect_this_assign_stmt(stmt: &Stmt, assigned: &mut HashSet<String>) {
        use tsc_rs_ast::StmtKind;
        match &stmt.kind {
            StmtKind::Block(stmts) => Self::collect_this_assignments(stmts, assigned),
            _ => Self::collect_this_assignments(std::slice::from_ref(stmt), assigned),
        }
    }

    fn collect_this_assign_expr(expr: &tsc_rs_ast::Expr, assigned: &mut HashSet<String>) {
        use tsc_rs_ast::ExprKind;
        match &expr.kind {
            ExprKind::Assign(assign_expr) => {
                if let ExprKind::Member(member) = &assign_expr.left.kind {
                    if matches!(member.object.kind, ExprKind::This) {
                        assigned.insert(member.property.to_string());
                    }
                }
                if let ExprKind::ElemAccess(element) = &assign_expr.left.kind {
                    if matches!(element.object.kind, ExprKind::This) {
                        match &element.index.kind {
                            ExprKind::StrLit(name) | ExprKind::NumLit(name) => {
                                assigned.insert(name.to_string());
                            }
                            _ => {
                                // `this[NS.key] = ...` initializes the computed
                                // member declared as `[NS.key]: T`.
                                if let Some(path) = Self::simple_expression_path(&element.index) {
                                    assigned.insert(format!("[{path}]"));
                                }
                            }
                        }
                    }
                }
            }
            ExprKind::Comma(exprs) => {
                for e in exprs {
                    Self::collect_this_assign_expr(e, assigned);
                }
            }
            ExprKind::Paren(inner) => {
                Self::collect_this_assign_expr(inner, assigned);
            }
            _ => {}
        }
    }
}
