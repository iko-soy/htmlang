use tower_lsp::lsp_types::*;

use htmlang::syntax::DefinitionKind;

use crate::{docs, tree};

pub(crate) fn hover_at(text: &str, position: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    let doc = if let Some(var_name) = word.strip_prefix('$') {
        hover_variable(text, var_name).or_else(|| docs::hover(&word))
    } else if let Some(fn_name) = word.strip_prefix('@') {
        hover_user_fn(text, fn_name).or_else(|| docs::hover(&word))
    } else {
        docs::hover(&word)
    }?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc,
        }),
        range: None,
    })
}

pub(crate) fn word_at(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = col;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(line[start..end].to_string())
}

pub(crate) fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'@' || c == b'$' || c == b'-' || c == b'_' || c == b':'
}

fn hover_variable(text: &str, name: &str) -> Option<String> {
    let defs = tree::definitions(text);
    if let Some(def) = defs.iter().find(|d| d.name == name) {
        match def.kind {
            DefinitionKind::Value => {
                let value = def.value.as_deref().unwrap_or("");
                return Some(format!("**${}** = `{}`", name, value));
            }
            DefinitionKind::Bundle => {
                return Some(format!(
                    "**${}** \u{2014} Attribute bundle\n\n`[{}]`",
                    name,
                    def.value.as_deref().unwrap_or("")
                ));
            }
            DefinitionKind::Function => {}
        }
    }
    // A function's parameter
    defs.iter()
        .find(|d| d.params.iter().any(|p| p.name == name))
        .map(|d| format!("**${}** \u{2014} Parameter of `@{}`", name, d.name))
}

fn hover_user_fn(text: &str, name: &str) -> Option<String> {
    let def = tree::definitions(text)
        .into_iter()
        .find(|d| d.kind == DefinitionKind::Function && d.name == name)?;

    // Comment lines right above the definition are its documentation
    let lines: Vec<&str> = text.lines().collect();
    let mut doc_lines: Vec<&str> = Vec::new();
    let mut j = def.line as usize;
    while j > 0 {
        j -= 1;
        let prev = lines[j].trim();
        match prev.strip_prefix("--") {
            Some(comment) => doc_lines.push(comment.strip_prefix(' ').unwrap_or(comment)),
            None => break,
        }
    }
    doc_lines.reverse();
    let doc_str = if doc_lines.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", doc_lines.join("\n"))
    };

    let params_str = if def.params.is_empty() {
        String::new()
    } else {
        let formatted: Vec<String> = def
            .params
            .iter()
            .map(|p| match &p.default {
                Some(default) => format!("${} (default: {})", p.name, default),
                None => format!("${}", p.name),
            })
            .collect();
        format!("\n\nParameters: {}", formatted.join(", "))
    };

    Some(format!(
        "**@{}** \u{2014} User function{}{}",
        name, params_str, doc_str
    ))
}
