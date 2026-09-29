//! Await-to-yield transform and dynamic import detection for the emitter.

use super::*;

impl<'a> Emitter<'a> {
    fn emit_async_generator_inner_params<'p>(
        &mut self,
        params: &'p [Param],
    ) -> Vec<(usize, String, &'p Pat, Option<&'p Expr>)> {
        if self.params_need_rest_transform(params) {
            self.emit_params_with_rest_transform(params)
        } else {
            self.emit_params(params);
            vec![]
        }
    }

    fn emit_async_generator_single_line_body<'p>(
        &mut self,
        body: &[Stmt],
        rest_infos: &[(usize, String, &'p Pat, Option<&'p Expr>)],
    ) {
        let preserve_const = self.preserve_const_enums_effective();
        let body_count = body
            .iter()
            .filter(|s| !stmt_is_erased(s, preserve_const))
            .count();
        let total = rest_infos.len() + body_count;
        let mut emitted = 0usize;
        for (_, temp_name, pat, _) in rest_infos {
            self.emit_rest_param_destructuring_await_to_yield(temp_name, pat);
            if self.output.ends_with('\n') {
                self.output.pop();
                self.at_line_start = false;
            }
            emitted += 1;
            if emitted < total {
                self.write(" ");
            }
        }
        for stmt in body {
            if stmt_is_erased(stmt, preserve_const) {
                continue;
            }
            self.emit_stmt_await_to_yield(stmt);
            if self.output.ends_with('\n') {
                self.output.pop();
                self.at_line_start = false;
            }
            emitted += 1;
            if emitted < total {
                self.write(" ");
            }
        }
    }

    fn collect_async_shadow_param_names(params: &[Param]) -> HashSet<AstString> {
        let mut names = HashSet::new();
        for param in params {
            let mut bound = Vec::new();
            collect_binding_names(&param.name, &mut bound);
            names.extend(bound.into_iter().map(AstString::from));
        }
        names
    }

    fn binding_name_set(pat: &Pat) -> HashSet<AstString> {
        let mut names = Vec::new();
        collect_binding_names(pat, &mut names);
        names.into_iter().map(AstString::from).collect()
    }

    fn async_var_shadow_name_blocked(blockers: &[HashSet<AstString>], name: &str) -> bool {
        blockers.iter().rev().any(|blocked| blocked.contains(name))
    }

    fn async_var_shadow_var_stmt_needs_transform_in_scope(
        var_stmt: &VarStmt,
        param_names: &HashSet<AstString>,
        blockers: &[HashSet<AstString>],
    ) -> bool {
        var_stmt.kind == VarKind::Var
            && var_stmt.modifiers & MOD_DECLARE == 0
            && var_stmt.declarations.iter().any(|decl| {
                let mut bound = Vec::new();
                collect_binding_names(&decl.name, &mut bound);
                bound.into_iter().any(|name| {
                    param_names.contains(name.as_str())
                        && !Self::async_var_shadow_name_blocked(blockers, &name)
                })
            })
    }

    fn async_var_shadow_var_stmt_needs_transform(&self, var_stmt: &VarStmt) -> bool {
        self.async_var_shadow_names
            .as_ref()
            .is_some_and(|param_names| {
                Self::async_var_shadow_var_stmt_needs_transform_in_scope(
                    var_stmt,
                    param_names,
                    &self.async_var_shadow_blockers,
                )
            })
    }

    fn async_var_shadow_push_names_from_var_stmt(
        var_stmt: &VarStmt,
        ordered: &mut Vec<AstString>,
        seen: &mut HashSet<AstString>,
    ) {
        for decl in &var_stmt.declarations {
            let mut names = Vec::new();
            collect_binding_names(&decl.name, &mut names);
            for name in names {
                let name: AstString = name.into();
                if seen.insert(name.clone()) {
                    ordered.push(name);
                }
            }
        }
    }

    fn collect_async_var_shadow_hoists_in_stmt(
        stmt: &Stmt,
        param_names: &HashSet<AstString>,
        blockers: &mut Vec<HashSet<AstString>>,
        ordered: &mut Vec<AstString>,
        seen: &mut HashSet<AstString>,
    ) {
        match &stmt.kind {
            StmtKind::Var(var_stmt) => {
                if Self::async_var_shadow_var_stmt_needs_transform_in_scope(
                    var_stmt,
                    param_names,
                    blockers,
                ) {
                    Self::async_var_shadow_push_names_from_var_stmt(var_stmt, ordered, seen);
                }
            }
            StmtKind::Block(stmts) => {
                for stmt in stmts {
                    Self::collect_async_var_shadow_hoists_in_stmt(
                        stmt,
                        param_names,
                        blockers,
                        ordered,
                        seen,
                    );
                }
            }
            StmtKind::If(if_stmt) => {
                Self::collect_async_var_shadow_hoists_in_stmt(
                    &if_stmt.consequent,
                    param_names,
                    blockers,
                    ordered,
                    seen,
                );
                if let Some(alt) = &if_stmt.alternate {
                    Self::collect_async_var_shadow_hoists_in_stmt(
                        alt,
                        param_names,
                        blockers,
                        ordered,
                        seen,
                    );
                }
            }
            StmtKind::While(wh) => Self::collect_async_var_shadow_hoists_in_stmt(
                &wh.body,
                param_names,
                blockers,
                ordered,
                seen,
            ),
            StmtKind::DoWhile(dw) => Self::collect_async_var_shadow_hoists_in_stmt(
                &dw.body,
                param_names,
                blockers,
                ordered,
                seen,
            ),
            StmtKind::For(for_stmt) => {
                if let Some(ForInit::Var(var_stmt)) = &for_stmt.init {
                    if Self::async_var_shadow_var_stmt_needs_transform_in_scope(
                        var_stmt,
                        param_names,
                        blockers,
                    ) {
                        Self::async_var_shadow_push_names_from_var_stmt(var_stmt, ordered, seen);
                    }
                }
                Self::collect_async_var_shadow_hoists_in_stmt(
                    &for_stmt.body,
                    param_names,
                    blockers,
                    ordered,
                    seen,
                );
            }
            StmtKind::ForIn(for_in) => {
                if let ForInOfLeft::Var(var_stmt) = &for_in.left {
                    if Self::async_var_shadow_var_stmt_needs_transform_in_scope(
                        var_stmt,
                        param_names,
                        blockers,
                    ) {
                        Self::async_var_shadow_push_names_from_var_stmt(var_stmt, ordered, seen);
                    }
                }
                Self::collect_async_var_shadow_hoists_in_stmt(
                    &for_in.body,
                    param_names,
                    blockers,
                    ordered,
                    seen,
                );
            }
            StmtKind::ForOf(for_of) => {
                if let ForInOfLeft::Var(var_stmt) = &for_of.left {
                    if Self::async_var_shadow_var_stmt_needs_transform_in_scope(
                        var_stmt,
                        param_names,
                        blockers,
                    ) {
                        Self::async_var_shadow_push_names_from_var_stmt(var_stmt, ordered, seen);
                    }
                }
                Self::collect_async_var_shadow_hoists_in_stmt(
                    &for_of.body,
                    param_names,
                    blockers,
                    ordered,
                    seen,
                );
            }
            StmtKind::Switch(sw) => {
                for case in &sw.cases {
                    for stmt in &case.consequent {
                        Self::collect_async_var_shadow_hoists_in_stmt(
                            stmt,
                            param_names,
                            blockers,
                            ordered,
                            seen,
                        );
                    }
                }
            }
            StmtKind::Labeled(labeled) => Self::collect_async_var_shadow_hoists_in_stmt(
                &labeled.body,
                param_names,
                blockers,
                ordered,
                seen,
            ),
            StmtKind::With(with_stmt) => Self::collect_async_var_shadow_hoists_in_stmt(
                &with_stmt.body,
                param_names,
                blockers,
                ordered,
                seen,
            ),
            StmtKind::Try(try_stmt) => {
                for stmt in &try_stmt.block {
                    Self::collect_async_var_shadow_hoists_in_stmt(
                        stmt,
                        param_names,
                        blockers,
                        ordered,
                        seen,
                    );
                }
                if let Some(handler) = &try_stmt.handler {
                    if let Some(param) = &handler.param {
                        blockers.push(Self::binding_name_set(param));
                    }
                    for stmt in &handler.body {
                        Self::collect_async_var_shadow_hoists_in_stmt(
                            stmt,
                            param_names,
                            blockers,
                            ordered,
                            seen,
                        );
                    }
                    if handler.param.is_some() {
                        blockers.pop();
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for stmt in finalizer {
                        Self::collect_async_var_shadow_hoists_in_stmt(
                            stmt,
                            param_names,
                            blockers,
                            ordered,
                            seen,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_async_var_shadow_hoists(
        stmts: &[Stmt],
        param_names: &HashSet<AstString>,
    ) -> Vec<AstString> {
        let mut blockers = Vec::new();
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        for stmt in stmts {
            Self::collect_async_var_shadow_hoists_in_stmt(
                stmt,
                param_names,
                &mut blockers,
                &mut ordered,
                &mut seen,
            );
        }
        ordered
    }

    fn emit_pending_async_var_shadow_hoists_inline(&mut self) -> bool {
        if self.async_var_shadow_top_level_hoists.is_empty() {
            return false;
        }
        let hoists = std::mem::take(&mut self.async_var_shadow_top_level_hoists);
        self.write("var ");
        for (i, name) in hoists.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(name);
        }
        self.write(";");
        true
    }

    fn emit_pending_async_var_shadow_hoists_multiline(&mut self) {
        if !self.emit_pending_async_var_shadow_hoists_inline() {
            return;
        }
        self.newline();
    }

    fn emit_async_var_shadow_decl_assignment_expr(
        &mut self,
        decl: &VarDeclarator,
        in_for_header: bool,
    ) -> bool {
        let Some(init) = decl.init.as_ref() else {
            return false;
        };
        let needs_obj_rest =
            self.needs_downlevel("object-spread") && pat_has_object_rest(&decl.name);
        let needs_object_parens = !in_for_header
            && match &decl.name.kind {
                PatKind::Object(props) if needs_obj_rest => props
                    .iter()
                    .any(|prop| !matches!(prop, ObjPatProp::Rest(_))),
                PatKind::Object(_) => true,
                _ => false,
            };
        if needs_object_parens {
            self.write("(");
        }
        if needs_obj_rest {
            self.emit_var_declarator_object_rest_transform(decl);
        } else {
            self.emit_binding_name(&decl.name);
            self.write(" = ");
            self.emit_expr_await_to_yield(init);
        }
        if needs_object_parens {
            self.write(")");
        }
        true
    }

    fn emit_async_var_shadow_var_stmt_rewrite(&mut self, var_stmt: &VarStmt) -> bool {
        let mut emitted = false;
        for decl in &var_stmt.declarations {
            if decl.init.is_none() {
                continue;
            }
            if emitted {
                self.writeln(";");
            }
            self.emit_async_var_shadow_decl_assignment_expr(decl, false);
            emitted = true;
        }
        if emitted {
            self.writeln(";");
        }
        emitted
    }

    fn emit_async_var_shadow_for_init_rewrite(&mut self, var_stmt: &VarStmt) {
        let mut first = true;
        for decl in &var_stmt.declarations {
            if decl.init.is_none() {
                continue;
            }
            if !first {
                self.write(", ");
            }
            self.emit_async_var_shadow_decl_assignment_expr(decl, true);
            first = false;
        }
    }

    fn emit_async_var_shadow_for_in_of_left_rewrite(&mut self, var_stmt: &VarStmt) {
        if let Some(decl) = var_stmt.declarations.first() {
            self.emit_binding_name(&decl.name);
        }
    }

    pub(crate) fn can_emit_simple_for_await_downlevel(
        &self,
        fo: &ForOfStmt,
    ) -> Option<(&'static str, String)> {
        let ForInOfLeft::Var(var_stmt) = &fo.left else {
            return None;
        };
        if var_stmt.declarations.len() != 1 {
            return None;
        }
        let decl = &var_stmt.declarations[0];
        if decl.init.is_some() {
            return None;
        }
        let PatKind::Ident(name) = &decl.name.kind else {
            return None;
        };
        let kw = match var_stmt.kind {
            VarKind::Var => "var",
            VarKind::Let => "let",
            VarKind::Const => "const",
            _ => return None,
        };
        Some((kw, name.to_string()))
    }

    pub(super) fn simple_for_of_using_binding(&self, fo: &ForOfStmt) -> Option<(String, bool)> {
        let ForInOfLeft::Var(var_stmt) = &fo.left else {
            return None;
        };
        if var_stmt.declarations.len() != 1 {
            return None;
        }
        let declaration = &var_stmt.declarations[0];
        if declaration.init.is_some() {
            return None;
        }
        let PatKind::Ident(name) = &declaration.name.kind else {
            return None;
        };
        match var_stmt.kind {
            VarKind::Using => Some((name.to_string(), false)),
            VarKind::AwaitUsing => Some((name.to_string(), true)),
            _ => None,
        }
    }

    /// Find the iterable identifier name from a for-await statement in the given
    /// statements. Returns `Some(name)` if the first for-await has a simple
    /// identifier iterable (e.g. `for await (... of x)`), None otherwise.
    fn find_for_await_ident_name(stmts: &[Stmt]) -> Option<String> {
        fn find(stmt: &Stmt) -> Option<Option<String>> {
            match &stmt.kind {
                StmtKind::ForOf(fo) if fo.is_await => Some(match &fo.right.kind {
                    ExprKind::Ident(name) => Some(name.to_string()),
                    _ => None,
                }),
                StmtKind::Labeled(labeled) => find(&labeled.body),
                StmtKind::Block(stmts) => stmts.iter().find_map(find),
                _ => None,
            }
        }
        for stmt in stmts {
            if let Some(result) = find(stmt) {
                return result;
            }
        }
        None
    }

    fn emit_simple_for_await_downlevel(&mut self, fo: &ForOfStmt) {
        let Some((binding_kw, binding_name)) = self.can_emit_simple_for_await_downlevel(fo) else {
            return;
        };
        // Check if iterable is a simple identifier — different temp naming scheme.
        let ident_iterable = if let ExprKind::Ident(name) = &fo.right.kind {
            Some(name.to_string())
        } else {
            None
        };
        self.writeln("try {");
        self.indent += 1;
        if let Some(ref iter_name) = ident_iterable {
            // Simple identifier: hoisted vars, name-based temps, no `var` in for-init.
            let iter_var = format!("{iter_name}_1");
            let result_var = format!("{iter_name}_1_1");
            self.write(&format!("for (_d = true, {iter_var} = "));
            self.write(self.helper_prefix());
            self.write("__asyncValues(");
            self.emit_expr_await_to_yield(&fo.right);
            self.writeln(&format!(
                "); {result_var} = yield {iter_var}.next(), _a = {result_var}.done, !_a; _d = true) {{"
            ));
            self.indent += 1;
            self.writeln(&format!("_c = {result_var}.value;"));
        } else {
            // Complex expression: for-init `var` with temp names.
            self.write("for (var _d = true, _e = ");
            self.write(self.helper_prefix());
            self.write("__asyncValues(");
            self.emit_expr_await_to_yield(&fo.right);
            self.writeln("), _f; _f = yield _e.next(), _a = _f.done, !_a; _d = true) {");
            self.indent += 1;
            self.writeln("_c = _f.value;");
        }
        self.writeln("_d = false;");
        self.write(binding_kw);
        self.write(" ");
        self.write(&binding_name);
        self.writeln(" = _c;");
        match &fo.body.kind {
            StmtKind::Block(stmts) => self.emit_async_stmt_list_with_using(stmts),
            _ => self.emit_stmt_await_to_yield(&fo.body),
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("catch (e_1_1) { e_1 = { error: e_1_1 }; }");
        self.writeln("finally {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        if let Some(ref iter_name) = ident_iterable {
            let iter_var = format!("{iter_name}_1");
            self.writeln(&format!(
                "if (!_d && !_a && (_b = {iter_var}.return)) yield _b.call({iter_var});"
            ));
        } else {
            self.writeln("if (!_d && !_a && (_b = _e.return)) yield _b.call(_e);");
        }
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally { if (e_1) throw e_1.error; }");
        self.indent -= 1;
        self.writeln("}");
    }

    fn next_resource_for_await_suffix(&mut self, iterable_ident: Option<&str>) -> Option<usize> {
        self.resource_for_await_counter += 1;
        let classic_names = ["_a", "e_1", "_b", "_c", "_d", "_e", "_f", "e_1_1"];
        let classic_iter_names_are_safe = iterable_ident.is_none_or(|name| {
            !self.source_has_identifier(&format!("{name}_1"))
                && !self.source_has_identifier(&format!("{name}_1_1"))
        });
        if self.resource_for_await_counter == 1
            && classic_names
                .iter()
                .all(|name| !self.source_has_identifier(name))
            && classic_iter_names_are_safe
        {
            return None;
        }
        let mut suffix = self.resource_for_await_counter;
        loop {
            let names = [
                format!("_forAwaitDone_{suffix}"),
                format!("_forAwaitError_{suffix}"),
                format!("_forAwaitReturn_{suffix}"),
                format!("_forAwaitValue_{suffix}"),
                format!("_forAwaitNeedsClose_{suffix}"),
                format!("_forAwaitIterator_{suffix}"),
                format!("_forAwaitResult_{suffix}"),
                format!("_forAwaitCaught_{suffix}"),
            ];
            if names.iter().all(|name| !self.source_has_identifier(name)) {
                self.resource_for_await_counter = suffix;
                return Some(suffix);
            }
            suffix += 1;
        }
    }

    fn emit_simple_for_await_using_downlevel(&mut self, fo: &ForOfStmt, labels: &[&str]) {
        let Some((binding_name, is_async)) = self.simple_for_of_using_binding(fo) else {
            return;
        };
        let ident_iterable = if let ExprKind::Ident(name) = &fo.right.kind {
            Some(name.to_string())
        } else {
            None
        };
        let suffix = self.next_resource_for_await_suffix(ident_iterable.as_deref());
        let (done, error, return_fn, value, needs_close, iterator, result, caught) =
            if let Some(n) = suffix {
                let names = (
                    format!("_forAwaitDone_{n}"),
                    format!("_forAwaitError_{n}"),
                    format!("_forAwaitReturn_{n}"),
                    format!("_forAwaitValue_{n}"),
                    format!("_forAwaitNeedsClose_{n}"),
                    format!("_forAwaitIterator_{n}"),
                    format!("_forAwaitResult_{n}"),
                    format!("_forAwaitCaught_{n}"),
                );
                self.writeln(&format!(
                    "var {}, {}, {}, {}, {}, {}, {};",
                    names.0, names.1, names.2, names.3, names.4, names.5, names.6
                ));
                names
            } else {
                (
                    "_a".into(),
                    "e_1".into(),
                    "_b".into(),
                    "_c".into(),
                    "_d".into(),
                    ident_iterable
                        .as_ref()
                        .map_or_else(|| "_e".into(), |name| format!("{name}_1")),
                    ident_iterable
                        .as_ref()
                        .map_or_else(|| "_f".into(), |name| format!("{name}_1_1")),
                    "e_1_1".into(),
                )
            };
        self.writeln("try {");
        self.indent += 1;
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        let declare_iterator_state_in_for = suffix.is_none() && ident_iterable.is_none();
        if declare_iterator_state_in_for {
            self.write(&format!("for (var {needs_close} = true, {iterator} = "));
        } else {
            self.write(&format!("for ({needs_close} = true, {iterator} = "));
        }
        self.write(self.helper_prefix());
        self.write("__asyncValues(");
        self.emit_expr_await_to_yield(&fo.right);
        if declare_iterator_state_in_for {
            self.write(&format!("), {result}; {result} = yield "));
        } else {
            self.write(&format!("); {result} = yield "));
        }
        if self.in_async_generator_transform {
            self.write(self.helper_prefix());
            self.write("__await(");
        }
        self.write(&format!("{iterator}.next()"));
        if self.in_async_generator_transform {
            self.write(")");
        }
        self.writeln(&format!(
            ", {done} = {result}.done, !{done}; {needs_close} = true) {{"
        ));
        self.indent += 1;
        self.writeln(&format!("{value} = {result}.value;"));
        self.writeln(&format!("{needs_close} = false;"));
        self.emit_async_for_of_using_iteration(&binding_name, &value, is_async, &fo.body);
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!(
            "catch ({caught}) {{ {error} = {{ error: {caught} }}; }}"
        ));
        self.writeln("finally {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        self.write(&format!(
            "if (!{needs_close} && !{done} && ({return_fn} = {iterator}.return)) yield "
        ));
        if self.in_async_generator_transform {
            self.write(self.helper_prefix());
            self.write("__await(");
        }
        self.write(&format!("{return_fn}.call({iterator})"));
        if self.in_async_generator_transform {
            self.write(")");
        }
        self.writeln(";");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("finally {{ if ({error}) throw {error}.error; }}"));
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_async_for_of_using_iteration(
        &mut self,
        binding_name: &str,
        value_name: &str,
        is_async: bool,
        body: &Stmt,
    ) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        self.writeln(&format!(
            "const {binding_name} = {}__addDisposableResource(env_{env_num}, {value_name}, {is_async});",
            self.helper_prefix()
        ));
        match &body.kind {
            StmtKind::Block(stmts) => self.emit_async_stmt_list_with_using(stmts),
            _ => self.emit_stmt_await_to_yield(body),
        }
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if is_async {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.write("yield ");
            if self.in_async_generator_transform {
                self.write(self.helper_prefix());
                self.write("__await(");
            }
            self.write(&format!("result_{env_num}"));
            if self.in_async_generator_transform {
                self.write(")");
            }
            self.writeln(";");
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_simple_for_of_using(&mut self, fo: &ForOfStmt, preserve_await: bool, labels: &[&str]) {
        let Some((binding_name, is_async)) = self.simple_for_of_using_binding(fo) else {
            return;
        };
        let value_name = self.make_unique_loop_resource_value_name(&binding_name);
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write(if preserve_await {
            "for await (const "
        } else {
            "for (const "
        });
        self.write(&value_name);
        self.write(" of ");
        self.emit_expr_await_to_yield(&fo.right);
        self.writeln(") {");
        self.indent += 1;
        self.emit_async_for_of_using_iteration(&binding_name, &value_name, is_async, &fo.body);
        self.indent -= 1;
        self.writeln("}");
    }

    /// Lower a synchronous resource-bearing for-of inside an async function
    /// that has itself become a generator. The loop must not retain native
    /// for-of syntax at ES5, while the body still needs await-to-yield emit.
    fn emit_simple_for_of_using_es5(&mut self, fo: &ForOfStmt, labels: &[&str]) {
        let Some((binding_name, is_async)) = self.simple_for_of_using_binding(fo) else {
            return;
        };
        let index = self.make_unique_loop_resource_value_name(&format!("{binding_name}_index"));
        let values = self.make_unique_loop_resource_value_name(&format!("{binding_name}_values"));
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write(&format!("for (var {index} = 0, {values} = "));
        self.emit_expr_await_to_yield(&fo.right);
        self.write(&format!("; {index} < {values}.length; {index}++) {{"));
        self.newline();
        self.indent += 1;
        self.emit_async_for_of_using_iteration(
            &binding_name,
            &format!("{values}[{index}]"),
            is_async,
            &fo.body,
        );
        self.indent -= 1;
        self.writeln("}");
    }

    /// Emit a for-await-of downlevel pattern using `await` (not `yield`).
    /// Used for top-level for-await in ESM modules where the code isn't
    /// inside an async function being transformed to a generator.
    pub(crate) fn emit_for_await_downlevel_with_await(&mut self, fo: &ForOfStmt) {
        if self.fn_scope_depth == 0 {
            self.emit_top_level_for_await_downlevel_canonical(fo);
            return;
        }
        let Some((binding_kw, binding_name)) = self.can_emit_simple_for_await_downlevel(fo) else {
            return;
        };
        let mut n = self.native_for_await_counter + 1;
        loop {
            let names = [
                format!("_nativeForAwaitDone_{n}"),
                format!("_nativeForAwaitError_{n}"),
                format!("_nativeForAwaitReturn_{n}"),
                format!("_nativeForAwaitValue_{n}"),
                format!("_nativeForAwaitNeedsClose_{n}"),
                format!("_nativeForAwaitIterator_{n}"),
                format!("_nativeForAwaitResult_{n}"),
                format!("_nativeForAwaitCaught_{n}"),
            ];
            if names.iter().all(|name| !self.source_has_identifier(name)) {
                break;
            }
            n += 1;
        }
        self.native_for_await_counter = n;
        let done = format!("_nativeForAwaitDone_{n}");
        let error = format!("_nativeForAwaitError_{n}");
        let return_fn = format!("_nativeForAwaitReturn_{n}");
        let value = format!("_nativeForAwaitValue_{n}");
        let needs_close = format!("_nativeForAwaitNeedsClose_{n}");
        let iter_var = format!("_nativeForAwaitIterator_{n}");
        let result_var = format!("_nativeForAwaitResult_{n}");
        let caught = format!("_nativeForAwaitCaught_{n}");
        self.writeln(&format!("var {done}, {error}, {return_fn}, {value};"));
        self.writeln("try {");
        self.indent += 1;
        self.write(&format!("for (var {needs_close} = true, {iter_var} = "));
        self.write(self.helper_prefix());
        self.write("__asyncValues(");
        self.emit_expr(&fo.right);
        self.writeln(&format!(
            "), {result_var}; {result_var} = await {iter_var}.next(), {done} = {result_var}.done, !{done}; {needs_close} = true) {{"
        ));
        self.indent += 1;
        self.writeln(&format!("{value} = {result_var}.value;"));
        self.writeln(&format!("{needs_close} = false;"));
        self.write(binding_kw);
        self.write(" ");
        self.write(&binding_name);
        self.writeln(&format!(" = {value};"));
        match &fo.body.kind {
            StmtKind::Block(stmts) => {
                if let Some(first_using) = crate::emit_stmt::first_using_index(stmts) {
                    for stmt in &stmts[..first_using] {
                        self.emit_leading_comments(stmt.span.start);
                        self.emit_stmt(stmt);
                        self.advance_comment_pos(stmt.span.end);
                    }
                    self.emit_block_using_dispose_scope(&stmts[first_using..]);
                } else {
                    for stmt in stmts {
                        self.emit_leading_comments(stmt.span.start);
                        self.emit_stmt(stmt);
                        self.advance_comment_pos(stmt.span.end);
                    }
                }
            }
            _ => self.emit_stmt(&fo.body),
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!(
            "catch ({caught}) {{ {error} = {{ error: {caught} }}; }}"
        ));
        self.writeln("finally {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        self.writeln(&format!(
            "if (!{needs_close} && !{done} && ({return_fn} = {iter_var}.return)) await {return_fn}.call({iter_var});"
        ));
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("finally {{ if ({error}) throw {error}.error; }}"));
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_top_level_for_await_downlevel_canonical(&mut self, fo: &ForOfStmt) {
        let Some((binding_kw, binding_name)) = self.can_emit_simple_for_await_downlevel(fo) else {
            return;
        };
        let (iter_var, result_var) = if let ExprKind::Ident(name) = &fo.right.kind {
            (format!("{name}_1"), format!("{name}_1_1"))
        } else {
            ("_e".to_string(), "_f".to_string())
        };
        self.writeln("try {");
        self.indent += 1;
        self.write(&format!("for (var _d = true, {iter_var} = "));
        self.write(self.helper_prefix());
        self.write("__asyncValues(");
        self.emit_expr(&fo.right);
        self.writeln(&format!(
            "), {result_var}; {result_var} = await {iter_var}.next(), _a = {result_var}.done, !_a; _d = true) {{"
        ));
        self.indent += 1;
        self.writeln(&format!("_c = {result_var}.value;"));
        self.writeln("_d = false;");
        self.write(binding_kw);
        self.write(" ");
        self.write(&binding_name);
        self.writeln(" = _c;");
        match &fo.body.kind {
            StmtKind::Block(stmts) => {
                if let Some(first_using) = crate::emit_stmt::first_using_index(stmts) {
                    for stmt in &stmts[..first_using] {
                        self.emit_leading_comments(stmt.span.start);
                        self.emit_stmt(stmt);
                        self.advance_comment_pos(stmt.span.end);
                    }
                    self.emit_block_using_dispose_scope(&stmts[first_using..]);
                } else {
                    for stmt in stmts {
                        self.emit_leading_comments(stmt.span.start);
                        self.emit_stmt(stmt);
                        self.advance_comment_pos(stmt.span.end);
                    }
                }
            }
            _ => self.emit_stmt(&fo.body),
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("catch (e_1_1) { e_1 = { error: e_1_1 }; }");
        self.writeln("finally {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        self.writeln(&format!(
            "if (!_d && !_a && (_b = {iter_var}.return)) await _b.call({iter_var});"
        ));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally { if (e_1) throw e_1.error; }");
        self.indent -= 1;
        self.writeln("}");
    }

    pub(crate) fn emit_for_await_using_downlevel_with_await(
        &mut self,
        fo: &ForOfStmt,
        labels: &[&str],
    ) {
        let Some((binding_name, is_async)) = self.simple_for_of_using_binding(fo) else {
            return;
        };
        let ident_iterable = if let ExprKind::Ident(name) = &fo.right.kind {
            Some(name.to_string())
        } else {
            None
        };
        let suffix = self.next_resource_for_await_suffix(ident_iterable.as_deref());
        let (done, error, return_fn, value, needs_close, iterator, result, caught) =
            if let Some(n) = suffix {
                let names = (
                    format!("_forAwaitDone_{n}"),
                    format!("_forAwaitError_{n}"),
                    format!("_forAwaitReturn_{n}"),
                    format!("_forAwaitValue_{n}"),
                    format!("_forAwaitNeedsClose_{n}"),
                    format!("_forAwaitIterator_{n}"),
                    format!("_forAwaitResult_{n}"),
                    format!("_forAwaitCaught_{n}"),
                );
                self.writeln(&format!(
                    "var {}, {}, {}, {}, {}, {}, {};",
                    names.0, names.1, names.2, names.3, names.4, names.5, names.6
                ));
                names
            } else {
                (
                    "_a".into(),
                    "e_1".into(),
                    "_b".into(),
                    "_c".into(),
                    "_d".into(),
                    ident_iterable
                        .as_ref()
                        .map_or_else(|| "_e".into(), |name| format!("{name}_1")),
                    ident_iterable
                        .as_ref()
                        .map_or_else(|| "_f".into(), |name| format!("{name}_1_1")),
                    "e_1_1".into(),
                )
            };
        self.writeln("try {");
        self.indent += 1;
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write(&format!("for (var {needs_close} = true, {iterator} = "));
        self.write(self.helper_prefix());
        self.write("__asyncValues(");
        self.emit_expr(&fo.right);
        self.writeln(&format!(
            "), {result}; {result} = await {iterator}.next(), {done} = {result}.done, !{done}; {needs_close} = true) {{"
        ));
        self.indent += 1;
        self.writeln(&format!("{value} = {result}.value;"));
        self.writeln(&format!("{needs_close} = false;"));
        self.emit_native_for_of_using_iteration(&binding_name, &value, is_async, &fo.body);
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!(
            "catch ({caught}) {{ {error} = {{ error: {caught} }}; }}"
        ));
        self.writeln("finally {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        self.writeln(&format!(
            "if (!{needs_close} && !{done} && ({return_fn} = {iterator}.return)) await {return_fn}.call({iterator});"
        ));
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("finally {{ if ({error}) throw {error}.error; }}"));
        self.indent -= 1;
        self.writeln("}");
    }

    /// Emit the __awaiter helper function.
    pub(crate) fn emit_awaiter_helper(&mut self) {
        self.writeln("var __awaiter = (this && this.__awaiter) || function (thisArg, _arguments, P, generator) {");
        self.indent += 1;
        self.writeln("function adopt(value) { return value instanceof P ? value : new P(function (resolve) { resolve(value); }); }");
        self.writeln("return new (P || (P = Promise))(function (resolve, reject) {");
        self.indent += 1;
        self.writeln("function fulfilled(value) { try { step(generator.next(value)); } catch (e) { reject(e); } }");
        self.writeln("function rejected(value) { try { step(generator[\"throw\"](value)); } catch (e) { reject(e); } }");
        self.writeln("function step(result) { result.done ? resolve(result.value) : adopt(result.value).then(fulfilled, rejected); }");
        self.writeln("step((generator = generator.apply(thisArg, _arguments || [])).next());");
        self.indent -= 1;
        self.writeln("});");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __await helper function for downleveled async generators.
    pub(crate) fn emit_await_helper(&mut self) {
        self.writeln("var __await = (this && this.__await) || function (v) { return this instanceof __await ? (this.v = v, this) : new __await(v); }");
    }

    /// Emit the __asyncGenerator helper function for downleveled async generators.
    pub(crate) fn emit_async_generator_helper(&mut self) {
        self.writeln("var __asyncGenerator = (this && this.__asyncGenerator) || function (thisArg, _arguments, generator) {");
        self.indent += 1;
        self.writeln("if (!Symbol.asyncIterator) throw new TypeError(\"Symbol.asyncIterator is not defined.\");");
        self.writeln("var g = generator.apply(thisArg, _arguments || []), i, q = [];");
        self.writeln("return i = Object.create((typeof AsyncIterator === \"function\" ? AsyncIterator : Object).prototype), verb(\"next\"), verb(\"throw\"), verb(\"return\", awaitReturn), i[Symbol.asyncIterator] = function () { return this; }, i;");
        self.writeln("function awaitReturn(f) { return function (v) { return Promise.resolve(v).then(f, reject); }; }");
        self.writeln("function verb(n, f) { if (g[n]) { i[n] = function (v) { return new Promise(function (a, b) { q.push([n, v, a, b]) > 1 || resume(n, v); }); }; if (f) i[n] = f(i[n]); } }");
        self.writeln(
            "function resume(n, v) { try { step(g[n](v)); } catch (e) { settle(q[0][3], e); } }",
        );
        self.writeln("function step(r) { r.value instanceof __await ? Promise.resolve(r.value.v).then(fulfill, reject) : settle(q[0][2], r); }");
        self.writeln("function fulfill(value) { resume(\"next\", value); }");
        self.writeln("function reject(value) { resume(\"throw\", value); }");
        self.writeln(
            "function settle(f, v) { if (f(v), q.shift(), q.length) resume(q[0][0], q[0][1]); }",
        );
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __asyncValues helper function.
    pub(crate) fn emit_async_values_helper(&mut self) {
        self.writeln("var __asyncValues = (this && this.__asyncValues) || function (o) {");
        self.indent += 1;
        self.writeln("if (!Symbol.asyncIterator) throw new TypeError(\"Symbol.asyncIterator is not defined.\");");
        self.writeln("var m = o[Symbol.asyncIterator], i;");
        self.writeln("return m ? m.call(o) : (o = typeof __values === \"function\" ? __values(o) : o[Symbol.iterator](), i = {}, verb(\"next\"), verb(\"throw\"), verb(\"return\"), i[Symbol.asyncIterator] = function () { return this; }, i);");
        self.writeln("function verb(n) { i[n] = o[n] && function (v) { return new Promise(function (resolve, reject) { v = o[n](v), settle(resolve, reject, v.done, v.value); }); }; }");
        self.writeln("function settle(resolve, reject, d, v) { Promise.resolve(v).then(function(v) { resolve({ value: v, done: d }); }, reject); }");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __asyncDelegator helper function.
    pub(crate) fn emit_async_delegator_helper(&mut self) {
        self.writeln("var __asyncDelegator = (this && this.__asyncDelegator) || function (o) {");
        self.indent += 1;
        self.writeln("var i, p;");
        self.writeln("return i = {}, verb(\"next\"), verb(\"throw\", function (e) { throw e; }), verb(\"return\"), i[Symbol.iterator] = function () { return this; }, i;");
        self.writeln("function verb(n, f) { i[n] = o[n] ? function (v) { return (p = !p) ? { value: __await(o[n](v)), done: false } : f ? f(v) : v; } : f; }");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __makeTemplateObject helper function.
    /// Used to lower tagged templates with invalid escape sequences (target < ES2018).
    pub(crate) fn emit_make_template_object_helper(&mut self) {
        self.writeln("var __makeTemplateObject = (this && this.__makeTemplateObject) || function (cooked, raw) {");
        self.indent += 1;
        self.writeln("if (Object.defineProperty) { Object.defineProperty(cooked, \"raw\", { value: raw }); } else { cooked.raw = raw; }");
        self.writeln("return cooked;");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __rest helper function.
    /// Matches TypeScript 5.x exact output with `propertyIsEnumerable`.
    pub(crate) fn emit_rest_helper(&mut self) {
        self.writeln("var __rest = (this && this.__rest) || function (s, e) {");
        self.indent += 1;
        self.writeln("var t = {};");
        self.writeln(
            "for (var p in s) if (Object.prototype.hasOwnProperty.call(s, p) && e.indexOf(p) < 0)",
        );
        self.indent += 1;
        self.writeln("t[p] = s[p];");
        self.indent -= 1;
        self.writeln("if (s != null && typeof Object.getOwnPropertySymbols === \"function\")");
        self.indent += 1;
        self.writeln("for (var i = 0, p = Object.getOwnPropertySymbols(s); i < p.length; i++) {");
        self.indent += 1;
        self.writeln(
            "if (e.indexOf(p[i]) < 0 && Object.prototype.propertyIsEnumerable.call(s, p[i]))",
        );
        self.indent += 1;
        self.writeln("t[p[i]] = s[p[i]];");
        self.indent -= 1;
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("return t;");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit a function body wrapped in __asyncGenerator for downlevel async generators.
    pub(crate) fn emit_async_generator_body(
        &mut self,
        body: &[Stmt],
        generator_name: Option<&str>,
        inner_params: Option<&[Param]>,
    ) {
        // Check if the source async generator function was single-line.
        let has_for_await = body.iter().any(source_has_for_await);
        let source_single_line = !has_for_await
            && !crate::emit_stmt::has_using_declaration(body)
            && self.awaiter_enclosing_span.as_ref().map_or(false, |span| {
                let start = span.start as usize;
                let end = span.end as usize;
                if start < end && end <= self.source.len() {
                    let text = &self.source[start..end];
                    text.rfind('{')
                        .map_or(false, |bp| !text[bp..].contains('\n'))
                } else {
                    false
                }
            });
        self.fn_scope_depth += 1;
        // Save and scan for super hoisting.
        let saved_super_active = self.async_super_active;
        let saved_super_names = std::mem::take(&mut self.async_super_names);
        let saved_super_elem = self.async_super_has_element_access;
        let saved_super_write = self.async_super_has_write;
        let saved_super_suffix = std::mem::take(&mut self.async_super_suffix);
        let needs_super_hoist = if self.current_class_name.is_some() {
            let (names, has_elem, has_write) = Self::scan_super_accesses(body);
            if !names.is_empty() || has_elem {
                self.async_super_active = true;
                self.async_super_names = names;
                self.async_super_has_element_access = has_elem;
                self.async_super_has_write = has_write;
                self.async_super_suffix = Self::compute_super_suffix(body);
                true
            } else {
                false
            }
        } else {
            false
        };
        let preserve_const_for_count = self.preserve_const_enums_effective();
        let non_erased_body_count = body
            .iter()
            .filter(|s| !stmt_is_erased(s, preserve_const_for_count))
            .count();
        if source_single_line && (!body.is_empty() || inner_params.is_some()) && !needs_super_hoist
        {
            // Emit everything on a single line:
            // { return __asyncGenerator(this, arguments, function*() { stmts }); }
            self.write("{ return ");
            self.write(self.helper_prefix());
            self.write("__asyncGenerator(this, arguments, function*");
            if let Some(name) = generator_name {
                self.write(" ");
                self.write(name);
            } else if inner_params.is_none() {
                self.write(" ");
            }
            let prev_async_gen = self.in_async_generator_transform;
            self.in_async_generator_transform = true;
            self.awaiter_enclosing_span = None;
            if let Some(params) = inner_params {
                self.write("(");
                let rest_infos = self.emit_async_generator_inner_params(params);
                let total = rest_infos.len() + non_erased_body_count;
                self.write(")");
                if total == 0 {
                    self.write(" { }");
                } else {
                    self.write(" { ");
                    self.emit_async_generator_single_line_body(body, &rest_infos);
                    self.write(" }");
                }
            } else {
                let total = non_erased_body_count;
                self.write("()");
                if total == 0 {
                    self.write(" { }");
                } else {
                    self.write(" { ");
                    let preserve_const = self.preserve_const_enums_effective();
                    for s in body {
                        if stmt_is_erased(s, preserve_const) {
                            continue;
                        }
                        self.emit_stmt_await_to_yield(s);
                    }
                    if self.output.ends_with('\n') {
                        self.output.pop();
                        self.at_line_start = false;
                    }
                    self.write(" }");
                }
            }
            self.in_async_generator_transform = prev_async_gen;
            self.write("); }");
        } else if source_single_line && needs_super_hoist {
            self.write("{ ");
            self.emit_super_hoisting(true);
            if self.output.ends_with('\n') {
                self.output.pop();
                self.at_line_start = false;
            }
            self.write(" return ");
            self.write(self.helper_prefix());
            self.write("__asyncGenerator(this, arguments, function*");
            if let Some(name) = generator_name {
                self.write(" ");
                self.write(name);
            } else if inner_params.is_none() {
                self.write(" ");
            }
            let prev_async_gen = self.in_async_generator_transform;
            self.in_async_generator_transform = true;
            if let Some(params) = inner_params {
                self.write("(");
                let rest_infos = self.emit_async_generator_inner_params(params);
                let total = rest_infos.len() + non_erased_body_count;
                self.write(")");
                if total == 0 {
                    self.write(" { }");
                } else {
                    self.write(" { ");
                    self.emit_async_generator_single_line_body(body, &rest_infos);
                    self.write(" }");
                }
            } else {
                let total = non_erased_body_count;
                self.write("()");
                if total == 0 {
                    self.write(" { }");
                } else {
                    self.write(" { ");
                    let preserve_const = self.preserve_const_enums_effective();
                    for s in body {
                        if stmt_is_erased(s, preserve_const) {
                            continue;
                        }
                        self.emit_stmt_await_to_yield(s);
                    }
                    if self.output.ends_with('\n') {
                        self.output.pop();
                        self.at_line_start = false;
                    }
                    self.write(" }");
                }
            }
            self.in_async_generator_transform = prev_async_gen;
            self.write("); }");
        } else {
            self.writeln("{");
            self.indent += 1;
            if needs_super_hoist {
                self.emit_super_hoisting(true);
            }
            self.write("return ");
            self.write(self.helper_prefix());
            self.write("__asyncGenerator(this, arguments, function*");
            if let Some(name) = generator_name {
                self.write(" ");
                self.write(name);
            } else if inner_params.is_none() {
                self.write(" ");
            }
            let prev_async_gen = self.in_async_generator_transform;
            self.in_async_generator_transform = true;
            if let Some(params) = inner_params {
                self.write("(");
                let rest_infos = self.emit_async_generator_inner_params(params);
                if source_single_line {
                    self.write(") { ");
                    self.emit_async_generator_single_line_body(body, &rest_infos);
                    self.writeln(" });");
                } else {
                    self.write(") ");
                    self.writeln("{");
                    self.indent += 1;
                    for (_, temp_name, pat, _) in &rest_infos {
                        self.emit_rest_param_destructuring_await_to_yield(temp_name, pat);
                    }
                    self.emit_rest_lifted_defaults();
                    let preserve_const = self.preserve_const_enums_effective();
                    for s in body {
                        if stmt_is_erased(s, preserve_const) {
                            continue;
                        }
                        self.emit_stmt_await_to_yield(s);
                    }
                    self.indent -= 1;
                    self.write("}");
                    self.writeln(");");
                }
            } else {
                self.write("() ");
                self.emit_block_with_await_to_yield(body);
                self.writeln(");");
            }
            self.in_async_generator_transform = prev_async_gen;
            self.indent -= 1;
            self.write("}");
        }
        // Restore super hoisting state.
        self.async_super_active = saved_super_active;
        self.async_super_names = saved_super_names;
        self.async_super_has_element_access = saved_super_elem;
        self.async_super_has_write = saved_super_write;
        self.async_super_suffix = saved_super_suffix;
        self.fn_scope_depth -= 1;
    }

    /// Emit an `__awaiter` body. When `moved_params` is `Some`, the parameters
    /// had default values and were moved from the outer function to the generator
    /// function; `arguments` is passed instead of `void 0`.
    pub(crate) fn emit_awaiter_body_with_params(
        &mut self,
        body: &[Stmt],
        moved_params: Option<&[Param]>,
        outer_params: &[Param],
        arguments_alias: Option<&str>,
    ) {
        let es5_plan = (moved_params.is_none()
            && (self.needs_generator_helper || self.options.no_emit_helpers == Some(true)))
        .then(|| self.plan_es5_async_body(body))
        .flatten();
        self.fn_scope_depth += 1;
        let saved_async_var_shadow_names = self.async_var_shadow_names.take();
        let saved_async_var_shadow_blockers = std::mem::take(&mut self.async_var_shadow_blockers);
        let saved_async_var_shadow_top_level_hoists =
            std::mem::take(&mut self.async_var_shadow_top_level_hoists);
        if moved_params.is_none() {
            let param_names = Self::collect_async_shadow_param_names(outer_params);
            if !param_names.is_empty() {
                let hoists = Self::collect_async_var_shadow_hoists(body, &param_names);
                if !hoists.is_empty() {
                    self.async_var_shadow_names = Some(param_names);
                    self.async_var_shadow_top_level_hoists = hoists;
                }
            }
        }
        self.writeln("{");
        self.indent += 1;
        if let Some(alias) = arguments_alias {
            self.write("var ");
            self.write(alias);
            self.writeln(" = arguments;");
        }
        // Check for super property accesses that need hoisting before __awaiter.
        // This only applies inside class methods (current_class_name is set).
        let saved_super_active = self.async_super_active;
        let saved_super_names = std::mem::take(&mut self.async_super_names);
        let saved_super_elem = self.async_super_has_element_access;
        let saved_super_write = self.async_super_has_write;
        let saved_super_suffix = std::mem::take(&mut self.async_super_suffix);
        if self.current_class_name.is_some() {
            let (names, has_elem, has_write) = Self::scan_super_accesses(body);
            if !names.is_empty() || has_elem {
                self.async_super_active = true;
                self.async_super_names = names;
                self.async_super_has_element_access = has_elem;
                self.async_super_has_write = has_write;
                // Check for naming conflicts with _super/_superIndex in the body
                self.async_super_suffix = Self::compute_super_suffix(body);
                self.emit_super_hoisting(false);
            }
        }
        self.write("return ");
        self.write(self.helper_prefix());
        if moved_params.is_some() {
            self.write("__awaiter(this, arguments, void 0, function* (");
            if let Some(params) = moved_params {
                self.emit_params(params);
            }
            self.write(") ");
        } else {
            self.write(if es5_plan.is_some() {
                "__awaiter(this, void 0, void 0, function () "
            } else {
                "__awaiter(this, void 0, void 0, function* () "
            });
        }
        // TypeScript collapses single-statement bodies to single-line when the
        // source async function body was on a single line.
        let has_for_await = body.iter().any(source_has_for_await);
        let source_single_line = !has_for_await
            && !crate::emit_stmt::has_using_declaration(body)
            && self.awaiter_enclosing_span.as_ref().map_or(false, |span| {
                let start = span.start as usize;
                let end = span.end as usize;
                if start < end && end <= self.source.len() {
                    let text = &self.source[start..end];
                    text.rfind('{')
                        .map_or(false, |bp| !text[bp..].contains('\n'))
                } else {
                    false
                }
            });
        let prev_arguments_alias = self.current_arguments_alias.clone();
        if let Some(alias) = arguments_alias {
            self.current_arguments_alias = Some(alias.to_string());
        }
        if let Some(plan) = &es5_plan {
            self.awaiter_enclosing_span = None;
            self.emit_es5_async_generator(plan, outer_params);
        } else if source_single_line && !body.is_empty() {
            self.awaiter_enclosing_span = None;
            self.write("{ ");
            let preserve_const = self.preserve_const_enums_effective();
            let has_body_output = body.iter().any(|s| !stmt_is_erased(s, preserve_const));
            if self.emit_pending_async_var_shadow_hoists_inline() && has_body_output {
                self.write(" ");
            }
            for s in body {
                if stmt_is_erased(s, preserve_const) {
                    continue;
                }
                self.emit_stmt_await_to_yield(s);
            }
            if self.output.ends_with('\n') {
                self.output.pop();
                self.at_line_start = false;
            }
            self.write(" }");
        } else {
            self.emit_block_with_await_to_yield(body);
        }
        self.current_arguments_alias = prev_arguments_alias;
        // Restore super hoisting state.
        self.async_super_active = saved_super_active;
        self.async_super_names = saved_super_names;
        self.async_super_has_element_access = saved_super_elem;
        self.async_super_has_write = saved_super_write;
        self.async_super_suffix = saved_super_suffix;
        self.async_var_shadow_names = saved_async_var_shadow_names;
        self.async_var_shadow_blockers = saved_async_var_shadow_blockers;
        self.async_var_shadow_top_level_hoists = saved_async_var_shadow_top_level_hoists;
        self.writeln(");");
        self.indent -= 1;
        self.write("}");
        self.fn_scope_depth -= 1;
    }

    /// Emit a block converting `await` expressions to `yield` expressions.
    ///
    /// TypeScript emits bodies inline when the original source block was on a
    /// single line (no newlines between `{` and `}`).  We approximate this by
    /// checking the source span of the statements.
    pub(crate) fn emit_block_with_await_to_yield(&mut self, stmts: &[Stmt]) {
        if stmts.is_empty() {
            // Check if the enclosing function body was multi-line in source.
            if let Some(span) = self.awaiter_enclosing_span.take() {
                let start = span.start as usize;
                let end = span.end as usize;
                if start < end && end <= self.source.len() {
                    let fn_text = &self.source[start..end];
                    if let Some(brace_pos) = fn_text.rfind('{') {
                        if fn_text[brace_pos..].contains('\n') {
                            self.writeln("{");
                            self.write("}");
                            return;
                        }
                    }
                }
            }
            self.write("{ }");
            return;
        }
        // Clear the span since we're handling a non-empty block.
        self.awaiter_enclosing_span = None;
        // TypeScript always emits multi-line blocks in the async transform,
        // even when the source was single-line (e.g. `{ return; }`).
        self.writeln("{");
        self.indent += 1;
        if stmts.iter().any(source_has_for_await) {
            // When iterable is a simple identifier, TypeScript hoists all vars
            // including name-derived iterator temps. Otherwise, the for-init
            // declares its own vars with `var`.
            if let Some(iter_name) = Self::find_for_await_ident_name(stmts) {
                self.writeln(&format!(
                    "var _a, e_1, _b, _c, _d, {iter_name}_1, {iter_name}_1_1;"
                ));
            } else {
                self.writeln("var _a, e_1, _b, _c;");
            }
        }
        // Record position for inserting temp var declarations after emission.
        let saved_awaiter_temps = self.awaiter_body_temp_vars.len();
        let temp_insert_pos = self.output.len();
        self.emit_pending_async_var_shadow_hoists_multiline();
        self.emit_async_stmt_list_with_using(stmts);
        // Insert any temp vars allocated during body emission.
        if self.awaiter_body_temp_vars.len() > saved_awaiter_temps {
            let temps: Vec<String> = self.awaiter_body_temp_vars[saved_awaiter_temps..].to_vec();
            let indent = "    ".repeat(self.indent);
            let decl = format!("{}var {};\n", indent, temps.join(", "));
            self.output.insert_str(temp_insert_pos, &decl);
        }
        self.indent -= 1;
        self.write("}");
    }

    /// Emit a statement list in an await-to-yield context, preserving the
    /// lexical resource scope when the list directly contains `using`.
    fn emit_async_stmt_list_with_using(&mut self, stmts: &[Stmt]) {
        let preserve_const = self.preserve_const_enums_effective();
        let using_scope_idx = self
            .needs_downlevel("using")
            .then(|| crate::emit_stmt::first_using_index(stmts))
            .flatten();
        let ordinary_end = using_scope_idx.unwrap_or(stmts.len());
        for stmt in &stmts[..ordinary_end] {
            if stmt_is_erased(stmt, preserve_const) {
                self.advance_comment_pos(stmt.span.end);
                continue;
            }
            self.emit_leading_comments(stmt.span.start);
            self.emit_stmt_await_to_yield(stmt);
            self.advance_comment_pos(stmt.span.end);
        }
        if let Some(first_using) = using_scope_idx {
            self.emit_async_using_dispose_scope(&stmts[first_using..]);
        }
    }

    /// Emit a resource-management scope inside a generator used by `__awaiter`
    /// or `__asyncGenerator`. This mirrors the normal using transform, but all
    /// expressions and the asynchronous disposal result must pass through the
    /// await-to-yield protocol of the enclosing helper.
    fn emit_async_using_dispose_scope(&mut self, stmts: &[Stmt]) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        let has_await_using = stmts.iter().any(|stmt| {
            matches!(&stmt.kind, StmtKind::Var(var_stmt) if var_stmt.kind == VarKind::AwaitUsing)
        });

        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Var(var_stmt)
                    if matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing) =>
                {
                    let is_async = var_stmt.kind == VarKind::AwaitUsing;
                    for declaration in &var_stmt.declarations {
                        let PatKind::Ident(name) = &declaration.name.kind else {
                            // Invalid binding patterns are retained by the recovery
                            // emitter instead of silently dropping source text.
                            self.emit_stmt_await_to_yield(stmt);
                            break;
                        };
                        let Some(initializer) = declaration.init.as_ref() else {
                            continue;
                        };
                        self.write("const ");
                        self.write(name);
                        self.write(" = ");
                        self.write(self.helper_prefix());
                        self.write(&format!("__addDisposableResource(env_{env_num}, "));
                        self.indent += 1;
                        self.emit_async_using_initializer(name, initializer, 1);
                        self.indent -= 1;
                        self.write(if is_async { ", true" } else { ", false" });
                        self.writeln(");");
                    }
                }
                _ => self.emit_stmt_await_to_yield(stmt),
            }
        }
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if has_await_using {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.write("yield ");
            if self.in_async_generator_transform {
                self.write(self.helper_prefix());
                self.write("__await(");
            }
            self.write(&format!("result_{env_num}"));
            if self.in_async_generator_transform {
                self.write(")");
            }
            self.writeln(";");
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_async_using_initializer(
        &mut self,
        name: &str,
        initializer: &Expr,
        inline_container_indents_preowned: usize,
    ) {
        let ExprKind::ClassExpr(class_decl) = &initializer.kind else {
            let previous = self.await_expr_inline_container_indents_preowned;
            self.await_expr_inline_container_indents_preowned = inline_container_indents_preowned;
            self.emit_expr_await_to_yield(initializer);
            self.await_expr_inline_container_indents_preowned = previous;
            return;
        };
        if class_decl.name.is_some() {
            self.emit_expr_await_to_yield(initializer);
            return;
        }

        let binding_name = ClassExprBindingName::Literal(name.to_string());
        if Self::expr_needs_class_expr_binding_name(initializer) {
            let previous = self.class_expr_binding_name.replace(binding_name);
            let previous_using = self.in_using_class_initializer;
            self.in_using_class_initializer = true;
            self.emit_expr_await_to_yield(initializer);
            self.in_using_class_initializer = previous_using;
            self.class_expr_binding_name = previous;
            return;
        }

        self.needs_set_function_name_helper = true;
        let temp = self.next_temp_var();
        self.write("(");
        self.write(&temp);
        self.write(" = ");
        self.indent += 1;
        self.emit_expr_await_to_yield(initializer);
        self.writeln(",");
        self.write(self.helper_prefix());
        self.write("__setFunctionName(");
        self.write(&temp);
        self.write(", \"");
        self.write(name);
        self.writeln("\"),");
        self.write(&temp);
        self.indent -= 1;
        self.write(")");
    }

    /// Emit a statement, converting `await` to `yield` within expressions.
    pub(crate) fn emit_stmt_await_to_yield(&mut self, stmt: &Stmt) {
        self.record_mapping(stmt.span);
        match &stmt.kind {
            StmtKind::Var(var_stmt) => {
                if var_stmt.modifiers & MOD_DECLARE != 0 {
                    return;
                }
                if self.async_var_shadow_var_stmt_needs_transform(var_stmt) {
                    self.emit_async_var_shadow_var_stmt_rewrite(var_stmt);
                    self.append_trailing_comment(stmt.span);
                    return;
                }
                let kw = self.emitted_var_keyword(var_stmt);
                self.write(kw);
                self.write(" ");
                for (i, decl) in var_stmt.declarations.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_binding_name(&decl.name);
                    if let Some(ref init) = decl.init {
                        self.write(" = ");
                        self.emit_expr_await_to_yield(init);
                    }
                }
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Expr(expr) => {
                // `++await x` / `--await x` → split into `++;\nyield x;`
                // because `++yield x` is invalid (yield is not an assignment target).
                if let ExprKind::Update(up) = &expr.kind {
                    if matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec)
                        && matches!(up.argument.kind, ExprKind::Await(_))
                    {
                        let op = match up.op {
                            UpdateOp::PreInc => "++",
                            _ => "--",
                        };
                        self.write(op);
                        self.writeln(";");
                        self.emit_expr_await_to_yield(&up.argument);
                        self.writeln(";");
                        self.append_trailing_comment(stmt.span);
                        return;
                    }
                }
                self.emit_expr_await_to_yield(expr);
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Return(arg) => {
                self.write("return");
                if let Some(ref expr) = arg {
                    if self.in_async_generator_transform {
                        // In async generators, `return <value>` becomes
                        // `return yield __await(<value>)`.
                        self.write(" yield ");
                        self.write(self.helper_prefix());
                        self.write("__await(");
                        self.emit_expr_await_to_yield(expr);
                        self.write(")");
                    } else {
                        self.write(" ");
                        self.emit_expr_await_to_yield(expr);
                    }
                }
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::If(if_stmt) => {
                self.write("if (");
                self.emit_expr_await_to_yield(&if_stmt.test);
                self.write(")");
                if matches!(if_stmt.consequent.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
                self.emit_stmt_body_await_to_yield(&if_stmt.consequent);
                if let Some(ref alt) = if_stmt.alternate {
                    if matches!(alt.kind, StmtKind::If(_)) {
                        self.write("else ");
                        self.emit_stmt_await_to_yield(alt);
                    } else {
                        self.write("else");
                        if matches!(alt.kind, StmtKind::Block(_)) {
                            self.write(" ");
                        }
                        self.emit_stmt_body_await_to_yield(alt);
                    }
                }
            }
            StmtKind::While(wh) => {
                self.write("while (");
                self.emit_expr_await_to_yield(&wh.test);
                self.write(")");
                if matches!(wh.body.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
                self.emit_stmt_body_await_to_yield(&wh.body);
            }
            StmtKind::For(for_stmt) => {
                if let Some(ForInit::Var(var_stmt)) = &for_stmt.init {
                    if self.needs_downlevel("using")
                        && matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing)
                    {
                        self.emit_async_for_using_dispose_scope(for_stmt, var_stmt, &[]);
                        return;
                    }
                }
                self.write("for (");
                if let Some(ref init) = for_stmt.init {
                    match init {
                        ForInit::Var(vs) if self.async_var_shadow_var_stmt_needs_transform(vs) => {
                            self.emit_async_var_shadow_for_init_rewrite(vs);
                        }
                        ForInit::Var(vs) => {
                            let kw = self.emitted_var_keyword(vs);
                            self.write(kw);
                            self.write(" ");
                            for (i, decl) in vs.declarations.iter().enumerate() {
                                if i > 0 {
                                    self.write(", ");
                                }
                                self.emit_binding_name(&decl.name);
                                if let Some(ref vinit) = decl.init {
                                    self.write(" = ");
                                    self.emit_expr_await_to_yield(vinit);
                                }
                            }
                        }
                        ForInit::Expr(expr) => self.emit_expr_await_to_yield(expr),
                    }
                }
                self.write(";");
                if let Some(ref test) = for_stmt.test {
                    self.write(" ");
                    self.emit_expr_await_to_yield(test);
                }
                self.write(";");
                if let Some(ref update) = for_stmt.update {
                    self.write(" ");
                    self.emit_expr_await_to_yield(update);
                }
                self.write(")");
                if matches!(for_stmt.body.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
                self.emit_stmt_body_await_to_yield(&for_stmt.body);
            }
            StmtKind::DoWhile(dw) => {
                self.write("do");
                if let StmtKind::Block(stmts) = &dw.body.kind {
                    self.write(" ");
                    self.awaiter_enclosing_span = Some(dw.body.span);
                    self.emit_block_with_await_to_yield(stmts);
                    // Put `while` on same line as closing `}`
                    self.write(" ");
                } else {
                    self.newline();
                    self.indent += 1;
                    self.emit_stmt_await_to_yield(&dw.body);
                    self.indent -= 1;
                }
                self.write("while (");
                self.emit_expr_await_to_yield(&dw.test);
                self.writeln(");");
            }
            StmtKind::ForIn(fi) => {
                self.write("for (");
                match &fi.left {
                    ForInOfLeft::Var(vs) if self.async_var_shadow_var_stmt_needs_transform(vs) => {
                        self.emit_async_var_shadow_for_in_of_left_rewrite(vs);
                    }
                    _ => self.emit_for_in_of_left_await_to_yield(&fi.left),
                }
                self.write(" in ");
                self.emit_expr_await_to_yield(&fi.right);
                self.write(")");
                if matches!(fi.body.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
                self.emit_stmt_body_await_to_yield(&fi.body);
            }
            StmtKind::ForOf(fo) => {
                let using_binding = self.simple_for_of_using_binding(fo);
                if fo.is_await && using_binding.is_some() {
                    self.emit_simple_for_await_using_downlevel(fo, &[]);
                } else if using_binding.is_some() {
                    if self.needs_downlevel("for-of") {
                        self.emit_simple_for_of_using_es5(fo, &[]);
                    } else {
                        self.emit_simple_for_of_using(fo, fo.is_await, &[]);
                    }
                } else if fo.is_await
                    && !self.in_async_generator_transform
                    && self.can_emit_simple_for_await_downlevel(fo).is_some()
                {
                    self.emit_simple_for_await_downlevel(fo);
                } else if fo.is_await {
                    self.write("for await (");
                } else {
                    self.write("for (");
                }
                if using_binding.is_none()
                    && !(fo.is_await
                        && !self.in_async_generator_transform
                        && self.can_emit_simple_for_await_downlevel(fo).is_some())
                {
                    match &fo.left {
                        ForInOfLeft::Var(vs)
                            if self.async_var_shadow_var_stmt_needs_transform(vs) =>
                        {
                            self.emit_async_var_shadow_for_in_of_left_rewrite(vs);
                        }
                        _ => self.emit_for_in_of_left_await_to_yield(&fo.left),
                    }
                    self.write(" of ");
                    self.emit_expr_await_to_yield(&fo.right);
                    self.write(")");
                    if matches!(fo.body.kind, StmtKind::Block(_)) {
                        self.write(" ");
                    }
                    self.emit_stmt_body_await_to_yield(&fo.body);
                }
            }
            StmtKind::Switch(sw) => {
                if self.needs_downlevel("using")
                    && sw
                        .cases
                        .iter()
                        .any(|case| crate::emit_stmt::has_using_declaration(&case.consequent))
                {
                    self.emit_async_switch_using_dispose_scope(sw);
                    return;
                }
                self.write("switch (");
                self.emit_expr_await_to_yield(&sw.discriminant);
                self.writeln(") {");
                self.indent += 1;
                for case in &sw.cases {
                    if let Some(ref test) = case.test {
                        self.write("case ");
                        self.emit_expr_await_to_yield(test);
                        self.write(":");
                    } else {
                        self.write("default:");
                    }
                    // Check if single-statement case body was on same line in source
                    let is_single_simple = case.consequent.len() == 1
                        && matches!(
                            case.consequent[0].kind,
                            StmtKind::Break(_)
                                | StmtKind::Continue(_)
                                | StmtKind::Return(_)
                                | StmtKind::Expr(_)
                                | StmtKind::Throw(_)
                        )
                        && {
                            let label_end = case
                                .test
                                .as_ref()
                                .map(|t| t.span.end as usize)
                                .unwrap_or(case.span.start as usize);
                            let body_start = case.consequent[0].span.start as usize;
                            label_end <= self.source.len()
                                && body_start <= self.source.len()
                                && !self.source[label_end..body_start].contains('\n')
                        };
                    if is_single_simple {
                        self.write(" ");
                        self.emit_async_stmt_list_with_using(&case.consequent);
                    } else {
                        self.newline();
                        self.indent += 1;
                        self.emit_async_stmt_list_with_using(&case.consequent);
                        self.indent -= 1;
                    }
                }
                self.indent -= 1;
                self.writeln("}");
            }
            StmtKind::Labeled(labeled) => {
                let mut labels = vec![labeled.label.as_str()];
                let mut labeled_body = labeled.body.as_ref();
                while let StmtKind::Labeled(inner) = &labeled_body.kind {
                    labels.push(inner.label.as_str());
                    labeled_body = inner.body.as_ref();
                }
                if let StmtKind::For(for_stmt) = &labeled_body.kind {
                    if let Some(ForInit::Var(var_stmt)) = &for_stmt.init {
                        if self.needs_downlevel("using")
                            && matches!(var_stmt.kind, VarKind::Using | VarKind::AwaitUsing)
                        {
                            self.emit_async_for_using_dispose_scope(for_stmt, var_stmt, &labels);
                            return;
                        }
                    }
                }
                if let StmtKind::ForOf(for_of) = &labeled_body.kind {
                    if self.needs_downlevel("using")
                        && self.simple_for_of_using_binding(for_of).is_some()
                    {
                        if for_of.is_await {
                            self.emit_simple_for_await_using_downlevel(for_of, &labels);
                        } else if self.needs_downlevel("for-of") {
                            self.emit_simple_for_of_using_es5(for_of, &labels);
                        } else {
                            self.emit_simple_for_of_using(for_of, for_of.is_await, &labels);
                        }
                        return;
                    }
                }
                self.write(&labeled.label);
                self.write(": ");
                self.emit_stmt_await_to_yield(&labeled.body);
            }
            StmtKind::Block(stmts) => {
                self.awaiter_enclosing_span = Some(stmt.span);
                self.emit_block_with_await_to_yield(stmts);
                self.newline();
            }
            StmtKind::Throw(expr) => {
                self.write("throw ");
                self.emit_expr_await_to_yield(expr);
                self.writeln(";");
                self.append_trailing_comment(stmt.span);
            }
            StmtKind::Try(try_stmt) => {
                self.writeln("try {");
                self.indent += 1;
                self.emit_async_stmt_list_with_using(&try_stmt.block);
                self.indent -= 1;
                self.writeln("}");
                if let Some(ref handler) = try_stmt.handler {
                    let catch_blockers = handler
                        .param
                        .as_ref()
                        .map(Self::binding_name_set)
                        .filter(|names| !names.is_empty());
                    if handler.body.is_empty() {
                        // Check if the catch block was multi-line in source
                        let catch_start = handler.span.start as usize;
                        let catch_end = handler.span.end as usize;
                        let is_multiline =
                            if catch_start < catch_end && catch_end <= self.source.len() {
                                let src = &self.source[catch_start..catch_end];
                                // The block's `{` is the last `{` in the handler span
                                if let Some(block_open) = src.rfind('{') {
                                    src[block_open..].contains('\n')
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                        self.write("catch");
                        if let Some(ref param) = handler.param {
                            self.write(" (");
                            self.emit_binding_name(param);
                            self.write(")");
                        } else if self.effective_target() < ScriptTarget::ES2019 {
                            let ch = (b'a' + self.catch_auto_param_counter) as char;
                            self.catch_auto_param_counter += 1;
                            self.write(&format!(" (_{ch})"));
                        }
                        if is_multiline {
                            self.writeln(" {");
                            self.writeln("}");
                        } else {
                            self.writeln(" { }");
                        }
                    } else {
                        if let Some(ref param) = handler.param {
                            self.write("catch (");
                            self.emit_binding_name(param);
                            self.writeln(") {");
                        } else if self.effective_target() < ScriptTarget::ES2019 {
                            let ch = (b'a' + self.catch_auto_param_counter) as char;
                            self.catch_auto_param_counter += 1;
                            self.write(&format!("catch (_{ch}) "));
                            self.writeln("{");
                        } else {
                            self.writeln("catch {");
                        }
                        self.indent += 1;
                        if let Some(ref blockers) = catch_blockers {
                            self.async_var_shadow_blockers.push(blockers.clone());
                        }
                        self.emit_async_stmt_list_with_using(&handler.body);
                        if catch_blockers.is_some() {
                            self.async_var_shadow_blockers.pop();
                        }
                        self.indent -= 1;
                        self.writeln("}");
                    }
                }
                if let Some(ref fin) = try_stmt.finalizer {
                    self.writeln("finally {");
                    self.indent += 1;
                    self.emit_async_stmt_list_with_using(fin);
                    self.indent -= 1;
                    self.writeln("}");
                }
            }
            StmtKind::With(w) => {
                self.write("with (");
                self.emit_expr_await_to_yield(&w.object);
                self.write(")");
                if matches!(w.body.kind, StmtKind::Block(_)) {
                    self.write(" ");
                }
                self.emit_stmt_body_await_to_yield(&w.body);
            }
            _ => self.emit_stmt(stmt),
        }
    }

    /// Like `emit_for_in_of_left` but replaces `await` with `yield` in
    /// source-copied error-recovery patterns.
    fn emit_for_in_of_left_await_to_yield(&mut self, left: &ForInOfLeft) {
        match left {
            ForInOfLeft::Expr(expr) => self.emit_expr_await_to_yield(expr),
            ForInOfLeft::Pat(pat) if matches!(&pat.kind, PatKind::Ident(name) if name == "<error>") =>
            {
                let text = self.copy_span_trimmed(pat.span);
                let text = text.replace("await ", "yield ");
                self.write(&text);
            }
            _ => self.emit_for_in_of_left(left),
        }
    }

    pub(crate) fn emit_stmt_body_await_to_yield(&mut self, stmt: &Stmt) {
        if matches!(stmt.kind, StmtKind::Block(_)) {
            self.emit_stmt_await_to_yield(stmt);
        } else {
            self.newline();
            self.indent += 1;
            self.emit_stmt_await_to_yield(stmt);
            self.indent -= 1;
        }
    }

    /// All clauses in a switch share one CaseBlock lexical scope. A resource
    /// declared in one clause therefore remains live across fallthrough and is
    /// disposed only when control leaves the switch as a whole.
    fn emit_async_switch_using_dispose_scope(&mut self, switch_stmt: &SwitchStmt) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        let has_await = switch_stmt.cases.iter().any(|case| {
            case.consequent.iter().any(
                |stmt| matches!(&stmt.kind, StmtKind::Var(var) if var.kind == VarKind::AwaitUsing),
            )
        });

        self.writeln("{");
        self.indent += 1;
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        self.write("switch (");
        self.emit_expr_await_to_yield(&switch_stmt.discriminant);
        self.writeln(") {");
        self.indent += 1;
        for case in &switch_stmt.cases {
            if let Some(test) = case.test.as_deref() {
                self.write("case ");
                self.emit_expr_await_to_yield(test);
                self.writeln(":");
            } else {
                self.writeln("default:");
            }
            self.indent += 1;
            for stmt in &case.consequent {
                self.emit_leading_comments(stmt.span.start);
                if let StmtKind::Var(var) = &stmt.kind {
                    if matches!(var.kind, VarKind::Using | VarKind::AwaitUsing) {
                        let is_async = var.kind == VarKind::AwaitUsing;
                        for declaration in &var.declarations {
                            let (PatKind::Ident(name), Some(initializer)) =
                                (&declaration.name.kind, declaration.init.as_deref())
                            else {
                                self.emit_stmt_await_to_yield(stmt);
                                continue;
                            };
                            self.write("const ");
                            self.write(name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write(&format!("__addDisposableResource(env_{env_num}, "));
                            self.emit_async_using_initializer(name, initializer, 0);
                            self.write(if is_async { ", true" } else { ", false" });
                            self.writeln(");");
                        }
                        self.advance_comment_pos(stmt.span.end);
                        continue;
                    }
                }
                self.emit_stmt_await_to_yield(stmt);
                self.advance_comment_pos(stmt.span.end);
            }
            self.indent -= 1;
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if has_await {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.write("yield ");
            if self.in_async_generator_transform {
                self.write(self.helper_prefix());
                self.write("__await(");
            }
            self.write(&format!("result_{env_num}"));
            if self.in_async_generator_transform {
                self.write(")");
            }
            self.writeln(";");
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
    }

    /// A `using` declaration in a classic `for` initializer owns a scope that
    /// includes the entire loop. Keep that scope inside the awaiter's generator
    /// so abrupt completion still executes disposal before the async function
    /// settles.
    fn emit_async_for_using_dispose_scope(
        &mut self,
        for_stmt: &ForStmt,
        var_stmt: &VarStmt,
        labels: &[&str],
    ) {
        self.needs_add_disposable_resource_helper = true;
        self.needs_dispose_resources_helper = true;
        let env_num = self.next_using_env_num();
        let is_async = var_stmt.kind == VarKind::AwaitUsing;

        self.writeln("{");
        self.indent += 1;
        self.writeln(&format!(
            "const env_{env_num} = {{ stack: [], error: void 0, hasError: false }};"
        ));
        self.writeln("try {");
        self.indent += 1;
        self.write("const ");
        for (index, declaration) in var_stmt.declarations.iter().enumerate() {
            if index > 0 {
                self.write(", ");
            }
            self.emit_binding_name(&declaration.name);
            if let Some(initializer) = declaration.init.as_ref() {
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write(&format!("__addDisposableResource(env_{env_num}, "));
                self.indent += 1;
                if let PatKind::Ident(name) = &declaration.name.kind {
                    self.emit_async_using_initializer(name, initializer, 1);
                } else {
                    self.emit_expr_await_to_yield(initializer);
                }
                self.indent -= 1;
                self.write(if is_async { ", true)" } else { ", false)" });
            }
        }
        self.writeln(";");
        for label in labels {
            self.write(label);
            self.writeln(":");
        }
        self.write("for (;");
        if let Some(test) = for_stmt.test.as_ref() {
            self.write(" ");
            self.emit_expr_await_to_yield(test);
        }
        self.write(";");
        if let Some(update) = for_stmt.update.as_ref() {
            self.write(" ");
            self.emit_expr_await_to_yield(update);
        }
        self.write(")");
        if matches!(for_stmt.body.kind, StmtKind::Block(_)) {
            self.write(" ");
        }
        self.emit_stmt_body_await_to_yield(&for_stmt.body);
        self.indent -= 1;
        self.writeln("}");
        self.writeln(&format!("catch (e_{env_num}) {{"));
        self.indent += 1;
        self.writeln(&format!("env_{env_num}.error = e_{env_num};"));
        self.writeln(&format!("env_{env_num}.hasError = true;"));
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally {");
        self.indent += 1;
        if is_async {
            self.writeln(&format!(
                "const result_{env_num} = {}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
            self.writeln(&format!("if (result_{env_num})"));
            self.indent += 1;
            self.write("yield ");
            if self.in_async_generator_transform {
                self.write(self.helper_prefix());
                self.write("__await(");
            }
            self.write(&format!("result_{env_num}"));
            if self.in_async_generator_transform {
                self.write(")");
            }
            self.writeln(";");
            self.indent -= 1;
        } else {
            self.writeln(&format!(
                "{}__disposeResources(env_{env_num});",
                self.helper_prefix()
            ));
        }
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
    }

    /// Check if an expression is a string literal, unwrapping parenthesized
    /// expressions.  Used for dynamic import() where the parser may wrap
    /// the specifier in parens (e.g., `import<T>("./0")` → arg is `("./0")`).
    pub(super) fn is_import_string_literal(arg: &Expr) -> bool {
        match &arg.kind {
            ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) => true,
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::is_import_string_literal(inner)
            }
            ExprKind::TypeAssertion(ta) => Self::is_import_string_literal(&ta.expr),
            ExprKind::As(a) => Self::is_import_string_literal(&a.expr),
            ExprKind::Satisfies(s) => Self::is_import_string_literal(&s.expr),
            ExprKind::Instantiation(inst) => Self::is_import_string_literal(&inst.expr),
            _ => false,
        }
    }

    /// Get the inner string literal from a potentially parenthesized or
    /// type-wrapped expression, for use when emitting the require() call.
    pub(super) fn unwrap_import_string_literal(arg: &Expr) -> &Expr {
        match &arg.kind {
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => {
                Self::unwrap_import_string_literal(inner)
            }
            ExprKind::TypeAssertion(ta) => Self::unwrap_import_string_literal(&ta.expr),
            ExprKind::As(a) => Self::unwrap_import_string_literal(&a.expr),
            ExprKind::Satisfies(s) => Self::unwrap_import_string_literal(&s.expr),
            ExprKind::Instantiation(inst) => Self::unwrap_import_string_literal(&inst.expr),
            _ => arg,
        }
    }

    pub(crate) fn is_dynamic_import_call(&self, call: &CallExpr) -> bool {
        if call.optional || call.args.len() > 2 {
            return false;
        }
        let is_import_callee = matches!(call.callee.kind, ExprKind::Ident(ref name) if name == "import")
            || {
                let start = call.callee.span.start as usize;
                let end = call.callee.span.end as usize;
                start < end
                    && end <= self.source.len()
                    && self.source[start..end].trim() == "import"
            };
        if !is_import_callee {
            return false;
        }
        // Skip import() calls that appear to be from type intersection/union fragments
        // (error recovery from type positions like `& import("pkg")` or `| import("pkg")`)
        let start = call.callee.span.start as usize;
        if start > 0 {
            let before = self.source[..start].trim_end();
            if before.ends_with('&') || before.ends_with('|') {
                return false;
            }
        }
        true
    }

    pub(super) fn emit_rewritten_relative_import_arg(&mut self, arg: &Expr) {
        let inner = Self::unwrap_import_string_literal(arg);
        let literal = match &inner.kind {
            ExprKind::StrLit(value) | ExprKind::NoSubstTemplate(value) => Some(value.as_str()),
            _ => None,
        };
        if let Some(value) = literal {
            let preserve_jsx = self.options.jsx == Some(JsxEmit::Preserve);
            let rewritten = self.rewrite_relative_import_specifier(value, preserve_jsx);
            let start = inner.span.start as usize;
            let quote = self
                .source
                .as_bytes()
                .get(start)
                .copied()
                .filter(|q| matches!(*q, b'\'' | b'"' | b'`'))
                .map(char::from)
                .unwrap_or('"');
            self.write(&quote.to_string());
            self.write(&rewritten);
            self.write(&quote.to_string());
            return;
        }

        self.write("__rewriteRelativeImportExtension(");
        self.emit_expr(arg);
        if self.options.jsx == Some(JsxEmit::Preserve) {
            self.write(", true");
        }
        self.write(")");
    }

    /// Resolve a literal dynamic-import argument to its bundle-local module id.
    ///
    /// The harness supplies an explicit map because resolving `./b.js` to `b.ts`
    /// (and therefore the bundle id `b`) needs the complete compilation.  When
    /// the map option is present, an absent entry deliberately means that the
    /// import was unresolved and its original spelling must be preserved.
    fn out_file_dynamic_import_specifier(&self, arg: &Expr) -> Option<String> {
        if self.options.out_file.is_none() {
            return None;
        }
        let inner = Self::unwrap_import_string_literal(arg);
        let value = match &inner.kind {
            ExprKind::StrLit(value) | ExprKind::NoSubstTemplate(value) => value,
            _ => return None,
        };

        if let Some(raw_map) =
            super::get_other_option(self.options, "__tsrsOutFileDynamicImportMap")
        {
            if let Some(encoded_entries) = raw_map.strip_prefix("hex-v1\n") {
                return encoded_entries.lines().find_map(|line| {
                    let (specifier, module_id) = line.split_once('\t')?;
                    let specifier = Self::decode_hidden_option_component(specifier)?;
                    if specifier != value.as_str() {
                        return None;
                    }
                    Self::decode_hidden_option_component(module_id)
                });
            }
            return raw_map.lines().find_map(|line| {
                let (specifier, module_id) = line.split_once('\t')?;
                (specifier == value.as_str()).then(|| module_id.to_string())
            });
        }

        // Without a program/module-resolution context, preserve the source
        // spelling. Guessing a bundle id by stripping `./` is wrong for
        // nested sources, explicit extensions, and parent-directory imports.
        None
    }

    fn decode_hidden_option_component(value: &str) -> Option<String> {
        if !value.len().is_multiple_of(2) {
            return None;
        }
        let mut decoded = Vec::with_capacity(value.len() / 2);
        for pair in value.as_bytes().chunks_exact(2) {
            let high = (pair[0] as char).to_digit(16)? as u8;
            let low = (pair[1] as char).to_digit(16)? as u8;
            decoded.push((high << 4) | low);
        }
        String::from_utf8(decoded).ok()
    }

    fn emit_out_file_dynamic_import_arg(&mut self, arg: &Expr) -> bool {
        let Some(module_id) = self.out_file_dynamic_import_specifier(arg) else {
            return false;
        };
        self.emit_dynamic_import_string_lit(arg, &module_id);
        true
    }

    /// Emit a dynamic import argument for System/CommonJS bundle transforms.
    pub(crate) fn emit_system_import_arg(&mut self, arg: &Expr) {
        if self.emit_out_file_dynamic_import_arg(arg) {
            return;
        }
        self.emit_expr(arg);
    }

    /// Same as `emit_system_import_arg` but uses `emit_expr_await_to_yield`
    /// for the fallback (used inside `__awaiter` generator bodies).
    pub(crate) fn emit_system_import_arg_await(&mut self, arg: &Expr) {
        if self.emit_out_file_dynamic_import_arg(arg) {
            return;
        }
        self.emit_expr_await_to_yield(arg);
    }

    /// Check if an expression is a phantom arg from parser error recovery
    /// (e.g., `import()` producing a `)` token as an expression arg).
    pub(crate) fn is_phantom_arg(&self, arg: &Expr) -> bool {
        let start = arg.span.start as usize;
        let end = arg.span.end as usize;
        if start >= end || end > self.source.len() {
            return true; // zero-width or invalid span
        }
        let text = self.source[start..end].trim();
        text.is_empty() || text == ")"
    }

    pub(crate) fn emit_dynamic_import_call_commonjs(&mut self, arg: Option<&Expr>) {
        let arg = match arg {
            Some(a) => a,
            None => {
                // import() with no arguments (grammar error recovery)
                self.write("Promise.resolve().then(() => ");
                self.write(self.helper_prefix());
                self.write("__importStar(require()))");
                return;
            }
        };
        if Self::is_import_string_literal(arg) {
            let inner = Self::unwrap_import_string_literal(arg);
            self.write("Promise.resolve().then(() => ");
            self.write(self.helper_prefix());
            self.write("__importStar(require(");
            if self.rewrite_relative_import_extensions() {
                self.emit_rewritten_relative_import_arg(inner);
            } else if self.options.out_file.is_some() {
                self.emit_system_import_arg(inner);
            } else {
                self.emit_expr(inner);
            }
            self.write(")))");
        } else {
            // Non-literal specifier: evaluate to string via template literal,
            // then pass the resolved value to require().
            self.write("Promise.resolve(`${");
            if self.rewrite_relative_import_extensions() {
                self.emit_rewritten_relative_import_arg(arg);
            } else {
                self.emit_expr(arg);
            }
            self.write("}`).then(s => ");
            self.write(self.helper_prefix());
            self.write("__importStar(require(s)))");
        }
    }

    /// Emit a dynamic import() call for UMD modules.
    /// UMD must work in both CJS and AMD environments, so the output is:
    ///   _a = <arg>, __syncRequire
    ///     ? Promise.resolve().then(() => __importStar(require(_a)))
    ///     : new Promise((resolve_1, reject_1) => { require([_a], resolve_1, reject_1); }).then(__importStar)
    /// For string literal args:
    ///   __syncRequire
    ///     ? Promise.resolve().then(() => __importStar(require(<arg>)))
    ///     : new Promise((resolve_1, reject_1) => { require([<arg>], resolve_1, reject_1); }).then(__importStar)
    pub(crate) fn emit_dynamic_import_call_umd(&mut self, arg: Option<&Expr>) {
        let arg = match arg {
            Some(a) => a,
            None => {
                // import() with no arguments (grammar error recovery)
                self.write("Promise.resolve().then(() => ");
                self.write(self.helper_prefix());
                self.write("__importStar(require()))");
                return;
            }
        };
        let is_string_lit = Self::is_import_string_literal(arg);
        let prefix = self.helper_prefix().to_string();
        if is_string_lit {
            self.amd_import_counter += 1;
            let n = self.amd_import_counter;
            let resolve = format!("resolve_{n}");
            let reject = format!("reject_{n}");
            let inner = Self::unwrap_import_string_literal(arg);
            // No temp needed for string literals.
            self.write("__syncRequire ? Promise.resolve().then(() => ");
            self.write(&prefix);
            self.write("__importStar(require(");
            if !self.emit_out_file_dynamic_import_arg(inner) {
                self.emit_expr(inner);
            }
            self.write("))) : new Promise((");
            self.write(&resolve);
            self.write(", ");
            self.write(&reject);
            self.write(") => { require([");
            if !self.emit_out_file_dynamic_import_arg(inner) {
                self.emit_expr(inner);
            }
            self.write("], ");
            self.write(&resolve);
            self.write(", ");
            self.write(&reject);
            self.write("); }).then(");
            self.write(&prefix);
            self.write("__importStar)");
        } else {
            // Non-literal: use a temp var to evaluate once.
            // Emit the argument BEFORE incrementing the counter so that nested
            // import() calls get lower counter numbers (matching TypeScript's
            // left-to-right evaluation order).
            let tmp = self.next_temp_var();
            self.write(&tmp);
            self.write(" = ");
            self.emit_expr(arg);
            self.amd_import_counter += 1;
            let n = self.amd_import_counter;
            let resolve = format!("resolve_{n}");
            let reject = format!("reject_{n}");
            self.write(", __syncRequire ? Promise.resolve().then(() => ");
            self.write(&prefix);
            self.write("__importStar(require(");
            self.write(&tmp);
            self.write("))) : new Promise((");
            self.write(&resolve);
            self.write(", ");
            self.write(&reject);
            self.write(") => { require([");
            self.write(&tmp);
            self.write("], ");
            self.write(&resolve);
            self.write(", ");
            self.write(&reject);
            self.write("); }).then(");
            self.write(&prefix);
            self.write("__importStar)");
        }
    }

    /// Helper to emit the AMD `new Promise(...)` import() pattern.
    fn emit_amd_import_promise(&mut self, n: usize, _prefix: &str) {
        let resolve = format!("resolve_{n}");
        let reject = format!("reject_{n}");
        self.write("new Promise((");
        self.write(&resolve);
        self.write(", ");
        self.write(&reject);
        self.write(") => { require([");
    }

    /// Helper to emit the closing part of AMD import() promise.
    fn emit_amd_import_promise_close(&mut self, n: usize, prefix: &str) {
        let resolve = format!("resolve_{n}");
        let reject = format!("reject_{n}");
        self.write("], ");
        self.write(&resolve);
        self.write(", ");
        self.write(&reject);
        self.write("); }).then(");
        self.write(prefix);
        self.write("__importStar)");
    }

    /// Emit a dynamic import() call for AMD modules.
    /// AMD pattern: `new Promise((resolve_N, reject_N) => { require([arg], resolve_N, reject_N); }).then(__importStar)`
    pub(crate) fn emit_dynamic_import_call_amd(&mut self, arg: Option<&Expr>) {
        let prefix = self.helper_prefix().to_string();
        self.amd_import_counter += 1;
        let n = self.amd_import_counter;
        self.emit_amd_import_promise(n, &prefix);
        if let Some(arg) = arg {
            self.emit_amd_dynamic_import_arg(arg);
        }
        self.emit_amd_import_promise_close(n, &prefix);
    }

    /// Emit a dynamic import() call for AMD modules inside an __awaiter generator body.
    pub(crate) fn emit_dynamic_import_call_amd_await_to_yield(&mut self, arg: Option<&Expr>) {
        let prefix = self.helper_prefix().to_string();
        self.amd_import_counter += 1;
        let n = self.amd_import_counter;
        self.emit_amd_import_promise(n, &prefix);
        if let Some(arg) = arg {
            if self.options.out_file.is_some() {
                if let Some(stripped) = self.strip_dot_slash_from_string_lit(arg) {
                    self.emit_dynamic_import_string_lit(arg, &stripped);
                } else {
                    self.emit_expr_await_to_yield(arg);
                }
            } else {
                self.emit_expr_await_to_yield(arg);
            }
        }
        self.emit_amd_import_promise_close(n, &prefix);
    }

    /// Emit a dynamic import argument for AMD, stripping "./" prefix in outFile bundles.
    fn emit_amd_dynamic_import_arg(&mut self, arg: &Expr) {
        if self.options.out_file.is_some() {
            if let Some(stripped) = self.strip_dot_slash_from_string_lit(arg) {
                self.emit_dynamic_import_string_lit(arg, &stripped);
                return;
            }
        }
        self.emit_expr(arg);
    }

    /// If the expression is a string literal starting with "./", return the value
    /// with "./" stripped. Returns None otherwise.
    fn strip_dot_slash_from_string_lit(&self, arg: &Expr) -> Option<String> {
        self.out_file_dynamic_import_specifier(arg)
    }

    /// Emit a string literal with a modified value, preserving the original quote style.
    fn emit_dynamic_import_string_lit(&mut self, arg: &Expr, value: &str) {
        let inner = Self::unwrap_import_string_literal(arg);
        let span_start = inner.span.start as usize;
        let span_end = inner.span.end as usize;
        let quote = if span_start < span_end && span_end <= self.source.len() {
            self.source
                .as_bytes()
                .get(span_start)
                .copied()
                .filter(|q| *q == b'"' || *q == b'\'')
                .map(char::from)
                .unwrap_or('"')
        } else {
            '"'
        };
        self.write(if quote == '\'' { "'" } else { "\"" });
        self.write(&super::emit_expr::escape_js_string_for_quote(value, quote));
        self.write(if quote == '\'' { "'" } else { "\"" });
    }

    pub(crate) fn emit_dynamic_import_call_commonjs_await_to_yield(&mut self, arg: Option<&Expr>) {
        let arg = match arg {
            Some(a) => a,
            None => {
                self.write("Promise.resolve().then(() => ");
                self.write(self.helper_prefix());
                self.write("__importStar(require()))");
                return;
            }
        };
        if Self::is_import_string_literal(arg) {
            let inner = Self::unwrap_import_string_literal(arg);
            self.write("Promise.resolve().then(() => ");
            self.write(self.helper_prefix());
            self.write("__importStar(require(");
            if !self.emit_out_file_dynamic_import_arg(inner) {
                self.emit_expr_await_to_yield(inner);
            }
            self.write(")))");
        } else {
            self.write("Promise.resolve(`${");
            self.emit_expr_await_to_yield(arg);
            self.write("}`).then(s => ");
            self.write(self.helper_prefix());
            self.write("__importStar(require(s)))");
        }
    }

    /// Emit an expression, converting `await expr` to `yield expr`.
    pub(crate) fn emit_expr_await_to_yield(&mut self, expr: &Expr) {
        if let ExprKind::Ident(name) = &expr.kind {
            if name == "arguments" {
                if let Some(alias) = self.current_arguments_alias.clone() {
                    self.write(&alias);
                    return;
                }
            }
        }
        if self.try_emit_const_enum_ref(expr) {
            return;
        }
        match &expr.kind {
            ExprKind::Await(inner) => {
                self.write("yield ");
                if self.in_async_generator_transform {
                    self.write(self.helper_prefix());
                    self.write("__await(");
                    self.emit_expr_await_to_yield(inner);
                    self.write(")");
                } else {
                    self.emit_expr_await_to_yield(inner);
                }
            }
            ExprKind::FnExpr(_) | ExprKind::Arrow(_) => {
                self.emit_expr(expr);
            }
            ExprKind::Paren(inner) => {
                match &inner.kind {
                    // When parens wrap a type expression that will be erased,
                    // strip unnecessary parens just like emit_expr does.
                    ExprKind::NonNull(inside) => {
                        let stripped = strip_type_layers(inside);
                        let is_safe_primary = matches!(
                            stripped.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Member(_)
                                | ExprKind::ElemAccess(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::This
                                | ExprKind::Super
                                | ExprKind::Paren(_)
                                | ExprKind::Template(_)
                                | ExprKind::TaggedTemplate(_)
                                | ExprKind::NumLit(_)
                                | ExprKind::BigIntLit(_)
                                | ExprKind::StrLit(_)
                                | ExprKind::BoolLit(_)
                                | ExprKind::NullLit
                                | ExprKind::RegexpLit(_)
                                | ExprKind::NoSubstTemplate(_)
                                | ExprKind::MetaProp(_)
                                | ExprKind::ClassExpr(_)
                        );
                        if is_safe_primary {
                            self.emit_expr_await_to_yield(inside);
                        } else {
                            self.write("(");
                            self.emit_expr_await_to_yield(inside);
                            self.write(")");
                        }
                    }
                    ExprKind::TypeAssertion(inside) => {
                        let stripped = strip_type_layers(&inside.expr);
                        let is_safe_primary = matches!(
                            stripped.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Member(_)
                                | ExprKind::ElemAccess(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::This
                                | ExprKind::Super
                                | ExprKind::Paren(_)
                                | ExprKind::Template(_)
                                | ExprKind::TaggedTemplate(_)
                                | ExprKind::NumLit(_)
                                | ExprKind::BigIntLit(_)
                                | ExprKind::StrLit(_)
                                | ExprKind::BoolLit(_)
                                | ExprKind::NullLit
                                | ExprKind::RegexpLit(_)
                                | ExprKind::NoSubstTemplate(_)
                                | ExprKind::MetaProp(_)
                                | ExprKind::ClassExpr(_)
                        );
                        if is_safe_primary {
                            self.emit_expr_await_to_yield(&inside.expr);
                        } else {
                            self.write("(");
                            self.emit_expr_await_to_yield(&inside.expr);
                            self.write(")");
                        }
                    }
                    ExprKind::As(inside) => {
                        let stripped = strip_type_layers(&inside.expr);
                        let is_safe_primary = matches!(
                            stripped.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Member(_)
                                | ExprKind::ElemAccess(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::This
                                | ExprKind::Super
                                | ExprKind::Paren(_)
                                | ExprKind::Template(_)
                                | ExprKind::TaggedTemplate(_)
                                | ExprKind::NumLit(_)
                                | ExprKind::BigIntLit(_)
                                | ExprKind::StrLit(_)
                                | ExprKind::BoolLit(_)
                                | ExprKind::NullLit
                                | ExprKind::RegexpLit(_)
                                | ExprKind::NoSubstTemplate(_)
                                | ExprKind::MetaProp(_)
                                | ExprKind::ClassExpr(_)
                        );
                        if is_safe_primary {
                            self.emit_expr_await_to_yield(&inside.expr);
                        } else {
                            self.write("(");
                            self.emit_expr_await_to_yield(&inside.expr);
                            self.write(")");
                        }
                    }
                    ExprKind::Satisfies(inside) => {
                        let stripped = strip_type_layers(&inside.expr);
                        let is_safe_primary = matches!(
                            stripped.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Member(_)
                                | ExprKind::ElemAccess(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::This
                                | ExprKind::Super
                                | ExprKind::Paren(_)
                                | ExprKind::Template(_)
                                | ExprKind::TaggedTemplate(_)
                                | ExprKind::NumLit(_)
                                | ExprKind::BigIntLit(_)
                                | ExprKind::StrLit(_)
                                | ExprKind::BoolLit(_)
                                | ExprKind::NullLit
                                | ExprKind::RegexpLit(_)
                                | ExprKind::NoSubstTemplate(_)
                                | ExprKind::MetaProp(_)
                                | ExprKind::ClassExpr(_)
                        );
                        if is_safe_primary {
                            self.emit_expr_await_to_yield(&inside.expr);
                        } else {
                            self.write("(");
                            self.emit_expr_await_to_yield(&inside.expr);
                            self.write(")");
                        }
                    }
                    ExprKind::Instantiation(inside) => {
                        let stripped = strip_type_layers(&inside.expr);
                        let is_safe_primary = matches!(
                            stripped.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Member(_)
                                | ExprKind::ElemAccess(_)
                                | ExprKind::Call(_)
                                | ExprKind::New(_)
                                | ExprKind::ArrayLit(_)
                                | ExprKind::This
                                | ExprKind::Super
                                | ExprKind::Paren(_)
                                | ExprKind::Template(_)
                                | ExprKind::TaggedTemplate(_)
                                | ExprKind::NumLit(_)
                                | ExprKind::BigIntLit(_)
                                | ExprKind::StrLit(_)
                                | ExprKind::BoolLit(_)
                                | ExprKind::NullLit
                                | ExprKind::RegexpLit(_)
                                | ExprKind::NoSubstTemplate(_)
                                | ExprKind::MetaProp(_)
                                | ExprKind::ClassExpr(_)
                        );
                        if is_safe_primary {
                            self.emit_expr_await_to_yield(&inside.expr);
                        } else {
                            self.write("(");
                            self.emit_expr_await_to_yield(&inside.expr);
                            self.write(")");
                        }
                    }
                    _ => {
                        self.write("(");
                        self.emit_expr_await_to_yield(inner);
                        self.write(")");
                    }
                }
            }
            ExprKind::Binary(bin) => {
                // Downlevel ** to Math.pow() when targeting < ES2016
                if bin.op == BinaryOp::Exp && self.needs_downlevel("exponentiation") {
                    self.write("Math.pow(");
                    // Inside Math.pow(...), yield is safe without extra parens
                    // since it's delimited by ( and , or ).
                    // Strip outer parens — they were for grouping in infix
                    // but are unnecessary inside function call arguments.
                    self.emit_expr_await_to_yield(&bin.left);
                    self.write(", ");
                    self.emit_expr_await_to_yield(&bin.right);
                    self.write(")");
                } else {
                    // Wrap LHS in parens if it's a bare `await` (becomes `yield`)
                    // since `yield` has low precedence: `await p || a` -> `(yield p) || a`
                    // But NOT if already wrapped in Paren — the Paren case emits parens already.
                    let lhs_needs_parens = expr_is_bare_await(&bin.left);
                    if lhs_needs_parens {
                        self.write("(");
                    }
                    self.emit_expr_await_to_yield(&bin.left);
                    if lhs_needs_parens {
                        self.write(")");
                    }
                    self.write(" ");
                    self.write(binary_op_str(bin.op));
                    self.write(" ");
                    // Wrap RHS in parens if it's a bare `await` (becomes `yield`)
                    // since `yield` has low precedence and needs protection in binary ops.
                    let rhs_needs_parens = expr_is_bare_await(&bin.right);
                    if rhs_needs_parens {
                        self.write("(");
                    }
                    self.emit_expr_await_to_yield(&bin.right);
                    if rhs_needs_parens {
                        self.write(")");
                    }
                }
            }
            ExprKind::Assign(assign) => {
                // Super property write: super.x = v → _super.x = v
                if self.async_super_active && assign.op == AssignOp::Assign {
                    if let ExprKind::Member(ref mem) = assign.left.kind {
                        if matches!(&mem.object.kind, ExprKind::Super) {
                            let sfx = self.async_super_suffix.clone();
                            self.write("_super");
                            self.write(&sfx);
                            self.write(".");
                            self.write(&mem.property);
                            self.write(" = ");
                            self.emit_expr_await_to_yield(&assign.right);
                            return;
                        }
                    }
                    if let ExprKind::ElemAccess(ref ea) = assign.left.kind {
                        if matches!(&ea.object.kind, ExprKind::Super) {
                            let sfx = self.async_super_suffix.clone();
                            self.write("_superIndex");
                            self.write(&sfx);
                            self.write("(");
                            self.emit_expr_await_to_yield(&ea.index);
                            self.write(").value = ");
                            self.emit_expr_await_to_yield(&assign.right);
                            return;
                        }
                    }
                }
                // Private field SET: obj.#field = value → __classPrivateFieldSet(obj, _C_field, value, "f")
                if assign.op == AssignOp::Assign && self.needs_downlevel("private-fields") {
                    if let ExprKind::Member(ref mem) = assign.left.kind {
                        if mem.property.starts_with('#') {
                            let field_name = normalize_unicode_escapes(&mem.property[1..]);
                            if self
                                .current_class_private_fields
                                .contains(field_name.as_str())
                            {
                                self.needs_private_field_set = true;
                                let field_var = if let Some(ref cn) = self.current_class_name {
                                    let norm_cn = normalize_unicode_escapes(cn);
                                    format!("_{}_{}", norm_cn, field_name)
                                } else {
                                    format!("_{}", field_name)
                                };
                                self.write(self.helper_prefix());
                                self.write("__classPrivateFieldSet(");
                                self.emit_expr_await_to_yield(&mem.object);
                                self.write(", ");
                                self.write(&field_var);
                                self.write(", ");
                                self.emit_expr_await_to_yield(&assign.right);
                                self.write(", \"f\")");
                                return;
                            }
                        }
                    }
                }
                self.emit_expr_await_to_yield(&assign.left);
                self.write(" ");
                self.write(assign_op_str(assign.op));
                self.write(" ");
                self.emit_expr_await_to_yield(&assign.right);
            }
            ExprKind::Cond(cond) => {
                self.emit_expr_await_to_yield(&cond.test);
                self.write(" ? ");
                self.emit_expr_await_to_yield(&cond.consequent);
                self.write(" : ");
                self.emit_expr_await_to_yield(&cond.alternate);
            }
            ExprKind::Call(call) => {
                // Super method call hoisting:
                // super.x() → _super.x.call(this)
                // super["x"]() → _superIndex("x").call(this) or _superIndex("x").value.call(this)
                // For optional calls (?.()) on super, lower to temp var + null check pattern.
                // super.method?.() → (_a = _super.method) === null || _a === void 0 ? void 0 : _a.call(this)
                if self.async_super_active && call.optional {
                    if let ExprKind::Member(ref mem) = call.callee.kind {
                        if matches!(&mem.object.kind, ExprKind::Super) {
                            let sfx = self.async_super_suffix.clone();
                            let tmp = self.make_temp_name();
                            self.awaiter_body_temp_vars.push(tmp.clone());
                            self.write("(");
                            self.write(&tmp);
                            self.write(" = _super");
                            self.write(&sfx);
                            self.write(".");
                            self.write(&mem.property);
                            self.write(") === null || ");
                            self.write(&tmp);
                            self.write(" === void 0 ? void 0 : ");
                            self.write(&tmp);
                            self.write(".call(this");
                            for arg in &call.args {
                                self.write(", ");
                                self.emit_expr_await_to_yield(arg);
                            }
                            self.write(")");
                            return;
                        }
                    }
                }
                if self.async_super_active && !call.optional {
                    if let ExprKind::Member(ref mem) = call.callee.kind {
                        if matches!(&mem.object.kind, ExprKind::Super) {
                            let sfx = self.async_super_suffix.clone();
                            self.write("_super");
                            self.write(&sfx);
                            self.write(".");
                            self.write(&mem.property);
                            self.write(".call(this");
                            for arg in &call.args {
                                self.write(", ");
                                self.emit_expr_await_to_yield(arg);
                            }
                            self.write(")");
                            return;
                        }
                    }
                    if let ExprKind::ElemAccess(ref ea) = call.callee.kind {
                        if matches!(&ea.object.kind, ExprKind::Super) {
                            let sfx = self.async_super_suffix.clone();
                            self.write("_superIndex");
                            self.write(&sfx);
                            self.write("(");
                            self.emit_expr_await_to_yield(&ea.index);
                            if self.async_super_has_write {
                                self.write(").value.call(this");
                            } else {
                                self.write(").call(this");
                            }
                            for arg in &call.args {
                                self.write(", ");
                                self.emit_expr_await_to_yield(arg);
                            }
                            self.write(")");
                            return;
                        }
                    }
                }
                if self.is_system()
                    && !self.system_context_fn.is_empty()
                    && self.is_dynamic_import_call(call)
                {
                    // System modules: import(x) → context_N.import(x)
                    let ctx = self.system_context_fn.clone();
                    self.write(&ctx);
                    self.write(".import(");
                    self.emit_system_import_arg_await(&call.args[0]);
                    self.write(")");
                } else if self.should_downlevel_dynamic_import()
                    && self.is_dynamic_import_call(call)
                {
                    let arg = call
                        .args
                        .first()
                        .map(|v| &**v)
                        .filter(|a| !self.is_phantom_arg(a));
                    if self.is_umd() {
                        // UMD await-to-yield uses same UMD pattern (with __syncRequire ternary).
                        // Any temp vars allocated must go into awaiter_body_temp_vars
                        // so they hoist into the generator function body.
                        // Non-literal args produce a comma expression (_a = arg, __syncRequire ? ...)
                        // which needs parens after `yield` to bind correctly.
                        let needs_parens = arg.is_some_and(|a| !Self::is_import_string_literal(a));
                        let temps_before = self.temp_var_names.len();
                        if needs_parens {
                            self.write("(");
                        }
                        self.emit_dynamic_import_call_umd(arg);
                        if needs_parens {
                            self.write(")");
                        }
                        for t in self.temp_var_names.drain(temps_before..) {
                            self.awaiter_body_temp_vars.push(t.to_string());
                        }
                    } else if self.is_amd() {
                        self.emit_dynamic_import_call_amd_await_to_yield(arg);
                    } else {
                        self.emit_dynamic_import_call_commonjs_await_to_yield(arg);
                    }
                } else {
                    let quoted_cjs_import = match &call.callee.kind {
                        ExprKind::Ident(local)
                            if self.is_commonjs()
                                && self.cjs_string_import_locals.contains(local.as_str()) =>
                        {
                            self.cjs_import_map.get(local.as_str()).cloned()
                        }
                        _ => None,
                    };
                    if let Some((var_name, imported)) = quoted_cjs_import {
                        self.write("(0, ");
                        self.write_cjs_import_access(
                            &var_name,
                            &imported,
                            match &call.callee.kind {
                                ExprKind::Ident(local) => local,
                                _ => unreachable!(),
                            },
                        );
                        self.write(")");
                    } else {
                        self.emit_expr_await_to_yield(&call.callee);
                    }
                    if call.optional {
                        self.write("?.");
                    }
                    self.write("(");
                    for (i, arg) in call.args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.emit_expr_await_to_yield(arg);
                    }
                    self.write(")");
                }
            }
            ExprKind::New(new_expr) => {
                self.write("new ");
                self.emit_expr_await_to_yield(&new_expr.callee);
                if let Some(ref args) = new_expr.args {
                    self.write("(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.emit_expr_await_to_yield(arg);
                    }
                    self.write(")");
                }
            }
            ExprKind::Member(mem) => {
                // Super property read: super.x → _super.x
                if self.async_super_active && matches!(&mem.object.kind, ExprKind::Super) {
                    let sfx = self.async_super_suffix.clone();
                    self.write("_super");
                    self.write(&sfx);
                    self.write(".");
                    self.write(&mem.property);
                    return;
                }
                // Private field GET: obj.#field → __classPrivateFieldGet(obj, _C_field, "f")
                if mem.property.starts_with('#') && self.needs_downlevel("private-fields") {
                    let field_name = normalize_unicode_escapes(&mem.property[1..]);
                    if let Some(method_var) = self
                        .current_class_private_methods
                        .get(field_name.as_str())
                        .cloned()
                    {
                        self.needs_private_field_get = true;
                        let brand_var = if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_instances", norm_cn)
                        } else {
                            "_instances".to_string()
                        };
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        self.emit_expr_await_to_yield(&mem.object);
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", \"m\", ");
                        self.write(&method_var);
                        self.write(")");
                    } else if let Some((getter_var, _)) = self
                        .current_class_private_accessors
                        .get(field_name.as_str())
                        .cloned()
                    {
                        self.needs_private_field_get = true;
                        let brand_var = if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_instances", norm_cn)
                        } else {
                            "_instances".to_string()
                        };
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        self.emit_expr_await_to_yield(&mem.object);
                        self.write(", ");
                        self.write(&brand_var);
                        self.write(", \"a\"");
                        if let Some(ref gv) = getter_var {
                            self.write(", ");
                            self.write(gv);
                        }
                        self.write(")");
                    } else if self
                        .current_class_private_fields
                        .contains(field_name.as_str())
                    {
                        self.needs_private_field_get = true;
                        let field_var = if let Some(ref cn) = self.current_class_name {
                            let norm_cn = normalize_unicode_escapes(cn);
                            format!("_{}_{}", norm_cn, field_name)
                        } else {
                            format!("_{}", field_name)
                        };
                        self.write(self.helper_prefix());
                        self.write("__classPrivateFieldGet(");
                        self.emit_expr_await_to_yield(&mem.object);
                        self.write(", ");
                        self.write(&field_var);
                        self.write(", \"f\")");
                    } else {
                        self.emit_expr_await_to_yield(&mem.object);
                        self.write(".");
                    }
                } else {
                    let prev_member_ctx = self.in_member_object_context;
                    self.in_member_object_context = true;
                    self.emit_expr_await_to_yield(&mem.object);
                    self.in_member_object_context = prev_member_ctx;
                    if mem.optional {
                        self.write("?.");
                    } else {
                        self.write(".");
                    }
                    self.write(&mem.property);
                }
            }
            ExprKind::ElemAccess(ea) => {
                // Super element read: super["x"] → _superIndex("x") or _superIndex("x").value
                if self.async_super_active && matches!(&ea.object.kind, ExprKind::Super) {
                    let sfx = self.async_super_suffix.clone();
                    self.write("_superIndex");
                    self.write(&sfx);
                    self.write("(");
                    self.emit_expr_await_to_yield(&ea.index);
                    if self.async_super_has_write {
                        self.write(").value");
                    } else {
                        self.write(")");
                    }
                    return;
                }
                let prev_member_ctx = self.in_member_object_context;
                self.in_member_object_context = true;
                self.emit_expr_await_to_yield(&ea.object);
                self.in_member_object_context = prev_member_ctx;
                if ea.optional {
                    self.write("?.");
                }
                self.write("[");
                self.emit_expr_await_to_yield(&ea.index);
                self.write("]");
            }
            ExprKind::ArrayLit(elements) => {
                let start = expr.span.start as usize;
                let end = expr.span.end as usize;
                let is_multiline = start < end
                    && end <= self.source.len()
                    && self.source[start..end].contains('\n');
                let source_comma_count =
                    super::emit_expr::array_literal_top_level_comma_count(self.source, expr.span);
                let base_separator_count = elements.len().saturating_sub(1);
                let has_trailing_comma = source_comma_count > base_separator_count;
                let first_on_same_line = is_multiline
                    && elements
                        .first()
                        .and_then(|slot| slot.as_ref())
                        .is_some_and(|first_elem| {
                            let bracket_pos = expr.span.start as usize;
                            let first_pos = first_elem.span.start as usize;
                            bracket_pos < first_pos
                                && first_pos <= self.source.len()
                                && !self.source[bracket_pos..first_pos].contains('\n')
                        });

                self.write("[");
                let consumed_preowned_indent =
                    self.await_expr_inline_container_indents_preowned > 0;
                if consumed_preowned_indent {
                    self.await_expr_inline_container_indents_preowned -= 1;
                } else {
                    self.indent += 1;
                }
                if is_multiline && !first_on_same_line {
                    self.newline();
                }
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(",");
                        if is_multiline {
                            self.newline();
                        } else {
                            self.write(" ");
                        }
                    }
                    if let Some(ref e) = elem {
                        self.emit_expr_await_to_yield(e);
                    }
                }
                if has_trailing_comma {
                    self.write(",");
                }
                if is_multiline && !first_on_same_line && !self.at_line_start {
                    self.newline();
                }
                self.indent -= 1;
                if consumed_preowned_indent {
                    self.await_expr_inline_container_indents_preowned += 1;
                }
                self.write("]");
                if consumed_preowned_indent {
                    self.indent += 1;
                }
            }
            ExprKind::Spread(inner) => {
                self.write("...");
                self.emit_expr_await_to_yield(inner);
            }
            ExprKind::Update(up) => {
                let op = match up.op {
                    UpdateOp::PreInc => "++",
                    UpdateOp::PreDec => "--",
                    UpdateOp::PostInc => "++",
                    UpdateOp::PostDec => "--",
                };
                let prefix = matches!(up.op, UpdateOp::PreInc | UpdateOp::PreDec);
                if prefix {
                    self.write(op);
                }
                self.emit_expr_await_to_yield(&up.argument);
                if !prefix {
                    self.write(op);
                }
            }
            ExprKind::Unary(un) => {
                let op = unary_op_str(un.op);
                self.write(op);
                // Wrap yield (from await-to-yield) in parens for correct
                // precedence: `!await x` -> `!(yield x)`, not `!yield x`
                if matches!(un.argument.kind, ExprKind::Await(_)) {
                    self.write("(");
                    self.emit_expr_await_to_yield(&un.argument);
                    self.write(")");
                } else {
                    self.emit_expr_await_to_yield(&un.argument);
                }
            }
            ExprKind::Typeof(inner) | ExprKind::Void(inner) | ExprKind::Delete(inner) => {
                let kw = match &expr.kind {
                    ExprKind::Typeof(_) => "typeof",
                    ExprKind::Void(_) => "void",
                    ExprKind::Delete(_) => "delete",
                    _ => unreachable!(),
                };
                self.write(kw);
                self.write(" ");
                // Wrap yield (from await-to-yield) in parens for correct
                // precedence: `typeof await x` -> `typeof (yield x)`
                if matches!(inner.kind, ExprKind::Await(_)) {
                    self.write("(");
                    self.emit_expr_await_to_yield(inner);
                    self.write(")");
                } else {
                    self.emit_expr_await_to_yield(inner);
                }
            }
            ExprKind::Template(tpl) => {
                self.emit_template_await_to_yield(tpl);
            }
            ExprKind::Yield(delegate, arg) => {
                if self.in_async_generator_transform {
                    if *delegate {
                        // yield* expr → yield __await(yield* __asyncDelegator(__asyncValues(expr)))
                        let prefix = self.helper_prefix().to_string();
                        self.write("yield ");
                        self.write(&prefix);
                        self.write("__await(yield* ");
                        self.write(&prefix);
                        self.write("__asyncDelegator(");
                        self.write(&prefix);
                        self.write("__asyncValues(");
                        if let Some(inner) = arg {
                            self.emit_expr_await_to_yield(inner);
                        }
                        self.write(")))");
                    } else {
                        // yield expr → yield yield __await(expr)
                        // yield (no arg) → yield yield __await(void 0)
                        self.write("yield yield ");
                        self.write(self.helper_prefix());
                        self.write("__await(");
                        if let Some(inner) = arg {
                            self.emit_expr_await_to_yield(inner);
                        } else {
                            self.write("void 0");
                        }
                        self.write(")");
                    }
                } else {
                    self.emit_expr(expr);
                }
            }
            ExprKind::TypeAssertion(ta_expr) => {
                if self.preserve_type_annotations {
                    let start = ta_expr.type_node.span.start as usize;
                    let end = ta_expr.type_node.span.end as usize;
                    if end <= self.source.len() {
                        self.write("<");
                        self.write(&self.source[start..end]);
                        self.write(">");
                    }
                }
                self.emit_expr_await_to_yield(&ta_expr.expr)
            }
            ExprKind::As(as_expr) => {
                if self.preserve_type_annotations {
                    self.emit_expr_await_to_yield(&as_expr.expr);
                    self.write(" as ");
                    let start = as_expr.type_node.span.start as usize;
                    let end = as_expr.type_node.span.end as usize;
                    if end <= self.source.len() {
                        self.write(&self.source[start..end]);
                    }
                } else {
                    self.emit_expr_await_to_yield(&as_expr.expr);
                }
            }
            ExprKind::Satisfies(sat_expr) => {
                if self.preserve_type_annotations {
                    self.emit_expr_await_to_yield(&sat_expr.expr);
                    self.write(" satisfies ");
                    let start = sat_expr.type_node.span.start as usize;
                    let end = sat_expr.type_node.span.end as usize;
                    if end <= self.source.len() {
                        self.write(&self.source[start..end]);
                    }
                } else {
                    self.emit_expr_await_to_yield(&sat_expr.expr);
                }
            }
            ExprKind::NonNull(inner) => self.emit_expr_await_to_yield(inner),
            ExprKind::Instantiation(inst) => self.emit_expr_await_to_yield(&inst.expr),
            ExprKind::ObjectLit(props) => {
                if props.is_empty() {
                    self.write("{}");
                    return;
                }
                // Downlevel object spread to Object.assign for target < ES2018
                let has_spread = props.iter().any(|p| matches!(p, ObjLitProp::Spread(_, _)));
                if has_spread && self.needs_downlevel("object-spread") {
                    self.emit_object_spread_downlevel(props, expr.span.end);
                    return;
                }
                // Preserve compact formatting for single-line source object
                // literals to match TypeScript's async downlevel emit.
                let start = expr.span.start as usize;
                let end = expr.span.end as usize;
                let source_single_line = start < end
                    && end <= self.source.len()
                    && !self.source[start..end].contains('\n');
                // Detect trailing comma in source
                let has_trailing_comma = if !props.is_empty() && end > 0 && end <= self.source.len()
                {
                    let last_prop = &props[props.len() - 1];
                    let lp_end = match last_prop {
                        ObjLitProp::Property(p) => p.span.end as usize,
                        ObjLitProp::Method(m) => m.span.end as usize,
                        ObjLitProp::Get(a) | ObjLitProp::Set(a) => a.span.end as usize,
                        ObjLitProp::Spread(_, sp) => sp.end as usize,
                        ObjLitProp::Shorthand(_, sp) => sp.end as usize,
                        ObjLitProp::ShorthandDefault(_, _, sp) => sp.end as usize,
                    };
                    if lp_end > 0 && lp_end < end {
                        let between = &self.source[lp_end..end];
                        let code = if let Some(idx) = between.find("//") {
                            &between[..idx]
                        } else {
                            between
                        };
                        code.contains(',')
                    } else {
                        false
                    }
                } else {
                    false
                };
                if source_single_line {
                    self.write("{ ");
                    let consumed_preowned_indent =
                        self.await_expr_inline_container_indents_preowned > 0;
                    if consumed_preowned_indent {
                        self.await_expr_inline_container_indents_preowned -= 1;
                    } else {
                        self.indent += 1;
                    }
                    for (i, prop) in props.iter().enumerate() {
                        self.emit_obj_lit_prop_await_to_yield(prop);
                        if i < props.len() - 1 {
                            self.write(", ");
                        } else if has_trailing_comma {
                            self.write(",");
                        }
                    }
                    if consumed_preowned_indent {
                        self.await_expr_inline_container_indents_preowned += 1;
                    } else {
                        self.indent -= 1;
                    }
                    self.write(" }");
                } else {
                    self.writeln("{");
                    let consumed_preowned_indent =
                        self.await_expr_inline_container_indents_preowned > 0;
                    if consumed_preowned_indent {
                        self.await_expr_inline_container_indents_preowned -= 1;
                    } else {
                        self.indent += 1;
                    }
                    for (i, prop) in props.iter().enumerate() {
                        self.emit_obj_lit_prop_await_to_yield(prop);
                        if i < props.len() - 1 || has_trailing_comma {
                            self.writeln(",");
                        } else {
                            self.newline();
                        }
                    }
                    self.indent -= 1;
                    if consumed_preowned_indent {
                        self.await_expr_inline_container_indents_preowned += 1;
                    }
                    self.write("}");
                    if consumed_preowned_indent {
                        self.indent += 1;
                    }
                }
            }
            ExprKind::Comma(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr_await_to_yield(e);
                }
            }
            // For leaf nodes, emit directly instead of going through emit_expr
            // which may source-copy the entire span (including any `await` keywords
            // that should have been converted to `yield`).
            ExprKind::Ident(name) => {
                if name == "<error>" {
                    return;
                }
                // CJS export qualification: `x` → `exports.x`
                if self.export_target.as_ref().is_some_and(|t| t == "exports")
                    && self.cjs_var_export_names.contains(name.as_str())
                    && !self.cjs_param_shadows.contains(name.as_str())
                {
                    self.write_cjs_export_access("exports", name);
                    return;
                }
                // CJS import reference rewriting: `x` → `a_1.default`
                if let Some((var_name, imported)) = self.cjs_import_map.get(name.as_str()).cloned()
                {
                    if imported.is_empty() {
                        self.write(&var_name);
                    } else {
                        self.write_cjs_import_access(&var_name, &imported, name);
                    }
                    return;
                }
                // Namespace export qualification: `x` → `NS.x`
                if self.export_target.as_ref().is_some_and(|t| t != "exports")
                    && self.namespace_exports.contains(name.as_str())
                    && !self.cjs_param_shadows.contains(name.as_str())
                {
                    let target = self.export_target.clone().unwrap();
                    self.write(&target);
                    self.write(".");
                }
                self.write(name);
            }
            ExprKind::NumLit(n) => self.write(&Self::normalize_numeric_literal(n)),
            ExprKind::StrLit(_) => self.copy_span(expr.span),
            ExprKind::NoSubstTemplate(_) => self.emit_expr(expr),
            ExprKind::BoolLit(v) => self.write(if *v { "true" } else { "false" }),
            ExprKind::NullLit => self.write("null"),
            ExprKind::This => self.write("this"),
            ExprKind::Super => self.write("super"),
            _ => self.emit_expr(expr),
        }
    }

    pub(crate) fn emit_obj_lit_prop_await_to_yield(&mut self, prop: &ObjLitProp) {
        match prop {
            ObjLitProp::Property(p) => {
                self.emit_prop_name(&p.key);
                self.write(": ");
                self.emit_expr_await_to_yield(&p.value);
            }
            ObjLitProp::Shorthand(name, _) => {
                let ns_qualify = self
                    .export_target
                    .as_ref()
                    .filter(|t| *t != "exports")
                    .is_some_and(|_| {
                        (self.namespace_exports.contains(name.as_str())
                            || self
                                .ns_export_stack
                                .iter()
                                .any(|(_, exports)| exports.contains(name.as_str())))
                            && !self.ns_local_bindings.contains(name.as_str())
                    });
                let needs_expand = ns_qualify
                    || self.cjs_import_map.get(name.as_str()).is_some_and(
                        |(var_name, imported)| {
                            !imported.is_empty() || var_name.as_str() != name.as_str()
                        },
                    );
                if needs_expand {
                    self.write(name);
                    self.write(": ");
                    self.emit_value_name_ref(name);
                } else {
                    self.write(name);
                }
            }
            ObjLitProp::Spread(expr, _) => {
                self.write("...");
                self.emit_expr_await_to_yield(expr);
            }
            _ => self.emit_obj_lit_prop(prop),
        }
    }

    pub(crate) fn class_decl_has_dynamic_import_call(&self, class_decl: &ClassDecl) -> bool {
        if class_decl.modifiers & MOD_DECLARE != 0 {
            return false;
        }
        class_decl
            .decorators
            .iter()
            .any(|decorator| self.expr_has_dynamic_import_call(decorator))
            || class_decl
                .extends
                .as_ref()
                .is_some_and(|e| self.expr_has_dynamic_import_call(e))
            || class_decl.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Property(prop) => {
                    let erased = prop.modifiers & (MOD_DECLARE | MOD_ABSTRACT) != 0;
                    if erased {
                        // Legacy decorators still emit a `__decorate` application for
                        // an otherwise-erased declared/abstract property. That call
                        // evaluates both the decorators and a computed property name.
                        self.options.experimental_decorators == Some(true)
                            && !matches!(prop.name, PropName::Private(..))
                            && !prop.decorators.is_empty()
                            && (self.prop_name_has_dynamic_import_call(&prop.name)
                                || prop
                                    .decorators
                                    .iter()
                                    .any(|decorator| self.expr_has_dynamic_import_call(decorator)))
                    } else {
                        self.prop_name_has_dynamic_import_call(&prop.name)
                            || prop
                                .decorators
                                .iter()
                                .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                            || prop
                                .initializer
                                .as_ref()
                                .is_some_and(|e| self.expr_has_dynamic_import_call(e))
                    }
                }
                ClassMemberKind::Method(method) => method.body.as_ref().is_some_and(|stmts| {
                    self.prop_name_has_dynamic_import_call(&method.name)
                        || method
                            .decorators
                            .iter()
                            .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                        || self.params_have_dynamic_import_call(&method.params)
                        || stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                }),
                ClassMemberKind::Constructor(ctor) => ctor.body.as_ref().is_some_and(|stmts| {
                    ctor.decorators
                        .iter()
                        .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                        || self.params_have_dynamic_import_call(&ctor.params)
                        || stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                }),
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    acc.body.as_ref().is_some_and(|stmts| {
                        self.prop_name_has_dynamic_import_call(&acc.name)
                            || acc
                                .decorators
                                .iter()
                                .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                            || self.params_have_dynamic_import_call(&acc.params)
                            || stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                    })
                }
                ClassMemberKind::StaticBlock(stmts) => {
                    stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                }
                _ => false,
            })
    }

    fn params_have_dynamic_import_call(&self, params: &[Param]) -> bool {
        params.iter().any(|p| {
            p.decorators
                .iter()
                .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                || self.pat_has_dynamic_import_call(&p.name)
                || p.initializer
                    .as_ref()
                    .is_some_and(|e| self.expr_has_dynamic_import_call(e))
        })
    }

    fn prop_name_has_dynamic_import_call(&self, name: &PropName) -> bool {
        matches!(name, PropName::Computed(expr, _) if self.expr_has_dynamic_import_call(expr))
    }

    fn var_stmt_has_dynamic_import_call(&self, var: &VarStmt) -> bool {
        var.declarations.iter().any(|declaration| {
            self.pat_has_dynamic_import_call(&declaration.name)
                || declaration
                    .init
                    .as_ref()
                    .is_some_and(|expr| self.expr_has_dynamic_import_call(expr))
        })
    }

    fn for_in_of_left_has_dynamic_import_call(&self, left: &ForInOfLeft) -> bool {
        match left {
            ForInOfLeft::Var(var) => self.var_stmt_has_dynamic_import_call(var),
            ForInOfLeft::Pat(pat) => self.pat_has_dynamic_import_call(pat),
            ForInOfLeft::Expr(expr) => self.expr_has_dynamic_import_call(expr),
        }
    }

    fn pat_has_dynamic_import_call(&self, pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Assign(inner_pat, expr) => {
                self.pat_has_dynamic_import_call(inner_pat)
                    || self.expr_has_dynamic_import_call(expr)
            }
            PatKind::Object(props) => props.iter().any(|prop| match prop {
                ObjPatProp::KeyValue(name, pat) => {
                    self.prop_name_has_dynamic_import_call(name)
                        || self.pat_has_dynamic_import_call(pat)
                }
                ObjPatProp::Rest(pat) => self.pat_has_dynamic_import_call(pat),
                ObjPatProp::ShorthandAssign(_, expr, _) => self.expr_has_dynamic_import_call(expr),
                ObjPatProp::Shorthand(_, _) => false,
            }),
            PatKind::Array(elems) => elems.iter().flatten().any(|elem| match elem {
                ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => {
                    self.pat_has_dynamic_import_call(pat)
                }
            }),
            PatKind::Rest(inner) => self.pat_has_dynamic_import_call(inner),
            PatKind::Ident(_) => false,
        }
    }

    fn jsx_attributes_have_dynamic_import_call(&self, attributes: &[JsxAttribute]) -> bool {
        attributes.iter().any(|attribute| match attribute {
            JsxAttribute::Normal { value, .. } => value
                .as_ref()
                .is_some_and(|expr| self.expr_has_dynamic_import_call(expr)),
            JsxAttribute::Spread(expr, _) => self.expr_has_dynamic_import_call(expr),
        })
    }

    fn jsx_children_have_dynamic_import_call(&self, children: &[JsxChild]) -> bool {
        children.iter().any(|child| match child {
            JsxChild::Element(expr) => self.expr_has_dynamic_import_call(expr),
            JsxChild::Expression(expr, _) => expr
                .as_ref()
                .is_some_and(|expr| self.expr_has_dynamic_import_call(expr)),
            JsxChild::Fragment(fragment) => {
                self.jsx_children_have_dynamic_import_call(&fragment.children)
            }
            JsxChild::Text(_, _) => false,
        })
    }

    pub(crate) fn expr_has_dynamic_import_call(&self, expr: &Expr) -> bool {
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::Call(call) => {
                    if self.is_dynamic_import_call(call) {
                        return true;
                    }
                    for arg in call.args.iter().rev() {
                        stack.push(arg);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::New(new_expr) => {
                    if let Some(args) = &new_expr.args {
                        for arg in args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                    stack.push(&new_expr.callee);
                }
                ExprKind::Member(mem) => stack.push(&mem.object),
                ExprKind::ElemAccess(ea) => {
                    stack.push(&ea.index);
                    stack.push(&ea.object);
                }
                ExprKind::Cond(cond) => {
                    stack.push(&cond.alternate);
                    stack.push(&cond.consequent);
                    stack.push(&cond.test);
                }
                ExprKind::Binary(bin) => {
                    stack.push(&bin.right);
                    stack.push(&bin.left);
                }
                ExprKind::Unary(un) => stack.push(&un.argument),
                ExprKind::Update(up) => stack.push(&up.argument),
                ExprKind::Assign(assign) => {
                    stack.push(&assign.right);
                    stack.push(&assign.left);
                }
                ExprKind::Paren(inner)
                | ExprKind::NonNull(inner)
                | ExprKind::Spread(inner)
                | ExprKind::Await(inner)
                | ExprKind::Delete(inner)
                | ExprKind::Typeof(inner)
                | ExprKind::Void(inner) => stack.push(inner),
                ExprKind::TypeAssertion(ta) => stack.push(&ta.expr),
                ExprKind::As(a) => stack.push(&a.expr),
                ExprKind::Satisfies(s) => stack.push(&s.expr),
                ExprKind::Instantiation(inst) => stack.push(&inst.expr),
                ExprKind::Yield(_, Some(inner)) => stack.push(inner),
                ExprKind::Yield(_, None) => {}
                ExprKind::ArrayLit(elems) => {
                    for inner in elems.iter().rev().flatten() {
                        stack.push(inner);
                    }
                }
                ExprKind::ObjectLit(props) => {
                    for prop in props.iter().rev() {
                        match prop {
                            ObjLitProp::Property(p) => {
                                if self.prop_name_has_dynamic_import_call(&p.key) {
                                    return true;
                                }
                                stack.push(&p.value);
                            }
                            ObjLitProp::Spread(e, _) => stack.push(e),
                            ObjLitProp::Method(m) => {
                                if self.prop_name_has_dynamic_import_call(&m.name)
                                    || self.params_have_dynamic_import_call(&m.params)
                                    || m.body.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                                {
                                    return true;
                                }
                            }
                            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                                if self.prop_name_has_dynamic_import_call(&a.name)
                                    || self.params_have_dynamic_import_call(&a.params)
                                    || a.body.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                                {
                                    return true;
                                }
                            }
                            ObjLitProp::Shorthand(_, _) => {}
                            ObjLitProp::ShorthandDefault(_, e, _) => stack.push(e),
                        }
                    }
                }
                ExprKind::FnExpr(fn_decl) => {
                    if fn_decl.body.as_ref().is_some_and(|stmts| {
                        fn_decl
                            .decorators
                            .iter()
                            .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                            || self.params_have_dynamic_import_call(&fn_decl.params)
                            || stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                    }) {
                        return true;
                    }
                }
                ExprKind::Arrow(arrow) => {
                    if self.params_have_dynamic_import_call(&arrow.params) {
                        return true;
                    }
                    match &arrow.body {
                        ArrowBody::Expr(e) => stack.push(e),
                        ArrowBody::Block(stmts) => {
                            if stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s)) {
                                return true;
                            }
                        }
                    }
                }
                ExprKind::ClassExpr(class_decl) => {
                    if self.class_decl_has_dynamic_import_call(class_decl) {
                        return true;
                    }
                }
                ExprKind::Template(tpl) => {
                    for inner in tpl.exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::TaggedTemplate(tagged) => {
                    for inner in tagged.quasi.exprs.iter().rev() {
                        stack.push(inner);
                    }
                    stack.push(&tagged.tag);
                }
                ExprKind::Comma(exprs) => {
                    for inner in exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::JsxElement(element) => {
                    if self.expr_has_dynamic_import_call(&element.name)
                        || self.jsx_attributes_have_dynamic_import_call(&element.attributes)
                        || self.jsx_children_have_dynamic_import_call(&element.children)
                    {
                        return true;
                    }
                }
                ExprKind::JsxSelfClosing(element) => {
                    if self.expr_has_dynamic_import_call(&element.name)
                        || self.jsx_attributes_have_dynamic_import_call(&element.attributes)
                    {
                        return true;
                    }
                }
                ExprKind::JsxFragment(fragment) => {
                    if self.jsx_children_have_dynamic_import_call(&fragment.children) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(crate) fn stmt_has_dynamic_import_call(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
                self.expr_has_dynamic_import_call(e)
            }
            StmtKind::Return(e) => e
                .as_ref()
                .is_some_and(|expr| self.expr_has_dynamic_import_call(expr)),
            StmtKind::Var(v) => {
                v.modifiers & MOD_DECLARE == 0 && self.var_stmt_has_dynamic_import_call(v)
            }
            StmtKind::If(i) => {
                self.expr_has_dynamic_import_call(&i.test)
                    || self.stmt_has_dynamic_import_call(&i.consequent)
                    || i.alternate
                        .as_ref()
                        .is_some_and(|a| self.stmt_has_dynamic_import_call(a))
            }
            StmtKind::While(w) => {
                self.expr_has_dynamic_import_call(&w.test)
                    || self.stmt_has_dynamic_import_call(&w.body)
            }
            StmtKind::DoWhile(d) => {
                self.stmt_has_dynamic_import_call(&d.body)
                    || self.expr_has_dynamic_import_call(&d.test)
            }
            StmtKind::For(f) => {
                f.init.as_ref().is_some_and(|init| match init {
                    ForInit::Expr(e) => self.expr_has_dynamic_import_call(e),
                    ForInit::Var(v) => self.var_stmt_has_dynamic_import_call(v),
                }) || f
                    .test
                    .as_ref()
                    .is_some_and(|e| self.expr_has_dynamic_import_call(e))
                    || f.update
                        .as_ref()
                        .is_some_and(|e| self.expr_has_dynamic_import_call(e))
                    || self.stmt_has_dynamic_import_call(&f.body)
            }
            StmtKind::ForIn(fi) => {
                self.for_in_of_left_has_dynamic_import_call(&fi.left)
                    || self.expr_has_dynamic_import_call(&fi.right)
                    || self.stmt_has_dynamic_import_call(&fi.body)
            }
            StmtKind::ForOf(fo) => {
                self.for_in_of_left_has_dynamic_import_call(&fo.left)
                    || self.expr_has_dynamic_import_call(&fo.right)
                    || self.stmt_has_dynamic_import_call(&fo.body)
            }
            StmtKind::Switch(sw) => {
                self.expr_has_dynamic_import_call(&sw.discriminant)
                    || sw.cases.iter().any(|c| {
                        c.test
                            .as_ref()
                            .is_some_and(|e| self.expr_has_dynamic_import_call(e))
                            || c.consequent
                                .iter()
                                .any(|s| self.stmt_has_dynamic_import_call(s))
                    })
            }
            StmtKind::Try(t) => {
                t.block.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                    || t.handler.as_ref().is_some_and(|h| {
                        h.param
                            .as_ref()
                            .is_some_and(|pat| self.pat_has_dynamic_import_call(pat))
                            || h.body.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                    })
                    || t.finalizer
                        .as_ref()
                        .is_some_and(|f| f.iter().any(|s| self.stmt_has_dynamic_import_call(s)))
            }
            StmtKind::Block(stmts) => stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s)),
            StmtKind::FnDecl(fn_decl) => {
                fn_decl.modifiers & MOD_DECLARE == 0
                    && fn_decl.body.as_ref().is_some_and(|stmts| {
                        fn_decl
                            .decorators
                            .iter()
                            .any(|decorator| self.expr_has_dynamic_import_call(decorator))
                            || self.params_have_dynamic_import_call(&fn_decl.params)
                            || stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                    })
            }
            StmtKind::ClassDecl(class_decl) => self.class_decl_has_dynamic_import_call(class_decl),
            StmtKind::EnumDecl(enum_decl) => {
                enum_decl.modifiers & MOD_DECLARE == 0
                    && (!enum_decl.is_const || self.preserve_const_enums_effective())
                    && enum_decl.members.iter().any(|member| {
                        self.prop_name_has_dynamic_import_call(&member.name)
                            || member
                                .initializer
                                .as_ref()
                                .is_some_and(|expr| self.expr_has_dynamic_import_call(expr))
                    })
            }
            StmtKind::ModuleDecl(module_decl)
                if module_decl.modifiers & MOD_DECLARE == 0
                    && !crate::analysis::module_decl_is_type_only(
                        module_decl,
                        self.preserve_const_enums_effective(),
                    ) =>
            {
                match &module_decl.body {
                    Some(ModuleBody::Block(stmts)) => {
                        stmts.iter().any(|s| self.stmt_has_dynamic_import_call(s))
                    }
                    Some(ModuleBody::Module(inner)) => {
                        let tmp = Stmt {
                            kind: StmtKind::ModuleDecl(Box::new((**inner).clone())),
                            span: inner.span,
                        };
                        self.stmt_has_dynamic_import_call(&tmp)
                    }
                    None => false,
                }
            }
            StmtKind::ImportEquals(ie) => self.expr_has_dynamic_import_call(&ie.module_ref),
            StmtKind::Export(ed) => match &ed.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.stmt_has_dynamic_import_call(inner)
                }
                ExportDeclKind::Default(e) => self.expr_has_dynamic_import_call(e),
                _ => false,
            },
            StmtKind::Labeled(l) => self.stmt_has_dynamic_import_call(&l.body),
            StmtKind::With(w) => {
                self.expr_has_dynamic_import_call(&w.object)
                    || self.stmt_has_dynamic_import_call(&w.body)
            }
            _ => false,
        }
    }

    fn class_decl_has_import_meta(&self, class_decl: &ClassDecl) -> bool {
        class_decl
            .extends
            .as_ref()
            .is_some_and(|e| self.expr_has_import_meta(e))
            || class_decl.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Property(prop) => prop
                    .initializer
                    .as_ref()
                    .is_some_and(|e| self.expr_has_import_meta(e)),
                ClassMemberKind::Method(method) => {
                    self.params_have_import_meta(&method.params)
                        || method
                            .body
                            .as_ref()
                            .is_some_and(|stmts| stmts.iter().any(|s| self.stmt_has_import_meta(s)))
                }
                ClassMemberKind::Constructor(ctor) => {
                    self.params_have_import_meta(&ctor.params)
                        || ctor
                            .body
                            .as_ref()
                            .is_some_and(|stmts| stmts.iter().any(|s| self.stmt_has_import_meta(s)))
                }
                ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                    self.params_have_import_meta(&acc.params)
                        || acc
                            .body
                            .as_ref()
                            .is_some_and(|stmts| stmts.iter().any(|s| self.stmt_has_import_meta(s)))
                }
                ClassMemberKind::StaticBlock(stmts) => {
                    stmts.iter().any(|s| self.stmt_has_import_meta(s))
                }
                _ => false,
            })
    }

    fn params_have_import_meta(&self, params: &[Param]) -> bool {
        params.iter().any(|p| {
            self.pat_has_import_meta(&p.name)
                || p.initializer
                    .as_ref()
                    .is_some_and(|e| self.expr_has_import_meta(e))
        })
    }

    fn pat_has_import_meta(&self, pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Assign(inner_pat, expr) => {
                self.pat_has_import_meta(inner_pat) || self.expr_has_import_meta(expr)
            }
            PatKind::Object(props) => props.iter().any(|prop| match prop {
                ObjPatProp::KeyValue(_, pat) | ObjPatProp::Rest(pat) => {
                    self.pat_has_import_meta(pat)
                }
                ObjPatProp::ShorthandAssign(_, expr, _) => self.expr_has_import_meta(expr),
                ObjPatProp::Shorthand(_, _) => false,
            }),
            PatKind::Array(elems) => elems.iter().flatten().any(|elem| match elem {
                ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => self.pat_has_import_meta(pat),
            }),
            PatKind::Rest(inner) => self.pat_has_import_meta(inner),
            PatKind::Ident(_) => false,
        }
    }

    pub(crate) fn expr_has_import_meta(&self, expr: &Expr) -> bool {
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::MetaProp(mp) => {
                    if mp.meta == "import" && mp.property == "meta" {
                        return true;
                    }
                }
                ExprKind::Call(call) => {
                    for arg in call.args.iter().rev() {
                        stack.push(arg);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::New(new_expr) => {
                    if let Some(args) = &new_expr.args {
                        for arg in args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                    stack.push(&new_expr.callee);
                }
                ExprKind::Member(mem) => stack.push(&mem.object),
                ExprKind::ElemAccess(ea) => {
                    stack.push(&ea.index);
                    stack.push(&ea.object);
                }
                ExprKind::Cond(cond) => {
                    stack.push(&cond.alternate);
                    stack.push(&cond.consequent);
                    stack.push(&cond.test);
                }
                ExprKind::Binary(bin) => {
                    stack.push(&bin.right);
                    stack.push(&bin.left);
                }
                ExprKind::Unary(un) => stack.push(&un.argument),
                ExprKind::Update(up) => stack.push(&up.argument),
                ExprKind::Assign(assign) => {
                    stack.push(&assign.right);
                    stack.push(&assign.left);
                }
                ExprKind::Paren(inner)
                | ExprKind::NonNull(inner)
                | ExprKind::Spread(inner)
                | ExprKind::Await(inner)
                | ExprKind::Delete(inner)
                | ExprKind::Typeof(inner)
                | ExprKind::Void(inner) => stack.push(inner),
                ExprKind::TypeAssertion(ta) => stack.push(&ta.expr),
                ExprKind::As(a) => stack.push(&a.expr),
                ExprKind::Satisfies(s) => stack.push(&s.expr),
                ExprKind::Instantiation(inst) => stack.push(&inst.expr),
                ExprKind::Yield(_, Some(inner)) => stack.push(inner),
                ExprKind::Yield(_, None) => {}
                ExprKind::ArrayLit(elems) => {
                    for inner in elems.iter().rev().flatten() {
                        stack.push(inner);
                    }
                }
                ExprKind::ObjectLit(props) => {
                    for prop in props.iter().rev() {
                        match prop {
                            ObjLitProp::Property(p) => stack.push(&p.value),
                            ObjLitProp::Spread(e, _) => stack.push(e),
                            ObjLitProp::Method(m) => {
                                if self.params_have_import_meta(&m.params)
                                    || m.body.iter().any(|s| self.stmt_has_import_meta(s))
                                {
                                    return true;
                                }
                            }
                            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                                if self.params_have_import_meta(&a.params)
                                    || a.body.iter().any(|s| self.stmt_has_import_meta(s))
                                {
                                    return true;
                                }
                            }
                            ObjLitProp::Shorthand(_, _) => {}
                            ObjLitProp::ShorthandDefault(_, e, _) => stack.push(e),
                        }
                    }
                }
                ExprKind::FnExpr(fn_decl) => {
                    if self.params_have_import_meta(&fn_decl.params)
                        || fn_decl
                            .body
                            .as_ref()
                            .is_some_and(|stmts| stmts.iter().any(|s| self.stmt_has_import_meta(s)))
                    {
                        return true;
                    }
                }
                ExprKind::Arrow(arrow) => {
                    if self.params_have_import_meta(&arrow.params) {
                        return true;
                    }
                    match &arrow.body {
                        ArrowBody::Expr(e) => stack.push(e),
                        ArrowBody::Block(stmts) => {
                            if stmts.iter().any(|s| self.stmt_has_import_meta(s)) {
                                return true;
                            }
                        }
                    }
                }
                ExprKind::ClassExpr(class_decl) => {
                    if self.class_decl_has_import_meta(class_decl) {
                        return true;
                    }
                }
                ExprKind::Template(tpl) => {
                    for inner in tpl.exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                ExprKind::TaggedTemplate(tagged) => {
                    for inner in tagged.quasi.exprs.iter().rev() {
                        stack.push(inner);
                    }
                    stack.push(&tagged.tag);
                }
                ExprKind::Comma(exprs) => {
                    for inner in exprs.iter().rev() {
                        stack.push(inner);
                    }
                }
                _ => {}
            }
        }
        false
    }

    pub(crate) fn stmt_has_import_meta(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
                self.expr_has_import_meta(e)
            }
            StmtKind::Return(e) => e
                .as_ref()
                .is_some_and(|expr| self.expr_has_import_meta(expr)),
            StmtKind::Var(v) => v.declarations.iter().any(|d| {
                d.init
                    .as_ref()
                    .is_some_and(|e| self.expr_has_import_meta(e))
            }),
            StmtKind::If(i) => {
                self.expr_has_import_meta(&i.test)
                    || self.stmt_has_import_meta(&i.consequent)
                    || i.alternate
                        .as_ref()
                        .is_some_and(|a| self.stmt_has_import_meta(a))
            }
            StmtKind::While(w) => {
                self.expr_has_import_meta(&w.test) || self.stmt_has_import_meta(&w.body)
            }
            StmtKind::DoWhile(d) => {
                self.stmt_has_import_meta(&d.body) || self.expr_has_import_meta(&d.test)
            }
            StmtKind::For(f) => {
                f.init.as_ref().is_some_and(|init| match init {
                    ForInit::Expr(e) => self.expr_has_import_meta(e),
                    ForInit::Var(v) => v.declarations.iter().any(|d| {
                        d.init
                            .as_ref()
                            .is_some_and(|e| self.expr_has_import_meta(e))
                    }),
                }) || f
                    .test
                    .as_ref()
                    .is_some_and(|e| self.expr_has_import_meta(e))
                    || f.update
                        .as_ref()
                        .is_some_and(|e| self.expr_has_import_meta(e))
                    || self.stmt_has_import_meta(&f.body)
            }
            StmtKind::ForIn(fi) => {
                self.expr_has_import_meta(&fi.right) || self.stmt_has_import_meta(&fi.body)
            }
            StmtKind::ForOf(fo) => {
                self.expr_has_import_meta(&fo.right) || self.stmt_has_import_meta(&fo.body)
            }
            StmtKind::Switch(sw) => {
                self.expr_has_import_meta(&sw.discriminant)
                    || sw.cases.iter().any(|c| {
                        c.test
                            .as_ref()
                            .is_some_and(|e| self.expr_has_import_meta(e))
                            || c.consequent.iter().any(|s| self.stmt_has_import_meta(s))
                    })
            }
            StmtKind::Try(t) => {
                t.block.iter().any(|s| self.stmt_has_import_meta(s))
                    || t.handler
                        .as_ref()
                        .is_some_and(|h| h.body.iter().any(|s| self.stmt_has_import_meta(s)))
                    || t.finalizer
                        .as_ref()
                        .is_some_and(|f| f.iter().any(|s| self.stmt_has_import_meta(s)))
            }
            StmtKind::Block(stmts) => stmts.iter().any(|s| self.stmt_has_import_meta(s)),
            StmtKind::FnDecl(fn_decl) => {
                self.params_have_import_meta(&fn_decl.params)
                    || fn_decl
                        .body
                        .as_ref()
                        .is_some_and(|stmts| stmts.iter().any(|s| self.stmt_has_import_meta(s)))
            }
            StmtKind::ClassDecl(class_decl) => self.class_decl_has_import_meta(class_decl),
            StmtKind::ModuleDecl(module_decl) => match &module_decl.body {
                Some(ModuleBody::Block(stmts)) => {
                    stmts.iter().any(|s| self.stmt_has_import_meta(s))
                }
                Some(ModuleBody::Module(inner)) => {
                    let tmp = Stmt {
                        kind: StmtKind::ModuleDecl(Box::new((**inner).clone())),
                        span: inner.span,
                    };
                    self.stmt_has_import_meta(&tmp)
                }
                None => false,
            },
            StmtKind::ImportEquals(ie) => self.expr_has_import_meta(&ie.module_ref),
            StmtKind::Export(ed) => match &ed.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.stmt_has_import_meta(inner)
                }
                ExportDeclKind::Default(e) => self.expr_has_import_meta(e),
                _ => false,
            },
            StmtKind::Labeled(l) => self.stmt_has_import_meta(&l.body),
            StmtKind::With(w) => {
                self.expr_has_import_meta(&w.object) || self.stmt_has_import_meta(&w.body)
            }
            _ => false,
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Async method super hoisting
    // ──────────────────────────────────────────────────────────────────

    /// Scan an async method body for `super.prop` and `super[expr]` accesses.
    /// Returns (named property names, has_element_access, has_write_access).
    pub(crate) fn scan_super_accesses(stmts: &[Stmt]) -> (Vec<String>, bool, bool) {
        let mut names = Vec::<String>::new();
        let mut has_element = false;
        let mut has_write = false;
        Self::scan_super_in_stmts(stmts, &mut names, &mut has_element, &mut has_write);
        // Deduplicate while preserving order.
        let mut seen = HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        (names, has_element, has_write)
    }

    pub(crate) fn scan_super_accesses_in_expr(expr: &Expr) -> (Vec<String>, bool, bool) {
        let mut names = Vec::<String>::new();
        let mut has_element = false;
        let mut has_write = false;
        Self::scan_super_in_expr(expr, &mut names, &mut has_element, &mut has_write);
        let mut seen = HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        (names, has_element, has_write)
    }

    pub(crate) fn async_arrow_super_hoist_info(
        &self,
        expr: &Expr,
    ) -> Option<(Vec<String>, bool, bool)> {
        let ExprKind::Arrow(arrow) = &expr.kind else {
            return None;
        };
        if !arrow.is_async || !self.needs_downlevel("async") || self.current_class_name.is_none() {
            return None;
        }
        let (names, has_element, has_write) = match &arrow.body {
            ArrowBody::Expr(expr) => Self::scan_super_accesses_in_expr(expr),
            ArrowBody::Block(stmts) => Self::scan_super_accesses(stmts),
        };
        if names.is_empty() && !has_element {
            None
        } else {
            Some((names, has_element, has_write))
        }
    }

    fn scan_super_in_stmts(
        stmts: &[Stmt],
        names: &mut Vec<String>,
        has_element: &mut bool,
        has_write: &mut bool,
    ) {
        for s in stmts {
            Self::scan_super_in_stmt(s, names, has_element, has_write);
        }
    }

    fn scan_super_in_stmt(
        stmt: &Stmt,
        names: &mut Vec<String>,
        has_element: &mut bool,
        has_write: &mut bool,
    ) {
        match &stmt.kind {
            StmtKind::Expr(e) => Self::scan_super_in_expr(e, names, has_element, has_write),
            StmtKind::Return(Some(e)) => Self::scan_super_in_expr(e, names, has_element, has_write),
            StmtKind::Var(v) => {
                for d in &v.declarations {
                    if let Some(ref init) = d.init {
                        Self::scan_super_in_expr(init, names, has_element, has_write);
                    }
                }
            }
            StmtKind::If(i) => {
                Self::scan_super_in_expr(&i.test, names, has_element, has_write);
                Self::scan_super_in_stmt(&i.consequent, names, has_element, has_write);
                if let Some(ref alt) = i.alternate {
                    Self::scan_super_in_stmt(alt, names, has_element, has_write);
                }
            }
            StmtKind::Block(stmts) => {
                Self::scan_super_in_stmts(stmts, names, has_element, has_write);
            }
            StmtKind::For(f) => {
                Self::scan_super_in_stmt(&f.body, names, has_element, has_write);
            }
            StmtKind::ForIn(f) => {
                Self::scan_super_in_stmt(&f.body, names, has_element, has_write);
            }
            StmtKind::ForOf(f) => {
                Self::scan_super_in_stmt(&f.body, names, has_element, has_write);
            }
            StmtKind::While(w) => {
                Self::scan_super_in_expr(&w.test, names, has_element, has_write);
                Self::scan_super_in_stmt(&w.body, names, has_element, has_write);
            }
            StmtKind::DoWhile(d) => {
                Self::scan_super_in_stmt(&d.body, names, has_element, has_write);
                Self::scan_super_in_expr(&d.test, names, has_element, has_write);
            }
            StmtKind::Try(t) => {
                Self::scan_super_in_stmts(&t.block, names, has_element, has_write);
                if let Some(ref c) = t.handler {
                    Self::scan_super_in_stmts(&c.body, names, has_element, has_write);
                }
                if let Some(ref f) = t.finalizer {
                    Self::scan_super_in_stmts(f, names, has_element, has_write);
                }
            }
            StmtKind::Switch(s) => {
                Self::scan_super_in_expr(&s.discriminant, names, has_element, has_write);
                for c in &s.cases {
                    Self::scan_super_in_stmts(&c.consequent, names, has_element, has_write);
                }
            }
            StmtKind::Throw(e) => {
                Self::scan_super_in_expr(e, names, has_element, has_write);
            }
            StmtKind::Labeled(l) => {
                Self::scan_super_in_stmt(&l.body, names, has_element, has_write);
            }
            _ => {}
        }
    }

    fn scan_super_in_expr(
        expr: &Expr,
        names: &mut Vec<String>,
        has_element: &mut bool,
        has_write: &mut bool,
    ) {
        match &expr.kind {
            ExprKind::Member(mem) => {
                if matches!(&mem.object.kind, ExprKind::Super) {
                    names.push(mem.property.to_string());
                } else {
                    Self::scan_super_in_expr(&mem.object, names, has_element, has_write);
                }
            }
            ExprKind::ElemAccess(ea) => {
                if matches!(&ea.object.kind, ExprKind::Super) {
                    *has_element = true;
                } else {
                    Self::scan_super_in_expr(&ea.object, names, has_element, has_write);
                }
                Self::scan_super_in_expr(&ea.index, names, has_element, has_write);
            }
            ExprKind::Call(call) => {
                Self::scan_super_in_expr(&call.callee, names, has_element, has_write);
                for arg in &call.args {
                    Self::scan_super_in_expr(arg, names, has_element, has_write);
                }
            }
            ExprKind::New(new_expr) => {
                Self::scan_super_in_expr(&new_expr.callee, names, has_element, has_write);
                if let Some(ref args) = new_expr.args {
                    for arg in args {
                        Self::scan_super_in_expr(arg, names, has_element, has_write);
                    }
                }
            }
            ExprKind::Assign(assign) => {
                // Check for super write on LHS
                match &assign.left.kind {
                    ExprKind::Member(mem) if matches!(&mem.object.kind, ExprKind::Super) => {
                        names.push(mem.property.to_string());
                        *has_write = true;
                    }
                    ExprKind::ElemAccess(ea) if matches!(&ea.object.kind, ExprKind::Super) => {
                        *has_element = true;
                        *has_write = true;
                    }
                    _ => {
                        Self::scan_super_in_expr(&assign.left, names, has_element, has_write);
                    }
                }
                Self::scan_super_in_expr(&assign.right, names, has_element, has_write);
            }
            ExprKind::Binary(bin) => {
                Self::scan_super_in_expr(&bin.left, names, has_element, has_write);
                Self::scan_super_in_expr(&bin.right, names, has_element, has_write);
            }
            ExprKind::Unary(u) => {
                Self::scan_super_in_expr(&u.argument, names, has_element, has_write);
            }
            ExprKind::Update(u) => {
                Self::scan_super_in_expr(&u.argument, names, has_element, has_write);
            }
            ExprKind::Cond(c) => {
                Self::scan_super_in_expr(&c.test, names, has_element, has_write);
                Self::scan_super_in_expr(&c.consequent, names, has_element, has_write);
                Self::scan_super_in_expr(&c.alternate, names, has_element, has_write);
            }
            ExprKind::Paren(inner) => {
                Self::scan_super_in_expr(inner, names, has_element, has_write);
            }
            ExprKind::Comma(exprs) => {
                for e in exprs {
                    Self::scan_super_in_expr(e, names, has_element, has_write);
                }
            }
            ExprKind::ArrayLit(elts) => {
                for e in elts.iter().flatten() {
                    Self::scan_super_in_expr(e, names, has_element, has_write);
                }
            }
            ExprKind::ObjectLit(props) => {
                for p in props {
                    match p {
                        ObjLitProp::Property(prop) => {
                            Self::scan_super_in_expr(&prop.value, names, has_element, has_write);
                        }
                        ObjLitProp::Spread(e, _) => {
                            Self::scan_super_in_expr(e, names, has_element, has_write);
                        }
                        ObjLitProp::ShorthandDefault(_, e, _) => {
                            Self::scan_super_in_expr(e, names, has_element, has_write);
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::Template(tpl) => {
                for e in &tpl.exprs {
                    Self::scan_super_in_expr(e, names, has_element, has_write);
                }
            }
            // Arrow functions and nested function expressions create new super scopes
            // so do NOT recurse into them.
            ExprKind::Arrow(a) => {
                // Arrow captures outer super, so DO scan inside.
                match &a.body {
                    ArrowBody::Expr(e) => {
                        Self::scan_super_in_expr(e, names, has_element, has_write);
                    }
                    ArrowBody::Block(stmts) => {
                        Self::scan_super_in_stmts(stmts, names, has_element, has_write);
                    }
                }
            }
            ExprKind::FnExpr(_) | ExprKind::ClassExpr(_) => {
                // These create new `super` scopes — don't recurse.
            }
            ExprKind::Spread(inner) => {
                Self::scan_super_in_expr(inner, names, has_element, has_write);
            }
            ExprKind::Yield(_, arg) => {
                if let Some(ref a) = arg {
                    Self::scan_super_in_expr(a, names, has_element, has_write);
                }
            }
            ExprKind::Await(inner) => {
                Self::scan_super_in_expr(inner, names, has_element, has_write);
            }
            ExprKind::NonNull(inner) => {
                Self::scan_super_in_expr(inner, names, has_element, has_write);
            }
            ExprKind::TypeAssertion(inner) => {
                Self::scan_super_in_expr(&inner.expr, names, has_element, has_write);
            }
            ExprKind::As(inner) => {
                Self::scan_super_in_expr(&inner.expr, names, has_element, has_write);
            }
            ExprKind::Satisfies(inner) => {
                Self::scan_super_in_expr(&inner.expr, names, has_element, has_write);
            }
            ExprKind::Instantiation(inner) => {
                Self::scan_super_in_expr(&inner.expr, names, has_element, has_write);
            }
            ExprKind::Delete(inner) | ExprKind::Void(inner) | ExprKind::Typeof(inner) => {
                Self::scan_super_in_expr(inner, names, has_element, has_write);
            }
            ExprKind::TaggedTemplate(ttl) => {
                Self::scan_super_in_expr(&ttl.tag, names, has_element, has_write);
                for e in &ttl.quasi.exprs {
                    Self::scan_super_in_expr(e, names, has_element, has_write);
                }
            }
            _ => {}
        }
    }

    /// Check if the body declares `_super` or `_superIndex` as local variables,
    /// and return a suffix like "_1" to avoid conflicts.
    fn compute_super_suffix(body: &[Stmt]) -> String {
        let has_conflict = body.iter().any(|s| {
            if let StmtKind::Var(var_stmt) = &s.kind {
                var_stmt.declarations.iter().any(|d| {
                    if let PatKind::Ident(ref name) = d.name.kind {
                        name == "_super" || name == "_superIndex"
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        });
        if has_conflict {
            "_1".to_string()
        } else {
            String::new()
        }
    }

    /// Emit super hoisting declarations before __awaiter/__asyncGenerator.
    /// Called from `emit_awaiter_body_with_params` and `emit_async_generator_body`.
    /// `is_generator` controls whether an empty `_super` is emitted for element-access-only.
    pub(crate) fn emit_super_hoisting(&mut self, is_generator: bool) {
        if !self.async_super_active {
            return;
        }
        let suffix = &self.async_super_suffix.clone();
        // _superIndex
        if self.async_super_has_element_access {
            if self.async_super_has_write {
                self.write("const _superIndex");
                self.write(suffix);
                self.writeln(" = (function (geti, seti) {");
                self.indent += 1;
                self.writeln("const cache = Object.create(null);");
                self.write("return name => cache[name] || (cache[name] = { ");
                self.write("get value() { return geti(name); }, ");
                self.writeln("set value(v) { seti(name, v); } });");
                self.indent -= 1;
                self.writeln("})(name => super[name], (name, value) => super[name] = value);");
            } else {
                self.write("const _superIndex");
                self.write(suffix);
                self.writeln(" = name => super[name];");
            }
        }
        // _super — for generators, always emitted when there's any super access (even element-only).
        // For __awaiter, only emitted when there are named property accesses.
        if !self.async_super_names.is_empty()
            || (is_generator && self.async_super_has_element_access)
        {
            let names = self.async_super_names.clone();
            if names.is_empty() {
                self.write("const _super");
                self.write(suffix);
                self.writeln(" = Object.create(null, {});");
            } else {
                self.write("const _super");
                self.write(suffix);
                self.write(" = Object.create(null, {");
                self.indent += 1;
                for (i, name) in names.iter().enumerate() {
                    self.newline();
                    self.write(name);
                    self.write(": { get: () => super.");
                    self.write(name);
                    if self.async_super_has_write {
                        self.write(", set: v => super.");
                        self.write(name);
                        self.write(" = v");
                    }
                    self.write(" }");
                    if i + 1 < names.len() {
                        self.write(",");
                    }
                }
                self.indent -= 1;
                self.newline();
                self.writeln("});");
            }
        }
    }

    // -----------------------------------------------------------------------
    // using/await using disposal helpers
    // -----------------------------------------------------------------------

    pub(crate) fn emit_add_disposable_resource_helper(&mut self) {
        self.writeln("var __addDisposableResource = (this && this.__addDisposableResource) || function (env, value, async) {");
        self.indent += 1;
        self.writeln("if (value !== null && value !== void 0) {");
        self.indent += 1;
        self.writeln("if (typeof value !== \"object\" && typeof value !== \"function\") throw new TypeError(\"Object expected.\");");
        self.writeln("var dispose, inner;");
        self.writeln("if (async) {");
        self.indent += 1;
        self.writeln("if (!Symbol.asyncDispose) throw new TypeError(\"Symbol.asyncDispose is not defined.\");");
        self.writeln("dispose = value[Symbol.asyncDispose];");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("if (dispose === void 0) {");
        self.indent += 1;
        self.writeln(
            "if (!Symbol.dispose) throw new TypeError(\"Symbol.dispose is not defined.\");",
        );
        self.writeln("dispose = value[Symbol.dispose];");
        self.writeln("if (async) inner = dispose;");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(
            "if (typeof dispose !== \"function\") throw new TypeError(\"Object not disposable.\");",
        );
        self.writeln("if (inner) dispose = function() { try { inner.call(this); } catch (e) { return Promise.reject(e); } };");
        self.writeln("env.stack.push({ value: value, dispose: dispose, async: async });");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("else if (async) {");
        self.indent += 1;
        self.writeln("env.stack.push({ async: true });");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("return value;");
        self.indent -= 1;
        self.writeln("};");
    }

    pub(crate) fn emit_dispose_resources_helper(&mut self) {
        self.writeln("var __disposeResources = (this && this.__disposeResources) || (function (SuppressedError) {");
        self.indent += 1;
        self.writeln("return function (env) {");
        self.indent += 1;
        self.writeln("function fail(e) {");
        self.indent += 1;
        self.writeln("env.error = env.hasError ? new SuppressedError(e, env.error, \"An error was suppressed during disposal.\") : e;");
        self.writeln("env.hasError = true;");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("var r, s = 0;");
        self.writeln("function next() {");
        self.indent += 1;
        self.writeln("while (r = env.stack.pop()) {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        self.writeln("if (!r.async && s === 1) return s = 0, env.stack.push(r), Promise.resolve().then(next);");
        self.writeln("if (r.dispose) {");
        self.indent += 1;
        self.writeln("var result = r.dispose.call(r.value);");
        self.writeln("if (r.async) return s |= 2, Promise.resolve(result).then(next, function(e) { fail(e); return next(); });");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("else s |= 1;");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("catch (e) {");
        self.indent += 1;
        self.writeln("fail(e);");
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("}");
        self.writeln(
            "if (s === 1) return env.hasError ? Promise.reject(env.error) : Promise.resolve();",
        );
        self.writeln("if (env.hasError) throw env.error;");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("return next();");
        self.indent -= 1;
        self.writeln("};");
        self.indent -= 1;
        self.writeln("})(typeof SuppressedError === \"function\" ? SuppressedError : function (error, suppressed, message) {");
        self.indent += 1;
        self.writeln("var e = new Error(message);");
        self.writeln(
            "return e.name = \"SuppressedError\", e.error = error, e.suppressed = suppressed, e;",
        );
        self.indent -= 1;
        self.writeln("});");
    }
}
