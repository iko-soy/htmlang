//! Variables in htmlang strings: `$name`, `$record.field` and `${...}`.
//!
//! The syntax tree finds every slot first: a run of text, an attribute's
//! value, an element's argument, a `@let` value, a file path, the title of
//! `@page`, the value of `@meta`. Each slot is then interpolated here, once.
//! A reference fills its own slot, and what it inserts is never read again
//! as htmlang: a value can't become an attribute name, a whole attribute, a
//! comma between attributes, markup or another `$name`.
//!
//! - `$` followed by a letter or `_` starts a name, which goes on with
//!   letters, digits, `_` and `-` (a `-` only when a name character follows
//!   it). `$--name` is the value of the custom property `--name`.
//! - `.field` continues the name only when the value is a record or a list
//!   (`$post.title`, `$tags.0`). After `@let lang fr`, `$lang.json` is
//!   `fr.json`.
//! - `${name}` is the same name, delimited (`${size}px`), and `${EXPR}`
//!   inserts the value of an expression (see `expr.rs`).
//! - A `$` before anything else is text: `$5`, `$$`, a `$` at the end.
//! - An undefined name is an error. A field that a record doesn't have is
//!   empty, so optional fields of `@data` records work.

use crate::expr::{self, Value};

/// The variables in scope, as the evaluator stores them.
pub trait Scope {
    /// Whether the name or field path (`post.title`) has a value of its own.
    fn defined(&self, path: &str) -> bool;
    /// The value of a name or field path.
    fn value(&self, path: &str) -> Option<Value>;
    /// Whether `path` is a record or a list, whose fields `.field` reads.
    fn has_fields(&self, path: &str) -> bool;
}

/// A problem found while interpolating, at a byte offset into the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// A name with no definition.
    Undefined { name: String, offset: usize },
    /// A `${...}` whose expression doesn't evaluate.
    Invalid { message: String, offset: usize },
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The length of a word at the start of `s` whose first character passes
/// `first`: name characters, with `-` between them.
fn word_len(s: &str, first: impl Fn(char) -> bool) -> usize {
    let Some(c) = s.chars().next().filter(|&c| first(c)) else {
        return 0;
    };
    let mut end = c.len_utf8();
    let mut i = end;
    while let Some(c) = s[i..].chars().next() {
        if is_name_char(c) {
            i += c.len_utf8();
            end = i;
        } else if c == '-' {
            let rest = &s[i..];
            let dashes = rest.len() - rest.trim_start_matches('-').len();
            if !rest[dashes..].starts_with(is_name_char) {
                break;
            }
            i += dashes;
        } else {
            break;
        }
    }
    end
}

/// The length of the variable name at the start of `s` (the text after a
/// `$`), by syntax alone, or 0 when `s` doesn't start one.
pub fn name_len(s: &str) -> usize {
    match s.strip_prefix("--") {
        Some(rest) => match word_len(rest, char::is_alphabetic) {
            0 => 0,
            n => n + 2,
        },
        None => word_len(s, |c| c.is_alphabetic() || c == '_'),
    }
}

/// Whether `s` is a whole name path, `name` or `name.field.field`.
fn is_path(s: &str) -> bool {
    let n = name_len(s);
    if n == 0 || matches!(&s[..n], "true" | "false" | "not" | "and" | "or") {
        return false;
    }
    let mut rest = &s[n..];
    while let Some(field) = rest.strip_prefix('.') {
        let f = word_len(field, is_name_char);
        if f == 0 {
            return false;
        }
        rest = &field[f..];
    }
    rest.is_empty()
}

/// The name path at the start of `s` (after a `$`) and its length: the
/// name, and each `.field` while the value so far is a record or a list.
fn path_at(s: &str, scope: &dyn Scope) -> Option<(String, usize)> {
    let n = name_len(s);
    if n == 0 {
        return None;
    }
    let mut path = s[..n].to_string();
    let mut len = n;
    while let Some(rest) = s[len..].strip_prefix('.') {
        let f = word_len(rest, is_name_char);
        if f == 0 {
            break;
        }
        let field = format!("{}.{}", path, &rest[..f]);
        if !scope.defined(&field) && !scope.has_fields(&path) {
            break;
        }
        path = field;
        len += 1 + f;
    }
    Some((path, len))
}

/// A reference, after its `$`.
#[derive(Debug, Clone, PartialEq)]
pub enum Reference<'a> {
    /// `$name`, `$name.field` or `${name}`: the name path.
    Var(String),
    /// `${EXPR}`: the expression.
    Expr(&'a str),
}

/// The reference that `s` (the text after a `$`) starts with, and its
/// length. `None` when the `$` is text.
pub fn reference<'a>(s: &'a str, scope: &dyn Scope) -> Option<(Reference<'a>, usize)> {
    if s.starts_with('{') {
        let close = matching_brace(s)?;
        let inner = &s[1..close];
        let reference = if is_path(inner.trim()) {
            Reference::Var(inner.trim().to_string())
        } else {
            Reference::Expr(inner)
        };
        return Some((reference, close + 1));
    }
    let (path, len) = path_at(s, scope)?;
    Some((Reference::Var(path), len))
}

/// The value of a name path, or `None` when it is undefined. A record has
/// no text of its own, and a field a record doesn't have is empty.
pub fn resolve(path: &str, scope: &dyn Scope) -> Option<Value> {
    if let Some(value) = scope.value(path) {
        return Some(value);
    }
    if scope.has_fields(path) {
        return Some(Value::Str(String::new()));
    }
    let (record, _) = path.rsplit_once('.')?;
    scope.has_fields(record).then(|| Value::Str(String::new()))
}

/// Fill in the `$name`s and `${...}`s of one slot's text. Each one that
/// can't be filled is left as written and reported.
pub fn interpolate(text: &str, scope: &dyn Scope) -> (String, Vec<Problem>) {
    let mut problems = Vec::new();
    if !text.contains('$') {
        return (text.to_string(), problems);
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(found) = text[pos..].find('$') {
        let dollar = pos + found;
        out.push_str(&text[pos..dollar]);
        let after = &text[dollar + 1..];
        let Some((reference, len)) = reference(after, scope) else {
            out.push('$');
            pos = dollar + 1;
            continue;
        };
        let written = &text[dollar..dollar + 1 + len];
        match reference {
            Reference::Var(path) => match resolve(&path, scope) {
                Some(value) => out.push_str(&value.to_string()),
                None => {
                    problems.push(Problem::Undefined {
                        name: path,
                        offset: dollar,
                    });
                    out.push_str(written);
                }
            },
            Reference::Expr(source) => match expr::eval(source, scope) {
                Ok(value) => out.push_str(&value.to_string()),
                Err(error) => {
                    problems.push(error.at(dollar + 2));
                    out.push_str(written);
                }
            },
        }
        pos = dollar + 1 + len;
    }
    out.push_str(&text[pos..]);
    (out, problems)
}

/// The names `text` refers to, by syntax alone: the first name of each
/// path, including those inside `${...}`.
pub fn names(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut pos = 0;
    while let Some(i) = text[pos..].find('$') {
        let after = pos + i + 1;
        let rest = &text[after..];
        pos = after;
        if rest.starts_with('{')
            && let Some(close) = matching_brace(rest)
        {
            let inner = &rest[1..close];
            let trimmed = inner.trim();
            if is_path(trimmed) {
                found.push(&trimmed[..name_len(trimmed)]);
            } else {
                found.extend(names(inner));
            }
            pos = after + close + 1;
            continue;
        }
        let n = name_len(rest);
        if n > 0 {
            found.push(&rest[..n]);
            pos = after + n;
        }
    }
    found
}

/// Index of the `}` matching the `{` that `s` starts with (skipping
/// braces inside string literals).
pub(crate) fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_string = !in_string,
            _ if in_string => {}
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Variables stored as the evaluator stores them: a record's fields
    /// under `name.field`, a list's length under `name#`.
    pub(crate) struct Map(pub HashMap<String, String>);

    impl Map {
        pub(crate) fn new(pairs: &[(&str, &str)]) -> Self {
            Map(pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect())
        }
    }

    impl Scope for Map {
        fn defined(&self, path: &str) -> bool {
            self.0.contains_key(path)
        }
        fn value(&self, path: &str) -> Option<Value> {
            if let Some(len) = self.0.get(&format!("{}#", path)) {
                let len: usize = len.parse().unwrap_or(0);
                return Some(Value::List(
                    (0..len)
                        .map(|i| {
                            self.0
                                .get(&format!("{}.{}", path, i))
                                .cloned()
                                .unwrap_or_default()
                        })
                        .collect(),
                ));
            }
            self.0.get(path).cloned().map(Value::Str)
        }
        fn has_fields(&self, path: &str) -> bool {
            self.0.contains_key(&format!("{}#", path))
                || self
                    .0
                    .keys()
                    .any(|k| k.strip_prefix(path).is_some_and(|r| r.starts_with('.')))
        }
    }

    fn vars() -> Map {
        Map::new(&[
            ("lang", "fr"),
            ("n", "3"),
            ("n-1", "two"),
            ("post", ""),
            ("post.title", "Hello"),
            ("tags#", "2"),
            ("tags", "a, b"),
            ("tags.0", "a"),
            ("tags.1", "b"),
            ("--brand", "#3b82f6"),
        ])
    }

    fn fill(text: &str) -> String {
        interpolate(text, &vars()).0
    }

    #[test]
    fn names_end_where_they_must() {
        assert_eq!(fill("locales/$lang.json"), "locales/fr.json");
        assert_eq!(fill("$post.title!"), "Hello!");
        assert_eq!(fill("$tags.1 and $tags"), "b and a, b");
        assert_eq!(fill("${lang}uage"), "fruage");
        assert_eq!(fill("$lang- $lang:"), "fr- fr:");
        assert_eq!(fill("$n-1"), "two");
        assert_eq!(fill("$--brand"), "#3b82f6");
        assert_eq!(fill("1..$n"), "1..3");
    }

    #[test]
    fn a_dollar_before_anything_else_is_text() {
        assert_eq!(fill("$5 and $$ and $"), "$5 and $$ and $");
        assert_eq!(fill("a $-x ${"), "a $-x ${");
    }

    #[test]
    fn missing_fields_are_empty_and_undefined_names_are_errors() {
        assert_eq!(fill("[$post.summary]"), "[]");
        let (out, problems) = interpolate("x $nope y", &vars());
        assert_eq!(out, "x $nope y");
        assert_eq!(
            problems,
            [Problem::Undefined {
                name: "nope".into(),
                offset: 2
            }]
        );
    }

    #[test]
    fn expressions() {
        assert_eq!(fill("${$n * 2}px"), "6px");
        let (_, problems) = interpolate("a ${$n + $m}", &vars());
        assert_eq!(
            problems,
            [Problem::Undefined {
                name: "m".into(),
                offset: 9
            }]
        );
    }

    #[test]
    fn inserted_values_are_never_read_again() {
        let scope = Map::new(&[("a", "$b"), ("b", "no")]);
        assert_eq!(interpolate("$a", &scope).0, "$b");
    }

    #[test]
    fn names_by_syntax() {
        assert_eq!(
            names("$a.b ${c} ${upper($d)} $5 $--e"),
            ["a", "c", "d", "--e"]
        );
    }
}
