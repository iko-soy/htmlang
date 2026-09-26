//! The editor grammar's word lists come from the compiler's tables: this
//! test fails when a directive, an element or an attribute is added,
//! renamed or removed in one place and not the other.

use std::collections::BTreeSet;

use htmlang::ast::{DIRECTIVES, ElementKind};

fn grammar() -> serde_json::Value {
    let text = std::fs::read_to_string("editors/vscode/syntaxes/htmlang.tmLanguage.json")
        .expect("read the TextMate grammar");
    serde_json::from_str(&text).expect("the TextMate grammar is JSON")
}

/// The alternatives of the first `(?:a|b|c)` group in a pattern.
fn alternatives(pattern: &str) -> BTreeSet<String> {
    let start = pattern.find("(?:").expect("an alternation group") + 3;
    let end = start + pattern[start..].find(')').expect("a closed group");
    pattern[start..end].split('|').map(str::to_string).collect()
}

fn pattern(grammar: &serde_json::Value, rule: &str) -> String {
    grammar["repository"][rule]["match"]
        .as_str()
        .unwrap_or_else(|| panic!("the grammar has no `{}` rule", rule))
        .to_string()
}

#[test]
fn grammar_directives_are_the_compilers() {
    let grammar = grammar();
    let listed = alternatives(&pattern(&grammar, "directive"));
    let compiled: BTreeSet<String> = DIRECTIVES.iter().map(|d| d.name.to_string()).collect();
    assert_eq!(
        listed, compiled,
        "update the `directive` rule of the TextMate grammar"
    );
}

#[test]
fn grammar_elements_are_the_compilers() {
    let grammar = grammar();
    let listed = alternatives(&pattern(&grammar, "element"));
    let compiled: BTreeSet<String> = ElementKind::all_names().map(str::to_string).collect();
    assert_eq!(
        listed, compiled,
        "update the `element` rule of the TextMate grammar"
    );
}

#[test]
fn grammar_attribute_names_are_style_attributes() {
    let grammar = grammar();
    for name in alternatives(&pattern(&grammar, "attribute-name")) {
        assert!(
            htmlang::vocab::is_style_attribute(&name),
            "the grammar highlights `{}`, which isn't an attribute",
            name
        );
    }
}

#[test]
fn grammar_escapes_are_the_compilers() {
    let grammar = grammar();
    // Each alternative is the character(s) after the backslash, with a
    // regex escape where one is needed
    let listed: BTreeSet<String> = alternatives(&pattern(&grammar, "escape"))
        .into_iter()
        .map(|a| a.strip_prefix('\\').map_or(a.clone(), str::to_string))
        .collect();
    let compiled: BTreeSet<String> = htmlang::syntax::ESCAPES
        .iter()
        .map(|e| e[1..].to_string())
        .collect();
    assert_eq!(
        listed, compiled,
        "update the `escape` rule of the TextMate grammar"
    );
}

#[test]
fn grammar_literal_elements_are_the_compilers() {
    let grammar = grammar();
    let compiled: BTreeSet<String> = htmlang::ast::TAGS
        .iter()
        .filter(|spec| spec.literal)
        .map(|spec| spec.name.to_string())
        .collect();
    for rule in ["literal-line", "literal-inline"] {
        let begin = grammar["repository"][rule]["begin"]
            .as_str()
            .unwrap_or_else(|| panic!("the grammar has no `{}` rule", rule));
        // The names are the group after the `@`
        let names = &begin[begin.find("@(?:").expect("an element name") + 1..];
        assert_eq!(
            alternatives(names),
            compiled,
            "update the `{}` rule of the TextMate grammar",
            rule
        );
    }
}

#[test]
fn grammar_leading_arguments_are_the_compilers() {
    let grammar = grammar();
    let compiled: BTreeSet<String> = ElementKind::all_names()
        .filter(|name| {
            ElementKind::from_name(name).is_some_and(|kind| !kind.arg().attributes().is_empty())
        })
        .map(str::to_string)
        .collect();
    let rule = pattern(&grammar, "leading-argument");
    // The names are the group after the `@`
    let names = &rule[rule.find("@(?:").expect("an element name") + 1..];
    assert_eq!(
        alternatives(names),
        compiled,
        "update the `leading-argument` rule of the TextMate grammar"
    );
}
