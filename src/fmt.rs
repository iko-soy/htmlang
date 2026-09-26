const MAX_LINE_WIDTH: usize = 100;
/// Once a line has already been wrapped over multiple attribute lines, we keep it
/// wrapped even after formatting as long as the attribute count stays >1. This
/// prevents format→format→format oscillation around the wrap threshold.
const WRAP_MIN_WIDTH: usize = 80;

/// Split an attribute list on top-level commas: commas inside `(...)`,
/// `[...]`, `{...}` or `"..."` belong to the value (e.g. `rgba(0,0,0,0.1)`).
fn split_attrs(attrs_str: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut start = 0;
    for (i, c) in attrs_str.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '(' | '[' | '{' if !in_str => depth += 1,
            ')' | ']' | '}' if !in_str => depth -= 1,
            ',' if !in_str && depth <= 0 => {
                parts.push(&attrs_str[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&attrs_str[start..]);
    parts
        .into_iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Normalize spacing in a comma-separated attribute string. Attribute order
/// is preserved: later attributes can override earlier ones (e.g.
/// `center-x, margin 0`), so reordering could change the output.
fn normalize_attrs(attrs_str: &str) -> String {
    split_attrs(attrs_str).join(", ")
}

/// Bracket depth left open by the attribute lists on a (possibly joined)
/// line. Only brackets that start an attribute list count: one at the start
/// of the line, one right after the `@name` (or `@let name`) head, or one
/// after a chained `> @name`. Brackets in trailing text are ignored, so
/// `@text Use [ to open` does not start a multi-line block.
fn open_attr_depth(line: &str) -> i32 {
    let mut depth = 0i32;
    let mut region_seen = false;
    for (i, b) in line.bytes().enumerate() {
        if depth > 0 {
            match b {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            continue;
        }
        if b != b'[' {
            continue;
        }
        let before = line[..i].trim_end();
        let starts_region = if !region_seen {
            // Only `@name` or `@name word` may precede the first list.
            let mut tokens = before.split_whitespace();
            match (tokens.next(), tokens.next(), tokens.next()) {
                (None, _, _) => true,
                (Some(t), None, _) | (Some(t), Some(_), None) => t.starts_with('@'),
                _ => false,
            }
        } else {
            // A later list belongs to a chained element: `... > @link [`.
            let mut tokens = before.split_whitespace().rev();
            matches!((tokens.next(), tokens.next()), (Some(t), Some(">")) if t.starts_with('@'))
        };
        if starts_region {
            region_seen = true;
            depth = 1;
        } else if !region_seen {
            // Text precedes the first bracket; nothing on this line opens an
            // attribute list.
            return 0;
        }
    }
    depth
}

/// Directives whose indented body is foreign content (CSS, JS, Markdown,
/// JSON, raw HTML) that must not be reformatted.
fn is_raw_body_header(code: &str) -> bool {
    [
        "@style",
        "@script",
        "@markdown",
        "@head",
        "@raw",
    ]
    .iter()
    .any(|d| code == *d || code.strip_prefix(d).is_some_and(|r| r.starts_with(' ')))
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

/// Split a line into (code, trailing_comment). The comment starts at the first `--`
/// that lies outside quoted strings and outside `[...]` attribute lists. Returns
/// (whole_line, None) if there is no trailing comment.
fn split_trailing_comment(line: &str) -> (&str, Option<&str>) {
    let bytes = line.as_bytes();
    let mut in_str: Option<u8> = None;
    let mut bracket_depth = 0i32;
    let mut i = 0;
    while i + 1 < bytes.len() {
        let c = bytes[i];
        match in_str {
            Some(q) => {
                if c == b'\\' {
                    i += 2;
                    continue;
                } else if c == q {
                    in_str = None;
                }
            }
            None => match c {
                b'"' | b'\'' => in_str = Some(c),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth -= 1,
                b'-' if bytes[i + 1] == b'-'
                    && bracket_depth == 0
                    // Require a space (or start-of-line) before the `--` so we
                    // don't split inside identifiers or CSS values like `a--b`.
                    && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') =>
                {
                    let (before, after) = line.split_at(i);
                    return (before.trim_end(), Some(after));
                }
                _ => {}
            },
        }
        i += 1;
    }
    (line, None)
}

/// Format an htmlang source file with normalized indentation (2 spaces per level)
/// and cleaned-up whitespace. Bodies of raw-content directives (`@style`,
/// `@script`, ...) are kept verbatim apart from a uniform shift.
pub fn format(input: &str) -> String {
    let mut output = String::new();
    let mut indent_stack: Vec<i32> = vec![-1]; // sentinel
    let mut bracket_depth: i32 = 0;
    let mut bracket_base_level: usize = 0;
    let mut bracket_content = String::new();
    // Original lines of the pending multi-line bracket block, emitted as-is
    // (re-indented) if the block holds comments, or verbatim if it never closes.
    let mut bracket_raw: Vec<&str> = Vec::new();
    let mut bracket_has_comment = false;
    let mut bracket_delta: i32 = 0;
    let mut pending_comment: Option<String> = None;
    // Inside a raw-content body: (header raw indent, indent delta).
    let mut raw_body: Option<(i32, i32)> = None;

    for line in input.lines() {
        let trimmed = line.trim();

        if let Some((header_indent, delta)) = raw_body {
            if trimmed.is_empty() {
                output.push('\n');
                continue;
            }
            let raw_indent = (line.len() - line.trim_start().len()) as i32;
            if raw_indent > header_indent {
                output.push_str(&shift_line(line, delta));
                output.push('\n');
                continue;
            }
            raw_body = None;
        }

        // Inside multi-line bracket continuation — collect content. Blank and
        // comment lines are skipped by the parser here, so they must not end
        // up in the attribute list.
        if bracket_depth > 0 {
            bracket_raw.push(line);
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("--") {
                bracket_has_comment = true;
                continue;
            }
            bracket_content.push(' ');
            bracket_content.push_str(trimmed);
            bracket_depth = open_attr_depth(&bracket_content);
            if bracket_depth <= 0 {
                let level = bracket_base_level;
                let raw_lines = std::mem::take(&mut bracket_raw);
                let full = std::mem::take(&mut bracket_content);
                if bracket_has_comment {
                    // Keep the block as written (re-indented) so comments stay
                    // where the author put them.
                    bracket_has_comment = false;
                    pending_comment = None;
                    for (i, raw) in raw_lines.iter().enumerate() {
                        if i == 0 {
                            output.push_str(&"  ".repeat(level));
                            output.push_str(raw.trim());
                        } else if !raw.trim().is_empty() {
                            output.push_str(&shift_line(raw, bracket_delta));
                        }
                        output.push('\n');
                    }
                    continue;
                }
                let formatted = format_line_with_brackets(&full);
                let attrs_count = count_attrs(&formatted);
                let indented_len = level * 2 + formatted.len();
                // The original was wrapped, so stay wrapped above WRAP_MIN_WIDTH (hysteresis).
                let should_wrap = (indented_len > MAX_LINE_WIDTH)
                    || (attrs_count > 1 && indented_len > WRAP_MIN_WIDTH);
                if should_wrap && let Some(mut wrapped) = wrap_attrs(&formatted, level) {
                    if let Some(cmt) = pending_comment.take() {
                        if wrapped.ends_with('\n') {
                            wrapped.pop();
                        }
                        wrapped.push(' ');
                        wrapped.push_str(&cmt);
                        wrapped.push('\n');
                    }
                    output.push_str(&wrapped);
                    continue;
                }
                output.push_str(&"  ".repeat(level));
                output.push_str(&formatted);
                if let Some(cmt) = pending_comment.take() {
                    output.push(' ');
                    output.push_str(&cmt);
                }
                output.push('\n');
            }
            continue;
        }

        // Preserve blank lines
        if trimmed.is_empty() {
            output.push('\n');
            continue;
        }

        // Preserve comments at current indent level
        if trimmed.starts_with("--") {
            let raw_indent = (line.len() - line.trim_start().len()) as i32;
            while indent_stack.len() > 1 && *indent_stack.last().unwrap() >= raw_indent {
                indent_stack.pop();
            }
            let level = indent_stack.len() - 1;
            output.push_str(&"  ".repeat(level));
            output.push_str(trimmed);
            output.push('\n');
            indent_stack.push(raw_indent);
            continue;
        }

        let raw_indent = (line.len() - line.trim_start().len()) as i32;

        // Pop stack to find parent
        while indent_stack.len() > 1 && *indent_stack.last().unwrap() >= raw_indent {
            indent_stack.pop();
        }

        let level = indent_stack.len() - 1;
        let is_code = trimmed.starts_with('@');

        // Split off a trailing `-- comment` before doing bracket math.
        let (code, trailing) = split_trailing_comment(trimmed);
        let code = code.trim_end();
        let trailing = trailing.map(|s| s.trim().to_string());

        bracket_depth = if is_code { open_attr_depth(code) } else { 0 };
        if bracket_depth > 0 {
            // Start of multi-line bracket
            bracket_base_level = level;
            bracket_content = code.to_string();
            bracket_raw = vec![line];
            bracket_has_comment = false;
            bracket_delta = (level * 2) as i32 - raw_indent;
            pending_comment = trailing;
            indent_stack.push(raw_indent);
            continue;
        }

        // Only element/attribute lines have attribute lists; text lines are
        // left untouched so bracketed prose isn't rewritten.
        let formatted = if is_code {
            format_line_with_brackets(code)
        } else {
            code.to_string()
        };

        if is_raw_body_header(code) {
            raw_body = Some((raw_indent, (level * 2) as i32 - raw_indent));
        }

        let indented_len = level * 2 + formatted.len();
        // Single-line emission — only wrap when we strictly exceed the ceiling.
        if is_code
            && indented_len > MAX_LINE_WIDTH
            && formatted.contains('[')
            && let Some(mut wrapped) = wrap_attrs(&formatted, level)
        {
            if let Some(cmt) = &trailing {
                // Reattach comment on the final `]` line.
                if wrapped.ends_with('\n') {
                    wrapped.pop();
                }
                wrapped.push(' ');
                wrapped.push_str(cmt);
                wrapped.push('\n');
            }
            output.push_str(&wrapped);
            indent_stack.push(raw_indent);
            continue;
        }

        output.push_str(&"  ".repeat(level));
        output.push_str(&formatted);
        if let Some(cmt) = trailing {
            output.push(' ');
            output.push_str(&cmt);
        }
        output.push('\n');

        indent_stack.push(raw_indent);
    }

    // An attribute list that never closed: emit what we collected unchanged
    // rather than dropping it.
    for raw in bracket_raw {
        output.push_str(raw);
        output.push('\n');
    }

    output
}

/// Find the byte range of the first top-level `[...]` in `line`, ignoring
/// brackets inside double-quoted strings.
fn find_attr_brackets(line: &str) -> Option<(usize, usize)> {
    let bracket_start = line.find('[')?;
    let mut depth = 0;
    let mut in_str = false;
    let mut prev = '\0';
    for (i, c) in line.char_indices().skip_while(|&(i, _)| i < bracket_start) {
        if in_str {
            if prev != '\\' && c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((bracket_start, i));
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

/// Format a single line, normalizing the attribute list inside [...] brackets.
fn format_line_with_brackets(line: &str) -> String {
    let Some((bracket_start, bracket_end)) = find_attr_brackets(line) else {
        return line.to_string();
    };
    let before = &line[..bracket_start];
    let attrs_inner = &line[bracket_start + 1..bracket_end];
    let after = &line[bracket_end + 1..];
    format!("{}[{}]{}", before, normalize_attrs(attrs_inner), after)
}

fn count_attrs(line: &str) -> usize {
    match find_attr_brackets(line) {
        Some((start, end)) => split_attrs(&line[start + 1..end]).len(),
        None => 0,
    }
}

fn wrap_attrs(line: &str, indent_level: usize) -> Option<String> {
    let (bracket_start, bracket_end) = find_attr_brackets(line)?;
    let before = &line[..bracket_start];
    let attrs_inner = &line[bracket_start + 1..bracket_end];
    let after = &line[bracket_end + 1..];

    let parts = split_attrs(attrs_inner);
    if parts.len() <= 1 {
        return None;
    }

    let base_indent = "  ".repeat(indent_level);
    let attr_indent = "  ".repeat(indent_level + 1);
    let mut result = String::new();
    result.push_str(&base_indent);
    result.push_str(before);
    result.push_str("[\n");
    for (i, part) in parts.iter().enumerate() {
        result.push_str(&attr_indent);
        result.push_str(part);
        if i < parts.len() - 1 {
            result.push(',');
        }
        result.push('\n');
    }
    result.push_str(&base_indent);
    result.push(']');
    result.push_str(after);
    result.push('\n');
    Some(result)
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
    }

    #[test]
    fn commas_inside_parens_stay_in_value() {
        let src = "@el [box-shadow 0 1px 3px rgba(0,0,0,0.1),font-weight bold] hi\n";
        assert_eq!(format(src), "@el [box-shadow 0 1px 3px rgba(0,0,0,0.1), font-weight bold] hi\n");
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
}
