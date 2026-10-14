//! Constructor references use constructor declarations and construction sites,
//! rather than every occurrence of the containing class name.
use super::*;
use crate::ProjectState;

fn member_access(expr: &Expr) -> Option<(&Expr, &str, Span)> {
    match &expr.kind {
        ExprKind::Member(member) => Some((
            &member.object,
            &member.property,
            Span::new(expr.span.end - member.property.len() as u32, expr.span.end),
        )),
        ExprKind::ElemAccess(member) => match &member.index.kind {
            ExprKind::StrLit(name) if !name.contains('\\') => {
                Some((&member.object, name, member.index.span))
            }
            _ => None,
        },
        _ => None,
    }
}

fn local_class_member(
    index: &Index<'_>,
    table: &tsc_rs_symbols::SymbolTable,
    receiver: &Expr,
    name: &str,
) -> Option<u32> {
    let (class_binding, static_member) = match &receiver.kind {
        ExprKind::Ident(identifier) => {
            let binding = index.binding(identifier, receiver.span.start)?;
            if binding.flags & tsc_rs_symbols::SYM_CLASS != 0 {
                (binding, true)
            } else {
                let (annotation, initializer) =
                    if let Some(variable) = index.variable(identifier, receiver.span.start) {
                        (variable.type_ann.as_ref(), variable.init.as_deref())
                    } else {
                        let parameter = index.parameters.iter().find(|parameter| {
                            binding
                                .declarations
                                .iter()
                                .any(|declaration| declaration.span == parameter.name.span)
                        })?;
                        (parameter.type_ann.as_ref(), None)
                    };
                let constructor = if let Some(annotation) = annotation {
                    let TypeNodeKind::Reference(reference) = &annotation.kind else {
                        return None;
                    };
                    reference.name.as_ref()
                } else {
                    let ExprKind::New(call) = &initializer?.kind else {
                        return None;
                    };
                    call.callee.as_ref()
                };
                let ExprKind::Ident(name) = &constructor.kind else {
                    return None;
                };
                (index.binding(name, constructor.span.start)?, false)
            }
        }
        ExprKind::New(call) => {
            let ExprKind::Ident(name) = &call.callee.kind else {
                return None;
            };
            (index.binding(name, call.callee.span.start)?, false)
        }
        _ => return None,
    };
    if class_binding.flags & tsc_rs_symbols::SYM_CLASS == 0 || class_binding.declarations.len() != 1
    {
        return None;
    }
    let scope = index.symbols.scope_graph.binding_scope(class_binding.id)?;
    if index.symbols.symbols.iter().any(|symbol| {
        symbol.id != class_binding.id
            && symbol.name == class_binding.name
            && index.symbols.scope_graph.binding_scope(symbol.id) == Some(scope)
    }) {
        return None;
    }
    let declaration = &class_binding.declarations[0];
    let class = index
        .classes
        .iter()
        .find_map(|(class, _)| (class.name_span == Some(declaration.span)).then_some(*class))?;
    if class.extends.is_some() || !class.implements.is_empty() {
        return None;
    }
    let mut matching = Vec::new();
    for member in &class.members {
        let (property, modifiers) = match &member.kind {
            ClassMemberKind::Property(p) => (&p.name, p.modifiers),
            ClassMemberKind::Method(m) => (&m.name, m.modifiers),
            ClassMemberKind::GetAccessor(a) | ClassMemberKind::SetAccessor(a) => {
                (&a.name, a.modifiers)
            }
            ClassMemberKind::Constructor(c)
                if c.params.iter().any(|p| {
                    p.modifiers & (MOD_PUBLIC | MOD_PRIVATE | MOD_PROTECTED | MOD_READONLY) != 0
                }) =>
            {
                return None
            }
            _ => continue,
        };
        let property_name = match property {
            PropName::Ident(n, _) | PropName::String(n, _) if !n.contains('\\') => n,
            _ => return None,
        };
        if property_name == name {
            matching.push((property.span(), modifiers));
        }
    }
    let [(span, modifiers)] = matching.as_slice() else {
        return None;
    };
    if (modifiers & MOD_STATIC != 0) != static_member {
        return None;
    }
    table.position_to_symbol.get(&span.start).copied()
}

pub(crate) fn class_member_at(
    qe: &QueryEngine,
    file: &str,
    table: &tsc_rs_symbols::SymbolTable,
    offset: u32,
) -> Option<u32> {
    let sf = tsc_rs_parser::parse(file, qe.get_source_file(file)?);
    let index = Index::new(&sf);
    let (receiver, name, _) = index
        .expressions
        .iter()
        .filter_map(|expression| member_access(expression))
        .find(|(_, _, span)| contains(*span, offset))?;
    local_class_member(&index, table, receiver, name)
}

/// Distinct class declarations without heritage have independent members.
/// Textual reference recovery must not join their declaration sites merely
/// because the members have the same name.
pub(crate) fn conflicting_member_declarations(
    qe: &QueryEngine,
    file: &str,
    table: &tsc_rs_symbols::SymbolTable,
    member_id: u32,
) -> Vec<Span> {
    let Some(member) = table.get_symbol(member_id) else {
        return Vec::new();
    };
    let owners: Vec<_> = table
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.flags & tsc_rs_symbols::SYM_CLASS != 0
                && symbol.members.get(&member.name) == Some(&member_id)
        })
        .collect();
    let [owner] = owners.as_slice() else {
        return Vec::new();
    };
    let conflicts: Vec<_> = table
        .symbols
        .iter()
        .filter(|symbol| {
            symbol.id != owner.id
                && symbol.flags & tsc_rs_symbols::SYM_CLASS != 0
                && symbol.members.contains_key(&member.name)
        })
        .collect();
    if conflicts.is_empty() {
        return Vec::new();
    }
    let Some(source) = qe.get_source_file(file) else {
        return Vec::new();
    };
    let sf = tsc_rs_parser::parse(file, source);
    let index = Index::new(&sf);
    let independent = |symbol: &tsc_rs_symbols::Symbol| {
        symbol.declarations.len() == 1
            && symbol.declarations[0].file_name == file
            && index.classes.iter().any(|(class, _)| {
                class.name_span == Some(symbol.declarations[0].span)
                    && class.extends.is_none()
                    && class.implements.is_empty()
            })
    };
    if !independent(owner) {
        return Vec::new();
    }
    let mut excluded: Vec<_> = conflicts
        .into_iter()
        .filter(|symbol| independent(symbol))
        .filter_map(|symbol| table.get_symbol(symbol.members[&member.name]))
        .flat_map(|symbol| &symbol.declarations)
        .filter(|declaration| declaration.file_name == file)
        .map(|declaration| declaration.span)
        .collect();
    for expression in &index.expressions {
        let Some((receiver, name, span)) = member_access(expression) else {
            continue;
        };
        if name != member.name {
            continue;
        }
        if let Some(other_id) = local_class_member(&index, table, receiver, name) {
            if other_id != member_id {
                excluded.push(span);
            }
        }
    }
    excluded
}

pub(crate) fn constructors(
    qe: &QueryEngine,
    file: &str,
    offset: u32,
    project: Option<&ProjectState>,
) -> Option<Vec<Target>> {
    let source = qe.get_source_file(file)?;
    if !matches!(
        crate::extract_token_at(source, offset).as_str(),
        "constructor" | "public" | "private" | "protected"
    ) {
        return None;
    }
    let sf = tsc_rs_parser::parse(file, source);
    let index = Index::new(&sf);
    let class = index.classes.iter().find_map(|(class, _)| {
        class.members.iter().any(|member| matches!(&member.kind,
            ClassMemberKind::Constructor(c) if member.span.start <= offset && offset <= c.keyword_span.end
        )).then_some(*class)
    })?;
    let keywords: Vec<_> = class
        .members
        .iter()
        .filter_map(|m| match &m.kind {
            ClassMemberKind::Constructor(c) => Some(c.keyword_span),
            _ => None,
        })
        .collect();
    let target = (file.to_owned(), keywords.first()?.start);
    let is_target = |owner: Option<Target>| owner.is_some_and(|(f, s)| (f, s.start) == target);
    let mut references: Vec<_> = keywords.iter().map(|s| (file.to_owned(), *s)).collect();
    let mut paths = vec![file.to_owned()];
    if let Some(project) = project {
        paths.extend(
            project
                .files
                .keys()
                .filter(|p| p.as_str() != file && p.as_str() != "__lib__.d.ts")
                .cloned(),
        );
    }
    for path in paths {
        let Some(source) = qe.get_source_file(&path) else {
            continue;
        };
        let sf = tsc_rs_parser::parse(&path, source);
        let index = Index::new(&sf);
        for stmt in &sf.statements {
            if let StmtKind::Import(import) = &stmt.kind {
                if let ImportClause::Named { named, .. } = &import.specifiers {
                    for spec in named.iter().filter(|s| s.imported.is_some()) {
                        let remote = imported_binding(qe, &sf, &index, &spec.local, &mut vec![]);
                        if is_target(remote.and_then(|t| constructor_owner(qe, t, &mut vec![]))) {
                            let start = spec.span.start;
                            references.push((
                                path.clone(),
                                Span::new(
                                    start,
                                    start + spec.imported.as_ref().unwrap().len() as u32,
                                ),
                            ));
                        }
                    }
                }
            }
        }
        for expr in &index.expressions {
            let callee = match &expr.kind {
                ExprKind::New(call) => call.callee.as_ref(),
                ExprKind::Call(call) if matches!(call.callee.kind, ExprKind::Super) => {
                    call.callee.as_ref()
                }
                _ => continue,
            };
            let class = class_reference(qe, &sf, &index, callee);
            if is_target(class.and_then(|t| constructor_owner(qe, t, &mut vec![]))) {
                let span = match &callee.kind {
                    ExprKind::Member(m) => {
                        Span::new(callee.span.end - m.property.len() as u32, callee.span.end)
                    }
                    _ => callee.span,
                };
                references.push((path.clone(), span));
            }
        }
    }
    references.sort_by_key(|(file, span)| (file.clone(), span.start, span.end));
    references.dedup();
    Some(references)
}

fn class_reference(
    qe: &QueryEngine,
    sf: &SourceFile,
    index: &Index<'_>,
    expr: &Expr,
) -> Option<Target> {
    match &expr.kind {
        ExprKind::Ident(name) => {
            if let Some(symbol) = index.binding(name, expr.span.start) {
                if let Some((class, _)) = index.classes.iter().find(|(c, _)| {
                    symbol.declarations.iter().any(|d| {
                        d.span.start == c.span.start
                            || c.name_span.is_some_and(|s| s.start == d.span.start)
                    })
                }) {
                    return Some((sf.file_name.clone(), class.name_span.unwrap_or(class.span)));
                }
                // Value aliases are deliberately not constructor references.
                if symbol.flags & tsc_rs_symbols::SYM_IMPORT == 0 {
                    return None;
                }
            }
            imported_binding(qe, sf, index, name, &mut vec![])
        }
        ExprKind::Member(m) => {
            let ExprKind::Ident(name) = &m.object.kind else {
                return None;
            };
            for stmt in &sf.statements {
                if let StmtKind::Import(i) = &stmt.kind {
                    if matches!(&i.specifiers, ImportClause::Named { namespace: Some(n), .. } if n==name)
                    {
                        return export_target(
                            qe,
                            &module_path(qe, &i.source, &sf.file_name)?,
                            Some(&m.property),
                            &mut vec![],
                        );
                    }
                }
            }
            None
        }
        ExprKind::This => {
            let class = index.enclosing_class(expr.span.start)?;
            // Only static members construct their enclosing class through this.
            if !class.members.iter().any(|m| {
                contains(m.span, expr.span.start)
                    && match &m.kind {
                        ClassMemberKind::Method(m) => m.modifiers & MOD_STATIC != 0,
                        ClassMemberKind::Property(p) => p.modifiers & MOD_STATIC != 0,
                        ClassMemberKind::StaticBlock(_) => true,
                        _ => false,
                    }
            }) {
                return None;
            }
            Some((sf.file_name.clone(), class.name_span.unwrap_or(class.span)))
        }
        ExprKind::Super => {
            let class = index.enclosing_class(expr.span.start)?;
            class_reference(qe, sf, index, class.extends.as_deref()?)
        }
        ExprKind::Paren(inner) => class_reference(qe, sf, index, inner),
        _ => None,
    }
}

fn constructor_owner(
    qe: &QueryEngine,
    target: Target,
    seen: &mut Vec<(String, u32)>,
) -> Option<Target> {
    let key = (target.0.clone(), target.1.start);
    if seen.contains(&key) || seen.len() >= 32 {
        return None;
    }
    seen.push(key);
    let sf = tsc_rs_parser::parse(&target.0, qe.get_source_file(&target.0)?);
    let index = Index::new(&sf);
    let (class, _) = index.classes.iter().find(|(c, _)| {
        c.span.start == target.1.start || c.name_span.is_some_and(|s| s.start == target.1.start)
    })?;
    if let Some(span) = class.members.iter().find_map(|m| match &m.kind {
        ClassMemberKind::Constructor(c) => Some(c.keyword_span),
        _ => None,
    }) {
        return Some((target.0, span));
    }
    constructor_owner(
        qe,
        class_reference(qe, &sf, &index, class.extends.as_deref()?)?,
        seen,
    )
}
