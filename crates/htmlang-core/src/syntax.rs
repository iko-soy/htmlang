//! The syntax tree: source text read into lines, element heads, attribute
//! lists and text, before anything is evaluated.
//!
//! The tree is lossless enough to print the file back: comments and blank
//! lines are nodes, every piece keeps its source [`Span`], and verbatim
//! bodies keep their text. It reads no files and substitutes no variables.
//! The evaluator (`parser.rs`), the formatter and the language server all
//! work on this one tree, and what a directive accepts comes from the
//! [`DIRECTIVES`](crate::ast::DIRECTIVES) table.
//!
//! The grammar, line by line:
//! - A line starting with `--` is a comment; an empty line is blank.
//! - A line starting with `@NAME` is a directive when `NAME` is in the
//!   directive table, and an element (or function call) otherwise. An
//!   element line is a chain of heads `@name [attributes]` joined by `>`,
//!   then text. A `[` opens an attribute list only right after a head's
//!   name, or where a directive's grammar has one (`@page [..]`,
//!   `@let name [..]`, `@let @name [..]`); anywhere else it is text.
//! - What a `@let` defines is decided here, from its line: `@let @name` is
//!   a function (its body is the indented block), `@let name [..]` an
//!   attribute bundle and any other `@let name ...` a value.
//! - Any other line is text, in which `{@name [attributes] text}` is an
//!   inline element.
//! - An attribute list (or `@data`'s inline JSON) may continue onto the
//!   following lines until it closes.
//! - Lines indented deeper than a line are its body. What the body may be
//!   is the directive's or element's [`BodyKind`].

use crate::ast::{self, ArgGrammar, BodyKind, DirectiveSpec};
use crate::diagnostic::{Diagnostic, Severity, code};

/// A range of the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    /// Byte offsets into the source.
    pub start: usize,
    pub end: usize,
    /// 1-based line and 0-based byte column of `start`.
    pub line: usize,
    pub column: usize,
}

/// A parsed file.
#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// Syntax errors: they are reported for every line, whether or not it
    /// is ever evaluated.
    pub diagnostics: Vec<Diagnostic>,
    /// One past the largest node id.
    pub(crate) end_id: usize,
}

/// One line of the file (with its continuation lines), and the lines
/// indented under it.
#[derive(Clone, Debug)]
pub struct Node {
    /// Unique within a parse, in source order.
    pub id: usize,
    /// Width of the leading whitespace of its first line, in bytes.
    pub indent: usize,
    /// The node's own line(s), without its children.
    pub span: Span,
    /// How many source lines `span` covers.
    pub line_count: usize,
    /// The node's own lines joined into one (continuation lines joined with
    /// a space), without surrounding whitespace.
    pub source: String,
    pub kind: NodeKind,
    /// The lines indented under it, trivia included.
    pub children: Vec<Node>,
}

#[derive(Clone, Debug)]
pub enum NodeKind {
    /// An empty line.
    Blank,
    /// A `--` comment line; its text is the node's `source`.
    Comment,
    /// A line of text.
    Text(Text),
    /// `@name [attrs] > @name [attrs] text`: an element or a function call.
    Element(ElementLine),
    Directive(Directive),
    /// The body of a verbatim directive or element (`@style`, `@script`).
    Verbatim(Verbatim),
}

/// Text: plain runs and inline elements.
#[derive(Clone, Debug)]
pub struct Text {
    pub raw: String,
    pub span: Span,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug)]
pub enum Segment {
    /// Text as written: escapes are still in it and `$name`s are not
    /// substituted.
    Plain { raw: String, span: Span },
    /// `{@name [attrs] text}`.
    Inline(Box<Inline>),
}

#[derive(Clone, Debug)]
pub struct Inline {
    /// What is between the braces.
    pub raw: String,
    /// Including the braces.
    pub span: Span,
    pub head: Head,
    pub text: Option<Text>,
    /// Whether the closing `}` was found.
    pub closed: bool,
}

/// `@name [attributes]`.
#[derive(Clone, Debug)]
pub struct Head {
    /// Without the `@`.
    pub name: String,
    /// Including the `@`.
    pub name_span: Span,
    pub attrs: Option<AttrList>,
    pub span: Span,
}

/// `[attr, attr]`.
#[derive(Clone, Debug)]
pub struct AttrList {
    pub attrs: Vec<Attr>,
    /// Including the brackets.
    pub span: Span,
    /// Whether the closing `]` was found.
    pub closed: bool,
}

/// One attribute of a list, as written.
#[derive(Clone, Debug)]
pub struct Attr {
    /// The attribute's text, trimmed.
    pub raw: String,
    pub key: String,
    pub value: Option<String>,
    /// Written `key=value`.
    pub html: bool,
    pub span: Span,
    /// The attribute is `if(CONDITION, A, B)` as a whole.
    pub choice: Option<Box<Choice>>,
}

/// A whole attribute `if(CONDITION, A, B)`: `A` or `B`, by the condition.
#[derive(Clone, Debug)]
pub struct Choice {
    /// The condition, an expression; `None` when it is empty.
    pub condition: Option<Arg>,
    /// The branches after the condition, as written: `A`, and `B` when
    /// there is one. Any other number is an error the evaluator reports.
    pub branches: Vec<Branch>,
}

/// One branch of an [`if()`](Choice).
#[derive(Clone, Debug)]
pub enum Branch {
    /// Nothing: the attribute is left out.
    Empty,
    /// One attribute (or `$bundle`, or another `if()`).
    Attr(Attr),
    /// `[attr, attr]`: several attributes at once. `trailing` is text
    /// written after the `]`, an error.
    Group {
        list: AttrList,
        trailing: Option<Arg>,
    },
}

impl Branch {
    /// The branch's attributes as written.
    pub fn attrs(&self) -> &[Attr] {
        match self {
            Branch::Empty => &[],
            Branch::Attr(attr) => std::slice::from_ref(attr),
            Branch::Group { list, .. } => &list.attrs,
        }
    }
}

/// A directive's argument text, as written.
#[derive(Clone, Debug)]
pub struct Arg {
    pub raw: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ElementLine {
    /// The heads joined by `>`; the last one gets the children.
    pub chain: Vec<Head>,
    /// Text after the last head.
    pub text: Option<Text>,
}

#[derive(Clone, Debug)]
pub struct Directive {
    pub spec: &'static DirectiveSpec,
    /// Including the `@`.
    pub name_span: Span,
    pub args: DirectiveArgs,
}

impl Directive {
    pub fn name(&self) -> &'static str {
        self.spec.name
    }
}

/// A directive's arguments, read by its [`ArgGrammar`].
#[derive(Clone, Debug)]
pub enum DirectiveArgs {
    Text(Option<Arg>),
    Page {
        attrs: Option<AttrList>,
        title: Option<Arg>,
    },
    Let(LetDef),
    Condition(Arg),
    Else {
        condition: Option<Arg>,
    },
    Each(Loop),
    Data {
        name: String,
        name_span: Span,
        source: Arg,
    },
    /// The header has a syntax error, which is already reported.
    Invalid,
}

/// `@let NAME ...` or `@let @NAME [params]`. Its kind is decided here,
/// from the line alone: an `@` before the name makes a function, a `[`
/// after it a bundle, anything else a value.
#[derive(Clone, Debug)]
pub struct LetDef {
    /// Without the function's `@`.
    pub name: String,
    /// Just the name, without the function's `@`.
    pub name_span: Span,
    pub form: LetForm,
}

#[derive(Clone, Debug)]
pub enum LetForm {
    /// `@let name text`.
    Value(Arg),
    /// `@let name "text"`: the text between the quotes.
    Quoted(Arg),
    /// `@let name = EXPR`: the expression.
    Computed(Arg),
    /// `@let name [attrs]`.
    Bundle(AttrList),
    /// `@let @name [param, param default]` (or `@let @name`) with an
    /// indented body.
    Function(Function),
}

/// A function's head: its parameter list as written, and the parameters
/// read from it.
#[derive(Clone, Debug, Default)]
pub struct Function {
    /// `[param, param default]`; `None` for `@let @name`.
    pub list: Option<AttrList>,
    pub params: Vec<Param>,
}

/// One parameter: `title` (required) or `tone #f9fafb` (with a default).
/// The body uses it as `$title`.
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    /// The default as written, for `name value`.
    pub default: Option<String>,
    /// Just the name.
    pub name_span: Span,
    /// The whole parameter.
    pub span: Span,
}

/// `@each $item, $index in LIST`.
#[derive(Clone, Debug)]
pub struct Loop {
    pub item: String,
    pub index: Option<String>,
    pub list: Arg,
}

#[derive(Clone, Debug)]
pub struct Verbatim {
    /// The body's lines without their common indentation.
    pub text: String,
    /// Shown on the page as text, HTML-escaped: the body of `@code` and
    /// `@textarea`. Otherwise it goes into the page as it is (`@raw`,
    /// `@script`) or is read by its directive (`@style`, `@markdown`).
    pub escaped: bool,
}

// ---------------------------------------------------------------------------
// Queries used by the evaluator and the tools
// ---------------------------------------------------------------------------

impl Node {
    /// Blank lines and comments.
    pub fn is_trivia(&self) -> bool {
        matches!(self.kind, NodeKind::Blank | NodeKind::Comment)
    }

    /// The directive on this line, if it is one.
    pub fn directive(&self) -> Option<&Directive> {
        match &self.kind {
            NodeKind::Directive(d) => Some(d),
            _ => None,
        }
    }

    /// Whether this line is the directive `@name`.
    pub fn is_directive(&self, name: &str) -> bool {
        self.directive().is_some_and(|d| d.name() == name)
    }

    /// Children other than blank lines and comments.
    pub fn body(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter(|c| !c.is_trivia())
    }

    /// The last source line of the node and its children.
    pub fn end_line(&self) -> usize {
        let own = self.span.line + self.source_lines() - 1;
        self.children
            .iter()
            .filter(|c| !matches!(c.kind, NodeKind::Blank))
            .map(Node::end_line)
            .fold(own, usize::max)
    }

    fn source_lines(&self) -> usize {
        self.line_count.max(1)
    }

    /// Visit this node and everything under it, in source order.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        for child in &self.children {
            child.walk(f);
        }
    }

    /// The heads on this line: an element's chain, and the inline
    /// elements in its text.
    pub fn heads(&self) -> Vec<&Head> {
        let mut heads = Vec::new();
        match &self.kind {
            NodeKind::Element(line) => {
                heads.extend(line.chain.iter());
                if let Some(text) = &line.text {
                    text.inline_heads(&mut heads);
                }
            }
            NodeKind::Text(text) => text.inline_heads(&mut heads),
            _ => {}
        }
        heads
    }
}

impl Text {
    fn inline_heads<'a>(&'a self, out: &mut Vec<&'a Head>) {
        for segment in &self.segments {
            if let Segment::Inline(inline) = segment {
                out.push(&inline.head);
                if let Some(text) = &inline.text {
                    text.inline_heads(out);
                }
            }
        }
    }

    /// Split off the leading argument, the first token, with the rest as
    /// text: `/about` and `About us` in `@link /about About us`. The token
    /// ends at a space outside `"..."` and `${...}`, so neither is split,
    /// and it is taken before any `$name` is filled in.
    pub fn split_leading(&self) -> (Arg, Option<Text>) {
        let end = leading_token_len(&self.raw);
        let shifted = Shifted { base: self.span };
        let token = Arg {
            raw: self.raw[..end].to_string(),
            span: shifted.span(0, end),
        };
        let rest_start = self.raw.len() - self.raw[end..].trim_start().len();
        if rest_start >= self.raw.len() {
            return (token, None);
        }
        let reader = Reader {
            text: &self.raw,
            spans: &shifted,
        };
        (token, Some(reader.text(rest_start, self.raw.len()).0))
    }
}

/// The length of the first token of `s`: up to the first whitespace that
/// isn't inside `"..."` or `${...}` and isn't escaped.
pub fn leading_token_len(s: &str) -> usize {
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let escape = escape_len(rest);
        if escape > 0 {
            i += escape;
            continue;
        }
        if rest.starts_with('"') {
            // To the closing quote, or the end of the text
            let mut j = 1;
            while j < rest.len() && !rest[j..].starts_with('"') {
                j += match escape_len(&rest[j..]) {
                    0 => rest[j..].chars().next().map_or(1, char::len_utf8),
                    n => n,
                };
            }
            i += (j + 1).min(rest.len());
            continue;
        }
        if rest.starts_with("${")
            && let Some(close) = crate::interp::matching_brace(&rest[1..])
        {
            i += close + 2;
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        if c.is_whitespace() {
            break;
        }
        i += c.len_utf8();
    }
    i
}

impl Tree {
    /// Visit every node, in source order.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        for node in &self.nodes {
            node.walk(f);
        }
    }

    /// Every `@let` in the file, wherever it is.
    pub fn definitions(&self) -> Vec<Definition<'_>> {
        let mut out = Vec::new();
        self.walk(&mut |node| {
            let Some(Directive {
                args: DirectiveArgs::Let(def),
                ..
            }) = node.directive()
            else {
                return;
            };
            let (kind, params, value): (_, &[Param], _) = match &def.form {
                LetForm::Value(value) => (DefinitionKind::Value, &[], Some(value.raw.clone())),
                LetForm::Quoted(value) => (
                    DefinitionKind::Value,
                    &[],
                    Some(format!("\"{}\"", value.raw)),
                ),
                LetForm::Computed(expr) => {
                    (DefinitionKind::Value, &[], Some(format!("= {}", expr.raw)))
                }
                LetForm::Bundle(attrs) => (
                    DefinitionKind::Bundle,
                    &[],
                    Some(
                        attrs
                            .attrs
                            .iter()
                            .map(|a| a.raw.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                ),
                LetForm::Function(function) => {
                    (DefinitionKind::Function, function.params.as_slice(), None)
                }
            };
            out.push(Definition {
                name: &def.name,
                name_span: def.name_span,
                kind,
                params,
                value,
                node,
            });
        });
        out
    }

    /// The names visible at `line` (1-based), by the lexical rule the
    /// compiler follows: a definition (`@let`, `@data`) is visible from the
    /// line after it to the end of its block; an `@each` line's variables
    /// in its body; a function's parameters (and the function itself) in
    /// its body, which also sees what is visible where the function is
    /// defined. In the order they are defined, so a later one with the same
    /// name hides an earlier one.
    pub fn visible_at(&self, line: usize) -> Vec<Visible<'_>> {
        let mut out = Vec::new();
        visible_in(&self.nodes, line, &mut out);
        out
    }

    /// The innermost node whose own lines include `line` (1-based).
    pub fn node_at_line(&self, line: usize) -> Option<&Node> {
        let mut found = None;
        self.walk(&mut |node| {
            let last = node.span.line + node.source_lines() - 1;
            if (node.span.line..=last).contains(&line) && !matches!(node.kind, NodeKind::Blank) {
                found = Some(node);
            }
        });
        found
    }
}

/// What a `@let` defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefinitionKind {
    /// A value (`@let x 1`, `@let x = 1 + 1`, `@let x "text"`), used as `$x`.
    Value,
    /// An attribute bundle (`@let x [...]`), used as `[$x]`.
    Bundle,
    /// A function (`@let @x [a]` with a body), called as `@x`.
    Function,
}

/// A `@let`, for tools.
#[derive(Clone, Debug)]
pub struct Definition<'a> {
    pub name: &'a str,
    pub name_span: Span,
    pub kind: DefinitionKind,
    pub params: &'a [Param],
    /// The value as written, for a value or a bundle.
    pub value: Option<String>,
    /// The `@let` line, with the function's body as its children.
    pub node: &'a Node,
}

/// A name visible at a line (see [`Tree::visible_at`]).
#[derive(Clone, Debug)]
pub struct Visible<'a> {
    pub name: &'a str,
    pub kind: VisibleKind,
    /// Where the name is written in its definition (for a loop variable,
    /// the `@each` line).
    pub span: Span,
}

/// What defines a visible name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibleKind {
    /// A `@let`, of this kind.
    Let(DefinitionKind),
    /// A function's parameter, in its body.
    Parameter,
    /// An `@each` variable, in its body.
    Loop,
    /// `@data $name`.
    Data,
}

fn visible_in<'a>(block: &'a [Node], line: usize, out: &mut Vec<Visible<'a>>) {
    for node in block {
        if node.span.line >= line {
            break;
        }
        let inside = line <= node.end_line();
        match node.directive().map(|d| &d.args) {
            Some(DirectiveArgs::Let(def)) => {
                let kind = match &def.form {
                    LetForm::Function(_) => DefinitionKind::Function,
                    LetForm::Bundle(_) => DefinitionKind::Bundle,
                    _ => DefinitionKind::Value,
                };
                // `@let t.greeting` gives the record `t` a field
                let name = def.name.split('.').next().unwrap_or(&def.name);
                out.push(Visible {
                    name,
                    kind: VisibleKind::Let(kind),
                    span: def.name_span,
                });
                if let (true, LetForm::Function(function)) = (inside, &def.form) {
                    out.extend(function.params.iter().map(|p| Visible {
                        name: &p.name,
                        kind: VisibleKind::Parameter,
                        span: p.name_span,
                    }));
                }
            }
            Some(DirectiveArgs::Data {
                name, name_span, ..
            }) => out.push(Visible {
                name,
                kind: VisibleKind::Data,
                span: *name_span,
            }),
            Some(DirectiveArgs::Each(each)) if inside => {
                for name in std::iter::once(&each.item).chain(&each.index) {
                    out.push(Visible {
                        name,
                        kind: VisibleKind::Loop,
                        span: node.span,
                    });
                }
            }
            _ => {}
        }
        if inside {
            visible_in(&node.children, line, out);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse source text into a syntax tree.
pub fn parse(source: &str) -> Tree {
    parse_from(source, 0)
}

/// Parse, numbering the nodes from `first_id`.
pub(crate) fn parse_from(source: &str, first_id: usize) -> Tree {
    let lines = physical_lines(source);
    let index = LineIndex {
        starts: lines.iter().map(|l| l.0).collect(),
    };
    let mut diagnostics = Vec::new();
    let entries = scan(&lines, &index, &mut diagnostics);
    let mut next_id = first_id;
    let mut pos = 0;
    let mut nodes = build(entries, &mut pos, None, &mut next_id);
    literal_text(&mut nodes, false);
    check(&mut nodes, &mut diagnostics);
    diagnostics.sort_by_key(|d| d.line);
    Tree {
        nodes,
        diagnostics,
        end_id: next_id,
    }
}

/// The source's lines with their byte offsets, like `str::lines`.
fn physical_lines(source: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0;
    for piece in source.split('\n') {
        lines.push((start, piece.strip_suffix('\r').unwrap_or(piece)));
        start += piece.len() + 1;
    }
    if source.is_empty() || source.ends_with('\n') {
        lines.pop();
    }
    lines
}

struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    fn span(&self, start: usize, end: usize) -> Span {
        let line = self.starts.partition_point(|&s| s <= start).max(1);
        Span {
            start,
            end,
            line,
            column: start - self.starts.get(line - 1).copied().unwrap_or(0),
        }
    }
}

fn leading_whitespace(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// A node before the tree is built: one logical line.
struct Entry {
    indent: usize,
    span: Span,
    span_lines: usize,
    source: String,
    kind: NodeKind,
}

/// Read the lines into entries: comments, blank lines, headers (with their
/// continuation lines joined) and verbatim bodies.
fn scan(
    lines: &[(usize, &str)],
    index: &LineIndex,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (start, line) = lines[i];
        let trimmed = line.trim();
        let indent = leading_whitespace(line);
        let begin = start + indent;
        if trimmed.is_empty() {
            entries.push(Entry {
                // Resolved below, from the next line
                indent: 0,
                span: index.span(start, start + line.len()),
                span_lines: 1,
                source: String::new(),
                kind: NodeKind::Blank,
            });
            i += 1;
            continue;
        }
        if trimmed.starts_with("--") {
            entries.push(Entry {
                indent,
                span: index.span(begin, begin + trimmed.len()),
                span_lines: 1,
                source: trimmed.to_string(),
                kind: NodeKind::Comment,
            });
            i += 1;
            continue;
        }

        // A header, joined with its continuation lines while an attribute
        // list (or inline JSON) is open. Blank lines and comments inside it
        // are skipped.
        let mut logical = Logical::default();
        logical.push(trimmed, begin);
        let (mut kind, mut open, mut problems) = logical.header(index);
        let mut last = i;
        while open && last + 1 < lines.len() {
            last += 1;
            let (next_start, next) = lines[last];
            let t = next.trim();
            if t.is_empty() || t.starts_with("--") {
                continue;
            }
            logical.push(t, next_start + leading_whitespace(next));
            (kind, open, problems) = logical.header(index);
        }
        diagnostics.extend(problems);
        let end = lines[last].0 + lines[last].1.len();
        // What the lines under it are is decided by the directive, or by
        // the last element of a chain (`@pre > @code`)
        let (body, escaped) = match &kind {
            NodeKind::Directive(d) => (d.spec.body, false),
            NodeKind::Element(line) => line.chain.last().map_or((BodyKind::Htmlang, false), |h| {
                (ast::body_kind(&h.name), ast::has_literal_text(&h.name))
            }),
            _ => (BodyKind::Htmlang, false),
        };
        entries.push(Entry {
            indent,
            span: index.span(begin, end),
            span_lines: last - i + 1,
            source: logical.text.clone(),
            kind,
        });
        i = last + 1;

        // A verbatim body: every following line indented deeper (or blank),
        // up to the last such non-blank line.
        if body == BodyKind::Verbatim {
            let mut body_end = i;
            let mut k = i;
            while k < lines.len() {
                let l = lines[k].1;
                k += 1;
                if l.trim().is_empty() {
                    continue;
                }
                if leading_whitespace(l) <= indent {
                    break;
                }
                body_end = k;
            }
            if body_end > i {
                let body = &lines[i..body_end];
                let common = body
                    .iter()
                    .filter(|(_, l)| !l.trim().is_empty())
                    .map(|(_, l)| leading_whitespace(l))
                    .min()
                    .unwrap_or(0);
                let text: Vec<&str> = body
                    .iter()
                    .map(|(_, l)| l.get(common..).unwrap_or("").trim_end())
                    .collect();
                let (last_start, last_line) = lines[body_end - 1];
                entries.push(Entry {
                    indent: indent + 1,
                    span: index.span(lines[i].0, last_start + last_line.len()),
                    span_lines: body_end - i,
                    source: String::new(),
                    kind: NodeKind::Verbatim(Verbatim {
                        text: text.join("\n"),
                        escaped,
                    }),
                });
                i = body_end;
            }
        }
    }

    // A blank line belongs where the next line does.
    let mut next_indent = 0;
    for entry in entries.iter_mut().rev() {
        if matches!(entry.kind, NodeKind::Blank) {
            entry.indent = next_indent;
        } else {
            next_indent = entry.indent;
        }
    }
    entries
}

/// Build the nodes for the entries from `pos` indented deeper than `parent`.
fn build(
    entries: Vec<Entry>,
    pos: &mut usize,
    parent: Option<usize>,
    next_id: &mut usize,
) -> Vec<Node> {
    // Entries are taken out of the vector one by one, in order.
    let mut entries: Vec<Option<Entry>> = entries.into_iter().map(Some).collect();
    build_from(&mut entries, pos, parent, next_id)
}

fn build_from(
    entries: &mut [Option<Entry>],
    pos: &mut usize,
    parent: Option<usize>,
    next_id: &mut usize,
) -> Vec<Node> {
    let mut nodes = Vec::new();
    while let Some(Some(entry)) = entries.get(*pos) {
        if parent.is_some_and(|p| entry.indent <= p) {
            break;
        }
        let Some(entry) = entries[*pos].take() else {
            break;
        };
        *pos += 1;
        let id = *next_id;
        *next_id += 1;
        let takes_children = !matches!(
            entry.kind,
            NodeKind::Blank | NodeKind::Comment | NodeKind::Verbatim(_)
        );
        let children = if takes_children {
            build_from(entries, pos, Some(entry.indent), next_id)
        } else {
            Vec::new()
        };
        nodes.push(Node {
            id,
            indent: entry.indent,
            span: entry.span,
            line_count: entry.span_lines,
            source: entry.source,
            kind: entry.kind,
            children,
        });
    }
    nodes
}

/// Make the text of `@code` and `@textarea` literal, on their line, inline
/// (`{@code {@link /x y}}`) and in the lines under a chain they are in the
/// middle of (`@code > @b`): a `{@...}` in it is text. (The lines under
/// `@code` itself are a verbatim body, see `scan`.) `inside` says that
/// `nodes` are in such an element.
fn literal_text(nodes: &mut [Node], inside: bool) {
    for node in nodes {
        let mut literal = inside;
        match &mut node.kind {
            NodeKind::Text(text) => text.literal_where_asked(inside),
            NodeKind::Element(line) => {
                literal |= line
                    .chain
                    .iter()
                    .any(|head| ast::has_literal_text(&head.name));
                if let Some(text) = &mut line.text {
                    text.literal_where_asked(literal);
                }
            }
            _ => {}
        }
        literal_text(&mut node.children, literal);
    }
}

impl Text {
    /// Make the whole text literal when `literal`, and otherwise the text
    /// of the inline elements in it that show their text as written.
    fn literal_where_asked(&mut self, literal: bool) {
        if literal {
            self.segments = match self.raw.is_empty() {
                true => Vec::new(),
                false => vec![Segment::Plain {
                    raw: self.raw.clone(),
                    span: self.span,
                }],
            };
            return;
        }
        for segment in &mut self.segments {
            if let Segment::Inline(inline) = segment
                && let Some(text) = &mut inline.text
            {
                text.literal_where_asked(ast::has_literal_text(&inline.head.name));
            }
        }
    }
}

/// Checks on the built tree: what each line's body may be, and `@else`
/// placement.
fn check(nodes: &mut [Node], diagnostics: &mut Vec<Diagnostic>) {
    #[derive(PartialEq)]
    enum Opener {
        If,
        Each,
    }
    let mut opener = None;
    for node in nodes.iter_mut() {
        if node.is_trivia() {
            continue;
        }
        let line = node.span.line;
        match node.directive().map(|d| (d.name(), &d.args)) {
            Some(("if", _)) => opener = Some(Opener::If),
            Some(("each", _)) => opener = Some(Opener::Each),
            Some(("else", args)) => {
                let else_if = matches!(args, DirectiveArgs::Else { condition: Some(_) });
                opener = match opener {
                    Some(Opener::If) if else_if => Some(Opener::If),
                    Some(Opener::If) | Some(Opener::Each) if !else_if => None,
                    _ => {
                        diagnostics.push(
                            Diagnostic::error(
                                code::STRAY_ELSE,
                                line,
                                "@else without matching @if".to_string(),
                            )
                            .source(node.source.clone()),
                        );
                        None
                    }
                };
            }
            _ => opener = None,
        }
        check_body(node, diagnostics);
        check(&mut node.children, diagnostics);
    }
}

/// Whether the lines indented under `node` fit what it takes.
fn check_body(node: &mut Node, diagnostics: &mut Vec<Diagnostic>) {
    let first_child = node.body().next().map(|c| (c.span.line, c.source.clone()));
    let has_verbatim = node
        .children
        .iter()
        .any(|c| matches!(c.kind, NodeKind::Verbatim(_)));
    let (line, source) = (node.span.line, node.source.clone());
    // `@code` and `@textarea` (also at the end of a chain) take their text
    // on their line or as a verbatim block, like a verbatim directive
    if let NodeKind::Element(element) = &node.kind
        && element.text.is_some()
        && has_verbatim
        && let Some(head) = element.chain.last()
        && ast::has_literal_text(&head.name)
    {
        diagnostics.push(
            Diagnostic::error(
                code::UNEXPECTED_BODY,
                line,
                format!(
                    "@{} takes its text either on its own line or in an indented block, not both",
                    head.name
                ),
            )
            .source(source.clone()),
        );
    }
    let NodeKind::Directive(directive) = &mut node.kind else {
        return;
    };
    let name = directive.spec.name;
    let mut report = |code, (line, source): (usize, String), message: String| {
        diagnostics.push(Diagnostic::error(code, line, message).source(source))
    };
    let mut problem = |at, message| report(code::UNEXPECTED_BODY, at, message);
    match (&mut directive.args, directive.spec.body) {
        // Only a function takes a body, and it needs one
        (DirectiveArgs::Let(def), _) => {
            let what = match &def.form {
                LetForm::Function(_) => {
                    if first_child.is_none() {
                        report(
                            code::INVALID_DEFINITION,
                            (line, source),
                            format!(
                                "`@let @{}` needs an indented body: the lines the function \
                                 produces",
                                def.name
                            ),
                        );
                    }
                    return;
                }
                LetForm::Bundle(_) => "an attribute bundle",
                _ => "a value",
            };
            if let Some(child) = first_child {
                problem(
                    child,
                    format!(
                        "`@let {}` defines {}, which takes no indented block \
                         (a function is `@let @{} [param]` with a body)",
                        def.name, what, def.name
                    ),
                );
            }
        }
        (_, BodyKind::None) => {
            if let Some(child) = first_child {
                problem(
                    child,
                    format!(
                        "@{} takes no indented block: this line isn't part of it, so unindent it",
                        name
                    ),
                );
            }
        }
        (DirectiveArgs::Text(Some(_)), BodyKind::Verbatim) if has_verbatim => problem(
            (line, source),
            match name {
                "markdown" => "@markdown reads either a file named on its line or the Markdown \
                     in its indented block, not both"
                    .to_string(),
                _ => format!(
                    "@{} takes its content either on its own line or in an indented block, not both",
                    name
                ),
            },
        ),
        _ => {}
    }
}

/// The parameters in a function's list, `[title, tone #f9fafb]`: read
/// like a call's attributes, a bare word is a required parameter and
/// `name value` one with a default.
fn params(list: &AttrList, source: &str, problems: &mut Vec<Diagnostic>) -> Vec<Param> {
    let mut out: Vec<Param> = Vec::new();
    for attr in &list.attrs {
        let problem = |message: String| {
            let diagnostic = Diagnostic::error(code::INVALID_DEFINITION, attr.span.line, message)
                .source(source)
                .subject(attr.raw.clone());
            // The column, where the line shown is the parameter's own
            match attr.span.line == list.span.line {
                true => diagnostic.column(attr.span.column),
                false => diagnostic,
            }
        };
        let name = attr.key.strip_prefix('$').unwrap_or(&attr.key);
        if attr.key.starts_with('$') {
            problems.push(
                problem(format!(
                    "a parameter is named without `$`: `{}`, used as `${}` in the body",
                    name, name
                ))
                .subject(attr.key.clone())
                .suggest(Some(name)),
            );
        } else if attr.html {
            problems.push(problem(format!(
                "a parameter's default is written `{} VALUE`, without `=`",
                name
            )));
        } else if crate::interp::name_len(name) != name.len() || name.starts_with("--") {
            problems.push(problem(format!(
                "'{}' is not a parameter name: a parameter is a name, or a name and its \
                 default (`tone #f9fafb`)",
                attr.raw
            )));
            continue;
        }
        if out.iter().any(|p| p.name == name) {
            problems.push(problem(format!("parameter '{}' is declared twice", name)));
            continue;
        }
        let skip = attr.key.len() - name.len();
        out.push(Param {
            name: name.to_string(),
            default: attr.value.clone(),
            name_span: Span {
                start: attr.span.start + skip,
                end: attr.span.start + attr.key.len(),
                line: attr.span.line,
                column: attr.span.column + skip,
            },
            span: attr.span,
        });
    }
    // A default is filled in when the function is called, with the
    // parameters before it already bound: a later one isn't there yet.
    for (i, param) in out.iter().enumerate() {
        let Some(default) = &param.default else {
            continue;
        };
        for later in &out[i + 1..] {
            if crate::interp::names(default).contains(&later.name.as_str()) {
                let diagnostic = Diagnostic::error(
                    code::INVALID_DEFINITION,
                    param.span.line,
                    format!(
                        "the default of '{}' uses `${}`, a parameter declared after it: \
                         a default can use only the parameters before it, so declare '{}' \
                         first",
                        param.name, later.name, later.name
                    ),
                )
                .source(source)
                .subject(later.name.clone());
                problems.push(match param.span.line == list.span.line {
                    true => diagnostic.column(param.span.column),
                    false => diagnostic,
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Reading one logical line
// ---------------------------------------------------------------------------

/// Maps positions in a text to source spans.
trait Spans {
    fn span(&self, start: usize, end: usize) -> Span;
}

/// One logical line: physical lines joined with a space, and where each
/// piece came from.
#[derive(Default)]
struct Logical {
    text: String,
    /// (offset in `text`, offset in the source, length)
    pieces: Vec<(usize, usize, usize)>,
}

impl Logical {
    fn push(&mut self, piece: &str, source_offset: usize) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.pieces
            .push((self.text.len(), source_offset, piece.len()));
        self.text.push_str(piece);
    }

    fn source_offset(&self, pos: usize) -> usize {
        let i = self.pieces.partition_point(|p| p.0 <= pos).max(1) - 1;
        let (at, source, len) = self.pieces[i];
        source + (pos - at).min(len)
    }

    /// Parse the line: its kind, whether an attribute list (or inline JSON)
    /// is still open at its end, and its syntax errors.
    fn header(&self, index: &LineIndex) -> (NodeKind, bool, Vec<Diagnostic>) {
        let spans = LogicalSpans {
            logical: self,
            index,
        };
        let reader = Reader {
            text: &self.text,
            spans: &spans,
        };
        reader.line()
    }
}

struct LogicalSpans<'a> {
    logical: &'a Logical,
    index: &'a LineIndex,
}

impl Spans for LogicalSpans<'_> {
    fn span(&self, start: usize, end: usize) -> Span {
        let start_src = self.logical.source_offset(start);
        let end_src = self.logical.source_offset(end).max(start_src);
        self.index.span(start_src, end_src)
    }
}

/// Spans of a single-line text that starts at `base`.
struct Shifted {
    base: Span,
}

impl Spans for Shifted {
    fn span(&self, start: usize, end: usize) -> Span {
        Span {
            start: self.base.start + start,
            end: self.base.start + end,
            line: self.base.line,
            column: self.base.column + start,
        }
    }
}

/// The grammar of one logical line.
struct Reader<'a> {
    text: &'a str,
    spans: &'a dyn Spans,
}

/// The escapes, the same in every htmlang string (text, arguments,
/// attribute values, `@let`, `@page`, `@meta`): the character(s) after the
/// backslash stand for themselves. A backslash before anything else is
/// kept as written, so CSS's `\201C` and a pattern's `\d` pass through.
pub const ESCAPES: &[&str] = &[
    "\\\\", "\\$", "\\@", "\\{", "\\}", "\\[", "\\]", "\\,", "\\\"", "\\--",
];

/// The length of the escape `s` starts with, or 0.
pub fn escape_len(s: &str) -> usize {
    if !s.starts_with('\\') {
        return 0;
    }
    ESCAPES
        .iter()
        .find(|escape| s.starts_with(**escape))
        .map_or(0, |escape| escape.len())
}

/// `s` with its escapes replaced by what they stand for.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(found) = s[i..].find('\\') {
        let at = i + found;
        out.push_str(&s[i..at]);
        let len = escape_len(&s[at..]);
        if len == 0 {
            out.push('\\');
            i = at + 1;
        } else {
            out.push_str(&s[at + 1..at + len]);
            i = at + len;
        }
    }
    out.push_str(&s[i..]);
    out
}

/// The parts of `text` between the commas that aren't escaped (`\,`) or
/// inside `(...)`, `[...]`, `{...}` (so `${...}` too) or `"..."`: the items
/// of a list written in the source, or the attributes of a list.
pub fn split_list(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut parts = Vec::new();
    let mut from = 0;
    let mut depth = 0i32;
    let mut quoted = false;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let escape = escape_len(rest);
        if escape > 0 {
            i += escape;
            continue;
        }
        let Some(c) = rest.chars().next() else { break };
        match c {
            '"' => quoted = !quoted,
            _ if quoted => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth <= 0 => {
                parts.push(from..i);
                from = i + 1;
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    parts.push(from..text.len());
    parts
}

/// The text between the quotes when `s` is one quoted string, `"..."`, and
/// nothing else: `\"` inside it is not its end.
pub fn quoted_string(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('"')?;
    let mut i = 0;
    while i < inner.len() {
        let rest = &inner[i..];
        if rest.starts_with('"') {
            return (i + 1 == inner.len()).then(|| &inner[..i]);
        }
        i += match escape_len(rest) {
            0 => rest.chars().next().map_or(1, char::len_utf8),
            n => n,
        };
    }
    None
}

impl Reader<'_> {
    fn span(&self, start: usize, end: usize) -> Span {
        self.spans.span(start, end)
    }

    fn skip_ws(&self, mut pos: usize, limit: usize) -> usize {
        while pos < limit {
            match self.text[pos..].chars().next() {
                Some(c) if c.is_whitespace() => pos += c.len_utf8(),
                _ => break,
            }
        }
        pos
    }

    /// The end of a name starting at `pos`: the first whitespace or `[`.
    fn name_end(&self, pos: usize, limit: usize) -> usize {
        self.text[pos..limit]
            .find(|c: char| c.is_whitespace() || c == '[')
            .map_or(limit, |i| pos + i)
    }

    fn arg(&self, start: usize, end: usize) -> Option<Arg> {
        let piece = &self.text[start..end];
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            return None;
        }
        let s = start + (piece.len() - piece.trim_start().len());
        Some(Arg {
            raw: trimmed.to_string(),
            span: self.span(s, s + trimmed.len()),
        })
    }

    fn line(&self) -> (NodeKind, bool, Vec<Diagnostic>) {
        let mut problems = Vec::new();
        let (kind, open) = if self.text.starts_with('@') {
            let end = self.name_end(1, self.text.len());
            match ast::directive(&self.text[1..end]) {
                Some(spec) => {
                    let (args, open) = self.directive_args(spec, end, &mut problems);
                    let directive = Directive {
                        spec,
                        name_span: self.span(0, end),
                        args,
                    };
                    (NodeKind::Directive(directive), open)
                }
                None => {
                    let (line, open) = self.element_line();
                    (NodeKind::Element(line), open)
                }
            }
        } else {
            let (text, open) = self.text(0, self.text.len());
            (NodeKind::Text(text), open)
        };
        if open {
            // An unclosed quote keeps the list open to the end of the file
            let mut quotes = 0;
            let mut chars = self.text.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        chars.next();
                    }
                    '"' => quotes += 1,
                    _ => {}
                }
            }
            let message = if quotes % 2 == 1 {
                "unclosed '[' in attribute list: a `\"` in it isn't closed (write `\\\"` for a \
                 quote character)"
            } else {
                "unclosed '[' in attribute list"
            };
            problems.push(self.error(code::UNCLOSED_BRACKET, message.to_string()));
        }
        (kind, open, problems)
    }

    fn error(&self, code: &'static str, message: String) -> Diagnostic {
        Diagnostic::error(code, self.span(0, 0).line, message).source(self.text.to_string())
    }

    /// `@name [attrs] > @name [attrs] text`.
    fn element_line(&self) -> (ElementLine, bool) {
        let len = self.text.len();
        let mut chain = Vec::new();
        let mut pos = 0;
        loop {
            let (head, end) = self.head(pos, len);
            let unclosed = head.attrs.as_ref().is_some_and(|a| !a.closed);
            chain.push(head);
            if unclosed {
                return (ElementLine { chain, text: None }, true);
            }
            let after = self.skip_ws(end, len);
            if let Some(rest) = self.text[after..].strip_prefix('>') {
                let next = self.skip_ws(after + 1, len);
                if next > after + 1 && rest.trim_start().starts_with('@') {
                    pos = next;
                    continue;
                }
            }
            let (text, open) = if after < len {
                let (text, open) = self.text(after, len);
                (Some(text), open)
            } else {
                (None, false)
            };
            return (ElementLine { chain, text }, open);
        }
    }

    /// `@name [attrs]` at `pos`, and where it ends.
    fn head(&self, pos: usize, limit: usize) -> (Head, usize) {
        let name_end = self.name_end(pos + 1, limit);
        let name = self.text[pos + 1..name_end].to_string();
        let bracket = self.skip_ws(name_end, limit);
        let (attrs, end) = if self.text[bracket..limit].starts_with('[') {
            let list = self.attr_list(bracket, limit);
            let end = list.1;
            (Some(list.0), end)
        } else {
            (None, name_end)
        };
        let head = Head {
            name,
            name_span: self.span(pos, name_end),
            attrs,
            span: self.span(pos, end),
        };
        (head, end)
    }

    /// `[attr, attr]` starting at `open`, and where it ends. Brackets
    /// inside `"..."` and escaped ones (`\]`) don't count.
    fn attr_list(&self, open: usize, limit: usize) -> (AttrList, usize) {
        let mut depth = 0;
        let mut quoted = false;
        let mut close = None;
        let mut i = open;
        while i < limit {
            let rest = &self.text[i..limit];
            let escape = escape_len(rest);
            if escape > 0 {
                i += escape;
                continue;
            }
            let Some(c) = rest.chars().next() else { break };
            match c {
                '"' => quoted = !quoted,
                _ if quoted => {}
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
            i += c.len_utf8();
        }
        let inner_end = close.unwrap_or(limit);
        let attrs = self
            .split_commas(open + 1, inner_end)
            .into_iter()
            .filter_map(|(s, e)| self.attr(s, e))
            .collect();
        let end = close.map_or(limit, |c| c + 1);
        let list = AttrList {
            attrs,
            span: self.span(open, end),
            closed: close.is_some(),
        };
        (list, end)
    }

    /// Ranges between the commas that aren't escaped (`\,`) or inside
    /// `(...)`, `[...]`, `{...}` or `"..."`.
    fn split_commas(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        split_list(&self.text[start..end])
            .into_iter()
            .map(|part| (start + part.start, start + part.end))
            .collect()
    }

    fn attr(&self, start: usize, end: usize) -> Option<Attr> {
        let arg = self.arg(start, end)?;
        let (key, value, html) = split_attribute(&arg.raw);
        let piece = &self.text[start..end];
        let s = start + (piece.len() - piece.trim_start().len());
        let choice = self.choice(s, s + arg.raw.len()).map(Box::new);
        Some(Attr {
            key,
            value,
            html,
            raw: arg.raw,
            span: arg.span,
            choice,
        })
    }

    /// `if(CONDITION, A, B)` when it is all of `start..end`: the condition
    /// and the branches, each an attribute, a `[group]` (a branch that
    /// starts with `[`) or nothing.
    fn choice(&self, start: usize, end: usize) -> Option<Choice> {
        let text = &self.text[start..end];
        if !text.starts_with("if(") || self.closing_paren(start + 2, end)? + 1 != end {
            return None;
        }
        let mut parts = self.split_commas(start + 3, end - 1).into_iter();
        let (s, e) = parts.next()?;
        let condition = self.arg(s, e);
        let branches = parts
            .map(|(s, e)| {
                let at = self.skip_ws(s, e);
                if self.text[at..e].starts_with('[') {
                    let (list, after) = self.attr_list(at, e);
                    let trailing = self.arg(after, e);
                    Branch::Group { list, trailing }
                } else {
                    self.attr(s, e).map_or(Branch::Empty, Branch::Attr)
                }
            })
            .collect();
        Some(Choice {
            condition,
            branches,
        })
    }

    /// The `)` that closes the `(` at `open`, before `limit`. Parentheses
    /// inside `"..."` and escaped characters don't count.
    fn closing_paren(&self, open: usize, limit: usize) -> Option<usize> {
        let mut depth = 0;
        let mut quoted = false;
        let mut i = open;
        while i < limit {
            let rest = &self.text[i..limit];
            let escape = escape_len(rest);
            if escape > 0 {
                i += escape;
                continue;
            }
            let c = rest.chars().next()?;
            match c {
                '"' => quoted = !quoted,
                _ if quoted => {}
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += c.len_utf8();
        }
        None
    }

    /// Text from `start` to `end` (trimmed), and whether an inline
    /// element's attribute list is still open at its end.
    fn text(&self, start: usize, end: usize) -> (Text, bool) {
        let piece = &self.text[start..end];
        let s = start + (piece.len() - piece.trim_start().len());
        let e = s + piece.trim().len();
        let mut segments = Vec::new();
        let mut open = false;
        let mut plain = s;
        let mut i = s;
        let flush = |segments: &mut Vec<Segment>, from: usize, to: usize| {
            if to > from {
                segments.push(Segment::Plain {
                    raw: self.text[from..to].to_string(),
                    span: self.span(from, to),
                });
            }
        };
        while i < e {
            let rest = &self.text[i..e];
            if rest.starts_with('\\') {
                i += escape_len(rest).max(1);
                continue;
            }
            if rest.starts_with("{@") {
                flush(&mut segments, plain, i);
                let (inline, inline_open, next) = self.inline(i, e);
                open |= inline_open;
                segments.push(Segment::Inline(Box::new(inline)));
                i = next;
                plain = i;
                continue;
            }
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
        flush(&mut segments, plain, e);
        let text = Text {
            raw: self.text[s..e].to_string(),
            span: self.span(s, e),
            segments,
        };
        (text, open)
    }

    /// `{@name [attrs] text}` at `start`: the element, whether its
    /// attribute list is open at `limit`, and where it ends.
    fn inline(&self, start: usize, limit: usize) -> (Inline, bool, usize) {
        let mut depth = 0;
        let mut close = None;
        let mut i = start + 1;
        while i < limit {
            let rest = &self.text[i..limit];
            if rest.starts_with('\\') {
                i += escape_len(rest).max(1);
                continue;
            }
            match rest.chars().next() {
                Some('{') => depth += 1,
                Some('}') if depth == 0 => {
                    close = Some(i);
                    break;
                }
                Some('}') => depth -= 1,
                _ => {}
            }
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
        let inner_end = close.unwrap_or(limit);
        let (head, head_end) = self.head(start + 1, inner_end);
        let attrs_open = head.attrs.as_ref().is_some_and(|a| !a.closed);
        let after = self.skip_ws(head_end, inner_end);
        let (text, text_open) = if after < inner_end {
            let (text, open) = self.text(after, inner_end);
            (Some(text), open)
        } else {
            (None, false)
        };
        let end = close.map_or(limit, |c| c + 1);
        let inline = Inline {
            raw: self.text[start + 1..inner_end].to_string(),
            span: self.span(start, end),
            head,
            text,
            closed: close.is_some(),
        };
        (inline, close.is_none() && (attrs_open || text_open), end)
    }

    /// A directive's arguments, by its grammar, and whether they continue
    /// onto the next line.
    fn directive_args(
        &self,
        spec: &'static DirectiveSpec,
        name_end: usize,
        problems: &mut Vec<Diagnostic>,
    ) -> (DirectiveArgs, bool) {
        let len = self.text.len();
        let name = spec.name;
        // A verbatim directive takes no attributes: `[` right after its name
        // opens a list, as after any name, and the list is an error
        let mut name_end = name_end;
        let bracket = self.skip_ws(name_end, len);
        if spec.body == BodyKind::Verbatim && self.text[bracket..].starts_with('[') {
            let (list, end) = self.attr_list(bracket, len);
            let reason = match name {
                "raw" => "its HTML goes into the page as written, so put them in the HTML",
                "head" => "its HTML goes into the <head> as written, so put them in the HTML",
                "markdown" => {
                    "to style the Markdown, put @markdown in an element that has them \
                     (`@el [padding 8]` with @markdown indented under it)"
                }
                _ => "its CSS goes into the page as written",
            };
            let hint = match name {
                "markdown" => String::new(),
                _ => format!(
                    ". Content that starts with `[` goes in an indented block under @{}",
                    name
                ),
            };
            problems.push(
                self.error(
                    code::UNEXPECTED_ARGUMENT,
                    format!("@{} takes no attributes: {}{}", name, reason, hint),
                )
                .subject(self.text[bracket..end].to_string())
                .column(list.span.column),
            );
            if !list.closed {
                return (DirectiveArgs::Invalid, true);
            }
            name_end = end;
        }
        let rest = self.text[name_end..].trim();
        let mut missing = |what: &str| {
            problems.push(self.error(code::MISSING_ARGUMENT, format!("@{} needs {}", name, what)));
            (DirectiveArgs::Invalid, false)
        };
        match spec.args {
            ArgGrammar::Text => match self.arg(name_end, len) {
                // Without a body, the line is all the directive has
                None if spec.body == BodyKind::None => missing(match name {
                    "include" => "a file: `@include header.hl`",
                    "meta" => "a name and a value: `@meta description A small site`",
                    _ => "an argument",
                }),
                arg => (DirectiveArgs::Text(arg), false),
            },
            ArgGrammar::AttrsText => {
                let bracket = self.skip_ws(name_end, len);
                let (attrs, after) = if self.text[bracket..].starts_with('[') {
                    let (list, end) = self.attr_list(bracket, len);
                    (Some(list), end)
                } else {
                    (None, name_end)
                };
                let open = attrs.as_ref().is_some_and(|a| !a.closed);
                let title = if open { None } else { self.arg(after, len) };
                (DirectiveArgs::Page { attrs, title }, open)
            }
            ArgGrammar::Expression => match self.arg(name_end, len) {
                Some(condition) => (DirectiveArgs::Condition(condition), false),
                None => missing("a condition"),
            },
            ArgGrammar::Else => {
                if rest.is_empty() {
                    return (DirectiveArgs::Else { condition: None }, false);
                }
                let at = self.skip_ws(name_end, len);
                let after_if = at + 2;
                let is_if = self.text[at..].starts_with("if")
                    && self.text[after_if..]
                        .chars()
                        .next()
                        .is_none_or(char::is_whitespace);
                if !is_if {
                    problems.push(
                        self.error(
                            code::UNEXPECTED_ARGUMENT,
                            "@else takes nothing on its line (a condition is `@else if COND`)"
                                .to_string(),
                        ),
                    );
                    return (DirectiveArgs::Else { condition: None }, false);
                }
                match self.arg(after_if, len) {
                    Some(condition) => (
                        DirectiveArgs::Else {
                            condition: Some(condition),
                        },
                        false,
                    ),
                    None => {
                        problems.push(self.error(
                            code::MISSING_ARGUMENT,
                            "@else if needs a condition".to_string(),
                        ));
                        (
                            DirectiveArgs::Else {
                                condition: Some(Arg {
                                    raw: String::new(),
                                    span: self.span(after_if, after_if),
                                }),
                            },
                            false,
                        )
                    }
                }
            }
            ArgGrammar::Loop => {
                let Some((names, list)) = rest.split_once(" in ") else {
                    problems.push(self.error(
                        code::INVALID_LOOP,
                        "@each requires: @each $item in LIST".to_string(),
                    ));
                    return (DirectiveArgs::Invalid, false);
                };
                let written: Vec<&str> = names.split(',').map(str::trim).collect();
                // Like `@data $name`, the variables are written with `$`
                if let Some(bare) = written.iter().find(|n| !n.starts_with('$') || n.len() == 1) {
                    let fixed: Vec<String> = written
                        .iter()
                        .enumerate()
                        .map(|(i, n)| match n.trim_start_matches('$') {
                            "" if i == 0 => "$item".to_string(),
                            "" => "$index".to_string(),
                            name => format!("${}", name),
                        })
                        .collect();
                    let problem = self.error(
                        code::INVALID_LOOP,
                        format!(
                            "@each names its variables with `$`: `@each {} in LIST`",
                            fixed.join(", ")
                        ),
                    );
                    // A name without its `$` gets it; a missing name has
                    // nothing to replace
                    let problem = match bare.trim_start_matches('$').is_empty() {
                        true => problem,
                        false => problem.subject(*bare).suggest(Some(format!("${}", bare))),
                    };
                    problems.push(problem);
                }
                let names: Vec<&str> = written
                    .iter()
                    .map(|v| v.strip_prefix('$').unwrap_or(v))
                    .collect();
                if names.len() > 2 {
                    problems.push(
                        self.error(
                            code::INVALID_LOOP,
                            "@each takes `$item` or `$item, $index`: to loop over records, \
                         load them with @data and use `$item.key`"
                                .to_string(),
                        ),
                    );
                    return (DirectiveArgs::Invalid, false);
                }
                let list_start = len - list.trim_start().len();
                let list = Arg {
                    raw: list.trim().to_string(),
                    span: self.span(list_start, len),
                };
                let each = Loop {
                    item: names[0].to_string(),
                    index: names.get(1).map(|s| s.to_string()),
                    list,
                };
                (DirectiveArgs::Each(each), false)
            }
            ArgGrammar::Data => {
                let at = self.skip_ws(name_end, len);
                let named = self.text[at..].strip_prefix('$').and_then(|named| {
                    let (name, source) = named.split_once(char::is_whitespace)?;
                    Some((name, source.trim()))
                });
                let Some((data_name, source)) = named.filter(|(_, source)| !source.is_empty())
                else {
                    problems.push(self.error(
                        code::MISSING_ARGUMENT,
                        format!(
                            "@data needs a name: write `@data $name {}` and use `$name.key`",
                            rest.trim_start_matches('$')
                        ),
                    ));
                    return (DirectiveArgs::Invalid, false);
                };
                let name_start = at + 1;
                let source_start = len - source.len();
                let open = source.starts_with(['[', '{']) && !json_balanced(source);
                let args = DirectiveArgs::Data {
                    name: data_name.to_string(),
                    name_span: self.span(name_start, name_start + data_name.len()),
                    source: Arg {
                        raw: source.to_string(),
                        span: self.span(source_start, len),
                    },
                };
                (args, open)
            }
            ArgGrammar::Definition => {
                let at = self.skip_ws(name_end, len);
                if at >= len {
                    return missing("a name: `@let name value`");
                }
                // `@let @name`: a function
                let function = self.text[at..].starts_with('@');
                let name_at = at + usize::from(function);
                let def_name_end = self.name_end(name_at, len);
                let written = &self.text[name_at..def_name_end];
                let def_name = written.strip_prefix('$').unwrap_or(written).to_string();
                let name_span = self.span(name_at + written.len() - def_name.len(), def_name_end);
                let invalid = |message: String| {
                    self.error(code::INVALID_DEFINITION, message)
                        .subject(written.to_string())
                };
                if function {
                    let valid = def_name.chars().next().is_some_and(char::is_alphabetic)
                        && def_name
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '-' || c == '_');
                    if def_name.is_empty() {
                        return missing("a function name: `@let @name [param]`");
                    }
                    if !valid || written.starts_with('$') {
                        problems.push(invalid(format!(
                            "'{}' is not a function name: a function is named with letters, \
                             digits, `-` and `_`, starting with a letter, as in `@let @card`",
                            written
                        )));
                    } else if let Some(message) = shadowed(&def_name) {
                        let mut warning = self
                            .error(code::SHADOWS_BUILT_IN, message)
                            .subject(def_name.clone())
                            .column(name_span.column);
                        warning.severity = Severity::Warning;
                        problems.push(warning);
                    }
                } else if written.starts_with('$') {
                    problems.push(
                        invalid(format!(
                            "`@let` names are written without `$`: `@let {}`, used as `${}`",
                            def_name, def_name
                        ))
                        .suggest(Some(def_name.clone())),
                    );
                } else if !is_definition_name(&def_name) {
                    problems.push(invalid(format!(
                        "'{}' is not a name: a @let name is letters, digits, `-` and `_`, \
                         starting with a letter (`@let gap 8`)",
                        written
                    )));
                }
                let value_at = self.skip_ws(def_name_end, len);
                let value = &self.text[value_at..];
                let mut open = false;
                // Text after a closed list's `]`
                let mut after_list = |end: usize, what: &str| {
                    let rest = self.text[end..].trim();
                    if !rest.is_empty() {
                        problems.push(self.error(
                            code::UNEXPECTED_ARGUMENT,
                            format!("unexpected '{}' after the {}", rest, what),
                        ));
                    }
                };
                let form = if function {
                    let list = if value.starts_with('[') {
                        let (list, end) = self.attr_list(value_at, len);
                        open = !list.closed;
                        if list.closed {
                            after_list(end, "parameter list");
                        }
                        Some(list)
                    } else {
                        if !value.is_empty() {
                            problems.push(self.error(
                                code::INVALID_DEFINITION,
                                format!(
                                    "a function's parameters go in brackets: \
                                     `@let @{} [param, param default]`",
                                    def_name
                                ),
                            ));
                        }
                        None
                    };
                    let params = match &list {
                        Some(list) => params(list, self.text, problems),
                        None => Vec::new(),
                    };
                    LetForm::Function(Function { list, params })
                } else if value.is_empty() {
                    problems.push(self.error(
                        code::MISSING_ARGUMENT,
                        format!(
                            "`@let {}` needs a value: `@let {} VALUE` \
                             (a function is `@let @{}` with an indented body)",
                            def_name, def_name, def_name
                        ),
                    ));
                    return (DirectiveArgs::Invalid, false);
                } else if let Some(expr) = value.strip_prefix('=') {
                    let expr_at = len - expr.trim_start().len();
                    LetForm::Computed(Arg {
                        raw: expr.trim().to_string(),
                        span: self.span(expr_at, len),
                    })
                } else if value.starts_with('[') {
                    let (list, end) = self.attr_list(value_at, len);
                    open = !list.closed;
                    if list.closed {
                        after_list(end, "attribute bundle");
                    }
                    LetForm::Bundle(list)
                } else if let Some(inner) = quoted_string(value) {
                    LetForm::Quoted(Arg {
                        raw: inner.to_string(),
                        span: self.span(value_at + 1, len - 1),
                    })
                } else {
                    match self.arg(value_at, len) {
                        Some(arg) => LetForm::Value(arg),
                        None => return (DirectiveArgs::Invalid, false),
                    }
                };
                let def = LetDef {
                    name: def_name,
                    name_span,
                    form,
                };
                (DirectiveArgs::Let(def), open)
            }
        }
    }
}

/// Why a function can't be named `name`: it is the name of a built-in
/// element, which the function would replace everywhere after it, or of a
/// directive, which it could never replace.
fn shadowed(name: &str) -> Option<String> {
    if ast::ElementKind::from_name(name).is_some() {
        Some(format!(
            "@let @{0} replaces the built-in element @{0}: every @{0} after this line \
             calls the function. Give the function another name",
            name
        ))
    } else if ast::directive(name).is_some() {
        Some(format!(
            "@{0} is a directive, so a function named @{0} can never be called: give it \
             another name",
            name
        ))
    } else {
        None
    }
}

/// Whether `name` can be a value's or bundle's name, as `@let name`: a
/// name `$name` reaches (`t.greeting` too, for a record's field), or a
/// custom property `--name`.
fn is_definition_name(name: &str) -> bool {
    let n = crate::interp::name_len(name);
    if n == 0 || matches!(&name[..n], "true" | "false" | "not" | "and" | "or") {
        return false;
    }
    if name.starts_with("--") {
        return n == name.len();
    }
    name[n..].split('.').skip(1).all(|field| {
        !field.is_empty()
            && field
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    }) && (n == name.len() || name[n..].starts_with('.'))
}

/// An attribute's key, value, and whether it is written `key=value`:
/// `padding 20`, `type=email`, `required`.
pub(crate) fn split_attribute(raw: &str) -> (String, Option<String>, bool) {
    if let Some((key, value)) = split_html_attribute(raw) {
        return (key.to_string(), Some(value.to_string()), true);
    }
    match raw.split_once(' ') {
        Some((key, value)) => (
            key.trim().to_string(),
            Some(value.trim().to_string()),
            false,
        ),
        None => (raw.to_string(), None, false),
    }
}

/// Split an HTML attribute written `key=value` (`alt=`, `type=email`,
/// `aria-label=Close menu`). Returns `None` for style attributes.
pub(crate) fn split_html_attribute(part: &str) -> Option<(&str, &str)> {
    let first_token = part.split(char::is_whitespace).next()?;
    let eq = first_token.find('=')?;
    let key = &part[..eq];
    let valid_key = key.starts_with(|c: char| c.is_ascii_alphabetic())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    valid_key.then(|| (key, part[eq + 1..].trim()))
}

/// Whether the brackets and braces of inline JSON are balanced.
fn json_balanced(json: &str) -> bool {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for c in json.chars() {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth <= 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(nodes: &[Node]) -> Vec<&'static str> {
        nodes
            .iter()
            .map(|n| match &n.kind {
                NodeKind::Blank => "blank",
                NodeKind::Comment => "comment",
                NodeKind::Text(_) => "text",
                NodeKind::Element(_) => "element",
                NodeKind::Directive(d) => d.name(),
                NodeKind::Verbatim(_) => "verbatim",
            })
            .collect()
    }

    fn codes(tree: &Tree) -> Vec<&'static str> {
        tree.diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn lines_become_nodes_with_trivia() {
        let tree = parse("-- intro\n@el [padding 4]\n  Hello\n\n  @text x\n@text y\n");
        assert_eq!(kinds(&tree.nodes), ["comment", "element", "element"]);
        assert_eq!(kinds(&tree.nodes[1].children), ["text", "blank", "element"]);
        assert!(tree.diagnostics.is_empty());
    }

    #[test]
    fn heads_attributes_and_spans() {
        let src = "@row [spacing 8, id=main] > @link [color red] /home Home\n";
        let tree = parse(src);
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        let names: Vec<_> = line.chain.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["row", "link"]);
        let attrs = &line.chain[0].attrs.as_ref().unwrap().attrs;
        assert_eq!(attrs[0].key, "spacing");
        assert_eq!(attrs[0].value.as_deref(), Some("8"));
        assert!(attrs[1].html);
        assert_eq!(&src[attrs[1].span.start..attrs[1].span.end], "id=main");
        assert_eq!(attrs[1].span.column, 17);
        assert_eq!(line.text.as_ref().unwrap().raw, "/home Home");
    }

    #[test]
    fn a_bracket_opens_a_list_only_after_a_head() {
        let tree = parse("@text Use [ to open\n@text next\n");
        assert_eq!(kinds(&tree.nodes), ["element", "element"]);
        assert!(tree.diagnostics.is_empty());
        let tree = parse("@el [\n  padding 4,\n  -- note\n  color red\n]\n  child\n");
        assert_eq!(kinds(&tree.nodes), ["element"]);
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        let list = line.chain[0].attrs.as_ref().unwrap();
        assert_eq!(list.attrs.len(), 2);
        assert_eq!(list.attrs[1].span.line, 4);
        assert_eq!(kinds(&tree.nodes[0].children), ["text"]);
    }

    #[test]
    fn a_whole_attribute_if_is_read_into_its_branches() {
        let src = "@el [if($a == \"x, y\", [padding 4, if($b, gap 1)], $card), width if(media(print): 1px), if(c, )]\n";
        let tree = parse(src);
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        let attrs = &line.chain[0].attrs.as_ref().unwrap().attrs;
        assert_eq!(attrs.len(), 3);
        let choice = attrs[0].choice.as_ref().expect("an if()");
        let condition = choice.condition.as_ref().unwrap();
        assert_eq!(condition.raw, "$a == \"x, y\"");
        assert_eq!(
            &src[condition.span.start..condition.span.end],
            condition.raw
        );
        let Branch::Group { list, trailing } = &choice.branches[0] else {
            panic!("{:?}", choice.branches[0])
        };
        assert!(trailing.is_none());
        assert_eq!(list.attrs.len(), 2);
        assert_eq!(list.attrs[0].key, "padding");
        assert_eq!(
            &src[list.attrs[0].span.start..list.attrs[0].span.end],
            "padding 4"
        );
        assert!(list.attrs[1].choice.is_some(), "an if() inside a group");
        assert!(matches!(&choice.branches[1], Branch::Attr(a) if a.key == "$card"));
        // An if() inside a value is CSS's; an empty branch is empty
        assert!(attrs[1].choice.is_none());
        let empty = attrs[2].choice.as_ref().unwrap();
        assert!(matches!(empty.branches[..], [Branch::Empty]));
        // A group spanning lines keeps each attribute's line
        let tree = parse("@el [if($on, [\n  padding 4,\n  margin 2\n])]\n");
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        let choice = line.chain[0].attrs.as_ref().unwrap().attrs[0]
            .choice
            .clone()
            .unwrap();
        let group = choice.branches[0].attrs();
        assert_eq!((group[0].span.line, group[0].span.column), (2, 2));
        assert_eq!((group[1].span.line, group[1].span.column), (3, 2));
    }

    #[test]
    fn a_chain_needs_complete_heads() {
        let tree = parse("@text Ask me > @support\n");
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        assert_eq!(line.chain.len(), 1);
        assert_eq!(line.text.as_ref().unwrap().raw, "Ask me > @support");
    }

    #[test]
    fn escapes_are_one_closed_table() {
        assert_eq!(
            unescape(r#"\\ \$ \@ \{ \} \[ \] \, \" \-- \201C \d \"#),
            r#"\ $ @ { } [ ] , " -- \201C \d \"#
        );
        assert_eq!(quoted_string(r#""a\"b""#), Some(r#"a\"b"#));
        assert_eq!(quoted_string(r#""head head" "side main""#), None);
        assert_eq!(quoted_string(r#""a"#), None);
        assert_eq!(quoted_string(r#""""#), Some(""));
    }

    #[test]
    fn escaped_commas_and_brackets_stay_in_the_value() {
        let tree = parse("@el [transition opacity 1s\\, color 1s, content \"]\", width 4\\]] x\n");
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        let attrs = &line.chain[0].attrs.as_ref().unwrap().attrs;
        let raw: Vec<&str> = attrs.iter().map(|a| a.raw.as_str()).collect();
        assert_eq!(
            raw,
            [
                r"transition opacity 1s\, color 1s",
                r#"content "]""#,
                r"width 4\]"
            ]
        );
        assert_eq!(line.text.as_ref().unwrap().raw, "x");
    }

    #[test]
    fn a_let_is_quoted_only_when_it_is_one_string() {
        let tree = parse("@let a \"x \\\" y\"\n@let b \"h h\" \"s m\"\n");
        let forms: Vec<&LetForm> = tree
            .nodes
            .iter()
            .map(|n| match &n.directive().unwrap().args {
                DirectiveArgs::Let(def) => &def.form,
                _ => panic!(),
            })
            .collect();
        assert!(matches!(forms[0], LetForm::Quoted(arg) if arg.raw == r#"x \" y"#));
        assert!(matches!(forms[1], LetForm::Value(arg) if arg.raw == r#""h h" "s m""#));
    }

    #[test]
    fn inline_elements_and_escapes() {
        let tree = parse("Read {@link /docs the {@b docs}} \\{@not} now\n");
        let NodeKind::Text(text) = &tree.nodes[0].kind else {
            panic!()
        };
        assert_eq!(text.segments.len(), 3);
        let Segment::Inline(inline) = &text.segments[1] else {
            panic!()
        };
        assert_eq!(inline.head.name, "link");
        assert_eq!(inline.text.as_ref().unwrap().segments.len(), 2);
    }

    #[test]
    fn the_text_of_code_and_textarea_has_no_inline_elements() {
        let plain = |text: &Text| {
            text.segments.len() == 1 && matches!(text.segments[0], Segment::Plain { .. })
        };
        // On the element's line and in a chain
        let tree = parse("@code {@link /x y}\n@pre > @code {@b x}\n");
        for node in &tree.nodes[..2] {
            let NodeKind::Element(line) = &node.kind else {
                panic!()
            };
            assert!(plain(line.text.as_ref().unwrap()), "{:?}", line.text);
        }
        // The lines under it are a verbatim body, shown as text, also at
        // the end of a chain; @pre's own lines are htmlang
        let tree = parse("@textarea\n  {@b x}\n@pre > @code\n  @b y\n    -- z\n@pre\n  @b w\n");
        for (node, text) in [(0, "{@b x}"), (1, "@b y\n  -- z")] {
            let NodeKind::Verbatim(body) = &tree.nodes[node].children[0].kind else {
                panic!("{:?}", tree.nodes[node].children)
            };
            assert!(body.escaped);
            assert_eq!(body.text, text);
        }
        assert!(matches!(
            tree.nodes[2].children[0].kind,
            NodeKind::Element(_)
        ));
        assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
        // Its line and a body together are an error
        assert_eq!(codes(&parse("@code x\n  y\n")), [code::UNEXPECTED_BODY]);
        assert_eq!(
            codes(&parse("@pre > @code x\n  y\n")),
            [code::UNEXPECTED_BODY]
        );
        // Inline: `{@code ...}`'s text is literal, the line around it isn't
        let tree = parse("@h2 Write {@code {@link /x y}} or {@b {@code {@i z}}}\n");
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!()
        };
        let heads: Vec<&str> = tree.nodes[0]
            .heads()
            .iter()
            .map(|h| h.name.as_str())
            .collect();
        assert_eq!(heads, ["h2", "code", "b", "code"]);
        let Segment::Inline(code) = &line.text.as_ref().unwrap().segments[1] else {
            panic!()
        };
        assert!(plain(code.text.as_ref().unwrap()));
        assert_eq!(code.text.as_ref().unwrap().raw, "{@link /x y}");
        // Any other element's text is parsed
        let tree = parse("@h1 Hello {@text [color red] world}\n");
        assert_eq!(tree.nodes[0].heads().len(), 2);
    }

    #[test]
    fn inline_attribute_lists_continue() {
        let tree = parse("Press {@kbd [font-weight bold,\n  color red] K} to go\n@text x\n");
        assert_eq!(kinds(&tree.nodes), ["text", "element"]);
        assert!(tree.diagnostics.is_empty());
    }

    #[test]
    fn directives_follow_the_table() {
        let tree = parse(
            "@let @card [title, icon x]\n  @el\n    @children\n@if $a\n  A\n@else if $b\n  B\n@else\n  C\n@each $x in 1..3\n  $x\n@else\n  none\n",
        );
        assert_eq!(
            kinds(&tree.nodes),
            ["let", "if", "else", "else", "each", "else"]
        );
        let defs = tree.definitions();
        assert_eq!(defs[0].kind, DefinitionKind::Function);
        assert_eq!(defs[0].params[1].default.as_deref(), Some("x"));
        assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
    }

    #[test]
    fn names_are_visible_to_the_end_of_their_block() {
        let tree = parse(
            "@let a 1\n@let @card [title]\n  @let inner 2\n  @el $title\n@if $a\n  @let b [padding 4]\n  @el x\n@each $x, $i in 1, 2\n  @text $x\n@data $d {}\n@let t.greeting hi\n@el end\n",
        );
        let names = |line| {
            tree.visible_at(line)
                .iter()
                .map(|v| v.name.to_string())
                .collect::<Vec<_>>()
        };
        // In a function's body: its parameters, itself, and what is above it
        assert_eq!(names(4), ["a", "card", "title", "inner"]);
        assert_eq!(names(7), ["a", "card", "b"]);
        assert_eq!(names(9), ["a", "card", "x", "i"]);
        assert_eq!(names(12), ["a", "card", "d", "t"]);
        assert_eq!(names(1), Vec::<String>::new());
        let kinds: Vec<VisibleKind> = tree.visible_at(9).iter().map(|v| v.kind).collect();
        assert_eq!(kinds[2], VisibleKind::Loop);
    }

    #[test]
    fn a_let_says_its_kind_on_its_line() {
        let tree = parse(
            "@let a 1\n@let b [padding 4]\n@let @c [p, q two words]\n  @el\n@let @d\n  @el\n@let @e [\n  x,\n  y 1\n]\n  @el\n",
        );
        assert!(tree.diagnostics.is_empty(), "{:?}", tree.diagnostics);
        let defs = tree.definitions();
        let kinds: Vec<_> = defs.iter().map(|d| (d.name, d.kind)).collect();
        assert_eq!(
            kinds,
            [
                ("a", DefinitionKind::Value),
                ("b", DefinitionKind::Bundle),
                ("c", DefinitionKind::Function),
                ("d", DefinitionKind::Function),
                ("e", DefinitionKind::Function),
            ]
        );
        // A bare word is required; `name value` has a default, spaces and all
        let c = &defs[2];
        assert_eq!(
            (c.params[0].name.as_str(), c.params[0].default.as_deref()),
            ("p", None)
        );
        assert_eq!(c.params[1].default.as_deref(), Some("two words"));
        // The name's span leaves out the `@`
        assert_eq!((c.name_span.column, c.params[1].name_span.column), (6, 12));
        assert!(defs[3].params.is_empty());
        let e: Vec<_> = defs[4]
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.name_span.line))
            .collect();
        assert_eq!(e, [("x", 8), ("y", 9)]);
    }

    #[test]
    fn verbatim_bodies() {
        let tree = parse("@style\n  :root {\n    --brand: red;\n  }\n\n@raw <hr>\n@text x\n");
        assert_eq!(kinds(&tree.nodes), ["style", "blank", "raw", "element"]);
        let [child] = tree.nodes[0].children.as_slice() else {
            panic!()
        };
        let NodeKind::Verbatim(body) = &child.kind else {
            panic!()
        };
        assert_eq!(body.text, ":root {\n  --brand: red;\n}");
    }

    #[test]
    fn multi_line_inline_json() {
        let tree = parse("@data $x {\n  \"a\": [1, 2],\n  \"b\": \"]\"\n}\n@text $x.a\n");
        assert_eq!(kinds(&tree.nodes), ["data", "element"]);
        let Some(Directive {
            args: DirectiveArgs::Data { source, .. },
            ..
        }) = tree.nodes[0].directive()
        else {
            panic!()
        };
        assert!(source.raw.ends_with('}'));
    }

    #[test]
    fn the_leading_token_keeps_quotes_and_expressions_whole() {
        fn token(s: &str) -> &str {
            &s[..leading_token_len(s)]
        }
        assert_eq!(token("/about About us"), "/about");
        assert_eq!(token("$url More"), "$url");
        assert_eq!(token("/page/${$n + 1} Next"), "/page/${$n + 1}");
        assert_eq!(token("${ {\"a\": 1} } x"), "${ {\"a\": 1} }");
        assert_eq!(token("\"my page.html\" Open"), "\"my page.html\"");
        assert_eq!(token("\"a \\\" b\" c"), "\"a \\\" b\"");
        assert_eq!(token("a\\,b c"), "a\\,b");
        assert_eq!(token("${unclosed x"), "${unclosed");
        assert_eq!(token("\"unclosed x"), "\"unclosed x");
        assert_eq!(token("single"), "single");

        let tree = parse("@link /page/${$n + 1} Next  page\n");
        let NodeKind::Element(line) = &tree.nodes[0].kind else {
            panic!("an element")
        };
        let (token, rest) = line.text.as_ref().unwrap().split_leading();
        assert_eq!(token.raw, "/page/${$n + 1}");
        assert_eq!(token.span.column, 6);
        let rest = rest.unwrap();
        assert_eq!(rest.raw, "Next  page");
        assert_eq!(rest.span.column, 22);
    }

    #[test]
    fn body_kinds_are_checked() {
        let tree = parse("@page Home\n  @text x\n@meta a b\n-- fine\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        assert_eq!(tree.diagnostics[0].line, 2);
        let tree = parse("@let x = 1\n  @text y\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        let tree = parse("@raw <hr>\n  <br>\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        let tree = parse("@style x\n  a {}\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
    }

    #[test]
    fn verbatim_directives_take_their_line_and_no_attributes() {
        // The rest of the line is the one-line body (or @markdown's file)
        for src in [
            "@style .a { color: red }\n",
            "@head <meta name=x>\n",
            "@raw <hr>\n",
            "@markdown notes.md\n",
        ] {
            let tree = parse(src);
            assert!(
                tree.diagnostics.is_empty(),
                "{}: {:?}",
                src,
                tree.diagnostics
            );
            let Some(DirectiveArgs::Text(Some(arg))) = tree.nodes[0].directive().map(|d| &d.args)
            else {
                panic!("{}", src)
            };
            assert_eq!(arg.raw, src.split_once(' ').unwrap().1.trim_end());
        }
        // `[` right after the name is an attribute list, which is an error;
        // the text after it is still the line's content
        for name in ["style", "head", "raw", "markdown"] {
            let tree = parse(&format!("@{} [id=x] rest\n", name));
            assert_eq!(codes(&tree), [code::UNEXPECTED_ARGUMENT], "@{}", name);
            assert_eq!(tree.diagnostics[0].column, Some(name.len() + 2));
            let Some(DirectiveArgs::Text(Some(arg))) = tree.nodes[0].directive().map(|d| &d.args)
            else {
                panic!("@{}", name)
            };
            assert_eq!(arg.raw, "rest");
        }
        // A line and a block together: @markdown's line is a file
        let tree = parse("@markdown notes.md\n  # Title\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        assert!(
            tree.diagnostics[0]
                .message
                .contains("a file named on its line"),
            "{}",
            tree.diagnostics[0].message
        );
        // In the indented block, `[` is content
        assert!(
            parse("@style\n  [hidden] { display: none }\n")
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn syntax_errors() {
        assert_eq!(codes(&parse("@else\n  x\n")), [code::STRAY_ELSE]);
        assert_eq!(codes(&parse("@if\n  x\n")), [code::MISSING_ARGUMENT]);
        assert_eq!(codes(&parse("@each $x\n  x\n")), [code::INVALID_LOOP]);
        assert_eq!(
            codes(&parse("@el [padding 4\n@text x\n")),
            [code::UNCLOSED_BRACKET]
        );
        assert_eq!(codes(&parse("@data x.json\n")), [code::MISSING_ARGUMENT]);
    }

    #[test]
    fn node_lookup_and_extent() {
        let tree = parse("@el [\n  padding 4\n]\n  @text a\n\n@text b\n");
        assert_eq!(tree.nodes[0].end_line(), 4);
        assert_eq!(tree.node_at_line(2).map(|n| n.span.line), Some(1));
        assert_eq!(tree.node_at_line(4).map(|n| n.span.line), Some(4));
    }
}
