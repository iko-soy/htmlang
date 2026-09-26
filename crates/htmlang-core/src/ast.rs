use std::collections::HashMap;

#[derive(Debug)]
#[allow(dead_code)]
pub struct Document {
    /// The page `@page` makes: without one, the output is a fragment.
    pub page: Option<Page>,
    pub meta_tags: Vec<(String, String)>,
    pub head_blocks: Vec<String>,
    pub variables: HashMap<String, String>,
    pub defines: HashMap<String, Vec<Attribute>>,
    pub css_vars: Vec<(String, String)>,
    pub custom_css: Vec<String>,
    pub og_tags: Vec<(String, String)>,
    pub nodes: Vec<Node>,
}

/// What `@page` says: the page is the root element. Its styles style
/// `<body>`, which lays out the page's top-level elements as a column; its
/// HTML attributes (`lang=en`, `dir=rtl`, `class=x`) go on `<html>`.
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub title: String,
    /// `key=value` attributes, and bare boolean HTML attributes, for `<html>`
    pub html_attrs: Vec<Attribute>,
    /// Styles, prefixed ones included, for `<body>`
    pub styles: Vec<Attribute>,
    /// `favicon FILE`, `@page`'s one word of htmlang's own
    pub favicon: Option<String>,
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
    /// The value of the element's leading attribute (see
    /// [`ElementKind::arg`]): the first token of `@link /about About`, or
    /// its `href=` when it has no argument. For `@slot`, the slot's name.
    /// Any other text after the attributes is content, a first child.
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
            ElementKind::Fragment | ElementKind::Children | ElementKind::Slot(_) => Layout::Native,
            ElementKind::Tag(spec) => spec.layout,
        }
    }

    /// The CSS every such element starts with, besides its layout's
    /// `display` (browser margins reset, the list markers of a column, a
    /// link's colour). It goes into the element's own generated class,
    /// never onto a global element selector, so HTML from `@markdown` and
    /// `@raw` keeps the browser's defaults.
    pub fn css(&self) -> &'static str {
        match self {
            ElementKind::Paragraph => "margin:0;",
            ElementKind::Link => "text-decoration:none;color:inherit;",
            ElementKind::Image => "display:block;",
            ElementKind::Tag(spec) => spec.css,
            _ => "",
        }
    }

    /// What the first token after its attributes is: `@link`'s `href`,
    /// `@image`'s `src`, or its row's [`TagArg`].
    pub fn arg(&self) -> TagArg {
        match self {
            ElementKind::Link => TagArg::Attr("href"),
            ElementKind::Image => TagArg::Attr("src"),
            ElementKind::Tag(spec) => spec.arg,
            _ => TagArg::Child,
        }
    }

    /// The HTML attribute its leading argument fills, when it has one;
    /// `in_picture`: the element is directly inside `@picture`.
    pub fn leading_attribute(&self, in_picture: bool) -> Option<&'static str> {
        self.arg().attribute(in_picture)
    }

    /// Whether its indented body is foreign text written into the page as
    /// it is (`@script`). (`@code` and `@textarea` also have a verbatim
    /// body, shown as text: see [`TagSpec::literal`].)
    pub fn is_verbatim(&self) -> bool {
        self.spec().is_some_and(|spec| spec.verbatim)
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
    /// Its content: the first line of its text, parsed like any text
    /// (`@h1 Hello {@text [color red] world}`). The element's layout
    /// decides how it combines with the lines under it.
    Child,
    /// Its leading attribute: the first token fills this HTML attribute
    /// (`@iframe URL` sets `src`), and the rest of the text is content, or
    /// an error on an element without content. The token ends at a space
    /// outside `"..."` and `${...}`.
    Attr(&'static str),
    /// `@source`'s leading attribute, which depends on where it is:
    /// `srcset` directly inside `@picture`, `src` elsewhere (`@video`,
    /// `@audio`).
    Source,
}

impl TagArg {
    /// The HTML attribute the leading argument fills; `in_picture`: the
    /// element is directly inside `@picture`.
    pub fn attribute(self, in_picture: bool) -> Option<&'static str> {
        match self {
            TagArg::Child => None,
            TagArg::Attr(attr) => Some(attr),
            TagArg::Source if in_picture => Some("srcset"),
            TagArg::Source => Some("src"),
        }
    }

    /// The attributes the leading argument may fill, wherever the element
    /// is: one, or `src` and `srcset` for `@source`.
    pub fn attributes(self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = [self.attribute(false), self.attribute(true)]
            .into_iter()
            .flatten()
            .collect();
        out.dedup();
        out
    }
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
    /// Its text is shown as written (`@code`, `@textarea`). On its line and
    /// inline, a `{@...}` is text, not an inline element (escapes and
    /// `$names` still work). Its indented body is verbatim: nothing in it is
    /// parsed, and it is shown HTML-escaped, its lines and indentation kept.
    pub literal: bool,
    /// Its indented body is foreign text written into the page exactly as
    /// it is (`@script`'s JavaScript): nothing in it is parsed as htmlang.
    pub verbatim: bool,
}

impl TagSpec {
    const DEFAULT: TagSpec = TagSpec {
        name: "",
        html: "",
        css: "",
        arg: TagArg::Child,
        layout: Layout::Native,
        literal: false,
        verbatim: false,
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
    TagSpec { name: "li", html: "li", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "dl", html: "dl", css: "margin:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "dt", html: "dt", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "dd", html: "dd", css: "margin:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
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
    TagSpec { name: "td", html: "td", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "th", html: "th", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "button", html: "button", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "label", html: "label", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "option", html: "option", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "textarea", html: "textarea", layout: Layout::Native, literal: true, ..TagSpec::DEFAULT },
    TagSpec { name: "summary", html: "summary", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "cite", html: "cite", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "figcaption", html: "figcaption", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "legend", html: "legend", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "time", html: "time", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "mark", html: "mark", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "abbr", html: "abbr", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "code", html: "code", css: "font-family:ui-monospace,monospace;", layout: Layout::Text, literal: true, ..TagSpec::DEFAULT },
    TagSpec { name: "kbd", html: "kbd", css: "font-family:ui-monospace,monospace;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "pre", html: "pre", css: "margin:0;white-space:pre;font-family:ui-monospace,monospace;", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "h1", html: "h1", css: "margin:0;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "h2", html: "h2", css: "margin:0;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "h3", html: "h3", css: "margin:0;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "h4", html: "h4", css: "margin:0;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "h5", html: "h5", css: "margin:0;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "h6", html: "h6", css: "margin:0;", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "input", html: "input", layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "hr", html: "hr", layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "source", html: "source", arg: TagArg::Source, layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "video", html: "video", arg: TagArg::Attr("src"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "audio", html: "audio", arg: TagArg::Attr("src"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "iframe", html: "iframe", arg: TagArg::Attr("src"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "script", html: "script", arg: TagArg::Attr("src"), layout: Layout::Native, verbatim: true, ..TagSpec::DEFAULT },
    // Flow containers
    TagSpec { name: "hgroup", html: "hgroup", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "menu", html: "menu", css: "margin:0;padding-left:0;list-style:none;", layout: Layout::Column, ..TagSpec::DEFAULT },
    // Phrasing: text with the browser's own style, and no CSS of htmlang's
    TagSpec { name: "b", html: "b", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "i", html: "i", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "strong", html: "strong", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "em", html: "em", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "small", html: "small", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "s", html: "s", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "u", html: "u", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "sub", html: "sub", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "sup", html: "sup", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "q", html: "q", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "var", html: "var", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "samp", html: "samp", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "dfn", html: "dfn", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "bdi", html: "bdi", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "bdo", html: "bdo", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "ins", html: "ins", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "del", html: "del", layout: Layout::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "br", html: "br", layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "wbr", html: "wbr", layout: Layout::Void, ..TagSpec::DEFAULT },
    // HTML's own display: table parts, option groups, ruby, embedded content
    TagSpec { name: "caption", html: "caption", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "colgroup", html: "colgroup", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "col", html: "col", layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "tfoot", html: "tfoot", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "optgroup", html: "optgroup", arg: TagArg::Attr("label"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "ruby", html: "ruby", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "rt", html: "rt", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "rp", html: "rp", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "track", html: "track", arg: TagArg::Attr("src"), layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "embed", html: "embed", arg: TagArg::Attr("src"), layout: Layout::Void, ..TagSpec::DEFAULT },
    TagSpec { name: "object", html: "object", arg: TagArg::Attr("data"), layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "map", html: "map", layout: Layout::Native, ..TagSpec::DEFAULT },
    TagSpec { name: "area", html: "area", arg: TagArg::Attr("href"), layout: Layout::Void, ..TagSpec::DEFAULT },
    // htmlang's own
    TagSpec { name: "grid", html: "div", layout: Layout::Grid, ..TagSpec::DEFAULT },
    TagSpec { name: "in-front", html: "div", css: "position:absolute;inset:0;", layout: Layout::Column, ..TagSpec::DEFAULT },
    TagSpec { name: "behind", html: "div", css: "position:absolute;inset:0;z-index:-1;", layout: Layout::Column, ..TagSpec::DEFAULT },
];

/// HTML elements htmlang writes under another name. The HTML name isn't an
/// element: the unknown-element error for `@a` suggests `@link`. (Not a
/// second spelling: only the suggestion reads this.)
pub const HTML_NAMES_WRITTEN_OTHERWISE: &[(&str, &str)] = &[
    ("a", "link"),
    ("img", "image"),
    ("span", "text"),
    ("p", "paragraph"),
    ("div", "el"),
];

/// Whether the element named `name` (without the `@`) shows its text as
/// written: `{@code {@link /x y}}` prints the braces, and its indented body
/// is verbatim text, HTML-escaped. The syntax tree reads this, so every
/// tool agrees on it.
pub fn has_literal_text(name: &str) -> bool {
    ElementKind::from_name(name)
        .and_then(|kind| kind.spec())
        .is_some_and(|spec| spec.literal)
}

/// How a directive reads the rest of its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgGrammar {
    /// Free text, taken as written (`@include file.hl`, `@meta name value`).
    /// For a directive with a verbatim body it is the one-line form of the
    /// body (`@raw <hr>`, `@style .a { color: red }`) or, for `@markdown`,
    /// the file.
    Text,
    /// An optional `[attributes]` list, then text (`@page [lang=en] Title`).
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
    /// Foreign text (CSS, HTML, JavaScript, Markdown, a code sample), kept
    /// exactly as written: nothing in it is parsed, and `--` is not a
    /// comment. The text on the header's line is its one-line form (or a
    /// file or URL: `@markdown notes.md`, `@script app.js`); the line and a
    /// body together are an error. A verbatim directive takes no
    /// attributes.
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
}

/// Every directive.
#[rustfmt::skip]
pub static DIRECTIVES: &[DirectiveSpec] = &[
    DirectiveSpec { name: "page", args: ArgGrammar::AttrsText, body: BodyKind::None },
    DirectiveSpec { name: "let", args: ArgGrammar::Definition, body: BodyKind::Htmlang },
    DirectiveSpec { name: "include", args: ArgGrammar::Text, body: BodyKind::None },
    DirectiveSpec { name: "data", args: ArgGrammar::Data, body: BodyKind::None },
    DirectiveSpec { name: "meta", args: ArgGrammar::Text, body: BodyKind::None },
    DirectiveSpec { name: "if", args: ArgGrammar::Expression, body: BodyKind::Htmlang },
    DirectiveSpec { name: "else", args: ArgGrammar::Else, body: BodyKind::Htmlang },
    DirectiveSpec { name: "each", args: ArgGrammar::Loop, body: BodyKind::Htmlang },
    DirectiveSpec { name: "style", args: ArgGrammar::Text, body: BodyKind::Verbatim },
    DirectiveSpec { name: "head", args: ArgGrammar::Text, body: BodyKind::Verbatim },
    DirectiveSpec { name: "raw", args: ArgGrammar::Text, body: BodyKind::Verbatim },
    DirectiveSpec { name: "markdown", args: ArgGrammar::Text, body: BodyKind::Verbatim },
];

/// The directive named `name` (without the `@`).
pub fn directive(name: &str) -> Option<&'static DirectiveSpec> {
    DIRECTIVES.iter().find(|d| d.name == name)
}

/// What the lines indented under `@name` are: the directive's body kind,
/// or an element's (verbatim for a row that says so: `@script`, and
/// `@code` and `@textarea`, whose text is literal; htmlang for every other
/// element).
pub fn body_kind(name: &str) -> BodyKind {
    let verbatim = |spec: &TagSpec| spec.verbatim || spec.literal;
    match directive(name) {
        Some(spec) => spec.body,
        None if ElementKind::from_name(name)
            .and_then(|kind| kind.spec())
            .is_some_and(verbatim) =>
        {
            BodyKind::Verbatim
        }
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
            "b",
            "i",
            "strong",
            "em",
            "small",
            "sub",
            "sup",
            "q",
            "del",
        ];
        for name in text {
            assert_eq!(layout(name), Layout::Text, "@{}", name);
        }
        let columns = [
            "el", "li", "dd", "ul", "ol", "dl", "search", "address", "noscript", "section", "form",
            "in-front", "behind", "hgroup", "menu",
        ];
        for name in columns {
            assert_eq!(layout(name), Layout::Column, "@{}", name);
        }
        assert_eq!(layout("row"), Layout::Row);
        assert_eq!(layout("grid"), Layout::Grid);
        for name in [
            "table", "tr", "select", "option", "picture", "video", "pre", "canvas", "caption",
            "colgroup", "tfoot", "optgroup", "ruby", "rt", "object",
        ] {
            assert_eq!(layout(name), Layout::Native, "@{}", name);
        }
        for name in [
            "image", "input", "hr", "source", "br", "wbr", "col", "track", "embed", "area",
        ] {
            assert_eq!(layout(name), Layout::Void, "@{}", name);
        }
    }

    #[test]
    fn html_names_written_otherwise_are_not_elements() {
        for (html, htmlang) in super::HTML_NAMES_WRITTEN_OTHERWISE {
            assert!(ElementKind::from_name(html).is_none(), "@{}", html);
            let kind = ElementKind::from_name(htmlang).unwrap();
            let written = match kind {
                ElementKind::El => "div",
                ElementKind::Text => "span",
                ElementKind::Paragraph => "p",
                ElementKind::Link => "a",
                ElementKind::Image => "img",
                _ => panic!("@{} is not one of htmlang's own", htmlang),
            };
            assert_eq!(written, *html);
        }
        // No two rows share a name, and none is a directive's
        let mut names: Vec<&str> = ElementKind::all_names().collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "an element name is listed twice");
        for name in names {
            assert!(super::directive(name).is_none(), "@{}", name);
        }
    }

    #[test]
    fn each_row_names_at_most_one_leading_attribute() {
        let leading = |name: &str, in_picture: bool| {
            ElementKind::from_name(name)
                .unwrap()
                .leading_attribute(in_picture)
        };
        for (name, attr) in [
            ("link", "href"),
            ("area", "href"),
            ("image", "src"),
            ("script", "src"),
            ("iframe", "src"),
            ("video", "src"),
            ("audio", "src"),
            ("track", "src"),
            ("embed", "src"),
            ("form", "action"),
            ("object", "data"),
            ("optgroup", "label"),
        ] {
            assert_eq!(leading(name, false), Some(attr), "@{}", name);
            assert_eq!(leading(name, true), Some(attr), "@{}", name);
        }
        assert_eq!(leading("source", false), Some("src"));
        assert_eq!(leading("source", true), Some("srcset"));
        for name in ["el", "text", "h1", "button", "slot", "fragment", "picture"] {
            assert_eq!(leading(name, false), None, "@{}", name);
        }
        // @script is an ordinary row whose body is kept as written
        let script = ElementKind::from_name("script").unwrap();
        assert!(script.is_verbatim() && script.spec().is_some());
        assert_eq!(super::body_kind("script"), super::BodyKind::Verbatim);
        assert_eq!(super::body_kind("el"), super::BodyKind::Htmlang);
        // @code and @textarea: a verbatim body, shown as text; @pre is an
        // ordinary element, so `@pre > @code` and wrapper functions work
        for name in ["code", "textarea"] {
            assert_eq!(
                super::body_kind(name),
                super::BodyKind::Verbatim,
                "@{}",
                name
            );
            assert!(super::has_literal_text(name), "@{}", name);
        }
        assert_eq!(super::body_kind("pre"), super::BodyKind::Htmlang);
        // Every verbatim directive takes its line as its one-line body or file
        for spec in super::DIRECTIVES {
            if spec.body == super::BodyKind::Verbatim {
                assert_eq!(spec.args, super::ArgGrammar::Text, "@{}", spec.name);
            }
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
