use super::*;

pub(crate) fn is_compilable_source_file(file: &TestFile, ctx: &BaselinePathContext) -> bool {
    let lower = file.name.to_ascii_lowercase();
    let is_declaration_like = lower.ends_with(".d.ts")
        || lower.ends_with(".d.tsx")
        || lower.ends_with(".d.mts")
        || lower.ends_with(".d.cts")
        || lower.ends_with(".d.js")
        || lower.ends_with(".d.jsx")
        || lower.ends_with(".d.mjs")
        || lower.ends_with(".d.cjs")
        || is_arbitrary_declaration_source_name(&file.name);
    let is_ts_like = (lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".mts")
        || lower.ends_with(".cts"))
        && !is_declaration_like
        && !is_node_modules_path(&file.name);
    let is_ts_like_node_modules = (lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".mts")
        || lower.ends_with(".cts"))
        && !is_declaration_like
        && is_node_modules_path(&file.name)
        // Keep historical behavior for tsconfig-style tests, but still allow
        // explicit non-tsconfig baselines that provide node_modules .ts files.
        && ctx.tsconfig_dir.is_none();
    let is_js_like = ctx.allow_js
        && (lower.ends_with(".js")
            || lower.ends_with(".jsx")
            || lower.ends_with(".mjs")
            || lower.ends_with(".cjs"))
        && !is_declaration_like
        && !is_node_modules_path(&file.name);
    if !is_ts_like && !is_js_like && !is_ts_like_node_modules {
        return false;
    }
    // Empty .tsx placeholders are commonly used in module-resolution tests and
    // should not produce standalone JS output sections.
    if (lower.ends_with(".tsx") || lower.ends_with(".jsx")) && file.content.trim().is_empty() {
        return false;
    }
    true
}

pub(crate) fn is_arbitrary_declaration_source_name(name: &str) -> bool {
    let normalized = normalize_header_path(name);
    let base = basename(&normalized).to_ascii_lowercase();
    for ext in [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.ends_with(".d") || stem.contains(".d.");
        }
    }
    false
}

pub(crate) fn is_js_like_source_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // .js, .mjs, .cjs are JavaScript source files that should NOT be
    // overwritten when compiling in-place. However, .jsx and .tsx contain
    // TypeScript/JSX that should be emitted (like .ts files).
    lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs")
}

pub(crate) fn should_emit_js_output_for_source(name: &str, ctx: &BaselinePathContext) -> bool {
    if !is_js_like_source_name(name) {
        return true;
    }
    // JS inputs only emit when redirected; otherwise TS would overwrite sources.
    // @suppressOutputPathCheck overrides this for test cases that want JS output
    // even without outDir.
    ctx.out_dir.is_some() || ctx.out_file.is_some() || ctx.suppress_output_path_check
}

pub(crate) fn should_emit_compilable_source_file(
    file: &TestFile,
    ctx: &BaselinePathContext,
) -> bool {
    if !is_compilable_source_file(file, ctx) {
        return false;
    }
    // For tsconfig-style tests, JS files that live outside the config root are
    // often present as ambient fixtures and should not be emitted as outputs.
    if ctx.tsconfig_dir.is_some()
        && is_js_like_source_name(&file.name)
        && !is_within_tsconfig_dir(&file.name, ctx)
    {
        return false;
    }
    true
}

pub(crate) fn is_within_tsconfig_dir(path: &str, ctx: &BaselinePathContext) -> bool {
    let Some(cfg_dir) = ctx.tsconfig_dir.as_deref() else {
        return true;
    };
    strip_prefix_path(&normalize_header_path(path), cfg_dir).is_some()
}

pub(crate) fn is_node_modules_path(name: &str) -> bool {
    let lower = normalize_header_path(name).to_ascii_lowercase();
    lower.starts_with("node_modules/") || lower.contains("/node_modules/")
}
