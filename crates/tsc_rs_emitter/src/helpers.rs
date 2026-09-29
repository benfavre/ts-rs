use super::*;

impl<'a> Emitter<'a> {
    pub(super) fn emit_extends_helper(&mut self) {
        self.writeln("var __extends = (this && this.__extends) || (function () {");
        self.writeln("    var extendStatics = function (d, b) {");
        self.writeln("        extendStatics = Object.setPrototypeOf ||");
        self.writeln("            ({ __proto__: [] } instanceof Array && function (d, b) { d.__proto__ = b; }) ||");
        self.writeln("            function (d, b) { for (var p in b) if (Object.prototype.hasOwnProperty.call(b, p)) d[p] = b[p]; };");
        self.writeln("        return extendStatics(d, b);");
        self.writeln("    };");
        self.writeln("    return function (d, b) {");
        self.writeln("        if (typeof b !== \"function\" && b !== null)");
        self.writeln("            throw new TypeError(\"Class extends value \" + String(b) + \" is not a constructor or null\");");
        self.writeln("        extendStatics(d, b);");
        self.writeln("        function __() { this.constructor = d; }");
        self.writeln("        d.prototype = b === null ? Object.create(b) : (__.prototype = b.prototype, new __());");
        self.writeln("    };");
        self.writeln("})();");
    }

    pub(super) fn rewrite_relative_import_extensions(&self) -> bool {
        self.options.other.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("rewriterelativeimportextensions")
                && value.eq_ignore_ascii_case("true")
        })
    }

    pub(super) fn rewrite_relative_import_specifier(
        &self,
        source: &str,
        preserve_jsx: bool,
    ) -> String {
        if !self.rewrite_relative_import_extensions()
            || !(source.starts_with("./") || source.starts_with("../"))
        {
            return source.to_string();
        }

        let lower = source.to_ascii_lowercase();
        if lower.ends_with(".tsx") {
            let base = &source[..source.len() - 4];
            return format!("{base}{}", if preserve_jsx { ".jsx" } else { ".js" });
        }

        for (suffix, replacement) in [(".mts", ".mjs"), (".cts", ".cjs"), (".ts", ".js")] {
            if !lower.ends_with(suffix) {
                continue;
            }
            let base = &source[..source.len() - suffix.len()];
            let lower_base = &lower[..lower.len() - suffix.len()];
            let is_declaration = if suffix == ".ts" {
                lower_base.ends_with(".d") || lower_base.contains(".d.")
            } else {
                lower_base.ends_with(".d")
            };
            return if is_declaration {
                source.to_string()
            } else {
                format!("{base}{replacement}")
            };
        }

        source.to_string()
    }

    pub(super) fn source_needs_rewrite_relative_import_helper(&self) -> bool {
        if !self.rewrite_relative_import_extensions() {
            return false;
        }

        ["import(", "require("].iter().any(|needle| {
            let mut offset = 0;
            while let Some(found) = self.source[offset..].find(needle) {
                let args_start = offset + found + needle.len();
                let bytes = self.source.as_bytes();
                let mut i = args_start;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] != b')' {
                    if bytes[i] != b'\'' && bytes[i] != b'"' {
                        return true;
                    }
                    let quote = bytes[i];
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == b'\\' {
                            i += 2;
                            continue;
                        }
                        if bytes[i] == quote {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] != b')' && bytes[i] != b',' {
                        return true;
                    }
                }
                offset = args_start;
            }
            false
        })
    }

    pub(super) fn emit_rewrite_relative_import_extension_helper(&mut self) {
        self.writeln("var __rewriteRelativeImportExtension = (this && this.__rewriteRelativeImportExtension) || function (path, preserveJsx) {");
        self.writeln("    if (typeof path === \"string\" && /^\\.\\.?\\//.test(path)) {");
        self.writeln("        return path.replace(/\\.(tsx)$|((?:\\.d)?)((?:\\.[^./]+?)?)\\.([cm]?)ts$/i, function (m, tsx, d, ext, cm) {");
        self.writeln("            return tsx ? preserveJsx ? \".jsx\" : \".js\" : d && (!ext || !cm) ? m : (d + ext + \".\" + cm.toLowerCase() + \"js\");");
        self.writeln("        });");
        self.writeln("    }");
        self.writeln("    return path;");
        self.writeln("};");
    }

    // ------------------------------------------------------------------
    // CJS interop helpers
    // ------------------------------------------------------------------

    /// Emit the `__createBinding` helper function.
    /// Uses raw strings to match TypeScript's exact formatting (2-space inner indent).
    pub(super) fn emit_create_binding_helper(&mut self) {
        self.writeln("var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {");
        self.writeln("    if (k2 === undefined) k2 = k;");
        self.writeln("    var desc = Object.getOwnPropertyDescriptor(m, k);");
        self.writeln("    if (!desc || (\"get\" in desc ? !m.__esModule : desc.writable || desc.configurable)) {");
        self.writeln("      desc = { enumerable: true, get: function() { return m[k]; } };");
        self.writeln("    }");
        self.writeln("    Object.defineProperty(o, k2, desc);");
        self.writeln("}) : (function(o, m, k, k2) {");
        self.writeln("    if (k2 === undefined) k2 = k;");
        self.writeln("    o[k2] = m[k];");
        self.writeln("}));");
    }

    /// Emit the `__setModuleDefault` helper function.
    /// Uses raw strings to match TypeScript's exact formatting.
    pub(super) fn emit_set_module_default_helper(&mut self) {
        self.writeln("var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {");
        self.writeln("    Object.defineProperty(o, \"default\", { enumerable: true, value: v });");
        self.writeln("}) : function(o, v) {");
        self.writeln("    o[\"default\"] = v;");
        self.writeln("});");
    }

    /// Emit the `__importStar` helper function.
    /// Uses raw strings to match TypeScript's exact formatting.
    pub(super) fn emit_import_star_helper(&mut self) {
        self.writeln("var __importStar = (this && this.__importStar) || (function () {");
        self.writeln("    var ownKeys = function(o) {");
        self.writeln("        ownKeys = Object.getOwnPropertyNames || function (o) {");
        self.writeln("            var ar = [];");
        self.writeln("            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;");
        self.writeln("            return ar;");
        self.writeln("        };");
        self.writeln("        return ownKeys(o);");
        self.writeln("    };");
        self.writeln("    return function (mod) {");
        self.writeln("        if (mod && mod.__esModule) return mod;");
        self.writeln("        var result = {};");
        self.writeln("        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== \"default\") __createBinding(result, mod, k[i]);");
        self.writeln("        __setModuleDefault(result, mod);");
        self.writeln("        return result;");
        self.writeln("    };");
        self.writeln("})();");
    }

    /// Emit the `__importDefault` helper function.
    /// Uses raw strings to match TypeScript's exact formatting.
    pub(super) fn emit_import_default_helper(&mut self) {
        self.writeln("var __importDefault = (this && this.__importDefault) || function (mod) {");
        self.writeln("    return (mod && mod.__esModule) ? mod : { \"default\": mod };");
        self.writeln("};");
    }

    /// Emit the `__exportStar` helper function.
    /// Uses raw strings to match TypeScript's exact formatting.
    pub(super) fn emit_export_star_helper(&mut self) {
        self.writeln("var __exportStar = (this && this.__exportStar) || function(m, exports) {");
        self.writeln("    for (var p in m) if (p !== \"default\" && !Object.prototype.hasOwnProperty.call(exports, p)) __createBinding(exports, m, p);");
        self.writeln("};");
    }

    /// Emit CJS "base" helpers: `__createBinding` and `__setModuleDefault`.
    /// These come before decorator helpers in TypeScript's emit order.
    pub(super) fn emit_cjs_base_helpers(&mut self) {
        // __createBinding is needed by both __importStar and __exportStar
        if self.needs_import_star || self.needs_export_star {
            self.emit_create_binding_helper();
        }
        // __setModuleDefault is needed by __importStar
        if self.needs_import_star {
            self.emit_set_module_default_helper();
        }
    }

    /// Emit CJS "star" helpers: `__importStar` and `__exportStar`.
    /// In TypeScript's emit order, these come BEFORE transform helpers
    /// (`__decorate`, `__metadata`, `__awaiter`, etc.).
    pub(super) fn emit_cjs_star_helpers(&mut self) {
        let export_before_import = self.needs_import_star
            && self.needs_export_star
            && !self.needs_import_default
            && self.first_export_star_helper_use.unwrap_or(usize::MAX)
                < self.first_import_star_helper_use.unwrap_or(usize::MAX);
        if export_before_import {
            self.emit_export_star_helper();
        }
        if self.needs_import_star {
            self.emit_import_star_helper();
        }
        if self.needs_export_star && !export_before_import {
            self.emit_export_star_helper();
        }
    }

    /// Emit CJS `__importDefault` helper.
    /// In TypeScript's emit order, this comes AFTER transform helpers.
    pub(super) fn emit_cjs_import_default_helper_if_needed(&mut self) {
        if self.needs_import_default {
            self.emit_import_default_helper();
        }
    }

    /// Emit CJS "import/export" helpers: `__importStar`, `__importDefault`, `__exportStar`.
    /// Combined version for AMD/UMD where all helpers are emitted together.
    pub(super) fn emit_cjs_import_helpers(&mut self) {
        self.emit_cjs_star_helpers();
        self.emit_cjs_import_default_helper_if_needed();
    }

    /// Emit all needed CJS interop helpers at the current position.
    /// Default order matches TypeScript for most files:
    /// `__createBinding` → `__setModuleDefault` → `__importStar` → `__importDefault` → `__exportStar`.
    /// For star re-export files where `__exportStar` appears before any `__importStar` use,
    /// TypeScript emits `__exportStar` before `__importStar`.
    pub(super) fn emit_cjs_helpers(&mut self) {
        self.emit_cjs_base_helpers();
        self.emit_cjs_import_helpers();
    }

    /// Returns true if any transform helper function is needed that requires
    /// a tslib import. When importHelpers=true, CJS interop helpers
    /// (__importStar, __importDefault) are emitted as tslib_1.__importStar(...)
    /// etc., so they also require a tslib import.
    pub(super) fn any_tslib_helper_needed(&self) -> bool {
        self.needs_extends_helper
            || self.needs_decorate_helper
            || self.needs_es_decorate_helper
            || self.needs_metadata_helper
            || self.needs_param_helper
            || self.needs_prop_key_helper
            || self.needs_run_initializers_helper
            || self.needs_awaiter_helper
            || self.needs_await_helper
            || self.needs_async_generator_helper
            || self.needs_private_field_get
            || self.needs_private_field_set
            || self.needs_set_function_name_helper
            || self.needs_add_disposable_resource_helper
            || self.needs_dispose_resources_helper
            || self.needs_export_star
            || self.needs_rest_helper
            || self.needs_read_helper
            || self.needs_spread_array_helper
            || self.needs_values_helper
            || (self.es_module_interop() && (self.needs_import_star || self.needs_import_default))
    }

    /// Returns the prefix for helper function calls.
    /// When `importHelpers` is enabled AND the file is a CJS-like module,
    /// returns `"tslib_1."` so that `__decorate(...)` becomes `tslib_1.__decorate(...)`.
    /// For ESM, helpers are imported as named imports (`import { __rest } from "tslib"`)
    /// so no prefix is needed.
    /// Script files always inline helpers even with importHelpers=true.
    pub(super) fn helper_prefix(&self) -> &'static str {
        if self.uses_cjs_tslib_namespace() {
            "tslib_1."
        } else {
            ""
        }
    }

    pub(super) fn uses_cjs_tslib_namespace(&self) -> bool {
        self.options.import_helpers == Some(true)
            && self.is_module_file
            && (self.is_cjs_like()
                || (self.effective_module_kind() == ModuleKind::Preserve
                    && self.preserve_module_uses_require))
    }

    pub(super) fn named_tslib_helpers(&self) -> Vec<&'static str> {
        let mut helpers: Vec<&'static str> = Vec::new();
        if self.needs_extends_helper {
            helpers.push("__extends");
        }
        if self.needs_awaiter_helper {
            helpers.push("__awaiter");
        }
        if self.needs_decorate_helper {
            helpers.push("__decorate");
        }
        if self.needs_es_decorate_helper {
            helpers.push("__esDecorate");
        }
        if self.needs_metadata_helper {
            helpers.push("__metadata");
        }
        if self.needs_param_helper {
            helpers.push("__param");
        }
        if self.needs_prop_key_helper {
            helpers.push("__propKey");
        }
        if self.needs_rest_helper {
            helpers.push("__rest");
        }
        if self.needs_read_helper {
            helpers.push("__read");
        }
        if self.needs_spread_array_helper {
            helpers.push("__spreadArray");
        }
        if self.needs_run_initializers_helper {
            helpers.push("__runInitializers");
        }
        if self.needs_values_helper {
            helpers.push("__values");
        }
        if self.needs_async_values_helper {
            helpers.push("__asyncValues");
        }
        if self.needs_await_helper {
            helpers.push("__await");
        }
        if self.needs_async_delegator_helper {
            helpers.push("__asyncDelegator");
        }
        if self.needs_async_generator_helper {
            helpers.push("__asyncGenerator");
        }
        if self.private_field_get_first {
            if self.needs_private_field_get {
                helpers.push("__classPrivateFieldGet");
            }
            if self.needs_private_field_set {
                helpers.push("__classPrivateFieldSet");
            }
        } else {
            if self.needs_private_field_set {
                helpers.push("__classPrivateFieldSet");
            }
            if self.needs_private_field_get {
                helpers.push("__classPrivateFieldGet");
            }
        }
        if self.needs_private_field_in {
            helpers.push("__classPrivateFieldIn");
        }
        if self.needs_set_function_name_helper {
            helpers.push("__setFunctionName");
        }
        if self.needs_add_disposable_resource_helper {
            helpers.push("__addDisposableResource");
        }
        if self.needs_dispose_resources_helper {
            helpers.push("__disposeResources");
        }
        helpers.sort();
        helpers
    }

    pub(super) fn mark_import_star_helper_use(&mut self, stmt_idx: usize) {
        self.needs_import_star = true;
        if self.first_import_star_helper_use.is_none() {
            self.first_import_star_helper_use = Some(stmt_idx);
        }
    }

    pub(super) fn mark_import_default_helper_use(&mut self, stmt_idx: usize) {
        self.needs_import_default = true;
        if self.first_import_default_helper_use.is_none() {
            self.first_import_default_helper_use = Some(stmt_idx);
        }
    }

    pub(super) fn mark_export_star_helper_use(&mut self, stmt_idx: usize) {
        self.needs_export_star = true;
        if self.first_export_star_helper_use.is_none() {
            self.first_export_star_helper_use = Some(stmt_idx);
        }
    }

    /// Scan all statements for const enum declarations and compute member values.
    /// Stores results in `self.const_enum_values` for inlining at usage sites.
    pub(super) fn scan_const_enums(&mut self, stmts: &[Stmt]) {
        self.scan_const_enums_with_prefix(stmts, None);
    }

    /// Pre-scan top-level `const` variable declarations with simple literal
    /// initializers (`const EV = 1`, `const d = 'd'`) so that enum initializer
    /// constant-folding can resolve bare identifier references.
    pub(super) fn scan_file_consts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            let var = match &stmt.kind {
                StmtKind::Var(v) if v.kind == VarKind::Const => v,
                StmtKind::Export(export_decl) => {
                    if let ExportDeclKind::Decl(ref inner) = export_decl.kind {
                        if let StmtKind::Var(v) = &inner.kind {
                            if v.kind == VarKind::Const {
                                v
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            for decl in &var.declarations {
                let name = match &decl.name.kind {
                    PatKind::Ident(n) => n.clone(),
                    _ => continue,
                };
                if let Some(ref init) = decl.init {
                    let mut expr = init.as_ref();
                    while let ExprKind::Paren(inner) = &expr.kind {
                        expr = inner;
                    }
                    match &expr.kind {
                        ExprKind::NumLit(s) => {
                            if let Some(v) = parse_js_number(s) {
                                self.file_consts.insert(name.to_string(), v);
                            }
                        }
                        ExprKind::Unary(unary) if unary.op == UnaryOp::Neg => {
                            if let ExprKind::NumLit(s) = &unary.argument.kind {
                                if let Some(v) = parse_js_number(s) {
                                    self.file_consts.insert(name.to_string(), -v);
                                }
                            }
                        }
                        ExprKind::StrLit(s) => {
                            self.file_string_consts
                                .insert(name.to_string(), s.to_string());
                        }
                        ExprKind::NoSubstTemplate(s) => {
                            self.file_string_consts
                                .insert(name.to_string(), s.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Scan a function body for const enum declarations and temporarily add them
    /// to `const_enum_values`. Returns the keys that were added (and their
    /// previous values, if any) so they can be removed/restored after emission.
    pub(super) fn push_local_const_enums(
        &mut self,
        stmts: &[Stmt],
    ) -> Vec<((String, String), Option<ConstEnumValue>)> {
        if !self.should_inline_const_enums() {
            return Vec::new();
        }
        // The const enum a statement declares, directly or export-wrapped.
        // Single source of truth for both the pre-scan and the collection loop
        // below, so the two can't drift.
        fn stmt_const_enum_decl(stmt: &Stmt) -> Option<&EnumDecl> {
            match &stmt.kind {
                StmtKind::EnumDecl(e) if e.is_const => Some(e),
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Decl(inner) => match &inner.kind {
                        StmtKind::EnumDecl(e) if e.is_const => Some(e),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            }
        }
        // Cheap pre-scan: if this block declares no const enum, there is nothing
        // to collect or to overwrite, so the result is empty — skip the
        // (potentially large) const_enum_values key snapshot + the scans below.
        // Hot on deeply-nested typed code (many blocks, no const enums): the
        // snapshot was the dominant SipHash cost per emit_block_for_decl_body.
        if !stmts.iter().any(|s| stmt_const_enum_decl(s).is_some()) {
            return Vec::new();
        }
        // Snapshot the keys before scanning
        let keys_before: std::collections::HashSet<(String, String)> =
            self.const_enum_values.keys().cloned().collect();
        // Save current values for any const enum names that might be overwritten
        let mut saved = Vec::new();
        for stmt in stmts {
            let enum_decl = stmt_const_enum_decl(stmt);
            if let Some(e) = enum_decl {
                // Save any existing entries for this enum name before overwriting
                let existing: Vec<_> = self
                    .const_enum_values
                    .keys()
                    .filter(|(obj, _)| obj == &e.name)
                    .cloned()
                    .collect();
                for key in existing {
                    let val = self.const_enum_values.get(&key).cloned();
                    saved.push((key, val));
                }
                self.collect_const_enum_values(e, &e.name);
            }
        }
        // Also record any newly-added keys that weren't in saved yet
        for key in self.const_enum_values.keys() {
            if !keys_before.contains(key) && !saved.iter().any(|(k, _)| k == key) {
                saved.push((key.clone(), None));
            }
        }
        saved
    }

    /// Remove locally-added const enum entries and restore any previous values.
    pub(super) fn pop_local_const_enums(
        &mut self,
        saved: Vec<((String, String), Option<ConstEnumValue>)>,
    ) {
        for (key, prev) in saved.into_iter().rev() {
            if let Some(prev_val) = prev {
                self.const_enum_values.insert(key, prev_val);
            } else {
                self.const_enum_values.remove(&key);
            }
        }
    }

    /// Propagate const-enum member maps through local `import x = Y.Z` aliases.
    /// This enables inlining for paths like `x.Member` and `x.Nested.Member`.
    pub(super) fn scan_const_enum_aliases(&mut self, stmts: &[Stmt]) {
        let mut changed = true;
        while changed {
            changed = false;
            for stmt in stmts {
                match &stmt.kind {
                    StmtKind::ImportEquals(ie) => {
                        if let Some(rhs_path) = self.expr_member_path(&ie.module_ref) {
                            if self.merge_const_enum_alias_values(&ie.name, &rhs_path) {
                                changed = true;
                            }
                        }
                    }
                    StmtKind::Export(export_decl) => {
                        if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                            match &inner.kind {
                                StmtKind::ImportEquals(ie) => {
                                    if let Some(rhs_path) = self.expr_member_path(&ie.module_ref) {
                                        if self.merge_const_enum_alias_values(&ie.name, &rhs_path) {
                                            changed = true;
                                        }
                                    }
                                }
                                StmtKind::ModuleDecl(m) => match &m.body {
                                    Some(ModuleBody::Block(body)) => {
                                        if self.scan_const_enum_aliases_block(body) {
                                            changed = true;
                                        }
                                    }
                                    Some(ModuleBody::Module(inner_mod)) => {
                                        let inner_stmt = Stmt {
                                            kind: StmtKind::ModuleDecl(Box::new(
                                                inner_mod.as_ref().clone(),
                                            )),
                                            span: Default::default(),
                                        };
                                        if self.scan_const_enum_aliases_block(&[inner_stmt]) {
                                            changed = true;
                                        }
                                    }
                                    None => {}
                                },
                                _ => {}
                            }
                        }
                    }
                    StmtKind::ModuleDecl(m) => match &m.body {
                        Some(ModuleBody::Block(body)) => {
                            if self.scan_const_enum_aliases_block(body) {
                                changed = true;
                            }
                        }
                        Some(ModuleBody::Module(inner)) => {
                            let inner_stmt = Stmt {
                                kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                                span: Default::default(),
                            };
                            if self.scan_const_enum_aliases_block(&[inner_stmt]) {
                                changed = true;
                            }
                        }
                        None => {}
                    },
                    _ => {}
                }
            }
        }
    }

    pub(super) fn scan_const_enum_aliases_block(&mut self, stmts: &[Stmt]) -> bool {
        let mut changed = false;
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::ImportEquals(ie) => {
                    if let Some(rhs_path) = self.expr_member_path(&ie.module_ref) {
                        if self.merge_const_enum_alias_values(&ie.name, &rhs_path) {
                            changed = true;
                        }
                    }
                }
                StmtKind::Export(export_decl) => {
                    if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                        match &inner.kind {
                            StmtKind::ImportEquals(ie) => {
                                if let Some(rhs_path) = self.expr_member_path(&ie.module_ref) {
                                    if self.merge_const_enum_alias_values(&ie.name, &rhs_path) {
                                        changed = true;
                                    }
                                }
                            }
                            StmtKind::ModuleDecl(m) => match &m.body {
                                Some(ModuleBody::Block(body)) => {
                                    if self.scan_const_enum_aliases_block(body) {
                                        changed = true;
                                    }
                                }
                                Some(ModuleBody::Module(inner_mod)) => {
                                    // Wrap inner module in a single-element slice for recursive scan
                                    let inner_stmt = Stmt {
                                        kind: StmtKind::ModuleDecl(Box::new(
                                            inner_mod.as_ref().clone(),
                                        )),
                                        span: Default::default(),
                                    };
                                    if self.scan_const_enum_aliases_block(&[inner_stmt]) {
                                        changed = true;
                                    }
                                }
                                None => {}
                            },
                            _ => {}
                        }
                    }
                }
                StmtKind::ModuleDecl(m) => {
                    match &m.body {
                        Some(ModuleBody::Block(body)) => {
                            if self.scan_const_enum_aliases_block(body) {
                                changed = true;
                            }
                        }
                        Some(ModuleBody::Module(inner)) => {
                            // Wrap inner module in a single-element slice for recursive scan
                            let inner_stmt = Stmt {
                                kind: StmtKind::ModuleDecl(Box::new(inner.as_ref().clone())),
                                span: Default::default(),
                            };
                            if self.scan_const_enum_aliases_block(&[inner_stmt]) {
                                changed = true;
                            }
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
        changed
    }

    pub(super) fn merge_const_enum_alias_values(&mut self, alias: &str, target_path: &str) -> bool {
        let mut to_insert: Vec<((String, String), ConstEnumValue)> = Vec::new();
        let target_suffix = format!(".{target_path}");
        for ((obj_name, member_name), value) in &self.const_enum_values {
            if obj_name == target_path {
                to_insert.push(((alias.to_string(), member_name.clone()), value.clone()));
                continue;
            }
            if let Some(rest) = obj_name.strip_prefix(target_path) {
                if rest.starts_with('.') {
                    let mut aliased = String::with_capacity(alias.len() + rest.len());
                    aliased.push_str(alias);
                    aliased.push_str(rest);
                    to_insert.push(((aliased, member_name.clone()), value.clone()));
                }
                continue;
            }
            if let Some(prefix) = obj_name.strip_suffix(&target_suffix) {
                let mut aliased = String::new();
                if !prefix.is_empty() {
                    aliased.push_str(prefix);
                    aliased.push('.');
                }
                aliased.push_str(alias);
                to_insert.push(((aliased, member_name.clone()), value.clone()));
                to_insert.push(((alias.to_string(), member_name.clone()), value.clone()));
            }
        }
        let mut changed = false;
        for (k, v) in to_insert {
            if self.const_enum_values.get(&k) != Some(&v) {
                self.const_enum_values.insert(k, v);
                changed = true;
            }
        }
        changed
    }

    pub(super) fn scan_const_enums_with_prefix(&mut self, stmts: &[Stmt], ns_prefix: Option<&str>) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::EnumDecl(e) if e.is_const => {
                    let enum_name = if let Some(prefix) = ns_prefix {
                        format!("{prefix}.{}", e.name)
                    } else {
                        e.name.clone()
                    };
                    self.collect_const_enum_values(e, &enum_name);
                }
                StmtKind::Export(export_decl) => {
                    if let ExportDeclKind::Decl(ref inner) = export_decl.kind {
                        match &inner.kind {
                            StmtKind::EnumDecl(e) => {
                                if e.is_const {
                                    let enum_name = if let Some(prefix) = ns_prefix {
                                        format!("{prefix}.{}", e.name)
                                    } else {
                                        e.name.clone()
                                    };
                                    self.collect_const_enum_values(e, &enum_name);
                                }
                            }
                            StmtKind::ModuleDecl(m) => {
                                self.scan_module_body_const_enums(m, ns_prefix);
                            }
                            _ => {}
                        }
                    }
                }
                StmtKind::ModuleDecl(m) => {
                    self.scan_module_body_const_enums(m, ns_prefix);
                }
                _ => {}
            }
        }
    }

    fn scan_module_body_const_enums(&mut self, m: &ModuleDecl, ns_prefix: Option<&str>) {
        let next_prefix = match &m.name {
            ModuleName::Ident(name) => Some(if let Some(prefix) = ns_prefix {
                format!("{prefix}.{name}")
            } else {
                name.clone()
            }),
            ModuleName::String(_) => ns_prefix.map(|s| s.to_string()),
        };
        match &m.body {
            Some(ModuleBody::Block(ref body)) => {
                self.scan_const_enums_with_prefix(body, next_prefix.as_deref());
            }
            Some(ModuleBody::Module(ref inner)) => {
                self.scan_module_body_const_enums(inner, next_prefix.as_deref());
            }
            None => {}
        }
    }

    /// Compute and store const enum member values for a single enum declaration.
    pub(super) fn collect_const_enum_values(&mut self, e: &EnumDecl, enum_name: &str) {
        let mut auto_value: f64 = 0.0;
        // Build a local map for cross-member references within this enum.
        let mut local_values: Vec<(String, ConstEnumValue)> = Vec::new();

        for member in &e.members {
            let mem_name = match &member.name {
                PropName::Ident(n, _) | PropName::String(n, _) => n.to_string(),
                _ => continue,
            };

            if let Some(ref init) = member.initializer {
                if is_string_literal(init) {
                    let s = match &init.kind {
                        ExprKind::StrLit(s) => s.to_string(),
                        ExprKind::NoSubstTemplate(s) => s.to_string(),
                        _ => continue,
                    };
                    let val = ConstEnumValue::String(s);
                    local_values.push((mem_name.clone(), val.clone()));
                    self.const_enum_values
                        .insert((enum_name.to_string(), mem_name), val);
                    // String members don't affect auto_value
                } else {
                    // Try to evaluate numeric value, including cross-member references
                    if let Some(v) = self.eval_const_enum_expr(init, enum_name, &local_values) {
                        let val = ConstEnumValue::Number(v);
                        local_values.push((mem_name.clone(), val.clone()));
                        self.const_enum_values
                            .insert((enum_name.to_string(), mem_name), val);
                        auto_value = v + 1.0;
                    } else if let Some(v) = try_extract_numeric_value_const_enum(init) {
                        let val = ConstEnumValue::Number(v);
                        local_values.push((mem_name.clone(), val.clone()));
                        self.const_enum_values
                            .insert((enum_name.to_string(), mem_name), val);
                        auto_value = v + 1.0;
                    }
                }
            } else {
                let val = ConstEnumValue::Number(auto_value);
                local_values.push((mem_name.clone(), val.clone()));
                self.const_enum_values
                    .insert((enum_name.to_string(), mem_name), val);
                auto_value += 1.0;
            }
        }
    }

    /// Evaluate a const enum initializer expression, resolving cross-member references.
    pub(super) fn eval_const_enum_expr(
        &self,
        expr: &Expr,
        enum_name: &str,
        local_values: &[(String, ConstEnumValue)],
    ) -> Option<f64> {
        match &expr.kind {
            ExprKind::NumLit(s) => s.parse::<f64>().ok(),
            ExprKind::Unary(un) if un.op == UnaryOp::Neg => self
                .eval_const_enum_expr(&un.argument, enum_name, local_values)
                .map(|v| -v),
            ExprKind::Unary(un) if un.op == UnaryOp::Pos => {
                self.eval_const_enum_expr(&un.argument, enum_name, local_values)
            }
            ExprKind::Unary(un) if un.op == UnaryOp::BitNot => self
                .eval_const_enum_expr(&un.argument, enum_name, local_values)
                .map(|v| (!(v as i64)) as f64),
            ExprKind::Paren(inner) => self.eval_const_enum_expr(inner, enum_name, local_values),
            ExprKind::Binary(bin) => {
                let l = self.eval_const_enum_expr(&bin.left, enum_name, local_values)?;
                let r = self.eval_const_enum_expr(&bin.right, enum_name, local_values)?;
                Some(match bin.op {
                    BinaryOp::Add => l + r,
                    BinaryOp::Sub => l - r,
                    BinaryOp::Mul => l * r,
                    BinaryOp::Div => {
                        if r == 0.0 {
                            return None;
                        }
                        l / r
                    }
                    BinaryOp::Mod => {
                        if r == 0.0 {
                            return None;
                        }
                        l % r
                    }
                    BinaryOp::BitAnd => ((l as i64) & (r as i64)) as f64,
                    BinaryOp::BitOr => ((l as i64) | (r as i64)) as f64,
                    BinaryOp::BitXor => ((l as i64) ^ (r as i64)) as f64,
                    BinaryOp::Shl => (l as i64).wrapping_shl(r as u32) as f64,
                    BinaryOp::Shr => (l as i64).wrapping_shr(r as u32) as f64,
                    BinaryOp::UShr => ((l as u64).wrapping_shr(r as u32)) as f64,
                    BinaryOp::Exp => l.powf(r),
                    _ => return None,
                })
            }
            // Reference to another member of the same enum: just `MemberName`
            ExprKind::Ident(name) => {
                for (n, v) in local_values {
                    if n == name.as_str() {
                        if let ConstEnumValue::Number(num) = v {
                            return Some(*num);
                        }
                    }
                }
                if let Some(ConstEnumValue::Number(num)) = self
                    .const_enum_values
                    .get(&(enum_name.to_string(), name.to_string()))
                {
                    return Some(*num);
                }
                None
            }
            // Reference via `EnumName.MemberName`
            ExprKind::Member(mem) => {
                let local_enum_name = enum_name.rsplit('.').next().unwrap_or(enum_name);
                if let Some(obj_path) = self.expr_member_path(&mem.object) {
                    if obj_path == enum_name || obj_path == local_enum_name {
                        for (n, v) in local_values {
                            if n == mem.property.as_str() {
                                if let ConstEnumValue::Number(num) = v {
                                    return Some(*num);
                                }
                            }
                        }
                        if let Some(ConstEnumValue::Number(num)) = self
                            .const_enum_values
                            .get(&(enum_name.to_string(), mem.property.to_string()))
                        {
                            return Some(*num);
                        }
                    }
                    if let Some(ConstEnumValue::Number(num)) = self
                        .const_enum_values
                        .get(&(obj_path, mem.property.to_string()))
                    {
                        return Some(*num);
                    }
                }
                None
            }
            ExprKind::ElemAccess(ea) => {
                let local_enum_name = enum_name.rsplit('.').next().unwrap_or(enum_name);
                let Some(obj_path) = self.expr_member_path(&ea.object) else {
                    return None;
                };
                let member_name = match &ea.index.kind {
                    ExprKind::StrLit(s) | ExprKind::NoSubstTemplate(s) => s.to_string(),
                    ExprKind::NumLit(s) => s.to_string(),
                    _ => return None,
                };
                if obj_path == enum_name || obj_path == local_enum_name {
                    for (n, v) in local_values {
                        if n == &member_name {
                            if let ConstEnumValue::Number(num) = v {
                                return Some(*num);
                            }
                        }
                    }
                    if let Some(ConstEnumValue::Number(num)) = self
                        .const_enum_values
                        .get(&(enum_name.to_string(), member_name.clone()))
                    {
                        return Some(*num);
                    }
                }
                if let Some(ConstEnumValue::Number(num)) =
                    self.const_enum_values.get(&(obj_path, member_name))
                {
                    return Some(*num);
                }
                None
            }
            _ => None,
        }
    }

    /// Pre-scan statements to check if any tagged template has invalid escape
    /// sequences (for __makeTemplateObject helper, target < ES2018).
    pub(super) fn scan_needs_make_template_object(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            if self.scan_stmt_for_tagged_template_lowering(stmt) {
                self.needs_make_template_object_helper = true;
                return;
            }
        }
    }

    fn scan_stmt_for_tagged_template_lowering(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Expr(expr) => self.scan_expr_for_tagged_template_lowering(expr),
            StmtKind::Var(var_stmt) => self.scan_var_for_tagged_template_lowering(var_stmt),
            StmtKind::Return(Some(expr)) | StmtKind::Throw(expr) => {
                self.scan_expr_for_tagged_template_lowering(expr)
            }
            StmtKind::If(if_stmt) => {
                self.scan_expr_for_tagged_template_lowering(&if_stmt.test)
                    || self.scan_stmt_for_tagged_template_lowering(&if_stmt.consequent)
                    || if_stmt
                        .alternate
                        .as_ref()
                        .is_some_and(|s| self.scan_stmt_for_tagged_template_lowering(s))
            }
            StmtKind::While(wh) => {
                self.scan_expr_for_tagged_template_lowering(&wh.test)
                    || self.scan_stmt_for_tagged_template_lowering(&wh.body)
            }
            StmtKind::DoWhile(dw) => {
                self.scan_stmt_for_tagged_template_lowering(&dw.body)
                    || self.scan_expr_for_tagged_template_lowering(&dw.test)
            }
            StmtKind::For(for_stmt) => {
                for_stmt.init.as_ref().is_some_and(|init| match init {
                    ForInit::Var(var_stmt) => self.scan_var_for_tagged_template_lowering(var_stmt),
                    ForInit::Expr(expr) => self.scan_expr_for_tagged_template_lowering(expr),
                }) || for_stmt
                    .test
                    .as_ref()
                    .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                    || for_stmt
                        .update
                        .as_ref()
                        .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                    || self.scan_stmt_for_tagged_template_lowering(&for_stmt.body)
            }
            StmtKind::ForIn(for_in) => {
                self.scan_for_in_of_left_for_tagged_template_lowering(&for_in.left)
                    || self.scan_expr_for_tagged_template_lowering(&for_in.right)
                    || self.scan_stmt_for_tagged_template_lowering(&for_in.body)
            }
            StmtKind::ForOf(for_of) => {
                self.scan_for_in_of_left_for_tagged_template_lowering(&for_of.left)
                    || self.scan_expr_for_tagged_template_lowering(&for_of.right)
                    || self.scan_stmt_for_tagged_template_lowering(&for_of.body)
            }
            StmtKind::Switch(switch_stmt) => {
                self.scan_expr_for_tagged_template_lowering(&switch_stmt.discriminant)
                    || switch_stmt.cases.iter().any(|case| {
                        case.test
                            .as_ref()
                            .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                            || case
                                .consequent
                                .iter()
                                .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                    })
            }
            StmtKind::Try(try_stmt) => {
                try_stmt
                    .block
                    .iter()
                    .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                    || try_stmt.handler.as_ref().is_some_and(|handler| {
                        handler
                            .param
                            .as_ref()
                            .is_some_and(|pat| self.scan_pat_for_tagged_template_lowering(pat))
                            || handler
                                .body
                                .iter()
                                .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                    })
                    || try_stmt.finalizer.as_ref().is_some_and(|stmts| {
                        stmts
                            .iter()
                            .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                    })
            }
            StmtKind::Block(stmts) => stmts
                .iter()
                .any(|s| self.scan_stmt_for_tagged_template_lowering(s)),
            StmtKind::FnDecl(fn_decl) => self.scan_fn_for_tagged_template_lowering(fn_decl),
            StmtKind::ClassDecl(class_decl) => {
                self.scan_class_for_tagged_template_lowering(class_decl)
            }
            StmtKind::EnumDecl(enum_decl) => enum_decl.members.iter().any(|member| {
                self.scan_prop_name_for_tagged_template_lowering(&member.name)
                    || member
                        .initializer
                        .as_ref()
                        .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
            }),
            StmtKind::ModuleDecl(module_decl) => module_decl.body.as_ref().is_some_and(|body| {
                let mut current = body;
                loop {
                    match current {
                        ModuleBody::Block(stmts) => {
                            break stmts
                                .iter()
                                .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt));
                        }
                        ModuleBody::Module(module) => {
                            let Some(body) = &module.body else {
                                break false;
                            };
                            current = body;
                        }
                    }
                }
            }),
            StmtKind::ImportEquals(import) => {
                self.scan_expr_for_tagged_template_lowering(&import.module_ref)
            }
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(stmt) | ExportDeclKind::DefaultDecl(stmt) => {
                    self.scan_stmt_for_tagged_template_lowering(stmt)
                }
                ExportDeclKind::Default(expr) => self.scan_expr_for_tagged_template_lowering(expr),
                _ => false,
            },
            StmtKind::ExportAssign(expr) => self.scan_expr_for_tagged_template_lowering(expr),
            StmtKind::Labeled(labeled) => {
                self.scan_stmt_for_tagged_template_lowering(&labeled.body)
            }
            StmtKind::With(with_stmt) => {
                self.scan_expr_for_tagged_template_lowering(&with_stmt.object)
                    || self.scan_stmt_for_tagged_template_lowering(&with_stmt.body)
            }
            _ => false,
        }
    }

    fn scan_var_for_tagged_template_lowering(&self, var_stmt: &VarStmt) -> bool {
        var_stmt.declarations.iter().any(|decl| {
            self.scan_pat_for_tagged_template_lowering(&decl.name)
                || decl
                    .init
                    .as_ref()
                    .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
        })
    }

    fn scan_for_in_of_left_for_tagged_template_lowering(&self, left: &ForInOfLeft) -> bool {
        match left {
            ForInOfLeft::Var(var_stmt) => self.scan_var_for_tagged_template_lowering(var_stmt),
            ForInOfLeft::Pat(pat) => self.scan_pat_for_tagged_template_lowering(pat),
            ForInOfLeft::Expr(expr) => self.scan_expr_for_tagged_template_lowering(expr),
        }
    }

    fn scan_pat_for_tagged_template_lowering(&self, pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Array(elements) => elements.iter().flatten().any(|element| match element {
                ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => {
                    self.scan_pat_for_tagged_template_lowering(pat)
                }
            }),
            PatKind::Object(properties) => properties.iter().any(|property| match property {
                ObjPatProp::KeyValue(name, pat) => {
                    self.scan_prop_name_for_tagged_template_lowering(name)
                        || self.scan_pat_for_tagged_template_lowering(pat)
                }
                ObjPatProp::Rest(pat) => self.scan_pat_for_tagged_template_lowering(pat),
                ObjPatProp::ShorthandAssign(_, expr, _) => {
                    self.scan_expr_for_tagged_template_lowering(expr)
                }
                ObjPatProp::Shorthand(..) => false,
            }),
            PatKind::Assign(pat, expr) => {
                self.scan_pat_for_tagged_template_lowering(pat)
                    || self.scan_expr_for_tagged_template_lowering(expr)
            }
            PatKind::Rest(pat) => self.scan_pat_for_tagged_template_lowering(pat),
            PatKind::Ident(_) => false,
        }
    }

    fn scan_params_for_tagged_template_lowering(&self, params: &[Param]) -> bool {
        params.iter().any(|param| {
            self.scan_pat_for_tagged_template_lowering(&param.name)
                || param
                    .initializer
                    .as_ref()
                    .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                || param
                    .decorators
                    .iter()
                    .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
        })
    }

    fn scan_fn_for_tagged_template_lowering(&self, function: &FnDecl) -> bool {
        function
            .decorators
            .iter()
            .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
            || self.scan_params_for_tagged_template_lowering(&function.params)
            || function.body.as_ref().is_some_and(|body| {
                body.iter()
                    .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
            })
    }

    fn scan_prop_name_for_tagged_template_lowering(&self, name: &PropName) -> bool {
        matches!(name, PropName::Computed(expr, _) if self.scan_expr_for_tagged_template_lowering(expr))
    }

    fn scan_class_for_tagged_template_lowering(&self, class: &ClassDecl) -> bool {
        class
            .extends
            .as_ref()
            .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
            || class
                .decorators
                .iter()
                .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
            || class.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Property(property) => {
                    self.scan_prop_name_for_tagged_template_lowering(&property.name)
                        || property
                            .initializer
                            .as_ref()
                            .is_some_and(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                        || property
                            .decorators
                            .iter()
                            .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                }
                ClassMemberKind::Method(method) => {
                    self.scan_prop_name_for_tagged_template_lowering(&method.name)
                        || self.scan_params_for_tagged_template_lowering(&method.params)
                        || method.body.as_ref().is_some_and(|body| {
                            body.iter()
                                .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                        })
                        || method
                            .decorators
                            .iter()
                            .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                }
                ClassMemberKind::Constructor(constructor) => {
                    self.scan_params_for_tagged_template_lowering(&constructor.params)
                        || constructor.body.as_ref().is_some_and(|body| {
                            body.iter()
                                .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                        })
                        || constructor
                            .decorators
                            .iter()
                            .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    self.scan_prop_name_for_tagged_template_lowering(&accessor.name)
                        || self.scan_params_for_tagged_template_lowering(&accessor.params)
                        || accessor.body.as_ref().is_some_and(|body| {
                            body.iter()
                                .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt))
                        })
                        || accessor
                            .decorators
                            .iter()
                            .any(|expr| self.scan_expr_for_tagged_template_lowering(expr))
                }
                ClassMemberKind::StaticBlock(stmts) => stmts
                    .iter()
                    .any(|stmt| self.scan_stmt_for_tagged_template_lowering(stmt)),
                _ => false,
            })
    }

    fn scan_expr_for_tagged_template_lowering(&self, expr: &Expr) -> bool {
        let mut stack = vec![expr];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::TaggedTemplate(tagged) => {
                    if self.is_js_file && tagged.type_args.is_some() {
                        // In a JavaScript file this is a binary comparison
                        // whose right operand is an untagged template. It must
                        // not request the tagged-template helper.
                        for e in tagged.quasi.exprs.iter().rev() {
                            stack.push(e);
                        }
                        stack.push(&tagged.tag);
                        continue;
                    }
                    if self.effective_target() < ScriptTarget::ES2015
                        || crate::emit_stmt_helpers::tagged_template_needs_lowering(&tagged.quasi)
                    {
                        return true;
                    }
                    for e in tagged.quasi.exprs.iter().rev() {
                        stack.push(e);
                    }
                    stack.push(&tagged.tag);
                }
                ExprKind::Call(call) => {
                    for arg in call.args.iter().rev() {
                        stack.push(arg);
                    }
                    stack.push(&call.callee);
                }
                ExprKind::Binary(b) => {
                    stack.push(&b.right);
                    stack.push(&b.left);
                }
                ExprKind::Assign(a) => {
                    stack.push(&a.right);
                    stack.push(&a.left);
                }
                ExprKind::Paren(inner) | ExprKind::Spread(inner) => stack.push(inner),
                ExprKind::TypeAssertion(assertion) => stack.push(&assertion.expr),
                ExprKind::As(as_expr) => stack.push(&as_expr.expr),
                ExprKind::Satisfies(satisfies) => stack.push(&satisfies.expr),
                ExprKind::NonNull(inner) => stack.push(inner),
                ExprKind::Instantiation(instantiation) => stack.push(&instantiation.expr),
                ExprKind::Unary(unary) => stack.push(&unary.argument),
                ExprKind::Update(update) => stack.push(&update.argument),
                ExprKind::Yield(_, Some(inner))
                | ExprKind::Await(inner)
                | ExprKind::Delete(inner)
                | ExprKind::Typeof(inner)
                | ExprKind::Void(inner) => stack.push(inner),
                ExprKind::Cond(c) => {
                    stack.push(&c.alternate);
                    stack.push(&c.consequent);
                    stack.push(&c.test);
                }
                ExprKind::Comma(exprs) => {
                    for e in exprs.iter().rev() {
                        stack.push(e);
                    }
                }
                ExprKind::Member(m) => stack.push(&m.object),
                ExprKind::ElemAccess(ea) => {
                    stack.push(&ea.index);
                    stack.push(&ea.object);
                }
                ExprKind::Arrow(arrow) => {
                    if self.scan_params_for_tagged_template_lowering(&arrow.params) {
                        return true;
                    }
                    match &arrow.body {
                        tsc_rs_ast::ArrowBody::Block(stmts) => {
                            for s in stmts {
                                if self.scan_stmt_for_tagged_template_lowering(s) {
                                    return true;
                                }
                            }
                        }
                        tsc_rs_ast::ArrowBody::Expr(e) => {
                            stack.push(e);
                        }
                    }
                }
                ExprKind::FnExpr(fn_decl) => {
                    if self.scan_fn_for_tagged_template_lowering(fn_decl) {
                        return true;
                    }
                }
                ExprKind::ClassExpr(class) => {
                    if self.scan_class_for_tagged_template_lowering(class) {
                        return true;
                    }
                }
                ExprKind::New(new_expr) => {
                    if let Some(args) = &new_expr.args {
                        for arg in args.iter().rev() {
                            stack.push(arg);
                        }
                    }
                    stack.push(&new_expr.callee);
                }
                ExprKind::ArrayLit(elements) => {
                    for element in elements.iter().rev().flatten() {
                        stack.push(element);
                    }
                }
                ExprKind::ObjectLit(properties) => {
                    for property in properties {
                        let found = match property {
                            ObjLitProp::Property(property) => {
                                self.scan_prop_name_for_tagged_template_lowering(&property.key)
                                    || self.scan_expr_for_tagged_template_lowering(&property.value)
                            }
                            ObjLitProp::ShorthandDefault(_, expr, _)
                            | ObjLitProp::Spread(expr, _) => {
                                self.scan_expr_for_tagged_template_lowering(expr)
                            }
                            ObjLitProp::Method(method) => {
                                self.scan_prop_name_for_tagged_template_lowering(&method.name)
                                    || self.scan_params_for_tagged_template_lowering(&method.params)
                                    || method.body.iter().any(|stmt| {
                                        self.scan_stmt_for_tagged_template_lowering(stmt)
                                    })
                            }
                            ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                                self.scan_prop_name_for_tagged_template_lowering(&accessor.name)
                                    || self
                                        .scan_params_for_tagged_template_lowering(&accessor.params)
                                    || accessor.body.iter().any(|stmt| {
                                        self.scan_stmt_for_tagged_template_lowering(stmt)
                                    })
                            }
                            ObjLitProp::Shorthand(..) => false,
                        };
                        if found {
                            return true;
                        }
                    }
                }
                ExprKind::Template(template) => {
                    for expr in template.exprs.iter().rev() {
                        stack.push(expr);
                    }
                }
                ExprKind::JsxElement(element) => {
                    stack.push(&element.name);
                    for attribute in &element.attributes {
                        match attribute {
                            JsxAttribute::Normal {
                                value: Some(expr), ..
                            }
                            | JsxAttribute::Spread(expr, _) => stack.push(expr),
                            _ => {}
                        }
                    }
                    for child in &element.children {
                        match child {
                            JsxChild::Element(expr) | JsxChild::Expression(Some(expr), _) => {
                                stack.push(expr)
                            }
                            JsxChild::Fragment(fragment) => {
                                if self.scan_jsx_fragment_for_tagged_template_lowering(fragment) {
                                    return true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                ExprKind::JsxSelfClosing(element) => {
                    stack.push(&element.name);
                    for attribute in &element.attributes {
                        match attribute {
                            JsxAttribute::Normal {
                                value: Some(expr), ..
                            }
                            | JsxAttribute::Spread(expr, _) => stack.push(expr),
                            _ => {}
                        }
                    }
                }
                ExprKind::JsxFragment(fragment) => {
                    if self.scan_jsx_fragment_for_tagged_template_lowering(fragment) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn scan_jsx_fragment_for_tagged_template_lowering(&self, fragment: &JsxFragment) -> bool {
        fragment.children.iter().any(|child| match child {
            JsxChild::Element(expr) | JsxChild::Expression(Some(expr), _) => {
                self.scan_expr_for_tagged_template_lowering(expr)
            }
            JsxChild::Fragment(fragment) => {
                self.scan_jsx_fragment_for_tagged_template_lowering(fragment)
            }
            _ => false,
        })
    }

    /// Pre-scan statements to check if any async function exists (for __awaiter helper).
    pub(super) fn scan_needs_awaiter(&mut self, stmts: &[Stmt]) {
        let needs_async = self.needs_downlevel("async");
        let needs_async_generator = self.needs_downlevel("async-generator");
        if !needs_async && !needs_async_generator {
            return;
        }
        // Expression forms can contain async functions even when the legacy
        // source scan does not descend through their enclosing syntax.
        if needs_async {
            self.scan_es5_generator_helpers(stmts);
            self.needs_awaiter_helper |= self.needs_generator_helper;
        }
        // ES2017 preserves native async functions but still lowers for-await,
        // and downleveled async generators use the same iterator helper.
        if needs_async_generator && stmts.iter().any(source_has_for_await) {
            self.needs_async_values_helper = true;
            self.async_values_before_await = false;
        }
        for stmt in stmts {
            if !(needs_async && source_has_async(stmt))
                && !(needs_async_generator && source_has_async_generator(stmt))
            {
                continue;
            }
            if needs_async && source_has_awaiter(stmt) {
                self.needs_awaiter_helper = true;
                if source_has_for_await(stmt) {
                    self.needs_async_values_helper = true;
                    // for-await-of uses __asyncValues after __await/__asyncGenerator
                    self.async_values_before_await = false;
                }
            }
            if needs_async_generator && source_has_async_generator(stmt) {
                self.needs_await_helper = true;
                self.needs_async_generator_helper = true;
                if source_has_async_generator_delegate(stmt) {
                    self.needs_async_values_helper = true;
                    self.needs_async_delegator_helper = true;
                    // Check if yield* operand contains nested async gen —
                    // affects helper emission ordering
                    if source_yield_star_has_nested_async_gen(stmt) {
                        self.async_values_before_await = false;
                    }
                }
            }
            if (!needs_async || self.needs_awaiter_helper)
                && (!needs_async_generator || self.needs_async_generator_helper)
            {
                return;
            }
        }
    }

    /// Emit a for-of loop where the binding has object rest destructuring.
    /// Transforms `for (let { x, ...rest } of arr)` to:
    ///   `for (let _a of arr) { let { x } = _a, rest = __rest(_a, ["x"]); ... }`
    pub(super) fn emit_for_of_object_rest(&mut self, fo: &ForOfStmt) {
        let mut nested_var_left = None;
        let mut nested_pat_left = None;
        match &fo.left {
            ForInOfLeft::Var(vs) => {
                if let Some(decl) = vs.declarations.first() {
                    if !matches!(decl.name.kind, PatKind::Object(_)) {
                        nested_var_left = self
                            .rewrite_nested_obj_rest_pat_with_temps(
                                &decl.name,
                                crate::helpers_rest::RestTempAlloc::Inline,
                            )
                            .map(|(pat, transforms)| (vs.kind, pat, transforms));
                    }
                }
            }
            ForInOfLeft::Pat(pat) => {
                if !matches!(pat.kind, PatKind::Object(_)) {
                    nested_pat_left = self.rewrite_nested_obj_rest_pat_with_temps(
                        pat,
                        crate::helpers_rest::RestTempAlloc::Hoisted,
                    );
                }
            }
            ForInOfLeft::Expr(_) => {}
        }
        let temp = self.next_inline_temp_var();

        // for (let _a of right) {
        self.write("for (let ");
        self.write(&temp);
        self.write(" of ");
        self.emit_expr(&fo.right);
        self.writeln(") {");
        self.indent += 1;

        // Emit destructuring prefix based on left kind
        match &fo.left {
            ForInOfLeft::Var(vs) => {
                if let Some(decl) = vs.declarations.first() {
                    if let PatKind::Object(props) = &decl.name.kind {
                        let kw = self.emitted_var_keyword(vs);
                        self.write(kw);
                        self.write(" ");
                        self.emit_obj_rest_destr_with_temp(props, &temp);
                        self.writeln(";");
                    } else if let Some((kind, rewritten_pat, transforms)) = nested_var_left.take() {
                        let _ = kind;
                        let kw = self.emitted_var_keyword(vs);
                        self.write(kw);
                        self.write(" ");
                        self.emit_binding_name(&rewritten_pat);
                        self.write(" = ");
                        self.write(&temp);
                        for transform in transforms {
                            self.write(", ");
                            self.emit_obj_rest_destr_with_temp(&transform.props, &transform.temp);
                        }
                        self.writeln(";");
                    }
                }
            }
            ForInOfLeft::Pat(pat) => {
                if let PatKind::Object(props) = &pat.kind {
                    self.write("(");
                    self.emit_obj_rest_destr_with_temp(props, &temp);
                    self.writeln(");");
                } else if let Some((rewritten_pat, transforms)) = nested_pat_left.take() {
                    self.emit_binding_name(&rewritten_pat);
                    self.write(" = ");
                    self.write(&temp);
                    for transform in transforms {
                        self.write(", ");
                        self.emit_obj_rest_destr_with_temp(&transform.props, &transform.temp);
                    }
                    self.writeln(";");
                }
            }
            ForInOfLeft::Expr(_) => {}
        }

        // Emit original body statements
        match &fo.body.kind {
            StmtKind::Block(stmts) => {
                for s in stmts {
                    self.emit_leading_comments(s.span.start);
                    self.emit_stmt(s);
                    self.advance_comment_pos(s.span.end);
                }
            }
            _ => {
                self.emit_stmt(&fo.body);
            }
        }

        self.indent -= 1;
        self.writeln("}");
    }

    /// Emit rest param destructuring for the await-to-yield path (inside
    /// async function bodies that are being downleveled).
    pub(super) fn emit_rest_param_destructuring_await_to_yield(
        &mut self,
        temp_name: &str,
        pat: &Pat,
    ) {
        if let Some(props) = crate::helpers_rest::top_level_object_props(pat) {
            let mut non_rest: Vec<&ObjPatProp> = Vec::new();
            let mut rest_binding: Option<&Pat> = None;
            let mut excluded_keys: Vec<String> = Vec::new();

            for prop in props {
                match prop {
                    ObjPatProp::Rest(p) => {
                        rest_binding = Some(p);
                    }
                    ObjPatProp::KeyValue(key, _) => {
                        if let Some(key_str) = prop_name_to_string(key) {
                            excluded_keys.push(key_str);
                        }
                        non_rest.push(prop);
                    }
                    ObjPatProp::Shorthand(name, _) => {
                        excluded_keys.push(name.to_string());
                        non_rest.push(prop);
                    }
                    ObjPatProp::ShorthandAssign(name, _, _) => {
                        excluded_keys.push(name.to_string());
                        non_rest.push(prop);
                    }
                }
            }

            self.write("var ");

            if !non_rest.is_empty() {
                self.write("{ ");
                for (i, prop) in non_rest.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    match prop {
                        ObjPatProp::KeyValue(key, val) => {
                            self.emit_prop_name(key);
                            self.write(": ");
                            self.emit_binding_name(val);
                        }
                        ObjPatProp::Shorthand(name, _) => {
                            self.write(name);
                        }
                        ObjPatProp::ShorthandAssign(name, init, _) => {
                            self.write(name);
                            self.write(" = ");
                            self.emit_expr_await_to_yield(init);
                        }
                        ObjPatProp::Rest(_) => unreachable!(),
                    }
                }
                self.write(" } = ");
                self.write(temp_name);
            }

            if let Some(rest_pat) = rest_binding {
                if !non_rest.is_empty() {
                    self.write(", ");
                }
                self.emit_binding_name(rest_pat);
                self.write(" = ");
                self.write(self.helper_prefix());
                self.write("__rest(");
                self.write(temp_name);
                self.write(", [");
                for (i, key) in excluded_keys.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write("\"");
                    self.write(key);
                    self.write("\"");
                }
                self.write("])");
            }

            self.writeln(";");
        }
    }

    /// Pre-scan statements to check if any for-of loop exists (for __values helper).
    pub(super) fn scan_needs_values(&mut self, stmts: &[Stmt]) {
        if !self.needs_downlevel("for-of") {
            return;
        }
        if self.options.down_level_iteration != Some(true) {
            return;
        }
        for stmt in stmts {
            if source_has_for_of(stmt) {
                self.needs_values_helper = true;
                return;
            }
        }
    }

    /// Emit the __values helper for downlevelIteration for-of transforms.
    pub(super) fn emit_values_helper(&mut self) {
        self.writeln("var __values = (this && this.__values) || function(o) {");
        self.indent += 1;
        self.writeln(
            "var s = typeof Symbol === \"function\" && Symbol.iterator, m = s && o[s], i = 0;",
        );
        self.writeln("if (m) return m.call(o);");
        self.writeln("if (o && typeof o.length === \"number\") return {");
        self.indent += 1;
        self.writeln("next: function () {");
        self.indent += 1;
        self.writeln("if (o && i >= o.length) o = void 0;");
        self.writeln("return { value: o && o[i++], done: !o };");
        self.indent -= 1;
        self.writeln("}");
        self.indent -= 1;
        self.writeln("};");
        self.writeln("throw new TypeError(s ? \"Object is not iterable.\" : \"Symbol.iterator is not defined.\");");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit TypeScript's iterable-to-array helper used by downleveled array
    /// binding patterns when `downLevelIteration` is enabled.
    pub(super) fn emit_read_helper(&mut self) {
        self.writeln("var __read = (this && this.__read) || function (o, n) {");
        self.indent += 1;
        self.writeln("var m = typeof Symbol === \"function\" && o[Symbol.iterator];");
        self.writeln("if (!m) return o;");
        self.writeln("var i = m.call(o), r, ar = [], e;");
        self.writeln("try {");
        self.indent += 1;
        self.writeln("while ((n === void 0 || n-- > 0) && !(r = i.next()).done) ar.push(r.value);");
        self.indent -= 1;
        self.writeln("}");
        self.write("catch (error) { e = { error: error };");
        self.writeln(" }");
        self.writeln("finally {");
        self.indent += 1;
        self.writeln("try {");
        self.indent += 1;
        self.writeln("if (r && !r.done && (m = i[\"return\"])) m.call(i);");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("finally { if (e) throw e.error; }");
        self.indent -= 1;
        self.writeln("}");
        self.writeln("return ar;");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Scan statements for classes with private fields (need downlevel helpers).
    pub(super) fn scan_needs_private_fields(&mut self, stmts: &[Stmt]) {
        let downlevel_private = self.needs_downlevel("private-fields");
        let use_define_semantics = self.use_define_for_class_fields();
        let use_define = use_define_semantics && !self.needs_downlevel("class-fields");
        // Track which helper is encountered first to determine emission order.
        let mut first_determined = false;

        for stmt in stmts {
            if !use_define
                && self.needs_downlevel("auto-accessors")
                && class_has_accessor_stmt(stmt)
            {
                // Auto-accessors always need both Get and Set helpers.
                // Auto-accessor getters are emitted first, so Get comes first.
                if !first_determined && !self.needs_private_field_get {
                    self.private_field_get_first = true;
                    first_determined = true;
                }
                self.needs_private_field_get = true;
                self.needs_private_field_set = true;
            }
            if downlevel_private {
                let (get, set) = class_needs_private_field_helpers_stmt(stmt);
                if !first_determined && (get || set) {
                    // Use the FIRST private access in source order to determine helper order.
                    // class_first_private_access_stmt returns the access kind of just the first
                    // class member that has private field usage, not the aggregate of all members.
                    let (first_get, first_set) = class_first_private_access_stmt(stmt);
                    if first_set && !first_get {
                        self.private_field_get_first = false;
                    } else {
                        self.private_field_get_first = true;
                    }
                    first_determined = true;
                }
                if get {
                    self.needs_private_field_get = true;
                }
                if set {
                    self.needs_private_field_set = true;
                }
                // Check for `#field in obj` expressions
                if !self.needs_private_field_in && class_needs_private_field_in_stmt(stmt) {
                    self.needs_private_field_in = true;
                }
            }
        }
    }

    /// Emit the __classPrivateFieldGet helper function.
    pub(super) fn emit_private_field_get_helper(&mut self) {
        self.writeln("var __classPrivateFieldGet = (this && this.__classPrivateFieldGet) || function (receiver, state, kind, f) {");
        self.indent += 1;
        self.writeln("if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a getter\");");
        self.writeln("if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot read private member from an object whose class did not declare it\");");
        self.writeln("return kind === \"m\" ? f : kind === \"a\" ? f.call(receiver) : f ? f.value : state.get(receiver);");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __classPrivateFieldIn helper function.
    #[allow(dead_code)] // private field `in` operator downlevel
    pub(super) fn emit_private_field_in_helper(&mut self) {
        self.writeln("var __classPrivateFieldIn = (this && this.__classPrivateFieldIn) || function(state, receiver) {");
        self.indent += 1;
        self.writeln("if (receiver === null || (typeof receiver !== \"object\" && typeof receiver !== \"function\")) throw new TypeError(\"Cannot use 'in' operator on non-object\");");
        self.writeln(
            "return typeof state === \"function\" ? receiver === state : state.has(receiver);",
        );
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __classPrivateFieldSet helper function.
    pub(super) fn emit_private_field_set_helper(&mut self) {
        self.writeln("var __classPrivateFieldSet = (this && this.__classPrivateFieldSet) || function (receiver, state, value, kind, f) {");
        self.indent += 1;
        self.writeln(
            "if (kind === \"m\") throw new TypeError(\"Private method is not writable\");",
        );
        self.writeln("if (kind === \"a\" && !f) throw new TypeError(\"Private accessor was defined without a setter\");");
        self.writeln("if (typeof state === \"function\" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError(\"Cannot write private member to an object whose class did not declare it\");");
        self.writeln("return (kind === \"a\" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value)), value;");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Emit the __setFunctionName helper function.
    /// Used for class expressions assigned to named bindings with static fields.
    pub(super) fn emit_set_function_name_helper(&mut self) {
        self.writeln("var __setFunctionName = (this && this.__setFunctionName) || function (f, name, prefix) {");
        self.indent += 1;
        self.writeln("if (typeof name === \"symbol\") name = name.description ? \"[\".concat(name.description, \"]\") : \"\";");
        self.writeln("return Object.defineProperty(f, \"name\", { configurable: true, value: prefix ? \"\".concat(prefix, \" \", name) : name });");
        self.indent -= 1;
        self.writeln("};");
    }

    pub(super) fn emit_prop_key_helper(&mut self) {
        self.writeln("var __propKey = (this && this.__propKey) || function (x) {");
        self.indent += 1;
        self.writeln("return typeof x === \"symbol\" ? x : \"\".concat(x);");
        self.indent -= 1;
        self.writeln("};");
    }

    /// Pre-scan statements to determine which CJS interop helpers are needed.
    /// TypeScript 5.x always wraps namespace imports with `__importStar` and
    /// default imports with `__importDefault` in CJS output (regardless of
    /// esModuleInterop). `__createBinding` + `__exportStar` are also always
    /// used for `export * from`.
    pub(super) fn scan_needed_helpers_for_import_decl(
        &mut self,
        import_decl: &ImportDecl,
        stmt_idx: usize,
    ) {
        if import_decl.type_only {
            return;
        }
        if self.import_decl_has_missing_namespace_as_recovery(import_decl) && !self.is_commonjs() {
            return;
        }
        // Parser-recovery malformed import:
        //   `import Foo From "./Foo";`
        // should not schedule __importDefault helper.
        if import_decl.source.is_empty() {
            let raw = self.copy_span_trimmed(import_decl.span);
            let start = (import_decl.span.end as usize).min(self.source.len());
            let rest = &self.source[start..];
            let eol = rest.find('\n').unwrap_or(rest.len());
            let line_tail = &rest[..eol];
            let norm = raw.split_whitespace().collect::<Vec<_>>().join(" ");
            if norm.starts_with("import ")
                && !norm.contains('{')
                && !norm.contains('*')
                && line_tail.trim_start().starts_with("From ")
            {
                return;
            }
        }
        match &import_decl.specifiers {
            ImportClause::Named {
                default,
                namespace,
                named,
            } => {
                let keep_default = default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                let keep_namespace = namespace.as_ref().is_some_and(|ns| {
                    ns != "<error>"
                        && !self.is_import_elided(ns)
                        && !self.merged_namespace_names.contains(ns.as_str())
                });
                let has_named = named
                    .iter()
                    .any(|s| !s.is_type && !self.is_import_elided(&s.local));
                let has_named_default = named.iter().any(|s| {
                    !s.is_type
                        && !self.is_import_elided(&s.local)
                        && s.imported.as_deref() == Some("default")
                });
                let has_named_non_default = named.iter().any(|s| {
                    !s.is_type
                        && !self.is_import_elided(&s.local)
                        && s.imported.as_deref() != Some("default")
                });
                if keep_namespace {
                    // import * as ns from 'mod' → __importStar
                    self.mark_import_star_helper_use(stmt_idx);
                } else if keep_default && has_named {
                    // import d, { a } from 'mod' → __importStar (combo)
                    self.mark_import_star_helper_use(stmt_idx);
                } else if has_named_default && has_named_non_default {
                    // import { default as d, a } from 'mod' → __importStar
                    self.mark_import_star_helper_use(stmt_idx);
                } else if keep_default {
                    // import d from 'mod' → __importDefault (default-only)
                    self.mark_import_default_helper_use(stmt_idx);
                } else if has_named_default {
                    // import { default as d } from 'mod' → __importDefault
                    self.mark_import_default_helper_use(stmt_idx);
                }
                // Named-only: import { a } from 'mod' → no helper needed
            }
            ImportClause::Require(_) => {}
        }
    }

    pub(super) fn import_decl_has_missing_namespace_as_recovery(
        &self,
        import_decl: &ImportDecl,
    ) -> bool {
        if !import_decl.source.is_empty() {
            return false;
        }
        let ImportClause::Named {
            default,
            named,
            namespace: Some(namespace),
        } = &import_decl.specifiers
        else {
            return false;
        };
        if default.is_some() || !named.is_empty() || namespace != "from" {
            return false;
        }
        self.copy_span_trimmed(import_decl.span)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .starts_with("import * from ")
    }

    pub(super) fn scan_needed_helpers(&mut self, stmts: &[Stmt]) {
        // Pre-scan for namespace declarations — when `import * as X` merges with
        // `namespace X`, TSC elides the import and uses only the namespace.
        let mut namespace_decl_names: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        for stmt in stmts {
            if let StmtKind::ModuleDecl(md) = &stmt.kind {
                if let ModuleName::Ident(name) = &md.name {
                    namespace_decl_names.insert(name.as_str());
                }
            }
        }
        self.merged_namespace_names = namespace_decl_names
            .iter()
            .map(|s| AstString::from(*s))
            .collect();

        for (stmt_idx, stmt) in stmts.iter().enumerate() {
            match &stmt.kind {
                StmtKind::Import(import_decl) => {
                    self.scan_needed_helpers_for_import_decl(import_decl, stmt_idx);
                }
                StmtKind::Export(export_decl) => {
                    match &export_decl.kind {
                        ExportDeclKind::Decl(inner) => {
                            if let StmtKind::Import(import_decl) = &inner.kind {
                                self.scan_needed_helpers_for_import_decl(import_decl, stmt_idx);
                            }
                        }
                        ExportDeclKind::All {
                            type_only, alias, ..
                        } => {
                            if *type_only {
                                continue;
                            }
                            if alias.is_some() {
                                self.mark_import_star_helper_use(stmt_idx);
                            } else {
                                self.mark_export_star_helper_use(stmt_idx);
                            }
                        }
                        ExportDeclKind::Named {
                            specifiers,
                            source: Some(_),
                            type_only,
                            ..
                        } => {
                            // export { default } from './mod' needs __importDefault
                            if !type_only {
                                for spec in specifiers {
                                    if !spec.is_type && spec.local == "default" {
                                        self.mark_import_default_helper_use(stmt_idx);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            if self.should_downlevel_dynamic_import() && self.stmt_has_dynamic_import_call(stmt) {
                self.mark_import_star_helper_use(stmt_idx);
            }
        }

        // Pre-scan for `using`/`await using` declarations that need disposal helpers.
        // This must be done during the pre-scan phase so helpers are emitted at the top
        // (either inline or via tslib import).
        if self.needs_downlevel("using") && self.options.no_emit_helpers != Some(true) {
            if self.file_has_using_declarations(stmts) {
                self.needs_add_disposable_resource_helper = true;
                self.needs_dispose_resources_helper = true;
            }
        }
    }

    /// Check if any statement (recursively) contains a `using`/`await using` declaration.
    pub(super) fn file_has_using_declarations(&self, stmts: &[Stmt]) -> bool {
        stmts
            .iter()
            .any(|stmt| self.stmt_has_using_declaration(stmt))
    }

    fn stmt_has_using_declaration(&self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Var(var) => self.var_stmt_has_using_declaration(var),
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
                self.expr_has_using_declaration(expr)
            }
            StmtKind::Return(expr) => expr
                .as_deref()
                .is_some_and(|expr| self.expr_has_using_declaration(expr)),
            StmtKind::If(stmt) => {
                self.expr_has_using_declaration(&stmt.test)
                    || self.stmt_has_using_declaration(&stmt.consequent)
                    || stmt
                        .alternate
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_has_using_declaration(stmt))
            }
            StmtKind::While(stmt) => {
                self.expr_has_using_declaration(&stmt.test)
                    || self.stmt_has_using_declaration(&stmt.body)
            }
            StmtKind::DoWhile(stmt) => {
                self.stmt_has_using_declaration(&stmt.body)
                    || self.expr_has_using_declaration(&stmt.test)
            }
            StmtKind::For(stmt) => {
                stmt.init.as_ref().is_some_and(|init| match init {
                    ForInit::Var(var) => self.var_stmt_has_using_declaration(var),
                    ForInit::Expr(expr) => self.expr_has_using_declaration(expr),
                }) || stmt
                    .test
                    .as_deref()
                    .is_some_and(|expr| self.expr_has_using_declaration(expr))
                    || stmt
                        .update
                        .as_deref()
                        .is_some_and(|expr| self.expr_has_using_declaration(expr))
                    || self.stmt_has_using_declaration(&stmt.body)
            }
            StmtKind::ForIn(stmt) => {
                self.for_in_of_left_has_using_declaration(&stmt.left)
                    || self.expr_has_using_declaration(&stmt.right)
                    || self.stmt_has_using_declaration(&stmt.body)
            }
            StmtKind::ForOf(stmt) => {
                self.for_in_of_left_has_using_declaration(&stmt.left)
                    || self.expr_has_using_declaration(&stmt.right)
                    || self.stmt_has_using_declaration(&stmt.body)
            }
            StmtKind::Switch(stmt) => {
                self.expr_has_using_declaration(&stmt.discriminant)
                    || stmt.cases.iter().any(|case| {
                        case.test
                            .as_deref()
                            .is_some_and(|expr| self.expr_has_using_declaration(expr))
                            || self.file_has_using_declarations(&case.consequent)
                    })
            }
            StmtKind::Try(stmt) => {
                self.file_has_using_declarations(&stmt.block)
                    || stmt.handler.as_ref().is_some_and(|handler| {
                        handler
                            .param
                            .as_ref()
                            .is_some_and(|pat| self.pat_has_using_declaration(pat))
                            || self.file_has_using_declarations(&handler.body)
                    })
                    || stmt
                        .finalizer
                        .as_deref()
                        .is_some_and(|body| self.file_has_using_declarations(body))
            }
            StmtKind::Block(stmts) => self.file_has_using_declarations(stmts),
            StmtKind::FnDecl(function) => self.fn_has_using_declaration(function),
            StmtKind::ClassDecl(class) => self.class_has_using_declaration(class),
            StmtKind::EnumDecl(enumeration) => enumeration.members.iter().any(|member| {
                self.prop_name_has_using_declaration(&member.name)
                    || member
                        .initializer
                        .as_deref()
                        .is_some_and(|expr| self.expr_has_using_declaration(expr))
            }),
            StmtKind::ModuleDecl(module) => module.body.as_ref().is_some_and(|body| match body {
                ModuleBody::Block(stmts) => self.file_has_using_declarations(stmts),
                ModuleBody::Module(module) => self.module_has_using_declaration(module),
            }),
            StmtKind::ImportEquals(import) => self.expr_has_using_declaration(&import.module_ref),
            StmtKind::Export(export) => match &export.kind {
                ExportDeclKind::Decl(stmt) | ExportDeclKind::DefaultDecl(stmt) => {
                    self.stmt_has_using_declaration(stmt)
                }
                ExportDeclKind::Default(expr) => self.expr_has_using_declaration(expr),
                ExportDeclKind::Named { .. } | ExportDeclKind::All { .. } => false,
            },
            StmtKind::Labeled(stmt) => self.stmt_has_using_declaration(&stmt.body),
            StmtKind::With(stmt) => {
                self.expr_has_using_declaration(&stmt.object)
                    || self.stmt_has_using_declaration(&stmt.body)
            }
            StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty
            | StmtKind::InterfaceDecl(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::Import(_)
            | StmtKind::Debugger => false,
        }
    }

    fn var_stmt_has_using_declaration(&self, var: &VarStmt) -> bool {
        matches!(var.kind, VarKind::Using | VarKind::AwaitUsing)
            || var.declarations.iter().any(|declaration| {
                self.pat_has_using_declaration(&declaration.name)
                    || declaration
                        .init
                        .as_deref()
                        .is_some_and(|expr| self.expr_has_using_declaration(expr))
            })
    }

    fn for_in_of_left_has_using_declaration(&self, left: &ForInOfLeft) -> bool {
        match left {
            ForInOfLeft::Var(var) => self.var_stmt_has_using_declaration(var),
            ForInOfLeft::Pat(pat) => self.pat_has_using_declaration(pat),
            ForInOfLeft::Expr(expr) => self.expr_has_using_declaration(expr),
        }
    }

    fn module_has_using_declaration(&self, module: &ModuleDecl) -> bool {
        module.body.as_ref().is_some_and(|body| match body {
            ModuleBody::Block(stmts) => self.file_has_using_declarations(stmts),
            ModuleBody::Module(module) => self.module_has_using_declaration(module),
        })
    }

    fn fn_has_using_declaration(&self, function: &FnDecl) -> bool {
        function
            .decorators
            .iter()
            .any(|expr| self.expr_has_using_declaration(expr))
            || function
                .params
                .iter()
                .any(|param| self.param_has_using_declaration(param))
            || function
                .body
                .as_deref()
                .is_some_and(|body| self.file_has_using_declarations(body))
    }

    fn param_has_using_declaration(&self, param: &Param) -> bool {
        self.pat_has_using_declaration(&param.name)
            || param
                .initializer
                .as_deref()
                .is_some_and(|expr| self.expr_has_using_declaration(expr))
            || param
                .decorators
                .iter()
                .any(|expr| self.expr_has_using_declaration(expr))
    }

    fn class_has_using_declaration(&self, class: &ClassDecl) -> bool {
        class
            .extends
            .as_deref()
            .is_some_and(|expr| self.expr_has_using_declaration(expr))
            || class
                .decorators
                .iter()
                .any(|expr| self.expr_has_using_declaration(expr))
            || class.members.iter().any(|member| match &member.kind {
                ClassMemberKind::Property(prop) => {
                    self.prop_name_has_using_declaration(&prop.name)
                        || prop
                            .initializer
                            .as_deref()
                            .is_some_and(|expr| self.expr_has_using_declaration(expr))
                        || prop
                            .decorators
                            .iter()
                            .any(|expr| self.expr_has_using_declaration(expr))
                }
                ClassMemberKind::Method(method) => {
                    self.prop_name_has_using_declaration(&method.name)
                        || method
                            .decorators
                            .iter()
                            .any(|expr| self.expr_has_using_declaration(expr))
                        || method
                            .params
                            .iter()
                            .any(|param| self.param_has_using_declaration(param))
                        || method
                            .body
                            .as_deref()
                            .is_some_and(|body| self.file_has_using_declarations(body))
                }
                ClassMemberKind::Constructor(constructor) => {
                    constructor
                        .decorators
                        .iter()
                        .any(|expr| self.expr_has_using_declaration(expr))
                        || constructor
                            .params
                            .iter()
                            .any(|param| self.param_has_using_declaration(param))
                        || constructor
                            .body
                            .as_deref()
                            .is_some_and(|body| self.file_has_using_declarations(body))
                }
                ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                    self.prop_name_has_using_declaration(&accessor.name)
                        || accessor
                            .decorators
                            .iter()
                            .any(|expr| self.expr_has_using_declaration(expr))
                        || accessor
                            .params
                            .iter()
                            .any(|param| self.param_has_using_declaration(param))
                        || accessor
                            .body
                            .as_deref()
                            .is_some_and(|body| self.file_has_using_declarations(body))
                }
                ClassMemberKind::IndexSignature(signature) => signature
                    .params
                    .iter()
                    .any(|param| self.param_has_using_declaration(param)),
                ClassMemberKind::StaticBlock(body) => self.file_has_using_declarations(body),
                ClassMemberKind::SemicolonClassElement => false,
            })
    }

    fn prop_name_has_using_declaration(&self, name: &PropName) -> bool {
        matches!(name, PropName::Computed(expr, _) if self.expr_has_using_declaration(expr))
    }

    fn pat_has_using_declaration(&self, pat: &Pat) -> bool {
        match &pat.kind {
            PatKind::Ident(_) => false,
            PatKind::Array(elements) => elements.iter().flatten().any(|element| match element {
                ArrayPatElem::Pat(pat) | ArrayPatElem::Rest(pat) => {
                    self.pat_has_using_declaration(pat)
                }
            }),
            PatKind::Object(properties) => properties.iter().any(|property| match property {
                ObjPatProp::KeyValue(name, pat) => {
                    self.prop_name_has_using_declaration(name)
                        || self.pat_has_using_declaration(pat)
                }
                ObjPatProp::Rest(pat) => self.pat_has_using_declaration(pat),
                ObjPatProp::ShorthandAssign(_, expr, _) => self.expr_has_using_declaration(expr),
                ObjPatProp::Shorthand(_, _) => false,
            }),
            PatKind::Assign(pat, expr) => {
                self.pat_has_using_declaration(pat) || self.expr_has_using_declaration(expr)
            }
            PatKind::Rest(pat) => self.pat_has_using_declaration(pat),
        }
    }

    fn expr_has_using_declaration(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Template(template) => template
                .exprs
                .iter()
                .any(|expr| self.expr_has_using_declaration(expr)),
            ExprKind::TaggedTemplate(template) => {
                self.expr_has_using_declaration(&template.tag)
                    || template
                        .quasi
                        .exprs
                        .iter()
                        .any(|expr| self.expr_has_using_declaration(expr))
            }
            ExprKind::ObjectLit(properties) => properties.iter().any(|property| match property {
                ObjLitProp::Property(prop) => {
                    self.prop_name_has_using_declaration(&prop.key)
                        || self.expr_has_using_declaration(&prop.value)
                }
                ObjLitProp::ShorthandDefault(_, expr, _) | ObjLitProp::Spread(expr, _) => {
                    self.expr_has_using_declaration(expr)
                }
                ObjLitProp::Method(method) => {
                    self.prop_name_has_using_declaration(&method.name)
                        || method
                            .params
                            .iter()
                            .any(|param| self.param_has_using_declaration(param))
                        || self.file_has_using_declarations(&method.body)
                }
                ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                    self.prop_name_has_using_declaration(&accessor.name)
                        || accessor
                            .params
                            .iter()
                            .any(|param| self.param_has_using_declaration(param))
                        || self.file_has_using_declarations(&accessor.body)
                }
                ObjLitProp::Shorthand(_, _) => false,
            }),
            ExprKind::Arrow(arrow) => {
                arrow
                    .params
                    .iter()
                    .any(|param| self.param_has_using_declaration(param))
                    || match &arrow.body {
                        ArrowBody::Block(stmts) => self.file_has_using_declarations(stmts),
                        ArrowBody::Expr(expr) => self.expr_has_using_declaration(expr),
                    }
            }
            ExprKind::FnExpr(function) => self.fn_has_using_declaration(function),
            ExprKind::ClassExpr(class) => self.class_has_using_declaration(class),
            ExprKind::Call(call) => {
                self.expr_has_using_declaration(&call.callee)
                    || call
                        .args
                        .iter()
                        .any(|arg| self.expr_has_using_declaration(arg))
            }
            ExprKind::New(new_expr) => {
                self.expr_has_using_declaration(&new_expr.callee)
                    || new_expr.args.as_ref().is_some_and(|args| {
                        args.iter().any(|arg| self.expr_has_using_declaration(arg))
                    })
            }
            ExprKind::Member(member) => self.expr_has_using_declaration(&member.object),
            ExprKind::ElemAccess(access) => {
                self.expr_has_using_declaration(&access.object)
                    || self.expr_has_using_declaration(&access.index)
            }
            ExprKind::Paren(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner)
            | ExprKind::NonNull(inner) => self.expr_has_using_declaration(inner),
            ExprKind::As(as_expr) => self.expr_has_using_declaration(&as_expr.expr),
            ExprKind::Satisfies(satisfies) => self.expr_has_using_declaration(&satisfies.expr),
            ExprKind::TypeAssertion(assertion) => self.expr_has_using_declaration(&assertion.expr),
            ExprKind::Instantiation(instantiation) => {
                self.expr_has_using_declaration(&instantiation.expr)
            }
            ExprKind::Cond(cond) => {
                self.expr_has_using_declaration(&cond.test)
                    || self.expr_has_using_declaration(&cond.consequent)
                    || self.expr_has_using_declaration(&cond.alternate)
            }
            ExprKind::Binary(binary) => {
                self.expr_has_using_declaration(&binary.left)
                    || self.expr_has_using_declaration(&binary.right)
            }
            ExprKind::Unary(unary) => self.expr_has_using_declaration(&unary.argument),
            ExprKind::Update(update) => self.expr_has_using_declaration(&update.argument),
            ExprKind::Assign(assign) => {
                self.expr_has_using_declaration(&assign.left)
                    || self.expr_has_using_declaration(&assign.right)
            }
            ExprKind::Comma(exprs) => exprs
                .iter()
                .any(|expr| self.expr_has_using_declaration(expr)),
            ExprKind::ArrayLit(elements) => elements
                .iter()
                .flatten()
                .any(|expr| self.expr_has_using_declaration(expr)),
            ExprKind::Yield(_, expr) => expr
                .as_deref()
                .is_some_and(|expr| self.expr_has_using_declaration(expr)),
            ExprKind::JsxElement(element) => {
                self.expr_has_using_declaration(&element.name)
                    || self.jsx_attributes_have_using_declaration(&element.attributes)
                    || self.jsx_children_have_using_declaration(&element.children)
            }
            ExprKind::JsxSelfClosing(element) => {
                self.expr_has_using_declaration(&element.name)
                    || self.jsx_attributes_have_using_declaration(&element.attributes)
            }
            ExprKind::JsxFragment(fragment) => {
                self.jsx_children_have_using_declaration(&fragment.children)
            }
            ExprKind::Ident(_)
            | ExprKind::NumLit(_)
            | ExprKind::BigIntLit(_)
            | ExprKind::StrLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::NullLit
            | ExprKind::RegexpLit(_)
            | ExprKind::NoSubstTemplate(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::MetaProp(_)
            | ExprKind::Omitted => false,
        }
    }

    fn jsx_attributes_have_using_declaration(&self, attributes: &[JsxAttribute]) -> bool {
        attributes.iter().any(|attribute| match attribute {
            JsxAttribute::Normal { value, .. } => value
                .as_deref()
                .is_some_and(|expr| self.expr_has_using_declaration(expr)),
            JsxAttribute::Spread(expr, _) => self.expr_has_using_declaration(expr),
        })
    }

    fn jsx_children_have_using_declaration(&self, children: &[JsxChild]) -> bool {
        children.iter().any(|child| match child {
            JsxChild::Text(_, _) => false,
            JsxChild::Element(expr) => self.expr_has_using_declaration(expr),
            JsxChild::Expression(expr, _) => expr
                .as_deref()
                .is_some_and(|expr| self.expr_has_using_declaration(expr)),
            JsxChild::Fragment(fragment) => {
                self.jsx_children_have_using_declaration(&fragment.children)
            }
        })
    }

    // ------------------------------------------------------------------
    // Import elision
    // ------------------------------------------------------------------

    /// Determine whether import elision should be active.
    pub(super) fn should_elide_imports(&self) -> bool {
        if self.is_js_file || self.options.verbatim_module_syntax == Some(true) {
            return false;
        }
        // All modes (Remove, Preserve, Error) trigger import elision.
        // The deprecated Preserve mode in TypeScript 5.x still removes
        // imports whose bindings are only used in type positions.
        true
    }

    pub(super) fn preserve_const_enums_effective(&self) -> bool {
        self.options.preserve_const_enums.unwrap_or(false)
            || self.options.isolated_modules.unwrap_or(false)
            || self.options.verbatim_module_syntax == Some(true)
    }

    pub(super) fn can_inline_imported_const_enums(&self) -> bool {
        !self.options.isolated_modules.unwrap_or(false)
    }

    /// Whether const enum values should be inlined at usage sites.
    /// Returns false when `isolatedModules` or `verbatimModuleSyntax` is set
    /// (TS 5.x treats const enums as regular enums in these modes).
    pub(super) fn should_inline_const_enums(&self) -> bool {
        !self.options.isolated_modules.unwrap_or(false)
            && self.options.verbatim_module_syntax != Some(true)
    }

    pub(super) fn rebuild_const_enum_object_names(&mut self) {
        self.const_enum_object_names.clear();
        for (name, _) in self.const_enum_values.keys() {
            self.const_enum_object_names
                .insert(AstString::from(name.as_str()));
        }
    }

    pub(super) fn is_const_enum_object_name(&self, name: &str) -> bool {
        self.can_inline_imported_const_enums() && self.const_enum_object_names.contains(name)
    }

    pub(super) fn is_const_enum_object_or_base_name(&self, name: &str) -> bool {
        if self.is_const_enum_object_name(name) {
            return true;
        }
        if !self.can_inline_imported_const_enums() {
            return false;
        }
        self.const_enum_object_names.iter().any(|obj| {
            obj.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('.'))
        })
    }

    fn has_foldable_member_constants_for_object(&self, name: &str) -> bool {
        self.file_consts.keys().any(|key| {
            key.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('.'))
        }) || self.file_string_consts.keys().any(|key| {
            key.strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('.'))
        })
    }

    /// Perform import elision analysis: scan all statements to determine which
    /// imported names are used in value positions.
    pub(super) fn analyze_import_elision(&mut self, stmts: &[Stmt]) {
        self.value_used_imports.clear();
        self.import_sources_with_value_bindings.clear();
        self.jsx_factory_import_retained.clear();
        self.jsx_element_import_retained.clear();
        self.import_elision_active = false;
        let should_elide = self.should_elide_imports();

        // Collect all imported names and map import sources to their local bindings.
        let mut imported_names: HashSet<AstString> = HashSet::new();
        let mut source_binding_names: HashMap<AstString, Vec<AstString>> = HashMap::new();
        // Separately track all require-import names (even those excluded by
        // global_type_only_names) so JSX factory detection can find them.
        let mut all_require_import_names: HashSet<AstString> = HashSet::new();
        // Track names from exported imports — these must never be elided.
        let mut exported_import_names: HashSet<AstString> = HashSet::new();
        // Track namespace imports from type-only external modules so JSX
        // factory detection can find them even when they're not in imported_names.
        let mut all_namespace_import_names: HashSet<AstString> = HashSet::new();
        for stmt in stmts {
            let (import_decl, is_exported) = match &stmt.kind {
                StmtKind::Import(import_decl) => (Some(import_decl), false),
                // Exported imports (`export import X = require(...)`) should
                // never be elided — the export provides a value binding.
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Decl(inner) => match &inner.kind {
                        StmtKind::Import(import_decl) => (Some(import_decl), true),
                        _ => (None, false),
                    },
                    _ => (None, false),
                },
                _ => (None, false),
            };
            let Some(import_decl) = import_decl else {
                continue;
            };
            if import_decl.type_only {
                continue;
            }
            let source_is_type_only_external = self
                .type_only_external_modules
                .contains(import_decl.source.as_str());
            let source_is_type_only_require = self
                .type_only_require_specs
                .contains(import_decl.source.as_str());

            match &import_decl.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    let mut bindings: Vec<AstString> = Vec::new();
                    if let Some(d) = default.as_ref() {
                        let default_is_effectively_type_only = source_is_type_only_external
                            || self.type_only_import_names.contains(d.as_str())
                            || self.global_type_only_export_names.contains("default");
                        if !default_is_effectively_type_only {
                            imported_names.insert(AstString::from(d.as_str()));
                            bindings.push(AstString::from(d.as_str()));
                        }
                    }
                    for spec in named {
                        let imported = spec.imported.as_ref().unwrap_or(&spec.local);
                        let is_effectively_type_only = spec.is_type
                            || self.type_only_import_names.contains(spec.local.as_str())
                            || self
                                .global_type_only_export_names
                                .contains(imported.as_str());
                        if !is_effectively_type_only {
                            imported_names.insert(AstString::from(spec.local.as_str()));
                            bindings.push(AstString::from(spec.local.as_str()));
                        }
                    }
                    if let Some(ns) = namespace.as_ref() {
                        // Always track namespace imports for JSX factory detection,
                        // even from type-only external modules (e.g. `import * as React from 'react'`
                        // where react.d.ts has `declare module "react" {}`).
                        all_namespace_import_names.insert(AstString::from(ns.as_str()));
                        if !source_is_type_only_external
                            && !self.type_only_import_names.contains(ns.as_str())
                        {
                            imported_names.insert(AstString::from(ns.as_str()));
                            bindings.push(AstString::from(ns.as_str()));
                        }
                    }
                    if !bindings.is_empty() {
                        source_binding_names
                            .entry(AstString::from(import_decl.source.as_str()))
                            .or_default()
                            .extend(bindings);
                    }
                }
                ImportClause::Require(name) => {
                    all_require_import_names.insert(AstString::from(name.as_str()));
                    if !source_is_type_only_external
                        && !source_is_type_only_require
                        && !self.global_type_only_export_names.contains(name.as_str())
                        && !self.global_type_only_names.contains(name.as_str())
                    {
                        imported_names.insert(AstString::from(name.as_str()));
                        source_binding_names
                            .entry(AstString::from(import_decl.source.as_str()))
                            .or_default()
                            .push(AstString::from(name.as_str()));
                    }
                    // Exported import-require must never be elided — the export
                    // provides a runtime value binding regardless of usage.
                    if is_exported {
                        exported_import_names.insert(AstString::from(name.as_str()));
                    }
                }
            }
        }

        // Also collect non-exported import-equals names for elision analysis.
        // TypeScript elides `import X = Y;` at file level when X is never
        // used in a value position (even if Y has a runtime value).
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::ImportEquals(ie) => {
                    imported_names.insert(AstString::from(ie.name.as_str()));
                }
                // Exported import-equals must NOT be elided — skip them.
                _ => {}
            }
        }

        if !should_elide {
            for (source, bindings) in source_binding_names {
                if !bindings.is_empty() {
                    self.import_sources_with_value_bindings.insert(source);
                }
            }
            return;
        }

        // Remove duplicate local bindings per source to avoid repeated scans.
        for bindings in source_binding_names.values_mut() {
            bindings.sort();
            bindings.dedup();
        }

        if imported_names.is_empty() && all_require_import_names.is_empty() {
            self.import_elision_active = true;
            return;
        }

        // Build a set of names where `import X = ...` is shadowed by a
        // type-only declaration (TypeAlias or InterfaceDecl with the same name).
        // TypeScript erases such import-equals statements, so their RHS
        // references should NOT count as value uses of imported bindings.
        let type_shadowed_import_equals: HashSet<String> = {
            // Collect all type-only declaration names in the file.
            let mut type_names: HashSet<String> = HashSet::new();
            for s in stmts {
                match &s.kind {
                    StmtKind::TypeAlias(t) => {
                        type_names.insert(t.name.clone());
                    }
                    StmtKind::InterfaceDecl(i) => {
                        type_names.insert(i.name.clone());
                    }
                    StmtKind::Export(ed) => {
                        if let ExportDeclKind::Decl(d) = &ed.kind {
                            match &d.kind {
                                StmtKind::TypeAlias(t) => {
                                    type_names.insert(t.name.clone());
                                }
                                StmtKind::InterfaceDecl(i) => {
                                    type_names.insert(i.name.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Find import-equals names that also have a type-only declaration.
            let mut shadowed: HashSet<String> = HashSet::new();
            for s in stmts {
                if let StmtKind::ImportEquals(ie) = &s.kind {
                    if type_names.contains(ie.name.as_str()) {
                        shadowed.insert(ie.name.clone());
                    }
                }
            }
            shadowed
        };

        // Collect import-equals statements for deferred RHS scanning.
        // Their RHS should only be scanned if the import-equals name itself
        // is used as a value — otherwise the RHS refs shouldn't keep imports alive.
        let mut import_equals_stmts: Vec<(&str, &Expr)> = Vec::new();
        for stmt in stmts {
            if let StmtKind::ImportEquals(ie) = &stmt.kind {
                if !type_shadowed_import_equals.contains(ie.name.as_str()) {
                    import_equals_stmts.push((ie.name.as_str(), &ie.module_ref));
                }
            }
        }

        // Scan all non-import, non-import-equals statements for value usage of imported names.
        // Track local declarations progressively: after processing a statement
        // that declares `const X = init`, scan the init for import refs first,
        // THEN remove `X` from imported_names so subsequent statements don't
        // count their `X` references as import uses.
        let mut value_used: HashSet<AstString> = HashSet::new();
        let mut value_used_non_member: HashSet<AstString> = HashSet::new();
        let mut value_used_export: HashSet<AstString> = HashSet::new();
        // The loop below only ever SHRINKS imported_names (local shadowing), so
        // the first-byte mask computed once here stays a sound superset for
        // every per-statement re-borrow (stale bits are documented as safe).
        let imported_name_bits = FilteredNames::compute_bits(&imported_names);
        for stmt in stmts {
            // Nothing left that could be value-used — later statements can't
            // add anything, so skip their whole-subtree walks.
            if imported_names.is_empty() {
                break;
            }
            let is_import_stmt = match &stmt.kind {
                StmtKind::Import(_) => true,
                StmtKind::Export(export_decl) => {
                    if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                        matches!(inner.kind, StmtKind::Import(_))
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if is_import_stmt {
                continue;
            }
            // Skip ALL import-equals statements — their RHS will be scanned
            // in a deferred pass below, only if the name is value-used.
            if matches!(&stmt.kind, StmtKind::ImportEquals(..)) {
                continue;
            }
            let nf = FilteredNames::with_bits(&imported_names, imported_name_bits);
            collect_value_refs_stmt(stmt, &nf, &mut value_used, &mut value_used_non_member);

            // After scanning this statement, remove any locally-declared names
            // from imported_names so subsequent statements won't count references
            // to the local binding as import value-uses.
            fn remove_local_decl_names(s: &Stmt, names: &mut HashSet<AstString>) {
                match &s.kind {
                    StmtKind::Var(v) => {
                        for decl in &v.declarations {
                            if let PatKind::Ident(name) = &decl.name.kind {
                                names.remove(name.as_str());
                            }
                        }
                    }
                    // Don't remove function/class/enum names from imported_names.
                    // When a function/class/enum has the same name as an import,
                    // it's always a duplicate-identifier error. TypeScript
                    // conservatively keeps the import in this case, so subsequent
                    // references to the name should still count as import uses.
                    _ => {}
                }
            }
            remove_local_decl_names(stmt, &mut imported_names);
            if let StmtKind::Export(ed) = &stmt.kind {
                if let ExportDeclKind::Decl(d) = &ed.kind {
                    remove_local_decl_names(d, &mut imported_names);
                }
            }
        }

        // imported_names is no longer mutated below — build the first-byte
        // filter once and share it across the remaining value-ref walks
        // (deferred import-equals RHS + export-position + enum-init scans, incl.
        // the second full-file walk for enum-init-vs-outside analysis).
        let nf2 = FilteredNames::new(&imported_names);

        // Deferred import-equals RHS scanning: only scan RHS of import-equals
        // whose name is value-used. Iterate until stable to handle transitive
        // chains like `import f = foo.M1; import g = f.X;`.
        loop {
            let mut changed = false;
            for &(ie_name, rhs) in &import_equals_stmts {
                if !value_used.contains(ie_name) {
                    continue;
                }
                let prev_len = value_used.len();
                collect_value_refs_expr(
                    rhs,
                    &nf2,
                    &mut value_used,
                    &mut value_used_non_member,
                    false,
                );
                if value_used.len() > prev_len {
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Track explicit export-position uses so const-enum imports can still
        // be retained when runtime enum values are preserved.
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Named {
                        specifiers,
                        source: None,
                        type_only: false,
                    } => {
                        for spec in specifiers {
                            if spec.is_type {
                                continue;
                            }
                            if imported_names.contains(spec.local.as_str()) {
                                value_used_export.insert(AstString::from(spec.local.as_str()));
                            }
                        }
                    }
                    ExportDeclKind::Default(expr) => {
                        if let ExprKind::Ident(name) = &expr.kind {
                            if imported_names.contains(name.as_str()) {
                                value_used_export.insert(AstString::from(name.as_str()));
                            }
                        }
                    }
                    _ => {}
                },
                StmtKind::ExportAssign(expr) => {
                    let mut export_refs: HashSet<AstString> = HashSet::new();
                    let mut export_refs_non_member: HashSet<AstString> = HashSet::new();
                    collect_value_refs_expr(
                        expr,
                        &nf2,
                        &mut export_refs,
                        &mut export_refs_non_member,
                        false,
                    );
                    value_used_export.extend(export_refs);
                }
                _ => {}
            }
        }
        // Collect names referenced in enum member initializers, and names
        // referenced outside enum member initializers. Regular enum member
        // initializer values get folded at compile time (e.g.
        // `enum E { A = ImportedEnum.X }` → `E["A"] = 0`), so if an import
        // is used ONLY in such positions, it can be elided for non-const enums
        // with known values.
        let (enum_init_refs, value_used_outside_enum_init): (
            HashSet<AstString>,
            HashSet<AstString>,
        ) = {
            let mut init_refs: HashSet<AstString> = HashSet::new();
            let mut init_refs_nm: HashSet<AstString> = HashSet::new();
            let mut outside_refs: HashSet<AstString> = HashSet::new();
            let mut outside_refs_nm: HashSet<AstString> = HashSet::new();
            for stmt in stmts {
                let en = match &stmt.kind {
                    StmtKind::EnumDecl(e) => Some(e),
                    StmtKind::Export(ed) => match &ed.kind {
                        ExportDeclKind::Decl(d) => match &d.kind {
                            StmtKind::EnumDecl(e) => Some(e),
                            _ => None,
                        },
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(en) = en {
                    // Scan enum member initializers separately
                    for m in &en.members {
                        if let Some(ref init) = m.initializer {
                            collect_value_refs_expr(
                                init,
                                &nf2,
                                &mut init_refs,
                                &mut init_refs_nm,
                                false,
                            );
                        }
                        // Enum member names (computed) count as outside refs
                        collect_value_refs_prop_name(
                            &m.name,
                            &nf2,
                            &mut outside_refs,
                            &mut outside_refs_nm,
                        );
                    }
                } else {
                    // Non-enum statements: scan into outside_refs
                    let is_import = match &stmt.kind {
                        StmtKind::Import(_) | StmtKind::ImportEquals(..) => true,
                        StmtKind::Export(ed) => matches!(
                            &ed.kind,
                            ExportDeclKind::Decl(inner) if matches!(inner.kind, StmtKind::Import(_))
                        ),
                        _ => false,
                    };
                    if !is_import {
                        collect_value_refs_stmt(
                            stmt,
                            &nf2,
                            &mut outside_refs,
                            &mut outside_refs_nm,
                        );
                    }
                }
            }
            (init_refs, outside_refs)
        };

        // Non-type-only `export { name }` is a value use — the imported
        // binding must be retained to provide the exported value at runtime.
        value_used.extend(value_used_export.iter().cloned());

        // Exported import declarations (`export import X = require(...)`)
        // must never be elided — the export provides a runtime binding.
        value_used.extend(exported_import_names.iter().cloned());

        // Imported const enums referenced only as member-object bases
        // (e.g. `E.A`) are compile-time only and should not keep runtime
        // import bindings alive. TypeScript elides such imports even when
        // preserveConstEnums is true, because the inlined literal values
        // are self-contained and don't need the runtime import.
        value_used.retain(|name| {
            let is_ce = self.is_const_enum_object_or_base_name(name);
            let has_foldable = self.should_inline_const_enums()
                && self.has_foldable_member_constants_for_object(name);
            // For non-const-enum names with foldable constants (regular enums
            // from external files): only filter if the name is used EXCLUSIVELY
            // in enum member initializers (which get folded at compile time).
            // If the name is also used in runtime expressions (e.g. `let x = Foo.A`),
            // the import must be kept.
            let foldable_enum_init_only = has_foldable
                && !is_ce
                && enum_init_refs.contains(name)
                && !value_used_outside_enum_init.contains(name);
            !(is_ce || foldable_enum_init_only)
                || value_used_non_member.contains(name)
                || (is_ce && self.runtime_export_import_names.contains(name))
                || (self.preserve_const_enums_effective() && value_used_export.contains(name))
                // Regular enums (not const) with foldable constants must be
                // retained when re-exported — the export binding needs the
                // runtime value even though member accesses are folded.
                || (!is_ce && has_foldable && value_used_export.contains(name))
        });

        // When jsx: react (or react-native / preserve), JSX elements implicitly
        // reference the JSX factory (`React.createElement` by default).  If any
        // JSX exists in the file, mark the factory root as value-used so its
        // import isn't elided.  Similarly, JSX fragments implicitly reference
        // the fragment factory (`React.Fragment` by default).
        if matches!(
            self.options.jsx,
            Some(JsxEmit::React) | Some(JsxEmit::ReactNative) | Some(JsxEmit::Preserve)
        ) {
            if stmts.iter().any(|s| stmt_contains_jsx(s)) {
                // Use per-file @jsx pragma if present, then compiler option,
                // then default "React".
                let effective_factory = self
                    .jsx_pragma_factory
                    .as_deref()
                    .or(self.options.jsx_factory.as_deref())
                    .unwrap_or("React");
                let factory_root = effective_factory
                    .split('.')
                    .next()
                    .unwrap_or("React")
                    .to_string();
                // Check both imported_names and all_require_import_names.
                // The factory root may be in global_type_only_names (e.g.
                // `declare namespace React`) but the require import still
                // creates a value binding needed by JSX at runtime.
                if imported_names.contains(factory_root.as_str())
                    || all_require_import_names.contains(factory_root.as_str())
                    || all_namespace_import_names.contains(factory_root.as_str())
                {
                    let factory_root_ast = AstString::from(factory_root.as_str());
                    value_used.insert(factory_root_ast.clone());
                    imported_names.insert(factory_root_ast.clone());
                    self.jsx_factory_import_retained.insert(factory_root_ast);
                }
                // Also mark the fragment factory root so its import isn't elided.
                // Only default to "React" when the main factory is also the
                // default (React.createElement).  When a custom jsxFactory is
                // set, the fragment factory must be explicitly provided via
                // jsxFragmentFactory — otherwise there is no implicit React dep.
                let effective_frag = self
                    .jsx_pragma_fragment
                    .as_deref()
                    .or(self.options.jsx_fragment_factory.as_deref());
                let has_custom_factory =
                    self.jsx_pragma_factory.is_some() || self.options.jsx_factory.is_some();
                let frag_factory_root = if let Some(frag) = effective_frag {
                    frag.split('.').next().unwrap_or("React").to_string()
                } else if !has_custom_factory {
                    // Default factory → default fragment factory is React.Fragment
                    "React".to_string()
                } else {
                    // Custom factory without explicit fragment factory —
                    // no implicit React dependency for fragments.
                    String::new()
                };
                if !frag_factory_root.is_empty()
                    && (imported_names.contains(frag_factory_root.as_str())
                        || all_require_import_names.contains(frag_factory_root.as_str())
                        || all_namespace_import_names.contains(frag_factory_root.as_str()))
                {
                    let frag_root_ast = AstString::from(frag_factory_root.as_str());
                    value_used.insert(frag_root_ast.clone());
                    imported_names.insert(frag_root_ast.clone());
                    self.jsx_factory_import_retained.insert(frag_root_ast);
                }
            }
        }

        // Uppercase JSX component names like <MyComponent /> reference values
        // whose imports must not be elided (they become React.createElement(MyComponent, ...)
        // or _jsx(MyComponent, ...) or are preserved as-is in preserve mode).
        {
            let jsx_component_names = collect_jsx_component_names(stmts);
            for name in &jsx_component_names {
                if imported_names.contains(name.as_str()) {
                    value_used.insert(AstString::from(name.as_str()));
                    // In JSX preserve mode, tags stay as-is, so the identifier
                    // must exist at runtime. Mark it separately to override
                    // reference-path preamble elision.
                    if self.jsx_is_preserve() {
                        self.jsx_element_import_retained
                            .insert(AstString::from(name.as_str()));
                    }
                }
            }
        }

        // When emitDecoratorMetadata is on, type annotations on decorated
        // class members are serialized at runtime via __metadata calls.
        // Imported types referenced in those annotations must be retained.
        // We use serialize_type_for_metadata to determine the ACTUAL runtime
        // value — only imports whose names appear directly in the serialized
        // form need to be retained (e.g. `SomeClass | null | string` → `Object`
        // does NOT reference SomeClass at runtime).
        if self.options.emit_decorator_metadata == Some(true)
            && self.options.experimental_decorators == Some(true)
        {
            let mut metadata_serialized: Vec<String> = Vec::new();
            let mut metadata_type_refs: HashSet<String> = HashSet::new();
            let type_only_import_names_ref = &self.type_only_import_names;
            let global_type_only_names_ref = &self.global_type_only_names;
            let enum_decl_names_ref = &self.enum_decl_names;
            let strict_null_checks = self
                .options
                .strict_null_checks
                .unwrap_or(self.options.strict.unwrap_or(false));
            for stmt in stmts {
                let class = match &stmt.kind {
                    StmtKind::ClassDecl(c) => Some(c),
                    StmtKind::Export(e) => {
                        if let ExportDeclKind::Decl(d) = &e.kind {
                            if let StmtKind::ClassDecl(c) = &d.kind {
                                Some(c)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let Some(class) = class else { continue };
                // Collect type-only names for this class (class type params).
                let mut class_type_only: HashSet<AstString> = self.type_only_decl_names.clone();
                if let Some(ref type_params) = class.type_params {
                    for tp in type_params {
                        class_type_only.insert(AstString::from(tp.name.as_str()));
                    }
                }
                for member in &class.members {
                    match &member.kind {
                        ClassMemberKind::Method(m) => {
                            if m.decorators.is_empty()
                                && m.params.iter().all(|p| p.decorators.is_empty())
                            {
                                continue;
                            }
                            for p in &m.params {
                                if let Some(type_ann) = &p.type_ann {
                                    collect_metadata_type_refs(
                                        type_ann,
                                        self.source,
                                        &class_type_only,
                                        type_only_import_names_ref,
                                        global_type_only_names_ref,
                                        enum_decl_names_ref,
                                        &mut metadata_type_refs,
                                        strict_null_checks,
                                    );
                                }
                                metadata_serialized.push(serialize_param_type_for_metadata(
                                    p,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    strict_null_checks,
                                ));
                            }
                            if let Some(ref t) = m.return_type {
                                collect_metadata_type_refs(
                                    t,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    &mut metadata_type_refs,
                                    strict_null_checks,
                                );
                                metadata_serialized.push(serialize_type_for_metadata(
                                    t,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    strict_null_checks,
                                ));
                            }
                        }
                        ClassMemberKind::Property(p) => {
                            if p.decorators.is_empty() {
                                continue;
                            }
                            if let Some(ref t) = p.type_ann {
                                collect_metadata_type_refs(
                                    t,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    &mut metadata_type_refs,
                                    strict_null_checks,
                                );
                                metadata_serialized.push(serialize_type_for_metadata(
                                    t,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    strict_null_checks,
                                ));
                            }
                        }
                        ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                            if a.decorators.is_empty() {
                                continue;
                            }
                            let is_getter = matches!(&member.kind, ClassMemberKind::GetAccessor(_));
                            let acc_name = prop_name_str(&a.name);
                            // Scan own params
                            for p in &a.params {
                                if let Some(type_ann) = &p.type_ann {
                                    collect_metadata_type_refs(
                                        type_ann,
                                        self.source,
                                        &class_type_only,
                                        type_only_import_names_ref,
                                        global_type_only_names_ref,
                                        enum_decl_names_ref,
                                        &mut metadata_type_refs,
                                        strict_null_checks,
                                    );
                                }
                                metadata_serialized.push(serialize_param_type_for_metadata(
                                    p,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    strict_null_checks,
                                ));
                            }
                            if let Some(ref t) = a.return_type {
                                collect_metadata_type_refs(
                                    t,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    &mut metadata_type_refs,
                                    strict_null_checks,
                                );
                                metadata_serialized.push(serialize_type_for_metadata(
                                    t,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    strict_null_checks,
                                ));
                            }
                            // Also scan paired accessor's types (for design:type
                            // and design:paramtypes which merge get/set info)
                            if is_getter {
                                // Find paired setter for param types + fallback design:type
                                for other in &class.members {
                                    if let ClassMemberKind::SetAccessor(s) = &other.kind {
                                        if prop_name_str(&s.name) == acc_name {
                                            for p in &s.params {
                                                if let Some(type_ann) = &p.type_ann {
                                                    collect_metadata_type_refs(
                                                        type_ann,
                                                        self.source,
                                                        &class_type_only,
                                                        type_only_import_names_ref,
                                                        global_type_only_names_ref,
                                                        enum_decl_names_ref,
                                                        &mut metadata_type_refs,
                                                        strict_null_checks,
                                                    );
                                                }
                                                metadata_serialized.push(
                                                    serialize_param_type_for_metadata(
                                                        p,
                                                        self.source,
                                                        &class_type_only,
                                                        type_only_import_names_ref,
                                                        global_type_only_names_ref,
                                                        enum_decl_names_ref,
                                                        strict_null_checks,
                                                    ),
                                                );
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Find paired getter for return type
                                for other in &class.members {
                                    if let ClassMemberKind::GetAccessor(g) = &other.kind {
                                        if prop_name_str(&g.name) == acc_name {
                                            if let Some(ref t) = g.return_type {
                                                collect_metadata_type_refs(
                                                    t,
                                                    self.source,
                                                    &class_type_only,
                                                    type_only_import_names_ref,
                                                    global_type_only_names_ref,
                                                    enum_decl_names_ref,
                                                    &mut metadata_type_refs,
                                                    strict_null_checks,
                                                );
                                                metadata_serialized.push(
                                                    serialize_type_for_metadata(
                                                        t,
                                                        self.source,
                                                        &class_type_only,
                                                        type_only_import_names_ref,
                                                        global_type_only_names_ref,
                                                        enum_decl_names_ref,
                                                        strict_null_checks,
                                                    ),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // Constructor parameters for class-level decorators
                if !class.decorators.is_empty() {
                    for member in &class.members {
                        if let ClassMemberKind::Constructor(ctor) = &member.kind {
                            for p in &ctor.params {
                                if let Some(type_ann) = &p.type_ann {
                                    collect_metadata_type_refs(
                                        type_ann,
                                        self.source,
                                        &class_type_only,
                                        type_only_import_names_ref,
                                        global_type_only_names_ref,
                                        enum_decl_names_ref,
                                        &mut metadata_type_refs,
                                        strict_null_checks,
                                    );
                                }
                                metadata_serialized.push(serialize_param_type_for_metadata(
                                    p,
                                    self.source,
                                    &class_type_only,
                                    type_only_import_names_ref,
                                    global_type_only_names_ref,
                                    enum_decl_names_ref,
                                    strict_null_checks,
                                ));
                            }
                        }
                    }
                }
            }
            for reference in &metadata_type_refs {
                let root = reference.split('.').next().unwrap_or(reference.as_str());
                if imported_names.contains(root) {
                    value_used.insert(AstString::from(root));
                }
            }
            // Check each serialized metadata value — if it matches an
            // imported name, mark that import as value-used.
            // For qualified names like `t1.T1` (namespace import member access),
            // also check the root identifier (e.g. `t1`) against imported names.
            for serialized in &metadata_serialized {
                if imported_names.contains(serialized.as_str()) {
                    value_used.insert(AstString::from(serialized.as_str()));
                } else if let Some(root) = serialized.split('.').next() {
                    if root != serialized.as_str() && imported_names.contains(root) {
                        value_used.insert(AstString::from(root));
                    }
                }
            }
        }

        for (source, bindings) in source_binding_names {
            if bindings.iter().any(|name| value_used.contains(name)) {
                self.import_sources_with_value_bindings.insert(source);
            }
        }

        self.value_used_imports = value_used;
        self.import_elision_active = true;
    }

    /// Check if an import name should be elided.
    pub(super) fn is_import_elided(&self, name: &str) -> bool {
        self.import_elision_active && !self.value_used_imports.contains(name)
    }

    pub(super) fn should_elide_require_import(&self, import_decl: &ImportDecl) -> bool {
        fn reference_path_targets_declaration_file(line: &str) -> bool {
            let Some(path_idx) = line.find("path") else {
                return false;
            };
            let after_path = &line[path_idx + 4..];
            let Some(eq_idx) = after_path.find('=') else {
                return false;
            };
            let after_eq = after_path[eq_idx + 1..].trim_start();
            let Some(quote) = after_eq.chars().next() else {
                return false;
            };
            if quote != '"' && quote != '\'' {
                return false;
            }
            let rest = &after_eq[quote.len_utf8()..];
            let Some(end_idx) = rest.find(quote) else {
                return false;
            };
            let path = &rest[..end_idx];
            path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
        }

        if import_decl.source.starts_with("./")
            || import_decl.source.starts_with("../")
            || import_decl.source.starts_with('/')
        {
            return false;
        }
        let start = import_decl.span.start as usize;
        if start == 0 || start > self.source.len() {
            return false;
        }
        let prefix = &self.source[..start];
        let mut has_reference_path = false;
        for line in prefix.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("///<reference path")
                || trimmed.starts_with("/// <reference path")
            {
                if !reference_path_targets_declaration_file(trimmed) {
                    return false;
                }
                has_reference_path = true;
                continue;
            }
            if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            return false;
        }
        has_reference_path
    }

    pub(super) fn import_decl_will_emit(&self, import_decl: &ImportDecl) -> bool {
        if import_decl.type_only {
            // verbatimModuleSyntax: `import type { ... } from "mod"` with named specifiers
            // emits as `import {} from "mod"`. Default-only type imports are erased.
            if self.options.verbatim_module_syntax == Some(true) && !self.is_cjs_like() {
                return matches!(
                    &import_decl.specifiers,
                    ImportClause::Named { named, .. } if !named.is_empty()
                );
            }
            return false;
        }

        if self.is_cjs_like() {
            match &import_decl.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    if named
                        .iter()
                        .any(|s| s.local == "<error>" || s.imported.as_deref() == Some("<error>"))
                    {
                        return true;
                    }
                    let non_type: Vec<_> = named
                        .iter()
                        .filter(|s| {
                            !s.is_type
                                && !self.is_import_elided(&s.local)
                                && !self.merged_namespace_names.contains(s.local.as_str())
                        })
                        .collect();
                    let keep_default = default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                    let keep_namespace = namespace.as_ref().is_some_and(|ns| {
                        ns != "<error>"
                            && !self.is_import_elided(ns)
                            && !self.merged_namespace_names.contains(ns.as_str())
                    });
                    let has_bindings = keep_default || keep_namespace || !non_type.is_empty();
                    if has_bindings {
                        return true;
                    }

                    let had_original_bindings =
                        default.is_some() || namespace.is_some() || !named.is_empty(); // any named specifier (even type-only)
                                                                                       // Also check source text: `import { } from "m"` contains `{` but
                                                                                       // has an empty named list after stripping type-only specifiers.
                                                                                       // Distinguish it from pure side-effect `import "m"` by checking the
                                                                                       // span text for an opening brace.
                    let has_empty_named_brace = !had_original_bindings && {
                        let start = import_decl.span.start as usize;
                        let end = import_decl.span.end as usize;
                        if start <= self.source.len() && end <= self.source.len() && start <= end {
                            self.source[start..end].contains('{')
                        } else {
                            false
                        }
                    };
                    if had_original_bindings || has_empty_named_brace {
                        // Keep the import for side effects if the source is
                        // in the keep-side-effect set (e.g. .js file with
                        // `export type *` re-exporting values).
                        if self
                            .cjs_keep_side_effect_sources
                            .contains(import_decl.source.as_str())
                        {
                            return true;
                        }
                        return false;
                    }
                    // Side-effect import in CJS-like emit.
                    return !self
                        .import_sources_with_value_bindings
                        .contains(import_decl.source.as_str());
                }
                ImportClause::Require(name) => {
                    // Reference-path preamble elision: when the import follows
                    // only `/// <reference path>` directives, elide the import
                    // entirely (it's ambient). References stay as bare names;
                    // JSX/runtime cases opt back in below.
                    if self.should_elide_require_import(import_decl) {
                        // Override: keep the import if it's the JSX factory root,
                        // or used as a JSX element tag name.
                        if self.jsx_factory_import_retained.contains(name.as_str()) {
                            return true;
                        }
                        if self.jsx_element_import_retained.contains(name.as_str()) {
                            return true;
                        }
                        return false;
                    }
                    if !self.is_import_elided(name) {
                        return true;
                    }
                    return false;
                }
            }
        }

        match &import_decl.specifiers {
            ImportClause::Named {
                default,
                named,
                namespace,
            } => {
                if named
                    .iter()
                    .any(|s| s.local == "<error>" || s.imported.as_deref() == Some("<error>"))
                {
                    return true;
                }
                let non_type: Vec<_> = named
                    .iter()
                    .filter(|s| {
                        !s.is_type
                            && !self.is_import_elided(&s.local)
                            && !self.merged_namespace_names.contains(s.local.as_str())
                    })
                    .collect();
                let keep_default = default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                let keep_namespace = namespace.as_ref().is_some_and(|ns| {
                    ns != "<error>"
                        && !self.is_import_elided(ns)
                        && !self.merged_namespace_names.contains(ns.as_str())
                });
                if keep_default || keep_namespace || !non_type.is_empty() {
                    return true;
                }
                let had_original_bindings =
                    default.is_some() || namespace.is_some() || named.iter().any(|s| !s.is_type);
                !had_original_bindings
            }
            ImportClause::Require(name) => !self.is_import_elided(name),
        }
    }

    pub(super) fn stmt_import_decl(stmt: &Stmt) -> Option<&ImportDecl> {
        match &stmt.kind {
            StmtKind::Import(import_decl) => Some(import_decl),
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) => {
                    if let StmtKind::Import(import_decl) = &inner.kind {
                        Some(import_decl)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns `true` when an import-equals statement will produce output
    /// (should not be elided). Mirrors the elision checks in `emit_stmt`'s
    /// `ImportEquals` handler so that the AMD/UMD module loop can skip
    /// leading comments for elided import-equals.
    pub(super) fn import_equals_will_emit(&self, name: &str, rhs: &Expr) -> bool {
        if self.import_equals_rhs_is_type_only_require_spec(rhs) {
            return false;
        }
        if crate::emit_stmt_helpers::import_equals_rhs_has_error(rhs) {
            return true; // error recovery always emits something
        }
        let verbatim_keep = self.options.verbatim_module_syntax == Some(true);
        if !verbatim_keep && !self.import_equals_rhs_has_runtime_value(rhs) {
            if !self.should_preserve_script_type_only_import_equals(name, rhs) {
                return false;
            }
        }
        if !verbatim_keep
            && self.is_module_file
            && self.export_target.as_ref().map_or(true, |t| t == "exports")
            && self.is_import_elided(name)
        {
            return false;
        }
        if self.type_only_decl_names.contains(name) {
            return false;
        }
        true
    }

    // ------------------------------------------------------------------
    // Export helpers
    // ------------------------------------------------------------------

    pub(super) fn next_omitted_array_temp(&mut self) -> String {
        let n = self.omitted_array_destructure_counter;
        self.omitted_array_destructure_counter += 1;
        let letter = (b'a' + (n % 26) as u8) as char;
        let suffix = n / 26;
        if suffix == 0 {
            format!("_{letter}")
        } else {
            format!("_{letter}{suffix}")
        }
    }

    pub(super) fn collect_omitted_array_chain(
        &mut self,
        pat: &Pat,
        parent_temp: &str,
        chain: &mut Vec<(String, String, usize)>,
    ) {
        match &pat.kind {
            PatKind::Array(elems) => {
                for (idx, elem_opt) in elems.iter().enumerate() {
                    let Some(elem) = elem_opt else { continue };
                    match elem {
                        ArrayPatElem::Pat(p) => {
                            if matches!(p.kind, PatKind::Array(_) | PatKind::Object(_)) {
                                let child = self.next_omitted_array_temp();
                                chain.push((child.clone(), parent_temp.to_string(), idx));
                                self.collect_omitted_array_chain(p, &child, chain);
                            }
                        }
                        ArrayPatElem::Rest(p) => {
                            // Rare for omitted-name patterns; skip unsupported rest lowering.
                            if matches!(p.kind, PatKind::Array(_) | PatKind::Object(_)) {
                                let child = self.next_omitted_array_temp();
                                chain.push((child.clone(), parent_temp.to_string(), idx));
                                self.collect_omitted_array_chain(p, &child, chain);
                            }
                        }
                    }
                }
            }
            PatKind::Object(props) => {
                for prop in props {
                    match prop {
                        ObjPatProp::KeyValue(_, value) | ObjPatProp::Rest(value) => {
                            if matches!(value.kind, PatKind::Array(_) | PatKind::Object(_)) {
                                let child = self.next_omitted_array_temp();
                                // Object-pattern omitted plans are not index-addressable in this
                                // lightweight lowering; keep recursion to reserve stable temp names.
                                // No direct chain entry is emitted for object keys here.
                                self.collect_omitted_array_chain(value, &child, chain);
                            }
                        }
                        ObjPatProp::Shorthand(_, _) | ObjPatProp::ShorthandAssign(_, _, _) => {}
                    }
                }
            }
            PatKind::Assign(inner, _) | PatKind::Rest(inner) => {
                self.collect_omitted_array_chain(inner, parent_temp, chain);
            }
            PatKind::Ident(_) => {}
        }
    }

    pub(super) fn prepare_omitted_array_destructure_exports(
        &mut self,
        stmts: &[Stmt],
    ) -> Vec<String> {
        self.omitted_array_destructure_plans.clear();
        self.omitted_array_destructure_counter = 0;
        let mut temps: Vec<String> = Vec::new();

        let mut handle_var_stmt = |this: &mut Emitter<'a>, var_stmt: &VarStmt| {
            for decl in &var_stmt.declarations {
                if decl.init.is_none() {
                    continue;
                }
                let mut names = Vec::new();
                collect_binding_names(&decl.name, &mut names);
                if !names.is_empty() {
                    continue;
                }
                if !matches!(decl.name.kind, PatKind::Array(_)) {
                    continue;
                }

                let root_temp = this.next_omitted_array_temp();
                let mut chain: Vec<(String, String, usize)> = Vec::new();
                this.collect_omitted_array_chain(&decl.name, &root_temp, &mut chain);
                if chain.is_empty() {
                    continue;
                }
                let plan = OmittedArrayDestructurePlan { root_temp, chain };
                temps.extend(plan.all_temps());
                this.omitted_array_destructure_plans
                    .insert(decl.span.start, plan);
            }
        };

        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Export(export_decl) => {
                    if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                        if let StmtKind::Var(var_stmt) = &inner.kind {
                            handle_var_stmt(self, var_stmt);
                        }
                    }
                }
                StmtKind::Var(var_stmt) if var_stmt.modifiers & MOD_EXPORT != 0 => {
                    handle_var_stmt(self, var_stmt);
                }
                _ => {}
            }
        }

        // Preserve first-seen order while removing duplicates.
        let mut seen = HashSet::new();
        temps.retain(|t| seen.insert(t.clone()));
        temps
    }

    /// Emit a var statement with export assignment using the current `export_target`.
    /// For `export const x = 1;` → `target.x = 1;` (CJS: `exports.x = 1;`)
    /// For `export const { a, b } = expr;` → `const { a, b } = expr; target.a = a;`
    pub(super) fn emit_var_export(&mut self, var_stmt: &VarStmt) {
        if var_stmt.modifiers & MOD_DECLARE != 0 {
            return;
        }
        let target = match self.export_target.clone() {
            Some(t) => t,
            None if self.is_cjs_like() => "exports".to_string(),
            None => {
                self.emit_var_stmt(var_stmt);
                return;
            }
        };
        // For CJS top-level with export=, just emit the var without assignment.
        // Skip entirely when all declarators lack initializers — the preamble
        // `exports.x = void 0;` already covers those.
        if target == "exports" && self.has_export_assign {
            if var_stmt.declarations.iter().any(|d| d.init.is_some()) {
                self.emit_var_stmt(var_stmt);
            }
            return;
        }
        // For namespace targets, combine simple declarators into comma-separated
        // assignments: `M.c1 = 0, M.c2 = "";`
        if target != "exports" {
            // Collect simple (single-name + initializer) declarators
            let mut first = true;
            for decl in &var_stmt.declarations {
                let mut names = Vec::new();
                collect_binding_names(&decl.name, &mut names);
                if let Some(ref init) = decl.init {
                    if names.len() == 1 {
                        if !first {
                            self.write(", ");
                        }
                        first = false;
                        self.write(&target);
                        self.write(".");
                        self.write(&names[0]);
                        self.write(" = ");
                        self.emit_expr(init);
                        match single_destructure_access(&decl.name) {
                            DestructureAccess::Member(ref prop) => {
                                if self.member_object_needs_double_dot(init) {
                                    self.write("..");
                                } else {
                                    self.write(".");
                                }
                                self.write(prop);
                            }
                            DestructureAccess::Index(idx) => {
                                self.write("[");
                                self.write(&idx.to_string());
                                self.write("]");
                            }
                            DestructureAccess::None => {}
                        }
                    } else {
                        // Destructuring in namespace: use temp variable.
                        // TypeScript emits:
                        //   var _a;
                        //   _a = RHS, NS.a = _a[0], NS.b = _a[1];  (array)
                        //   _a = RHS, NS.a = _a.a, NS.b = _a.b;    (object)
                        if !first {
                            self.writeln(";");
                            first = true;
                        }
                        let tmp = self.next_inline_temp_var();
                        // Build (name, access) pairs from the pattern.
                        let mut accesses: Vec<(String, String)> = Vec::new();
                        collect_destructure_accesses(&decl.name, &tmp, &mut accesses);
                        // Register the temp var for hoisting to the top of the
                        // enclosing IIFE/scope. The namespace emit code inserts
                        // `var _a, _b;` at the saved insertion point.
                        self.ns_temp_var_names.push(tmp.clone());
                        // Emit: _a = RHS, Target.name1 = _a.access1, ...
                        self.write(&tmp);
                        self.write(" = ");
                        self.emit_expr(init);
                        for (name, access) in &accesses {
                            self.write(", ");
                            self.write(&target);
                            self.write(".");
                            self.write(name);
                            self.write(" = ");
                            if access.starts_with("__rest(") {
                                let prefix = self.helper_prefix().to_string();
                                self.write(&prefix);
                            }
                            self.write(access);
                        }
                        self.writeln(";");
                    }
                }
            }
            if !first {
                self.writeln(";");
            }
            return;
        }

        // CJS exports case.
        // TypeScript emits `exports.name = init;` for simple initializers,
        // but `var name = init; exports.name = name;` for function/class expressions
        // (because the function body may reference `name` before the export).
        for decl in &var_stmt.declarations {
            let mut names = Vec::new();
            collect_binding_names(&decl.name, &mut names);
            if let Some(ref init) = decl.init {
                let is_destructuring = !matches!(decl.name.kind, PatKind::Ident(_));
                // Multi-property or empty destructuring needs a temp var.
                // Single-property destructuring is handled by the
                // `else if !names.is_empty()` branch below which emits
                // `exports.name = RHS.prop;` directly without a temp.
                let is_multi_destructuring = is_destructuring && names.len() > 1;
                // For CJS exports, anonymous class expressions with static
                // initializers produce an IIFE comma expression that TSC
                // directly assigns to `exports.X` (no local variable).
                // Unwrap parens/type assertions to find the inner expression.
                let inner_init = {
                    let mut e = init.as_ref();
                    loop {
                        match &e.kind {
                            ExprKind::Paren(inner) | ExprKind::NonNull(inner) => e = inner,
                            ExprKind::As(a) => e = &a.expr,
                            ExprKind::Satisfies(s) => e = &s.expr,
                            ExprKind::TypeAssertion(ta) => e = &ta.expr,
                            _ => break,
                        }
                    }
                    e
                };
                let init_needs_local = match &inner_init.kind {
                    ExprKind::ClassExpr(cd)
                        if cd.name.is_none() && class_has_static_initializers(cd) =>
                    {
                        false
                    }
                    // Standard-decorated class expression → IIFE wrapper;
                    // the IIFE result can be directly assigned to exports.X.
                    ExprKind::ClassExpr(cd)
                        if !cd.decorators.is_empty()
                            && !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_expr_can_emit_simple_standard_decorator_wrapper(cd) =>
                    {
                        false
                    }
                    _ => expr_needs_local_var(init),
                };
                let needs_local = !is_destructuring && (names.len() > 1 || init_needs_local);
                if is_multi_destructuring {
                    // CJS multi-prop destructuring: use temp variable at file level.
                    // TypeScript emits: _a = RHS, exports.name = _a.prop, ...;
                    let tmp = self.next_temp_var();
                    let mut accesses: Vec<(String, String)> = Vec::new();
                    collect_destructure_accesses(&decl.name, &tmp, &mut accesses);
                    self.write(&tmp);
                    self.write(" = ");
                    self.emit_expr(init);
                    for (name, access) in &accesses {
                        self.write(", ");
                        self.write(&target);
                        self.write(".");
                        self.write(name);
                        self.write(" = ");
                        // Add helper prefix for __rest calls when using tslib
                        if access.starts_with("__rest(") {
                            let prefix = self.helper_prefix().to_string();
                            self.write(&prefix);
                        }
                        self.write(access);
                    }
                    self.writeln(";");
                } else if needs_local {
                    let kw = self.emitted_var_keyword(var_stmt);
                    self.write(kw);
                    self.write(" ");
                    self.emit_binding_name(&decl.name);
                    self.write(" = ");
                    // Set binding name for anonymous class expressions
                    // so __setFunctionName can be emitted in the IIFE.
                    if matches!(&inner_init.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                        if let PatKind::Ident(ref ident) = decl.name.kind {
                            self.class_expr_binding_name =
                                Some(ClassExprBindingName::Literal(ident.to_string()));
                        }
                    }
                    self.emit_expr(init);
                    self.class_expr_binding_name = None;
                    self.strip_trailing_newline();
                    if self.output.ends_with(';') {
                        self.newline();
                    } else {
                        self.writeln(";");
                    }
                    for name in &names {
                        self.write(&target);
                        self.write(".");
                        self.write(name);
                        self.write(" = ");
                        self.write(name);
                        self.writeln(";");
                    }
                } else if !names.is_empty() {
                    if is_destructuring {
                        if let Some(binding_span) = first_binding_span(&decl.name) {
                            self.emit_leading_comments(binding_span.start);
                        }
                    }
                    // Check if this is a single-name destructuring pattern.
                    // For `export let { toString } = 1;`, we need to emit
                    // `exports.toString = 1..toString;` (property access on init).
                    // For `export let [bar] = [1];`, we emit
                    // `exports.bar = [1][0];` (element access on init).
                    let access = single_destructure_access(&decl.name);
                    // Simple case: exports.name = init;
                    self.write(&target);
                    self.write(".");
                    self.write(&names[0]);
                    self.write(" = ");
                    if target == "exports" {
                        if let ExprKind::Ident(ref init_name) = init.kind {
                            if self.cjs_var_export_names.contains(init_name.as_str()) {
                                self.write("exports.");
                                self.write(init_name);
                                match &access {
                                    DestructureAccess::Member(ref prop) => {
                                        self.write(".");
                                        self.write(prop);
                                    }
                                    DestructureAccess::Index(idx) => {
                                        self.write("[");
                                        self.write(&idx.to_string());
                                        self.write("]");
                                    }
                                    DestructureAccess::None => {}
                                }
                                self.writeln(";");
                                continue;
                            }
                        }
                    }
                    // Set binding name for anonymous class expressions
                    // so __setFunctionName can be emitted in the IIFE.
                    if matches!(&inner_init.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                        if let PatKind::Ident(ref ident) = decl.name.kind {
                            self.class_expr_binding_name =
                                Some(ClassExprBindingName::Literal(ident.to_string()));
                        }
                    }
                    self.emit_expr(init);
                    self.class_expr_binding_name = None;
                    match &access {
                        DestructureAccess::Member(ref prop) => {
                            // Emit property access for destructured property.
                            // For numeric literals like `1`, use `..` to avoid
                            // ambiguity with decimal points: `1..toString`.
                            if self.member_object_needs_double_dot(init) {
                                self.write("..");
                            } else {
                                self.write(".");
                            }
                            self.write(prop);
                        }
                        DestructureAccess::Index(idx) => {
                            self.write("[");
                            self.write(&idx.to_string());
                            self.write("]");
                        }
                        DestructureAccess::None => {}
                    }
                    self.strip_trailing_newline();
                    if self.output.ends_with(';') {
                        self.newline();
                    } else {
                        self.writeln(";");
                    }
                } else {
                    // No names (e.g. omitted destructuring). If we pre-scanned an
                    // omitted-array plan for this declaration, emit runtime element
                    // accesses that preserve destructuring side effects.
                    if let Some(plan) = self
                        .omitted_array_destructure_plans
                        .get(&decl.span.start)
                        .cloned()
                    {
                        self.write(&plan.root_temp);
                        self.write(" = ");
                        self.emit_expr(init);
                        for (child, parent, index) in &plan.chain {
                            self.write(", ");
                            self.write(child);
                            self.write(" = ");
                            self.write(parent);
                            self.write("[");
                            self.write(&index.to_string());
                            self.write("]");
                        }
                        self.writeln(";");
                    } else if is_destructuring {
                        // Empty destructuring pattern (e.g. `export const [] = [];`
                        // or `export const {} = {};`). TypeScript still creates a temp
                        // var to preserve the RHS evaluation side effects.
                        let tmp = self.next_temp_var();
                        self.write(&tmp);
                        self.write(" = ");
                        self.emit_expr(init);
                        self.writeln(";");
                    } else {
                        self.emit_expr(init);
                        self.strip_trailing_newline();
                        if self.output.ends_with(';') {
                            self.newline();
                        } else {
                            self.writeln(";");
                        }
                    }
                }
            }
            // Declarations without initializer are already handled by the
            // pre-declaration chain (exports.x = void 0;) at the top of the file.
        }
    }

    /// Emit the exact ES5 export shape for the narrow top-level shorthand plus
    /// object-rest bridge. `target = None` selects ESM's `export var` spelling;
    /// CJS and AMD pass their `exports` target. The RHS is embedded in the first
    /// property access so its hygienic temp is initialized exactly once before
    /// the rest helper observes the same object.
    pub(super) fn emit_direct_es5_object_rest_export(
        &mut self,
        target: Option<&str>,
        init: &Expr,
        binding_name: &str,
        binding_span: Span,
        rest_name: &str,
        rest_span: Span,
    ) {
        let temp_name = self.next_temp_var();
        match target {
            Some(target) => {
                self.record_mapping_with_name(binding_span, binding_name);
                self.write_cjs_export_access(target, binding_name);
            }
            None => {
                self.write("export var ");
                self.record_mapping_with_name(binding_span, binding_name);
                self.write(binding_name);
            }
        }
        self.write(" = (");
        self.write(&temp_name);
        self.write(" = ");
        self.emit_expr(init);
        self.write(", ");
        self.write(&temp_name);
        self.write(").");
        self.write(binding_name);
        self.write(", ");
        self.record_mapping_with_name(rest_span, rest_name);
        match target {
            Some(target) => self.write_cjs_export_access(target, rest_name),
            None => self.write(rest_name),
        }
        self.write(" = ");
        self.write(self.helper_prefix());
        self.write("__rest(");
        self.write(&temp_name);
        self.write(", [\"");
        self.write(&super::emit_expr::escape_js_string_for_quote(
            binding_name,
            '"',
        ));
        self.writeln("\"]);");
    }
}
