use super::*;

pub(crate) fn parse_bool_option_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub(crate) fn option_flag_true(options: &CompilerOptions, key: &str) -> bool {
    options
        .other
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case(key) && parse_bool_option_value(v) == Some(true))
}

pub(crate) fn option_flag_bool(options: &CompilerOptions, key: &str) -> Option<bool> {
    options.other.iter().rev().find_map(|(k, v)| {
        k.eq_ignore_ascii_case(key)
            .then(|| parse_bool_option_value(v))
            .flatten()
    })
}

pub(crate) fn full_emit_paths(options: &CompilerOptions) -> bool {
    option_flag_true(options, "fullemitpaths")
}

pub(crate) fn baseline_path_context(test_case: &TestCase) -> BaselinePathContext {
    let tsconfig = find_tsconfig_file(test_case);
    let tsconfig_dir = tsconfig.map(|f| dirname(&f.name));
    let compiler_options_json =
        tsconfig.and_then(|f| extract_json_object(&f.content, "compilerOptions"));

    let out_dir = test_case
        .options
        .out_dir
        .clone()
        .or_else(|| {
            compiler_options_json
                .as_deref()
                .and_then(|j| extract_json_string(j, "outDir"))
        })
        .map(|s| normalize_header_path(&s))
        .filter(|s| !s.is_empty());
    let out_file = test_case
        .options
        .out_file
        .clone()
        .or_else(|| {
            compiler_options_json
                .as_deref()
                .and_then(|j| extract_json_string(j, "outFile"))
        })
        .map(|s| normalize_header_path(&s))
        .filter(|s| !s.is_empty());
    let root_dir = test_case
        .options
        .root_dir
        .clone()
        .or_else(|| {
            compiler_options_json
                .as_deref()
                .and_then(|j| extract_json_string(j, "rootDir"))
        })
        .map(|s| normalize_header_path(&s))
        .filter(|s| !s.is_empty());
    let root_dirs = compiler_options_json
        .as_deref()
        .and_then(|j| extract_json_string_array(j, "rootDirs"))
        .unwrap_or_default()
        .into_iter()
        .map(|raw| {
            let normalized = normalize_header_path(&raw);
            if is_absolute_path(&normalized) {
                normalized
            } else if let Some(cfg_dir) = tsconfig_dir.as_deref() {
                join_path(cfg_dir, &normalized)
            } else {
                normalized
            }
        })
        .map(|p| normalize_path_segments(&p))
        .collect::<Vec<_>>();
    let base_url = test_case
        .options
        .base_url
        .clone()
        .or_else(|| {
            compiler_options_json
                .as_deref()
                .and_then(|j| extract_json_string(j, "baseUrl"))
        })
        .map(|raw| normalize_header_path(&raw))
        .filter(|s| !s.is_empty())
        .map(|normalized| {
            if is_absolute_path(&normalized) {
                normalized
            } else if let Some(cfg_dir) = tsconfig_dir.as_deref() {
                normalize_path_segments(&join_path(cfg_dir, &normalized))
            } else {
                normalize_path_segments(&normalized)
            }
        });
    let paths = test_case.options.paths.clone().or_else(|| {
        compiler_options_json
            .as_deref()
            .and_then(|j| extract_json_object(j, "paths"))
    });
    let resolve_json_module = test_case.options.resolve_json_module == Some(true)
        || compiler_options_json
            .as_deref()
            .and_then(|j| extract_json_bool(j, "resolveJsonModule"))
            .unwrap_or(false);
    let emit_declaration_only = test_case.options.emit_declaration_only == Some(true)
        || compiler_options_json
            .as_deref()
            .and_then(|j| extract_json_bool(j, "emitDeclarationOnly"))
            .unwrap_or(false);
    let module_suffixes = module_suffixes_from_options(&test_case.options).or_else(|| {
        compiler_options_json
            .as_deref()
            .and_then(|j| extract_json_string_array(j, "moduleSuffixes"))
    });
    let allow_js = test_case.options.allow_js == Some(true)
        || test_case.options.check_js == Some(true)
        || compiler_options_json
            .as_deref()
            .and_then(|j| {
                extract_json_bool(j, "allowJs").or_else(|| extract_json_bool(j, "checkJs"))
            })
            .unwrap_or(false);
    let use_case_sensitive_file_names =
        option_flag_bool(&test_case.options, "useCaseSensitiveFileNames").or_else(|| {
            compiler_options_json
                .as_deref()
                .and_then(|j| extract_json_bool(j, "useCaseSensitiveFileNames"))
        });

    let include_node_modules_in_common = full_emit_paths(&test_case.options);
    let common_inputs: Vec<String> = test_case
        .files
        .iter()
        .filter(|f| {
            if include_node_modules_in_common {
                contributes_to_common_source_dir_with_options(&f.name, true)
            } else {
                contributes_to_common_source_dir(&f.name)
            }
        })
        .map(|f| normalize_header_path(&f.name))
        .filter(|name| {
            if let Some(cfg_dir) = tsconfig_dir.as_deref() {
                strip_prefix_path(name, cfg_dir).is_some()
            } else {
                true
            }
        })
        .collect();

    // Emittable inputs exclude .d.ts files — used for AMD/System bundled
    // module name computation (TypeScript's computeCommonSourceDirectoryOfFilesToEmit).
    let emittable_inputs: Vec<String> = common_inputs
        .iter()
        .filter(|name| {
            let lower = name.to_ascii_lowercase();
            !(lower.ends_with(".d.ts")
                || lower.ends_with(".d.tsx")
                || lower.ends_with(".d.mts")
                || lower.ends_with(".d.cts"))
        })
        .cloned()
        .collect();

    let no_resolve = test_case.options.no_resolve == Some(true)
        || option_flag_true(&test_case.options, "noresolve")
        || compiler_options_json
            .as_deref()
            .and_then(|j| extract_json_bool(j, "noResolve"))
            .unwrap_or(false);

    BaselinePathContext {
        emit_declaration_only,
        full_emit_paths: full_emit_paths(&test_case.options),
        resolve_json_module,
        allow_js,
        no_resolve,
        use_case_sensitive_file_names,
        out_dir,
        out_file,
        root_dir,
        root_dirs,
        base_url,
        paths,
        tsconfig_dir,
        common_source_dir: common_directory(&common_inputs),
        common_emittable_source_dir: common_directory(&emittable_inputs),
        module_suffixes: module_suffixes.unwrap_or_else(|| vec![String::new()]),
        suppress_output_path_check: option_flag_true(&test_case.options, "suppressOutputPathCheck"),
        base_module: effective_compiler_options(test_case).module,
        package_type_by_dir: package_json_module_type_by_dir(test_case),
    }
}

pub(crate) fn has_explicit_module_suffixes(ctx: &BaselinePathContext) -> bool {
    ctx.module_suffixes.len() > 1
        || ctx
            .module_suffixes
            .first()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
}

pub(crate) fn fixup_other_options(opts: &mut CompilerOptions) {
    // Pick up options that parse_option() in tsc_rs_ast doesn't map to typed
    // fields yet (they end up in `other`).
    if opts.inline_source_map.is_none() {
        opts.inline_source_map = option_flag_bool(opts, "inlinesourcemap");
    }
}

/// Fix multi-value test directives that `parse_option()` in tsc_rs_ast cannot
/// handle.  Directives like `// @module: commonjs, esnext` or
/// `// @jsx: react, preserve` pass a comma-separated string to the enum parser
/// which fails on the whole thing.  We re-scan the raw source and pick the
/// first valid value for `module`, `jsx`, and `moduleDetection`.
pub(crate) fn fixup_multi_value_options(test_case: &mut TestCase, source: &str) {
    // Only fix if the field is still None (i.e. parse_option() couldn't parse it).
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("//") {
            // Directives appear at the top of the file before any code.
            // However some tests have directives interspersed with @Filename,
            // so keep scanning.
            continue;
        }
        let after_slash = trimmed.trim_start_matches('/').trim();
        if !after_slash.starts_with('@') {
            continue;
        }
        let after_at = &after_slash[1..];
        let (key, value) = match after_at.split_once(':') {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim()),
            None => continue,
        };
        if !value.contains(',') {
            continue;
        }
        match key.as_str() {
            "module" if test_case.options.module.is_none() => {
                test_case.options.module = value
                    .split(',')
                    .filter_map(|v| ModuleKind::parse(v.trim()))
                    .next();
            }
            "jsx" if test_case.options.jsx.is_none() => {
                test_case.options.jsx = value
                    .split(',')
                    .filter_map(|v| JsxEmit::parse(v.trim()))
                    .next();
            }
            "moduledetection" if test_case.options.module_detection.is_none() => {
                test_case.options.module_detection =
                    value.split(',').map(|v| v.trim().to_string()).next();
            }
            _ => {}
        }
    }
}

pub(crate) fn effective_compiler_options(test_case: &TestCase) -> CompilerOptions {
    let mut opts = test_case.options.clone();
    let compiler_options_json = find_tsconfig_file(test_case)
        .and_then(|f| extract_json_object(&f.content, "compilerOptions"));
    let Some(json) = compiler_options_json.as_deref() else {
        fixup_other_options(&mut opts);
        return opts;
    };

    macro_rules! fill_bool {
        ($field:ident, $name:literal) => {
            if opts.$field.is_none() {
                opts.$field = extract_json_bool(json, $name);
            }
        };
    }
    macro_rules! fill_string {
        ($field:ident, $name:literal) => {
            if opts.$field.is_none() {
                opts.$field = extract_json_string(json, $name);
            }
        };
    }

    if opts.target.is_none() {
        opts.target = extract_json_string(json, "target").and_then(|v| ScriptTarget::parse(&v));
    }
    if opts.module.is_none() {
        opts.module = extract_json_string(json, "module").and_then(|v| ModuleKind::parse(&v));
    }
    if opts.jsx.is_none() {
        opts.jsx = extract_json_string(json, "jsx").and_then(|v| JsxEmit::parse(&v));
    }
    if opts.imports_not_used_as_values.is_none() {
        opts.imports_not_used_as_values = extract_json_string(json, "importsNotUsedAsValues")
            .and_then(|v| ImportsNotUsedAsValues::parse(&v));
    }

    fill_bool!(strict, "strict");
    fill_bool!(no_implicit_any, "noImplicitAny");
    fill_bool!(no_implicit_returns, "noImplicitReturns");
    fill_bool!(no_unused_locals, "noUnusedLocals");
    fill_bool!(no_unused_parameters, "noUnusedParameters");
    fill_bool!(strict_null_checks, "strictNullChecks");
    fill_bool!(strict_function_types, "strictFunctionTypes");
    fill_bool!(
        strict_property_initialization,
        "strictPropertyInitialization"
    );
    fill_bool!(no_emit, "noEmit");
    fill_bool!(declaration, "declaration");
    fill_bool!(source_map, "sourceMap");
    fill_bool!(inline_source_map, "inlineSourceMap");
    fill_bool!(allow_js, "allowJs");
    fill_bool!(check_js, "checkJs");
    fill_bool!(es_module_interop, "esModuleInterop");
    fill_bool!(
        allow_synthetic_default_imports,
        "allowSyntheticDefaultImports"
    );
    fill_bool!(skip_lib_check, "skipLibCheck");
    fill_bool!(skip_default_lib_check, "skipDefaultLibCheck");
    fill_bool!(no_lib, "noLib");
    fill_bool!(no_error_truncation, "noErrorTruncation");
    fill_bool!(experimental_decorators, "experimentalDecorators");
    fill_bool!(emit_decorator_metadata, "emitDecoratorMetadata");
    fill_bool!(use_define_for_class_fields, "useDefineForClassFields");
    fill_bool!(verbatim_module_syntax, "verbatimModuleSyntax");
    fill_bool!(no_check, "noCheck");
    fill_bool!(no_resolve, "noResolve");
    fill_bool!(down_level_iteration, "downlevelIteration");
    fill_bool!(import_helpers, "importHelpers");
    fill_bool!(emit_bom, "emitBOM");
    fill_bool!(remove_comments, "removeComments");
    fill_bool!(no_emit_helpers, "noEmitHelpers");
    fill_bool!(always_strict, "alwaysStrict");
    fill_bool!(exact_optional_property_types, "exactOptionalPropertyTypes");
    fill_bool!(isolated_modules, "isolatedModules");
    fill_bool!(preserve_const_enums, "preserveConstEnums");
    fill_bool!(resolve_json_module, "resolveJsonModule");
    fill_bool!(isolated_declarations, "isolatedDeclarations");
    fill_bool!(emit_declaration_only, "emitDeclarationOnly");
    fill_bool!(no_emit_on_error, "noEmitOnError");
    fill_bool!(incremental, "incremental");
    fill_bool!(composite, "composite");
    fill_bool!(
        force_consistent_casing_in_file_names,
        "forceConsistentCasingInFileNames"
    );
    fill_bool!(
        no_property_access_from_index_signature,
        "noPropertyAccessFromIndexSignature"
    );
    fill_bool!(use_unknown_in_catch_variables, "useUnknownInCatchVariables");

    fill_string!(out_file, "outFile");
    fill_string!(out_dir, "outDir");
    fill_string!(root_dir, "rootDir");
    fill_string!(module_resolution, "moduleResolution");
    fill_string!(base_url, "baseUrl");
    fill_string!(paths, "paths");
    fill_string!(new_line, "newLine");
    fill_string!(module_detection, "moduleDetection");
    fill_string!(jsx_factory, "jsxFactory");
    fill_string!(jsx_fragment_factory, "jsxFragmentFactory");
    fill_string!(ts_build_info_file, "tsBuildInfoFile");

    if opts.lib.is_empty() {
        if let Some(lib) = extract_json_string_array(json, "lib") {
            opts.lib = lib;
        }
    }
    if opts.types.is_none() {
        if let Some(types) = extract_json_string_array(json, "types") {
            opts.types = Some(types);
        }
    }

    // Extract mapRoot / sourceRoot from tsconfig.json into `other` so the
    // emitter can pick them up (it looks for them case-insensitively).
    // Only add if there is no existing entry from a `// @mapRoot:` directive.
    if !opts
        .other
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("maproot"))
    {
        if let Some(mr) = extract_json_string(json, "mapRoot") {
            opts.other.push(("maproot".to_string(), mr));
        }
    }
    if !opts
        .other
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("sourceroot"))
    {
        if let Some(sr) = extract_json_string(json, "sourceRoot") {
            opts.other.push(("sourceroot".to_string(), sr));
        }
    }

    // Preserve directive precedence for declaration options stored in `other`.
    if !opts
        .other
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("declarationDir"))
    {
        if let Some(value) = extract_json_string(json, "declarationDir") {
            opts.other.push(("declarationdir".into(), value));
        }
    }
    if !opts
        .other
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("declarationMap"))
    {
        if let Some(value) = extract_json_bool(json, "declarationMap") {
            opts.other
                .push(("declarationmap".into(), value.to_string()));
        }
    }

    // Extract ignoreDeprecations from tsconfig.json — needed for TS5101/TS5107 suppression
    if !opts
        .other
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("ignoredeprecations"))
    {
        if let Some(id) = extract_json_string(json, "ignoreDeprecations") {
            opts.other.push(("ignoredeprecations".to_string(), id));
        }
    }

    fixup_other_options(&mut opts);
    opts
}

pub(crate) fn package_json_module_type_by_dir(test_case: &TestCase) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for file in &test_case.files {
        if !basename(&file.name).eq_ignore_ascii_case("package.json") {
            continue;
        }
        let is_module = extract_json_string(&file.content, "type")
            .is_some_and(|pkg_type| pkg_type.eq_ignore_ascii_case("module"));
        let dir = dirname(&normalize_header_path(&file.name));
        map.insert(dir, is_module);
    }
    map
}

pub(crate) fn effective_module_kind_for_source(
    source_name: &str,
    base_module: Option<ModuleKind>,
    package_type_by_dir: &HashMap<String, bool>,
) -> Option<ModuleKind> {
    match base_module {
        Some(ModuleKind::Node16)
        | Some(ModuleKind::Node18)
        | Some(ModuleKind::Node20)
        | Some(ModuleKind::NodeNext) => {}
        _ => return base_module,
    }

    let lower = source_name.to_ascii_lowercase();
    if lower.ends_with(".mts") || lower.ends_with(".mjs") {
        return Some(ModuleKind::ESNext);
    }
    if lower.ends_with(".cts") || lower.ends_with(".cjs") {
        return Some(ModuleKind::CommonJS);
    }

    let mut dir = dirname(&normalize_header_path(source_name));
    loop {
        if let Some(is_module) = package_type_by_dir.get(&dir).copied() {
            return Some(if is_module {
                ModuleKind::ESNext
            } else {
                ModuleKind::CommonJS
            });
        }
        let parent = dirname(&dir);
        if parent == dir {
            break;
        }
        dir = parent;
    }
    Some(ModuleKind::CommonJS)
}

pub(crate) fn module_suffixes_from_options(options: &CompilerOptions) -> Option<Vec<String>> {
    let raw = options
        .other
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("modulesuffixes"))
        .map(|(_, v)| v.clone())?;
    parse_suffix_list(&raw)
}

pub(crate) fn parse_suffix_list(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let mut values = Vec::new();
        let mut rest = &trimmed[1..trimmed.len() - 1];
        while let Some(start) = rest.find('"').or_else(|| rest.find('\'')) {
            let quote = rest.as_bytes()[start] as char;
            rest = &rest[start + 1..];
            if let Some(end) = rest.find(quote) {
                values.push(rest[..end].to_string());
                rest = &rest[end + 1..];
            } else {
                break;
            }
        }
        if values.is_empty() {
            Some(vec![String::new()])
        } else {
            Some(values)
        }
    } else {
        let values: Vec<String> = trimmed
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .collect();
        if values.is_empty() {
            None
        } else {
            Some(values)
        }
    }
}

pub(crate) fn find_tsconfig_file(test_case: &TestCase) -> Option<&TestFile> {
    test_case
        .files
        .iter()
        .find(|f| basename(&f.name).eq_ignore_ascii_case("tsconfig.json"))
}

/// Try to find a parameterized baseline file when `{stem}.js` doesn't exist.
///
/// TypeScript tests with multi-value options like `// @target: ES5, ES2015`
/// produce separate baselines named `{stem}(target=es2015).js`, etc.
/// We prefer ES2015 since our emitter doesn't perform ES5 lowering.
/// When `opts` specifies a concrete module/jsx/target, baselines matching
/// those values are preferred.
pub(crate) fn find_parameterized_baseline(
    baseline_dir: &Path,
    stem: &str,
    opts: &CompilerOptions,
    actual_output: &str,
) -> (bool, String) {
    // Build option-matching strings from the compiled options so we can
    // prefer baselines that exactly match what we compiled with.
    let target_str = opts.target.map(|t| {
        format!(
            "target={}",
            match t {
                ScriptTarget::ES3 => "es3",
                ScriptTarget::ES5 => "es5",
                ScriptTarget::ES2015 => "es2015",
                ScriptTarget::ES2016 => "es2016",
                ScriptTarget::ES2017 => "es2017",
                ScriptTarget::ES2018 => "es2018",
                ScriptTarget::ES2019 => "es2019",
                ScriptTarget::ES2020 => "es2020",
                ScriptTarget::ES2021 => "es2021",
                ScriptTarget::ES2022 => "es2022",
                ScriptTarget::ES2023 => "es2023",
                ScriptTarget::ES2024 => "es2024",
                ScriptTarget::ES2025 => "es2025",
                ScriptTarget::ESNext => "esnext",
            }
        )
    });
    let module_str = opts.module.map(|m| {
        format!(
            "module={}",
            match m {
                ModuleKind::None => "none",
                ModuleKind::CommonJS => "commonjs",
                ModuleKind::AMD => "amd",
                ModuleKind::UMD => "umd",
                ModuleKind::System => "system",
                ModuleKind::ES2015 => "es2015",
                ModuleKind::ES2020 => "es2020",
                ModuleKind::ES2022 => "es2022",
                ModuleKind::ESNext => "esnext",
                ModuleKind::Node16 => "node16",
                ModuleKind::Node18 => "node18",
                ModuleKind::Node20 => "node20",
                ModuleKind::NodeNext => "nodenext",
                ModuleKind::Preserve => "preserve",
            }
        )
    });
    let jsx_str = opts.jsx.map(|j| {
        format!(
            "jsx={}",
            match j {
                JsxEmit::None => "none",
                JsxEmit::Preserve => "preserve",
                JsxEmit::React => "react",
                JsxEmit::ReactNative => "react-native",
                JsxEmit::ReactJSX => "react-jsx",
                JsxEmit::ReactJSXDev => "react-jsxdev",
            }
        )
    });
    let module_detect_str = opts
        .module_detection
        .as_deref()
        .map(|md| format!("moduledetection={}", md.to_lowercase()));
    let es_module_interop_str = opts
        .es_module_interop
        .map(|v| format!("esmoduleinterop={}", if v { "true" } else { "false" }));

    // Collect option strings for scoring (target, module, jsx, moduledetection,
    // esModuleInterop are the primary parameterization axes).
    let option_strs: Vec<&str> = [
        target_str.as_deref(),
        module_str.as_deref(),
        jsx_str.as_deref(),
        module_detect_str.as_deref(),
        es_module_interop_str.as_deref(),
    ]
    .iter()
    .filter_map(|o| *o)
    .collect();

    // Glob for any `{stem}(*.js` file.
    let pattern = format!("{}(", stem);
    let mut candidates: Vec<PathBuf> = if let Ok(entries) = std::fs::read_dir(baseline_dir) {
        entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    name.starts_with(&pattern) && name.ends_with(".js")
                } else {
                    false
                }
            })
            .collect()
    } else {
        return (false, String::new());
    };

    if candidates.is_empty() {
        return (false, String::new());
    }

    // Sort candidates by option-matching score, then tiebreakers.
    candidates.sort_by(|a, b| {
        let a_name = a.file_name().unwrap().to_str().unwrap();
        let b_name = b.file_name().unwrap().to_str().unwrap();
        let a_name_lower = a_name.to_lowercase();
        let b_name_lower = b_name.to_lowercase();

        // Use boundary-aware matching: option value must be followed by
        // `,`, `)`, or end of string so that e.g. `jsx=react` does not
        // spuriously match `jsx=react-jsx` or `jsx=react-jsxdev`.
        let match_score = |name: &str| -> usize {
            let mut score = 0;
            for opt in &option_strs {
                if let Some(pos) = name.find(opt) {
                    let after = pos + opt.len();
                    let boundary = after >= name.len()
                        || matches!(name.as_bytes().get(after), Some(b',' | b')' | b'.' | b' '));
                    if boundary {
                        score += 1;
                    }
                }
            }
            score
        };
        let a_score = match_score(&a_name_lower);
        let b_score = match_score(&b_name_lower);

        b_score
            .cmp(&a_score)
            .then_with(|| {
                let a_es5 = a_name_lower.contains("target=es5");
                let b_es5 = b_name_lower.contains("target=es5");
                a_es5.cmp(&b_es5)
            })
            .then_with(|| {
                let a_strict_false = a_name_lower.contains("alwaysstrict=false");
                let b_strict_false = b_name_lower.contains("alwaysstrict=false");
                a_strict_false.cmp(&b_strict_false)
            })
            .then_with(|| a_name.cmp(b_name))
    });

    // If there is actual output to compare against, try all candidates and
    // pick the one with the best combined score (option match + content match).
    // Candidates with conflicting option values get a large penalty to avoid
    // picking a baseline for a completely different compilation variant (e.g.,
    // jsx=preserve vs jsx=react).
    let actual_trimmed = actual_output.trim_end_matches('\n');
    if !actual_trimmed.is_empty() {
        let mut best_content = String::new();
        let mut best_adjusted = usize::MAX;
        let mut found_any = false;

        // Compute a penalty-aware match score for each candidate. A candidate
        // that actively conflicts with a compiled option (e.g., has
        // `jsx=preserve` when we compiled with `jsx=react`) gets penalized.
        let candidate_penalty = |name: &str| -> usize {
            let mut penalty = 0usize;
            for opt in &option_strs {
                // Extract the option key (e.g., "jsx" from "jsx=react").
                let key = opt.split('=').next().unwrap_or(opt);
                let key_prefix = format!("{}=", key);
                // Check if the candidate has this option key at all.
                if let Some(pos) = name.find(&key_prefix) {
                    // The candidate mentions this option; check if it matches.
                    if let Some(opos) = name.find(opt) {
                        let after = opos + opt.len();
                        let boundary = after >= name.len()
                            || matches!(
                                name.as_bytes().get(after),
                                Some(b',' | b')' | b'.' | b' ')
                            );
                        if !boundary {
                            // Value is a prefix but not boundary-matched (e.g.,
                            // `jsx=react` matching `jsx=react-jsx`): penalize.
                            penalty += 200;
                        }
                        // Else: exact match, no penalty.
                    } else {
                        // The candidate has the same option key but a different
                        // value (e.g., candidate has `jsx=preserve`, we want
                        // `jsx=react`): heavy penalty.
                        let _ = pos; // suppress unused warning
                        penalty += 200;
                    }
                }
                // If the candidate doesn't mention this key at all, no penalty
                // (allows fallback to variants with different axes).
            }
            penalty
        };

        for path in &candidates {
            if let Ok(raw) = std::fs::read_to_string(path) {
                let content = normalize_text(&raw);
                let stripped = strip_dts_sections(&content);
                let stripped_trimmed = stripped.trim_end_matches('\n');

                // Exact match — return immediately.
                if stripped_trimmed == actual_trimmed {
                    return (true, content);
                }

                // Count mismatched lines + option conflict penalty.
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let penalty = candidate_penalty(&name);
                let exp_lines: Vec<&str> = stripped_trimmed.lines().collect();
                let act_lines: Vec<&str> = actual_trimmed.lines().collect();
                let max_len = exp_lines.len().max(act_lines.len());
                let mut mismatches = max_len.saturating_sub(exp_lines.len().min(act_lines.len()));
                for i in 0..exp_lines.len().min(act_lines.len()) {
                    if exp_lines[i] != act_lines[i] {
                        mismatches += 1;
                    }
                }

                let adjusted = mismatches + penalty;
                if !found_any || adjusted < best_adjusted {
                    best_adjusted = adjusted;
                    best_content = content;
                    found_any = true;
                }
            }
        }

        if found_any {
            return (true, best_content);
        }
    }

    // Fallback: return the top-scored candidate without comparing content.
    if let Some(path) = candidates.first() {
        if let Ok(s) = std::fs::read_to_string(path) {
            return (true, normalize_text(&s));
        }
    }

    (false, String::new())
}

pub(crate) fn remove_exact_line(text: &str, target_trimmed: &str) -> String {
    let ends_with_newline = text.ends_with('\n');
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| line.trim() != target_trimmed)
        .collect();
    let mut out = kept.join("\n");
    if ends_with_newline && !out.is_empty() {
        out.push('\n');
    }
    out
}

pub(crate) fn merge_const_enum_object_values(
    out: &mut HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
    target_obj: &str,
    source_obj: &str,
    source_vals: &HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
) -> bool {
    let mut changed = false;
    for ((obj_name, member_name), value) in source_vals {
        if obj_name != source_obj {
            continue;
        }
        let key = (target_obj.to_string(), member_name.clone());
        if out.get(&key) != Some(value) {
            out.insert(key, value.clone());
            changed = true;
        }
    }
    changed
}

pub(crate) fn merge_const_enum_object_values_with_prefix(
    out: &mut HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
    target_prefix: &str,
    source_prefix: &str,
    source_vals: &HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
) -> bool {
    let mut changed = false;
    for ((obj_name, member_name), value) in source_vals {
        let target_obj = if obj_name == source_prefix {
            target_prefix.to_string()
        } else if let Some(rest) = obj_name.strip_prefix(source_prefix) {
            if !rest.starts_with('.') {
                continue;
            }
            let mut name = String::with_capacity(target_prefix.len() + rest.len());
            name.push_str(target_prefix);
            name.push_str(rest);
            name
        } else {
            continue;
        };
        let key = (target_obj, member_name.clone());
        if out.get(&key) != Some(value) {
            out.insert(key, value.clone());
            changed = true;
        }
    }
    changed
}

pub(crate) fn merge_const_enum_namespace_values(
    out: &mut HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
    namespace_obj: &str,
    source_vals: &HashMap<(String, String), tsc_rs_emitter::ConstEnumValue>,
) -> bool {
    let mut changed = false;
    for ((obj_name, member_name), value) in source_vals {
        if obj_name == "export=" {
            continue;
        }
        let mut target_obj = String::with_capacity(namespace_obj.len() + 1 + obj_name.len());
        target_obj.push_str(namespace_obj);
        target_obj.push('.');
        target_obj.push_str(obj_name);
        let key = (target_obj, member_name.clone());
        if out.get(&key) != Some(value) {
            out.insert(key, value.clone());
            changed = true;
        }
    }
    changed
}

pub(crate) fn path_from_expr(expr: &tsc_rs_ast::Expr) -> Option<String> {
    match &expr.kind {
        tsc_rs_ast::ExprKind::Ident(name) => Some(name.to_string()),
        tsc_rs_ast::ExprKind::Member(mem) => {
            let mut base = path_from_expr(&mem.object)?;
            base.push('.');
            base.push_str(&mem.property);
            Some(base)
        }
        tsc_rs_ast::ExprKind::Paren(inner) | tsc_rs_ast::ExprKind::NonNull(inner) => {
            path_from_expr(inner)
        }
        tsc_rs_ast::ExprKind::TypeAssertion(ta) => path_from_expr(&ta.expr),
        tsc_rs_ast::ExprKind::As(a) => path_from_expr(&a.expr),
        tsc_rs_ast::ExprKind::Satisfies(s) => path_from_expr(&s.expr),
        tsc_rs_ast::ExprKind::Instantiation(inst) => path_from_expr(&inst.expr),
        _ => None,
    }
}

// --- AMD wrapping functions ---

pub(crate) fn section_is_amd_wrapped_module(section: &str) -> bool {
    let mut remaining = section.trim_start();
    loop {
        if remaining.starts_with("//") {
            if let Some(pos) = remaining.find('\n') {
                remaining = remaining[pos + 1..].trim_start();
                continue;
            }
            return false;
        }
        if remaining.starts_with("/*") {
            if let Some(end) = remaining.find("*/") {
                remaining = remaining[end + 2..].trim_start();
                continue;
            }
            return false;
        }
        break;
    }
    if remaining.starts_with("define(") {
        return true;
    }
    // AMD sections may have top-level helper declarations (var __createBinding = ...)
    // before the define() call.  Check if define() appears anywhere in the section.
    // A line starting with `define(` after helpers still makes this an AMD module.
    for line in remaining.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("define(") {
            return true;
        }
    }
    false
}

pub(crate) fn normalize_amd_dep_specifier(spec: &str, module_id: &str) -> String {
    if spec == "require" || spec == "exports" || spec == "module" {
        return spec.to_string();
    }

    let mut normalized = if spec.starts_with("./") || spec.starts_with("../") {
        let base_dir = dirname(module_id);
        if base_dir.is_empty() {
            normalize_path_segments(spec)
        } else {
            join_path(&base_dir, spec)
        }
    } else if spec.starts_with('/') {
        spec.trim_start_matches('/').to_string()
    } else {
        spec.to_string()
    };

    if spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/') {
        normalized = strip_known_module_extension(&normalized);
        normalized = normalize_path_segments(&normalized);
        while normalized.starts_with("./") {
            normalized = normalized[2..].to_string();
        }
    }

    normalized
}

pub(crate) fn parse_quoted_string_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote != b'"' && quote != b'\'' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        while i < bytes.len() {
            if bytes[i] == quote {
                let mut backslashes = 0usize;
                let mut j = i;
                while j > start && bytes[j - 1] == b'\\' {
                    backslashes += 1;
                    j -= 1;
                }
                if backslashes.is_multiple_of(2) {
                    out.push(text[start..i].to_string());
                    i += 1;
                    break;
                }
            }
            i += 1;
        }
    }
    out
}

pub(crate) fn find_matching_char(text: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = text.as_bytes();
    if start >= bytes.len() || bytes[start] != open {
        return None;
    }
    let mut depth = 0i32;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == quote {
                        let mut backslashes = 0usize;
                        let mut j = i;
                        while j > 0 && bytes[j - 1] == b'\\' {
                            backslashes += 1;
                            j -= 1;
                        }
                        if backslashes.is_multiple_of(2) {
                            break;
                        }
                    }
                    i += 1;
                }
            }
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

pub(crate) fn normalize_amd_define_section(section: &str, module_id: &str) -> String {
    let Some(define_idx) = section.find("define(") else {
        return section.to_string();
    };

    let mut cursor = define_idx + "define(".len();
    let bytes = section.as_bytes();
    let mut explicit_define_name: Option<String> = None;
    while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= bytes.len() {
        return section.to_string();
    }

    if bytes[cursor] == b'"' || bytes[cursor] == b'\'' {
        let quote = bytes[cursor];
        cursor += 1;
        let name_start = cursor;
        while cursor < bytes.len() {
            if bytes[cursor] == quote {
                let mut backslashes = 0usize;
                let mut j = cursor;
                while j > 0 && bytes[j - 1] == b'\\' {
                    backslashes += 1;
                    j -= 1;
                }
                if backslashes.is_multiple_of(2) {
                    explicit_define_name = Some(section[name_start..cursor].to_string());
                    cursor += 1;
                    break;
                }
            }
            cursor += 1;
        }
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b',' {
            cursor += 1;
        }
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
    }

    if cursor >= bytes.len() || bytes[cursor] != b'[' {
        return section.to_string();
    }
    let Some(dep_end) = find_matching_char(section, cursor, b'[', b']') else {
        return section.to_string();
    };

    let deps = parse_quoted_string_list(&section[cursor + 1..dep_end]);
    let normalized_deps: Vec<String> = deps
        .iter()
        .map(|dep| format!("\"{}\"", normalize_amd_dep_specifier(dep, module_id)))
        .collect();
    let deps_text = normalized_deps.join(", ");
    let define_name = explicit_define_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(module_id);

    let mut out = String::new();
    out.push_str(&section[..define_idx]);
    out.push_str(&format!("define(\"{define_name}\", [{deps_text}]"));
    out.push_str(&section[dep_end + 1..]);
    out
}

pub(crate) fn emit_amd_json_module_section(module_id: &str, content: &str) -> String {
    let json = content.trim_end_matches('\n');
    if json.is_empty() {
        format!("define(\"{module_id}\", [], {{}});\n")
    } else {
        format!("define(\"{module_id}\", [], {json});\n")
    }
}

// --- Prologue/directive preservation functions ---

pub(crate) fn collect_source_prologue_directives(source: &str) -> (Option<String>, Vec<String>) {
    let mut lines = source.lines();
    let mut shebang = None;
    let mut directives = Vec::new();

    if let Some(first) = lines.next() {
        if first.starts_with("#!") {
            shebang = Some(first.to_string());
        } else {
            let trimmed = first.trim_start();
            if (trimmed.starts_with("///") || trimmed.starts_with("///<"))
                && (trimmed.contains("<reference") || trimmed.contains("<amd-"))
            {
                directives.push(first.trim_end().to_string());
            } else if !trimmed.is_empty()
                && !trimmed.starts_with("//")
                && !trimmed.starts_with("/*")
            {
                return (shebang, directives);
            }
        }
    }

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            if (trimmed.starts_with("///") || trimmed.starts_with("///<"))
                && (trimmed.contains("<reference") || trimmed.contains("<amd-"))
            {
                directives.push(line.trim_end().to_string());
            }
            continue;
        }
        if trimmed.starts_with("/*") {
            continue;
        }
        break;
    }

    (shebang, directives)
}

pub(crate) fn preserve_source_js_prologue(
    source: &str,
    emitted: &str,
    remove_comments: bool,
    source_is_module: bool,
) -> String {
    let (shebang, mut directives) = collect_source_prologue_directives(source);
    let source_has_explicit_use_strict = source_has_explicit_use_strict_prologue(source);
    if remove_comments {
        directives.clear();
    }
    if shebang.is_none() && directives.is_empty() {
        return emitted.to_string();
    }

    let mut out = emitted.to_string();

    if let Some(sb) = shebang {
        if !out.starts_with("#!") {
            out = format!("{sb}\n{out}");
        }
    }

    if directives.is_empty() {
        return out;
    }

    // AMD/UMD output strips all reference path directives from JS output.
    let is_amd_or_umd = out.trim_start().starts_with("define(")
        || out.trim_start().starts_with("(function (factory)");

    let mut missing: Vec<String> = Vec::new();
    let mut kept_directives: Vec<String> = Vec::new();
    for directive in directives.iter().cloned() {
        let directive_trimmed = directive.trim().to_string();
        if directive_trimmed.contains("<amd-module") && out.contains("System.register(") {
            out = remove_exact_line(&out, &directive_trimmed);
            continue;
        }
        // AMD/UMD output does not preserve reference path directives.
        if is_amd_or_umd && parse_triple_slash_reference(&directive).is_some() {
            out = remove_exact_line(&out, &directive_trimmed);
            continue;
        }
        if source_is_module {
            if let Some(path) = parse_triple_slash_reference(&directive) {
                if !should_preserve_module_reference_directive(&path, source, &out) {
                    out = remove_exact_line(&out, &directive_trimmed);
                    continue;
                }
            }
        }
        kept_directives.push(directive.clone());
        let exists = out.lines().any(|line| line.trim() == directive.trim());
        if !exists {
            missing.push(directive);
        }
    }

    let mut normalized = maybe_reorder_declaration_reference_prologue(
        &out,
        &kept_directives,
        source_is_module,
        source_has_explicit_use_strict,
    );
    normalized = maybe_reorder_prologue_directives(&normalized, &kept_directives);
    if normalized != out {
        out = normalized;
        missing = kept_directives
            .iter()
            .filter(|directive| !out.lines().any(|line| line.trim() == directive.trim()))
            .cloned()
            .collect();
    }

    if missing.is_empty() {
        return out;
    }

    let mut insert_at = 0usize;
    if out.starts_with("#!") {
        if let Some(pos) = out.find('\n') {
            insert_at = pos + 1;
        }
    }
    if out[insert_at..].starts_with("\"use strict\";\n") && !source_has_explicit_use_strict {
        insert_at += "\"use strict\";\n".len();
        if source_is_module {
            const CJS_ESMODULE_MARKER: &str =
                "Object.defineProperty(exports, \"__esModule\", { value: true });\n";
            // Use find() instead of starts_with() to skip past any CJS helper
            // function blocks (var __createBinding, var __importStar, etc.)
            // that appear between "use strict" and __esModule.
            if let Some(esmod_offset) = out[insert_at..].find(CJS_ESMODULE_MARKER) {
                insert_at += esmod_offset + CJS_ESMODULE_MARKER.len();
            }
            // Skip past the CJS exports prologue: `exports.X = void 0;` and
            // `exports.X = name;` lines that immediately follow __esModule.
            while let Some(nl) = out[insert_at..].find('\n') {
                let line = &out[insert_at..insert_at + nl];
                if line.starts_with("exports.") && line.ends_with(';') {
                    insert_at += nl + 1;
                } else {
                    break;
                }
            }
        }
    }

    // If the output beyond the prologue is empty (only whitespace/newlines)
    // or is just a bare module marker (`export {};`), there are no real runtime
    // statements.  Do not insert directives because TypeScript strips them when
    // the file body is entirely erased.
    let remaining_after_prologue = out[insert_at..].trim();
    if remaining_after_prologue.is_empty() || remaining_after_prologue == "export {};" {
        return out;
    }

    let mut rebuilt = String::new();
    rebuilt.push_str(&out[..insert_at]);
    if !rebuilt.is_empty() && !rebuilt.ends_with('\n') {
        rebuilt.push('\n');
    }
    for directive in missing {
        rebuilt.push_str(&directive);
        rebuilt.push('\n');
    }
    rebuilt.push_str(&out[insert_at..]);

    let rebuilt = maybe_reorder_declaration_reference_prologue(
        &rebuilt,
        &kept_directives,
        source_is_module,
        source_has_explicit_use_strict,
    );
    maybe_reorder_prologue_directives(&rebuilt, &kept_directives)
}

fn maybe_reorder_declaration_reference_prologue(
    emitted: &str,
    _directives: &[String],
    _source_is_module: bool,
    _source_has_explicit_use_strict: bool,
) -> String {
    // The emitter already places reference directives in the correct position
    // (as leading comments on the first statement, after the CJS prologue).
    // No reordering is needed.
    emitted.to_string()
}

fn maybe_reorder_prologue_directives(emitted: &str, directives: &[String]) -> String {
    if directives.len() <= 1 {
        return emitted.to_string();
    }

    let expected_trimmed: Vec<String> = directives.iter().map(|d| d.trim().to_string()).collect();
    let ends_with_newline = emitted.ends_with('\n');
    let lines: Vec<&str> = emitted.lines().collect();
    if lines.is_empty() {
        return emitted.to_string();
    }

    let mut start = None;
    let search_limit = lines.len().min(40);
    for (idx, line) in lines.iter().take(search_limit).enumerate() {
        if parse_triple_slash_reference(line).is_some() {
            start = Some(idx);
            break;
        }
    }
    let Some(start_idx) = start else {
        return emitted.to_string();
    };

    let mut end_idx = start_idx;
    while end_idx < lines.len() && parse_triple_slash_reference(lines[end_idx]).is_some() {
        end_idx += 1;
    }

    let actual_trimmed: Vec<String> = lines[start_idx..end_idx]
        .iter()
        .map(|line| line.trim().to_string())
        .collect();
    if actual_trimmed == expected_trimmed {
        return emitted.to_string();
    }
    if actual_trimmed.len() != expected_trimmed.len() {
        return emitted.to_string();
    }

    let mut expected_counts: HashMap<&str, usize> = HashMap::new();
    for line in &expected_trimmed {
        *expected_counts.entry(line.as_str()).or_insert(0) += 1;
    }
    let mut actual_counts: HashMap<&str, usize> = HashMap::new();
    for line in &actual_trimmed {
        *actual_counts.entry(line.as_str()).or_insert(0) += 1;
    }
    if expected_counts != actual_counts {
        return emitted.to_string();
    }

    let mut rebuilt = String::new();
    for line in &lines[..start_idx] {
        rebuilt.push_str(line);
        rebuilt.push('\n');
    }
    for directive in directives {
        rebuilt.push_str(directive);
        rebuilt.push('\n');
    }
    for line in &lines[end_idx..] {
        rebuilt.push_str(line);
        rebuilt.push('\n');
    }
    if !ends_with_newline && rebuilt.ends_with('\n') {
        rebuilt.pop();
    }
    rebuilt
}

pub(crate) fn is_declaration_reference_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".d.ts")
        || lower.ends_with(".d.tsx")
        || lower.ends_with(".d.mts")
        || lower.ends_with(".d.cts")
}

fn should_preserve_module_reference_directive(path: &str, source: &str, emitted: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let is_declaration_reference = is_declaration_reference_path(path);
    if is_declaration_reference {
        return true;
    }

    let stem = if let Some(stem) = lower
        .strip_suffix(".ts")
        .or_else(|| lower.strip_suffix(".tsx"))
        .or_else(|| lower.strip_suffix(".mts"))
        .or_else(|| lower.strip_suffix(".cts"))
    {
        stem
    } else {
        return true;
    };

    let base = basename(stem);
    let emitted_without_ref_directives = emitted
        .lines()
        .filter(|line| parse_triple_slash_reference(line).is_none())
        .collect::<Vec<_>>()
        .join("\n");
    let emitted_lower = emitted_without_ref_directives.to_ascii_lowercase();
    if emitted_lower.contains(stem) || (!base.is_empty() && emitted_lower.contains(&base)) {
        for line in source.lines() {
            for spec in parse_from_specifiers(line)
                .into_iter()
                .chain(parse_import_call_specifiers(line))
                .chain(parse_require_specifiers(line))
            {
                if !specifier_is_relative_or_absolute(&spec) {
                    continue;
                }
                let spec_lower = normalize_header_path(&spec).to_ascii_lowercase();
                let spec_stem = strip_supported_script_extension(&spec_lower);
                let spec_base = basename(spec_stem);
                if spec_stem == stem || (!base.is_empty() && spec_base == base) {
                    return true;
                }
            }
        }
    }

    false
}

pub(crate) fn source_has_explicit_use_strict_prologue(source: &str) -> bool {
    let mut lines = source.lines();
    if let Some(first) = lines.next() {
        if !first.starts_with("#!") {
            lines = source.lines();
        }
    }
    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        return trimmed.starts_with("\"use strict\"") || trimmed.starts_with("'use strict'");
    }
    false
}

pub(crate) fn strip_supported_script_extension(path: &str) -> &str {
    for ext in [
        ".d.ts", ".d.tsx", ".d.mts", ".d.cts", ".tsx", ".mts", ".cts", ".ts", ".jsx", ".js",
    ] {
        if let Some(stem) = path.strip_suffix(ext) {
            return stem;
        }
    }
    path
}

pub(crate) fn specifier_is_relative_or_absolute(spec: &str) -> bool {
    spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/')
}

pub(crate) fn source_file_has_import_equals_require(source: &str) -> bool {
    source.lines().any(|line| {
        // Only match top-level statements (no leading whitespace).
        // An indented `import foo = require(...)` inside a namespace body does
        // NOT create a module-level dependency and should not trigger ordering
        // signals or dependency detection.
        if line.starts_with(' ') || line.starts_with('\t') {
            return false;
        }
        let trimmed = line.trim_start();
        // Match `import X = require(...)` and `export import X = require(...)`
        let after_export = if let Some(stripped) = trimmed.strip_prefix("export ") {
            stripped.trim_start()
        } else {
            trimmed
        };
        (after_export.starts_with("import ") || after_export.starts_with("import\t"))
            && trimmed.contains("= require(")
    })
}

// --- Import/require specifier parsing ---

pub(crate) fn parse_require_specifiers(line: &str) -> Vec<String> {
    let mut specs = Vec::new();
    let mut rest = line;
    loop {
        let Some(pos) = rest.find("require(") else {
            break;
        };
        rest = &rest[pos + "require(".len()..];
        let trimmed = rest.trim_start();
        let Some(quote) = trimmed.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            rest = trimmed;
            continue;
        }
        let after_quote = &trimmed[quote.len_utf8()..];
        if let Some(end) = after_quote.find(quote) {
            specs.push(after_quote[..end].to_string());
            rest = &after_quote[end + quote.len_utf8()..];
        } else {
            break;
        }
    }
    specs
}

pub(crate) fn parse_from_specifiers(line: &str) -> Vec<String> {
    let mut specs = Vec::new();
    let mut rest = line;
    loop {
        let Some(pos) = rest.find("from") else {
            break;
        };
        rest = &rest[pos + "from".len()..];
        let trimmed = rest.trim_start();
        let Some(quote) = trimmed.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            rest = trimmed;
            continue;
        }
        let after_quote = &trimmed[quote.len_utf8()..];
        if let Some(end) = after_quote.find(quote) {
            specs.push(after_quote[..end].to_string());
            rest = &after_quote[end + quote.len_utf8()..];
        } else {
            break;
        }
    }
    specs
}

pub(crate) fn parse_import_call_specifiers(line: &str) -> Vec<String> {
    let mut specs = Vec::new();
    let mut rest = line;
    loop {
        let Some(pos) = rest.find("import(") else {
            break;
        };
        rest = &rest[pos + "import(".len()..];
        let trimmed = rest.trim_start();
        let Some(quote) = trimmed.chars().next() else {
            break;
        };
        if quote != '"' && quote != '\'' {
            rest = trimmed;
            continue;
        }
        let after_quote = &trimmed[quote.len_utf8()..];
        if let Some(end) = after_quote.find(quote) {
            specs.push(after_quote[..end].to_string());
            rest = &after_quote[end + quote.len_utf8()..];
        } else {
            break;
        }
    }
    specs
}

pub(crate) fn require_specifier_from_expr(expr: &tsc_rs_ast::Expr) -> Option<&str> {
    let mut current = expr;
    loop {
        match &current.kind {
            tsc_rs_ast::ExprKind::Paren(inner) | tsc_rs_ast::ExprKind::NonNull(inner) => {
                current = inner
            }
            tsc_rs_ast::ExprKind::As(a) => current = &a.expr,
            tsc_rs_ast::ExprKind::Satisfies(s) => current = &s.expr,
            tsc_rs_ast::ExprKind::TypeAssertion(ta) => current = &ta.expr,
            tsc_rs_ast::ExprKind::Instantiation(inst) => current = &inst.expr,
            _ => break,
        }
    }
    let tsc_rs_ast::ExprKind::Call(call) = &current.kind else {
        return None;
    };
    let tsc_rs_ast::ExprKind::Ident(callee) = &call.callee.kind else {
        return None;
    };
    if callee != "require" {
        return None;
    }
    let first = call.args.first()?;
    match &first.kind {
        tsc_rs_ast::ExprKind::StrLit(spec) | tsc_rs_ast::ExprKind::NoSubstTemplate(spec) => {
            Some(spec.as_str())
        }
        _ => None,
    }
}

pub(crate) fn parse_triple_slash_reference(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with("///") || !trimmed.contains("<reference") {
        return None;
    }
    let path_idx = trimmed.find("path=")?;
    let after = &trimmed[path_idx + "path=".len()..];
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

pub(crate) fn parse_symlink_aliases(source: &str) -> HashMap<String, Vec<String>> {
    let mut aliases: HashMap<String, Vec<String>> = HashMap::new();
    let mut current_file: Option<String> = None;

    for line in source.lines() {
        let Some((key, value)) = parse_directive_kv(line) else {
            continue;
        };
        if key.eq_ignore_ascii_case("filename") {
            current_file = Some(normalize_header_path(&value));
            continue;
        }
        if key.eq_ignore_ascii_case("symlink") || key.eq_ignore_ascii_case("link") {
            if let Some(file_name) = current_file.as_ref() {
                let paths: Vec<String> = value
                    .split(',')
                    .map(|s| normalize_header_path(s.trim()))
                    .filter(|s| !s.is_empty() && !is_node_modules_path(s))
                    .collect();
                if !paths.is_empty() {
                    aliases.entry(file_name.clone()).or_default().extend(paths);
                }
            }
        }
    }

    aliases
}

pub(crate) fn parse_link_path_mappings(source: &str) -> Vec<(String, String)> {
    let mut mappings = Vec::new();
    for line in source.lines() {
        let Some((key, value)) = parse_directive_kv(line) else {
            continue;
        };
        if !(key.eq_ignore_ascii_case("symlink") || key.eq_ignore_ascii_case("link")) {
            continue;
        }
        let Some((from, to)) = value.split_once("->") else {
            continue;
        };
        let from = normalize_header_path(from.trim());
        let to = normalize_header_path(to.trim());
        if from.is_empty() || to.is_empty() {
            continue;
        }
        mappings.push((from, to));
    }
    mappings
}

pub(crate) fn parse_directive_kv(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("//")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('@')?;
    let (key, value) = rest.split_once(':')?;
    Some((key.trim().to_string(), value.trim().to_string()))
}

// --- JSON utility functions ---

pub(crate) fn extract_json_string(json: &str, field: &str) -> Option<String> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start().strip_prefix('"')?;
    let end = after_colon.find('"')?;
    Some(after_colon[..end].to_string())
}

pub(crate) fn extract_json_bool(json: &str, field: &str) -> Option<bool> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    if after_colon.starts_with("true") {
        Some(true)
    } else if after_colon.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

pub(crate) fn extract_json_string_array(json: &str, field: &str) -> Option<Vec<String>> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start().strip_prefix('[')?;
    let end = after_colon.find(']')?;
    let mut result = Vec::new();
    let mut rest = &after_colon[..end];
    while let Some(start) = rest.find('"').or_else(|| rest.find('\'')) {
        let quote = rest.as_bytes()[start] as char;
        rest = &rest[start + 1..];
        if let Some(q_end) = rest.find(quote) {
            result.push(rest[..q_end].to_string());
            rest = &rest[q_end + 1..];
        } else {
            break;
        }
    }
    Some(result)
}

pub(crate) fn extract_json_object(json: &str, field: &str) -> Option<String> {
    let search = format!("\"{field}\"");
    let idx = json.find(&search)?;
    let after_key = &json[idx + search.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    if !after_colon.starts_with('{') {
        return None;
    }
    let mut depth = 0i32;
    let mut end_idx = None;
    for (i, ch) in after_colon.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    end_idx.map(|i| after_colon[..=i].to_string())
}

/// Strip trailing commas before `}` and `]` so that JSONC (JSON with trailing
/// commas) can be parsed by strict-JSON parsers like `serde_json`.  Handles
/// commas separated from the closing bracket by arbitrary whitespace.
pub(crate) fn strip_json_trailing_commas(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut in_string = false;
    while i < len {
        let b = bytes[i];
        if in_string {
            out.push(b as char);
            // Skip escaped characters inside strings.
            if b == b'\\' && i + 1 < len {
                i += 1;
                out.push(bytes[i] as char);
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b',' => {
                // Look ahead past whitespace; if the next non-whitespace char
                // is `}` or `]`, this comma is trailing — skip it.
                let mut j = i + 1;
                while j < len
                    && (bytes[j] == b' '
                        || bytes[j] == b'\t'
                        || bytes[j] == b'\n'
                        || bytes[j] == b'\r')
                {
                    j += 1;
                }
                if j < len && (bytes[j] == b'}' || bytes[j] == b']') {
                    // Drop the comma; the whitespace will be copied normally.
                    i += 1;
                } else {
                    out.push(',');
                    i += 1;
                }
            }
            _ => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

pub(crate) fn format_json_module_output(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Parser-recovery fallback for plain token streams in malformed JSON files.
    // TypeScript formats `contents Not read` as `{ contents, Not, read }`.
    let cleaned_check = strip_json_trailing_commas(trimmed);
    if serde_json::from_str::<serde_json::Value>(&cleaned_check).is_err() {
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if !tokens.is_empty()
            && tokens.iter().all(|t| {
                let mut chars = t.chars();
                let Some(first) = chars.next() else {
                    return false;
                };
                (first == '_' || first == '$' || first.is_ascii_alphabetic())
                    && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
            })
        {
            return format!("{{ {} }}\n", tokens.join(", "));
        }
        return trimmed.trim_end_matches('\n').to_string() + "\n";
    }

    // TypeScript preserves source JSON structure (inline vs multi-line),
    // strips trailing commas, normalizes to 4-space indentation, and
    // adds spaces inside inline objects.

    let cleaned = cleaned_check;

    // Special case: empty object/array → collapse to single line.
    let no_ws: String = cleaned.chars().filter(|c| !c.is_whitespace()).collect();
    if no_ws == "{}" || no_ws == "[]" {
        return no_ws + "\n";
    }

    // Re-indent based on brace/bracket nesting depth (not source whitespace).
    // This correctly handles mixed indentation in source.
    let mut out = String::new();
    let mut depth: i32 = 0;
    for line in cleaned.lines() {
        let stripped = line.trim();
        if stripped.is_empty() {
            out.push('\n');
            continue;
        }

        // If line starts with a closing brace/bracket, decrease depth before indenting.
        let first_char = stripped.chars().next().unwrap_or(' ');
        if first_char == '}' || first_char == ']' {
            depth -= 1;
            if depth < 0 {
                depth = 0;
            }
        }

        // Count opens/closes on this line (outside strings) to track depth.
        let mut line_opens = 0i32;
        let mut line_closes = 0i32;
        let mut in_str = false;
        let mut prev_ch = '\0';
        for ch in stripped.chars() {
            if in_str {
                if ch == '"' && prev_ch != '\\' {
                    in_str = false;
                }
            } else {
                match ch {
                    '"' => in_str = true,
                    '{' | '[' => line_opens += 1,
                    '}' | ']' => line_closes += 1,
                    _ => {}
                }
            }
            prev_ch = ch;
        }

        // Write indentation and content.
        for _ in 0..(depth * 4) {
            out.push(' ');
        }
        out.push_str(stripped);
        out.push('\n');

        // Update depth: the first close was already handled above (pre-decrement).
        // Net change = opens - remaining_closes.
        let already_handled_close = if first_char == '}' || first_char == ']' {
            1
        } else {
            0
        };
        depth += line_opens - (line_closes - already_handled_close);
    }

    // Remove trailing empty lines and ensure single final newline.
    let result = out.trim_end().to_string() + "\n";
    normalize_json_inline_object_spacing(&result)
}

/// Normalize spaces inside inline JSON objects:
/// `{"x": 12}` → `{ "x": 12 }`, `{"x": 12, "y": 1}` → `{ "x": 12, "y": 1 }`
/// Only applies to objects that start and end on the same line.
fn normalize_json_inline_object_spacing(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 32);
    for line in input.lines() {
        // Process inline objects: {" at start of object → { "
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut result = String::with_capacity(chars.len() + 8);
        while i < chars.len() {
            if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '"' {
                // Check if there's a matching `}` on the same portion (inline object)
                let mut depth = 1;
                let mut j = i + 1;
                let mut in_str = false;
                while j < chars.len() && depth > 0 {
                    if in_str {
                        if chars[j] == '\\' {
                            j += 1; // skip escaped char
                        } else if chars[j] == '"' {
                            in_str = false;
                        }
                    } else {
                        match chars[j] {
                            '"' => in_str = true,
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                    j += 1;
                }
                if depth == 0 {
                    // Found matching `}` on same line — add space after `{`
                    result.push('{');
                    result.push(' ');
                    // Also add space before the matching `}`
                    let close_pos = j - 1; // position of `}`
                                           // Process content between `{` and `}`
                    let mut k = i + 1;
                    while k < close_pos {
                        result.push(chars[k]);
                        k += 1;
                    }
                    // Add space before `}` if not already there
                    if !result.ends_with(' ') {
                        result.push(' ');
                    }
                    result.push('}');
                    i = j;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        out.push_str(&result);
        out.push('\n');
    }
    // Remove the extra trailing newline we added
    if out.ends_with('\n') && !input.ends_with('\n') {
        out.pop();
    }
    // If input ended with exactly one newline, we have the right amount
    out
}

/// Format a JSON value matching TypeScript's output style:
/// - Objects use 4-space indentation, one property per line
/// - Arrays stay on a single line (compact), with inline objects
/// - Empty objects/arrays are `{}`/`[]`
// Name conversion functions.
pub(crate) fn js_to_dts_name(name: &str) -> String {
    if let Some(base) = name.strip_suffix(".mjs") {
        format!("{base}.d.mts")
    } else if let Some(base) = name.strip_suffix(".cjs") {
        format!("{base}.d.cts")
    } else if let Some((base, _)) = name.rsplit_once('.') {
        format!("{base}.d.ts")
    } else {
        format!("{name}.d.ts")
    }
}

/// Convert a `.ts` / `.tsx` file name to its `.js` / `.jsx` equivalent.
pub(crate) fn ts_to_dts_name(name: &str) -> String {
    if let Some(base) = name.strip_suffix(".tsx") {
        format!("{base}.d.ts")
    } else if let Some(base) = name.strip_suffix(".jsx") {
        format!("{base}.d.ts")
    } else if let Some(base) = name.strip_suffix(".mts") {
        format!("{base}.d.mts")
    } else if let Some(base) = name.strip_suffix(".mjs") {
        format!("{base}.d.mts")
    } else if let Some(base) = name.strip_suffix(".cts") {
        format!("{base}.d.cts")
    } else if let Some(base) = name.strip_suffix(".cjs") {
        format!("{base}.d.cts")
    } else if let Some(base) = name.strip_suffix(".ts") {
        format!("{base}.d.ts")
    } else if let Some(base) = name.strip_suffix(".js") {
        format!("{base}.d.ts")
    } else {
        format!("{name}.d.ts")
    }
}

pub(crate) fn ts_to_js_name_with_jsx(name: &str, jsx: Option<tsc_rs_ast::JsxEmit>) -> String {
    if let Some(base) = name.strip_suffix(".tsx") {
        // TypeScript only produces .jsx output when jsx=preserve.
        // react-native preserves JSX syntax but still emits .js files.
        let ext = match jsx {
            Some(tsc_rs_ast::JsxEmit::Preserve) => "jsx",
            _ => "js",
        };
        format!("{base}.{ext}")
    } else if let Some(base) = name.strip_suffix(".jsx") {
        let ext = match jsx {
            Some(tsc_rs_ast::JsxEmit::Preserve) => "jsx",
            _ => "js",
        };
        format!("{base}.{ext}")
    } else if let Some(base) = name.strip_suffix(".mts") {
        format!("{base}.mjs")
    } else if let Some(base) = name.strip_suffix(".cts") {
        format!("{base}.cjs")
    } else if let Some(base) = name.strip_suffix(".ts") {
        format!("{base}.js")
    } else {
        // Unknown extension -- leave as-is.
        name.to_string()
    }
}

// --- Source map rewriting ---

/// When `mapRoot` is set, rewrite the `//# sourceMappingURL=...` line in the
/// emitted JavaScript to use the correct relative path from the JS output
/// location to the source map file under the resolved mapRoot.
pub(crate) fn rewrite_source_map_url_for_map_root(
    javascript: &str,
    source_name: &str,
    jsx: Option<tsc_rs_ast::JsxEmit>,
    effective_options: &tsc_rs_ast::CompilerOptions,
    path_ctx: &BaselinePathContext,
) -> String {
    // Only rewrite when sourceMap is set (not inline) and mapRoot is present.
    if effective_options.source_map != Some(true) {
        return javascript.to_string();
    }
    if effective_options.inline_source_map == Some(true) {
        return javascript.to_string();
    }
    let map_root_raw = effective_options.other.iter().rev().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("maproot") {
            Some(v.clone())
        } else {
            None
        }
    });
    let Some(map_root_raw) = map_root_raw else {
        return javascript.to_string();
    };

    // Resolve mapRoot relative to tsconfig dir (or treat as relative to source).
    let resolved_map_root = if is_absolute_path(&map_root_raw) {
        normalize_path_segments(&map_root_raw)
    } else if let Some(ref cfg_dir) = path_ctx.tsconfig_dir {
        join_path(cfg_dir, &map_root_raw)
    } else {
        map_root_raw.clone()
    };

    // Compute the full JS output path.
    let full_js_path = full_output_path_for_source_js(source_name, jsx, path_ctx);
    let js_dir = dirname(&full_js_path);

    // Compute the relative source path (same as what goes into outDir).
    let rel_source = source_relative_path(source_name, path_ctx);
    let rel_js = ts_to_js_name_with_jsx(&rel_source, jsx);

    // The map file is at <resolvedMapRoot>/<rel_js>.map
    let map_file_path = join_path(&resolved_map_root, &format!("{}.map", rel_js));

    // Compute relative path from JS output dir to map file.
    let new_url = make_relative_path(&js_dir, &map_file_path);

    // Replace the sourceMappingURL line.
    if let Some(pos) = javascript.rfind("//# sourceMappingURL=") {
        let line_end = javascript[pos..]
            .find('\n')
            .map(|i| pos + i)
            .unwrap_or(javascript.len());
        format!(
            "{}//# sourceMappingURL={}{}",
            &javascript[..pos],
            new_url,
            &javascript[line_end..]
        )
    } else {
        javascript.to_string()
    }
}

// --- Bundle/out-file functions ---

pub(crate) fn bundle_out_file_js(js_sections: &[(String, String, String)]) -> String {
    // Two-pass approach: first collect all cleaned sections and extract
    // prologue directives from non-first sections, then assemble with
    // prologues hoisted to right after the first section's prologue.
    let mut cleaned_sections: Vec<String> = Vec::new();
    let mut hoisted_prologues: Vec<String> = Vec::new();
    let mut seen_helpers: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut is_first = true;

    for (_, _, section) in js_sections {
        let mut text = section.trim_end_matches('\n').to_string();
        // Strip shebangs from non-first sections (TypeScript keeps only the
        // first file's shebang in the bundle).
        if !is_first && text.starts_with("#!") {
            if let Some(nl) = text.find('\n') {
                text = text[nl + 1..].to_string();
            }
        }
        if !is_first && text.starts_with("\"use strict\";\n") {
            text = text.trim_start_matches("\"use strict\";\n").to_string();
        }
        // Deduplicate helper functions in outFile bundles.  Helpers follow
        // the pattern: `var __NAME = (this && this.__NAME) || function ...`
        // spanning one or more lines until a top-level `};` line.
        // Track helpers from first section and strip duplicates from later ones.
        {
            let mut deduped = String::new();
            let mut skip_until_end = false;
            for line in text.lines() {
                if skip_until_end {
                    if line == "};" || line == "})();" {
                        skip_until_end = false;
                    }
                    continue;
                }
                if let Some(rest) = line.strip_prefix("var __") {
                    if let Some(eq_pos) = rest.find(" = (this && this.__") {
                        let helper_name = &rest[..eq_pos];
                        if is_first {
                            seen_helpers.insert(helper_name.to_string());
                        } else if seen_helpers.contains(helper_name) {
                            // Skip this duplicate helper definition
                            if !line.ends_with("};") {
                                skip_until_end = true;
                            }
                            continue;
                        } else {
                            seen_helpers.insert(helper_name.to_string());
                        }
                    }
                }
                if !deduped.is_empty() {
                    deduped.push('\n');
                }
                deduped.push_str(line);
            }
            text = deduped;
        }
        // Extract prologue directives from non-first sections and hoist them.
        if !is_first {
            loop {
                if text.starts_with('"') || text.starts_with('\'') {
                    if let Some(semi_nl) = text.find(";\n") {
                        let directive = text[..semi_nl + 1].to_string();
                        let d = directive.trim();
                        let is_string_directive = (d.starts_with('"') && d.ends_with("\";"))
                            || (d.starts_with('\'') && d.ends_with("';"));
                        if is_string_directive {
                            hoisted_prologues.push(directive);
                            text = text[semi_nl + 2..].to_string();
                            continue;
                        }
                    }
                }
                break;
            }
        }
        // Strip per-file sourceMappingURL lines; they don't belong in a
        // bundled --out file.  The correct out-file-level source map URL
        // is added by the caller.
        if let Some(pos) = text.rfind("//# sourceMappingURL=") {
            let url_line = text[pos..].lines().next().unwrap_or("");
            let start = if pos > 0 && text.as_bytes().get(pos - 1) == Some(&b'\n') {
                pos - 1
            } else {
                pos
            };
            text = format!("{}{}", &text[..start], &text[pos + url_line.len()..]);
            text = text.trim_end_matches('\n').to_string();
        }
        if !text.is_empty() {
            cleaned_sections.push(text);
        }
        is_first = false;
    }

    // Assemble: for the first section, insert hoisted prologues right after
    // the prologue directives (shebang + "use strict"), before the code body.
    let mut out = String::new();
    for (i, text) in cleaned_sections.iter().enumerate() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if i == 0 && !hoisted_prologues.is_empty() {
            // Find the end of the first section's prologue (shebang + "use strict" + existing prologues).
            let mut prologue_end = 0;
            let mut remaining = text.as_str();
            loop {
                if remaining.starts_with("#!") {
                    if let Some(nl) = remaining.find('\n') {
                        prologue_end += nl + 1;
                        remaining = &text[prologue_end..];
                        continue;
                    }
                }
                if remaining.starts_with("\"use strict\";\n") {
                    prologue_end += "\"use strict\";\n".len();
                    remaining = &text[prologue_end..];
                    continue;
                }
                if remaining.starts_with('"') || remaining.starts_with('\'') {
                    if let Some(semi_nl) = remaining.find(";\n") {
                        let d = remaining[..semi_nl + 1].trim();
                        let is_string = (d.starts_with('"') && d.ends_with("\";"))
                            || (d.starts_with('\'') && d.ends_with("';"));
                        if is_string {
                            prologue_end += semi_nl + 2;
                            remaining = &text[prologue_end..];
                            continue;
                        }
                    }
                }
                break;
            }
            // Write the prologue part of section 1.
            out.push_str(&text[..prologue_end]);
            // Insert hoisted prologues.
            for p in &hoisted_prologues {
                out.push_str(p);
                out.push('\n');
            }
            // Write the code part of section 1.
            out.push_str(&text[prologue_end..]);
            out.push('\n');
        } else {
            out.push_str(text);
            out.push('\n');
        }
    }
    out
}

/// Extract the inline `data:application/json;base64,...` sourcemap URI from
/// the last js_section that contains one.  Returns the raw data-URI string
/// (the part after `//# sourceMappingURL=`).
pub(crate) fn extract_inline_sourcemap_from_sections(
    js_sections: &[(String, String, String)],
) -> Option<String> {
    for (_, _, section) in js_sections.iter().rev() {
        if let Some(pos) = section.rfind("//# sourceMappingURL=data:") {
            let after = &section[pos + "//# sourceMappingURL=".len()..];
            let url = after.lines().next().unwrap_or("").trim();
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
    }
    None
}

/// Patch the `"file":"..."` field inside a `data:application/json;base64,...`
/// inline sourcemap URI to use `new_file_name` instead.
/// Returns the original URI unchanged if decoding / patching fails.
pub(crate) fn patch_inline_sourcemap_file_field(data_uri: &str, new_file_name: &str) -> String {
    let prefix = "data:application/json;base64,";
    if !data_uri.starts_with(prefix) {
        return data_uri.to_string();
    }
    let b64 = &data_uri[prefix.len()..];
    // Decode the base64.
    let decoded = match base64_decode(b64) {
        Some(d) => d,
        None => return data_uri.to_string(),
    };
    let json = match String::from_utf8(decoded) {
        Ok(s) => s,
        Err(_) => return data_uri.to_string(),
    };
    // Replace the "file":"..." field.  The JSON is compact so we look for
    // the exact pattern `"file":"<old_value>"`.
    let patched = replace_json_string_field(&json, "file", new_file_name);
    // Re-encode.
    let new_b64 = base64_encode_str(patched.as_bytes());
    format!("{}{}", prefix, new_b64)
}

// --- Base64 utilities ---

/// Decode a base64 string into bytes.  Returns None on invalid input.
pub(crate) fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;
    for ch in input.chars() {
        if ch == '=' {
            break;
        }
        let val = CHARS.iter().position(|&c| c == ch as u8)?;
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Encode bytes to base64.
pub(crate) fn base64_encode_str(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Replace the value of a JSON string field (compact JSON).
/// Handles the pattern `"<key>":"<value>"` → `"<key>":"<new_value>"`.
pub(crate) fn replace_json_string_field(json: &str, key: &str, new_value: &str) -> String {
    // Build the search pattern.
    let search = format!("\"{}\":\"", key);
    if let Some(pos) = json.find(&search) {
        let after_key = &json[pos + search.len()..];
        // Find the closing quote (un-escaped).
        let end = find_json_string_end(after_key);
        let before = &json[..pos + search.len()];
        let after = &after_key[end..];
        // Escape the new value for JSON.
        let escaped = escape_json_str(new_value);
        format!("{}{}{}", before, escaped, after)
    } else {
        json.to_string()
    }
}

/// Find the end of a JSON string value (the position of the closing `"`),
/// starting just after the opening `"`.
fn find_json_string_end(s: &str) -> usize {
    let mut chars = s.char_indices();
    while let Some((i, ch)) = chars.next() {
        match ch {
            '"' => return i,
            '\\' => {
                // Skip the escaped character.
                chars.next();
            }
            _ => {}
        }
    }
    s.len()
}

/// Escape a string for inclusion in a JSON string value.
fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn normalize_amd_out_file_bundle(bundle: &str, has_non_amd_sections: bool) -> String {
    let mut out = bundle.to_string();
    let needs_top_level_strict =
        has_non_amd_sections && out.contains("Object.defineProperty(exports, \"__esModule\"");
    if needs_top_level_strict && !out.starts_with("\"use strict\";\n") {
        out = format!("\"use strict\";\n{out}");
    }
    out
}
