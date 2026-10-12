//! Mapping of CQL / annotation names to Rust identifiers.

const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "try",
    "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do", "final", "macro",
    "override", "priv", "typeof", "unsized", "virtual", "yield",
];

/// `GetUserById` -> `get_user_by_id`, `HTTPServer` -> `http_server`.
pub fn to_snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev = i.checked_sub(1).map(|p| chars[p]);
            let next = chars.get(i + 1).copied();
            let boundary = prev.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            });
            if boundary && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out
}

/// `get_user` -> `GetUser`; already-PascalCase input is kept.
pub fn to_pascal_case(s: &str) -> String {
    let mut out = String::new();
    for part in s.split(|c: char| !c.is_ascii_alphanumeric()) {
        let mut cs = part.chars();
        if let Some(first) = cs.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(cs.as_str());
        }
    }
    out
}

/// A valid, non-keyword Rust identifier for a CQL name (keywords get a
/// trailing underscore).
pub fn field_ident(cql_name: &str) -> String {
    let mut s = to_snake_case(cql_name).trim_matches('_').to_string();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    if KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    }
    s
}

pub fn const_ident(snake: &str) -> String {
    snake.to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case() {
        assert_eq!(to_snake_case("GetUserById"), "get_user_by_id");
        assert_eq!(to_snake_case("getUserByID"), "get_user_by_id");
        assert_eq!(to_snake_case("HTTPServer"), "http_server");
        assert_eq!(to_snake_case("already_snake"), "already_snake");
        assert_eq!(to_snake_case("Query2Go"), "query2_go");
    }

    #[test]
    fn pascal_case() {
        assert_eq!(to_pascal_case("get_user"), "GetUser");
        assert_eq!(to_pascal_case("GetUserById"), "GetUserById");
    }

    #[test]
    fn idents() {
        assert_eq!(field_ident("type"), "type_");
        assert_eq!(field_ident("userId"), "user_id");
        assert_eq!(field_ident("1st"), "_1st");
        assert_eq!(field_ident("a b"), "a_b");
    }
}
