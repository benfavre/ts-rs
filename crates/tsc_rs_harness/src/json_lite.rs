//! Minimal JSON reader for package.json / tsconfig.json fragments in test
//! cases (comments allowed, trailing commas tolerated).

use super::strip_json_comments;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub(crate) fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

pub(crate) fn parse_json_lenient(src: &str) -> Option<JsonValue> {
    let cleaned = strip_json_comments(src);
    let chars: Vec<char> = cleaned.chars().collect();
    let mut pos = 0;
    let value = parse_value(&chars, &mut pos)?;
    Some(value)
}

fn skip_ws(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn parse_value(chars: &[char], pos: &mut usize) -> Option<JsonValue> {
    skip_ws(chars, pos);
    let c = *chars.get(*pos)?;
    match c {
        '{' => {
            *pos += 1;
            let mut entries = Vec::new();
            loop {
                skip_ws(chars, pos);
                match chars.get(*pos)? {
                    '}' => {
                        *pos += 1;
                        return Some(JsonValue::Object(entries));
                    }
                    ',' => {
                        *pos += 1;
                    }
                    '"' => {
                        let key = parse_string(chars, pos)?;
                        skip_ws(chars, pos);
                        if chars.get(*pos) != Some(&':') {
                            return None;
                        }
                        *pos += 1;
                        let value = parse_value(chars, pos)?;
                        entries.push((key, value));
                    }
                    _ => return None,
                }
            }
        }
        '[' => {
            *pos += 1;
            let mut items = Vec::new();
            loop {
                skip_ws(chars, pos);
                match chars.get(*pos)? {
                    ']' => {
                        *pos += 1;
                        return Some(JsonValue::Array(items));
                    }
                    ',' => {
                        *pos += 1;
                    }
                    _ => items.push(parse_value(chars, pos)?),
                }
            }
        }
        '"' => parse_string(chars, pos).map(JsonValue::String),
        't' if chars[*pos..].starts_with(&['t', 'r', 'u', 'e']) => {
            *pos += 4;
            Some(JsonValue::Bool(true))
        }
        'f' if chars[*pos..].starts_with(&['f', 'a', 'l', 's', 'e']) => {
            *pos += 5;
            Some(JsonValue::Bool(false))
        }
        'n' if chars[*pos..].starts_with(&['n', 'u', 'l', 'l']) => {
            *pos += 4;
            Some(JsonValue::Null)
        }
        _ => {
            let start = *pos;
            while *pos < chars.len()
                && (chars[*pos].is_ascii_digit()
                    || matches!(chars[*pos], '-' | '+' | '.' | 'e' | 'E'))
            {
                *pos += 1;
            }
            if start == *pos {
                return None;
            }
            let text: String = chars[start..*pos].iter().collect();
            text.parse::<f64>().ok().map(JsonValue::Number)
        }
    }
}

fn parse_string(chars: &[char], pos: &mut usize) -> Option<String> {
    if chars.get(*pos) != Some(&'"') {
        return None;
    }
    *pos += 1;
    let mut out = String::new();
    while *pos < chars.len() {
        let c = chars[*pos];
        *pos += 1;
        match c {
            '"' => return Some(out),
            '\\' => {
                let escaped = *chars.get(*pos)?;
                *pos += 1;
                match escaped {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let hex: String = chars.get(*pos..*pos + 4)?.iter().collect();
                        *pos += 4;
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
    None
}

/// Outcome of looking up a subpath in a package.json `exports` map.
#[derive(Debug, PartialEq)]
pub(crate) enum ExportsLookup {
    /// The package declares no `exports`; fall back to file lookup.
    NoExportsMap,
    /// Some key matches and maps to a target.
    Target,
    /// No key matches, or the best match is `null` (an exclusion).
    Blocked,
}

/// Resolve `entry` (`.` or `./sub/path`) against a package.json text. Exact
/// keys win; otherwise the pattern key with the longest prefix (then longest
/// suffix) is taken, as in Node's PATTERN_KEY_COMPARE.
pub(crate) fn lookup_package_exports(package_json: &str, entry: &str) -> ExportsLookup {
    lookup_package_map(package_json, "exports", entry)
}

/// Resolve a `#specifier` against a package.json `imports` map.
pub(crate) fn lookup_package_imports(package_json: &str, specifier: &str) -> ExportsLookup {
    lookup_package_map(package_json, "imports", specifier)
}

fn lookup_package_map(package_json: &str, field: &str, entry: &str) -> ExportsLookup {
    let Some(root) = parse_json_lenient(package_json) else {
        return ExportsLookup::NoExportsMap;
    };
    let Some(exports) = root.get(field) else {
        return ExportsLookup::NoExportsMap;
    };
    let is_subpath_map = matches!(exports, JsonValue::Object(entries)
        if entries.iter().all(|(k, _)| k.starts_with('.') || k.starts_with('#')) && !entries.is_empty());
    if !is_subpath_map {
        return if entry == "." && *exports != JsonValue::Null {
            ExportsLookup::Target
        } else {
            ExportsLookup::Blocked
        };
    }
    let JsonValue::Object(entries) = exports else {
        return ExportsLookup::Blocked;
    };
    if let Some((_, target)) = entries.iter().find(|(k, _)| k == entry && !k.contains('*')) {
        return if *target == JsonValue::Null {
            ExportsLookup::Blocked
        } else {
            ExportsLookup::Target
        };
    }
    let mut best: Option<(usize, usize, &JsonValue)> = None;
    for (key, target) in entries {
        let Some(star) = key.find('*') else {
            continue;
        };
        let (prefix, suffix) = (&key[..star], &key[star + 1..]);
        if suffix.contains('*')
            || entry.len() < prefix.len() + suffix.len()
            || !entry.starts_with(prefix)
            || !entry.ends_with(suffix)
        {
            continue;
        }
        let rank = (prefix.len(), suffix.len());
        if best.is_none_or(|(p, s, _)| rank > (p, s)) {
            best = Some((prefix.len(), suffix.len(), target));
        }
    }
    match best {
        Some((_, _, JsonValue::Null)) | None => ExportsLookup::Blocked,
        Some(_) => ExportsLookup::Target,
    }
}

/// Target strings for `entry` in the package.json map `field`, with `*`
/// substituted and condition objects / arrays flattened. `None` when no key
/// matches; an empty vector when the best match is `null`.
/// Every condition accepted when the importer's format is unknown.
pub(crate) const ANY_CONDITIONS: &[&str] = &["types", "import", "require", "node", "default"];

pub(crate) fn package_map_targets(
    package_json: &str,
    field: &str,
    entry: &str,
) -> Option<Vec<String>> {
    package_map_targets_with_conditions(package_json, field, entry, ANY_CONDITIONS)
}

/// Like [`package_map_targets`], resolving condition objects with Node's
/// rule: keys are walked in declaration order and the first one in
/// `conditions` (or `default`) is taken.
pub(crate) fn package_map_targets_with_conditions(
    package_json: &str,
    field: &str,
    entry: &str,
    conditions: &[&str],
) -> Option<Vec<String>> {
    let root = parse_json_lenient(package_json)?;
    let map = root.get(field)?;
    let is_subpath_map = matches!(map, JsonValue::Object(entries)
        if !entries.is_empty() && entries.iter().all(|(k, _)| k.starts_with('.') || k.starts_with('#')));
    let (target, matched): (&JsonValue, String) = if !is_subpath_map {
        if entry != "." {
            return None;
        }
        (map, String::new())
    } else {
        let JsonValue::Object(entries) = map else {
            return None;
        };
        if let Some((_, target)) = entries.iter().find(|(k, _)| k == entry && !k.contains('*')) {
            (target, String::new())
        } else {
            let mut best: Option<(usize, usize, &JsonValue, String)> = None;
            for (key, target) in entries {
                let Some(star) = key.find('*') else {
                    continue;
                };
                let (prefix, suffix) = (&key[..star], &key[star + 1..]);
                if suffix.contains('*')
                    || entry.len() < prefix.len() + suffix.len()
                    || !entry.starts_with(prefix)
                    || !entry.ends_with(suffix)
                {
                    continue;
                }
                let rank = (prefix.len(), suffix.len());
                if best.as_ref().is_none_or(|(p, s, _, _)| rank > (*p, *s)) {
                    let matched = entry[prefix.len()..entry.len() - suffix.len()].to_string();
                    best = Some((prefix.len(), suffix.len(), target, matched));
                }
            }
            let (_, _, target, matched) = best?;
            (target, matched)
        }
    };
    let mut out = Vec::new();
    collect_map_targets(target, &matched, conditions, &mut out);
    Some(out)
}

fn collect_map_targets(
    value: &JsonValue,
    matched: &str,
    conditions: &[&str],
    out: &mut Vec<String>,
) {
    match value {
        JsonValue::String(s) => out.push(s.replace('*', matched)),
        JsonValue::Array(items) => items
            .iter()
            .for_each(|item| collect_map_targets(item, matched, conditions, out)),
        JsonValue::Object(entries) => {
            // Node: the first key (in declaration order) that is an active
            // condition wins; `default` always matches.
            if let Some((_, item)) = entries
                .iter()
                .find(|(key, _)| key == "default" || conditions.contains(&key.as_str()))
            {
                collect_map_targets(item, matched, conditions, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_patterns_prefer_the_most_specific_key_and_honor_null() {
        let pkg = r#"{ "name": "inner", "exports": { ".": "./index.js", "./cjs/*": "./cjs/*.cjs", "./cjs/exclude/*": null } }"#;
        assert_eq!(lookup_package_exports(pkg, "."), ExportsLookup::Target);
        assert_eq!(
            lookup_package_exports(pkg, "./cjs/a"),
            ExportsLookup::Target
        );
        assert_eq!(
            lookup_package_exports(pkg, "./cjs/exclude/index"),
            ExportsLookup::Blocked
        );
        assert_eq!(
            lookup_package_exports(pkg, "./other"),
            ExportsLookup::Blocked
        );
        assert_eq!(
            lookup_package_exports(r#"{ "name": "x" }"#, "./other"),
            ExportsLookup::NoExportsMap
        );
        assert_eq!(
            lookup_package_exports(r#"{ "exports": "./index.js" }"#, "."),
            ExportsLookup::Target
        );
        assert_eq!(
            lookup_package_exports(
                r#"{ "exports": { "import": "./a.mjs", "require": "./a.cjs" } }"#,
                "./a"
            ),
            ExportsLookup::Blocked
        );
    }
}
