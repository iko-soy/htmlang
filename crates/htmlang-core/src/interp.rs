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
//!   it).
//! - `$--name` is an error: a custom property is read one way, with
//!   `var(--name)`, in the browser, so it follows a token redefined under
//!   `dark:`. See [`custom_property_len`].
//! - `.field` continues the name only when the value is a record or a list
//!   (`$post.title`, `$tags.0`). After `@let lang fr`, `$lang.json` is
//!   `fr.json`, and `$a..$b` is two names.
//! - `${name}` is the same name, delimited (`${size}px`), and `${EXPR}`
//!   inserts the value of an expression (see `expr.rs`).
//! - A `$` before anything else is text: `$5`, `$$`, a `$` at the end.
//! - An undefined name is an error. A field that a record doesn't have is
//!   empty, and so are its fields (`$post.author.name` when `$post` has no
//!   `author`), so optional fields of `@data` records work.
//! - Quoted text (`@let arrow "→ "`, `@card [quote "→ "]`) remembers its
//!   quotes: a CSS value gets them (`content $arrow` is `content:"→ "`),
//!   text and HTML attribute values don't. See [`Sink`].

use crate::expr;
use crate::value::Value;

/// The variables in scope, as the evaluator stores them.
pub trait Scope {
    /// The value of a variable, by its name (without fields).
    fn get(&self, name: &str) -> Option<Value>;
}

/// Where a slot's value goes, which decides what quoted text inserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sink {
    /// Text, an HTML attribute value, a path, a parameter: quoted text
    /// inserts what it says, without the quotes.
    Text,
    /// A CSS value: quoted text inserts itself with its quotes, or, inside a
    /// quoted string of the value, what it says with `"` escaped as `\"`.
    Css,
}

/// Whether a CSS value is inside a quoted string after `text`, when it
/// was (`inside`) before it. A backslash inside a string escapes the next
/// character.
fn css_string_after(text: &str, mut inside: bool) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if inside => {
                chars.next();
            }
            '"' => inside = !inside,
            _ => {}
        }
    }
    inside
}

/// `text` as it goes inside a CSS string: `"` escaped as `\"`. A backslash
/// and the character after it are a CSS escape and stay as written
/// (`\201C`), but a backslash at the end is doubled, so it can't escape
/// the rest of the string (`a\` gives `a\\`).
pub fn css_string_body(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push('\\');
                out.push(chars.next().unwrap_or('\\'));
            }
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// A problem found while interpolating, at a byte offset into the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// A name with no definition.
    Undefined { name: String, offset: usize },
    /// `$--name`: a custom property, which is read with `var(--name)`.
    CustomProperty { name: String, offset: usize },
    /// A `${...}` whose expression doesn't evaluate.
    Invalid { message: String, offset: usize },
    /// A record put where text goes, which it has none of: it would print
    /// nothing.
    Record { message: String, offset: usize },
}

/// The problem with putting `value`, written `written`, where text goes:
/// a record has no text.
fn no_text(written: &str, value: &Value, offset: usize) -> Option<Problem> {
    let name = written.strip_prefix('$').map(|name| {
        name.strip_prefix('{')
            .and_then(|n| n.strip_suffix('}'))
            .map_or(name, str::trim)
    });
    let fields = match value {
        Value::Record(fields) => fields,
        // A list of records, which would print only what isn't a record
        Value::List(list)
            if list.written.is_none()
                && list
                    .items
                    .iter()
                    .any(|item| matches!(item, Value::Record(_))) =>
        {
            let example = match (name, list.items.first()) {
                (Some(name), Some(Value::Record(fields))) if is_path(name) => {
                    match fields.first() {
                        Some((field, _)) => format!(", such as `${}.0.{}`", name, field),
                        None => String::new(),
                    }
                }
                _ => String::new(),
            };
            return Some(Problem::Record {
                message: format!(
                    "'{}' is a list of records, which has no text of its own: loop over it \
                     with @each, or write a field of one of its items{}",
                    written, example
                ),
                offset,
            });
        }
        _ => return None,
    };
    let example = match (name, fields.first()) {
        (Some(name), Some((field, _))) if is_path(name) => {
            format!(", such as `${}.{}`", name, field)
        }
        _ => String::new(),
    };
    Some(Problem::Record {
        message: format!(
            "'{}' is a record, which has no text of its own: write one of its fields{}",
            written, example
        ),
        offset,
    })
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
    word_len(s, |c| c.is_alphabetic() || c == '_')
}

/// The length of the custom property's name `--name` at the start of `s`
/// (the text after a `$`), or 0. `$--name` is not a variable and not text
/// either: it is an error that says to write `var(--name)`.
pub fn custom_property_len(s: &str) -> usize {
    match s.strip_prefix("--") {
        Some(rest) => match word_len(rest, |c| c.is_alphanumeric() || c == '_') {
            0 => 0,
            n => n + 2,
        },
        None => 0,
    }
}

/// What is wrong with `$NAME`, where `name` is a custom property's name
/// (`--brand`).
pub fn custom_property_message(name: &str) -> String {
    format!(
        "`${}` is not a variable: a custom property is read with `var({})`",
        name, name
    )
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

/// What a name path leads to so far: a value, or a field that a record
/// (or an item that a list) doesn't have, which is empty, and so is every
/// field of it.
#[derive(Clone)]
enum Step {
    Value(Value),
    Missing,
}

impl Step {
    /// The field `name` of this step, or `None` when it has no fields
    /// (text, a number, true or false).
    fn field(&self, name: &str) -> Option<Step> {
        let Step::Value(value) = self else {
            return Some(Step::Missing);
        };
        match value {
            Value::Record(fields) => Some(
                fields
                    .iter()
                    .find(|(key, _)| key == name)
                    .map_or(Step::Missing, |(_, value)| Step::Value(value.clone())),
            ),
            Value::List(list) => Some(
                name.parse::<usize>()
                    .ok()
                    .and_then(|i| list.items.get(i).cloned())
                    .map_or(Step::Missing, Step::Value),
            ),
            _ => None,
        }
    }

    fn value(self) -> Value {
        match self {
            Step::Value(value) => value,
            Step::Missing => Value::empty(),
        }
    }
}

/// The name path at the start of `s` (after a `$`) and its length: the
/// name, and each `.field` while the value so far is a record or a list,
/// or a field one of them doesn't have.
fn path_at(s: &str, scope: &dyn Scope) -> Option<(String, usize)> {
    let n = name_len(s);
    if n == 0 {
        return None;
    }
    let mut path = s[..n].to_string();
    let mut len = n;
    let mut step = scope.get(&path).map(Step::Value);
    while let Some(rest) = s[len..].strip_prefix('.') {
        let f = word_len(rest, is_name_char);
        let Some(field) = step
            .as_ref()
            .filter(|_| f > 0)
            .and_then(|v| v.field(&rest[..f]))
        else {
            break;
        };
        path = format!("{}.{}", path, &rest[..f]);
        len += 1 + f;
        step = Some(field);
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

/// The value of a name path, or `None` when it is undefined. A field that
/// a record doesn't have is empty, and so is any field of it.
pub fn resolve(path: &str, scope: &dyn Scope) -> Option<Value> {
    let mut parts = path.split('.');
    let mut step = Step::Value(scope.get(parts.next()?)?);
    for field in parts {
        step = step.field(field)?;
    }
    Some(step.value())
}

/// Fill in the `$name`s and `${...}`s of one slot's text, for text (see
/// [`Sink::Text`]). Each one that can't be filled is left as written and
/// reported.
pub fn interpolate(text: &str, scope: &dyn Scope) -> (String, Vec<Problem>) {
    interpolate_for(text, scope, Sink::Text)
}

/// [`interpolate`] for a slot whose value goes to `sink`.
pub fn interpolate_for(text: &str, scope: &dyn Scope, sink: Sink) -> (String, Vec<Problem>) {
    let mut problems = Vec::new();
    if !text.contains('$') {
        return (text.to_string(), problems);
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    // Whether the CSS value is inside a quoted string at `pos`
    let mut in_string = false;
    while let Some(found) = text[pos..].find('$') {
        let dollar = pos + found;
        out.push_str(&text[pos..dollar]);
        if sink == Sink::Css {
            in_string = css_string_after(&text[pos..dollar], in_string);
        }
        let after = &text[dollar + 1..];
        let Some((reference, len)) = reference(after, scope) else {
            let n = custom_property_len(after);
            if n > 0 {
                problems.push(Problem::CustomProperty {
                    name: after[..n].to_string(),
                    offset: dollar,
                });
            }
            out.push_str(&text[dollar..dollar + 1 + n]);
            pos = dollar + 1 + n;
            continue;
        };
        let written = &text[dollar..dollar + 1 + len];
        let insert = |value: String, quoted: Option<String>| match (sink, quoted) {
            (Sink::Css, Some(css)) if in_string => {
                // The string's body, without its quotes
                let body = css.strip_prefix('"').unwrap_or(&css);
                body.strip_suffix('"').unwrap_or(body).to_string()
            }
            (Sink::Css, Some(css)) => css,
            (Sink::Css, None) if in_string => css_string_body(&value),
            _ => value,
        };
        match reference {
            Reference::Var(path) => match resolve(&path, scope) {
                Some(value) => match no_text(written, &value, dollar) {
                    Some(problem) => problems.push(problem),
                    None => out.push_str(&insert(
                        value.to_string(),
                        value.quoted_css().map(str::to_string),
                    )),
                },
                None => {
                    problems.push(Problem::Undefined {
                        name: path,
                        offset: dollar,
                    });
                    out.push_str(written);
                }
            },
            Reference::Expr(source) => match expr::eval(source, scope) {
                Ok(value) => match no_text(written, &value, dollar) {
                    Some(problem) => problems.push(problem),
                    None => out.push_str(&insert(value.to_string(), None)),
                },
                Err(error) => {
                    problems.push(match error {
                        // At the `${`
                        expr::Error::Invalid(message) => Problem::Invalid {
                            message,
                            offset: dollar,
                        },
                        // At the name, inside the braces
                        error => error.at(dollar + 2),
                    });
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
    name_spans(text)
        .into_iter()
        .map(|span| &text[span])
        .collect()
}

/// Where the names of [`names`] are in `text`, as byte ranges of the name
/// alone: after the `$` of `$name`, inside the braces of `${name}`.
pub fn name_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut found = Vec::new();
    collect_name_spans(text, 0, &mut found);
    found
}

fn collect_name_spans(text: &str, base: usize, found: &mut Vec<std::ops::Range<usize>>) {
    let mut pos = 0;
    while let Some(i) = text[pos..].find('$') {
        let after = pos + i + 1;
        let rest = &text[after..];
        pos = after;
        // `\$` is a dollar sign (and `\\$` a backslash before a name)
        let backslashes = text[..after - 1].len() - text[..after - 1].trim_end_matches('\\').len();
        if backslashes % 2 == 1 {
            continue;
        }
        if rest.starts_with('{')
            && let Some(close) = matching_brace(rest)
        {
            let inner = &rest[1..close];
            let trimmed = inner.trim_start();
            if is_path(trimmed.trim_end()) {
                let start = base + after + 1 + inner.len() - trimmed.len();
                found.push(start..start + name_len(trimmed));
            } else {
                collect_name_spans(inner, base + after + 1, found);
            }
            pos = after + close + 1;
            continue;
        }
        let n = name_len(rest);
        if n > 0 {
            found.push(base + after..base + after + n);
            pos = after + n;
        }
    }
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

    /// Variables by name: text from `new`, any value from `with`.
    pub(crate) struct Map(pub HashMap<String, Value>);

    impl Map {
        pub(crate) fn new(pairs: &[(&str, &str)]) -> Self {
            Map(pairs
                .iter()
                .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
                .collect())
        }

        pub(crate) fn with(mut self, name: &str, value: Value) -> Self {
            self.0.insert(name.to_string(), value);
            self
        }
    }

    impl Scope for Map {
        fn get(&self, name: &str) -> Option<Value> {
            self.0.get(name).cloned()
        }
    }

    pub(crate) fn text_list(items: &[&str]) -> Value {
        Value::list(items.iter().map(|s| Value::Str(s.to_string())).collect())
    }

    pub(crate) fn record(fields: &[(&str, &str)]) -> Value {
        Value::Record(std::rc::Rc::new(
            fields
                .iter()
                .map(|(k, v)| (k.to_string(), Value::Str(v.to_string())))
                .collect(),
        ))
    }

    fn vars() -> Map {
        Map::new(&[("lang", "fr"), ("n", "3"), ("n-1", "two")])
            .with("post", record(&[("title", "Hello")]))
            .with("tags", text_list(&["a", "b"]))
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
        assert_eq!(fill("1..$n"), "1..3");
        // A name ends at `..`
        assert_eq!(fill("$n..$n $post.title..x"), "3..3 Hello..x");
    }

    #[test]
    fn a_dollar_before_anything_else_is_text() {
        assert_eq!(fill("$5 and $$ and $"), "$5 and $$ and $");
        assert_eq!(fill("a $-x ${"), "a $-x ${");
    }

    #[test]
    fn a_custom_property_is_not_a_variable() {
        let (out, problems) = interpolate("x $--brand-dark $-- $--1", &vars());
        assert_eq!(out, "x $--brand-dark $-- $--1");
        assert_eq!(
            problems,
            [
                Problem::CustomProperty {
                    name: "--brand-dark".into(),
                    offset: 2
                },
                Problem::CustomProperty {
                    name: "--1".into(),
                    offset: 20
                }
            ]
        );
        assert!(custom_property_message("--brand").contains("`var(--brand)`"));
        assert_eq!(names("$--e $f"), ["f"]);
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
    fn quoted_text_keeps_its_quotes_only_in_css() {
        let quoted = Value::Quoted(crate::ast::Quoted {
            text: r#"a"b"#.into(),
            css: r#""a\"b""#.into(),
        });
        let scope = Map::new(&[("n", r#"x"y"#)]).with("q", quoted);
        let fill = |text: &str, sink| interpolate_for(text, &scope, sink).0;
        assert_eq!(fill("$q", Sink::Text), r#"a"b"#);
        assert_eq!(fill("$q", Sink::Css), r#""a\"b""#);
        // Inside a CSS string, what it says, escaped as the string needs
        assert_eq!(
            fill(r#""($q) $n" $q"#, Sink::Css),
            r#""(a\"b) x\"y" "a\"b""#
        );
        assert_eq!(fill(r#""\" $q""#, Sink::Css), r#""\" a\"b""#);
        // What an unquoted value holds goes into a string that stays valid
        let scope = Map::new(&[("end", r"a\"), ("esc", r"\201C"), ("q", r#"x\"y"#)]);
        let fill = |text: &str| interpolate_for(text, &scope, Sink::Css).0;
        assert_eq!(fill(r#""$end""#), r#""a\\""#);
        assert_eq!(fill(r#""$esc""#), r#""\201C""#);
        assert_eq!(fill(r#""$q""#), r#""x\"y""#);
    }

    #[test]
    fn inserted_values_are_never_read_again() {
        let scope = Map::new(&[("a", "$b"), ("b", "no")]);
        assert_eq!(interpolate("$a", &scope).0, "$b");
    }

    #[test]
    fn names_by_syntax() {
        assert_eq!(names("$a.b ${c} ${upper($d)} $5 $--e"), ["a", "c", "d"]);
        assert_eq!(names(r"\$a \\$b \\\$c"), ["b"]);
        assert_eq!(
            name_spans("x ${ size }px ${$n + ${m}} $a- b"),
            [5..9, 17..18, 23..24, 28..29]
        );
    }
}
