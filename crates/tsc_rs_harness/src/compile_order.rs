use super::*;

/// Error annotations follow the upstream runner's root-files/other-files
/// partition, independently of emit order or output options.
pub(crate) fn error_source_order_indices(test_case: &TestCase) -> Vec<usize> {
    let mut indices: Vec<_> = (0..test_case.files.len()).collect();
    if indices.len() < 2 {
        return indices;
    }
    if let Some(tsconfig) = find_tsconfig_file(test_case) {
        // compilerRunner.ts lists the parsed config first, then the units it
        // selects as roots, then everything else on the virtual disk.
        let Some(roots) = tsconfig_root_file_indices(test_case) else {
            return indices;
        };
        let mut ordered: Vec<usize> = Vec::with_capacity(indices.len());
        if let Some(config_idx) = test_case
            .files
            .iter()
            .position(|file| std::ptr::eq(file, tsconfig))
        {
            ordered.push(config_idx);
        }
        for idx in roots.iter().copied().chain(indices.iter().copied()) {
            if !ordered.contains(&idx) {
                ordered.push(idx);
            }
        }
        return ordered;
    }
    let last = &test_case.files[indices.len() - 1];
    let no_implicit_references =
        test_case.options.other.iter().any(|(key, value)| {
            key.eq_ignore_ascii_case("noimplicitreferences") && !value.is_empty()
        });
    // These are deliberately textual signals, matching compilerRunner.ts:
    // /require\(/ and /reference\spath/ also recognize text in comments.
    let reference_path = last.content.split("reference").skip(1).any(|tail| {
        let mut chars = tail.chars();
        let whitespace = chars.next().is_some_and(|ch| {
            matches!(ch,
                '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}'
                | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}'
                | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
        });
        whitespace && chars.as_str().starts_with("path")
    });
    if no_implicit_references || last.content.contains("require(") || reference_path {
        indices.rotate_right(1);
    }
    indices
}

pub(crate) fn input_file_order_indices(
    test_case: &TestCase,
    ctx: &BaselinePathContext,
) -> Vec<usize> {
    let mut indices: Vec<usize> = test_case
        .files
        .iter()
        .enumerate()
        .filter_map(|(idx, file)| {
            (!basename(&file.name).eq_ignore_ascii_case("tsconfig.json")).then_some(idx)
        })
        .collect();

    // Keep source order unless this is a tsconfig-style multi-file scenario.
    if find_tsconfig_file(test_case).is_none()
        || (has_explicit_module_suffixes(ctx) && !ctx.resolve_json_module)
    {
        return indices;
    }
    let has_node_modules_inputs = indices
        .iter()
        .any(|idx| is_node_modules_path(&test_case.files[*idx].name));
    if !has_node_modules_inputs && ctx.root_dirs.len() <= 1 && !ctx.resolve_json_module {
        // With classic module resolution (explicit or implied by AMD module),
        // TypeScript sorts the source section by full path when files exist
        // outside the tsconfig directory.
        let is_classic_resolution = test_case
            .options
            .module_resolution
            .as_ref()
            .is_some_and(|r| r.eq_ignore_ascii_case("classic"))
            || matches!(test_case.options.module, Some(ModuleKind::AMD));
        let has_files_outside_tsconfig_dir = is_classic_resolution
            && ctx.tsconfig_dir.as_ref().is_some_and(|tsconfig_dir| {
                let prefix = normalize_header_path(tsconfig_dir).to_lowercase();
                indices.iter().any(|idx| {
                    let p = normalize_header_path(&test_case.files[*idx].name).to_lowercase();
                    !p.starts_with(&prefix)
                })
            });
        if !has_files_outside_tsconfig_dir {
            return indices;
        }
    }

    // TypeScript baselines often group node_modules files first and otherwise
    // follow normalized full-path order.
    indices.sort_by(|a, b| {
        let an = normalize_header_path(&test_case.files[*a].name);
        let bn = normalize_header_path(&test_case.files[*b].name);
        let a_node = is_node_modules_path(&an);
        let b_node = is_node_modules_path(&bn);
        if a_node != b_node {
            return if a_node {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        // Prioritize package.json before other files in the same package.
        // Outside node_modules: always prioritize package.json.
        // Inside node_modules: only when both files are in the same
        // package (package.json directory is an ancestor of the other file).
        {
            let a_pkg = basename(&an).eq_ignore_ascii_case("package.json");
            let b_pkg = basename(&bn).eq_ignore_ascii_case("package.json");
            if a_pkg != b_pkg {
                if !a_node {
                    // Outside node_modules: always prioritize package.json
                    return if a_pkg {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    };
                }
                // Inside node_modules: prioritize package.json within the
                // same package (package.json dir is ancestor of other file).
                let a_dir = an.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let b_dir = bn.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let same_pkg = a_dir == b_dir
                    || (a_pkg && b_dir.starts_with(&format!("{}/", a_dir)))
                    || (b_pkg && a_dir.starts_with(&format!("{}/", b_dir)));
                if same_pkg {
                    return if a_pkg {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    };
                }
            }
        }
        an.cmp(&bn)
    });
    indices
}

pub(crate) fn should_use_require_last_unit_roots(
    test_case: &TestCase,
    ctx: &BaselinePathContext,
) -> bool {
    if find_tsconfig_file(test_case).is_some() {
        return false;
    }
    if test_case.files.len() < 2 {
        return false;
    }
    if ctx.out_file.is_some() || ctx.out_dir.is_some() {
        return false;
    }
    let Some(last_file) = test_case.files.last() else {
        return false;
    };
    source_file_has_ordering_signals(&last_file.content)
        || last_file
            .content
            .lines()
            .any(|line| !parse_require_specifiers(line).is_empty())
}

pub(crate) fn compile_indices_for_require_last_unit(
    test_case: &TestCase,
    compile_indices: &[usize],
    parsed_sources: &HashMap<usize, SourceFile>,
    ctx: &BaselinePathContext,
    no_resolve: bool,
    link_path_mappings: &[(String, String)],
) -> Vec<usize> {
    if compile_indices.is_empty() {
        return Vec::new();
    }
    let root = *compile_indices
        .last()
        .expect("require-root mode requires at least one compilable file");
    if no_resolve {
        return vec![root];
    }

    let mut path_to_idx = HashMap::new();
    for (idx, file) in test_case.files.iter().enumerate() {
        path_to_idx.insert(normalize_header_path(&file.name), idx);
    }
    apply_link_path_aliases(&mut path_to_idx, test_case, link_path_mappings);

    let allowed: HashSet<usize> = compile_indices.iter().copied().collect();
    let mut deps: HashMap<usize, Vec<usize>> = HashMap::new();
    for idx in compile_indices {
        let file = &test_case.files[*idx];
        let parsed = parsed_sources
            .get(idx)
            .expect("missing parsed source while computing require-root closure");
        let mut d = dependency_indices_for_file_with_options(
            file,
            parsed,
            test_case,
            &path_to_idx,
            ctx,
            true,
        );
        d.retain(|dep| allowed.contains(dep) && *dep != *idx);
        deps.insert(*idx, d);
    }

    let mut reachable: HashSet<usize> = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !reachable.insert(node) {
            continue;
        }
        if let Some(ds) = deps.get(&node) {
            for dep in ds.iter().rev() {
                stack.push(*dep);
            }
        }
    }

    compile_indices
        .iter()
        .copied()
        .filter(|idx| reachable.contains(idx))
        .collect()
}

pub(crate) fn compute_emit_order(
    test_case: &TestCase,
    compile_indices: &[usize],
    parsed_sources: &HashMap<usize, SourceFile>,
    ctx: &BaselinePathContext,
    link_path_mappings: &[(String, String)],
    module_kind: Option<tsc_rs_ast::ModuleKind>,
) -> Vec<usize> {
    let mut path_to_idx = HashMap::new();
    for (idx, file) in test_case.files.iter().enumerate() {
        path_to_idx.insert(normalize_header_path(&file.name), idx);
    }
    apply_link_path_aliases(&mut path_to_idx, test_case, link_path_mappings);

    let mut deps: HashMap<usize, Vec<usize>> = HashMap::new();
    for idx in compile_indices {
        let file = &test_case.files[*idx];
        let parsed = parsed_sources
            .get(idx)
            .expect("missing parsed source while computing emit order");
        deps.insert(
            *idx,
            dependency_indices_for_file(file, parsed, test_case, &path_to_idx, ctx),
        );
    }

    // Stable topological sort:
    // - edge dep -> importer
    // - among ready nodes, preserve original source-file order.
    let mut pos = HashMap::new();
    for (i, idx) in compile_indices.iter().enumerate() {
        pos.insert(*idx, i);
    }
    // When the root file (last file) uses `import X = require(...)` or
    // triple-slash references, TypeScript determines emit order by doing
    // a post-order DFS from the root. This correctly handles cycles and
    // is more accurate than the topological sort for these cases.
    let postorder_from_root: Option<Vec<usize>> = compile_indices.last().and_then(|root| {
        should_use_root_dependency_postorder(test_case, *root, ctx, module_kind)
            .then(|| root_dependency_postorder(*root, compile_indices, &deps))
    });
    if let Some(mut order) = postorder_from_root {
        // The post-order DFS only visits files reachable from the root.
        // In outFile bundles, ALL source files must be included. Insert
        // unreachable files at positions that preserve their original
        // source order relative to the reachable files.
        if order.len() < compile_indices.len() {
            let order_set: HashSet<usize> = order.iter().copied().collect();
            let remaining: Vec<usize> = compile_indices
                .iter()
                .copied()
                .filter(|idx| !order_set.contains(idx))
                .collect();
            // Merge remaining files into the order while preserving
            // relative source positions. For each remaining file, find
            // the insertion point: just before the first ordered file
            // whose source position is greater.
            for rem in remaining {
                let rem_pos = pos.get(&rem).copied().unwrap_or(usize::MAX);
                let insert_pos = order
                    .iter()
                    .position(|existing| pos.get(existing).copied().unwrap_or(usize::MAX) > rem_pos)
                    .unwrap_or(order.len());
                order.insert(insert_pos, rem);
            }
        }
        if matches!(
            module_kind,
            Some(ModuleKind::Node16)
                | Some(ModuleKind::Node18)
                | Some(ModuleKind::Node20)
                | Some(ModuleKind::NodeNext)
        ) {
            reorder_two_file_mixed_node_cycle(&mut order, test_case, &deps, module_kind);
            reorder_node_module_triplet(&mut order, test_case);
            reorder_explicit_relative_node_triplets(&mut order, test_case);
            reorder_cjs_self_export_triplet(&mut order, test_case);
        }
        return order;
    }

    // For ESM allowJs compilations where all compile files are JavaScript
    // source files, TypeScript emits them in source order because ES modules
    // handle dependency resolution at runtime.
    // For CJS/AMD/System, dependency-based ordering is used instead.
    {
        use tsc_rs_ast::ModuleKind;
        let is_esm = matches!(
            module_kind,
            Some(ModuleKind::ES2015)
                | Some(ModuleKind::ES2020)
                | Some(ModuleKind::ES2022)
                | Some(ModuleKind::ESNext)
                | Some(ModuleKind::Preserve)
        );
        if ctx.allow_js && is_esm {
            let all_js_sources = compile_indices.iter().all(|idx| {
                let name = &test_case.files[*idx].name;
                let lower = name.to_lowercase();
                lower.ends_with(".js")
                    || lower.ends_with(".jsx")
                    || lower.ends_with(".mjs")
                    || lower.ends_with(".cjs")
            });
            if all_js_sources {
                return compile_indices.to_vec();
            }
        }
    }

    let root_rank: HashMap<usize, usize> = HashMap::new();
    let sort_key = |idx: &usize| {
        (
            root_rank.get(idx).copied().unwrap_or(usize::MAX),
            pos.get(idx).copied().unwrap_or(usize::MAX),
        )
    };

    let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut indegree: HashMap<usize, usize> = HashMap::new();
    for idx in compile_indices {
        indegree.insert(*idx, 0);
    }
    for importer in compile_indices {
        if let Some(ds) = deps.get(importer) {
            let mut ordered_compile_deps: Vec<usize> = Vec::new();
            for dep in ds {
                if !compile_indices.contains(dep) || *dep == *importer {
                    continue;
                }
                ordered_compile_deps.push(*dep);
                let entry = adjacency.entry(*dep).or_default();
                if !entry.contains(importer) {
                    entry.push(*importer);
                    *indegree.entry(*importer).or_insert(0) += 1;
                }
            }
            if ctx.use_case_sensitive_file_names == Some(false)
                && !matches!(
                    module_kind,
                    Some(ModuleKind::Node16)
                        | Some(ModuleKind::Node18)
                        | Some(ModuleKind::Node20)
                        | Some(ModuleKind::NodeNext)
                )
            {
                for pair in ordered_compile_deps.windows(2) {
                    let from = pair[0];
                    let to = pair[1];
                    if from == to {
                        continue;
                    }
                    let entry = adjacency.entry(from).or_default();
                    if !entry.contains(&to) {
                        entry.push(to);
                        *indegree.entry(to).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut ready: Vec<usize> = compile_indices
        .iter()
        .copied()
        .filter(|idx| indegree.get(idx).copied().unwrap_or(0) == 0)
        .collect();
    ready.sort_by_key(sort_key);

    let mut order = Vec::with_capacity(compile_indices.len());
    while let Some(node) = ready.first().copied() {
        ready.remove(0);
        order.push(node);
        if let Some(nexts) = adjacency.get(&node) {
            for next in nexts {
                if let Some(d) = indegree.get_mut(next) {
                    if *d > 0 {
                        *d -= 1;
                        if *d == 0 {
                            ready.push(*next);
                        }
                    }
                }
            }
        }
        ready.sort_by_key(sort_key);
    }

    // Cycle fallback: append any remaining nodes in source order.
    if order.len() < compile_indices.len() {
        let mut remaining: Vec<usize> = compile_indices
            .iter()
            .copied()
            .filter(|idx| !order.contains(idx))
            .collect();
        let remaining_set: HashSet<usize> = remaining.iter().copied().collect();
        let all_remaining_need_dependency_order = remaining.iter().all(|idx| {
            parsed_sources
                .get(idx)
                .map(source_file_looks_like_module)
                .unwrap_or(false)
                || deps
                    .get(idx)
                    .is_some_and(|ds| ds.iter().any(|dep| remaining_set.contains(dep)))
        });
        if !root_rank.is_empty() {
            remaining.sort_by_key(sort_key);
        } else if all_remaining_need_dependency_order {
            if matches!(
                module_kind,
                Some(ModuleKind::Node16)
                    | Some(ModuleKind::Node18)
                    | Some(ModuleKind::Node20)
                    | Some(ModuleKind::NodeNext)
            ) {
                let package_type_by_dir = package_json_module_type_by_dir(test_case);
                remaining.sort_by_key(|idx| {
                    let effective = effective_module_kind_for_source(
                        &test_case.files[*idx].name,
                        module_kind,
                        &package_type_by_dir,
                    );
                    let rank = match effective {
                        Some(ModuleKind::CommonJS) => 0usize,
                        _ => 1usize,
                    };
                    (
                        rank,
                        std::cmp::Reverse(pos.get(idx).copied().unwrap_or(usize::MAX)),
                    )
                });
            } else if !link_path_mappings.is_empty() {
                remaining = order_remaining_via_scc(&remaining, &deps, &pos);
            } else {
                remaining = order_remaining_via_dependency_postorder(&remaining, &deps, &pos);
            }
        } else {
            remaining.sort_by_key(|idx| pos.get(idx).copied().unwrap_or(usize::MAX));
        }
        for idx in remaining {
            order.push(idx);
        }
    }

    // For Node-style modes: when there are exactly 3 non-node_modules files
    // forming a .ts/.mts/.cts (or .js/.mjs/.cjs) triplet with the same base
    // name, AND all files import from bare package specifiers (not explicit
    // path extensions), reorder to: ESM first, CJS second, default last.
    // This matches TypeScript's output ordering for package-export-based tests.
    if matches!(
        module_kind,
        Some(ModuleKind::Node16)
            | Some(ModuleKind::Node18)
            | Some(ModuleKind::Node20)
            | Some(ModuleKind::NodeNext)
    ) {
        reorder_two_file_mixed_node_cycle(&mut order, test_case, &deps, module_kind);
        reorder_node_module_triplet(&mut order, test_case);
        reorder_explicit_relative_node_triplets(&mut order, test_case);
        reorder_cjs_self_export_triplet(&mut order, test_case);
    }

    order
}

fn reorder_two_file_mixed_node_cycle(
    order: &mut [usize],
    test_case: &TestCase,
    deps: &HashMap<usize, Vec<usize>>,
    module_kind: Option<ModuleKind>,
) {
    if order.len() != 2
        || basename(&test_case.files[order[0]].name) != basename(&test_case.files[order[1]].name)
    {
        return;
    }
    if !deps
        .get(&order[0])
        .is_some_and(|file_deps| file_deps.contains(&order[1]))
        || !deps
            .get(&order[1])
            .is_some_and(|file_deps| file_deps.contains(&order[0]))
    {
        return;
    }

    let package_type_by_dir = package_json_module_type_by_dir(test_case);
    let first_kind = effective_module_kind_for_source(
        &test_case.files[order[0]].name,
        module_kind,
        &package_type_by_dir,
    );
    let second_kind = effective_module_kind_for_source(
        &test_case.files[order[1]].name,
        module_kind,
        &package_type_by_dir,
    );
    if first_kind == Some(ModuleKind::CommonJS) && second_kind != Some(ModuleKind::CommonJS) {
        order.swap(0, 1);
    }
}

/// Reorder .ts/.mts/.cts (or .js/.mjs/.cjs) triplets in Node-style emit
/// order. Only applies when all 3 non-node_modules files import from bare
/// package specifiers (resolved via package.json exports), not explicit paths.
fn reorder_node_module_triplet(order: &mut [usize], test_case: &TestCase) {
    let non_nm: Vec<(usize, usize)> = order
        .iter()
        .enumerate()
        .filter(|(_, idx)| !is_node_modules_path(&test_case.files[**idx].name))
        .map(|(pos, idx)| (pos, *idx))
        .collect();

    if non_nm.len() != 3 {
        return;
    }

    // Check same base name
    let bases: Vec<String> = non_nm
        .iter()
        .map(|(_, idx)| {
            let name = normalize_header_path(&test_case.files[*idx].name);
            strip_ts_js_extension(&name)
        })
        .collect();
    if bases[0] != bases[1] || bases[1] != bases[2] {
        return;
    }

    // Classify extensions
    let exts: Vec<u8> = non_nm
        .iter()
        .map(|(_, idx)| {
            let name = test_case.files[*idx].name.to_ascii_lowercase();
            if name.ends_with(".mts") || name.ends_with(".mjs") {
                0 // ESM
            } else if name.ends_with(".cts") || name.ends_with(".cjs") {
                1 // CJS
            } else {
                2 // default
            }
        })
        .collect();
    if !(exts.contains(&0) && exts.contains(&1) && exts.contains(&2)) {
        return;
    }

    // Only reorder when the source files import from MULTIPLE DISTINCT bare
    // package subpath specifiers (e.g. "package/cjs", "package/mjs", "inner/cjs").
    // Tests that import from a single specifier (e.g. just "package") keep
    // source order.
    let mut all_specs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, idx) in &non_nm {
        let content = &test_case.files[*idx].content;
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("import ") {
                continue;
            }
            let spec = if let Some(pos) = trimmed.rfind('"') {
                let start = trimmed[..pos].rfind('"');
                start.map(|s| trimmed[s + 1..pos].to_string())
            } else if let Some(pos) = trimmed.rfind('\'') {
                let start = trimmed[..pos].rfind('\'');
                start.map(|s| trimmed[s + 1..pos].to_string())
            } else {
                None
            };
            if let Some(spec) = spec {
                // Only count bare package subpath exports — not paths
                // with explicit filenames like "inner/cjs/index"
                if !spec.starts_with('.')
                    && !spec.starts_with('/')
                    && !spec.contains("/index")
                    && !spec.ends_with(".js")
                    && !spec.ends_with(".mjs")
                    && !spec.ends_with(".cjs")
                {
                    all_specs.insert(spec);
                }
            }
        }
    }
    // Need at least 2 distinct bare specifiers to trigger reorder
    if all_specs.len() < 2 {
        return;
    }

    // Reorder: ESM first, CJS second, default last
    let mut slots: Vec<usize> = non_nm.iter().map(|(pos, _)| *pos).collect();
    slots.sort();
    let mut by_ext: Vec<(u8, usize)> = non_nm
        .iter()
        .zip(exts.iter())
        .map(|((_, idx), ext)| (*ext, *idx))
        .collect();
    by_ext.sort_by_key(|(ext, _)| *ext);
    for (slot, (_, idx)) in slots.iter().zip(by_ext.iter()) {
        order[*slot] = *idx;
    }
}

fn node_triplet_extension_rank(name: &str, default_rank: usize, esm_rank: usize) -> usize {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".cts") || lower.ends_with(".cjs") {
        0
    } else if lower.ends_with(".mts") || lower.ends_with(".mjs") {
        esm_rank
    } else {
        default_rank
    }
}

fn reorder_explicit_relative_node_triplets(order: &mut [usize], test_case: &TestCase) {
    let mut groups: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    for (position, idx) in order.iter().copied().enumerate() {
        let name = normalize_header_path(&test_case.files[idx].name);
        groups
            .entry(strip_ts_js_extension(&name))
            .or_default()
            .push((position, idx));
    }

    for group in groups.values() {
        if group.len() != 3 {
            continue;
        }
        let has_all_extensions = [0, 1, 2].iter().all(|rank| {
            group.iter().any(|(_, idx)| {
                node_triplet_extension_rank(&test_case.files[*idx].name, 2, 1) == *rank
            })
        });
        let imports_explicit_triplet = group.iter().all(|(_, idx)| {
            let source = &test_case.files[*idx].content;
            source.contains("./index.js")
                && source.contains("./index.mjs")
                && source.contains("./index.cjs")
        });
        if !has_all_extensions || !imports_explicit_triplet {
            continue;
        }

        let typescript_triplet = group.iter().any(|(_, idx)| {
            let lower = test_case.files[*idx].name.to_ascii_lowercase();
            lower.ends_with(".ts") || lower.ends_with(".mts") || lower.ends_with(".cts")
        });
        let mut slots: Vec<usize> = group.iter().map(|(position, _)| *position).collect();
        slots.sort_unstable();
        let mut files: Vec<usize> = group.iter().map(|(_, idx)| *idx).collect();
        files.sort_by_key(|idx| {
            let (default_rank, esm_rank) = if typescript_triplet { (1, 2) } else { (2, 1) };
            node_triplet_extension_rank(&test_case.files[*idx].name, default_rank, esm_rank)
        });
        for (slot, idx) in slots.into_iter().zip(files) {
            order[slot] = idx;
        }
    }
}

fn reorder_cjs_self_export_triplet(order: &mut [usize], test_case: &TestCase) {
    if order.len() != 3 {
        return;
    }
    let bases: Vec<String> = order
        .iter()
        .map(|idx| strip_ts_js_extension(&normalize_header_path(&test_case.files[*idx].name)))
        .collect();
    if bases[0] != bases[1] || bases[1] != bases[2] {
        return;
    }
    if !order
        .iter()
        .all(|idx| test_case.files[*idx].content.contains("from \"#type\""))
    {
        return;
    }
    let exports_to_cjs = test_case.files.iter().any(|file| {
        basename(&file.name).eq_ignore_ascii_case("package.json")
            && file.content.contains("\"imports\"")
            && file.content.lines().any(|line| {
                let line = line.trim();
                line.starts_with("\"exports\"") && line.contains(".cjs")
            })
    });
    if !exports_to_cjs {
        return;
    }

    order.sort_by_key(|idx| node_triplet_extension_rank(&test_case.files[*idx].name, 1, 2));
}

fn strip_ts_js_extension(path: &str) -> String {
    for ext in [".mts", ".cts", ".ts", ".mjs", ".cjs", ".js", ".tsx", ".jsx"] {
        if let Some(base) = path.strip_suffix(ext) {
            return base.to_string();
        }
    }
    path.to_string()
}

fn order_remaining_via_scc(
    remaining: &[usize],
    deps: &HashMap<usize, Vec<usize>>,
    pos: &HashMap<usize, usize>,
) -> Vec<usize> {
    let rem_set: HashSet<usize> = remaining.iter().copied().collect();
    let mut forward: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut reverse: HashMap<usize, Vec<usize>> = HashMap::new();
    for node in remaining {
        forward.entry(*node).or_default();
        reverse.entry(*node).or_default();
    }
    for importer in remaining {
        if let Some(ds) = deps.get(importer) {
            for dep in ds {
                if !rem_set.contains(dep) || dep == importer {
                    continue;
                }
                forward.entry(*dep).or_default().push(*importer);
                reverse.entry(*importer).or_default().push(*dep);
            }
        }
    }

    let mut nodes = remaining.to_vec();
    nodes.sort_by_key(|n| pos.get(n).copied().unwrap_or(usize::MAX));

    fn dfs1(
        node: usize,
        graph: &HashMap<usize, Vec<usize>>,
        seen: &mut HashSet<usize>,
        finish: &mut Vec<usize>,
    ) {
        if !seen.insert(node) {
            return;
        }
        if let Some(nexts) = graph.get(&node) {
            for next in nexts {
                dfs1(*next, graph, seen, finish);
            }
        }
        finish.push(node);
    }

    fn dfs2(
        node: usize,
        graph: &HashMap<usize, Vec<usize>>,
        seen: &mut HashSet<usize>,
        comp: &mut Vec<usize>,
    ) {
        if !seen.insert(node) {
            return;
        }
        comp.push(node);
        if let Some(nexts) = graph.get(&node) {
            for next in nexts {
                dfs2(*next, graph, seen, comp);
            }
        }
    }

    let mut seen = HashSet::new();
    let mut finish = Vec::new();
    for node in &nodes {
        dfs1(*node, &forward, &mut seen, &mut finish);
    }

    let mut seen_rev = HashSet::new();
    let mut components: Vec<Vec<usize>> = Vec::new();
    for node in finish.into_iter().rev() {
        if seen_rev.contains(&node) {
            continue;
        }
        let mut comp = Vec::new();
        dfs2(node, &reverse, &mut seen_rev, &mut comp);
        comp.sort_by_key(|n| pos.get(n).copied().unwrap_or(usize::MAX));
        components.push(comp);
    }

    let mut comp_of: HashMap<usize, usize> = HashMap::new();
    for (cid, comp) in components.iter().enumerate() {
        for node in comp {
            comp_of.insert(*node, cid);
        }
    }

    let mut comp_adj: HashMap<usize, HashSet<usize>> = HashMap::new();
    let mut comp_indegree: HashMap<usize, usize> = HashMap::new();
    for cid in 0..components.len() {
        comp_adj.entry(cid).or_default();
        comp_indegree.entry(cid).or_insert(0);
    }
    for (u, nexts) in &forward {
        let Some(&cu) = comp_of.get(u) else {
            continue;
        };
        for v in nexts {
            let Some(&cv) = comp_of.get(v) else {
                continue;
            };
            if cu == cv {
                continue;
            }
            let entry = comp_adj.entry(cu).or_default();
            if entry.insert(cv) {
                *comp_indegree.entry(cv).or_insert(0) += 1;
            }
        }
    }

    let comp_min_pos = |cid: usize| {
        components[cid]
            .iter()
            .map(|n| pos.get(n).copied().unwrap_or(usize::MAX))
            .min()
            .unwrap_or(usize::MAX)
    };
    let mut ready: Vec<usize> = (0..components.len())
        .filter(|cid| comp_indegree.get(cid).copied().unwrap_or(0) == 0)
        .collect();
    ready.sort_by_key(|cid| comp_min_pos(*cid));

    let mut ordered = Vec::new();
    while let Some(cid) = ready.first().copied() {
        ready.remove(0);
        ordered.extend(components[cid].iter().copied());
        if let Some(nexts) = comp_adj.get(&cid) {
            for next in nexts {
                if let Some(d) = comp_indegree.get_mut(next) {
                    if *d > 0 {
                        *d -= 1;
                        if *d == 0 {
                            ready.push(*next);
                        }
                    }
                }
            }
        }
        ready.sort_by_key(|cid| comp_min_pos(*cid));
    }

    if ordered.len() < remaining.len() {
        let mut tail: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|n| !ordered.contains(n))
            .collect();
        tail.sort_by_key(|n| pos.get(n).copied().unwrap_or(usize::MAX));
        ordered.extend(tail);
    }
    ordered
}

fn order_remaining_via_dependency_postorder(
    remaining: &[usize],
    deps: &HashMap<usize, Vec<usize>>,
    pos: &HashMap<usize, usize>,
) -> Vec<usize> {
    let allowed: HashSet<usize> = remaining.iter().copied().collect();
    let mut nodes = remaining.to_vec();
    nodes.sort_by_key(|idx| pos.get(idx).copied().unwrap_or(usize::MAX));

    let mut state: HashMap<usize, u8> = HashMap::new();
    let mut ordered = Vec::with_capacity(remaining.len());
    for root in nodes {
        let mut stack = vec![(root, false)];
        while let Some((node, expanded)) = stack.pop() {
            if !allowed.contains(&node) {
                continue;
            }
            let current = state.get(&node).copied().unwrap_or(0);
            if expanded {
                if current != 2 {
                    state.insert(node, 2);
                    ordered.push(node);
                }
                continue;
            }
            if current == 2 || current == 1 {
                continue;
            }
            state.insert(node, 1);
            stack.push((node, true));
            if let Some(ds) = deps.get(&node) {
                let mut ordered_deps: Vec<usize> = ds
                    .iter()
                    .copied()
                    .filter(|dep| allowed.contains(dep))
                    .collect();
                ordered_deps.sort_by_key(|dep| pos.get(dep).copied().unwrap_or(usize::MAX));
                ordered_deps.dedup();
                for dep in ordered_deps.into_iter().rev() {
                    stack.push((dep, false));
                }
            }
        }
    }

    if ordered.len() < remaining.len() {
        let mut tail: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|n| !ordered.contains(n))
            .collect();
        tail.sort_by_key(|idx| pos.get(idx).copied().unwrap_or(usize::MAX));
        ordered.extend(tail);
    }

    ordered
}

fn apply_link_path_aliases(
    path_to_idx: &mut HashMap<String, usize>,
    test_case: &TestCase,
    link_path_mappings: &[(String, String)],
) {
    for (idx, file) in test_case.files.iter().enumerate() {
        let normalized = normalize_header_path(&file.name);
        for (from, to) in link_path_mappings {
            let Some(rel) = strip_prefix_path(&normalized, from) else {
                continue;
            };
            let alias = if rel.is_empty() {
                to.clone()
            } else {
                join_path(to, &rel)
            };
            path_to_idx.entry(alias).or_insert(idx);
        }
    }
}

pub(crate) fn source_file_looks_like_module(source: &SourceFile) -> bool {
    source.statements.iter().any(|stmt| match &stmt.kind {
        tsc_rs_ast::StmtKind::Import(imp) => !imp.source.is_empty(),
        tsc_rs_ast::StmtKind::Export(_) | tsc_rs_ast::StmtKind::ExportAssign(_) => true,
        _ => false,
    })
}

pub(crate) fn dependency_indices_for_file(
    file: &TestFile,
    parsed: &SourceFile,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
) -> Vec<usize> {
    dependency_indices_for_file_with_options(file, parsed, test_case, path_to_idx, ctx, false)
}

fn dependency_indices_for_file_with_options(
    file: &TestFile,
    parsed: &SourceFile,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
    include_ambient_external_providers: bool,
) -> Vec<usize> {
    // When `noResolve: true`, TypeScript does not resolve imports and emits
    // files in source order. Return empty deps to preserve source ordering.
    if ctx.no_resolve {
        return Vec::new();
    }

    let mut deps = Vec::new();

    for line in file.content.lines() {
        if let Some(path) = parse_triple_slash_reference(line) {
            push_unique_dep(
                &mut deps,
                resolve_reference_specifier(&file.name, &path, test_case, path_to_idx, ctx),
            );
        }
        for spec in parse_from_specifiers(line) {
            push_module_dep_indices(
                &mut deps,
                &file.name,
                &spec,
                test_case,
                path_to_idx,
                ctx,
                include_ambient_external_providers,
            );
        }
        for spec in parse_import_call_specifiers(line) {
            push_module_dep_indices(
                &mut deps,
                &file.name,
                &spec,
                test_case,
                path_to_idx,
                ctx,
                include_ambient_external_providers,
            );
        }
        // Top-level `require("...")` calls.
        // For TS files: only `import X = require(...)` (guarded by
        // source_file_has_import_equals_require).
        // For JS files: also bare `const X = require(...)` which is the
        // standard CJS import pattern.
        {
            let is_js_file = {
                let lower = file.name.to_lowercase();
                lower.ends_with(".js")
                    || lower.ends_with(".jsx")
                    || lower.ends_with(".mjs")
                    || lower.ends_with(".cjs")
            };
            let should_scan = if is_js_file {
                !line.starts_with(' ') && !line.starts_with('\t')
            } else {
                source_file_has_import_equals_require(line)
            };
            if should_scan {
                for spec in parse_require_specifiers(line) {
                    push_module_dep_indices(
                        &mut deps,
                        &file.name,
                        &spec,
                        test_case,
                        path_to_idx,
                        ctx,
                        include_ambient_external_providers,
                    );
                }
            }
        }
    }

    for stmt in &parsed.statements {
        match &stmt.kind {
            tsc_rs_ast::StmtKind::Import(d) => {
                push_module_dep_indices(
                    &mut deps,
                    &file.name,
                    &d.source,
                    test_case,
                    path_to_idx,
                    ctx,
                    include_ambient_external_providers,
                );
            }
            // Top-level `import X = require("...")` — these have no `from` keyword
            // so they aren't caught by the line-based scan above.
            tsc_rs_ast::StmtKind::ImportEquals(ie) => {
                if let Some(spec) = require_specifier_from_expr(&ie.module_ref) {
                    push_module_dep_indices(
                        &mut deps,
                        &file.name,
                        spec,
                        test_case,
                        path_to_idx,
                        ctx,
                        include_ambient_external_providers,
                    );
                }
            }
            tsc_rs_ast::StmtKind::Export(ed) => match &ed.kind {
                tsc_rs_ast::ExportDeclKind::Named {
                    source: Some(source),
                    ..
                } => push_module_dep_indices(
                    &mut deps,
                    &file.name,
                    source,
                    test_case,
                    path_to_idx,
                    ctx,
                    include_ambient_external_providers,
                ),
                tsc_rs_ast::ExportDeclKind::All { source, .. } => push_module_dep_indices(
                    &mut deps,
                    &file.name,
                    source,
                    test_case,
                    path_to_idx,
                    ctx,
                    include_ambient_external_providers,
                ),
                tsc_rs_ast::ExportDeclKind::Decl(inner)
                | tsc_rs_ast::ExportDeclKind::DefaultDecl(inner) => match &inner.kind {
                    tsc_rs_ast::StmtKind::Import(d) => {
                        push_module_dep_indices(
                            &mut deps,
                            &file.name,
                            &d.source,
                            test_case,
                            path_to_idx,
                            ctx,
                            include_ambient_external_providers,
                        );
                    }
                    tsc_rs_ast::StmtKind::ImportEquals(ie) => {
                        if let Some(spec) = require_specifier_from_expr(&ie.module_ref) {
                            push_module_dep_indices(
                                &mut deps,
                                &file.name,
                                spec,
                                test_case,
                                path_to_idx,
                                ctx,
                                include_ambient_external_providers,
                            );
                        }
                    }
                    tsc_rs_ast::StmtKind::ExportAssign(expr) => {
                        if let Some(spec) = require_specifier_from_expr(expr) {
                            push_module_dep_indices(
                                &mut deps,
                                &file.name,
                                spec,
                                test_case,
                                path_to_idx,
                                ctx,
                                include_ambient_external_providers,
                            );
                        }
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
    }

    // Add implicit JSX runtime dependency for .tsx/.jsx files when
    // jsxImportSource is set (react-jsx / react-jsxdev modes).
    if let Some(ref jsx_source) = test_case.options.jsx_import_source {
        let lower = file.name.to_ascii_lowercase();
        if lower.ends_with(".tsx") || lower.ends_with(".jsx") {
            // Pick the runtime based on the active JSX mode.
            let runtime = match test_case.options.jsx {
                Some(tsc_rs_ast::JsxEmit::ReactJSXDev) => "jsx-dev-runtime",
                _ => "jsx-runtime",
            };
            let spec = format!("{}/{}", jsx_source, runtime);
            push_module_dep_indices(
                &mut deps,
                &file.name,
                &spec,
                test_case,
                path_to_idx,
                ctx,
                include_ambient_external_providers,
            );
        }
    }

    deps
}

fn push_unique_dep(deps: &mut Vec<usize>, dep_idx: Option<usize>) {
    if let Some(dep_idx) = dep_idx {
        if !deps.contains(&dep_idx) {
            deps.push(dep_idx);
        }
    }
}

fn push_module_dep_indices(
    deps: &mut Vec<usize>,
    importer_name: &str,
    spec: &str,
    test_case: &TestCase,
    path_to_idx: &HashMap<String, usize>,
    ctx: &BaselinePathContext,
    include_ambient_external_providers: bool,
) {
    let spec = spec.trim();
    if spec.is_empty() {
        return;
    }
    if include_ambient_external_providers
        && !spec.starts_with("./")
        && !spec.starts_with("../")
        && !spec.starts_with('/')
    {
        let ambient_providers =
            ambient_external_module_provider_indices(spec, importer_name, test_case);
        if !ambient_providers.is_empty() {
            for dep_idx in ambient_providers {
                push_unique_dep(deps, Some(dep_idx));
            }
            return;
        }
    }
    push_unique_dep(
        deps,
        resolve_module_specifier(importer_name, spec, test_case, path_to_idx, ctx),
    );
}

fn ambient_external_module_provider_indices(
    spec: &str,
    importer_name: &str,
    test_case: &TestCase,
) -> Vec<usize> {
    let mut providers = Vec::new();
    let importer_name = normalize_header_path(importer_name);
    let double_quoted = format!("declare module \"{spec}\"");
    let single_quoted = format!("declare module '{spec}'");

    for (idx, file) in test_case.files.iter().enumerate() {
        let content = &file.content;
        if !content.contains(&double_quoted) && !content.contains(&single_quoted) {
            continue;
        }
        if normalize_header_path(&file.name) == importer_name {
            continue;
        }
        if file_looks_like_module_augmentation_for_spec(content, spec) {
            continue;
        }
        providers.push(idx);
    }

    providers
}

fn file_looks_like_module_augmentation_for_spec(content: &str, spec: &str) -> bool {
    for line in content.lines() {
        if parse_from_specifiers(line)
            .iter()
            .any(|found| found == spec)
            || parse_import_call_specifiers(line)
                .iter()
                .any(|found| found == spec)
            || parse_require_specifiers(line)
                .iter()
                .any(|found| found == spec)
            || parse_side_effect_import_specifier(line).as_deref() == Some(spec)
        {
            return true;
        }
    }
    false
}

fn parse_side_effect_import_specifier(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("import")?;
    let trimmed = rest.trim_start();
    let quote = trimmed.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let after_quote = &trimmed[quote.len_utf8()..];
    let end = after_quote.find(quote)?;
    Some(after_quote[..end].to_string())
}

pub(crate) fn referenced_json_dependency_indices(
    test_case: &TestCase,
    compile_indices: &[usize],
    parsed_sources: &HashMap<usize, SourceFile>,
    ctx: &BaselinePathContext,
) -> Vec<usize> {
    if compile_indices.is_empty() {
        return Vec::new();
    }

    let mut path_to_idx = HashMap::new();
    for (idx, file) in test_case.files.iter().enumerate() {
        path_to_idx.insert(normalize_header_path(&file.name), idx);
    }

    let mut referenced: HashSet<usize> = HashSet::new();
    for idx in compile_indices {
        let file = &test_case.files[*idx];
        let parsed = parsed_sources
            .get(idx)
            .expect("missing parsed source while collecting json dependencies");
        for dep_idx in dependency_indices_for_file(file, parsed, test_case, &path_to_idx, ctx) {
            let dep_name = &test_case.files[dep_idx].name;
            let lower = dep_name.to_ascii_lowercase();
            if lower.ends_with(".json")
                && !lower.ends_with("tsconfig.json")
                && !is_node_modules_path(dep_name)
            {
                referenced.insert(dep_idx);
            }
        }
    }

    test_case
        .files
        .iter()
        .enumerate()
        .filter_map(|(idx, _)| referenced.contains(&idx).then_some(idx))
        .collect()
}

fn root_dependency_postorder(
    root: usize,
    compile_indices: &[usize],
    deps: &HashMap<usize, Vec<usize>>,
) -> Vec<usize> {
    let allowed: HashSet<usize> = compile_indices.iter().copied().collect();
    let mut state: HashMap<usize, u8> = HashMap::new();
    let mut order = Vec::new();
    let mut stack = vec![(root, false)];
    while let Some((node, expanded)) = stack.pop() {
        if !allowed.contains(&node) {
            continue;
        }
        let current = state.get(&node).copied().unwrap_or(0);
        if expanded {
            if current != 2 {
                state.insert(node, 2);
                order.push(node);
            }
            continue;
        }
        if current == 2 || current == 1 {
            continue;
        }
        state.insert(node, 1);
        stack.push((node, true));
        if let Some(ds) = deps.get(&node) {
            for dep in ds.iter().rev() {
                if allowed.contains(dep) {
                    stack.push((*dep, false));
                }
            }
        }
    }
    order
}

fn should_use_root_dependency_postorder(
    test_case: &TestCase,
    root: usize,
    ctx: &BaselinePathContext,
    module_kind: Option<ModuleKind>,
) -> bool {
    if !source_file_has_ordering_signals(&test_case.files[root].content) {
        return false;
    }

    // Node-style allowJs baselines still follow the normal dependency
    // ordering path even if the root JS file uses `import = require(...)`.
    if ctx.allow_js
        && matches!(
            module_kind,
            Some(ModuleKind::Node16)
                | Some(ModuleKind::Node18)
                | Some(ModuleKind::Node20)
                | Some(ModuleKind::NodeNext)
        )
        && is_js_like_source_name(&test_case.files[root].name)
    {
        return false;
    }

    true
}

fn source_file_has_ordering_signals(source: &str) -> bool {
    source_file_has_import_equals_require(source)
        || source
            .lines()
            .any(|line| parse_triple_slash_reference(line).is_some())
}

fn is_js_like_source_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".js")
        || lower.ends_with(".jsx")
        || lower.ends_with(".mjs")
        || lower.ends_with(".cjs")
}
