use tower_lsp::lsp_types::*;

use htmlang::syntax::{DefinitionKind, VisibleKind};

use crate::{docs, tree};

pub(crate) fn hover_at(text: &str, position: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    let doc = if word == "if" && is_if_attribute(text, position.line, line, col) {
        Some(docs::if_attribute())
    } else if let Some(var_name) = word.strip_prefix('$') {
        hover_variable(text, var_name, position.line).or_else(|| docs::hover(&word))
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

/// Whether the `if` at `col` of `line` (line `line_idx` of `text`) starts a
/// whole attribute, `if(CONDITION, A, B)`: one the parser read as such in
/// an element's list (also on a later line of a list that spans lines),
/// or, in any other list on one line, where an attribute starts (a `(`
/// inside a value is CSS's own `if()`).
fn is_if_attribute(text: &str, line_idx: u32, line: &str, col: usize) -> bool {
    fn starts_choice(attrs: &[htmlang::syntax::Attr], at: Position) -> bool {
        attrs.iter().any(|attr| {
            attr.choice.as_ref().is_some_and(|choice| {
                tree::range(attr.span).start == at
                    || choice
                        .branches
                        .iter()
                        .any(|branch| starts_choice(branch.attrs(), at))
            })
        })
    }
    let bytes = line.as_bytes();
    let mut start = col.min(bytes.len());
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    if !line[start..].starts_with("if(") {
        return false;
    }
    let at = Position::new(line_idx, start as u32);
    let parsed = htmlang::syntax::parse(text);
    let in_tree = tree::node_at(&parsed, line_idx).is_some_and(|node| {
        node.heads()
            .iter()
            .filter_map(|head| head.attrs.as_ref())
            .any(|list| starts_choice(&list.attrs, at))
    });
    in_tree
        || crate::completion::attr_context(&line[..start])
            .is_some_and(|context| context.segment.trim().is_empty())
}

pub(crate) fn word_at(line: &str, col: usize) -> Option<String> {
    if let Some(span) = variable_at(line, col) {
        return Some(format!("${}", &line[span]));
    }
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

/// The name of the variable reference at `col` of `line`, ended as the
/// compiler ends it: the byte range of the name alone, after the `$` of
/// `$name` or inside the braces of `${name}`.
pub(crate) fn variable_at(line: &str, col: usize) -> Option<std::ops::Range<usize>> {
    htmlang::interp::name_spans(line)
        .into_iter()
        .find(|span| span.start - 1 <= col && col <= span.end)
}

/// The variable references named `name` on `line`: each as a byte range
/// that includes the `$` of `$name`, or is the name alone inside `${name}`.
pub(crate) fn variable_refs_named(line: &str, name: &str) -> Vec<std::ops::Range<usize>> {
    htmlang::interp::name_spans(line)
        .into_iter()
        .filter(|span| &line[span.clone()] == name)
        .map(|span| match line[..span.start].ends_with('$') {
            true => span.start - 1..span.end,
            false => span,
        })
        .collect()
}

pub(crate) fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'@' || c == b'$' || c == b'-' || c == b'_' || c == b':'
}

/// What `$name` means on line `line` (0-based): the definition visible
/// there (see `htmlang::syntax::Tree::visible_at`), else the first of that
/// name in the file.
fn hover_variable(text: &str, name: &str, line: u32) -> Option<String> {
    let defs = tree::definitions(text);
    let parsed = htmlang::syntax::parse(text);
    let visible = parsed
        .visible_at(line as usize + 1)
        .into_iter()
        .rev()
        .find(|v| v.name == name && v.kind != VisibleKind::Let(DefinitionKind::Function));
    let def = match visible.as_ref().map(|v| v.kind) {
        Some(VisibleKind::Parameter) => {
            // The function whose body the line is in
            return defs
                .iter()
                .filter(|d| d.line <= line && line <= d.end_line)
                .rfind(|d| d.params.iter().any(|p| p.name == name))
                .map(|d| format!("**${}** \u{2014} Parameter of `@{}`", name, d.name));
        }
        Some(VisibleKind::Loop) => {
            return Some(format!("**${}** \u{2014} `@each` variable", name));
        }
        Some(VisibleKind::Data) => {
            return Some(format!("**${}** \u{2014} Loaded with `@data`", name));
        }
        Some(VisibleKind::Let(_)) => {
            visible.and_then(|v| defs.iter().find(|d| d.name_range == tree::range(v.span)))
        }
        None => defs.iter().find(|d| d.name == name),
    };
    if let Some(def) = def {
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
                Some(default) => format!("`${}` (default: {})", p.name, default),
                None => format!("`${}` (required)", p.name),
            })
            .collect();
        format!("\n\nParameters: {}", formatted.join(", "))
    };

    // Where a call's content goes
    let mut content = Vec::new();
    if !def.slots.is_empty() {
        let slots: Vec<String> = def.slots.iter().map(|s| format!("`{}`", s)).collect();
        content.push(format!("Slots: {}", slots.join(", ")));
    }
    content.push(match def.takes_content {
        true => "Content goes where `@children` is".to_string(),
        false => "Takes no content (no `@children`)".to_string(),
    });
    let content_str = format!("\n\n{}", content.join("\n\n"));

    Some(format!(
        "**@{}** \u{2014} User function{}{}{}",
        name, params_str, content_str, doc_str
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_text(text: &str, line: u32, character: u32) -> String {
        match hover_at(text, Position::new(line, character)).map(|h| h.contents) {
            Some(HoverContents::Markup(markup)) => markup.value,
            other => panic!("no hover: {:?}", other),
        }
    }

    #[test]
    fn a_whole_attribute_if_has_a_hover() {
        let text = "@el [padding 4, if($on, [color red])] x\n@el [width if(media(print): 1px)] y\n";
        assert!(hover_text(text, 0, 17).contains("Attributes chosen by a condition"));
        assert!(hover_at(text, Position::new(1, 12)).is_none());
        // On a later line of a list that spans lines, and inside a group
        let text = "@el [\n  padding 4,\n  if($on, [if($b, gap 1)])\n]\n  x\n";
        assert!(hover_text(text, 2, 3).contains("Attributes chosen by a condition"));
        assert!(hover_text(text, 2, 12).contains("Attributes chosen by a condition"));
    }

    #[test]
    fn a_variable_s_hover_is_the_definition_visible_there() {
        let text = "@let x 1\n@el\n  @let x 2\n  @text $x\n@text $x\n@let @card [x]\n  @text $x\n@each $x in a, b\n  @text $x\n";
        assert!(hover_text(text, 3, 9).contains("= `2`"));
        assert!(hover_text(text, 4, 7).contains("= `1`"));
        assert!(hover_text(text, 6, 9).contains("Parameter of `@card`"));
        assert!(hover_text(text, 8, 9).contains("`@each` variable"));
    }

    #[test]
    fn a_function_s_hover_says_where_its_content_goes() {
        let text =
            "@let @card\n  @el\n    @slot footer\n    @children\n@let @dot\n  @el\n@card\n@dot\n";
        let card = hover_text(text, 6, 2);
        assert!(card.contains("Slots: `footer`"), "{}", card);
        assert!(card.contains("where `@children` is"), "{}", card);
        let dot = hover_text(text, 7, 2);
        assert!(dot.contains("Takes no content"), "{}", dot);
        assert!(!dot.contains("Slots"), "{}", dot);
    }
}
