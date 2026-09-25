use std::collections::HashMap;

use htmlang::parser::ParseResult;
use tower_lsp::lsp_types::*;

use crate::completion::in_brackets;

// ---------------------------------------------------------------------------
// Document symbols (outline view)
// ---------------------------------------------------------------------------

#[allow(deprecated)] // SymbolInformation::deprecated is deprecated but needed for the struct
pub(crate) fn document_symbols(text: &str) -> Vec<SymbolInformation> {
    let mut symbols = Vec::new();

    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let line_num = i as u32;

        // @let definitions (variables, attribute bundles, and components)
        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let rest_trimmed = rest.trim();
            let parts: Vec<&str> = rest_trimmed.split_whitespace().collect();
            if let Some(&name) = parts.first() {
                let value_part = rest_trimmed.get(name.len()..).unwrap_or("").trim_start();
                let has_body = text
                    .lines()
                    .nth(i + 1)
                    .map(|l| l.starts_with("  ") || l.starts_with('\t'))
                    .unwrap_or(false);

                if has_body && (value_part.is_empty() || value_part.starts_with('$')) {
                    // Component/function definition
                    let params = parts[1..].join(" ");
                    let detail = if params.is_empty() {
                        None
                    } else {
                        Some(format!("({})", params))
                    };
                    symbols.push(SymbolInformation {
                        name: format!("@{}", name),
                        kind: SymbolKind::FUNCTION,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: Url::parse("file:///").unwrap(), // replaced by caller
                            range: Range::new(
                                Position::new(line_num, 0),
                                Position::new(line_num, line.len() as u32),
                            ),
                        },
                        container_name: detail,
                    });
                } else if value_part.starts_with('[') {
                    // Attribute bundle
                    symbols.push(SymbolInformation {
                        name: format!("${}", name),
                        kind: SymbolKind::CONSTANT,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: Url::parse("file:///").unwrap(),
                            range: Range::new(
                                Position::new(line_num, 0),
                                Position::new(line_num, line.len() as u32),
                            ),
                        },
                        container_name: Some("attribute bundle".to_string()),
                    });
                } else if !value_part.is_empty() {
                    // Scalar variable
                    symbols.push(SymbolInformation {
                        name: format!("${}", name),
                        kind: SymbolKind::VARIABLE,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: Url::parse("file:///").unwrap(),
                            range: Range::new(
                                Position::new(line_num, 0),
                                Position::new(line_num, line.len() as u32),
                            ),
                        },
                        container_name: Some(format!("= {}", value_part)),
                    });
                }
            }
        }

        // @keyframes definitions
        if let Some(rest) = trimmed.strip_prefix("@keyframes ") {
            let name = rest.trim();
            if !name.is_empty() {
                symbols.push(SymbolInformation {
                    name: format!("@keyframes {}", name),
                    kind: SymbolKind::EVENT,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: Url::parse("file:///").unwrap(),
                        range: Range::new(
                            Position::new(line_num, 0),
                            Position::new(line_num, line.len() as u32),
                        ),
                    },
                    container_name: Some("animation".to_string()),
                });
            }
        }
    }

    symbols
}

// ---------------------------------------------------------------------------
// Code actions (quick-fixes for typo suggestions)
// ---------------------------------------------------------------------------

pub(crate) fn code_actions(
    text: &str,
    selection: &Range,
    diagnostics: &[Diagnostic],
    uri: &Url,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    for diag in diagnostics {
        let msg = &diag.message;

        // Extract "did you mean 'X'?" or "did you mean @X?" suggestions
        if let Some(suggestion) = extract_suggestion(msg) {
            let line = diag.range.start.line as usize;
            let lines: Vec<&str> = text.lines().collect();
            if let Some(source_line) = lines.get(line) {
                // Determine what to replace
                let (old_text, new_text) = if msg.contains("unknown element") {
                    // Replace @wrong with @suggestion
                    let old = extract_between(msg, "unknown element @", ",")
                        .or_else(|| extract_between(msg, "unknown element @", ""));
                    if let Some(old) = old {
                        (format!("@{}", old), format!("@{}", suggestion))
                    } else {
                        continue;
                    }
                } else if msg.contains("unknown attribute") {
                    // Replace wrong with suggestion in attribute list
                    let old = extract_between(msg, "unknown attribute '", "'");
                    if let Some(old) = old {
                        (old.to_string(), suggestion.to_string())
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                if let Some(col) = source_line.find(&old_text) {
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, col as u32),
                            Position::new(diag.range.start.line, (col + old_text.len()) as u32),
                        ),
                        new_text: new_text.clone(),
                    };

                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);

                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Replace with '{}'", new_text),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }

        // Quick-fix: remove unused @let variable
        if msg.contains("unused variable") {
            let line = diag.range.start.line as usize;
            let lines: Vec<&str> = text.lines().collect();
            if let Some(source_line) = lines.get(line) {
                let trimmed = source_line.trim_start();
                if trimmed.starts_with("@let ") {
                    let var_name = trimmed
                        .strip_prefix("@let ")
                        .and_then(|r| r.split_whitespace().next())
                        .unwrap_or("?");
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, 0),
                            Position::new(diag.range.start.line + 1, 0),
                        ),
                        new_text: String::new(),
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Remove unused variable '${}'", var_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }

        // Quick-fix: remove unused attribute bundle (@let name [...])
        if msg.contains("unused attribute bundle") {
            let line = diag.range.start.line as usize;
            let lines: Vec<&str> = text.lines().collect();
            if let Some(source_line) = lines.get(line) {
                let trimmed = source_line.trim_start();
                if trimmed.starts_with("@let ") {
                    let def_name = trimmed
                        .strip_prefix("@let ")
                        .and_then(|r| {
                            let r = r.trim();
                            r.find('[')
                                .map(|b| r[..b].trim())
                                .or_else(|| r.split_whitespace().next())
                        })
                        .unwrap_or("?");
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, 0),
                            Position::new(diag.range.start.line + 1, 0),
                        ),
                        new_text: String::new(),
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Remove unused attribute bundle '${}'", def_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }

        // Quick-fix: remove unused function (@let name ... with body)
        if msg.contains("unused function") {
            let line = diag.range.start.line as usize;
            let lines: Vec<&str> = text.lines().collect();
            if let Some(source_line) = lines.get(line) {
                let trimmed = source_line.trim_start();
                if trimmed.starts_with("@let ") {
                    let fn_name = trimmed
                        .strip_prefix("@let ")
                        .and_then(|r| r.split_whitespace().next())
                        .unwrap_or("?");
                    // Find the end of the function body (indented lines below)
                    let start_indent = source_line.len() - trimmed.len();
                    let mut end_line = line;
                    let mut j = line + 1;
                    while j < lines.len() {
                        let l = lines[j];
                        if l.trim().is_empty() {
                            j += 1;
                            continue;
                        }
                        let indent = l.len() - l.trim_start().len();
                        if indent <= start_indent {
                            break;
                        }
                        end_line = j;
                        j += 1;
                    }
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, 0),
                            Position::new(end_line as u32 + 1, 0),
                        ),
                        new_text: String::new(),
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Remove unused function '@{}'", fn_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }

        // Quick-fix: add alt attribute to @image
        if msg.contains("@image should have") && msg.contains("alt") {
            let line = diag.range.start.line as usize;
            let lines: Vec<&str> = text.lines().collect();
            if let Some(source_line) = lines.get(line) {
                if let Some(bracket_pos) = source_line.find('[') {
                    let insert_pos = bracket_pos + 1;
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, insert_pos as u32),
                            Position::new(diag.range.start.line, insert_pos as u32),
                        ),
                        new_text: "alt , ".into(),
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Add alt attribute".into(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                } else if source_line.contains("@image") {
                    // No brackets yet, add them
                    if let Some(img_pos) = source_line.find("@image") {
                        let after_image = img_pos + "@image".len();
                        let edit = TextEdit {
                            range: Range::new(
                                Position::new(diag.range.start.line, after_image as u32),
                                Position::new(diag.range.start.line, after_image as u32),
                            ),
                            new_text: " [alt ]".into(),
                        };
                        let mut changes = HashMap::new();
                        changes.insert(uri.clone(), vec![edit]);
                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Add alt attribute".into(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diag.clone()]),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }));
                    }
                }
            }
        }

        // Quick-fix: add missing type to @input
        if msg.contains("@input missing 'type'") {
            let line = diag.range.start.line as usize;
            let lines: Vec<&str> = text.lines().collect();
            if let Some(source_line) = lines.get(line) {
                if let Some(bracket_pos) = source_line.find('[') {
                    let insert_pos = bracket_pos + 1;
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, insert_pos as u32),
                            Position::new(diag.range.start.line, insert_pos as u32),
                        ),
                        new_text: "type text, ".into(),
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Add type=\"text\" attribute".into(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                } else if source_line.contains("@input")
                    && let Some(pos) = source_line.find("@input")
                {
                    let after = pos + "@input".len();
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(diag.range.start.line, after as u32),
                            Position::new(diag.range.start.line, after as u32),
                        ),
                        new_text: " [type text]".into(),
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Add type=\"text\" attribute".into(),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }

        // Quick-fix: low contrast ratio — suggest swapping to a high-contrast pair
        if msg.contains("low contrast ratio") {
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Acknowledged: low contrast ratio".into(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diag.clone()]),
                is_preferred: Some(false),
                ..Default::default()
            }));
        }

        // Quick-fix: auto-import suggestion for unknown element @name
        // Searches current directory and subdirectories for component definitions
        if msg.contains("unknown element @")
            && let Some(fn_name) = extract_between(msg, "unknown element @", ",")
                .or_else(|| extract_between(msg, "unknown element @", ""))
        {
            let fn_name = fn_name.trim();
            if !fn_name.is_empty()
                && let Ok(file_path) = uri.to_file_path()
                && let Some(dir) = file_path.parent()
            {
                // Search current dir and subdirs for .hl files defining this function
                let mut search_dirs = vec![dir.to_path_buf()];
                // Also search parent dir's subdirs (for project-wide imports)
                if let Some(parent) = dir.parent()
                    && let Ok(entries) = std::fs::read_dir(parent)
                {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() && p != dir {
                            search_dirs.push(p);
                        }
                    }
                }
                for search_dir in &search_dirs {
                    if let Ok(entries) = std::fs::read_dir(search_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().and_then(|e| e.to_str()) != Some("hl") {
                                continue;
                            }
                            if path == file_path {
                                continue;
                            }
                            if let Ok(content) = std::fs::read_to_string(&path) {
                                let defines_fn = content.lines().any(|l| {
                                    let t = l.trim();
                                    if let Some(rest) = t.strip_prefix("@let ") {
                                        rest.split_whitespace().next() == Some(fn_name)
                                    } else {
                                        false
                                    }
                                });
                                if defines_fn {
                                    // Compute relative path from current file's dir
                                    let rel = path
                                        .strip_prefix(dir)
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_else(|_| {
                                            path.file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("")
                                                .to_string()
                                        });
                                    let already_imported = text.lines().any(|l| {
                                        let t = l.trim();
                                        t == format!("@import {}", rel)
                                            || t == format!("@include {}", rel)
                                    });
                                    if !already_imported {
                                        // Offer @import (all definitions)
                                        let import_line = format!("@import {}\n", rel);
                                        let edit = TextEdit {
                                            range: Range::new(
                                                Position::new(0, 0),
                                                Position::new(0, 0),
                                            ),
                                            new_text: import_line,
                                        };
                                        let mut changes = HashMap::new();
                                        changes.insert(uri.clone(), vec![edit]);
                                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                            title: format!(
                                                "Add '@import {}' for @{}",
                                                rel, fn_name
                                            ),
                                            kind: Some(CodeActionKind::QUICKFIX),
                                            diagnostics: Some(vec![diag.clone()]),
                                            edit: Some(WorkspaceEdit {
                                                changes: Some(changes),
                                                ..Default::default()
                                            }),
                                            ..Default::default()
                                        }));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Refactoring: extract selection to @let component
    if selection.start.line != selection.end.line {
        let lines: Vec<&str> = text.lines().collect();
        let start_line = selection.start.line as usize;
        let end_line = (selection.end.line as usize).min(lines.len().saturating_sub(1));
        if start_line < lines.len() && end_line < lines.len() {
            // Collect selected lines
            let selected: Vec<&str> = lines[start_line..=end_line].to_vec();
            if !selected.is_empty() {
                // Determine the minimum indentation of selected lines (ignoring blank lines)
                let min_indent = selected
                    .iter()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.len() - l.trim_start().len())
                    .min()
                    .unwrap_or(0);

                // Build the function body with two-space indentation relative to @let
                let fn_body: String = selected
                    .iter()
                    .map(|l| {
                        if l.trim().is_empty() {
                            String::from("\n")
                        } else {
                            let stripped = if l.len() > min_indent {
                                &l[min_indent..]
                            } else {
                                l.trim_start()
                            };
                            format!("  {}\n", stripped)
                        }
                    })
                    .collect();

                let fn_def = format!("@let extracted\n{}", fn_body);
                let indent = " ".repeat(min_indent);
                let fn_call = format!("{}@extracted", indent);

                // Build edits: replace selected lines with @extracted call, and insert @let definition at top
                let replace_edit = TextEdit {
                    range: Range::new(
                        Position::new(selection.start.line, 0),
                        Position::new(selection.end.line + 1, 0),
                    ),
                    new_text: format!("{}\n", fn_call),
                };
                let insert_edit = TextEdit {
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                    new_text: format!("{}\n", fn_def),
                };
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), vec![insert_edit, replace_edit]);
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Extract to @let component".into(),
                    kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                    diagnostics: None,
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                }));
            }
        }
    }

    // Refactoring: extract attributes to @let attribute bundle
    // Works on a single line with [attrs] — extracts attrs into a @let
    {
        let lines: Vec<&str> = text.lines().collect();
        let line_idx = selection.start.line as usize;
        if line_idx < lines.len() {
            let line = lines[line_idx];
            if let Some(bracket_start) = line.find('[')
                && let Some(bracket_end) = line[bracket_start..].find(']')
            {
                let attrs_str = &line[bracket_start + 1..bracket_start + bracket_end];
                // Only offer if there are at least 2 attributes
                let attr_count = attrs_str.split(',').count();
                if attr_count >= 2 {
                    let define_name = "extracted-style";
                    let define_line = format!("@let {} [{}]\n", define_name, attrs_str.trim());
                    let indent = " ".repeat(line.len() - line.trim_start().len());

                    // Replace [attrs] with [$extracted-style]
                    let new_line = format!(
                        "{}{}[${}]{}",
                        indent,
                        &line.trim_start()[..line.trim_start().find('[').unwrap_or(0)],
                        define_name,
                        &line[bracket_start + bracket_end + 1..]
                    );

                    let replace_edit = TextEdit {
                        range: Range::new(
                            Position::new(line_idx as u32, 0),
                            Position::new(line_idx as u32, line.len() as u32),
                        ),
                        new_text: new_line,
                    };
                    let insert_edit = TextEdit {
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        new_text: define_line,
                    };
                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![insert_edit, replace_edit]);
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: "Extract to @let attribute bundle".into(),
                        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                        diagnostics: None,
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }
    }

    actions
}

fn extract_suggestion(msg: &str) -> Option<&str> {
    // "did you mean @X?" or "did you mean 'X'?"
    if let Some(idx) = msg.find("did you mean @") {
        let start = idx + "did you mean @".len();
        let rest = &msg[start..];
        let end = rest.find('?').unwrap_or(rest.len());
        return Some(&rest[..end]);
    }
    if let Some(idx) = msg.find("did you mean '") {
        let start = idx + "did you mean '".len();
        let rest = &msg[start..];
        let end = rest.find('\'')?;
        return Some(&rest[..end]);
    }
    None
}

fn extract_between<'a>(msg: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let start = msg.find(prefix)? + prefix.len();
    let rest = &msg[start..];
    if suffix.is_empty() {
        Some(rest.trim())
    } else {
        let end = rest.find(suffix)?;
        Some(&rest[..end])
    }
}

// ---------------------------------------------------------------------------
// Color provider
// ---------------------------------------------------------------------------

pub(crate) fn find_colors(text: &str) -> Vec<ColorInformation> {
    let mut colors = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let mut start = 0;
        // Detect hex colors (#fff, #ffffff, #ffffffff)
        while let Some(pos) = line[start..].find('#') {
            let abs_pos = start + pos;
            let hex_start = abs_pos + 1;
            let hex_end = line[hex_start..]
                .find(|c: char| !c.is_ascii_hexdigit())
                .map(|p| hex_start + p)
                .unwrap_or(line.len());
            let hex = &line[hex_start..hex_end];
            let len = hex.len();
            if (len == 3 || len == 6 || len == 8)
                && let Some((r, g, b, a)) = parse_hex_color(hex)
            {
                colors.push(ColorInformation {
                    range: Range::new(
                        Position::new(line_idx as u32, abs_pos as u32),
                        Position::new(line_idx as u32, hex_end as u32),
                    ),
                    color: Color {
                        red: r as f32 / 255.0,
                        green: g as f32 / 255.0,
                        blue: b as f32 / 255.0,
                        alpha: a as f32 / 255.0,
                    },
                });
            }
            start = hex_end;
        }
        // Detect named CSS colors
        for &(name, r, g, b) in NAMED_CSS_COLORS {
            let lower_line = line.to_lowercase();
            let mut search_start = 0;
            while let Some(pos) = lower_line[search_start..].find(name) {
                let abs = search_start + pos;
                let end = abs + name.len();
                // Ensure it's a word boundary (not inside another word)
                let before_ok = abs == 0 || !line.as_bytes()[abs - 1].is_ascii_alphanumeric();
                let after_ok = end >= line.len() || !line.as_bytes()[end].is_ascii_alphanumeric();
                if before_ok && after_ok {
                    colors.push(ColorInformation {
                        range: Range::new(
                            Position::new(line_idx as u32, abs as u32),
                            Position::new(line_idx as u32, end as u32),
                        ),
                        color: Color {
                            red: r as f32 / 255.0,
                            green: g as f32 / 255.0,
                            blue: b as f32 / 255.0,
                            alpha: 1.0,
                        },
                    });
                }
                search_start = end;
            }
        }
    }
    colors
}

/// Find a CSS named color whose RGB triple matches exactly. Used by the
/// color presentation handler to offer "red" alongside "#ff0000".
pub(crate) fn named_color_for(r: u8, g: u8, b: u8) -> Option<&'static str> {
    NAMED_CSS_COLORS
        .iter()
        .find(|&&(_, nr, ng, nb)| nr == r && ng == g && nb == b)
        .map(|&(name, _, _, _)| name)
}

/// Named CSS colors: (name, r, g, b)
const NAMED_CSS_COLORS: &[(&str, u8, u8, u8)] = &[
    ("red", 255, 0, 0),
    ("green", 0, 128, 0),
    ("blue", 0, 0, 255),
    ("white", 255, 255, 255),
    ("black", 0, 0, 0),
    ("orange", 255, 165, 0),
    ("yellow", 255, 255, 0),
    ("purple", 128, 0, 128),
    ("pink", 255, 192, 203),
    ("gray", 128, 128, 128),
    ("grey", 128, 128, 128),
    ("navy", 0, 0, 128),
    ("teal", 0, 128, 128),
    ("maroon", 128, 0, 0),
    ("aqua", 0, 255, 255),
    ("cyan", 0, 255, 255),
    ("fuchsia", 255, 0, 255),
    ("magenta", 255, 0, 255),
    ("lime", 0, 255, 0),
    ("olive", 128, 128, 0),
    ("silver", 192, 192, 192),
    ("coral", 255, 127, 80),
    ("salmon", 250, 128, 114),
    ("tomato", 255, 99, 71),
    ("gold", 255, 215, 0),
    ("khaki", 240, 230, 140),
    ("violet", 238, 130, 238),
    ("indigo", 75, 0, 130),
    ("crimson", 220, 20, 60),
    ("turquoise", 64, 224, 208),
    ("plum", 221, 160, 221),
    ("orchid", 218, 112, 214),
    ("sienna", 160, 82, 45),
    ("tan", 210, 180, 140),
    ("peru", 205, 133, 63),
    ("chocolate", 210, 105, 30),
    ("firebrick", 178, 34, 34),
    ("darkred", 139, 0, 0),
    ("darkgreen", 0, 100, 0),
    ("darkblue", 0, 0, 139),
    ("darkgray", 169, 169, 169),
    ("darkgrey", 169, 169, 169),
    ("lightgray", 211, 211, 211),
    ("lightgrey", 211, 211, 211),
    ("lightblue", 173, 216, 230),
    ("lightgreen", 144, 238, 144),
    ("lightyellow", 255, 255, 224),
    ("lightcoral", 240, 128, 128),
    ("lightpink", 255, 182, 193),
    ("lightsalmon", 255, 160, 122),
    ("steelblue", 70, 130, 180),
    ("royalblue", 65, 105, 225),
    ("dodgerblue", 30, 144, 255),
    ("deepskyblue", 0, 191, 255),
    ("cornflowerblue", 100, 149, 237),
    ("midnightblue", 25, 25, 112),
    ("slateblue", 106, 90, 205),
    ("mediumblue", 0, 0, 205),
    ("springgreen", 0, 255, 127),
    ("limegreen", 50, 205, 50),
    ("forestgreen", 34, 139, 34),
    ("seagreen", 46, 139, 87),
    ("darkslategray", 47, 79, 79),
    ("darkslategrey", 47, 79, 79),
    ("cadetblue", 95, 158, 160),
    ("mediumaquamarine", 102, 205, 170),
    ("darkorange", 255, 140, 0),
    ("orangered", 255, 69, 0),
    ("deeppink", 255, 20, 147),
    ("hotpink", 255, 105, 180),
    ("mediumvioletred", 199, 21, 133),
    ("palevioletred", 219, 112, 147),
    ("sandybrown", 244, 164, 96),
    ("goldenrod", 218, 165, 32),
    ("darkgoldenrod", 184, 134, 11),
    ("saddlebrown", 139, 69, 19),
    ("wheat", 245, 222, 179),
    ("beige", 245, 245, 220),
    ("linen", 250, 240, 230),
    ("ivory", 255, 255, 240),
    ("snow", 255, 250, 250),
    ("honeydew", 240, 255, 240),
    ("azure", 240, 255, 255),
    ("lavender", 230, 230, 250),
    ("mistyrose", 255, 228, 225),
    ("seashell", 255, 245, 238),
];

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8, u8)> {
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some((r, g, b, 255))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some((r, g, b, 255))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Folding ranges
// ---------------------------------------------------------------------------

pub(crate) fn folding_ranges(text: &str) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        // Fold blocks that start with @let (with body), @if, @else, @each, @match, @style, @head, @keyframes
        if trimmed.starts_with("@let ")
            || trimmed.starts_with("@if ")
            || trimmed == "@else"
            || trimmed.starts_with("@else if ")
            || trimmed.starts_with("@each ")
            || trimmed.starts_with("@match ")
            || trimmed == "@style"
            || trimmed == "@head"
            || trimmed.starts_with("@keyframes ")
        {
            let start_indent = lines[i].len() - lines[i].trim_start().len();
            let start_line = i;
            let mut end_line = i;
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    j += 1;
                    continue;
                }
                let indent = l.len() - l.trim_start().len();
                if indent <= start_indent {
                    break;
                }
                end_line = j;
                j += 1;
            }
            if end_line > start_line {
                ranges.push(FoldingRange {
                    start_line: start_line as u32,
                    start_character: None,
                    end_line: end_line as u32,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                });
            }
        }
        // Fold comment blocks (lines starting with --)
        if trimmed.starts_with("--") {
            let start_line = i;
            let mut end_line = i;
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim().starts_with("--") {
                end_line = j;
                j += 1;
            }
            if end_line > start_line {
                ranges.push(FoldingRange {
                    start_line: start_line as u32,
                    start_character: None,
                    end_line: end_line as u32,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Comment),
                    collapsed_text: None,
                });
            }
        }
        i += 1;
    }

    // Fold any multi-line `[...]` attribute list. We track bracket depth
    // across lines so nested brackets and brackets that close later in the
    // file are handled the same way.
    let mut stack: Vec<(u32, u32)> = Vec::new(); // (line, col) of each unmatched `[`
    for (line_idx, line) in lines.iter().enumerate() {
        for (col, ch) in line.char_indices() {
            match ch {
                '[' => stack.push((line_idx as u32, col as u32)),
                ']' => {
                    if let Some((start_line, start_col)) = stack.pop()
                        && start_line as usize != line_idx
                    {
                        ranges.push(FoldingRange {
                            start_line,
                            start_character: Some(start_col),
                            end_line: line_idx as u32,
                            end_character: Some(col as u32 + 1),
                            kind: Some(FoldingRangeKind::Region),
                            collapsed_text: None,
                        });
                    }
                }
                _ => {}
            }
        }
    }

    ranges
}

// ---------------------------------------------------------------------------
// Semantic tokens
// ---------------------------------------------------------------------------

pub(crate) fn semantic_tokens(text: &str, result: &ParseResult) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;

    // Build set of unused variables by parsing diagnostics
    let mut unused_vars: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in &result.diagnostics {
        if d.message.contains("unused variable '$")
            && let Some(start) = d.message.find("'$")
        {
            let rest = &d.message[start + 2..];
            if let Some(end) = rest.find('\'') {
                unused_vars.insert(rest[..end].to_string());
            }
        }
        if d.message.contains("unused function '@")
            && let Some(start) = d.message.find("'@")
        {
            let rest = &d.message[start + 2..];
            if let Some(end) = rest.find('\'') {
                unused_vars.insert(format!("@{}", &rest[..end]));
            }
        }
        if d.message.contains("unused define '$")
            && let Some(start) = d.message.find("'$")
        {
            let rest = &d.message[start + 2..];
            if let Some(end) = rest.find('\'') {
                unused_vars.insert(rest[..end].to_string());
            }
        }
        if d.message.contains("unused mixin '")
            && let Some(start) = d.message.find("unused mixin '")
        {
            let rest = &d.message[start + 14..];
            if let Some(end) = rest.find('\'') {
                unused_vars.insert(rest[..end].to_string());
            }
        }
    }

    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let line_num = line_idx as u32;

        // Detect comments
        if trimmed.starts_with("--") {
            let col = (line.len() - trimmed.len()) as u32;
            push_token(
                &mut tokens,
                &mut prev_line,
                &mut prev_start,
                line_num,
                col,
                trimmed.len() as u32,
                4,
                0,
            );
            continue;
        }

        // Scan for @keywords
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'@' {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
                {
                    i += 1;
                }
                let word = &line[start..i];
                // Directives and built-in elements are keywords; any other
                // `@name` is a function call.
                let token_type = if is_builtin_name(&word[1..]) { 0 } else { 2 };
                // Mark unused definitions with deprecated modifier (dimmed)
                let modifier = if trimmed.starts_with("@let ") && word != "@let" {
                    let name_part = &word[1..]; // strip @
                    if unused_vars.contains(&format!("@{}", name_part)) {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                };
                push_token(
                    &mut tokens,
                    &mut prev_line,
                    &mut prev_start,
                    line_num,
                    start as u32,
                    (i - start) as u32,
                    token_type,
                    modifier,
                );
            } else if bytes[i] == b'$' {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
                {
                    i += 1;
                }
                if i > start + 1 {
                    // Check if this is an unused variable definition on a @let line
                    let var_name = &line[start + 1..i];
                    let modifier = if trimmed.starts_with("@let ") && unused_vars.contains(var_name)
                    {
                        1
                    } else {
                        0
                    };
                    push_token(
                        &mut tokens,
                        &mut prev_line,
                        &mut prev_start,
                        line_num,
                        start as u32,
                        (i - start) as u32,
                        1,
                        modifier,
                    ); // variable
                }
            } else {
                i += 1;
            }
        }
    }
    tokens
}

/// A directive or built-in element name (without `@`), from the compiler.
fn is_builtin_name(name: &str) -> bool {
    htmlang::parser::known_directives().contains(&name)
        || htmlang::ast::ElementKind::from_name(name).is_some()
        // Directives the parser handles outside its directive list
        || name == "data"
}

#[allow(clippy::too_many_arguments)]
fn push_token(
    tokens: &mut Vec<SemanticToken>,
    prev_line: &mut u32,
    prev_start: &mut u32,
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    token_modifiers_bitset: u32,
) {
    let delta_line = line - *prev_line;
    let delta_start = if delta_line == 0 {
        start - *prev_start
    } else {
        start
    };
    tokens.push(SemanticToken {
        delta_line,
        delta_start,
        length,
        token_type,
        token_modifiers_bitset,
    });
    *prev_line = line;
    *prev_start = start;
}

// ---------------------------------------------------------------------------
// Inlay hints
// ---------------------------------------------------------------------------

pub(crate) fn inlay_hints(text: &str) -> Vec<InlayHint> {
    // Build variable map: name -> value
    let mut vars: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ")
            && let Some((name, value)) = rest.trim().split_once(' ')
        {
            vars.insert(name, value.trim());
        }
    }

    let mut hints = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        // Skip @let definition lines — the value is already visible there
        if trimmed.starts_with("@let ") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'$' {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
                {
                    i += 1;
                }
                if i > start + 1 {
                    let var_name = &line[start + 1..i];
                    if let Some(value) = vars.get(var_name) {
                        hints.push(InlayHint {
                            position: Position::new(line_idx as u32, i as u32),
                            label: InlayHintLabel::String(format!(" \u{2192} {}", value)),
                            kind: None,
                            text_edits: None,
                            tooltip: None,
                            padding_left: Some(false),
                            padding_right: Some(true),
                            data: None,
                        });
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    hints
}

// ---------------------------------------------------------------------------
// Signature help
// ---------------------------------------------------------------------------

pub(crate) fn get_signature_help(text: &str, position: Position) -> Option<SignatureHelp> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let before = &line[..col];

    // Check if we're inside a function call: @funcname [...
    let trimmed = before.trim_start();
    if !trimmed.starts_with('@') {
        return None;
    }

    // Extract the function name
    let after_at = &trimmed[1..];
    let name_end = after_at
        .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .unwrap_or(after_at.len());
    let fn_name = &after_at[..name_end];

    // Prefer to surface signature help inside an argument list, but don't hide
    // the signature from callers who trigger explicitly (e.g. hover over the
    // function name itself). We still need the cursor to be on or after the
    // `@name` token — `fn_name` being non-empty is the check for that.
    if fn_name.is_empty() {
        return None;
    }
    let inside_args = in_brackets(before);

    // Find the @let component definition
    for (line_idx, line_text) in text.lines().enumerate() {
        let t = line_text.trim_start();
        if let Some(rest) = t.strip_prefix("@let ") {
            let rest = rest.trim();
            let def_name_end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
                .unwrap_or(rest.len());
            let def_name = &rest[..def_name_end];
            if def_name != fn_name {
                continue;
            }
            let params_str = &rest[def_name_end..].trim();
            let params: Vec<&str> = params_str
                .split_whitespace()
                .filter(|p| p.starts_with('$'))
                .collect();

            if params.is_empty() {
                return None;
            }

            let param_labels: Vec<ParameterInformation> = params
                .iter()
                .map(|p| {
                    let name = p.trim_start_matches('$');
                    let (label, doc) = if let Some((n, default)) = name.split_once('=') {
                        (n.to_string(), Some(format!("Default: {}", default)))
                    } else {
                        (name.to_string(), None)
                    };
                    ParameterInformation {
                        label: ParameterLabel::Simple(label),
                        documentation: doc.map(Documentation::String),
                    }
                })
                .collect();

            let sig_label = format!("@{} {}", fn_name, params.join(" "));

            // Determine active parameter by counting commas before cursor inside
            // brackets. When the cursor hasn't entered the argument list yet,
            // highlight the first parameter.
            let active_param = if inside_args {
                let bracket_start = before.rfind('[').unwrap_or(0);
                let inside = &before[bracket_start..];
                inside.matches(',').count() as u32
            } else {
                0
            };

            return Some(SignatureHelp {
                signatures: vec![SignatureInformation {
                    label: sig_label,
                    documentation: Some(Documentation::String(format!(
                        "Defined at line {}",
                        line_idx + 1
                    ))),
                    parameters: Some(param_labels),
                    active_parameter: Some(active_param),
                }],
                active_signature: Some(0),
                active_parameter: Some(active_param),
            });
        }
    }
    None
}
