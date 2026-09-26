use std::collections::{HashMap, HashSet};

use htmlang::diagnostic::code;
use htmlang::parser::ParseResult;
use htmlang::syntax::{self, DefinitionKind, NodeKind, Tree};
use tower_lsp::lsp_types::*;

use crate::completion::in_brackets;
use crate::tree::{self as lsp_tree, Def};

// ---------------------------------------------------------------------------
// Document symbols (outline view)
// ---------------------------------------------------------------------------

#[allow(deprecated)] // SymbolInformation::deprecated is deprecated but needed for the struct
pub(crate) fn document_symbols(text: &str) -> Vec<SymbolInformation> {
    let lines: Vec<&str> = text.lines().collect();
    let whole_line = |line: u32| {
        let len = lines.get(line as usize).map_or(0, |l| l.len()) as u32;
        Range::new(Position::new(line, 0), Position::new(line, len))
    };
    let symbol = |name: String, kind, line, detail| SymbolInformation {
        name,
        kind,
        tags: None,
        deprecated: None,
        location: Location {
            uri: Url::parse("file:///").unwrap(), // replaced by caller
            range: whole_line(line),
        },
        container_name: detail,
    };

    let mut symbols = Vec::new();
    // @let definitions (variables, attribute bundles, and functions)
    for def in lsp_tree::definitions(text) {
        match def.kind {
            DefinitionKind::Function => {
                let params: Vec<String> = def.params.iter().map(param_label).collect();
                let detail = (!params.is_empty()).then(|| format!("({})", params.join(" ")));
                symbols.push(symbol(
                    format!("@{}", def.name),
                    SymbolKind::FUNCTION,
                    def.line,
                    detail,
                ));
            }
            DefinitionKind::Bundle => symbols.push(symbol(
                format!("${}", def.name),
                SymbolKind::CONSTANT,
                def.line,
                Some("attribute bundle".to_string()),
            )),
            DefinitionKind::Value => {
                if let Some(value) = def.value.filter(|v| !v.is_empty()) {
                    symbols.push(symbol(
                        format!("${}", def.name),
                        SymbolKind::VARIABLE,
                        def.line,
                        Some(format!("= {}", value.trim_start_matches("= "))),
                    ));
                }
            }
        }
    }

    // @keyframes rules, in @style bodies
    let tree = syntax::parse(text);
    let verbatim = lsp_tree::verbatim_lines(&tree);
    for (i, line) in lines.iter().enumerate() {
        if !verbatim.contains(&(i as u32)) {
            continue;
        }
        if let Some(rest) = line.trim().strip_prefix("@keyframes ") {
            let name = rest.split(['{', ' ']).next().unwrap_or("");
            if !name.is_empty() {
                symbols.push(symbol(
                    format!("@keyframes {}", name),
                    SymbolKind::EVENT,
                    i as u32,
                    Some("animation".to_string()),
                ));
            }
        }
    }
    symbols.sort_by_key(|s| s.location.range.start.line);
    symbols
}

/// A parameter as written: `$tone` or `$tone=info`.
fn param_label(param: &lsp_tree::Param) -> String {
    match &param.default {
        Some(default) => format!("${}={}", param.name, default),
        None => format!("${}", param.name),
    }
}

// ---------------------------------------------------------------------------
// Code actions: quick fixes keyed on diagnostic codes, and refactorings
// ---------------------------------------------------------------------------

/// The compiler's code for an LSP diagnostic.
fn diagnostic_code(diag: &Diagnostic) -> Option<&str> {
    match &diag.code {
        Some(NumberOrString::String(code)) => Some(code),
        _ => None,
    }
}

/// A field of the diagnostic's data: `subject` (what it is about) or
/// `suggestion` (what to write instead).
fn diagnostic_field<'a>(diag: &'a Diagnostic, key: &str) -> Option<&'a str> {
    diag.data.as_ref()?.get(key)?.as_str()
}

fn quick_fix(
    title: String,
    diag: &Diagnostic,
    uri: &Url,
    edits: Vec<TextEdit>,
) -> CodeActionOrCommand {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn insert(line: u32, column: usize, text: &str) -> TextEdit {
    let at = Position::new(line, column as u32);
    TextEdit {
        range: Range::new(at, at),
        new_text: text.to_string(),
    }
}

/// Where `word` appears in `line` as a whole name.
fn find_word(line: &str, word: &str) -> Option<usize> {
    let is_name = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
    line.match_indices(word).map(|(at, _)| at).find(|&at| {
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        // A `@name` or `$name` starts with its sigil; a bare word must not
        // be the tail of a longer name.
        let starts = word.starts_with(['@', '$']) || !before.is_some_and(is_name);
        starts && !after.is_some_and(is_name)
    })
}

pub(crate) fn code_actions(
    text: &str,
    tree: &Tree,
    selection: &Range,
    diagnostics: &[Diagnostic],
    uri: &Url,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let defs = lsp_tree::definitions(text);

    for diag in diagnostics {
        let Some(code) = diagnostic_code(diag) else {
            continue;
        };
        let line = diag.range.start.line;
        let source_line = lines.get(line as usize).copied().unwrap_or("");
        let subject = diagnostic_field(diag, "subject");
        let suggestion = diagnostic_field(diag, "suggestion");

        match code {
            // Replace a misspelled name with the closest known one.
            code::UNKNOWN_ELEMENT
            | code::UNKNOWN_ATTRIBUTE
            | code::UNKNOWN_COLOR
            | code::UNDEFINED_VARIABLE => {
                let (Some(subject), Some(suggestion)) = (subject, suggestion) else {
                    continue;
                };
                let sigil = match code {
                    code::UNKNOWN_ELEMENT => "@",
                    code::UNDEFINED_VARIABLE => "$",
                    _ => "",
                };
                let old = format!("{}{}", sigil, subject);
                let new = format!("{}{}", sigil, suggestion);
                if let Some(col) = find_word(source_line, &old) {
                    let edit = TextEdit {
                        range: Range::new(
                            Position::new(line, col as u32),
                            Position::new(line, (col + old.len()) as u32),
                        ),
                        new_text: new.clone(),
                    };
                    actions.push(quick_fix(
                        format!("Replace with '{}'", new),
                        diag,
                        uri,
                        vec![edit],
                    ));
                }
            }

            // Remove a definition nothing uses, with its body.
            code::UNUSED_VARIABLE | code::UNUSED_BUNDLE | code::UNUSED_FUNCTION => {
                let Some(def) = defs
                    .iter()
                    .find(|d| d.line == line && subject.is_none_or(|s| s == d.name))
                else {
                    continue;
                };
                let (what, shown) = match def.kind {
                    DefinitionKind::Value => ("variable", format!("${}", def.name)),
                    DefinitionKind::Bundle => ("attribute bundle", format!("${}", def.name)),
                    DefinitionKind::Function => ("function", format!("@{}", def.name)),
                };
                let edit = TextEdit {
                    range: Range::new(
                        Position::new(def.line, 0),
                        Position::new(def.end_line + 1, 0),
                    ),
                    new_text: String::new(),
                };
                actions.push(quick_fix(
                    format!("Remove unused {} '{}'", what, shown),
                    diag,
                    uri,
                    vec![edit],
                ));
            }

            // Add a missing attribute to the element the diagnostic is on.
            code::MISSING_ALT | code::MISSING_INPUT_TYPE => {
                let (element, attr, title) = if code == code::MISSING_ALT {
                    ("image", "alt", "Add alt attribute")
                } else {
                    ("input", "type text", "Add type=\"text\" attribute")
                };
                let Some(node) = lsp_tree::node_at(tree, line) else {
                    continue;
                };
                let Some(head) = node.heads().into_iter().find(|h| h.name == element) else {
                    continue;
                };
                let edit = match &head.attrs {
                    Some(list) => {
                        let at = lsp_tree::range(list.span).start;
                        let sep = if list.attrs.is_empty() { "" } else { ", " };
                        insert(
                            at.line,
                            at.character as usize + 1,
                            &format!("{}{}", attr, sep),
                        )
                    }
                    None => {
                        let end = lsp_tree::range(head.name_span).end;
                        insert(end.line, end.character as usize, &format!(" [{}]", attr))
                    }
                };
                actions.push(quick_fix(title.to_string(), diag, uri, vec![edit]));
            }

            code::LOW_CONTRAST => {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Acknowledged: low contrast ratio".into(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    is_preferred: Some(false),
                    ..Default::default()
                }));
            }

            _ => {}
        }

        // An unknown element may be a function defined in a nearby file:
        // offer to include it.
        if code == code::UNKNOWN_ELEMENT
            && let Some(name) = subject
        {
            actions.extend(auto_imports(name, tree, diag, uri));
        }
    }

    actions.extend(extract_component(&lines, selection, uri));
    actions.extend(extract_bundle(tree, selection, uri));
    actions
}

/// `@include` fixes for a function `name` defined in a `.hl` file in this
/// directory or a sibling directory.
fn auto_imports(name: &str, tree: &Tree, diag: &Diagnostic, uri: &Url) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();
    let Ok(file_path) = uri.to_file_path() else {
        return actions;
    };
    let Some(dir) = file_path.parent() else {
        return actions;
    };
    let mut search_dirs = vec![dir.to_path_buf()];
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
    let mut included = HashSet::new();
    tree.walk(&mut |node| {
        if let Some((path, _)) = lsp_tree::file_argument(node)
            && node.is_directive("include")
        {
            included.insert(path);
        }
    });
    for search_dir in &search_dirs {
        let Ok(entries) = std::fs::read_dir(search_dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("hl") || path == file_path {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let defines = syntax::parse(&content)
                .definitions()
                .iter()
                .any(|d| d.kind == DefinitionKind::Function && d.name == name);
            if !defines {
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| {
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string()
                });
            if included.contains(&rel) {
                continue;
            }
            let edit = insert(0, 0, &format!("@include {}\n", rel));
            actions.push(quick_fix(
                format!("Add '@include {}' for @{}", rel, name),
                diag,
                uri,
                vec![edit],
            ));
        }
    }
    actions
}

/// Refactoring: extract the selected lines into a `@let` function.
fn extract_component(lines: &[&str], selection: &Range, uri: &Url) -> Option<CodeActionOrCommand> {
    if selection.start.line == selection.end.line {
        return None;
    }
    let start_line = selection.start.line as usize;
    let end_line = (selection.end.line as usize).min(lines.len().saturating_sub(1));
    if start_line >= lines.len() || start_line > end_line {
        return None;
    }
    let selected = &lines[start_line..=end_line];
    // The smallest indentation of the selected lines, blank lines aside
    let min_indent = selected
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    // The body, indented two spaces under the @let
    let fn_body: String = selected
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::from("\n")
            } else {
                let stripped = l.get(min_indent..).unwrap_or_else(|| l.trim_start());
                format!("  {}\n", stripped)
            }
        })
        .collect();

    let fn_def = format!("@let extracted\n{}", fn_body);
    let fn_call = format!("{}@extracted", " ".repeat(min_indent));
    let replace_edit = TextEdit {
        range: Range::new(
            Position::new(selection.start.line, 0),
            Position::new(selection.end.line + 1, 0),
        ),
        new_text: format!("{}\n", fn_call),
    };
    let insert_edit = insert(0, 0, &format!("{}\n", fn_def));
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![insert_edit, replace_edit]);
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Extract to @let component".into(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }))
}

/// Refactoring: move an element's attribute list (two attributes or more)
/// into a `@let` attribute bundle.
fn extract_bundle(tree: &Tree, selection: &Range, uri: &Url) -> Option<CodeActionOrCommand> {
    let node = lsp_tree::node_at(tree, selection.start.line)?;
    // Only a line on its own: the list's span is then a plain range.
    if node.line_count > 1 {
        return None;
    }
    let list = node.heads().into_iter().find_map(|head| {
        head.attrs
            .as_ref()
            .filter(|list| list.closed && list.attrs.len() >= 2)
            .filter(|list| list.span.line == selection.start.line as usize + 1)
    })?;
    let name = "extracted-style";
    let attrs: Vec<&str> = list.attrs.iter().map(|a| a.raw.as_str()).collect();
    let define_line = format!("@let {} [{}]\n", name, attrs.join(", "));
    let replace_edit = TextEdit {
        range: lsp_tree::range(list.span),
        new_text: format!("[${}]", name),
    };
    let insert_edit = insert(0, 0, &define_line);
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![insert_edit, replace_edit]);
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Extract to @let attribute bundle".into(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }))
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

pub(crate) fn folding_ranges(tree: &Tree) -> Vec<FoldingRange> {
    let fold = |start: usize, end: usize, kind| FoldingRange {
        start_line: start.saturating_sub(1) as u32,
        start_character: None,
        end_line: end.saturating_sub(1) as u32,
        end_character: None,
        kind: Some(kind),
        collapsed_text: None,
    };
    let mut ranges = Vec::new();
    let mut comment_run: Option<(usize, usize)> = None;
    let mut comments = Vec::new();
    tree.walk(&mut |node| {
        match node.kind {
            NodeKind::Comment => comments.push(node.span.line),
            // A verbatim body folds with the line that opens it.
            NodeKind::Blank | NodeKind::Verbatim(_) => {}
            _ => {
                let end = node.end_line();
                if end > node.span.line {
                    // A block with a body, or a header over several lines
                    ranges.push(fold(node.span.line, end, FoldingRangeKind::Region));
                }
            }
        }
    });
    // Runs of consecutive comment lines
    comments.sort_unstable();
    for line in comments {
        comment_run = match comment_run {
            Some((start, end)) if line == end + 1 => Some((start, line)),
            Some((start, end)) => {
                if end > start {
                    ranges.push(fold(start, end, FoldingRangeKind::Comment));
                }
                Some((line, line))
            }
            None => Some((line, line)),
        };
    }
    if let Some((start, end)) = comment_run
        && end > start
    {
        ranges.push(fold(start, end, FoldingRangeKind::Comment));
    }
    ranges
}

// ---------------------------------------------------------------------------
// Semantic tokens
// ---------------------------------------------------------------------------

const TOKEN_KEYWORD: u32 = 0;
const TOKEN_VARIABLE: u32 = 1;
const TOKEN_FUNCTION: u32 = 2;
const TOKEN_COMMENT: u32 = 4;
const MODIFIER_UNUSED: u32 = 1;

pub(crate) fn semantic_tokens(text: &str, tree: &Tree, result: &ParseResult) -> Vec<SemanticToken> {
    // Definitions the compiler reported unused: `$name` or `@name`
    let unused: HashSet<String> = result
        .diagnostics
        .iter()
        .filter_map(|d| {
            let sigil = match d.code {
                code::UNUSED_VARIABLE | code::UNUSED_BUNDLE => "$",
                code::UNUSED_FUNCTION => "@",
                _ => return None,
            };
            Some(format!("{}{}", sigil, d.subject.as_deref()?))
        })
        .collect();

    // (line, column, length, type, modifiers), from the tree
    let mut found: Vec<(u32, u32, u32, u32, u32)> = Vec::new();
    let mut push = |span: syntax::Span, token: u32, modifier: u32| {
        let r = lsp_tree::range(span);
        found.push((
            r.start.line,
            r.start.character,
            r.end.character - r.start.character,
            token,
            modifier,
        ));
    };
    let mut comments: HashSet<u32> = HashSet::new();
    tree.walk(&mut |node| {
        match &node.kind {
            NodeKind::Comment => {
                comments.insert(node.span.line.saturating_sub(1) as u32);
            }
            NodeKind::Directive(directive) => {
                push(directive.name_span, TOKEN_KEYWORD, 0);
                if let syntax::DirectiveArgs::Let(def) = &directive.args {
                    let function = matches!(def.form, syntax::LetForm::Function(_));
                    let (token, key) = if function {
                        (TOKEN_FUNCTION, format!("@{}", def.name))
                    } else {
                        (TOKEN_VARIABLE, format!("${}", def.name))
                    };
                    let modifier = if unused.contains(&key) {
                        MODIFIER_UNUSED
                    } else {
                        0
                    };
                    push(def.name_span, token, modifier);
                }
            }
            _ => {}
        }
        for head in node.heads() {
            // Built-in elements are keywords; any other `@name` is a call.
            let token = if is_builtin_name(&head.name) {
                TOKEN_KEYWORD
            } else {
                TOKEN_FUNCTION
            };
            push(head.name_span, token, 0);
        }
    });

    // `$name` references, anywhere but in comments and verbatim bodies
    let verbatim = lsp_tree::verbatim_lines(tree);
    for (line_idx, line) in text.lines().enumerate() {
        let line_num = line_idx as u32;
        if comments.contains(&line_num) {
            let trimmed = line.trim();
            let col = (line.len() - line.trim_start().len()) as u32;
            found.push((line_num, col, trimmed.len() as u32, TOKEN_COMMENT, 0));
            continue;
        }
        if verbatim.contains(&line_num) {
            continue;
        }
        for (start, end) in variable_refs(line) {
            found.push((
                line_num,
                start as u32,
                (end - start) as u32,
                TOKEN_VARIABLE,
                0,
            ));
        }
    }

    found.sort_by_key(|t| (t.0, t.1));
    found.dedup_by_key(|t| (t.0, t.1));
    let mut tokens = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;
    let mut prev_end: Option<(u32, u32)> = None;
    for (line, start, length, token_type, modifiers) in found {
        // Overlapping tokens aren't allowed
        if prev_end.is_some_and(|(l, end)| l == line && start < end) {
            continue;
        }
        push_token(
            &mut tokens,
            &mut prev_line,
            &mut prev_start,
            line,
            start,
            length,
            token_type,
            modifiers,
        );
        prev_end = Some((line, start + length));
    }
    tokens
}

/// A directive or built-in element name (without `@`), from the compiler.
fn is_builtin_name(name: &str) -> bool {
    htmlang::ast::directive(name).is_some() || htmlang::ast::ElementKind::from_name(name).is_some()
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

pub(crate) fn inlay_hints(text: &str, tree: &Tree) -> Vec<InlayHint> {
    // Values and bundles by name, and the lines that define them
    let mut values: HashMap<String, String> = HashMap::new();
    let mut skip: HashSet<u32> = lsp_tree::verbatim_lines(tree);
    for def in lsp_tree::definitions(text) {
        skip.insert(def.line);
        match def.kind {
            DefinitionKind::Value => {
                if let Some(value) = def.value {
                    values.insert(def.name, value);
                }
            }
            DefinitionKind::Bundle => {
                values.insert(def.name, format!("[{}]", def.value.unwrap_or_default()));
            }
            DefinitionKind::Function => {}
        }
    }
    tree.walk(&mut |node| {
        if matches!(node.kind, NodeKind::Comment) {
            skip.insert(node.span.line.saturating_sub(1) as u32);
        }
    });

    let mut hints = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        if skip.contains(&(line_idx as u32)) {
            continue;
        }
        for (start, end) in variable_refs(line) {
            if let Some(value) = values.get(&line[start + 1..end]) {
                hints.push(InlayHint {
                    position: Position::new(line_idx as u32, end as u32),
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
    }
    hints
}

/// The `$name` references on a line, as byte ranges that include the `$`.
/// A name ends as the compiler ends it; `$5` is text.
fn variable_refs(line: &str) -> Vec<(usize, usize)> {
    let mut refs = Vec::new();
    let mut from = 0;
    while let Some(i) = line[from..].find('$') {
        let start = from + i;
        let len = htmlang::interp::name_len(&line[start + 1..]);
        if len > 0 {
            refs.push((start, start + 1 + len));
        }
        from = start + 1 + len;
    }
    refs
}

// ---------------------------------------------------------------------------
// Signature help
// ---------------------------------------------------------------------------

pub(crate) fn get_signature_help(text: &str, position: Position) -> Option<SignatureHelp> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let before = line.get(..col)?;

    // Check if we're inside a function call: @funcname [...
    let trimmed = before.trim_start();
    let after_at = trimmed.strip_prefix('@')?;
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

    let def: Def = lsp_tree::definitions(text)
        .into_iter()
        .find(|d| d.kind == DefinitionKind::Function && d.name == fn_name)?;
    if def.params.is_empty() {
        return None;
    }
    let param_labels: Vec<ParameterInformation> = def
        .params
        .iter()
        .map(|p| ParameterInformation {
            label: ParameterLabel::Simple(p.name.clone()),
            documentation: p
                .default
                .as_ref()
                .map(|d| Documentation::String(format!("Default: {}", d))),
        })
        .collect();
    let written: Vec<String> = def.params.iter().map(param_label).collect();
    let sig_label = format!("@{} {}", fn_name, written.join(" "));

    // The active parameter: commas before the cursor inside the brackets,
    // or the first parameter before the argument list is entered.
    let active_param = if inside_args {
        let bracket_start = before.rfind('[').unwrap_or(0);
        before[bracket_start..].matches(',').count() as u32
    } else {
        0
    };

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: sig_label,
            documentation: Some(Documentation::String(format!(
                "Defined at line {}",
                def.line + 1
            ))),
            parameters: Some(param_labels),
            active_parameter: Some(active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lsp_diagnostics(text: &str) -> Vec<Diagnostic> {
        let result = htmlang::parser::parse(text);
        result
            .diagnostics
            .iter()
            .map(|d| Diagnostic {
                range: Range::new(
                    Position::new(d.line.saturating_sub(1) as u32, 0),
                    Position::new(d.line.saturating_sub(1) as u32, 1000),
                ),
                code: Some(NumberOrString::String(d.code.into())),
                message: d.message.clone(),
                data: Some(serde_json::json!({ "subject": d.subject, "suggestion": d.suggestion })),
                ..Default::default()
            })
            .collect()
    }

    fn fixes(text: &str) -> Vec<(String, Vec<TextEdit>)> {
        let uri = Url::parse("file:///tmp/none/page.hl").unwrap();
        let tree = syntax::parse(text);
        let point = Range::new(Position::new(0, 0), Position::new(0, 0));
        code_actions(text, &tree, &point, &lsp_diagnostics(text), &uri)
            .into_iter()
            .filter_map(|action| match action {
                CodeActionOrCommand::CodeAction(action) => {
                    let edits = action.edit?.changes?.remove(&uri)?;
                    Some((action.title, edits))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn quick_fixes_are_keyed_on_codes() {
        let found = fixes("@el\n  @paragrap Hi\n");
        let (title, edits) = found
            .iter()
            .find(|(title, _)| title.starts_with("Replace"))
            .expect("replace fix");
        assert_eq!(title, "Replace with '@paragraph'");
        assert_eq!(edits[0].range.start, Position::new(1, 2));

        let found = fixes("@el [paddin 4]\n");
        assert!(
            found.iter().any(|(t, _)| t == "Replace with 'padding'"),
            "{:?}",
            found
        );

        let found = fixes("@let card $x\n  @el $x\n\n  @el more\n@el\n");
        let (title, edits) = found
            .iter()
            .find(|(t, _)| t.starts_with("Remove"))
            .expect("remove fix");
        assert_eq!(title, "Remove unused function '@card'");
        assert_eq!(
            edits[0].range,
            Range::new(Position::new(0, 0), Position::new(4, 0))
        );

        let found = fixes("@image cat.png\n");
        let (_, edits) = found
            .iter()
            .find(|(t, _)| t == "Add alt attribute")
            .expect("alt fix");
        assert_eq!(edits[0].new_text, " [alt]");
        assert_eq!(edits[0].range.start, Position::new(0, 6));
    }

    #[test]
    fn folding_follows_the_tree() {
        let text = "-- a\n-- b\n@el\n  @text x\n\n  @text y\n@style\n  .a {\n  }\n@text z\n";
        let ranges = folding_ranges(&syntax::parse(text));
        let spans: Vec<(u32, u32)> = ranges.iter().map(|r| (r.start_line, r.end_line)).collect();
        assert!(spans.contains(&(0, 1)), "{:?}", spans);
        assert!(spans.contains(&(2, 5)), "{:?}", spans);
        assert!(spans.contains(&(6, 8)), "{:?}", spans);
        assert_eq!(spans.len(), 3, "{:?}", spans);
    }

    #[test]
    fn semantic_tokens_skip_verbatim_bodies() {
        let text = "@let gap 4\n@style\n  @media (x) { a { b: $c } }\n@el [padding $gap]\n@card\n";
        let tokens = semantic_tokens(text, &syntax::parse(text), &htmlang::parser::parse(text));
        let mut line = 0;
        let mut lines = Vec::new();
        for t in &tokens {
            line += t.delta_line;
            lines.push(line);
        }
        assert!(!lines.contains(&2), "{:?}", tokens);
        // `@let`, `gap`, `@style`, `@el`, `$gap`, `@card` (a call)
        assert_eq!(tokens.len(), 6, "{:?}", tokens);
        assert_eq!(tokens[5].token_type, TOKEN_FUNCTION);
    }

    #[test]
    fn variable_tokens_end_where_the_compiler_ends_names() {
        assert_eq!(
            variable_refs("$lang.json costs $5, ${x} $--brand $a- b"),
            [(0, 5), (26, 34), (35, 37)]
        );
        // A typo'd variable gets the compiler's suggestion as a fix
        let found = fixes("@let gap 8\n@el [padding $gpa]\n");
        assert!(
            found.iter().any(|(t, _)| t == "Replace with '$gap'"),
            "{:?}",
            found
        );
    }

    #[test]
    fn signature_help_and_inlay_hints_use_definitions() {
        let text =
            "@let pad [padding 4]\n@let card $title $tone=info\n  @el [$pad] $title\n@card [\n";
        let help = get_signature_help(text, Position::new(3, 7)).expect("signature");
        assert_eq!(help.signatures[0].label, "@card $title $tone=info");
        let hints = inlay_hints(text, &syntax::parse(text));
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].position, Position::new(2, 11));
    }
}
