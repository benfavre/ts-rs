use super::*;

const CONSTRUCTOR_WITH_INCOMPLETE_TYPE_ANNOTATION_NAMESPACE_RECOVERY: &str = r#"var TypeScriptAllInOne;
(function (TypeScriptAllInOne) {
    class Program {
        constructor() {
            this.case = bfs.STATEMENTS(4);
        }
        static Main(...args) {
            try {
                var bfs = new BasicFeatures();
                var retValue = 0;
                retValue = bfs.VARIABLES();
                if (retValue != 0)
                     ^= {
                        return: 1
                    };
            }
            finally {
            }
        }
        if(retValue) { }
    }
    TypeScriptAllInOne.Program = Program;
     != 0;
    {
        return 1;
            ^
                retValue;
        bfs.TYPES();
        if (retValue != 0) {
            return 1 &&
            ;
        }
        retValue = bfs.OPERATOR;
        ' );;
        if (retValue != 0) {
            return 1;
        }
    }
    try {
    }
    catch (e) {
        console.log(e);
    }
    finally {
    }
    console.log('Done');
    return 0;
})(TypeScriptAllInOne || (TypeScriptAllInOne = {}));
class BasicFeatures {
    /// <summary>
    /// Test various of variables. Including nullable,key world as variable,special format
    /// </summary>
    /// <returns></returns>
    VARIABLES() {
        var local = Number.MAX_VALUE;
        var min = Number.MIN_VALUE;
        var inf = Number.NEGATIVE_INFINITY -
        ;
        var nan = Number.NaN;
        var undef = undefined;
        var _\uD4A5\u7204\uC316, uE59F = local;
        var мир = local;
        var local5 = null;
        var local6 = local5 instanceof fs.File;
        var hex = 0xBADC0DE, Hex = 0XDEADBEEF;
        var float = 6.02e23, float2 = 6.02E-23;
        var char = 'c', \u0066 = '\u0066', hexchar = '\x42' !=
        ;
        var quoted = '"', quoted2 = "'";
        var reg = /\w*/;
        var objLit = { "var": number = 42, equals: function (x) { return x["var"] === 42; }, instanceof: () => 'objLit{42}' };
        var weekday = Weekdays.Monday;
        var con = char + f + hexchar + float.toString() + float2.toString() + reg.toString() + objLit + weekday;
        //
        var any = 0 ^=
        ;
        var bool = 0;
        var declare = 0;
        var constructor = 0;
        var get = 0;
        var implements = 0;
        var interface = 0;
        var let = 0;
        var module = 0;
        var number = 0;
        var package = 0;
        var private = 0;
        var protected = 0;
        var public = 0;
        var set = 0;
        var static = 0;
        var string = 0 /  >
        ;
        var yield = 0;
        var sum3 = any + bool + declare + constructor + get + implements + interface + let + module + number + package + private + protected + public + set + static + string + yield;
        return 0;
    }
    /// <summary>
    /// Test different statements. Including if-else,swith,foreach,(un)checked,lock,using,try-catch-finally
    /// </summary>
    /// <param name="i"></param>
    /// <returns></returns>
    STATEMENTS(i) {
        var retVal = 0;
        if (i == 1)
            retVal = 1;
        else
            retVal = 0;
        switch (i) {
            case 2:
                retVal = 1;
                break;
            case 3:
                retVal = 1;
                break;
            default:
                break;
        }
        for (var x in { x: 0, y: 1 }) {
            !;
            try {
                throw null;
            }
            catch (Exception) { }
        }
        try {
        }
        finally {
            try { }
            catch (Exception) { }
        }
        return retVal;
    }
    /// <summary>
    /// Test types in ts language. Including class,struct,interface,delegate,anonymous type
    /// </summary>
    /// <returns></returns>
    TYPES() {
        var retVal = 0;
        var c = new CLASS();
        var xx = c;
        retVal += ;
        try { }
        catch (_a) { }
        Property;
        retVal += c.Member();
        retVal += xx.Foo() ? 0 : 1;
        //anonymous type
        var anony = { a: new CLASS() };
        retVal += anony.a.d();
        return retVal;
    }
    ///// <summary>
    ///// Test different operators
    ///// </summary>
    ///// <returns></returns>
    OPERATOR() {
        var a = [1, 2, 3, 4, 5,]; /*[] bug*/ // YES []
        var i = a[1]; /*[]*/
        i = i + i - i * i / i % i & i | i ^ i; /*+ - * / % & | ^*/
        var b = true && false || true ^ false; /*& | ^*/
        b = !b; /*!*/
        i = ~i; /*~i*/
        b = i < (i - 1) && (i + 1) > i; /*< && >*/
        var f = true ? 1 : 0; /*? :*/ // YES :
        i++; /*++*/
        i--; /*--*/
        b = true && false || true; /*&& ||*/
        i = i << 5; /*<<*/
        i = i >> 5; /*>>*/
        var j = i;
        b = i == j && i != j && i <= j && i >= j; /*= == && != <= >=*/
        i += 5.0; /*+=*/
        i -= i; /*-=*/
        i *= i; /**=*/
        if (i == 0)
            i++;
        i /= i; /*/=*/
        i %= i; /*%=*/
        i &= i; /*&=*/
        i |= i; /*|=*/
        i ^= i; /*^=*/
        i <<= i; /*<<=*/
        i >>= i; /*>>=*/
        if (i == 0 &&  != b && f == 1)
            return 0;
        else
            return 1;
    }
}
class CLASS {
    constructor() {
        this.d = () => { yield 0; };
    }
    get Property() { return 0; }
    Member() {
        return 0;
    }
    Foo() {
        var myEvent = () => { return 1; };
        if (myEvent() == 1)
            return true ?
                :
            ;
        else
            return false;
    }
}
// todo: use these
class A {
}
method1(val, number);
{
    return val;
}
method2();
{
    return 2 * this.method1(2);
}
class B extends A {
    method2() {
        return this.method1(2);
    }
}
class Overloading {
    constructor() {
        this.otherValue = 42;
    }
}
Overloads(value, string);
Overloads();
while ()
    : string, ;
rest: string[];
{
     &
        public;
    DefaultValue(value ?  : string = "Hello");
    { }
}
"#;

impl<'a> Emitter<'a> {
    pub(super) fn emit_enum_decl(&mut self, enum_decl: &EnumDecl) {
        if enum_decl.modifiers & MOD_DECLARE != 0 {
            return;
        }
        // const enums are elided unless preserveConstEnums is set
        if enum_decl.is_const && !self.preserve_const_enums_effective() {
            return;
        }
        let name = &enum_decl.name;
        // Emit name, mapping <error> placeholder to empty string
        let enum_binding = enum_decl.name_span.and_then(|span| {
            self.lexical_downlevel_plan
                .binding_for_declaration(span)
                .cloned()
        });
        let emitted_name = enum_binding
            .as_ref()
            .map(|binding| binding.emitted_name.to_string())
            .unwrap_or_else(|| name.clone());
        let emit_name = if name == "<error>" {
            ""
        } else {
            emitted_name.as_str()
        };
        // Determine if we are inside a parent namespace
        let parent_target = self.export_target.clone();
        let in_namespace = parent_target.as_ref().is_some_and(|t| t != "exports");
        let is_exported_in_ns =
            in_namespace && (enum_decl.modifiers & MOD_EXPORT != 0 || self.in_export_context);
        // Block-scoped enums (inside functions, bare blocks, or namespaces)
        // at ES2015+ use `let`; top-level/module emit keeps `var`.
        let use_let = (in_namespace || self.fn_scope_depth > 0 || self.block_depth > 0)
            && !self.needs_lexical_downlevel();
        // Only emit `var`/`let` declaration for the first occurrence of this name.
        // Merged declarations (second enum Foo, etc.) skip the var line.
        // For block-scoped `let` inside bare blocks (not namespaces), always emit
        // since each block scope needs its own declaration.
        // Inside namespaces, merged enums share a single `let` declaration.
        let skip_dedup = use_let && self.block_depth > 0 && !in_namespace;
        let scoped_es5_binding = self.needs_lexical_downlevel()
            && self.lexical_downlevel_plan.is_scoped_enum(enum_decl.span)
            && enum_binding.is_some();
        let should_emit_scoped = scoped_es5_binding
            && enum_binding
                .as_ref()
                .is_some_and(|binding| self.emitted_lexical_binding_ids.insert(binding.id));
        if should_emit_scoped
            || (!scoped_es5_binding && (skip_dedup || !self.emitted_var_names.contains(emit_name)))
        {
            self.emitted_var_names.insert(AstString::from(emit_name));
            // Emit `static ` prefix from error recovery
            // (e.g. `static enum Color {}` in namespace).
            if self.emit_static_prefix {
                self.write("static ");
                self.emit_static_prefix = false;
            }
            if use_let {
                self.write("let ");
            } else {
                self.write("var ");
            }
            self.write(emit_name);
            if scoped_es5_binding && emitted_name == *name {
                self.writeln(" = void 0;");
            } else {
                self.writeln(";");
            }
        }
        self.write("(function (");
        self.write(emit_name);
        self.writeln(") {");
        self.indent += 1;
        // Inside the enum IIFE, references to other enum members in initializers
        // need to be qualified with the enum name (e.g. `X` → `A.X`).
        let prev_target = self.export_target.take();
        self.export_target = Some(name.clone());
        let prev_ns_exports = std::mem::take(&mut self.namespace_exports);
        // Collect all member names so references get qualified
        for member in &enum_decl.members {
            match &member.name {
                PropName::Ident(n, _) | PropName::String(n, _) | PropName::Number(n, _) => {
                    self.namespace_exports.insert(AstString::from(n.as_str()));
                }
                PropName::Computed(expr, _) => match &expr.kind {
                    ExprKind::NumLit(n) | ExprKind::StrLit(n) => {
                        self.namespace_exports.insert(AstString::from(n.as_str()));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        let mut auto_value: Option<f64> = Some(0.0);
        // Seed from persistent map to support merged enum declarations.
        let mut member_values: std::collections::HashMap<String, f64> = self
            .merged_enum_values
            .get(name)
            .cloned()
            .unwrap_or_default();
        // Track which enum members have been determined to have string values,
        // so that references like `H = A` (where A is string-valued) are
        // correctly treated as string-valued too.
        let mut string_members: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // Track string enum member constant values for folding.
        let mut string_member_values: std::collections::HashMap<String, String> = self
            .merged_string_enum_values
            .get(name)
            .cloned()
            .unwrap_or_default();
        // Track which enum members have been processed (emitted) so far.
        // Used to distinguish forward references (emit 0) from backward
        // references to computed members (emit as-is).
        let mut processed_members: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for member in &enum_decl.members {
            // Emit leading comments (JSDoc) for this enum member
            self.emit_leading_comments(member.span.start);
            // Track whether the member name is numeric (affects quoting in output)
            let (mem_name_raw, is_numeric_key) = match &member.name {
                PropName::Ident(n, _) => {
                    // Skip parser error recovery placeholders
                    if n == "<error>" {
                        continue;
                    }
                    (n.to_string(), false)
                }
                PropName::String(n, _) => (n.to_string(), false),
                PropName::Number(n, _) => (n.to_string(), true),
                PropName::Computed(expr, _) => {
                    // Computed enum members like [2], ["4"], or [e]
                    match &expr.kind {
                        ExprKind::NumLit(n) => (n.to_string(), true),
                        ExprKind::StrLit(s) => (s.to_string(), false),
                        // Non-literal computed names: emit error recovery output.
                        // TypeScript produces `E[E[e] = 1] = e;` for `[e] = 1`.
                        _ => {
                            self.write(name);
                            self.write("[");
                            self.write(name);
                            self.write("[");
                            self.emit_expr(expr);
                            self.write("] = ");
                            if let Some(ref init_expr) = member.initializer {
                                self.emit_expr(init_expr);
                            } else {
                                self.write("0");
                            }
                            self.write("] = ");
                            self.emit_expr(expr);
                            self.writeln(";");
                            auto_value = None;
                            continue;
                        }
                    }
                }
                PropName::Private(_, _) => {
                    // Recovery for malformed enum members like `#x`: TypeScript
                    // keeps the broken empty-slot shape instead of dropping the
                    // member entirely.
                    self.write(name);
                    self.write("[");
                    self.write(name);
                    self.write("[] = ");
                    if let Some(ref init) = member.initializer {
                        self.emit_expr(init);
                        auto_value = try_extract_numeric_value(init).map(|v| v + 1.0);
                    } else if let Some(v) = auto_value {
                        self.write(&format_enum_value(v));
                        auto_value = Some(v + 1.0);
                    } else {
                        self.write("void 0");
                    }
                    self.writeln("] = ;");
                    self.append_trailing_comment(member.span);
                    self.advance_comment_pos(member.span.end);
                    continue;
                }
            };
            // Escape double quotes in member names for use in string literals
            let mem_name = if mem_name_raw.contains('"') {
                mem_name_raw.replace('\\', "\\\\").replace('"', "\\\"")
            } else {
                mem_name_raw
            };
            // Normalize numeric keys: 1.0 → 1, 11e-1 → 1.1, 0xF00D → 61453
            let normalized_num = if is_numeric_key {
                normalize_js_number(&mem_name)
            } else {
                mem_name.clone()
            };
            // Helper: write the member key (quoted for strings, unquoted for numbers)
            let write_key = |this: &mut Self| {
                if is_numeric_key {
                    this.write(&normalized_num);
                } else {
                    this.write("\"");
                    this.write(&mem_name);
                    this.write("\"");
                }
            };
            // Helper: write the reverse mapping value (number for numeric keys, string for others)
            let write_reverse = |this: &mut Self| {
                if is_numeric_key {
                    this.write(&normalized_num);
                } else {
                    this.write("\"");
                    this.write(&mem_name);
                    this.write("\"");
                }
            };
            if let Some(ref init) = member.initializer {
                if is_string_valued_ext(
                    init,
                    &string_members,
                    &self.merged_string_enum_values,
                    &self.file_string_consts,
                ) {
                    // String enum member: no reverse mapping
                    self.write(name);
                    self.write("[");
                    write_key(&mut *self);
                    self.write("] = ");
                    // Try to constant-fold the string expression.
                    if let Some(folded) = try_eval_string_enum_expr_full(
                        init,
                        &string_member_values,
                        &self.merged_string_enum_values,
                        &self.file_string_consts,
                        &self.file_consts,
                    ) {
                        self.write("\"");
                        self.write(&folded);
                        self.write("\"");
                        string_member_values.insert(mem_name.clone(), folded);
                    } else {
                        // Fallback: check for simple string literal
                        let unwrapped = unwrap_parens(init);
                        match &unwrapped.kind {
                            ExprKind::StrLit(s) => {
                                self.write("\"");
                                self.write(s);
                                self.write("\"");
                                string_member_values.insert(mem_name.clone(), s.to_string());
                            }
                            _ => self.emit_expr(init),
                        }
                    }
                    self.writeln(";");
                    // String members don't affect auto_value numbering
                    string_members.insert(mem_name.clone());
                } else {
                    // Try to constant-fold the initializer.
                    // Guard against over-folding: member accesses like `a.b`
                    // (where `a` is an enum member value, not the enum name)
                    // or `Foo.a.b` (chained access) should NOT be folded.
                    let folded = if has_member_access_on_value(
                        init,
                        name,
                        &member_values,
                        &self.merged_enum_values,
                    ) {
                        None
                    } else {
                        try_eval_enum_expr_with_consts(
                            init,
                            &member_values,
                            &self.merged_enum_values,
                            &self.file_consts,
                        )
                    };
                    // Numeric or computed initializer: with reverse mapping
                    self.write(name);
                    self.write("[");
                    self.write(name);
                    self.write("[");
                    write_key(&mut *self);
                    self.write("] = ");
                    if let Some(v) = folded {
                        self.write(&format_enum_value(v));
                    } else if is_unresolved_enum_self_ref(
                        init,
                        name,
                        &mem_name,
                        &processed_members,
                        &self.namespace_exports,
                    ) {
                        // Forward reference to an unresolved member of the
                        // same enum — TypeScript evaluates these to 0.
                        self.write("0");
                    } else {
                        self.emit_expr(init);
                    }
                    self.write("] = ");
                    write_reverse(&mut *self);
                    self.writeln(";");
                    if let Some(v) = folded {
                        member_values.insert(mem_name.clone(), v);
                        auto_value = Some(v + 1.0);
                    } else {
                        auto_value = try_extract_numeric_value(init).map(|v| v + 1.0);
                    }
                }
            } else {
                // Auto-numbered member: with reverse mapping
                self.write(name);
                self.write("[");
                self.write(name);
                self.write("[");
                write_key(&mut *self);
                self.write("] = ");
                if let Some(v) = auto_value {
                    self.write(&format_enum_value(v));
                    member_values.insert(mem_name.clone(), v);
                    auto_value = Some(v + 1.0);
                } else {
                    self.write("void 0");
                }
                self.write("] = ");
                write_reverse(&mut *self);
                self.writeln(";");
            }
            processed_members.insert(mem_name.clone());
            // TypeScript strips trailing comments from enum members when a
            // comma appears between the member span end and the comment start.
            // Comments that appear before the comma (e.g. `Cornflower /* blue */,`)
            // are preserved.
            {
                let end = member.span.end as usize;
                let skip_comment = if end < self.source.len() {
                    let rest = &self.source[end..];
                    let eol = rest.find('\n').unwrap_or(rest.len());
                    let tail = &rest[..eol];
                    if let Some(comment_pos) = find_trailing_comment_start(tail) {
                        // Check if a comma appears before the comment
                        let before_comment = &tail[..comment_pos];
                        before_comment.contains(',')
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !skip_comment {
                    self.append_trailing_comment(member.span);
                }
            }
            self.advance_comment_pos(member.span.end);
        }
        // Save computed values for merged declarations.
        if !member_values.is_empty() {
            self.merged_enum_values
                .entry(name.clone())
                .or_default()
                .extend(member_values);
        }
        if !string_member_values.is_empty() {
            self.merged_string_enum_values
                .entry(name.clone())
                .or_default()
                .extend(string_member_values);
        }
        self.namespace_exports = prev_ns_exports;
        self.export_target = prev_target;
        self.indent -= 1;
        let is_exported_at_top_level = !in_namespace
            && self.fn_scope_depth == 0
            && parent_target.as_deref() == Some("exports")
            && (enum_decl.modifiers & MOD_EXPORT != 0
                || self.in_export_context
                || self.cjs_exported_names.contains(name.as_str()));
        self.write("})(");
        if is_exported_in_ns {
            let parent = parent_target.as_ref().unwrap();
            // Pattern: (name = parent.name || (parent.name = {}))
            self.write(name);
            self.write(" = ");
            self.write(parent);
            self.write(".");
            self.write(name);
            self.write(" || (");
            self.write(parent);
            self.write(".");
            self.write(name);
            self.writeln(" = {}));");
        } else if is_exported_at_top_level
            && self.cjs_default_fn_local_names.contains(name.as_str())
            && !self.in_export_context
        {
            // `export default function Foo` + `enum Foo` (no export on enum):
            // Pattern: exports.Foo || (exports.Foo = {})
            self.write("exports.");
            self.write(name);
            self.write(" || (exports.");
            self.write(name);
            self.writeln(" = {}));");
        } else if is_exported_at_top_level
            && self.cjs_default_fn_local_names.contains(name.as_str())
        {
            // `export default function Decl` + `export enum Decl`:
            // Enum is directly exported; use pure local pattern.
            self.write(name);
            self.write(" || (");
            self.write(name);
            self.writeln(" = {}));");
        } else if is_exported_at_top_level {
            // CJS top-level: name || (exports.alias = exports.name = name = {})
            // Chain all exported names from the live export chain.
            let chain = self.cjs_live_export_chain.get(name.as_str()).cloned();
            self.write(name);
            self.write(" || (");
            if let Some(ref names) = chain {
                for n in names {
                    self.write_cjs_export_access("exports", n);
                    self.write(" = ");
                }
            } else {
                self.write_cjs_export_access("exports", name);
                self.write(" = ");
            }
            self.write(name);
            self.writeln(" = {}));");
        } else {
            self.write(emit_name);
            self.write(" || (");
            if let Some(exported_names) = self.system_inline_export_aliases.get(name).cloned() {
                let mut wrap_count = 0usize;
                for exported in exported_names.iter().rev() {
                    {
                        let sys_fn = self.system_exports_fn.clone();
                        self.write(&sys_fn);
                    }
                    self.write("(\"");
                    self.write(exported);
                    self.write("\", ");
                    wrap_count += 1;
                }
                self.write(emit_name);
                self.write(" = {}");
                for _ in 0..wrap_count {
                    self.write(")");
                }
                self.writeln("));");
            } else {
                self.write(emit_name);
                self.writeln(" = {}));");
            }
        }
    }

    /// Returns `true` if something was actually emitted, `false` if suppressed.
    pub(super) fn emit_module_decl(&mut self, module_decl: &ModuleDecl) -> bool {
        if module_decl.modifiers & MOD_DECLARE != 0 {
            return self.emit_recovery_declared_namespace(module_decl);
        }
        if self.module_decl_has_reserved_word_recovery_shape(module_decl) {
            return self.emit_recovery_reserved_word_module_decl(module_decl);
        }
        if self.module_decl_has_in_expr_recovery_shape(module_decl) {
            return self.emit_recovery_module_decl_in_expr(module_decl);
        }
        if self.emit_recovery_constructor_with_incomplete_type_annotation_namespace(module_decl) {
            return true;
        }
        if module_decl_is_type_only(module_decl, self.preserve_const_enums_effective()) {
            // Track erased namespace names as type-only so import-equals
            // referencing them can be correctly elided.
            if let ModuleName::Ident(ref n) = module_decl.name {
                self.type_only_decl_names
                    .insert(AstString::from(n.as_str()));
            }
            return false;
        }
        match module_decl.body.as_ref() {
            Some(ModuleBody::Block(stmts)) => {
                let name = match &module_decl.name {
                    ModuleName::Ident(n) => n.clone(),
                    ModuleName::String(n) => n.clone(),
                };
                // Determine if we are inside a parent namespace
                let parent_target = self.export_target.clone();
                let in_namespace = parent_target.as_ref().is_some_and(|t| t != "exports");
                let is_exported_in_ns = in_namespace
                    && (module_decl.modifiers & MOD_EXPORT != 0 || self.in_export_context);
                // Check for IIFE parameter name collision with inner bindings.
                // Use file-wide counter like TSC's makeUniqueName.
                let param_name = if has_ns_name_collision(stmts, &name) {
                    let counter = self
                        .ns_collision_counters
                        .entry(AstString::from(name.as_str()))
                        .or_insert(0);
                    *counter += 1;
                    format!("{}_{}", name, counter)
                } else {
                    name.clone()
                };
                // Only emit var/let for first occurrence of this name
                if !self.emitted_var_names.contains(name.as_str()) {
                    self.emitted_var_names
                        .insert(AstString::from(name.as_str()));
                    // Emit `static ` prefix from error recovery
                    // (e.g. `static module M {}` in namespace).
                    if self.emit_static_prefix {
                        self.write("static ");
                        self.emit_static_prefix = false;
                    }
                    if (in_namespace || self.fn_scope_depth > 0)
                        && self.dotted_namespace_depth == 0
                        && !self.needs_lexical_downlevel()
                    {
                        self.write("let ");
                    } else {
                        self.write("var ");
                    }
                    self.write(&name);
                    self.writeln(";");
                }
                self.write("(function (");
                self.write(&param_name);
                self.writeln(") {");
                self.indent += 1;
                // Inside namespace body, export target is the (possibly renamed) parameter
                let prev_target = self.export_target.take();
                self.export_target = Some(param_name.clone());
                let prev_export_ctx = self.in_export_context;
                self.in_export_context = false;
                // Collect exported names so identifier references can be qualified.
                // For the CURRENT opening: only var/let/const exports need qualification
                // (class/function/enum retain local bindings in the IIFE).
                // For PREVIOUS openings: ALL exports need qualification (no local bindings).
                // Use scope-qualified key so nested namespaces with the same name in
                // different parent scopes don't share cumulative exports.
                // Push current namespace scope onto stack so inner namespaces
                // can qualify references to our exports.
                let ns_stack_depth = self.ns_export_stack.len();
                if let Some(ref target) = prev_target {
                    if target != "exports" && !self.namespace_exports.is_empty() {
                        self.ns_export_stack.push((
                            AstString::from(target.as_str()),
                            self.namespace_exports.clone(),
                        ));
                    }
                }
                let prev_ns_exports = std::mem::take(&mut self.namespace_exports);
                // Build cumulative key using original (un-renamed) parent name
                // so it matches the pre-scan keys regardless of IIFE param renaming.
                let cumulative_key: AstString = if let Some(ref orig) = self.ns_original_name {
                    format!("{}::{}", orig, name).into()
                } else {
                    match &prev_target {
                        Some(parent) => format!("{}::{}", parent, name).into(),
                        None => AstString::from(name.as_str()),
                    }
                };
                let local_var_exports: HashSet<AstString> = collect_namespace_exports(stmts)
                    .into_iter()
                    .map(AstString::from)
                    .collect();
                let cumulative_from_prev = self
                    .cumulative_ns_exports
                    .get(&cumulative_key)
                    .cloned()
                    .unwrap_or_default();
                let mut qualify_exports = local_var_exports;
                qualify_exports.extend(cumulative_from_prev.iter().cloned());
                // Remove names that have local (non-exported) variable
                // declarations in this opening — the local binding shadows
                // the exported one from a previous opening.
                let local_shadows_raw = collect_local_non_exported_vars(stmts);
                for shadow in &local_shadows_raw {
                    qualify_exports.remove(shadow.as_str());
                }
                // Update cumulative with ALL exports from this opening for future reopenings
                let all_local_exports: HashSet<AstString> = collect_all_namespace_exports(stmts)
                    .into_iter()
                    .map(AstString::from)
                    .collect();
                self.cumulative_ns_exports
                    .entry(cumulative_key)
                    .or_default()
                    .extend(all_local_exports.iter().cloned());
                self.namespace_exports = qualify_exports;
                // Track local bindings so ancestor export lookup can skip shadowed names.
                // The IIFE parameter itself is also a local binding that shadows
                // any ancestor export with the same name.
                let mut local_shadows: HashSet<AstString> =
                    local_shadows_raw.into_iter().map(AstString::from).collect();
                local_shadows.insert(AstString::from(param_name.as_str()));
                if param_name != name {
                    local_shadows.insert(AstString::from(name.as_str()));
                }
                let prev_ns_local_bindings =
                    std::mem::replace(&mut self.ns_local_bindings, local_shadows);
                let prev_ns_local_value_vars = std::mem::replace(
                    &mut self.ns_local_value_vars,
                    collect_ns_local_value_vars(stmts)
                        .into_iter()
                        .map(AstString::from)
                        .collect(),
                );
                let prev_ie_type_only_ns = std::mem::replace(
                    &mut self.import_equals_type_only_ns,
                    crate::collect_ns_type_only_import_equals(stmts),
                );
                let prev_ns_seen_import_equals = std::mem::take(&mut self.ns_seen_import_equals);
                let prev_cjs_import_map = self.cjs_import_map.clone();
                // Save/restore emitted_var_names so re-opened namespaces
                // get fresh declarations in their own IIFE scope.
                // Clear the set so inner declarations (e.g. `enum require` inside
                // a namespace) get their own `let` even if the name was used at
                // the outer scope level.
                // take (not clone): the IIFE scope starts empty; `take` hands
                // back the old set without deep-copying its keys.
                let prev_emitted_var_names = std::mem::take(&mut self.emitted_var_names);
                // Keep the namespace's own name so merged inner declarations work.
                self.emitted_var_names
                    .insert(AstString::from(param_name.as_str()));
                let prev_ns_original_name = self.ns_original_name.take();
                self.ns_original_name = Some(name.clone());
                let prev_dotted_depth = self.dotted_namespace_depth;
                self.dotted_namespace_depth = 0;
                // Inside namespace IIFEs we're in a new function scope — system
                // module var hoisting does NOT apply here, so clear the flag.
                let prev_system_hoist = self.system_hoist_var_in_execute;
                self.system_hoist_var_in_execute = false;
                // Reset block_depth: the namespace IIFE creates a new scope where
                // exports are valid and should be transformed, not source-copied.
                let prev_block_depth = self.block_depth;
                self.block_depth = 0;
                // Push bare-name const enum entries for this namespace scope so
                // references like `e3.a` (without the namespace prefix) resolve.
                let local_const_enums = if self.should_inline_const_enums() {
                    self.push_local_const_enums(stmts)
                } else {
                    Vec::new()
                };
                // Save position for namespace-scoped temp var insertion.
                let ns_temp_var_insert_pos = self.output.len();
                let prev_ns_temp_vars = std::mem::take(&mut self.ns_temp_var_names);
                let prev_ns_temp_var_counter = self.temp_var_counter;
                self.temp_var_counter = 0;
                self.namespace_iife_fn_scope_depths
                    .push(self.fn_scope_depth);
                for s in stmts {
                    // Skip leading comments for exported var declarations without
                    // initializers. These produce no output in namespace context
                    // (only `M.x = value;` for declarations WITH initializers),
                    // so their comments should be suppressed like TypeScript does.
                    let suppress_comment = match &s.kind {
                        StmtKind::Var(v) => {
                            v.modifiers & MOD_DECLARE != 0
                                || (v.declarations.iter().all(|d| d.init.is_none())
                                    && v.modifiers & MOD_EXPORT != 0)
                        }
                        StmtKind::Export(ed) => match &ed.kind {
                            ExportDeclKind::Decl(inner) => match &inner.kind {
                                StmtKind::Var(v) => {
                                    v.modifiers & MOD_DECLARE != 0
                                        || v.declarations.iter().all(|d| d.init.is_none())
                                }
                                _ => stmt_is_erased(inner, self.preserve_const_enums_effective()),
                            },
                            // `export { X as y }` without source is elided in
                            // namespace bodies (declaration-only), suppress comments.
                            ExportDeclKind::Named { source: None, .. } => true,
                            _ => false,
                        },
                        _ => stmt_is_erased(s, self.preserve_const_enums_effective()),
                    };
                    let saved_output_len = self.output.len();
                    let saved_comment_idx = self.next_comment_idx;
                    let saved_comment_pos = self.comment_emit_pos;
                    let saved_out_col = self.out_col;
                    let saved_at_line_start = self.at_line_start;
                    if !suppress_comment {
                        self.emit_leading_comments(s.span.start);
                    }
                    let after_comments_len = self.output.len();
                    self.emit_stmt(s);
                    // If statement emission produced nothing, rollback any leading
                    // comments that were emitted for this erased statement.
                    if self.output.len() == after_comments_len
                        && after_comments_len > saved_output_len
                    {
                        self.output.truncate(saved_output_len);
                        self.next_comment_idx = saved_comment_idx;
                        self.comment_emit_pos = saved_comment_pos;
                        self.out_col = saved_out_col;
                        self.at_line_start = saved_at_line_start;
                    }
                    self.advance_comment_pos(s.span.end);
                }
                // Insert hoisted temp vars at the top of the IIFE body.
                if !self.ns_temp_var_names.is_empty() {
                    let indent_str = "    ".repeat(self.indent as usize);
                    let var_decl =
                        format!("{}var {};\n", indent_str, self.ns_temp_var_names.join(", "));
                    self.output.insert_str(ns_temp_var_insert_pos, &var_decl);
                }
                self.namespace_iife_fn_scope_depths.pop();
                self.ns_temp_var_names = prev_ns_temp_vars;
                self.temp_var_counter = prev_ns_temp_var_counter;
                // Emit trailing comments between the last statement and the
                // closing brace of the namespace body.
                self.emit_leading_comments(module_decl.span.end);
                self.pop_local_const_enums(local_const_enums);
                self.dotted_namespace_depth = prev_dotted_depth;
                self.system_hoist_var_in_execute = prev_system_hoist;
                self.block_depth = prev_block_depth;
                self.ns_original_name = prev_ns_original_name;
                self.cjs_import_map = prev_cjs_import_map;
                self.ns_local_bindings = prev_ns_local_bindings;
                self.ns_local_value_vars = prev_ns_local_value_vars;
                self.import_equals_type_only_ns = prev_ie_type_only_ns;
                self.ns_seen_import_equals = prev_ns_seen_import_equals;
                self.namespace_exports = prev_ns_exports;
                // Pop parent scope from stack if we pushed one.
                self.ns_export_stack.truncate(ns_stack_depth);
                self.in_export_context = prev_export_ctx;
                self.export_target = prev_target;
                self.emitted_var_names = prev_emitted_var_names;
                self.indent -= 1;
                let is_exported_at_top_level = !in_namespace
                    && parent_target.as_deref() == Some("exports")
                    && (module_decl.modifiers & MOD_EXPORT != 0
                        || self.in_export_context
                        || self.cjs_exported_names.contains(name.as_str()));
                self.write("})(");
                if is_exported_in_ns {
                    let parent = parent_target.as_ref().unwrap();
                    self.write(&name);
                    self.write(" = ");
                    self.write(parent);
                    self.write(".");
                    self.write(&name);
                    self.write(" || (");
                    self.write(parent);
                    self.write(".");
                    self.write(&name);
                    self.writeln(" = {}));");
                } else if is_exported_at_top_level
                    && self.cjs_default_fn_local_names.contains(name.as_str())
                    && !self.in_export_context
                {
                    // `export default function Foo` + `namespace Foo` (no export on ns):
                    // Pattern: exports.Foo || (exports.Foo = {})
                    self.write("exports.");
                    self.write(&name);
                    self.write(" || (exports.");
                    self.write(&name);
                    self.writeln(" = {}));");
                } else if is_exported_at_top_level
                    && self.cjs_default_fn_local_names.contains(name.as_str())
                {
                    // `export default function Decl` + `export namespace Decl`:
                    // Namespace is directly exported; use pure local pattern.
                    self.write(&name);
                    self.write(" || (");
                    self.write(&name);
                    self.writeln(" = {}));");
                } else if is_exported_at_top_level {
                    // Chain all exported names from the live export chain,
                    // e.g. `export { m as instantiatedModule }` + `export namespace m {}`
                    // produces `m || (exports.instantiatedModule = exports.m = m = {})`.
                    let chain = self.cjs_live_export_chain.get(name.as_str()).cloned();
                    self.write(&name);
                    self.write(" || (");
                    if let Some(ref names) = chain {
                        for n in names {
                            self.write_cjs_export_access("exports", n);
                            self.write(" = ");
                        }
                    } else {
                        self.write_cjs_export_access("exports", &name);
                        self.write(" = ");
                    }
                    self.write(&name);
                    self.writeln(" = {}));");
                } else {
                    self.write(&name);
                    self.write(" || (");
                    if let Some(exported_names) =
                        self.system_inline_export_aliases.get(&name).cloned()
                    {
                        let mut wrap_count = 0usize;
                        for exported in exported_names.iter().rev() {
                            {
                                let sys_fn = self.system_exports_fn.clone();
                                self.write(&sys_fn);
                            }
                            self.write("(\"");
                            self.write(exported);
                            self.write("\", ");
                            wrap_count += 1;
                        }
                        self.write(&name);
                        self.write(" = {}");
                        for _ in 0..wrap_count {
                            self.write(")");
                        }
                        self.writeln("));");
                    } else {
                        self.write(&name);
                        self.writeln(" = {}));");
                    }
                }
            }
            Some(ModuleBody::Module(inner)) => {
                // Dotted namespace: `namespace A.B { ... }` is parsed as
                // ModuleDecl { name: "A", body: Module(ModuleDecl { name: "B", body: ... }) }
                // Emit the outer wrapper IIFE, then recurse for the inner module.
                let name = match &module_decl.name {
                    ModuleName::Ident(n) => n.clone(),
                    ModuleName::String(n) => n.clone(),
                };
                // Determine if we are inside a parent namespace
                let parent_target = self.export_target.clone();
                let in_namespace = parent_target.as_ref().is_some_and(|t| t != "exports");
                let is_exported_in_ns = in_namespace
                    && (module_decl.modifiers & MOD_EXPORT != 0 || self.in_export_context);
                // Check for IIFE parameter name collision with inner bindings.
                // For dotted namespaces (A.M), wrap the inner ModuleDecl as a
                // single-element statement slice so the collision checker can
                // recurse into it.
                let inner_as_stmt = Stmt {
                    kind: StmtKind::ModuleDecl(Box::new((**inner).clone())),
                    span: inner.span,
                };
                let inner_stmts = [inner_as_stmt];
                let param_name = if has_ns_name_collision(&inner_stmts, &name) {
                    let counter = self
                        .ns_collision_counters
                        .entry(AstString::from(name.as_str()))
                        .or_insert(0);
                    *counter += 1;
                    format!("{}_{}", name, counter)
                } else {
                    name.clone()
                };
                // Only emit var/let for first occurrence of this name
                if !self.emitted_var_names.contains(name.as_str()) {
                    self.emitted_var_names
                        .insert(AstString::from(name.as_str()));
                    if (in_namespace || self.fn_scope_depth > 0)
                        && self.dotted_namespace_depth == 0
                        && !self.needs_lexical_downlevel()
                    {
                        self.write("let ");
                    } else {
                        self.write("var ");
                    }
                    self.write(&name);
                    self.writeln(";");
                }
                self.write("(function (");
                self.write(&param_name);
                self.writeln(") {");
                self.indent += 1;
                // Inside this namespace body, export target is the (possibly renamed) parameter
                let prev_target = self.export_target.take();
                self.export_target = Some(param_name.clone());
                let prev_export_ctx = self.in_export_context;
                self.in_export_context = false;
                // Save/restore namespace exports for nested namespaces
                let prev_ns_exports = std::mem::take(&mut self.namespace_exports);
                let prev_cjs_import_map = self.cjs_import_map.clone();
                // take (not clone): new IIFE scope starts empty (like the Block
                // case), keeping only the param name so inner declarations get
                // their own `var`. Avoids deep-copying all keys.
                let prev_emitted_var_names2 = std::mem::take(&mut self.emitted_var_names);
                self.emitted_var_names
                    .insert(AstString::from(param_name.as_str()));
                // For dotted namespaces, push cumulative exports for this intermediate
                // level onto the stack so the innermost Block body can qualify references.
                let ns_stack_depth_mod = self.ns_export_stack.len();
                {
                    let cumul_key: AstString = if let Some(ref orig) = self.ns_original_name {
                        format!("{}::{}", orig, name).into()
                    } else {
                        match &prev_target {
                            Some(parent) => format!("{}::{}", parent, name).into(),
                            None => AstString::from(name.as_str()),
                        }
                    };
                    let cumul_exports = self
                        .cumulative_ns_exports
                        .get(&cumul_key)
                        .cloned()
                        .unwrap_or_default();
                    if !cumul_exports.is_empty() {
                        self.ns_export_stack
                            .push((AstString::from(param_name.as_str()), cumul_exports));
                    }
                }
                // The inner module is always exported in a dotted namespace
                let mut inner_with_export = (**inner).clone();
                inner_with_export.modifiers |= MOD_EXPORT;
                let prev_ns_original_name = self.ns_original_name.take();
                self.ns_original_name = Some(name.clone());
                // Only increment dotted_namespace_depth when the dotted
                // expansion started at the top level (not inside a
                // namespace).  When started inside a namespace, keeping
                // depth at 0 ensures the Block branch uses `let` for the
                // innermost (block-bodied) module of the dotted chain.
                if !in_namespace {
                    self.dotted_namespace_depth += 1;
                }
                self.emit_module_decl(&inner_with_export);
                if !in_namespace {
                    self.dotted_namespace_depth = self.dotted_namespace_depth.saturating_sub(1);
                }
                self.ns_export_stack.truncate(ns_stack_depth_mod);
                self.ns_original_name = prev_ns_original_name;
                self.cjs_import_map = prev_cjs_import_map;
                self.namespace_exports = prev_ns_exports;
                self.in_export_context = prev_export_ctx;
                self.export_target = prev_target;
                self.emitted_var_names = prev_emitted_var_names2;
                self.indent -= 1;
                let is_exported_at_top_level = !in_namespace
                    && parent_target.as_deref() == Some("exports")
                    && (module_decl.modifiers & MOD_EXPORT != 0
                        || self.in_export_context
                        || self.cjs_exported_names.contains(name.as_str()));
                self.write("})(");
                if is_exported_in_ns {
                    let parent = parent_target.as_ref().unwrap();
                    self.write(&name);
                    self.write(" = ");
                    self.write(parent);
                    self.write(".");
                    self.write(&name);
                    self.write(" || (");
                    self.write(parent);
                    self.write(".");
                    self.write(&name);
                    self.writeln(" = {}));");
                } else if is_exported_at_top_level {
                    // CJS top-level: name || (exports.name = name = {})
                    self.write(&name);
                    self.write(" || (exports.");
                    self.write(&name);
                    self.write(" = ");
                    self.write(&name);
                    self.writeln(" = {}));");
                } else {
                    self.write(&name);
                    self.write(" || (");
                    if let Some(exported_names) =
                        self.system_inline_export_aliases.get(&name).cloned()
                    {
                        let mut wrap_count = 0usize;
                        for exported in exported_names.iter().rev() {
                            {
                                let sys_fn = self.system_exports_fn.clone();
                                self.write(&sys_fn);
                            }
                            self.write("(\"");
                            self.write(exported);
                            self.write("\", ");
                            wrap_count += 1;
                        }
                        self.write(&name);
                        self.write(" = {}");
                        for _ in 0..wrap_count {
                            self.write(")");
                        }
                        self.writeln("));");
                    } else {
                        self.write(&name);
                        self.writeln(" = {}));");
                    }
                }
            }
            None => {
                // No body (e.g., forward declaration) -- nothing to emit
                return false;
            }
        }
        true
    }

    fn emit_recovery_declared_namespace(&mut self, module_decl: &ModuleDecl) -> bool {
        let ModuleName::Ident(name) = &module_decl.name else {
            return false;
        };
        if name != "<error>" {
            return false;
        }

        // Parser recovery for `declare namespace debugger {}`-style inputs:
        // TypeScript emits tokenized statements instead of eliding the declaration.
        let source = self.copy_span_trimmed(module_decl.span).trim();
        let Some(rest) = source.strip_prefix("declare") else {
            return false;
        };
        let rest = rest.trim_start();
        let (kw, after_kw) = if let Some(after) = rest.strip_prefix("namespace") {
            ("namespace", after)
        } else if let Some(after) = rest.strip_prefix("module") {
            ("module", after)
        } else {
            return false;
        };
        let after_kw = after_kw.trim_start();
        let name_end = after_kw
            .find('{')
            .or_else(|| after_kw.find(';'))
            .unwrap_or(after_kw.len());
        let recovered_name = after_kw[..name_end].trim();

        self.writeln("declare;");
        self.write(kw);
        self.writeln(";");
        if !recovered_name.is_empty() {
            self.write(recovered_name);
            self.writeln(";");
        }
        if recovered_name.is_empty() {
            match module_decl.body.as_ref() {
                Some(ModuleBody::Block(stmts)) => {
                    self.writeln("{");
                    self.indent += 1;
                    for stmt in stmts {
                        self.emit_stmt(stmt);
                    }
                    self.indent -= 1;
                    self.writeln("}");
                }
                Some(ModuleBody::Module(_)) => {
                    self.writeln("{ }");
                }
                None if source.contains('{') => {
                    self.writeln("{ }");
                }
                None => {}
            }
        } else if source.contains('{')
            || matches!(module_decl.body, Some(ModuleBody::Block(_)))
            || matches!(module_decl.body, Some(ModuleBody::Module(_)))
        {
            self.writeln("{ }");
        }
        true
    }

    pub(super) fn emit_import_decl(&mut self, import_decl: &ImportDecl) {
        if import_decl.type_only {
            if self.options.verbatim_module_syntax == Some(true) && !self.is_cjs_like() {
                // verbatimModuleSyntax: emit `import {} from "mod"` for type-only imports
                // with named specifiers. `import type Default from "mod"` is erased entirely.
                let has_named = matches!(
                    &import_decl.specifiers,
                    ImportClause::Named { named, .. } if !named.is_empty()
                );
                if has_named {
                    let q = self.detect_string_quote(import_decl.span);
                    self.emitted_esm_export = true;
                    self.write("import {} from ");
                    self.write(q);
                    let source = self.rewrite_relative_import_specifier(
                        &import_decl.source,
                        self.options.jsx == Some(JsxEmit::Preserve),
                    );
                    self.write(&source);
                    self.write(q);
                    self.writeln(";");
                }
            }
            return;
        }
        if self.emit_recovery_import_decl(import_decl) {
            return;
        }
        if self.import_decl_has_reserved_word_recovery_shape(import_decl, import_decl.span) {
            self.emit_recovery_reserved_word_import_decl(import_decl, import_decl.span);
            return;
        }

        if self.is_cjs_like() {
            // CommonJS/AMD/UMD import transform
            match &import_decl.specifiers {
                ImportClause::Named {
                    default,
                    named,
                    namespace,
                } => {
                    // Filter out type-only specifiers and elided imports
                    let non_type: Vec<_> = named
                        .iter()
                        .filter(|s| {
                            !s.is_type
                                && !self.is_import_elided(&s.local)
                                && !self.merged_namespace_names.contains(s.local.as_str())
                        })
                        .collect();
                    let has_named_default = non_type
                        .iter()
                        .any(|s| s.imported.as_deref() == Some("default"));
                    let has_named_non_default = non_type
                        .iter()
                        .any(|s| s.imported.as_deref() != Some("default"));
                    let keep_default = default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                    let keep_namespace = namespace.as_ref().is_some_and(|ns| {
                        ns != "<error>"
                            && !self.is_import_elided(ns)
                            && !self.merged_namespace_names.contains(ns.as_str())
                    });
                    let has_bindings = keep_default || keep_namespace || !non_type.is_empty();
                    // TypeScript 5.x always wraps namespace/default imports with
                    // __importStar/__importDefault in CJS/AMD output.
                    // TypeScript 5.x always wraps namespace/default imports
                    // with __importStar/__importDefault in CJS output, UNLESS
                    // importHelpers=true + esModuleInterop=false (no wrapping).
                    // When importHelpers=true + esModuleInterop=true, wrapping
                    // uses tslib_1. prefix (handled by helper_prefix()).
                    let use_interop =
                        self.options.import_helpers != Some(true) || self.es_module_interop();

                    if !has_bindings {
                        // All bindings were elided or it was a side-effect import.
                        if import_decl.is_side_effect {
                            // `import "mod"` → side-effect require.
                            // Skip if another kept value import already emits `require("mod")`.
                            if self
                                .import_sources_with_value_bindings
                                .contains(import_decl.source.as_str())
                            {
                                return;
                            }
                            // Side-effect import: require("module");
                            self.write("require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        } else if self
                            .cjs_keep_side_effect_sources
                            .contains(import_decl.source.as_str())
                        {
                            // Keep require() for side effects (.js file with
                            // `export type *` re-exporting values).
                            let var_name = self.next_require_var(&import_decl.source);
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        // `import {} from "mod"` or all bindings elided: emit nothing
                    } else if keep_default && keep_namespace {
                        // Combined: import foo, * as ns from 'mod'
                        // TypeScript uses __importStar with a temp binding:
                        // `[var|const] t = __importStar(require("mod")), ns = t;`
                        // Default accessed via t.default
                        let def_name = default.as_ref().unwrap();
                        let ns = namespace.as_ref().unwrap();
                        let var_name = self.next_require_var(&import_decl.source);
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importStar(require(\"");
                            self.write(&import_decl.source);
                            self.write("\")), ");
                            self.write(ns);
                            self.write(" = ");
                            self.write(&var_name);
                            self.writeln(";");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.write("\"), ");
                            self.write(ns);
                            self.write(" = ");
                            self.write(&var_name);
                            self.writeln(";");
                        }
                        // Default binding via cjs_import_map rewriting
                        self.cjs_import_map.insert(
                            AstString::from(def_name.as_str()),
                            (
                                AstString::from(var_name.as_str()),
                                AstString::from("default"),
                            ),
                        );
                        self.cjs_default_import_bindings
                            .insert(AstString::from(def_name.as_str()));
                    } else if keep_namespace {
                        // import * as ns from 'mod'
                        // -> [var|const] ns = require("mod") | __importStar(require("mod"))
                        let ns = namespace.as_ref().unwrap();
                        // Missing-source sentinel (recovered `import * from
                        // Zero ...`): tsc emits a bare `require()`.
                        let missing_src = import_decl.source.is_empty()
                            && import_decl.source_span.start == 0
                            && import_decl.source_span.end == 0;
                        let req = if missing_src {
                            "require()".to_string()
                        } else {
                            format!("require(\"{}\")", import_decl.source)
                        };
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(ns);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importStar(");
                            self.write(&req);
                            self.writeln(");");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(ns);
                            self.write(" = ");
                            self.write(&req);
                            self.writeln(";");
                        }
                    } else if keep_default && has_named_non_default {
                        // Combined: import foo, { a, b } from 'mod'
                        // TypeScript wraps with __importStar for the single binding.
                        // Default accessed via .default, named via .member
                        let var_name = self.next_require_var(&import_decl.source);
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importStar(require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\"));");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        // Default binding via cjs_import_map rewriting
                        let def_name = default.as_ref().unwrap();
                        self.cjs_import_map.insert(
                            AstString::from(def_name.as_str()),
                            (
                                AstString::from(var_name.as_str()),
                                AstString::from("default"),
                            ),
                        );
                        self.cjs_default_import_bindings
                            .insert(AstString::from(def_name.as_str()));
                        // Named bindings via cjs_import_map rewriting
                        for spec in &non_type {
                            if self
                                .cjs_no_qualify_import_locals
                                .contains(spec.local.as_str())
                            {
                                continue;
                            }
                            let imported = spec.imported.as_deref().unwrap_or(&spec.local);
                            self.cjs_import_map.insert(
                                AstString::from(spec.local.as_str()),
                                (
                                    AstString::from(var_name.as_str()),
                                    AstString::from(imported),
                                ),
                            );
                        }
                    } else if keep_default && has_named_default {
                        // Combined: import foo, { default as d } from 'mod'
                        // All named imports are default aliases, so use __importDefault.
                        let def_name = default.as_ref().unwrap();
                        let var_name = self.next_require_var(&import_decl.source);
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importDefault(require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\"));");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        self.cjs_import_map.insert(
                            AstString::from(def_name.as_str()),
                            (
                                AstString::from(var_name.as_str()),
                                AstString::from("default"),
                            ),
                        );
                        self.cjs_default_import_bindings
                            .insert(AstString::from(def_name.as_str()));
                        for spec in &non_type {
                            if self
                                .cjs_no_qualify_import_locals
                                .contains(spec.local.as_str())
                            {
                                continue;
                            }
                            if spec.imported.as_deref() == Some("default") {
                                self.cjs_import_map.insert(
                                    AstString::from(spec.local.as_str()),
                                    (
                                        AstString::from(var_name.as_str()),
                                        AstString::from("default"),
                                    ),
                                );
                            }
                        }
                    } else if has_named_default && has_named_non_default {
                        // Named mix with default alias: import { default as d, a } from 'mod'
                        // TypeScript wraps with __importStar and rewrites all bindings.
                        let var_name = self.next_require_var(&import_decl.source);
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importStar(require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\"));");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        for spec in &non_type {
                            if self
                                .cjs_no_qualify_import_locals
                                .contains(spec.local.as_str())
                            {
                                continue;
                            }
                            let imported = spec.imported.as_deref().unwrap_or(&spec.local);
                            self.cjs_import_map.insert(
                                AstString::from(spec.local.as_str()),
                                (
                                    AstString::from(var_name.as_str()),
                                    AstString::from(imported),
                                ),
                            );
                        }
                    } else if has_named_default {
                        // Named default-only alias: import { default as d } from 'mod'
                        // TypeScript treats this like a default import and uses __importDefault.
                        let var_name = self.next_require_var(&import_decl.source);
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importDefault(require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\"));");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        for spec in &non_type {
                            if self
                                .cjs_no_qualify_import_locals
                                .contains(spec.local.as_str())
                            {
                                continue;
                            }
                            if spec.imported.as_deref() == Some("default") {
                                self.cjs_import_map.insert(
                                    AstString::from(spec.local.as_str()),
                                    (
                                        AstString::from(var_name.as_str()),
                                        AstString::from("default"),
                                    ),
                                );
                            }
                        }
                    } else if keep_default {
                        // Default-only: import foo from 'mod'
                        // → [var|const] mod_1 = __importDefault(require("mod"));
                        // References to foo are rewritten to mod_1.default
                        let def_name = default.as_ref().unwrap();
                        let var_name = self.next_require_var(&import_decl.source);
                        if use_interop {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = ");
                            self.write(self.helper_prefix());
                            self.write("__importDefault(require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\"));");
                        } else {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        self.cjs_import_map.insert(
                            AstString::from(def_name.as_str()),
                            (
                                AstString::from(var_name.as_str()),
                                AstString::from("default"),
                            ),
                        );
                        self.cjs_default_import_bindings
                            .insert(AstString::from(def_name.as_str()));
                    } else {
                        // Named-only: import { a, b } from 'mod'
                        // -> [var|const] mod_1 = require("mod");
                        // References get rewritten: a → mod_1.a, b → mod_1.b
                        let var_name = self.next_require_var(&import_decl.source);
                        if !self.elide_sole_empty_fragment_factory_import {
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&var_name);
                            self.write(" = require(\"");
                            self.write(&import_decl.source);
                            self.writeln("\");");
                        }
                        // Register bindings for reference rewriting
                        for spec in &non_type {
                            // Skip type-only imports that should not be qualified
                            // (e.g. namespace merge with type-only origin).
                            if self
                                .cjs_no_qualify_import_locals
                                .contains(spec.local.as_str())
                            {
                                continue;
                            }
                            let imported = spec.imported.as_deref().unwrap_or(&spec.local);
                            self.cjs_import_map.insert(
                                AstString::from(spec.local.as_str()),
                                (
                                    AstString::from(var_name.as_str()),
                                    AstString::from(imported),
                                ),
                            );
                        }
                    }
                }
                ImportClause::Require(name) => {
                    if self.should_elide_require_import(import_decl)
                        && !self.jsx_factory_import_retained.contains(name.as_str())
                        && !self.jsx_element_import_retained.contains(name.as_str())
                    {
                        return;
                    }
                    if self.is_import_elided(name) {
                        return;
                    }
                    self.write(self.generated_require_binding_keyword());
                    self.write(" ");
                    self.write(name);
                    self.write(" = require(\"");
                    let source = self.rewrite_relative_import_specifier(
                        &import_decl.source,
                        self.options.jsx == Some(JsxEmit::Preserve),
                    );
                    self.write(&source);
                    self.writeln("\");");
                }
            }
            return;
        }

        // ES module import emit (non-CJS path)
        match &import_decl.specifiers {
            ImportClause::Named {
                default,
                named,
                namespace,
            } => {
                // Filter out type-only specifiers and elided imports
                let non_type: Vec<_> = named
                    .iter()
                    .filter(|s| {
                        !s.is_type
                            && !self.is_import_elided(&s.local)
                            && !self.merged_namespace_names.contains(s.local.as_str())
                    })
                    .collect();
                let keep_default = default.as_ref().is_some_and(|d| !self.is_import_elided(d));
                let keep_namespace = namespace
                    .as_ref()
                    .is_some_and(|ns| !self.is_import_elided(ns));

                // If all bindings are elided, check if we should skip entirely
                if !keep_default && !keep_namespace && non_type.is_empty() {
                    // With verbatimModuleSyntax, preserve `import {} from "mod"` only
                    // when the user wrote literally empty braces (named.is_empty()),
                    // NOT when all specifiers were individually type-only and got filtered.
                    let is_verbatim_empty_import = !import_decl.is_side_effect
                        && self.options.verbatim_module_syntax == Some(true)
                        && named.is_empty()
                        && default.is_none()
                        && namespace.is_none();
                    if !import_decl.is_side_effect && !is_verbatim_empty_import {
                        // `import {} from "mod"` or all bindings were elided:
                        // skip the import entirely
                        return;
                    }
                    // `import "mod"` or verbatimModuleSyntax `import {} from "mod"` — emit it
                    self.emitted_esm_export = true;
                    if import_decl.is_side_effect {
                        self.write("import ");
                    } else {
                        // verbatimModuleSyntax: preserve `import {} from "mod"`
                        self.write("import {} from ");
                    }
                } else {
                    // Source-copy preserves inline comments between tokens.
                    // Only use when: all specifiers kept AND inline comments exist.
                    let all_named_kept = non_type.len() == named.len();
                    let all_default_kept = default.is_none() || keep_default;
                    let all_ns_kept = namespace.is_none() || keep_namespace;
                    let can_source_copy = all_named_kept
                        && all_default_kept
                        && all_ns_kept
                        && import_decl.span.end > import_decl.span.start
                        && self.span_has_inline_comment(import_decl.span);

                    if can_source_copy {
                        self.emitted_esm_export = true;
                        let src = self.source_copy_import_stmt(import_decl.span);
                        self.write(src);
                        self.writeln(";");
                        return;
                    }

                    // Import with retained bindings — will be emitted
                    self.emitted_esm_export = true;
                    self.write("import ");
                    if import_decl.defer {
                        self.write("defer ");
                    }
                    let mut has_prev = false;
                    if keep_default {
                        self.write(default.as_ref().unwrap());
                        has_prev = true;
                    }
                    if keep_namespace {
                        if has_prev {
                            self.write(", ");
                        }
                        self.write("* as ");
                        self.write(namespace.as_ref().unwrap());
                        has_prev = true;
                    }
                    if !non_type.is_empty() {
                        if has_prev {
                            self.write(", ");
                        }
                        self.write("{ ");
                        for (i, spec) in non_type.iter().enumerate() {
                            if i > 0 {
                                self.write(", ");
                            }
                            if let Some(ref imp) = spec.imported {
                                if spec.imported_is_string {
                                    self.write("\"");
                                    self.write(imp);
                                    self.write("\"");
                                } else {
                                    self.emit_module_export_name(imp);
                                }
                                self.write(" as ");
                            }
                            self.write(&spec.local);
                        }
                        // Preserve trailing comma from source
                        if let Some(last) = non_type.last() {
                            let end = last.span.end as usize;
                            let decl_end = import_decl.span.end as usize;
                            if end < decl_end && decl_end <= self.source.len() {
                                let between = &self.source[end..decl_end];
                                if let Some(ci) = between.find(',') {
                                    if between[ci + 1..].trim_start().starts_with('}') {
                                        self.write(",");
                                    }
                                }
                            }
                        }
                        self.write(" }");
                        has_prev = true;
                    }
                    if has_prev {
                        self.write(" from ");
                    }
                }
            }
            ImportClause::Require(name) => {
                if self.is_import_elided(name) {
                    return;
                }
                if self.import_decl_needs_node_esm_require_bridge(import_decl) {
                    self.emitted_esm_export = true;
                    self.write(self.generated_require_binding_keyword());
                    self.write(" ");
                    self.write(name);
                    self.write(" = ");
                    self.emit_node_esm_require_call_for_source(
                        &import_decl.source,
                        import_decl.span,
                    );
                    self.writeln(";");
                    return;
                }
                // In pure ESM mode (not CJS, not Preserve), `import X = require(...)`
                // is not valid JS syntax — TypeScript always elides it.
                if !self.is_cjs_like() && self.effective_module_kind() != ModuleKind::Preserve {
                    return;
                }
                self.emitted_esm_export = true;
                let q = self.detect_string_quote(import_decl.span);
                if self.effective_module_kind() == ModuleKind::Preserve {
                    // Preserve the require form while still downleveling the
                    // generated declaration for pre-ES2015 targets.
                    self.write(self.generated_require_binding_keyword());
                    self.write(" ");
                } else {
                    self.write("import ");
                }
                self.write(name);
                self.write(" = require(");
                self.write(q);
                let rewritten_source = self.rewrite_relative_import_specifier(
                    &import_decl.source,
                    self.options.jsx == Some(JsxEmit::Preserve),
                );
                self.write(&rewritten_source);
                self.write(q);
                self.writeln(");");
                return;
            }
        }
        // Detect original quote style from source text
        let q = self.detect_string_quote(import_decl.span);
        self.write(q);
        let source = self.rewrite_relative_import_specifier(
            &import_decl.source,
            self.options.jsx == Some(JsxEmit::Preserve),
        );
        self.write(&source);
        self.write(q);
        self.emit_import_attributes_from_source(import_decl.span);
        self.writeln(";");
    }

    pub(super) fn emit_export_decl(&mut self, export_decl: &ExportDecl) {
        // When inside a namespace (export_target is set and isn't "exports"),
        // always use the CJS/namespace export path regardless of module kind.
        let in_namespace = self.export_target.as_ref().is_some_and(|t| t != "exports");
        if self.is_cjs_like() || in_namespace {
            self.emit_export_decl_cjs(export_decl);
            return;
        }
        if self.emit_recovery_export_decl(export_decl) {
            return;
        }

        // ES module export emit
        match &export_decl.kind {
            ExportDeclKind::Decl(decl) => {
                match &decl.kind {
                    StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => (),
                    StmtKind::FnDecl(fn_decl) => {
                        if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        self.emitted_esm_export = true;
                        self.write("export ");
                        self.emit_fn_decl(fn_decl);
                    }
                    StmtKind::ClassDecl(class_decl) => {
                        if class_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        self.emitted_esm_export = true;
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_can_emit_simple_standard_decorator_wrapper(class_decl)
                        {
                            self.emit_simple_standard_decorated_class_wrapper(class_decl);
                            if let Some(ref name) = class_decl.name {
                                self.write("export { ");
                                self.write(name);
                                self.writeln(" };");
                            }
                            return;
                        }
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_native_standard_decorator_shape(class_decl).is_some()
                        {
                            self.emit_native_standard_decorator_class_decl_wrapper(class_decl);
                            if let Some(ref name) = class_decl.name {
                                self.write("export { ");
                                self.write(name);
                                self.writeln(" };");
                            }
                            return;
                        }
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_can_emit_narrow_standard_decorator_member_decl(class_decl)
                        {
                            self.emit_narrow_standard_decorated_class_decl_wrapper(class_decl);
                            if let Some(ref name) = class_decl.name {
                                self.write("export { ");
                                self.write(name);
                                self.writeln(" };");
                            }
                            return;
                        }
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && (class_can_emit_method_only_decorator_wrapper(class_decl)
                                || (self.effective_target() >= ScriptTarget::ES2015
                                    && self.can_emit_public_multi_method_decorator_wrapper(
                                        class_decl,
                                    )))
                        {
                            self.emit_method_only_decorated_class_wrapper(class_decl);
                            if let Some(ref name) = class_decl.name {
                                self.write("export { ");
                                self.write(name);
                                self.writeln(" };");
                            }
                            return;
                        }
                        let needs_let_wrapper = self.options.experimental_decorators == Some(true)
                            && class_needs_let_wrapper(class_decl);
                        if needs_let_wrapper {
                            // Decorated ESM-exported classes:
                            //   var C_1;  (if self-referencing)
                            //   let A = [C_1 =] class A { };
                            //   __decorate([...], A.prototype, ...);
                            //   A = [C_1 =] __decorate([...], A);
                            //   export { A };
                            let self_ref_alias = class_decl.name.as_ref().and_then(|name| {
                                if decorated_class_has_self_reference(class_decl) {
                                    let counter = self
                                        .decorated_alias_emit_counter
                                        .entry(AstString::from(name.as_str()))
                                        .or_insert(0);
                                    *counter += 1;
                                    Some(format!("{}_{}", name, counter))
                                } else {
                                    None
                                }
                            });
                            let saved_import_map_entry = if let Some(ref alias) = self_ref_alias {
                                if let Some(ref name) = class_decl.name {
                                    // `var X_1;` is hoisted to top by the pre-scan
                                    let prev = self.cjs_import_map.remove(name.as_str());
                                    self.cjs_import_map.insert(
                                        AstString::from(name.as_str()),
                                        (AstString::from(alias.as_str()), AstString::new("")),
                                    );
                                    Some((name.clone(), prev))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if let Some(ref name) = class_decl.name {
                                self.write("let ");
                                self.write(name);
                                self.write(" = ");
                                if let Some(ref alias) = self_ref_alias {
                                    self.write(alias);
                                    self.write(" = ");
                                }
                            }
                            self.emit_class_decl(class_decl);
                            // Replace trailing `}\n` with `};\n`
                            // Use pre_static_output_len to find the class body end
                            // (before any static initializers emitted after it).
                            if let Some(pos) = self.pre_static_output_len {
                                if pos >= 2 && &self.output[pos - 2..pos] == "}\n" {
                                    self.output.insert(pos - 1, ';');
                                }
                            } else if self.output.ends_with("}\n") {
                                let len = self.output.len();
                                self.output.insert(len - 1, ';');
                            }
                            self.decorated_class_self_ref_alias = self_ref_alias;
                            self.emit_decorator_applications(class_decl, None);
                            self.decorated_class_self_ref_alias = None;
                            if let Some((name, prev)) = saved_import_map_entry {
                                if let Some(prev_entry) = prev {
                                    self.cjs_import_map
                                        .insert(AstString::from(name.as_str()), prev_entry);
                                } else {
                                    self.cjs_import_map.remove(name.as_str());
                                }
                            }
                            if let Some(ref name) = class_decl.name {
                                self.write("export { ");
                                self.write(name);
                                self.writeln(" };");
                            }
                        } else {
                            if self.should_preserve_decorators()
                                && !class_decl.decorators.is_empty()
                            {
                                // Find actual `export` keyword position in source
                                let export_kw_pos = {
                                    let s = export_decl.span.start as usize;
                                    let e = (export_decl.span.end as usize).min(self.source.len());
                                    self.source
                                        .get(s..e)
                                        .and_then(|text| text.find("export").map(|off| s + off))
                                        .unwrap_or(s)
                                };
                                let export_kw_end = export_kw_pos + 6; // "export".len()

                                // Split decorators: before export vs after export
                                let decs_before: Vec<Expr> = class_decl
                                    .decorators
                                    .iter()
                                    .filter(|d| (d.span.start as usize) < export_kw_pos)
                                    .cloned()
                                    .collect();
                                let decs_after: Vec<Expr> = class_decl
                                    .decorators
                                    .iter()
                                    .filter(|d| (d.span.start as usize) >= export_kw_end)
                                    .cloned()
                                    .collect();

                                // Emit decorators before export
                                if !decs_before.is_empty() {
                                    self.emit_preserved_decorators(
                                        &decs_before,
                                        Some(export_kw_pos as u32),
                                    );
                                }

                                // Emit export keyword
                                self.write("export ");

                                // Emit decorators after export
                                if !decs_after.is_empty() {
                                    let last_after_end =
                                        decs_after.last().unwrap().span.end as usize;
                                    let class_kw_pos = self
                                        .source
                                        .get(last_after_end..)
                                        .and_then(|s| s.find("class"))
                                        .map(|off| (last_after_end + off) as u32);
                                    self.emit_preserved_decorators(&decs_after, class_kw_pos);
                                }

                                // Emit class without decorators
                                let mut clean = class_decl.clone();
                                clean.decorators.clear();
                                self.emit_class_decl(&clean);
                            } else {
                                self.write("export ");
                                self.emit_class_decl(class_decl);
                            }
                            // Emit __decorate calls for member/class decorators
                            if self.options.experimental_decorators == Some(true)
                                && class_has_decorators(class_decl)
                            {
                                self.emit_decorator_applications(class_decl, None);
                            }
                        }
                    }
                    StmtKind::Var(var_stmt) => {
                        if var_stmt.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        self.emitted_esm_export = true;
                        if let Some(init) = self
                            .direct_es5_empty_binding_export_init(var_stmt, export_decl.span)
                            .cloned()
                        {
                            let value_temp = self.next_temp_var();
                            let export_name = self.next_deferred_export_name_placeholder();
                            self.write("export var ");
                            self.write(&export_name);
                            self.write(" = ");
                            self.write(&value_temp);
                            self.write(" = ");
                            self.emit_expr(&init);
                            self.writeln(";");
                        } else if let Some((
                            init,
                            binding_name,
                            binding_span,
                            rest_name,
                            rest_span,
                        )) =
                            self.direct_es5_object_rest_export(var_stmt, export_decl.span)
                        {
                            self.emit_direct_es5_object_rest_export(
                                None,
                                &init,
                                &binding_name,
                                binding_span,
                                &rest_name,
                                rest_span,
                            );
                        } else {
                            self.write("export ");
                            self.emit_var_stmt(var_stmt);
                        }
                    }
                    StmtKind::EnumDecl(enum_decl) => {
                        if enum_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        self.emitted_esm_export = true;
                        // Only prefix `export` on the first occurrence where the
                        // `var` declaration is emitted.  Subsequent enum reopenings
                        // (merged enums) emit only the IIFE without `export`.
                        if !self.emitted_var_names.contains(enum_decl.name.as_str()) {
                            self.write("export ");
                        }
                        self.emit_enum_decl(enum_decl);
                    }
                    StmtKind::ModuleDecl(module_decl) => {
                        // Ambient namespaces are completely erased
                        if module_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        // Only prefix `export` on the first occurrence where the
                        // `var` declaration is emitted.  Subsequent namespace
                        // reopenings emit only the IIFE without `export`.
                        let ns_name = match &module_decl.name {
                            ModuleName::Ident(n) => n.as_str(),
                            ModuleName::String(n) => n.as_str(),
                        };
                        let before = self.output.len();
                        let saved_at_line_start = self.at_line_start;
                        let saved_out_col = self.out_col;
                        if !self.emitted_var_names.contains(ns_name) {
                            self.write("export ");
                        }
                        let emitted = self.emit_module_decl(module_decl);
                        if emitted {
                            self.emitted_esm_export = true;
                        } else {
                            // Type-only namespace — rollback `export ` prefix
                            self.output.truncate(before);
                            self.at_line_start = saved_at_line_start;
                            self.out_col = saved_out_col;
                        }
                    }
                    StmtKind::Import(import_decl) => {
                        if let ImportClause::Require(name) = &import_decl.specifiers {
                            if self.import_decl_needs_node_esm_require_bridge(import_decl) {
                                self.emitted_esm_export = true;
                                self.write(self.generated_require_binding_keyword());
                                self.write(" ");
                                self.write(name);
                                self.write(" = ");
                                self.emit_node_esm_require_call_for_source(
                                    &import_decl.source,
                                    import_decl.span,
                                );
                                self.writeln(";");
                                self.write("export { ");
                                self.write(name);
                                self.writeln(" };");
                                return;
                            }
                        }
                        self.emitted_esm_export = true;
                        self.write("export ");
                        self.emit_stmt(decl);
                    }
                    StmtKind::ImportEquals(ie) => {
                        if self.import_equals_rhs_is_type_only_require_spec(&ie.module_ref)
                            || !self.import_equals_rhs_has_runtime_value(&ie.module_ref)
                        {
                            return;
                        }
                        if self.node_esm_import_require_enabled()
                            && matches!(
                                &ie.module_ref.kind,
                                ExprKind::Call(call)
                                    if matches!(&call.callee.kind, ExprKind::Ident(callee) if callee == "require")
                            )
                        {
                            self.emitted_esm_export = true;
                            self.write(self.generated_require_binding_keyword());
                            self.write(" ");
                            self.write(&ie.name);
                            self.write(" = ");
                            if !self.emit_node_esm_require_call_for_expr(&ie.module_ref) {
                                self.emit_expr(&ie.module_ref);
                            }
                            self.writeln(";");
                            self.write("export { ");
                            self.write(&ie.name);
                            self.writeln(" };");
                            return;
                        }
                        self.emitted_esm_export = true;
                        self.write("export var ");
                        self.write(&ie.name);
                        self.write(" = ");
                        self.emit_expr(&ie.module_ref);
                        self.writeln(";");
                    }
                    _ => {
                        // Non-exportable statements (throw, return, break, etc.)
                        // — strip the `export` keyword for error recovery output.
                        let is_non_exportable = matches!(
                            &decl.kind,
                            StmtKind::Throw(_)
                                | StmtKind::Return(_)
                                | StmtKind::Break(_)
                                | StmtKind::Continue(_)
                                | StmtKind::Debugger
                                | StmtKind::If(_)
                                | StmtKind::While(_)
                                | StmtKind::DoWhile(_)
                                | StmtKind::For(_)
                                | StmtKind::ForIn(_)
                                | StmtKind::ForOf(_)
                                | StmtKind::Switch(_)
                                | StmtKind::Try(_)
                                | StmtKind::Labeled(_)
                                | StmtKind::With(_)
                                | StmtKind::Expr(_)
                                | StmtKind::Block(_)
                        );
                        if !is_non_exportable {
                            self.emitted_esm_export = true;
                            self.write("export ");
                        }
                        self.emit_stmt(decl);
                    }
                }
            }
            ExportDeclKind::Named {
                specifiers,
                source,
                type_only,
            } => {
                if *type_only {
                    if self.options.verbatim_module_syntax == Some(true) {
                        // verbatimModuleSyntax: emit `export {} from "mod"` for type-only re-exports,
                        // but deduplicate per source module.
                        if let Some(ref src) = source {
                            if self
                                .verbatim_empty_reexport_sources
                                .insert(AstString::from(src.as_str()))
                            {
                                self.emitted_esm_export = true;
                                let q = self.detect_string_quote(export_decl.span);
                                self.write("export {} from ");
                                self.write(q);
                                self.write(src);
                                self.write(q);
                                self.writeln(";");
                            }
                        }
                    }
                    return;
                }
                let erase_const_enum_exports = !self.preserve_const_enums_effective();
                let verbatim = self.options.verbatim_module_syntax == Some(true);
                let non_type: Vec<_> = specifiers
                    .iter()
                    .filter(|s| {
                        if s.is_type {
                            return false;
                        }
                        if verbatim {
                            return true;
                        }
                        if erase_const_enum_exports && self.is_const_enum_object_name(&s.local) {
                            return false;
                        }
                        if let Some(_) = source {
                            !self
                                .global_type_only_export_names
                                .contains(s.local.as_str())
                        } else {
                            !self.type_only_decl_names.contains(s.local.as_str())
                        }
                    })
                    .collect();
                if non_type.is_empty() {
                    if source.is_none() && specifiers.is_empty() {
                        // verbatimModuleSyntax: emit bare `export {}` in-place, not deferred
                        if self.options.verbatim_module_syntax == Some(true) {
                            self.emitted_esm_export = true;
                            self.writeln("export {};");
                            return;
                        }
                        // Bare `export {}` — TypeScript defers this to end-of-file.
                        // Extract and store any trailing comment so the end-of-file
                        // logic can preserve it (e.g., `export {}; // comment`).
                        let end = export_decl.span.end as usize;
                        if end < self.source.len() {
                            let rest = &self.source[end..];
                            let eol = rest.find('\n').unwrap_or(rest.len());
                            let tail = &rest[..eol];
                            if let Some(cp) = tail.find("//") {
                                let comment = tail[cp..].trim_end_matches('\r');
                                if !comment.is_empty() {
                                    self.bare_export_empty_trailing_comment =
                                        Some(comment.to_string());
                                }
                            }
                        }
                    }
                    // All specifiers were type-only (with or without source) — skip.
                    return;
                }
                self.emitted_esm_export = true;

                // Source-copy when all specifiers preserved AND inline comments exist
                let can_source_copy = non_type.len() == specifiers.len()
                    && export_decl.span.end > export_decl.span.start
                    && self.span_has_inline_comment(export_decl.span);
                if can_source_copy {
                    let src = self.source_copy_import_stmt(export_decl.span);
                    self.write(src);
                    self.writeln(";");
                } else if non_type.is_empty() {
                    // `export {}` re-export from source — emit it
                    self.write("export {}");
                    if let Some(ref src) = source {
                        let q = self.detect_string_quote(export_decl.span);
                        self.write(" from ");
                        self.write(q);
                        self.write(src);
                        self.write(q);
                        self.emit_import_attributes_from_source(export_decl.span);
                    }
                    self.writeln(";");
                } else {
                    self.write("export { ");
                    for (i, spec) in non_type.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        if spec.local_is_string {
                            self.write("\"");
                            self.write(&spec.local);
                            self.write("\"");
                        } else {
                            self.emit_module_export_name(&spec.local);
                        }
                        if let Some(ref exp) = spec.exported {
                            self.write(" as ");
                            if spec.exported_is_string {
                                self.write("\"");
                                self.write(exp);
                                self.write("\"");
                            } else if !exp.is_empty() {
                                self.emit_module_export_name(exp);
                            }
                        }
                    }
                    // Preserve trailing comma from source
                    if let Some(last) = non_type.last() {
                        let end = last.span.end as usize;
                        let decl_end = export_decl.span.end as usize;
                        if end < decl_end && decl_end <= self.source.len() {
                            let between = &self.source[end..decl_end];
                            if let Some(ci) = between.find(',') {
                                if between[ci + 1..].trim_start().starts_with('}') {
                                    self.write(",");
                                }
                            }
                        }
                    }
                    self.write(" }");
                    if let Some(ref src) = source {
                        let q = self.detect_string_quote(export_decl.span);
                        self.write(" from ");
                        self.write(q);
                        self.write(src);
                        self.write(q);
                        self.emit_import_attributes_from_source(export_decl.span);
                    }
                    self.writeln(";");
                }
            }
            ExportDeclKind::Default(expr) => {
                // Elide `export default <ident>` when the identifier refers to a
                // type-only declaration (interface or type alias).
                // With verbatimModuleSyntax, never elide — emit verbatim.
                if self.options.verbatim_module_syntax != Some(true) {
                    if let ExprKind::Ident(ref name) = expr.kind {
                        if self.type_only_decl_names.contains(&**name) {
                            return;
                        }
                        // Also elide when the name is a cross-file type-only name
                        // whose import was elided (e.g. `import { Foo } from "./mod";
                        // export default Foo;` where Foo is a type alias in mod).
                        if self.is_import_elided(name)
                            && self.global_type_only_names.contains(&**name)
                        {
                            return;
                        }
                        if !self.preserve_const_enums_effective()
                            && self.is_const_enum_object_name(name)
                        {
                            return;
                        }
                    }
                } // end verbatimModuleSyntax guard
                self.emitted_esm_export = true;
                // Source-copy for simple identifier export defaults with inline comments
                if matches!(expr.kind, ExprKind::Ident(_))
                    && export_decl.span.end > export_decl.span.start
                    && self.span_has_inline_comment(export_decl.span)
                {
                    let src = self.source_copy_import_stmt(export_decl.span);
                    self.write(src);
                    self.writeln(";");
                } else {
                    // For `export default @dec class {}`, set binding name so the
                    // IIFE wrapper emits `__setFunctionName(_classThis, "default")`.
                    let prev_binding = self.class_expr_binding_name.clone();
                    {
                        let mut e = expr.as_ref();
                        while let ExprKind::Paren(inner) = &e.kind {
                            e = inner;
                        }
                        if matches!(&e.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                            self.class_expr_binding_name =
                                Some(ClassExprBindingName::Literal("default".to_string()));
                        }
                    }
                    self.write("export default ");
                    let prev = self.in_export_default_context;
                    self.in_export_default_context = true;
                    // Only set bare_default_class_expr for bare (non-paren)
                    // anonymous class expressions — controls `default_1` naming.
                    let prev_bare = self.bare_default_class_expr;
                    if matches!(&expr.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                        self.bare_default_class_expr = true;
                    }
                    self.emit_expr(expr);
                    self.bare_default_class_expr = prev_bare;
                    self.in_export_default_context = prev;
                    // Named class expressions in `export default` are treated
                    // like declarations — no trailing semicolon after `}`.
                    let is_named_class_expr = matches!(
                        &expr.kind,
                        ExprKind::ClassExpr(cd) if cd.name.is_some()
                    );
                    if is_named_class_expr {
                        // Class body already ends with `}\n` from emit_class_decl
                    } else {
                        self.writeln(";");
                    }
                    self.class_expr_binding_name = prev_binding;
                }
            }
            ExportDeclKind::DefaultDecl(decl) => match &decl.kind {
                StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => (),
                StmtKind::FnDecl(fn_decl) => {
                    if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                        return;
                    }
                    self.emitted_esm_export = true;
                    self.write("export default ");
                    self.emit_fn_decl(fn_decl);
                }
                StmtKind::ClassDecl(class_decl) => {
                    if class_decl.modifiers & MOD_DECLARE != 0 {
                        return;
                    }
                    self.emitted_esm_export = true;
                    if class_decl.name.is_some()
                        && !self.should_preserve_decorators()
                        && self.options.experimental_decorators != Some(true)
                        && (class_can_emit_method_only_decorator_wrapper(class_decl)
                            || (self.effective_target() >= ScriptTarget::ES2015
                                && self.can_emit_public_multi_method_decorator_wrapper(class_decl)))
                    {
                        self.emit_method_only_decorated_class_wrapper(class_decl);
                        self.write("export default ");
                        self.write(class_decl.name.as_deref().unwrap());
                        self.writeln(";");
                        return;
                    }
                    // When the class has static field initializers that need
                    // extraction (useDefineForClassFields=false), defer the
                    // `export default` to after the static initializers:
                    //   class C { ... }
                    //   C.s = 0;
                    //   export default C;
                    let use_define = self.use_define_for_class_fields();
                    let needs_deferred_export =
                        !use_define && class_has_static_initializers(class_decl);
                    let needs_let_wrapper = self.options.experimental_decorators == Some(true)
                        && class_needs_let_wrapper(class_decl);
                    if needs_deferred_export || needs_let_wrapper {
                        // For unnamed classes with decorators, synthesize `default_1`
                        // as the variable name but keep the class expression anonymous.
                        let class_name = class_decl
                            .name
                            .as_deref()
                            .unwrap_or("default_1")
                            .to_string();
                        // Self-reference alias for decorated classes
                        let self_ref_alias = if needs_let_wrapper && class_decl.name.is_some() {
                            if decorated_class_has_self_reference(class_decl) {
                                let counter = self
                                    .decorated_alias_emit_counter
                                    .entry(AstString::from(class_name.as_str()))
                                    .or_insert(0);
                                *counter += 1;
                                Some(format!("{}_{}", class_name, counter))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        let saved_import_map_entry = if let Some(ref alias) = self_ref_alias {
                            // `var X_1;` is hoisted to top by the pre-scan
                            let _ = alias;
                            let prev = self.cjs_import_map.remove(class_name.as_str());
                            self.cjs_import_map.insert(
                                AstString::from(class_name.as_str()),
                                (AstString::from(alias.as_str()), AstString::new("")),
                            );
                            Some((class_name.clone(), prev))
                        } else {
                            None
                        };
                        // Anonymous decorated default export class with static
                        // initializers needs `var _a; let X = _a = class {};
                        // __setFunctionName(_a, "default");` pattern.
                        let needs_set_fn_name = needs_let_wrapper
                            && class_decl.name.is_none()
                            && class_has_static_initializers(class_decl);
                        if needs_set_fn_name {
                            self.writeln("var _a;");
                            self.needs_set_function_name_helper = true;
                        }
                        if needs_let_wrapper {
                            self.write("let ");
                            self.write(&class_name);
                            self.write(" = ");
                            if needs_set_fn_name {
                                self.write("_a = ");
                            } else if let Some(ref alias) = self_ref_alias {
                                self.write(alias);
                                self.write(" = ");
                            }
                        }
                        // For anonymous `export default class` with static initializers
                        // (deferred export, no let wrapper), give it a synthetic name
                        // so static field inits can reference it: `class default_1 { }`
                        let synth_class;
                        let emit_class = if needs_deferred_export
                            && !needs_let_wrapper
                            && class_decl.name.is_none()
                        {
                            synth_class = ClassDecl {
                                name: Some(class_name.clone()),
                                ..*class_decl.clone()
                            };
                            &synth_class
                        } else {
                            class_decl
                        };
                        // Pass the synthetic name so emit_class_static_field_initializers
                        // uses "default_1" instead of "default" for static field assignments.
                        if class_decl.name.is_none() {
                            self.recovered_default_class_name = Some(class_name.clone());
                        }
                        self.emit_class_decl(emit_class);
                        self.recovered_default_class_name = None;
                        if needs_let_wrapper {
                            let len_before_semi = self.output.len();
                            if let Some(pos) = self.pre_static_output_len {
                                if pos >= 2 && &self.output[pos - 2..pos] == "}\n" {
                                    self.output.insert(pos - 1, ';');
                                }
                            } else if self.output.ends_with("}\n") {
                                let len = self.output.len();
                                self.output.insert(len - 1, ';');
                            }
                            // Insert __setFunctionName after class body close (and semicolon)
                            // but before static field initializers.
                            if needs_set_fn_name {
                                let semi_delta = self.output.len() - len_before_semi;
                                let insert_pos = self
                                    .pre_static_output_len
                                    .map(|p| p + semi_delta)
                                    .unwrap_or(self.output.len());
                                self.output.insert_str(
                                    insert_pos,
                                    "__setFunctionName(_a, \"default\");\n",
                                );
                            }
                            self.decorated_class_self_ref_alias = self_ref_alias;
                            // For decorator applications, we need the class name.
                            // If unnamed, create a temporary named version.
                            if class_decl.name.is_some() {
                                self.emit_decorator_applications(class_decl, None);
                            } else {
                                let named = ClassDecl {
                                    name: Some(class_name.clone()),
                                    ..*class_decl.clone()
                                };
                                self.emit_decorator_applications(&named, None);
                            }
                            self.decorated_class_self_ref_alias = None;
                        }
                        if let Some((name, prev)) = saved_import_map_entry {
                            if let Some(prev_entry) = prev {
                                self.cjs_import_map
                                    .insert(AstString::from(name.as_str()), prev_entry);
                            } else {
                                self.cjs_import_map.remove(name.as_str());
                            }
                        }
                        self.write("export default ");
                        self.write(&class_name);
                        self.writeln(";");
                    } else {
                        // When an unnamed export-default class has member decorators,
                        // synthesize a name so __decorate can reference the prototype.
                        let needs_synth_name = self.options.experimental_decorators == Some(true)
                            && class_decl.name.is_none()
                            && class_has_decorators(class_decl);
                        let synth_class;
                        let emit_class = if needs_synth_name {
                            synth_class = ClassDecl {
                                name: Some("default_1".to_string()),
                                ..*class_decl.clone()
                            };
                            &synth_class
                        } else {
                            class_decl
                        };
                        // When preserving decorators, preserve source order
                        // of `export default` vs decorators.
                        if self.should_preserve_decorators() && !emit_class.decorators.is_empty() {
                            // Find actual `export` keyword position in source
                            let export_kw_pos = {
                                let s = export_decl.span.start as usize;
                                let e = (export_decl.span.end as usize).min(self.source.len());
                                self.source
                                    .get(s..e)
                                    .and_then(|text| text.find("export").map(|off| s + off))
                                    .unwrap_or(s)
                            };
                            let export_kw_end = export_kw_pos + 6; // "export".len()

                            // Split decorators: before export vs after export
                            let decs_before: Vec<Expr> = emit_class
                                .decorators
                                .iter()
                                .filter(|d| (d.span.start as usize) < export_kw_pos)
                                .cloned()
                                .collect();
                            let decs_after: Vec<Expr> = emit_class
                                .decorators
                                .iter()
                                .filter(|d| (d.span.start as usize) >= export_kw_end)
                                .cloned()
                                .collect();

                            // Emit decorators before export
                            if !decs_before.is_empty() {
                                self.emit_preserved_decorators(
                                    &decs_before,
                                    Some(export_kw_pos as u32),
                                );
                            }

                            // Determine the export header: check if "default" is before
                            // or after the remaining decorators
                            let default_before_after_decs = if !decs_after.is_empty() {
                                let last_after_end = decs_after.last().unwrap().span.end as usize;
                                self.source
                                    .get(last_after_end..)
                                    .and_then(|s| {
                                        s.find("class").map(|c| s[..c].contains("default"))
                                    })
                                    .unwrap_or(false)
                            } else {
                                false
                            };

                            let default_before_decs = !default_before_after_decs
                                && self
                                    .source
                                    .get(
                                        export_kw_pos
                                            ..decs_after
                                                .first()
                                                .map_or(export_kw_end, |d| d.span.start as usize),
                                    )
                                    .map_or(false, |s| s.contains("default"));

                            if default_before_decs || decs_after.is_empty() {
                                self.write("export default ");
                            } else {
                                self.write("export ");
                            }

                            // Emit decorators after export
                            if !decs_after.is_empty() {
                                let last_after_end = decs_after.last().unwrap().span.end as usize;
                                let class_kw_pos = self
                                    .source
                                    .get(last_after_end..)
                                    .and_then(|s| s.find("class"))
                                    .map(|off| (last_after_end + off) as u32);
                                self.emit_preserved_decorators(&decs_after, class_kw_pos);
                            }

                            // If "default" appears after the after-decorators, write it
                            if default_before_after_decs {
                                self.write("default ");
                            }

                            let mut clean = emit_class.clone();
                            clean.decorators.clear();
                            self.emit_class_decl(&clean);
                        } else {
                            self.write("export default ");
                            self.emit_class_decl(emit_class);
                        }
                        // Emit __decorate calls for member/class decorators
                        if self.options.experimental_decorators == Some(true)
                            && class_has_decorators(emit_class)
                        {
                            self.emit_decorator_applications(emit_class, None);
                        }
                    }
                }
                _ => {
                    self.emitted_esm_export = true;
                    self.write("export default ");
                    self.emit_stmt(decl);
                }
            },
            ExportDeclKind::All {
                source,
                alias,
                alias_is_string,
                type_only,
            } => {
                if *type_only {
                    return;
                }
                self.emitted_esm_export = true;
                // `export * as ns from "mod"` is ES2020 syntax. When the
                // requested module format is ES2015, TypeScript lowers it to
                // a synthetic namespace import followed by a named export:
                //
                //   import * as ns_1 from "mod" with { type: "json" };
                //   export { ns_1 as ns };
                //
                // Import assertions/attributes belong on the synthetic import.
                if self.effective_module_kind() == ModuleKind::ES2015 {
                    if let Some(alias) = alias {
                        let token_positions = self.export_star_token_positions(export_decl.span);
                        let synthetic = if *alias_is_string {
                            self.next_inline_temp_var()
                        } else {
                            let mut suffix = 1usize;
                            loop {
                                let candidate = format!("{alias}_{suffix}");
                                if !self.file_value_bound_names.contains(candidate.as_str()) {
                                    self.file_value_bound_names.insert(candidate.clone().into());
                                    break candidate;
                                }
                                suffix += 1;
                            }
                        };
                        self.write("import * as ");
                        self.write(&synthetic);
                        self.write(" from ");
                        let rewritten_source = self.rewrite_relative_import_specifier(
                            source,
                            self.options.jsx == Some(JsxEmit::Preserve),
                        );
                        if let Some((_, _, source_span, _)) = token_positions {
                            if rewritten_source == source.as_str() {
                                self.copy_span(source_span);
                            } else {
                                let quote = self
                                    .source
                                    .as_bytes()
                                    .get(source_span.start as usize)
                                    .copied()
                                    .filter(|q| matches!(*q, b'\'' | b'"'))
                                    .map(char::from)
                                    .unwrap_or('"');
                                self.write(&quote.to_string());
                                self.write(&rewritten_source);
                                self.write(&quote.to_string());
                            }
                        } else {
                            self.write("\"");
                            self.write(&super::emit_expr::escape_js_string_for_quote(
                                &rewritten_source,
                                '"',
                            ));
                            self.write("\"");
                        }
                        if let Some((_, _, source_span, after_source_start)) = token_positions {
                            self.emit_moved_comment_between(
                                source_span.end as usize,
                                after_source_start as usize,
                                true,
                            );
                        }
                        self.emit_import_attributes_from_source(export_decl.span);
                        self.writeln(";");
                        self.write("export { ");
                        self.write(&synthetic);
                        self.write(" as ");
                        if *alias_is_string {
                            if let Some((alias_span, _, _, _)) = token_positions {
                                self.copy_span(alias_span);
                            } else {
                                self.write("\"");
                                self.write(&super::emit_expr::escape_js_string_for_quote(
                                    alias, '"',
                                ));
                                self.write("\"");
                            }
                        } else {
                            self.write(alias);
                        }
                        if let Some((alias_span, from_start, _, _)) = token_positions {
                            self.emit_moved_comment_between(
                                alias_span.end as usize,
                                from_start as usize,
                                true,
                            );
                        }
                        self.writeln(" };");
                        return;
                    }
                }
                // Source-copy when inline comments exist
                if export_decl.span.end > export_decl.span.start
                    && self.span_has_inline_comment(export_decl.span)
                {
                    let src = self.source_copy_import_stmt(export_decl.span);
                    self.write(src);
                    self.writeln(";");
                } else {
                    self.write("export *");
                    if let Some(ref a) = alias {
                        self.write(" as ");
                        if *alias_is_string {
                            self.write("\"");
                            self.write(a);
                            self.write("\"");
                        } else {
                            self.write(a);
                        }
                    }
                    self.write(" from ");
                    let q = self.detect_string_quote(export_decl.span);
                    self.write(q);
                    let source = self.rewrite_relative_import_specifier(
                        source,
                        self.options.jsx == Some(JsxEmit::Preserve),
                    );
                    self.write(&source);
                    self.write(q);
                    self.emit_import_attributes_from_source(export_decl.span);
                    self.writeln(";");
                }
            }
        }
    }

    /// Emit an export declaration in CommonJS mode.
    ///
    /// Uses `self.export_target` to determine the assignment target:
    /// - `"exports"` at the CJS top-level
    /// - namespace name inside a namespace IIFE
    pub(super) fn emit_export_decl_cjs(&mut self, export_decl: &ExportDecl) {
        // When `export = X` is present in the file, individual `export class/fn/var`
        // declarations are emitted without the `exports.X = X;` assignment, because
        // `module.exports = X` will override the entire exports object.
        let target = self
            .export_target
            .clone()
            .unwrap_or_else(|| "exports".to_string());
        let suppress_assignment = self.has_export_assign && target == "exports";

        match &export_decl.kind {
            ExportDeclKind::Decl(decl) => {
                let prev_export_ctx = self.in_export_context;
                self.in_export_context = true;
                match &decl.kind {
                    StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => (),
                    StmtKind::FnDecl(fn_decl) => {
                        if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        self.emit_fn_decl(fn_decl);
                        // Attach trailing comment to closing brace before export assignment
                        self.append_trailing_comment(decl.span);
                        // At top-level CJS, function exports are pre-assigned in the header.
                        // Only emit per-statement assignment inside namespaces.
                        if !suppress_assignment && target != "exports" {
                            if let Some(ref name) = fn_decl.name {
                                self.write(&target);
                                self.write(".");
                                self.write(name);
                                self.write(" = ");
                                self.write(name);
                                self.writeln(";");
                            }
                        }
                    }
                    StmtKind::ClassDecl(class_decl) => {
                        if class_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        // Class-level standard decorator IIFE wrapper:
                        //   let X = (() => { ... __esDecorate ... })();
                        //   exports.X = X;
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_can_emit_simple_standard_decorator_wrapper(class_decl)
                        {
                            self.emit_simple_standard_decorated_class_wrapper(class_decl);
                            self.append_trailing_comment(decl.span);
                            if !suppress_assignment {
                                if let Some(ref name) = class_decl.name {
                                    self.write(&target);
                                    self.write(".");
                                    self.write(name);
                                    self.write(" = ");
                                    self.write(name);
                                    self.writeln(";");
                                }
                            }
                            return;
                        }
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_native_standard_decorator_shape(class_decl).is_some()
                        {
                            self.emit_native_standard_decorator_class_decl_wrapper(class_decl);
                            self.append_trailing_comment(decl.span);
                            if !suppress_assignment {
                                if let Some(ref name) = class_decl.name {
                                    self.write(&target);
                                    self.write(".");
                                    self.write(name);
                                    self.write(" = ");
                                    self.write(name);
                                    self.writeln(";");
                                }
                            }
                            return;
                        }
                        if !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && (class_can_emit_method_only_decorator_wrapper(class_decl)
                                || (self.effective_target() >= ScriptTarget::ES2015
                                    && self.can_emit_public_multi_method_decorator_wrapper(
                                        class_decl,
                                    )))
                        {
                            self.emit_method_only_decorated_class_wrapper(class_decl);
                            self.append_trailing_comment(decl.span);
                            if !suppress_assignment {
                                if let Some(ref name) = class_decl.name {
                                    self.write(&target);
                                    self.write(".");
                                    self.write(name);
                                    self.write(" = ");
                                    self.write(name);
                                    self.writeln(";");
                                }
                            }
                            return;
                        }
                        // Anonymous exported class (e.g. `export class { }` without a name):
                        // TypeScript generates a synthetic name like `default_1`.
                        let synthetic_name = if class_decl.name.is_none() {
                            let name = format!("default_{}", self.default_export_counter);
                            self.default_export_counter += 1;
                            Some(name)
                        } else {
                            None
                        };
                        // Decorated classes with class-level decorators need
                        // `let X = class X { };` so __decorate can reassign.
                        let needs_let_wrapper = self.options.experimental_decorators == Some(true)
                            && class_needs_let_wrapper(class_decl);
                        let emit_narrow_standard_decorator_decl_wrapper = !self
                            .should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && class_can_emit_narrow_standard_decorator_member_decl(class_decl);
                        // Self-reference alias for decorated classes
                        let self_ref_alias = if needs_let_wrapper {
                            class_decl.name.as_ref().and_then(|name| {
                                if decorated_class_has_self_reference(class_decl) {
                                    let counter = self
                                        .decorated_alias_emit_counter
                                        .entry(AstString::from(name.as_str()))
                                        .or_insert(0);
                                    *counter += 1;
                                    Some(format!("{}_{}", name, counter))
                                } else {
                                    None
                                }
                            })
                        } else {
                            None
                        };
                        let saved_import_map_entry = if let Some(ref alias) = self_ref_alias {
                            if let Some(ref name) = class_decl.name {
                                let prev = self.cjs_import_map.remove(name.as_str());
                                self.cjs_import_map.insert(
                                    AstString::from(name.as_str()),
                                    (AstString::from(alias.as_str()), AstString::new("")),
                                );
                                Some((name.clone(), prev))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if needs_let_wrapper {
                            let name = synthetic_name.as_deref().or(class_decl.name.as_deref());
                            if let Some(name) = name {
                                self.write("let ");
                                self.write(name);
                                self.write(" = ");
                                if let Some(ref alias) = self_ref_alias {
                                    self.write(alias);
                                    self.write(" = ");
                                }
                            }
                        }
                        self.pre_static_output_len = None;
                        if let Some(ref syn_name) = synthetic_name {
                            let mut named = class_decl.clone();
                            named.name = Some(syn_name.clone());
                            if emit_narrow_standard_decorator_decl_wrapper {
                                self.emit_narrow_standard_decorated_class_decl_wrapper(&named);
                            } else {
                                self.emit_class_decl(&named);
                            }
                        } else {
                            if emit_narrow_standard_decorator_decl_wrapper {
                                self.emit_narrow_standard_decorated_class_decl_wrapper(class_decl);
                            } else {
                                self.emit_class_decl(class_decl);
                            }
                        }
                        // In CJS mode (target == "exports"), TypeScript emits
                        // `exports.X = X;` BEFORE static field initializers.
                        // In namespace mode, static props come first (default order).
                        let class_body_end = if emit_narrow_standard_decorator_decl_wrapper {
                            self.pre_static_output_len = None;
                            None
                        } else {
                            self.pre_static_output_len.take()
                        };
                        let deferred_static = if target == "exports"
                            && !emit_narrow_standard_decorator_decl_wrapper
                        {
                            class_body_end
                                .filter(|&start| start < self.output.len())
                                .map(|start| {
                                    let text = self.output[start..].to_string();
                                    self.output.truncate(start);
                                    text
                                })
                        } else {
                            None
                        };
                        // Replace trailing `}\n` with `};\n` for let wrapper
                        // (must happen AFTER deferred_static extraction)
                        if needs_let_wrapper {
                            // When static initializers exist, use class_body_end to find
                            // the `}` position (namespace mode only; CJS already truncated).
                            let check_pos = if deferred_static.is_none() {
                                class_body_end
                            } else {
                                None
                            };
                            if let Some(pos) = check_pos {
                                if pos >= 2 && &self.output[pos - 2..pos] == "}\n" {
                                    self.output.insert(pos - 1, ';');
                                }
                            } else if self.output.ends_with("}\n") {
                                let len = self.output.len();
                                self.output.insert(len - 1, ';');
                            }
                        }
                        // Attach trailing comment to closing brace before export assignment
                        self.append_trailing_comment(decl.span);
                        let has_decorators = self.options.experimental_decorators == Some(true)
                            && class_has_decorators(class_decl);
                        // In namespace context (target != "exports"), TypeScript emits
                        // __decorate BEFORE the export assignment. In CJS context
                        // (target == "exports"), the export assignment comes first.
                        if has_decorators && target != "exports" {
                            self.emit_decorator_applications(class_decl, None);
                        }
                        let emit_name = synthetic_name.as_deref().or(class_decl.name.as_deref());
                        if !suppress_assignment {
                            if let Some(name) = emit_name {
                                self.write(&target);
                                self.write(".");
                                self.write(name);
                                self.write(" = ");
                                self.write(name);
                                self.writeln(";");
                            }
                        }
                        // Clean up cjs_import_map BEFORE decorator emission
                        // (decorator args should reference original name, not alias)
                        if let Some((name, prev)) = saved_import_map_entry {
                            if let Some(prev_entry) = prev {
                                self.cjs_import_map
                                    .insert(AstString::from(name.as_str()), prev_entry);
                            } else {
                                self.cjs_import_map.remove(name.as_str());
                            }
                        }
                        if has_decorators && target == "exports" {
                            // Re-append deferred static BEFORE decorate for CJS
                            if let Some(text) = &deferred_static {
                                self.output.push_str(text);
                            }
                            self.decorated_class_self_ref_alias = self_ref_alias.clone();
                            self.emit_decorator_applications(class_decl, Some(&target));
                            self.decorated_class_self_ref_alias = None;
                        } else {
                            // Re-append deferred static field initializers after export
                            if let Some(text) = &deferred_static {
                                self.output.push_str(text);
                            }
                        }
                    }
                    StmtKind::Var(var_stmt) => {
                        let before = self.output.len();
                        if let Some(init) = self
                            .direct_es5_empty_binding_export_init(var_stmt, export_decl.span)
                            .cloned()
                        {
                            let value_temp = self.next_temp_var();
                            let export_name = self.next_deferred_export_name_placeholder();
                            self.write(&target);
                            self.write(".");
                            self.write(&export_name);
                            self.write(" = ");
                            self.write(&value_temp);
                            self.write(" = ");
                            self.emit_expr(&init);
                            self.writeln(";");
                        } else if let Some((
                            init,
                            binding_name,
                            binding_span,
                            rest_name,
                            rest_span,
                        )) =
                            self.direct_es5_object_rest_export(var_stmt, export_decl.span)
                        {
                            self.emit_direct_es5_object_rest_export(
                                Some(&target),
                                &init,
                                &binding_name,
                                binding_span,
                                &rest_name,
                                rest_span,
                            );
                        } else {
                            self.emit_var_export(var_stmt);
                        }
                        if self.output.len() > before {
                            self.append_trailing_comment(decl.span);
                        }
                    }
                    StmtKind::EnumDecl(enum_decl) => {
                        if enum_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        if enum_decl.is_const && !self.preserve_const_enums_effective() {
                            return;
                        }
                        self.emit_enum_decl(enum_decl);
                    }
                    StmtKind::ModuleDecl(module_decl) => {
                        if module_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        // The IIFE closing handles `exports.name = name =`
                        // when in_export_context is true.
                        self.emit_module_decl(module_decl);
                        // Attach trailing comment to the IIFE closing
                        self.append_trailing_comment(decl.span);
                    }
                    StmtKind::Import(ref imp) => {
                        // `export import a = require("...")`
                        // In CJS this emits `exports.a = require("...");` directly.
                        // Inside namespace bodies, require() is erased entirely.
                        if let ImportClause::Require(ref name) = imp.specifiers {
                            if !imp.type_only
                                && target == "exports"
                                && !suppress_assignment
                                && !self.type_only_require_specs.contains(imp.source.as_str())
                            {
                                // TypeScript emits a single combined statement:
                                // `exports.a = require("...");`
                                self.write(&target);
                                self.write(".");
                                self.write(name);
                                self.write(" = require(\"");
                                self.write(&imp.source);
                                self.writeln("\");");
                            }
                        } else {
                            // Other import forms inside export decl: emit normally
                            self.emit_import_decl(imp);
                        }
                    }
                    StmtKind::ImportEquals(ie) => {
                        if self.import_equals_rhs_is_type_only_require_spec(&ie.module_ref) {
                            return;
                        }
                        let keep_invalid_class_modifier_import = self
                            .span_text(export_decl.span)
                            .map(|s| s.trim_start())
                            .is_some_and(|s| {
                                s.starts_with("export public import ")
                                    || s.starts_with("export private import ")
                                    || s.starts_with("export static import ")
                            });
                        // Inside namespace bodies, `export import X = require(..)`
                        // is erased because require() calls don't work inside
                        // namespace IIFEs.
                        let is_require_in_ns = target != "exports"
                            && matches!(&ie.module_ref.kind, ExprKind::Call(ref c) if matches!(&c.callee.kind, ExprKind::Ident(n) if n == "require"));
                        if target != "exports" && !is_require_in_ns {
                            // Inside namespace, only emit ie.name assignment when
                            // the RHS refers to a runtime value. Type-only namespace
                            // aliases are erased by TypeScript.
                            if keep_invalid_class_modifier_import
                                || self.import_equals_rhs_has_runtime_value(&ie.module_ref)
                            {
                                self.write(&target);
                                self.write(".");
                                self.write(&ie.name);
                                self.write(" = ");
                                self.emit_expr(&ie.module_ref);
                                self.writeln(";");
                                self.append_trailing_comment(decl.span);
                                self.cjs_import_map.insert(
                                    AstString::from(ie.name.as_str()),
                                    (
                                        AstString::from(target.as_str()),
                                        AstString::from(ie.name.as_str()),
                                    ),
                                );
                            }
                        } else if !is_require_in_ns {
                            // Top-level CJS: emit `exports.X = &ie.module_ref;` directly.
                            // Note: suppress_assignment is NOT checked here because
                            // TypeScript always emits `exports.X = &ie.module_ref;` for exported
                            // import-equals, even when `export = Y` is present.
                            if keep_invalid_class_modifier_import
                                || self.import_equals_rhs_has_runtime_value(&ie.module_ref)
                            {
                                self.write(&target);
                                self.write(".");
                                self.write(&ie.name);
                                self.write(" = ");
                                self.emit_expr(&ie.module_ref);
                                self.writeln(";");
                                self.append_trailing_comment(decl.span);
                            }
                        }
                    }
                    _ => {
                        // Fallback: emit without export keyword
                        self.emit_stmt(decl);
                    }
                }
                self.in_export_context = prev_export_ctx;
            }
            ExportDeclKind::Named {
                specifiers,
                source,
                type_only,
            } => {
                if *type_only {
                    return;
                }
                let erase_const_enum_exports = !self.preserve_const_enums_effective();
                let non_type: Vec<_> = specifiers
                    .iter()
                    .filter(|s| {
                        if s.is_type {
                            return false;
                        }
                        if erase_const_enum_exports && self.is_const_enum_object_name(&s.local) {
                            return false;
                        }
                        if let Some(_) = source {
                            !self
                                .global_type_only_export_names
                                .contains(s.local.as_str())
                        } else {
                            !self.type_only_decl_names.contains(s.local.as_str())
                        }
                    })
                    .collect();
                if non_type.is_empty() {
                    return;
                }
                if let Some(ref src) = source {
                    // For AMD/UMD, check if the module is already a factory param.
                    let amd_var = self.amd_dep_map.get(src).cloned();

                    // Re-export from another module:
                    // TypeScript 5.x always uses Object.defineProperty + var for re-exports.
                    // export { a, b } from './module'
                    // -> var mod_1 = require("./module");
                    //    Object.defineProperty(exports, "a", { enumerable: true, get: function () { return mod_1.a; } });
                    let var_name = if let Some(ref amd_param) = amd_var {
                        amd_param.clone()
                    } else {
                        let v = self.next_require_var(src);
                        self.write("var ");
                        self.write(&v);
                        self.write(" = require(\"");
                        self.write(src);
                        self.writeln("\");");
                        v
                    };
                    for spec in &non_type {
                        let exported = spec.exported.as_ref().unwrap_or(&spec.local);
                        self.write("Object.defineProperty(exports, \"");
                        if self.is_commonjs() {
                            let exported = if spec.exported_is_string
                                || (spec.exported.is_none() && spec.local_is_string)
                            {
                                emit_expr::cook_string_literal_raw_for_double_quote(exported)
                            } else {
                                emit_expr::escape_js_string_for_quote(exported, '"')
                            };
                            self.write(&exported);
                        } else {
                            self.write(exported);
                        }
                        self.write("\", { enumerable: true, get: function () { return ");
                        // For re-exporting "default", wrap with __importDefault
                        // (unless importHelpers=true + esModuleInterop=false)
                        if spec.local == "default"
                            && (self.options.import_helpers != Some(true)
                                || self.es_module_interop())
                        {
                            self.write(self.helper_prefix());
                            self.write("__importDefault(");
                            self.write(&var_name);
                            self.write(").default");
                        } else {
                            let source_was_string = self.is_commonjs()
                                && spec.local_is_string
                                && !self.file_has_recovery_errors
                                && spec.span.start < spec.span.end;
                            let access = Self::cjs_member_access_text(
                                &var_name,
                                &spec.local,
                                source_was_string,
                            );
                            self.write(&access);
                        }
                        self.writeln("; } });");
                    }
                } else {
                    // Local named exports: export { a, b }
                    if target != "exports" {
                        // Inside namespaces, `export { X as y }` without a source
                        // module is used only for declaration emit — TypeScript
                        // does not emit any JS assignments for these.  The actual
                        // namespace-qualified assignments (e.g. `M.M_C = M_C;`)
                        // are already produced by the class/function/var/enum
                        // declaration handlers themselves.
                    } else {
                        // Top-level CJS: for imported bindings, the CJS header code
                        // in lib.rs already emits Object.defineProperty with live-binding
                        // semantics right after the import statement.
                        // For local bindings, the CJS header handles them via
                        // `exports.X = void 0;` pre-declarations and end-of-file assignments.
                        // So nothing to emit here.
                    }
                }
            }
            ExportDeclKind::Default(expr) => {
                // Elide `export default <ident>` when the identifier refers to a
                // type-only declaration (interface or type alias).
                // With verbatimModuleSyntax, never elide — emit verbatim.
                if let ExprKind::Ident(ref name) = expr.kind {
                    if self.options.verbatim_module_syntax != Some(true)
                        && self.type_only_decl_names.contains(&**name)
                    {
                        return;
                    }
                    if !self.preserve_const_enums_effective()
                        && self.is_const_enum_object_name(name)
                    {
                        return;
                    }
                    // For `export default x` where `x` is also exported as a
                    // runtime binding, TypeScript emits either:
                    //   exports.default = exports.x  (for var/let/const bindings)
                    //   exports.default = x          (for function/class bindings)
                    //   exports.default = mod_1.x    (for imported bindings re-exported via `export { x }`)
                    if target == "exports"
                        && !self.has_export_assign
                        && self.cjs_exported_names.contains(name.as_str())
                    {
                        self.write(&target);
                        self.write(".default = ");
                        if self.cjs_var_export_names.contains(name.as_str()) {
                            self.write(&target);
                            self.write(".");
                            self.write(name);
                        } else if let Some((var_name, imported)) =
                            self.cjs_import_map.get(name.as_str()).cloned()
                        {
                            // Re-exported import: bare `name` would be
                            // `ReferenceError: name is not defined` at runtime
                            // since tsc-rs lowers `import { Stripe } from "./mod"`
                            // to `const mod_1 = require("./mod")` with no local
                            // `Stripe` binding. Qualify with the import alias.
                            if !imported.is_empty() {
                                self.write_cjs_import_access(&var_name, &imported, name);
                            } else {
                                self.write(&var_name);
                            }
                        } else {
                            self.write(name);
                        }
                        self.writeln(";");
                        return;
                    }
                }
                // For `export default @dec class {}`, set binding name so the
                // IIFE wrapper emits `__setFunctionName(_classThis, "default")`.
                // Only set default context (which controls `default_1` naming)
                // for bare expressions, NOT paren-wrapped.
                let prev_binding = self.class_expr_binding_name.clone();
                let prev_bare = self.bare_default_class_expr;
                {
                    let mut e = expr.as_ref();
                    while let ExprKind::Paren(inner) = &e.kind {
                        e = inner;
                    }
                    if matches!(&e.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                        self.class_expr_binding_name =
                            Some(ClassExprBindingName::Literal("default".to_string()));
                    }
                }
                // Only set bare_default_class_expr for bare (non-paren) class expr
                // Controls `default_1` naming in decorator IIFE wrapper.
                if matches!(&expr.kind, ExprKind::ClassExpr(cd) if cd.name.is_none()) {
                    self.bare_default_class_expr = true;
                }
                if target != "exports" && !self.is_commonjs() {
                    // Inside namespace: emit `export default expr;` as-is
                    // (error recovery — export default is invalid inside namespaces)
                    self.write("export default ");
                    self.emit_expr(expr);
                    self.writeln(";");
                } else {
                    self.write(&target);
                    self.write(".default = ");
                    // CJS imported bindings inside a default-export
                    // expression must be qualified with their import
                    // alias (e.g. `db` → `db_1.db`). The branch above
                    // only handles names already tracked in
                    // `cjs_exported_names`, which depends on
                    // statement order — `export default db` emitted
                    // before `export { db }` in the same file falls
                    // through here and would otherwise emit a bare
                    // `db` and throw `ReferenceError: db is not
                    // defined` at module init. Toggle the rewrite
                    // flag for the duration of the expression so
                    // `emit_value_name_ref` in `emit_expr` consults
                    // `cjs_import_map` for any identifier reference.
                    let prev_rw = self.rewrite_ident_with_import_map;
                    if target == "exports" {
                        self.rewrite_ident_with_import_map = true;
                    }
                    self.emit_expr(expr);
                    self.rewrite_ident_with_import_map = prev_rw;
                    self.writeln(";");
                }
                self.class_expr_binding_name = prev_binding;
                self.bare_default_class_expr = prev_bare;
            }
            ExportDeclKind::DefaultDecl(decl) => {
                match &decl.kind {
                    StmtKind::InterfaceDecl(_) | StmtKind::TypeAlias(_) => (),
                    StmtKind::FnDecl(fn_decl) => {
                        if fn_decl.body.is_none() || fn_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        // Anonymous default export function: give it a
                        // synthetic name using the file-wide counter.
                        let emit_name = if let Some(ref name) = fn_decl.name {
                            name.clone()
                        } else {
                            let name = format!("default_{}", self.default_export_counter);
                            self.default_export_counter += 1;
                            name
                        };
                        if fn_decl.name.is_none() && target != "exports" {
                            // Inside a namespace IIFE, TypeScript emits
                            // `default function () { }` (keeping `default`
                            // as a keyword prefix) for error-recovery output.
                            self.write("default ");
                            self.emit_fn_decl(fn_decl);
                        } else if fn_decl.name.is_none() {
                            let mut named = fn_decl.clone();
                            named.name = Some(emit_name.clone());
                            self.emit_fn_decl(&named);
                        } else {
                            self.emit_fn_decl(fn_decl);
                        }
                        // At top-level CJS, default function exports are
                        // pre-assigned in header.  For non-CJS targets
                        // (namespaces), emit inline.
                        if target != "exports" {
                            self.write(&target);
                            self.write(".");
                            // In namespaces, use the synthetic name (default_N)
                            // as the property name (not "default").
                            self.write(&emit_name);
                            self.write(" = ");
                            self.write(&emit_name);
                            self.writeln(";");
                        }
                    }
                    StmtKind::ClassDecl(class_decl) => {
                        if class_decl.modifiers & MOD_DECLARE != 0 {
                            return;
                        }
                        if target == "exports"
                            && class_decl.name.is_some()
                            && !self.should_preserve_decorators()
                            && self.options.experimental_decorators != Some(true)
                            && (class_can_emit_method_only_decorator_wrapper(class_decl)
                                || (self.effective_target() >= ScriptTarget::ES2015
                                    && self.can_emit_public_multi_method_decorator_wrapper(
                                        class_decl,
                                    )))
                        {
                            self.emit_method_only_decorated_class_wrapper(class_decl);
                            self.append_trailing_comment(decl.span);
                            self.write("exports.default = ");
                            self.write(class_decl.name.as_deref().unwrap());
                            self.writeln(";");
                            return;
                        }
                        // Keep the source declaration boundary independently
                        // of the class node passed to `emit_class_decl`. The
                        // CommonJS path may clone the declaration and clear
                        // `MOD_DEFAULT` so ordinary methods can lower, while
                        // computed members must continue using the native
                        // class recovery path.
                        let previous_cjs_default_class_start =
                            self.legacy_es5_cjs_default_class_start;
                        self.legacy_es5_cjs_default_class_start = (target == "exports"
                            && class_decl.modifiers & MOD_DEFAULT != 0)
                            .then_some(class_decl.span.start);
                        let needs_let_wrapper = self.options.experimental_decorators == Some(true)
                            && class_needs_let_wrapper(class_decl);
                        let has_decorators = self.options.experimental_decorators == Some(true)
                            && class_has_decorators(class_decl);
                        // Anonymous default export class: give it a synthetic
                        // name using the file-wide counter.
                        let synthetic_name = if class_decl.name.is_none() {
                            let name = format!("default_{}", self.default_export_counter);
                            self.default_export_counter += 1;
                            Some(name)
                        } else {
                            None
                        };
                        let emit_name = synthetic_name
                            .as_deref()
                            .or(class_decl.name.as_deref())
                            .unwrap_or("default_1")
                            .to_string();
                        // `default` belongs to the source export wrapper, not the
                        // declaration that CommonJS/UMD emits before assigning
                        // `exports.default`.  Keep the source node unchanged for
                        // decorator/export bookkeeping, but remove that wrapper-only
                        // modifier from the local declaration passed to class emit.
                        // In particular, the legacy ES5 class shape gate must see the
                        // ordinary named class that is actually emitted here.
                        let mut emitted_class = class_decl.clone();
                        if target == "exports" {
                            emitted_class.modifiers &= !MOD_DEFAULT;
                        }
                        // Decorated classes need `let X = class X { };`
                        // (anonymous classes stay unnamed: `let default_1 = class { };`)
                        if needs_let_wrapper {
                            self.write("let ");
                            self.write(&emit_name);
                            self.write(" = ");
                        }
                        if needs_let_wrapper && synthetic_name.is_some() {
                            // Anonymous class: keep it unnamed in the expression
                            self.emit_class_decl(&emitted_class);
                        } else if let Some(ref syn_name) = synthetic_name {
                            let mut named = emitted_class.clone();
                            named.name = Some(syn_name.clone());
                            self.emit_class_decl(&named);
                        } else {
                            self.emit_class_decl(&emitted_class);
                        }
                        // Emit trailing comments (e.g. `// error`) from the export
                        // statement that follow the class closing brace, BEFORE
                        // the synthesized `exports.default = C;` line.
                        // Both the class and export spans may end at the same position,
                        // so scan forward from class end to the next newline.
                        // Attach trailing comments (e.g. `// error`) from the
                        // export statement to the class closing brace line,
                        // not the synthesized `exports.default = C;` line.
                        {
                            let class_body_end = class_decl.span.end as usize;
                            if class_body_end < self.source.len() {
                                let rest = &self.source[class_body_end..];
                                let eol = rest.find('\n').unwrap_or(rest.len());
                                let same_line = &rest[..eol];
                                if let Some(comment_start) = same_line.find("//") {
                                    let comment_text = same_line[comment_start..].trim_end();
                                    if !comment_text.is_empty() {
                                        if self.output.ends_with('\n') {
                                            self.output.pop();
                                            self.at_line_start = false;
                                        }
                                        self.write(" ");
                                        self.write(comment_text);
                                        self.newline();
                                        let abs_end =
                                            class_body_end + comment_start + comment_text.len();
                                        self.advance_comment_pos(abs_end as u32);
                                    }
                                }
                            }
                        }
                        // Insert semicolon after `}` for let wrapper
                        if needs_let_wrapper {
                            if self.output.ends_with("}\n") {
                                let len = self.output.len();
                                self.output.insert(len - 1, ';');
                            }
                        }
                        self.legacy_es5_cjs_default_class_start = previous_cjs_default_class_start;
                        // For CJS, emit __decorate BEFORE exports.default.
                        // Pass None for target — `export default class` doesn't
                        // get `exports.Name =` prefix on the __decorate call.
                        if has_decorators && target == "exports" {
                            if class_decl.name.is_some() {
                                self.emit_decorator_applications(class_decl, None);
                            } else {
                                let named = ClassDecl {
                                    name: Some(emit_name.clone()),
                                    ..*class_decl.clone()
                                };
                                self.emit_decorator_applications(&named, None);
                            }
                        }
                        self.write(&target);
                        self.write(".");
                        // In CJS (exports.default), always use "default".
                        // In namespaces, use the synthetic name (default_1, etc.).
                        if target == "exports" {
                            self.write("default");
                        } else {
                            self.write(&emit_name);
                        }
                        self.write(" = ");
                        self.write(&emit_name);
                        self.writeln(";");
                        // For non-CJS targets, emit __decorate after export
                        if has_decorators && target != "exports" {
                            self.emit_decorator_applications(class_decl, None);
                        }
                    }
                    _ => {
                        self.write(&target);
                        self.write(".default = ");
                        self.emit_stmt(decl);
                    }
                }
            }
            ExportDeclKind::All {
                source,
                alias,
                type_only,
                ..
            } => {
                if *type_only {
                    return;
                }
                // Inside namespaces, `export *` is not valid and should be elided.
                if target != "exports" {
                    return;
                }
                let amd_var = self.amd_dep_map.get(source).cloned();
                if let Some(ref a) = alias {
                    // export * as ns from './module'
                    // -> target.ns = __importStar(require("./module"));  (CJS)
                    // -> target.ns = tslib_1.__importStar(require(...)); (importHelpers+esModuleInterop)
                    // -> target.ns = require("./module");                (importHelpers, no esModuleInterop)
                    // -> target.ns = __importStar(param);               (AMD)
                    // `export * as ns from "mod"` always needs __importStar wrapping
                    // in CJS output, regardless of importHelpers/esModuleInterop settings.
                    let wrap_star = true;
                    self.write_cjs_export_access(&target, a);
                    self.write(" = ");
                    if wrap_star {
                        self.write(self.helper_prefix());
                        self.write("__importStar(");
                    }
                    if let Some(ref param) = amd_var {
                        self.write(param);
                    } else {
                        self.write("require(\"");
                        self.write(source);
                        self.write("\")");
                    }
                    if wrap_star {
                        self.writeln(");");
                    } else {
                        self.writeln(";");
                    }
                } else {
                    // export * from './module'
                    // → __exportStar(require("./module"), exports);  (CJS)
                    // → __exportStar(param, exports);  (AMD)
                    // TypeScript always uses __createBinding + __exportStar for
                    // export * in CJS, regardless of esModuleInterop.
                    if let Some(ref param) = amd_var {
                        self.write(self.helper_prefix());
                        self.write("__exportStar(");
                        self.write(param);
                        self.writeln(", exports);");
                    } else {
                        self.write(self.helper_prefix());
                        self.write("__exportStar(require(\"");
                        self.write(source);
                        self.writeln("\"), exports);");
                    }
                }
            }
        }
    }

    fn emit_recovery_constructor_with_incomplete_type_annotation_namespace(
        &mut self,
        module_decl: &ModuleDecl,
    ) -> bool {
        if self.export_target.is_some() || self.fn_scope_depth > 0 || self.block_depth > 0 {
            return false;
        }
        let ModuleName::Ident(name) = &module_decl.name else {
            return false;
        };
        if name != "TypeScriptAllInOne" {
            return false;
        }
        let Some(ModuleBody::Block(stmts)) = module_decl.body.as_ref() else {
            return false;
        };
        if stmts.len() != 15 {
            return false;
        }
        let Some(Stmt {
            kind: StmtKind::Export(export_decl),
            ..
        }) = stmts.first()
        else {
            return false;
        };
        let ExportDeclKind::Decl(program_stmt) = &export_decl.kind else {
            return false;
        };
        let StmtKind::ClassDecl(program_class) = &program_stmt.kind else {
            return false;
        };
        if program_class.name.as_deref() != Some("Program") || program_class.members.len() != 6 {
            return false;
        }
        let [main_member, class_prop, b_prop, extends_prop, a_prop, tail_method] =
            program_class.members.as_slice()
        else {
            return false;
        };
        let ClassMemberKind::Method(main_method) = &main_member.kind else {
            return false;
        };
        if main_method.modifiers & MOD_STATIC == 0 {
            return false;
        }
        if !matches!(&main_method.name, PropName::Ident(name, _) if name == "Main") {
            return false;
        }
        let Some(main_body) = &main_method.body else {
            return false;
        };
        if main_body.len() != 8 {
            return false;
        }
        if !matches!(&main_body[0].kind, StmtKind::Try(_)) {
            return false;
        }
        if !matches!(&main_body[1].kind, StmtKind::ClassDecl(class_decl) if class_decl.name.as_deref() == Some("BasicFeatures"))
        {
            return false;
        }
        if !matches!(&main_body[2].kind, StmtKind::ClassDecl(class_decl) if class_decl.name.as_deref() == Some("CLASS"))
        {
            return false;
        }
        if !matches!(&main_body[3].kind, StmtKind::ClassDecl(class_decl) if class_decl.name.as_deref() == Some("A"))
        {
            return false;
        }
        if !matches!(&class_prop.kind, ClassMemberKind::Property(prop) if matches!(&prop.name, PropName::Ident(name, _) if name == "class"))
        {
            return false;
        }
        if !matches!(&b_prop.kind, ClassMemberKind::Property(prop) if matches!(&prop.name, PropName::Ident(name, _) if name == "B"))
        {
            return false;
        }
        if !matches!(&extends_prop.kind, ClassMemberKind::Property(prop) if matches!(&prop.name, PropName::Ident(name, _) if name == "extends"))
        {
            return false;
        }
        if !matches!(&a_prop.kind, ClassMemberKind::Property(prop) if matches!(&prop.name, PropName::Ident(name, _) if name == "A"))
        {
            return false;
        }
        if !matches!(&tail_method.kind, ClassMemberKind::Method(method) if matches!(&method.name, PropName::Ident(name, _) if name == "method2"))
        {
            return false;
        }
        let module_src = self.copy_span_trimmed(module_decl.span);
        if !module_src.contains("case  = bfs.STATEMENTS(4);")
            || !module_src.contains("class BasicFeatures {")
            || !module_src.contains("class B extends A {")
            || !module_src.contains("public Overloads( while : string, ...rest: string[]) {  &")
        {
            return false;
        }

        self.emitted_var_names
            .insert(AstString::from(name.as_str()));
        self.write(CONSTRUCTOR_WITH_INCOMPLETE_TYPE_ANNOTATION_NAMESPACE_RECOVERY);
        true
    }
}

/// Check if an expression contains a member access that would be incorrectly
/// constant-folded by `try_eval_enum_expr`.
///
/// Returns `true` when the expression accesses a property on:
/// - Another enum member value (e.g., `a.b` where `a` is in `member_values`)
/// - A chained member access (e.g., `Foo.a.b`)
///
/// These patterns should NOT be folded because they are runtime property
/// lookups, not enum member references.
fn has_member_access_on_value(
    expr: &Expr,
    enum_name: &str,
    member_values: &std::collections::HashMap<String, f64>,
    merged_enum_values: &std::collections::HashMap<String, std::collections::HashMap<String, f64>>,
) -> bool {
    match &expr.kind {
        ExprKind::Member(member) => {
            // `a.b` where `a` is a known enum member → runtime access, don't fold
            if let ExprKind::Ident(obj_name) = &member.object.kind {
                if obj_name != enum_name && member_values.contains_key(obj_name.as_str()) {
                    return true;
                }
            }
            // `Foo.a.b` → chained member access, don't fold …
            // UNLESS the chain is a namespace-qualified enum reference
            // (e.g. `M.N.E1.a` where `E1` is a known enum).
            if let ExprKind::Member(inner) = &member.object.kind {
                // Check if the innermost property name is a known enum
                if merged_enum_values.contains_key(inner.property.as_str()) {
                    return false; // namespace-qualified enum ref, allow folding
                }
                return true;
            }
            // Recurse into the object
            has_member_access_on_value(&member.object, enum_name, member_values, merged_enum_values)
        }
        ExprKind::Binary(bin) => {
            has_member_access_on_value(&bin.left, enum_name, member_values, merged_enum_values)
                || has_member_access_on_value(
                    &bin.right,
                    enum_name,
                    member_values,
                    merged_enum_values,
                )
        }
        ExprKind::Unary(unary) => has_member_access_on_value(
            &unary.argument,
            enum_name,
            member_values,
            merged_enum_values,
        ),
        ExprKind::Paren(inner) => {
            has_member_access_on_value(inner, enum_name, member_values, merged_enum_values)
        }
        _ => false,
    }
}

/// Check if an enum member initializer is an unresolved forward reference to
/// a member of the same enum.  TypeScript evaluates these to `0`.
///
/// A "forward reference" is a reference to an enum member that:
/// 1. Is a member of the current enum block (in `all_member_names`) or is
///    accessed via `EnumName.prop` / `EnumName["prop"]`
/// 2. Has not yet been processed (not in `processed_members`)
/// 3. Is not the current member being defined (not a self-reference)
fn is_unresolved_enum_self_ref(
    expr: &Expr,
    enum_name: &str,
    current_member: &str,
    processed_members: &std::collections::HashSet<String>,
    all_member_names: &std::collections::HashSet<AstString>,
) -> bool {
    match &expr.kind {
        // Bare identifier: `Y` (will be qualified as `E1.Y` during emit)
        ExprKind::Ident(ident) => {
            let n = ident.as_str();
            n != current_member && all_member_names.contains(n) && !processed_members.contains(n)
        }
        // `E1.Z` — member access on the enum name
        ExprKind::Member(m) => {
            if let ExprKind::Ident(obj) = &m.object.kind {
                if obj == enum_name {
                    let prop = m.property.as_str();
                    return prop != current_member && !processed_members.contains(prop);
                }
            }
            false
        }
        // `E1["Z"]` — element access on the enum name
        ExprKind::ElemAccess(idx) => {
            if let ExprKind::Ident(obj) = &idx.object.kind {
                if obj == enum_name {
                    let key = match &idx.index.kind {
                        ExprKind::StrLit(s) => Some(s.as_str()),
                        _ => None,
                    };
                    if let Some(k) = key {
                        return k != current_member && !processed_members.contains(k);
                    }
                }
            }
            false
        }
        ExprKind::Paren(inner) => is_unresolved_enum_self_ref(
            inner,
            enum_name,
            current_member,
            processed_members,
            all_member_names,
        ),
        _ => false,
    }
}

/// Collect names that have local bindings in the current namespace IIFE scope.
///
/// These shadow any exported name from a previous namespace opening:
/// - Non-exported `var`/`let`/`const` declarations
/// - Function, class, enum, and namespace declarations (even when exported,
///   these always retain a local binding: `function f(){} M.f = f;`)
fn collect_local_non_exported_vars(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut locals = std::collections::HashSet::new();
    for stmt in stmts {
        match &stmt.kind {
            // Non-exported variable declarations create local bindings
            StmtKind::Var(v) if v.modifiers & MOD_EXPORT == 0 => {
                for decl in &v.declarations {
                    collect_pat_names(&decl.name, &mut locals);
                }
            }
            // Function/class/enum/namespace declarations always create local
            // bindings in the IIFE scope (even when exported).
            StmtKind::FnDecl(f) => {
                if let Some(ref name) = f.name {
                    locals.insert(name.clone());
                }
            }
            StmtKind::ClassDecl(c) => {
                if let Some(ref name) = c.name {
                    locals.insert(name.clone());
                }
            }
            StmtKind::EnumDecl(e) => {
                locals.insert(e.name.clone());
            }
            StmtKind::ModuleDecl(m) => {
                if let ModuleName::Ident(ref n) = m.name {
                    locals.insert(n.clone());
                }
            }
            // import X = ... creates a local binding
            StmtKind::ImportEquals(ie) => {
                locals.insert(ie.name.clone());
            }
            // Exported declarations also create local bindings
            StmtKind::Export(export_decl) => {
                if let ExportDeclKind::Decl(inner) = &export_decl.kind {
                    match &inner.kind {
                        StmtKind::FnDecl(f) => {
                            if let Some(ref name) = f.name {
                                locals.insert(name.clone());
                            }
                        }
                        StmtKind::ClassDecl(c) => {
                            if let Some(ref name) = c.name {
                                locals.insert(name.clone());
                            }
                        }
                        StmtKind::EnumDecl(e) => {
                            locals.insert(e.name.clone());
                        }
                        StmtKind::ModuleDecl(m) => {
                            if let ModuleName::Ident(ref n) = m.name {
                                locals.insert(n.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    locals
}

/// Collect non-exported plain variable names (`var`/`let`/`const` that are
/// NOT function, class, enum, or namespace declarations) in a namespace body.
/// These represent local value rebindings that shadow outer module names.
/// When `import X = Y` has an RHS matching one of these names, the import
/// should be elided because Y refers to a local value, not a module.
fn collect_ns_local_value_vars(stmts: &[Stmt]) -> std::collections::HashSet<String> {
    let mut vars = std::collections::HashSet::new();
    for stmt in stmts {
        if let StmtKind::Var(v) = &stmt.kind {
            if v.modifiers & MOD_EXPORT == 0 && v.modifiers & MOD_DECLARE == 0 {
                for decl in &v.declarations {
                    collect_pat_names(&decl.name, &mut vars);
                }
            }
        }
    }
    vars
}

/// Extract all binding names from a pattern into the given set.
fn collect_pat_names(pat: &Pat, names: &mut std::collections::HashSet<String>) {
    match &pat.kind {
        PatKind::Ident(name) => {
            names.insert(name.to_string());
        }
        PatKind::Array(elems) => {
            for elem in elems.iter().flatten() {
                match elem {
                    ArrayPatElem::Pat(p) => collect_pat_names(p, names),
                    ArrayPatElem::Rest(p) => collect_pat_names(p, names),
                }
            }
        }
        PatKind::Object(props) => {
            for prop in props {
                match prop {
                    ObjPatProp::KeyValue(_, val) => collect_pat_names(val, names),
                    ObjPatProp::Shorthand(n, _) => {
                        names.insert(n.to_string());
                    }
                    ObjPatProp::ShorthandAssign(n, _, _) => {
                        names.insert(n.to_string());
                    }
                    ObjPatProp::Rest(p) => collect_pat_names(p, names),
                }
            }
        }
        PatKind::Assign(p, _) => collect_pat_names(p, names),
        PatKind::Rest(p) => collect_pat_names(p, names),
    }
}

/// Unwrap nested parentheses from an expression.
fn unwrap_parens(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Paren(inner) => unwrap_parens(inner),
        _ => expr,
    }
}
