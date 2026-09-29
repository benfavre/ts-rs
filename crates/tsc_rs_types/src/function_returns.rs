//! Diagnostics for reachable function endpoints, separate from checking the
//! types of individual return expressions.

use crate::{accessor_flow, Type, TypeChecker};
use tsc_rs_ast::*;

impl TypeChecker {
    pub(crate) fn check_function_completion(
        &mut self,
        body: &[Stmt],
        annotation: Option<&TypeNode>,
        function_span: Span,
        is_async: bool,
        is_generator: bool,
    ) {
        if annotation.is_none() && self.compiler_options.no_implicit_returns != Some(true) {
            return;
        }
        let return_type = annotation.map(|node| {
            let ty = self.resolve_type_node(node);
            self.completion_return_type(ty, is_async, is_generator, 0)
        });
        if return_type
            .as_ref()
            .is_some_and(Self::implicit_return_exempt)
        {
            return;
        }
        let (reaches_end, has_return) = accessor_flow::function_completion(
            body,
            &|expr| self.completion_never_expression(expr),
            &|switch| {
                self.completion_exhaustive_switches
                    .contains(&switch.discriminant.span.start)
            },
        );
        if !reaches_end {
            return;
        }
        let (code, message) = if matches!(return_type, Some(Type::Never)) {
            (
                2534,
                "A function returning 'never' cannot have a reachable end point.",
            )
        } else if return_type.is_some() && !has_return {
            (2355, "A function whose declared type is neither 'undefined', 'void', nor 'any' must return a value.")
        } else if return_type.as_ref().is_some_and(|ty| {
            self.strict_null_checks && !self.is_assignable_to(&Type::Undefined, ty)
        }) {
            (2366, "Function lacks ending return statement and return type does not include 'undefined'.")
        } else if self.compiler_options.no_implicit_returns == Some(true) {
            if return_type.is_none() {
                if !has_return {
                    return;
                }
                let inferred = self.infer_return_type_from_stmts(body);
                if Self::implicit_return_exempt(&inferred) {
                    return;
                }
            }
            (7030, "Not all code paths return a value.")
        } else {
            return;
        };
        let span = annotation.map_or(function_span, |node| node.span);
        if self
            .reported_duplicate_spans
            .insert((code, span.start, span.end))
        {
            self.diagnostics.push(Diagnostic {
                code,
                message: message.to_string(),
                category: DiagnosticCategory::Error,
                file_name: self.current_file_name.clone(),
                span: Some(span),
                related: None,
            });
        }
    }

    fn implicit_return_exempt(ty: &Type) -> bool {
        matches!(ty, Type::Any | Type::Error | Type::Void | Type::Undefined)
            || matches!(ty, Type::Union(members) if members.iter().any(|ty| matches!(ty, Type::Void)))
    }

    fn completion_return_type(
        &self,
        ty: Type,
        is_async: bool,
        is_generator: bool,
        depth: u8,
    ) -> Type {
        if depth > 16 {
            return Type::Error;
        }
        if let Type::TypeReference(name, args) = &ty {
            if is_generator
                && matches!(
                    name.as_str(),
                    "Generator"
                        | "AsyncGenerator"
                        | "Iterator"
                        | "AsyncIterator"
                        | "IterableIterator"
                        | "AsyncIterableIterator"
                )
            {
                let result = args.get(1).cloned().unwrap_or(Type::Any);
                return self.completion_return_type(result, is_async, false, depth + 1);
            }
            if is_async && matches!(name.as_str(), "Promise" | "PromiseLike") {
                return self.completion_return_type(
                    args.first().cloned().unwrap_or(Type::Any),
                    true,
                    false,
                    depth + 1,
                );
            }
            if let Some(resolved) = self.resolve_type_for_assignability(&ty) {
                if resolved != ty {
                    return self.completion_return_type(
                        resolved,
                        is_async,
                        is_generator,
                        depth + 1,
                    );
                }
            }
        }
        if is_generator {
            return Type::Error;
        }
        if let Type::TypeReference(name, _) = &ty {
            if !self.is_known_type_name(name)
                && !self.enum_info.contains_key(name)
                && (!self.type_name_is_known(name) || self.file_import_binding_names.contains(name))
            {
                return Type::Error;
            }
        }
        ty
    }

    pub(crate) fn completion_annotation_marker(name: &str) -> String {
        format!("__tsrs_completion_annotated__{name}")
    }

    pub(crate) fn completion_mark_parameter(&mut self, parameter: &Param) {
        if let (PatKind::Ident(name), Some(annotation)) =
            (&parameter.name.kind, &parameter.type_ann)
        {
            self.completion_mark_annotation(name, annotation);
        }
    }

    pub(crate) fn completion_mark_annotation(&mut self, name: &str, annotation: &TypeNode) {
        // Scalar annotations cannot provide a callable dotted name.
        if !matches!(
            annotation.kind,
            TypeNodeKind::Keyword(_) | TypeNodeKind::Literal(_)
        ) {
            self.declare_var(&Self::completion_annotation_marker(name), Type::Never);
        }
    }

    fn completion_never_expression(&self, expr: &Expr) -> bool {
        self.completion_never_calls.contains(&expr.span.start)
    }

    pub(crate) fn completion_explicit_call(&self, expr: &Expr) -> bool {
        let ExprKind::Call(call) = &expr.kind else {
            return false;
        };
        !call.optional && self.completion_explicit_name(&call.callee)
    }

    fn completion_explicit_name(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(name) => {
                self.nearer_binding_is(&Self::function_marker_name(name), name) == Some(true)
                    || self.nearer_binding_is(&Self::completion_annotation_marker(name), name)
                        == Some(true)
            }
            ExprKind::Paren(inner) => self.completion_explicit_name(inner),
            ExprKind::Member(member) => {
                !member.optional && self.completion_explicit_name(&member.object)
            }
            ExprKind::This | ExprKind::Super => true,
            _ => false,
        }
    }

    pub(crate) fn completion_switch_exhaustive(
        &self,
        switch: &SwitchStmt,
        discriminant: &Type,
    ) -> bool {
        let Some(members) = self.completion_finite_members(discriminant, 0) else {
            return false;
        };
        let cases: Vec<_> = switch
            .cases
            .iter()
            .filter_map(|case| case.test.as_ref())
            .map(|test| {
                // Lightweight expression inference retains an enum's named type;
                // exhaustiveness needs the particular member's constant value.
                if let ExprKind::Member(member) = &test.kind {
                    if let Type::TypeReference(name, _) = self.infer_expr_type(&member.object) {
                        if let Some(value) = self.enum_info.get(&name).and_then(|members| {
                            members.iter().find(|(name, _)| name == member.property)
                        }) {
                            return value.1.clone();
                        }
                    }
                }
                self.infer_expr_type(test)
            })
            .collect();
        members.iter().all(|member| {
            cases.iter().any(|case| {
                let case = match case {
                    Type::EnumVariant {
                        value: Some(value), ..
                    } => value.as_ref(),
                    other => other,
                };
                member == case
            })
        })
    }

    fn completion_finite_members(&self, ty: &Type, depth: u8) -> Option<Vec<Type>> {
        if depth > 16 {
            return None;
        }
        match ty {
            Type::Never => Some(Vec::new()),
            Type::Boolean => Some(vec![
                Type::BooleanLiteral(false),
                Type::BooleanLiteral(true),
            ]),
            Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::BigIntLiteral(_)
            | Type::Null
            | Type::Undefined => Some(vec![ty.clone()]),
            Type::EnumVariant {
                value: Some(value), ..
            } => self.completion_finite_members(value, depth + 1),
            Type::Union(members) => {
                let mut result = Vec::new();
                for member in members.iter() {
                    result.extend(self.completion_finite_members(member, depth + 1)?);
                }
                Some(result)
            }
            Type::TypeParameter(_) => self
                .declared_type_param_constraint(ty)
                .and_then(|constraint| self.completion_finite_members(&constraint, depth + 1)),
            Type::TypeReference(name, _) if self.enum_info.contains_key(name) => {
                let mut result = Vec::new();
                for (_, value) in &self.enum_info[name] {
                    result.extend(self.completion_finite_members(value, depth + 1)?);
                }
                Some(result)
            }
            Type::TypeReference(_, _) => self
                .resolve_type_for_assignability(ty)
                .filter(|resolved| resolved != ty)
                .and_then(|resolved| self.completion_finite_members(&resolved, depth + 1)),
            _ => None,
        }
    }
}
