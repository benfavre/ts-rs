//! Result recovery adapted from pingdotgg/ts-rust checker_p11.rs:
//! getCandidateForOverloadFailure and getLongestCandidateIndex. See NOTICE.txt.
use super::*;

impl TypeChecker {
    pub(super) fn failed_overload_return_type(
        &self,
        candidates: &[FunctionType],
        arguments: &[Type],
    ) -> Type {
        if candidates
            .iter()
            .all(|candidate| candidate.type_params.is_empty())
        {
            return crate::type_ops::simplify_intersection(
                candidates
                    .iter()
                    .map(|candidate| candidate.return_type.as_ref().clone())
                    .collect(),
            );
        }
        let candidate = candidates
            .iter()
            .find(|candidate| {
                candidate
                    .params
                    .iter()
                    .any(|(name, _)| name.starts_with("..."))
                    || candidate
                        .params
                        .iter()
                        .filter(|(name, _)| name != "this")
                        .count()
                        >= arguments.len()
            })
            .or_else(|| {
                candidates
                    .iter()
                    .max_by_key(|candidate| candidate.params.len())
            });
        let Some(candidate) = candidate else {
            return Type::Error;
        };
        let mut inferred = self.infer_type_arguments_for_call(
            arguments,
            &candidate.params,
            &candidate.type_params,
        );
        Self::apply_type_param_defaults(candidate, &mut inferred);
        for (index, name) in candidate.type_params.iter().enumerate() {
            if inferred
                .get(name)
                .is_none_or(|ty| matches!(ty, Type::Unknown | Type::Never))
            {
                let ty = candidate
                    .type_param_constraints
                    .get(index)
                    .and_then(Option::as_ref)
                    .map(|constraint| Self::substitute(constraint, &inferred))
                    .unwrap_or(Type::Unknown);
                inferred.insert(name.clone(), ty);
            }
        }
        Self::substitute(&candidate.return_type, &inferred)
    }
}
