//! What the integration tests share.
//!
//! A generated class is named by a hash of its style (`hl-2k9xq0m`, see
//! `htmlang::codegen::CLASS_DIGITS`), and a function's scoped `@style` by
//! the function and a hash of the stylesheet (`hl-fn-card-1x0a9zq`). The
//! tests read the HTML with the classes numbered instead, `hl-a`, `hl-b`,
//! ... in the order they first appear in a `class=`, and `hl-fn-card`: a
//! test says which element has which class, and a snapshot changes when
//! the styles do, not when a hash does. The names themselves are tested
//! in `codegen.rs` and `codegen_tests.rs`.

#![allow(dead_code)]

/// `htmlang::codegen`, with the HTML's classes numbered.
pub mod codegen {
    pub use htmlang::codegen::*;

    use super::numbered;
    use htmlang::ast::Document;

    pub fn generate(doc: &Document) -> String {
        numbered(&htmlang::codegen::generate(doc))
    }

    pub fn generate_dev(doc: &Document) -> String {
        numbered(&htmlang::codegen::generate_dev(doc))
    }

    pub fn generate_partial(doc: &Document) -> String {
        numbered(&htmlang::codegen::generate_partial(doc))
    }

    pub fn generate_partial_dev(doc: &Document) -> String {
        numbered(&htmlang::codegen::generate_partial_dev(doc))
    }

    pub fn generate_minified(doc: &Document) -> String {
        numbered(&htmlang::codegen::generate_minified(doc))
    }

    pub fn generate_with(doc: &Document, opts: &CodegenOptions) -> String {
        numbered(&htmlang::codegen::generate_with(doc, opts))
    }
}

/// `html` with its generated classes numbered in the order they first
/// appear in a `class=` (then in the CSS), and the hash left out of a
/// function's scoped class.
pub fn numbered(html: &str) -> String {
    let mut order: Vec<&str> = Vec::new();
    for value in html.split("class=\"").skip(1) {
        let value = value.split('"').next().unwrap_or("");
        for name in value.split_whitespace() {
            if let Some(Name::Generated(name)) = name_at(name) {
                if !order.contains(&name) {
                    order.push(name);
                }
            }
        }
    }
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find("hl-") {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match name_at(rest) {
            Some(Name::Generated(name)) => {
                let index = order.iter().position(|n| *n == name).unwrap_or_else(|| {
                    order.push(name);
                    order.len() - 1
                });
                out.push_str("hl-");
                out.push_str(&sequential(index));
                rest = &rest[name.len()..];
            }
            Some(Name::Scoped { name, function }) => {
                out.push_str("hl-fn-");
                out.push_str(function);
                rest = &rest[name.len()..];
            }
            None => {
                out.push_str("hl-");
                rest = &rest[3..];
            }
        }
    }
    out.push_str(rest);
    out
}

enum Name<'a> {
    /// `hl-2k9xq0m`
    Generated(&'a str),
    /// `hl-fn-card-1x0a9zq`, and `card`
    Scoped { name: &'a str, function: &'a str },
}

/// The generated class `text` starts with, if any.
fn name_at(text: &str) -> Option<Name<'_>> {
    let after = text.strip_prefix("hl-")?;
    let word = |s: &str, dash: bool| {
        s.find(|c: char| !(c.is_ascii_digit() || c.is_ascii_lowercase() || (dash && c == '-')))
            .unwrap_or(s.len())
    };
    if let Some(scoped) = after.strip_prefix("fn-") {
        let end = word(scoped, true);
        let (function, hash) = scoped[..end].rsplit_once('-')?;
        let digits = htmlang::codegen::CLASS_DIGITS;
        return (hash.len() == digits).then(|| Name::Scoped {
            name: &text[..6 + end],
            function,
        });
    }
    let end = word(after, false);
    let generated = end >= htmlang::codegen::CLASS_DIGITS && !after[end..].starts_with('-');
    generated.then(|| Name::Generated(&text[..3 + end]))
}

/// The names classes had before they were hashes: a..z, then aa..a9,
/// ba..b9, ..., z9, then aaa, ...
fn sequential(index: usize) -> String {
    const REST: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut n = index;
    let mut len = 1;
    let mut count = 26usize;
    while n >= count {
        n -= count;
        len += 1;
        count = count.saturating_mul(36);
    }
    let mut tail = Vec::with_capacity(len - 1);
    for _ in 1..len {
        tail.push(REST[n % 36]);
        n /= 36;
    }
    let mut name = String::with_capacity(len);
    name.push((b'a' + n as u8) as char);
    name.extend(tail.iter().rev().map(|&b| b as char));
    name
}
