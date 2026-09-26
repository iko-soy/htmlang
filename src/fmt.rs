//! The formatter: prints the syntax tree back with normalized indentation
//! (two spaces per level) and attribute lists (`[a, b]`, wrapped one per
//! line when long). Comments, blank lines, text and verbatim bodies are
//! kept as written.

use crate::syntax::{self, AttrList, DirectiveArgs, LetForm, Node, NodeKind};

const MAX_LINE_WIDTH: usize = 100;
/// Once a line has already been wrapped over multiple attribute lines, we keep it
/// wrapped even after formatting as long as the attribute count stays >1. This
/// prevents format→format→format oscillation around the wrap threshold.
const WRAP_MIN_WIDTH: usize = 80;

/// Format an htmlang source file.
pub fn format(input: &str) -> String {
    let tree = syntax::parse(input);
    let lines: Vec<&str> = input.lines().collect();
    let mut out = String::new();
    for node in &tree.nodes {
        print_node(node, 0, &lines, &mut out);
    }
    out
}

/// A formatted header line, and where its first attribute list is (for
/// wrapping).
struct Header {
    text: String,
    /// Byte range of the first `[...]` in `text`, and its attributes.
    list: Option<(usize, usize, Vec<String>)>,
}

impl Header {
    fn new() -> Self {
        Header {
            text: String::new(),
            list: None,
        }
    }

    fn push(&mut self, s: &str) {
        self.text.push_str(s);
    }

    fn push_list(&mut self, list: &AttrList) {
        let attrs: Vec<String> = list.attrs.iter().map(attr_text).collect();
        let start = self.text.len();
        self.text.push('[');
        self.text.push_str(&attrs.join(", "));
        self.text.push(']');
        if self.list.is_none() {
            self.list = Some((start, self.text.len(), attrs));
        }
    }
}

/// An attribute as printed: as written, except that a whole-attribute
/// `if(CONDITION, A, B)` prints its groups like any list (`[a, b]`, on
/// one line). A misshapen `if()` is kept as written.
fn attr_text(attr: &syntax::Attr) -> String {
    let Some(choice) = &attr.choice else {
        return attr.raw.clone();
    };
    let Some(condition) = &choice.condition else {
        return attr.raw.clone();
    };
    let mut parts = vec![condition.raw.clone()];
    for branch in &choice.branches {
        parts.push(match branch {
            syntax::Branch::Empty => String::new(),
            syntax::Branch::Attr(attr) => attr_text(attr),
            syntax::Branch::Group {
                list,
                trailing: None,
            } if list.closed => {
                let attrs: Vec<String> = list.attrs.iter().map(attr_text).collect();
                format!("[{}]", attrs.join(", "))
            }
            syntax::Branch::Group { .. } => return attr.raw.clone(),
        });
    }
    format!("if({})", parts.join(", ").trim_end())
}

fn print_node(node: &Node, level: usize, lines: &[&str], out: &mut String) {
    let indent = "  ".repeat(level);
    // Verbatim children keep their offset from the header.
    let delta = (level * 2) as i32 - node.indent as i32;
    match &node.kind {
        NodeKind::Blank => out.push('\n'),
        NodeKind::Comment => {
            out.push_str(&indent);
            out.push_str(&node.source);
            out.push('\n');
        }
        NodeKind::Verbatim(_) => print_raw(node, lines, delta, false, out),
        _ => match header(node) {
            // An attribute list that never closed: keep the lines unchanged
            _ if is_open(node) => {
                for line in own_lines(node, lines) {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            // A multi-line header with comments in it (or inline JSON):
            // re-indented, but otherwise as written
            None => print_raw(node, lines, delta, true, out),
            Some(_) if node.line_count > 1 && has_comment(node, lines) => {
                print_raw(node, lines, delta, true, out)
            }
            Some(header) => print_header(header, node, level, out),
        },
    }
    for child in &node.children {
        match child.kind {
            NodeKind::Verbatim(_) => print_raw(child, lines, delta, false, out),
            _ => print_node(child, level + 1, lines, out),
        }
    }
}

/// The node's own source lines.
fn own_lines<'a>(node: &Node, lines: &[&'a str]) -> Vec<&'a str> {
    let first = node.span.line.saturating_sub(1);
    let last = (first + node.line_count).min(lines.len());
    lines[first.min(last)..last].to_vec()
}

fn has_comment(node: &Node, lines: &[&str]) -> bool {
    own_lines(node, lines)
        .iter()
        .skip(1)
        .any(|l| l.trim_start().starts_with("--"))
}

/// Whether an attribute list on the line never closed.
fn is_open(node: &Node) -> bool {
    let open = |list: &Option<AttrList>| list.as_ref().is_some_and(|l| !l.closed);
    match &node.kind {
        NodeKind::Element(line) => line.chain.iter().any(|h| open(&h.attrs)),
        NodeKind::Directive(d) => match &d.args {
            DirectiveArgs::Page { attrs, .. } => open(attrs),
            DirectiveArgs::Let(def) => match &def.form {
                LetForm::Bundle(list) => !list.closed,
                LetForm::Function(function) => open(&function.list),
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}

/// Print a node's own lines, shifted by `delta` columns. With
/// `first_at_level`, the first line is re-indented instead.
fn print_raw(node: &Node, lines: &[&str], delta: i32, first_at_level: bool, out: &mut String) {
    for (i, line) in own_lines(node, lines).into_iter().enumerate() {
        if line.trim().is_empty() {
            out.push('\n');
            continue;
        }
        if i == 0 && first_at_level {
            let level_indent = (node.indent as i32 + delta).max(0) as usize;
            out.push_str(&" ".repeat(level_indent));
            out.push_str(line.trim());
        } else {
            out.push_str(&shift_line(line, delta));
        }
        out.push('\n');
    }
}

/// The canonical text of a line, or `None` to keep it as written
/// (multi-line inline JSON).
fn header(node: &Node) -> Option<Header> {
    let mut h = Header::new();
    match &node.kind {
        NodeKind::Text(_) => {
            if node.line_count > 1 {
                return None;
            }
            h.push(&node.source);
        }
        NodeKind::Element(line) => {
            for (i, head) in line.chain.iter().enumerate() {
                if i > 0 {
                    h.push(" > ");
                }
                h.push("@");
                h.push(&head.name);
                if let Some(list) = &head.attrs {
                    h.push(" ");
                    h.push_list(list);
                }
            }
            if let Some(text) = &line.text {
                // Text continued over lines (an inline element's list): as
                // written
                if text.span.line != node.span.line + node.line_count - 1 {
                    return None;
                }
                h.push(" ");
                h.push(&text.raw);
            }
        }
        NodeKind::Directive(d) => match &d.args {
            DirectiveArgs::Page { attrs, title } => {
                h.push("@page");
                if let Some(list) = attrs {
                    h.push(" ");
                    h.push_list(list);
                }
                if let Some(title) = title {
                    h.push(" ");
                    h.push(&title.raw);
                }
            }
            // A list with text after its `]` (an error) is kept as written
            DirectiveArgs::Let(def)
                if matches!(def.form, LetForm::Bundle(_)) && ends_with_list(node) =>
            {
                let LetForm::Bundle(list) = &def.form else {
                    return None;
                };
                h.push("@let ");
                h.push(&def.name);
                h.push(" ");
                h.push_list(list);
            }
            DirectiveArgs::Let(def)
                if matches!(def.form, LetForm::Function(_)) && ends_with_list(node) =>
            {
                let LetForm::Function(function) = &def.form else {
                    return None;
                };
                h.push("@let @");
                h.push(&def.name);
                if let Some(list) = &function.list {
                    h.push(" ");
                    h.push_list(list);
                }
            }
            _ if node.line_count > 1 => return None,
            _ => h.push(&node.source),
        },
        _ => return None,
    }
    Some(h)
}

/// Whether a `@let` line is written as the formatter would print it, up
/// to spacing: its name as the tree reads it, then its list (if any) and
/// nothing after the `]`. Anything else (`@let $pad [..]`, `@let @c $t`,
/// text after the `]`) is an error the formatter keeps as written.
fn ends_with_list(node: &Node) -> bool {
    let Some(DirectiveArgs::Let(def)) = node.directive().map(|d| &d.args) else {
        return true;
    };
    let (list, name) = match &def.form {
        LetForm::Bundle(list) => (Some(list), def.name.clone()),
        LetForm::Function(function) => (function.list.as_ref(), format!("@{}", def.name)),
        _ => return true,
    };
    let first = node.source.lines().next().unwrap_or("");
    let head = first.split('[').next().unwrap_or(first);
    let words: Vec<&str> = head.split_whitespace().collect();
    words == ["@let", name.as_str()]
        && list.is_none_or(|list| !list.closed || node.source.trim_end().ends_with(']'))
}

fn print_header(header: Header, node: &Node, level: usize, out: &mut String) {
    let indent = "  ".repeat(level);
    let len = level * 2 + header.text.len();
    let was_wrapped = node.line_count > 1;
    if let Some((start, end, attrs)) = &header.list {
        // Wrap when too long; a list that was already wrapped stays wrapped
        // above the lower threshold (hysteresis).
        let wrap = len > MAX_LINE_WIDTH || (was_wrapped && len > WRAP_MIN_WIDTH);
        // A line of a list that starts with `--` is a comment, so a custom
        // property (`--gap 12`) can't start one: such a list stays on its line
        let custom = attrs.iter().any(|a| a.starts_with("--"));
        if wrap && attrs.len() > 1 && !custom {
            out.push_str(&indent);
            out.push_str(&header.text[..*start]);
            out.push_str("[\n");
            for (i, attr) in attrs.iter().enumerate() {
                out.push_str(&indent);
                out.push_str("  ");
                out.push_str(attr);
                if i + 1 < attrs.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&indent);
            out.push(']');
            out.push_str(&header.text[*end..]);
            out.push('\n');
            return;
        }
    }
    out.push_str(&indent);
    out.push_str(&header.text);
    out.push('\n');
}

/// Re-indent a verbatim line by `delta` bytes, keeping its relative
/// indentation. Removes at most the existing leading whitespace.
fn shift_line(line: &str, delta: i32) -> String {
    if delta >= 0 {
        format!("{}{}", " ".repeat(delta as usize), line)
    } else {
        let lead = line.len() - line.trim_start().len();
        let remove = (delta.unsigned_abs() as usize).min(lead);
        line[remove..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotent_short_line() {
        let src = "@row [spacing 10]\n  Hi\n";
        assert_eq!(format(src), format(&format(src)));
    }

    #[test]
    fn idempotent_at_wrap_threshold() {
        // Build a line long enough to cross MAX_LINE_WIDTH after the first format.
        let src = "@row [padding 20, background white, border-radius 8, border 1 solid #e5e7eb, color #333, box-shadow 0 2px 4px rgba(0,0,0,0.1)]\n  child\n";
        let a = format(src);
        let b = format(&a);
        assert_eq!(a, b, "formatter must be idempotent across runs");
        assert!(a.starts_with("@row [\n  padding 20,\n"), "{a}");
    }

    #[test]
    fn a_whole_attribute_if_prints_its_groups_like_lists() {
        let src = "@el [if($on, [\n    padding 8,\n    color red\n  ], [ ]),  if($on,   margin 2)]\n  a\n";
        let out = format(src);
        assert_eq!(
            out,
            "@el [if($on, [padding 8, color red], []), if($on, margin 2)]\n  a\n"
        );
        assert_eq!(format(&out), out);
        // A misshapen if() is kept as written
        let src = "@el [if($on, [padding 4] junk), if($on)]\n  a\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn a_list_with_a_custom_property_is_not_wrapped() {
        // Wrapped, `--gap 12px,` would start a line and become a comment
        let input = format!(
            "@el [padding 4, --gap 12px, {}]\n  x\n",
            vec!["color red"; 12].join(", ")
        );
        let output = format(&input);
        assert_eq!(output, input);
        assert!(output.lines().all(|l| !l.trim_start().starts_with("--")));
    }

    #[test]
    fn preserves_trailing_comment() {
        let src = "@text [font-weight bold] hello -- greeting\n";
        let out = format(src);
        assert!(
            out.contains("-- greeting"),
            "trailing comment should survive format: {out}"
        );
    }

    #[test]
    fn bracket_inside_string_is_ignored() {
        let src = "@text [content \"a [b] c\", font-weight bold] hi\n";
        let out = format(src);
        // Single-line emission — no multi-line bracket block should be triggered.
        assert_eq!(out.lines().count(), 1, "got:\n{out}");
    }

    #[test]
    fn comments_inside_brackets_are_not_split() {
        let src = "@el [content \"--not a comment\"]\n";
        let out = format(src);
        assert!(out.contains("\"--not a comment\""));
    }

    #[test]
    fn apostrophe_does_not_swallow_file() {
        let src = "@button [aria-label=Don't click, padding 10] Go\n@text after\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn unclosed_bracket_is_kept_verbatim() {
        let src = "@el [padding 10,\n color red\n@text x\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn bracket_in_text_does_not_join_lines() {
        let src = "@el\n  Use [ to open a list\n  @text [font-weight bold] Second\n";
        assert_eq!(format(src), src);
        let src = "@text Use [ to open\n@text next\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn urls_with_brackets_are_kept() {
        let src = "@link /a[x,y] T\n@image [alt=a] /img[1].png\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn commas_inside_parens_stay_in_value() {
        let src = "@el [box-shadow 0 1px 3px rgba(0,0,0,0.1),font-weight bold] hi\n";
        assert_eq!(
            format(src),
            "@el [box-shadow 0 1px 3px rgba(0,0,0,0.1), font-weight bold] hi\n"
        );
    }

    #[test]
    fn escaped_commas_stay_in_value() {
        let src = "@el [transition opacity 0.3s ease-in-out\\, transform 0.3s ease-in-out\\, box-shadow 0.3s, font-family \"Open Sans\"\\, sans-serif, content \"a\\\"]\"] hi\n";
        let out = format(src);
        assert_eq!(
            out,
            "@el [\n  transition opacity 0.3s ease-in-out\\, transform 0.3s ease-in-out\\, box-shadow 0.3s,\n  font-family \"Open Sans\"\\, sans-serif,\n  content \"a\\\"]\"\n] hi\n"
        );
        assert_eq!(format(&out), out);
        let src = "@el [padding 4\\]x, color red] hi\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn attribute_order_is_preserved() {
        let src = "@el [center-x, margin 0, width 200]\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn raw_blocks_are_verbatim() {
        let src = "@raw\n  <pre>\n          deeply indented\n  </pre>\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn script_body_is_not_reformatted() {
        let src = "@el\n    @script\n        let [x,width] = f();\n";
        assert_eq!(format(src), "@el\n  @script\n      let [x,width] = f();\n");
    }

    #[test]
    fn comment_inside_multiline_brackets_is_idempotent() {
        let src = "@el [\n  padding 10,\n  -- note\n  color red\n]\n  @text hi\n";
        let once = format(src);
        assert_eq!(once, src);
        assert_eq!(format(&once), once);
    }

    #[test]
    fn indentation_is_normalized_and_trivia_kept() {
        let src = "-- header\n@el\n    @text a\n\n    -- note\n    @row\n        @text b\n";
        assert_eq!(
            format(src),
            "-- header\n@el\n  @text a\n\n  -- note\n  @row\n    @text b\n"
        );
    }

    #[test]
    fn multi_line_json_is_kept() {
        let src = "@data $team [\n  {\"name\": \"Ada\", \"role\": \"Engineering, research\"},\n  {\"name\": \"Grace\"}\n]\n@text x\n";
        assert_eq!(format(src), src);
    }

    #[test]
    fn function_heads_are_normalized() {
        let src = "@let  @card[title,tone #fff]\n  @el $title\n@let   @spacer\n  @el\n";
        assert_eq!(
            format(src),
            "@let @card [title, tone #fff]\n  @el $title\n@let @spacer\n  @el\n"
        );
        // Text after the list is an error; the formatter keeps it
        let src = "@let @card [title] extra\n  @el $title\n";
        assert_eq!(format(src), src);
        // So are parameters outside brackets and a `$` before the name
        for src in [
            "@let @c $t\n  @el $t\n",
            "@let  @$c [t]\n  @el $t\n",
            "@let  $pad [padding 4]\n",
        ] {
            assert_eq!(format(src), src);
        }
    }

    #[test]
    fn heads_are_normalized() {
        let src = "@el[padding 4]  >  @link /x Go\n@let card [padding 4,color red]\n@page [lang en]  Home\n";
        assert_eq!(
            format(src),
            "@el [padding 4] > @link /x Go\n@let card [padding 4, color red]\n@page [lang en] Home\n"
        );
    }
}
