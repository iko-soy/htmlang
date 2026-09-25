//! Parsing: source text to a syntax tree, before anything is evaluated.
//!
//! This step knows the shape of the language: indentation, continuation
//! lines of attribute lists, verbatim blocks (`@raw """…"""`, the bodies of
//! `@markdown` and `@script`), multi-line strings, and the directives whose
//! structure spans several lines (`@if` / `@else` chains, `@each` with an
//! `@else`, function definitions). It reads no files and substitutes no
//! variables; `parser.rs` evaluates the tree.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) enum LineContent {
    Normal(String),
    Raw(String),
}

/// One logical source line (continuations joined), without its indented
/// children.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub indent: usize,
    pub content: LineContent,
    pub line_num: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum Syntax {
    /// Verbatim text.
    Raw { line: usize, text: String },
    /// An element, text, a function call or a single-line directive, with
    /// the lines indented under it.
    Line {
        line: usize,
        indent: usize,
        text: String,
        children: Vec<Syntax>,
    },
    /// `@let NAME $param $param=default` with an indented body.
    Function {
        line: usize,
        name: String,
        params: Vec<String>,
        defaults: HashMap<String, String>,
        body: Vec<Syntax>,
    },
    /// `@if` with its `@else if` and `@else` branches.
    If { branches: Vec<Branch> },
    /// `@each $var in LIST` with its body and the `@else` for an empty list.
    Each {
        line: usize,
        header: String,
        body: Vec<Syntax>,
        empty: Vec<Syntax>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Branch {
    pub line: usize,
    /// `None` for `@else`.
    pub condition: Option<String>,
    pub body: Vec<Syntax>,
}

impl Syntax {
    pub fn line(&self) -> usize {
        match self {
            Syntax::Raw { line, .. }
            | Syntax::Line { line, .. }
            | Syntax::Function { line, .. }
            | Syntax::Each { line, .. } => *line,
            Syntax::If { branches } => branches.first().map_or(0, |b| b.line),
        }
    }

    /// The node's first source line, for diagnostics.
    pub fn source(&self) -> String {
        match self {
            Syntax::Raw { text, .. } | Syntax::Line { text, .. } => text.clone(),
            Syntax::Function { name, params, defaults, .. } => {
                let mut out = format!("@let {}", name);
                for param in params {
                    match defaults.get(param) {
                        Some(default) => out.push_str(&format!(" ${}={}", param, default)),
                        None => out.push_str(&format!(" ${}", param)),
                    }
                }
                out
            }
            Syntax::If { branches } => branches
                .first()
                .map(|b| format!("@if {}", b.condition.as_deref().unwrap_or("")))
                .unwrap_or_default(),
            Syntax::Each { header, .. } => format!("@each {}", header),
        }
    }
}

/// Parse source text into a syntax tree.
pub(crate) fn parse(input: &str) -> Vec<Syntax> {
    let lines = preprocess(input);
    let mut pos = 0;
    build(&lines, &mut pos, None, false)
}

/// Build the nodes for the lines from `pos` indented deeper than `parent`.
/// A `plain` block is text (the body of `@style`, `@head` or `@keyframes`),
/// so no directive in it is given structure.
fn build(lines: &[Line], pos: &mut usize, parent: Option<usize>, plain: bool) -> Vec<Syntax> {
    let mut nodes = Vec::new();
    let deeper = |line: &Line| parent.is_none_or(|p| line.indent > p);
    while *pos < lines.len() && deeper(&lines[*pos]) {
        let line = &lines[*pos];
        *pos += 1;
        let text = match &line.content {
            LineContent::Raw(text) => {
                nodes.push(Syntax::Raw {
                    line: line.line_num,
                    text: text.clone(),
                });
                continue;
            }
            LineContent::Normal(text) => text.clone(),
        };
        // Text and one-line directives take no body: lines indented under
        // them are their siblings.
        if !plain && !takes_body(&text) {
            nodes.push(Syntax::Line {
                line: line.line_num,
                indent: line.indent,
                text,
                children: Vec::new(),
            });
            continue;
        }
        let text_body = plain
            || text == "@style"
            || text == "@head"
            || text.starts_with("@head ")
            || text.starts_with("@keyframes ");
        let children = build(lines, pos, Some(line.indent), text_body);
        if plain {
            nodes.push(Syntax::Line {
                line: line.line_num,
                indent: line.indent,
                text,
                children,
            });
            continue;
        }

        if let Some(condition) = text.strip_prefix("@if ") {
            let mut branches = vec![Branch {
                line: line.line_num,
                condition: Some(condition.trim().to_string()),
                body: children,
            }];
            while let Some((next_line, next)) = sibling(lines, *pos, line.indent) {
                let condition = if let Some(condition) = next.strip_prefix("@else if ") {
                    Some(condition.trim().to_string())
                } else if next == "@else" {
                    None
                } else {
                    break;
                };
                *pos += 1;
                let last = condition.is_none();
                branches.push(Branch {
                    line: next_line,
                    condition,
                    body: build(lines, pos, Some(line.indent), false),
                });
                if last {
                    break;
                }
            }
            nodes.push(Syntax::If { branches });
            continue;
        }

        if let Some(header) = text.strip_prefix("@each ") {
            let mut empty = Vec::new();
            if let Some((_, "@else")) = sibling(lines, *pos, line.indent) {
                *pos += 1;
                empty = build(lines, pos, Some(line.indent), false);
            }
            nodes.push(Syntax::Each {
                line: line.line_num,
                header: header.trim().to_string(),
                body: children,
                empty,
            });
            continue;
        }

        if let Some(rest) = text.strip_prefix("@let ")
            && !children.is_empty()
            && let Some((name, params)) = rest.split_whitespace().collect::<Vec<_>>().split_first()
        {
            let mut names = Vec::new();
            let mut defaults = HashMap::new();
            for param in params {
                let param = param.strip_prefix('$').unwrap_or(param);
                match param.split_once('=') {
                    Some((param, default)) => {
                        names.push(param.to_string());
                        defaults.insert(param.to_string(), default.to_string());
                    }
                    None => names.push(param.to_string()),
                }
            }
            nodes.push(Syntax::Function {
                line: line.line_num,
                name: name.to_string(),
                params: names,
                defaults,
                body: children,
            });
            continue;
        }

        nodes.push(Syntax::Line {
            line: line.line_num,
            indent: line.indent,
            text,
            children,
        });
    }
    nodes
}

/// Whether the lines indented under `text` belong to it.
fn takes_body(text: &str) -> bool {
    const ONE_LINE: &[&str] = &["@page", "@meta", "@data", "@include"];
    let name = text.split([' ', '[']).next().unwrap_or("");
    text.starts_with('@') && !ONE_LINE.contains(&name)
}

/// The line at `pos` if it is a sibling at `indent`, with its text.
fn sibling(lines: &[Line], pos: usize, indent: usize) -> Option<(usize, &str)> {
    let line = lines.get(pos).filter(|l| l.indent == indent)?;
    match &line.content {
        LineContent::Normal(text) => Some((line.line_num, text.trim())),
        LineContent::Raw(_) => None,
    }
}

/// For `@let name """…` (or `@let name = """…`) without the closing `"""`
/// on the same line, the text after the opening quotes.
fn opens_multiline_string(line: &str) -> Option<&str> {
    let (_, value) = line.strip_prefix("@let ")?.trim_start().split_once(' ')?;
    let value = value.trim_start();
    let value = value.strip_prefix('=').map_or(value, str::trim_start);
    let open = value.strip_prefix("\"\"\"")?;
    if open.len() >= 3 && open.ends_with("\"\"\"") {
        return None;
    }
    Some(open)
}

pub(crate) fn preprocess(input: &str) -> Vec<Line> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let mut lines = Vec::new();
    let mut i = 0;

    while i < raw_lines.len() {
        let line = raw_lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with("--") {
            i += 1;
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Inline `@markdown` and `@script` blocks keep their bodies verbatim:
        // in Markdown, blank lines separate paragraphs, `---` is a rule (not
        // a comment) and code indentation matters; in JavaScript, newlines,
        // `{...}` and `$` must reach the output untouched.
        let verbatim_body = trimmed == "@markdown"
            || trimmed == "@script"
            || trimmed.starts_with("@script ")
            || trimmed.starts_with("@script[");
        if verbatim_body {
            lines.push(Line {
                indent,
                content: LineContent::Normal(trimmed.to_string()),
                line_num: i + 1,
            });
            let body_start = i + 1;
            let mut body_end = body_start;
            let mut j = body_start;
            while j < raw_lines.len() {
                let l = raw_lines[j];
                if l.trim().is_empty() {
                    j += 1;
                    continue;
                }
                if l.len() - l.trim_start().len() <= indent {
                    break;
                }
                j += 1;
                body_end = j;
            }
            let body = &raw_lines[body_start..body_end];
            let body_indent = body
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);
            if !body.is_empty() {
                let text: Vec<&str> = body
                    .iter()
                    .map(|l| l.get(body_indent..).unwrap_or("").trim_end())
                    .collect();
                lines.push(Line {
                    indent: indent + 1,
                    content: LineContent::Raw(text.join("\n")),
                    line_num: body_start + 1,
                });
            }
            i = body_end.max(body_start);
            continue;
        }

        // Handle @raw """..."""
        if let Some(raw_rest) = trimmed.strip_prefix("@raw") {
            let after_raw = raw_rest.trim_start();
            if let Some(after_open) = after_raw.strip_prefix("\"\"\"") {
                // Single-line: @raw """content"""
                if after_open.ends_with("\"\"\"") && after_open.len() >= 3 {
                    let content = &after_open[..after_open.len() - 3];
                    lines.push(Line {
                        indent,
                        content: LineContent::Raw(content.to_string()),
                        line_num: i + 1,
                    });
                    i += 1;
                    continue;
                }

                // Multiline: collect until closing """
                let mut raw_content = String::new();
                if !after_open.is_empty() {
                    raw_content.push_str(after_open);
                    raw_content.push('\n');
                }
                i += 1;
                while i < raw_lines.len() {
                    if raw_lines[i].trim() == "\"\"\"" {
                        i += 1;
                        break;
                    }
                    raw_content.push_str(raw_lines[i]);
                    raw_content.push('\n');
                    i += 1;
                }

                lines.push(Line {
                    indent,
                    content: LineContent::Raw(raw_content.trim_end_matches('\n').to_string()),
                    line_num: i,
                });
                continue;
            }
        }

        // `@let name """` opens a multi-line string running to a `"""` line;
        // it becomes one line holding the whole value.
        if let Some(open) = opens_multiline_string(trimmed) {
            let mut value: Vec<&str> = Vec::new();
            if !open.is_empty() {
                value.push(open);
            }
            let first_line_num = i + 1;
            i += 1;
            while i < raw_lines.len() {
                let next = raw_lines[i].trim();
                i += 1;
                if next == "\"\"\"" {
                    break;
                }
                if !next.is_empty() {
                    value.push(next);
                }
            }
            let head = &trimmed[..trimmed.len() - open.len()];
            lines.push(Line {
                indent,
                content: LineContent::Normal(format!("{}{}\"\"\"", head, value.join("\n"))),
                line_num: first_line_num,
            });
            continue;
        }

        // Join continuation lines for multi-line attribute brackets
        let first_line_num = i + 1;
        let mut full = trimmed.to_string();
        while open_attr_depth(&full) > 0 && i + 1 < raw_lines.len() {
            i += 1;
            let next = raw_lines[i].trim();
            if next.is_empty() || next.starts_with("--") {
                continue;
            }
            full.push(' ');
            full.push_str(next);
        }

        lines.push(Line {
            indent,
            content: LineContent::Normal(full),
            line_num: first_line_num,
        });
        i += 1;
    }

    lines
}

/// Bracket depth left open at the end of `line`, counting only attribute
/// lists: a `[` that starts the line or follows an `@name` token (optionally
/// with one more word, as in `@let name [`). Brackets in text content, such
/// as `@text [bold] Use [ to open`, are ignored.
pub(crate) fn open_attr_depth(line: &str) -> i32 {
    if !line.starts_with('@') {
        // Text: an inline `{@name [` whose list runs onto the next line
        let Some(start) = line.rfind("{@") else {
            return 0;
        };
        let inline = &line[start + 1..];
        return if inline.contains('}') { 0 } else { open_attr_depth(inline) };
    }
    let bytes = line.as_bytes();
    let mut depth: i32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' if depth > 0 => depth += 1,
            b'[' => {
                // Is this the start of an attribute list?
                let before = line[..i].trim_end();
                let last_directive = before
                    .rsplit([']', '>'])
                    .next()
                    .unwrap_or("")
                    .trim();
                let tokens: Vec<&str> = last_directive.split_whitespace().collect();
                let starts_list = match tokens.as_slice() {
                    [] => before.is_empty() || before.ends_with('>'),
                    [name] => name.starts_with('@'),
                    [name, _] => name.starts_with('@'),
                    _ => false,
                };
                if starts_list {
                    depth = 1;
                } else if before.ends_with(']') {
                    // Text after a closed attribute list: stop scanning.
                    return 0;
                }
            }
            b']' if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    depth
}


#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(nodes: &[Syntax]) -> Vec<&'static str> {
        nodes
            .iter()
            .map(|n| match n {
                Syntax::Raw { .. } => "raw",
                Syntax::Line { .. } => "line",
                Syntax::Function { .. } => "function",
                Syntax::If { .. } => "if",
                Syntax::Each { .. } => "each",
            })
            .collect()
    }

    #[test]
    fn groups_control_flow_and_definitions() {
        let tree = parse(
            "@let card $title $icon=x\n  @el\n    @children\n@if $a\n  A\n@else if $b\n  B\n@else\n  C\n@each $x in 1..3\n  $x\n@else\n  none\n@text done\n",
        );
        assert_eq!(kinds(&tree), ["function", "if", "each", "line"]);
        let Syntax::Function { name, params, defaults, body, .. } = &tree[0] else { unreachable!() };
        assert_eq!((name.as_str(), params.len(), defaults["icon"].as_str()), ("card", 2, "x"));
        assert_eq!(kinds(body), ["line"]);
        let Syntax::If { branches } = &tree[1] else { unreachable!() };
        let conditions: Vec<_> = branches.iter().map(|b| b.condition.as_deref()).collect();
        assert_eq!(conditions, [Some("$a"), Some("$b"), None]);
        let Syntax::Each { empty, .. } = &tree[2] else { unreachable!() };
        assert_eq!(kinds(empty), ["line"]);
    }

    #[test]
    fn text_bodies_stay_plain() {
        let tree = parse("@style\n  @if x {\n    a\n  }\n");
        let Syntax::Line { children, .. } = &tree[0] else { unreachable!() };
        assert_eq!(kinds(children), ["line", "line"]);
    }

    #[test]
    fn multiline_strings_become_one_line() {
        let tree = parse("@let msg \"\"\"\n  Hello\n  there\n\"\"\"\n@text $msg\n");
        assert_eq!(kinds(&tree), ["line", "line"]);
        assert_eq!(tree[0].source(), "@let msg \"\"\"Hello\nthere\"\"\"");
    }
}
