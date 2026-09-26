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
use crate::diagnostic::{Diagnostic, code};

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
    None,
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

    /// Split off the first word (a URL), with the rest as text:
    /// `@link /about About us`.
    pub fn split_first_word(&self) -> (String, Option<Text>) {
        let Some(space) = self.raw.find(char::is_whitespace) else {
            return (self.raw.clone(), None);
        };
        let word = self.raw[..space].to_string();
        let rest_start = self.raw.len() - self.raw[space..].trim_start().len();
        if rest_start >= self.raw.len() {
            return (word, None);
        }
        let shifted = Shifted { base: self.span };
        let reader = Reader {
            text: &self.raw,
            spans: &shifted,
        };
        (word, Some(reader.text(rest_start, self.raw.len()).0))
    }
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
        let body = match &kind {
            NodeKind::Directive(d) => d.spec.body,
            NodeKind::Element(line) => line
                .chain
                .last()
                .map_or(BodyKind::Htmlang, |h| ast::body_kind(&h.name)),
            _ => BodyKind::Htmlang,
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
            format!(
                "@{} takes its content either on its own line or in an indented block, not both",
                name
            ),
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
            Diagnostic::error(code::INVALID_DEFINITION, attr.span.line, message)
                .source(source)
                .subject(attr.raw.clone())
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
            problems.push(self.error(
                code::UNCLOSED_BRACKET,
                "unclosed '[' in attribute list".to_string(),
            ));
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
        let mut parts = Vec::new();
        let mut from = start;
        let mut depth = 0i32;
        let mut quoted = false;
        let mut i = start;
        while i < end {
            let rest = &self.text[i..end];
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
                    parts.push((from, i));
                    from = i + 1;
                }
                _ => {}
            }
            i += c.len_utf8();
        }
        parts.push((from, end));
        parts
    }

    fn attr(&self, start: usize, end: usize) -> Option<Attr> {
        let arg = self.arg(start, end)?;
        let (key, value, html) = split_attribute(&arg.raw);
        Some(Attr {
            key,
            value,
            html,
            raw: arg.raw,
            span: arg.span,
        })
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
        let rest = self.text[name_end..].trim();
        let name = spec.name;
        let mut missing = |what: &str| {
            problems.push(self.error(code::MISSING_ARGUMENT, format!("@{} needs {}", name, what)));
            (DirectiveArgs::Invalid, false)
        };
        match spec.args {
            ArgGrammar::None => {
                if !rest.is_empty() {
                    problems.push(self.error(
                        code::UNEXPECTED_ARGUMENT,
                        format!(
                            "@{} takes nothing on its line: its content is the indented block under it",
                            name
                        ),
                    ));
                }
                (DirectiveArgs::None, false)
            }
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
                    let problem = self.error(
                        code::INVALID_LOOP,
                        format!(
                            "@each names its variables with `$`: `@each ${} in LIST`",
                            if bare.is_empty() { "item" } else { bare }
                        ),
                    );
                    let problem = match bare.is_empty() {
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
                             digits and `-`, as in `@let @card`",
                            written
                        )));
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
    fn body_kinds_are_checked() {
        let tree = parse("@page Home\n  @text x\n@meta a b\n-- fine\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        assert_eq!(tree.diagnostics[0].line, 2);
        let tree = parse("@let x = 1\n  @text y\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        let tree = parse("@raw <hr>\n  <br>\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_BODY]);
        let tree = parse("@style x\n  a {}\n");
        assert_eq!(codes(&tree), [code::UNEXPECTED_ARGUMENT]);
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
