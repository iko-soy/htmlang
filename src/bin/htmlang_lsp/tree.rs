//! The compiler's syntax tree in the editor's terms: definitions, lines and
//! ranges. Every feature that needs to know what a line *is* (a `@let`, an
//! `@include`, a verbatim body) asks here instead of matching raw text.

use htmlang::syntax::{self, DefinitionKind, Node, NodeKind, Span};
use tower_lsp::lsp_types::{Position, Range};

/// A `@let`, with LSP positions (0-based lines, byte columns).
#[derive(Clone, Debug)]
pub(crate) struct Def {
    pub name: String,
    pub kind: DefinitionKind,
    pub name_range: Range,
    /// The `@let` line.
    pub line: u32,
    /// The last line of the definition, its body included.
    pub end_line: u32,
    pub params: Vec<Param>,
    /// The value as written (a value or a bundle).
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Param {
    pub name: String,
    pub default: Option<String>,
    /// Just its name.
    pub name_range: Range,
}

/// A single-line span as an LSP range.
pub(crate) fn range(span: Span) -> Range {
    let line = span.line.saturating_sub(1) as u32;
    Range::new(
        Position::new(line, span.column as u32),
        Position::new(
            line,
            (span.column + span.end.saturating_sub(span.start)) as u32,
        ),
    )
}

/// Every `@let` in `text`, in source order.
pub(crate) fn definitions(text: &str) -> Vec<Def> {
    let tree = syntax::parse(text);
    tree.definitions()
        .into_iter()
        .map(|def| Def {
            name: def.name.to_string(),
            kind: def.kind,
            name_range: range(def.name_span),
            line: def.node.span.line.saturating_sub(1) as u32,
            end_line: def.node.end_line().saturating_sub(1) as u32,
            params: def
                .params
                .iter()
                .map(|p| Param {
                    name: p.name.clone(),
                    default: p.default.clone(),
                    name_range: range(p.name_span),
                })
                .collect(),
            value: def.value,
        })
        .collect()
}

/// The 0-based lines of verbatim bodies (CSS, JavaScript, HTML, Markdown),
/// in which nothing is htmlang.
pub(crate) fn verbatim_lines(tree: &syntax::Tree) -> std::collections::HashSet<u32> {
    let mut lines = std::collections::HashSet::new();
    tree.walk(&mut |node| {
        if matches!(node.kind, NodeKind::Verbatim(_)) {
            let first = node.span.line.saturating_sub(1) as u32;
            for line in first..first + node.line_count as u32 {
                lines.insert(line);
            }
        }
    });
    lines
}

/// The node whose own lines include the 0-based `line`.
pub(crate) fn node_at(tree: &syntax::Tree, line: u32) -> Option<&Node> {
    tree.node_at_line(line as usize + 1)
}

/// The file argument of a directive that names one (`@include`,
/// `@markdown`, `@data`), with its range: not inline JSON, an environment
/// variable or a glob.
pub(crate) fn file_argument(node: &Node) -> Option<(String, Range)> {
    use syntax::DirectiveArgs;
    let arg = match &node.directive()?.args {
        DirectiveArgs::Text(Some(arg)) if node.is_directive("include") => arg,
        DirectiveArgs::Text(Some(arg)) if node.is_directive("markdown") => arg,
        DirectiveArgs::Data { source, .. } => source,
        _ => return None,
    };
    let path = arg.raw.trim_matches('"');
    let is_file = !path.is_empty()
        && !path.starts_with(['[', '{'])
        && !path.starts_with("env:")
        && !path.contains(['*', '?', '$']);
    is_file.then(|| {
        let offset = arg.raw.find(path).unwrap_or(0);
        let mut r = range(arg.span);
        r.start.character += offset as u32;
        r.end.character = r.start.character + path.len() as u32;
        (path.to_string(), r)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_come_from_the_tree() {
        let text = "@let a 1\n@let @card [title, tone x]\n  @el $title\n@let b [padding 4]\n@let c = 1 + 1\n@el\n  @let d 1\n";
        let defs = definitions(text);
        let names: Vec<_> = defs.iter().map(|d| (d.name.as_str(), d.kind)).collect();
        assert_eq!(
            names,
            [
                ("a", DefinitionKind::Value),
                ("card", DefinitionKind::Function),
                ("b", DefinitionKind::Bundle),
                ("c", DefinitionKind::Value),
                ("d", DefinitionKind::Value),
            ]
        );
        let card = &defs[1];
        assert_eq!((card.line, card.end_line), (1, 2));
        assert_eq!(card.params[1].name_range.start, Position::new(1, 19));
        assert_eq!(card.params[1].default.as_deref(), Some("x"));
        // The name without the function's `@`
        assert_eq!(card.name_range.start, Position::new(1, 6));
    }
}
