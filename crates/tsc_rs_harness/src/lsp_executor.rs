//! Executes LSP operations against the in-tree compiler for fourslash tests.
//!
//! Builds a QueryEngine + per-file analysis from the test's virtual files,
//! then runs hover/definition/completions/references at marker positions.

use std::collections::HashMap;

use tsc_rs_ast::{SourceFile, Span};
use tsc_rs_query::QueryEngine;
use tsc_rs_server::harness_api;
use tsc_rs_symbols::SymbolTable;
use tsc_rs_types::TypeCheckOutput;

use crate::lsp_parser::{LspTest, Marker};

// ---------------------------------------------------------------------------
// Per-file analysis result
// ---------------------------------------------------------------------------

pub struct FileAnalysis {
    pub source_file: SourceFile,
    pub sym_table: SymbolTable,
    pub check_output: TypeCheckOutput,
    pub source_text: String,
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// Build a QueryEngine and per-file analysis for the test.
///
/// Uses the QueryEngine's linked symbol tables (which have cross-file
/// import declarations resolved via `link_imports`) rather than
/// independently re-parsing and re-binding each file.
pub fn build_analysis(test: &LspTest) -> (QueryEngine, HashMap<String, FileAnalysis>) {
    let mut qe = QueryEngine::with_options(test.options.clone());

    // Add all files to the query engine
    for file in &test.files {
        let _ = qe.add_source(file.name.clone(), file.content.clone());
    }
    let _ = qe.check_all();

    // Build per-file analysis using QE's linked symbol tables when available
    let mut analyses = HashMap::new();
    for file in &test.files {
        let sf = tsc_rs_parser::parse(&file.name, &file.content);

        // Prefer the QE's linked symbol table (has cross-file declarations)
        // and check output. Fall back to independent parse/bind if not found.
        let (sym_table, check_output) = if let (Some(qe_syms), Some(qe_check)) = (
            qe.get_file_symbols(&file.name),
            qe.get_file_check_output(&file.name),
        ) {
            (qe_syms.clone(), qe_check)
        } else {
            let sym_table = tsc_rs_symbols::bind(&sf);
            let mut checker = tsc_rs_types::TypeChecker::new();
            checker.enable_control_flow_narrowing();
            let check_output = checker.check_with_options(&sf, &sym_table, &test.options);
            (sym_table, check_output)
        };

        analyses.insert(
            file.name.clone(),
            FileAnalysis {
                source_file: sf,
                sym_table,
                check_output,
                source_text: file.content.clone(),
            },
        );
    }

    (qe, analyses)
}

// ---------------------------------------------------------------------------
// Hover (QuickInfo)
// ---------------------------------------------------------------------------

/// Execute hover at a marker position.
pub fn hover_at_marker(
    qe: &QueryEngine,
    analyses: &HashMap<String, FileAnalysis>,
    marker: &Marker,
) -> Option<String> {
    let analysis = analyses.get(&marker.file_name)?;
    harness_api::hover_at(
        qe,
        &marker.file_name,
        &analysis.sym_table,
        &analysis.check_output,
        &analysis.source_text,
        marker.position,
    )
}

// ---------------------------------------------------------------------------
// GoToDefinition
// ---------------------------------------------------------------------------

/// Execute go-to-definition at a marker position.
pub fn definition_at_marker(
    qe: &QueryEngine,
    analyses: &HashMap<String, FileAnalysis>,
    marker: &Marker,
) -> Option<(String, Span)> {
    let analysis = analyses.get(&marker.file_name)?;
    // Build a project state from all analyzed files so cross-file
    // import definitions can be resolved.
    let project = if analyses.len() > 1 {
        let file_map: std::collections::HashMap<String, (String, tsc_rs_symbols::SymbolTable)> =
            analyses
                .iter()
                .filter(|(name, _)| *name != &marker.file_name)
                .map(|(name, fa)| (name.clone(), (fa.source_text.clone(), fa.sym_table.clone())))
                .collect();
        Some(harness_api::ProjectState::from_file_map(file_map))
    } else {
        None
    };
    harness_api::definition_at(
        qe,
        &analysis.sym_table,
        &analysis.source_file,
        marker.position,
        project.as_ref(),
        &analysis.check_output,
    )
}

// ---------------------------------------------------------------------------
// Completions
// ---------------------------------------------------------------------------

/// Execute completions at a marker position. Returns list of (label, kind).
pub fn completions_at_marker(
    qe: &QueryEngine,
    analyses: &HashMap<String, FileAnalysis>,
    marker: &Marker,
) -> Vec<(String, u64)> {
    completions_at_marker_with_prefs(qe, analyses, marker, false)
}

pub fn completions_at_marker_with_prefs(
    qe: &QueryEngine,
    analyses: &HashMap<String, FileAnalysis>,
    marker: &Marker,
    include_module_exports: bool,
) -> Vec<(String, u64)> {
    let Some(analysis) = analyses.get(&marker.file_name) else {
        return Vec::new();
    };
    let items = harness_api::completions_at_with_prefs(
        &analysis.sym_table,
        &analysis.check_output,
        &analysis.source_text,
        marker.position,
        qe,
        &marker.file_name,
        include_module_exports,
    );
    items
        .into_iter()
        .filter_map(|v| {
            let label = v.get("label")?.as_str()?.to_string();
            let kind = v.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);
            Some((label, kind))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// References
// ---------------------------------------------------------------------------

/// Execute find-all-references at a marker position.
pub fn references_at_marker(
    qe: &QueryEngine,
    analyses: &HashMap<String, FileAnalysis>,
    marker: &Marker,
) -> Vec<(String, Span)> {
    let Some(analysis) = analyses.get(&marker.file_name) else {
        return Vec::new();
    };
    // Build a project state from all other analyzed files so cross-file
    // references can be resolved.
    let project = if analyses.len() > 1 {
        let file_map: std::collections::HashMap<String, (String, tsc_rs_symbols::SymbolTable)> =
            analyses
                .iter()
                .filter(|(name, _)| *name != &marker.file_name)
                .map(|(name, fa)| (name.clone(), (fa.source_text.clone(), fa.sym_table.clone())))
                .collect();
        Some(harness_api::ProjectState::from_file_map(file_map))
    } else {
        None
    };
    harness_api::references_at(
        qe,
        &analysis.sym_table,
        &marker.file_name,
        marker.position,
        project.as_ref(),
    )
}

// ---------------------------------------------------------------------------
// Signature Help
// ---------------------------------------------------------------------------

/// Execute signature help at a marker position.
pub fn signature_help_at_marker(
    qe: &QueryEngine,
    analyses: &HashMap<String, FileAnalysis>,
    marker: &Marker,
) -> Option<serde_json::Value> {
    let analysis = analyses.get(&marker.file_name)?;
    harness_api::signature_help_at(
        qe,
        &marker.file_name,
        &analysis.source_file,
        &analysis.sym_table,
        &analysis.check_output,
        &analysis.source_text,
        marker.position,
    )
}
