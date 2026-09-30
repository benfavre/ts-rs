use std::sync::Arc;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tsc_rs_ast::{
    CompilerOptions, ExportDeclKind, Expr, ExprKind, ModuleBody, ModuleName, PatKind, PropName,
    ScriptTarget, SourceFile, Stmt, StmtKind, TypeMemberKind, TypeNode, TypeNodeKind,
};

#[derive(Debug, Clone)]
pub struct StdLibSource {
    pub file_name: String,
    pub source: String,
}

static TYPESCRIPT_LIB_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
static LIB_MEMBER_METADATA: OnceLock<Option<Arc<LibMemberMetadata>>> = OnceLock::new();
static LIB_MEMBER_AVAILABILITY: OnceLock<Mutex<HashMap<String, Arc<StdLibMemberAvailability>>>> =
    OnceLock::new();

type MemberKey = (String, String);

#[derive(Debug, Clone, Default)]
struct LibraryDeclarations {
    members: HashSet<MemberKey>,
    owners: HashSet<String>,
    types: HashSet<String>,
    values: HashSet<String>,
    value_owners: HashMap<String, String>,
    constructor_returns: HashMap<String, String>,
}

impl LibraryDeclarations {
    fn extend(&mut self, other: &Self) {
        self.members.extend(other.members.iter().cloned());
        self.owners.extend(other.owners.iter().cloned());
        self.types.extend(other.types.iter().cloned());
        self.values.extend(other.values.iter().cloned());
        self.value_owners.extend(other.value_owners.clone());
        self.constructor_returns
            .extend(other.constructor_returns.clone());
    }
}

#[derive(Debug)]
struct LibMemberMetadata {
    recommendations: HashMap<MemberKey, &'static str>,
    global_recommendations: HashMap<String, &'static str>,
    ecmascript_owners: HashSet<String>,
    value_owners: HashMap<String, String>,
    constructor_instances: HashMap<String, String>,
    declarations_by_file: HashMap<String, LibraryDeclarations>,
}

/// Standard-library declarations available under one compiler-option set.
///
/// This is intentionally declaration-derived rather than a second hand-written
/// table of JavaScript editions. It lets the compact builtin checker preserve
/// useful future member types while still diagnosing members excluded by the
/// selected `target`/`lib`.
#[derive(Debug)]
pub(crate) struct StdLibMemberAvailability {
    active: HashSet<MemberKey>,
    active_owners: HashSet<String>,
    active_types: HashSet<String>,
    active_values: HashSet<String>,
    metadata: Arc<LibMemberMetadata>,
}

impl StdLibMemberAvailability {
    /// The configured libs declare `name` as a global type or value.
    pub(crate) fn declares_global(&self, name: &str) -> bool {
        self.active_types.contains(name) || self.active_values.contains(name)
    }

    /// Global type names the configured libs declare.
    /// Global VALUE names the configured libs declare (`Error`, `Math`, …).
    pub(crate) fn global_value_names(&self) -> impl Iterator<Item = &str> {
        self.active_values.iter().map(String::as_str)
    }

    pub(crate) fn global_type_names(&self) -> impl Iterator<Item = &str> {
        self.active_types.iter().map(String::as_str)
    }

    /// Whether the configured libs declare `owner` at all, and if so whether
    /// they declare `property` on it.
    pub(crate) fn declares_member(&self, owner: &str, property: &str) -> Option<bool> {
        if !self.active_owners.contains(owner) {
            return None;
        }
        Some(
            self.active
                .contains(&(owner.to_string(), property.to_string())),
        )
    }

    pub(crate) fn recommendation(&self, owner: &str, property: &str) -> Option<&'static str> {
        let key = (owner.to_string(), property.to_string());
        if self.active.contains(&key) {
            None
        } else {
            self.metadata.recommendations.get(&key).copied()
        }
    }

    pub(crate) fn global_recommendation(&self, name: &str) -> Option<&'static str> {
        // TypeScript only chooses TS2583 for this explicit set of missing
        // standard globals. Other names declared by a newer library (for
        // example `Proxy`) retain the general TS2304 diagnostic even though
        // the declaration index can still identify a suggested library.
        if !is_future_lib_global_diagnostic_name(name)
            || self.active_types.contains(name)
            || self.active_values.contains(name)
        {
            None
        } else {
            self.suggested_lib(name)
        }
    }

    /// The library tsc names for a missing standard global: the FIRST key of
    /// its per-type feature table (`getScriptTargetFeatures`), which is not
    /// always the library that declares the name (`AsyncIterator` → es2015).
    fn suggested_lib(&self, name: &str) -> Option<&'static str> {
        tsc_feature_table_first_lib(name)
            .or_else(|| self.metadata.global_recommendations.get(name).copied())
    }

    /// The configured libs declare `name` as a VALUE (not merely a type).
    pub(crate) fn declares_global_value(&self, name: &str) -> bool {
        self.active_values.contains(name)
    }

    /// Library to suggest when `name` is a standard global whose TYPE the
    /// configured libs declare but whose VALUE they do not (TS2585:
    /// `Symbol()` / `Promise.resolve` with only es5).
    pub(crate) fn type_only_value_recommendation(&self, name: &str) -> Option<&'static str> {
        if !is_future_lib_global_diagnostic_name(name)
            || !self.active_types.contains(name)
            || self.active_values.contains(name)
        {
            None
        } else {
            self.suggested_lib(name)
        }
    }

    pub(crate) fn value_owner(&self, path: &str) -> Option<&str> {
        self.metadata.value_owners.get(path).map(String::as_str)
    }

    pub(crate) fn value_is_known(&self, path: &str) -> bool {
        self.metadata.value_owners.contains_key(path)
    }

    pub(crate) fn owner_is_active(&self, owner: &str) -> bool {
        self.active_owners.contains(owner)
            || owner
                .strip_prefix("typeof ")
                .is_some_and(|value| self.active_values.contains(value))
    }

    pub(crate) fn owner_is_ecmascript(&self, owner: &str) -> bool {
        self.metadata.ecmascript_owners.contains(owner)
    }

    pub(crate) fn value_is_active(&self, path: &str) -> bool {
        self.active_values.contains(path)
    }

    /// Whether the configured lib files declare `property` on `owner`.
    pub(crate) fn member_is_active(&self, owner: &str, property: &str) -> bool {
        self.active
            .contains(&(owner.to_string(), property.to_string()))
    }

    pub(crate) fn constructor_instance(&self, path: &str) -> Option<&str> {
        self.metadata
            .constructor_instances
            .get(path)
            .map(String::as_str)
    }
}

static ANY_LIB_GLOBALS: OnceLock<HashSet<String>> = OnceLock::new();

/// `name` is declared as a global type or value by ANY TypeScript lib file
/// (`lib.*.d.ts`), whatever the configured libs. Used as a conservative
/// "known" test where `/// <reference lib="...">` directives may have widened
/// the program's libs.
pub(crate) fn any_lib_declares_global(name: &str) -> bool {
    ANY_LIB_GLOBALS
        .get_or_init(|| {
            let mut names = HashSet::new();
            let Some(lib_dir) = find_typescript_lib_dir() else {
                return names;
            };
            let Ok(entries) = std::fs::read_dir(lib_dir) else {
                return names;
            };
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().into_owned();
                if !(file_name.starts_with("lib.") && file_name.ends_with(".d.ts")) {
                    continue;
                }
                let Ok(source) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                let declarations =
                    extract_library_declarations(&StdLibSource { file_name, source });
                names.extend(declarations.types);
                names.extend(declarations.values);
            }
            names
        })
        .contains(name)
}

static LIB_FILE_BY_GLOBAL: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();

/// The library file declaring global `name`, preferring `lib.es5.d.ts` (the
/// first lib in program order) and then the alphabetically first lib, which
/// is what tsc's "declared here" note names for lib symbols.
pub(crate) fn lib_file_declaring_global(name: &str) -> Option<String> {
    LIB_FILE_BY_GLOBAL
        .get_or_init(|| {
            let mut by_name = std::collections::HashMap::new();
            let Some(lib_dir) = find_typescript_lib_dir() else {
                return by_name;
            };
            let Ok(entries) = std::fs::read_dir(lib_dir) else {
                return by_name;
            };
            let mut files: Vec<(String, std::path::PathBuf)> = entries
                .flatten()
                .map(|entry| {
                    (
                        entry.file_name().to_string_lossy().into_owned(),
                        entry.path(),
                    )
                })
                .filter(|(file_name, _)| {
                    file_name.starts_with("lib.") && file_name.ends_with(".d.ts")
                })
                .collect();
            files.sort_by(|(a, _), (b, _)| {
                (a != "lib.es5.d.ts")
                    .cmp(&(b != "lib.es5.d.ts"))
                    .then_with(|| a.cmp(b))
            });
            for (file_name, path) in files {
                let Ok(source) = std::fs::read_to_string(path) else {
                    continue;
                };
                let declarations = extract_library_declarations(&StdLibSource {
                    file_name: file_name.clone(),
                    source,
                });
                for declared in declarations.values.into_iter().chain(declarations.types) {
                    by_name.entry(declared).or_insert_with(|| file_name.clone());
                }
            }
            by_name
        })
        .get(name)
        .cloned()
}

/// First library key of tsc's `getScriptTargetFeatures` entry for a global
/// type name (TypeScript 6.0.3). Only the names that can produce TS2583 /
/// TS2585 are listed.
fn tsc_feature_table_first_lib(name: &str) -> Option<&'static str> {
    Some(match name {
        "Map" | "Set" | "Promise" | "Symbol" | "WeakMap" | "WeakSet" | "Iterator"
        | "AsyncIterator" | "Reflect" => "es2015",
        "SharedArrayBuffer" | "Atomics" => "es2017",
        "AsyncIterable" | "AsyncIterableIterator" | "AsyncGenerator" | "AsyncGeneratorFunction" => {
            "es2018"
        }
        "BigInt" | "BigInt64Array" | "BigUint64Array" => "es2020",
        _ => return None,
    })
}

pub(crate) fn is_future_lib_global_diagnostic_name(name: &str) -> bool {
    matches!(
        name,
        "Map"
            | "Set"
            | "Promise"
            | "Symbol"
            | "WeakMap"
            | "WeakSet"
            | "Iterator"
            | "AsyncIterator"
            | "SharedArrayBuffer"
            | "Atomics"
            | "AsyncIterable"
            | "AsyncIterableIterator"
            | "AsyncGenerator"
            | "AsyncGeneratorFunction"
            | "BigInt"
            | "Reflect"
            | "BigInt64Array"
            | "BigUint64Array"
    )
}

pub(crate) fn stdlib_member_availability(
    options: &CompilerOptions,
) -> Option<Arc<StdLibMemberAvailability>> {
    let metadata = LIB_MEMBER_METADATA
        .get_or_init(build_lib_member_metadata)
        .clone()?;
    let cache = LIB_MEMBER_AVAILABILITY.get_or_init(|| Mutex::new(HashMap::new()));
    let key = format!(
        "{:?}\0{}\0{}",
        options.target.unwrap_or(ScriptTarget::ES5),
        options.no_lib == Some(true),
        options.lib.join("\0").to_ascii_lowercase()
    );
    if let Some(cached) = cache.lock().ok()?.get(&key).cloned() {
        return Some(cached);
    }

    let mut active = HashSet::new();
    let mut active_owners = HashSet::new();
    let mut active_types = HashSet::new();
    let mut active_values = HashSet::new();
    for source in load_stdlib_sources(options) {
        let declarations =
            if let Some(declarations) = metadata.declarations_by_file.get(&source.file_name) {
                declarations.clone()
            } else {
                extract_library_declarations(&source)
            };
        active.extend(declarations.members);
        active_owners.extend(declarations.owners);
        active_types.extend(declarations.types);
        active_values.extend(declarations.values);
    }
    let availability = Arc::new(StdLibMemberAvailability {
        active,
        active_owners,
        active_types,
        active_values,
        metadata,
    });
    cache.lock().ok()?.insert(key, availability.clone());
    Some(availability)
}

fn build_lib_member_metadata() -> Option<Arc<LibMemberMetadata>> {
    find_typescript_lib_dir()?;
    let editions = [
        (ScriptTarget::ES5, None),
        (ScriptTarget::ES2015, Some("es2015")),
        (ScriptTarget::ES2016, Some("es2016")),
        (ScriptTarget::ES2017, Some("es2017")),
        (ScriptTarget::ES2018, Some("es2018")),
        (ScriptTarget::ES2019, Some("es2019")),
        (ScriptTarget::ES2020, Some("es2020")),
        (ScriptTarget::ES2021, Some("es2021")),
        (ScriptTarget::ES2022, Some("es2022")),
        (ScriptTarget::ES2023, Some("es2023")),
        (ScriptTarget::ES2024, Some("es2024")),
        (ScriptTarget::ES2025, Some("es2025")),
        (ScriptTarget::ESNext, Some("esnext")),
    ];

    let mut declarations_by_file = HashMap::new();
    let mut cumulative = Vec::new();
    for (target, recommendation) in editions {
        let options = CompilerOptions {
            target: Some(target),
            // Use the language bundle itself rather than the target's `full`
            // default. Host libraries may reference newer language bundles
            // (TypeScript 6's `dom` references `es2015`), which would make an
            // ES5 baseline appear to contain Array.from/Math.trunc.
            lib: vec![recommendation.unwrap_or("es5").to_string()],
            ..CompilerOptions::default()
        };
        let sources = load_stdlib_sources(&options);
        let mut declarations = LibraryDeclarations::default();
        for source in sources {
            let per_file = declarations_by_file
                .entry(source.file_name.clone())
                .or_insert_with(|| extract_library_declarations(&source));
            declarations.extend(per_file);
        }
        cumulative.push(declarations);
    }

    let base_members = &cumulative[0].members;
    let base_globals: HashSet<_> = cumulative[0]
        .types
        .iter()
        .chain(&cumulative[0].values)
        .cloned()
        .collect();
    let mut recommendations = HashMap::new();
    let mut global_recommendations = HashMap::new();
    for ((_, recommendation), declarations) in editions.iter().zip(&cumulative).skip(1) {
        let recommendation = recommendation.expect("post-ES5 editions have a label");
        for member in &declarations.members {
            // A declaration already present in the baseline library is not a
            // "newer lib" feature. This matters for partial explicit `lib`
            // selections such as `es2015.core`, which intentionally omit
            // fundamental ES5 and DOM declarations.
            if base_members.contains(member) {
                continue;
            }
            recommendations
                .entry(member.clone())
                .or_insert(recommendation);
        }
        for name in declarations.types.iter().chain(&declarations.values) {
            if !base_globals.contains(name) {
                global_recommendations
                    .entry(name.clone())
                    .or_insert(recommendation);
            }
        }
    }

    let mut all = LibraryDeclarations::default();
    for declarations in &cumulative {
        all.extend(declarations);
    }
    let constructor_instances = all
        .value_owners
        .iter()
        .filter_map(|(value_path, constructor_owner)| {
            all.constructor_returns
                .get(constructor_owner)
                .map(|instance| (value_path.clone(), instance.clone()))
        })
        .collect();

    Some(Arc::new(LibMemberMetadata {
        recommendations,
        global_recommendations,
        ecmascript_owners: all.owners.clone(),
        value_owners: all.value_owners,
        constructor_instances,
        declarations_by_file,
    }))
}

fn extract_library_declarations(source: &StdLibSource) -> LibraryDeclarations {
    let file = tsc_rs_parser::parse(&source.file_name, &source.source);
    let mut declarations = LibraryDeclarations::default();
    collect_library_statements(&file, &file.statements, "", &mut declarations);
    declarations
}

fn collect_library_statements(
    file: &SourceFile,
    statements: &[Stmt],
    namespace: &str,
    declarations: &mut LibraryDeclarations,
) {
    for statement in statements {
        match &statement.kind {
            StmtKind::InterfaceDecl(interface) => {
                declarations.owners.insert(interface.name.clone());
                declarations
                    .types
                    .insert(qualify(namespace, &interface.name));
                for member in &interface.members {
                    match &member.kind {
                        TypeMemberKind::PropertySig(signature) => {
                            if let Some(name) = library_property_name(&signature.name) {
                                declarations
                                    .members
                                    .insert((interface.name.clone(), name.to_string()));
                            }
                        }
                        TypeMemberKind::MethodSig(signature) => {
                            if let Some(name) = library_property_name(&signature.name) {
                                declarations
                                    .members
                                    .insert((interface.name.clone(), name.to_string()));
                            }
                        }
                        TypeMemberKind::GetAccessorSig(signature)
                        | TypeMemberKind::SetAccessorSig(signature) => {
                            if let Some(name) = library_property_name(&signature.name) {
                                declarations
                                    .members
                                    .insert((interface.name.clone(), name.to_string()));
                            }
                        }
                        TypeMemberKind::ConstructSig(signature) => {
                            if let Some(display) = signature
                                .return_type
                                .as_ref()
                                .and_then(|return_type| simple_type_display(file, return_type))
                            {
                                declarations
                                    .constructor_returns
                                    .entry(interface.name.clone())
                                    .or_insert(display);
                            }
                        }
                        TypeMemberKind::CallSig(_) | TypeMemberKind::IndexSig(_) => {}
                    }
                }
            }
            StmtKind::Var(variable) => {
                for declarator in &variable.declarations {
                    let PatKind::Ident(name) = &declarator.name.kind else {
                        continue;
                    };
                    let value_path = qualify(namespace, name);
                    declarations.values.insert(value_path.clone());
                    if !namespace.is_empty() {
                        declarations
                            .members
                            .insert((format!("typeof {namespace}"), name.to_string()));
                    }
                    if let Some(owner) = declarator
                        .type_ann
                        .as_ref()
                        .and_then(|node| simple_type_name(file, node))
                    {
                        declarations.value_owners.insert(value_path, owner);
                    }
                }
            }
            StmtKind::FnDecl(function) => {
                if let Some(name) = &function.name {
                    declarations.values.insert(qualify(namespace, name));
                    if !namespace.is_empty() {
                        declarations
                            .members
                            .insert((format!("typeof {namespace}"), name.clone()));
                    }
                }
            }
            StmtKind::ClassDecl(class) => {
                if let Some(name) = &class.name {
                    declarations.owners.insert(name.clone());
                    declarations.types.insert(qualify(namespace, name));
                    declarations.values.insert(qualify(namespace, name));
                    if !namespace.is_empty() {
                        declarations
                            .members
                            .insert((format!("typeof {namespace}"), name.clone()));
                    }
                }
            }
            StmtKind::TypeAlias(type_alias) => {
                declarations
                    .types
                    .insert(qualify(namespace, &type_alias.name));
            }
            StmtKind::ModuleDecl(module) => {
                let name = match &module.name {
                    ModuleName::Ident(name) | ModuleName::String(name) => name,
                };
                let nested = qualify(namespace, name);
                declarations.values.insert(nested.clone());
                if let Some(body) = &module.body {
                    match body {
                        ModuleBody::Block(inner) => {
                            collect_library_statements(file, inner, &nested, declarations);
                        }
                        ModuleBody::Module(inner) => {
                            let synthetic = Stmt {
                                kind: StmtKind::ModuleDecl(inner.clone()),
                                span: inner.span,
                            };
                            collect_library_statements(
                                file,
                                std::slice::from_ref(&synthetic),
                                &nested,
                                declarations,
                            );
                        }
                    }
                }
            }
            StmtKind::Export(export) => {
                if let ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) =
                    &export.kind
                {
                    collect_library_statements(
                        file,
                        std::slice::from_ref(inner),
                        namespace,
                        declarations,
                    );
                }
            }
            _ => {}
        }
    }
}

fn qualify(namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        name.to_string()
    } else {
        format!("{namespace}.{name}")
    }
}

fn library_property_name(name: &PropName) -> Option<&str> {
    match name {
        PropName::Ident(name, _) | PropName::String(name, _) | PropName::Private(name, _) => {
            Some(name)
        }
        PropName::Number(_, _) | PropName::Computed(_, _) => None,
    }
}

fn simple_type_name(file: &SourceFile, node: &TypeNode) -> Option<String> {
    let TypeNodeKind::Reference(reference) = &node.kind else {
        return None;
    };
    simple_expr_name(&reference.name).or_else(|| {
        file.text
            .get(reference.name.span.start as usize..reference.name.span.end as usize)
            .map(str::to_string)
    })
}

fn simple_expr_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.to_string()),
        ExprKind::Member(member) => Some(format!(
            "{}.{}",
            simple_expr_name(&member.object)?,
            member.property
        )),
        _ => None,
    }
}

fn simple_type_display(file: &SourceFile, node: &TypeNode) -> Option<String> {
    file.text
        .get(node.span.start as usize..node.span.end as usize)
        .map(str::trim)
        .filter(|display| !display.is_empty())
        .map(str::to_string)
}

pub fn load_stdlib_sources(options: &CompilerOptions) -> Vec<StdLibSource> {
    if options.no_lib == Some(true) {
        return Vec::new();
    }

    let Some(lib_dir) = find_typescript_lib_dir() else {
        return Vec::new();
    };

    let mut sources = Vec::new();
    let mut visited = HashSet::new();

    for root in root_lib_entries(options, lib_dir) {
        load_lib_recursive(lib_dir, &root, &mut visited, &mut sources);
    }

    sources
}

pub fn load_combined_stdlib_source(options: &CompilerOptions) -> Option<String> {
    let sources = load_stdlib_sources(options);
    if sources.is_empty() {
        return None;
    }

    let mut combined = String::new();
    for lib in sources {
        combined.push_str("// ");
        combined.push_str(&lib.file_name);
        combined.push('\n');
        combined.push_str(&lib.source);
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push('\n');
    }
    Some(combined)
}

fn find_typescript_lib_dir() -> Option<&'static PathBuf> {
    TYPESCRIPT_LIB_DIR
        .get_or_init(|| {
            if let Ok(explicit) = std::env::var("TSC_RS_TYPESCRIPT_LIB_DIR") {
                let path = PathBuf::from(explicit);
                if path.is_dir() {
                    return Some(path);
                }
            }

            let mut search_roots = Vec::new();
            search_roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
            if let Ok(current) = std::env::current_dir() {
                search_roots.push(current);
            }

            // Prefer a project/workspace `node_modules/typescript` from any
            // ancestor (the TypeScript the baselines were produced with); the
            // editor extension's bundled copy is only a last-resort fallback,
            // as it may lag the compiler version.
            for start in &search_roots {
                for ancestor in start.ancestors() {
                    let candidate = ancestor.join("node_modules").join("typescript").join("lib");
                    if candidate.is_dir() {
                        return Some(candidate);
                    }
                }
            }
            for start in &search_roots {
                for ancestor in start.ancestors() {
                    let candidate = ancestor
                        .join("editors")
                        .join("vscode")
                        .join("node_modules")
                        .join("typescript")
                        .join("lib");
                    if candidate.is_dir() {
                        return Some(candidate);
                    }
                }
            }

            None
        })
        .as_ref()
}

pub(crate) fn is_typescript_standard_library_file(file_name: &str) -> bool {
    let Some(name) = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    if !name.starts_with("lib.") || !name.ends_with(".d.ts") {
        return false;
    }
    let Some(lib_dir) = find_typescript_lib_dir() else {
        return false;
    };
    let (Ok(file), Ok(lib_dir)) = (fs::canonicalize(file_name), fs::canonicalize(lib_dir)) else {
        return false;
    };
    file.parent() == Some(lib_dir.as_path())
}

fn root_lib_entries(options: &CompilerOptions, lib_dir: &Path) -> Vec<String> {
    if !options.lib.is_empty() {
        return options
            .lib
            .iter()
            .filter_map(|name| normalize_lib_name(name))
            .collect();
    }

    let preferred = match options.target.unwrap_or(ScriptTarget::ES5) {
        ScriptTarget::ES3 | ScriptTarget::ES5 => "lib.d.ts",
        // TypeScript names the full ES2015 default bundle `lib.es6.d.ts`;
        // `lib.es2015.d.ts` is only the language-core dependency bundle.
        ScriptTarget::ES2015 => "lib.es6.d.ts",
        ScriptTarget::ES2016 => "lib.es2016.full.d.ts",
        ScriptTarget::ES2017 => "lib.es2017.full.d.ts",
        ScriptTarget::ES2018 => "lib.es2018.full.d.ts",
        ScriptTarget::ES2019 => "lib.es2019.full.d.ts",
        ScriptTarget::ES2020 => "lib.es2020.full.d.ts",
        ScriptTarget::ES2021 => "lib.es2021.full.d.ts",
        ScriptTarget::ES2022 => "lib.es2022.full.d.ts",
        ScriptTarget::ES2023 => "lib.es2023.full.d.ts",
        ScriptTarget::ES2024 => "lib.es2024.full.d.ts",
        ScriptTarget::ES2025 => "lib.es2025.full.d.ts",
        ScriptTarget::ESNext => "lib.esnext.full.d.ts",
    };

    let selected = if lib_dir.join(preferred).is_file() {
        preferred
    } else {
        "lib.d.ts"
    };

    vec![selected.to_string()]
}

fn load_lib_recursive(
    lib_dir: &Path,
    file_name: &str,
    visited: &mut HashSet<String>,
    out: &mut Vec<StdLibSource>,
) {
    let Some(normalized) = normalize_lib_name(file_name) else {
        return;
    };
    if !visited.insert(normalized.clone()) {
        return;
    }

    let path = lib_dir.join(&normalized);
    let Ok(source) = fs::read_to_string(&path) else {
        return;
    };

    for dep in extract_reference_deps(&source) {
        load_lib_recursive(lib_dir, &dep, visited, out);
    }

    out.push(StdLibSource {
        file_name: path.to_string_lossy().into_owned(),
        source,
    });
}

fn extract_reference_deps(source: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("///") || !trimmed.contains("<reference") {
            continue;
        }

        if let Some(lib_name) = extract_reference_attr(trimmed, "lib") {
            if let Some(normalized) = normalize_lib_name(lib_name) {
                deps.push(normalized);
            }
        }

        if let Some(path_name) = extract_reference_attr(trimmed, "path") {
            if let Some(normalized) = normalize_lib_name(path_name) {
                deps.push(normalized);
            }
        }
    }
    deps
}

fn extract_reference_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let needle = format!("{attr}={quote}");
        if let Some(start) = line.find(&needle) {
            let value = &line[start + needle.len()..];
            if let Some(end) = value.find(quote) {
                return Some(&value[..end]);
            }
        }
    }
    None
}

fn normalize_lib_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let normalized = trimmed.replace('\\', "/").to_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(normalized.as_str());

    if file_name.ends_with(".d.ts") {
        if file_name.starts_with("lib.") {
            return Some(file_name.to_string());
        }
        return Some(format!("lib.{file_name}"));
    }

    if file_name.starts_with("lib.") {
        return Some(format!("{file_name}.d.ts"));
    }

    Some(format!("lib.{file_name}.d.ts"))
}
