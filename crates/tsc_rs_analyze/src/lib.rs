//! Static analysis tools for TypeScript monorepos.
//!
//! Provides import graph walking and dependency auditing without requiring
//! full type checking — only parsing and module resolution.

pub mod closure;
pub mod cycles;
pub mod dead_code;
pub mod externals;
pub mod import_graph;
pub mod opaque_imports;
pub mod package_deps;
pub mod utils;
