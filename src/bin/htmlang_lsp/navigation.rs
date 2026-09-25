use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::hover::{is_word_byte, word_at};
use crate::state::WorkspaceIndex;

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

    // Check for @include file path navigation
    let trimmed = line.trim();
    if let Some(filename) = trimmed.strip_prefix("@include ") {
        let filename = filename.trim();
        if !filename.is_empty() {
            let file_path = uri.to_file_path().ok()?;
            let dir = file_path.parent()?;
            let target = dir.join(filename);
            if target.exists() {
                let target_uri = Url::from_file_path(&target).ok()?;
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: target_uri,
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                }));
            }
        }
    }

    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    // Find definition location
    let (def_line, def_col, def_len) = if let Some(name) = word.strip_prefix('$') {
        find_definition(text, name)?
    } else if let Some(name) = word.strip_prefix('@') {
        find_fn_definition(text, name)?
    } else {
        return None;
    };

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),
        range: Range::new(
            Position::new(def_line, def_col),
            Position::new(def_line, def_col + def_len),
        ),
    }))
}

/// Find @let definition for a $name reference (variable, attribute bundle, or function param).
pub(crate) fn find_definition(text: &str, name: &str) -> Option<(u32, u32, u32)> {
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let offset = (line.len() - trimmed.len()) as u32;

        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let rest_trimmed = rest.trim();
            // Match variable or attribute bundle: @let name value / @let name [...]
            if let Some((n, _)) = rest_trimmed.split_once(' ') {
                if n == name {
                    let col = offset + "@let ".len() as u32;
                    return Some((i as u32, col, n.len() as u32));
                }
                // Check function parameters
                let parts: Vec<&str> = rest_trimmed.split_whitespace().collect();
                for param in &parts[1..] {
                    let p = param.strip_prefix('$').unwrap_or(param);
                    let p = p.split('=').next().unwrap_or(p);
                    if p == name
                        && let Some(pos) = line.find(param)
                    {
                        return Some((i as u32, pos as u32, param.len() as u32));
                    }
                }
            } else if rest_trimmed == name {
                // @let name (no value, function with no params)
                let col = offset + "@let ".len() as u32;
                return Some((i as u32, col, name.len() as u32));
            }
        }
    }
    None
}

/// Find @let function definition for an @name function call.
pub(crate) fn find_fn_definition(text: &str, name: &str) -> Option<(u32, u32, u32)> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let offset = (line.len() - trimmed.len()) as u32;

        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.first() == Some(&name) {
                // Verify it's a function (has body or params)
                let has_body = lines
                    .get(i + 1)
                    .map(|l| l.starts_with("  ") || l.starts_with('\t'))
                    .unwrap_or(false);
                if has_body {
                    let col = offset + "@let ".len() as u32;
                    return Some((i as u32, col, name.len() as u32));
                }
            }
        }
    }
    None
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

    for (i, line) in text.lines().enumerate() {
        let line_num = i as u32;
        let trimmed = line.trim();

        if is_var {
            // Rename @let definition
            if let Some(rest) = trimmed.strip_prefix("@let ")
                && let Some((n, _)) = rest.trim().split_once(' ')
                && n == name
                && let Some(pos) = line.find(n)
            {
                edits.push(TextEdit {
                    range: Range::new(
                        Position::new(line_num, pos as u32),
                        Position::new(line_num, (pos + n.len()) as u32),
                    ),
                    new_text: new_base.to_string(),
                });
            }

            // Rename @let attribute bundle or parameter definitions
            if let Some(rest) = trimmed.strip_prefix("@let ") {
                let rest_trimmed = rest.trim();
                // Attribute bundle: @let name [...]
                if let Some(bracket) = rest_trimmed.find('[') {
                    let n = rest_trimmed[..bracket].trim();
                    if n == name
                        && let Some(pos) = line.find(n)
                    {
                        edits.push(TextEdit {
                            range: Range::new(
                                Position::new(line_num, pos as u32),
                                Position::new(line_num, (pos + n.len()) as u32),
                            ),
                            new_text: new_base.to_string(),
                        });
                        continue; // Don't also match $ references on this line
                    }
                }
                // Function parameter definitions
                let parts: Vec<&str> = rest_trimmed.split_whitespace().collect();
                for param in &parts[1..] {
                    let p = param.strip_prefix('$').unwrap_or(param);
                    let p = p.split('=').next().unwrap_or(p);
                    if p == name
                        && let Some(pos) = line.find(param)
                    {
                        let prefix = if param.starts_with('$') { "$" } else { "" };
                        edits.push(TextEdit {
                            range: Range::new(
                                Position::new(line_num, pos as u32),
                                Position::new(line_num, (pos + param.len()) as u32),
                            ),
                            new_text: format!("{}{}", prefix, new_base),
                        });
                    }
                }
            }

            // Rename all $name references
            let search = format!("${}", name);
            let replace = format!("${}", new_base);
            let mut offset = 0;
            while let Some(pos) = line[offset..].find(&search) {
                let abs_pos = offset + pos;
                // Check it's not part of a longer identifier
                let after = abs_pos + search.len();
                let is_end = after >= line.len()
                    || !line.as_bytes()[after].is_ascii_alphanumeric()
                        && line.as_bytes()[after] != b'-'
                        && line.as_bytes()[after] != b'_';
                if is_end {
                    edits.push(TextEdit {
                        range: Range::new(
                            Position::new(line_num, abs_pos as u32),
                            Position::new(line_num, (abs_pos + search.len()) as u32),
                        ),
                        new_text: replace.clone(),
                    });
                }
                offset = abs_pos + search.len();
            }
        } else {
            // Rename @let function definition
            if let Some(rest) = trimmed.strip_prefix("@let ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.first() == Some(&name)
                    && let Some(pos) = line.find(&format!("@let {}", name))
                {
                    let start = pos + 5; // skip "@let "
                    edits.push(TextEdit {
                        range: Range::new(
                            Position::new(line_num, start as u32),
                            Position::new(line_num, (start + name.len()) as u32),
                        ),
                        new_text: new_base.to_string(),
                    });
                }
            }

            // Rename @name function calls
            let search = format!("@{}", name);
            let replace = format!("@{}", new_base);
            let mut offset = 0;
            while let Some(pos) = line[offset..].find(&search) {
                let abs_pos = offset + pos;
                // Don't match @let definition (handled above)
                if trimmed.starts_with("@let ") {
                    offset = abs_pos + search.len();
                    continue;
                }
                // Check it's not part of a longer identifier
                let after = abs_pos + search.len();
                let is_end = after >= line.len()
                    || !line.as_bytes()[after].is_ascii_alphanumeric()
                        && line.as_bytes()[after] != b'-'
                        && line.as_bytes()[after] != b'_';
                if is_end {
                    edits.push(TextEdit {
                        range: Range::new(
                            Position::new(line_num, abs_pos as u32),
                            Position::new(line_num, (abs_pos + search.len()) as u32),
                        ),
                        new_text: replace.clone(),
                    });
                }
                offset = abs_pos + search.len();
            }
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
