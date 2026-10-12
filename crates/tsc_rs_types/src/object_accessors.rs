//! Native declaration flags for getSpreadSymbol, adapted from pingdotgg/ts-rust
//! checker_p15.rs (see NOTICE.txt). A setter-only accessor retains its declared
//! type for ordinary access, but object spread reads an undefined value.

use crate::TypeChecker;
use tsc_rs_ast::{ObjLitProp, TypeMember, TypeMemberKind};

impl TypeChecker {
    pub(crate) fn declaration_setter_only_names(&self, members: &[TypeMember]) -> Vec<String> {
        members.iter().filter_map(|member| {
            let TypeMemberKind::SetAccessorSig(setter) = &member.kind else { return None; };
            let name = self.prop_name_to_string(&setter.name);
            (!members.iter().any(|member| matches!(&member.kind,
                TypeMemberKind::GetAccessorSig(getter) if self.prop_name_to_string(&getter.name) == name)))
                .then_some(name)
        }).collect()
    }

    pub(crate) fn object_literal_setter_only_names(
        &self,
        properties: &[ObjLitProp],
    ) -> Vec<String> {
        if !properties
            .iter()
            .any(|property| matches!(property, ObjLitProp::Set(_)))
        {
            return Vec::new();
        }
        // Object spread produces ordinary data properties. A later direct
        // accessor starts a new declaration for an incoming spread member.
        let mut accessors = rustc_hash::FxHashMap::<String, (bool, bool)>::default();
        for property in properties {
            match property {
                ObjLitProp::Set(setter) => {
                    accessors
                        .entry(self.prop_name_to_string(&setter.name))
                        .or_default()
                        .1 = true
                }
                ObjLitProp::Get(getter) => {
                    accessors
                        .entry(self.prop_name_to_string(&getter.name))
                        .or_default()
                        .0 = true
                }
                ObjLitProp::Property(property) => {
                    accessors.remove(&self.prop_name_to_string(&property.key));
                }
                ObjLitProp::Method(method) => {
                    accessors.remove(&self.prop_name_to_string(&method.name));
                }
                ObjLitProp::Shorthand(name, _) | ObjLitProp::ShorthandDefault(name, _, _) => {
                    accessors.remove(name.as_str());
                }
                ObjLitProp::Spread(expression, _) => {
                    if !accessors.is_empty() {
                        let mut copied = Vec::new();
                        let _ = self.merge_spread_into_object_properties(
                            &self.infer_expr_type(expression),
                            &mut copied,
                        );
                        for (name, _) in copied {
                            accessors.remove(&name);
                        }
                    }
                }
            }
        }
        let mut names: Vec<_> = accessors
            .into_iter()
            .filter_map(|(name, (getter, setter))| (setter && !getter).then_some(name))
            .collect();
        names.sort();
        names
    }
}
