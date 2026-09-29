//! ES5 resumable control flow for async bodies.
//!
//! Build the complete plan before emitting anything. Unsupported suspension
//! shapes leave the existing transform intact; they cannot produce a partly
//! lowered function. Each yield terminates a block and resumes in its successor.
use super::*;
mod discovery;
mod exceptions;
mod expressions;
mod for_in;
mod loops;
mod objects;
mod switches;

const STATE_BINDING: &str = "\0generator_state\0";

#[derive(Clone)]
enum Operation {
    Expr(Expr),
    Assign(AstString, Expr),
    Return(Option<Expr>),
    Throw(Expr),
    Yield(Expr),
    Jump(usize),
    Branch(Expr, usize),
    BranchTrue(Expr, usize),
    PendingJump(usize),
    NativeControl(bool, Option<String>),
    Loop(Box<NativeLoop>),
    Block(Box<NativeBlock>),
    Switch(Box<GeneratorSwitch>),
    SetLabel(usize),
    If(Expr, OperationBody, Option<OperationBody>),
    TryRegion(usize, Option<usize>, Option<usize>, usize),
    NativeTry(Box<NativeTry>),
    Catch(AstString),
    EndFinally,
}

#[derive(Clone)]
struct NativeTry {
    body: OperationBody,
    catch: Option<(Option<AstString>, OperationBody)>,
    finally: Option<OperationBody>,
}

#[derive(Clone)]
struct OperationBody {
    operations: Vec<Operation>,
    braced: bool,
}

#[derive(Clone)]
enum NativeLoopKind {
    While(Expr),
    DoWhile(Expr),
    For(Option<Expr>, Option<Expr>, Option<Expr>),
    ForIn(Expr, Expr),
}

#[derive(Clone)]
struct NativeLoop {
    kind: NativeLoopKind,
    labels: Vec<String>,
    body: OperationBody,
}

#[derive(Clone)]
struct NativeBlock {
    labels: Vec<String>,
    body: OperationBody,
}

#[derive(Clone)]
struct GeneratorSwitch {
    value: Expr,
    cases: Vec<(Option<Expr>, OperationBody)>,
    labels: Vec<String>,
    dispatch: bool,
}

#[derive(Clone)]
struct ControlScope {
    is_loop: bool,
    label_only: bool,
    labels: Vec<String>,
    // Resolve break and continue destinations after planning. Switch scopes
    // use only the first slot; continue searches outward for a loop.
    targets: Option<(usize, usize)>,
}

#[derive(Clone)]
pub(super) struct GeneratorPlan {
    blocks: Vec<Vec<Operation>>,
    locals: Vec<AstString>,
    temps: Vec<AstString>,
    object_key_temps: Vec<AstString>,
    state: String,
    directives: Vec<Stmt>,
    reserved_temp_count: usize,
    control_scopes: Vec<ControlScope>,
    jump_targets: Vec<Option<usize>>,
    catch_bindings: Vec<lexical_downlevel::BindingId>,
    has_catch: bool,
    used_loop_index: bool,
    active_catch_bindings: Vec<(AstString, Option<lexical_downlevel::BindingId>)>,
}

impl GeneratorPlan {
    fn push(&mut self, op: Operation) {
        self.blocks.last_mut().unwrap().push(op);
    }

    fn terminated(&self) -> bool {
        matches!(
            self.blocks.last().and_then(|b| b.last()),
            Some(
                Operation::Return(_)
                    | Operation::Throw(_)
                    | Operation::Jump(_)
                    | Operation::Yield(_)
                    | Operation::PendingJump(_)
                    | Operation::NativeControl(_, _)
                    | Operation::EndFinally
            )
        )
    }

    fn label(&mut self) -> usize {
        let label = self.blocks.len();
        if !self.terminated() {
            self.push(Operation::SetLabel(label));
        }
        self.blocks.push(Vec::new());
        label
    }

    fn untyped(expr: &Expr) -> &Expr {
        match &expr.kind {
            ExprKind::As(inner) => Self::untyped(&inner.expr),
            ExprKind::TypeAssertion(inner) => Self::untyped(&inner.expr),
            ExprKind::Satisfies(inner) => Self::untyped(&inner.expr),
            ExprKind::NonNull(inner) => Self::untyped(inner),
            ExprKind::Instantiation(inner) => Self::untyped(&inner.expr),
            _ => expr,
        }
    }

    fn source<'s>(expr: &Expr, source: &'s str) -> &'s str {
        source
            .get(expr.span.start as usize..expr.span.end as usize)
            .unwrap_or("")
    }

    fn has_await(expr: &Expr, source: &str) -> bool {
        // Conservative for nested function bodies, strings and recovery ASTs.
        if matches!(expr.kind, ExprKind::Await(_)) || Self::source(expr, source).contains("await") {
            return true;
        }
        // Synthetic containers have no source span, but their original
        // operands still carry suspension information.
        expr.span == Span::new(0, 0)
            && expression_children_any(expr, &mut |child| Self::has_await(child, source))
    }

    fn statements(&mut self, stmts: &[Stmt], context: &Emitter<'_>) -> Option<()> {
        for stmt in stmts {
            self.statement(stmt, context)?;
        }
        Some(())
    }

    fn statement(&mut self, stmt: &Stmt, context: &Emitter<'_>) -> Option<()> {
        let source = context.source;
        match &stmt.kind {
            StmtKind::Empty | StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => {}
            StmtKind::Expr(expr) => {
                let value = self.expression(expr, context)?;
                self.push(Operation::Expr(value));
            }
            StmtKind::Return(value) => {
                let value = match value {
                    Some(value) => Some(self.expression(value, context)?),
                    None => None,
                };
                self.push(Operation::Return(value));
            }
            StmtKind::Throw(value) => {
                let value = self.expression(value, context)?;
                self.push(Operation::Throw(value));
            }
            StmtKind::Var(vars) if vars.kind == VarKind::Var => {
                for decl in &vars.declarations {
                    let PatKind::Ident(name) = &decl.name.kind else {
                        return None;
                    };
                    if !self.locals.contains(name) {
                        self.locals.push(name.clone());
                    }
                    if let Some(init) = &decl.init {
                        let value = self.expression(init, context)?;
                        let target = self.catch_assignment_name(name);
                        self.push(Operation::Assign(target, value));
                    }
                }
            }
            StmtKind::Block(stmts) => {
                self.block_statement(stmt.span, stmts, Vec::new(), context)?
            }
            StmtKind::If(branch) => {
                let value = self.expression(&branch.test, context)?;
                let arm_has_await = |stmt: &Stmt| {
                    source
                        .get(stmt.span.start as usize..stmt.span.end as usize)
                        .unwrap_or("")
                        .contains("await")
                };
                if !arm_has_await(&branch.consequent)
                    && branch
                        .alternate
                        .as_ref()
                        .is_none_or(|arm| !arm_has_await(arm))
                {
                    let consequent = self.native_body(&branch.consequent, context)?;
                    let alternate = match &branch.alternate {
                        Some(arm) => Some(self.native_body(arm, context)?),
                        None => None,
                    };
                    self.push(Operation::If(value, consequent, alternate));
                    return Some(());
                }
                let origin = self.blocks.len() - 1;
                let index = self.blocks[origin].len();
                self.push(Operation::Branch(value, 0));
                self.statement_body(&branch.consequent, context)?;
                let then_end = self.blocks.len() - 1;
                let jump = if self.terminated() {
                    None
                } else {
                    let index = self.blocks[then_end].len();
                    self.push(Operation::Jump(0));
                    Some(index)
                };
                let alternate = self.label();
                if let Operation::Branch(_, target) = &mut self.blocks[origin][index] {
                    *target = alternate;
                }
                if let Some(alt) = &branch.alternate {
                    self.statement_body(alt, context)?;
                }
                let end = if branch.alternate.is_some() {
                    self.label()
                } else {
                    alternate
                };
                if let Some(index) = jump {
                    if let Operation::Jump(target) = &mut self.blocks[then_end][index] {
                        *target = end;
                    }
                }
            }
            StmtKind::While(_)
            | StmtKind::DoWhile(_)
            | StmtKind::For(_)
            | StmtKind::ForIn(_)
            | StmtKind::Labeled(_) => self.loop_statement(stmt, context)?,
            StmtKind::Switch(switch) => self.switch_statement(switch, Vec::new(), context)?,
            StmtKind::Try(try_) => self.try_statement(stmt.span, try_, context)?,
            StmtKind::Break(label) => self.control_statement(false, label)?,
            StmtKind::Continue(label) => self.control_statement(true, label)?,
            _ => return None,
        }
        Some(())
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn plan_es5_async_body(&self, body: &[Stmt]) -> Option<GeneratorPlan> {
        if self.effective_target() >= ScriptTarget::ES2015
            || self.options.import_helpers == Some(true)
        {
            return None;
        }
        // Super requires a class home-object environment. The existing async
        // method transform continues to own that rewrite.
        if body.iter().any(|s| {
            self.source_between(s.span.start, s.span.end)
                .contains("super")
        }) {
            return None;
        }
        let directive_count = body.iter().take_while(|stmt| matches!(&stmt.kind, StmtKind::Expr(expr) if matches!(expr.kind, ExprKind::StrLit(_)))).count();
        let mut plan = GeneratorPlan {
            blocks: vec![Vec::new()],
            locals: Vec::new(),
            temps: Vec::new(),
            object_key_temps: Vec::new(),
            state: STATE_BINDING.to_string(),
            directives: body[..directive_count].to_vec(),
            reserved_temp_count: self.class_scope_temp_reserved,
            control_scopes: Vec::new(),
            jump_targets: Vec::new(),
            catch_bindings: Vec::new(),
            has_catch: false,
            used_loop_index: false,
            active_catch_bindings: Vec::new(),
        };
        plan.statements(&body[directive_count..], self)?;
        plan.resolve_jumps()?;
        plan.state = plan.fresh_name(self);
        plan.resolve_state_bindings();
        if !plan.terminated() {
            plan.push(Operation::Return(None));
        }
        Some(plan)
    }

    pub(super) fn scan_es5_generator_helpers(&mut self, stmts: &[Stmt]) {
        self.needs_generator_helper |= self.effective_target() < ScriptTarget::ES2015
            && self.options.import_helpers != Some(true)
            && stmts.iter().any(|stmt| self.stmt_needs_es5_generator(stmt));
    }

    pub(super) fn emit_es5_async_generator(&mut self, plan: &GeneratorPlan, params: &[Param]) {
        self.emit_es5_async_generator_layout(plan, params, false);
    }

    pub(super) fn emit_es5_async_generator_layout(
        &mut self,
        plan: &GeneratorPlan,
        params: &[Param],
        compact: bool,
    ) {
        let mut renamed_plan;
        let plan = if plan.catch_bindings.is_empty() {
            plan
        } else {
            renamed_plan = plan.clone();
            self.prepare_generator_catches(&mut renamed_plan);
            &renamed_plan
        };
        let names = plan
            .locals
            .iter()
            .chain(plan.temps.iter())
            .chain(plan.object_key_temps.iter())
            .chain(params.iter().filter_map(|param| {
                if let PatKind::Ident(name) = &param.name.kind {
                    Some(name)
                } else {
                    None
                }
            }));
        let mut hidden_imports = Vec::new();
        let mut added_shadows = Vec::new();
        for name in names {
            if let Some(binding) = self.cjs_import_map.remove(name.as_str()) {
                hidden_imports.push((name.clone(), binding));
            }
            if self.cjs_param_shadows.insert(name.clone()) {
                added_shadows.push(name.clone());
            }
        }
        let previous_catch_scope = self.generator_catch_scope;
        self.generator_catch_scope |= plan.has_catch;
        self.emit_scoped_body_with_param_initializers_layout(
            &[],
            &plan.directives,
            compact,
            |emitter, _| {
                emitter.emit_es5_generator_contents(plan, params, compact);
            },
        );
        self.generator_catch_scope = previous_catch_scope;
        for name in added_shadows {
            self.cjs_param_shadows.remove(&name);
        }
        self.restore_shadowed_cjs_imports(hidden_imports);
    }

    fn emit_es5_generator_contents(
        &mut self,
        plan: &GeneratorPlan,
        params: &[Param],
        compact: bool,
    ) {
        // The callback parameter shares a name space with expression temps
        // referenced from its enclosing generator closure.
        self.temp_var_counter = self.temp_var_counter.max(plan.reserved_temp_count);
        if !plan.object_key_temps.is_empty() {
            self.write("var ");
            self.write(
                &plan
                    .object_key_temps
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if compact {
                self.write("; ");
            } else {
                self.writeln(";");
            }
        }
        let locals: Vec<_> = plan.locals.iter().chain(plan.temps.iter()).filter(|name| !params.iter().any(|param| matches!(&param.name.kind, PatKind::Ident(parameter) if parameter == *name))).collect();
        if !locals.is_empty() {
            self.write("var ");
            self.write(
                &locals
                    .iter()
                    .map(|n| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            if compact {
                self.write("; ");
            } else {
                self.writeln(";");
            }
        }
        self.write("return ");
        self.write(self.helper_prefix());
        self.write("__generator(this, function (");
        self.write(&plan.state);
        self.writeln(") {");
        self.indent += 1;
        let switch = plan.blocks.len() > 1;
        if switch {
            self.write("switch (");
            self.write(&plan.state);
            self.writeln(".label) {");
            self.indent += 1;
        }
        for (label, block) in plan.blocks.iter().enumerate() {
            let inline = switch && block.len() == 1;
            if switch {
                self.write(&format!("case {label}:"));
                if inline {
                    self.write(" ");
                } else {
                    self.newline();
                    self.indent += 1;
                }
            }
            for op in block {
                self.emit_generator_operation(op, &plan.state);
            }
            if switch && !inline {
                self.indent -= 1;
            }
        }
        if switch {
            self.indent -= 1;
            self.writeln("}");
        }
        self.indent -= 1;
        self.writeln("});");
    }
    fn emit_generator_value(&mut self, value: &Expr) {
        let comma = matches!(GeneratorPlan::untyped(value).kind, ExprKind::Comma(_));
        if comma {
            self.write("(");
        }
        self.emit_expr(value);
        if comma {
            self.write(")");
        }
    }

    fn emit_generator_operation(&mut self, op: &Operation, state: &str) {
        match op {
            Operation::SetLabel(label) => self.writeln(&format!("{state}.label = {label};")),
            Operation::If(test, consequent, alternate) => {
                self.write("if (");
                self.emit_expr(test);
                self.write(")");
                self.emit_generator_body(consequent, state);
                if let Some(alternate) = alternate {
                    self.write("else");
                    self.emit_generator_body(alternate, state);
                }
            }
            Operation::Loop(loop_) => self.emit_generator_loop(loop_, state),
            Operation::Block(block) => {
                for label in &block.labels {
                    self.write(label);
                    self.write(": ");
                }
                self.writeln("{");
                self.indent += 1;
                for op in &block.body.operations {
                    self.emit_generator_operation(op, state);
                }
                self.indent -= 1;
                self.writeln("}");
            }
            Operation::Switch(switch) => self.emit_generator_switch(switch, state),
            Operation::NativeControl(is_continue, label) => {
                self.write(if *is_continue { "continue" } else { "break" });
                if let Some(label) = label {
                    self.write(" ");
                    self.write(label);
                }
                self.writeln(";");
            }
            Operation::PendingJump(_) => unreachable!("unresolved control target"),
            Operation::TryRegion(start, catch, finally, end) => self.writeln(&format!(
                "{state}.trys.push([{start}, {}, {}, {end}]);",
                catch.map(|label| label.to_string()).unwrap_or_default(),
                finally.map(|label| label.to_string()).unwrap_or_default(),
            )),
            Operation::NativeTry(try_) => self.emit_generator_native_try(try_, state),
            Operation::Catch(name) => self.writeln(&format!("{name} = {state}.sent();")),
            Operation::EndFinally => self.writeln("return [7 /*endfinally*/];"),

            Operation::Expr(value) => {
                self.emit_expr(value);
                self.writeln(";");
            }
            Operation::Assign(name, value) => {
                self.write(name);
                self.write(" = ");
                self.emit_generator_value(value);
                self.writeln(";");
            }
            Operation::Return(value) => {
                self.write("return [2 /*return*/");
                if let Some(value) = value {
                    self.write(", ");
                    self.emit_generator_value(value);
                }
                self.writeln("];");
            }
            Operation::Throw(value) => {
                self.write("throw ");
                self.emit_expr(value);
                self.writeln(";");
            }
            Operation::Yield(value) => {
                self.write("return [4 /*yield*/, ");
                self.emit_generator_value(value);
                self.writeln("];");
            }
            Operation::Jump(target) => self.writeln(&format!("return [3 /*break*/, {target}];")),
            Operation::Branch(value, target) | Operation::BranchTrue(value, target) => {
                let negate = matches!(op, Operation::Branch(_, _));
                self.write(if negate { "if (!" } else { "if (" });
                let needs_parens = negate
                    && matches!(
                        GeneratorPlan::untyped(value).kind,
                        ExprKind::Binary(_)
                            | ExprKind::Assign(_)
                            | ExprKind::Cond(_)
                            | ExprKind::Comma(_)
                    );
                if needs_parens {
                    self.write("(");
                }
                self.emit_expr(value);
                if needs_parens {
                    self.write(")");
                }
                self.write(") ");
                self.writeln(&format!("return [3 /*break*/, {target}];"));
            }
        }
    }
}

impl<'a> Emitter<'a> {
    pub(super) fn emit_generator_helper(&mut self) {
        for line in r#"var __generator = (this && this.__generator) || function (thisArg, body) {
    var _ = { label: 0, sent: function() { if (t[0] & 1) throw t[1]; return t[1]; }, trys: [], ops: [] }, f, y, t, g = Object.create((typeof Iterator === "function" ? Iterator : Object).prototype);
    return g.next = verb(0), g["throw"] = verb(1), g["return"] = verb(2), typeof Symbol === "function" && (g[Symbol.iterator] = function() { return this; }), g;
    function verb(n) { return function (v) { return step([n, v]); }; }
    function step(op) {
        if (f) throw new TypeError("Generator is already executing.");
        while (g && (g = 0, op[0] && (_ = 0)), _) try {
            if (f = 1, y && (t = op[0] & 2 ? y["return"] : op[0] ? y["throw"] || ((t = y["return"]) && t.call(y), 0) : y.next) && !(t = t.call(y, op[1])).done) return t;
            if (y = 0, t) op = [op[0] & 2, t.value];
            switch (op[0]) {
                case 0: case 1: t = op; break;
                case 4: _.label++; return { value: op[1], done: false };
                case 5: _.label++; y = op[1]; op = [0]; continue;
                case 7: op = _.ops.pop(); _.trys.pop(); continue;
                default:
                    if (!(t = _.trys, t = t.length > 0 && t[t.length - 1]) && (op[0] === 6 || op[0] === 2)) { _ = 0; continue; }
                    if (op[0] === 3 && (!t || (op[1] > t[0] && op[1] < t[3]))) { _.label = op[1]; break; }
                    if (op[0] === 6 && _.label < t[1]) { _.label = t[1]; t = op; break; }
                    if (t && _.label < t[2]) { _.label = t[2]; _.ops.push(op); break; }
                    if (t[2]) _.ops.pop();
                    _.trys.pop(); continue;
            }
            op = body.call(thisArg, _);
        } catch (e) { op = [6, e]; y = 0; } finally { f = t = 0; }
        if (op[0] & 5) throw op[1]; return { value: op[0] ? op[1] : void 0, done: true };
    }
};
"#.lines() {
            self.writeln(line);
        }
    }
}

// Follow expression evaluation containers, stopping at nested function scopes.
fn expression_children_any(expr: &Expr, predicate: &mut impl FnMut(&Expr) -> bool) -> bool {
    if let Some(inner) = expr.kind.type_layer_inner() {
        return predicate(inner);
    }
    match &expr.kind {
        ExprKind::Call(call) => predicate(&call.callee) || call.args.iter().any(|e| predicate(e)),
        ExprKind::New(new) => {
            predicate(&new.callee)
                || new
                    .args
                    .as_ref()
                    .is_some_and(|args| args.iter().any(|e| predicate(e)))
        }
        ExprKind::Member(member) => predicate(&member.object),
        ExprKind::ElemAccess(access) => predicate(&access.object) || predicate(&access.index),
        ExprKind::ArrayLit(elements) => elements.iter().flatten().any(|e| predicate(e)),
        ExprKind::Binary(binary) => predicate(&binary.left) || predicate(&binary.right),
        ExprKind::Assign(assign) => predicate(&assign.left) || predicate(&assign.right),
        ExprKind::Unary(unary) => predicate(&unary.argument),
        ExprKind::Update(update) => predicate(&update.argument),
        ExprKind::Paren(e)
        | ExprKind::Spread(e)
        | ExprKind::Await(e)
        | ExprKind::Delete(e)
        | ExprKind::Typeof(e)
        | ExprKind::Void(e) => predicate(e),
        ExprKind::Cond(cond) => {
            predicate(&cond.test) || predicate(&cond.consequent) || predicate(&cond.alternate)
        }
        ExprKind::Comma(values) => values.iter().any(|e| predicate(e)),
        ExprKind::Template(template) => template.exprs.iter().any(|e| predicate(e)),
        ExprKind::TaggedTemplate(tagged) => {
            predicate(&tagged.tag) || tagged.quasi.exprs.iter().any(|e| predicate(e))
        }
        ExprKind::ObjectLit(props) => props.iter().any(|prop| match prop {
            ObjLitProp::Property(prop) => {
                matches!(&prop.key, PropName::Computed(e, _) if predicate(e))
                    || predicate(&prop.value)
            }
            ObjLitProp::Spread(e, _) | ObjLitProp::ShorthandDefault(_, e, _) => predicate(e),
            ObjLitProp::Method(m) => matches!(&m.name, PropName::Computed(e, _) if predicate(e)),
            ObjLitProp::Get(a) | ObjLitProp::Set(a) => {
                matches!(&a.name, PropName::Computed(e, _) if predicate(e))
            }
            _ => false,
        }),
        _ => false,
    }
}
