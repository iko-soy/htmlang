use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use htmlang::syntax::DefinitionKind;

use crate::hover::{is_word_byte, word_at};
use crate::state::WorkspaceIndex;
use crate::tree;

// ---------------------------------------------------------------------------
// Go to definition
// ---------------------------------------------------------------------------

pub(crate) fn definition_at(
    text: &str,
    position: Position,
    uri: &Url,
) -> Option<GotoDefinitionResponse> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;

    // The file named by `@include`, `@markdown` or `@data`
    let tree = htmlang::syntax::parse(text);
    if let Some(node) = tree::node_at(&tree, position.line)
        && let Some((filename, _)) = tree::file_argument(node)
    {
        let file_path = uri.to_file_path().ok()?;
        let target = file_path.parent()?.join(filename);
        if target.exists() {
            let target_uri = Url::from_file_path(&target).ok()?;
            return Some(GotoDefinitionResponse::Scalar(Location {
                uri: target_uri,
                range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            }));
        }
    }

    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;
    let range = if let Some(name) = word.strip_prefix('$') {
        find_definition(text, name)?
    } else {
        find_fn_definition(text, word.strip_prefix('@')?)?
    };
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range,
    }))
}

/// Where the `$name` a reference uses is defined: a value, an attribute
/// bundle, or a function's parameter.
pub(crate) fn find_definition(text: &str, name: &str) -> Option<Range> {
    let defs = tree::definitions(text);
    defs.iter()
        .find(|d| d.kind != DefinitionKind::Function && d.name == name)
        .map(|d| d.name_range)
        .or_else(|| {
            defs.iter()
                .flat_map(|d| &d.params)
                .find(|p| p.name == name)
                .map(|p| p.name_range)
        })
}

/// Where the function an `@name` call uses is defined.
pub(crate) fn find_fn_definition(text: &str, name: &str) -> Option<Range> {
    tree::definitions(text)
        .into_iter()
        .find(|d| d.kind == DefinitionKind::Function && d.name == name)
        .map(|d| d.name_range)
}

// ---------------------------------------------------------------------------
// Rename
// ---------------------------------------------------------------------------

pub(crate) fn prepare_rename_at(text: &str, position: Position) -> Option<PrepareRenameResponse> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    // Only allow renaming $variables and @function calls/definitions
    if !word.starts_with('$') && !word.starts_with('@') {
        return None;
    }

    let name = &word[1..];

    // Check that the symbol actually has a definition
    if word.starts_with('$') {
        find_definition(text, name)?;
    } else {
        find_fn_definition(text, name)?;
    }

    // Find the range of the word in the line
    let bytes = line.as_bytes();
    let mut start = col;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }

    Some(PrepareRenameResponse::Range(Range::new(
        Position::new(position.line, start as u32),
        Position::new(position.line, end as u32),
    )))
}

pub(crate) fn rename_at(
    text: &str,
    position: Position,
    new_name: &str,
    uri: &Url,
) -> Option<WorkspaceEdit> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    let is_var = word.starts_with('$');
    let name = &word[1..]; // strip $ or @

    // Strip $ or @ from new_name if user included it
    let new_base = new_name
        .strip_prefix('$')
        .or_else(|| new_name.strip_prefix('@'))
        .unwrap_or(new_name);

    let mut edits = Vec::new();
    let edit = |range: Range, new_text: String| TextEdit { range, new_text };

    // The definitions, where the name is written without its sigil
    for def in tree::definitions(text) {
        if is_var {
            if def.kind != DefinitionKind::Function && def.name == name {
                edits.push(edit(def.name_range, new_base.to_string()));
            }
            for param in def.params.iter().filter(|p| p.name == name) {
                edits.push(edit(param.name_range, new_base.to_string()));
            }
        } else if def.kind == DefinitionKind::Function && def.name == name {
            edits.push(edit(def.name_range, new_base.to_string()));
        }
    }

    // The references: `$name` or `@name`, outside verbatim bodies
    let verbatim = tree::verbatim_lines(&htmlang::syntax::parse(text));
    let sigil = if is_var { '$' } else { '@' };
    let search = format!("{}{}", sigil, name);
    let replace = format!("{}{}", sigil, new_base);
    for (i, line) in text.lines().enumerate() {
        let line_num = i as u32;
        if verbatim.contains(&line_num) {
            continue;
        }
        let mut offset = 0;
        while let Some(pos) = line[offset..].find(&search) {
            let abs_pos = offset + pos;
            let after = abs_pos + search.len();
            // Not part of a longer name
            let is_end = line[after..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '-' || c == '_'));
            if is_end {
                let range = Range::new(
                    Position::new(line_num, abs_pos as u32),
                    Position::new(line_num, after as u32),
                );
                // A parameter's `$` belongs to the edit above
                if !edits.iter().any(|e| overlaps(&e.range, &range)) {
                    edits.push(edit(range, replace.clone()));
                }
            }
            offset = after;
        }
    }

    if edits.is_empty() {
        return None;
    }

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn overlaps(a: &Range, b: &Range) -> bool {
    a.start.line == b.start.line
        && a.start.character < b.end.character
        && b.start.character < a.end.character
}

// ---------------------------------------------------------------------------
// Linked editing ranges
// ---------------------------------------------------------------------------

pub(crate) fn linked_editing_ranges(text: &str, position: Position) -> Option<LinkedEditingRanges> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());

    // Find the $variable at the cursor
    let bytes = line.as_bytes();
    let mut start = col;
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'$'
            || bytes[start - 1] == b'-'
            || bytes[start - 1] == b'_')
    {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric()
            || bytes[end] == b'$'
            || bytes[end] == b'-'
            || bytes[end] == b'_')
    {
        end += 1;
    }
    if start == end {
        return None;
    }

    let word = &line[start..end];
    if !word.starts_with('$') {
        return None;
    }

    // Find all occurrences of this $variable in the document
    let mut ranges = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let line_bytes = line.as_bytes();
        let mut offset = 0;
        while let Some(pos) = line[offset..].find(word) {
            let abs_pos = offset + pos;
            // Check it's a whole word match
            let before_ok = abs_pos == 0 || {
                let c = line_bytes[abs_pos - 1];
                !c.is_ascii_alphanumeric() && c != b'-' && c != b'_'
            };
            let after_end = abs_pos + word.len();
            let after_ok = after_end >= line.len() || {
                let c = line_bytes[after_end];
                !c.is_ascii_alphanumeric() && c != b'-' && c != b'_'
            };
            if before_ok && after_ok {
                ranges.push(Range::new(
                    Position::new(line_idx as u32, abs_pos as u32),
                    Position::new(line_idx as u32, after_end as u32),
                ));
            }
            offset = abs_pos + word.len();
        }
    }

    if ranges.len() < 2 {
        return None;
    }

    Some(LinkedEditingRanges {
        ranges,
        word_pattern: None,
    })
}

// ---------------------------------------------------------------------------
// Cross-file lookups (workspace-aware)
// ---------------------------------------------------------------------------

/// Identify the symbol token (e.g. `@card` or `$primary`) under the cursor.
/// Returns `None` for plain words that aren't a directive or variable
/// reference.
pub(crate) fn symbol_at(text: &str, position: Position) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
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
    let word = &line[start..end];
    if word.starts_with('$') || word.starts_with('@') {
        return Some(word.to_string());
    }
    // Cursor on the bare name part (e.g. inside "card" of "@card") — synthesize
    // the prefix from the preceding byte if it's `@` or `$`.
    if start > 0 {
        let prev = bytes[start - 1];
        if prev == b'@' {
            return Some(format!("@{}", word));
        }
        if prev == b'$' {
            return Some(format!("${}", word));
        }
    }
    None
}

/// Resolve a definition by consulting every other file in the workspace
/// index. Returns the first match (deterministic by HashMap iteration order
/// is not guaranteed, but typical projects only define a name once).
pub(crate) fn cross_file_definition(
    text: &str,
    position: Position,
    index: &WorkspaceIndex,
) -> Option<GotoDefinitionResponse> {
    let symbol = symbol_at(text, position)?;
    let mut hits = index.find_symbol(&symbol);
    if hits.is_empty() {
        return None;
    }
    if hits.len() == 1 {
        return Some(GotoDefinitionResponse::Scalar(hits.remove(0)));
    }
    Some(GotoDefinitionResponse::Array(hits))
}

/// Scan `text` for occurrences of `symbol` (a `$name` or `@name` token) and
/// emit one `Location` per word-boundary match. Used to extend the local
/// `find_references` result across the workspace.
pub(crate) fn find_references_for_symbol(text: &str, symbol: &str, uri: &Url) -> Vec<Location> {
    let mut out = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let mut offset = 0;
        while let Some(pos) = line[offset..].find(symbol) {
            let abs_pos = offset + pos;
            let after = abs_pos + symbol.len();
            let bytes = line.as_bytes();
            let before_ok = abs_pos == 0 || {
                let c = bytes[abs_pos - 1];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'-'
            };
            let after_ok = after >= line.len() || {
                let c = bytes[after];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'-'
            };
            if before_ok && after_ok {
                out.push(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, abs_pos as u32),
                        Position::new(line_idx as u32, after as u32),
                    ),
                });
            }
            offset = after;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Find references
// ---------------------------------------------------------------------------

pub(crate) fn find_references(text: &str, position: Position, uri: &Url) -> Vec<Location> {
    let lines: Vec<&str> = text.lines().collect();
    let line = match lines.get(position.line as usize) {
        Some(l) => *l,
        None => return vec![],
    };
    let col = (position.character as usize).min(line.len());
    let bytes = line.as_bytes();

    // Find the word at cursor
    let mut start = col;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    if start == end {
        return vec![];
    }
    let word = &line[start..end];

    // Determine the search pattern
    let search = if word.starts_with('$') || word.starts_with('@') {
        word.to_string()
    } else if start > 0 && bytes[start - 1] == b'$' {
        format!("${}", word)
    } else if start > 0 && bytes[start - 1] == b'@' {
        format!("@{}", word)
    } else {
        return vec![];
    };

    let mut locations = Vec::new();
    for (line_idx, line_text) in text.lines().enumerate() {
        let mut offset = 0;
        while let Some(pos) = line_text[offset..].find(&search) {
            let abs_pos = offset + pos;
            let after = abs_pos + search.len();
            // Ensure word boundary
            let before_ok = abs_pos == 0 || {
                let c = line_text.as_bytes()[abs_pos - 1];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'-'
            };
            let after_ok = after >= line_text.len() || {
                let c = line_text.as_bytes()[after];
                !c.is_ascii_alphanumeric() && c != b'_' && c != b'-'
            };
            if before_ok && after_ok {
                locations.push(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, abs_pos as u32),
                        Position::new(line_idx as u32, after as u32),
                    ),
                });
            }
            offset = after;
        }
    }

    locations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_edits_definitions_and_references_once() {
        let text = "@let card $title $tone=info\n  @el [color $tone] $title\n@style\n  .x { content: \"$title\" }\n@card [title Hi]\n";
        let uri = Url::parse("file:///tmp/none/page.hl").unwrap();
        let edit = rename_at(text, Position::new(1, 22), "heading", &uri).expect("rename");
        let edits = edit.changes.unwrap().remove(&uri).unwrap();
        let mut at: Vec<(u32, u32, &str)> = edits
            .iter()
            .map(|e| {
                (
                    e.range.start.line,
                    e.range.start.character,
                    e.new_text.as_str(),
                )
            })
            .collect();
        at.sort_unstable();
        // The parameter (name only) and the reference; not the CSS body
        assert_eq!(at, [(0, 11, "heading"), (1, 20, "$heading")]);

        let found = find_fn_definition(text, "card").expect("definition");
        assert_eq!(found.start, Position::new(0, 5));
        let found = find_definition(text, "tone").expect("parameter");
        assert_eq!(found.start, Position::new(0, 18));
    }
}
