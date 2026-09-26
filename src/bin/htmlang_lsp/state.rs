use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use htmlang::parser::ParseResult;
use tower_lsp::lsp_types::*;

use crate::analysis::document_symbols;

/// In-memory document state with a lazily-computed parse cache.
///
/// The parse is cached per (text, version) tuple — a new `DocumentEntry` is
/// created whenever the text changes, so the OnceLock is always consistent
/// with the entry's text.
pub struct DocumentEntry {
    pub text: String,
    pub version: i32,
    /// The directory of the file, so relative `@include` and `@data` paths
    /// resolve as they do on the command line. `None` for unsaved buffers.
    pub base: Option<PathBuf>,
    parse: OnceLock<Arc<ParseResult>>,
    tree: OnceLock<Arc<htmlang::syntax::Tree>>,
    symbols: OnceLock<Arc<Vec<SymbolInformation>>>,
}

impl DocumentEntry {
    pub fn new(text: String, version: i32, base: Option<PathBuf>) -> Self {
        Self {
            text,
            version,
            base,
            parse: OnceLock::new(),
            tree: OnceLock::new(),
            symbols: OnceLock::new(),
        }
    }

    pub fn parse(&self) -> Arc<ParseResult> {
        self.parse
            .get_or_init(|| {
                Arc::new(htmlang::parser::parse_with_base(
                    &self.text,
                    self.base.as_deref(),
                ))
            })
            .clone()
    }

    /// The syntax tree the compiler evaluates, shared by every feature.
    pub fn tree(&self) -> Arc<htmlang::syntax::Tree> {
        self.tree
            .get_or_init(|| Arc::new(htmlang::syntax::parse(&self.text)))
            .clone()
    }

    pub fn symbols(&self) -> Arc<Vec<SymbolInformation>> {
        self.symbols
            .get_or_init(|| Arc::new(document_symbols(&self.text)))
            .clone()
    }
}

/// The directory a document's relative paths resolve against.
pub fn base_dir(uri: &Url) -> Option<PathBuf> {
    uri.to_file_path()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Apply a single content change event to a string. Used when the LSP client
/// negotiates incremental sync. `utf8` selects how `Position::character` is
/// interpreted: UTF-8 bytes when the client agreed to the `utf-8` position
/// encoding, UTF-16 code units (the LSP default) otherwise.
pub fn apply_change(text: &mut String, change: &TextDocumentContentChangeEvent, utf8: bool) {
    if let Some(range) = change.range {
        let start = position_to_byte(text, range.start, utf8);
        let end = position_to_byte(text, range.end, utf8).max(start);
        text.replace_range(start..end, &change.text);
    } else {
        text.clear();
        text.push_str(&change.text);
    }
}

/// Convert an LSP `Position` to a byte offset in `text`.
///
/// Per the LSP spec, a `character` past the end of the line resolves to the
/// end of that line, and a line past the end of the document resolves to the
/// end of the document. The result always lies on a char boundary.
fn position_to_byte(text: &str, pos: Position, utf8: bool) -> usize {
    let mut line_start = 0;
    for _ in 0..pos.line {
        match text[line_start..].find('\n') {
            Some(nl) => line_start += nl + 1,
            None => return text.len(),
        }
    }
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |nl| line_start + nl);
    let line = &text[line_start..line_end];
    let target = pos.character as usize;

    let mut units = 0;
    for (byte_idx, ch) in line.char_indices() {
        if units >= target {
            return line_start + byte_idx;
        }
        units += if utf8 { ch.len_utf8() } else { ch.len_utf16() };
    }
    line_end
}

// ---------------------------------------------------------------------------
// Workspace index
// ---------------------------------------------------------------------------

/// Cached symbols for every `.hl` file under the workspace root that the LSP
/// has scanned. Built lazily on first workspace-wide query and refreshed via
/// the file watcher.
pub struct WorkspaceIndex {
    /// Per-file symbols. Each entry's location URI already points at the file.
    pub by_file: HashMap<PathBuf, Vec<SymbolInformation>>,
    pub root: Option<PathBuf>,
    /// Set to `true` once a full scan has been performed; subsequent queries
    /// skip the rescan and rely on file-watcher updates.
    pub scanned: bool,
}

impl WorkspaceIndex {
    pub fn new() -> Self {
        Self {
            by_file: HashMap::new(),
            root: None,
            scanned: false,
        }
    }

    /// Set the workspace root once. The root is sticky for the LSP session.
    pub fn set_root(&mut self, root: PathBuf) {
        if self.root.is_none() {
            self.root = Some(root);
        }
    }

    /// Walk the workspace root and index every `.hl` file found, skipping
    /// the usual junk directories.
    pub fn scan(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && (name.starts_with('.')
                        || name == "target"
                        || name == "node_modules"
                        || name == "dist"
                        || name == "out"
                        || name == "build")
                {
                    continue;
                }
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_some_and(|e| e == "hl") {
                    self.update_from_disk(&path);
                }
            }
        }
        self.scanned = true;
    }

    /// Re-read a file from disk and refresh its entry in the index.
    pub fn update_from_disk(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        self.update_from_text(path, &text);
    }

    /// Replace the index entry for `path` with symbols extracted from `text`.
    pub fn update_from_text(&mut self, path: &Path, text: &str) {
        let Ok(uri) = Url::from_file_path(path) else {
            return;
        };
        let mut syms = document_symbols(text);
        for s in &mut syms {
            s.location.uri = uri.clone();
        }
        self.by_file.insert(path.to_path_buf(), syms);
    }

    pub fn remove(&mut self, path: &Path) {
        self.by_file.remove(path);
    }

    /// Look for a symbol with `name` (e.g. `@card` or `$primary`) anywhere in
    /// the index, returning every matching location.
    pub fn find_symbol(&self, name: &str) -> Vec<Location> {
        let mut out = Vec::new();
        for syms in self.by_file.values() {
            for s in syms {
                if s.name == name {
                    out.push(s.location.clone());
                }
            }
        }
        out
    }

    /// Iterate over every (file path, file text) pair in the index. The text
    /// is read fresh from disk — callers that need consistent views with open
    /// editor buffers should consult those buffers first.
    pub fn iter_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.by_file.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(start: (u32, u32), end: (u32, u32), text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: Some(Range::new(
                Position::new(start.0, start.1),
                Position::new(end.0, end.1),
            )),
            range_length: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn apply_change_full_replace() {
        let mut s = String::from("hello\nworld");
        apply_change(
            &mut s,
            &TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "replaced".into(),
            },
            true,
        );
        assert_eq!(s, "replaced");
    }

    #[test]
    fn apply_change_insert_in_middle() {
        let mut s = String::from("@row [width 200]\n@text hello");
        // Position 15 is just before the closing `]` on line 0.
        apply_change(&mut s, &change((0, 15), (0, 15), ", padding 10"), true);
        assert_eq!(s, "@row [width 200, padding 10]\n@text hello");
    }

    #[test]
    fn apply_change_delete_across_lines() {
        let mut s = String::from("line one\nline two\nline three");
        apply_change(&mut s, &change((0, 4), (2, 4), ""), true);
        assert_eq!(s, "line three");
    }

    #[test]
    fn apply_change_replace_at_eof() {
        let mut s = String::from("abc");
        apply_change(&mut s, &change((0, 3), (0, 3), "def"), true);
        assert_eq!(s, "abcdef");
    }

    #[test]
    fn apply_change_utf16_positions() {
        // "é" is 2 UTF-8 bytes but 1 UTF-16 unit; "😀" is 4 bytes / 2 units.
        let mut s = String::from("é😀x\nnext");
        apply_change(&mut s, &change((0, 3), (0, 4), "y"), false);
        assert_eq!(s, "é😀y\nnext");
    }

    #[test]
    fn apply_change_utf8_positions_with_multibyte() {
        let mut s = String::from("é😀x\nnext");
        apply_change(&mut s, &change((0, 6), (0, 7), "y"), true);
        assert_eq!(s, "é😀y\nnext");
    }

    #[test]
    fn apply_change_clamps_character_to_line_end() {
        // A character past the end of line 0 must not spill into line 1.
        let mut s = String::from("ab\ncd");
        apply_change(&mut s, &change((0, 99), (0, 99), "!"), false);
        assert_eq!(s, "ab!\ncd");
    }

    #[test]
    fn relative_includes_resolve_from_the_documents_folder() {
        let dir = std::env::temp_dir().join(format!("htmlang-lsp-base-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("part.hl"), "@let helper\n  @el\n    @children\n").unwrap();
        std::fs::write(dir.join("site.json"), "{\"name\": \"Demo\"}").unwrap();
        let uri = Url::from_file_path(dir.join("page.hl")).unwrap();
        let text = "@include part.hl\n@data $site site.json\n@helper $site.name\n".to_string();
        let entry = DocumentEntry::new(text, 1, base_dir(&uri));
        let result = entry.parse();
        std::fs::remove_dir_all(&dir).ok();
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    }
}
