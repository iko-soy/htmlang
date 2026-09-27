use tower_lsp::lsp_types::*;

use htmlang::syntax::{DefinitionKind, VisibleKind};

use crate::{docs, tree};

pub(crate) fn hover_at(text: &str, position: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    // `if`, also after prefixes (`hover:if(...)`)
    let base = htmlang::vocab::base_attribute(&word);
    let prefix = &word[..word.len() - base.len()];
    let doc = if base == "if" && is_if_attribute(text, position.line, line, col) {
        Some(docs::if_attribute())
    } else if base == "if" && !prefix.is_empty() {
        docs::hover(prefix)
    } else if let Some(var_name) = word.strip_prefix('$') {
        hover_variable(text, var_name, position.line).or_else(|| docs::hover(&word))
    } else if let Some(fn_name) = word.strip_prefix('@') {
        hover_user_fn(text, fn_name).or_else(|| docs::hover(&word))
    } else {
        hover_parameter(text, position, &word).or_else(|| docs::hover(&word))
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
            let prefixed = attr
                .prefixed
                .as_ref()
                .is_some_and(|p| starts_choice(p.target.attrs(), at));
            prefixed
                || attr.choice.as_ref().is_some_and(|choice| {
                    tree::range(attr.span).start == at
                        || choice
                            .branches
                            .iter()
                            .any(|branch| starts_choice(branch.attrs(), at))
                })
        })
    }
    // The word's start, after any prefixes (`hover:if`)
    let bytes = line.as_bytes();
    let mut start = col.min(bytes.len());
    while start > 0 && is_word_byte(bytes[start - 1]) && bytes[start - 1] != b':' {
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
    // A prefix's argument is part of the word: `has(> img):padding`
    while bytes.get(end) == Some(&b'(')
        && let Some(close) = htmlang::vocab::closing_paren(line, end)
        && bytes.get(close + 1) == Some(&b':')
    {
        end = close + 1;
        while end < bytes.len() && is_word_byte(bytes[end]) {
            end += 1;
        }
    }
    let start = crate::completion::back_over_arguments(line, start);
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

/// A parameter's name where it is declared (`@let @card [title]`) or
/// passed (`@card [title Hi]`): what it is, and its default.
fn hover_parameter(text: &str, position: Position, word: &str) -> Option<String> {
    let defs = tree::definitions(text);
    let describe = |function: &str, param: &tree::Param| {
        let given = match &param.default {
            Some(default) => format!("default: {}", default),
            None => "required".to_string(),
        };
        format!(
            "**{}** — parameter of `@{}` ({}), passed as `{} VALUE`",
            param.name, function, given, param.name
        )
    };
    // Declared
    for def in defs.iter().filter(|d| d.kind == DefinitionKind::Function) {
        if let Some(param) = def.params.iter().find(|p| {
            p.name == word
                && p.name_range.start.line == position.line
                && p.name_range.start.character <= position.character
                && position.character <= p.name_range.end.character
        }) {
            return Some(describe(&def.name, param));
        }
    }
    // Passed at a call
    let parsed = htmlang::syntax::parse(text);
    let node = tree::node_at(&parsed, position.line)?;
    for head in node.heads() {
        let Some(def) = defs
            .iter()
            .find(|d| d.kind == DefinitionKind::Function && d.name == head.name)
        else {
            continue;
        };
        let Some(list) = &head.attrs else {
            continue;
        };
        for attr in list.attrs.iter().filter(|a| a.key == word && !a.html) {
            let start = tree::range(attr.span).start;
            let on_it = start.line == position.line
                && start.character <= position.character
                && position.character <= start.character + attr.key.len() as u32;
            if let (true, Some(param)) = (on_it, def.params.iter().find(|p| p.name == word)) {
                return Some(describe(&def.name, param));
            }
        }
    }
    None
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
        if !htmlang::syntax::is_comment(prev) {
            break;
        }
        let comment = &prev[2..];
        doc_lines.push(comment.strip_prefix(' ').unwrap_or(comment));
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

    #[test]
    fn a_parameter_s_name_is_described_as_a_parameter() {
        let text = "@let @card [title, tone info]\n  @el [background $tone] $title\n@card [title Hi, tone warm]\n";
        let at = |line, character| match hover_at(text, Position::new(line, character)) {
            Some(Hover {
                contents: HoverContents::Markup(markup),
                ..
            }) => markup.value,
            other => panic!("{:?}", other),
        };
        assert!(
            at(2, 8).contains("parameter of `@card` (required)"),
            "{}",
            at(2, 8)
        );
        assert!(at(2, 18).contains("(default: info)"), "{}", at(2, 18));
        assert!(at(0, 13).contains("parameter of `@card`"), "{}", at(0, 13));
        // Elsewhere, `title` is the HTML attribute
        let text = "@el [title=x] y\n";
        let hover = hover_at(text, Position::new(0, 6)).expect("hover");
        let HoverContents::Markup(markup) = hover.contents else {
            panic!()
        };
        assert!(markup.value.contains("HTML attribute"), "{}", markup.value);
    }

    fn hover_text(text: &str, line: u32, character: u32) -> String {
        match hover_at(text, Position::new(line, character)).map(|h| h.contents) {
            Some(HoverContents::Markup(markup)) => markup.value,
            other => panic!("no hover: {:?}", other),
        }
    }

    #[test]
    fn a_prefix_with_an_argument_is_part_of_the_word() {
        let text =
            "@el [has(> img):padding 0, nth-child(odd):hover:color red, marker:color red] x\n";
        let padding = hover_text(text, 0, 18);
        assert!(padding.contains("`:has(> img)`"), "{}", padding);
        let color = hover_text(text, 0, 50);
        assert!(color.contains("`:nth-child(odd)`"), "{}", color);
        assert!(color.contains("`:hover`"), "{}", color);
        let marker = hover_text(text, 0, 62);
        assert!(marker.contains("`::marker` pseudo-element"), "{}", marker);
    }

    #[test]
    fn an_element_prefix_is_part_of_the_word() {
        let text = "@table [md:@td:padding 8, @link:hover:color red] x\n";
        let padding = hover_text(text, 0, 17);
        assert!(padding.contains("every `<td>` inside"), "{}", padding);
        assert!(padding.contains("`md`"), "{}", padding);
        let color = hover_text(text, 0, 40);
        assert!(color.contains("every `<a>` inside"), "{}", color);
        assert!(color.contains("`:hover`"), "{}", color);
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
        // After prefixes, and inside a prefixed group
        let text = "@el [hover:if($on, color red), md:[if($b, gap 1)]] x
";
        assert!(hover_text(text, 0, 12).contains("Attributes chosen by a condition"));
        assert!(hover_text(text, 0, 36).contains("Attributes chosen by a condition"));
        assert!(hover_text(text, 0, 6).contains(":hover"));
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

    #[test]
    fn a_css_property_s_hover_says_whether_its_numbers_are_px() {
        let text = "@el [box-shadow 0 2 4 black, flex 1 1 240px, --cols 3] x\n";
        let shadow = hover_text(text, 0, 7);
        assert!(shadow.contains("a length: every bare number"), "{}", shadow);
        let flex = hover_text(text, 0, 31);
        assert!(flex.contains("its numbers stay numbers"), "{}", flex);
        let custom = hover_text(text, 0, 48);
        assert!(custom.contains("as it is (no px)"), "{}", custom);
    }
}
