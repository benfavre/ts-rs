//! TS7027 inside function bodies that live in expressions (arrows, function
//! expressions, object-literal methods, class members). The statement walker
//! in `lib.rs` hands every expression position to [`TypeChecker::unreachable_in_expr`],
//! which finds those bodies and runs the same statement-list check on them.

use tsc_rs_ast::*;

use crate::TypeChecker;

impl TypeChecker {
    pub(crate) fn unreachable_in_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::FnExpr(function) => {
                if let Some(body) = &function.body {
                    self.check_unreachable_code_in_stmts(body);
                }
            }
            ExprKind::Arrow(arrow) => match &arrow.body {
                ArrowBody::Block(body) => self.check_unreachable_code_in_stmts(body),
                ArrowBody::Expr(body) => self.unreachable_in_expr(body),
            },
            ExprKind::ClassExpr(class) => self.unreachable_in_class(class),
            ExprKind::ObjectLit(properties) => {
                for property in properties {
                    match property {
                        ObjLitProp::Property(property) => {
                            self.unreachable_in_prop_name(&property.key);
                            self.unreachable_in_expr(&property.value);
                        }
                        ObjLitProp::ShorthandDefault(_, value, _)
                        | ObjLitProp::Spread(value, _) => {
                            self.unreachable_in_expr(value);
                        }
                        ObjLitProp::Method(method) => {
                            self.unreachable_in_prop_name(&method.name);
                            self.check_unreachable_code_in_stmts(&method.body);
                        }
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                            self.unreachable_in_prop_name(&accessor.name);
                            self.check_unreachable_code_in_stmts(&accessor.body);
                        }
                        ObjLitProp::Shorthand(..) => {}
                    }
                }
            }
            ExprKind::ArrayLit(elements) => {
                for element in elements.iter().flatten() {
                    self.unreachable_in_expr(element);
                }
            }
            ExprKind::Template(template) => {
                for part in &template.exprs {
                    self.unreachable_in_expr(part);
                }
            }
            ExprKind::TaggedTemplate(tagged) => {
                self.unreachable_in_expr(&tagged.tag);
                for part in &tagged.quasi.exprs {
                    self.unreachable_in_expr(part);
                }
            }
            ExprKind::Call(call) => {
                self.unreachable_in_expr(&call.callee);
                for argument in &call.args {
                    self.unreachable_in_expr(argument);
                }
            }
            ExprKind::New(new) => {
                self.unreachable_in_expr(&new.callee);
                for argument in new.args.iter().flatten() {
                    self.unreachable_in_expr(argument);
                }
            }
            ExprKind::Member(member) => self.unreachable_in_expr(&member.object),
            ExprKind::ElemAccess(access) => {
                self.unreachable_in_expr(&access.object);
                self.unreachable_in_expr(&access.index);
            }
            ExprKind::Cond(conditional) => {
                self.unreachable_in_expr(&conditional.test);
                self.unreachable_in_expr(&conditional.consequent);
                self.unreachable_in_expr(&conditional.alternate);
            }
            ExprKind::Binary(binary) => {
                self.unreachable_in_expr(&binary.left);
                self.unreachable_in_expr(&binary.right);
            }
            ExprKind::Assign(assign) => {
                self.unreachable_in_expr(&assign.left);
                self.unreachable_in_expr(&assign.right);
            }
            ExprKind::Unary(unary) => self.unreachable_in_expr(&unary.argument),
            ExprKind::Update(update) => self.unreachable_in_expr(&update.argument),
            ExprKind::TypeAssertion(assertion) => self.unreachable_in_expr(&assertion.expr),
            ExprKind::As(assertion) => self.unreachable_in_expr(&assertion.expr),
            ExprKind::Satisfies(assertion) => self.unreachable_in_expr(&assertion.expr),
            ExprKind::Instantiation(instantiation) => self.unreachable_in_expr(&instantiation.expr),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.unreachable_in_expr(inner),
            ExprKind::Yield(_, Some(inner)) => self.unreachable_in_expr(inner),
            ExprKind::Comma(parts) => {
                for part in parts {
                    self.unreachable_in_expr(part);
                }
            }
            _ => {}
        }
    }

    fn unreachable_in_prop_name(&mut self, name: &PropName) {
        if let PropName::Computed(expr, _) = name {
            self.unreachable_in_expr(expr);
        }
    }

    pub(crate) fn unreachable_in_class(&mut self, class: &ClassDecl) {
        if let Some(extends) = &class.extends {
            self.unreachable_in_expr(extends);
        }
        for member in &class.members {
            match &member.kind {
                ClassMemberKind::Property(property) => {
                    self.unreachable_in_prop_name(&property.name);
                    if let Some(initializer) = &property.initializer {
                        self.unreachable_in_expr(initializer);
                    }
                }
                ClassMemberKind::Method(method) => {
                    self.unreachable_in_prop_name(&method.name);
                    if let Some(body) = &method.body {
                        self.check_unreachable_code_in_stmts(body);
                    }
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    self.unreachable_in_prop_name(&accessor.name);
                    if let Some(body) = &accessor.body {
                        self.check_unreachable_code_in_stmts(body);
                    }
                }
                ClassMemberKind::Constructor(constructor) => {
                    if let Some(body) = &constructor.body {
                        self.check_unreachable_code_in_stmts(body);
                    }
                }
                ClassMemberKind::StaticBlock(body) => self.check_unreachable_code_in_stmts(body),
                _ => {}
            }
        }
    }
}
