use super::*;

pub(crate) fn normalized_out_file_name(
    ctx: &BaselinePathContext,
    source_hint: Option<&str>,
) -> Option<String> {
    let out_file = ctx.out_file.as_ref()?;
    let resolved = resolve_option_path(out_file, ctx, source_hint);
    let display = if ctx.full_emit_paths {
        resolved
    } else {
        basename(&resolved)
    };
    if display.is_empty() {
        None
    } else {
        Some(display)
    }
}

pub(crate) fn resolve_option_path(
    value: &str,
    ctx: &BaselinePathContext,
    source_hint: Option<&str>,
) -> String {
    let value = trim_trailing_slash_preserve_root(value);
    if is_absolute_path(&value) {
        return value;
    }
    if let Some(cfg_dir) = ctx.tsconfig_dir.as_deref() {
        return join_path(cfg_dir, &value);
    }
    let _ = source_hint;
    value
}

pub(crate) fn output_name_for_source_js(
    source_name: &str,
    jsx: Option<tsc_rs_ast::JsxEmit>,
    ctx: &BaselinePathContext,
) -> String {
    output_name_for_source(source_name, |name| ts_to_js_name_with_jsx(name, jsx), ctx)
}

pub(crate) fn output_name_for_source_dts(source_name: &str, ctx: &BaselinePathContext) -> String {
    output_name_for_source(source_name, ts_to_dts_name, ctx)
}

pub(crate) fn output_name_for_non_ts_source(
    source_name: &str,
    ctx: &BaselinePathContext,
) -> String {
    let normalized_source = normalize_header_path(source_name);
    let final_path = if ctx.out_dir.is_some() {
        let out_base = resolved_out_dir_base(source_name, ctx);
        let rel = source_relative_path(source_name, ctx);
        join_path(&out_base, &rel)
    } else {
        normalized_source
    };
    if ctx.full_emit_paths {
        normalize_header_path(&final_path)
    } else {
        basename(&final_path)
    }
}

pub(crate) fn output_name_for_source<F>(
    source_name: &str,
    to_output: F,
    ctx: &BaselinePathContext,
) -> String
where
    F: Fn(&str) -> String,
{
    let normalized_source = normalize_header_path(source_name);
    let final_path = if ctx.out_dir.is_some() {
        let out_base = resolved_out_dir_base(source_name, ctx);
        let rel = source_relative_path(source_name, ctx);
        let rel_out = to_output(&rel);
        join_path(&out_base, &rel_out)
    } else {
        to_output(&normalized_source)
    };
    if ctx.full_emit_paths {
        normalize_header_path(&final_path)
    } else {
        basename(&final_path)
    }
}

pub(crate) fn resolved_out_dir_base(source_name: &str, ctx: &BaselinePathContext) -> String {
    let out_dir = ctx.out_dir.as_deref().unwrap_or_default();
    resolve_option_path(out_dir, ctx, Some(source_name))
}

pub(crate) fn resolved_root_dir(source_name: &str, ctx: &BaselinePathContext) -> Option<String> {
    if let Some(root_dir) = ctx.root_dir.as_deref() {
        return Some(resolve_option_path(root_dir, ctx, Some(source_name)));
    }
    ctx.common_source_dir.clone()
}

pub(crate) fn source_relative_path(source_name: &str, ctx: &BaselinePathContext) -> String {
    let source = normalize_header_path(source_name);
    if let Some(root_dir) = resolved_root_dir(source_name, ctx) {
        if let Some(rel) = strip_prefix_path(&source, &root_dir) {
            if rel.is_empty() {
                return basename(&source);
            }
            return rel;
        }
    }
    if !is_absolute_path(&source) {
        return source;
    }
    basename(&source)
}

pub(crate) fn would_overwrite_input_source(
    source_name: &str,
    jsx: Option<tsc_rs_ast::JsxEmit>,
    test_case: &TestCase,
    ctx: &BaselinePathContext,
) -> bool {
    if ctx.out_dir.is_some() || ctx.out_file.is_some() || ctx.suppress_output_path_check {
        return false;
    }
    let source = normalize_header_path(source_name);
    let out_path = normalize_header_path(&ts_to_js_name_with_jsx(&source, jsx));
    let out_cmp = out_path.to_ascii_lowercase();
    test_case
        .files
        .iter()
        .any(|f| normalize_header_path(&f.name).to_ascii_lowercase() == out_cmp)
}

pub(crate) fn strip_known_module_extension(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    for ext in [
        ".d.ts", ".d.tsx", ".d.mts", ".d.cts", ".d.js", ".d.jsx", ".d.mjs", ".d.cjs", ".tsx",
        ".ts", ".mts", ".cts", ".jsx", ".js", ".mjs", ".cjs", ".json",
    ] {
        if lower.ends_with(ext) {
            return path[..path.len() - ext.len()].to_string();
        }
    }
    path.to_string()
}

pub(crate) fn source_relative_module_path(source_name: &str, ctx: &BaselinePathContext) -> String {
    let source = normalize_header_path(source_name);
    if let Some(root_dir) = resolved_root_dir(source_name, ctx) {
        if let Some(rel) = strip_prefix_path(&source, &root_dir) {
            if !rel.is_empty() {
                return rel;
            }
        }
    }

    // For AMD/System module names, use the emittable source dir (excludes
    // .d.ts) as TypeScript does in computeCommonSourceDirectoryOfFilesToEmit,
    // falling back to the full common source dir.
    let common_dir = ctx
        .common_emittable_source_dir
        .as_deref()
        .or(ctx.common_source_dir.as_deref());
    if let Some(common_dir) = common_dir {
        if let Some(rel) = strip_prefix_path(&source, common_dir) {
            if !rel.is_empty() {
                return rel;
            }
        }
    }

    let stripped = if is_windows_absolute(&source) {
        source[3..].to_string()
    } else {
        source.trim_start_matches('/').to_string()
    };
    if stripped.is_empty() {
        basename(&source)
    } else {
        stripped
    }
}

pub(crate) fn amd_module_id_for_source(source_name: &str, ctx: &BaselinePathContext) -> String {
    let rel = normalize_path_segments(&source_relative_module_path(source_name, ctx));
    let module_id = strip_known_module_extension(&rel);
    if module_id.is_empty() {
        strip_known_module_extension(&basename(source_name))
    } else {
        module_id
    }
}

/// Compute the full output path for a JS file (always absolute, ignoring full_emit_paths display setting).
pub(crate) fn full_output_path_for_source_js(
    source_name: &str,
    jsx: Option<tsc_rs_ast::JsxEmit>,
    ctx: &BaselinePathContext,
) -> String {
    let normalized_source = normalize_header_path(source_name);
    if ctx.out_dir.is_some() {
        let out_base = resolved_out_dir_base(source_name, ctx);
        let rel = source_relative_path(source_name, ctx);
        let rel_out = ts_to_js_name_with_jsx(&rel, jsx);
        normalize_header_path(&join_path(&out_base, &rel_out))
    } else {
        normalize_header_path(&ts_to_js_name_with_jsx(&normalized_source, jsx))
    }
}
