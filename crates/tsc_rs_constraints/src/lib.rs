//! Explicit constraint solver primitives.
//!
//! Constraints are first-class graph nodes that track *why* a type relationship
//! exists. This enables "explain why type X is Y" queries and forms the
//! foundation for constraint-based type inference in Phase 2.

use std::collections::{HashMap, HashSet, VecDeque};
use tsc_rs_ast::StableNodeId;

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConstraintId(pub u32);

// ---------------------------------------------------------------------------
// Constraint kinds & sources
// ---------------------------------------------------------------------------

/// The kind of type relationship a constraint represents.
#[derive(Debug, Clone)]
pub enum ConstraintKind {
    /// `source` is assignable to `target`.
    Assignable,
    /// `source extends target` (conditional type check).
    Extends,
    /// `source` is an instantiation of a generic `target`.
    Instantiation,
    /// `source` receives a contextual type from `target`.
    Contextual,
}

/// Where a constraint originated, for diagnostics and tracing.
#[derive(Debug, Clone)]
pub enum ConstraintSource {
    /// Constraint from an assignment expression or variable initializer.
    Assignment { node: StableNodeId },
    /// Constraint from a return statement matching a declared return type.
    Return { node: StableNodeId },
    /// Constraint from a function argument matching a parameter type.
    Argument { node: StableNodeId },
    /// Constraint from a conditional type `extends` clause.
    ConditionalExtends { node: StableNodeId },
    /// Constraint inferred by the type checker (no single source node).
    Inferred,
}

// ---------------------------------------------------------------------------
// Constraint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Constraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
    pub source: ConstraintSource,
    /// The source type (left-hand side of the relationship).
    pub source_type: u64,
    /// The target type (right-hand side of the relationship).
    pub target_type: u64,
}

// ---------------------------------------------------------------------------
// Constraint graph
// ---------------------------------------------------------------------------

/// A directed graph of type constraints.
pub struct ConstraintGraph {
    constraints: Vec<Constraint>,
    by_source_type: HashMap<u64, Vec<ConstraintId>>,
    by_target_type: HashMap<u64, Vec<ConstraintId>>,
    by_source_node: HashMap<StableNodeId, Vec<ConstraintId>>,
    next_id: u32,
}

impl Default for ConstraintGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ConstraintGraph {
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
            by_source_type: HashMap::new(),
            by_target_type: HashMap::new(),
            by_source_node: HashMap::new(),
            next_id: 0,
        }
    }

    /// Add a constraint to the graph.
    pub fn add(
        &mut self,
        kind: ConstraintKind,
        source: ConstraintSource,
        source_type: u64,
        target_type: u64,
    ) -> ConstraintId {
        let id = ConstraintId(self.next_id);
        self.next_id += 1;
        self.constraints.push(Constraint {
            id,
            kind,
            source,
            source_type,
            target_type,
        });
        self.by_source_type.entry(source_type).or_default().push(id);
        self.by_target_type.entry(target_type).or_default().push(id);
        if let Some(node) = constraint_source_node(&self.constraints[id.0 as usize].source) {
            self.by_source_node.entry(node).or_default().push(id);
        }
        id
    }

    /// Look up a constraint by id.
    pub fn get(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.id == id)
    }

    /// Return all constraints that mention a given type id (as source or target).
    pub fn constraints_for_type(&self, type_id: u64) -> Vec<&Constraint> {
        let mut ids = Vec::new();
        if let Some(outgoing) = self.by_source_type.get(&type_id) {
            ids.extend(outgoing.iter().copied());
        }
        if let Some(incoming) = self.by_target_type.get(&type_id) {
            ids.extend(incoming.iter().copied());
        }
        ids.sort_by_key(|id| id.0);
        ids.dedup_by_key(|id| id.0);
        ids.into_iter().filter_map(|id| self.get(id)).collect()
    }

    /// Return outgoing constraints for a specific source type id.
    pub fn outgoing(&self, type_id: u64) -> Vec<&Constraint> {
        self.by_source_type
            .get(&type_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.get(*id))
            .collect()
    }

    /// Return incoming constraints for a specific target type id.
    pub fn incoming(&self, type_id: u64) -> Vec<&Constraint> {
        self.by_target_type
            .get(&type_id)
            .into_iter()
            .flatten()
            .filter_map(|id| self.get(*id))
            .collect()
    }

    /// Return constraints that originated from a specific AST node.
    pub fn constraints_from_node(&self, node: StableNodeId) -> Vec<&Constraint> {
        self.by_source_node
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|id| self.get(*id))
            .collect()
    }

    /// Find a shortest explanation chain from one type id to another.
    ///
    /// Returns the sequence of constraints whose target/source links connect
    /// `source_type` to `target_type`.
    pub fn find_path(&self, source_type: u64, target_type: u64) -> Option<Vec<&Constraint>> {
        if source_type == target_type {
            return Some(Vec::new());
        }

        let mut queue = VecDeque::new();
        let mut visited_types = HashSet::new();
        let mut previous: HashMap<u64, (u64, ConstraintId)> = HashMap::new();

        queue.push_back(source_type);
        visited_types.insert(source_type);

        while let Some(current) = queue.pop_front() {
            for constraint in self.outgoing(current) {
                let next = constraint.target_type;
                if !visited_types.insert(next) {
                    continue;
                }
                previous.insert(next, (current, constraint.id));
                if next == target_type {
                    let mut path_ids = Vec::new();
                    let mut walk = target_type;
                    while let Some(&(prev, constraint_id)) = previous.get(&walk) {
                        path_ids.push(constraint_id);
                        if prev == source_type {
                            break;
                        }
                        walk = prev;
                    }
                    path_ids.reverse();
                    return Some(path_ids.into_iter().filter_map(|id| self.get(id)).collect());
                }
                queue.push_back(next);
            }
        }

        None
    }
}

fn constraint_source_node(source: &ConstraintSource) -> Option<StableNodeId> {
    match source {
        ConstraintSource::Assignment { node }
        | ConstraintSource::Return { node }
        | ConstraintSource::Argument { node }
        | ConstraintSource::ConditionalExtends { node } => Some(*node),
        ConstraintSource::Inferred => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_get_constraint() {
        let mut graph = ConstraintGraph::new();
        let id = graph.add(ConstraintKind::Assignable, ConstraintSource::Inferred, 1, 2);
        let c = graph.get(id).unwrap();
        assert_eq!(c.source_type, 1);
        assert_eq!(c.target_type, 2);
    }

    #[test]
    fn constraints_for_type_finds_both_directions() {
        let mut graph = ConstraintGraph::new();
        graph.add(
            ConstraintKind::Assignable,
            ConstraintSource::Inferred,
            10,
            20,
        );
        graph.add(ConstraintKind::Extends, ConstraintSource::Inferred, 30, 10);
        let result = graph.constraints_for_type(10);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn outgoing_and_incoming_are_indexed() {
        let mut graph = ConstraintGraph::new();
        graph.add(ConstraintKind::Assignable, ConstraintSource::Inferred, 1, 2);
        graph.add(ConstraintKind::Extends, ConstraintSource::Inferred, 1, 3);
        graph.add(ConstraintKind::Contextual, ConstraintSource::Inferred, 4, 2);

        assert_eq!(graph.outgoing(1).len(), 2);
        assert_eq!(graph.incoming(2).len(), 2);
    }

    #[test]
    fn constraints_from_node_finds_originating_constraints() {
        let mut graph = ConstraintGraph::new();
        let node = StableNodeId(42);
        graph.add(
            ConstraintKind::Assignable,
            ConstraintSource::Assignment { node },
            1,
            2,
        );
        graph.add(ConstraintKind::Extends, ConstraintSource::Inferred, 2, 3);

        let result = graph.constraints_from_node(node);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_type, 1);
        assert_eq!(result[0].target_type, 2);
    }

    #[test]
    fn find_path_traces_constraint_chain() {
        let mut graph = ConstraintGraph::new();
        graph.add(ConstraintKind::Assignable, ConstraintSource::Inferred, 1, 2);
        graph.add(ConstraintKind::Extends, ConstraintSource::Inferred, 2, 3);
        graph.add(ConstraintKind::Contextual, ConstraintSource::Inferred, 3, 4);

        let path = graph.find_path(1, 4).expect("expected path");
        assert_eq!(path.len(), 3);
        assert_eq!(path[0].source_type, 1);
        assert_eq!(path[2].target_type, 4);
    }
}
