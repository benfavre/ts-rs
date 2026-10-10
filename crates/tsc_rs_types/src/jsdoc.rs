//! JSDoc types for checked JavaScript (`checkJs` / `// @ts-check`).
//!
//! tsc reads `@type`, `@param`, `@returns`, `@typedef`, `@satisfies` and
//! parenthesized `/** @type {T} */ (expr)` casts as type annotations. The
//! checker sees them by checking a copy of the file whose AST carries those
//! annotations as if they were TypeScript syntax. Type text is parsed at
//! its original offsets so diagnostics point into the comment, as tsc's do.

use tsc_rs_ast::*;

/// One `{type}` of a JSDoc tag: its text range in the file.
#[derive(Debug, Clone)]
struct TagType {
    start: usize,
    end: usize,
}

#[derive(Debug, Clone)]
struct Tag {
    name: String,
    /// A child of a `@typedef`/`@callback` in the same comment.
    owned: bool,
    /// The tag name's span in the file (tsc reports `@satisfies` there).
    name_span: Span,
    ty: Option<TagType>,
    /// The word after the type (`@param {T} x` → `x`; `[x]` and `[x=d]`
    /// mark the parameter optional).
    target: Option<String>,
    optional: bool,
}

#[derive(Debug, Clone)]
struct JsDoc {
    end: u32,
    tags: Vec<Tag>,
}

/// The file with its JSDoc types written in as annotations, or `None`
/// when nothing applies.
pub(crate) fn desugar(file: &SourceFile) -> Option<SourceFile> {
    let docs = collect(file);
    if docs.is_empty() {
        return None;
    }
    // A JavaScript function can be a constructor whose name, as a type,
    // means its instances; that is not modeled, so such types are skipped.
    let function_names: Vec<String> = file
        .statements
        .iter()
        .flat_map(|stmt| match &stmt.kind {
            StmtKind::FnDecl(function) => function.name.clone().into_iter().collect::<Vec<_>>(),
            StmtKind::Var(var_stmt) => var_stmt
                .declarations
                .iter()
                .filter(|declaration| {
                    declaration
                        .init
                        .as_ref()
                        .is_some_and(|init| matches!(init.kind, ExprKind::FnExpr(_)))
                })
                .filter_map(|declaration| match &declaration.name.kind {
                    PatKind::Ident(name) => Some(name.to_string()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    let names_function = |ty: &TagType| {
        file.text.get(ty.start..ty.end).is_some_and(|text| {
            text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
                .any(|word| function_names.iter().any(|name| name == word))
        })
    };
    let mut out = file.clone();
    let mut changed = false;
    let mut aliases = Vec::new();
    for doc in &docs {
        for (index, tag) in doc.tags.iter().enumerate() {
            // `@callback Name` + its `@param`/`@returns`/`@template` tags is
            // a function type alias.
            if tag.name == "callback" {
                let Some(name) = &tag.target else { continue };
                let mut children: Vec<&Tag> = doc.tags[index + 1..]
                    .iter()
                    .take_while(|child| child.owned)
                    .collect();
                // The comment's other `@template` tags parameterize it too.
                children.extend(
                    doc.tags
                        .iter()
                        .filter(|other| other.name == "template" && !other.owned),
                );
                if let Some(alias) = callback_alias(&file.text, name, tag, &children) {
                    aliases.push(alias);
                }
                continue;
            }
            if tag.name != "typedef" {
                continue;
            }
            let Some(name) = &tag.target else {
                continue;
            };
            // `@typedef Name` without a type is an object of its properties.
            let anchor = TagType {
                start: (tag.name_span.end as usize).max(7),
                end: tag.name_span.end as usize,
            };
            let ty = tag.ty.as_ref().unwrap_or(&anchor);
            if names_function(ty) {
                continue;
            }
            let declared = if tag.ty.is_some() {
                file.text.get(ty.start..ty.end).unwrap_or("").trim()
            } else {
                "Object"
            };
            let properties: Vec<&Tag> = doc.tags[index + 1..]
                .iter()
                .take_while(|tag| tag.name != "typedef" && tag.name != "callback")
                .filter(|tag| matches!(tag.name.as_str(), "property" | "prop"))
                .collect();
            let type_ann = if matches!(declared, "Object" | "object") && !properties.is_empty() {
                object_type_text(&file.text, &properties)
                    .and_then(|text| parse_type_text(&text, ty.start))
            } else {
                parse_type_at(&file.text, ty, false)
            };
            let Some(type_ann) = type_ann else {
                continue;
            };
            // The comment's `@template` tags parameterize the typedef.
            let templates: Vec<&Tag> = doc
                .tags
                .iter()
                .filter(|tag| tag.name == "template" && !tag.owned)
                .collect();
            aliases.push(Stmt {
                kind: StmtKind::TypeAlias(Box::new(TypeAliasDecl {
                    name: name.clone(),
                    name_span: None,
                    type_params: template_params_from(&file.text, &templates),
                    type_ann,
                    modifiers: MOD_NONE,
                    span: Span::new(ty.start as u32, ty.end as u32),
                })),
                span: Span::new(ty.start as u32, ty.end as u32),
            });
        }
    }
    // Non-generic typedefs only: a generic one's parameters cannot be
    // copied out of its scope (the declared type contextually types the
    // function instead).
    let typedefs: Vec<(String, TypeNode)> = aliases
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::TypeAlias(alias) if alias.type_params.is_none() => {
                Some((alias.name.clone(), alias.type_ann.clone()))
            }
            _ => None,
        })
        .collect();
    let generic_typedefs: Vec<String> = aliases
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::TypeAlias(alias) if alias.type_params.is_some() => Some(alias.name.clone()),
            _ => None,
        })
        .collect();
    let mut walker = Walker {
        text: &file.text,
        docs: &docs,
        names_function: &names_function,
        typedefs: &typedefs,
        generic_typedefs: &generic_typedefs,
        changed: false,
    };
    walker.stmts(&mut out.statements);
    changed |= walker.changed;
    if !aliases.is_empty() {
        changed = true;
        let mut statements = aliases;
        statements.append(&mut out.statements);
        out.statements = statements;
    }
    changed.then_some(out)
}

fn collect(file: &SourceFile) -> Vec<JsDoc> {
    let mut docs = Vec::new();
    for comment in &file.comments {
        let Some(text) = file.text.get(comment.pos as usize..comment.end as usize) else {
            continue;
        };
        if !text.starts_with("/**") || text.starts_with("/**/") {
            continue;
        }
        let tags = parse_tags(text, comment.pos as usize);
        if !tags.is_empty() {
            docs.push(JsDoc {
                end: comment.end,
                tags,
            });
        }
    }
    docs
}

fn parse_tags(text: &str, base: usize) -> Vec<Tag> {
    let bytes = text.as_bytes();
    let mut tags = Vec::new();
    let mut at = 0;
    while let Some(found) = text[at..].find('@') {
        let start = at + found;
        // A tag starts a line (after `*`) or follows whitespace.
        let preceded =
            start == 0 || bytes[start - 1].is_ascii_whitespace() || bytes[start - 1] == b'*';
        let name_end = text[start + 1..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .map_or(text.len(), |offset| start + 1 + offset);
        let name = &text[start + 1..name_end];
        at = name_end.max(start + 1);
        if !preceded || name.is_empty() {
            continue;
        }
        let mut cursor = skip_space(text, name_end);
        let mut ty = brace_type(text, base, cursor);
        if let Some((_, close)) = ty {
            cursor = skip_space(text, close + 1);
        }
        let mut optional = false;
        // `@template T, U` names a list: keep the rest of the line.
        let target = if name == "template" {
            let line_end = text[cursor..]
                .find(['\n', '\r'])
                .map_or(text.len(), |offset| cursor + offset);
            let line = text[cursor..line_end].trim_end_matches("*/").trim();
            (!line.is_empty()).then(|| line.to_string())
        } else if bytes.get(cursor) == Some(&b'[') {
            optional = true;
            let inner_end = text[cursor..]
                .find(']')
                .map_or(text.len(), |offset| cursor + offset);
            let inner = &text[cursor + 1..inner_end];
            cursor = inner_end + 1;
            Some(inner.split('=').next().unwrap_or("").trim().to_string())
        } else {
            let word_end = text[cursor..]
                .find(|c: char| {
                    !(c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '\\' | '{' | '}'))
                })
                .map_or(text.len(), |offset| cursor + offset);
            // `\u{61}`'s braces belong to the name only after a backslash.
            let word = &text[cursor..word_end];
            let word = match word.find('{') {
                Some(brace) if !word[..brace].ends_with("\\u") => &word[..brace],
                _ => word,
            };
            cursor += word.len();
            (!word.is_empty()).then(|| unescape_name(word))
        };
        // `@param name {Type}`: the type may follow the name.
        if ty.is_none() {
            ty = brace_type(text, base, skip_space(text, cursor));
        }
        let ty = ty.map(|(ty, _)| ty);
        if let Some(ty) = &ty {
            if text[ty.start - base..ty.end - base]
                .trim_end()
                .ends_with('=')
            {
                optional = true;
            }
        }
        tags.push(Tag {
            name: name.to_string(),
            name_span: Span::new((base + start + 1) as u32, (base + name_end) as u32),
            ty,
            target,
            optional,
            owned: false,
        });
    }
    // A `@typedef` owns the `@property`/`@type` tags after it, and a
    // `@callback` its `@param`/`@returns`/`@template`; the rest describe the
    // commented code.
    let mut owner: Option<&'static str> = None;
    for tag in &mut tags {
        match tag.name.as_str() {
            "typedef" => owner = Some("typedef"),
            "callback" => owner = Some("callback"),
            name => {
                let child = match owner {
                    Some("typedef") => matches!(name, "property" | "prop" | "type"),
                    Some("callback") => {
                        matches!(
                            name,
                            "param"
                                | "arg"
                                | "argument"
                                | "returns"
                                | "return"
                                | "template"
                                | "this"
                        )
                    }
                    _ => false,
                };
                if child {
                    tag.owned = true;
                } else {
                    owner = None;
                }
            }
        }
    }
    tags
}

/// The `{…}` at `at`, as the type's file range and the closing brace's
/// offset in `text`.
fn brace_type(text: &str, base: usize, at: usize) -> Option<(TagType, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    for (offset, byte) in bytes[at..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let close = at + offset;
                    return Some((
                        TagType {
                            start: base + at + 1,
                            end: base + close,
                        },
                        close,
                    ));
                }
            }
            _ => {}
        }
    }
    None
}

/// A tag's name with `\uXXXX` / `\u{X}` escapes decoded.
fn unescape_name(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find("\\u") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let (hex, consumed) = if let Some(braced) = after.strip_prefix('{') {
            match braced.find('}') {
                Some(close) => (&braced[..close], close + 2),
                None => ("", 0),
            }
        } else {
            (after.get(..4).unwrap_or(""), 4)
        };
        match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
            Some(c) if consumed > 0 => {
                out.push(c);
                rest = &after[consumed..];
            }
            _ => {
                out.push_str("\\u");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn skip_space(text: &str, from: usize) -> usize {
    text[from.min(text.len())..]
        .find(|c: char| !c.is_whitespace())
        .map_or(text.len(), |offset| from + offset)
}

/// Parses the JSDoc type at `ty` into a TypeScript type node whose spans
/// are the type's own offsets in the file.
/// `@template {C} T, U` tags as type parameters.
fn template_params_from(text: &str, tags: &[&Tag]) -> Option<Vec<TypeParam>> {
    let mut params = Vec::new();
    for tag in tags {
        let constraint = tag
            .ty
            .as_ref()
            .and_then(|ty| parse_type_at(text, ty, false))
            .map(Box::new);
        let names = tag.target.as_deref().unwrap_or("");
        // `@template const T` / `in`/`out` variance modifiers.
        let names = names.trim_start();
        let names = ["const ", "in ", "out "]
            .iter()
            .fold(names, |rest, modifier| {
                rest.strip_prefix(modifier).unwrap_or(rest)
            });
        // `[T=Default]` names one parameter with a default; a description
        // (`- text`) may follow the names.
        if let Some(bracketed) = names.strip_prefix('[') {
            let inner = bracketed.split(']').next().unwrap_or("");
            let (name, default) = match inner.split_once('=') {
                Some((name, default)) => (name.trim(), Some(default.trim())),
                None => (inner.trim(), None),
            };
            if !name.is_empty() {
                let start = tag.name_span.start as usize;
                params.push(TypeParam {
                    name: name.to_string(),
                    name_span: tag.name_span,
                    constraint: constraint.clone(),
                    default: default
                        .and_then(|text| convert(text, false))
                        .and_then(|text| parse_type_text(&text, start.max(7)))
                        .map(Box::new),
                    modifiers: MOD_NONE,
                    span: tag.name_span,
                });
            }
            continue;
        }
        let names = names.split(" - ").next().unwrap_or("");
        for name in names.split(',') {
            let name: String = name
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if name.is_empty() {
                continue;
            }
            params.push(TypeParam {
                name,
                name_span: tag.name_span,
                constraint: constraint.clone(),
                default: None,
                modifiers: MOD_NONE,
                span: tag.name_span,
            });
        }
    }
    (!params.is_empty()).then_some(params)
}

/// `param_tag`: a trailing `=` marks an optional parameter (recorded on the
/// tag) rather than adding `undefined` to the type.
fn parse_type_at(text: &str, ty: &TagType, param_tag: bool) -> Option<TypeNode> {
    let raw = text.get(ty.start..ty.end)?;
    let converted = convert(raw.trim(), param_tag)?;
    let lead = raw.len() - raw.trim_start().len();
    parse_type_text(&converted, ty.start + lead)
}

/// Parses TypeScript type text as if it started at byte `start` of the
/// file, so the type node's spans are file offsets.
fn parse_type_text(converted: &str, start: usize) -> Option<TypeNode> {
    if start < 7 {
        return None;
    }
    let mut buffer = " ".repeat(start - 7);
    buffer.push_str("type _=");
    buffer.push_str(converted);
    buffer.push(';');
    let parsed = tsc_rs_parser::parse("jsdoc.ts", &buffer);
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    parsed
        .statements
        .into_iter()
        .find_map(|stmt| match stmt.kind {
            StmtKind::TypeAlias(alias) => Some(alias.type_ann),
            _ => None,
        })
}

/// `@callback Name` as `type Name<T…> = (p: P, …) => R`.
fn callback_alias(text: &str, name: &str, tag: &Tag, children: &[&Tag]) -> Option<Stmt> {
    let mut params = Vec::new();
    let mut returns = "any".to_string();
    for child in children {
        let raw = child.ty.as_ref().and_then(|ty| text.get(ty.start..ty.end));
        match child.name.as_str() {
            "param" | "arg" | "argument" => {
                let param_name = child.target.as_deref()?;
                if param_name.contains('.') {
                    continue;
                }
                let raw = raw.unwrap_or("any").trim();
                if let Some(element) = raw.strip_prefix("...") {
                    params.push(format!("...{param_name}: {}[]", convert(element, true)?));
                } else {
                    let ty = convert(raw, true)?;
                    params.push(format!(
                        "{param_name}{}: {ty}",
                        if child.optional { "?" } else { "" }
                    ));
                }
            }
            "returns" | "return" => returns = convert(raw?.trim(), false)?,
            "this" => params.insert(0, format!("this: {}", convert(raw?.trim(), false)?)),
            _ => {}
        }
    }
    // The synthesized type is anchored at the tag (at least past the
    // parse prefix's width).
    let start = (tag.name_span.start as usize).max(7);
    let text_of_type = format!("({}) => {returns}", params.join(", "));
    let type_ann = parse_type_text(&text_of_type, start)?;
    // The alias spans its synthesized type: type parameters are scoped by
    // the declaration's span.
    let span = Span::new(start as u32, (start + text_of_type.len()) as u32);
    let templates: Vec<&Tag> = children
        .iter()
        .copied()
        .filter(|child| child.name == "template")
        .collect();
    Some(Stmt {
        kind: StmtKind::TypeAlias(Box::new(TypeAliasDecl {
            name: name.to_string(),
            name_span: None,
            type_params: template_params_from(text, &templates),
            type_ann,
            modifiers: MOD_NONE,
            span,
        })),
        span,
    })
}

/// A `@returns` type parsed in return position, where a type predicate
/// (`x is T`, `asserts x`) is allowed.
fn parse_return_type_text(converted: &str, start: usize) -> Option<TypeNode> {
    if start < 11 {
        return None;
    }
    let mut buffer = " ".repeat(start - 11);
    buffer.push_str("type _=()=>");
    buffer.push_str(converted);
    buffer.push(';');
    let parsed = tsc_rs_parser::parse("jsdoc.ts", &buffer);
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    parsed
        .statements
        .into_iter()
        .find_map(|stmt| match stmt.kind {
            StmtKind::TypeAlias(alias) => match alias.type_ann.kind {
                TypeNodeKind::Function(function) => Some(*function.return_type),
                _ => None,
            },
            _ => None,
        })
}

/// `@typedef {Object} T` + `@property` tags (dotted names nest) as an
/// object type literal.
fn object_type_text(text: &str, properties: &[&Tag]) -> Option<String> {
    #[derive(Default)]
    struct Node {
        ty: Option<String>,
        optional: bool,
        children: Vec<(String, Node)>,
    }
    fn render(node: &Node) -> String {
        let members: Vec<String> = node
            .children
            .iter()
            .map(|(name, child)| {
                let ty = if child.children.is_empty() {
                    child.ty.clone().unwrap_or_else(|| "any".into())
                } else {
                    render(child)
                };
                format!("{name}{}: {ty}", if child.optional { "?" } else { "" })
            })
            .collect();
        format!("{{ {} }}", members.join("; "))
    }
    let mut root = Node::default();
    for property in properties {
        let path = property.target.as_ref()?;
        let ty = match &property.ty {
            Some(ty) => convert(text.get(ty.start..ty.end)?.trim(), true)?,
            None => "any".into(),
        };
        let mut node = &mut root;
        for segment in path.split('.') {
            let index = match node.children.iter().position(|(name, _)| name == segment) {
                Some(index) => index,
                None => {
                    node.children.push((segment.to_string(), Node::default()));
                    node.children.len() - 1
                }
            };
            node = &mut node.children[index].1;
        }
        // `{Object}` with nested properties becomes the nested literal.
        if !matches!(ty.as_str(), "Object" | "object") || node.ty.is_some() {
            node.ty = Some(ty);
        }
        node.optional = property.optional;
    }
    Some(render(&root))
}

/// JSDoc-only type syntax as TypeScript; `None` for forms not handled yet.
fn convert(raw: &str, param_tag: bool) -> Option<String> {
    // A type spanning comment lines: each continuation line's leading
    // `*` becomes a space (same length, so offsets hold).
    let joined;
    let mut text = raw.trim();
    if text.contains('\n') {
        joined = text
            .split('\n')
            .enumerate()
            .map(|(index, line)| {
                let line = line.trim_end_matches('\r');
                if index == 0 {
                    return line.to_string();
                }
                let indent = line.len() - line.trim_start().len();
                match line.trim_start().strip_prefix('*') {
                    Some(rest) => format!("{} {rest}", " ".repeat(indent)),
                    None => line.to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        text = joined.as_str();
    }
    // `T=`: an optional parameter (recorded on the tag), elsewhere
    // `T | undefined`.
    if let Some(stripped) = text.strip_suffix('=') {
        let inner = convert(stripped.trim_end(), param_tag)?;
        return Some(if param_tag {
            inner
        } else {
            format!("{inner} | undefined")
        });
    }
    // `...T` rest parameter element type.
    if let Some(stripped) = text.strip_prefix("...") {
        return convert(stripped, param_tag).map(|inner| format!("{inner}[]"));
    }
    if let Some(stripped) = text.strip_prefix('?') {
        if stripped.is_empty() {
            return Some("any".into());
        }
        return convert(stripped, param_tag).map(|inner| format!("{inner} | null"));
    }
    if let Some(stripped) = text.strip_prefix('!') {
        return convert(stripped, param_tag);
    }
    if text == "*" || text == "?" {
        return Some("any".into());
    }
    if let Some(rest) = text.strip_prefix("function(") {
        return convert_closure_function(rest);
    }
    if text.contains("function(") || text.contains('*') {
        return None;
    }
    // `Array.<T>` is `Array<T>` (the space keeps every offset).
    let text = text.replace(".<", " <");
    // Qualified names (`ns.T`) may name JavaScript expando declarations,
    // which are not resolved as types yet. `import("m").T` is ordinary
    // TypeScript: the member access after an import's `)` is allowed.
    if text
        .split(['"', '\''])
        .step_by(2)
        .any(|part| part.contains('.') && !part.trim_start().starts_with(')'))
    {
        return None;
    }
    // `exports` / `module` name a CommonJS module's value shape.
    if text
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .any(|word| matches!(word, "exports" | "module"))
    {
        return None;
    }
    Some(primitive_names(&text))
}

/// Closure's `function(T1, T2, ...T3): R` as `(arg0: T1, arg1: T2,
/// ...args: T3[]) => R` (`R` defaults to `any`); `this:`/`new:` forms are
/// not handled.
fn convert_closure_function(rest: &str) -> Option<String> {
    let mut depth = 0usize;
    let mut close = None;
    for (offset, c) in rest.char_indices() {
        match c {
            '(' | '<' | '{' | '[' => depth += 1,
            ')' | '>' | '}' | ']' if depth > 0 => depth -= 1,
            ')' => {
                close = Some(offset);
                break;
            }
            _ => {}
        }
    }
    let close = close?;
    let mut params = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for c in rest[..close].chars() {
        match c {
            '(' | '<' | '{' | '[' => depth += 1,
            ')' | '>' | '}' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                params.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        params.push(current);
    }
    let mut rendered = Vec::new();
    for (index, param) in params.iter().enumerate() {
        let param = param.trim();
        if param.starts_with("this:") || param.starts_with("new:") {
            return None;
        }
        if let Some(element) = param.strip_prefix("...") {
            rendered.push(format!("...args: {}[]", convert(element, true)?));
        } else {
            let optional = param.ends_with('=');
            let ty = convert(param, true)?;
            rendered.push(format!(
                "arg{index}{}: {ty}",
                if optional { "?" } else { "" }
            ));
        }
    }
    let after = rest[close + 1..].trim();
    let returns = match after.strip_prefix(':') {
        Some(ret) => convert(ret.trim(), false)?,
        None if after.is_empty() => "any".to_string(),
        None => return None,
    };
    Some(format!("({}) => {returns}", rendered.join(", ")))
}

/// getIntendedTypeFromJSDocTypeReference: `String`, `Number`, `BigInt`,
/// `Boolean`, `Void`, `Undefined` and `Null` name the primitive types.
/// Each replacement has the same length, so offsets are kept.
fn primitive_names(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let flush = |word: &mut String, out: &mut String| {
        out.push_str(match word.as_str() {
            "String" => "string",
            "Number" => "number",
            "BigInt" => "bigint",
            "Boolean" => "boolean",
            "Void" => "void",
            "Undefined" => "undefined",
            "Null" => "null",
            // Without type arguments these are tsc's `any[]`/`Promise<any>`.
            "array" => "any[]",
            "promise" => "Promise<any>",
            other => other,
        });
        word.clear();
    };
    for c in text.chars() {
        if let Some(q) = quote {
            out.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '$' {
            word.push(c);
            continue;
        }
        flush(&mut word, &mut out);
        if c == '"' || c == '\'' {
            quote = Some(c);
        }
        out.push(c);
    }
    flush(&mut word, &mut out);
    out
}

struct Walker<'a> {
    text: &'a str,
    docs: &'a [JsDoc],
    names_function: &'a dyn Fn(&TagType) -> bool,
    /// `@typedef` names → their parsed types, so `@type {Alias}` on a
    /// function can supply its parameter types.
    typedefs: &'a [(String, TypeNode)],
    /// Generic `@typedef`/`@callback` names: a `@type` naming one leaves
    /// the function to contextual typing.
    generic_typedefs: &'a [String],
    changed: bool,
}

impl Walker<'_> {
    /// The JSDoc comment directly before `start` (only whitespace, and
    /// `export`/`default` keywords, between them).
    fn doc_before(&self, start: u32) -> Option<&JsDoc> {
        let index = self.docs.partition_point(|doc| doc.end <= start);
        let doc = self.docs.get(index.checked_sub(1)?)?;
        let between = self.text.get(doc.end as usize..start as usize)?;
        // `export`/`default` keywords sit between a declaration's comment
        // and the declaration itself.
        between
            .split_whitespace()
            .all(|word| matches!(word, "export" | "default" | "declare"))
            .then_some(doc)
    }

    /// The comment's own tag of this name (not one a typedef/callback owns).
    fn tag<'d>(doc: &'d JsDoc, name: &str) -> Option<&'d Tag> {
        doc.tags.iter().find(|tag| tag.name == name && !tag.owned)
    }

    fn type_of(&self, tag: &Tag) -> Option<TypeNode> {
        let param_tag = matches!(tag.name.as_str(), "param" | "arg" | "argument");
        let ty = tag.ty.as_ref().filter(|ty| !(self.names_function)(ty))?;
        if matches!(tag.name.as_str(), "returns" | "return") {
            let raw = self.text.get(ty.start..ty.end)?;
            let converted = convert(raw.trim(), false)?;
            let lead = raw.len() - raw.trim_start().len();
            return parse_return_type_text(&converted, ty.start + lead);
        }
        parse_type_at(self.text, ty, param_tag)
    }

    fn stmts(&mut self, stmts: &mut [Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        let doc = self.doc_before(stmt.span.start).cloned();
        match &mut stmt.kind {
            StmtKind::Var(var_stmt) => {
                if let Some(doc) = &doc {
                    self.annotate_var(var_stmt, doc);
                }
                for declaration in &mut var_stmt.declarations {
                    if let Some(init) = &mut declaration.init {
                        self.expr(init);
                    }
                }
            }
            StmtKind::FnDecl(function) => {
                if let Some(doc) = &doc {
                    self.annotate_function(
                        &mut function.params,
                        &mut function.return_type,
                        &mut function.type_params,
                        doc,
                    );
                }
                self.annotate_inline_params(&mut function.params);
                if let Some(body) = &mut function.body {
                    self.stmts(body);
                }
            }
            StmtKind::Export(export) => match &mut export.kind {
                ExportDeclKind::Decl(inner) | ExportDeclKind::DefaultDecl(inner) => {
                    if let Some(doc) = &doc {
                        match &mut inner.kind {
                            StmtKind::Var(var_stmt) => self.annotate_var(var_stmt, doc),
                            StmtKind::FnDecl(function) => self.annotate_function(
                                &mut function.params,
                                &mut function.return_type,
                                &mut function.type_params,
                                doc,
                            ),
                            _ => {}
                        }
                    }
                    // The inner statement's own walk (its doc was applied).
                    match &mut inner.kind {
                        StmtKind::Var(var_stmt) => {
                            for declaration in &mut var_stmt.declarations {
                                if let Some(init) = &mut declaration.init {
                                    self.expr(init);
                                }
                            }
                        }
                        StmtKind::FnDecl(function) => {
                            if let Some(body) = &mut function.body {
                                self.stmts(body);
                            }
                        }
                        _ => self.stmt(inner),
                    }
                }
                ExportDeclKind::Default(expr) => self.expr(expr),
                _ => {}
            },
            StmtKind::Expr(expr) | StmtKind::Throw(expr) | StmtKind::ExportAssign(expr) => {
                self.expr(expr)
            }
            StmtKind::Return(Some(expr)) => {
                // A comment on `return function…` documents that function.
                if let Some(doc) = &doc {
                    self.annotate_function_expr(expr, doc);
                }
                self.expr(expr)
            }
            StmtKind::If(if_stmt) => {
                self.expr(&mut if_stmt.test);
                self.stmt(&mut if_stmt.consequent);
                if let Some(alternate) = &mut if_stmt.alternate {
                    self.stmt(alternate);
                }
            }
            StmtKind::While(w) => {
                self.expr(&mut w.test);
                self.stmt(&mut w.body);
            }
            StmtKind::DoWhile(w) => {
                self.stmt(&mut w.body);
                self.expr(&mut w.test);
            }
            StmtKind::For(f) => {
                if let Some(ForInit::Var(var_stmt)) = &mut f.init {
                    for declaration in &mut var_stmt.declarations {
                        if let Some(init) = &mut declaration.init {
                            self.expr(init);
                        }
                    }
                }
                self.stmt(&mut f.body);
            }
            StmtKind::ForIn(f) => self.stmt(&mut f.body),
            StmtKind::ForOf(f) => self.stmt(&mut f.body),
            StmtKind::Block(stmts) => self.stmts(stmts),
            StmtKind::Try(t) => {
                self.stmts(&mut t.block);
                if let Some(handler) = &mut t.handler {
                    self.stmts(&mut handler.body);
                }
                if let Some(finalizer) = &mut t.finalizer {
                    self.stmts(finalizer);
                }
            }
            StmtKind::Switch(s) => {
                self.expr(&mut s.discriminant);
                for case in &mut s.cases {
                    self.stmts(&mut case.consequent);
                }
            }
            StmtKind::Labeled(l) => self.stmt(&mut l.body),
            StmtKind::ClassDecl(class) => {
                if let Some(doc) = &doc {
                    if class.type_params.is_none() {
                        if let Some(params) = self.template_params(doc, false) {
                            class.type_params = Some(params);
                            self.changed = true;
                        }
                    }
                }
                self.class(class)
            }
            _ => {}
        }
    }

    fn class(&mut self, class: &mut ClassDecl) {
        for member in &mut class.members {
            let doc = self.doc_before(member.span.start).cloned();
            match &mut member.kind {
                ClassMemberKind::Method(method) => {
                    if let Some(doc) = &doc {
                        self.annotate_function(
                            &mut method.params,
                            &mut method.return_type,
                            &mut method.type_params,
                            doc,
                        );
                    }
                    self.annotate_inline_params(&mut method.params);
                    if let Some(body) = &mut method.body {
                        self.stmts(body);
                    }
                }
                ClassMemberKind::Constructor(ctor) => {
                    if let Some(doc) = &doc {
                        let (mut no_return, mut no_type_params) = (None, None);
                        self.annotate_function(
                            &mut ctor.params,
                            &mut no_return,
                            &mut no_type_params,
                            doc,
                        );
                    }
                    self.annotate_inline_params(&mut ctor.params);
                    if let Some(body) = &mut ctor.body {
                        self.stmts(body);
                    }
                }
                ClassMemberKind::Property(property) => {
                    if property.type_ann.is_none() {
                        if let Some(ty) = doc
                            .as_ref()
                            .and_then(|doc| Self::tag(doc, "type"))
                            .and_then(|tag| self.type_of(tag))
                        {
                            property.type_ann = Some(ty);
                            self.changed = true;
                        }
                    }
                    if let Some(value) = &mut property.initializer {
                        // `@param`/`@returns` on the property document its
                        // function initializer.
                        if let Some(doc) = &doc {
                            match &mut value.kind {
                                ExprKind::Arrow(arrow) => self.annotate_function(
                                    &mut arrow.params,
                                    &mut arrow.return_type,
                                    &mut arrow.type_params,
                                    doc,
                                ),
                                ExprKind::FnExpr(function) => self.annotate_function(
                                    &mut function.params,
                                    &mut function.return_type,
                                    &mut function.type_params,
                                    doc,
                                ),
                                _ => {}
                            }
                        }
                        self.expr(value);
                    }
                }
                _ => {}
            }
        }
    }

    fn annotate_var(&mut self, var_stmt: &mut VarStmt, doc: &JsDoc) {
        // A declared type that is not converted yet reads as `any`.
        let declared = Self::tag(doc, "type").and_then(|tag| {
            self.type_of(tag).or_else(|| {
                tag.ty
                    .as_ref()
                    .and_then(|ty| parse_type_text("any", ty.start))
            })
        });
        // `@satisfies` on the declaration checks its initializer.
        let satisfies_tag = Self::tag(doc, "satisfies");
        if let Some((type_node, tag_span)) =
            satisfies_tag.and_then(|tag| self.type_of(tag).map(|ty| (ty, tag.name_span)))
        {
            for declaration in &mut var_stmt.declarations {
                if let Some(init) = &mut declaration.init {
                    // An empty `{}` initializer is an open JavaScript literal.
                    if matches!(&init.kind, ExprKind::ObjectLit(props) if props.is_empty()) {
                        continue;
                    }
                    let span = tag_span;
                    let inner = std::mem::replace(
                        init.as_mut(),
                        Expr {
                            kind: ExprKind::Omitted,
                            span,
                        },
                    );
                    **init = Expr {
                        kind: ExprKind::Satisfies(Box::new(SatisfiesExpr {
                            expr: Box::new(inner),
                            type_node: type_node.clone(),
                        })),
                        span,
                    };
                    self.changed = true;
                }
            }
        }
        for declaration in &mut var_stmt.declarations {
            if declaration.type_ann.is_none() {
                if let Some(ty) = &declared {
                    declaration.type_ann = Some(ty.clone());
                    self.changed = true;
                    // The initializer function is typed by the signature too.
                    if let Some(init) = &mut declaration.init {
                        match &mut init.kind {
                            ExprKind::FnExpr(function) => self.apply_function_type(
                                &mut function.params,
                                &mut function.return_type,
                                &mut function.type_params,
                                ty,
                            ),
                            ExprKind::Arrow(arrow) => self.apply_function_type(
                                &mut arrow.params,
                                &mut arrow.return_type,
                                &mut arrow.type_params,
                                ty,
                            ),
                            _ => {}
                        }
                    }
                    continue;
                }
                // A `@type` that did not parse still documents a function.
                if let (Some(tag), Some(init)) = (Self::tag(doc, "type"), &mut declaration.init) {
                    let at = tag.ty.as_ref().map_or(0, |ty| ty.start);
                    match &mut init.kind {
                        ExprKind::FnExpr(function) => {
                            self.untyped_params_any(&mut function.params, at)
                        }
                        ExprKind::Arrow(arrow) => self.untyped_params_any(&mut arrow.params, at),
                        _ => {}
                    }
                }
            }
            // `@param`/`@returns` on `const f = function/arrow`.
            if let Some(init) = &mut declaration.init {
                match &mut init.kind {
                    ExprKind::FnExpr(function) => self.annotate_function(
                        &mut function.params,
                        &mut function.return_type,
                        &mut function.type_params,
                        doc,
                    ),
                    ExprKind::Arrow(arrow) => self.annotate_function(
                        &mut arrow.params,
                        &mut arrow.return_type,
                        &mut arrow.type_params,
                        doc,
                    ),
                    _ => {}
                }
            }
        }
    }

    fn annotate_function(
        &mut self,
        params: &mut [Param],
        return_type: &mut Option<TypeNode>,
        type_params: &mut Option<Vec<TypeParam>>,
        doc: &JsDoc,
    ) {
        if type_params.is_none() {
            if let Some(params) = self.template_params(doc, false) {
                *type_params = Some(params);
                self.changed = true;
            }
        }
        // `@type {Signature}` on the function itself.
        if let Some(tag) = Self::tag(doc, "type") {
            match self.type_of(tag) {
                Some(ty) => self.apply_function_type(params, return_type, type_params, &ty),
                None => {
                    let at = tag.ty.as_ref().map_or(0, |ty| ty.start);
                    self.untyped_params_any(params, at);
                }
            }
        }
        // Top-level `@param` tags in order: a destructured parameter takes
        // the tag at its position (its name cannot match).
        let top_level: Vec<&Tag> = doc
            .tags
            .iter()
            .filter(|tag| {
                !tag.owned
                    && matches!(tag.name.as_str(), "param" | "arg" | "argument")
                    && tag
                        .target
                        .as_ref()
                        .is_some_and(|target| !target.contains('.'))
            })
            .collect();
        for (index, param) in params.iter_mut().enumerate() {
            if param.type_ann.is_some() || matches!(param.name.kind, PatKind::Ident(_)) {
                continue;
            }
            let Some(tag) = top_level.get(index) else {
                continue;
            };
            let root = tag.target.clone().unwrap_or_default();
            // `@param {Object} opts` + `@param {T} opts.x`: an object type.
            let nested: Vec<Tag> = doc
                .tags
                .iter()
                .filter(|other| {
                    !other.owned
                        && matches!(other.name.as_str(), "param" | "arg" | "argument")
                        && other
                            .target
                            .as_ref()
                            .is_some_and(|target| target.starts_with(&format!("{root}.")))
                })
                .map(|other| {
                    let mut child = other.clone();
                    child.target = child
                        .target
                        .as_ref()
                        .map(|target| target[root.len() + 1..].to_string());
                    child
                })
                .collect();
            let at = tag.ty.as_ref().map_or(0, |ty| ty.start);
            let declared = tag
                .ty
                .as_ref()
                .and_then(|ty| self.text.get(ty.start..ty.end))
                .unwrap_or("")
                .trim();
            let ty = if matches!(declared, "Object" | "object") && !nested.is_empty() {
                let refs: Vec<&Tag> = nested.iter().collect();
                object_type_text(self.text, &refs).and_then(|text| parse_type_text(&text, at))
            } else {
                self.type_of(tag)
            };
            param.type_ann = ty.or_else(|| parse_type_text("any", at));
            self.changed = true;
        }
        for tag in &doc.tags {
            if tag.owned {
                continue;
            }
            match tag.name.as_str() {
                "param" | "arg" | "argument" => {
                    let Some(target) = &tag.target else { continue };
                    let Some(param) = params.iter_mut().find(|param| {
                        matches!(&param.name.kind, PatKind::Ident(name) if name == target.as_str())
                    }) else {
                        continue;
                    };
                    if param.type_ann.is_none() {
                        // A documented parameter whose type is not converted
                        // yet reads as `any` rather than as undocumented
                        // (which would be an implicit any).
                        let ty = self.type_of(tag).or_else(|| {
                            tag.ty
                                .as_ref()
                                .and_then(|ty| parse_type_text("any", ty.start))
                        });
                        if let Some(ty) = ty {
                            param.type_ann = Some(ty);
                            if tag.optional && param.initializer.is_none() {
                                param.optional = true;
                            }
                            self.changed = true;
                        }
                    }
                }
                "returns" | "return" => {
                    if return_type.is_none() {
                        if let Some(ty) = self.type_of(tag) {
                            *return_type = Some(ty);
                            self.changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn template_params(&self, doc: &JsDoc, owned: bool) -> Option<Vec<TypeParam>> {
        let tags: Vec<&Tag> = doc
            .tags
            .iter()
            .filter(|tag| tag.name == "template" && tag.owned == owned)
            .collect();
        template_params_from(self.text, &tags)
    }

    /// The function type a `@type` names, through a `@typedef` alias.
    fn function_type_of(&self, ty: &TypeNode) -> Option<FnTypeNode> {
        match &ty.kind {
            TypeNodeKind::Function(function) => Some((**function).clone()),
            TypeNodeKind::Reference(reference) => {
                let ExprKind::Ident(name) = &reference.name.kind else {
                    return None;
                };
                let name = name.as_str();
                self.typedefs
                    .iter()
                    .find(|(alias, _)| alias == name)
                    .and_then(|(_, body)| match &body.kind {
                        TypeNodeKind::Function(function) => Some((**function).clone()),
                        _ => None,
                    })
            }
            _ => None,
        }
    }

    /// `@type {(a: A) => R}` on a function: its parameters and return take
    /// the signature's types. A type that is not a known function type
    /// still documents the function, so its parameters read as `any`.
    fn apply_function_type(
        &mut self,
        params: &mut [Param],
        return_type: &mut Option<TypeNode>,
        type_params: &mut Option<Vec<TypeParam>>,
        ty: &TypeNode,
    ) {
        if let TypeNodeKind::Reference(reference) = &ty.kind {
            let generic = reference.type_args.is_some()
                || matches!(&reference.name.kind, ExprKind::Ident(name)
                    if self.generic_typedefs.iter().any(|generic| generic == name.as_str()));
            if generic {
                return;
            }
        }
        match self.function_type_of(ty) {
            Some(function) => {
                let sources = function.params.iter().filter(
                    |source| !matches!(&source.name.kind, PatKind::Ident(name) if name == "this"),
                );
                for (param, source) in params.iter_mut().zip(sources) {
                    if param.type_ann.is_none() {
                        param.type_ann = source.type_ann.clone();
                        if source.optional && param.initializer.is_none() {
                            param.optional = true;
                        }
                    }
                }
                if return_type.is_none() {
                    *return_type = Some((*function.return_type).clone());
                }
                if type_params.is_none() {
                    *type_params = function.type_params.clone();
                }
            }
            None => self.untyped_params_any(params, ty.span.start as usize),
        }
        self.changed = true;
    }

    fn untyped_params_any(&mut self, params: &mut [Param], at: usize) {
        for param in params.iter_mut() {
            if param.type_ann.is_none() {
                param.type_ann = parse_type_text(if param.dotdotdot { "any[]" } else { "any" }, at);
                self.changed = true;
            }
        }
    }

    /// `/** @type {T} */ name` written in a parameter list.
    fn annotate_inline_params(&mut self, params: &mut [Param]) {
        for param in params.iter_mut() {
            if param.type_ann.is_some() {
                continue;
            }
            let Some(doc) = self.doc_before(param.span.start).cloned() else {
                continue;
            };
            if let Some(ty) = Self::tag(&doc, "type").and_then(|tag| self.type_of(tag)) {
                param.type_ann = Some(ty);
                self.changed = true;
            }
        }
    }

    fn annotate_function_expr(&mut self, expr: &mut Expr, doc: &JsDoc) {
        match &mut expr.kind {
            ExprKind::FnExpr(function) => self.annotate_function(
                &mut function.params,
                &mut function.return_type,
                &mut function.type_params,
                doc,
            ),
            ExprKind::Arrow(arrow) => self.annotate_function(
                &mut arrow.params,
                &mut arrow.return_type,
                &mut arrow.type_params,
                doc,
            ),
            ExprKind::Paren(inner) => self.annotate_function_expr(inner, doc),
            _ => {}
        }
    }

    fn function_body(&mut self, body: &mut Option<Vec<Stmt>>) {
        if let Some(body) = body {
            self.stmts(body);
        }
    }

    fn expr(&mut self, expr: &mut Expr) {
        // `/** @type {T} */ (e)` and `/** @satisfies {T} */ (e)`.
        if matches!(expr.kind, ExprKind::Paren(_)) {
            if let Some(doc) = self.doc_before(expr.span.start).cloned() {
                // A cast whose type is not converted yet asserts `any`.
                let cast = Self::tag(&doc, "type").and_then(|tag| {
                    self.type_of(tag).or_else(|| {
                        tag.ty
                            .as_ref()
                            .and_then(|ty| parse_type_text("any", ty.start))
                    })
                });
                let satisfies = Self::tag(&doc, "satisfies")
                    .and_then(|tag| self.type_of(tag).map(|ty| (ty, tag.name_span)));
                if let ExprKind::Paren(inner) = &mut expr.kind {
                    self.expr(inner);
                }
                // The parenthesized expression itself is the operand, so a
                // fresh object literal keeps its excess-property check.
                let operand = |expr: &mut Expr| -> Box<Expr> {
                    match std::mem::replace(&mut expr.kind, ExprKind::Omitted) {
                        ExprKind::Paren(inner) => inner,
                        other => Box::new(Expr {
                            kind: other,
                            span: expr.span,
                        }),
                    }
                };
                if let Some((type_node, tag_span)) = satisfies {
                    let inner = operand(expr);
                    expr.kind = ExprKind::Satisfies(Box::new(SatisfiesExpr {
                        expr: inner,
                        type_node,
                    }));
                    expr.span = tag_span;
                    self.changed = true;
                } else if let Some(type_node) = cast {
                    let mut inner = operand(expr);
                    // A cast function is typed by the asserted signature.
                    match &mut inner.kind {
                        ExprKind::FnExpr(function) => self.apply_function_type(
                            &mut function.params,
                            &mut function.return_type,
                            &mut function.type_params,
                            &type_node,
                        ),
                        ExprKind::Arrow(arrow) => self.apply_function_type(
                            &mut arrow.params,
                            &mut arrow.return_type,
                            &mut arrow.type_params,
                            &type_node,
                        ),
                        _ => {}
                    }
                    expr.kind = ExprKind::As(Box::new(AsExpr {
                        expr: inner,
                        type_node,
                    }));
                    self.changed = true;
                }
                return;
            }
        }
        match &mut expr.kind {
            ExprKind::Paren(inner)
            | ExprKind::NonNull(inner)
            | ExprKind::Spread(inner)
            | ExprKind::Await(inner)
            | ExprKind::Delete(inner)
            | ExprKind::Typeof(inner)
            | ExprKind::Void(inner) => self.expr(inner),
            ExprKind::Yield(_, Some(inner)) => self.expr(inner),
            ExprKind::ArrayLit(items) => {
                for item in items.iter_mut().flatten() {
                    self.expr(item);
                }
            }
            ExprKind::ObjectLit(props) => {
                for prop in props {
                    match prop {
                        ObjLitProp::Property(property) => self.expr(&mut property.value),
                        ObjLitProp::Spread(inner, _) => self.expr(inner),
                        ObjLitProp::Method(method) => self.stmts(&mut method.body),
                        ObjLitProp::Get(accessor) | ObjLitProp::Set(accessor) => {
                            self.stmts(&mut accessor.body)
                        }
                        _ => {}
                    }
                }
            }
            ExprKind::FnExpr(function) => {
                if let Some(doc) = self.doc_before(expr.span.start).cloned() {
                    self.annotate_function(
                        &mut function.params,
                        &mut function.return_type,
                        &mut function.type_params,
                        &doc,
                    );
                }
                self.annotate_inline_params(&mut function.params);
                self.function_body(&mut function.body)
            }
            ExprKind::Arrow(arrow) => {
                if let Some(doc) = self.doc_before(expr.span.start).cloned() {
                    self.annotate_function(
                        &mut arrow.params,
                        &mut arrow.return_type,
                        &mut arrow.type_params,
                        &doc,
                    );
                }
                self.annotate_inline_params(&mut arrow.params);
                match &mut arrow.body {
                    ArrowBody::Block(stmts) => self.stmts(stmts),
                    ArrowBody::Expr(body) => self.expr(body),
                }
            }
            ExprKind::ClassExpr(class) => self.class(class),
            ExprKind::Call(call) => {
                self.expr(&mut call.callee);
                for arg in &mut call.args {
                    self.expr(arg);
                }
            }
            ExprKind::New(new) => {
                self.expr(&mut new.callee);
                for arg in new.args.iter_mut().flatten() {
                    self.expr(arg);
                }
            }
            ExprKind::Member(member) => self.expr(&mut member.object),
            ExprKind::ElemAccess(access) => {
                self.expr(&mut access.object);
                self.expr(&mut access.index);
            }
            ExprKind::Cond(cond) => {
                self.expr(&mut cond.test);
                self.expr(&mut cond.consequent);
                self.expr(&mut cond.alternate);
            }
            ExprKind::Binary(binary) => {
                self.expr(&mut binary.left);
                self.expr(&mut binary.right);
            }
            ExprKind::Unary(unary) => self.expr(&mut unary.argument),
            ExprKind::Assign(assign) => self.expr(&mut assign.right),
            ExprKind::Comma(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            _ => {}
        }
    }
}
