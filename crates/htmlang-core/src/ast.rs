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
    pub canonical: Option<String>,
    pub base_url: Option<String>,
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
    pub line_num: usize,
}


#[derive(Debug, Clone)]
pub struct Attribute {
    pub key: String,
    pub value: Option<String>,
    /// Written `key=value`: an HTML attribute rather than a style.
    pub html: bool,
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

    /// Does this element lay its children out in a flex column (`@el` and
    /// the semantic containers)?
    pub fn is_column(&self) -> bool {
        matches!(self, ElementKind::El)
            || self.spec().is_some_and(|spec| spec.css.contains("flex-direction:column"))
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
            "row", "el", "text", "paragraph", "link", "image", "script", "fragment",
            "children", "slot",
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
    /// Printed as the element's text (`@li First`).
    Text,
    /// Emitted as this HTML attribute (`@iframe URL` sets `src`).
    Attr(&'static str),
}

/// How an element compiles: the data behind [`ElementKind::Tag`].
#[derive(Debug, PartialEq)]
pub struct TagSpec {
    /// Name in htmlang source, without the `@`.
    pub name: &'static str,
    /// HTML element emitted.
    pub html: &'static str,
    /// Default CSS (flex column, margin reset, ...).
    pub css: &'static str,
    pub arg: TagArg,
    /// Bare text children are wrapped in `<span>` (flex containers).
    pub wraps_text: bool,
    /// Accepts layout attributes for its children (`spacing`, ...).
    pub container: bool,
    /// Void element: no children, no closing tag (`<hr>`, `<input>`).
    pub void: bool,
    /// Children are joined with spaces, like a paragraph (headings).
    pub inline: bool,
}

impl TagSpec {
    const DEFAULT: TagSpec = TagSpec {
        name: "",
        html: "",
        css: "",
        arg: TagArg::Child,
        wraps_text: false,
        container: false,
        void: false,
        inline: false,
    };
}

/// Every element besides the core ones in [`ElementKind`].
pub static TAGS: &[TagSpec] = &[
    TagSpec { name: "nav", html: "nav", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "header", html: "header", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "footer", html: "footer", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "main", html: "main", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "section", html: "section", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "article", html: "article", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "aside", html: "aside", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "details", html: "details", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "dialog", html: "dialog", css: "display:flex;flex-direction:column;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "search", html: "search", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "address", html: "address", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "noscript", html: "noscript", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "form", html: "form", css: "display:flex;flex-direction:column;", arg: TagArg::Attr("action"), wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "figure", html: "figure", css: "display:flex;flex-direction:column;margin:0;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "blockquote", html: "blockquote", css: "display:flex;flex-direction:column;margin:0;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "fieldset", html: "fieldset", css: "display:flex;flex-direction:column;border:1px solid currentColor;padding:8px;margin:0;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "datalist", html: "datalist", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "ul", html: "ul", css: "margin:0;padding-left:0;list-style:none;", container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "ol", html: "ol", css: "margin:0;padding-left:0;list-style:none;", container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "li", html: "li", css: "display:flex;flex-direction:column;", arg: TagArg::Text, wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "dl", html: "dl", css: "margin:0;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "dt", html: "dt", arg: TagArg::Text, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "dd", html: "dd", css: "margin:0;display:flex;flex-direction:column;", arg: TagArg::Text, wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "table", html: "table", ..TagSpec::DEFAULT },
    TagSpec { name: "thead", html: "thead", ..TagSpec::DEFAULT },
    TagSpec { name: "tbody", html: "tbody", ..TagSpec::DEFAULT },
    TagSpec { name: "tr", html: "tr", ..TagSpec::DEFAULT },
    TagSpec { name: "select", html: "select", ..TagSpec::DEFAULT },
    TagSpec { name: "picture", html: "picture", ..TagSpec::DEFAULT },
    TagSpec { name: "progress", html: "progress", ..TagSpec::DEFAULT },
    TagSpec { name: "meter", html: "meter", ..TagSpec::DEFAULT },
    TagSpec { name: "output", html: "output", ..TagSpec::DEFAULT },
    TagSpec { name: "canvas", html: "canvas", container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "td", html: "td", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "th", html: "th", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "button", html: "button", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "label", html: "label", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "option", html: "option", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "textarea", html: "textarea", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "summary", html: "summary", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "cite", html: "cite", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "figcaption", html: "figcaption", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "legend", html: "legend", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "time", html: "time", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "mark", html: "mark", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "abbr", html: "abbr", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "code", html: "code", css: "font-family:ui-monospace,monospace;", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "kbd", html: "kbd", css: "font-family:ui-monospace,monospace;", arg: TagArg::Text, ..TagSpec::DEFAULT },
    TagSpec { name: "pre", html: "pre", css: "margin:0;white-space:pre;font-family:ui-monospace,monospace;", ..TagSpec::DEFAULT },
    TagSpec { name: "h1", html: "h1", css: "margin:0;", arg: TagArg::Text, inline: true, ..TagSpec::DEFAULT },
    TagSpec { name: "h2", html: "h2", css: "margin:0;", arg: TagArg::Text, inline: true, ..TagSpec::DEFAULT },
    TagSpec { name: "h3", html: "h3", css: "margin:0;", arg: TagArg::Text, inline: true, ..TagSpec::DEFAULT },
    TagSpec { name: "h4", html: "h4", css: "margin:0;", arg: TagArg::Text, inline: true, ..TagSpec::DEFAULT },
    TagSpec { name: "h5", html: "h5", css: "margin:0;", arg: TagArg::Text, inline: true, ..TagSpec::DEFAULT },
    TagSpec { name: "h6", html: "h6", css: "margin:0;", arg: TagArg::Text, inline: true, ..TagSpec::DEFAULT },
    TagSpec { name: "input", html: "input", void: true, ..TagSpec::DEFAULT },
    TagSpec { name: "hr", html: "hr", void: true, ..TagSpec::DEFAULT },
    TagSpec { name: "source", html: "source", arg: TagArg::Attr("src"), void: true, ..TagSpec::DEFAULT },
    TagSpec { name: "video", html: "video", arg: TagArg::Attr("src"), ..TagSpec::DEFAULT },
    TagSpec { name: "audio", html: "audio", arg: TagArg::Attr("src"), ..TagSpec::DEFAULT },
    TagSpec { name: "iframe", html: "iframe", arg: TagArg::Attr("src"), container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "grid", html: "div", css: "display:grid;", wraps_text: true, container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "in-front", html: "div", css: "display:flex;flex-direction:column;position:absolute;inset:0;", container: true, ..TagSpec::DEFAULT },
    TagSpec { name: "behind", html: "div", css: "display:flex;flex-direction:column;position:absolute;inset:0;z-index:-1;", container: true, ..TagSpec::DEFAULT },
];

/// Elements that print their argument as their text (`@text Hello`,
/// `@li First`).
pub fn renders_argument_as_text(kind: &ElementKind) -> bool {
    *kind == ElementKind::Text || kind.spec().is_some_and(|spec| spec.arg == TagArg::Text)
}
