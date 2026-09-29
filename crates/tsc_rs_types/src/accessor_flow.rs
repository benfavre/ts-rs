//! Reachability for getter and function completion diagnostics.
//!
//! Getter checks follow binder reachability. General function completion can
//! additionally use resolved never calls and exhaustive switches; those facts
//! are recorded while checking each statement, before leaving its scope.
use tsc_rs_ast::*;

#[derive(Default)]
struct Flow {
    normal: bool,
    returned: bool,
    throws: bool,
    breaks: Vec<Option<String>>,
    continues: Vec<Option<String>>,
}

impl Flow {
    fn normal() -> Self {
        Self {
            normal: true,
            ..Self::default()
        }
    }

    fn merge(mut self, other: Self) -> Self {
        self.normal |= other.normal;
        self.returned |= other.returned;
        self.throws |= other.throws;
        self.breaks.extend(other.breaks);
        self.continues.extend(other.continues);
        self
    }
}

#[derive(Default)]
struct Semantics<'a> {
    never_expression: Option<&'a dyn Fn(&Expr) -> bool>,
    exhaustive_switch: Option<&'a dyn Fn(&SwitchStmt) -> bool>,
}

/// Semantic completion shares the getter's binder flow, adding resolved
/// non-returning calls and exhaustive switches without changing getter checks.
pub(crate) fn function_completion(
    body: &[Stmt],
    never_expression: &dyn Fn(&Expr) -> bool,
    exhaustive_switch: &dyn Fn(&SwitchStmt) -> bool,
) -> (bool, bool) {
    let flow = sequence(
        body,
        &Semantics {
            never_expression: Some(never_expression),
            exhaustive_switch: Some(exhaustive_switch),
        },
    );
    (flow.normal, flow.returned)
}

pub(crate) fn missing_return(body: &[Stmt]) -> bool {
    let flow = sequence(body, &Semantics::default());
    flow.normal && !flow.returned
}

fn sequence(body: &[Stmt], semantics: &Semantics<'_>) -> Flow {
    let mut flow = Flow::normal();
    for stmt in body {
        if !flow.normal {
            break;
        }
        let next = statement(stmt, &[], semantics);
        flow.normal = false;
        flow = flow.merge(next);
    }
    flow
}

fn truth(expr: &Expr) -> Option<bool> {
    match &expr.kind {
        ExprKind::BoolLit(value) => Some(*value),
        ExprKind::Binary(binary) => {
            let left = truth(&binary.left);
            let right = truth(&binary.right);
            match binary.op {
                BinaryOp::LogAnd if left == Some(false) || right == Some(false) => Some(false),
                BinaryOp::LogAnd if left == Some(true) => right,
                BinaryOp::LogOr if left == Some(true) || right == Some(true) => Some(true),
                BinaryOp::LogOr if left == Some(false) => right,
                _ => None,
            }
        }
        // Binder flow does not fold arbitrary truthy values, negation, or
        // parenthesized literals here; those remain potentially two-way.
        _ => None,
    }
}

fn loop_flow(
    body: &Stmt,
    condition: Option<bool>,
    at_least_once: bool,
    labels: &[String],
    semantics: &Semantics<'_>,
) -> Flow {
    if !at_least_once && condition == Some(false) {
        return Flow::normal();
    }
    let mut flow = statement(body, &[], semantics);
    let has_break = flow.breaks.iter().any(Option::is_none);
    flow.breaks.retain(Option::is_some);
    let continues_here =
        |label: &Option<String>| label.as_ref().is_none_or(|label| labels.contains(label));
    let has_continue = flow.continues.iter().any(continues_here);
    flow.continues.retain(|label| !continues_here(label));
    flow.normal =
        has_break || (condition != Some(true) && (!at_least_once || flow.normal || has_continue));
    flow
}

fn statement(stmt: &Stmt, loop_labels: &[String], semantics: &Semantics<'_>) -> Flow {
    match &stmt.kind {
        StmtKind::Expr(expr) if semantics.never_expression.is_some_and(|check| check(expr)) => {
            Flow::default()
        }
        StmtKind::Return(_) => Flow {
            returned: true,
            ..Flow::default()
        },
        StmtKind::Throw(_) => Flow {
            throws: true,
            ..Flow::default()
        },
        StmtKind::Break(label) => Flow {
            breaks: vec![label.clone()],
            ..Flow::default()
        },
        StmtKind::Continue(label) => Flow {
            continues: vec![label.clone()],
            ..Flow::default()
        },
        StmtKind::Block(body) => sequence(body, semantics),
        StmtKind::If(branch) => match truth(&branch.test) {
            Some(true) => statement(&branch.consequent, &[], semantics),
            Some(false) => branch
                .alternate
                .as_deref()
                .map_or_else(Flow::normal, |stmt| statement(stmt, &[], semantics)),
            None => statement(&branch.consequent, &[], semantics).merge(
                branch
                    .alternate
                    .as_deref()
                    .map_or_else(Flow::normal, |stmt| statement(stmt, &[], semantics)),
            ),
        },
        StmtKind::While(loop_stmt) => loop_flow(
            &loop_stmt.body,
            truth(&loop_stmt.test),
            false,
            loop_labels,
            semantics,
        ),
        StmtKind::DoWhile(loop_stmt) => loop_flow(
            &loop_stmt.body,
            truth(&loop_stmt.test),
            true,
            loop_labels,
            semantics,
        ),
        StmtKind::For(loop_stmt) => loop_flow(
            &loop_stmt.body,
            loop_stmt.test.as_deref().map_or(Some(true), truth),
            false,
            loop_labels,
            semantics,
        ),
        StmtKind::ForIn(loop_stmt) => {
            loop_flow(&loop_stmt.body, None, false, loop_labels, semantics)
        }
        StmtKind::ForOf(loop_stmt) => {
            loop_flow(&loop_stmt.body, None, false, loop_labels, semantics)
        }
        StmtKind::Switch(switch) => {
            let mut result = Flow {
                normal: !switch.cases.iter().any(|case| case.test.is_none())
                    && !semantics
                        .exhaustive_switch
                        .is_some_and(|check| check(switch)),
                ..Flow::default()
            };
            let mut suffix_reaches_end = true;
            for case in switch.cases.iter().rev() {
                let mut flow = sequence(&case.consequent, semantics);
                let has_break = flow.breaks.iter().any(Option::is_none);
                flow.breaks.retain(Option::is_some);
                flow.normal = has_break || (flow.normal && suffix_reaches_end);
                suffix_reaches_end = flow.normal;
                result = result.merge(flow);
            }
            result
        }
        StmtKind::Try(try_stmt) => {
            let mut flow = sequence(&try_stmt.block, semantics);
            if let Some(handler) = &try_stmt.handler {
                // A catch entry is potentially reachable even without an
                // explicit throw in the try block.
                flow.throws = false;
                flow = flow.merge(sequence(&handler.body, semantics));
            }
            if let Some(finalizer) = &try_stmt.finalizer {
                if flow.normal
                    || flow.returned
                    || flow.throws
                    || !flow.breaks.is_empty()
                    || !flow.continues.is_empty()
                {
                    let final_flow = sequence(finalizer, semantics);
                    if final_flow.normal {
                        let normal = flow.normal;
                        flow = flow.merge(final_flow);
                        flow.normal = normal;
                    } else {
                        let returned = flow.returned;
                        flow = final_flow;
                        flow.returned |= returned;
                    }
                }
            }
            flow
        }
        StmtKind::Labeled(label) => {
            let mut labels = loop_labels.to_vec();
            labels.push(label.label.clone());
            let mut flow = statement(&label.body, &labels, semantics);
            let exits_here = |target: &Option<String>| target.as_ref() == Some(&label.label);
            flow.normal |= flow.breaks.iter().any(exits_here);
            flow.breaks.retain(|target| !exits_here(target));
            flow
        }
        StmtKind::With(with) => statement(&with.body, &[], semantics),
        // Declarations and nested expressions do not return from this function.
        _ => Flow::normal(),
    }
}
