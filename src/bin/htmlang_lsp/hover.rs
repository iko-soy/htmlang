use tower_lsp::lsp_types::*;

use crate::docs;

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
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ")
            && let Some((n, v)) = rest.trim().split_once(' ')
            && n == name
        {
            return Some(format!("**${}** = `{}`", name, v.trim()));
        }
    }

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let rest = rest.trim();
            // Attribute bundle: @let name [...]
            if let Some(bracket) = rest.find('[') {
                let def_name = rest[..bracket].trim();
                if def_name == name {
                    return Some(format!(
                        "**${}** \u{2014} Attribute bundle\n\n`{}`",
                        name, trimmed
                    ));
                }
            }
            // Function parameter: @let fn-name $param (with body)
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(fn_name) = parts.first() {
                for param in &parts[1..] {
                    let p = param.strip_prefix('$').unwrap_or(param);
                    if p == name {
                        return Some(format!(
                            "**${}** \u{2014} Parameter of `@{}`",
                            name, fn_name
                        ));
                    }
                }
            }
        }
    }

    None
}

fn hover_user_fn(text: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.first() == Some(&name) {
                let params = &parts[1..];

                // Collect doc-comment lines above the definition (lines starting with --)
                let mut doc_lines: Vec<&str> = Vec::new();
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    let prev = lines[j].trim();
                    if let Some(comment) = prev.strip_prefix("-- ") {
                        doc_lines.push(comment);
                    } else if let Some(comment) = prev.strip_prefix("--") {
                        doc_lines.push(comment);
                    } else {
                        break;
                    }
                }
                doc_lines.reverse();

                let doc_str = if doc_lines.is_empty() {
                    String::new()
                } else {
                    format!("\n\n{}", doc_lines.join("\n"))
                };

                // Format params showing defaults
                let params_str = if params.is_empty() {
                    String::new()
                } else {
                    let formatted: Vec<String> = params
                        .iter()
                        .map(|p| {
                            if p.contains('=') {
                                let (name, default) = p.split_once('=').unwrap();
                                format!("{} (default: {})", name, default)
                            } else {
                                p.to_string()
                            }
                        })
                        .collect();
                    format!("\n\nParameters: {}", formatted.join(", "))
                };

                return Some(format!(
                    "**@{}** \u{2014} User function{}{}",
                    name, params_str, doc_str
                ));
            }
        }
    }
    None
}
