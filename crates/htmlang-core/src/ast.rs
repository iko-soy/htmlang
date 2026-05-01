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
    pub keyframes: Vec<(String, String)>,
    pub css_vars: Vec<(String, String)>,
    pub custom_css: Vec<String>,
    pub og_tags: Vec<(String, String)>,
    pub custom_breakpoints: Vec<(String, String)>,
    pub theme_tokens: Vec<(String, String)>,
    pub canonical: Option<String>,
    pub base_url: Option<String>,
    pub font_faces: Vec<(String, String)>,
    pub json_ld_blocks: Vec<String>,
    pub scope_blocks: Vec<String>,
    pub starting_style_blocks: Vec<String>,
    pub manifest: Option<ManifestConfig>,
    pub preload_hints: Vec<PreloadHint>,
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct ManifestConfig {
    pub name: String,
    pub short_name: Option<String>,
    pub start_url: String,
    pub display: String,
    pub background_color: Option<String>,
    pub theme_color: Option<String>,
    pub description: Option<String>,
    pub icons: Vec<(String, String)>, // (src, sizes)
}

#[derive(Debug, Clone)]
pub struct PreloadHint {
    pub href: String,
    pub as_type: String, // "font", "image", "style", "script"
    pub crossorigin: bool,
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

#[derive(Debug, Clone, PartialEq)]
pub enum ElementKind {
    Row,
    Column,
    El,
    Text,
    Paragraph,
    Image,
    Link,
    Children,
    // Form elements
    Input,
    Button,
    Select,
    Textarea,
    Option,
    Label,
    // Slots
    Slot(String),
    // Semantic elements
    Nav,
    Header,
    Footer,
    Main,
    Section,
    Article,
    Aside,
    // List elements
    List,
    ListItem,
    // Table elements
    Table,
    TableHead,
    TableBody,
    TableRow,
    TableCell,
    TableHeaderCell,
    // Media elements
    Video,
    Audio,
    // Additional semantic elements
    Form,
    Details,
    Summary,
    Blockquote,
    Cite,
    Code,
    Pre,
    HorizontalRule,
    Figure,
    FigCaption,
    Progress,
    Meter,
    Fragment,
    Dialog,
    DefinitionList,
    DefinitionTerm,
    DefinitionDescription,
    Fieldset,
    Legend,
    Picture,
    Source,
    Time,
    Mark,
    Kbd,
    Abbr,
    Datalist,
    Iframe,
    Output,
    Canvas,
    // Heading elements
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    // New convenience elements
    Grid,
    Stack,
    Spacer,
    Badge,
    Tooltip,
    Avatar,
    Carousel,
    Chip,
    Tag,
    // Overlay positioning (elm-ui inspired)
    InFront,
    Behind,
    // HTML5 elements
    Script,
    Noscript,
    Address,
    Search,
    Breadcrumb,
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub key: String,
    pub value: Option<String>,
}
