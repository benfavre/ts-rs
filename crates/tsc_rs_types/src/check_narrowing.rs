//! Type narrowing analysis methods for TypeChecker.

use std::sync::Arc;

use super::*;

impl TypeChecker {
    // -----------------------------------------------------------------------
    // Type narrowing analysis
    // -----------------------------------------------------------------------

    /// Analyze a condition expression and return narrowing info for the
    /// consequent (true) branch and alternate (false) branch.
    /// Returns (consequent_narrows, alternate_narrows).
    pub(crate) fn analyze_narrowing(&self, expr: &Expr) -> NarrowingResult {
        match &expr.kind {
            // typeof x === "string" narrows x to string in consequent
            ExprKind::Binary(bin) => {
                match bin.op {
                    BinaryOp::StrictEq | BinaryOp::Eq => {
                        if let Some((equal, not_equal)) =
                            self.optional_chain_nullish_comparison(bin)
                        {
                            return (equal, not_equal);
                        }
                        // typeof x === "string"
                        if let Some((name, narrowed_ty)) =
                            self.extract_typeof_narrowing(&bin.left, &bin.right)
                        {
                            let alt_narrows =
                                self.compute_negated_typeof_narrowing(&name, &narrowed_ty);
                            return (vec![(name.clone(), narrowed_ty)], alt_narrows);
                        }
                        if let Some((name, narrowed_ty)) =
                            self.extract_typeof_narrowing(&bin.right, &bin.left)
                        {
                            let alt_narrows =
                                self.compute_negated_typeof_narrowing(&name, &narrowed_ty);
                            return (vec![(name.clone(), narrowed_ty)], alt_narrows);
                        }
                        // Loose `x == null` / `x == undefined` matches BOTH
                        // null and undefined, so the false branch removes both
                        // (`if (x == null) return; x.foo()` must narrow out
                        // undefined too). The strict `===` forms below stay
                        // per-type.
                        if matches!(bin.op, BinaryOp::Eq) {
                            if let Some(name) = self
                                .extract_null_equality(&bin.left, &bin.right)
                                .or_else(|| self.extract_null_equality(&bin.right, &bin.left))
                                .or_else(|| self.extract_undefined_equality(&bin.left, &bin.right))
                                .or_else(|| self.extract_undefined_equality(&bin.right, &bin.left))
                            {
                                return (
                                    self.keep_nullish_of_var(&name),
                                    self.remove_nullish_from_var(&name),
                                );
                            }
                        }
                        // x === null
                        if let Some(name) = self.extract_null_equality(&bin.left, &bin.right) {
                            let alt = self.remove_type_from_var(&name, &Type::Null);
                            return (self.exactly_nullish(&name, Type::Null), alt);
                        }
                        if let Some(name) = self.extract_null_equality(&bin.right, &bin.left) {
                            let alt = self.remove_type_from_var(&name, &Type::Null);
                            return (self.exactly_nullish(&name, Type::Null), alt);
                        }
                        // x === undefined
                        if let Some(name) = self.extract_undefined_equality(&bin.left, &bin.right) {
                            let alt = self.remove_type_from_var(&name, &Type::Undefined);
                            return (self.exactly_nullish(&name, Type::Undefined), alt);
                        }
                        if let Some(name) = self.extract_undefined_equality(&bin.right, &bin.left) {
                            let alt = self.remove_type_from_var(&name, &Type::Undefined);
                            return (self.exactly_nullish(&name, Type::Undefined), alt);
                        }
                        // Discriminated union: x.kind === "value" or x.kind === 42
                        if let Some((var_name, consequent_ty, alternate_ty)) =
                            self.extract_discriminant_narrowing(&bin.left, &bin.right)
                        {
                            return (
                                vec![(var_name.clone(), consequent_ty)],
                                vec![(var_name, alternate_ty)],
                            );
                        }
                        if let Some((var_name, consequent_ty, alternate_ty)) =
                            self.extract_discriminant_narrowing(&bin.right, &bin.left)
                        {
                            return (
                                vec![(var_name.clone(), consequent_ty)],
                                vec![(var_name, alternate_ty)],
                            );
                        }
                        // x === <literal>: consequent narrows x to the literal;
                        // the alternate EXCLUDES it — a single-literal x
                        // narrows to `never`, which exempts later comparisons
                        // (tsc's capturedLetConstInLoop shape: after
                        // `if (x == 1) break;` a '1'-typed const is never).
                        if let Some((name, lit_ty)) = self
                            .extract_ident_literal_equality(&bin.left, &bin.right)
                            .or_else(|| self.extract_ident_literal_equality(&bin.right, &bin.left))
                        {
                            let alt = self.remove_type_from_var(&name, &lit_ty);
                            return (vec![(name, lit_ty)], alt);
                        }
                        // `x?.p === <non-nullish>` proves `x` is non-nullish in
                        // the true branch: had `x` been nullish the chain would
                        // short-circuit to `undefined`, which cannot equal a
                        // value whose type admits neither `undefined` nor
                        // `null`. The pervasive event-handler guard
                        // `if (currentJob?.id === jobId) { currentJob.output … }`
                        // depends on this.
                        let eq_chain = self.optional_chain_equality_narrowings(bin);
                        if !eq_chain.is_empty() {
                            return (eq_chain, vec![]);
                        }
                    }
                    BinaryOp::StrictNe | BinaryOp::Ne => {
                        if let Some((equal, not_equal)) =
                            self.optional_chain_nullish_comparison(bin)
                        {
                            return (not_equal, equal);
                        }
                        // typeof x !== "string" narrows the consequent to the
                        // complement and the alternate to the named typeof
                        // type. This is the inverse of the equality case.
                        if let Some((name, narrowed_ty)) =
                            self.extract_typeof_narrowing(&bin.left, &bin.right)
                        {
                            let consequent =
                                self.compute_negated_typeof_narrowing(&name, &narrowed_ty);
                            return (consequent, vec![(name, narrowed_ty)]);
                        }
                        if let Some((name, narrowed_ty)) =
                            self.extract_typeof_narrowing(&bin.right, &bin.left)
                        {
                            let consequent =
                                self.compute_negated_typeof_narrowing(&name, &narrowed_ty);
                            return (consequent, vec![(name, narrowed_ty)]);
                        }
                        // Loose `x != null` / `x != undefined`: the TRUE branch
                        // removes both null and undefined (the common
                        // `if (x != null) { x.foo() }` guard).
                        if matches!(bin.op, BinaryOp::Ne) {
                            if let Some(name) = self
                                .extract_null_equality(&bin.left, &bin.right)
                                .or_else(|| self.extract_null_equality(&bin.right, &bin.left))
                                .or_else(|| self.extract_undefined_equality(&bin.left, &bin.right))
                                .or_else(|| self.extract_undefined_equality(&bin.right, &bin.left))
                            {
                                return (
                                    self.remove_nullish_from_var(&name),
                                    self.keep_nullish_of_var(&name),
                                );
                            }
                        }
                        // x !== null -> narrow out null in consequent
                        if let Some(name) = self.extract_null_equality(&bin.left, &bin.right) {
                            let consequent = self.remove_type_from_var(&name, &Type::Null);
                            return (consequent, self.exactly_nullish(&name, Type::Null));
                        }
                        if let Some(name) = self.extract_null_equality(&bin.right, &bin.left) {
                            let consequent = self.remove_type_from_var(&name, &Type::Null);
                            return (consequent, self.exactly_nullish(&name, Type::Null));
                        }
                        // x !== undefined
                        if let Some(name) = self.extract_undefined_equality(&bin.left, &bin.right) {
                            let consequent = self.remove_type_from_var(&name, &Type::Undefined);
                            return (consequent, self.exactly_nullish(&name, Type::Undefined));
                        }
                        if let Some(name) = self.extract_undefined_equality(&bin.right, &bin.left) {
                            let consequent = self.remove_type_from_var(&name, &Type::Undefined);
                            return (consequent, self.exactly_nullish(&name, Type::Undefined));
                        }
                        // Discriminated union (negated): x.kind !== "value"
                        if let Some((var_name, consequent_ty, alternate_ty)) =
                            self.extract_discriminant_narrowing(&bin.left, &bin.right)
                        {
                            // For !==, swap: consequent gets the non-matching, alternate gets matching
                            return (
                                vec![(var_name.clone(), alternate_ty)],
                                vec![(var_name, consequent_ty)],
                            );
                        }
                        if let Some((var_name, consequent_ty, alternate_ty)) =
                            self.extract_discriminant_narrowing(&bin.right, &bin.left)
                        {
                            return (
                                vec![(var_name.clone(), alternate_ty)],
                                vec![(var_name, consequent_ty)],
                            );
                        }
                        // `x?.p !== <non-nullish>`: the chain being non-nullish
                        // is proven on the FALSE branch (the `===` case),
                        // not the true one.
                        let eq_chain = self.optional_chain_equality_narrowings(bin);
                        if !eq_chain.is_empty() {
                            return (vec![], eq_chain);
                        }
                    }
                    // `"key" in x`: in the true branch `x` also has `key`
                    // (tsc narrows to `T & Record<"key", unknown>`), of type
                    // `unknown`.
                    BinaryOp::In => {
                        if let (ExprKind::StrLit(key), ExprKind::Ident(name)) =
                            (&bin.left.kind, &bin.right.kind)
                        {
                            if let Some(declared) = self.lookup_var(name).cloned() {
                                let known = !matches!(
                                    self.resolve_member_on_type(&declared, key),
                                    Type::Any | Type::Error | Type::Never
                                );
                                if !known
                                    && matches!(
                                        declared,
                                        Type::TypeReference(..) | Type::ObjectType(_)
                                    )
                                {
                                    // Recorded as a fact about the path `x.key`
                                    // (not about `x`), so it ends with the
                                    // branch.
                                    return (
                                        vec![(format!("{name}.{key}"), Type::Unknown)],
                                        vec![],
                                    );
                                }
                            }
                        }
                    }
                    BinaryOp::InstanceOf => {
                        // `x instanceof Foo` and `obj.x instanceof Foo`
                        // both narrow their LHS to Foo in the true branch.
                        // Property-path form (`if (params.dateFrom instanceof Date)`)
                        // is the dominant strict-null-check pattern in
                        // services-style code; without it the same dotted
                        // access keeps `Date | undefined` inside the guard
                        // and trips ~hundreds of TS2532 "Object is possibly
                        // undefined" diagnostics across the monorepo.
                        if let ExprKind::Ident(ref class_name) = bin.right.kind {
                            let lhs_name: Option<std::string::String> = match &bin.left.kind {
                                ExprKind::Ident(name) => Some(name.to_string()),
                                ExprKind::Member(mem) => {
                                    if let ExprKind::Ident(obj) = &mem.object.kind {
                                        Some(format!("{}.{}", obj, mem.property))
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            };
                            if let Some(name) = lhs_name {
                                let mut target = Type::TypeReference(
                                    class_name.to_string(),
                                    Arc::from([] as [Type; 0]),
                                );
                                // A type-parameter-typed operand narrows to
                                // `T & C` (tsc), staying assignable to `T`; in a
                                // union, parameter members intersect and other
                                // members stay when they could be instances.
                                if let Some(declared) = self.lookup_var(&name).cloned() {
                                    // A parameter whose constraint already relates
                                    // to the class stays `T` (tsc checkDerived).
                                    let narrow_param = |this: &Self, param: &Type| -> Type {
                                        let constrained = this
                                            .declared_type_param_constraint(param)
                                            .is_some_and(|c| this.is_assignable_to(&c, &target));
                                        if constrained {
                                            param.clone()
                                        } else {
                                            Type::Intersection(
                                                vec![param.clone(), target.clone()].into(),
                                            )
                                        }
                                    };
                                    if self.active_type_parameter_name(&declared).is_some() {
                                        target = narrow_param(self, &declared);
                                    } else if let Type::Union(members) = &declared {
                                        if members
                                            .iter()
                                            .any(|m| self.active_type_parameter_name(m).is_some())
                                        {
                                            // Non-parameter members survive only as
                                            // instances of the class or a derived
                                            // class (object shapes are dropped).
                                            let kept: Vec<Type> = members
                                                .iter()
                                                .filter_map(|m| {
                                                    if self.active_type_parameter_name(m).is_some()
                                                    {
                                                        Some(narrow_param(self, m))
                                                    } else if let Type::TypeReference(
                                                        member_class,
                                                        _,
                                                    ) = m
                                                    {
                                                        self.class_derives_from(
                                                            member_class,
                                                            class_name,
                                                        )
                                                        .then(|| m.clone())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .collect();
                                            if !kept.is_empty() {
                                                target = Type::flatten_union(kept);
                                            }
                                        }
                                    }
                                }
                                // A union naming the class itself with type
                                // arguments (`T | Promise<T>` against `Promise`)
                                // splits into those members and the rest.
                                if !name.contains('.') {
                                    if let Some(Type::Union(members)) = self.lookup_var(&name) {
                                        let is_instance = |member: &Type| {
                                            matches!(member, Type::TypeReference(member_class, args)
                                                if member_class == class_name.as_str()
                                                    && !args.is_empty())
                                        };
                                        let (instances, others): (Vec<Type>, Vec<Type>) =
                                            members.iter().cloned().partition(is_instance);
                                        if !instances.is_empty()
                                            && !others.is_empty()
                                            && !others.iter().any(|member| {
                                                matches!(member, Type::Any | Type::Unknown)
                                                    || self
                                                        .active_type_parameter_name(member)
                                                        .is_some()
                                            })
                                        {
                                            return (
                                                vec![(
                                                    name.clone(),
                                                    Type::flatten_union(instances),
                                                )],
                                                vec![(name, Type::flatten_union(others))],
                                            );
                                        }
                                    }
                                }
                                let alternate = self.remove_type_from_var(&name, &target);
                                return (vec![(name.clone(), target)], alternate);
                            }
                        }
                    }
                    BinaryOp::LogAnd => {
                        // `x && y` is truthy only when BOTH operands are truthy,
                        // so the consequent carries the union of each side's
                        // truthy facts. This lets a chained guard like
                        // `a.b && a.b.n && a.b.n.foo()` see every prior link
                        // when checking the next access. The false branch can't
                        // attribute the failure to either operand, so the
                        // alternate stays empty.
                        let (left_c, left_a) = self.analyze_narrowing(&bin.left);
                        let (right_c, right_a) = self.analyze_narrowing(&bin.right);
                        // Whichever operand was falsy, a path BOTH narrow on
                        // their false side is one of the two results:
                        // after `if (!a?.x && !a?.y) return`, `a` is defined.
                        let alternate = self.facts_of_either(left_a, &right_a);
                        let mut consequent = left_c;
                        for (name, ty) in right_c {
                            // Keep the LEFT fact on a shared path: it is the
                            // already-applied guard, and a bare `object` from a
                            // right-side `typeof x === "object"` must not
                            // clobber a concrete shape the left established
                            // (`x && typeof x === "object"` on a `{a?:T}` keeps
                            // `{a?:T}`, not `object`).
                            if consequent.iter().any(|(n, _)| *n == name) {
                                continue;
                            }
                            // `typeof x === "object"` yields a wide `object`; it
                            // must not narrow an `any`-typed target (TS keeps
                            // `any` there), else a valid `x as T[]` reads as a
                            // spurious TS2352.
                            if matches!(ty, Type::Object) && self.narrowing_path_is_any(&name) {
                                continue;
                            }
                            consequent.push((name, ty));
                        }
                        return (consequent, alternate);
                    }
                    BinaryOp::LogOr => {
                        // Dual of `&&`: `x || y` is FALSY only when BOTH
                        // operands are falsy, so the ALTERNATE carries the
                        // union of each side's falsy facts. This is what makes
                        // the pervasive early-exit guard
                        //   if (!row || !row.a) return;   // row, row.a defined
                        // narrow after the statement, and gives the `else`
                        // branch of `if (!x || cond)` its De Morgan facts
                        // (`!(!x || c)` ⇒ `x`). The truthy branch can't
                        // attribute success to either operand, so the
                        // consequent stays empty.
                        let (left_c, left_a) = self.analyze_narrowing(&bin.left);
                        let (right_c, right_a) = self.analyze_narrowing(&bin.right);
                        // Whichever operand was truthy, a path BOTH narrow is
                        // one of their two results:
                        // `a?.b?.x || a?.b?.y` proves `a.b` either way.
                        let consequent = self.facts_of_either(left_c, &right_c);
                        let mut alternate = left_a;
                        for (name, ty) in right_a {
                            if matches!(ty, Type::Object) && self.narrowing_path_is_any(&name) {
                                continue;
                            }
                            // BOTH facts hold on the falsy branch, so a shared
                            // path INTERSECTS them (unlike `&&`'s keep-left):
                            // `x === null || x === undefined` leaves
                            // `{string,undefined} ∩ {string,null}` = `string`.
                            if let Some(slot) = alternate.iter_mut().find(|(n, _)| *n == name) {
                                slot.1 = Self::intersect_narrowed(&slot.1, &ty);
                                continue;
                            }
                            alternate.push((name, ty));
                        }
                        return (consequent, alternate);
                    }
                    _ => {}
                }
            }
            // Truthiness narrowing: if (x) -> remove null/undefined from x
            ExprKind::Ident(ref name) => {
                if let Some(facts) = self.lookup_condition_alias(name) {
                    return facts.clone();
                }
                if let Some(ty) = self.lookup_var(name) {
                    let mut narrowed = self.remove_null_undefined(ty);
                    // `boolean` splits into its literals: `true` when the
                    // test passes, `false` otherwise.
                    let has_boolean = matches!(ty, Type::Boolean)
                        || matches!(ty, Type::Union(members) if members.iter().any(|m| matches!(m, Type::Boolean)));
                    let mut alternate = vec![];
                    if has_boolean {
                        narrowed = narrow_by_truthiness(&narrowed, true);
                        let falsy = narrow_by_truthiness(ty, false);
                        if falsy != *ty && !matches!(falsy, Type::Never) {
                            alternate.push((name.to_string(), falsy));
                        }
                    }
                    if narrowed != *ty || !alternate.is_empty() {
                        return (vec![(name.to_string(), narrowed)], alternate);
                    }
                }
            }
            // Truthiness narrowing on a property path: `if (obj.prop)` →
            // remove null/undefined from `obj.prop` for the consequent.
            // Property reads pick this up via `lookup_narrowed` in the
            // Member-arm of `check_expr`. Without this, services-style
            // code that defensively checks `if (rec.value) { use(rec.value) }`
            // sees the un-narrowed type and tripped TS2532 on every read.
            ExprKind::Member(mem) => {
                // An OPTIONAL-CHAIN access used as a truthiness condition
                // proves every link before the `?.` is non-nullish: if `x` were
                // null/undefined, `x?.y` would short-circuit to `undefined`
                // (falsy). So `if (x?.y)` / `x?.y ? … : …` narrows `x` (and
                // each intermediate link of `x?.a?.b`) to NonNullable in the
                // consequent. Without this the guarded body still saw
                // `x: T | undefined` and every `x.y` access inside was flagged
                // TS2532 — the dominant shape in the routers
                // (`if (filters?.status) { … filters.status … }`). The inline
                // object-type case happened to work through the path-narrowing
                // below; an aliased/computed base (`Filters | undefined`, a
                // Prisma model, a Zod `_output`) did not.
                let chain_narrows = self.optional_chain_root_narrowings(expr);

                // `this.prop` truthiness narrows the path on the enclosing
                // class's instance type (`this.parent ? this.parent.depth : 0`).
                if matches!(mem.object.kind, ExprKind::This) {
                    if let Some(class_name) = self.enclosing_class_names.last().cloned() {
                        let key = format!("this.{}", mem.property);
                        let class_ref = Type::TypeReference(
                            class_name,
                            std::sync::Arc::from(Vec::<Type>::new()),
                        );
                        let prop_ty = self.lookup_narrowed(&key).unwrap_or_else(|| {
                            match self.resolve_member_on_type(&class_ref, &mem.property) {
                                Type::Optional(inner) => {
                                    Type::flatten_union(vec![Type::clone(&inner), Type::Undefined])
                                }
                                other => other,
                            }
                        });
                        let narrowed = self.remove_null_undefined(&prop_ty);
                        if narrowed != prop_ty && !matches!(prop_ty, Type::Any | Type::Error) {
                            return (vec![(key, narrowed)], vec![]);
                        }
                    }
                }

                if let ExprKind::Ident(obj) = &mem.object.kind {
                    let key = format!("{}.{}", obj, mem.property);
                    // We can't `lookup_var` a dotted path; instead resolve
                    // the receiver's property type and strip nullables.
                    // A class name receives its static members.
                    let receiver = self.lookup_var(obj).cloned().or_else(|| {
                        self.class_info
                            .contains_key(obj.as_str())
                            .then(|| Type::Typeof(obj.as_str().into()))
                    });
                    if let Some(obj_ty) = receiver {
                        // A surrounding guard may already have narrowed this
                        // exact path. Compose from that fact rather than
                        // restoring the declared property union from `obj`.
                        let prop_ty = self.lookup_narrowed(&key).unwrap_or_else(|| {
                            // The property is read off the receiver's
                            // non-nullish part (`x?.p`, or `x.p` itself).
                            self.narrowing_member_type(
                                &self.remove_null_undefined(&obj_ty),
                                &mem.property,
                            )
                        });
                        let narrowed = self.remove_null_undefined(&prop_ty);

                        let mut consequent = chain_narrows.clone();
                        let mut alternate = Vec::new();
                        if narrowed != prop_ty {
                            consequent.push((key, narrowed));
                        }

                        // Property truthiness ALSO discriminates the receiver
                        // union itself — independently of whether the property
                        // had a nullable to strip. For
                        // `{ error: E; value?: never } |
                        //  { value: V; error?: never }`, the fall-through after
                        // `if (result.error) return` is the value branch. This
                        // equally covers a BOOLEAN-literal discriminant
                        // (`{success:true;data:T} | {success:false;error:E}`,
                        // i.e. zod's `safeParse` result): there `success` is
                        // `true`/`false` with no nullable to remove, so gating
                        // this on the strip above skipped it and
                        // `if (!r.success) throw; r.data` stayed possibly-
                        // undefined. Resolve the receiver to its apparent type
                        // first — the union commonly arrives as a generic alias
                        // reference (`ZodSafeParseResult<T>`).
                        let receiver_apparent = match &obj_ty {
                            Type::Union(_) => obj_ty.clone(),
                            other => self
                                .resolve_type_for_assignability(other)
                                .unwrap_or_else(|| other.clone()),
                        };
                        if let Type::Union(members) = &receiver_apparent {
                            let narrow_receiver = |is_truthy| {
                                let kept: Vec<Type> = members
                                    .iter()
                                    .filter(|member| {
                                        let member_prop =
                                            self.resolve_member_on_type(member, &mem.property);
                                        !matches!(
                                            narrow_by_truthiness(&member_prop, is_truthy),
                                            Type::Never
                                        )
                                    })
                                    .cloned()
                                    .collect();
                                Type::flatten_union(kept)
                            };
                            // A truthy `x.p` / `x?.p` also means `x` itself
                            // is neither `null` nor `undefined`.
                            let truthy_receiver =
                                self.remove_null_undefined(&narrow_receiver(true));
                            if truthy_receiver != receiver_apparent
                                && !matches!(truthy_receiver, Type::Never)
                            {
                                // One fact per name: a later entry for the
                                // same name would replace the chain's.
                                match consequent.iter_mut().find(|(name, _)| name == obj) {
                                    Some(slot) => slot.1 = truthy_receiver,
                                    None => consequent.push((obj.to_string(), truthy_receiver)),
                                }
                            }
                            let falsy_receiver = narrow_receiver(false);
                            if falsy_receiver != receiver_apparent
                                && !matches!(falsy_receiver, Type::Never)
                            {
                                alternate.push((obj.to_string(), falsy_receiver));
                            }
                        }

                        // Return only when THIS branch established something
                        // beyond the optional-chain root narrowings — otherwise
                        // fall through to the multi-level path logic below,
                        // which resolves `a.x` via `resolve_member_path_type`
                        // (that strips nullish BETWEEN segments and so succeeds
                        // where `resolve_member_on_type` on a `T | null`
                        // receiver returns a bare `Any`). Returning early here
                        // would drop that path narrowing.
                        if consequent.len() > chain_narrows.len() || !alternate.is_empty() {
                            return (consequent, alternate);
                        }
                    }
                }
                // Multi-level / optional-chain path (`a.b.n`, `a?.b?.c`): the
                // Ident-object branch above owns the single-level case (plus
                // its receiver-union discrimination); here we only narrow the
                // leaf path's own type by stripping null/undefined, which is
                // what the pervasive `a.b && a.b.n && a.b.n.foo()` guard needs
                // for its deeper links.
                if let Some(path) = Self::member_path(expr) {
                    if path.contains('.') {
                        if let Some(prop_ty) = self.resolve_member_path_type(&path) {
                            let narrowed = self.remove_null_undefined(&prop_ty);
                            if narrowed != prop_ty {
                                let mut consequent = chain_narrows.clone();
                                consequent.push((path, narrowed));
                                return (consequent, vec![]);
                            }
                        }
                    }
                }
                if !chain_narrows.is_empty() {
                    return (chain_narrows, vec![]);
                }
            }
            // Type guard call: if (isString(x)) -> narrow x to string
            ExprKind::Call(call) => {
                // A truthy optional-chain call proves that every receiver
                // before an optional link existed. The member-expression arm
                // handles `if (a?.b)`, but a call wraps that member as the
                // callee (`if (a?.b?.test())`) and otherwise hides the chain
                // from the narrowing entry point.
                let chain_narrows = self.optional_chain_root_narrowings(&call.callee);

                // Special case: `Array.isArray(x)` — its lib.dom signature
                // declares `x is any[]` but the builtin definition in
                // tsc_rs_types stores it as a plain `(arg: any) => boolean`
                // (no `TypePredicate` attached). Recognize the pattern
                // directly so user code that branches on `Array.isArray`
                // narrows the argument to an array in the true branch
                // and removes any array members from it in the false
                // branch. Without this `string | string[]` callers had
                // to manually cast after the guard.
                if let ExprKind::Member(ref mem) = call.callee.kind {
                    if let ExprKind::Ident(ref obj_name) = mem.object.kind {
                        let obj_s: &str = obj_name;
                        let prop_s: &str = &mem.property;
                        // `ArrayBuffer.isView(arg)` is declared by lib.es5 as
                        // `arg is ArrayBufferView`. Some lightweight checker
                        // paths model the builtin call as plain boolean and
                        // lose that predicate, so preserve the standard-library
                        // guard explicitly just as we do for Array.isArray.
                        if obj_s == "ArrayBuffer" && prop_s == "isView" {
                            if let Some(arg_name) =
                                call.args.first().and_then(|arg| Self::member_path(arg))
                            {
                                let target = Type::TypeReference(
                                    "ArrayBufferView".to_string(),
                                    Arc::from([] as [Type; 0]),
                                );
                                let mut consequent = chain_narrows.clone();
                                consequent.push((arg_name, target));
                                return (consequent, vec![]);
                            }
                        }
                        if obj_s == "Array" && prop_s == "isArray" {
                            if let Some(arg) = call.args.first() {
                                if let Some(arg_name) = Self::member_path(arg) {
                                    let is_member_path = arg_name.contains('.');
                                    // True branch: narrow to Array<Any>.
                                    // False branch: remove ALL array-typed
                                    // union members (any Array<T> and any
                                    // Tuple), not just an exact
                                    // `Array<Any>` match. Without the
                                    // permissive match, `string | string[]`
                                    // after `if (Array.isArray(x))`'s
                                    // else-branch kept `string[]`
                                    // alongside `string`, so a fallthrough
                                    // `return [x]` inferred
                                    // `(string | string[])[]` instead of
                                    // `string[]`.
                                    //
                                    // For a dotted path, retain the declared
                                    // array constituents instead:
                                    // `Array.isArray(config.items)` narrows
                                    // `T[] | undefined` to `T[]`, while
                                    // `Where | Where[] | undefined` narrows to
                                    // `Where[]`. Never replace a typed member
                                    // path with `any[]`: doing so pollutes
                                    // recursive object spreads downstream.
                                    let source_ty = if is_member_path {
                                        self.resolve_member_path_type(&arg_name)
                                    } else {
                                        self.lookup_var(&arg_name).cloned()
                                    };
                                    let (array_side, non_array_side) = source_ty
                                        .as_ref()
                                        .map(Self::partition_array_guard_type)
                                        .unwrap_or((None, None));
                                    let target = if is_member_path {
                                        array_side
                                    } else {
                                        Some(Type::Array(Arc::new(Type::Any)))
                                    };
                                    let Some(target) = target else {
                                        if !chain_narrows.is_empty() {
                                            return (chain_narrows, vec![]);
                                        }
                                        return (vec![], vec![]);
                                    };
                                    let alt = if let Some(non_array) = non_array_side {
                                        if source_ty.as_ref() != Some(&non_array) {
                                            vec![(arg_name.clone(), non_array)]
                                        } else {
                                            vec![]
                                        }
                                    } else {
                                        vec![]
                                    };
                                    let mut consequent = chain_narrows.clone();
                                    consequent.push((arg_name, target));
                                    return (consequent, alt);
                                }
                            }
                        }
                    }
                }
                let callee_ty = self.infer_expr_type(&call.callee);
                if let Type::Function(ref ft) = callee_ty {
                    if let Some(ref pred) = ft.type_predicate {
                        if !pred.is_asserts {
                            // Find which argument corresponds to the predicate param
                            if let Some(param_idx) =
                                ft.params.iter().position(|(n, _)| n == &pred.param_name)
                            {
                                if let Some(arg) = call.args.get(param_idx) {
                                    if let ExprKind::Ident(ref arg_name) = arg.kind {
                                        let target = Type::clone(&pred.target_type);
                                        // A type-parameter-typed argument narrows to
                                        // `T & Guard` (tsc), staying assignable to `T`.
                                        let target = match self.lookup_var(arg_name).cloned() {
                                            Some(declared)
                                                if self
                                                    .active_type_parameter_name(&declared)
                                                    .is_some() =>
                                            {
                                                Type::Intersection(vec![declared, target].into())
                                            }
                                            _ => target,
                                        };
                                        // In the true branch, narrow to target type
                                        // In the false branch, remove target type from the variable
                                        let alt = self.remove_type_from_var(arg_name, &target);
                                        let mut consequent = chain_narrows.clone();
                                        consequent.push((arg_name.to_string(), target));
                                        return (consequent, alt);
                                    }
                                    // `!isNil(a?.b?.c)`: a guard for `undefined`
                                    // that fails proves the chain did not
                                    // short-circuit, so every link before a
                                    // `?.` is non-nullish in the false branch.
                                    let guards_undefined = match pred.target_type.as_ref() {
                                        Type::Undefined => true,
                                        Type::Union(members) => members.contains(&Type::Undefined),
                                        _ => false,
                                    };
                                    if guards_undefined {
                                        let roots = self.optional_chain_root_narrowings(arg);
                                        if !roots.is_empty() {
                                            return (vec![], roots);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !chain_narrows.is_empty() {
                    return (chain_narrows, vec![]);
                }
            }
            // Logical negation reverses the condition's control-flow facts.
            // This is important for early-exit guards such as
            // `if (!value) return;`: the false branch of `!value` is the
            // truthy branch of `value`, and therefore applies after the
            // terminating consequent.
            ExprKind::Unary(un) if un.op == UnaryOp::LogNot => {
                let (consequent, alternate) = self.analyze_narrowing(&un.argument);
                return (alternate, consequent);
            }
            // `xs[0]` (a literal index on a nameable array): truthy means
            // that element is there.
            ExprKind::ElemAccess(access) => {
                if let Some(key) = Self::element_path(expr) {
                    let object = self.infer_expr_type(&access.object);
                    let object = match &object {
                        Type::TypeReference(..) => self
                            .resolve_type_for_assignability(&object)
                            .unwrap_or(object),
                        _ => object,
                    };
                    let object = self.remove_null_undefined(&object);
                    if let Type::Array(element) = &object {
                        let present = self.remove_null_undefined(element);
                        if !matches!(present, Type::Never | Type::Any) {
                            // With `a.b?.[0]`, the chain did not
                            // short-circuit either.
                            let mut facts = self.optional_chain_root_narrowings(expr);
                            facts.push((key, present));
                            return (facts, vec![]);
                        }
                    }
                }
                // `a?.b?.[0]` truthy: the chain did not short-circuit.
                let roots = self.optional_chain_root_narrowings(expr);
                if !roots.is_empty() {
                    return (roots, vec![]);
                }
            }
            ExprKind::Paren(inner) => return self.analyze_narrowing(inner),
            _ => {}
        }
        (vec![], vec![])
    }

    /// Dotted member path for an ident / non-computed member chain rooted at
    /// an identifier: `a` → "a", `a.b` → "a.b", `a?.b?.c` → "a.b.c". Returns
    /// None the moment a segment is computed (`a[i]`) or the root isn't a
    /// plain identifier, so only statically-nameable paths produce a key.
    /// Narrowing key of `path[<numeric literal>]` (`a.b[0]` → "a.b[0]"),
    /// for a statically nameable `path`.
    pub(crate) fn element_path(expr: &Expr) -> Option<std::string::String> {
        let ExprKind::ElemAccess(access) = &expr.kind else {
            return None;
        };
        let ExprKind::NumLit(index) = &access.index.kind else {
            return None;
        };
        let base = Self::member_path(&access.object)?;
        Some(format!("{base}[{index}]"))
    }

    pub(crate) fn member_path(expr: &Expr) -> Option<std::string::String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::This => Some("this".to_string()),
            ExprKind::Member(mem) => {
                let base = Self::member_path(&mem.object)?;
                Some(format!("{}.{}", base, mem.property))
            }
            _ => None,
        }
    }

    /// Split a declared type into the values accepted and rejected by
    /// `Array.isArray`. Optional wrappers contribute `undefined` to the false
    /// side, and nested unions are flattened recursively. Keeping the real
    /// array constituent (instead of manufacturing `any[]`) is essential for
    /// recursive members such as `Where | Where[]`.
    fn partition_array_guard_type(ty: &Type) -> (Option<Type>, Option<Type>) {
        fn collect(ty: &Type, arrays: &mut Vec<Type>, non_arrays: &mut Vec<Type>) {
            match ty {
                Type::Array(_) | Type::Tuple(_) => arrays.push(ty.clone()),
                Type::Optional(inner) => {
                    collect(inner, arrays, non_arrays);
                    non_arrays.push(Type::Undefined);
                }
                Type::Union(members) => {
                    for member in members.iter() {
                        collect(member, arrays, non_arrays);
                    }
                }
                other => non_arrays.push(other.clone()),
            }
        }

        let mut arrays = Vec::new();
        let mut non_arrays = Vec::new();
        collect(ty, &mut arrays, &mut non_arrays);
        let finish = |members: Vec<Type>| {
            if members.is_empty() {
                None
            } else {
                Some(Type::flatten_union(members))
            }
        };
        (finish(arrays), finish(non_arrays))
    }

    /// Whether a narrowing target (a var name or dotted member path) currently
    /// resolves to `any`. Used to keep `typeof x === "object"` from narrowing
    /// an `any` down to a bare `object` (TS leaves `any` as `any`).
    pub(crate) fn narrowing_path_is_any(&self, name: &str) -> bool {
        let ty = if name.contains('.') {
            self.resolve_member_path_type(name)
        } else {
            self.lookup_narrowed(name)
                .or_else(|| self.lookup_var(name).cloned())
        };
        matches!(ty, Some(Type::Any))
    }

    /// Resolve the type of a dotted member path (`a.b.c`) by walking from the
    /// root binding, stripping null/undefined between segments so an
    /// optional-chain path resolves to its leaf property's type. Returns None
    /// if the root isn't bound.
    pub(crate) fn resolve_member_path_type(&self, path: &str) -> Option<Type> {
        // Compose nested guards from the most specific fact already in
        // scope. For example, the false branch of
        // `Array.isArray(where.AND)` records `where.AND` as the non-array
        // constituent; the nested `where.AND ? ... : ...` must start from
        // that fact instead of re-resolving AND from `where` and restoring
        // the eliminated array constituent.
        if let Some(narrowed) = self.lookup_narrowed(path) {
            return Some(narrowed);
        }
        let mut segs = path.split('.');
        let root = segs.next()?;
        let mut cur = if root == "this" {
            // The enclosing class's instance.
            let class_name = self.enclosing_class_names.last()?;
            Type::TypeReference(class_name.clone(), Arc::from([] as [Type; 0]))
        } else {
            // A class name roots a path to its static members.
            self.lookup_narrowed(root)
                .or_else(|| self.lookup_var(root).cloned())
                .or_else(|| {
                    self.class_info
                        .contains_key(root)
                        .then(|| Type::Typeof(root.into()))
                })?
        };
        for seg in segs {
            let base = self.remove_null_undefined(&cur);
            // A type parameter has its constraint's members.
            let base = self.declared_type_param_constraint(&base).unwrap_or(base);
            cur = self.narrowing_member_type(&base, seg);
        }
        Some(cur)
    }

    /// A member's declared type for narrowing; a class value (`typeof C`)
    /// has its static members.
    fn narrowing_member_type(&self, receiver: &Type, property: &str) -> Type {
        let ty = self.resolve_member_on_type(receiver, property);
        if matches!(ty, Type::Any) {
            if let Type::TypeReference(name, _) = receiver {
                if let Some(class) = name.strip_prefix("typeof ") {
                    let member = self.class_info.get(class).and_then(|info| {
                        info.static_properties
                            .iter()
                            .find(|(name, _)| name == property)
                            .map(|(_, ty)| ty.clone())
                    });
                    if let Some(member) = member {
                        return match member {
                            Type::Optional(inner) => {
                                Type::flatten_union(vec![Type::clone(&inner), Type::Undefined])
                            }
                            other => other,
                        };
                    }
                }
            }
        }
        ty
    }

    /// Extract typeof narrowing: typeof x === "string" -> Some(("x", Type::String))
    pub(crate) fn extract_typeof_narrowing(
        &self,
        left: &Expr,
        right: &Expr,
    ) -> Option<(std::string::String, Type)> {
        let type_lit = if let ExprKind::StrLit(ref s) = right.kind {
            s.as_str()
        } else {
            return None;
        };
        let narrowed_ty = match type_lit {
            "string" => Type::String,
            "number" => Type::Number,
            "boolean" => Type::Boolean,
            "undefined" => Type::Undefined,
            "object" => Type::Object,
            "function" => Type::Function(FunctionType {
                type_param_constraints: Vec::new(),
                params: Vec::new(),
                return_type: Arc::new(Type::Any),
                type_params: Vec::new(),
                type_param_defaults: Vec::new(),
                type_predicate: None,
            }),
            "symbol" => Type::Symbol,
            "bigint" => Type::BigInt,
            _ => return None,
        };
        // Both `typeof x` (ExprKind::Typeof) and `typeof x` parsed as
        // `ExprKind::Unary(UnaryOp::Typeof)` reach us; the argument is
        // either an Ident or a single-level Member access (`obj.prop`).
        // Real-world uses include `typeof field.options === "string"` —
        // without member-path narrowing, the next `field.options`
        // reference still hits the un-narrowed union and triggers TS2769.
        let arg = match &left.kind {
            ExprKind::Typeof(inner) => Some(inner.as_ref()),
            ExprKind::Unary(un) if un.op == UnaryOp::Typeof => Some(un.argument.as_ref()),
            _ => None,
        }?;
        let name = match &arg.kind {
            ExprKind::Ident(n) => Some(n.to_string()),
            // `typeof this` narrows an explicit `this` parameter binding.
            ExprKind::This => Some("this".to_string()),
            ExprKind::Member(mem) => {
                if let ExprKind::Ident(obj) = &mem.object.kind {
                    Some(format!("{}.{}", obj, mem.property))
                } else {
                    None
                }
            }
            _ => None,
        }?;
        // A type-parameter-typed operand narrows to `T & primitive` (tsc),
        // staying assignable to `T`.
        let narrowed_ty = match self.lookup_var(&name).cloned() {
            Some(declared) if self.active_type_parameter_name(&declared).is_some() => {
                Type::Intersection(vec![declared, narrowed_ty].into())
            }
            _ => narrowed_ty,
        };
        Some((name, narrowed_ty))
    }

    pub(crate) fn compute_negated_typeof_narrowing(
        &self,
        name: &str,
        _narrowed_ty: &Type,
    ) -> Vec<(std::string::String, Type)> {
        // In the else branch, we remove the narrowed type from the variable's type
        if let Some(Type::Union(members)) = self.lookup_var(name) {
            let filtered: Vec<_> = members
                .iter()
                .filter(|m| m != &_narrowed_ty)
                .cloned()
                .collect();
            if !filtered.is_empty() && filtered.len() < members.len() {
                let result = Type::flatten_union(filtered);
                return vec![(name.to_string(), result)];
            }
        }
        vec![]
    }

    /// Check if expr is `ident === null`
    pub(crate) fn extract_null_equality(
        &self,
        ident_side: &Expr,
        lit_side: &Expr,
    ) -> Option<std::string::String> {
        if matches!(lit_side.kind, ExprKind::NullLit) {
            return Self::nullish_comparison_target(ident_side);
        }
        None
    }

    /// Check if expr is `ident === undefined`
    pub(crate) fn extract_undefined_equality(
        &self,
        ident_side: &Expr,
        lit_side: &Expr,
    ) -> Option<std::string::String> {
        if let ExprKind::Ident(ref rhs) = lit_side.kind {
            if rhs == "undefined" {
                return Self::nullish_comparison_target(ident_side);
            }
        }
        None
    }

    /// The facts that hold when one of two conditions held, without knowing
    /// which: a path both narrow, to the union of their results.
    fn facts_of_either(
        &self,
        left: Vec<(std::string::String, Type)>,
        right: &[(std::string::String, Type)],
    ) -> Vec<(std::string::String, Type)> {
        left.into_iter()
            .filter_map(|(name, left_ty)| {
                // `any` is not narrowed by a comparison.
                if self.narrowing_path_is_any(&name) {
                    return None;
                }
                let (_, right_ty) = right.iter().find(|(n, _)| *n == name)?;
                let ty = if left_ty == *right_ty {
                    left_ty
                } else {
                    Type::flatten_union(vec![left_ty, right_ty.clone()])
                };
                Some((name, ty))
            })
            .collect()
    }

    /// `a?.b <op> null|undefined`: the facts of the "equal" and the "not
    /// equal" outcome. When the chain's value is not `undefined` (strict), or
    /// not nullish (loose), the chain did not short-circuit, so every link
    /// before a `?.` is non-nullish there and the path itself loses the
    /// compared value. The "equal" outcome proves nothing about the links.
    fn optional_chain_nullish_comparison(
        &self,
        bin: &tsc_rs_ast::BinaryExpr,
    ) -> Option<(
        Vec<(std::string::String, Type)>,
        Vec<(std::string::String, Type)>,
    )> {
        let is_undefined =
            |expr: &Expr| matches!(&expr.kind, ExprKind::Ident(name) if name == "undefined");
        let is_null = |expr: &Expr| matches!(expr.kind, ExprKind::NullLit);
        let (target, literal) = if is_undefined(&bin.right) || is_null(&bin.right) {
            (&bin.left, &bin.right)
        } else if is_undefined(&bin.left) || is_null(&bin.left) {
            (&bin.right, &bin.left)
        } else {
            return None;
        };
        if !matches!(target.kind, ExprKind::Member(_)) || !Self::expr_in_optional_chain(target) {
            return None;
        }
        let loose = matches!(bin.op, BinaryOp::Eq | BinaryOp::Ne);
        let mut not_equal = if loose || is_undefined(literal) {
            self.optional_chain_root_narrowings(target)
        } else {
            Vec::new()
        };
        if let Some(path) = Self::member_path(target) {
            let leaf = if loose {
                self.remove_nullish_from_var(&path)
            } else if is_null(literal) {
                self.remove_type_from_var(&path, &Type::Null)
            } else {
                self.remove_type_from_var(&path, &Type::Undefined)
            };
            // The leaf is only reachable under its roots.
            if loose || is_undefined(literal) {
                not_equal.extend(leaf);
            }
        }
        Some((Vec::new(), not_equal))
    }

    /// What a comparison against `null`/`undefined` narrows: a variable, or
    /// a property path on one (`a.b.c`, `this.x`).
    fn nullish_comparison_target(expr: &Expr) -> Option<std::string::String> {
        match &expr.kind {
            ExprKind::Ident(name) => Some(name.to_string()),
            ExprKind::Member(_) => Self::member_path(expr),
            ExprKind::Paren(inner) => Self::nullish_comparison_target(inner),
            _ => None,
        }
    }

    /// The fact "`name` is exactly `nullish`" (`x === null`'s true branch).
    /// A property path only gets it when its type is known to admit that
    /// value; an unresolvable or `any` path stays as it is.
    fn exactly_nullish(&self, name: &str, nullish: Type) -> Vec<(std::string::String, Type)> {
        if name.contains('.') {
            let admits = self
                .nullish_comparison_source(name)
                .is_some_and(|ty| match &ty {
                    Type::Union(members) => members.contains(&nullish),
                    other => *other == nullish,
                });
            if !admits {
                return vec![];
            }
        }
        vec![(name.to_string(), nullish)]
    }

    /// The type a nullish comparison of `name` starts from: the variable's,
    /// or for a property path the most specific fact in scope.
    fn nullish_comparison_source(&self, name: &str) -> Option<Type> {
        if name.contains('.') {
            self.resolve_member_path_type(name)
                .filter(|ty| !matches!(ty, Type::Any | Type::Error))
        } else {
            self.lookup_var(name).cloned()
        }
    }

    /// Remove a specific type from a variable's union type
    /// `x === <num/str/bool literal>` where one side is a bare identifier and
    /// the other a literal expression: returns the identifier name and the
    /// literal's type for equality narrowing.
    pub(crate) fn extract_ident_literal_equality(
        &self,
        ident_side: &Expr,
        lit_side: &Expr,
    ) -> Option<(std::string::String, Type)> {
        let ExprKind::Ident(name) = &ident_side.kind else {
            return None;
        };
        let lit_ty = match &lit_side.kind {
            ExprKind::NumLit(n) => Type::NumberLiteral(n.to_string()),
            ExprKind::StrLit(s) => Type::StringLiteral(s.to_string()),
            ExprKind::BoolLit(b) => Type::BooleanLiteral(*b),
            _ => return None,
        };
        Some((name.to_string(), lit_ty))
    }

    pub(crate) fn remove_type_from_var(
        &self,
        name: &str,
        to_remove: &Type,
    ) -> Vec<(std::string::String, Type)> {
        if let Some(ty) = self.nullish_comparison_source(name).as_ref() {
            let result = match ty {
                Type::Union(members) => {
                    let filtered: Vec<_> = members
                        .iter()
                        .filter(|m| m != &to_remove)
                        .cloned()
                        .collect();
                    Type::flatten_union(filtered)
                }
                other if other == to_remove => Type::Never,
                other => other.clone(),
            };
            if result != *ty {
                return vec![(name.to_string(), result)];
            }
        }
        vec![]
    }

    /// Narrowing for loose `x == null` / `x != null`: remove BOTH null and
    /// undefined from `x`'s type (loose equality matches both). Returns the
    /// narrowing fact, or empty if it changes nothing.
    pub(crate) fn remove_nullish_from_var(&self, name: &str) -> Vec<(std::string::String, Type)> {
        if let Some(ty) = self.nullish_comparison_source(name).as_ref() {
            let result = self.remove_null_undefined(ty);
            if result != *ty {
                return vec![(name.to_string(), result)];
            }
        }
        vec![]
    }

    /// The complement of `remove_nullish_from_var`: narrow `x` to just its
    /// null/undefined members (the `x == null` true branch).
    pub(crate) fn keep_nullish_of_var(&self, name: &str) -> Vec<(std::string::String, Type)> {
        if let Some(ty) = self.nullish_comparison_source(name).as_ref() {
            let kept: Type = match ty {
                Type::Union(members) => Type::flatten_union(
                    members
                        .iter()
                        .filter(|m| matches!(m, Type::Null | Type::Undefined))
                        .cloned()
                        .collect(),
                ),
                Type::Null | Type::Undefined => ty.clone(),
                _ => Type::Undefined,
            };
            if kept != *ty {
                return vec![(name.to_string(), kept)];
            }
        }
        vec![]
    }

    /// Remove null and undefined from a type
    pub(crate) fn remove_null_undefined(&self, ty: &Type) -> Type {
        match ty {
            Type::Union(members) => {
                let filtered: Vec<_> = members
                    .iter()
                    .filter(|m| !matches!(m, Type::Null | Type::Undefined))
                    .cloned()
                    .collect();
                Type::flatten_union(filtered)
            }
            Type::Null | Type::Undefined => Type::Never,
            // An optional member's `T?` is `T | undefined`.
            Type::Optional(inner) => self.remove_null_undefined(inner),
            other => other.clone(),
        }
    }

    /// `ty` without `undefined` (`null` stays).
    pub(crate) fn remove_undefined_only(&self, ty: &Type) -> Type {
        match ty {
            Type::Union(members) => Type::flatten_union(
                members
                    .iter()
                    .filter(|member| !matches!(member, Type::Undefined))
                    .cloned()
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Check if a type contains null
    pub(crate) fn type_contains_null(&self, ty: &Type) -> bool {
        match ty {
            Type::Null => true,
            Type::Union(members) => members.iter().any(|m| matches!(m, Type::Null)),
            _ => false,
        }
    }

    /// Check if a type contains undefined
    pub(crate) fn type_contains_undefined(&self, ty: &Type) -> bool {
        match ty {
            Type::Undefined => true,
            Type::Union(members) => members.iter().any(|m| matches!(m, Type::Undefined)),
            _ => false,
        }
    }

    /// Emit TS2531/TS2532/TS2533 for nullable member access under strict null checks
    pub(crate) fn check_nullable_access(&mut self, ty: &Type, object: &Expr) {
        if !self.strict_null_checks || !self.has_control_flow_narrowing {
            return;
        }
        let has_null = self.type_contains_null(ty);
        let has_undefined = self.type_contains_undefined(ty);
        if !has_null && !has_undefined {
            return;
        }
        let span = object.span;
        // tsc (reportObjectPossiblyNullOrUndefinedError): an entity name —
        // an identifier or a dotted chain of identifiers, under 100 chars —
        // is named (`'a.b' is possibly 'undefined'`, TS18047-18049);
        // anything else is "Object is possibly …" (TS2531-2533).
        fn entity_name(expr: &Expr) -> Option<std::string::String> {
            match &expr.kind {
                ExprKind::Ident(name) => Some(name.to_string()),
                ExprKind::Member(member) if !member.property.starts_with('#') => Some(format!(
                    "{}.{}",
                    entity_name(&member.object)?,
                    member.property
                )),
                _ => None,
            }
        }
        if let Some(name) = entity_name(object).filter(|name| name.len() < 100) {
            self.diagnostics
                .push(crate::diagnostics::error_name_possibly_nullish(
                    &name,
                    has_null,
                    has_undefined,
                    span,
                ));
        } else if has_null && has_undefined {
            self.diagnostics
                .push(error_possibly_null_or_undefined(span));
        } else if has_null {
            self.diagnostics.push(error_possibly_null(span));
        } else {
            self.diagnostics.push(error_possibly_undefined(span));
        }
    }

    // -----------------------------------------------------------------------
    // Discriminated union narrowing helpers
    // -----------------------------------------------------------------------

    /// Extract discriminated union narrowing pattern: `x.prop === literal`
    /// Returns (variable_name, type_when_matches, type_when_not_matches).
    /// `member_side` is the side with the member access, `lit_side` is the literal.
    /// For a truthiness condition that is an optional-chain access
    /// (`x?.y`, `x?.a?.b`), the narrowings implied for the links BEFORE each
    /// `?.`: every one of them must be non-nullish for the whole expression to
    /// be truthy. Returns `(path, non_nullable_type)` pairs for the consequent
    /// branch — empty when the expression contains no optional link or nothing
    /// is nullable there.
    /// For `x?.p === value` (either operand order), the narrowings proving the
    /// optional chain's links are non-nullish. Only fires when the COMPARED
    /// value's type admits neither `undefined` nor `null` — otherwise
    /// `x?.p === undefined` would be true precisely when `x` IS nullish, and
    /// narrowing would be unsound. Empty when no side is an optional chain.
    pub(crate) fn optional_chain_equality_narrowings(
        &self,
        bin: &BinaryExpr,
    ) -> Vec<(std::string::String, Type)> {
        // The chain may run through calls and element accesses
        // (`o?.bar()`, `o?.["foo"]`).
        let contains_optional = |e: &Expr| -> bool {
            let mut cur = e;
            loop {
                match &cur.kind {
                    ExprKind::Member(m) => {
                        if m.optional {
                            return true;
                        }
                        cur = &m.object;
                    }
                    ExprKind::Call(c) => {
                        if c.optional {
                            return true;
                        }
                        cur = &c.callee;
                    }
                    ExprKind::ElemAccess(a) => {
                        if a.optional {
                            return true;
                        }
                        cur = &a.object;
                    }
                    _ => return false,
                }
            }
        };
        let value_is_non_nullish = |me: &Self, e: &Expr| -> bool {
            let t = me.infer_expr_type(e);
            !matches!(t, Type::Undefined | Type::Null | Type::Any | Type::Unknown)
                && !Self::type_includes_undefined(&t)
                && !matches!(&t, Type::Union(m) if m.iter().any(|x| matches!(x, Type::Null)))
        };
        for (chain, other) in [(&bin.left, &bin.right), (&bin.right, &bin.left)] {
            if contains_optional(chain) && value_is_non_nullish(self, other) {
                let n = self.optional_chain_root_narrowings(chain);
                if !n.is_empty() {
                    return n;
                }
            }
        }
        Vec::new()
    }

    /// Intersect two narrowings of the SAME path — used for `||`'s falsy
    /// branch, where both operands' facts hold at once. Union types keep only
    /// the members present in both; identical types pass through; anything
    /// else conservatively keeps the left (already-established) fact.
    pub(crate) fn intersect_narrowed(left: &Type, right: &Type) -> Type {
        if left == right {
            return left.clone();
        }
        match (left, right) {
            (Type::Union(l), Type::Union(r)) => {
                let kept: Vec<Type> = l
                    .iter()
                    .filter(|m| r.iter().any(|o| o == *m))
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    left.clone()
                } else {
                    Type::flatten_union(kept)
                }
            }
            // A non-union on one side is already at least as narrow.
            (Type::Union(l), other) => {
                if l.iter().any(|m| m == other) {
                    other.clone()
                } else {
                    left.clone()
                }
            }
            (other, Type::Union(r)) => {
                if r.iter().any(|m| m == other) {
                    other.clone()
                } else {
                    left.clone()
                }
            }
            _ => left.clone(),
        }
    }

    pub(crate) fn optional_chain_root_narrowings(
        &self,
        expr: &Expr,
    ) -> Vec<(std::string::String, Type)> {
        let mut out: Vec<(std::string::String, Type)> = Vec::new();
        let mut cur = expr;
        loop {
            // Each optional link (`?.`, `?.()`, `?.[]`) proves the expression
            // BEFORE it non-nullish; calls and element accesses are walked
            // through so `o?.bar()` and `o?.["foo"]` narrow `o` too.
            let (optional, object): (bool, &Expr) = match &cur.kind {
                ExprKind::Member(mem) => (mem.optional, &mem.object),
                ExprKind::Call(call) => (call.optional, &call.callee),
                ExprKind::ElemAccess(access) => (access.optional, &access.object),
                _ => break,
            };
            if optional {
                if let Some(prefix) = Self::member_path(object) {
                    let prefix_ty = match &object.kind {
                        ExprKind::Ident(n) => self.lookup_var(n).cloned(),
                        _ => self.resolve_member_path_type(&prefix),
                    };
                    if let Some(ty) = prefix_ty {
                        let nn = self.remove_null_undefined(&ty);
                        if nn != ty && !matches!(nn, Type::Never) {
                            if !out.iter().any(|(p, _)| p == &prefix) {
                                out.push((prefix, nn));
                            }
                        }
                    }
                }
            }
            cur = object;
        }
        out
    }

    pub(crate) fn extract_discriminant_narrowing(
        &self,
        member_side: &Expr,
        lit_side: &Expr,
    ) -> Option<(std::string::String, Type, Type)> {
        // member_side must be a member expression: x.prop
        let mem = match &member_side.kind {
            ExprKind::Member(m) => m,
            _ => return None,
        };
        // The object must be an identifier (the variable we narrow)
        let var_name = match &mem.object.kind {
            ExprKind::Ident(name) => name.to_string(),
            _ => return None,
        };
        let disc_prop = &mem.property;

        // lit_side must be a string literal or number literal
        let disc_value = match &lit_side.kind {
            ExprKind::StrLit(s) => Type::StringLiteral(s.to_string()),
            ExprKind::NumLit(n) => Type::NumberLiteral(n.to_string()),
            _ => return None,
        };

        // Look up the variable's type; it must be a union
        let var_ty = self.lookup_var(&var_name)?.clone();
        let apparent_ty = self
            .resolve_type_for_assignability(&var_ty)
            .unwrap_or(var_ty);
        let members = match &apparent_ty {
            Type::Union(m) => m.clone(),
            Type::ObjectType(_) | Type::TypeReference(_, _) => Arc::from([apparent_ty.clone()]),
            _ => return None,
        };

        let (matching, non_matching) =
            self.partition_union_by_discriminant(&members, disc_prop, &disc_value);

        let consequent_ty = Type::flatten_union(matching);
        let alternate_ty = Type::flatten_union(non_matching);

        Some((var_name, consequent_ty, alternate_ty))
    }

    /// Partition union members into those where `property` has type matching
    /// `disc_value` and those that do not.
    pub(crate) fn partition_union_by_discriminant(
        &self,
        members: &[Type],
        property: &str,
        disc_value: &Type,
    ) -> (Vec<Type>, Vec<Type>) {
        let mut matching = Vec::new();
        let mut non_matching = Vec::new();
        for member in members {
            let prop_ty = self.resolve_member_on_type(member, property);
            if self.discriminant_matches(&prop_ty, disc_value) {
                matching.push(member.clone());
            } else {
                non_matching.push(member.clone());
            }
        }
        (matching, non_matching)
    }

    /// Check if a property type matches a discriminant literal value.
    /// A StringLiteral("circle") matches StringLiteral("circle").
    /// A union containing StringLiteral("circle") also matches.
    pub(crate) fn discriminant_matches(&self, prop_ty: &Type, disc_value: &Type) -> bool {
        match prop_ty {
            ty if ty == disc_value => true,
            Type::Union(members) => members.iter().any(|m| m == disc_value),
            _ => false,
        }
    }

    /// Narrow a union type by filtering out members where `property` matches `disc_value`.
    /// Returns the matching union members as a type.
    pub(crate) fn narrow_union_by_discriminant(
        &self,
        union_members: &[Type],
        property: &str,
        disc_value: &Type,
    ) -> Type {
        let (matching, _) =
            self.partition_union_by_discriminant(union_members, property, disc_value);
        Type::flatten_union(matching)
    }

    /// For switch(x.prop), extract the variable name, its union members, and the
    /// discriminant property name.  Returns None if the discriminant is not a
    /// member access on a union-typed variable.
    pub(crate) fn extract_switch_discriminant_info(
        &self,
        discriminant: &Expr,
    ) -> Option<(std::string::String, Vec<Type>, std::string::String)> {
        let mut discriminant = discriminant;
        while let ExprKind::Paren(inner) = &discriminant.kind {
            discriminant = inner;
        }
        let mem = match &discriminant.kind {
            ExprKind::Member(m) => m,
            _ => return None,
        };
        let var_name = match &mem.object.kind {
            ExprKind::Ident(name) => name.to_string(),
            _ => return None,
        };
        let var_ty = self.lookup_var(&var_name)?.clone();
        let apparent_ty = self
            .resolve_type_for_assignability(&var_ty)
            .unwrap_or(var_ty);
        let members = match apparent_ty {
            Type::Union(m) => m,
            Type::ObjectType(_) | Type::TypeReference(_, _) => Arc::from([apparent_ty]),
            _ => return None,
        };
        Some((
            var_name,
            members.iter().cloned().collect(),
            mem.property.to_string(),
        ))
    }
}
