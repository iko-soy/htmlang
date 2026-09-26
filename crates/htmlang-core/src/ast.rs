use std::collections::HashMap;

#[derive(Debug)]
#[allow(dead_code)]
pub struct Document {
    pub page_title: Option<String>,
    pub lang: Option<String>,
    pub favicon: Option<String>,
    pub meta_tags: Vec<(String, String)>,
    pub head_blocks: Vec<String>,
    pub variables: HashMap<String, String>,
    pub defines: HashMap<String, Vec<Attribute>>,
    pub css_vars: Vec<(String, String)>,
    pub custom_css: Vec<String>,
    pub og_tags: Vec<(String, String)>,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(Vec<TextSegment>),
    Raw(String),
}

#[derive(Debug, Clone)]
pub enum TextSegment {
    Plain(String),
    Inline(Element),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub kind: ElementKind,
    pub attrs: Vec<Attribute>,
    pub argument: Option<String>,
    pub children: Vec<Node>,
    /// The line it is written on; for an element a function's body wrote,
    /// the line of the call, since a function is an element at its call.
    pub line_num: usize,
    /// The function whose body wrote it, when it comes from a call.
    pub function: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub key: String,
    /// The value as it goes into the output: a style's with the quotes of
    /// quoted text, an HTML attribute's without them.
    pub value: Option<String>,
    /// Written `key=value`: an HTML attribute rather than a style.
    pub html: bool,
    /// When the value is quoted text (`"..."`, or a variable holding
    /// quoted text), both of its forms, for a parameter it binds.
    pub quoted: Option<Quoted>,
}

/// Quoted text, which remembers that it was quoted: a CSS value writes it
/// with its quotes, text and HTML attribute values without them.
#[derive(Debug, Clone, PartialEq)]
pub struct Quoted {
    /// What it says: `a"b` for `"a\"b"`.
    pub text: String,
    /// As CSS writes it, quotes included: `"a\"b"`.
    pub css: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElementKind {
    // Layout and text: the core of the language
    Row,
    El,
    Text,
    Paragraph,
    Link,
    Image,
    // Elements generated in a special way
    Script,
    Fragment,
    // Function bodies: placeholders for the caller's content
    Children,
    Slot(String),
    /// Any other element, described by its row in [`TAGS`].
    Tag(&'static TagSpec),
}

impl ElementKind {
    /// The element's name in htmlang source, without the `@`.
    pub fn name(&self) -> &str {
        match self {
            ElementKind::Row => "row",
            ElementKind::El => "el",
            ElementKind::Text => "text",
            ElementKind::Paragraph => "paragraph",
            ElementKind::Link => "link",
            ElementKind::Image => "image",
            ElementKind::Script => "script",
            ElementKind::Fragment => "fragment",
            ElementKind::Children => "children",
            ElementKind::Slot(_) => "slot",
            ElementKind::Tag(spec) => spec.name,
        }
    }

    pub fn spec(&self) -> Option<&'static TagSpec> {
        match self {
            ElementKind::Tag(spec) => Some(spec),
            _ => None,
        }
    }

    /// How the element lays out what is inside it. `@fragment`, `@children`
    /// and `@slot` have no element of their own: their content lands in the
    /// element around them and takes its layout.
    pub fn layout(&self) -> Layout {
        match self {
            ElementKind::Row => Layout::Row,
            ElementKind::El => Layout::Column,
            ElementKind::Text | ElementKind::Paragraph | ElementKind::Link => Layout::Text,
            ElementKind::Image => Layout::Void,
            ElementKind::Script
            | ElementKind::Fragment
            | ElementKind::Children
            | ElementKind::Slot(_) => Layout::Native,
            ElementKind::Tag(spec) => spec.layout,
        }
    }

    /// The CSS every such element starts with, besides its layout's
    /// `display` (browser margins reset, the list markers of a column).
    pub fn css(&self) -> &'static str {
        match self {
            ElementKind::Paragraph => "margin:0;",
            ElementKind::Tag(spec) => spec.css,
            _ => "",
        }
    }

    /// Is this the table element named `name` (e.g. `"main"`)?
    pub fn is_tag(&self, name: &str) -> bool {
        self.spec().is_some_and(|spec| spec.name == name)
    }

    /// Look up an element by its source name (without the `@`).
    pub fn from_name(name: &str) -> Option<ElementKind> {
        Some(match name {
            "row" => ElementKind::Row,
            "el" => ElementKind::El,
            "text" => ElementKind::Text,
            "paragraph" => ElementKind::Paragraph,
            "link" => ElementKind::Link,
            "image" => ElementKind::Image,
            "script" => ElementKind::Script,
            "fragment" => ElementKind::Fragment,
            "children" => ElementKind::Children,
            "slot" => ElementKind::Slot(String::new()),
            _ => ElementKind::Tag(TAGS.iter().find(|spec| spec.name == name)?),
        })
    }

    /// Every element name, for suggestions and completions.
    pub fn all_names() -> impl Iterator<Item = &'static str> {
        [
            "row",
            "el",
            "text",
            "paragraph",
            "link",
            "image",
            "script",
            "fragment",
            "children",
            "slot",
        ]
        .into_iter()
        .chain(TAGS.iter().map(|spec| spec.name))
    }
}

/// What an element does with the text after its name and attributes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TagArg {
    /// Leading text content (`@section Hello`).
    Child,
    /// Its first line of text, taken as written (`@li First`); the
    /// element's layout decides whether it is a child of its own.
    Text,
    /// Emitted as this HTML attribute (`@iframe URL` sets `src`).
    Attr(&'static str),
}

/// How an element lays out what is inside it: one value per element,
/// after elm-ui's layouts. It decides what its text lines are, whether
/// `spacing` works on it, and how its children's `width fill`, `center-x`
/// and `align-*` compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// A flex column (`@el`, `@section`, `@ul`, `@li`): each text line is a
    /// child of its own, and `spacing` is the gap between the children.
    Column,
    /// A flex row (`@row`): like a column, side by side.
    Row,
    /// A CSS grid (`@grid`): each text line is a cell.
    Grid,
    /// Flowing text (`@paragraph`, `@h1`, `@button`, `@td`): the argument,
    /// the text lines and the children are one run, joined with spaces. It
    /// has no gap for `spacing`, and a row, column or grid inside it is laid
    /// out inline (`@el` becomes an inline-flex `<span>`).
    Text,
    /// HTML's own layout, which htmlang leaves alone: tables, `@select`,
    /// `@picture`, media and form controls. Text lines are separated by a
    /// line break, which HTML shows as a space except in `@pre` and
    /// `@textarea`.
    Native,
    /// No content at all (`@input`, `@hr`, `@image`).
    Void,
}

impl Layout {
    /// A row, column or grid: an element that lays out its children, so
    /// `spacing`, `wrap` and `grid-cols` work on it.
    pub fn is_container(self) -> bool {
        matches!(self, Layout::Column | Layout::Row | Layout::Grid)
    }

    /// The name used in messages and the reference: `column`, `text`, ...
    pub fn name(self) -> &'static str {
        match self {
            Layout::Column => "column",
            Layout::Row => "row",
            Layout::Grid => "grid",
            Layout::Text => "text",
            Layout::Native => "native",
            Layout::Void => "void",
        }
    }
}

/// How an element compiles: the data behind [`ElementKind::Tag`].
#[derive(Debug, PartialEq)]
pub struct TagSpec {
    /// Name in htmlang source, without the `@`.
    pub name: &'static str,
    /// HTML element emitted.
    pub html: &'static str,
    /// Default CSS besides the layout's `display` (margin reset, ...).
    pub css: &'static str,
    pub arg: TagArg,
    /// How it lays out its content.
    pub layout: Layout,
}

impl TagSpec {
    const DEFAULT: TagSpec = TagSpec {
        name: "",
        html: "",
        css: "",
        arg: TagArg::Child,
        layout: Layout::Native,
    };
}

/// Every element besides the core ones in [`ElementKind`].
#[rustfmt::skip]
pub static TAGS: &[TagSpec] = &[
    TagSpec { name: "nav", html: "nav", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "header", html: "header", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "footer", html: "footer", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "main", html: "main", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "section", html: "section", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "article", html: "article", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "aside", html: "aside", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "details", html: "details", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "dialog", html: "dialog", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "search", html: "search", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "address", html: "address", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "noscript", html: "noscript", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "form", html: "form", arg: TagArg::Attr("action"), layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "figure", html: "figure", css: "margin:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "blockquote", html: "blockquote", css: "margin:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "fieldset", html: "fieldset", css: "border:1px solid currentColor;padding:8px;margin:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "datalist", html: "datalist", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "ul", html: "ul", css: "margin:0;padding-left:0;list-style:none;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "ol", html: "ol", css: "margin:0;padding-left:0;list-style:none;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "li", html: "li", arg: TagArg::Text, layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "dl", html: "dl", css: "margin:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "dt", html: "dt", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "dd", html: "dd", css: "margin:0;", arg: TagArg::Text, layout: Layout::Column },
    TagSpec { name: "table", html: "table", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "thead", html: "thead", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "tbody", html: "tbody", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "tr", html: "tr", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "select", html: "select", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "picture", html: "picture", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "progress", html: "progress", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "meter", html: "meter", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "output", html: "output", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "canvas", html: "canvas", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "td", html: "td", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "th", html: "th", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "button", html: "button", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "label", html: "label", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "option", html: "option", arg: TagArg::Text, layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "textarea", html: "textarea", arg: TagArg::Text, layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "summary", html: "summary", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "cite", html: "cite", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "figcaption", html: "figcaption", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "legend", html: "legend", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "time", html: "time", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "mark", html: "mark", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "abbr", html: "abbr", arg: TagArg::Text, layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "code", html: "code", css: "font-family:ui-monospace,monospace;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "kbd", html: "kbd", css: "font-family:ui-monospace,monospace;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "pre", html: "pre", css: "margin:0;white-space:pre;font-family:ui-monospace,monospace;", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "h1", html: "h1", css: "margin:0;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "h2", html: "h2", css: "margin:0;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "h3", html: "h3", css: "margin:0;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "h4", html: "h4", css: "margin:0;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "h5", html: "h5", css: "margin:0;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "h6", html: "h6", css: "margin:0;", arg: TagArg::Text, layout: Layout::Text },
    TagSpec { name: "input", html: "input", layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "hr", html: "hr", layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "source", html: "source", arg: TagArg::Attr("src"), layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "video", html: "video", arg: TagArg::Attr("src"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "audio", html: "audio", arg: TagArg::Attr("src"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "iframe", html: "iframe", arg: TagArg::Attr("src"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "grid", html: "div", layout: Layout::Grid, ..TagSpec::DEFAULT },
    TagSpec { name: "in-front", html: "div", css: "position:absolute;inset:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "behind", html: "div", css: "position:absolute;inset:0;z-index:-1;", layout: Layout::Column, ..TagSpec::DEFAULT },
];

/// Elements that print their argument as their text (`@text Hello`,
/// `@li First`).
pub fn renders_argument_as_text(kind: &ElementKind) -> bool {
    *kind == ElementKind::Text || kind.spec().is_some_and(|spec| spec.arg == TagArg::Text)
}

/// How a directive reads the rest of its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgGrammar {
    /// Nothing may follow the name (`@style`).
    None,
    /// Free text, taken as written (`@include file.hl`, `@meta name value`).
    Text,
    /// An optional `[attributes]` list, then text (`@page [lang en] Title`).
    AttrsText,
    /// An expression (`@if $count > 2`).
    Expression,
    /// A name, then a value, `= EXPR`, `"text"`, `[attributes]` or the
    /// parameters of a function (`@let`).
    Definition,
    /// Nothing, or `if EXPR` (`@else`, `@else if`).
    Else,
    /// `$item in LIST` or `$item, $index in LIST` (`@each`).
    Loop,
    /// `$name SOURCE`; inline JSON may span several lines (`@data`).
    Data,
}

/// What the lines indented under a directive or element are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// Nothing: indented lines are an error.
    None,
    /// htmlang: elements, text and directives.
    Htmlang,
    /// Foreign text (CSS, HTML, JavaScript, Markdown), kept exactly as
    /// written: nothing in it is parsed, and `--` is not a comment.
    Verbatim,
}

/// A directive: a `@name` that isn't an element. The syntax tree
/// (`syntax.rs`), the evaluator, the formatter and the editor tools all
/// read the directives' shapes from this table.
#[derive(Debug, PartialEq)]
pub struct DirectiveSpec {
    /// Name in htmlang source, without the `@`.
    pub name: &'static str,
    pub args: ArgGrammar,
    pub body: BodyKind,
    /// A verbatim directive may give its content as the rest of its line
    /// instead of an indented body (`@raw <hr>`, `@markdown file.md`).
    pub one_line: bool,
}

/// Every directive.
#[rustfmt::skip]
pub static DIRECTIVES: &[DirectiveSpec] = &[
    DirectiveSpec { name: "page", args: ArgGrammar::AttrsText, body: BodyKind::None, one_line: false },
    DirectiveSpec { name: "let", args: ArgGrammar::Definition, body: BodyKind::Htmlang, one_line: false },
    DirectiveSpec { name: "include", args: ArgGrammar::Text, body: BodyKind::None, one_line: false },
    DirectiveSpec { name: "data", args: ArgGrammar::Data, body: BodyKind::None, one_line: false },
    DirectiveSpec { name: "meta", args: ArgGrammar::Text, body: BodyKind::None, one_line: false },
    DirectiveSpec { name: "if", args: ArgGrammar::Expression, body: BodyKind::Htmlang, one_line: false },
    DirectiveSpec { name: "else", args: ArgGrammar::Else, body: BodyKind::Htmlang, one_line: false },
    DirectiveSpec { name: "each", args: ArgGrammar::Loop, body: BodyKind::Htmlang, one_line: false },
    DirectiveSpec { name: "style", args: ArgGrammar::None, body: BodyKind::Verbatim, one_line: false },
    DirectiveSpec { name: "head", args: ArgGrammar::None, body: BodyKind::Verbatim, one_line: false },
    DirectiveSpec { name: "raw", args: ArgGrammar::Text, body: BodyKind::Verbatim, one_line: true },
    DirectiveSpec { name: "markdown", args: ArgGrammar::Text, body: BodyKind::Verbatim, one_line: true },
];

/// The directive named `name` (without the `@`).
pub fn directive(name: &str) -> Option<&'static DirectiveSpec> {
    DIRECTIVES.iter().find(|d| d.name == name)
}

/// What the lines indented under `@name` are: the directive's body kind,
/// or an element's (`@script` holds JavaScript, every other element
/// htmlang).
pub fn body_kind(name: &str) -> BodyKind {
    match directive(name) {
        Some(spec) => spec.body,
        None if name == "script" => BodyKind::Verbatim,
        None => BodyKind::Htmlang,
    }
}

#[cfg(test)]
mod tests {
    use super::{ElementKind, Layout};

    fn layout(name: &str) -> Layout {
        ElementKind::from_name(name).unwrap().layout()
    }

    #[test]
    fn every_element_has_the_layout_it_is_documented_with() {
        let text = [
            "text",
            "paragraph",
            "link",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "button",
            "td",
            "th",
            "label",
            "summary",
            "legend",
            "figcaption",
            "dt",
            "cite",
            "code",
            "kbd",
            "mark",
            "abbr",
            "time",
        ];
        for name in text {
            assert_eq!(layout(name), Layout::Text, "@{}", name);
        }
        let columns = [
            "el", "li", "dd", "ul", "ol", "dl", "search", "address", "noscript", "section", "form",
            "in-front", "behind",
        ];
        for name in columns {
            assert_eq!(layout(name), Layout::Column, "@{}", name);
        }
        assert_eq!(layout("row"), Layout::Row);
        assert_eq!(layout("grid"), Layout::Grid);
        for name in [
            "table", "tr", "select", "option", "picture", "video", "pre", "canvas",
        ] {
            assert_eq!(layout(name), Layout::Native, "@{}", name);
        }
        for name in ["image", "input", "hr", "source"] {
            assert_eq!(layout(name), Layout::Void, "@{}", name);
        }
    }

    #[test]
    fn a_layout_s_display_is_not_repeated_in_the_row_s_css() {
        for spec in super::TAGS {
            assert!(
                !spec.css.contains("display"),
                "@{}: {}",
                spec.name,
                spec.css
            );
        }
    }
}
