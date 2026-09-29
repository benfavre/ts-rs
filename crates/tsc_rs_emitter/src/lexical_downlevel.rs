//! Structural ES5 lowering analysis for lexical bindings.
//!
//! This module deliberately works from the parsed AST rather than source text.
//! It builds lexical environments before resolving references, so declarations
//! which occur later in a block still participate in binding resolution.  The
//! emitter consumes the resulting span-keyed plan without mutating the AST.

use super::*;

pub(crate) type BindingId = usize;
pub(crate) type ScopeId = usize;
pub(crate) type LoopId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SpanKey {
    pub(crate) start: u32,
    pub(crate) end: u32,
}

impl From<Span> for SpanKey {
    fn from(span: Span) -> Self {
        Self {
            start: span.start,
            end: span.end,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingKind {
    Var,
    Parameter,
    Catch,
    Lexical,
    Function,
    Class,
    Enum,
    Module,
    Import,
}

#[derive(Debug, Clone)]
pub(crate) struct BindingPlan {
    pub(crate) id: BindingId,
    pub(crate) source_name: AstString,
    pub(crate) emitted_name: AstString,
    pub(crate) kind: BindingKind,
    pub(crate) declaration_spans: Vec<SpanKey>,
    pub(crate) scope: ScopeId,
    pub(crate) var_scope: ScopeId,
    pub(crate) enclosing_loop: Option<LoopId>,
    pub(crate) function_depth: usize,
    pub(crate) captured: bool,
    pub(crate) directly_mutated: bool,
    pub(crate) preserve_native: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopKind {
    Classic,
    ForIn,
    ForOf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoopControlKind {
    Break,
    Continue,
    Return,
}

#[derive(Debug, Clone)]
pub(crate) struct LoopControl {
    pub(crate) span: SpanKey,
    pub(crate) kind: LoopControlKind,
    pub(crate) label: Option<AstString>,
    pub(crate) target_loop: Option<LoopId>,
    pub(crate) owner_loop: LoopId,
}

#[derive(Debug, Clone)]
pub(crate) struct LoopPlan {
    pub(crate) id: LoopId,
    pub(crate) span: SpanKey,
    pub(crate) kind: LoopKind,
    pub(crate) ordinal: usize,
    pub(crate) helper_name: AstString,
    pub(crate) state_name: AstString,
    pub(crate) header_bindings: Vec<BindingId>,
    pub(crate) captured_bindings: Vec<BindingId>,
    pub(crate) body_local_captures: Vec<BindingId>,
    pub(crate) copy_out_bindings: Vec<BindingId>,
    pub(crate) copy_out_names: HashMap<BindingId, AstString>,
    pub(crate) controls: Vec<LoopControl>,
    pub(crate) captures_this: bool,
    pub(crate) new_target_name: Option<AstString>,
    pub(crate) new_target_references: HashSet<SpanKey>,
    pub(crate) iterator_destructure: bool,
    pub(crate) simple_iterator_array_comment_range: Option<SpanKey>,
    pub(crate) supported: bool,
    pub(crate) preserve_native: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    File,
    Function,
    Block,
    Loop,
    Catch,
    Class,
}

#[derive(Debug, Clone)]
pub(crate) struct ScopePlan {
    pub(crate) parent: Option<ScopeId>,
    pub(crate) var_scope: ScopeId,
    kind: ScopeKind,
    function_depth: usize,
    owns_arguments: bool,
    bindings: HashMap<AstString, BindingId>,
    emitted_names: HashSet<AstString>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LexicalDownlevelPlan {
    pub(crate) bindings: Vec<BindingPlan>,
    pub(crate) scopes: Vec<ScopePlan>,
    pub(crate) loops: Vec<LoopPlan>,
    declaration_bindings: HashMap<SpanKey, BindingId>,
    reference_bindings: HashMap<SpanKey, BindingId>,
    loop_by_span: HashMap<SpanKey, LoopId>,
    control_by_span: HashMap<SpanKey, LoopControl>,
    scoped_enum_spans: HashSet<SpanKey>,
    renamed_reference_starts: Vec<u32>,
    pub(crate) supported: bool,
    // Reuse the binding walk's distinction between values and assignment
    // targets to identify array spreads without another whole-file traversal.
    pub(crate) array_spreads: HashMap<SpanKey, crate::array_spread::SpreadHelpers>,
    pub(crate) invocation_spreads: HashMap<SpanKey, crate::array_spread::SpreadHelpers>,
}

impl LexicalDownlevelPlan {
    pub(crate) fn analyze(
        file: &SourceFile,
        down_level_iteration: bool,
        preserve_const_enums: bool,
    ) -> Self {
        Analyzer::new(down_level_iteration, preserve_const_enums).analyze(file)
    }

    pub(crate) fn binding_for_declaration(&self, span: Span) -> Option<&BindingPlan> {
        if !self.supported {
            return None;
        }
        self.declaration_bindings
            .get(&span.into())
            .and_then(|id| self.bindings.get(*id))
    }

    pub(crate) fn binding_for_reference(&self, span: Span) -> Option<&BindingPlan> {
        if !self.supported {
            return None;
        }
        self.reference_bindings
            .get(&span.into())
            .and_then(|id| self.bindings.get(*id))
    }

    pub(crate) fn rename_generator_catch(&mut self, id: BindingId, name: AstString) {
        let binding = &mut self.bindings[id];
        debug_assert_eq!(binding.kind, BindingKind::Catch);
        if binding.emitted_name == name {
            return;
        }
        binding.emitted_name = name;
        self.renamed_reference_starts.extend(
            self.reference_bindings
                .iter()
                .filter_map(|(span, binding)| (*binding == id).then_some(span.start)),
        );
        self.renamed_reference_starts.sort_unstable();
        self.renamed_reference_starts.dedup();
    }

    pub(crate) fn emitted_name_for_declaration(&self, span: Span) -> Option<&str> {
        let binding = self.binding_for_declaration(span)?;
        (binding.emitted_name != binding.source_name).then_some(binding.emitted_name.as_str())
    }

    pub(crate) fn emitted_name_for_reference(&self, span: Span) -> Option<&str> {
        let binding = self.binding_for_reference(span)?;
        (binding.emitted_name != binding.source_name).then_some(binding.emitted_name.as_str())
    }

    pub(crate) fn loop_plan(&self, span: Span) -> Option<&LoopPlan> {
        if !self.supported {
            return None;
        }
        self.loop_by_span
            .get(&span.into())
            .and_then(|id| self.loops.get(*id))
            .filter(|plan| plan.supported && !plan.captured_bindings.is_empty())
    }

    pub(crate) fn needs_read_helper(&self) -> bool {
        self.supported
            && self.loops.iter().any(|plan| {
                plan.supported && plan.iterator_destructure && !plan.captured_bindings.is_empty()
            })
    }

    pub(crate) fn simple_iterator_array_comment_ranges(&self) -> Vec<SpanKey> {
        self.loops
            .iter()
            .filter(|plan| !plan.preserve_native)
            .filter_map(|plan| plan.simple_iterator_array_comment_range)
            .collect()
    }

    pub(crate) fn simple_iterator_array_comment_range(&self, span: Span) -> Option<SpanKey> {
        self.loop_by_span
            .get(&span.into())
            .and_then(|id| self.loops.get(*id))
            .filter(|plan| !plan.preserve_native)
            .and_then(|plan| plan.simple_iterator_array_comment_range)
    }

    pub(crate) fn loop_preserves_native(&self, span: Span) -> bool {
        self.loop_by_span
            .get(&span.into())
            .and_then(|id| self.loops.get(*id))
            .is_some_and(|plan| plan.preserve_native)
    }

    pub(crate) fn control(&self, span: Span) -> Option<&LoopControl> {
        if !self.supported {
            return None;
        }
        self.control_by_span.get(&span.into())
    }

    pub(crate) fn is_scoped_enum(&self, span: Span) -> bool {
        self.supported && self.scoped_enum_spans.contains(&span.into())
    }

    pub(crate) fn has_renamed_reference_in(&self, span: Span) -> bool {
        if !self.supported {
            return false;
        }
        let i = self
            .renamed_reference_starts
            .partition_point(|start| *start < span.start);
        self.renamed_reference_starts
            .get(i)
            .is_some_and(|start| *start < span.end)
    }

    pub(crate) fn preserves_lexical_declaration_in(&self, span: Span) -> bool {
        self.bindings.iter().any(|binding| {
            binding.preserve_native
                && binding
                    .declaration_spans
                    .iter()
                    .any(|decl| decl.start >= span.start && decl.end <= span.end)
        })
    }
}

#[derive(Debug, Clone)]
struct RawControl {
    span: SpanKey,
    kind: LoopControlKind,
    label: Option<AstString>,
    target_loop: Option<LoopId>,
    target_is_non_loop_label: bool,
    containing_loops: Vec<LoopId>,
    function_depth: usize,
}

#[derive(Debug, Clone)]
struct LabelTarget {
    name: AstString,
    loop_id: Option<LoopId>,
}

struct Analyzer {
    plan: LexicalDownlevelPlan,
    scope_stack: Vec<ScopeId>,
    loop_stack: Vec<LoopId>,
    loop_header_stack: Vec<LoopId>,
    break_stack: Vec<Option<LoopId>>,
    continue_stack: Vec<Option<LoopId>>,
    labels: Vec<LabelTarget>,
    raw_controls: Vec<RawControl>,
    reserved_names: HashSet<AstString>,
    normal_function_depth: usize,
    loop_normal_depths: Vec<usize>,
    loop_function_depths: Vec<usize>,
    suspending_function_stack: Vec<bool>,
    down_level_iteration: bool,
    preserve_const_enums: bool,
    suppress_array_spread: bool,
    erased_type_only_import_equals: HashSet<SpanKey>,
}

impl Analyzer {
    fn new(down_level_iteration: bool, preserve_const_enums: bool) -> Self {
        Self {
            plan: LexicalDownlevelPlan {
                supported: true,
                ..Default::default()
            },
            scope_stack: Vec::new(),
            loop_stack: Vec::new(),
            loop_header_stack: Vec::new(),
            break_stack: Vec::new(),
            continue_stack: Vec::new(),
            labels: Vec::new(),
            raw_controls: Vec::new(),
            reserved_names: HashSet::new(),
            normal_function_depth: 0,
            loop_normal_depths: Vec::new(),
            loop_function_depths: Vec::new(),
            suspending_function_stack: Vec::new(),
            down_level_iteration,
            preserve_const_enums,
            suppress_array_spread: false,
            erased_type_only_import_equals: HashSet::new(),
        }
    }

    fn analyze(mut self, file: &SourceFile) -> LexicalDownlevelPlan {
        for stmt in &file.statements {
            if is_erased_type_only_import_equals(stmt, &file.text) {
                self.erased_type_only_import_equals.insert(stmt.span.into());
            }
        }
        for token in Scanner::new(&file.text).scan_all() {
            if token.kind == TokenKind::Identifier {
                let start = token.span.start as usize;
                let end = token.span.end as usize;
                if let Some(name) = file.text.get(start..end) {
                    self.reserved_names
                        .insert(normalize_unicode_escapes(name).into());
                }
            }
        }
        let root = self.new_scope(None, ScopeKind::File, 0);
        self.scope_stack.push(root);
        self.collect_hoisted_vars(&file.statements, root);
        self.visit_stmt_list(&file.statements, root);
        self.scope_stack.pop();
        self.finish_loops();
        self.plan.renamed_reference_starts = self
            .plan
            .reference_bindings
            .iter()
            .filter_map(|(span, id)| {
                let binding = &self.plan.bindings[*id];
                (binding.source_name != binding.emitted_name).then_some(span.start)
            })
            .collect();
        self.plan.renamed_reference_starts.sort_unstable();
        self.plan
    }

    fn current_scope(&self) -> ScopeId {
        *self.scope_stack.last().expect("lexical scope")
    }

    fn current_function_depth(&self) -> usize {
        self.plan.scopes[self.current_scope()].function_depth
    }

    fn new_scope(
        &mut self,
        parent: Option<ScopeId>,
        kind: ScopeKind,
        function_depth: usize,
    ) -> ScopeId {
        let id = self.plan.scopes.len();
        let var_scope = if matches!(kind, ScopeKind::File | ScopeKind::Function) {
            id
        } else {
            parent.map_or(id, |p| self.plan.scopes[p].var_scope)
        };
        self.plan.scopes.push(ScopePlan {
            parent,
            var_scope,
            kind,
            function_depth,
            owns_arguments: false,
            bindings: HashMap::new(),
            emitted_names: HashSet::new(),
        });
        id
    }

    fn nearest_var_scope(&self, scope: ScopeId) -> ScopeId {
        self.plan.scopes[scope].var_scope
    }

    fn declare(
        &mut self,
        scope: ScopeId,
        name: &str,
        span: Span,
        kind: BindingKind,
        lexical: bool,
        enclosing_loop: Option<LoopId>,
    ) -> BindingId {
        let key = SpanKey::from(span);
        if let Some(existing) = self.plan.scopes[scope].bindings.get(name).copied() {
            if self.plan.bindings[existing]
                .declaration_spans
                .contains(&key)
            {
                self.plan.declaration_bindings.insert(key, existing);
                return existing;
            }
            let prior = self.plan.bindings[existing].kind;
            let compatible_merge = matches!(
                (prior, kind),
                (BindingKind::Var, BindingKind::Var)
                    | (BindingKind::Var, BindingKind::Function)
                    | (BindingKind::Function, BindingKind::Var)
                    | (BindingKind::Function, BindingKind::Function)
                    | (BindingKind::Enum, BindingKind::Enum)
                    | (BindingKind::Module, BindingKind::Module)
                    | (BindingKind::Enum, BindingKind::Module)
                    | (BindingKind::Module, BindingKind::Enum)
                    | (BindingKind::Class, BindingKind::Module)
                    | (BindingKind::Module, BindingKind::Class)
                    | (BindingKind::Function, BindingKind::Module)
                    | (BindingKind::Module, BindingKind::Function)
            );
            if !compatible_merge {
                self.plan.supported = false;
            }
            self.plan.bindings[existing].declaration_spans.push(key);
            self.plan.declaration_bindings.insert(key, existing);
            return existing;
        }

        let var_scope = self.nearest_var_scope(scope);
        let source_name = AstString::from(name);
        let emitted_name = if lexical
            && self.plan.scopes[var_scope]
                .emitted_names
                .contains(source_name.as_str())
        {
            self.unique_lexical_name(var_scope, name)
        } else {
            source_name.clone()
        };
        self.plan.scopes[var_scope]
            .emitted_names
            .insert(emitted_name.clone());
        let id = self.plan.bindings.len();
        let preserve_native = lexical
            && (self
                .suspending_function_stack
                .last()
                .copied()
                .unwrap_or(false)
                || enclosing_loop.is_some_and(|loop_id| {
                    self.plan
                        .loops
                        .get(loop_id)
                        .is_some_and(|plan| plan.preserve_native)
                }));
        self.plan.bindings.push(BindingPlan {
            id,
            source_name: source_name.clone(),
            emitted_name,
            kind,
            declaration_spans: vec![key],
            scope,
            var_scope,
            enclosing_loop,
            function_depth: self.plan.scopes[scope].function_depth,
            captured: false,
            directly_mutated: false,
            preserve_native,
        });
        self.plan.scopes[scope].bindings.insert(source_name, id);
        self.plan.declaration_bindings.insert(key, id);
        id
    }

    fn unique_lexical_name(&self, var_scope: ScopeId, base: &str) -> AstString {
        let mut suffix = 1usize;
        loop {
            let candidate = AstString::from(format!("{base}_{suffix}"));
            if !self.reserved_names.contains(candidate.as_str())
                && !self.plan.scopes[var_scope]
                    .emitted_names
                    .contains(candidate.as_str())
            {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn unique_generated_name(&mut self, base: &str) -> AstString {
        if !self.reserved_names.contains(base) {
            let name = AstString::from(base);
            self.reserved_names.insert(name.clone());
            return name;
        }
        let mut suffix = 1usize;
        loop {
            let candidate = AstString::from(format!("{base}_{suffix}"));
            if !self.reserved_names.contains(candidate.as_str()) {
                self.reserved_names.insert(candidate.clone());
                return candidate;
            }
            suffix += 1;
        }
    }

    fn unique_generated_family(&mut self, prefix: &str, mut ordinal: usize) -> AstString {
        loop {
            let candidate = AstString::from(format!("{prefix}{ordinal}"));
            if !self.reserved_names.contains(candidate.as_str()) {
                self.reserved_names.insert(candidate.clone());
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn declare_pat(
        &mut self,
        pat: &Pat,
        scope: ScopeId,
        kind: BindingKind,
        lexical: bool,
        enclosing_loop: Option<LoopId>,
    ) -> Vec<BindingId> {
        let mut ids = Vec::new();
        self.declare_pat_into(pat, scope, kind, lexical, enclosing_loop, &mut ids);
        ids
    }

    fn declare_pat_into(
        &mut self,
        pat: &Pat,
        scope: ScopeId,
        kind: BindingKind,
        lexical: bool,
        enclosing_loop: Option<LoopId>,
        ids: &mut Vec<BindingId>,
    ) {
        match &pat.kind {
            PatKind::Ident(name) => {
                ids.push(self.declare(scope, name, pat.span, kind, lexical, enclosing_loop))
            }
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    let nested = match element {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => p,
                    };
                    self.declare_pat_into(nested, scope, kind, lexical, enclosing_loop, ids);
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(_, p) | ObjPatProp::Rest(p) => {
                            self.declare_pat_into(p, scope, kind, lexical, enclosing_loop, ids)
                        }
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => ids.push(self.declare(
                            scope,
                            name,
                            *span,
                            kind,
                            lexical,
                            enclosing_loop,
                        )),
                    }
                }
            }
            PatKind::Assign(inner, _) | PatKind::Rest(inner) => {
                self.declare_pat_into(inner, scope, kind, lexical, enclosing_loop, ids)
            }
        }
    }

    fn resolve_name(&self, mut scope: ScopeId, name: &str) -> Option<BindingId> {
        loop {
            if let Some(id) = self.plan.scopes[scope].bindings.get(name) {
                return Some(*id);
            }
            // Normal functions and methods have their own implicit arguments
            // binding; arrows continue resolving in their enclosing scope.
            if name == "arguments" && self.plan.scopes[scope].owns_arguments {
                return None;
            }
            scope = self.plan.scopes[scope].parent?;
        }
    }

    fn record_reference(&mut self, name: &str, span: Span, mutated: bool) {
        let Some(id) = self.resolve_name(self.current_scope(), name) else {
            return;
        };
        self.plan.reference_bindings.insert(span.into(), id);
        let depth = self.current_function_depth();
        if depth > self.plan.bindings[id].function_depth {
            self.plan.bindings[id].captured = true;
            if let Some(loop_id) = self.plan.bindings[id]
                .enclosing_loop
                .filter(|loop_id| self.loop_header_stack.contains(loop_id))
            {
                self.preserve_native_loop(loop_id);
            }
        }
        let mutation_is_in_iteration_body = self.plan.bindings[id]
            .enclosing_loop
            .is_some_and(|loop_id| self.loop_stack.contains(&loop_id));
        if mutated && mutation_is_in_iteration_body {
            self.plan.bindings[id].directly_mutated = true;
        }
    }

    fn preserve_native_loop(&mut self, loop_id: LoopId) {
        self.plan.loops[loop_id].supported = false;
        self.plan.loops[loop_id].preserve_native = true;
        for binding in &mut self.plan.bindings {
            if binding.enclosing_loop == Some(loop_id) {
                binding.preserve_native = true;
            }
        }
    }

    fn collect_reserved_stmt_list(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.collect_reserved_stmt(stmt);
        }
    }

    fn reserve_pat(&mut self, pat: &Pat) {
        let mut names = Vec::new();
        collect_binding_names(pat, &mut names);
        self.reserved_names
            .extend(names.into_iter().map(AstString::from));
    }

    fn collect_reserved_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Var(v) => {
                for decl in &v.declarations {
                    self.reserve_pat(&decl.name);
                    if let Some(init) = &decl.init {
                        self.collect_reserved_expr(init);
                    }
                }
            }
            StmtKind::FnDecl(f) => {
                if let Some(name) = &f.name {
                    self.reserved_names.insert(name.as_str().into());
                }
                for p in &f.params {
                    self.reserve_pat(&p.name);
                    if let Some(init) = &p.initializer {
                        self.collect_reserved_expr(init);
                    }
                }
                if let Some(body) = &f.body {
                    self.collect_reserved_stmt_list(body);
                }
            }
            StmtKind::ClassDecl(c) => {
                if let Some(name) = &c.name {
                    self.reserved_names.insert(name.as_str().into());
                }
                if let Some(ext) = &c.extends {
                    self.collect_reserved_expr(ext);
                }
                self.collect_reserved_class(c);
            }
            StmtKind::EnumDecl(e) => {
                self.reserved_names.insert(e.name.as_str().into());
                for member in &e.members {
                    if let Some(init) = &member.initializer {
                        self.collect_reserved_expr(init);
                    }
                }
            }
            StmtKind::Block(s) => self.collect_reserved_stmt_list(s),
            StmtKind::If(s) => {
                self.collect_reserved_expr(&s.test);
                self.collect_reserved_stmt(&s.consequent);
                if let Some(alt) = &s.alternate {
                    self.collect_reserved_stmt(alt);
                }
            }
            StmtKind::While(s) => {
                self.collect_reserved_expr(&s.test);
                self.collect_reserved_stmt(&s.body);
            }
            StmtKind::DoWhile(s) => {
                self.collect_reserved_stmt(&s.body);
                self.collect_reserved_expr(&s.test);
            }
            StmtKind::For(s) => {
                if let Some(init) = &s.init {
                    match init {
                        ForInit::Var(v) => {
                            for decl in &v.declarations {
                                self.reserve_pat(&decl.name);
                                if let Some(init) = &decl.init {
                                    self.collect_reserved_expr(init);
                                }
                            }
                        }
                        ForInit::Expr(e) => self.collect_reserved_expr(e),
                    }
                }
                if let Some(test) = &s.test {
                    self.collect_reserved_expr(test);
                }
                if let Some(update) = &s.update {
                    self.collect_reserved_expr(update);
                }
                self.collect_reserved_stmt(&s.body);
            }
            StmtKind::ForIn(s) => {
                self.collect_reserved_left(&s.left);
                self.collect_reserved_expr(&s.right);
                self.collect_reserved_stmt(&s.body);
            }
            StmtKind::ForOf(s) => {
                self.collect_reserved_left(&s.left);
                self.collect_reserved_expr(&s.right);
                self.collect_reserved_stmt(&s.body);
            }
            StmtKind::Switch(s) => {
                self.collect_reserved_expr(&s.discriminant);
                for case in &s.cases {
                    if let Some(test) = &case.test {
                        self.collect_reserved_expr(test);
                    }
                    self.collect_reserved_stmt_list(&case.consequent);
                }
            }
            StmtKind::Try(s) => {
                self.collect_reserved_stmt_list(&s.block);
                if let Some(handler) = &s.handler {
                    if let Some(param) = &handler.param {
                        self.reserve_pat(param);
                    }
                    self.collect_reserved_stmt_list(&handler.body);
                }
                if let Some(fin) = &s.finalizer {
                    self.collect_reserved_stmt_list(fin);
                }
            }
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
                self.collect_reserved_expr(e)
            }
            StmtKind::Return(Some(e)) => self.collect_reserved_expr(e),
            StmtKind::ModuleDecl(m) => self.collect_reserved_module(m),
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => {
                    self.collect_reserved_stmt(s)
                }
                ExportDeclKind::Default(e) => self.collect_reserved_expr(e),
                _ => {}
            },
            StmtKind::Labeled(s) => self.collect_reserved_stmt(&s.body),
            StmtKind::With(s) => {
                self.collect_reserved_expr(&s.object);
                self.collect_reserved_stmt(&s.body);
            }
            _ => {}
        }
    }

    fn collect_reserved_left(&mut self, left: &ForInOfLeft) {
        match left {
            ForInOfLeft::Var(v) => {
                for decl in &v.declarations {
                    self.reserve_pat(&decl.name);
                }
            }
            ForInOfLeft::Pat(p) => self.reserve_pat(p),
            ForInOfLeft::Expr(e) => self.collect_reserved_expr(e),
        }
    }

    fn collect_reserved_module(&mut self, module: &ModuleDecl) {
        if let ModuleName::Ident(name) = &module.name {
            self.reserved_names.insert(name.as_str().into());
        }
        match &module.body {
            Some(ModuleBody::Block(stmts)) => self.collect_reserved_stmt_list(stmts),
            Some(ModuleBody::Module(inner)) => self.collect_reserved_module(inner),
            None => {}
        }
    }

    fn collect_reserved_class(&mut self, class: &ClassDecl) {
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Property(p) => {
                    if let Some(init) = &p.initializer {
                        self.collect_reserved_expr(init);
                    }
                }
                ClassMemberKind::Method(m) => {
                    for p in &m.params {
                        self.reserve_pat(&p.name);
                    }
                    if let Some(body) = &m.body {
                        self.collect_reserved_stmt_list(body);
                    }
                }
                ClassMemberKind::Constructor(c) => {
                    for p in &c.params {
                        self.reserve_pat(&p.name);
                    }
                    if let Some(body) = &c.body {
                        self.collect_reserved_stmt_list(body);
                    }
                }
                ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                    for p in &a.params {
                        self.reserve_pat(&p.name);
                    }
                    if let Some(body) = &a.body {
                        self.collect_reserved_stmt_list(body);
                    }
                }
                ClassMemberKind::StaticBlock(stmts) => self.collect_reserved_stmt_list(stmts),
                _ => {}
            }
        }
    }

    fn collect_reserved_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Ident(name) => {
                self.reserved_names.insert(name.clone());
            }
            ExprKind::FnExpr(f) => {
                if let Some(name) = &f.name {
                    self.reserved_names.insert(name.as_str().into());
                }
                for p in &f.params {
                    self.reserve_pat(&p.name);
                }
                if let Some(body) = &f.body {
                    self.collect_reserved_stmt_list(body);
                }
            }
            ExprKind::Arrow(a) => {
                for p in &a.params {
                    self.reserve_pat(&p.name);
                }
                match &a.body {
                    ArrowBody::Expr(e) => self.collect_reserved_expr(e),
                    ArrowBody::Block(s) => self.collect_reserved_stmt_list(s),
                }
            }
            ExprKind::ClassExpr(c) => self.collect_reserved_class(c),
            _ => self.collect_reserved_expr_children(expr),
        }
    }

    fn collect_hoisted_vars(&mut self, stmts: &[Stmt], var_scope: ScopeId) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Var(v) if v.kind == VarKind::Var => {
                    for decl in &v.declarations {
                        self.declare_pat(&decl.name, var_scope, BindingKind::Var, false, None);
                    }
                }
                StmtKind::Block(s) => self.collect_hoisted_vars(s, var_scope),
                StmtKind::If(s) => {
                    self.collect_hoisted_vars(std::slice::from_ref(&s.consequent), var_scope);
                    if let Some(alt) = &s.alternate {
                        self.collect_hoisted_vars(std::slice::from_ref(alt), var_scope);
                    }
                }
                StmtKind::While(s) => {
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope)
                }
                StmtKind::DoWhile(s) => {
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope)
                }
                StmtKind::For(s) => {
                    if let Some(ForInit::Var(v)) = &s.init {
                        if v.kind == VarKind::Var {
                            for decl in &v.declarations {
                                self.declare_pat(
                                    &decl.name,
                                    var_scope,
                                    BindingKind::Var,
                                    false,
                                    None,
                                );
                            }
                        }
                    }
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope);
                }
                StmtKind::ForIn(s) => {
                    self.collect_hoisted_left(&s.left, var_scope);
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope);
                }
                StmtKind::ForOf(s) => {
                    self.collect_hoisted_left(&s.left, var_scope);
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope);
                }
                StmtKind::Switch(s) => {
                    for case in &s.cases {
                        self.collect_hoisted_vars(&case.consequent, var_scope);
                    }
                }
                StmtKind::Try(s) => {
                    self.collect_hoisted_vars(&s.block, var_scope);
                    if let Some(handler) = &s.handler {
                        self.collect_hoisted_vars(&handler.body, var_scope);
                    }
                    if let Some(fin) = &s.finalizer {
                        self.collect_hoisted_vars(fin, var_scope);
                    }
                }
                StmtKind::Export(e) => match &e.kind {
                    ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => {
                        self.collect_hoisted_vars(std::slice::from_ref(s), var_scope)
                    }
                    _ => {}
                },
                StmtKind::Labeled(s) => {
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope)
                }
                StmtKind::With(s) => {
                    self.collect_hoisted_vars(std::slice::from_ref(&s.body), var_scope)
                }
                // Never hoist across a function, class, or namespace boundary.
                _ => {}
            }
        }
    }

    fn collect_hoisted_left(&mut self, left: &ForInOfLeft, var_scope: ScopeId) {
        if let ForInOfLeft::Var(v) = left {
            if v.kind == VarKind::Var {
                for decl in &v.declarations {
                    self.declare_pat(&decl.name, var_scope, BindingKind::Var, false, None);
                }
            }
        }
    }

    fn predeclare_stmt_list(&mut self, stmts: &[Stmt], scope: ScopeId) {
        let scope_kind = self.plan.scopes[scope].kind;
        for stmt in stmts {
            let stmt = match &stmt.kind {
                StmtKind::Export(e) => match &e.kind {
                    ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => s.as_ref(),
                    _ => continue,
                },
                _ => stmt,
            };
            match &stmt.kind {
                StmtKind::Var(v) if v.kind != VarKind::Var => {
                    for decl in &v.declarations {
                        self.declare_pat(
                            &decl.name,
                            scope,
                            BindingKind::Lexical,
                            true,
                            self.loop_stack.last().copied(),
                        );
                    }
                }
                StmtKind::FnDecl(f) => {
                    if let Some(name) = &f.name {
                        let decl_scope =
                            if matches!(scope_kind, ScopeKind::File | ScopeKind::Function) {
                                self.nearest_var_scope(scope)
                            } else {
                                scope
                            };
                        if !matches!(scope_kind, ScopeKind::File | ScopeKind::Function) {
                            // Block functions have Annex-B interactions that require
                            // binder flags; keep the legacy lowering for the file.
                            self.plan.supported = false;
                        }
                        self.declare(
                            decl_scope,
                            name,
                            f.name_span.unwrap_or(f.span),
                            BindingKind::Function,
                            false,
                            self.loop_stack.last().copied(),
                        );
                    }
                }
                StmtKind::ClassDecl(c) => {
                    if let Some(name) = &c.name {
                        self.declare(
                            scope,
                            name,
                            c.name_span.unwrap_or(c.span),
                            BindingKind::Class,
                            true,
                            self.loop_stack.last().copied(),
                        );
                    }
                }
                StmtKind::EnumDecl(e) if e.modifiers & MOD_DECLARE == 0 => {
                    let lexical = !matches!(scope_kind, ScopeKind::File | ScopeKind::Function);
                    self.declare(
                        scope,
                        &e.name,
                        e.name_span.unwrap_or(e.span),
                        BindingKind::Enum,
                        lexical,
                        self.loop_stack.last().copied(),
                    );
                    if lexical {
                        self.plan.scoped_enum_spans.insert(e.span.into());
                    }
                }
                StmtKind::ModuleDecl(module)
                    if !crate::analysis::module_decl_is_type_only_standalone(module) =>
                {
                    if let ModuleName::Ident(name) = &module.name {
                        self.declare(
                            scope,
                            name,
                            module.name_span.unwrap_or(module.span),
                            BindingKind::Module,
                            !matches!(scope_kind, ScopeKind::File | ScopeKind::Function),
                            self.loop_stack.last().copied(),
                        );
                    }
                }
                StmtKind::Import(import)
                    if matches!(scope_kind, ScopeKind::File) && !import.type_only =>
                {
                    match &import.specifiers {
                        ImportClause::Named {
                            default,
                            named,
                            namespace,
                        } => {
                            if let Some(name) = default {
                                self.declare(
                                    scope,
                                    name,
                                    import.span,
                                    BindingKind::Import,
                                    false,
                                    None,
                                );
                            }
                            if let Some(name) = namespace {
                                self.declare(
                                    scope,
                                    name,
                                    import.span,
                                    BindingKind::Import,
                                    false,
                                    None,
                                );
                            }
                            for spec in named.iter().filter(|spec| !spec.is_type) {
                                self.declare(
                                    scope,
                                    &spec.local,
                                    spec.span,
                                    BindingKind::Import,
                                    false,
                                    None,
                                );
                            }
                        }
                        ImportClause::Require(name) => {
                            self.declare(
                                scope,
                                name,
                                import.span,
                                BindingKind::Import,
                                false,
                                None,
                            );
                        }
                    }
                }
                StmtKind::ImportEquals(import)
                    if matches!(scope_kind, ScopeKind::File)
                        && !self
                            .erased_type_only_import_equals
                            .contains(&stmt.span.into()) =>
                {
                    self.declare(
                        scope,
                        &import.name,
                        stmt.span,
                        BindingKind::Import,
                        false,
                        None,
                    );
                }
                _ => {}
            }
        }
    }

    fn visit_stmt_list(&mut self, stmts: &[Stmt], scope: ScopeId) {
        self.predeclare_stmt_list(stmts, scope);
        for stmt in stmts {
            self.visit_stmt(stmt);
        }
    }

    fn with_child_scope<F>(&mut self, kind: ScopeKind, function_depth: usize, f: F)
    where
        F: FnOnce(&mut Self, ScopeId),
    {
        let scope = self.new_scope(Some(self.current_scope()), kind, function_depth);
        self.scope_stack.push(scope);
        f(self, scope);
        self.scope_stack.pop();
    }

    fn new_loop(&mut self, span: Span, kind: LoopKind) -> LoopId {
        if let Some(id) = self.plan.loop_by_span.get(&span.into()).copied() {
            return id;
        }
        let id = self.plan.loops.len();
        let ordinal = id + 1;
        let helper_name = self.unique_generated_family("_loop_", ordinal);
        let state_name = self.unique_generated_name(&format!("state_{ordinal}"));
        self.plan.loops.push(LoopPlan {
            id,
            span: span.into(),
            kind,
            ordinal,
            helper_name,
            state_name,
            header_bindings: Vec::new(),
            captured_bindings: Vec::new(),
            body_local_captures: Vec::new(),
            copy_out_bindings: Vec::new(),
            copy_out_names: HashMap::new(),
            controls: Vec::new(),
            captures_this: false,
            new_target_name: None,
            new_target_references: HashSet::new(),
            iterator_destructure: false,
            simple_iterator_array_comment_range: None,
            supported: !self
                .suspending_function_stack
                .last()
                .copied()
                .unwrap_or(false),
            preserve_native: self
                .suspending_function_stack
                .last()
                .copied()
                .unwrap_or(false),
        });
        self.plan.loop_by_span.insert(span.into(), id);
        self.loop_normal_depths.push(self.normal_function_depth);
        self.loop_function_depths
            .push(self.current_function_depth());
        id
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        let previous = self.suppress_array_spread;
        self.suppress_array_spread |= stmt_is_erased(stmt, self.preserve_const_enums);
        self.visit_stmt_inner(stmt);
        self.suppress_array_spread = previous;
    }

    fn visit_stmt_inner(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Var(v) => self.visit_var_stmt(v),
            StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::ExportAssign(e) => {
                self.visit_expr(e, false)
            }
            StmtKind::Return(expr) => {
                if let Some(owner) = self.loop_stack.last().copied() {
                    self.raw_controls.push(RawControl {
                        span: stmt.span.into(),
                        kind: LoopControlKind::Return,
                        label: None,
                        target_loop: None,
                        target_is_non_loop_label: false,
                        containing_loops: self.loop_stack.clone(),
                        function_depth: self.current_function_depth(),
                    });
                    let _ = owner;
                }
                if let Some(expr) = expr {
                    self.visit_expr(expr, false);
                }
            }
            StmtKind::Block(stmts) => {
                let depth = self.current_function_depth();
                self.with_child_scope(ScopeKind::Block, depth, |this, scope| {
                    this.visit_stmt_list(stmts, scope)
                });
            }
            StmtKind::If(s) => {
                self.visit_expr(&s.test, false);
                self.visit_stmt(&s.consequent);
                if let Some(alt) = &s.alternate {
                    self.visit_stmt(alt);
                }
            }
            StmtKind::While(s) => {
                self.visit_nonlexical_loop(stmt.span, &s.body, Some(&s.test), true)
            }
            StmtKind::DoWhile(s) => {
                self.visit_nonlexical_loop(stmt.span, &s.body, Some(&s.test), false)
            }
            StmtKind::For(s) => self.visit_for(stmt.span, s),
            StmtKind::ForIn(s) => self.visit_for_in(stmt.span, s),
            StmtKind::ForOf(s) => self.visit_for_of(stmt.span, s),
            StmtKind::Switch(s) => self.visit_switch(s),
            StmtKind::Try(s) => self.visit_try(s),
            StmtKind::Break(label) => self.record_control(stmt.span, LoopControlKind::Break, label),
            StmtKind::Continue(label) => {
                self.record_control(stmt.span, LoopControlKind::Continue, label)
            }
            StmtKind::FnDecl(f) => {
                self.visit_function(f, false);
            }
            StmtKind::ClassDecl(c) => self.visit_class(c),
            StmtKind::EnumDecl(e) => {
                for member in &e.members {
                    self.visit_prop_name(&member.name);
                    if let Some(init) = &member.initializer {
                        self.visit_expr(init, false);
                    }
                }
            }
            StmtKind::ModuleDecl(m) => self.visit_module(m),
            StmtKind::ImportEquals(i)
                if !self
                    .erased_type_only_import_equals
                    .contains(&stmt.span.into()) =>
            {
                self.visit_expr(&i.module_ref, false)
            }
            StmtKind::Export(e) => match &e.kind {
                ExportDeclKind::Decl(s) | ExportDeclKind::DefaultDecl(s) => self.visit_stmt(s),
                ExportDeclKind::Default(e) => self.visit_expr(e, false),
                ExportDeclKind::Named {
                    specifiers,
                    source: None,
                    type_only: false,
                } => {
                    for spec in specifiers {
                        if !spec.is_type {
                            self.record_reference(&spec.local, spec.span, false);
                        }
                    }
                }
                _ => {}
            },
            StmtKind::Labeled(l) => self.visit_labeled(l),
            StmtKind::With(w) => {
                self.plan.supported = false;
                self.visit_expr(&w.object, false);
                self.visit_stmt(&w.body);
            }
            _ => {}
        }
    }

    fn visit_var_stmt(&mut self, var: &VarStmt) {
        // Direct declarations were predeclared by the containing statement list;
        // loop-header declarations are predeclared by the loop visitor.
        for decl in &var.declarations {
            self.visit_pat_initializers(&decl.name);
            if let Some(init) = &decl.init {
                self.visit_expr(init, false);
            }
        }
    }

    fn visit_pat_initializers(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => {
                            self.visit_pat_initializers(p)
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(key, p) => {
                            self.visit_prop_name(key);
                            self.visit_pat_initializers(p);
                        }
                        ObjPatProp::ShorthandAssign(_, init, _) => self.visit_expr(init, false),
                        ObjPatProp::Rest(p) => self.visit_pat_initializers(p),
                        ObjPatProp::Shorthand(_, _) => {}
                    }
                }
            }
            PatKind::Assign(p, init) => {
                self.visit_pat_initializers(p);
                self.visit_expr(init, false);
            }
            PatKind::Rest(p) => self.visit_pat_initializers(p),
            PatKind::Ident(_) => {}
        }
    }

    fn visit_nonlexical_loop(
        &mut self,
        _span: Span,
        body: &Stmt,
        test: Option<&Expr>,
        test_first: bool,
    ) {
        self.break_stack.push(None);
        self.continue_stack.push(None);
        if test_first {
            if let Some(test) = test {
                self.visit_expr(test, false);
            }
            self.visit_stmt(body);
        } else {
            self.visit_stmt(body);
            if let Some(test) = test {
                self.visit_expr(test, false);
            }
        }
        self.continue_stack.pop();
        self.break_stack.pop();
    }

    fn visit_for(&mut self, span: Span, stmt: &ForStmt) {
        let loop_id = self.new_loop(span, LoopKind::Classic);
        let depth = self.current_function_depth();
        self.with_child_scope(ScopeKind::Loop, depth, |this, scope| {
            if let Some(ForInit::Var(var)) = &stmt.init {
                let lexical = var.kind != VarKind::Var;
                let target_scope = if lexical {
                    scope
                } else {
                    this.nearest_var_scope(scope)
                };
                for decl in &var.declarations {
                    if lexical && !lexical_pattern_supported(&decl.name) {
                        this.plan.loops[loop_id].supported = false;
                    }
                    let ids = this.declare_pat(
                        &decl.name,
                        target_scope,
                        if lexical {
                            BindingKind::Lexical
                        } else {
                            BindingKind::Var
                        },
                        lexical,
                        lexical.then_some(loop_id),
                    );
                    if this.plan.loops[loop_id].preserve_native {
                        for id in &ids {
                            this.plan.bindings[*id].preserve_native = true;
                        }
                    }
                    if lexical {
                        this.plan.loops[loop_id].header_bindings.extend(ids);
                    }
                }
            }
            this.loop_header_stack.push(loop_id);
            match &stmt.init {
                Some(ForInit::Var(v)) => this.visit_var_stmt(v),
                Some(ForInit::Expr(e)) => this.visit_expr(e, false),
                None => {}
            }
            if let Some(test) = &stmt.test {
                this.visit_expr(test, false);
            }
            if let Some(update) = &stmt.update {
                this.visit_expr(update, false);
            }
            this.loop_header_stack.pop();
            this.loop_stack.push(loop_id);
            this.break_stack.push(Some(loop_id));
            this.continue_stack.push(Some(loop_id));
            this.visit_stmt(&stmt.body);
            this.continue_stack.pop();
            this.break_stack.pop();
            this.loop_stack.pop();
        });
    }

    fn visit_for_in(&mut self, span: Span, stmt: &ForInStmt) {
        // The RHS of for-in/of is evaluated outside the iteration binding.
        self.visit_expr(&stmt.right, false);
        self.visit_for_in_of_body(span, LoopKind::ForIn, &stmt.left, None, &stmt.body);
    }

    fn visit_for_of(&mut self, span: Span, stmt: &ForOfStmt) {
        self.visit_expr(&stmt.right, false);
        self.visit_for_in_of_body(
            span,
            LoopKind::ForOf,
            &stmt.left,
            Some(stmt.right.span.start),
            &stmt.body,
        );
        if stmt.is_await {
            if let Some(loop_id) = self.plan.loop_by_span.get(&span.into()).copied() {
                self.plan.loops[loop_id].supported = false;
            }
        }
    }

    fn visit_for_in_of_body(
        &mut self,
        span: Span,
        kind: LoopKind,
        left: &ForInOfLeft,
        value_start: Option<u32>,
        body: &Stmt,
    ) {
        let loop_id = self.new_loop(span, kind);
        let depth = self.current_function_depth();
        self.with_child_scope(ScopeKind::Loop, depth, |this, scope| {
            match left {
                ForInOfLeft::Var(var) => {
                    let lexical = var.kind != VarKind::Var;
                    if matches!(var.kind, VarKind::Using | VarKind::AwaitUsing) {
                        this.plan.loops[loop_id].supported = false;
                    }
                    let target_scope = if lexical {
                        scope
                    } else {
                        this.nearest_var_scope(scope)
                    };
                    if this.down_level_iteration && kind == LoopKind::ForOf {
                        if let Some(declaration) =
                            crate::analysis::eligible_es5_for_of_declaration(left)
                        {
                            let pattern = &declaration.name;
                            if crate::analysis::simple_es5_for_of_array_binding(pattern) {
                                this.plan.loops[loop_id].simple_iterator_array_comment_range =
                                    Some(SpanKey {
                                        start: declaration.full_start,
                                        end: value_start.unwrap_or(pattern.span.end),
                                    });
                            }
                        }
                    }
                    for decl in &var.declarations {
                        if this.down_level_iteration && lexical_pattern_contains_array(&decl.name) {
                            this.plan.loops[loop_id].iterator_destructure = true;
                        }
                        if lexical && !lexical_pattern_supported(&decl.name) {
                            this.plan.loops[loop_id].supported = false;
                        }
                        let ids = this.declare_pat(
                            &decl.name,
                            target_scope,
                            if lexical {
                                BindingKind::Lexical
                            } else {
                                BindingKind::Var
                            },
                            lexical,
                            lexical.then_some(loop_id),
                        );
                        if this.plan.loops[loop_id].preserve_native {
                            for id in &ids {
                                this.plan.bindings[*id].preserve_native = true;
                            }
                        }
                        if lexical {
                            this.plan.loops[loop_id].header_bindings.extend(ids);
                        }
                        this.visit_pat_initializers(&decl.name);
                    }
                }
                ForInOfLeft::Pat(pat) => {
                    this.visit_assignment_pat(pat);
                }
                ForInOfLeft::Expr(expr) => {
                    this.visit_assignment_target_expr(expr);
                }
            }
            this.loop_stack.push(loop_id);
            this.break_stack.push(Some(loop_id));
            this.continue_stack.push(Some(loop_id));
            this.visit_stmt(body);
            this.continue_stack.pop();
            this.break_stack.pop();
            this.loop_stack.pop();
        });
    }

    fn visit_assignment_pat(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Ident(name) => self.record_reference(name, pat.span, true),
            PatKind::Array(elements) => {
                for element in elements.iter().flatten() {
                    match element {
                        ArrayPatElem::Pat(p) | ArrayPatElem::Rest(p) => {
                            self.visit_assignment_pat(p)
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(key, p) => {
                            self.visit_prop_name(key);
                            self.visit_assignment_pat(p);
                        }
                        ObjPatProp::Shorthand(name, span)
                        | ObjPatProp::ShorthandAssign(name, _, span) => {
                            self.record_reference(name, *span, true)
                        }
                        ObjPatProp::Rest(p) => self.visit_assignment_pat(p),
                    }
                }
            }
            PatKind::Assign(p, init) => {
                self.visit_assignment_pat(p);
                self.visit_expr(init, false);
            }
            PatKind::Rest(p) => self.visit_assignment_pat(p),
        }
    }

    fn visit_switch(&mut self, stmt: &SwitchStmt) {
        self.visit_expr(&stmt.discriminant, false);
        self.break_stack.push(None);
        let depth = self.current_function_depth();
        self.with_child_scope(ScopeKind::Block, depth, |this, scope| {
            let all: Vec<Stmt> = stmt
                .cases
                .iter()
                .flat_map(|case| case.consequent.iter().cloned())
                .collect();
            this.predeclare_stmt_list(&all, scope);
            for case in &stmt.cases {
                if let Some(test) = &case.test {
                    this.visit_expr(test, false);
                }
                for child in &case.consequent {
                    this.visit_stmt(child);
                }
            }
        });
        self.break_stack.pop();
    }

    fn visit_try(&mut self, stmt: &TryStmt) {
        let depth = self.current_function_depth();
        self.with_child_scope(ScopeKind::Block, depth, |this, scope| {
            this.visit_stmt_list(&stmt.block, scope)
        });
        if let Some(handler) = &stmt.handler {
            self.with_child_scope(ScopeKind::Catch, depth, |this, scope| {
                if let Some(param) = &handler.param {
                    this.declare_pat(param, scope, BindingKind::Catch, false, None);
                    this.visit_pat_initializers(param);
                }
                this.visit_stmt_list(&handler.body, scope);
            });
        }
        if let Some(finalizer) = &stmt.finalizer {
            self.with_child_scope(ScopeKind::Block, depth, |this, scope| {
                this.visit_stmt_list(finalizer, scope)
            });
        }
    }

    fn visit_labeled(&mut self, stmt: &LabeledStmt) {
        let target_loop = self.ensure_loop_for_labeled_body(&stmt.body);
        self.labels.push(LabelTarget {
            name: stmt.label.as_str().into(),
            loop_id: target_loop,
        });
        self.visit_stmt(&stmt.body);
        self.labels.pop();
    }

    fn ensure_loop_for_labeled_body(&mut self, stmt: &Stmt) -> Option<LoopId> {
        let mut body = stmt;
        while let StmtKind::Labeled(label) = &body.kind {
            body = &label.body;
        }
        match &body.kind {
            StmtKind::For(_) => Some(self.new_loop(body.span, LoopKind::Classic)),
            StmtKind::ForIn(_) => Some(self.new_loop(body.span, LoopKind::ForIn)),
            StmtKind::ForOf(_) => Some(self.new_loop(body.span, LoopKind::ForOf)),
            _ => None,
        }
    }

    fn record_control(&mut self, span: Span, kind: LoopControlKind, label: &Option<String>) {
        let label_target = label.as_ref().and_then(|label| {
            self.labels
                .iter()
                .rev()
                .find(|target| target.name == label.as_str())
        });
        let target_loop = if label.is_some() {
            label_target.and_then(|target| target.loop_id)
        } else if kind == LoopControlKind::Continue {
            self.continue_stack.last().copied().flatten()
        } else {
            self.break_stack.last().copied().flatten()
        };
        self.raw_controls.push(RawControl {
            span: span.into(),
            kind,
            label: label.as_deref().map(AstString::from),
            target_loop,
            target_is_non_loop_label: label_target.is_some_and(|target| target.loop_id.is_none()),
            containing_loops: self.loop_stack.clone(),
            function_depth: self.current_function_depth(),
        });
    }

    fn visit_function(&mut self, function: &FnDecl, expression_name: bool) {
        self.visit_function_with_invocation(function, expression_name, false);
    }

    fn visit_function_with_invocation(
        &mut self,
        function: &FnDecl,
        expression_name: bool,
        _immediately_invoked: bool,
    ) {
        for decorator in &function.decorators {
            self.visit_expr(decorator, false);
        }
        let depth = self.current_function_depth() + 1;
        self.normal_function_depth += 1;
        self.suspending_function_stack
            .push(function.is_async || function.is_generator);
        self.with_child_scope(ScopeKind::Function, depth, |this, scope| {
            this.plan.scopes[scope].owns_arguments = true;
            if expression_name {
                if let Some(name) = &function.name {
                    this.declare(
                        scope,
                        name,
                        function.name_span.unwrap_or(function.span),
                        BindingKind::Function,
                        false,
                        None,
                    );
                }
            }
            for param in &function.params {
                this.declare_pat(&param.name, scope, BindingKind::Parameter, false, None);
            }
            if let Some(body) = &function.body {
                this.collect_hoisted_vars(body, scope);
            }
            for param in &function.params {
                this.visit_pat_initializers(&param.name);
                if let Some(init) = &param.initializer {
                    this.visit_expr(init, false);
                }
                for decorator in &param.decorators {
                    this.visit_expr(decorator, false);
                }
            }
            if let Some(body) = &function.body {
                this.visit_stmt_list(body, scope);
            }
        });
        self.suspending_function_stack.pop();
        self.normal_function_depth -= 1;
    }

    fn visit_arrow(&mut self, arrow: &ArrowFn, _immediately_invoked: bool) {
        let depth = self.current_function_depth() + 1;
        self.suspending_function_stack.push(arrow.is_async);
        self.with_child_scope(ScopeKind::Function, depth, |this, scope| {
            for param in &arrow.params {
                this.declare_pat(&param.name, scope, BindingKind::Parameter, false, None);
            }
            if let ArrowBody::Block(body) = &arrow.body {
                this.collect_hoisted_vars(body, scope);
            }
            for param in &arrow.params {
                this.visit_pat_initializers(&param.name);
                if let Some(init) = &param.initializer {
                    this.visit_expr(init, false);
                }
            }
            match &arrow.body {
                ArrowBody::Expr(expr) => this.visit_expr(expr, false),
                ArrowBody::Block(body) => this.visit_stmt_list(body, scope),
            }
        });
        self.suspending_function_stack.pop();
    }

    fn visit_module(&mut self, module: &ModuleDecl) {
        // Namespaces establish their own emitted wrapper scope.  Treat this as
        // a function boundary for lexical-collision planning.
        match &module.body {
            Some(ModuleBody::Block(stmts)) => {
                let depth = self.current_function_depth() + 1;
                self.with_child_scope(ScopeKind::Function, depth, |this, scope| {
                    this.collect_hoisted_vars(stmts, scope);
                    this.visit_stmt_list(stmts, scope);
                });
            }
            Some(ModuleBody::Module(inner)) => self.visit_module(inner),
            None => {}
        }
    }

    fn visit_class(&mut self, class: &ClassDecl) {
        if let Some(ext) = &class.extends {
            self.visit_expr(ext, false);
        }
        for decorator in &class.decorators {
            self.visit_expr(decorator, false);
        }
        let depth = self.current_function_depth();
        self.with_child_scope(ScopeKind::Class, depth, |this, scope| {
            if let Some(name) = &class.name {
                this.declare(
                    scope,
                    name,
                    class.name_span.unwrap_or(class.span),
                    BindingKind::Class,
                    false,
                    None,
                );
            }
            for member in &class.members {
                this.visit_class_member(member);
            }
        });
    }

    fn visit_class_member(&mut self, member: &ClassMember) {
        let previous = self.suppress_array_spread;
        self.suppress_array_spread |= matches!(&member.kind, ClassMemberKind::Property(prop) if prop.modifiers & (MOD_DECLARE | MOD_ABSTRACT) != 0);
        self.visit_class_member_inner(member);
        self.suppress_array_spread = previous;
    }

    fn visit_class_member_inner(&mut self, member: &ClassMember) {
        match &member.kind {
            ClassMemberKind::Property(p) => {
                self.visit_prop_name(&p.name);
                for decorator in &p.decorators {
                    self.visit_expr(decorator, false);
                }
                if let Some(init) = &p.initializer {
                    self.visit_expr(init, false);
                }
            }
            ClassMemberKind::Method(m) => {
                self.visit_prop_name(&m.name);
                for decorator in &m.decorators {
                    self.visit_expr(decorator, false);
                }
                self.visit_method_body(&m.params, m.body.as_deref(), m.is_async || m.is_generator);
            }
            ClassMemberKind::Constructor(c) => {
                self.visit_method_body(&c.params, c.body.as_deref(), false);
            }
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                self.visit_prop_name(&a.name);
                for decorator in &a.decorators {
                    self.visit_expr(decorator, false);
                }
                self.visit_method_body(&a.params, a.body.as_deref(), false);
            }
            ClassMemberKind::StaticBlock(stmts) => {
                let depth = self.current_function_depth();
                self.with_child_scope(ScopeKind::Block, depth, |this, scope| {
                    this.visit_stmt_list(stmts, scope)
                });
            }
            _ => {}
        }
    }

    fn visit_method_body(&mut self, params: &[Param], body: Option<&[Stmt]>, suspending: bool) {
        let depth = self.current_function_depth() + 1;
        self.normal_function_depth += 1;
        self.suspending_function_stack.push(suspending);
        self.with_child_scope(ScopeKind::Function, depth, |this, scope| {
            this.plan.scopes[scope].owns_arguments = true;
            for param in params {
                this.declare_pat(&param.name, scope, BindingKind::Parameter, false, None);
            }
            if let Some(body) = body {
                this.collect_hoisted_vars(body, scope);
            }
            for param in params {
                this.visit_pat_initializers(&param.name);
                if let Some(init) = &param.initializer {
                    this.visit_expr(init, false);
                }
            }
            if let Some(body) = body {
                this.visit_stmt_list(body, scope);
            }
        });
        self.suspending_function_stack.pop();
        self.normal_function_depth -= 1;
    }

    fn visit_prop_name(&mut self, name: &PropName) {
        if let PropName::Computed(expr, _) = name {
            self.visit_expr(expr, false);
        }
    }

    fn visit_expr(&mut self, expr: &Expr, mutated: bool) {
        match &expr.kind {
            ExprKind::Ident(name) => {
                self.record_reference(name, expr.span, mutated);
            }
            ExprKind::This => {
                for loop_id in self.loop_stack.iter().copied().rev() {
                    if self.loop_normal_depths.get(loop_id).copied()
                        == Some(self.normal_function_depth)
                    {
                        self.plan.loops[loop_id].captures_this = true;
                        break;
                    }
                }
            }
            ExprKind::Super => {
                // Moving a loop body into the generated (normal) helper would
                // make lexical `super` syntactically invalid. Keep every
                // enclosing loop at this method/function depth native instead;
                // its lexical declarations then continue to provide the
                // per-iteration environment without relocating `super`.
                let loops = self
                    .loop_stack
                    .iter()
                    .copied()
                    .filter(|loop_id| {
                        self.loop_normal_depths.get(*loop_id).copied()
                            == Some(self.normal_function_depth)
                    })
                    .collect::<Vec<_>>();
                for loop_id in loops {
                    self.preserve_native_loop(loop_id);
                }
            }
            ExprKind::MetaProp(meta)
                if meta.meta.as_str() == "new" && meta.property.as_str() == "target" =>
            {
                let loops = self
                    .loop_stack
                    .iter()
                    .copied()
                    .filter(|loop_id| {
                        self.loop_normal_depths.get(*loop_id).copied()
                            == Some(self.normal_function_depth)
                    })
                    .collect::<Vec<_>>();
                for loop_id in loops {
                    self.plan.loops[loop_id]
                        .new_target_references
                        .insert(expr.span.into());
                    if self.plan.loops[loop_id].new_target_name.is_none() {
                        let name = self.unique_generated_name("_newTarget");
                        self.plan.loops[loop_id].new_target_name = Some(name);
                    }
                }
            }
            ExprKind::Assign(assign) => {
                self.visit_assignment_target_expr(&assign.left);
                self.visit_expr(&assign.right, false);
            }
            ExprKind::Update(update) => self.visit_expr(&update.argument, true),
            ExprKind::Arrow(arrow) => {
                self.visit_arrow(arrow, false);
            }
            ExprKind::FnExpr(function) => {
                self.visit_function(function, true);
            }
            ExprKind::ClassExpr(class) => self.visit_class(class),
            ExprKind::Call(call) => {
                if !self.suppress_array_spread && crate::call_spread::eligible(call) {
                    let needs = crate::array_spread::argument_helper_requirements(
                        &call.args,
                        self.down_level_iteration,
                    );
                    self.plan.invocation_spreads.insert(expr.span.into(), needs);
                }
                self.visit_expr_children(expr);
            }
            ExprKind::ObjectLit(props) => self.visit_object_literal(props),
            ExprKind::ArrayLit(elements) => {
                if !self.suppress_array_spread
                    && elements
                        .iter()
                        .flatten()
                        .any(|e| matches!(e.kind, ExprKind::Spread(_)))
                {
                    let needs = crate::array_spread::helper_requirements(
                        elements,
                        self.down_level_iteration,
                    );
                    self.plan.array_spreads.insert(expr.span.into(), needs);
                }
                self.visit_expr_children(expr);
            }
            _ => self.visit_expr_children(expr),
        }
    }

    fn visit_assignment_target_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Ident(name) => self.record_reference(name, expr.span, true),
            ExprKind::ArrayLit(elements) => {
                for element in elements.iter().flatten() {
                    self.visit_assignment_target_expr(element);
                }
            }
            ExprKind::ObjectLit(props) => {
                for prop in props {
                    match prop {
                        ObjLitProp::Property(prop) => {
                            self.visit_prop_name(&prop.key);
                            self.visit_assignment_target_expr(&prop.value);
                        }
                        ObjLitProp::Shorthand(name, span) => {
                            self.record_reference(name, *span, true)
                        }
                        ObjLitProp::ShorthandDefault(name, init, span) => {
                            self.record_reference(name, *span, true);
                            self.visit_expr(init, false);
                        }
                        ObjLitProp::Spread(value, _) => self.visit_assignment_target_expr(value),
                        ObjLitProp::Method(_) | ObjLitProp::Get(_) | ObjLitProp::Set(_) => {}
                    }
                }
            }
            ExprKind::Paren(inner) | ExprKind::NonNull(inner) | ExprKind::Spread(inner) => {
                self.visit_assignment_target_expr(inner)
            }
            ExprKind::As(value) => self.visit_assignment_target_expr(&value.expr),
            ExprKind::Satisfies(value) => self.visit_assignment_target_expr(&value.expr),
            ExprKind::TypeAssertion(value) => self.visit_assignment_target_expr(&value.expr),
            _ => self.visit_expr(expr, false),
        }
    }

    fn visit_object_literal(&mut self, props: &[ObjLitProp]) {
        for prop in props {
            match prop {
                ObjLitProp::Property(p) => {
                    self.visit_prop_name(&p.key);
                    self.visit_expr(&p.value, false);
                }
                ObjLitProp::Shorthand(name, span) | ObjLitProp::ShorthandDefault(name, _, span) => {
                    self.record_reference(name, *span, false);
                    if let ObjLitProp::ShorthandDefault(_, init, _) = prop {
                        self.visit_expr(init, false);
                    }
                }
                ObjLitProp::Spread(expr, _) => self.visit_expr(expr, false),
                ObjLitProp::Method(method) => {
                    self.visit_prop_name(&method.name);
                    self.visit_method_body(
                        &method.params,
                        Some(&method.body),
                        method.is_async || method.is_generator,
                    );
                }
                ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                    self.visit_prop_name(&accessor.name);
                    self.visit_method_body(&accessor.params, Some(&accessor.body), false);
                }
            }
        }
    }

    fn visit_expr_children(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Call(call) => {
                let mut callee = call.callee.as_ref();
                loop {
                    if let ExprKind::Paren(inner) = &callee.kind {
                        callee = inner;
                    } else if let Some(inner) = callee.kind.type_layer_inner() {
                        callee = inner;
                    } else {
                        break;
                    }
                }
                match &callee.kind {
                    ExprKind::Arrow(arrow) => {
                        self.visit_arrow(arrow, true);
                    }
                    ExprKind::FnExpr(function) => {
                        self.visit_function_with_invocation(function, true, true);
                    }
                    ExprKind::Member(member)
                        if matches!(member.property.as_str(), "call" | "apply") =>
                    {
                        let mut receiver = member.object.as_ref();
                        loop {
                            if let ExprKind::Paren(inner) = &receiver.kind {
                                receiver = inner;
                            } else if let Some(inner) = receiver.kind.type_layer_inner() {
                                receiver = inner;
                            } else {
                                break;
                            }
                        }
                        match &receiver.kind {
                            ExprKind::Arrow(arrow) => {
                                self.visit_arrow(arrow, true);
                            }
                            ExprKind::FnExpr(function) => {
                                self.visit_function_with_invocation(function, true, true);
                            }
                            _ => self.visit_expr(&call.callee, false),
                        }
                    }
                    _ => self.visit_expr(&call.callee, false),
                }
                for arg in &call.args {
                    self.visit_expr(arg, false);
                }
            }
            ExprKind::New(new_expr) => {
                if !self.suppress_array_spread && crate::new_spread::eligible(new_expr) {
                    let needs = crate::array_spread::constructor_helper_requirements(
                        new_expr.args.as_deref().unwrap_or_default(),
                        self.down_level_iteration,
                    );
                    self.plan.invocation_spreads.insert(expr.span.into(), needs);
                }
                self.visit_expr(&new_expr.callee, false);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        self.visit_expr(arg, false);
                    }
                }
            }
            ExprKind::Member(member) => self.visit_expr(&member.object, false),
            ExprKind::ElemAccess(access) => {
                self.visit_expr(&access.object, false);
                self.visit_expr(&access.index, false);
            }
            ExprKind::Binary(binary) => {
                self.visit_expr(&binary.left, false);
                self.visit_expr(&binary.right, false);
            }
            ExprKind::Unary(unary) => self.visit_expr(&unary.argument, false),
            ExprKind::Paren(inner)
            | ExprKind::Spread(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.visit_expr(inner, false),
            ExprKind::Cond(cond) => {
                self.visit_expr(&cond.test, false);
                self.visit_expr(&cond.consequent, false);
                self.visit_expr(&cond.alternate, false);
            }
            ExprKind::ArrayLit(elements) => {
                for element in elements.iter().flatten() {
                    self.visit_expr(element, false);
                }
            }
            ExprKind::Template(template) => {
                for inner in &template.exprs {
                    self.visit_expr(inner, false);
                }
            }
            ExprKind::TaggedTemplate(tagged) => {
                self.visit_expr(&tagged.tag, false);
                for inner in &tagged.quasi.exprs {
                    self.visit_expr(inner, false);
                }
            }
            ExprKind::As(value) => self.visit_expr(&value.expr, false),
            ExprKind::Satisfies(value) => self.visit_expr(&value.expr, false),
            ExprKind::TypeAssertion(value) => self.visit_expr(&value.expr, false),
            ExprKind::Instantiation(value) => self.visit_expr(&value.expr, false),
            ExprKind::Comma(items) => {
                for item in items {
                    self.visit_expr(item, false);
                }
            }
            ExprKind::Yield(_, Some(value)) => self.visit_expr(value, false),
            ExprKind::JsxElement(element) => {
                self.visit_expr(&element.name, false);
                self.visit_jsx_attributes(&element.attributes);
                self.visit_jsx_children(&element.children);
            }
            ExprKind::JsxSelfClosing(element) => {
                self.visit_expr(&element.name, false);
                self.visit_jsx_attributes(&element.attributes);
            }
            ExprKind::JsxFragment(fragment) => self.visit_jsx_children(&fragment.children),
            _ => {}
        }
    }

    fn visit_jsx_attributes(&mut self, attributes: &[JsxAttribute]) {
        for attribute in attributes {
            match attribute {
                JsxAttribute::Normal {
                    value: Some(value), ..
                } => self.visit_expr(value, false),
                JsxAttribute::Spread(value, _) => self.visit_expr(value, false),
                _ => {}
            }
        }
    }

    fn visit_jsx_children(&mut self, children: &[JsxChild]) {
        for child in children {
            match child {
                JsxChild::Element(element) | JsxChild::Expression(Some(element), _) => {
                    self.visit_expr(element, false)
                }
                JsxChild::Fragment(fragment) => self.visit_jsx_children(&fragment.children),
                _ => {}
            }
        }
    }

    fn collect_reserved_expr_children(&mut self, _expr: &Expr) {
        // `analyze` reserves every identifier token before building scopes.
    }

    fn finish_loops(&mut self) {
        for binding in &self.plan.bindings {
            if !binding.captured {
                continue;
            }
            let Some(loop_id) = binding.enclosing_loop else {
                continue;
            };
            if self.plan.loops[loop_id]
                .header_bindings
                .contains(&binding.id)
            {
                self.plan.loops[loop_id].captured_bindings.push(binding.id);
            } else {
                self.plan.loops[loop_id]
                    .body_local_captures
                    .push(binding.id);
            }
        }

        for plan in &mut self.plan.loops {
            let needs_helper = plan.supported
                && (!plan.captured_bindings.is_empty() || !plan.body_local_captures.is_empty());
            if !needs_helper {
                continue;
            }
            // Once the body is moved, all lexical header bindings are helper
            // parameters, even if only a body-local binding triggered capture.
            plan.captured_bindings = plan.header_bindings.clone();
            plan.copy_out_bindings = plan
                .header_bindings
                .iter()
                .copied()
                .filter(|id| self.plan.bindings[*id].directly_mutated)
                .collect();
        }

        for loop_id in 0..self.plan.loops.len() {
            let ordinal = self.plan.loops[loop_id].ordinal;
            let bindings = self.plan.loops[loop_id].copy_out_bindings.clone();
            for binding in bindings {
                let emitted = self.plan.bindings[binding].emitted_name.clone();
                let name = self.unique_generated_name(&format!("out_{emitted}_{ordinal}"));
                self.plan.loops[loop_id]
                    .copy_out_names
                    .insert(binding, name);
            }
        }

        for raw in self.raw_controls.clone() {
            let target_pos = raw
                .target_loop
                .and_then(|target| raw.containing_loops.iter().position(|id| *id == target));
            let mut owner = None;
            for (position, loop_id) in raw.containing_loops.iter().copied().enumerate().rev() {
                let plan = &self.plan.loops[loop_id];
                let needs_helper = plan.supported
                    && (!plan.captured_bindings.is_empty() || !plan.body_local_captures.is_empty());
                if !needs_helper
                    || self.loop_function_depths.get(loop_id).copied() != Some(raw.function_depth)
                {
                    continue;
                }
                let escapes = raw.kind == LoopControlKind::Return
                    || raw.target_is_non_loop_label
                    || target_pos.is_some_and(|target| target <= position);
                if escapes {
                    owner = Some(loop_id);
                    break;
                }
            }
            let Some(owner_loop) = owner else {
                continue;
            };
            let control = LoopControl {
                span: raw.span,
                kind: raw.kind,
                label: raw.label,
                target_loop: raw.target_loop,
                owner_loop,
            };
            self.plan.control_by_span.insert(raw.span, control.clone());
            let owner_pos = raw
                .containing_loops
                .iter()
                .position(|id| *id == owner_loop)
                .unwrap_or(0);
            for loop_id in raw.containing_loops.iter().copied().take(owner_pos + 1) {
                let plan = &mut self.plan.loops[loop_id];
                if plan.supported
                    && (!plan.captured_bindings.is_empty() || !plan.body_local_captures.is_empty())
                    && !plan.controls.iter().any(|existing| {
                        existing.kind == control.kind
                            && existing.label == control.label
                            && existing.target_loop == control.target_loop
                    })
                {
                    plan.controls.push(control.clone());
                }
            }
        }
    }
}

fn lexical_pattern_supported(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Ident(_) => true,
        PatKind::Array(elements) => elements.iter().flatten().all(|element| match element {
            ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => lexical_pattern_supported(pat),
        }),
        PatKind::Object(props) => props.iter().all(|prop| match prop {
            ObjPatProp::KeyValue(PropName::Computed(_, _) | PropName::Private(_, _), _) => false,
            ObjPatProp::KeyValue(_, pat) | ObjPatProp::Rest(pat) => lexical_pattern_supported(pat),
            ObjPatProp::Shorthand(_, _) | ObjPatProp::ShorthandAssign(_, _, _) => true,
        }),
        PatKind::Assign(pat, _) | PatKind::Rest(pat) => lexical_pattern_supported(pat),
    }
}

fn lexical_pattern_contains_array(pat: &Pat) -> bool {
    match &pat.kind {
        PatKind::Array(_) => true,
        PatKind::Object(props) => props.iter().any(|prop| match prop {
            ObjPatProp::KeyValue(_, pat) | ObjPatProp::Rest(pat) => {
                lexical_pattern_contains_array(pat)
            }
            ObjPatProp::Shorthand(_, _) | ObjPatProp::ShorthandAssign(_, _, _) => false,
        }),
        PatKind::Assign(pat, _) | PatKind::Rest(pat) => lexical_pattern_contains_array(pat),
        PatKind::Ident(_) => false,
    }
}

fn is_erased_type_only_import_equals(stmt: &Stmt, source: &str) -> bool {
    match &stmt.kind {
        StmtKind::ImportEquals(_) => {
            let start = stmt.span.start as usize;
            let end = stmt.span.end as usize;
            let Some(text) = source.get(start..end) else {
                return false;
            };
            let tokens = Scanner::new(text).scan_all();
            matches!(tokens.first(), Some(token) if token.kind == TokenKind::Import)
                && matches!(tokens.get(1), Some(token) if token.kind == TokenKind::Type)
        }
        _ => false,
    }
}
