use super::*;

impl<'a> Emitter<'a> {
    // ------------------------------------------------------------------
    // System module emission
    // ------------------------------------------------------------------

    /// Try emitting a limited System.register wrapper for module files.
    /// Returns `true` when this path handled the file, `false` to let the
    /// caller fall back to the default non-System emit path.
    pub(super) fn emit_system_module(&mut self, file: &SourceFile) -> bool {
        #[derive(Clone)]
        enum PatternAssignment {
            Expr(Box<Expr>),
            Default {
                value: Box<Expr>,
                initializer: Box<Expr>,
                value_temp: Option<String>,
            },
            EmptyObject(Box<Expr>),
            EmptyArray(Box<Expr>),
            ObjectRest {
                base: Box<Expr>,
                excluded: Vec<String>,
            },
        }

        #[derive(Clone)]
        enum ExecuteAction {
            AssignVar {
                name: String,
                init: Box<Expr>,
                export_name: Option<String>,
            },
            ExportEmptyBinding {
                value_temp: String,
                export_name: String,
                init: Box<Expr>,
            },
            ExportObjectRestEs5 {
                temp_name: String,
                init: Box<Expr>,
                binding_name: String,
                binding_span: Span,
                rest_name: String,
                rest_span: Span,
            },
            EmitExportPattern {
                temp_name: String,
                init: Box<Expr>,
                assignments: Vec<(String, PatternAssignment)>,
            },
            AssignClass {
                name: String,
                class_decl: ClassDecl,
                export_name: Option<String>,
                /// Self-reference alias for decorated classes (e.g. `"Testing123_1"`).
                decorator_alias: Option<String>,
            },
            EmitStmt(Stmt),
            EmitNamedExport {
                exported: String,
                local: String,
                local_order: usize,
            },
            EmitDefaultExportExpr(Expr),
            /// Emit leading comments from the source at the given position.
            EmitLeadingComments(u32),
            /// using/await using: marks the start of the disposal scope.
            UsingDisposalStart {
                env_name: String,
                is_await: bool,
            },
            /// A using declaration: `name = __addDisposableResource(env, init, is_await)`
            AddDisposableResource {
                name: String,
                init: Box<Expr>,
                env_name: String,
                is_await: bool,
            },
        }

        #[derive(Clone)]
        enum SystemDepOp {
            AssignVar(String),
            AssignVarAndExport(String, String), // (var_name, export_name)
            ExportStar,
            ReExport(Vec<(String, String)>), // (exported, imported)
        }

        #[derive(Clone)]
        struct SystemDep {
            source: String,
            param: String,
            ops: Vec<SystemDepOp>,
        }

        fn system_dep_param_name(source: &str) -> String {
            let seg = source.rsplit('/').next().unwrap_or(source);
            let mut name = String::new();
            for (i, ch) in seg.chars().enumerate() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
                    if i == 0 && ch.is_ascii_digit() {
                        name.push('_');
                    }
                    name.push(ch);
                } else {
                    name.push('_');
                }
            }
            if name.is_empty() {
                name.push('_');
            }
            if name == "_" {
                "_1_1".to_string()
            } else {
                format!("{name}_1_1")
            }
        }

        fn expr_refers_known_type_only(expr: &Expr, type_only_names: &HashSet<AstString>) -> bool {
            match &expr.kind {
                ExprKind::Ident(name) => type_only_names.contains(name.as_str()),
                ExprKind::Member(mem) => type_only_names.contains(mem.property.as_str()),
                ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                    expr_refers_known_type_only(inner, type_only_names)
                }
                ExprKind::TypeAssertion(ta) => {
                    expr_refers_known_type_only(&ta.expr, type_only_names)
                }
                ExprKind::As(a) => expr_refers_known_type_only(&a.expr, type_only_names),
                ExprKind::Satisfies(s) => expr_refers_known_type_only(&s.expr, type_only_names),
                ExprKind::Instantiation(inst) => {
                    expr_refers_known_type_only(&inst.expr, type_only_names)
                }
                _ => false,
            }
        }

        fn is_ident_like(name: &str) -> bool {
            let mut chars = name.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            if !(first.is_ascii_alphabetic() || first == '_' || first == '$') {
                return false;
            }
            chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
        }

        fn ident_expr(name: &str) -> Expr {
            Expr {
                kind: ExprKind::Ident(name.into()),
                span: Span::default(),
            }
        }

        fn num_lit_expr(index: usize) -> Expr {
            Expr {
                kind: ExprKind::NumLit(index.to_string().into()),
                span: Span::default(),
            }
        }

        fn elem_access_expr(object: Expr, index: Expr) -> Expr {
            Expr {
                kind: ExprKind::ElemAccess(ElemAccessExpr {
                    object: Box::new(object),
                    index: Box::new(index),
                    optional: false,
                }),
                span: Span::default(),
            }
        }

        fn member_expr(object: Expr, property: &str) -> Expr {
            Expr {
                kind: ExprKind::Member(Box::new(MemberExpr {
                    object: Box::new(object),
                    property: property.into(),
                    optional: false,
                })),
                span: Span::default(),
            }
        }

        fn object_prop_access_expr(base: Expr, key: &PropName) -> Option<Expr> {
            match key {
                PropName::Ident(name, _) => Some(member_expr(base, name)),
                PropName::String(name, span) => {
                    if is_ident_like(name) {
                        Some(member_expr(base, name))
                    } else {
                        Some(elem_access_expr(
                            base,
                            Expr {
                                kind: ExprKind::StrLit(name.clone()),
                                span: *span,
                            },
                        ))
                    }
                }
                PropName::Number(n, _) => Some(elem_access_expr(
                    base,
                    Expr {
                        kind: ExprKind::NumLit(n.clone()),
                        span: Span::default(),
                    },
                )),
                _ => None,
            }
        }

        fn object_prop_excluded_key(key: &PropName) -> Option<String> {
            match key {
                PropName::Ident(name, _)
                | PropName::String(name, _)
                | PropName::Number(name, _) => Some(name.to_string()),
                PropName::Computed(expr, _) => match &expr.kind {
                    ExprKind::StrLit(name) | ExprKind::NumLit(name) => Some(name.to_string()),
                    _ => None,
                },
                PropName::Private(_, _) => None,
            }
        }

        fn collect_pattern_assignment_exprs(
            pat: &Pat,
            base: Expr,
            depth: usize,
            out: &mut Vec<(String, PatternAssignment)>,
        ) -> bool {
            match &pat.kind {
                PatKind::Ident(name) => {
                    out.push((name.to_string(), PatternAssignment::Expr(Box::new(base))));
                    true
                }
                PatKind::Assign(inner, initializer) => {
                    let PatKind::Ident(name) = &inner.kind else {
                        return false;
                    };
                    out.push((
                        name.to_string(),
                        PatternAssignment::Default {
                            value: Box::new(base),
                            initializer: initializer.clone(),
                            value_temp: None,
                        },
                    ));
                    true
                }
                PatKind::Rest(inner) => collect_pattern_assignment_exprs(inner, base, depth, out),
                PatKind::Array(elements) => {
                    if elements.is_empty() {
                        if depth > 0 {
                            out.push((
                                String::new(),
                                PatternAssignment::EmptyArray(Box::new(base)),
                            ));
                        }
                        return true;
                    }
                    for (idx, elem) in elements.iter().enumerate() {
                        let Some(elem) = elem else {
                            continue;
                        };
                        match elem {
                            ArrayPatElem::Pat(p) => {
                                let next = elem_access_expr(base.clone(), num_lit_expr(idx));
                                if !collect_pattern_assignment_exprs(p, next, depth + 1, out) {
                                    return false;
                                }
                            }
                            ArrayPatElem::Rest(_) => return false,
                        }
                    }
                    true
                }
                PatKind::Object(props) => {
                    if props.is_empty() {
                        if depth > 0 {
                            out.push((
                                String::new(),
                                PatternAssignment::EmptyObject(Box::new(base)),
                            ));
                        }
                        return true;
                    }
                    let mut excluded = Vec::new();
                    for prop in props {
                        match prop {
                            ObjPatProp::KeyValue(key, value) => {
                                let Some(next) = object_prop_access_expr(base.clone(), key) else {
                                    return false;
                                };
                                let Some(excluded_key) = object_prop_excluded_key(key) else {
                                    return false;
                                };
                                let assignments_before = out.len();
                                if !collect_pattern_assignment_exprs(value, next, depth + 1, out) {
                                    return false;
                                }
                                // A nested property access must not be duplicated: doing so
                                // would invoke a getter more than once. Leave shapes with
                                // multiple nested bindings to the general emitter.
                                if !matches!(value.kind, PatKind::Ident(_) | PatKind::Assign(_, _))
                                    && out.len() - assignments_before != 1
                                {
                                    return false;
                                }
                                excluded.push(excluded_key);
                            }
                            ObjPatProp::Shorthand(name, _) => {
                                out.push((
                                    name.to_string(),
                                    PatternAssignment::Expr(Box::new(member_expr(
                                        base.clone(),
                                        name,
                                    ))),
                                ));
                                excluded.push(name.to_string());
                            }
                            ObjPatProp::ShorthandAssign(name, initializer, _) => {
                                out.push((
                                    name.to_string(),
                                    PatternAssignment::Default {
                                        value: Box::new(member_expr(base.clone(), name)),
                                        initializer: initializer.clone(),
                                        value_temp: None,
                                    },
                                ));
                                excluded.push(name.to_string());
                            }
                            ObjPatProp::Rest(rest) => {
                                let PatKind::Ident(name) = &rest.kind else {
                                    return false;
                                };
                                out.push((
                                    name.to_string(),
                                    PatternAssignment::ObjectRest {
                                        base: Box::new(base.clone()),
                                        excluded: excluded.clone(),
                                    },
                                ));
                            }
                        }
                    }
                    true
                }
            }
        }

        fn allocate_pattern_default_temps(
            emitter: &mut Emitter<'_>,
            assignments: &mut [(String, PatternAssignment)],
            var_names: &mut Vec<String>,
            seen_var_names: &mut HashSet<String>,
        ) {
            for (_, assignment) in assignments {
                let PatternAssignment::Default { value_temp, .. } = assignment else {
                    continue;
                };
                let temp_name = emitter.next_omitted_array_temp();
                if seen_var_names.insert(temp_name.clone()) {
                    var_names.push(temp_name.clone());
                }
                *value_temp = Some(temp_name);
            }
        }

        fn add_var_stmt_names(
            var_stmt: &VarStmt,
            out: &mut Vec<String>,
            seen: &mut HashSet<String>,
        ) {
            if var_stmt.kind != VarKind::Var || var_stmt.modifiers & MOD_DECLARE != 0 {
                return;
            }
            for decl in &var_stmt.declarations {
                let mut names = Vec::new();
                collect_binding_names(&decl.name, &mut names);
                for name in names {
                    if seen.insert(name.clone()) {
                        out.push(name);
                    }
                }
            }
        }

        fn collect_function_scope_var_names(
            stmt: &Stmt,
            out: &mut Vec<String>,
            seen: &mut HashSet<String>,
        ) {
            match &stmt.kind {
                StmtKind::Var(var_stmt) => add_var_stmt_names(var_stmt, out, seen),
                StmtKind::Block(stmts) => {
                    for s in stmts {
                        collect_function_scope_var_names(s, out, seen);
                    }
                }
                StmtKind::If(if_stmt) => {
                    collect_function_scope_var_names(&if_stmt.consequent, out, seen);
                    if let Some(alt) = &if_stmt.alternate {
                        collect_function_scope_var_names(alt, out, seen);
                    }
                }
                StmtKind::For(for_stmt) => {
                    if let Some(ForInit::Var(var_stmt)) = &for_stmt.init {
                        add_var_stmt_names(var_stmt, out, seen);
                    }
                    collect_function_scope_var_names(&for_stmt.body, out, seen);
                }
                StmtKind::ForIn(for_in_stmt) => {
                    if let ForInOfLeft::Var(var_stmt) = &for_in_stmt.left {
                        add_var_stmt_names(var_stmt, out, seen);
                    }
                    collect_function_scope_var_names(&for_in_stmt.body, out, seen);
                }
                StmtKind::ForOf(for_of_stmt) => {
                    if let ForInOfLeft::Var(var_stmt) = &for_of_stmt.left {
                        add_var_stmt_names(var_stmt, out, seen);
                    }
                    collect_function_scope_var_names(&for_of_stmt.body, out, seen);
                }
                StmtKind::While(while_stmt) => {
                    collect_function_scope_var_names(&while_stmt.body, out, seen);
                }
                StmtKind::DoWhile(do_while_stmt) => {
                    collect_function_scope_var_names(&do_while_stmt.body, out, seen);
                }
                StmtKind::Switch(switch_stmt) => {
                    for case in &switch_stmt.cases {
                        for s in &case.consequent {
                            collect_function_scope_var_names(s, out, seen);
                        }
                    }
                }
                StmtKind::Try(try_stmt) => {
                    for s in &try_stmt.block {
                        collect_function_scope_var_names(s, out, seen);
                    }
                    if let Some(catch_clause) = &try_stmt.handler {
                        for s in &catch_clause.body {
                            collect_function_scope_var_names(s, out, seen);
                        }
                    }
                    if let Some(finalizer) = &try_stmt.finalizer {
                        for s in finalizer {
                            collect_function_scope_var_names(s, out, seen);
                        }
                    }
                }
                StmtKind::Labeled(labeled_stmt) => {
                    collect_function_scope_var_names(&labeled_stmt.body, out, seen);
                }
                StmtKind::With(with_stmt) => {
                    collect_function_scope_var_names(&with_stmt.body, out, seen);
                }
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                        collect_function_scope_var_names(inner, out, seen);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let preserve_const_enums = self.preserve_const_enums_effective();
        let mut system_deps: Vec<SystemDep> = Vec::new();
        let mut dep_index_by_source: HashMap<String, usize> = HashMap::new();
        let mut var_names: Vec<String> = Vec::new();
        let mut seen_var_names: HashSet<String> = HashSet::new();
        let mut system_transform_temp_names: Vec<String> = Vec::new();
        let mut deferred_system_export_names: Vec<String> = Vec::new();
        let mut local_export_names: Vec<String> = Vec::new();
        let mut seen_local_export_names: HashSet<String> = HashSet::new();
        let mut execute_actions: Vec<ExecuteAction> = Vec::new();
        let mut prelude_fn_decls: Vec<FnDecl> = Vec::new();
        let mut prelude_fn_decl_spans: Vec<Span> = Vec::new();
        let mut prelude_fn_names: HashSet<String> = HashSet::new();
        let mut prelude_export_calls: Vec<(String, String)> = Vec::new();
        let mut seen_prelude_export_calls: HashSet<(String, String)> = HashSet::new();
        let mut seen_execute_named_exports: HashSet<(String, String)> = HashSet::new();
        let mut default_export_counter = 1usize;
        let mut local_symbol_order: HashMap<String, usize> = HashMap::new();
        let mut next_local_symbol_order = 0usize;
        let mut decl_inline_export_names: HashSet<String> = HashSet::new();
        let mut inline_export_aliases: HashMap<String, Vec<String>> = HashMap::new();

        let ensure_dep = |source: &str,
                          suggested_param: Option<String>,
                          system_deps: &mut Vec<SystemDep>,
                          dep_index_by_source: &mut HashMap<String, usize>|
         -> usize {
            if let Some(idx) = dep_index_by_source.get(source).copied() {
                return idx;
            }
            let param = suggested_param.unwrap_or_else(|| system_dep_param_name(source));
            let idx = system_deps.len();
            system_deps.push(SystemDep {
                source: source.to_string(),
                param,
                ops: Vec::new(),
            });
            dep_index_by_source.insert(source.to_string(), idx);
            idx
        };
        let register_local_symbol = |name: &str,
                                     local_symbol_order: &mut HashMap<String, usize>,
                                     next_local_symbol_order: &mut usize|
         -> usize {
            if let Some(order) = local_symbol_order.get(name).copied() {
                return order;
            }
            let order = *next_local_symbol_order;
            *next_local_symbol_order += 1;
            local_symbol_order.insert(name.to_string(), order);
            order
        };

        let add_var_name =
            |name: &str, var_names: &mut Vec<String>, seen_var_names: &mut HashSet<String>| {
                if seen_var_names.insert(name.to_string()) {
                    var_names.push(name.to_string());
                }
            };
        let add_local_export_name =
            |name: &str,
             local_export_names: &mut Vec<String>,
             seen_local_export_names: &mut HashSet<String>| {
                if seen_local_export_names.insert(name.to_string()) {
                    local_export_names.push(name.to_string());
                }
            };
        let add_prelude_export_call =
            |exported: &str,
             local: &str,
             prelude_export_calls: &mut Vec<(String, String)>,
             seen_prelude_export_calls: &mut HashSet<(String, String)>| {
                let key = (exported.to_string(), local.to_string());
                if seen_prelude_export_calls.insert(key.clone()) {
                    prelude_export_calls.push(key);
                }
            };
        let add_execute_named_export =
            |exported: &str,
             local: &str,
             execute_actions: &mut Vec<ExecuteAction>,
             seen_execute_named_exports: &mut HashSet<(String, String)>,
             local_symbol_order: &HashMap<String, usize>| {
                let key = (exported.to_string(), local.to_string());
                if seen_execute_named_exports.insert(key.clone()) {
                    let local_order = local_symbol_order.get(local).copied().unwrap_or(usize::MAX);
                    execute_actions.push(ExecuteAction::EmitNamedExport {
                        exported: key.0,
                        local: key.1,
                        local_order,
                    });
                }
            };
        let add_var_decl = |name: &str,
                            init: &Option<Box<Expr>>,
                            exported: bool,
                            var_names: &mut Vec<String>,
                            seen_var_names: &mut HashSet<String>,
                            local_export_names: &mut Vec<String>,
                            seen_local_export_names: &mut HashSet<String>,
                            execute_actions: &mut Vec<ExecuteAction>,
                            local_symbol_order: &mut HashMap<String, usize>,
                            next_local_symbol_order: &mut usize| {
            register_local_symbol(name, local_symbol_order, next_local_symbol_order);
            if seen_var_names.insert(name.to_string()) {
                var_names.push(name.to_string());
            }
            if exported {
                if seen_local_export_names.insert(name.to_string()) {
                    local_export_names.push(name.to_string());
                }
                if let Some(init_expr) = init.clone() {
                    execute_actions.push(ExecuteAction::AssignVar {
                        name: name.to_string(),
                        init: init_expr,
                        export_name: Some(name.to_string()),
                    });
                }
            } else if let Some(init_expr) = init.clone() {
                execute_actions.push(ExecuteAction::AssignVar {
                    name: name.to_string(),
                    init: init_expr,
                    export_name: None,
                });
            }
        };

        // Pre-scan all statements to collect hoisted var names (including nested
        // ones inside if/for/etc.) into a separate set.  This lets `export { y }`
        // find `y` from `if (...) { var y = 1; }` during the main scan loop, without
        // affecting the ordering or deduplication of the main `var_names` / `seen_var_names`.
        let all_hoisted_var_names: HashSet<String> = {
            let mut prescan_names: Vec<String> = Vec::new();
            let mut prescan_seen: HashSet<String> = HashSet::new();
            for stmt in &file.statements {
                collect_function_scope_var_names(stmt, &mut prescan_names, &mut prescan_seen);
            }
            prescan_seen
        };

        // Pre-scan: collect all local names referenced in `export { name }`
        // (without source) so that inside a disposal scope we know which
        // assignments need `exports_fn(...)` wrapping.
        let named_export_locals: HashSet<String> = {
            let mut set = HashSet::new();
            for stmt in &file.statements {
                if let StmtKind::Export(ed) = &stmt.kind {
                    if let ExportDeclKind::Named {
                        specifiers,
                        source: None,
                        type_only: false,
                    } = &ed.kind
                    {
                        for spec in specifiers.iter().filter(|s| !s.is_type) {
                            set.insert(spec.local.to_string());
                        }
                    }
                }
            }
            set
        };

        for stmt in &file.statements {
            if is_use_strict_directive(stmt) {
                continue;
            }
            let recoverable_reserved_word_module = matches!(
                &stmt.kind,
                StmtKind::ModuleDecl(module_decl)
                    if self.module_decl_has_reserved_word_recovery_shape(module_decl)
            );
            if stmt_is_erased(stmt, preserve_const_enums) && !recoverable_reserved_word_module {
                continue;
            }

            let actions_len_before = execute_actions.len();

            match &stmt.kind {
                StmtKind::Import(import_decl) => {
                    if self.import_decl_has_reserved_word_recovery_shape(import_decl, stmt.span) {
                        execute_actions.push(ExecuteAction::EmitStmt(stmt.clone()));
                        continue;
                    }
                    if self.import_decl_has_missing_namespace_as_recovery(import_decl) {
                        if let ImportClause::Named {
                            namespace: Some(namespace),
                            ..
                        } = &import_decl.specifiers
                        {
                            add_var_name(namespace, &mut var_names, &mut seen_var_names);
                        }
                        continue;
                    }
                    if import_decl.type_only || import_decl.source.is_empty() {
                        continue;
                    }
                    match &import_decl.specifiers {
                        ImportClause::Named {
                            default,
                            named,
                            namespace,
                        } => {
                            let keep_default =
                                default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                            let keep_namespace = namespace
                                .as_ref()
                                .is_some_and(|ns| !self.is_import_elided(ns));
                            let non_type_named: Vec<&ImportSpecifier> = named
                                .iter()
                                .filter(|s| !s.is_type && !self.is_import_elided(&s.local))
                                .collect();
                            let has_bindings =
                                keep_namespace || keep_default || !non_type_named.is_empty();
                            if !has_bindings {
                                if namespace.is_none() && default.is_none() && named.is_empty() {
                                    // Side-effect import.
                                    ensure_dep(
                                        &import_decl.source,
                                        None,
                                        &mut system_deps,
                                        &mut dep_index_by_source,
                                    );
                                }
                                continue;
                            }
                            if let Some(ns) = namespace.as_ref().filter(|_| keep_namespace) {
                                let pure_namespace = !keep_default && non_type_named.is_empty();
                                if pure_namespace {
                                    add_var_name(ns, &mut var_names, &mut seen_var_names);
                                    register_local_symbol(
                                        ns,
                                        &mut local_symbol_order,
                                        &mut next_local_symbol_order,
                                    );
                                    let dep_idx = ensure_dep(
                                        &import_decl.source,
                                        Some(format!("{}_1", ns)),
                                        &mut system_deps,
                                        &mut dep_index_by_source,
                                    );
                                    let assign_exists =
                                        system_deps[dep_idx].ops.iter().any(|op| {
                                            matches!(op, SystemDepOp::AssignVar(existing) if existing == ns)
                                        });
                                    if !assign_exists {
                                        system_deps[dep_idx]
                                            .ops
                                            .push(SystemDepOp::AssignVar(ns.clone()));
                                    }
                                    self.cjs_import_map.insert(
                                        ns.clone().into(),
                                        (ns.clone().into(), AstString::default()),
                                    );
                                    continue;
                                }
                            }
                            let dep_var = self.next_require_var(&import_decl.source);
                            add_var_name(&dep_var, &mut var_names, &mut seen_var_names);
                            let dep_idx = ensure_dep(
                                &import_decl.source,
                                None,
                                &mut system_deps,
                                &mut dep_index_by_source,
                            );
                            let assign_exists = system_deps[dep_idx].ops.iter().any(|op| {
                                matches!(op, SystemDepOp::AssignVar(existing) if existing == &dep_var)
                            });
                            if !assign_exists {
                                system_deps[dep_idx]
                                    .ops
                                    .push(SystemDepOp::AssignVar(dep_var.clone()));
                            }
                            if let Some(local_default) = default.as_ref().filter(|_| keep_default) {
                                register_local_symbol(
                                    local_default,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                self.cjs_import_map.insert(
                                    local_default.clone().into(),
                                    (dep_var.clone().into(), "default".into()),
                                );
                                self.cjs_default_import_bindings
                                    .insert(local_default.clone().into());
                            }
                            for spec in non_type_named {
                                let imported = spec.imported.as_deref().unwrap_or(&spec.local);
                                register_local_symbol(
                                    &spec.local,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                self.cjs_import_map.insert(
                                    spec.local.clone().into(),
                                    (dep_var.clone().into(), imported.into()),
                                );
                            }
                        }
                        ImportClause::Require(local) => {
                            if self.is_import_elided(local) {
                                continue;
                            }
                            add_var_name(local, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                local,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            let dep_idx = ensure_dep(
                                &import_decl.source,
                                Some(format!("{}_1", local)),
                                &mut system_deps,
                                &mut dep_index_by_source,
                            );
                            let assign_exists = system_deps[dep_idx].ops.iter().any(|op| {
                                matches!(op, SystemDepOp::AssignVar(existing) if existing == local)
                            });
                            if !assign_exists {
                                system_deps[dep_idx]
                                    .ops
                                    .push(SystemDepOp::AssignVar(local.clone()));
                            }
                            self.cjs_import_map.insert(
                                local.clone().into(),
                                (local.clone().into(), AstString::default()),
                            );
                        }
                    }
                }
                StmtKind::ImportEquals(ie) => {
                    if expr_refers_known_type_only(&ie.module_ref, &self.type_only_decl_names) {
                        continue;
                    }
                    add_var_name(&ie.name, &mut var_names, &mut seen_var_names);
                    register_local_symbol(
                        &ie.name,
                        &mut local_symbol_order,
                        &mut next_local_symbol_order,
                    );
                    execute_actions.push(ExecuteAction::AssignVar {
                        name: ie.name.clone(),
                        init: ie.module_ref.clone(),
                        export_name: None,
                    });
                }
                StmtKind::ExportAssign(_) => {
                    // `export =` is erased in SystemJS emit.
                }
                StmtKind::Var(var_decl) => {
                    // using / await using → disposal scope
                    if matches!(var_decl.kind, VarKind::Using | VarKind::AwaitUsing)
                        && self.needs_downlevel("using")
                    {
                        let is_await = var_decl.kind == VarKind::AwaitUsing;
                        let env_num = self.next_using_env_num();
                        let env_name = format!("env_{}", env_num);
                        self.needs_add_disposable_resource_helper = true;
                        self.needs_dispose_resources_helper = true;

                        execute_actions.push(ExecuteAction::UsingDisposalStart {
                            env_name: env_name.clone(),
                            is_await,
                        });
                        for decl in &var_decl.declarations {
                            if let PatKind::Ident(name) = &decl.name.kind {
                                add_var_name(name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                if let Some(init) = &decl.init {
                                    execute_actions.push(ExecuteAction::AddDisposableResource {
                                        name: name.to_string(),
                                        init: Box::new((**init).clone()),
                                        env_name: env_name.clone(),
                                        is_await,
                                    });
                                }
                            }
                        }
                        continue;
                    }

                    let is_exported = var_decl.modifiers & MOD_EXPORT != 0;
                    if is_exported {
                        if let Some((init, binding_name, binding_span, rest_name, rest_span)) =
                            self.direct_es5_object_rest_export(var_decl, stmt.span)
                        {
                            let temp_name = self.make_temp_name();
                            add_var_name(&temp_name, &mut var_names, &mut seen_var_names);
                            system_transform_temp_names.push(temp_name.clone());
                            register_local_symbol(
                                &temp_name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            for name in [&binding_name, &rest_name] {
                                add_var_name(name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                add_local_export_name(
                                    name,
                                    &mut local_export_names,
                                    &mut seen_local_export_names,
                                );
                            }
                            execute_actions.push(ExecuteAction::ExportObjectRestEs5 {
                                temp_name,
                                init,
                                binding_name: binding_name.to_string(),
                                binding_span,
                                rest_name: rest_name.to_string(),
                                rest_span,
                            });
                            continue;
                        }
                    }
                    let all_ident = var_decl
                        .declarations
                        .iter()
                        .all(|decl| matches!(decl.name.kind, PatKind::Ident(_)));
                    if all_ident {
                        let var_actions_before = execute_actions.len();
                        for decl in &var_decl.declarations {
                            let PatKind::Ident(name) = &decl.name.kind else {
                                continue;
                            };
                            add_var_decl(
                                name,
                                &decl.init,
                                is_exported,
                                &mut var_names,
                                &mut seen_var_names,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                                &mut execute_actions,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                        }
                        if execute_actions.len() > var_actions_before {
                            execute_actions.insert(
                                var_actions_before,
                                ExecuteAction::EmitLeadingComments(stmt.span.start),
                            );
                        }
                        continue;
                    }

                    if !is_exported {
                        let mut all_lowered = true;
                        let mut lowered_actions: Vec<ExecuteAction> = Vec::new();
                        for decl in &var_decl.declarations {
                            let mut names = Vec::new();
                            collect_binding_names(&decl.name, &mut names);
                            for name in &names {
                                add_var_name(name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                            }
                            // Lower destructuring patterns to individual assignments
                            if let Some(ref init) = decl.init {
                                if !matches!(&decl.name.kind, PatKind::Ident(_)) {
                                    let mut assignments = Vec::new();
                                    if collect_pattern_assignment_exprs(
                                        &decl.name,
                                        (**init).clone(),
                                        0,
                                        &mut assignments,
                                    ) {
                                        let mut supported = true;
                                        for (name, rhs) in assignments {
                                            if let PatternAssignment::Expr(init) = rhs {
                                                lowered_actions.push(ExecuteAction::AssignVar {
                                                    name,
                                                    init,
                                                    export_name: None,
                                                });
                                            } else {
                                                supported = false;
                                                break;
                                            }
                                        }
                                        if supported {
                                            continue;
                                        }
                                    }
                                }
                            }
                            all_lowered = false;
                        }
                        if all_lowered && !lowered_actions.is_empty() {
                            execute_actions.extend(lowered_actions);
                        } else {
                            execute_actions.push(ExecuteAction::EmitStmt(stmt.clone()));
                        }
                        continue;
                    }

                    for decl in &var_decl.declarations {
                        let mut names = Vec::new();
                        collect_binding_names(&decl.name, &mut names);
                        if names.is_empty() {
                            if let Some(init) = decl.init.clone() {
                                let mut preview = Vec::new();
                                if !collect_pattern_assignment_exprs(
                                    &decl.name,
                                    ident_expr("__system_pattern_source"),
                                    0,
                                    &mut preview,
                                ) {
                                    return false;
                                }
                                let temp_name = self.next_omitted_array_temp();
                                add_var_name(&temp_name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    &temp_name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                if preview.is_empty() {
                                    execute_actions.push(ExecuteAction::AssignVar {
                                        name: temp_name,
                                        init,
                                        export_name: None,
                                    });
                                } else {
                                    let mut assignments = Vec::new();
                                    if !collect_pattern_assignment_exprs(
                                        &decl.name,
                                        ident_expr(&temp_name),
                                        0,
                                        &mut assignments,
                                    ) {
                                        return false;
                                    }
                                    allocate_pattern_default_temps(
                                        self,
                                        &mut assignments,
                                        &mut var_names,
                                        &mut seen_var_names,
                                    );
                                    execute_actions.push(ExecuteAction::EmitExportPattern {
                                        temp_name,
                                        init,
                                        assignments,
                                    });
                                }
                            }
                            continue;
                        }
                        let Some(init) = decl.init.clone() else {
                            for name in names {
                                add_var_name(&name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    &name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                add_local_export_name(
                                    &name,
                                    &mut local_export_names,
                                    &mut seen_local_export_names,
                                );
                            }
                            continue;
                        };

                        if names.len() == 1 {
                            let name = names[0].clone();
                            add_var_name(&name, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                &name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            add_local_export_name(
                                &name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            let mut assignments = Vec::new();
                            if !collect_pattern_assignment_exprs(
                                &decl.name,
                                (*init.clone()).clone(),
                                0,
                                &mut assignments,
                            ) {
                                return false;
                            }
                            let direct = if assignments.len() == 1 {
                                let (name, rhs) = assignments.remove(0);
                                if let PatternAssignment::Expr(rhs) = rhs {
                                    Some((name, rhs))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if let Some((name, rhs)) = direct {
                                execute_actions.push(ExecuteAction::AssignVar {
                                    name: name.clone(),
                                    init: rhs,
                                    export_name: Some(name),
                                });
                            } else {
                                let temp_name = self.next_omitted_array_temp();
                                add_var_name(&temp_name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    &temp_name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                assignments.clear();
                                if !collect_pattern_assignment_exprs(
                                    &decl.name,
                                    ident_expr(&temp_name),
                                    0,
                                    &mut assignments,
                                ) {
                                    return false;
                                }
                                allocate_pattern_default_temps(
                                    self,
                                    &mut assignments,
                                    &mut var_names,
                                    &mut seen_var_names,
                                );
                                execute_actions.push(ExecuteAction::EmitExportPattern {
                                    temp_name,
                                    init,
                                    assignments,
                                });
                            }
                            continue;
                        }

                        let mut preview = Vec::new();
                        if !collect_pattern_assignment_exprs(
                            &decl.name,
                            ident_expr("__system_pattern_source"),
                            0,
                            &mut preview,
                        ) {
                            return false;
                        }
                        let temp_name = self.next_omitted_array_temp();
                        add_var_name(&temp_name, &mut var_names, &mut seen_var_names);
                        register_local_symbol(
                            &temp_name,
                            &mut local_symbol_order,
                            &mut next_local_symbol_order,
                        );
                        for name in &names {
                            add_var_name(name, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            add_local_export_name(
                                name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                        }
                        let mut assignments = Vec::new();
                        if !collect_pattern_assignment_exprs(
                            &decl.name,
                            ident_expr(&temp_name),
                            0,
                            &mut assignments,
                        ) {
                            return false;
                        }
                        allocate_pattern_default_temps(
                            self,
                            &mut assignments,
                            &mut var_names,
                            &mut seen_var_names,
                        );
                        execute_actions.push(ExecuteAction::EmitExportPattern {
                            temp_name,
                            init,
                            assignments,
                        });
                    }
                }
                StmtKind::FnDecl(fn_decl) => {
                    if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                        continue;
                    }
                    let mut emit_fn = *fn_decl.clone();
                    emit_fn.modifiers &= !MOD_EXPORT;
                    if let Some(name) = emit_fn.name.clone() {
                        register_local_symbol(
                            &name,
                            &mut local_symbol_order,
                            &mut next_local_symbol_order,
                        );
                        prelude_fn_names.insert(name);
                    }
                    prelude_fn_decls.push(emit_fn);
                    prelude_fn_decl_spans.push(stmt.span);
                }
                StmtKind::ClassDecl(class_decl) => {
                    if class_decl.modifiers & MOD_DECLARE != 0 {
                        continue;
                    }
                    let Some(name) = class_decl.name.clone() else {
                        return false;
                    };
                    // For decorated classes with self-references, add the _1 alias
                    // to the var declaration list (e.g. `var Testing123_1, Testing123;`).
                    let decorator_alias = if self.options.experimental_decorators == Some(true)
                        && class_needs_let_wrapper(class_decl)
                        && decorated_class_has_self_reference(class_decl)
                    {
                        let counter = self
                            .decorated_alias_emit_counter
                            .entry(name.clone().into())
                            .or_insert(0);
                        *counter += 1;
                        let alias = format!("{}_{}", name, counter);
                        add_var_name(&alias, &mut var_names, &mut seen_var_names);
                        Some(alias)
                    } else {
                        None
                    };
                    add_var_name(&name, &mut var_names, &mut seen_var_names);
                    register_local_symbol(
                        &name,
                        &mut local_symbol_order,
                        &mut next_local_symbol_order,
                    );
                    let mut class_expr = *class_decl.clone();
                    class_expr.modifiers &= !MOD_EXPORT;
                    execute_actions.push(ExecuteAction::AssignClass {
                        name,
                        class_decl: class_expr,
                        export_name: None,
                        decorator_alias,
                    });
                }
                StmtKind::ModuleDecl(module_decl) => {
                    if module_decl.modifiers & MOD_DECLARE != 0 {
                        continue;
                    }
                    if let ModuleName::Ident(name) = &module_decl.name {
                        decl_inline_export_names.insert(name.clone());
                        if !prelude_fn_names.contains(name.as_str()) {
                            add_var_name(name, &mut var_names, &mut seen_var_names);
                        }
                        register_local_symbol(
                            name,
                            &mut local_symbol_order,
                            &mut next_local_symbol_order,
                        );
                        if module_decl.modifiers & MOD_EXPORT != 0 {
                            add_local_export_name(
                                name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            let aliases = inline_export_aliases.entry(name.clone()).or_default();
                            if !aliases.contains(name) {
                                aliases.push(name.clone());
                            }
                        }
                    }
                    execute_actions.push(ExecuteAction::EmitStmt(stmt.clone()));
                }
                StmtKind::EnumDecl(enum_decl) => {
                    if enum_decl.modifiers & MOD_DECLARE != 0 {
                        continue;
                    }
                    decl_inline_export_names.insert(enum_decl.name.clone());
                    add_var_name(&enum_decl.name, &mut var_names, &mut seen_var_names);
                    register_local_symbol(
                        &enum_decl.name,
                        &mut local_symbol_order,
                        &mut next_local_symbol_order,
                    );
                    if enum_decl.modifiers & MOD_EXPORT != 0 {
                        add_local_export_name(
                            &enum_decl.name,
                            &mut local_export_names,
                            &mut seen_local_export_names,
                        );
                        let aliases = inline_export_aliases
                            .entry(enum_decl.name.clone())
                            .or_default();
                        if !aliases.contains(&enum_decl.name) {
                            aliases.push(enum_decl.name.clone());
                        }
                    }
                    execute_actions.push(ExecuteAction::EmitStmt(stmt.clone()));
                }
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::All {
                        source,
                        alias: None,
                        type_only: false,
                        ..
                    } => {
                        let dep_idx =
                            ensure_dep(source, None, &mut system_deps, &mut dep_index_by_source);
                        if !system_deps[dep_idx]
                            .ops
                            .iter()
                            .any(|op| matches!(op, SystemDepOp::ExportStar))
                        {
                            system_deps[dep_idx].ops.push(SystemDepOp::ExportStar);
                        }
                    }
                    ExportDeclKind::Decl(decl_stmt) => match &decl_stmt.kind {
                        StmtKind::FnDecl(fn_decl) => {
                            if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                                continue;
                            }
                            let Some(name) = fn_decl.name.clone() else {
                                return false;
                            };
                            let mut emit_fn = *fn_decl.clone();
                            emit_fn.modifiers &= !MOD_EXPORT;
                            if prelude_fn_names.insert(name.clone()) {
                                prelude_fn_decls.push(emit_fn);
                                prelude_fn_decl_spans.push(stmt.span);
                            }
                            register_local_symbol(
                                &name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            add_local_export_name(
                                &name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            add_prelude_export_call(
                                &name,
                                &name,
                                &mut prelude_export_calls,
                                &mut seen_prelude_export_calls,
                            );
                        }
                        StmtKind::ClassDecl(class_decl) => {
                            if class_decl.modifiers & MOD_DECLARE != 0 {
                                continue;
                            }
                            let Some(name) = class_decl.name.clone() else {
                                return false;
                            };
                            // Add decorator alias BEFORE class name for correct var ordering
                            let dec_alias = if self.options.experimental_decorators == Some(true)
                                && class_needs_let_wrapper(class_decl)
                                && decorated_class_has_self_reference(class_decl)
                            {
                                let counter = self
                                    .decorated_alias_emit_counter
                                    .entry(name.clone().into())
                                    .or_insert(0);
                                *counter += 1;
                                let alias = format!("{}_{}", name, counter);
                                add_var_name(&alias, &mut var_names, &mut seen_var_names);
                                Some(alias)
                            } else {
                                None
                            };
                            add_var_name(&name, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                &name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            add_local_export_name(
                                &name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            let mut class_expr = *class_decl.clone();
                            class_expr.modifiers &= !MOD_EXPORT;
                            execute_actions.push(ExecuteAction::AssignClass {
                                name: name.clone(),
                                class_decl: class_expr,
                                export_name: Some(name),
                                decorator_alias: dec_alias,
                            });
                        }
                        StmtKind::EnumDecl(enum_decl) => {
                            if enum_decl.modifiers & MOD_DECLARE != 0 {
                                continue;
                            }
                            decl_inline_export_names.insert(enum_decl.name.clone());
                            add_var_name(&enum_decl.name, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                &enum_decl.name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            execute_actions.push(ExecuteAction::EmitStmt(*decl_stmt.clone()));
                            add_local_export_name(
                                &enum_decl.name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            let aliases = inline_export_aliases
                                .entry(enum_decl.name.clone())
                                .or_default();
                            if !aliases.contains(&enum_decl.name) {
                                aliases.push(enum_decl.name.clone());
                            }
                        }
                        StmtKind::ModuleDecl(module_decl) => {
                            if module_decl.modifiers & MOD_DECLARE != 0 {
                                continue;
                            }
                            execute_actions.push(ExecuteAction::EmitStmt(*decl_stmt.clone()));
                            if let ModuleName::Ident(name) = &module_decl.name {
                                decl_inline_export_names.insert(name.clone());
                                if !prelude_fn_names.contains(name.as_str()) {
                                    add_var_name(name, &mut var_names, &mut seen_var_names);
                                }
                                register_local_symbol(
                                    name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                add_local_export_name(
                                    name,
                                    &mut local_export_names,
                                    &mut seen_local_export_names,
                                );
                                let aliases =
                                    inline_export_aliases.entry(name.clone()).or_default();
                                if !aliases.contains(name) {
                                    aliases.push(name.clone());
                                }
                            }
                        }
                        StmtKind::ImportEquals(ie) => {
                            if expr_refers_known_type_only(
                                &ie.module_ref,
                                &self.type_only_decl_names,
                            ) {
                                continue;
                            }
                            add_var_name(&ie.name, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                &ie.name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            add_local_export_name(
                                &ie.name,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            execute_actions.push(ExecuteAction::AssignVar {
                                name: ie.name.clone(),
                                init: ie.module_ref.clone(),
                                export_name: Some(ie.name.clone()),
                            });
                        }
                        StmtKind::Import(import_decl) => {
                            // Handle `export import X = require("source")` which
                            // is parsed as Export > Decl > Import(Require).
                            // Since we're inside an ExportDeclKind::Decl, the import
                            // is explicitly exported and should never be elided.
                            if let ImportClause::Require(local) = &import_decl.specifiers {
                                if !import_decl.type_only {
                                    add_var_name(local, &mut var_names, &mut seen_var_names);
                                    register_local_symbol(
                                        local,
                                        &mut local_symbol_order,
                                        &mut next_local_symbol_order,
                                    );
                                    let dep_idx = ensure_dep(
                                        &import_decl.source,
                                        Some(format!("{}_1", local)),
                                        &mut system_deps,
                                        &mut dep_index_by_source,
                                    );
                                    let assign_exists = system_deps[dep_idx].ops.iter().any(|op| {
                                        matches!(op, SystemDepOp::AssignVar(existing) | SystemDepOp::AssignVarAndExport(existing, _) if existing == local)
                                    });
                                    if !assign_exists {
                                        system_deps[dep_idx].ops.push(
                                            SystemDepOp::AssignVarAndExport(
                                                local.clone(),
                                                local.clone(),
                                            ),
                                        );
                                    }
                                    self.cjs_import_map.insert(
                                        local.clone().into(),
                                        (local.clone().into(), AstString::default()),
                                    );
                                }
                            }
                        }
                        StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => {}
                        StmtKind::Var(var_decl) => {
                            if let Some(init) = self
                                .direct_es5_empty_binding_export_init(var_decl, stmt.span)
                                .cloned()
                            {
                                let value_temp = self.make_temp_name();
                                let export_name = self.next_deferred_export_name_placeholder();
                                add_var_name(&value_temp, &mut var_names, &mut seen_var_names);
                                system_transform_temp_names.push(value_temp.clone());
                                register_local_symbol(
                                    &value_temp,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                deferred_system_export_names.push(export_name.clone());
                                register_local_symbol(
                                    &export_name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                add_var_name(&export_name, &mut var_names, &mut seen_var_names);
                                add_local_export_name(
                                    &export_name,
                                    &mut local_export_names,
                                    &mut seen_local_export_names,
                                );
                                execute_actions.push(ExecuteAction::ExportEmptyBinding {
                                    value_temp,
                                    export_name,
                                    init: Box::new(init),
                                });
                                continue;
                            }
                            if let Some((init, binding_name, binding_span, rest_name, rest_span)) =
                                self.direct_es5_object_rest_export(var_decl, stmt.span)
                            {
                                let temp_name = self.make_temp_name();
                                add_var_name(&temp_name, &mut var_names, &mut seen_var_names);
                                system_transform_temp_names.push(temp_name.clone());
                                register_local_symbol(
                                    &temp_name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                for name in [&binding_name, &rest_name] {
                                    add_var_name(name, &mut var_names, &mut seen_var_names);
                                    register_local_symbol(
                                        name,
                                        &mut local_symbol_order,
                                        &mut next_local_symbol_order,
                                    );
                                    add_local_export_name(
                                        name,
                                        &mut local_export_names,
                                        &mut seen_local_export_names,
                                    );
                                }
                                execute_actions.push(ExecuteAction::ExportObjectRestEs5 {
                                    temp_name,
                                    init,
                                    binding_name: binding_name.to_string(),
                                    binding_span,
                                    rest_name: rest_name.to_string(),
                                    rest_span,
                                });
                                continue;
                            }
                            let all_ident = var_decl
                                .declarations
                                .iter()
                                .all(|decl| matches!(decl.name.kind, PatKind::Ident(_)));
                            if all_ident {
                                let var_actions_before = execute_actions.len();
                                for decl in &var_decl.declarations {
                                    let PatKind::Ident(name) = &decl.name.kind else {
                                        continue;
                                    };
                                    add_var_decl(
                                        name,
                                        &decl.init,
                                        true,
                                        &mut var_names,
                                        &mut seen_var_names,
                                        &mut local_export_names,
                                        &mut seen_local_export_names,
                                        &mut execute_actions,
                                        &mut local_symbol_order,
                                        &mut next_local_symbol_order,
                                    );
                                }
                                // Preserve leading comments from the original
                                // export statement on the first emitted action.
                                if execute_actions.len() > var_actions_before {
                                    execute_actions.insert(
                                        var_actions_before,
                                        ExecuteAction::EmitLeadingComments(stmt.span.start),
                                    );
                                }
                                continue;
                            }

                            for decl in &var_decl.declarations {
                                let mut names = Vec::new();
                                collect_binding_names(&decl.name, &mut names);
                                if names.is_empty() {
                                    if let Some(init) = decl.init.clone() {
                                        let mut preview = Vec::new();
                                        if !collect_pattern_assignment_exprs(
                                            &decl.name,
                                            ident_expr("__system_pattern_source"),
                                            0,
                                            &mut preview,
                                        ) {
                                            return false;
                                        }
                                        let temp_name = self.next_omitted_array_temp();
                                        add_var_name(
                                            &temp_name,
                                            &mut var_names,
                                            &mut seen_var_names,
                                        );
                                        register_local_symbol(
                                            &temp_name,
                                            &mut local_symbol_order,
                                            &mut next_local_symbol_order,
                                        );
                                        if preview.is_empty() {
                                            execute_actions.push(ExecuteAction::AssignVar {
                                                name: temp_name,
                                                init,
                                                export_name: None,
                                            });
                                        } else {
                                            let mut assignments = Vec::new();
                                            if !collect_pattern_assignment_exprs(
                                                &decl.name,
                                                ident_expr(&temp_name),
                                                0,
                                                &mut assignments,
                                            ) {
                                                return false;
                                            }
                                            allocate_pattern_default_temps(
                                                self,
                                                &mut assignments,
                                                &mut var_names,
                                                &mut seen_var_names,
                                            );
                                            execute_actions.push(
                                                ExecuteAction::EmitExportPattern {
                                                    temp_name,
                                                    init,
                                                    assignments,
                                                },
                                            );
                                        }
                                    }
                                    continue;
                                }
                                let Some(init) = decl.init.clone() else {
                                    for name in names {
                                        add_var_name(&name, &mut var_names, &mut seen_var_names);
                                        register_local_symbol(
                                            &name,
                                            &mut local_symbol_order,
                                            &mut next_local_symbol_order,
                                        );
                                        add_local_export_name(
                                            &name,
                                            &mut local_export_names,
                                            &mut seen_local_export_names,
                                        );
                                    }
                                    continue;
                                };

                                if names.len() == 1 {
                                    let name = names[0].clone();
                                    add_var_name(&name, &mut var_names, &mut seen_var_names);
                                    register_local_symbol(
                                        &name,
                                        &mut local_symbol_order,
                                        &mut next_local_symbol_order,
                                    );
                                    add_local_export_name(
                                        &name,
                                        &mut local_export_names,
                                        &mut seen_local_export_names,
                                    );
                                    let mut assignments = Vec::new();
                                    if !collect_pattern_assignment_exprs(
                                        &decl.name,
                                        (*init.clone()).clone(),
                                        0,
                                        &mut assignments,
                                    ) {
                                        return false;
                                    }
                                    let direct = if assignments.len() == 1 {
                                        let (name, rhs) = assignments.remove(0);
                                        if let PatternAssignment::Expr(rhs) = rhs {
                                            Some((name, rhs))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };
                                    if let Some((name, rhs)) = direct {
                                        execute_actions.push(ExecuteAction::AssignVar {
                                            name: name.clone(),
                                            init: rhs,
                                            export_name: Some(name),
                                        });
                                    } else {
                                        let temp_name = self.next_omitted_array_temp();
                                        add_var_name(
                                            &temp_name,
                                            &mut var_names,
                                            &mut seen_var_names,
                                        );
                                        register_local_symbol(
                                            &temp_name,
                                            &mut local_symbol_order,
                                            &mut next_local_symbol_order,
                                        );
                                        assignments.clear();
                                        if !collect_pattern_assignment_exprs(
                                            &decl.name,
                                            ident_expr(&temp_name),
                                            0,
                                            &mut assignments,
                                        ) {
                                            return false;
                                        }
                                        allocate_pattern_default_temps(
                                            self,
                                            &mut assignments,
                                            &mut var_names,
                                            &mut seen_var_names,
                                        );
                                        execute_actions.push(ExecuteAction::EmitExportPattern {
                                            temp_name,
                                            init,
                                            assignments,
                                        });
                                    }
                                    continue;
                                }

                                let mut preview = Vec::new();
                                if !collect_pattern_assignment_exprs(
                                    &decl.name,
                                    ident_expr("__system_pattern_source"),
                                    0,
                                    &mut preview,
                                ) {
                                    return false;
                                }
                                let temp_name = self.next_omitted_array_temp();
                                add_var_name(&temp_name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    &temp_name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                                for name in &names {
                                    add_var_name(name, &mut var_names, &mut seen_var_names);
                                    register_local_symbol(
                                        name,
                                        &mut local_symbol_order,
                                        &mut next_local_symbol_order,
                                    );
                                    add_local_export_name(
                                        name,
                                        &mut local_export_names,
                                        &mut seen_local_export_names,
                                    );
                                }
                                let mut assignments = Vec::new();
                                if !collect_pattern_assignment_exprs(
                                    &decl.name,
                                    ident_expr(&temp_name),
                                    0,
                                    &mut assignments,
                                ) {
                                    return false;
                                }
                                allocate_pattern_default_temps(
                                    self,
                                    &mut assignments,
                                    &mut var_names,
                                    &mut seen_var_names,
                                );
                                execute_actions.push(ExecuteAction::EmitExportPattern {
                                    temp_name,
                                    init,
                                    assignments,
                                });
                            }
                        }
                        _ => return false,
                    },
                    ExportDeclKind::DefaultDecl(decl_stmt) => match &decl_stmt.kind {
                        StmtKind::FnDecl(fn_decl) => {
                            if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                                continue;
                            }
                            let local_name = if let Some(name) = fn_decl.name.clone() {
                                name
                            } else {
                                let name = format!("default_{}", default_export_counter);
                                default_export_counter += 1;
                                name
                            };
                            let mut emit_fn = *fn_decl.clone();
                            emit_fn.modifiers &= !MOD_EXPORT;
                            emit_fn.name = Some(local_name.clone());
                            if prelude_fn_names.insert(local_name.clone()) {
                                prelude_fn_decls.push(emit_fn);
                                prelude_fn_decl_spans.push(stmt.span);
                            }
                            register_local_symbol(
                                &local_name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            add_prelude_export_call(
                                "default",
                                &local_name,
                                &mut prelude_export_calls,
                                &mut seen_prelude_export_calls,
                            );
                        }
                        StmtKind::ClassDecl(class_decl) => {
                            if class_decl.modifiers & MOD_DECLARE != 0 {
                                continue;
                            }
                            let local_name = class_decl.name.clone().unwrap_or_else(|| {
                                let name = format!("default_{}", default_export_counter);
                                default_export_counter += 1;
                                name
                            });
                            add_var_name(&local_name, &mut var_names, &mut seen_var_names);
                            register_local_symbol(
                                &local_name,
                                &mut local_symbol_order,
                                &mut next_local_symbol_order,
                            );
                            let mut class_expr = *class_decl.clone();
                            class_expr.modifiers &= !MOD_EXPORT;
                            execute_actions.push(ExecuteAction::AssignClass {
                                name: local_name,
                                class_decl: class_expr,
                                export_name: Some("default".to_string()),
                                decorator_alias: None,
                            });
                        }
                        _ => return false,
                    },
                    ExportDeclKind::Default(expr) => {
                        execute_actions
                            .push(ExecuteAction::EmitDefaultExportExpr((**expr).clone()));
                    }
                    ExportDeclKind::Named {
                        specifiers,
                        source: None,
                        type_only: false,
                    } => {
                        for spec in specifiers.iter().filter(|s| !s.is_type) {
                            let local = spec.local.as_str();
                            if self.type_only_decl_names.contains(local)
                                && !self.cjs_import_map.contains_key(local)
                            {
                                continue;
                            }
                            if let Some((_, imported)) = self.cjs_import_map.get(local) {
                                if !imported.is_empty()
                                    && self.type_only_decl_names.contains(imported.as_str())
                                {
                                    continue;
                                }
                            }
                            let has_runtime_local = seen_var_names.contains(local)
                                || all_hoisted_var_names.contains(local)
                                || prelude_fn_names.contains(local)
                                || self.cjs_import_map.contains_key(local);
                            if !has_runtime_local {
                                continue;
                            }
                            let exported = spec.exported.as_ref().unwrap_or(&spec.local).clone();
                            add_local_export_name(
                                &exported,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            if decl_inline_export_names.contains(local) {
                                let aliases =
                                    inline_export_aliases.entry(spec.local.clone()).or_default();
                                if !aliases.contains(&exported) {
                                    aliases.push(exported);
                                }
                                continue;
                            }
                            let assign_idx = execute_actions.iter().position(|action| {
                                matches!(
                                    action,
                                    ExecuteAction::AssignVar { name, .. } if name == local
                                )
                            });
                            if let Some(idx) = assign_idx {
                                let mut already_inline_export = false;
                                if let ExecuteAction::AssignVar { export_name, .. } =
                                    &execute_actions[idx]
                                {
                                    if let Some(existing) = export_name {
                                        already_inline_export = existing == &exported;
                                    }
                                }
                                if !already_inline_export {
                                    let key = (exported.clone(), spec.local.clone());
                                    if seen_execute_named_exports.insert(key.clone()) {
                                        let local_order = local_symbol_order
                                            .get(local)
                                            .copied()
                                            .unwrap_or(usize::MAX);
                                        execute_actions.insert(
                                            idx + 1,
                                            ExecuteAction::EmitNamedExport {
                                                exported: key.0,
                                                local: key.1,
                                                local_order,
                                            },
                                        );
                                    }
                                }
                                continue;
                            }
                            let assign_class_idx = execute_actions.iter().position(|action| {
                                matches!(
                                    action,
                                    ExecuteAction::AssignClass { name, .. } if name == local
                                )
                            });
                            if let Some(idx) = assign_class_idx {
                                let key = (exported.clone(), spec.local.clone());
                                if seen_execute_named_exports.insert(key.clone()) {
                                    let local_order = local_symbol_order
                                        .get(local)
                                        .copied()
                                        .unwrap_or(usize::MAX);
                                    execute_actions.insert(
                                        idx + 1,
                                        ExecuteAction::EmitNamedExport {
                                            exported: key.0,
                                            local: key.1,
                                            local_order,
                                        },
                                    );
                                }
                                continue;
                            }
                            if prelude_fn_names.contains(spec.local.as_str()) {
                                add_prelude_export_call(
                                    &exported,
                                    &spec.local,
                                    &mut prelude_export_calls,
                                    &mut seen_prelude_export_calls,
                                );
                            } else if self.cjs_import_map.contains_key(local) {
                                add_execute_named_export(
                                    &exported,
                                    &spec.local,
                                    &mut execute_actions,
                                    &mut seen_execute_named_exports,
                                    &local_symbol_order,
                                );
                            }
                        }
                    }
                    ExportDeclKind::Named {
                        specifiers,
                        source: Some(source),
                        type_only: false,
                    } => {
                        let dep_idx =
                            ensure_dep(source, None, &mut system_deps, &mut dep_index_by_source);
                        let mut re_export_group: Vec<(String, String)> = Vec::new();
                        for spec in specifiers.iter().filter(|s| !s.is_type) {
                            let exported = spec.exported.as_ref().unwrap_or(&spec.local).clone();
                            add_local_export_name(
                                &exported,
                                &mut local_export_names,
                                &mut seen_local_export_names,
                            );
                            let re_export = (exported, spec.local.clone());
                            if !re_export_group.contains(&re_export) {
                                re_export_group.push(re_export);
                            }
                        }
                        if !re_export_group.is_empty() {
                            system_deps[dep_idx]
                                .ops
                                .push(SystemDepOp::ReExport(re_export_group));
                        }
                    }
                    _ => return false,
                },
                // Type-only declarations are erased.
                StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => {}
                StmtKind::ForIn(fi) => {
                    let mut emit_stmt = stmt.clone();
                    if let ForInOfLeft::Var(vs) = &fi.left {
                        if vs.kind == VarKind::Var {
                            if vs.declarations.len() != 1 {
                                return false;
                            }
                            for decl in &vs.declarations {
                                let PatKind::Ident(name) = &decl.name.kind else {
                                    return false;
                                };
                                add_var_name(name, &mut var_names, &mut seen_var_names);
                                register_local_symbol(
                                    name,
                                    &mut local_symbol_order,
                                    &mut next_local_symbol_order,
                                );
                            }
                            if let StmtKind::ForIn(ref mut emit_fi) = emit_stmt.kind {
                                emit_fi.left = ForInOfLeft::Pat(vs.declarations[0].name.clone());
                            }
                        }
                    }
                    execute_actions.push(ExecuteAction::EmitStmt(emit_stmt));
                }
                _ => execute_actions.push(ExecuteAction::EmitStmt(stmt.clone())),
            }

            // If this statement produced new non-EmitStmt actions (e.g. AssignVar
            // from decomposed Var declarations), insert a leading-comment marker
            // so comments attached to the original statement are preserved.
            if execute_actions.len() > actions_len_before {
                let first_new = &execute_actions[actions_len_before];
                let needs_comments = !matches!(
                    first_new,
                    ExecuteAction::EmitStmt(_) | ExecuteAction::EmitNamedExport { .. }
                );
                if needs_comments {
                    execute_actions.insert(
                        actions_len_before,
                        ExecuteAction::EmitLeadingComments(stmt.span.start),
                    );
                }
            }
        }

        // System execute runs in function scope, so nested `var` declarations
        // in emitted statements must be hoisted to the prelude declaration list.
        for action in &execute_actions {
            if let ExecuteAction::EmitStmt(stmt) = action {
                collect_function_scope_var_names(stmt, &mut var_names, &mut seen_var_names);
            }
        }

        // Insert `_default` and `env_N` vars for disposal scopes.
        // `_default` goes right before the first directly-exported var that
        // appears after the disposal scope in the action list.
        // `env_N` goes at the very end.
        {
            let mut has_disposal = false;
            let mut needs_default = false;
            let mut disposal_env_names: Vec<String> = Vec::new();
            let mut in_disposal_scope = false;
            let mut first_export_var_after_disposal: Option<String> = None;

            for action in &execute_actions {
                match action {
                    ExecuteAction::UsingDisposalStart { env_name, .. } => {
                        has_disposal = true;
                        in_disposal_scope = true;
                        disposal_env_names.push(env_name.clone());
                    }
                    ExecuteAction::EmitDefaultExportExpr(_) if in_disposal_scope => {
                        needs_default = true;
                    }
                    ExecuteAction::AssignVar {
                        name,
                        export_name: Some(_),
                        ..
                    } if in_disposal_scope && first_export_var_after_disposal.is_none() => {
                        first_export_var_after_disposal = Some(name.clone());
                    }
                    _ => {}
                }
            }

            if has_disposal && needs_default {
                if let Some(ref export_var) = first_export_var_after_disposal {
                    // Insert _default right before the first exported var after disposal
                    if let Some(pos) = var_names.iter().position(|n| n == export_var) {
                        var_names.insert(pos, "_default".to_string());
                    } else {
                        var_names.push("_default".to_string());
                    }
                } else {
                    var_names.push("_default".to_string());
                }
                seen_var_names.insert("_default".to_string());
            }

            for env_name in disposal_env_names {
                if seen_var_names.insert(env_name.clone()) {
                    var_names.push(env_name);
                }
            }
        }

        // Value temps for empty and object-rest exports are ordinary transform
        // temps and lead source-declared bindings. Synthetic empty-export
        // bindings stay at their source-order slot; late expression temps are
        // inserted at the same generated-name boundary before either kind.
        let deferred_system_export_set = deferred_system_export_names
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        if !deferred_system_export_names.is_empty() {
            let transform_temp_set = system_transform_temp_names
                .iter()
                .cloned()
                .collect::<HashSet<_>>();
            var_names.retain(|name| !transform_temp_set.contains(name));
            let insert_at = var_names
                .iter()
                .position(|name| {
                    deferred_system_export_set.contains(name) || self.source_has_identifier(name)
                })
                .unwrap_or(var_names.len());
            for (offset, name) in system_transform_temp_names.iter().cloned().enumerate() {
                var_names.insert(insert_at + offset, name);
            }
        }

        for name in &var_names {
            self.emitted_var_names.insert(name.clone().into());
        }

        let system_module_name = self.find_amd_module_name(file);
        let sys_n = self.system_register_counter;
        let exports_fn = format!("exports_{}", sys_n);
        let context_var = format!("context_{}", sys_n);
        // Store on self so emit_expr / emit_declarations can use it.
        self.system_exports_fn = exports_fn.clone();
        self.system_context_fn = context_var.clone();
        // Increment counter for the next System.register call (outFile bundles).
        self.system_register_counter += 1;

        // In outFile bundles, strip leading "./" from dependency specifiers.
        let is_out_file_bundle = self.options.out_file.is_some();
        if is_out_file_bundle {
            for dep in system_deps.iter_mut() {
                if let Some(stripped) = dep.source.strip_prefix("./") {
                    dep.source = stripped.to_string();
                }
            }
        }

        self.write("System.register(");
        if let Some(name) = system_module_name {
            self.write("\"");
            self.write(&name);
            self.write("\", ");
        }
        self.write("[");
        for (idx, dep) in system_deps.iter().enumerate() {
            if idx > 0 {
                self.write(", ");
            }
            self.write("\"");
            self.write(&dep.source);
            self.write("\"");
        }
        self.write("], function (");
        self.write(&exports_fn);
        self.write(", ");
        self.write(&context_var);
        self.writeln(") {");
        self.indent += 1;
        self.module_wrapper_indent += 1;
        self.writeln("\"use strict\";");

        // Emit transform helpers needed inside the System.register callback.
        if self.options.no_emit_helpers != Some(true) && self.options.import_helpers != Some(true) {
            if self.needs_extends_helper {
                self.emit_extends_helper();
            }
            // Decorator helpers (__decorate, __metadata, __param)
            if self.needs_decorate_helper {
                self.emit_decorate_helper();
            }
            if self.needs_metadata_helper {
                self.emit_metadata_helper();
            }
            if self.needs_param_helper {
                self.emit_param_helper();
            }
            // Async helpers (__awaiter, etc.)
            self.scan_needs_awaiter(&file.statements);
            if self.needs_awaiter_helper {
                self.emit_awaiter_helper();
            }
            if self.needs_generator_helper {
                self.emit_generator_helper();
            }
            // using/await using disposal helpers
            if self.needs_add_disposable_resource_helper {
                self.emit_add_disposable_resource_helper();
            }
            if self.needs_dispose_resources_helper {
                self.emit_dispose_resources_helper();
            }
        }

        // Filter out <error> placeholder names from the hoisted var list.
        var_names.retain(|n| n != "<error>");
        let system_temp_start = self.temp_var_names.len();
        let system_resolved_deferred_start = self.resolved_deferred_temp_names.len();
        let mut late_system_temp_insert_pos = None;
        if !var_names.is_empty() {
            self.write("var ");
            for (idx, name) in var_names.iter().enumerate() {
                if idx > 0 {
                    self.write(", ");
                }
                if late_system_temp_insert_pos.is_none()
                    && (deferred_system_export_set.contains(name)
                        || self.source_has_identifier(name))
                {
                    late_system_temp_insert_pos = Some(self.output.len());
                }
                self.write(name);
            }
            self.writeln(";");
        }

        self.write("var __moduleName = ");
        self.write(&context_var);
        self.write(" && ");
        self.write(&context_var);
        self.writeln(".id;");
        let prev_rewrite_ident_with_import_map = self.rewrite_ident_with_import_map;
        let prev_system_inline_export_aliases =
            std::mem::take(&mut self.system_inline_export_aliases);
        let prev_system_live_export_names = std::mem::take(&mut self.system_live_export_names);
        self.rewrite_ident_with_import_map = true;
        self.system_inline_export_aliases = inline_export_aliases;
        self.system_live_export_names = local_export_names
            .iter()
            .map(|s| AstString::from(s.as_str()))
            .collect();

        let mut emitted_prelude_export_calls: HashSet<(String, String)> = HashSet::new();
        for (idx, fn_decl) in prelude_fn_decls.iter().enumerate() {
            if let Some(span) = prelude_fn_decl_spans.get(idx) {
                self.emit_leading_comments(span.start);
            }
            self.emit_fn_decl(fn_decl);
            if let Some(span) = prelude_fn_decl_spans.get(idx) {
                self.advance_comment_pos(span.end);
            }
            if let Some(local_name) = fn_decl.name.as_ref() {
                for (exported, local) in &prelude_export_calls {
                    if local != local_name {
                        continue;
                    }
                    self.write(&exports_fn);
                    self.write("(\"");
                    self.write(exported);
                    self.write("\", ");
                    self.emit_value_name_ref(local);
                    self.writeln(");");
                    emitted_prelude_export_calls.insert((exported.clone(), local.clone()));
                }
            }
        }
        for (exported, local) in &prelude_export_calls {
            if emitted_prelude_export_calls.contains(&(exported.clone(), local.clone())) {
                continue;
            }
            self.write(&exports_fn);
            self.write("(\"");
            self.write(exported);
            self.write("\", ");
            self.emit_value_name_ref(local);
            self.writeln(");");
        }

        let has_export_star = system_deps.iter().any(|dep| {
            dep.ops
                .iter()
                .any(|op| matches!(op, SystemDepOp::ExportStar))
        });
        let has_local_export_names = !local_export_names.is_empty();
        if has_export_star && has_local_export_names {
            self.writeln("var exportedNames_1 = {");
            self.indent += 1;
            for (idx, name) in local_export_names.iter().enumerate() {
                self.write("\"");
                self.write(name);
                self.write("\": true");
                if idx + 1 < local_export_names.len() {
                    self.writeln(",");
                } else {
                    self.newline();
                }
            }
            self.indent -= 1;
            self.writeln("};");
        }

        if has_export_star {
            self.writeln("function exportStar_1(m) {");
            self.indent += 1;
            self.writeln("var exports = {};");
            self.writeln("for (var n in m) {");
            self.indent += 1;
            if has_local_export_names {
                self.writeln(
                    "if (n !== \"default\" && !exportedNames_1.hasOwnProperty(n)) exports[n] = m[n];",
                );
            } else {
                self.writeln("if (n !== \"default\") exports[n] = m[n];");
            }
            self.indent -= 1;
            self.writeln("}");
            self.write(&exports_fn);
            self.writeln("(exports);");
            self.indent -= 1;
            self.writeln("}");
        }

        self.writeln("return {");
        self.indent += 1;
        if system_deps.is_empty() {
            self.writeln("setters: [],");
        } else {
            self.writeln("setters: [");
            self.indent += 1;
            for (idx, dep) in system_deps.iter().enumerate() {
                self.write("function (");
                if dep.ops.is_empty() {
                    self.write("_1");
                } else {
                    self.write(&dep.param);
                }
                self.writeln(") {");
                self.indent += 1;
                for op in &dep.ops {
                    match op {
                        SystemDepOp::AssignVar(var_name) => {
                            self.write(var_name);
                            self.write(" = ");
                            self.write(&dep.param);
                            self.writeln(";");
                        }
                        SystemDepOp::AssignVarAndExport(var_name, export_name) => {
                            self.write(var_name);
                            self.write(" = ");
                            self.write(&dep.param);
                            self.writeln(";");
                            self.write(&exports_fn);
                            self.write("(\"");
                            self.write(export_name);
                            self.write("\", ");
                            self.write(&dep.param);
                            self.writeln(");");
                        }
                        SystemDepOp::ReExport(re_exports) => {
                            if !re_exports.is_empty() {
                                self.write(&exports_fn);
                                self.writeln("({");
                                self.indent += 1;
                                for (re_idx, (exported, imported)) in re_exports.iter().enumerate()
                                {
                                    self.write("\"");
                                    self.write(exported);
                                    self.write("\": ");
                                    self.write(&dep.param);
                                    self.write("[\"");
                                    self.write(imported);
                                    self.write("\"]");
                                    if re_idx + 1 < re_exports.len() {
                                        self.writeln(",");
                                    } else {
                                        self.newline();
                                    }
                                }
                                self.indent -= 1;
                                self.writeln("});");
                            }
                        }
                        SystemDepOp::ExportStar => {
                            self.write("exportStar_1(");
                            self.write(&dep.param);
                            self.writeln(");");
                        }
                    }
                }
                self.indent -= 1;
                if idx + 1 < system_deps.len() {
                    self.writeln("},");
                } else {
                    self.writeln("}");
                }
            }
            self.indent -= 1;
            self.writeln("],");
        }
        let has_await_disposal = execute_actions
            .iter()
            .any(|a| matches!(a, ExecuteAction::UsingDisposalStart { is_await: true, .. }));
        if has_await_disposal {
            self.writeln("execute: async function () {");
        } else {
            self.writeln("execute: function () {");
        }
        self.indent += 1;
        let mut action_idx = 0usize;
        while action_idx < execute_actions.len() {
            if matches!(
                execute_actions[action_idx],
                ExecuteAction::EmitNamedExport { .. }
            ) {
                let run_start = action_idx;
                while action_idx < execute_actions.len()
                    && matches!(
                        execute_actions[action_idx],
                        ExecuteAction::EmitNamedExport { .. }
                    )
                {
                    action_idx += 1;
                }
                let mut run = execute_actions[run_start..action_idx].to_vec();
                run.sort_by(|a, b| {
                    let (a_order, a_name) = match a {
                        ExecuteAction::EmitNamedExport {
                            local_order,
                            exported,
                            ..
                        } => (*local_order, exported.as_str()),
                        _ => (usize::MAX, ""),
                    };
                    let (b_order, b_name) = match b {
                        ExecuteAction::EmitNamedExport {
                            local_order,
                            exported,
                            ..
                        } => (*local_order, exported.as_str()),
                        _ => (usize::MAX, ""),
                    };
                    a_order.cmp(&b_order).then_with(|| a_name.cmp(b_name))
                });
                for action in run {
                    if let ExecuteAction::EmitNamedExport {
                        exported, local, ..
                    } = action
                    {
                        self.write(&exports_fn);
                        self.write("(\"");
                        self.write(&exported);
                        self.write("\", ");
                        self.emit_value_name_ref(&local);
                        self.writeln(");");
                    }
                }
                continue;
            }

            let action = execute_actions[action_idx].clone();
            action_idx += 1;
            match action {
                ExecuteAction::AssignVar {
                    name,
                    init,
                    export_name,
                } => {
                    if let Some(exported) = export_name {
                        self.write(&exports_fn);
                        self.write("(\"");
                        self.write(&exported);
                        self.write("\", ");
                        self.write(&name);
                        self.write(" = ");
                        self.emit_expr(&init);
                        self.writeln(");");
                    } else {
                        self.write(&name);
                        self.write(" = ");
                        self.emit_expr(&init);
                        self.writeln(";");
                    }
                }
                ExecuteAction::ExportEmptyBinding {
                    value_temp,
                    export_name,
                    init,
                } => {
                    self.write(&exports_fn);
                    self.write("(\"");
                    self.write(&export_name);
                    self.write("\", ");
                    self.write(&export_name);
                    self.write(" = ");
                    self.write(&value_temp);
                    self.write(" = ");
                    self.emit_expr(&init);
                    self.writeln(");");
                }
                ExecuteAction::ExportObjectRestEs5 {
                    temp_name,
                    init,
                    binding_name,
                    binding_span,
                    rest_name,
                    rest_span,
                } => {
                    self.record_mapping_with_name(binding_span, &binding_name);
                    self.write(&exports_fn);
                    self.write("(\"");
                    self.write(&binding_name);
                    self.write("\", ");
                    self.write(&binding_name);
                    self.write(" = (");
                    self.write(&temp_name);
                    self.write(" = ");
                    self.emit_expr(&init);
                    self.write(", ");
                    self.write(&temp_name);
                    self.write(").");
                    self.write(&binding_name);
                    self.write("), ");
                    self.record_mapping_with_name(rest_span, &rest_name);
                    self.write(&exports_fn);
                    self.write("(\"");
                    self.write(&rest_name);
                    self.write("\", ");
                    self.write(&rest_name);
                    self.write(" = ");
                    self.write(self.helper_prefix());
                    self.write("__rest(");
                    self.write(&temp_name);
                    self.write(", [\"");
                    self.write(&super::emit_expr::escape_js_string_for_quote(
                        &binding_name,
                        '"',
                    ));
                    self.writeln("\"]));");
                }
                ExecuteAction::EmitExportPattern {
                    temp_name,
                    init,
                    assignments,
                } => {
                    self.write(&temp_name);
                    self.write(" = ");
                    self.emit_expr(&init);
                    for (name, rhs) in assignments {
                        self.write(", ");
                        match &rhs {
                            PatternAssignment::EmptyObject(value) => {
                                self.write("({} = ");
                                self.emit_expr(value);
                                self.write(")");
                                continue;
                            }
                            PatternAssignment::EmptyArray(value) => {
                                self.write("([] = ");
                                self.emit_expr(value);
                                self.write(")");
                                continue;
                            }
                            _ => {}
                        }
                        self.write(&exports_fn);
                        self.write("(\"");
                        self.write(&name);
                        self.write("\", ");
                        self.write(&name);
                        self.write(" = ");
                        match rhs {
                            PatternAssignment::Expr(rhs) => self.emit_expr(&rhs),
                            PatternAssignment::Default {
                                value,
                                initializer,
                                value_temp,
                            } => {
                                let value_temp = value_temp
                                    .as_deref()
                                    .expect("System pattern defaults must allocate a value temp");
                                self.write("(");
                                self.write(value_temp);
                                self.write(" = ");
                                self.emit_expr(&value);
                                self.write(") === void 0 ? ");
                                self.emit_expr(&initializer);
                                self.write(" : ");
                                self.write(value_temp);
                            }
                            PatternAssignment::ObjectRest { base, excluded } => {
                                self.write("__rest(");
                                self.emit_expr(&base);
                                self.write(", [");
                                for (idx, key) in excluded.iter().enumerate() {
                                    if idx > 0 {
                                        self.write(", ");
                                    }
                                    self.write("\"");
                                    self.write(&super::emit_expr::escape_js_string_for_quote(
                                        key, '"',
                                    ));
                                    self.write("\"");
                                }
                                self.write("])");
                            }
                            PatternAssignment::EmptyObject(_)
                            | PatternAssignment::EmptyArray(_) => unreachable!(),
                        }
                        self.write(")");
                    }
                    self.writeln(";");
                }
                ExecuteAction::AssignClass {
                    name,
                    class_decl,
                    export_name,
                    decorator_alias,
                } => {
                    let mut class_expr_decl = class_decl.clone();
                    if !self.use_define_for_class_fields() {
                        for member in &mut class_expr_decl.members {
                            if let ClassMemberKind::Property(prop) = &mut member.kind {
                                if prop.modifiers & MOD_STATIC != 0
                                    && prop.modifiers & MOD_DECLARE == 0
                                    && prop.modifiers & MOD_ABSTRACT == 0
                                {
                                    prop.initializer = None;
                                }
                            }
                        }
                    }
                    // Set up alias mapping for decorated class self-references
                    let saved_import_map_entry = if let Some(ref alias) = decorator_alias {
                        let prev = self.cjs_import_map.remove(name.as_str());
                        self.cjs_import_map.insert(
                            name.clone().into(),
                            (alias.clone().into(), AstString::default()),
                        );
                        Some((name.clone(), prev))
                    } else {
                        None
                    };
                    self.write(&name);
                    self.write(" = ");
                    if let Some(ref alias) = decorator_alias {
                        self.write(alias);
                        self.write(" = ");
                    }
                    let class_expr = Expr {
                        kind: ExprKind::ClassExpr(Box::new(class_expr_decl)),
                        span: Span::default(),
                    };
                    self.emit_expr(&class_expr);
                    self.strip_trailing_newline();
                    self.writeln(";");
                    let has_decorators = class_needs_let_wrapper(&class_decl);
                    let is_default_export = export_name.as_deref() == Some("default");
                    let is_decorated =
                        has_decorators && self.options.experimental_decorators == Some(true);
                    // For default decorated exports, suppress initial exports_1
                    // (it comes after __decorate instead)
                    if !is_default_export || !is_decorated {
                        if let Some(ref exported) = export_name {
                            self.write(&exports_fn);
                            self.write("(\"");
                            self.write(exported);
                            self.write("\", ");
                            self.write(&name);
                            self.writeln(");");
                        }
                    }
                    self.emit_class_static_field_initializers(&class_decl, &[]);
                    // Emit __decorate call for decorated classes
                    if is_decorated {
                        self.decorated_class_self_ref_alias = decorator_alias.clone();
                        // For unnamed default classes, set the generated name so
                        // emit_decorator_applications can emit the __decorate call
                        let mut decorate_class = class_decl.clone();
                        if decorate_class.name.is_none() {
                            decorate_class.name = Some(name.clone());
                        }
                        if let Some(ref exported) = export_name {
                            if is_default_export {
                                // Default export: standalone __decorate, then exports_1 after
                                self.emit_decorator_applications(&decorate_class, None);
                                self.write(&exports_fn);
                                self.write("(\"");
                                self.write(exported);
                                self.write("\", ");
                                self.write(&name);
                                self.writeln(");");
                            } else {
                                // Named export: wrap __decorate in exports_1(...)
                                self.write(&exports_fn);
                                self.write("(\"");
                                self.write(exported);
                                self.write("\", ");
                                self.emit_decorator_applications(&decorate_class, None);
                                // Strip the trailing ";\n" from emit_decorator_applications
                                // and close the exports_1 call
                                if self.output.ends_with(";\n") {
                                    let len = self.output.len();
                                    self.output.truncate(len - 2);
                                    self.at_line_start = false;
                                }
                                self.writeln(");");
                            }
                        } else {
                            self.emit_decorator_applications(&decorate_class, None);
                        }
                        self.decorated_class_self_ref_alias = None;
                    }
                    // Restore import map
                    if let Some((class_name, prev)) = saved_import_map_entry {
                        if let Some(prev_entry) = prev {
                            self.cjs_import_map.insert(class_name.into(), prev_entry);
                        } else {
                            self.cjs_import_map.remove(class_name.as_str());
                        }
                    }
                }
                ExecuteAction::EmitStmt(stmt) => {
                    let prev_system_hoist_var_in_execute = self.system_hoist_var_in_execute;
                    self.system_hoist_var_in_execute = true;
                    self.emit_leading_comments(stmt.span.start);
                    self.emit_stmt(&stmt);
                    self.advance_comment_pos(stmt.span.end);
                    self.system_hoist_var_in_execute = prev_system_hoist_var_in_execute;
                }
                ExecuteAction::EmitLeadingComments(pos) => {
                    self.emit_leading_comments(pos);
                }
                ExecuteAction::EmitNamedExport { .. } => {}
                ExecuteAction::EmitDefaultExportExpr(expr) => {
                    self.write(&exports_fn);
                    self.write("(\"default\", ");
                    self.emit_expr(&expr);
                    self.strip_trailing_newline();
                    self.writeln(");");
                }
                ExecuteAction::AddDisposableResource { .. } => {
                    // Handled inside UsingDisposalStart
                }
                ExecuteAction::UsingDisposalStart { env_name, is_await } => {
                    // Emit envelope init
                    self.write(&env_name);
                    self.writeln(" = { stack: [], error: void 0, hasError: false };");

                    // Open try block
                    self.writeln("try {");
                    self.indent += 1;

                    // Process all remaining actions inside the try block
                    while action_idx < execute_actions.len() {
                        let inner = execute_actions[action_idx].clone();
                        action_idx += 1;
                        match inner {
                            ExecuteAction::AddDisposableResource {
                                name,
                                init: inner_init,
                                env_name: inner_env,
                                is_await: inner_await,
                            } => {
                                self.write(&name);
                                self.write(" = ");
                                self.write(self.helper_prefix());
                                self.write("__addDisposableResource(");
                                self.write(&inner_env);
                                self.write(", ");
                                self.emit_expr(&inner_init);
                                self.write(if inner_await { ", true" } else { ", false" });
                                self.writeln(");");
                            }
                            ExecuteAction::AssignVar {
                                name,
                                init: inner_init,
                                export_name,
                            } => {
                                if let Some(exported) = export_name {
                                    self.write(&exports_fn);
                                    self.write("(\"");
                                    self.write(&exported);
                                    self.write("\", ");
                                    self.write(&name);
                                    self.write(" = ");
                                    self.emit_expr(&inner_init);
                                    self.writeln(");");
                                } else if named_export_locals.contains(&name) {
                                    // Re-exported local: wrap in exports_fn
                                    self.write(&exports_fn);
                                    self.write("(\"");
                                    self.write(&name);
                                    self.write("\", ");
                                    self.write(&name);
                                    self.write(" = ");
                                    self.emit_expr(&inner_init);
                                    self.writeln(");");
                                } else {
                                    self.write(&name);
                                    self.write(" = ");
                                    self.emit_expr(&inner_init);
                                    self.writeln(";");
                                }
                            }
                            ExecuteAction::ExportEmptyBinding {
                                value_temp,
                                export_name,
                                init,
                            } => {
                                self.write(&exports_fn);
                                self.write("(\"");
                                self.write(&export_name);
                                self.write("\", ");
                                self.write(&export_name);
                                self.write(" = ");
                                self.write(&value_temp);
                                self.write(" = ");
                                self.emit_expr(&init);
                                self.writeln(");");
                            }
                            ExecuteAction::EmitDefaultExportExpr(expr) => {
                                self.write(&exports_fn);
                                self.write("(\"default\", _default = ");
                                self.emit_expr(&expr);
                                self.strip_trailing_newline();
                                self.writeln(");");
                            }
                            ExecuteAction::EmitStmt(stmt) => {
                                let prev = self.system_hoist_var_in_execute;
                                self.system_hoist_var_in_execute = true;
                                self.emit_leading_comments(stmt.span.start);
                                self.emit_stmt(&stmt);
                                self.advance_comment_pos(stmt.span.end);
                                self.system_hoist_var_in_execute = prev;
                            }
                            ExecuteAction::EmitLeadingComments(pos) => {
                                self.emit_leading_comments(pos);
                            }
                            ExecuteAction::EmitNamedExport { .. } => {}
                            _ => {}
                        }
                    }

                    // Close try block
                    self.indent -= 1;
                    self.writeln("}");

                    // Catch block
                    let env_num_str = env_name.strip_prefix("env_").unwrap_or("1");
                    self.writeln(&format!("catch (e_{}) {{", env_num_str));
                    self.indent += 1;
                    self.writeln(&format!("{}.error = e_{};", env_name, env_num_str));
                    self.writeln(&format!("{}.hasError = true;", env_name));
                    self.indent -= 1;
                    self.writeln("}");

                    // Finally block
                    self.writeln("finally {");
                    self.indent += 1;
                    if is_await {
                        self.writeln(&format!(
                            "const result_{} = {}__disposeResources({});",
                            env_num_str,
                            self.helper_prefix(),
                            env_name
                        ));
                        self.writeln(&format!("if (result_{})", env_num_str));
                        self.indent += 1;
                        self.writeln(&format!("await result_{};", env_num_str));
                        self.indent -= 1;
                    } else {
                        self.writeln(&format!(
                            "{}__disposeResources({});",
                            self.helper_prefix(),
                            env_name
                        ));
                    }
                    self.indent -= 1;
                    self.writeln("}");
                }
            }
        }
        self.rewrite_ident_with_import_map = prev_rewrite_ident_with_import_map;
        self.system_inline_export_aliases = prev_system_inline_export_aliases;
        self.system_live_export_names = prev_system_live_export_names;
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("};");
        self.indent -= 1;
        self.writeln("});");
        self.indent = self.indent.saturating_sub(1);

        if !deferred_system_export_names.is_empty() {
            self.resolve_deferred_temp_placeholders();
            self.resolve_inline_deferred_temp_placeholders();
        }
        if self.temp_var_names.len() > system_temp_start
            || self.resolved_deferred_temp_names.len() > system_resolved_deferred_start
        {
            let late_names = self.temp_var_names[system_temp_start..]
                .iter()
                .chain(self.resolved_deferred_temp_names[system_resolved_deferred_start..].iter())
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if let Some(insert_pos) = late_system_temp_insert_pos {
                self.insert_generated_text(insert_pos, &format!("{late_names}, "));
            }
        }
        self.resolve_deferred_export_name_placeholders();

        true
    }

    // ------------------------------------------------------------------
    // AMD / UMD module emission
    // ------------------------------------------------------------------

    /// Represents a dependency extracted from an import statement for AMD/UMD emit.
    /// `source` is the module specifier string, `param` is the variable name
    /// used as the factory function parameter.

    /// Emit an AMD or UMD module wrapper around CJS-style body.
    pub(super) fn emit_amd_or_umd_module(&mut self, file: &SourceFile) {
        // Pre-scan: collect import dependencies and build factory parameter list.
        // Each import/re-export becomes a dependency in the define() array.
        // Imports with bindings and re-exports also get a factory parameter.
        struct AmdDep {
            source: String,
            param: Option<String>,
        }
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum AmdInteropWrap {
            ImportDefault,
            ImportStar,
        }
        let mut module_param_deps: Vec<AmdDep> = Vec::new();
        let mut module_no_param_deps: Vec<AmdDep> = Vec::new();
        // Map: local binding name → (param_var, imported_member)
        let mut import_param_map: Vec<(String, String, String)> = Vec::new();
        // Param variables that must be wrapped in __importDefault/__importStar in-body.
        let mut amd_param_wraps: Vec<(String, AmdInteropWrap)> = Vec::new();
        let mut amd_combined_ns_aliases: Vec<(String, String)> = Vec::new();
        // Import sources that need body emit (esModuleInterop default/namespace)
        let mut amd_body_imports: Vec<String> = Vec::new();

        let (named_amd_deps, unnamed_amd_deps) = self.collect_amd_dependency_directives();
        let named_amd_dep_alias_by_path: HashMap<String, String> =
            named_amd_deps.iter().cloned().collect();

        for stmt in &file.statements {
            match &stmt.kind {
                StmtKind::Import(imp) => {
                    if imp.type_only || imp.source.is_empty() {
                        continue;
                    }
                    let amd_named_alias = named_amd_dep_alias_by_path.get(&imp.source).cloned();
                    match &imp.specifiers {
                        ImportClause::Named {
                            default,
                            named,
                            namespace,
                        } => {
                            let non_type: Vec<_> = named
                                .iter()
                                .filter(|s| !s.is_type && !self.is_import_elided(&s.local))
                                .collect();
                            let has_named_default = non_type
                                .iter()
                                .any(|s| s.imported.as_deref() == Some("default"));
                            let has_named_non_default = non_type
                                .iter()
                                .any(|s| s.imported.as_deref() != Some("default"));
                            let keep_default =
                                default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                            let keep_namespace = namespace
                                .as_ref()
                                .is_some_and(|ns| !self.is_import_elided(ns));
                            let has_bindings =
                                keep_default || keep_namespace || !non_type.is_empty();

                            // For imports that have an AMD dependency alias
                            // (`/// <amd-dependency name="X" path="..."/>`), keep the
                            // source path as a no-param dependency and emit the import
                            // in-body using `require("X")` instead of a factory param.
                            if has_bindings {
                                if let Some(alias_name) = amd_named_alias.clone() {
                                    if !module_no_param_deps.iter().any(|d| d.source == alias_name)
                                    {
                                        module_no_param_deps.push(AmdDep {
                                            source: alias_name,
                                            param: None,
                                        });
                                    }
                                    if !amd_body_imports.contains(&imp.source) {
                                        amd_body_imports.push(imp.source.clone());
                                    }
                                    continue;
                                }
                            }

                            if !has_bindings {
                                let had_original_bindings = default.is_some()
                                    || namespace.is_some()
                                    || named.iter().any(|s| !s.is_type);
                                if !had_original_bindings {
                                    // Side-effect import: dep only, no param
                                    module_no_param_deps.push(AmdDep {
                                        source: imp.source.clone(),
                                        param: None,
                                    });
                                }
                                // Elided imports: no entry at all
                                continue;
                            }

                            // Import becomes a factory parameter; for default/namespace
                            // bindings, emit a post-prologue wrap assignment so runtime
                            // behavior matches CJS helper semantics.
                            let param_var =
                                if keep_namespace && !keep_default && non_type.is_empty() {
                                    // Pure namespace import: use the namespace name directly
                                    namespace.as_ref().unwrap().clone()
                                } else {
                                    self.next_require_var(&imp.source)
                                };
                            module_param_deps.push(AmdDep {
                                source: imp.source.clone(),
                                param: Some(param_var.clone()),
                            });

                            // Register bindings for reference rewriting
                            if keep_namespace && keep_default {
                                // Combined: import e1, * as e2 from 'mod'
                                // In AMD, the namespace gets a local alias: const e2 = t1_3;
                                // (emitted after param wraps). Don't put ns in cjs_import_map;
                                // it'll be a regular local variable.
                                let ns = namespace.as_ref().unwrap();
                                amd_combined_ns_aliases.push((ns.clone(), param_var.clone()));
                            } else if keep_namespace {
                                let ns = namespace.as_ref().unwrap();
                                if ns != &param_var {
                                    self.cjs_import_map.insert(
                                        ns.clone().into(),
                                        (param_var.clone().into(), AstString::default()),
                                    );
                                }
                            }
                            if keep_default {
                                let def = default.as_ref().unwrap();
                                import_param_map.push((
                                    def.clone(),
                                    param_var.clone(),
                                    "default".to_string(),
                                ));
                                self.cjs_default_import_bindings.insert(def.clone().into());
                            }
                            for spec in &non_type {
                                let imported = spec.imported.as_deref().unwrap_or(&spec.local);
                                import_param_map.push((
                                    spec.local.clone(),
                                    param_var.clone(),
                                    imported.to_string(),
                                ));
                            }

                            let wrap_kind = if self.options.import_helpers == Some(true) {
                                None
                            } else if keep_namespace
                                || (keep_default && has_named_non_default)
                                || (!keep_default && has_named_default && has_named_non_default)
                            {
                                Some(AmdInteropWrap::ImportStar)
                            } else if keep_default || has_named_default {
                                Some(AmdInteropWrap::ImportDefault)
                            } else {
                                None
                            };
                            if let Some(kind) = wrap_kind {
                                amd_param_wraps.push((param_var, kind));
                            }
                        }
                        ImportClause::Require(name) => {
                            if self.is_import_elided(name) {
                                continue;
                            }
                            if self.type_only_require_specs.contains(imp.source.as_str()) {
                                continue;
                            }
                            if self.is_umd() {
                                // UMD keeps `import = require()` in the factory body.
                                module_no_param_deps.push(AmdDep {
                                    source: imp.source.clone(),
                                    param: None,
                                });
                                if !amd_body_imports.contains(&imp.source) {
                                    amd_body_imports.push(imp.source.clone());
                                }
                            } else {
                                // AMD keeps `import = require()` as factory params.
                                module_param_deps.push(AmdDep {
                                    source: imp.source.clone(),
                                    param: Some(name.clone()),
                                });
                            }
                        }
                    }
                }
                StmtKind::Export(export_decl) => {
                    match &export_decl.kind {
                        ExportDeclKind::Named {
                            specifiers,
                            source: Some(src),
                            type_only,
                        } => {
                            if *type_only {
                                continue;
                            }
                            let has_non_type = specifiers.iter().any(|s| !s.is_type);
                            if !has_non_type {
                                continue;
                            }
                            let param_var = self.next_require_var(src);
                            module_param_deps.push(AmdDep {
                                source: src.clone(),
                                param: Some(param_var.clone()),
                            });
                            // Map source → param for emit_export_decl_cjs
                            self.amd_dep_map.insert(src.clone(), param_var);
                        }
                        ExportDeclKind::All {
                            source, type_only, ..
                        } => {
                            if *type_only {
                                continue;
                            }
                            let param_var = self.next_require_var(source);
                            module_param_deps.push(AmdDep {
                                source: source.clone(),
                                param: Some(param_var.clone()),
                            });
                            self.amd_dep_map.insert(source.clone(), param_var);
                        }
                        ExportDeclKind::Decl(ref inner) => {
                            // export import ... from "..." / export import a = require("...")
                            // Exported imports are always emitted (the export makes
                            // the binding a value export), so skip elision check.
                            if let StmtKind::Import(ref imp) = inner.kind {
                                if !imp.type_only {
                                    match &imp.specifiers {
                                        ImportClause::Require(ref name) => {
                                            if !self
                                                .type_only_require_specs
                                                .contains(imp.source.as_str())
                                            {
                                                module_param_deps.push(AmdDep {
                                                    source: imp.source.clone(),
                                                    param: Some(name.clone()),
                                                });
                                            }
                                        }
                                        ImportClause::Named {
                                            default,
                                            named,
                                            namespace,
                                        } => {
                                            let non_type: Vec<_> = named
                                                .iter()
                                                .filter(|s| {
                                                    !s.is_type && !self.is_import_elided(&s.local)
                                                })
                                                .collect();
                                            let keep_default = default
                                                .as_ref()
                                                .is_some_and(|d| !self.is_import_elided(d));
                                            let keep_namespace = namespace
                                                .as_ref()
                                                .is_some_and(|ns| !self.is_import_elided(ns));
                                            let has_bindings = keep_default
                                                || keep_namespace
                                                || !non_type.is_empty();
                                            if has_bindings {
                                                let param_var = if keep_namespace
                                                    && !keep_default
                                                    && non_type.is_empty()
                                                {
                                                    namespace.as_ref().unwrap().clone()
                                                } else {
                                                    self.next_require_var(&imp.source)
                                                };
                                                module_param_deps.push(AmdDep {
                                                    source: imp.source.clone(),
                                                    param: Some(param_var.clone()),
                                                });
                                                if keep_namespace && keep_default {
                                                    let ns = namespace.as_ref().unwrap();
                                                    amd_combined_ns_aliases
                                                        .push((ns.clone(), param_var.clone()));
                                                } else if keep_namespace {
                                                    let ns = namespace.as_ref().unwrap();
                                                    if ns != &param_var {
                                                        self.cjs_import_map.insert(
                                                            ns.clone().into(),
                                                            (
                                                                param_var.clone().into(),
                                                                AstString::default(),
                                                            ),
                                                        );
                                                    }
                                                }
                                                if keep_default {
                                                    let def = default.as_ref().unwrap();
                                                    import_param_map.push((
                                                        def.clone(),
                                                        param_var.clone(),
                                                        "default".to_string(),
                                                    ));
                                                    self.cjs_default_import_bindings
                                                        .insert(def.clone().into());
                                                }
                                                for spec in &non_type {
                                                    let imported = spec
                                                        .imported
                                                        .as_deref()
                                                        .unwrap_or(&spec.local);
                                                    import_param_map.push((
                                                        spec.local.clone(),
                                                        param_var.clone(),
                                                        imported.to_string(),
                                                    ));
                                                }
                                                let has_named_default = non_type.iter().any(|s| {
                                                    s.imported.as_deref() == Some("default")
                                                });
                                                let has_named_non_default =
                                                    non_type.iter().any(|s| {
                                                        s.imported.as_deref() != Some("default")
                                                    });
                                                let wrap_kind =
                                                    if self.options.import_helpers == Some(true) {
                                                        None
                                                    } else if keep_namespace
                                                        || (keep_default && has_named_non_default)
                                                        || (!keep_default
                                                            && has_named_default
                                                            && has_named_non_default)
                                                    {
                                                        Some(AmdInteropWrap::ImportStar)
                                                    } else if keep_default || has_named_default {
                                                        Some(AmdInteropWrap::ImportDefault)
                                                    } else {
                                                        None
                                                    };
                                                if let Some(kind) = wrap_kind {
                                                    amd_param_wraps.push((param_var, kind));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        let param_dep_sources: HashSet<&str> = named_amd_deps
            .iter()
            .map(|(source, _)| source.as_str())
            .chain(module_param_deps.iter().map(|d| d.source.as_str()))
            .collect();
        module_no_param_deps.retain(|d| !param_dep_sources.contains(d.source.as_str()));

        // Track named AMD dependency param names before consuming the iterator.
        let named_amd_deps_names: Vec<String> = named_amd_deps
            .iter()
            .map(|(_, name)| name.clone())
            .collect();
        let mut amd_deps: Vec<AmdDep> = Vec::new();
        for (source, name) in named_amd_deps {
            amd_deps.push(AmdDep {
                source,
                param: Some(name),
            });
        }
        amd_deps.extend(module_param_deps);
        for source in unnamed_amd_deps {
            amd_deps.push(AmdDep {
                source,
                param: None,
            });
        }
        amd_deps.extend(module_no_param_deps);

        // Scan helpers early so we know whether tslib is needed for AMD deps.
        self.scan_needed_helpers(&file.statements);

        // When importHelpers=true, add "tslib" to the AMD dependency list
        // instead of emitting inline helpers.  TypeScript places tslib right
        // after "require"/"exports" and before user-module deps.
        if self.options.import_helpers == Some(true) && self.any_tslib_helper_needed() {
            amd_deps.insert(
                0,
                AmdDep {
                    source: "tslib".to_string(),
                    param: Some("tslib_1".to_string()),
                },
            );
        }

        // Collect named AMD dependency param names (from `/// <amd-dependency name="X">`).
        // These stay as factory params even in UMD mode.
        let named_amd_param_names: HashSet<String> = named_amd_deps_names.iter().cloned().collect();

        // Build flat deps/params arrays from ordered amd_deps.
        let deps: Vec<&str> = amd_deps.iter().map(|d| d.source.as_str()).collect();
        let params: Vec<&str> = amd_deps.iter().filter_map(|d| d.param.as_deref()).collect();

        // For UMD, build param→source mapping for in-body require() calls.
        // TypeScript UMD uses CJS-style require() inside the factory body
        // for ES imports. Named AMD dependency params (from `/// <amd-dependency name="X">`)
        // stay as factory parameters.
        let umd_param_requires: Vec<(String, String)> = if self.is_umd() {
            amd_deps
                .iter()
                .filter_map(|d| {
                    d.param.as_ref().and_then(|p| {
                        if named_amd_param_names.contains(p) {
                            None // Keep as factory param
                        } else {
                            Some((p.clone(), d.source.clone()))
                        }
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        // TypeScript emits ALL helpers (CJS interop + transform) outside the
        // AMD/UMD wrapper, NOT when importHelpers=true (helpers come from tslib)
        // or noEmitHelpers=true.
        if self.options.no_emit_helpers != Some(true) && self.options.import_helpers != Some(true) {
            if self.needs_extends_helper {
                self.emit_extends_helper();
            }
            // Same ordering as CJS path in lib.rs:
            // 1. __createBinding, __setModuleDefault (base helpers)
            // 2. __decorate (if decorators present)
            // 3. __exportStar, __importStar (star helpers)
            // 4. __metadata, __param (decorator metadata)
            // 5. __awaiter, __setFunctionName, etc. (transform helpers)
            // 6. __importDefault (comes last)
            self.emit_cjs_base_helpers();
            if self.needs_decorate_helper {
                self.emit_decorate_helper();
            }
            self.emit_cjs_star_helpers();
            if self.needs_metadata_helper {
                self.emit_metadata_helper();
            }
            if self.needs_param_helper {
                self.emit_param_helper();
            }
            if self.needs_awaiter_helper {
                self.emit_awaiter_helper();
            }
            if self.needs_generator_helper {
                self.emit_generator_helper();
            }
            if self.needs_rest_helper {
                self.emit_rest_helper();
            }
            if self.needs_read_helper {
                self.emit_read_helper();
            }
            if self.needs_values_helper {
                self.emit_values_helper();
            }
            if self.async_values_before_await {
                if self.needs_async_values_helper {
                    self.emit_async_values_helper();
                }
                if self.needs_await_helper {
                    self.emit_await_helper();
                }
                if self.needs_async_delegator_helper {
                    self.emit_async_delegator_helper();
                }
                if self.needs_async_generator_helper {
                    self.emit_async_generator_helper();
                }
            } else {
                if self.needs_await_helper {
                    self.emit_await_helper();
                }
                if self.needs_async_generator_helper {
                    self.emit_async_generator_helper();
                }
                if self.needs_async_values_helper {
                    self.emit_async_values_helper();
                }
                if self.needs_async_delegator_helper {
                    self.emit_async_delegator_helper();
                }
            }
            if self.private_field_get_first {
                if self.needs_private_field_get {
                    self.emit_private_field_get_helper();
                }
                if self.needs_private_field_set {
                    self.emit_private_field_set_helper();
                }
            } else {
                if self.needs_private_field_set {
                    self.emit_private_field_set_helper();
                }
                if self.needs_private_field_get {
                    self.emit_private_field_get_helper();
                }
            }
            if self.needs_private_field_in {
                self.emit_private_field_in_helper();
            }
            // esDecorators helpers come after classFields helpers in TS pipeline
            if self.needs_set_function_name_helper {
                self.emit_set_function_name_helper();
            }
            self.emit_cjs_import_default_helper_if_needed();
        }

        // Emit `/// <reference path>` directives before the AMD/UMD wrapper,
        // but only when the referenced file isn't already covered by an import.
        // TypeScript skips reference directives whose base name matches an
        // import source (e.g. `/// <reference path="file1.d.ts"/>` is skipped
        // when `import x = require("file1")` exists).
        if let Some(first) = file.statements.first() {
            // Collect import source base names for filtering.
            let mut import_sources: Vec<String> = Vec::new();
            for stmt in &file.statements {
                match &stmt.kind {
                    StmtKind::Import(imp) if !imp.source.is_empty() => {
                        import_sources.push(imp.source.clone());
                    }
                    StmtKind::ImportEquals(ie) => {
                        // import x = require("module")
                        if let ExprKind::Call(call) = &ie.module_ref.kind {
                            if let Some(arg) = call.args.first() {
                                if let ExprKind::StrLit(s) = &arg.kind {
                                    import_sources.push(s.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            self.emit_filtered_reference_directives(first.span.start, &import_sources);
        }

        let amd_module_name = self.find_amd_module_name(file);
        let mut emitted_file_prologue = false;
        if let Some(first_stmt) = file.statements.first() {
            self.emit_file_prologue_comments(first_stmt.span.start);
            emitted_file_prologue = true;
        }

        if self.is_umd() {
            // UMD wrapper
            self.writeln("(function (factory) {");
            self.indent += 1;
            self.writeln(
                "if (typeof module === \"object\" && typeof module.exports === \"object\") {",
            );
            self.indent += 1;
            self.writeln("var v = factory(require, exports);");
            self.writeln("if (v !== undefined) module.exports = v;");
            self.indent -= 1;
            self.writeln("}");
            self.write("else if (typeof define === \"function\" && define.amd) {");
            self.newline();
            self.indent += 1;
            // define("name", ["require", "exports", ...deps], factory);
            self.write("define(");
            if let Some(ref name) = amd_module_name {
                self.write("\"");
                self.write(name);
                self.write("\", ");
            }
            self.write("[\"require\", \"exports\"");
            for dep in &deps {
                self.write(", \"");
                self.write(dep);
                self.write("\"");
            }
            self.write("], factory");
            self.writeln(");");
            self.indent -= 1;
            self.writeln("}");
            self.indent -= 1;
            // Factory function — UMD uses CJS-style require() in the body,
            // so ES import params are NOT included in the factory signature.
            // Only named AMD dependency params (`/// <amd-dependency name="X">`)
            // are kept as factory parameters.
            self.write("})(function (require, exports");
            for p in &params {
                if named_amd_param_names.contains(*p) {
                    self.write(", ");
                    self.write(p);
                }
            }
            self.writeln(") {");
        } else {
            // AMD wrapper
            self.write("define(");
            if let Some(ref name) = amd_module_name {
                self.write("\"");
                self.write(name);
                self.write("\", ");
            }
            self.write("[\"require\", \"exports\"");
            for dep in &deps {
                self.write(", \"");
                self.write(dep);
                self.write("\"");
            }
            self.write("], function (require, exports");
            for p in &params {
                self.write(", ");
                self.write(p);
            }
            self.writeln(") {");
        }

        self.indent += 1;
        self.module_wrapper_indent += 1;

        // Set up the CJS-like emit context inside the factory body.
        self.export_target = Some("exports".to_string());
        self.writeln("\"use strict\";");
        let wrapper_temp_start = self.temp_var_names.len();
        let wrapper_temp_insert_pos = self.output.len();

        // In UMD modules with dynamic import(), emit __syncRequire guard.
        // This lets the downleveled import() choose between CJS require and
        // AMD require at runtime.
        if self.is_umd()
            && file
                .statements
                .iter()
                .any(|s| self.stmt_has_dynamic_import_call(s))
        {
            self.writeln(
                "var __syncRequire = typeof module === \"object\" && typeof module.exports === \"object\";",
            );
        }

        let has_export_assign =
            has_value_export_assign(&file.statements, self.preserve_const_enums_effective());
        self.has_export_assign = has_export_assign;

        // Register AMD param bindings in the cjs_import_map so reference rewriting works.
        // Named imports: local → (param_var, imported_member) for member access rewriting.
        // Namespace/require imports were already registered during dep scanning above.
        for (local, param_var, imported) in &import_param_map {
            self.cjs_import_map.insert(
                local.clone().into(),
                (param_var.clone().into(), imported.clone().into()),
            );
        }

        let omitted_array_temps = self.prepare_omitted_array_destructure_exports(&file.statements);
        if !omitted_array_temps.is_empty() {
            self.write("var ");
            for (i, t) in omitted_array_temps.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(t);
            }
            self.writeln(";");
        }

        // Helpers are emitted OUTSIDE the wrapper (see above).

        // Emit __esModule marker for all module files, UNLESS:
        // 1. The file uses `export = ...` (has_export_assign), or
        // 2. The file is a module ONLY because of dynamic import() calls
        //    (no top-level import/export/export-modifier statements).
        let has_import_or_export_stmts = file.statements.iter().any(|s| {
            matches!(
                s.kind,
                StmtKind::Import(_) | StmtKind::Export(_) | StmtKind::ExportAssign(_)
            ) || stmt_has_export_modifier(s)
        });
        if !has_export_assign && has_import_or_export_stmts {
            self.writeln("Object.defineProperty(exports, \"__esModule\", { value: true });");
        }

        // Pre-declare exported bindings (same logic as CJS).
        // NOTE: amd_param_wraps (__importDefault/__importStar) are emitted AFTER
        // pre-declarations — see below after pre_decl_names emission.
        let mut pre_decl_names: Vec<String> = Vec::new();
        let mut var_export_names: Vec<String> = Vec::new();
        let mut fn_export_names: Vec<(String, String)> = Vec::new();
        let mut local_named_exports: Vec<(String, String)> = Vec::new();
        let mut modifier_export_names: Vec<String> = Vec::new();
        let file_fn_names: HashSet<String> = file
            .statements
            .iter()
            .filter_map(|s| match &s.kind {
                StmtKind::FnDecl(f) => f.name.clone(),
                StmtKind::Export(e) => {
                    if let ExportDeclKind::Decl(d) = &e.kind {
                        if let StmtKind::FnDecl(f) = &d.kind {
                            f.name.clone()
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        // Pre-scan namespace declarations to build cumulative export maps.
        // Use "exports" as parent key to match the CJS export_target used in AMD/UMD.
        prescan_namespace_exports_full(
            &file.statements,
            Some("exports"),
            &mut self.cumulative_ns_exports,
            Some(&mut self.cumulative_ns_type_exports),
        );

        let mut aliased_fn_exports: HashSet<String> = HashSet::new();
        for stmt in &file.statements {
            if let StmtKind::Export(ref export_decl) = stmt.kind {
                collect_export_names(
                    &export_decl.kind,
                    &mut pre_decl_names,
                    self.preserve_const_enums_effective(),
                );
                {
                    let counter = &mut self.default_export_counter;
                    collect_fn_export_names(&export_decl.kind, &mut fn_export_names, counter);
                }
                collect_local_named_export_assignments(&export_decl.kind, &mut local_named_exports);
                // Collect var-only export names (for CJS reference qualification).
                if let ExportDeclKind::Decl(ref decl) = export_decl.kind {
                    if let StmtKind::Var(ref v) = decl.kind {
                        if v.modifiers & MOD_DECLARE == 0 {
                            for d in &v.declarations {
                                collect_binding_names(&d.name, &mut var_export_names);
                            }
                        }
                    }
                    // `export import X = M.N;` — include in void 0 pre-declarations
                    // when the root of the RHS is a value-bound name OR a type-only
                    // namespace that is used as a value elsewhere in the file.
                    if let StmtKind::ImportEquals(ref ie) = decl.kind {
                        let root = expr_root_ident_static(&ie.module_ref);
                        if let Some(ref root_name) = root {
                            let preserve_const = self.preserve_const_enums_effective();
                            let fvbn =
                                collect_file_value_bound_names(&file.statements, preserve_const);
                            let root_is_value = fvbn.contains(root_name.as_str());
                            // Check if root is a type-only namespace (not in value-bound)
                            let root_is_type_only_ns = !root_is_value
                                && file.statements.iter().any(|s| match &s.kind {
                                    StmtKind::ModuleDecl(m) if m.modifiers & MOD_DECLARE == 0 => {
                                        matches!(&m.name, ModuleName::Ident(n) if n == root_name)
                                    }
                                    _ => false,
                                });
                            if root_is_type_only_ns {
                                // Type-only namespace root: always pre-declare.
                                pre_decl_names.push(ie.name.clone());
                                var_export_names.push(ie.name.clone());
                            } else if root_is_value {
                                // Value namespace: check if the member is type-only.
                                let member_is_type_only =
                                    if let ExprKind::Member(ref mem) = ie.module_ref.kind {
                                        let prop = &mem.property;
                                        self.cumulative_ns_type_exports
                                            .get(root_name.as_str())
                                            .is_some_and(|set| set.contains(prop.as_str()))
                                            || self
                                                .cumulative_ns_type_exports
                                                .get(format!("exports::{root_name}").as_str())
                                                .is_some_and(|set| set.contains(prop.as_str()))
                                    } else {
                                        false
                                    };
                                if !member_is_type_only {
                                    pre_decl_names.push(ie.name.clone());
                                    // Also add to var_export_names so references
                                    // to the alias are qualified with `exports.`.
                                    var_export_names.push(ie.name.clone());
                                }
                            }
                        }
                    }
                    // `export import X = require("...");` — add to
                    // var_export_names so references are qualified.
                    if let StmtKind::Import(ref imp) = decl.kind {
                        if let ImportClause::Require(ref name) = imp.specifiers {
                            if !self.type_only_require_specs.contains(imp.source.as_str()) {
                                var_export_names.push(name.clone());
                            }
                        }
                    }
                }
                if let ExportDeclKind::Named {
                    specifiers,
                    source: None,
                    type_only: false,
                } = &export_decl.kind
                {
                    for spec in specifiers {
                        if spec.is_type {
                            continue;
                        }
                        if file_fn_names.contains(&spec.local) {
                            let exported = spec.exported.as_ref().unwrap_or(&spec.local).clone();
                            aliased_fn_exports.insert(exported.clone());
                            fn_export_names.push((exported, spec.local.clone()));
                        }
                    }
                }
            }
            collect_modifier_export_names(stmt, &mut pre_decl_names);
            collect_modifier_export_names(stmt, &mut modifier_export_names);
            collect_modifier_fn_export_names(stmt, &mut fn_export_names);
            // Collect var-only modifier exports.
            if let StmtKind::Var(v) = &stmt.kind {
                if v.modifiers & MOD_EXPORT != 0 && v.modifiers & MOD_DECLARE == 0 {
                    for d in &v.declarations {
                        collect_binding_names(&d.name, &mut var_export_names);
                    }
                }
            }
        }
        {
            // Dedup with HashSet<&str> instead of HashSet<String> — borrowing
            // each name from the Vec avoids cloning every entry just to insert.
            // Compute keep flags first, then drop the borrow before mutating.
            let mut seen: HashSet<&str> = HashSet::new();
            let keeps: Vec<bool> = pre_decl_names
                .iter()
                .map(|n| seen.insert(n.as_str()))
                .collect();
            drop(seen);
            let mut iter = keeps.into_iter();
            pre_decl_names.retain(|_| iter.next().unwrap_or(false));
        }
        {
            let mut seen: HashSet<(&str, &str)> = HashSet::new();
            let keeps: Vec<bool> = local_named_exports
                .iter()
                .map(|(a, b)| seen.insert((a.as_str(), b.as_str())))
                .collect();
            drop(seen);
            let mut iter = keeps.into_iter();
            local_named_exports.retain(|_| iter.next().unwrap_or(false));
        }
        {
            let mut seen: HashSet<&str> = HashSet::new();
            let keeps: Vec<bool> = modifier_export_names
                .iter()
                .map(|n| seen.insert(n.as_str()))
                .collect();
            drop(seen);
            let mut iter = keeps.into_iter();
            modifier_export_names.retain(|_| iter.next().unwrap_or(false));
        }
        {
            let fn_names: HashSet<&str> = fn_export_names.iter().map(|(n, _)| n.as_str()).collect();
            pre_decl_names.retain(|n| {
                !fn_names.contains(n.as_str())
                    && !file_fn_names.contains(n)
                    && !aliased_fn_exports.contains(n)
            });
        }
        {
            let fn_names: HashSet<&str> = fn_export_names.iter().map(|(n, _)| n.as_str()).collect();
            let modifier_names: HashSet<&str> =
                modifier_export_names.iter().map(|n| n.as_str()).collect();
            local_named_exports.retain(|(exported, local)| {
                !fn_names.contains(exported.as_str())
                    && !file_fn_names.contains(local)
                    && !(exported == local && modifier_names.contains(local.as_str()))
            });
        }
        if !self.preserve_const_enums_effective() {
            // Build exported→local mapping to resolve aliases (e.g. `export { D as D1 }`)
            let exported_to_local: HashMap<&str, &str> = local_named_exports
                .iter()
                .map(|(e, l)| (e.as_str(), l.as_str()))
                .collect();
            pre_decl_names.retain(|n| {
                let local = exported_to_local
                    .get(n.as_str())
                    .copied()
                    .unwrap_or(n.as_str());
                !self.is_const_enum_object_name(local)
            });
            local_named_exports.retain(|(_, local)| !self.is_const_enum_object_name(local));
        }
        // Filter type-only names (interfaces, type aliases) from pre-declarations
        // and named export assignments.
        {
            let preserve_const = self.preserve_const_enums_effective();
            let file_ns_enum_names: HashSet<String> = file
                .statements
                .iter()
                .filter_map(|s| {
                    let inner = match &s.kind {
                        StmtKind::Export(ed) => match &ed.kind {
                            ExportDeclKind::Decl(d) | ExportDeclKind::DefaultDecl(d) => d,
                            _ => return None,
                        },
                        _ => s,
                    };
                    match &inner.kind {
                        StmtKind::EnumDecl(e)
                            if e.modifiers & MOD_DECLARE == 0
                                && (!e.is_const || preserve_const) =>
                        {
                            Some(e.name.clone())
                        }
                        StmtKind::ModuleDecl(m)
                            if m.modifiers & MOD_DECLARE == 0
                                && !module_decl_is_type_only(m, preserve_const) =>
                        {
                            if let ModuleName::Ident(ref n) = m.name {
                                Some(n.clone())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                })
                .collect();
            let named_export_local_map: HashMap<String, String> = local_named_exports
                .iter()
                .map(|(e, l)| (e.clone(), l.clone()))
                .collect();
            // Build re-export map: exported → local for `export { X } from "..."`
            let mut reexport_local_map: HashMap<String, String> = HashMap::new();
            for stmt in &file.statements {
                if let StmtKind::Export(ref ed) = stmt.kind {
                    if let ExportDeclKind::Named {
                        specifiers,
                        source: Some(_),
                        type_only: false,
                    } = &ed.kind
                    {
                        for spec in specifiers {
                            if !spec.is_type {
                                let exported =
                                    spec.exported.as_ref().unwrap_or(&spec.local).clone();
                                reexport_local_map.insert(exported, spec.local.clone());
                            }
                        }
                    }
                }
            }
            let global_type_only_ref = &self.global_type_only_names;
            let global_type_only_export_ref = &self.global_type_only_export_names;
            pre_decl_names.retain(|exported| {
                if let Some(local) = named_export_local_map.get(exported) {
                    // Filter if globally type-only (imported type)
                    if (global_type_only_ref.contains(local.as_str())
                        || global_type_only_export_ref.contains(local.as_str()))
                        && !file_fn_names.contains(local)
                        && !file_ns_enum_names.contains(local)
                        && !self.cjs_import_map.contains_key(local.as_str())
                    {
                        return false;
                    }
                } else if let Some(local) = reexport_local_map.get(exported) {
                    // Re-export: filter if the source name is type-only
                    if global_type_only_ref.contains(local.as_str())
                        || global_type_only_export_ref.contains(local.as_str())
                    {
                        return false;
                    }
                }
                true
            });
            local_named_exports.retain(|(_, local)| {
                // Filter globally type-only names
                if (global_type_only_ref.contains(local.as_str())
                    || global_type_only_export_ref.contains(local.as_str()))
                    && !file_fn_names.contains(local)
                    && !file_ns_enum_names.contains(local)
                    && !self.cjs_import_map.contains_key(local.as_str())
                {
                    return false;
                }
                true
            });
        }
        self.cjs_exported_names.clear();
        self.cjs_default_fn_local_names.clear();
        for n in &pre_decl_names {
            self.cjs_exported_names.insert(n.clone().into());
        }
        for (exported, local) in &fn_export_names {
            self.cjs_exported_names.insert(exported.clone().into());
            // For `export default function Foo`, also mark the local name
            // as exported so namespace IIFE closings use `exports.Foo`.
            if exported == "default" && local != "default" {
                self.cjs_exported_names.insert(local.clone().into());
                self.cjs_default_fn_local_names.insert(local.clone().into());
            }
        }
        for (exported, _) in &local_named_exports {
            self.cjs_exported_names.insert(exported.clone().into());
        }
        self.cjs_var_export_names.clear();
        for n in &var_export_names {
            self.cjs_var_export_names.insert(n.clone().into());
        }
        // Populate live export chain so enum/namespace IIFE closings
        // include aliased export names.
        self.cjs_live_export_chain.clear();
        self.cjs_live_export_keys.clear();
        for (exported, local) in &local_named_exports {
            let entry = self
                .cjs_live_export_chain
                .entry(local.clone().into())
                .or_insert_with(Vec::new);
            // For actual aliases (exported != local), also include the original
            // export name so the IIFE produces `exports.E1 = exports.E = E = {}`.
            if exported != local
                && entry.is_empty()
                && self.cjs_exported_names.contains(local.as_str())
            {
                entry.push(local.clone().into());
            }
            entry.push(exported.clone().into());
        }
        for chain in self.cjs_live_export_chain.values_mut() {
            chain.reverse();
        }
        self.cjs_live_export_keys = self.cjs_live_export_chain.keys().cloned().collect();
        if !pre_decl_names.is_empty() {
            for name in pre_decl_names.iter().rev() {
                self.write_cjs_export_access("exports", name);
                self.write(" = ");
            }
            self.writeln("void 0;");
        }
        for (exported, local) in &fn_export_names {
            self.write_cjs_export_access("exports", exported);
            self.write(" = ");
            self.write(local);
            self.writeln(";");
        }

        // Emit __importDefault/__importStar wraps AFTER pre-declarations.
        if !umd_param_requires.is_empty() {
            // UMD: emit a target-appropriate `param =
            // [wrap(]require("source")[)]` binding for each import.
            let wrap_map: HashMap<&str, &AmdInteropWrap> = amd_param_wraps
                .iter()
                .map(|(p, k)| (p.as_str(), k))
                .collect();
            for (param_var, source) in &umd_param_requires {
                self.write(self.generated_require_binding_keyword());
                self.write(" ");
                self.write(param_var);
                self.write(" = ");
                if let Some(wrap_kind) = wrap_map.get(param_var.as_str()) {
                    match wrap_kind {
                        AmdInteropWrap::ImportDefault => {
                            self.write(self.helper_prefix());
                            self.write("__importDefault(");
                        }
                        AmdInteropWrap::ImportStar => {
                            self.write(self.helper_prefix());
                            self.write("__importStar(");
                        }
                    }
                    self.write("require(\"");
                    self.write(source);
                    self.write("\")");
                    self.writeln(");");
                } else {
                    self.write("require(\"");
                    self.write(source);
                    self.writeln("\");");
                }
            }
        } else {
            // AMD: wrap factory parameters in-place
            for (param_var, wrap_kind) in &amd_param_wraps {
                self.write(param_var);
                self.write(" = ");
                match wrap_kind {
                    AmdInteropWrap::ImportDefault => {
                        self.write(self.helper_prefix());
                        self.write("__importDefault(");
                    }
                    AmdInteropWrap::ImportStar => {
                        self.write(self.helper_prefix());
                        self.write("__importStar(");
                    }
                }
                self.write(param_var);
                self.writeln(");");
            }
        }
        // NOTE: amd_combined_ns_aliases are emitted inline at each import
        // statement position during body emission (see below).

        // Emit body statements. For AMD/UMD, imports are already parameters,
        // so we skip import statements and let everything else use CJS emit paths.
        // Reset the default export counter for the emission phase (pre-scan already consumed it).
        self.default_export_counter = 1;

        let cjs_source_has_use_strict = file
            .statements
            .first()
            .map_or(false, |s| is_use_strict_directive(s));
        // A name is value-bound if it is declared as a non-ambient value
        // (var/let/const/fn/class/enum/namespace) or imported as a value binding.
        // Names that are only ambient or only type-referenced have no runtime value.
        let file_value_bound_names: HashSet<AstString> = {
            let preserve_const = self.preserve_const_enums_effective();
            collect_file_value_bound_names(&file.statements, preserve_const)
        };
        // Names of enum/namespace declarations that already emit inline export
        // assignments in AMD/UMD factory bodies.
        let file_ns_enum_names: HashSet<String> = {
            let preserve_const = self.preserve_const_enums_effective();
            file.statements
                .iter()
                .filter_map(|s| {
                    let inner = match &s.kind {
                        StmtKind::Export(ed) => match &ed.kind {
                            ExportDeclKind::Decl(d) | ExportDeclKind::DefaultDecl(d) => d,
                            _ => return None,
                        },
                        _ => s,
                    };
                    match &inner.kind {
                        StmtKind::EnumDecl(e)
                            if e.modifiers & MOD_DECLARE == 0
                                && (!e.is_const || preserve_const) =>
                        {
                            Some(e.name.clone())
                        }
                        StmtKind::ModuleDecl(m)
                            if m.modifiers & MOD_DECLARE == 0
                                && !module_decl_is_type_only(m, preserve_const) =>
                        {
                            if let ModuleName::Ident(ref n) = m.name {
                                Some(n.clone())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                })
                .collect()
        };
        let mut emitted_prologue = emitted_file_prologue;
        let mut emitted_import_local_export_idxs: HashSet<usize> = HashSet::new();
        for stmt in &file.statements {
            if stmt_is_export_assign(stmt) {
                continue;
            }
            let mut suppress_leading_for_stmt = false;
            let mut amd_import_source_override: Option<String> = None;
            // Skip import statements that are factory parameters.
            // Imports that need body emit (esModuleInterop) are NOT skipped.
            if let Some(imp) = Self::stmt_import_decl(stmt) {
                let needs_body_emit = amd_body_imports.contains(&imp.source);
                if !needs_body_emit {
                    // Emit namespace alias for combined imports at the import position:
                    // `import e1, * as e2 from 'mod'` → `const e2 = t1_3;`
                    if let ImportClause::Named {
                        namespace: Some(ns_name),
                        ..
                    } = &imp.specifiers
                    {
                        if let Some((_, param_var)) =
                            amd_combined_ns_aliases.iter().find(|(ns, _)| ns == ns_name)
                        {
                            self.write("const ");
                            self.write(ns_name);
                            self.write(" = ");
                            self.write(param_var);
                            self.writeln(";");
                        }
                    }
                    // For `export import a = require(...)` in AMD mode, the import
                    // is a factory parameter so we skip the import itself, but we
                    // still need to emit `exports.a = a;` for the export binding.
                    if let StmtKind::Export(ref ed) = stmt.kind {
                        if let ExportDeclKind::Decl(ref inner) = ed.kind {
                            if let StmtKind::Import(ref inner_imp) = inner.kind {
                                if let ImportClause::Require(ref name) = inner_imp.specifiers {
                                    if !inner_imp.type_only && !self.has_export_assign {
                                        if !self
                                            .type_only_require_specs
                                            .contains(inner_imp.source.as_str())
                                        {
                                            self.write("exports.");
                                            self.write(name);
                                            self.write(" = ");
                                            self.write(name);
                                            self.writeln(";");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !self.has_export_assign {
                        // Even when the import statement is skipped (factory param),
                        // we still need to emit named export bindings tied to it.
                        for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                            if emitted_import_local_export_idxs.contains(&idx) {
                                continue;
                            }
                            if !import_decl_binds_local(imp, local) {
                                continue;
                            }
                            if self.type_only_decl_names.contains(local.as_str())
                                && !self.cjs_import_map.contains_key(local.as_str())
                                && !file_value_bound_names.contains(local.as_str())
                            {
                                continue;
                            }
                            if !file_value_bound_names.contains(local.as_str())
                                && !self.cjs_import_map.contains_key(local.as_str())
                            {
                                continue;
                            }
                            if exported != "default" {
                                if let Some((var_name, prop)) =
                                    self.cjs_import_map.get(local.as_str()).cloned()
                                {
                                    if self.cjs_default_import_bindings.contains(local.as_str()) {
                                        // Default import: direct assignment
                                        self.write("exports.");
                                        self.write(exported);
                                        self.write(" = ");
                                        self.write(&var_name);
                                        self.write(".");
                                        self.write(&prop);
                                        self.writeln(";");
                                    } else {
                                        self.write("Object.defineProperty(exports, \"");
                                        self.write(exported);
                                        self.write(
                                            "\", { enumerable: true, get: function () { return ",
                                        );
                                        self.write(&var_name);
                                        self.write(".");
                                        self.write(&prop);
                                        self.writeln("; } });");
                                    }
                                } else {
                                    self.write("exports.");
                                    self.write(exported);
                                    self.write(" = ");
                                    self.emit_value_name_ref(local);
                                    self.writeln(";");
                                }
                            } else {
                                self.write("exports.");
                                self.write(exported);
                                self.write(" = ");
                                self.emit_value_name_ref(local);
                                self.writeln(";");
                            }
                            emitted_import_local_export_idxs.insert(idx);
                        }
                    }
                    self.advance_comment_pos(stmt.span.end);
                    continue;
                }
                if let Some(alias_name) = named_amd_dep_alias_by_path.get(&imp.source) {
                    amd_import_source_override = Some(alias_name.clone());
                }
                if self.is_umd() {
                    suppress_leading_for_stmt = true;
                }
            }
            if cjs_source_has_use_strict && is_use_strict_directive(stmt) {
                self.advance_comment_pos(stmt.span.end);
                continue;
            }
            let recoverable_fn_decl = matches!(
                &stmt.kind,
                StmtKind::FnDecl(fn_decl) if self.fn_decl_requires_recovery_emit(fn_decl)
            );
            let recoverable_type_alias = self.stmt_has_recoverable_type_alias_emit(stmt);
            let recoverable_reserved_word_module = matches!(
                &stmt.kind,
                StmtKind::ModuleDecl(module_decl)
                    if self.module_decl_has_reserved_word_recovery_shape(module_decl)
            );
            if stmt_is_erased(stmt, self.preserve_const_enums_effective())
                && !recoverable_fn_decl
                && !recoverable_type_alias
                && !recoverable_reserved_word_module
            {
                if !emitted_prologue {
                    self.emit_file_prologue_comments(stmt.span.start);
                    emitted_prologue = true;
                }
                self.advance_comment_pos(stmt.span.end);
                continue;
            }
            if let Some(import_decl) = Self::stmt_import_decl(stmt) {
                if !self.import_decl_will_emit(import_decl)
                    && !self.import_decl_has_reserved_word_recovery_shape(import_decl, stmt.span)
                {
                    self.advance_comment_pos(stmt.span.end);
                    continue;
                }
            }
            // Elide import-equals whose RHS has no runtime value.
            // Comments attached to elided import-equals should not appear.
            if let StmtKind::ImportEquals(ie) = &stmt.kind {
                if !self.import_equals_will_emit(&ie.name, &ie.module_ref) {
                    self.advance_comment_pos(stmt.span.end);
                    continue;
                }
            }
            // Suppress leading comments for exported var statements where
            // all declarators lack initializers (type-annotation-only).
            // TypeScript strips these comments because the declaration body
            // produces no output (only the preamble `exports.x = void 0;`).
            if !suppress_leading_for_stmt {
                let is_empty_export_var = match &stmt.kind {
                    StmtKind::Var(v) => {
                        v.modifiers & MOD_EXPORT != 0
                            && v.declarations.iter().all(|d| d.init.is_none())
                    }
                    StmtKind::Export(ed) => match &ed.kind {
                        ExportDeclKind::Decl(inner) => match &inner.kind {
                            StmtKind::Var(v) => v.declarations.iter().all(|d| d.init.is_none()),
                            _ => false,
                        },
                        _ => false,
                    },
                    _ => false,
                };
                if !is_empty_export_var {
                    self.emit_leading_comments(stmt.span.start);
                }
            }
            if let Some(alias_source) = amd_import_source_override {
                if let Some(import_decl) = Self::stmt_import_decl(stmt) {
                    let before = self.output.len();
                    self.emit_import_decl(import_decl);
                    let after = self.output.len();
                    if after > before && alias_source != import_decl.source {
                        let from = format!("require(\"{}\")", import_decl.source);
                        let to = format!("require(\"{}\")", alias_source);
                        let replaced = self.output[before..after].replace(&from, &to);
                        self.output.replace_range(before..after, &replaced);
                    }
                    if self.output.len() > before {
                        self.append_trailing_comment(stmt.span);
                    }
                } else {
                    self.emit_stmt(stmt);
                }
            } else {
                self.emit_stmt(stmt);
            }
            self.advance_comment_pos(stmt.span.end);
            if !self.has_export_assign {
                let declared_names: Vec<String> = match &stmt.kind {
                    StmtKind::Var(vs) if vs.modifiers & MOD_DECLARE == 0 => vs
                        .declarations
                        .iter()
                        .flat_map(|d| {
                            if let PatKind::Ident(name) = &d.name.kind {
                                // Uninitialized vars are already represented by
                                // the `exports.X = void 0;` pre-declaration.
                                if d.init.is_none() {
                                    for (idx, (_, local)) in local_named_exports.iter().enumerate()
                                    {
                                        if local == name {
                                            emitted_import_local_export_idxs.insert(idx);
                                        }
                                    }
                                    return vec![];
                                }
                                vec![name.to_string()]
                            } else {
                                let mut names = Vec::new();
                                collect_binding_names(&d.name, &mut names);
                                names
                            }
                        })
                        .collect(),
                    StmtKind::ClassDecl(cd) if cd.modifiers & MOD_DECLARE == 0 => {
                        let has_decos = self.options.experimental_decorators == Some(true)
                            && class_has_decorators(cd);
                        if has_decos
                            && self.export_target.as_deref() == Some("exports")
                            && !self.has_export_assign
                        {
                            if let Some(ref name) = cd.name {
                                for (idx, (_, local)) in local_named_exports.iter().enumerate() {
                                    if local == name {
                                        emitted_import_local_export_idxs.insert(idx);
                                    }
                                }
                            }
                            Vec::new()
                        } else {
                            cd.name.iter().cloned().collect()
                        }
                    }
                    // Handle exported declarations: extract names from inner decl
                    StmtKind::Export(ed) => match &ed.kind {
                        ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                            match &inner.kind {
                                StmtKind::Var(vs) if vs.modifiers & MOD_DECLARE == 0 => vs
                                    .declarations
                                    .iter()
                                    .filter_map(|d| {
                                        if d.init.is_some() {
                                            if let PatKind::Ident(name) = &d.name.kind {
                                                return Some(name.to_string());
                                            }
                                        }
                                        None
                                    })
                                    .collect(),
                                StmtKind::ClassDecl(cd) if cd.modifiers & MOD_DECLARE == 0 => {
                                    cd.name.iter().cloned().collect()
                                }
                                StmtKind::FnDecl(fd) if fd.modifiers & MOD_DECLARE == 0 => {
                                    fd.name.iter().cloned().collect()
                                }
                                _ => Vec::new(),
                            }
                        }
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                };
                if let Some(import_decl) = Self::stmt_import_decl(stmt) {
                    for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                        if emitted_import_local_export_idxs.contains(&idx) {
                            continue;
                        }
                        if !import_decl_binds_local(import_decl, local) {
                            continue;
                        }
                        if self.type_only_decl_names.contains(local.as_str())
                            && !self.cjs_import_map.contains_key(local.as_str())
                            && !file_value_bound_names.contains(local.as_str())
                        {
                            continue;
                        }
                        if !file_value_bound_names.contains(local.as_str())
                            && !self.cjs_import_map.contains_key(local.as_str())
                        {
                            continue;
                        }
                        if exported != "default" {
                            if let Some((var_name, prop)) =
                                self.cjs_import_map.get(local.as_str()).cloned()
                            {
                                if self.cjs_default_import_bindings.contains(local.as_str()) {
                                    self.write("exports.");
                                    self.write(exported);
                                    self.write(" = ");
                                    self.write(&var_name);
                                    self.write(".");
                                    self.write(&prop);
                                    self.writeln(";");
                                } else {
                                    self.write("Object.defineProperty(exports, \"");
                                    self.write(exported);
                                    self.write(
                                        "\", { enumerable: true, get: function () { return ",
                                    );
                                    self.write(&var_name);
                                    self.write(".");
                                    self.write(&prop);
                                    self.writeln("; } });");
                                }
                            } else {
                                self.write("exports.");
                                self.write(exported);
                                self.write(" = ");
                                self.emit_value_name_ref(local);
                                self.writeln(";");
                            }
                        } else {
                            self.write("exports.");
                            self.write(exported);
                            self.write(" = ");
                            self.emit_value_name_ref(local);
                            self.writeln(";");
                        }
                        emitted_import_local_export_idxs.insert(idx);
                    }
                }
                if !declared_names.is_empty() {
                    for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                        if emitted_import_local_export_idxs.contains(&idx) {
                            continue;
                        }
                        if !declared_names.contains(local) {
                            continue;
                        }
                        if self.type_only_decl_names.contains(local.as_str())
                            && !file_value_bound_names.contains(local.as_str())
                        {
                            continue;
                        }
                        if file_ns_enum_names.contains(local) {
                            continue;
                        }
                        self.write("exports.");
                        self.write(exported);
                        self.write(" = ");
                        // Var exports don't create standalone variables in AMD —
                        // they only exist on `exports`, so use `exports.local`.
                        if var_export_names.contains(local) {
                            self.write("exports.");
                        }
                        self.write(local);
                        self.writeln(";");
                        emitted_import_local_export_idxs.insert(idx);
                    }
                }
            }
            emitted_prologue = true;
        }
        if !self.has_export_assign {
            for (idx, (exported, local)) in local_named_exports.iter().enumerate() {
                if emitted_import_local_export_idxs.contains(&idx) {
                    continue;
                }
                // If the local name is type-only and has no value import rewrite,
                // there's no runtime value to export.
                if self.type_only_decl_names.contains(local.as_str())
                    && !self.cjs_import_map.contains_key(local.as_str())
                    && !file_value_bound_names.contains(local.as_str())
                {
                    continue;
                }
                if !file_value_bound_names.contains(local.as_str())
                    && !self.cjs_import_map.contains_key(local.as_str())
                {
                    continue;
                }
                if file_ns_enum_names.contains(local) {
                    continue;
                }
                if exported != "default" {
                    if let Some((var_name, prop)) = self.cjs_import_map.get(local.as_str()).cloned()
                    {
                        if self.cjs_default_import_bindings.contains(local.as_str()) {
                            self.write("exports.");
                            self.write(exported);
                            self.write(" = ");
                            self.write(&var_name);
                            self.write(".");
                            self.write(&prop);
                            self.writeln(";");
                        } else {
                            self.write("Object.defineProperty(exports, \"");
                            self.write(exported);
                            self.write("\", { enumerable: true, get: function () { return ");
                            self.write(&var_name);
                            self.write(".");
                            self.write(&prop);
                            self.writeln("; } });");
                        }
                    } else {
                        self.write("exports.");
                        self.write(exported);
                        self.write(" = ");
                        if var_export_names.contains(local) {
                            self.write("exports.");
                            self.write(local);
                        } else {
                            self.emit_value_name_ref(local);
                        }
                        self.writeln(";");
                    }
                } else {
                    self.write("exports.");
                    self.write(exported);
                    self.write(" = ");
                    if var_export_names.contains(local) {
                        self.write("exports.");
                        self.write(local);
                    } else {
                        self.emit_value_name_ref(local);
                    }
                    self.writeln(";");
                }
            }
        }
        // Emit export = (as return in AMD/UMD)
        for stmt in &file.statements {
            if stmt_is_export_assign(stmt) {
                self.emit_leading_comments(stmt.span.start);
                self.emit_stmt(stmt);
                self.advance_comment_pos(stmt.span.end);
                break;
            }
        }

        // Temps allocated while emitting a wrapped AMD/UMD body belong inside
        // the factory. The ordinary file-level insertion pass is intentionally
        // skipped for wrapped modules, so place these immediately after the
        // directive prologue and before the __esModule marker.
        self.resolve_deferred_export_name_placeholders();
        if self.temp_var_names.len() > wrapper_temp_start {
            let names = self.temp_var_names[wrapper_temp_start..]
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let declaration = format!("{}var {};\n", "    ".repeat(self.indent), names);
            self.insert_generated_text(wrapper_temp_insert_pos, &declaration);
        }

        self.indent -= 1;

        if self.is_umd() {
            self.writeln("});");
        } else {
            self.writeln("});");
        }
    }

    /// Find the amd-module name directive in triple-slash comments.
    /// `/// <amd-module name='ModuleName'/>` → Some("ModuleName")
    pub(super) fn find_amd_module_name(&self, file: &SourceFile) -> Option<String> {
        // TypeScript uses the LAST such directive if there are multiple.
        let mut result = None;
        for line in self.source.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed
                .strip_prefix("///<amd-module")
                .or_else(|| trimmed.strip_prefix("/// <amd-module"))
            {
                if let Some(name) = Self::extract_triple_slash_attr(rest, "name") {
                    result = Some(name);
                }
            }
        }
        if !(self.is_amd() || self.is_system() || self.is_umd()) {
            return None;
        }
        if result.is_some() {
            return result;
        }
        // When bundling (outFile is set), use the module name passed from the
        // harness via the "bundledModuleName" compiler option, or derive from
        // the file path as a fallback.
        if self.options.out_file.is_some() {
            if let Some(name) = super::get_other_option(self.options, "bundledmodulename") {
                return Some(name);
            }
            let name = &file.file_name;
            let module_name = name
                .strip_suffix(".ts")
                .or_else(|| name.strip_suffix(".tsx"))
                .or_else(|| name.strip_suffix(".js"))
                .or_else(|| name.strip_suffix(".jsx"))
                .unwrap_or(name);
            return Some(module_name.to_string());
        }
        None
    }

    pub(super) fn collect_amd_dependency_directives(&self) -> (Vec<(String, String)>, Vec<String>) {
        let mut named: Vec<(String, String)> = Vec::new();
        let mut unnamed: Vec<String> = Vec::new();
        for line in self.source.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed
                .strip_prefix("///<amd-dependency")
                .or_else(|| trimmed.strip_prefix("/// <amd-dependency"))
            else {
                continue;
            };
            let Some(path) = Self::extract_triple_slash_attr(rest, "path") else {
                continue;
            };
            if let Some(name) = Self::extract_triple_slash_attr(rest, "name") {
                named.push((path, name));
            } else {
                unnamed.push(path);
            }
        }
        (named, unnamed)
    }

    pub(super) fn extract_triple_slash_attr(text: &str, attr_name: &str) -> Option<String> {
        let needle = format!("{attr_name}=");
        let start = text.find(&needle)?;
        let after = &text[start + needle.len()..];
        let mut chars = after.chars();
        let quote = chars.next()?;
        if quote != '\'' && quote != '"' {
            return None;
        }
        let inner = &after[quote.len_utf8()..];
        let end = inner.find(quote)?;
        Some(inner[..end].to_string())
    }

    pub(super) fn first_quoted_literal<'b>(&self, text: &'b str) -> Option<&'b str> {
        let mut start_idx: Option<usize> = None;
        let mut quote = '\0';
        for (i, ch) in text.char_indices() {
            if start_idx.is_none() {
                if ch == '\'' || ch == '"' {
                    start_idx = Some(i);
                    quote = ch;
                }
                continue;
            }
            if ch == quote {
                let s = start_idx?;
                return Some(&text[s..=i]);
            }
        }
        None
    }

    fn import_specifier_has_invalid_unicode_escape_local(&self, spec: &ImportSpecifier) -> bool {
        if spec.local != "<error>" {
            return false;
        }
        let start = spec.span.start as usize;
        let end = spec.span.end as usize;
        if !(start < end && end <= self.source.len()) {
            return false;
        }
        let normalized = self.source[start..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        normalized
            .rsplit_once(" as ")
            .is_some_and(|(_, local_tail)| local_tail.contains("\\u"))
    }

    fn import_specifier_has_string_literal_local_error(&self, spec: &ImportSpecifier) -> bool {
        if spec.local != "<error>" {
            return false;
        }
        let start = spec.span.start as usize;
        let end = spec.span.end as usize;
        if !(start < end && end <= self.source.len()) {
            return false;
        }
        let normalized = self.source[start..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if let Some((_, local_tail)) = normalized.rsplit_once(" as ") {
            let local_tail = local_tail.trim_start();
            return local_tail.starts_with('"') || local_tail.starts_with('\'');
        }
        normalized.starts_with('"')
            || normalized.starts_with('\'')
            || normalized.starts_with("type \"")
            || normalized.starts_with("type '")
    }

    pub(super) fn import_decl_has_same_line_string_tail_elision_recovery(
        &self,
        import_decl: &ImportDecl,
    ) -> bool {
        if !import_decl.source.is_empty() {
            return false;
        }
        let raw = self.copy_span_trimmed(import_decl.span).trim();
        if raw.is_empty() {
            return false;
        }
        let start = (import_decl.span.end as usize).min(self.source.len());
        let rest = &self.source[start..];
        let eol = rest.find('\n').unwrap_or(rest.len());
        let line_tail = &rest[..eol];
        let norm = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        let tail_norm = line_tail.split_whitespace().collect::<Vec<_>>().join(" ");
        let tail_trim = line_tail.trim_start();
        norm.starts_with("import ")
            && norm.ends_with(" as as")
            && (tail_trim.starts_with('"') || tail_trim.starts_with('\''))
            && tail_norm.contains(" from ")
    }

    /// Emit parser-recovery forms for malformed imports where the parser
    /// produced an ImportDecl with an empty source.
    pub(super) fn emit_recovery_import_decl(&mut self, import_decl: &ImportDecl) -> bool {
        if !import_decl.source.is_empty() {
            // `import { 0n as foo } from "x"` / `import { foo as 0n } from "x"`
            // recover to:
            //   from;
            //   "x";
            // while still marking the file as a module.
            if let ImportClause::Named {
                default,
                named,
                namespace,
            } = &import_decl.specifiers
            {
                // Only trigger recovery when the import is truly malformed (e.g.,
                // `import { 0n as foo }`).  Skip recovery for imports where a
                // reserved keyword was used as a binding name (e.g.,
                // `import { yield as default }`) — those should be elided via
                // normal import elision since the keyword-as-binding is the only
                // error and the import is structurally valid.
                let has_truly_malformed = named.iter().any(|s| {
                    if s.local != "<error>" {
                        return false;
                    }
                    if self.import_specifier_has_string_literal_local_error(s) {
                        return false;
                    }
                    // If the error token at the specifier span's end was a keyword,
                    // it's a "keyword as binding" case → don't trigger recovery.
                    // Check by looking at the source text for the local name position.
                    let end = s.span.end as usize;
                    if end > 0 && end <= self.source.len() {
                        // Walk backwards from span end to find the token that failed
                        let before = self.source[..end].trim_end();
                        // Check the last word — if it's a JS keyword, skip recovery
                        let last_word = before
                            .rsplit(|c: char| c.is_whitespace())
                            .next()
                            .unwrap_or("");
                        let is_keyword = matches!(
                            last_word,
                            "break"
                                | "case"
                                | "catch"
                                | "class"
                                | "const"
                                | "continue"
                                | "debugger"
                                | "default"
                                | "delete"
                                | "do"
                                | "else"
                                | "enum"
                                | "export"
                                | "extends"
                                | "false"
                                | "finally"
                                | "for"
                                | "function"
                                | "if"
                                | "import"
                                | "in"
                                | "instanceof"
                                | "new"
                                | "null"
                                | "return"
                                | "super"
                                | "switch"
                                | "this"
                                | "throw"
                                | "true"
                                | "try"
                                | "typeof"
                                | "var"
                                | "void"
                                | "while"
                                | "with"
                        );
                        !is_keyword
                    } else {
                        true // can't check, treat as malformed
                    }
                });
                let malformed_named =
                    default.is_none() && namespace.is_none() && has_truly_malformed;
                if malformed_named {
                    if named
                        .iter()
                        .any(|s| self.import_specifier_has_invalid_unicode_escape_local(s))
                    {
                        return true;
                    }
                    let q = self.detect_string_quote(import_decl.span);
                    self.writeln("from;");
                    self.write(q);
                    self.write(&import_decl.source);
                    self.write(q);
                    self.writeln(";");
                    return true;
                }
            }
            return false;
        }
        let raw = self.copy_span_trimmed(import_decl.span).trim();
        if raw.is_empty() {
            return false;
        }
        let line_tail = {
            let start = (import_decl.span.end as usize).min(self.source.len());
            let rest = &self.source[start..];
            let eol = rest.find('\n').unwrap_or(rest.len());
            &rest[..eol]
        };
        let tail_norm = line_tail.split_whitespace().collect::<Vec<_>>().join(" ");
        let norm = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if self.import_decl_has_same_line_string_tail_elision_recovery(import_decl) {
            let line_end = (import_decl.span.end as usize)
                .saturating_add(line_tail.len())
                .min(self.source.len()) as u32;
            self.skip_recovery_until = self.skip_recovery_until.max(line_end);
            return true;
        }
        // `import Foo From "./Foo";` parser recovery shape:
        //   import decl: `import Foo` with empty source
        //   trailing source on same line: `From "./Foo";`
        // Emit:
        //   var Foo = From;
        //   "./Foo"; // ...
        // and skip parser-artifact follow-up statements from the same line.
        if norm.starts_with("import ") && !norm.contains('{') && !norm.contains('*') {
            let local = norm["import ".len()..].trim();
            let tail_trim = line_tail.trim_start();
            if !local.is_empty() && tail_trim.starts_with("From ") {
                if let Some(lit) = self.first_quoted_literal(line_tail) {
                    let line_end = (import_decl.span.end as usize)
                        .saturating_add(line_tail.len())
                        .min(self.source.len()) as u32;
                    self.skip_recovery_until = self.skip_recovery_until.max(line_end);
                    self.write("var ");
                    self.write(local);
                    self.write(" = From;");
                    self.newline();
                    self.write(lit);
                    self.write(";");
                    if let Some(comment_idx) = line_tail.find("//") {
                        let comment = line_tail[comment_idx..].trim_end();
                        if !comment.is_empty() {
                            self.write(" ");
                            self.write(comment);
                        }
                    }
                    self.newline();
                    return true;
                }
            }
        }
        // `import` token recovery with the remainder still present on the line.
        if raw == "import" {
            let tail_trim = line_tail.trim_start();
            // `import , { a } from "x";` parser recovery shape:
            //   import
            //   , { a }
            //   from
            //   "x";
            // TypeScript emits:
            //   {
            //       a;
            //   }
            //   from;
            //   "x";
            if tail_trim.starts_with(", {") {
                if let (Some(open), Some(close), Some(lit)) = (
                    tail_trim.find('{'),
                    tail_trim.rfind('}'),
                    self.first_quoted_literal(line_tail),
                ) {
                    if close > open {
                        let line_end = (import_decl.span.end as usize)
                            .saturating_add(line_tail.len())
                            .min(self.source.len()) as u32;
                        self.skip_recovery_until = self.skip_recovery_until.max(line_end);
                        let inside = tail_trim[open + 1..close].trim();
                        self.writeln("{");
                        if !inside.is_empty() {
                            self.indent += 1;
                            for item in inside.split(',') {
                                let item = item.trim();
                                if item.is_empty() {
                                    continue;
                                }
                                self.write(item);
                                self.writeln(";");
                            }
                            self.indent -= 1;
                        }
                        self.writeln("}");
                        self.writeln("from;");
                        self.write(lit);
                        self.writeln(";");
                        return true;
                    }
                }
            }
        }
        if raw == "import" && tail_norm.contains(" from ") {
            if let Some(lit) = self.first_quoted_literal(line_tail) {
                self.writeln("from;");
                self.write(lit);
                self.writeln(";");
                return true;
            }
        }
        // Bare `import` token with nothing else on the same line.
        // TypeScript keeps a recovery marker statement in JS output.
        if raw == "import" {
            let tail = line_tail.trim_start();
            // Type arguments are not valid on the `import` keyword. Recovery
            // keeps the keyword as an expression and strips the type arguments
            // from a following variable initializer using the same shape.
            if tail.starts_with('<') {
                let source_tail = &self.source[import_decl.span.end as usize..];
                let after_line = source_tail
                    .split_once('\n')
                    .map(|(_, after)| after)
                    .unwrap_or_default();
                let next_line_raw = after_line.lines().next().unwrap_or_default();
                let next_line = next_line_raw.trim();
                if let Some((binding, _)) = next_line.split_once("= import<") {
                    let binding = binding.trim_end();
                    if binding.starts_with("const ")
                        || binding.starts_with("let ")
                        || binding.starts_with("var ")
                    {
                        let consumed = source_tail.len() - after_line.len() + next_line_raw.len();
                        self.skip_recovery_until = self
                            .skip_recovery_until
                            .max(import_decl.span.end.saturating_add(consumed as u32));
                        self.writeln("import;");
                        self.write(binding);
                        self.writeln(" = (import);");
                        return true;
                    }
                }
                self.writeln("import;");
                return true;
            }
            // `import 10;` recovery: parser splits into `import` + `10;`.
            // TypeScript drops the `import` token marker and keeps `10;`.
            if tail.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return true;
            }
            self.writeln("import ;");
            return true;
        }
        // `import { a }, from "x";` may recover as `import { a }` with
        // `, from "x"` remaining on the source line.
        if norm.starts_with("import {") && tail_norm.starts_with(", from ") {
            if let Some(lit) = self.first_quoted_literal(line_tail) {
                let line_end = (import_decl.span.end as usize)
                    .saturating_add(line_tail.len())
                    .min(self.source.len()) as u32;
                self.skip_recovery_until = self.skip_recovery_until.max(line_end);
                self.write(raw);
                self.writeln(" from , from;");
                self.write(lit);
                self.writeln(";");
                return true;
            }
        }
        if let Some(lit) = self.first_quoted_literal(raw) {
            // `import { * } from "x";` recovery shape in baselines.
            if norm.starts_with("import { * } from ") || norm.starts_with("import {*} from ") {
                self.writeln("from;");
                self.write(lit);
                self.writeln(";");
                return true;
            }
            // `import { a }, from "x";` recovery shape in baselines.
            if norm.starts_with("import {") && norm.contains("}, from ") {
                if let Some(close_brace_idx) = raw.find('}') {
                    let prefix = raw[..=close_brace_idx].trim_end();
                    self.write(prefix);
                    self.writeln(" from , from;");
                    self.write(lit);
                    self.writeln(";");
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn emit_recovery_export_decl(&mut self, export_decl: &ExportDecl) -> bool {
        if self.export_decl_is_invalid_expr_recovery(export_decl) {
            let ExportDeclKind::Decl(inner) = &export_decl.kind else {
                return false;
            };
            if let StmtKind::Expr(expr) = &inner.kind {
                if let ExprKind::Ident(keyword) = &expr.kind {
                    self.write(keyword);
                    self.writeln(";");
                    return true;
                }
            }
        }

        let ExportDeclKind::Named {
            specifiers,
            source,
            type_only,
        } = &export_decl.kind
        else {
            return false;
        };
        if *type_only || source.is_some() {
            return false;
        }

        let raw = self.copy_span_trimmed(export_decl.span).trim().to_string();
        if raw.is_empty() {
            return false;
        }
        let norm = raw.split_whitespace().collect::<Vec<_>>().join(" ");

        // `export { foo as 0n };` -> `export { foo as  }; 0n; ;`
        // Don't trigger recovery for valid string literal names like `export { x as "" }`
        if specifiers.len() == 1
            && !specifiers[0].exported_is_string
            && specifiers[0]
                .exported
                .as_ref()
                .is_some_and(|e| e.is_empty())
        {
            let local = &specifiers[0].local;
            let invalid = if let Some(as_idx) = norm.find(" as ") {
                let after_as = &norm[as_idx + 4..];
                after_as
                    .split('}')
                    .next()
                    .map(str::trim)
                    .unwrap_or_default()
                    .to_string()
            } else {
                String::new()
            };
            self.write("export { ");
            self.emit_module_export_name(local);
            self.writeln(" as  };");
            if !invalid.is_empty() {
                self.write(&invalid);
                self.writeln(";");
            }
            self.writeln(";");
            self.emitted_esm_export = true;
            return true;
        }

        // `export { 0n as foo };` -> `0n; ;`
        if specifiers.is_empty() {
            if let Some(open_idx) = norm.find('{') {
                let after_open = &norm[open_idx + 1..];
                if let Some(as_idx) = after_open.find(" as ") {
                    let invalid = after_open[..as_idx].trim();
                    if !invalid.is_empty() {
                        self.write(invalid);
                        self.writeln(";");
                        self.writeln(";");
                        return true;
                    }
                }
            }
        }

        false
    }
}
