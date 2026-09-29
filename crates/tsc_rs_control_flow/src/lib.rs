//! Control-flow graph primitives.
//!
//! The CFG is built once during binding and reused for narrowing, reachability,
//! and definite-assignment analysis during type checking. Building it once
//! avoids the cost of reconstructing it on every check.

use std::collections::{HashSet, VecDeque};
use tsc_rs_ast::{Stmt, StmtKind};

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CfgNodeId(pub u32);

// ---------------------------------------------------------------------------
// Edge & node kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Sequential,
    CondTrue,
    CondFalse,
    Loop,
    Break,
    Continue,
    Throw,
    Return,
}

#[derive(Debug, Clone)]
pub enum CfgNodeKind {
    Entry,
    Exit,
    Statement,
    Condition,
    Merge,
    Loop,
}

#[derive(Debug, Clone)]
pub struct CfgNode {
    pub id: CfgNodeId,
    pub kind: CfgNodeKind,
}

// ---------------------------------------------------------------------------
// Control-flow graph
// ---------------------------------------------------------------------------

pub struct ControlFlowGraph {
    nodes: Vec<CfgNode>,
    edges: Vec<(CfgNodeId, CfgNodeId, EdgeKind)>,
    next_id: u32,
}

impl Default for ControlFlowGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlFlowGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            next_id: 0,
        }
    }

    pub fn add_node(&mut self, kind: CfgNodeKind) -> CfgNodeId {
        let id = CfgNodeId(self.next_id);
        self.next_id += 1;
        self.nodes.push(CfgNode { id, kind });
        id
    }

    pub fn add_edge(&mut self, from: CfgNodeId, to: CfgNodeId, kind: EdgeKind) {
        self.edges.push((from, to, kind));
    }

    pub fn successors(&self, id: CfgNodeId) -> Vec<(CfgNodeId, EdgeKind)> {
        self.edges
            .iter()
            .filter(|(from, _, _)| *from == id)
            .map(|(_, to, kind)| (*to, *kind))
            .collect()
    }

    pub fn predecessors(&self, id: CfgNodeId) -> Vec<(CfgNodeId, EdgeKind)> {
        self.edges
            .iter()
            .filter(|(_, to, _)| *to == id)
            .map(|(from, _, kind)| (*from, *kind))
            .collect()
    }

    /// BFS reachability from a given node.
    pub fn reachable_from(&self, id: CfgNodeId) -> Vec<CfgNodeId> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(id);
        visited.insert(id);
        while let Some(current) = queue.pop_front() {
            for (succ, _) in self.successors(current) {
                if visited.insert(succ) {
                    queue.push_back(succ);
                }
            }
        }
        visited.into_iter().collect()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ControlContext {
    label: Option<String>,
    break_target: CfgNodeId,
    continue_target: Option<CfgNodeId>,
}

pub struct CfgBuilder {
    graph: ControlFlowGraph,
    exit_node: Option<CfgNodeId>,
    control_stack: Vec<ControlContext>,
}

impl Default for CfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl CfgBuilder {
    pub fn new() -> Self {
        Self {
            graph: ControlFlowGraph::new(),
            exit_node: None,
            control_stack: Vec::new(),
        }
    }

    pub fn entry(&mut self) -> CfgNodeId {
        self.graph.add_node(CfgNodeKind::Entry)
    }

    pub fn exit(&mut self) -> CfgNodeId {
        let id = self.graph.add_node(CfgNodeKind::Exit);
        self.exit_node = Some(id);
        id
    }

    pub fn add_node(&mut self, kind: CfgNodeKind) -> CfgNodeId {
        self.graph.add_node(kind)
    }

    pub fn add_edge(&mut self, from: CfgNodeId, to: CfgNodeId, kind: EdgeKind) {
        self.graph.add_edge(from, to, kind);
    }

    /// Build a complete CFG from a list of statements.
    ///
    /// Creates entry and exit nodes, walks each statement building the
    /// appropriate control-flow structure, and returns the finished graph.
    pub fn build_from_stmts(stmts: &[Stmt]) -> ControlFlowGraph {
        let mut builder = CfgBuilder::new();
        let entry = builder.entry();
        let exit = builder.exit();

        let current = builder.walk_stmts(stmts, entry, exit);
        // If the last statement didn't terminate, connect to exit.
        if let Some(last) = current {
            builder.graph.add_edge(last, exit, EdgeKind::Sequential);
        }

        builder.graph
    }

    /// Walk a slice of statements, chaining them sequentially.
    /// Returns `Some(last_node)` if control falls through, or `None` if
    /// all paths terminated (return/throw).
    fn walk_stmts(
        &mut self,
        stmts: &[Stmt],
        mut current: CfgNodeId,
        exit: CfgNodeId,
    ) -> Option<CfgNodeId> {
        for stmt in stmts {
            match self.walk_stmt(stmt, current, exit) {
                Some(next) => current = next,
                None => return None,
            }
        }
        Some(current)
    }

    fn push_context(
        &mut self,
        label: Option<&str>,
        break_target: CfgNodeId,
        continue_target: Option<CfgNodeId>,
    ) {
        self.control_stack.push(ControlContext {
            label: label.map(ToOwned::to_owned),
            break_target,
            continue_target,
        });
    }

    fn pop_context(&mut self) {
        self.control_stack.pop();
    }

    fn find_break_target(&self, label: Option<&str>) -> Option<CfgNodeId> {
        self.control_stack
            .iter()
            .rev()
            .find(|ctx| match label {
                Some(label) => ctx.label.as_deref() == Some(label),
                None => true,
            })
            .map(|ctx| ctx.break_target)
    }

    fn find_continue_target(&self, label: Option<&str>) -> Option<CfgNodeId> {
        self.control_stack
            .iter()
            .rev()
            .find(|ctx| {
                ctx.continue_target.is_some()
                    && match label {
                        Some(label) => ctx.label.as_deref() == Some(label),
                        None => true,
                    }
            })
            .and_then(|ctx| ctx.continue_target)
    }

    /// Walk a single statement. Returns `Some(node)` where control continues,
    /// or `None` if the statement terminates control flow.
    fn walk_stmt(&mut self, stmt: &Stmt, current: CfgNodeId, exit: CfgNodeId) -> Option<CfgNodeId> {
        self.walk_stmt_with_label(stmt, current, exit, None)
    }

    fn walk_stmt_with_label(
        &mut self,
        stmt: &Stmt,
        current: CfgNodeId,
        exit: CfgNodeId,
        pending_label: Option<&str>,
    ) -> Option<CfgNodeId> {
        if let Some(label) = pending_label {
            match &stmt.kind {
                StmtKind::While(_)
                | StmtKind::DoWhile(_)
                | StmtKind::For(_)
                | StmtKind::ForIn(_)
                | StmtKind::ForOf(_)
                | StmtKind::Switch(_) => {}
                _ => return self.walk_labeled_stmt(stmt, current, exit, label),
            }
        }

        match &stmt.kind {
            // -- Simple statements: create a node, chain sequentially --------
            StmtKind::Var(_)
            | StmtKind::Expr(_)
            | StmtKind::Empty
            | StmtKind::Debugger
            | StmtKind::Import(_)
            | StmtKind::ImportEquals(..)
            | StmtKind::Export(_)
            | StmtKind::ExportAssign(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::InterfaceDecl(_)
            | StmtKind::EnumDecl(_)
            | StmtKind::ModuleDecl(_)
            | StmtKind::ClassDecl(_)
            | StmtKind::FnDecl(_) => {
                let node = self.graph.add_node(CfgNodeKind::Statement);
                self.graph.add_edge(current, node, EdgeKind::Sequential);
                Some(node)
            }

            // -- Block: walk contents ----------------------------------------
            StmtKind::Block(stmts) => self.walk_stmts(stmts, current, exit),

            // -- Labeled: walk body ------------------------------------------
            StmtKind::Labeled(labeled) => {
                self.walk_stmt_with_label(&labeled.body, current, exit, Some(&labeled.label))
            }

            // -- With: walk body ---------------------------------------------
            StmtKind::With(with_stmt) => self.walk_stmt(&with_stmt.body, current, exit),

            // -- Return: edge to exit, terminates ----------------------------
            StmtKind::Return(_) => {
                let node = self.graph.add_node(CfgNodeKind::Statement);
                self.graph.add_edge(current, node, EdgeKind::Sequential);
                self.graph.add_edge(node, exit, EdgeKind::Return);
                None
            }

            // -- Throw: edge to exit, terminates -----------------------------
            StmtKind::Throw(_) => {
                let node = self.graph.add_node(CfgNodeKind::Statement);
                self.graph.add_edge(current, node, EdgeKind::Sequential);
                self.graph.add_edge(node, exit, EdgeKind::Throw);
                None
            }

            // -- Break/Continue: jump to the enclosing target ----------------
            StmtKind::Break(label) => {
                let node = self.graph.add_node(CfgNodeKind::Statement);
                self.graph.add_edge(current, node, EdgeKind::Sequential);
                if let Some(target) = self.find_break_target(label.as_deref()) {
                    self.graph.add_edge(node, target, EdgeKind::Break);
                }
                None
            }

            StmtKind::Continue(label) => {
                let node = self.graph.add_node(CfgNodeKind::Statement);
                self.graph.add_edge(current, node, EdgeKind::Sequential);
                if let Some(target) = self.find_continue_target(label.as_deref()) {
                    self.graph.add_edge(node, target, EdgeKind::Continue);
                }
                None
            }

            // -- If/else: branch on condition --------------------------------
            StmtKind::If(if_stmt) => {
                let cond = self.graph.add_node(CfgNodeKind::Condition);
                self.graph.add_edge(current, cond, EdgeKind::Sequential);

                let merge = self.graph.add_node(CfgNodeKind::Merge);

                // True branch
                let true_end = self.walk_stmt(&if_stmt.consequent, cond, exit);
                // Connect cond -> first node of true branch via CondTrue.
                // The walk_stmt already created the edge from cond via Sequential,
                // so patch: replace the last Sequential from cond with CondTrue.
                self.patch_last_edge_from(cond, EdgeKind::CondTrue);
                if let Some(t) = true_end {
                    self.graph.add_edge(t, merge, EdgeKind::Sequential);
                }

                // False branch
                if let Some(alt) = &if_stmt.alternate {
                    let false_end = self.walk_stmt(alt, cond, exit);
                    self.patch_last_edge_from(cond, EdgeKind::CondFalse);
                    if let Some(f) = false_end {
                        self.graph.add_edge(f, merge, EdgeKind::Sequential);
                    }
                    // If both branches terminate, this merge is unreachable
                    if true_end.is_none() && false_end.is_none() {
                        return None;
                    }
                } else {
                    // No else: cond falls through on false
                    self.graph.add_edge(cond, merge, EdgeKind::CondFalse);
                }

                Some(merge)
            }

            // -- While loop --------------------------------------------------
            StmtKind::While(while_stmt) => {
                let loop_node = self.graph.add_node(CfgNodeKind::Loop);
                self.graph
                    .add_edge(current, loop_node, EdgeKind::Sequential);

                let cond = self.graph.add_node(CfgNodeKind::Condition);
                self.graph.add_edge(loop_node, cond, EdgeKind::Sequential);

                let merge = self.graph.add_node(CfgNodeKind::Merge);
                self.push_context(pending_label, merge, Some(cond));

                // Body
                let body_end = self.walk_stmt(&while_stmt.body, cond, exit);
                self.pop_context();
                self.patch_last_edge_from(cond, EdgeKind::CondTrue);
                if let Some(b) = body_end {
                    self.graph.add_edge(b, loop_node, EdgeKind::Loop);
                }

                // Exit loop on false
                self.graph.add_edge(cond, merge, EdgeKind::CondFalse);
                Some(merge)
            }

            // -- Do-while loop -----------------------------------------------
            StmtKind::DoWhile(do_while) => {
                let loop_node = self.graph.add_node(CfgNodeKind::Loop);
                self.graph
                    .add_edge(current, loop_node, EdgeKind::Sequential);

                let cond = self.graph.add_node(CfgNodeKind::Condition);
                let merge = self.graph.add_node(CfgNodeKind::Merge);
                self.push_context(pending_label, merge, Some(cond));

                // Body executes first
                let body_end = self.walk_stmt(&do_while.body, loop_node, exit);
                self.pop_context();

                if let Some(b) = body_end {
                    self.graph.add_edge(b, cond, EdgeKind::Sequential);
                }
                self.graph.add_edge(cond, loop_node, EdgeKind::CondTrue);
                self.graph.add_edge(cond, merge, EdgeKind::CondFalse);

                Some(merge)
            }

            // -- For loop ----------------------------------------------------
            StmtKind::For(for_stmt) => {
                // Init (treated as a statement node)
                let init_node = self.graph.add_node(CfgNodeKind::Statement);
                self.graph
                    .add_edge(current, init_node, EdgeKind::Sequential);

                let loop_node = self.graph.add_node(CfgNodeKind::Loop);
                self.graph
                    .add_edge(init_node, loop_node, EdgeKind::Sequential);

                let cond = self.graph.add_node(CfgNodeKind::Condition);
                self.graph.add_edge(loop_node, cond, EdgeKind::Sequential);

                let merge = self.graph.add_node(CfgNodeKind::Merge);
                let update = for_stmt
                    .update
                    .as_ref()
                    .map(|_| self.graph.add_node(CfgNodeKind::Statement));
                let continue_target = update.unwrap_or(cond);
                self.push_context(pending_label, merge, Some(continue_target));

                // Body
                let body_end = self.walk_stmt(&for_stmt.body, cond, exit);
                self.pop_context();
                self.patch_last_edge_from(cond, EdgeKind::CondTrue);

                if let Some(b) = body_end {
                    if let Some(update) = update {
                        self.graph.add_edge(b, update, EdgeKind::Sequential);
                        self.graph.add_edge(update, loop_node, EdgeKind::Loop);
                    } else {
                        self.graph.add_edge(b, loop_node, EdgeKind::Loop);
                    }
                }

                self.graph.add_edge(cond, merge, EdgeKind::CondFalse);
                Some(merge)
            }

            // -- For-in / for-of (similar structure) -------------------------
            StmtKind::ForIn(for_in) => {
                let loop_node = self.graph.add_node(CfgNodeKind::Loop);
                self.graph
                    .add_edge(current, loop_node, EdgeKind::Sequential);

                let cond = self.graph.add_node(CfgNodeKind::Condition);
                self.graph.add_edge(loop_node, cond, EdgeKind::Sequential);

                let merge = self.graph.add_node(CfgNodeKind::Merge);
                self.push_context(pending_label, merge, Some(cond));

                let body_end = self.walk_stmt(&for_in.body, cond, exit);
                self.pop_context();
                self.patch_last_edge_from(cond, EdgeKind::CondTrue);
                if let Some(b) = body_end {
                    self.graph.add_edge(b, loop_node, EdgeKind::Loop);
                }

                self.graph.add_edge(cond, merge, EdgeKind::CondFalse);
                Some(merge)
            }

            StmtKind::ForOf(for_of) => {
                let loop_node = self.graph.add_node(CfgNodeKind::Loop);
                self.graph
                    .add_edge(current, loop_node, EdgeKind::Sequential);

                let cond = self.graph.add_node(CfgNodeKind::Condition);
                self.graph.add_edge(loop_node, cond, EdgeKind::Sequential);

                let merge = self.graph.add_node(CfgNodeKind::Merge);
                self.push_context(pending_label, merge, Some(cond));

                let body_end = self.walk_stmt(&for_of.body, cond, exit);
                self.pop_context();
                self.patch_last_edge_from(cond, EdgeKind::CondTrue);
                if let Some(b) = body_end {
                    self.graph.add_edge(b, loop_node, EdgeKind::Loop);
                }

                self.graph.add_edge(cond, merge, EdgeKind::CondFalse);
                Some(merge)
            }

            // -- Switch ------------------------------------------------------
            StmtKind::Switch(switch_stmt) => {
                let cond = self.graph.add_node(CfgNodeKind::Condition);
                self.graph.add_edge(current, cond, EdgeKind::Sequential);

                let merge = self.graph.add_node(CfgNodeKind::Merge);
                let mut any_falls_through = false;
                self.push_context(pending_label, merge, None);

                for case in &switch_stmt.cases {
                    let case_end = self.walk_stmts(&case.consequent, cond, exit);
                    if let Some(c) = case_end {
                        self.graph.add_edge(c, merge, EdgeKind::Sequential);
                        any_falls_through = true;
                    }
                }

                // If no default case, control can skip all cases
                let has_default = switch_stmt.cases.iter().any(|c| c.test.is_none());
                if !has_default {
                    self.graph.add_edge(cond, merge, EdgeKind::CondFalse);
                    any_falls_through = true;
                }
                self.pop_context();

                if any_falls_through {
                    Some(merge)
                } else {
                    None
                }
            }

            // -- Try/catch/finally -------------------------------------------
            StmtKind::Try(try_stmt) => {
                let merge = self.graph.add_node(CfgNodeKind::Merge);

                // Try block
                let try_end = self.walk_stmts(&try_stmt.block, current, exit);
                if let Some(t) = try_end {
                    self.graph.add_edge(t, merge, EdgeKind::Sequential);
                }

                // Catch block
                if let Some(handler) = &try_stmt.handler {
                    let catch_entry = self.graph.add_node(CfgNodeKind::Statement);
                    self.graph.add_edge(current, catch_entry, EdgeKind::Throw);
                    let catch_end = self.walk_stmts(&handler.body, catch_entry, exit);
                    if let Some(c) = catch_end {
                        self.graph.add_edge(c, merge, EdgeKind::Sequential);
                    }
                }

                // Finally block
                if let Some(finalizer) = &try_stmt.finalizer {
                    let finally_entry = self.graph.add_node(CfgNodeKind::Statement);
                    self.graph
                        .add_edge(merge, finally_entry, EdgeKind::Sequential);
                    let finally_end = self.walk_stmts(finalizer, finally_entry, exit);
                    if let Some(f) = finally_end {
                        return Some(f);
                    }
                    return None;
                }

                Some(merge)
            }
        }
    }

    fn walk_labeled_stmt(
        &mut self,
        stmt: &Stmt,
        current: CfgNodeId,
        exit: CfgNodeId,
        label: &str,
    ) -> Option<CfgNodeId> {
        let merge = self.graph.add_node(CfgNodeKind::Merge);
        self.push_context(Some(label), merge, None);
        let body_end = self.walk_stmt_with_label(stmt, current, exit, None);
        self.pop_context();
        if let Some(end) = body_end {
            self.graph.add_edge(end, merge, EdgeKind::Sequential);
        }
        if self.graph.predecessors(merge).is_empty() {
            None
        } else {
            Some(merge)
        }
    }

    /// Patch the last edge originating from `from_node` to use the given kind.
    fn patch_last_edge_from(&mut self, from_node: CfgNodeId, kind: EdgeKind) {
        for (from, _, edge_kind) in self.graph.edges.iter_mut().rev() {
            if *from == from_node {
                *edge_kind = kind;
                return;
            }
        }
    }

    pub fn finish(self) -> ControlFlowGraph {
        self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_rs_ast::*;

    fn empty_span() -> Span {
        Span::new(0, 0)
    }

    fn expr_stmt(s: &str) -> Stmt {
        Stmt {
            kind: StmtKind::Expr(Box::new(Expr {
                kind: ExprKind::Ident(s.to_string().into()),
                span: empty_span(),
            })),
            span: empty_span(),
        }
    }

    fn return_stmt() -> Stmt {
        Stmt {
            kind: StmtKind::Return(None),
            span: empty_span(),
        }
    }

    fn throw_stmt() -> Stmt {
        Stmt {
            kind: StmtKind::Throw(Box::new(Expr {
                kind: ExprKind::Ident("err".to_string().into()),
                span: empty_span(),
            })),
            span: empty_span(),
        }
    }

    fn break_stmt() -> Stmt {
        Stmt {
            kind: StmtKind::Break(None),
            span: empty_span(),
        }
    }

    fn continue_stmt() -> Stmt {
        Stmt {
            kind: StmtKind::Continue(None),
            span: empty_span(),
        }
    }

    fn if_stmt(then_body: Stmt, else_body: Option<Stmt>) -> Stmt {
        Stmt {
            kind: StmtKind::If(IfStmt {
                test: Box::new(Expr {
                    kind: ExprKind::Ident("cond".to_string().into()),
                    span: empty_span(),
                }),
                consequent: Box::new(then_body),
                alternate: else_body.map(Box::new),
            }),
            span: empty_span(),
        }
    }

    fn while_stmt(body: Stmt) -> Stmt {
        Stmt {
            kind: StmtKind::While(WhileStmt {
                test: Box::new(Expr {
                    kind: ExprKind::Ident("cond".to_string().into()),
                    span: empty_span(),
                }),
                body: Box::new(body),
            }),
            span: empty_span(),
        }
    }

    #[test]
    fn build_simple_cfg() {
        let mut builder = CfgBuilder::new();
        let entry = builder.entry();
        let stmt = builder.add_node(CfgNodeKind::Statement);
        let exit = builder.exit();
        builder.add_edge(entry, stmt, EdgeKind::Sequential);
        builder.add_edge(stmt, exit, EdgeKind::Sequential);
        let cfg = builder.finish();

        assert_eq!(cfg.successors(entry), vec![(stmt, EdgeKind::Sequential)]);
        assert_eq!(cfg.predecessors(exit), vec![(stmt, EdgeKind::Sequential)]);
    }

    #[test]
    fn reachability() {
        let mut builder = CfgBuilder::new();
        let a = builder.add_node(CfgNodeKind::Entry);
        let b = builder.add_node(CfgNodeKind::Statement);
        let c = builder.add_node(CfgNodeKind::Exit);
        let d = builder.add_node(CfgNodeKind::Statement); // unreachable from a
        builder.add_edge(a, b, EdgeKind::Sequential);
        builder.add_edge(b, c, EdgeKind::Sequential);
        let cfg = builder.finish();

        let reachable = cfg.reachable_from(a);
        assert!(reachable.contains(&a));
        assert!(reachable.contains(&b));
        assert!(reachable.contains(&c));
        assert!(!reachable.contains(&d));
    }

    #[test]
    fn build_from_sequential_stmts() {
        let stmts = vec![expr_stmt("a"), expr_stmt("b"), expr_stmt("c")];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        // entry + 3 statements + exit = 5 nodes
        assert_eq!(cfg.node_count(), 5);
        // entry->s1, s1->s2, s2->s3, s3->exit = 4 edges
        assert_eq!(cfg.edge_count(), 4);
    }

    #[test]
    fn build_from_return_terminates() {
        let stmts = vec![expr_stmt("a"), return_stmt(), expr_stmt("unreachable")];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        // entry + stmt_a + return_node + exit = 4 nodes (unreachable not visited)
        // entry->a, a->ret, ret->exit(Return)
        let entry = CfgNodeId(0);
        let reachable = cfg.reachable_from(entry);
        // Should reach entry, stmt_a, return_node, exit
        assert_eq!(reachable.len(), 4);
    }

    #[test]
    fn build_from_throw_terminates() {
        let stmts = vec![throw_stmt()];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        let entry = CfgNodeId(0);
        let reachable = cfg.reachable_from(entry);
        // entry, throw_node, exit
        assert_eq!(reachable.len(), 3);
    }

    #[test]
    fn build_if_else() {
        let stmts = vec![if_stmt(expr_stmt("then"), Some(expr_stmt("else")))];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        // entry, cond, then_stmt, else_stmt, merge, exit = 6
        assert_eq!(cfg.node_count(), 6);
    }

    #[test]
    fn build_if_no_else() {
        let stmts = vec![if_stmt(expr_stmt("then"), None)];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        // entry, cond, then_stmt, merge, exit = 5
        assert_eq!(cfg.node_count(), 5);
    }

    #[test]
    fn build_while_loop() {
        let stmts = vec![while_stmt(expr_stmt("body"))];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        // entry, loop, cond, body_stmt, merge, exit = 6
        assert_eq!(cfg.node_count(), 6);
        // Check there's a Loop edge (body -> loop_node)
        let has_loop_edge = cfg.edges.iter().any(|(_, _, k)| *k == EdgeKind::Loop);
        assert!(has_loop_edge);
    }

    #[test]
    fn build_if_both_branches_return() {
        let stmts = vec![
            if_stmt(return_stmt(), Some(return_stmt())),
            expr_stmt("unreachable"),
        ];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        let _entry = CfgNodeId(0);
        let exit = CfgNodeId(1);
        // Both branches return, so the "unreachable" stmt should not be created
        // and there should be no Sequential edge to exit from merge
        let exit_preds = cfg.predecessors(exit);
        // Only Return edges should reach exit
        assert!(exit_preds.iter().all(|(_, k)| *k == EdgeKind::Return));
    }

    #[test]
    fn build_nested_if_while() {
        let stmts = vec![
            if_stmt(while_stmt(expr_stmt("loop_body")), None),
            expr_stmt("after"),
        ];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        // Should build without panic and have reasonable structure
        assert!(cfg.node_count() >= 7);
        assert!(cfg.edge_count() >= 7);
    }

    #[test]
    fn break_flows_to_loop_merge() {
        let stmts = vec![while_stmt(break_stmt()), expr_stmt("after")];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        let has_break_edge = cfg.edges.iter().any(|(_, _, k)| *k == EdgeKind::Break);
        assert!(has_break_edge);

        let entry = CfgNodeId(0);
        let reachable = cfg.reachable_from(entry);
        assert!(
            reachable.len() >= 6,
            "expected loop exit to reach trailing stmt"
        );
    }

    #[test]
    fn continue_flows_to_loop_condition() {
        let stmts = vec![while_stmt(continue_stmt())];
        let cfg = CfgBuilder::build_from_stmts(&stmts);
        let has_continue_edge = cfg.edges.iter().any(|(_, _, k)| *k == EdgeKind::Continue);
        assert!(has_continue_edge);
    }
}
