//! Declaration file (.d.ts) emitter.
//!
//! Transforms a TypeScript AST into a declaration file that contains only
//! type-level information: function signatures, class declarations without
//! method bodies, interfaces, type aliases, enums, variable type annotations,
//! and module/namespace declarations.

use std::collections::{HashMap, HashSet};
use tsc_rs_ast::*;

mod enums;
mod literals;
mod overloads;
mod parameters;
mod returns;
use enums::EnumValue;

pub struct DeclarationEmitter<'a> {
    source: &'a str,
    pub output: String,
    indent: usize,
    at_line_start: bool,
    inferred_var_types: HashMap<String, String>,
    inferred_function_return_types: HashMap<String, String>,
    enum_values: HashMap<String, HashMap<String, EnumValue>>,
    enum_scope: Vec<String>,
    enum_ambient: bool,
    strict_null_checks: bool,
}

impl<'a> DeclarationEmitter<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            output: String::new(),
            indent: 0,
            at_line_start: true,
            inferred_var_types: HashMap::new(),
            inferred_function_return_types: HashMap::new(),
            enum_values: HashMap::new(),
            enum_scope: Vec::new(),
            enum_ambient: false,
            strict_null_checks: false,
        }
    }

    pub fn with_options(source: &'a str, options: &CompilerOptions) -> Self {
        let mut emitter = Self::new(source);
        emitter.strict_null_checks = options
            .strict_null_checks
            .unwrap_or(options.strict.unwrap_or(false));
        emitter
    }

    fn write(&mut self, s: &str) {
        if self.at_line_start && !s.is_empty() && s != "\n" {
            const SPACES: &str = "                                                                                                                                ";
            let indent_bytes = (self.indent as usize) * 4;
            if indent_bytes > 0 {
                if indent_bytes <= SPACES.len() {
                    self.output.push_str(&SPACES[..indent_bytes]);
                } else {
                    for _ in 0..self.indent {
                        self.output.push_str("    ");
                    }
                }
            }
            self.at_line_start = false;
        }
        self.output.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.write(s);
        self.output.push('\n');
        self.at_line_start = true;
    }

    fn newline(&mut self) {
        self.output.push('\n');
        self.at_line_start = true;
    }

    fn copy_span(&mut self, span: Span) {
        let start = span.start as usize;
        let end = span.end as usize;
        if start < end && end <= self.source.len() {
            self.output.push_str(&self.source[start..end]);
        }
    }

    fn is_module_export_identifier_name(name: &str) -> bool {
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        let is_start = first == '$' || first == '_' || first.is_alphabetic();
        if !is_start {
            return false;
        }
        chars.all(|ch| ch == '$' || ch == '_' || ch.is_alphanumeric())
    }

    fn emit_module_export_name(&mut self, name: &str) {
        if Self::is_module_export_identifier_name(name) {
            self.write(name);
        } else {
            self.write("\"");
            self.write(name);
            self.write("\"");
        }
    }

    pub fn emit_source_file(&mut self, file: &SourceFile) {
        self.collect_inferred_types(file);
        let forced_module = Self::has_module_file_identity(file);
        let external_module = forced_module || Self::has_syntactic_module_indicator(file);
        let named_exports = Self::named_local_exports(&file.statements);
        let (retained_names, exported_names) =
            self.collect_retained_declaration_names(&file.statements, &named_exports);

        for stmt in &file.statements {
            if Self::is_function_overload_implementation(stmt, &file.statements) {
                continue;
            }
            if !external_module || Self::should_emit_external_stmt(stmt, &retained_names) {
                self.emit_stmt(stmt, true);
            }
        }

        let retains_private_declaration = retained_names
            .iter()
            .any(|name| !exported_names.contains(name));
        let has_explicit_export = file
            .statements
            .iter()
            .any(Self::stmt_has_explicit_boundary_export);
        let has_emitted_module_indicator = file.statements.iter().any(|stmt| {
            Self::should_emit_external_stmt(stmt, &retained_names)
                && Self::stmt_has_module_indicator(stmt)
        });
        if external_module
            && ((retains_private_declaration && !has_explicit_export)
                || (forced_module && !has_emitted_module_indicator))
        {
            self.writeln("export {};");
        }
    }

    fn named_local_exports(statements: &[Stmt]) -> HashSet<&str> {
        statements
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Named {
                        specifiers,
                        source: None,
                        ..
                    } => Some(specifiers.as_slice()),
                    _ => None,
                },
                _ => None,
            })
            .flatten()
            .map(|specifier| specifier.local.as_str())
            .collect()
    }

    fn has_syntactic_module_indicator(file: &SourceFile) -> bool {
        file.statements.iter().any(Self::stmt_has_module_indicator)
    }

    fn stmt_has_module_indicator(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Import(_) | StmtKind::Export(_) | StmtKind::ExportAssign(_) => true,
            StmtKind::FnDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::ClassDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::InterfaceDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::TypeAlias(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::EnumDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::Var(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::ModuleDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            _ => false,
        }
    }

    fn stmt_has_explicit_boundary_export(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Export(export_decl) => matches!(
                export_decl.kind,
                ExportDeclKind::Named { .. }
                    | ExportDeclKind::Default(_)
                    | ExportDeclKind::All { .. }
            ),
            StmtKind::ExportAssign(_) => true,
            _ => false,
        }
    }

    fn has_module_file_identity(file: &SourceFile) -> bool {
        let lower_name = file.file_name.to_ascii_lowercase();
        [".mts", ".cts"]
            .iter()
            .any(|extension| lower_name.ends_with(extension))
    }

    fn collect_retained_declaration_names(
        &self,
        statements: &[Stmt],
        named_exports: &HashSet<&str>,
    ) -> (HashSet<String>, HashSet<String>) {
        let declared_names: HashSet<String> = statements
            .iter()
            .flat_map(Self::stmt_declared_names)
            .collect();
        let mut exported_names: HashSet<String> = named_exports
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        for stmt in statements {
            if Self::stmt_has_direct_export_modifier(stmt) {
                exported_names.extend(Self::stmt_declared_names(stmt));
            }
            match &stmt.kind {
                StmtKind::Export(export_decl) => match &export_decl.kind {
                    ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                        exported_names.extend(Self::stmt_declared_names(inner));
                    }
                    ExportDeclKind::Default(expr) => {
                        let mut dependencies = HashSet::new();
                        Self::collect_entity_name_dependencies(
                            expr,
                            &HashSet::new(),
                            &mut dependencies,
                        );
                        exported_names.extend(
                            dependencies
                                .into_iter()
                                .filter(|name| declared_names.contains(name)),
                        );
                    }
                    _ => {}
                },
                StmtKind::ExportAssign(expr) => {
                    let mut dependencies = HashSet::new();
                    Self::collect_entity_name_dependencies(
                        expr,
                        &HashSet::new(),
                        &mut dependencies,
                    );
                    exported_names.extend(
                        dependencies
                            .into_iter()
                            .filter(|name| declared_names.contains(name)),
                    );
                }
                _ => {}
            }
        }

        let mut retained_names = exported_names.clone();
        loop {
            let before = retained_names.len();
            for stmt in statements {
                if Self::is_function_overload_implementation(stmt, statements) {
                    continue;
                }
                let names = Self::stmt_declared_names(stmt);
                if Self::stmt_has_direct_export_modifier(stmt)
                    || names.iter().any(|name| retained_names.contains(name))
                {
                    let dependencies = self.stmt_declaration_dependencies(stmt);
                    retained_names.extend(
                        dependencies
                            .into_iter()
                            .filter(|name| declared_names.contains(name)),
                    );
                }
            }
            if retained_names.len() == before {
                break;
            }
        }
        (retained_names, exported_names)
    }

    fn stmt_declaration_dependencies(&self, stmt: &Stmt) -> HashSet<String> {
        let mut dependencies = HashSet::new();
        let bound = HashSet::new();
        match &stmt.kind {
            StmtKind::FnDecl(decl) => {
                let scoped =
                    Self::collect_type_params(&decl.type_params, &bound, &mut dependencies);
                Self::collect_params(&decl.params, &scoped, &mut dependencies);
                if let Some(return_type) = &decl.return_type {
                    Self::collect_type_dependencies(return_type, &scoped, &mut dependencies);
                } else if let Some(name) = &decl.name {
                    if let Some(return_type) = self.inferred_function_return_types.get(name) {
                        Self::collect_inferred_type_dependencies(
                            return_type,
                            &scoped,
                            &mut dependencies,
                        );
                    }
                }
            }
            StmtKind::ClassDecl(decl) => {
                let scoped =
                    Self::collect_type_params(&decl.type_params, &bound, &mut dependencies);
                if let Some(extends) = &decl.extends {
                    Self::collect_entity_name_dependencies(extends, &scoped, &mut dependencies);
                }
                if let Some(type_args) = &decl.extends_type_args {
                    Self::collect_types(type_args, &scoped, &mut dependencies);
                }
                Self::collect_types(&decl.implements, &scoped, &mut dependencies);
                for member in &decl.members {
                    if self.is_class_overload_implementation(member, &decl.members) {
                        if let ClassMemberKind::Constructor(ctor) = &member.kind {
                            for param in &ctor.params {
                                if param.modifiers & (MOD_PUBLIC | MOD_PROTECTED | MOD_READONLY)
                                    != 0
                                    && param.modifiers & MOD_PRIVATE == 0
                                {
                                    if let Some(ty) = &param.type_ann {
                                        Self::collect_type_dependencies(
                                            ty,
                                            &scoped,
                                            &mut dependencies,
                                        );
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    Self::collect_class_member_dependencies(member, &scoped, &mut dependencies);
                }
            }
            StmtKind::InterfaceDecl(decl) => {
                let scoped =
                    Self::collect_type_params(&decl.type_params, &bound, &mut dependencies);
                Self::collect_types(&decl.extends, &scoped, &mut dependencies);
                Self::collect_type_members(&decl.members, &scoped, &mut dependencies);
            }
            StmtKind::TypeAlias(decl) => {
                let scoped =
                    Self::collect_type_params(&decl.type_params, &bound, &mut dependencies);
                Self::collect_type_dependencies(&decl.type_ann, &scoped, &mut dependencies);
            }
            StmtKind::EnumDecl(decl) => {
                for member in &decl.members {
                    Self::collect_prop_name_dependencies(&member.name, &bound, &mut dependencies);
                    if let Some(initializer) = &member.initializer {
                        Self::collect_entity_name_dependencies(
                            initializer,
                            &bound,
                            &mut dependencies,
                        );
                    }
                }
            }
            StmtKind::Var(decl) => {
                for declarator in &decl.declarations {
                    if let Some(type_ann) = &declarator.type_ann {
                        Self::collect_type_dependencies(type_ann, &bound, &mut dependencies);
                    } else if let PatKind::Ident(name) = &declarator.name.kind {
                        if let Some(inferred_type) = self.inferred_var_types.get(name.as_str()) {
                            Self::collect_inferred_type_dependencies(
                                inferred_type,
                                &bound,
                                &mut dependencies,
                            );
                        }
                    }
                }
            }
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    dependencies.extend(self.stmt_declaration_dependencies(inner));
                }
                _ => {}
            },
            _ => {}
        }
        dependencies
    }

    fn collect_type_params(
        params: &Option<Vec<TypeParam>>,
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) -> HashSet<String> {
        let mut scoped = bound.clone();
        if let Some(params) = params {
            scoped.extend(params.iter().map(|param| param.name.clone()));
            for param in params {
                if let Some(constraint) = &param.constraint {
                    Self::collect_type_dependencies(constraint, &scoped, dependencies);
                }
                if let Some(default) = &param.default {
                    Self::collect_type_dependencies(default, &scoped, dependencies);
                }
            }
        }
        scoped
    }

    fn collect_params(
        params: &[Param],
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        for param in params {
            if let Some(type_ann) = &param.type_ann {
                Self::collect_type_dependencies(type_ann, bound, dependencies);
            }
        }
    }

    fn collect_types(
        types: &[TypeNode],
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        for type_node in types {
            Self::collect_type_dependencies(type_node, bound, dependencies);
        }
    }

    fn collect_type_dependencies(
        type_node: &TypeNode,
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        match &type_node.kind {
            TypeNodeKind::Keyword(_) | TypeNodeKind::This | TypeNodeKind::Literal(_) => {}
            TypeNodeKind::Reference(reference) => {
                Self::collect_entity_name_dependencies(&reference.name, bound, dependencies);
                if let Some(type_args) = &reference.type_args {
                    Self::collect_types(type_args, bound, dependencies);
                }
            }
            TypeNodeKind::Array(inner)
            | TypeNodeKind::Keyof(inner)
            | TypeNodeKind::Unique(inner)
            | TypeNodeKind::Readonly(inner)
            | TypeNodeKind::Paren(inner)
            | TypeNodeKind::Rest(inner)
            | TypeNodeKind::Optional(inner)
            | TypeNodeKind::TypeOperator(_, inner) => {
                Self::collect_type_dependencies(inner, bound, dependencies);
            }
            TypeNodeKind::Tuple(elements) => {
                for element in elements {
                    Self::collect_type_dependencies(&element.type_node, bound, dependencies);
                }
            }
            TypeNodeKind::Union(types) | TypeNodeKind::Intersection(types) => {
                Self::collect_types(types, bound, dependencies);
            }
            TypeNodeKind::Function(function) | TypeNodeKind::Constructor(function) => {
                let scoped = Self::collect_type_params(&function.type_params, bound, dependencies);
                Self::collect_params(&function.params, &scoped, dependencies);
                Self::collect_type_dependencies(&function.return_type, &scoped, dependencies);
            }
            TypeNodeKind::TypeLit(members) => {
                Self::collect_type_members(members, bound, dependencies);
            }
            TypeNodeKind::Conditional(conditional) => {
                Self::collect_type_dependencies(&conditional.check, bound, dependencies);
                Self::collect_type_dependencies(&conditional.extends, bound, dependencies);
                Self::collect_type_dependencies(&conditional.true_type, bound, dependencies);
                Self::collect_type_dependencies(&conditional.false_type, bound, dependencies);
            }
            TypeNodeKind::Mapped(mapped) => {
                if let Some(constraint) = &mapped.type_param.constraint {
                    Self::collect_type_dependencies(constraint, bound, dependencies);
                }
                if let Some(default) = &mapped.type_param.default {
                    Self::collect_type_dependencies(default, bound, dependencies);
                }
                let mut scoped = bound.clone();
                scoped.insert(mapped.type_param.name.clone());
                if let Some(name_type) = &mapped.name_type {
                    Self::collect_type_dependencies(name_type, &scoped, dependencies);
                }
                if let Some(type_ann) = &mapped.type_ann {
                    Self::collect_type_dependencies(type_ann, &scoped, dependencies);
                }
            }
            TypeNodeKind::IndexedAccess(object, index) => {
                Self::collect_type_dependencies(object, bound, dependencies);
                Self::collect_type_dependencies(index, bound, dependencies);
            }
            TypeNodeKind::TypeQuery(expression) => {
                Self::collect_entity_name_dependencies(expression, bound, dependencies);
            }
            TypeNodeKind::Infer(_, constraint) => {
                if let Some(constraint) = constraint {
                    Self::collect_type_dependencies(constraint, bound, dependencies);
                }
            }
            TypeNodeKind::TemplateLit(template) => {
                Self::collect_types(&template.types, bound, dependencies);
            }
            TypeNodeKind::JSDocNullable(inner) => {
                if let Some(inner) = inner {
                    Self::collect_type_dependencies(inner, bound, dependencies);
                }
            }
            TypeNodeKind::ImportType(import_type) => {
                if let Some(type_args) = &import_type.type_args {
                    Self::collect_types(type_args, bound, dependencies);
                }
            }
            TypeNodeKind::Predicate(predicate) => {
                if let Some(type_ann) = &predicate.type_ann {
                    Self::collect_type_dependencies(type_ann, bound, dependencies);
                }
            }
            TypeNodeKind::NamedTupleMember(member) => {
                Self::collect_type_dependencies(&member.type_node, bound, dependencies);
            }
        }
    }

    fn collect_type_members(
        members: &[TypeMember],
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        for member in members {
            match &member.kind {
                TypeMemberKind::PropertySig(property) => {
                    Self::collect_prop_name_dependencies(&property.name, bound, dependencies);
                    if let Some(type_ann) = &property.type_ann {
                        Self::collect_type_dependencies(type_ann, bound, dependencies);
                    }
                }
                TypeMemberKind::MethodSig(method) => {
                    Self::collect_prop_name_dependencies(&method.name, bound, dependencies);
                    let scoped =
                        Self::collect_type_params(&method.type_params, bound, dependencies);
                    Self::collect_params(&method.params, &scoped, dependencies);
                    if let Some(return_type) = &method.return_type {
                        Self::collect_type_dependencies(return_type, &scoped, dependencies);
                    }
                }
                TypeMemberKind::CallSig(call) => {
                    let scoped = Self::collect_type_params(&call.type_params, bound, dependencies);
                    Self::collect_params(&call.params, &scoped, dependencies);
                    if let Some(return_type) = &call.return_type {
                        Self::collect_type_dependencies(return_type, &scoped, dependencies);
                    }
                }
                TypeMemberKind::ConstructSig(constructor) => {
                    let scoped =
                        Self::collect_type_params(&constructor.type_params, bound, dependencies);
                    Self::collect_params(&constructor.params, &scoped, dependencies);
                    if let Some(return_type) = &constructor.return_type {
                        Self::collect_type_dependencies(return_type, &scoped, dependencies);
                    }
                }
                TypeMemberKind::IndexSig(index) => {
                    Self::collect_params(&index.params, bound, dependencies);
                    if let Some(type_ann) = &index.type_ann {
                        Self::collect_type_dependencies(type_ann, bound, dependencies);
                    }
                }
                TypeMemberKind::GetAccessorSig(accessor)
                | TypeMemberKind::SetAccessorSig(accessor) => {
                    Self::collect_prop_name_dependencies(&accessor.name, bound, dependencies);
                    Self::collect_params(&accessor.params, bound, dependencies);
                    if let Some(return_type) = &accessor.return_type {
                        Self::collect_type_dependencies(return_type, bound, dependencies);
                    }
                }
            }
        }
    }

    fn collect_class_member_dependencies(
        member: &ClassMember,
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        match &member.kind {
            ClassMemberKind::Property(property) => {
                Self::collect_prop_name_dependencies(&property.name, bound, dependencies);
                if property.modifiers & MOD_PRIVATE == 0 {
                    if let Some(type_ann) = &property.type_ann {
                        Self::collect_type_dependencies(type_ann, bound, dependencies);
                    }
                }
            }
            ClassMemberKind::Method(method) => {
                Self::collect_prop_name_dependencies(&method.name, bound, dependencies);
                if method.modifiers & MOD_PRIVATE == 0 {
                    let scoped =
                        Self::collect_type_params(&method.type_params, bound, dependencies);
                    Self::collect_params(&method.params, &scoped, dependencies);
                    if let Some(return_type) = &method.return_type {
                        Self::collect_type_dependencies(return_type, &scoped, dependencies);
                    }
                }
            }
            ClassMemberKind::Constructor(constructor) => {
                Self::collect_params(&constructor.params, bound, dependencies);
            }
            ClassMemberKind::GetAccessor(accessor) | ClassMemberKind::SetAccessor(accessor) => {
                Self::collect_prop_name_dependencies(&accessor.name, bound, dependencies);
                if accessor.modifiers & MOD_PRIVATE == 0 {
                    let scoped =
                        Self::collect_type_params(&accessor.type_params, bound, dependencies);
                    Self::collect_params(&accessor.params, &scoped, dependencies);
                    if let Some(return_type) = &accessor.return_type {
                        Self::collect_type_dependencies(return_type, &scoped, dependencies);
                    }
                }
            }
            ClassMemberKind::IndexSignature(index) => {
                Self::collect_params(&index.params, bound, dependencies);
                if let Some(type_ann) = &index.type_ann {
                    Self::collect_type_dependencies(type_ann, bound, dependencies);
                }
            }
            ClassMemberKind::StaticBlock(_) | ClassMemberKind::SemicolonClassElement => {}
        }
    }

    fn collect_prop_name_dependencies(
        name: &PropName,
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        if let PropName::Computed(expression, _) = name {
            Self::collect_entity_name_dependencies(expression, bound, dependencies);
        }
    }

    fn collect_entity_name_dependencies(
        expression: &Expr,
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        match &expression.kind {
            ExprKind::Ident(name) => {
                if !bound.contains(name.as_str()) {
                    dependencies.insert(name.to_string());
                }
            }
            ExprKind::Member(member) => {
                Self::collect_entity_name_dependencies(&member.object, bound, dependencies);
            }
            ExprKind::ElemAccess(element) => {
                Self::collect_entity_name_dependencies(&element.object, bound, dependencies);
                Self::collect_entity_name_dependencies(&element.index, bound, dependencies);
            }
            ExprKind::Call(call) => {
                Self::collect_entity_name_dependencies(&call.callee, bound, dependencies);
                for argument in &call.args {
                    Self::collect_entity_name_dependencies(argument, bound, dependencies);
                }
            }
            ExprKind::New(new_expression) => {
                Self::collect_entity_name_dependencies(&new_expression.callee, bound, dependencies);
                if let Some(arguments) = &new_expression.args {
                    for argument in arguments {
                        Self::collect_entity_name_dependencies(argument, bound, dependencies);
                    }
                }
            }
            ExprKind::Cond(conditional) => {
                Self::collect_entity_name_dependencies(&conditional.test, bound, dependencies);
                Self::collect_entity_name_dependencies(
                    &conditional.consequent,
                    bound,
                    dependencies,
                );
                Self::collect_entity_name_dependencies(&conditional.alternate, bound, dependencies);
            }
            ExprKind::Binary(binary) => {
                Self::collect_entity_name_dependencies(&binary.left, bound, dependencies);
                Self::collect_entity_name_dependencies(&binary.right, bound, dependencies);
            }
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => {
                Self::collect_entity_name_dependencies(inner, bound, dependencies);
            }
            ExprKind::TypeAssertion(assertion) => {
                Self::collect_entity_name_dependencies(&assertion.expr, bound, dependencies);
            }
            ExprKind::As(expression) => {
                Self::collect_entity_name_dependencies(&expression.expr, bound, dependencies);
            }
            ExprKind::Satisfies(expression) => {
                Self::collect_entity_name_dependencies(&expression.expr, bound, dependencies);
            }
            ExprKind::Instantiation(expression) => {
                Self::collect_entity_name_dependencies(&expression.expr, bound, dependencies);
            }
            ExprKind::Comma(expressions) => {
                for expression in expressions {
                    Self::collect_entity_name_dependencies(expression, bound, dependencies);
                }
            }
            ExprKind::ArrayLit(expressions) => {
                for expression in expressions.iter().flatten() {
                    Self::collect_entity_name_dependencies(expression, bound, dependencies);
                }
            }
            _ => {}
        }
    }

    fn collect_inferred_type_dependencies(
        inferred_type: &str,
        bound: &HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) {
        let inferred_type = inferred_type
            .strip_prefix("typeof ")
            .unwrap_or(inferred_type);
        let root = inferred_type
            .split(|character: char| {
                !(character == '$' || character == '_' || character.is_alphanumeric())
            })
            .next()
            .unwrap_or_default();
        if !root.is_empty() && !bound.contains(root) {
            dependencies.insert(root.to_string());
        }
    }

    fn stmt_has_direct_export_modifier(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::FnDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::ClassDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::InterfaceDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::TypeAlias(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::EnumDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::Var(decl) => decl.modifiers & MOD_EXPORT != 0,
            StmtKind::ModuleDecl(decl) => decl.modifiers & MOD_EXPORT != 0,
            _ => false,
        }
    }

    fn should_emit_external_stmt(stmt: &Stmt, retained_names: &HashSet<String>) -> bool {
        match &stmt.kind {
            StmtKind::Import(_) | StmtKind::Export(_) | StmtKind::ExportAssign(_) => true,
            StmtKind::ModuleDecl(module_decl) if matches!(&module_decl.name, ModuleName::Ident(name) if name == "global") => {
                true
            }
            StmtKind::FnDecl(_)
            | StmtKind::ClassDecl(_)
            | StmtKind::InterfaceDecl(_)
            | StmtKind::TypeAlias(_)
            | StmtKind::EnumDecl(_)
            | StmtKind::Var(_)
            | StmtKind::ModuleDecl(_) => Self::stmt_declared_names(stmt)
                .iter()
                .any(|name| retained_names.contains(name)),
            _ => true,
        }
    }

    fn stmt_declared_names(stmt: &Stmt) -> Vec<String> {
        match &stmt.kind {
            StmtKind::FnDecl(decl) => decl.name.iter().cloned().collect(),
            StmtKind::ClassDecl(decl) => decl.name.iter().cloned().collect(),
            StmtKind::InterfaceDecl(decl) => vec![decl.name.clone()],
            StmtKind::TypeAlias(decl) => vec![decl.name.clone()],
            StmtKind::EnumDecl(decl) => vec![decl.name.clone()],
            StmtKind::Var(decl) => decl
                .declarations
                .iter()
                .flat_map(|declarator| Self::binding_names(&declarator.name))
                .collect(),
            StmtKind::ModuleDecl(decl) => match &decl.name {
                ModuleName::Ident(name) if name != "global" => vec![name.clone()],
                _ => Vec::new(),
            },
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    Self::stmt_declared_names(inner)
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn binding_names(pattern: &Pat) -> Vec<String> {
        match &pattern.kind {
            PatKind::Ident(name) => vec![name.to_string()],
            PatKind::Array(elements) => elements
                .iter()
                .flatten()
                .flat_map(|element| match element {
                    ArrayPatElem::Pat(pattern) | ArrayPatElem::Rest(pattern) => {
                        Self::binding_names(pattern)
                    }
                })
                .collect(),
            PatKind::Object(properties) => properties
                .iter()
                .flat_map(|property| match property {
                    ObjPatProp::KeyValue(_, pattern) | ObjPatProp::Rest(pattern) => {
                        Self::binding_names(pattern)
                    }
                    ObjPatProp::Shorthand(name, _) | ObjPatProp::ShorthandAssign(name, _, _) => {
                        vec![name.to_string()]
                    }
                })
                .collect(),
            PatKind::Assign(pattern, _) | PatKind::Rest(pattern) => Self::binding_names(pattern),
        }
    }

    /// Emit a statement for the declaration file.
    /// `top_level` indicates whether we are at the top level of the file.
    fn emit_stmt(&mut self, stmt: &Stmt, top_level: bool) {
        match &stmt.kind {
            StmtKind::FnDecl(fn_decl) => {
                let is_exported = fn_decl.modifiers & MOD_EXPORT != 0;
                let is_default = fn_decl.modifiers & MOD_DEFAULT != 0;
                if top_level {
                    if is_exported {
                        self.write("export ");
                        if is_default {
                            self.write("default ");
                        }
                    }
                    if !is_exported || !is_default {
                        self.write("declare ");
                    }
                }
                self.emit_fn_signature(fn_decl);
                self.writeln(";");
            }
            StmtKind::ClassDecl(class_decl) => {
                let is_exported = class_decl.modifiers & MOD_EXPORT != 0;
                let is_default = class_decl.modifiers & MOD_DEFAULT != 0;
                if top_level {
                    if is_exported {
                        self.write("export ");
                        if is_default {
                            self.write("default ");
                        }
                    }
                    if !is_exported || !is_default {
                        self.write("declare ");
                    }
                }
                self.emit_class_decl(class_decl);
            }
            StmtKind::InterfaceDecl(iface) => {
                if top_level && iface.modifiers & MOD_EXPORT != 0 {
                    self.write("export ");
                }
                self.emit_interface_decl(iface);
            }
            StmtKind::TypeAlias(ta) => {
                if top_level && ta.modifiers & MOD_EXPORT != 0 {
                    self.write("export ");
                }
                self.emit_type_alias(ta);
            }
            StmtKind::EnumDecl(enum_decl) => {
                if top_level {
                    if enum_decl.modifiers & MOD_EXPORT != 0 {
                        self.write("export ");
                    }
                    self.write("declare ");
                }
                self.emit_enum_decl(enum_decl);
            }
            StmtKind::Var(var_stmt) => {
                if top_level {
                    if var_stmt.modifiers & MOD_EXPORT != 0 {
                        self.write("export ");
                    }
                    self.write("declare ");
                }
                self.emit_var_decl(var_stmt);
            }
            StmtKind::ModuleDecl(module_decl) => {
                if top_level {
                    if module_decl.modifiers & MOD_EXPORT != 0 {
                        self.write("export ");
                    }
                    self.write("declare ");
                }
                self.emit_module_decl(module_decl);
            }
            StmtKind::Import(import_decl) => {
                self.emit_import_decl(import_decl);
            }
            StmtKind::Export(export_decl) => {
                self.emit_export_decl(export_decl);
            }
            StmtKind::ExportAssign(expr) => {
                self.write("export = ");
                self.copy_span(expr.span);
                self.writeln(";");
            }
            _ => {
                // Other statement kinds (if, while, for, etc.) are implementation
                // details and are not emitted in declaration files.
            }
        }
    }

    fn emit_fn_signature(&mut self, fn_decl: &FnDecl) {
        self.write("function ");
        if let Some(ref name) = fn_decl.name {
            self.write(name);
        }
        self.emit_type_params(&fn_decl.type_params);
        self.write("(");
        self.emit_params(&fn_decl.params);
        self.write(")");
        if let Some(ref ret) = fn_decl.return_type {
            self.write(": ");
            self.emit_type_node(ret);
        } else if fn_decl.body.is_none() {
            self.write(": any");
        } else if let Some(ret) = self.single_return_type(
            fn_decl.body.as_deref(),
            &fn_decl.params,
            fn_decl.is_async,
            fn_decl.is_generator,
        ) {
            self.write(": ");
            self.write(&ret);
        } else if let Some(ret) = fn_decl
            .name
            .as_ref()
            .and_then(|name| {
                self.inferred_function_return_types
                    .get(name.as_str())
                    .cloned()
            })
            .or_else(|| self.infer_function_return_type_text(fn_decl))
        {
            self.write(": ");
            self.write(&ret);
        }
    }

    fn emit_class_decl(&mut self, class_decl: &ClassDecl) {
        if class_decl.modifiers & MOD_ABSTRACT != 0 {
            self.write("abstract ");
        }
        self.write("class");
        if let Some(ref name) = class_decl.name {
            self.write(" ");
            self.write(name);
        }
        self.emit_type_params(&class_decl.type_params);
        if let Some(ref extends) = class_decl.extends {
            self.write(" extends ");
            self.copy_span(extends.span);
            if let Some(ref args) = class_decl.extends_type_args {
                self.write("<");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_type_node(arg);
                }
                self.write(">");
            }
        }
        if !class_decl.implements.is_empty() {
            self.write(" implements ");
            for (i, impl_ty) in class_decl.implements.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.emit_type_node(impl_ty);
            }
        }
        self.writeln(" {");
        self.indent += 1;
        if class_decl.members.iter().any(|member| match &member.kind {
            ClassMemberKind::Property(prop) => matches!(prop.name, PropName::Private(_, _)),
            ClassMemberKind::Method(method) => matches!(method.name, PropName::Private(_, _)),
            ClassMemberKind::GetAccessor(acc) | ClassMemberKind::SetAccessor(acc) => {
                matches!(acc.name, PropName::Private(_, _))
            }
            _ => false,
        }) {
            self.writeln("#private;");
        }
        self.emit_constructor_properties(class_decl);
        let mut private_methods = HashSet::new();
        for member in &class_decl.members {
            if self.is_class_overload_implementation(member, &class_decl.members) {
                continue;
            }
            if let ClassMemberKind::Method(method) = &member.kind {
                if method.modifiers & MOD_PRIVATE != 0
                    && !private_methods.insert((
                        method.modifiers & MOD_STATIC != 0,
                        self.overload_property_key(&method.name),
                    ))
                {
                    continue;
                }
            }
            self.emit_class_member(member);
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_constructor_properties(&mut self, class: &ClassDecl) {
        let mut emitted = HashSet::new();
        for member in &class.members {
            let ClassMemberKind::Constructor(constructor) = &member.kind else {
                continue;
            };
            for param in &constructor.params {
                if param.modifiers & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY) == 0
                {
                    continue;
                }
                let PatKind::Ident(name) = &param.name.kind else {
                    continue;
                };
                if !emitted.insert(name.clone()) {
                    continue;
                }
                self.emit_accessibility(param.modifiers);
                if param.modifiers & MOD_READONLY != 0 {
                    self.write("readonly ");
                }
                self.write(name);
                if param.optional {
                    self.write("?");
                }
                if param.modifiers & MOD_PRIVATE == 0 {
                    self.write(": ");
                    if let Some(ty) = &param.type_ann {
                        self.emit_type_node(ty);
                    } else {
                        let ty = param
                            .initializer
                            .as_deref()
                            .and_then(|expr| self.infer_var_type_text(expr));
                        self.write(ty.as_deref().unwrap_or("any"));
                    }
                }
                self.writeln(";");
            }
        }
    }

    fn emit_constructor_params(&mut self, params: &[Param]) {
        self.emit_parameter_list(params, true);
    }

    fn emit_class_member(&mut self, member: &ClassMember) {
        match &member.kind {
            ClassMemberKind::Property(prop) => {
                if matches!(prop.name, PropName::Private(_, _)) {
                    return;
                }
                self.emit_accessibility(prop.modifiers);
                if prop.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                if prop.modifiers & MOD_READONLY != 0 {
                    self.write("readonly ");
                }
                if prop.modifiers & MOD_ABSTRACT != 0 {
                    self.write("abstract ");
                }
                self.emit_prop_name(&prop.name);
                if prop.optional {
                    self.write("?");
                }
                if prop.modifiers & MOD_PRIVATE == 0 {
                    if let Some(ref ty) = prop.type_ann {
                        self.write(": ");
                        self.emit_type_node(ty);
                    } else if let Some(initializer) = prop.initializer.as_deref() {
                        let literal = (prop.modifiers & MOD_READONLY != 0)
                            .then(|| Self::const_initializer_text(initializer))
                            .flatten();
                        if let Some(literal) = literal {
                            self.write(" = ");
                            self.write(&literal);
                        } else if let Some(ty) = self.infer_var_type_text(initializer) {
                            self.write(": ");
                            self.write(&ty);
                        }
                    } else {
                        self.write(": any");
                    }
                }
                self.writeln(";");
            }
            ClassMemberKind::Method(method) => {
                if matches!(method.name, PropName::Private(_, _)) {
                    return;
                }
                self.emit_accessibility(method.modifiers);
                if method.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                if method.modifiers & MOD_ABSTRACT != 0 && method.modifiers & MOD_PRIVATE == 0 {
                    self.write("abstract ");
                }
                self.emit_prop_name(&method.name);
                if method.optional && method.modifiers & MOD_PRIVATE == 0 {
                    self.write("?");
                }
                if method.modifiers & MOD_PRIVATE == 0 {
                    self.emit_type_params(&method.type_params);
                    self.write("(");
                    self.emit_params(&method.params);
                    self.write(")");
                    if let Some(ref ret) = method.return_type {
                        self.write(": ");
                        self.emit_type_node(ret);
                    } else if let Some(ret) = Self::no_value_return_type(
                        method.body.as_deref(),
                        method.is_async,
                        method.is_generator,
                    ) {
                        self.write(": ");
                        self.write(ret);
                    } else if let Some(ret) = self.single_return_type(
                        method.body.as_deref(),
                        &method.params,
                        method.is_async,
                        method.is_generator,
                    ) {
                        self.write(": ");
                        self.write(&ret);
                    } else if method.body.is_none() {
                        self.write(": any");
                    }
                }
                self.writeln(";");
            }
            ClassMemberKind::Constructor(ctor) => {
                self.emit_accessibility(ctor.modifiers);
                self.write("constructor(");
                if ctor.modifiers & MOD_PRIVATE == 0 {
                    self.emit_constructor_params(&ctor.params);
                }
                self.writeln(");");
            }
            ClassMemberKind::GetAccessor(acc) => {
                if matches!(acc.name, PropName::Private(_, _)) {
                    return;
                }
                self.emit_accessibility(acc.modifiers);
                if acc.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                self.write("get ");
                self.emit_prop_name(&acc.name);
                self.write("(");
                if acc.modifiers & MOD_PRIVATE == 0 {
                    self.emit_params(&acc.params);
                }
                self.write(")");
                if acc.modifiers & MOD_PRIVATE == 0 {
                    if let Some(ref ret) = acc.return_type {
                        self.write(": ");
                        self.emit_type_node(ret);
                    }
                }
                self.writeln(";");
            }
            ClassMemberKind::SetAccessor(acc) => {
                if matches!(acc.name, PropName::Private(_, _)) {
                    return;
                }
                self.emit_accessibility(acc.modifiers);
                if acc.modifiers & MOD_STATIC != 0 {
                    self.write("static ");
                }
                self.write("set ");
                self.emit_prop_name(&acc.name);
                self.write("(");
                if acc.modifiers & MOD_PRIVATE != 0 {
                    self.write("value");
                } else {
                    self.emit_params(&acc.params);
                }
                self.writeln(");");
            }
            ClassMemberKind::IndexSignature(idx) => {
                if idx.modifiers & MOD_READONLY != 0 {
                    self.write("readonly ");
                }
                self.write("[");
                self.emit_params(&idx.params);
                self.write("]");
                if let Some(ref ty) = idx.type_ann {
                    self.write(": ");
                    self.emit_type_node(ty);
                }
                self.writeln(";");
            }
            ClassMemberKind::StaticBlock(_) | ClassMemberKind::SemicolonClassElement => {}
        }
    }

    fn emit_accessibility(&mut self, modifiers: ModifierFlags) {
        if modifiers & MOD_PRIVATE != 0 {
            self.write("private ");
        } else if modifiers & MOD_PROTECTED != 0 {
            self.write("protected ");
        }
    }

    fn emit_interface_decl(&mut self, iface: &InterfaceDecl) {
        self.write("interface ");
        self.write(&iface.name);
        self.emit_type_params(&iface.type_params);
        if !iface.extends.is_empty() {
            self.write(" extends ");
            for (i, ext) in iface.extends.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.emit_type_node(ext);
            }
        }
        self.writeln(" {");
        self.indent += 1;
        for member in &iface.members {
            self.emit_type_member(member);
        }
        self.indent -= 1;
        self.writeln("}");
    }

    fn emit_type_member(&mut self, member: &TypeMember) {
        match &member.kind {
            TypeMemberKind::PropertySig(prop) => {
                if prop.readonly {
                    self.write("readonly ");
                }
                self.emit_prop_name(&prop.name);
                if prop.optional {
                    self.write("?");
                }
                if let Some(ref ty) = prop.type_ann {
                    self.write(": ");
                    self.emit_type_node(ty);
                } else {
                    self.write(": any");
                }
                self.writeln(";");
            }
            TypeMemberKind::MethodSig(method) => {
                self.emit_prop_name(&method.name);
                if method.optional {
                    self.write("?");
                }
                self.emit_type_params(&method.type_params);
                self.write("(");
                self.emit_params(&method.params);
                self.write(")");
                if let Some(ref ret) = method.return_type {
                    self.write(": ");
                    self.emit_type_node(ret);
                } else {
                    self.write(": any");
                }
                self.writeln(";");
            }
            TypeMemberKind::CallSig(call) => {
                self.emit_type_params(&call.type_params);
                self.write("(");
                self.emit_params(&call.params);
                self.write(")");
                if let Some(ref ret) = call.return_type {
                    self.write(": ");
                    self.emit_type_node(ret);
                } else {
                    self.write(": any");
                }
                self.writeln(";");
            }
            TypeMemberKind::ConstructSig(ctor) => {
                self.write("new ");
                self.emit_type_params(&ctor.type_params);
                self.write("(");
                self.emit_params(&ctor.params);
                self.write(")");
                if let Some(ref ret) = ctor.return_type {
                    self.write(": ");
                    self.emit_type_node(ret);
                } else {
                    self.write(": any");
                }
                self.writeln(";");
            }
            TypeMemberKind::IndexSig(idx) => {
                if idx.modifiers & MOD_READONLY != 0 {
                    self.write("readonly ");
                }
                self.write("[");
                self.emit_params(&idx.params);
                self.write("]");
                if let Some(ref ty) = idx.type_ann {
                    self.write(": ");
                    self.emit_type_node(ty);
                } else {
                    self.write(": any");
                }
                self.writeln(";");
            }
            TypeMemberKind::GetAccessorSig(acc) => {
                self.write("get ");
                self.emit_prop_name(&acc.name);
                self.write("(");
                self.emit_params(&acc.params);
                self.write(")");
                if let Some(ref ret) = acc.return_type {
                    self.write(": ");
                    self.emit_type_node(ret);
                } else {
                    self.write(": any");
                }
                self.writeln(";");
            }
            TypeMemberKind::SetAccessorSig(acc) => {
                self.write("set ");
                self.emit_prop_name(&acc.name);
                self.write("(");
                self.emit_params(&acc.params);
                self.writeln(");");
            }
        }
    }

    fn emit_type_alias(&mut self, ta: &TypeAliasDecl) {
        self.write("type ");
        self.write(&ta.name);
        self.emit_type_params(&ta.type_params);
        self.write(" = ");
        self.emit_type_node(&ta.type_ann);
        self.writeln(";");
    }

    fn emit_var_decl(&mut self, var_stmt: &VarStmt) {
        let kw = match var_stmt.kind {
            VarKind::Var => "var",
            VarKind::Let => "let",
            VarKind::Const => "const",
            VarKind::Using => "using",
            VarKind::AwaitUsing => "await using",
        };
        self.write(kw);
        self.write(" ");
        for (i, decl) in var_stmt.declarations.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.emit_binding_pattern(&decl.name);
            if let Some(ref ty) = decl.type_ann {
                self.write(": ");
                self.emit_type_node(ty);
            } else if let Some(ref init) = decl.init {
                let literal = (var_stmt.kind == VarKind::Const
                    && matches!(decl.name.kind, PatKind::Ident(_)))
                .then(|| Self::const_initializer_text(init))
                .flatten();
                if let Some(literal) = literal {
                    self.write(" = ");
                    self.write(&literal);
                } else if let Some(ty) = self.infer_var_type_text(init) {
                    self.write(": ");
                    self.write(&ty);
                }
            }
        }
        self.writeln(";");
    }

    fn emit_module_decl(&mut self, module_decl: &ModuleDecl) {
        self.emit_module_decl_with_prefix(module_decl, true);
    }

    fn emit_module_decl_with_prefix(&mut self, module_decl: &ModuleDecl, prefix: bool) {
        let saved_ambient = self.enum_ambient;
        self.enum_ambient |= module_decl.modifiers & MOD_DECLARE != 0
            || matches!(module_decl.name, ModuleName::String(_));
        self.enum_scope.push(match &module_decl.name {
            ModuleName::Ident(name) => name.to_string(),
            ModuleName::String(name) => format!("\"{name}\""),
        });
        match &module_decl.name {
            ModuleName::Ident(name) => {
                if prefix && name != "global" {
                    self.write("namespace ");
                }
                self.write(name);
            }
            ModuleName::String(name) => {
                if prefix {
                    self.write("module ");
                }
                self.write("\"");
                self.write(name);
                self.write("\"");
            }
        }
        if let Some(body) = &module_decl.body {
            match body {
                ModuleBody::Block(statements) => {
                    self.writeln(" {");
                    self.indent += 1;
                    self.emit_namespace_body(statements);
                    self.indent -= 1;
                    self.writeln("}");
                }
                ModuleBody::Module(inner) => {
                    self.write(".");
                    self.emit_module_decl_with_prefix(inner, false);
                }
            }
        } else {
            self.writeln(";");
        }
        self.enum_scope.pop();
        self.enum_ambient = saved_ambient;
    }

    fn emit_namespace_body(&mut self, statements: &[Stmt]) {
        let named_exports = Self::named_local_exports(statements);
        let (retained, exported) =
            self.collect_retained_declaration_names(statements, &named_exports);
        let has_boundary = statements
            .iter()
            .any(Self::stmt_has_explicit_boundary_export);
        let has_private =
            !self.enum_ambient && retained.iter().any(|name| !exported.contains(name));
        let explicit_exports = has_boundary || has_private;
        for statement in statements {
            if Self::is_function_overload_implementation(statement, statements) {
                continue;
            }
            if !self.enum_ambient && !Self::should_emit_external_stmt(statement, &retained) {
                continue;
            }
            if let StmtKind::Export(export) = &statement.kind {
                if let ExportDeclKind::Decl(inner) = &export.kind {
                    if explicit_exports {
                        self.write("export ");
                    }
                    self.emit_stmt(inner, false);
                    continue;
                }
            }
            if explicit_exports && Self::stmt_has_direct_export_modifier(statement) {
                self.write("export ");
            }
            self.emit_stmt(statement, false);
        }
        if has_private && !has_boundary {
            self.writeln("export {};");
        }
    }

    fn emit_import_decl(&mut self, import_decl: &ImportDecl) {
        self.write("import ");
        if import_decl.type_only {
            self.write("type ");
        }
        match &import_decl.specifiers {
            ImportClause::Named {
                default,
                named,
                namespace,
            } => {
                let mut has_prev = false;
                if let Some(ref d) = default {
                    self.write(d);
                    has_prev = true;
                }
                if let Some(ref ns) = namespace {
                    if has_prev {
                        self.write(", ");
                    }
                    self.write("* as ");
                    self.write(ns);
                    has_prev = true;
                }
                if !named.is_empty() {
                    if has_prev {
                        self.write(", ");
                    }
                    self.write("{ ");
                    for (i, spec) in named.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        if spec.is_type {
                            self.write("type ");
                        }
                        if let Some(ref imp) = spec.imported {
                            self.emit_module_export_name(imp);
                            self.write(" as ");
                        }
                        self.write(&spec.local);
                    }
                    self.write(" }");
                    has_prev = true;
                }
                if has_prev {
                    self.write(" from ");
                }
            }
            ImportClause::Require(name) => {
                self.write(name);
                self.write(" = require(\"");
                self.write(&import_decl.source);
                self.writeln("\");");
                return;
            }
        }
        self.write("\"");
        self.write(&import_decl.source);
        self.writeln("\";");
    }

    fn emit_export_decl(&mut self, export_decl: &ExportDecl) {
        match &export_decl.kind {
            ExportDeclKind::Decl(decl) => {
                // For exported declarations, emit "export declare" for value-level
                // declarations (functions, classes, enums, variables), but just
                // "export" for type-level declarations (interfaces, type aliases).
                let needs_declare = matches!(
                    decl.kind,
                    StmtKind::FnDecl(_)
                        | StmtKind::ClassDecl(_)
                        | StmtKind::EnumDecl(_)
                        | StmtKind::Var(_)
                        | StmtKind::ModuleDecl(_)
                );
                self.write("export ");
                if needs_declare {
                    self.write("declare ");
                }
                self.emit_stmt(decl, false);
            }
            ExportDeclKind::Named {
                specifiers,
                source,
                type_only,
            } => {
                if specifiers.is_empty() && source.is_none() && !type_only {
                    self.writeln("export {};");
                    return;
                }
                self.write("export ");
                if *type_only {
                    self.write("type ");
                }
                self.write("{ ");
                for (i, spec) in specifiers.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    if spec.is_type {
                        self.write("type ");
                    }
                    self.emit_module_export_name(&spec.local);
                    if let Some(ref exp) = spec.exported {
                        self.write(" as ");
                        if !exp.is_empty() {
                            self.emit_module_export_name(exp);
                        }
                    }
                }
                self.write(" }");
                if let Some(ref src) = source {
                    self.write(" from \"");
                    self.write(src);
                    self.write("\"");
                }
                self.writeln(";");
            }
            ExportDeclKind::Default(expr) => {
                self.write("export default ");
                self.copy_span(expr.span);
                self.writeln(";");
            }
            ExportDeclKind::DefaultDecl(decl) => {
                self.write("export default ");
                match &decl.kind {
                    StmtKind::FnDecl(fn_decl) => {
                        self.emit_fn_signature(fn_decl);
                        self.writeln(";");
                    }
                    StmtKind::ClassDecl(class_decl) => {
                        self.emit_class_decl(class_decl);
                    }
                    _ => {
                        self.emit_stmt(decl, false);
                    }
                }
            }
            ExportDeclKind::All {
                source,
                alias,
                type_only,
                ..
            } => {
                self.write("export ");
                if *type_only {
                    self.write("type ");
                }
                self.write("*");
                if let Some(ref a) = alias {
                    self.write(" as ");
                    self.write(a);
                }
                self.write(" from \"");
                self.write(source);
                self.writeln("\";");
            }
        }
    }

    fn emit_params(&mut self, params: &[Param]) {
        self.emit_parameter_list(params, false);
    }

    fn collect_inferred_types(&mut self, file: &SourceFile) {
        let mut changed = true;
        while changed {
            changed = false;
            for stmt in &file.statements {
                changed |= self.collect_inferred_types_from_stmt(stmt);
            }
        }
    }

    fn collect_inferred_types_from_stmt(&mut self, stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Var(var_stmt) => {
                let mut changed = false;
                for decl in &var_stmt.declarations {
                    let PatKind::Ident(name) = &decl.name.kind else {
                        continue;
                    };
                    if decl.type_ann.is_some()
                        || self.inferred_var_types.contains_key(name.as_str())
                    {
                        continue;
                    }
                    let Some(init) = decl.init.as_deref() else {
                        continue;
                    };
                    let Some(ty) = self.infer_var_type_text(init) else {
                        continue;
                    };
                    self.inferred_var_types.insert(name.to_string(), ty);
                    changed = true;
                }
                changed
            }
            StmtKind::FnDecl(fn_decl) => {
                let Some(name) = fn_decl.name.as_ref() else {
                    return false;
                };
                if fn_decl.return_type.is_some()
                    || self
                        .inferred_function_return_types
                        .contains_key(name.as_str())
                {
                    return false;
                }
                let Some(ty) = self.infer_function_return_type_text(fn_decl) else {
                    return false;
                };
                self.inferred_function_return_types.insert(name.clone(), ty);
                true
            }
            StmtKind::Export(export_decl) => match &export_decl.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    self.collect_inferred_types_from_stmt(inner)
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn infer_function_return_type_text(&self, fn_decl: &FnDecl) -> Option<String> {
        let body = fn_decl.body.as_ref()?;
        Self::no_value_return_type(Some(body), fn_decl.is_async, fn_decl.is_generator)
            .map(str::to_string)
            .or_else(|| self.infer_return_type_from_stmts(body))
    }

    fn infer_return_type_from_stmts(&self, stmts: &[Stmt]) -> Option<String> {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Return(expr) => {
                    if let Some(expr) = expr.as_deref() {
                        if let Some(ty) = self.infer_expr_type_text(expr) {
                            return Some(ty);
                        }
                    }
                }
                StmtKind::If(if_stmt) => {
                    if let Some(ty) = self.infer_return_type_from_stmt(&if_stmt.consequent) {
                        return Some(ty);
                    }
                    if let Some(else_branch) = if_stmt.alternate.as_deref() {
                        if let Some(ty) = self.infer_return_type_from_stmt(else_branch) {
                            return Some(ty);
                        }
                    }
                }
                StmtKind::Block(stmts) => {
                    if let Some(ty) = self.infer_return_type_from_stmts(stmts) {
                        return Some(ty);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn infer_return_type_from_stmt(&self, stmt: &Stmt) -> Option<String> {
        match &stmt.kind {
            StmtKind::Block(stmts) => self.infer_return_type_from_stmts(stmts),
            StmtKind::Return(expr) => expr
                .as_deref()
                .and_then(|expr| self.infer_expr_type_text(expr)),
            _ => None,
        }
    }

    fn infer_var_type_text(&self, expr: &Expr) -> Option<String> {
        let expr = self.unwrap_inference_expr(expr);
        match &expr.kind {
            ExprKind::Member(_) => self
                .expr_source_text(expr)
                .map(|text| format!("typeof {text}")),
            _ => self.infer_expr_type_text(expr),
        }
    }

    fn infer_expr_type_text(&self, expr: &Expr) -> Option<String> {
        let expr = self.unwrap_inference_expr(expr);
        match &expr.kind {
            ExprKind::NumLit(_) => Some("number".to_string()),
            ExprKind::BigIntLit(_) => Some("bigint".to_string()),
            ExprKind::StrLit(_) | ExprKind::NoSubstTemplate(_) | ExprKind::Template(_) => {
                Some("string".to_string())
            }
            ExprKind::BoolLit(_) => Some("boolean".to_string()),
            ExprKind::NullLit => Some("null".to_string()),
            ExprKind::Ident(name) => self.inferred_var_types.get(name.as_str()).cloned(),
            ExprKind::New(new_expr) => self.expr_source_text(&new_expr.callee),
            ExprKind::Call(call) => match &self.unwrap_inference_expr(&call.callee).kind {
                ExprKind::Ident(name) => self
                    .inferred_function_return_types
                    .get(name.as_str())
                    .cloned(),
                _ => None,
            },
            _ => None,
        }
    }

    fn unwrap_inference_expr<'b>(&self, mut expr: &'b Expr) -> &'b Expr {
        loop {
            match &expr.kind {
                ExprKind::Paren(inner) | ExprKind::NonNull(inner) => expr = inner,
                ExprKind::TypeAssertion(inner) => expr = &inner.expr,
                ExprKind::As(inner) => expr = &inner.expr,
                ExprKind::Satisfies(inner) => expr = &inner.expr,
                ExprKind::Instantiation(inner) => expr = &inner.expr,
                _ => return expr,
            }
        }
    }

    fn expr_source_text(&self, expr: &Expr) -> Option<String> {
        let start = expr.span.start as usize;
        let end = expr.span.end as usize;
        (start < end && end <= self.source.len()).then(|| self.source[start..end].to_string())
    }

    fn emit_binding_pattern(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Ident(name) => self.write(name),
            PatKind::Array(elements) => {
                self.write("[");
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    if let Some(ref e) = elem {
                        match e {
                            ArrayPatElem::Pat(p) => self.emit_binding_pattern(p),
                            ArrayPatElem::Rest(p) => {
                                self.write("...");
                                self.emit_binding_pattern(p);
                            }
                        }
                    }
                }
                self.write("]");
            }
            PatKind::Object(props) => {
                self.write("{ ");
                for (i, prop) in props.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    match prop {
                        ObjPatProp::KeyValue(key, val) => {
                            self.emit_prop_name(key);
                            self.write(": ");
                            self.emit_binding_pattern(val);
                        }
                        ObjPatProp::Shorthand(name, _) => self.write(name),
                        ObjPatProp::ShorthandAssign(name, _, _) => self.write(name),
                        ObjPatProp::Rest(p) => {
                            self.write("...");
                            self.emit_binding_pattern(p);
                        }
                    }
                }
                self.write(" }");
            }
            PatKind::Assign(p, _) => {
                // In declaration files, default values are stripped
                self.emit_binding_pattern(p);
            }
            PatKind::Rest(p) => {
                self.write("...");
                self.emit_binding_pattern(p);
            }
        }
    }

    fn emit_prop_name(&mut self, name: &PropName) {
        match name {
            PropName::Ident(n, _) => self.write(n),
            PropName::String(n, span) => {
                let start = span.start as usize;
                let q = if start < self.source.len() && self.source.as_bytes()[start] == b'\'' {
                    "'"
                } else {
                    "\""
                };
                self.write(q);
                self.write(n);
                self.write(q);
            }
            PropName::Number(n, _) => self.write(n),
            PropName::Computed(expr, _) => {
                self.write("[");
                self.copy_span(expr.span);
                self.write("]");
            }
            PropName::Private(n, _) => {
                self.write("#");
                self.write(n);
            }
        }
    }

    fn emit_type_params(&mut self, type_params: &Option<Vec<TypeParam>>) {
        if let Some(ref params) = type_params {
            if params.is_empty() {
                return;
            }
            self.write("<");
            for (i, tp) in params.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                if tp.modifiers & MOD_IN != 0 {
                    self.write("in ");
                }
                if tp.modifiers & MOD_OUT != 0 {
                    self.write("out ");
                }
                self.write(&tp.name);
                if let Some(ref constraint) = tp.constraint {
                    self.write(" extends ");
                    self.emit_type_node(constraint);
                }
                if let Some(ref default) = tp.default {
                    self.write(" = ");
                    self.emit_type_node(default);
                }
            }
            self.write(">");
        }
    }

    fn emit_type_node(&mut self, ty: &TypeNode) {
        match &ty.kind {
            TypeNodeKind::Keyword(kw) => {
                let s = match kw {
                    KeywordTypeKind::Any => "any",
                    KeywordTypeKind::Unknown => "unknown",
                    KeywordTypeKind::Number => "number",
                    KeywordTypeKind::BigInt => "bigint",
                    KeywordTypeKind::String => "string",
                    KeywordTypeKind::Boolean => "boolean",
                    KeywordTypeKind::Void => "void",
                    KeywordTypeKind::Undefined => "undefined",
                    KeywordTypeKind::Null => "null",
                    KeywordTypeKind::Never => "never",
                    KeywordTypeKind::Object => "object",
                    KeywordTypeKind::Symbol => "symbol",
                    KeywordTypeKind::Intrinsic => "intrinsic",
                };
                self.write(s);
            }
            TypeNodeKind::Reference(type_ref) => {
                self.copy_span(type_ref.name.span);
                if let Some(ref args) = type_ref.type_args {
                    self.write("<");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.emit_type_node(arg);
                    }
                    self.write(">");
                }
            }
            TypeNodeKind::Array(inner) => {
                self.emit_type_node(inner);
                self.write("[]");
            }
            TypeNodeKind::Tuple(elements) => {
                self.write("[");
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    if elem.dotdotdot {
                        self.write("...");
                    }
                    if let Some(ref label) = elem.label {
                        self.write(label);
                        if elem.optional {
                            self.write("?");
                        }
                        self.write(": ");
                        self.emit_type_node(&elem.type_node);
                    } else {
                        self.emit_type_node(&elem.type_node);
                        if elem.optional {
                            self.write("?");
                        }
                    }
                }
                self.write("]");
            }
            TypeNodeKind::Union(types) => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(" | ");
                    }
                    self.emit_type_node(t);
                }
            }
            TypeNodeKind::Intersection(types) => {
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        self.write(" & ");
                    }
                    self.emit_type_node(t);
                }
            }
            TypeNodeKind::Function(fn_type) => {
                self.emit_type_params(&fn_type.type_params);
                self.write("(");
                self.emit_params(&fn_type.params);
                self.write(") => ");
                self.emit_type_node(&fn_type.return_type);
            }
            TypeNodeKind::Constructor(fn_type) => {
                self.write("new ");
                self.emit_type_params(&fn_type.type_params);
                self.write("(");
                self.emit_params(&fn_type.params);
                self.write(") => ");
                self.emit_type_node(&fn_type.return_type);
            }
            TypeNodeKind::TypeLit(members) => {
                if members.is_empty() {
                    self.write("{}");
                    return;
                }
                self.writeln("{");
                self.indent += 1;
                for m in members {
                    self.emit_type_member(m);
                }
                self.indent -= 1;
                self.write("}");
            }
            TypeNodeKind::Conditional(cond) => {
                self.emit_type_node(&cond.check);
                self.write(" extends ");
                self.emit_type_node(&cond.extends);
                self.write(" ? ");
                self.emit_type_node(&cond.true_type);
                self.write(" : ");
                self.emit_type_node(&cond.false_type);
            }
            TypeNodeKind::Mapped(mapped) => {
                self.write("{ ");
                if let Some(ref rm) = mapped.readonly {
                    match rm {
                        MappedModifier::Add => self.write("+readonly "),
                        MappedModifier::Remove => self.write("-readonly "),
                        MappedModifier::None => self.write("readonly "),
                    }
                }
                self.write("[");
                self.write(&mapped.type_param.name);
                if let Some(ref constraint) = mapped.type_param.constraint {
                    self.write(" in ");
                    self.emit_type_node(constraint);
                }
                if let Some(ref name_type) = mapped.name_type {
                    self.write(" as ");
                    self.emit_type_node(name_type);
                }
                self.write("]");
                if let Some(ref opt) = mapped.optional {
                    match opt {
                        MappedModifier::Add => self.write("+?"),
                        MappedModifier::Remove => self.write("-?"),
                        MappedModifier::None => self.write("?"),
                    }
                }
                if let Some(ref ty) = mapped.type_ann {
                    self.write(": ");
                    self.emit_type_node(ty);
                }
                self.write("; }");
            }
            TypeNodeKind::IndexedAccess(obj, idx) => {
                self.emit_type_node(obj);
                self.write("[");
                self.emit_type_node(idx);
                self.write("]");
            }
            TypeNodeKind::TypeQuery(expr) => {
                self.write("typeof ");
                self.copy_span(expr.span);
            }
            TypeNodeKind::Keyof(inner) => {
                self.write("keyof ");
                self.emit_type_node(inner);
            }
            TypeNodeKind::Unique(inner) => {
                self.write("unique ");
                self.emit_type_node(inner);
            }
            TypeNodeKind::Readonly(inner) => {
                self.write("readonly ");
                self.emit_type_node(inner);
            }
            TypeNodeKind::Infer(name, constraint) => {
                self.write("infer ");
                self.write(name);
                if let Some(ref c) = constraint {
                    self.write(" extends ");
                    self.emit_type_node(c);
                }
            }
            TypeNodeKind::TemplateLit(tpl) => {
                self.write("`");
                for (i, quasi) in tpl.quasis.iter().enumerate() {
                    self.write(&quasi.raw);
                    if i < tpl.types.len() {
                        self.write("${");
                        self.emit_type_node(&tpl.types[i]);
                        self.write("}");
                    }
                }
                self.write("`");
            }
            TypeNodeKind::This => self.write("this"),
            TypeNodeKind::Paren(inner) => {
                self.write("(");
                self.emit_type_node(inner);
                self.write(")");
            }
            TypeNodeKind::Rest(inner) => {
                self.write("...");
                self.emit_type_node(inner);
            }
            TypeNodeKind::Optional(inner) => {
                self.emit_type_node(inner);
                self.write("?");
            }
            TypeNodeKind::JSDocNullable(inner) => {
                self.write("?");
                if let Some(inner) = inner {
                    self.emit_type_node(inner);
                }
            }
            TypeNodeKind::Literal(lit) => match lit {
                LiteralTypeKind::Number(n) => self.write(n),
                LiteralTypeKind::String(s) => {
                    self.write("\"");
                    self.write(s);
                    self.write("\"");
                }
                LiteralTypeKind::Boolean(b) => {
                    self.write(if *b { "true" } else { "false" });
                }
                LiteralTypeKind::Null => self.write("null"),
                LiteralTypeKind::BigInt(n) => {
                    self.write(n);
                    self.write("n");
                }
                LiteralTypeKind::Minus(n) => {
                    self.write("-");
                    self.write(n);
                }
            },
            TypeNodeKind::ImportType(import_ty) => {
                if import_ty.is_typeof {
                    self.write("typeof ");
                }
                self.write("import(");
                self.emit_type_node(&import_ty.argument);
                self.write(")");
                if let Some(ref qualifier) = import_ty.qualifier {
                    self.write(".");
                    self.copy_span(qualifier.span);
                }
                if let Some(ref args) = import_ty.type_args {
                    self.write("<");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.emit_type_node(arg);
                    }
                    self.write(">");
                }
            }
            TypeNodeKind::Predicate(pred) => {
                if pred.asserts {
                    self.write("asserts ");
                }
                self.write(&pred.param_name);
                if let Some(ref ty) = pred.type_ann {
                    self.write(" is ");
                    self.emit_type_node(ty);
                }
            }
            TypeNodeKind::TypeOperator(op, inner) => {
                match op {
                    TypeOperatorKind::Keyof => self.write("keyof "),
                    TypeOperatorKind::Unique => self.write("unique "),
                    TypeOperatorKind::Readonly => self.write("readonly "),
                }
                self.emit_type_node(inner);
            }
            TypeNodeKind::NamedTupleMember(ntm) => {
                if ntm.dotdotdot {
                    self.write("...");
                }
                self.write(&ntm.name);
                if ntm.optional {
                    self.write("?");
                }
                self.write(": ");
                self.emit_type_node(&ntm.type_node);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_simple_declaration_types_from_initializers_and_returns() {
        let source = "\
export const a = 1;\n\
export class C {\n\
    public p: number;\n\
}\n\
export const c = new C();\n\
export function f() {\n\
    return c;\n\
}\n";
        let file = tsc_rs_parser::parse("test.ts", source);
        let mut emitter = DeclarationEmitter::new(source);
        emitter.emit_source_file(&file);

        assert!(emitter.output.contains("export declare const a = 1;"));
        assert!(emitter.output.contains("p: number;"));
        assert!(!emitter.output.contains("public p: number;"));
        assert!(emitter.output.contains("export declare const c: C;"));
        assert!(emitter.output.contains("export declare function f(): C;"));
    }
}
