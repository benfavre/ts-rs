//! Constructor references use constructor declarations and construction sites,
//! rather than every occurrence of the containing class name.
use super::*;
use crate::ProjectState;

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
