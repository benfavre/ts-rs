//! TS2774: truthiness checks of definitely-present function values.

use super::*;

#[derive(Clone, PartialEq, Eq)]
enum PathRoot {
    Binding(std::string::String, usize),
    This,
}

#[derive(Clone, PartialEq, Eq)]
enum PathPart {
    Property(std::string::String),
    Call,
}

#[derive(Clone, PartialEq, Eq)]
struct FunctionPath {
    root: PathRoot,
    parts: Vec<PathPart>,
}

impl FunctionPath {
    fn binding_name(&self) -> Option<&str> {
        match &self.root {
            PathRoot::Binding(name, _) => Some(name),
            PathRoot::This => None,
        }
    }
}

impl TypeChecker {
    fn uncalled_class_member_is_optional(&self, expr: &Expr) -> bool {
        let ExprKind::Member(member) = &expr.kind else {
            return false;
        };
        let (mut class_name, is_static) = if matches!(
            &Self::uncalled_strip_expr(&member.object).kind,
            ExprKind::This
        ) {
            let Some(name) = self.enclosing_class_names.last() else {
                return false;
            };
            (name.clone(), false)
        } else {
            match self.infer_expr_type(&member.object) {
                Type::This => {
                    let Some(name) = self.enclosing_class_names.last() else {
                        return false;
                    };
                    (name.clone(), false)
                }
                Type::TypeReference(name, _) => match name.strip_prefix("typeof ") {
                    Some(name) => (name.to_string(), true),
                    None => (name, false),
                },
                _ => return false,
            }
        };
        let mut seen = rustc_hash::FxHashSet::default();
        loop {
            if !seen.insert(class_name.clone()) {
                return false;
            }
            let Some(info) = self.class_info.get(class_name.as_str()) else {
                return false;
            };
            let optional = if is_static {
                &info.optional_static_properties
            } else {
                &info.optional_instance_properties
            };
            if optional.contains(member.property.as_str()) {
                return true;
            }
            let Some(base) = &info.extends else {
                return false;
            };
            class_name = base.clone();
        }
    }

    fn uncalled_strip_expr<'a>(mut expr: &'a Expr) -> &'a Expr {
        loop {
            expr = match &expr.kind {
                ExprKind::Paren(inner) | ExprKind::NonNull(inner) => inner,
                ExprKind::As(assertion) => &assertion.expr,
                ExprKind::Satisfies(satisfies) => &satisfies.expr,
                ExprKind::TypeAssertion(assertion) => &assertion.expr,
                ExprKind::Instantiation(instantiation) => &instantiation.expr,
                _ => return expr,
            };
        }
    }

    fn uncalled_condition_expr(mut expr: &Expr) -> &Expr {
        while let ExprKind::Paren(inner) = &expr.kind {
            expr = inner;
        }
        expr
    }

    fn uncalled_member_receiver_has_assertion(expr: &Expr) -> bool {
        let expr = Self::uncalled_condition_expr(expr);
        match &expr.kind {
            ExprKind::As(_)
            | ExprKind::Satisfies(_)
            | ExprKind::TypeAssertion(_)
            | ExprKind::NonNull(_)
            | ExprKind::Instantiation(_) => true,
            ExprKind::Member(member) => {
                Self::uncalled_member_receiver_has_assertion(&member.object)
            }
            ExprKind::ElemAccess(access) => {
                Self::uncalled_member_receiver_has_assertion(&access.object)
            }
            _ => false,
        }
    }

    fn uncalled_path(&self, expr: &Expr) -> Option<FunctionPath> {
        let expr = Self::uncalled_strip_expr(expr);
        match &expr.kind {
            ExprKind::Ident(name) => Some(FunctionPath {
                root: PathRoot::Binding(
                    name.to_string(),
                    self.nearest_binding_scope(name.as_str())?,
                ),
                parts: Vec::new(),
            }),
            ExprKind::This => Some(FunctionPath {
                root: PathRoot::This,
                parts: Vec::new(),
            }),
            ExprKind::Member(member) => {
                let mut path = self.uncalled_path(&member.object)?;
                path.parts
                    .push(PathPart::Property(member.property.to_string()));
                Some(path)
            }
            ExprKind::ElemAccess(access) => {
                let property = match &Self::uncalled_strip_expr(&access.index).kind {
                    ExprKind::StrLit(name) | ExprKind::NoSubstTemplate(name) => name.to_string(),
                    _ => return None,
                };
                let mut path = self.uncalled_path(&access.object)?;
                path.parts.push(PathPart::Property(property));
                Some(path)
            }
            ExprKind::Call(call) if !call.optional => {
                let mut path = self.uncalled_path(&call.callee)?;
                path.parts.push(PathPart::Call);
                Some(path)
            }
            _ => None,
        }
    }

    fn uncalled_type_is_definitely_callable(&self, ty: &Type, depth: usize) -> bool {
        if depth > 12 {
            return false;
        }
        match ty {
            Type::Function(_) => true,
            Type::ObjectType(info) => !info.call_signatures.is_empty(),
            Type::Union(parts) => {
                !parts.is_empty()
                    && parts
                        .iter()
                        .all(|part| self.uncalled_type_is_definitely_callable(part, depth + 1))
            }
            Type::Intersection(parts) => {
                !parts.iter().any(|part| {
                    matches!(
                        part,
                        Type::Any
                            | Type::Error
                            | Type::Unknown
                            | Type::TypeParameter(_)
                            | Type::Null
                            | Type::Undefined
                            | Type::Optional(_)
                    )
                }) && parts
                    .iter()
                    .any(|part| self.uncalled_type_is_definitely_callable(part, depth + 1))
            }
            Type::Readonly(inner) | Type::Instance(inner) => {
                self.uncalled_type_is_definitely_callable(inner, depth + 1)
            }
            Type::TypeParameter(_) => {
                self.typeparam_constraint_apparent(ty)
                    .is_some_and(|constraint| {
                        self.uncalled_type_is_definitely_callable(&constraint, depth + 1)
                    })
            }
            Type::TypeReference(_, _) => {
                if let Some(constraint) = self.typeparam_constraint_apparent(ty) {
                    self.uncalled_type_is_definitely_callable(&constraint, depth + 1)
                } else {
                    self.resolve_type_for_assignability(ty)
                        .filter(|resolved| resolved != ty)
                        .is_some_and(|resolved| {
                            self.uncalled_type_is_definitely_callable(&resolved, depth + 1)
                        })
                }
            }
            // `any`, error recovery, unknown, unconstrained type parameters,
            // optional and nullable values intentionally never qualify.
            _ => false,
        }
    }

    fn uncalled_type_has_uncertain_presence(&self, ty: &Type, depth: usize) -> bool {
        if depth > 12 {
            return true;
        }
        match ty {
            Type::Any
            | Type::Error
            | Type::Unknown
            | Type::Null
            | Type::Undefined
            | Type::Optional(_) => true,
            Type::Union(parts) => parts
                .iter()
                .any(|part| self.uncalled_type_has_uncertain_presence(part, depth + 1)),
            Type::TypeParameter(_) => {
                self.typeparam_constraint_apparent(ty)
                    .is_none_or(|constraint| {
                        self.uncalled_type_has_uncertain_presence(&constraint, depth + 1)
                    })
            }
            _ => false,
        }
    }

    fn uncalled_type_is_explicitly_optional(ty: &Type) -> bool {
        match ty {
            Type::Optional(_) | Type::Undefined => true,
            Type::Union(parts) => parts.iter().any(Self::uncalled_type_is_explicitly_optional),
            _ => false,
        }
    }

    fn uncalled_redundant_optional_member_type(&self, expr: &Expr) -> Option<Type> {
        let ExprKind::Member(member) = &expr.kind else {
            return None;
        };
        if !member.optional {
            return None;
        }
        let mut owner = self.infer_expr_type(&member.object);
        if let Some(apparent) = self.typeparam_constraint_apparent(&owner) {
            owner = apparent;
        }
        if self.uncalled_type_has_uncertain_presence(&owner, 0) {
            return None;
        }
        let property_span = Span::new(
            expr.span.end.saturating_sub(member.property.len() as u32),
            expr.span.end,
        );
        let property = self.cooked_identifier_name(&member.property, property_span);
        self.qualified_property_type(owner, &property)
    }

    fn uncalled_pat_binds(pattern: &Pat, name: &str) -> bool {
        match &pattern.kind {
            PatKind::Ident(binding) => binding.as_str() == name,
            PatKind::Array(elements) => elements.iter().flatten().any(|element| match element {
                ArrayPatElem::Pat(pattern) | ArrayPatElem::Rest(pattern) => {
                    Self::uncalled_pat_binds(pattern, name)
                }
            }),
            PatKind::Object(properties) => properties.iter().any(|property| match property {
                ObjPatProp::KeyValue(_, pattern) | ObjPatProp::Rest(pattern) => {
                    Self::uncalled_pat_binds(pattern, name)
                }
                ObjPatProp::Shorthand(binding, _) | ObjPatProp::ShorthandAssign(binding, _, _) => {
                    binding.as_str() == name
                }
            }),
            PatKind::Assign(pattern, _) | PatKind::Rest(pattern) => {
                Self::uncalled_pat_binds(pattern, name)
            }
        }
    }

    fn uncalled_params_shadow(params: &[Param], name: Option<&str>) -> bool {
        name.is_some_and(|name| {
            params
                .iter()
                .any(|param| Self::uncalled_pat_binds(&param.name, name))
        })
    }

    fn uncalled_stmt_declares_lexical_value(statement: &Stmt, name: &str) -> bool {
        match &statement.kind {
            StmtKind::Var(var) if var.kind != VarKind::Var => var
                .declarations
                .iter()
                .any(|declaration| Self::uncalled_pat_binds(&declaration.name, name)),
            StmtKind::FnDecl(function) => function.name.as_deref() == Some(name),
            StmtKind::ClassDecl(class) => class.name.as_deref() == Some(name),
            StmtKind::EnumDecl(enumeration) => enumeration.name == name,
            StmtKind::ModuleDecl(module) => {
                matches!(&module.name, ModuleName::Ident(module_name) if module_name == name)
            }
            StmtKind::ImportEquals(import) => import.name == name,
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    Self::uncalled_stmt_declares_lexical_value(inner, name)
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn uncalled_expr_uses(
        &self,
        expr: &Expr,
        wanted: &FunctionPath,
        binding_shadowed: bool,
        this_shadowed: bool,
    ) -> bool {
        let root_available = match &wanted.root {
            PathRoot::Binding(_, _) => !binding_shadowed,
            PathRoot::This => !this_shadowed,
        };
        if root_available && self.uncalled_path(expr).as_ref() == Some(wanted) {
            return true;
        }

        let recurse =
            |child: &Expr| self.uncalled_expr_uses(child, wanted, binding_shadowed, this_shadowed);
        match &expr.kind {
            ExprKind::Call(call) => {
                recurse(&call.callee) || call.args.iter().any(|argument| recurse(argument))
            }
            ExprKind::New(new_expr) => {
                recurse(&new_expr.callee)
                    || new_expr
                        .args
                        .as_ref()
                        .is_some_and(|arguments| arguments.iter().any(|argument| recurse(argument)))
            }
            ExprKind::Member(member) => recurse(&member.object),
            ExprKind::ElemAccess(access) => recurse(&access.object) || recurse(&access.index),
            ExprKind::Cond(condition) => {
                recurse(&condition.test)
                    || recurse(&condition.consequent)
                    || recurse(&condition.alternate)
            }
            ExprKind::Binary(binary) => recurse(&binary.left) || recurse(&binary.right),
            ExprKind::Unary(unary) => recurse(&unary.argument),
            ExprKind::Update(update) => recurse(&update.argument),
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => recurse(inner),
            ExprKind::As(assertion) => recurse(&assertion.expr),
            ExprKind::Satisfies(satisfies) => recurse(&satisfies.expr),
            ExprKind::TypeAssertion(assertion) => recurse(&assertion.expr),
            ExprKind::Instantiation(instantiation) => recurse(&instantiation.expr),
            ExprKind::Assign(assign) => recurse(&assign.left) || recurse(&assign.right),
            ExprKind::Comma(expressions) => {
                expressions.iter().any(|expression| recurse(expression))
            }
            ExprKind::ArrayLit(elements) => {
                elements.iter().flatten().any(|element| recurse(element))
            }
            ExprKind::Template(template) => {
                template.exprs.iter().any(|expression| recurse(expression))
            }
            ExprKind::TaggedTemplate(tagged) => {
                recurse(&tagged.tag)
                    || tagged
                        .quasi
                        .exprs
                        .iter()
                        .any(|expression| recurse(expression))
            }
            ExprKind::ObjectLit(properties) => properties.iter().any(|property| match property {
                ObjLitProp::Property(property) => recurse(&property.value),
                ObjLitProp::Shorthand(name, _) => {
                    !binding_shadowed
                        && matches!(
                            (&wanted.root, wanted.parts.as_slice()),
                            (PathRoot::Binding(wanted_name, _), [])
                                if wanted_name == name
                        )
                }
                ObjLitProp::ShorthandDefault(name, initializer, _) => {
                    (!binding_shadowed
                        && matches!(
                            (&wanted.root, wanted.parts.as_slice()),
                            (PathRoot::Binding(wanted_name, _), [])
                                if wanted_name == name
                        ))
                        || recurse(initializer)
                }
                ObjLitProp::Spread(expression, _) => recurse(expression),
                ObjLitProp::Method(method) => {
                    let shadowed = binding_shadowed
                        || Self::uncalled_params_shadow(&method.params, wanted.binding_name());
                    self.uncalled_stmts_use(&method.body, wanted, shadowed, true)
                }
                ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                    let shadowed = binding_shadowed
                        || Self::uncalled_params_shadow(&accessor.params, wanted.binding_name());
                    self.uncalled_stmts_use(&accessor.body, wanted, shadowed, true)
                }
            }),
            ExprKind::Arrow(arrow) => {
                let shadowed = binding_shadowed
                    || Self::uncalled_params_shadow(&arrow.params, wanted.binding_name());
                match &arrow.body {
                    ArrowBody::Expr(body) => {
                        self.uncalled_expr_uses(body, wanted, shadowed, this_shadowed)
                    }
                    ArrowBody::Block(body) => self.uncalled_stmts_use(
                        body,
                        wanted,
                        shadowed
                            || wanted.binding_name().is_some_and(|name| {
                                body.iter().any(|statement| {
                                    Self::uncalled_stmt_hoists_var(statement, name)
                                })
                            }),
                        this_shadowed,
                    ),
                }
            }
            ExprKind::FnExpr(function) => {
                let shadowed = binding_shadowed
                    || wanted
                        .binding_name()
                        .is_some_and(|name| function.name.as_deref() == Some(name))
                    || Self::uncalled_params_shadow(&function.params, wanted.binding_name());
                function.body.as_ref().is_some_and(|body| {
                    self.uncalled_stmts_use(
                        body,
                        wanted,
                        shadowed
                            || wanted.binding_name().is_some_and(|name| {
                                body.iter().any(|statement| {
                                    Self::uncalled_stmt_hoists_var(statement, name)
                                })
                            }),
                        true,
                    )
                })
            }
            ExprKind::Yield(_, value) => value.as_ref().is_some_and(|value| recurse(value)),
            _ => false,
        }
    }

    fn uncalled_stmts_use(
        &self,
        statements: &[Stmt],
        wanted: &FunctionPath,
        mut binding_shadowed: bool,
        this_shadowed: bool,
    ) -> bool {
        if let Some(name) = wanted.binding_name() {
            // Lexical declarations bind for the whole block (including TDZ),
            // and must not be mistaken for the outer condition binding merely
            // because their declaration appears later. Function-scoped `var`
            // shadowing is applied when entering the nested function body.
            binding_shadowed |= statements
                .iter()
                .any(|statement| Self::uncalled_stmt_declares_lexical_value(statement, name));
        }
        for statement in statements {
            if self.uncalled_stmt_uses(statement, wanted, binding_shadowed, this_shadowed) {
                return true;
            }
        }
        false
    }

    fn uncalled_stmt_hoists_var(statement: &Stmt, name: &str) -> bool {
        match &statement.kind {
            StmtKind::Var(var) if var.kind == VarKind::Var => var
                .declarations
                .iter()
                .any(|declaration| Self::uncalled_pat_binds(&declaration.name, name)),
            StmtKind::Block(statements) => statements
                .iter()
                .any(|statement| Self::uncalled_stmt_hoists_var(statement, name)),
            StmtKind::If(statement) => {
                Self::uncalled_stmt_hoists_var(&statement.consequent, name)
                    || statement
                        .alternate
                        .as_ref()
                        .is_some_and(|alternate| Self::uncalled_stmt_hoists_var(alternate, name))
            }
            StmtKind::While(statement) => Self::uncalled_stmt_hoists_var(&statement.body, name),
            StmtKind::DoWhile(statement) => Self::uncalled_stmt_hoists_var(&statement.body, name),
            StmtKind::For(statement) => {
                matches!(
                    &statement.init,
                    Some(ForInit::Var(var)) if var.kind == VarKind::Var
                        && var.declarations.iter().any(|declaration| {
                            Self::uncalled_pat_binds(&declaration.name, name)
                        })
                ) || Self::uncalled_stmt_hoists_var(&statement.body, name)
            }
            StmtKind::Labeled(statement) => Self::uncalled_stmt_hoists_var(&statement.body, name),
            StmtKind::With(statement) => Self::uncalled_stmt_hoists_var(&statement.body, name),
            // A nested function has its own var scope.
            _ => false,
        }
    }

    fn uncalled_stmt_uses(
        &self,
        statement: &Stmt,
        wanted: &FunctionPath,
        binding_shadowed: bool,
        this_shadowed: bool,
    ) -> bool {
        let expr_uses =
            |expr: &Expr| self.uncalled_expr_uses(expr, wanted, binding_shadowed, this_shadowed);
        match &statement.kind {
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
                expr_uses(expr)
            }
            StmtKind::Return(value) => value.as_ref().is_some_and(|value| expr_uses(value)),
            StmtKind::Block(statements) => {
                self.uncalled_stmts_use(statements, wanted, binding_shadowed, this_shadowed)
            }
            StmtKind::If(if_statement) => {
                expr_uses(&if_statement.test)
                    || self.uncalled_stmt_uses(
                        &if_statement.consequent,
                        wanted,
                        binding_shadowed,
                        this_shadowed,
                    )
                    || if_statement.alternate.as_ref().is_some_and(|alternate| {
                        self.uncalled_stmt_uses(alternate, wanted, binding_shadowed, this_shadowed)
                    })
            }
            StmtKind::While(while_statement) => {
                expr_uses(&while_statement.test)
                    || self.uncalled_stmt_uses(
                        &while_statement.body,
                        wanted,
                        binding_shadowed,
                        this_shadowed,
                    )
            }
            StmtKind::DoWhile(do_while) => {
                self.uncalled_stmt_uses(&do_while.body, wanted, binding_shadowed, this_shadowed)
                    || expr_uses(&do_while.test)
            }
            StmtKind::For(for_statement) => {
                let init_uses = for_statement.init.as_ref().is_some_and(|init| match init {
                    ForInit::Expr(expr) => expr_uses(expr),
                    ForInit::Var(var) => var.declarations.iter().any(|declaration| {
                        declaration
                            .init
                            .as_ref()
                            .is_some_and(|initializer| expr_uses(initializer))
                    }),
                });
                let loop_shadows = binding_shadowed
                    || wanted.binding_name().is_some_and(|name| {
                        matches!(
                            &for_statement.init,
                            Some(ForInit::Var(var)) if var.declarations.iter().any(|declaration| {
                                Self::uncalled_pat_binds(&declaration.name, name)
                            })
                        )
                    });
                init_uses
                    || for_statement
                        .test
                        .as_ref()
                        .is_some_and(|test| expr_uses(test))
                    || for_statement
                        .update
                        .as_ref()
                        .is_some_and(|update| expr_uses(update))
                    || self.uncalled_stmt_uses(
                        &for_statement.body,
                        wanted,
                        loop_shadows,
                        this_shadowed,
                    )
            }
            StmtKind::ForIn(for_statement) => {
                let left_uses = match &for_statement.left {
                    ForInOfLeft::Expr(expr) => expr_uses(expr),
                    ForInOfLeft::Var(var) => var.declarations.iter().any(|declaration| {
                        declaration
                            .init
                            .as_ref()
                            .is_some_and(|initializer| expr_uses(initializer))
                    }),
                    ForInOfLeft::Pat(_) => false,
                };
                let loop_shadows = binding_shadowed
                    || wanted
                        .binding_name()
                        .is_some_and(|name| match &for_statement.left {
                            ForInOfLeft::Var(var) => var.declarations.iter().any(|declaration| {
                                Self::uncalled_pat_binds(&declaration.name, name)
                            }),
                            ForInOfLeft::Pat(pattern) => Self::uncalled_pat_binds(pattern, name),
                            ForInOfLeft::Expr(_) => false,
                        });
                left_uses
                    || expr_uses(&for_statement.right)
                    || self.uncalled_stmt_uses(
                        &for_statement.body,
                        wanted,
                        loop_shadows,
                        this_shadowed,
                    )
            }
            StmtKind::ForOf(for_statement) => {
                let left_uses = match &for_statement.left {
                    ForInOfLeft::Expr(expr) => expr_uses(expr),
                    ForInOfLeft::Var(var) => var.declarations.iter().any(|declaration| {
                        declaration
                            .init
                            .as_ref()
                            .is_some_and(|initializer| expr_uses(initializer))
                    }),
                    ForInOfLeft::Pat(_) => false,
                };
                let loop_shadows = binding_shadowed
                    || wanted
                        .binding_name()
                        .is_some_and(|name| match &for_statement.left {
                            ForInOfLeft::Var(var) => var.declarations.iter().any(|declaration| {
                                Self::uncalled_pat_binds(&declaration.name, name)
                            }),
                            ForInOfLeft::Pat(pattern) => Self::uncalled_pat_binds(pattern, name),
                            ForInOfLeft::Expr(_) => false,
                        });
                left_uses
                    || expr_uses(&for_statement.right)
                    || self.uncalled_stmt_uses(
                        &for_statement.body,
                        wanted,
                        loop_shadows,
                        this_shadowed,
                    )
            }
            StmtKind::Switch(switch_statement) => {
                let switch_shadows = binding_shadowed
                    || wanted.binding_name().is_some_and(|name| {
                        switch_statement.cases.iter().any(|case| {
                            case.consequent.iter().any(|statement| {
                                Self::uncalled_stmt_declares_lexical_value(statement, name)
                            })
                        })
                    });
                expr_uses(&switch_statement.discriminant)
                    || switch_statement.cases.iter().any(|case| {
                        case.test.as_ref().is_some_and(|test| expr_uses(test))
                            || self.uncalled_stmts_use(
                                &case.consequent,
                                wanted,
                                switch_shadows,
                                this_shadowed,
                            )
                    })
            }
            StmtKind::Try(try_statement) => {
                self.uncalled_stmts_use(
                    &try_statement.block,
                    wanted,
                    binding_shadowed,
                    this_shadowed,
                ) || try_statement.handler.as_ref().is_some_and(|handler| {
                    let catch_shadows = binding_shadowed
                        || wanted.binding_name().is_some_and(|name| {
                            handler
                                .param
                                .as_ref()
                                .is_some_and(|parameter| Self::uncalled_pat_binds(parameter, name))
                        });
                    self.uncalled_stmts_use(&handler.body, wanted, catch_shadows, this_shadowed)
                }) || try_statement.finalizer.as_ref().is_some_and(|finalizer| {
                    self.uncalled_stmts_use(finalizer, wanted, binding_shadowed, this_shadowed)
                })
            }
            StmtKind::Var(var) => var.declarations.iter().any(|declaration| {
                declaration
                    .init
                    .as_ref()
                    .is_some_and(|init| expr_uses(init))
            }),
            StmtKind::FnDecl(function) => {
                let shadowed = binding_shadowed
                    || wanted
                        .binding_name()
                        .is_some_and(|name| function.name.as_deref() == Some(name))
                    || Self::uncalled_params_shadow(&function.params, wanted.binding_name());
                function.body.as_ref().is_some_and(|body| {
                    self.uncalled_stmts_use(
                        body,
                        wanted,
                        shadowed
                            || wanted.binding_name().is_some_and(|name| {
                                body.iter().any(|statement| {
                                    Self::uncalled_stmt_hoists_var(statement, name)
                                })
                            }),
                        true,
                    )
                })
            }
            StmtKind::Labeled(label) => {
                self.uncalled_stmt_uses(&label.body, wanted, binding_shadowed, this_shadowed)
            }
            StmtKind::With(with_statement) => {
                expr_uses(&with_statement.object)
                    || self.uncalled_stmt_uses(
                        &with_statement.body,
                        wanted,
                        binding_shadowed,
                        this_shadowed,
                    )
            }
            _ => false,
        }
    }

    fn uncalled_check_candidate(
        &mut self,
        expr: &Expr,
        later: &[&Expr],
        guarded_expr: Option<&Expr>,
        guarded_stmt: Option<&Stmt>,
    ) {
        let expr = Self::uncalled_condition_expr(expr);
        if !matches!(&expr.kind, ExprKind::Ident(_) | ExprKind::Member(_)) {
            return;
        }
        if let ExprKind::Member(member) = &expr.kind {
            // TypeScript does not issue TS2774 for a property reached through
            // an asserted receiver, even when the asserted property type is
            // callable.
            if Self::uncalled_member_receiver_has_assertion(&member.object) {
                return;
            }
            // Preserve optional class/property presence. `check_expr` returns
            // the callable payload for these accesses, so inspect the raw
            // inferred member type before that normalization.
            if !member.optional
                && Self::uncalled_type_is_explicitly_optional(&self.infer_expr_type(expr))
            {
                return;
            }
        }
        let Some(path) = self.uncalled_path(expr) else {
            return;
        };
        if self.uncalled_class_member_is_optional(expr) {
            return;
        }
        if let ExprKind::Ident(name) = &expr.kind {
            if self.nearer_binding_is(&Self::optional_value_marker_name(name), name) == Some(true) {
                return;
            }
        }
        let optional_member_type = self.uncalled_redundant_optional_member_type(expr);
        let ty = if let Some(ty) = optional_member_type {
            ty
        } else {
            let diagnostics_start = self.diagnostics.len();
            let ty = self.check_expr(expr);
            self.diagnostics.truncate(diagnostics_start);
            ty
        };
        if !self.uncalled_type_is_definitely_callable(&ty, 0) {
            return;
        }
        let used = later
            .iter()
            .any(|later| self.uncalled_expr_uses(later, &path, false, false))
            || guarded_expr
                .is_some_and(|guard| self.uncalled_expr_uses(guard, &path, false, false))
            || guarded_stmt
                .is_some_and(|guard| self.uncalled_stmt_uses(guard, &path, false, false));
        if !used {
            self.diagnostics
                .push(error_uncalled_function_truthiness(expr.span));
        }
    }

    fn uncalled_collect_and<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
        let stripped = Self::uncalled_condition_expr(expr);
        if let ExprKind::Binary(binary) = &stripped.kind {
            if binary.op == BinaryOp::LogAnd {
                Self::uncalled_collect_and(&binary.left, out);
                Self::uncalled_collect_and(&binary.right, out);
                return;
            }
        }
        out.push(expr);
    }

    fn uncalled_check_inner(
        &mut self,
        expr: &Expr,
        inherited_later: &[&Expr],
        guarded_expr: Option<&Expr>,
        guarded_stmt: Option<&Stmt>,
        result_is_condition: bool,
    ) {
        let stripped = Self::uncalled_condition_expr(expr);
        match &stripped.kind {
            ExprKind::Binary(binary) if binary.op == BinaryOp::LogAnd => {
                let mut operands = Vec::new();
                Self::uncalled_collect_and(expr, &mut operands);
                for (index, operand) in operands.iter().enumerate() {
                    let mut later = Vec::with_capacity(
                        operands.len().saturating_sub(index + 1) + inherited_later.len(),
                    );
                    later.extend_from_slice(&operands[index + 1..]);
                    later.extend_from_slice(inherited_later);
                    self.uncalled_check_inner(
                        operand,
                        &later,
                        guarded_expr,
                        guarded_stmt,
                        index + 1 < operands.len() || result_is_condition,
                    );
                }
            }
            ExprKind::Binary(binary)
                if matches!(binary.op, BinaryOp::LogOr | BinaryOp::NullCoal) =>
            {
                self.uncalled_check_inner(
                    &binary.left,
                    inherited_later,
                    guarded_expr,
                    guarded_stmt,
                    true,
                );
                self.uncalled_check_inner(
                    &binary.right,
                    inherited_later,
                    guarded_expr,
                    guarded_stmt,
                    result_is_condition,
                );
            }
            ExprKind::Unary(unary) if unary.op == UnaryOp::LogNot => {}
            _ if result_is_condition => {
                self.uncalled_check_candidate(stripped, inherited_later, guarded_expr, guarded_stmt)
            }
            _ => {}
        }
    }

    pub(crate) fn check_uncalled_function_condition(
        &mut self,
        expr: &Expr,
        guarded_expr: Option<&Expr>,
        guarded_stmt: Option<&Stmt>,
    ) {
        if self.strict_null_checks {
            self.uncalled_check_inner(expr, &[], guarded_expr, guarded_stmt, true);
        }
    }

    pub(crate) fn check_uncalled_function_logical_value(&mut self, expr: &Expr) {
        if self.strict_null_checks
            && matches!(
                &Self::uncalled_condition_expr(expr).kind,
                ExprKind::Binary(binary) if binary.op == BinaryOp::LogAnd
            )
        {
            self.uncalled_check_inner(expr, &[], None, None, false);
        }
    }
}
