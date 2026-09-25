use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::ast::*;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Help,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: usize,
    pub column: Option<usize>,
    pub message: String,
    pub severity: Severity,
    pub source_line: Option<String>,
}

pub struct ParseResult {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
    pub included_files: Vec<PathBuf>,
}

#[derive(Debug)]
struct ParseError {
    line: usize,
    message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum LineContent {
    Normal(String),
    Raw(String),
}

#[derive(Clone)]
struct Line {
    indent: usize,
    content: LineContent,
    line_num: usize,
}

#[derive(Clone)]
struct FnDef {
    params: Vec<String>,
    defaults: HashMap<String, String>,
    body_lines: Vec<Line>,
}

struct ParseContext {
    page_title: Option<String>,
    lang: Option<String>,
    favicon: Option<String>,
    meta_tags: Vec<(String, String)>,
    head_blocks: Vec<String>,
    variables: HashMap<String, String>,
    defines: HashMap<String, Vec<Attribute>>,
    functions: HashMap<String, FnDef>,
    keyframes: Vec<(String, String)>,
    css_vars: Vec<(String, String)>,
    custom_css: Vec<String>,
    og_tags: Vec<(String, String)>,
    custom_breakpoints: Vec<(String, String)>,
    diagnostics: Vec<Diagnostic>,
    base_path: Option<PathBuf>,
    included_files: Vec<PathBuf>,
    include_stack: Vec<PathBuf>,
    fn_call_stack: Vec<String>,
    file_cache: HashMap<PathBuf, String>,
    /// Track which @let variables are referenced (for unused warnings)
    used_variables: HashSet<String>,
    /// Track which functions are called (for unused warnings)
    used_functions: HashSet<String>,
    /// Track which attribute bundles are referenced (for unused warnings)
    used_defines: HashSet<String>,
    /// Line numbers of @let definitions (name -> line)
    let_lines: HashMap<String, usize>,
    /// Line numbers of function definitions (name -> line)
    fn_lines: HashMap<String, usize>,
    /// Line numbers of attribute bundle definitions (name -> line)
    define_lines: HashMap<String, usize>,
    /// Deprecated functions: name -> deprecation message
    deprecated_fns: HashMap<String, String>,
    /// Theme tokens: (name, value) pairs from @theme
    theme_tokens: Vec<(String, String)>,
    /// Canonical URL
    canonical: Option<String>,
    /// Base URL for relative links
    base_url: Option<String>,
    /// @font-face declarations: (font_name, url)
    font_faces: Vec<(String, String)>,
    /// @json-ld blocks
    json_ld_blocks: Vec<String>,
    /// @scope CSS blocks
    scope_blocks: Vec<String>,
    /// @starting-style CSS blocks
    starting_style_blocks: Vec<String>,
    /// @manifest configuration
    manifest: Option<crate::ast::ManifestConfig>,
    /// Track @import paths for circular dependency detection
    import_stack: Vec<PathBuf>,
    /// i18n translations: locale -> (key -> value)
    translations: HashMap<String, HashMap<String, String>>,
    /// Current active locale for translations
    active_locale: Option<String>,
}

struct Parser {
    lines: Vec<Line>,
    pos: usize,
}

pub fn parse(input: &str) -> ParseResult {
    parse_with_base(input, None)
}

pub fn parse_with_base(input: &str, base_path: Option<&Path>) -> ParseResult {
    let lines = preprocess(input);
    let mut parser = Parser { lines, pos: 0 };
    let mut ctx = ParseContext {
        page_title: None,
        lang: None,
        favicon: None,
        meta_tags: Vec::new(),
        head_blocks: Vec::new(),
        variables: HashMap::new(),
        defines: HashMap::new(),
        functions: HashMap::new(),
        keyframes: Vec::new(),
        css_vars: Vec::new(),
        custom_css: Vec::new(),
        og_tags: Vec::new(),
        custom_breakpoints: Vec::new(),
        diagnostics: Vec::new(),
        base_path: base_path.map(|p| p.to_path_buf()),
        included_files: Vec::new(),
        include_stack: Vec::new(),
        fn_call_stack: Vec::new(),
        file_cache: HashMap::new(),
        used_variables: HashSet::new(),
        used_functions: HashSet::new(),
        used_defines: HashSet::new(),
        let_lines: HashMap::new(),
        fn_lines: HashMap::new(),
        define_lines: HashMap::new(),
        deprecated_fns: HashMap::new(),
        theme_tokens: Vec::new(),
        canonical: None,
        base_url: None,
        font_faces: Vec::new(),
        json_ld_blocks: Vec::new(),
        scope_blocks: Vec::new(),
        starting_style_blocks: Vec::new(),
        manifest: None,
        import_stack: Vec::new(),
        translations: HashMap::new(),
        active_locale: None,
    };
    let nodes = parser.parse_children(0, &mut ctx);
    validate_tree(&nodes, None, &mut ctx.diagnostics);
    check_unused(&mut ctx);
    ParseResult {
        document: Document {
            page_title: ctx.page_title,
            lang: ctx.lang,
            favicon: ctx.favicon,
            meta_tags: ctx.meta_tags,
            head_blocks: ctx.head_blocks,
            variables: ctx.variables,
            defines: ctx.defines,
            keyframes: ctx.keyframes,
            css_vars: ctx.css_vars,
            custom_css: ctx.custom_css,
            og_tags: ctx.og_tags,
            custom_breakpoints: ctx.custom_breakpoints,
            theme_tokens: ctx.theme_tokens,
            canonical: ctx.canonical,
            base_url: ctx.base_url,
            font_faces: ctx.font_faces,
            json_ld_blocks: ctx.json_ld_blocks,
            scope_blocks: ctx.scope_blocks,
            starting_style_blocks: ctx.starting_style_blocks,
            manifest: ctx.manifest,
            preload_hints: collect_image_preload_hints(&nodes),
            nodes,
        },
        diagnostics: ctx.diagnostics,
        included_files: ctx.included_files,
    }
}

/// Scan nodes for @image elements and generate preload hints for early images.
fn collect_image_preload_hints(nodes: &[Node]) -> Vec<crate::ast::PreloadHint> {
    let mut hints = Vec::new();
    // Only preload the first few images (above-the-fold heuristic)
    collect_images_recursive(nodes, &mut hints, 3);
    hints
}

fn collect_images_recursive(nodes: &[Node], hints: &mut Vec<crate::ast::PreloadHint>, max: usize) {
    for node in nodes {
        if hints.len() >= max {
            return;
        }
        if let Node::Element(elem) = node {
            if elem.kind == ElementKind::Image
                && let Some(ref src) = elem.argument
                && !src.is_empty()
                && !src.starts_with("data:")
                && !src.starts_with('#')
            {
                hints.push(crate::ast::PreloadHint {
                    href: src.clone(),
                    as_type: "image".to_string(),
                    crossorigin: false,
                });
            }
            collect_images_recursive(&elem.children, hints, max);
        }
    }
}

// ---------------------------------------------------------------------------
// Preprocessing: strip comments/blanks, collapse @raw blocks
// ---------------------------------------------------------------------------

fn preprocess(input: &str) -> Vec<Line> {
    let raw_lines: Vec<&str> = input.lines().collect();
    let mut lines = Vec::new();
    let mut i = 0;

    while i < raw_lines.len() {
        let line = raw_lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with("--") {
            i += 1;
            continue;
        }

        let indent = line.len() - line.trim_start().len();

        // Inline `@markdown` and `@script` blocks keep their bodies verbatim:
        // in Markdown, blank lines separate paragraphs, `---` is a rule (not
        // a comment) and code indentation matters; in JavaScript, newlines,
        // `{...}` and `$` must reach the output untouched.
        let verbatim_body = trimmed == "@markdown"
            || trimmed == "@script"
            || trimmed.starts_with("@script ")
            || trimmed.starts_with("@script[");
        if verbatim_body {
            lines.push(Line {
                indent,
                content: LineContent::Normal(trimmed.to_string()),
                line_num: i + 1,
            });
            let body_start = i + 1;
            let mut body_end = body_start;
            let mut j = body_start;
            while j < raw_lines.len() {
                let l = raw_lines[j];
                if l.trim().is_empty() {
                    j += 1;
                    continue;
                }
                if l.len() - l.trim_start().len() <= indent {
                    break;
                }
                j += 1;
                body_end = j;
            }
            let body = &raw_lines[body_start..body_end];
            let body_indent = body
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);
            if !body.is_empty() {
                let text: Vec<&str> = body
                    .iter()
                    .map(|l| l.get(body_indent..).unwrap_or("").trim_end())
                    .collect();
                lines.push(Line {
                    indent: indent + 1,
                    content: LineContent::Raw(text.join("\n")),
                    line_num: body_start + 1,
                });
            }
            i = body_end.max(body_start);
            continue;
        }

        // Handle @raw """..."""
        if let Some(raw_rest) = trimmed.strip_prefix("@raw") {
            let after_raw = raw_rest.trim_start();
            if let Some(after_open) = after_raw.strip_prefix("\"\"\"") {
                // Single-line: @raw """content"""
                if after_open.ends_with("\"\"\"") && after_open.len() >= 3 {
                    let content = &after_open[..after_open.len() - 3];
                    lines.push(Line {
                        indent,
                        content: LineContent::Raw(content.to_string()),
                        line_num: i + 1,
                    });
                    i += 1;
                    continue;
                }

                // Multiline: collect until closing """
                let mut raw_content = String::new();
                if !after_open.is_empty() {
                    raw_content.push_str(after_open);
                    raw_content.push('\n');
                }
                i += 1;
                while i < raw_lines.len() {
                    if raw_lines[i].trim() == "\"\"\"" {
                        i += 1;
                        break;
                    }
                    raw_content.push_str(raw_lines[i]);
                    raw_content.push('\n');
                    i += 1;
                }

                lines.push(Line {
                    indent,
                    content: LineContent::Raw(raw_content.trim_end_matches('\n').to_string()),
                    line_num: i,
                });
                continue;
            }
        }

        // Join continuation lines for multi-line attribute brackets
        let first_line_num = i + 1;
        let mut full = trimmed.to_string();
        while open_attr_depth(&full) > 0 && i + 1 < raw_lines.len() {
            i += 1;
            let next = raw_lines[i].trim();
            if next.is_empty() || next.starts_with("--") {
                continue;
            }
            full.push(' ');
            full.push_str(next);
        }

        lines.push(Line {
            indent,
            content: LineContent::Normal(full),
            line_num: first_line_num,
        });
        i += 1;
    }

    lines
}

/// Bracket depth left open at the end of `line`, counting only attribute
/// lists: a `[` that starts the line or follows an `@name` token (optionally
/// with one more word, as in `@let name [`). Brackets in text content, such
/// as `@text [bold] Use [ to open`, are ignored.
fn open_attr_depth(line: &str) -> i32 {
    if !line.starts_with('@') && !line.starts_with('[') {
        return 0;
    }
    let bytes = line.as_bytes();
    let mut depth: i32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' if depth > 0 => depth += 1,
            b'[' => {
                // Is this the start of an attribute list?
                let before = line[..i].trim_end();
                let last_directive = before
                    .rsplit([']', '>'])
                    .next()
                    .unwrap_or("")
                    .trim();
                let tokens: Vec<&str> = last_directive.split_whitespace().collect();
                let starts_list = match tokens.as_slice() {
                    [] => before.is_empty() || before.ends_with('>'),
                    [name] => name.starts_with('@'),
                    [name, _] => name.starts_with('@'),
                    _ => false,
                };
                if starts_list {
                    depth = 1;
                } else if before.ends_with(']') {
                    // Text after a closed attribute list: stop scanning.
                    return 0;
                }
            }
            b']' if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    depth
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

impl Parser {
    fn parse_children(&mut self, min_indent: usize, ctx: &mut ParseContext) -> Vec<Node> {
        let mut nodes = Vec::new();

        while self.pos < self.lines.len() {
            let indent = self.lines[self.pos].indent;
            if indent < min_indent {
                break;
            }

            match self.parse_line(ctx) {
                Ok(Some(new_nodes)) => nodes.extend(new_nodes),
                Ok(None) => {}
                Err(e) => {
                    // Error recovery: record the diagnostic and continue parsing
                    // to report multiple errors in a single pass
                    let source = if self.pos > 0 && self.pos <= self.lines.len() {
                        match &self.lines[self.pos.saturating_sub(1)].content {
                            LineContent::Normal(s) => Some(s.clone()),
                            LineContent::Raw(s) => Some(s.clone()),
                        }
                    } else {
                        None
                    };
                    ctx.diagnostics.push(Diagnostic {
                        line: e.line,
                        column: None,
                        message: e.message,
                        severity: Severity::Error,
                        source_line: source,
                    });
                    // Skip forward past any deeper-indented children of the errored line
                    while self.pos < self.lines.len() && self.lines[self.pos].indent > indent {
                        self.pos += 1;
                    }
                }
            }
        }

        nodes
    }

    fn parse_line(&mut self, ctx: &mut ParseContext) -> Result<Option<Vec<Node>>, ParseError> {
        let line_num = self.lines[self.pos].line_num;
        let current_indent = self.lines[self.pos].indent;

        // Handle raw content
        if let LineContent::Raw(s) = &self.lines[self.pos].content {
            let content = s.clone();
            self.pos += 1;
            return Ok(Some(vec![Node::Raw(content)]));
        }

        // Normal content — clone to release borrow. Raw is already handled
        // above; a let-else pattern avoids a panic path if new LineContent
        // variants are added in the future.
        let LineContent::Normal(content) = &self.lines[self.pos].content else {
            return Err(ParseError {
                line: line_num,
                message: "internal: unexpected line content variant".to_string(),
            });
        };
        let content = content.clone();
        self.pos += 1;

        // --- Directives ---

        if let Some(rest) = content.strip_prefix("@page ") {
            ctx.page_title = Some(substitute_vars(rest, &ctx.variables));
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@lang ") {
            ctx.lang = Some(substitute_vars(rest.trim(), &ctx.variables));
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@favicon ") {
            ctx.favicon = Some(substitute_vars(rest.trim(), &ctx.variables));
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@let ") {
            let rest = rest.trim();

            // A `"""` value opens a multi-line string, whose indented lines
            // are its content — not a function body.
            let opens_triple_quote = rest.split_once(' ').is_some_and(|(_, v)| {
                let v = v.trim();
                v.strip_prefix("= ").unwrap_or(v).trim_start().starts_with("\"\"\"")
            });
            // Check if next lines are indented (function/component definition)
            let has_body = !opens_triple_quote
                && self.pos < self.lines.len()
                && self.lines[self.pos].indent > current_indent;

            if has_body {
                // Function definition: @let name $param1 $param2=default
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.is_empty() {
                    return Err(ParseError {
                        line: line_num,
                        message: "@let with body requires a name".to_string(),
                    });
                }
                let name = parts[0].to_string();
                let mut params = Vec::new();
                let mut defaults = HashMap::new();
                for part in &parts[1..] {
                    let part = part.strip_prefix('$').unwrap_or(part);
                    if let Some((param_name, default_val)) = part.split_once('=') {
                        params.push(param_name.to_string());
                        defaults.insert(param_name.to_string(), default_val.to_string());
                    } else {
                        params.push(part.to_string());
                    }
                }

                // Collect body lines (all lines indented deeper than @let)
                let mut body_lines = Vec::new();
                while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                    body_lines.push(self.lines[self.pos].clone());
                    self.pos += 1;
                }

                ctx.fn_lines.entry(name.clone()).or_insert(line_num);
                ctx.functions.insert(
                    name,
                    FnDef {
                        params,
                        defaults,
                        body_lines,
                    },
                );
                return Ok(None);
            }

            if let Some((name, value)) = rest.split_once(' ') {
                let value = value.trim();
                // Support @let name = expr syntax (strip leading =)
                let value = if let Some(after_eq) = value.strip_prefix("= ") {
                    after_eq.trim()
                } else if value == "=" {
                    ""
                } else {
                    value
                };

                // Attribute bundle: @let name [attr1, attr2, ...]
                if value.starts_with('[') {
                    let (attrs, _) = parse_attr_brackets(value, line_num, ctx)?;
                    ctx.defines.insert(name.to_string(), attrs);
                    ctx.define_lines.entry(name.to_string()).or_insert(line_num);
                    return Ok(None);
                }

                // Multi-line @let with triple quotes: @let name """..."""
                let value_str;
                let quoted = value.starts_with('"') && value.ends_with('"') && value.len() >= 2
                    || value.starts_with("\"\"\"");
                let value = if let Some(after_open) = value.strip_prefix("\"\"\"") {
                    if after_open.ends_with("\"\"\"") && after_open.len() >= 3 {
                        // Single-line triple-quote: @let name """value"""
                        value_str = after_open[..after_open.len() - 3].to_string();
                        value_str.as_str()
                    } else {
                        // Multi-line: collect indented body lines until closing """
                        let mut lines_buf = String::new();
                        if !after_open.is_empty() {
                            lines_buf.push_str(after_open);
                            lines_buf.push('\n');
                        }
                        while self.pos < self.lines.len() {
                            match &self.lines[self.pos].content {
                                LineContent::Normal(s) if s.trim() == "\"\"\"" => {
                                    self.pos += 1;
                                    break;
                                }
                                LineContent::Normal(s) => {
                                    lines_buf.push_str(s);
                                    lines_buf.push('\n');
                                }
                                LineContent::Raw(s) => {
                                    lines_buf.push_str(s);
                                    lines_buf.push('\n');
                                }
                            }
                            self.pos += 1;
                        }
                        value_str = lines_buf.trim_end_matches('\n').to_string();
                        value_str.as_str()
                    }
                } else if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    // Support quoted string interpolation: @let greeting "Hello $name"
                    &value[1..value.len() - 1]
                } else {
                    value
                };
                track_var_refs(value, &mut ctx.used_variables);
                let value = substitute_vars(value, &ctx.variables);
                // Quoted values are literal strings; only bare values are
                // evaluated as arithmetic (so `"1 / -1"` stays as written).
                let value = if quoted {
                    value
                } else {
                    evaluate_arithmetic(&value)
                };
                if name.starts_with("--") {
                    // CSS custom property
                    ctx.css_vars.push((name.to_string(), value.clone()));
                }
                ctx.variables.insert(name.to_string(), value);
                ctx.let_lines.entry(name.to_string()).or_insert(line_num);
            }
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@meta ") {
            let rest = rest.trim();
            if let Some((name, value)) = rest.split_once(' ') {
                let value = substitute_vars(value.trim(), &ctx.variables);
                ctx.meta_tags.push((name.trim().to_string(), value));
            }
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@og ") {
            let rest = rest.trim();
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let value = substitute_vars(parts[1].trim(), &ctx.variables);
                let value = if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    value[1..value.len() - 1].to_string()
                } else {
                    value
                };
                ctx.og_tags.push((parts[0].to_string(), value));
            }
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@breakpoint ") {
            let rest = rest.trim();
            if let Some((name, value)) = rest.split_once(' ') {
                ctx.custom_breakpoints.push((
                    name.trim().to_string(),
                    substitute_vars(value.trim(), &ctx.variables),
                ));
            }
            return Ok(None);
        }

        if content == "@head" || content.starts_with("@head ") {
            // Collect indented body lines as raw head content
            let mut head_content = String::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                match &self.lines[self.pos].content {
                    LineContent::Normal(s) => {
                        head_content.push_str(s.trim());
                        head_content.push('\n');
                    }
                    LineContent::Raw(s) => {
                        head_content.push_str(s);
                        head_content.push('\n');
                    }
                }
                self.pos += 1;
            }
            let trimmed = head_content.trim().to_string();
            if !trimmed.is_empty() {
                ctx.head_blocks.push(trimmed);
            }
            return Ok(None);
        }

        // --- @style block (raw CSS) ---
        if content.trim() == "@style" {
            let mut style_content = String::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                match &self.lines[self.pos].content {
                    LineContent::Normal(s) => {
                        style_content.push_str(s.trim());
                        style_content.push('\n');
                    }
                    LineContent::Raw(s) => {
                        style_content.push_str(s);
                        style_content.push('\n');
                    }
                }
                self.pos += 1;
            }
            let trimmed = style_content.trim().to_string();
            if !trimmed.is_empty() {
                ctx.custom_css.push(trimmed);
            }
            return Ok(None);
        }

        // --- @scope block (CSS scoping) ---
        if content.trim() == "@scope" || content.starts_with("@scope ") {
            let scope_selector = content
                .strip_prefix("@scope")
                .unwrap_or("")
                .trim()
                .to_string();
            let mut scope_content = String::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                match &self.lines[self.pos].content {
                    LineContent::Normal(s) => {
                        scope_content.push_str(s.trim());
                        scope_content.push('\n');
                    }
                    LineContent::Raw(s) => {
                        scope_content.push_str(s);
                        scope_content.push('\n');
                    }
                }
                self.pos += 1;
            }
            let trimmed = scope_content.trim().to_string();
            if !trimmed.is_empty() {
                let block = if scope_selector.is_empty() {
                    format!("@scope {{\n{}\n}}", trimmed)
                } else {
                    format!("@scope ({}) {{\n{}\n}}", scope_selector, trimmed)
                };
                ctx.scope_blocks.push(block);
            }
            return Ok(None);
        }

        // --- @starting-style block (entry animations) ---
        if content.trim() == "@starting-style" {
            let mut ss_content = String::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                match &self.lines[self.pos].content {
                    LineContent::Normal(s) => {
                        ss_content.push_str(s.trim());
                        ss_content.push('\n');
                    }
                    LineContent::Raw(s) => {
                        ss_content.push_str(s);
                        ss_content.push('\n');
                    }
                }
                self.pos += 1;
            }
            let trimmed = ss_content.trim().to_string();
            if !trimmed.is_empty() {
                ctx.starting_style_blocks.push(trimmed);
            }
            return Ok(None);
        }

        // --- @markdown block or file (convert markdown to HTML) ---
        if content.trim() == "@markdown" || content.trim().starts_with("@markdown ") {
            let arg = content.trim().strip_prefix("@markdown").unwrap().trim();
            if arg.is_empty() {
                // Inline markdown block: indented children
                let mut md_lines = Vec::new();
                while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                    match &self.lines[self.pos].content {
                        LineContent::Normal(s) => md_lines.push(s.clone()),
                        LineContent::Raw(s) => md_lines.extend(s.lines().map(String::from)),
                    }
                    self.pos += 1;
                }
                let html = markdown_to_html(&md_lines);
                return Ok(Some(vec![Node::Raw(html)]));
            } else {
                // External markdown file: @markdown file.md
                let filename = substitute_vars(arg, &ctx.variables);
                let resolved = match &ctx.base_path {
                    Some(base) => base.join(&filename),
                    None => PathBuf::from(&filename),
                };
                let md_text = if let Some(cached) = ctx.file_cache.get(&resolved) {
                    cached.clone()
                } else {
                    match std::fs::read_to_string(&resolved) {
                        Ok(text) => {
                            ctx.file_cache.insert(resolved.clone(), text.clone());
                            text
                        }
                        Err(e) => {
                            ctx.diagnostics.push(Diagnostic {
                                line: line_num,
                                column: None,
                                message: format!("cannot read markdown '{}': {}", filename, e),
                                severity: Severity::Error,
                                source_line: Some(content.clone()),
                            });
                            return Ok(None);
                        }
                    }
                };
                ctx.included_files.push(resolved);
                let md_lines: Vec<String> = md_text.lines().map(|l| l.to_string()).collect();
                let html = markdown_to_html(&md_lines);
                return Ok(Some(vec![Node::Raw(html)]));
            }
        }

        // --- @manifest (PWA web manifest) ---
        if content.trim() == "@manifest" || content.starts_with("@manifest ") {
            let mut name = content
                .strip_prefix("@manifest")
                .unwrap_or("")
                .trim()
                .to_string();
            if name.is_empty() {
                name = ctx.page_title.clone().unwrap_or_else(|| "App".to_string());
            }
            let name = substitute_vars(&name, &ctx.variables);
            let mut manifest = crate::ast::ManifestConfig {
                name: name.clone(),
                short_name: None,
                start_url: "/".to_string(),
                display: "standalone".to_string(),
                background_color: None,
                theme_color: None,
                description: None,
                icons: Vec::new(),
            };
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    if let Some((key, value)) = trimmed.split_once(' ') {
                        let value = substitute_vars(value.trim(), &ctx.variables);
                        match key.trim() {
                            "short_name" | "short-name" => manifest.short_name = Some(value),
                            "start_url" | "start-url" => manifest.start_url = value,
                            "display" => manifest.display = value,
                            "background_color" | "background-color" => {
                                manifest.background_color = Some(value)
                            }
                            "theme_color" | "theme-color" => manifest.theme_color = Some(value),
                            "description" => manifest.description = Some(value),
                            "icon" => {
                                // icon src sizes (e.g., icon /icon-192.png 192x192)
                                let parts: Vec<&str> = value.splitn(2, ' ').collect();
                                if parts.len() == 2 {
                                    manifest
                                        .icons
                                        .push((parts[0].to_string(), parts[1].to_string()));
                                } else {
                                    manifest.icons.push((value, "192x192".to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                self.pos += 1;
            }
            ctx.manifest = Some(manifest);
            return Ok(None);
        }

        // --- @with $var as alias (temporary variable rebinding) ---
        if let Some(rest) = content.strip_prefix("@with ") {
            let rest = rest.trim();
            // Parse: @with $source as alias
            if let Some((source_part, alias)) = rest.split_once(" as ") {
                let source_name = source_part
                    .trim()
                    .strip_prefix('$')
                    .unwrap_or(source_part.trim());
                let alias = alias.trim();
                ctx.used_variables.insert(source_name.to_string());
                let value = ctx.variables.get(source_name).cloned().unwrap_or_default();
                let old_value = ctx.variables.get(alias).cloned();
                ctx.variables.insert(alias.to_string(), value);
                // Parse body
                let children = self.parse_children(current_indent + 1, ctx);
                // Restore previous value
                if let Some(old) = old_value {
                    ctx.variables.insert(alias.to_string(), old);
                } else {
                    ctx.variables.remove(alias);
                }
                return Ok(Some(children));
            }
            return Err(ParseError {
                line: line_num,
                message: "@with requires: @with $var as alias".to_string(),
            });
        }

        // --- @assert directive (compile-time assertions) ---

        if let Some(rest) = content.strip_prefix("@assert ") {
            let rest = rest.trim();
            let condition = substitute_vars(rest, &ctx.variables);
            track_var_refs(rest, &mut ctx.used_variables);
            if !evaluate_condition(&condition) {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("assertion failed: {}", rest),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                });
            }
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@include ") {
            let filename = substitute_vars(rest.trim(), &ctx.variables);
            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            // Circular include check
            if ctx.include_stack.contains(&resolved) {
                let cycle_chain = format_include_chain(&ctx.include_stack);
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "circular include '{}' (cycle: {} → {})",
                        filename, cycle_chain, filename
                    ),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                });
                return Ok(None);
            }

            // Use file cache to avoid redundant reads
            let included_text = if let Some(cached) = ctx.file_cache.get(&resolved) {
                cached.clone()
            } else {
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => {
                        ctx.file_cache.insert(resolved.clone(), text.clone());
                        text
                    }
                    Err(e) => {
                        ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!("cannot include '{}': {}", filename, e),
                            severity: Severity::Error,
                            source_line: Some(content.clone()),
                        });
                        return Ok(None);
                    }
                }
            };

            ctx.included_files.push(resolved.clone());
            ctx.include_stack.push(resolved.clone());
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            let diag_count_before = ctx.diagnostics.len();
            let included_lines = preprocess(&included_text);
            let mut included_parser = Parser {
                lines: included_lines,
                pos: 0,
            };
            let nodes = included_parser.parse_children(0, ctx);

            // Annotate new diagnostics with include chain
            let include_chain = format_include_chain(&ctx.include_stack);
            for d in &mut ctx.diagnostics[diag_count_before..] {
                d.message = format!("{}\n  in {}", d.message, include_chain);
            }

            ctx.base_path = saved_base;
            ctx.include_stack.pop();
            return Ok(Some(nodes));
        }

        // --- @import (definitions only, no DOM nodes) ---

        if let Some(rest) = content.strip_prefix("@import ") {
            let rest = rest.trim();
            // Support @import "file.hl" as prefix — namespace imported definitions
            let (filename, alias) = if let Some((file_part, alias_part)) = rest.rsplit_once(" as ")
            {
                (
                    file_part.trim().to_string(),
                    Some(alias_part.trim().to_string()),
                )
            } else {
                (rest.to_string(), None)
            };
            // Strip quotes from filename
            let filename =
                if filename.starts_with('"') && filename.ends_with('"') && filename.len() >= 2 {
                    filename[1..filename.len() - 1].to_string()
                } else {
                    filename
                };
            let filename = substitute_vars(&filename, &ctx.variables);

            // Glob support: if filename contains *, expand to multiple imports
            if filename.contains('*') {
                let base_dir = match &ctx.base_path {
                    Some(base) => base.clone(),
                    None => PathBuf::from("."),
                };
                let pattern_path = Path::new(&filename);
                let (glob_dir, glob_pattern) = match pattern_path.parent() {
                    Some(dir) if !dir.as_os_str().is_empty() => (
                        base_dir.join(dir),
                        pattern_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    ),
                    _ => (base_dir.clone(), filename.clone()),
                };
                let matched_files = match std::fs::read_dir(&glob_dir) {
                    Ok(entries) => {
                        let mut files: Vec<PathBuf> = entries
                            .flatten()
                            .filter(|e| {
                                let name = e.file_name().to_string_lossy().to_string();
                                glob_match(&glob_pattern, &name)
                            })
                            .map(|e| e.path())
                            .collect();
                        files.sort();
                        files
                    }
                    Err(e) => {
                        ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!(
                                "cannot read directory for glob '{}': {}",
                                filename, e
                            ),
                            severity: Severity::Error,
                            source_line: Some(content.clone()),
                        });
                        return Ok(None);
                    }
                };
                if matched_files.is_empty() {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("no files matched glob pattern '{}'", filename),
                        severity: Severity::Warning,
                        source_line: Some(content.clone()),
                    });
                    return Ok(None);
                }
                for file_path in matched_files {
                    let rel_name = file_path
                        .strip_prefix(&base_dir)
                        .unwrap_or(&file_path)
                        .to_string_lossy()
                        .to_string();
                    // Synthesize an @import line for each matched file
                    let import_line = match &alias {
                        Some(pfx) => {
                            let stem = file_path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            format!("@import \"{}\" as {}.{}", rel_name, pfx, stem)
                        }
                        None => format!("@import \"{}\"", rel_name),
                    };
                    let synth_lines = preprocess(&import_line);
                    let mut synth_parser = Parser {
                        lines: synth_lines,
                        pos: 0,
                    };
                    let _ = synth_parser.parse_children(0, ctx);
                }
                return Ok(None);
            }

            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            if ctx.include_stack.contains(&resolved) || ctx.import_stack.contains(&resolved) {
                let cycle_chain = format_include_chain(&ctx.import_stack);
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "circular import '{}' (cycle: {} → {})",
                        filename, cycle_chain, filename
                    ),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                });
                return Ok(None);
            }

            let imported_text = if let Some(cached) = ctx.file_cache.get(&resolved) {
                cached.clone()
            } else {
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => {
                        ctx.file_cache.insert(resolved.clone(), text.clone());
                        text
                    }
                    Err(e) => {
                        ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!("cannot import '{}': {}", filename, e),
                            severity: Severity::Error,
                            source_line: Some(content.clone()),
                        });
                        return Ok(None);
                    }
                }
            };

            ctx.included_files.push(resolved.clone());
            ctx.include_stack.push(resolved.clone());
            ctx.import_stack.push(resolved.clone());
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            let diag_count_before = ctx.diagnostics.len();

            if let Some(ref prefix) = alias {
                // Snapshot just the *key sets* before parsing — much cheaper
                // than cloning the full HashMaps, since we only need to know
                // which definitions are new afterwards.
                let fn_keys_before: std::collections::HashSet<String> =
                    ctx.functions.keys().cloned().collect();
                let define_keys_before: std::collections::HashSet<String> =
                    ctx.defines.keys().cloned().collect();
                let var_keys_before: std::collections::HashSet<String> =
                    ctx.variables.keys().cloned().collect();

                let imported_lines = preprocess(&imported_text);
                let mut imported_parser = Parser {
                    lines: imported_lines,
                    pos: 0,
                };
                let _discarded_nodes = imported_parser.parse_children(0, ctx);

                // Remove newly added entries and re-insert them under the prefix.
                let new_fn_keys: Vec<String> = ctx
                    .functions
                    .keys()
                    .filter(|k| !fn_keys_before.contains(*k))
                    .cloned()
                    .collect();
                for name in new_fn_keys {
                    if let Some(def) = ctx.functions.remove(&name) {
                        ctx.functions.insert(format!("{}.{}", prefix, name), def);
                    }
                }
                let new_define_keys: Vec<String> = ctx
                    .defines
                    .keys()
                    .filter(|k| !define_keys_before.contains(*k))
                    .cloned()
                    .collect();
                for name in new_define_keys {
                    if let Some(attrs) = ctx.defines.remove(&name) {
                        ctx.defines.insert(format!("{}.{}", prefix, name), attrs);
                    }
                }
                let new_var_keys: Vec<String> = ctx
                    .variables
                    .keys()
                    .filter(|k| !var_keys_before.contains(*k))
                    .cloned()
                    .collect();
                for name in new_var_keys {
                    if let Some(val) = ctx.variables.remove(&name)
                        && !name.starts_with("__")
                    {
                        ctx.variables.insert(format!("{}.{}", prefix, name), val);
                    }
                }
            } else {
                // Parse the file but discard DOM nodes — only keep definitions
                let imported_lines = preprocess(&imported_text);
                let mut imported_parser = Parser {
                    lines: imported_lines,
                    pos: 0,
                };
                let _discarded_nodes = imported_parser.parse_children(0, ctx);
            }

            // Annotate new diagnostics with import chain
            let import_chain = format_include_chain(&ctx.include_stack);
            for d in &mut ctx.diagnostics[diag_count_before..] {
                d.message = format!("{}\n  in {}", d.message, import_chain);
            }

            ctx.base_path = saved_base;
            ctx.include_stack.pop();
            ctx.import_stack.pop();
            return Ok(None); // No nodes emitted
        }

        // --- @env (compile-time environment variables) ---

        if let Some(rest) = content.strip_prefix("@env ") {
            let rest = rest.trim();
            // @env VAR_NAME default_value  OR  @env VAR_NAME
            let (var_name, default_val) = if let Some((name, default)) = rest.split_once(' ') {
                (name.trim(), Some(default.trim()))
            } else {
                (rest, None)
            };
            let env_val = std::env::var(var_name)
                .ok()
                .or_else(|| default_val.map(|d| substitute_vars(d, &ctx.variables)));
            match env_val {
                Some(val) => {
                    // Store as variable with lowercase name
                    let key = var_name.to_lowercase().replace('-', "_");
                    ctx.variables.insert(key, val);
                }
                None => {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!(
                            "environment variable '{}' is not set and no default provided",
                            var_name
                        ),
                        severity: Severity::Warning,
                        source_line: Some(content.clone()),
                    });
                }
            }
            return Ok(None);
        }

        // --- @fetch (compile-time HTTP data fetching) ---

        if let Some(rest) = content.strip_prefix("@fetch ") {
            let rest = rest.trim();
            // @fetch $prefix https://url  OR  @fetch https://url
            let (prefix, url) = if rest.starts_with('$') {
                if let Some((p, u)) = rest.split_once(' ') {
                    (
                        p.strip_prefix('$').unwrap_or(p).to_string(),
                        u.trim().to_string(),
                    )
                } else {
                    return Err(ParseError {
                        line: line_num,
                        message: "@fetch requires: @fetch $prefix url or @fetch url".to_string(),
                    });
                }
            } else {
                (String::new(), rest.to_string())
            };

            let url = substitute_vars(&url, &ctx.variables);

            // Synchronous HTTP GET using std::net
            match fetch_url_blocking(&url) {
                Ok(body) => {
                    // Try to parse as JSON
                    match parse_json(&body) {
                        Some(json) => {
                            if prefix.is_empty() {
                                if let JsonValue::Object(pairs) = &json {
                                    for (key, val) in pairs {
                                        let mut sub = HashMap::new();
                                        flatten_json(key, val, &mut sub);
                                        for (k, v) in sub {
                                            ctx.variables.insert(k, v);
                                        }
                                    }
                                } else {
                                    // Store the raw body as a single variable
                                    ctx.variables.insert("__fetch_body".to_string(), body);
                                }
                            } else {
                                let mut sub = HashMap::new();
                                flatten_json(&prefix, &json, &mut sub);
                                for (k, v) in sub {
                                    ctx.variables.insert(k, v);
                                }
                            }
                        }
                        None => {
                            // Not JSON — store raw body
                            if prefix.is_empty() {
                                ctx.variables.insert("__fetch_body".to_string(), body);
                            } else {
                                ctx.variables.insert(prefix, body);
                            }
                        }
                    }
                }
                Err(e) => {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("@fetch failed for '{}': {}", url, e),
                        severity: Severity::Error,
                        source_line: Some(content.clone()),
                    });
                }
            }
            return Ok(None);
        }

        // --- @svg (inline SVG from file) ---

        if let Some(rest) = content.strip_prefix("@svg ") {
            let rest = rest.trim();
            // @svg file.svg  OR  @svg [attrs] file.svg
            let (attrs_part, filename) = if rest.starts_with('[') {
                if let Some(bracket_end) = rest.find(']') {
                    let attrs_str = &rest[..=bracket_end];
                    let file = rest[bracket_end + 1..].trim();
                    (Some(attrs_str.to_string()), file.to_string())
                } else {
                    (None, rest.to_string())
                }
            } else {
                (None, rest.to_string())
            };

            let filename = substitute_vars(&filename, &ctx.variables);
            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            match std::fs::read_to_string(&resolved) {
                Ok(svg_content) => {
                    let mut svg = svg_content.trim().to_string();
                    // Apply attributes (width, height, color/fill, class)
                    if let Some(ref attrs_str) = attrs_part {
                        let (attrs, _) = parse_attr_brackets(attrs_str, line_num, ctx)?;
                        for attr in &attrs {
                            match attr.key.as_str() {
                                "width" => {
                                    if let Some(ref val) = attr.value {
                                        svg = set_svg_attr(&svg, "width", val);
                                    }
                                }
                                "height" => {
                                    if let Some(ref val) = attr.value {
                                        svg = set_svg_attr(&svg, "height", val);
                                    }
                                }
                                "color" | "fill" => {
                                    if let Some(ref val) = attr.value {
                                        svg = set_svg_attr(&svg, "fill", val);
                                    }
                                }
                                "class" => {
                                    if let Some(ref val) = attr.value {
                                        svg = set_svg_attr(&svg, "class", val);
                                    }
                                }
                                "id" => {
                                    if let Some(ref val) = attr.value {
                                        svg = set_svg_attr(&svg, "id", val);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    return Ok(Some(vec![Node::Raw(svg)]));
                }
                Err(e) => {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("cannot load SVG '{}': {}", filename, e),
                        severity: Severity::Error,
                        source_line: Some(content.clone()),
                    });
                    return Ok(None);
                }
            }
        }

        // --- @css-property (CSS @property rule for typed custom properties) ---

        if let Some(rest) = content.strip_prefix("@css-property ") {
            let rest = rest.trim();
            // @css-property --name
            //   syntax "<color>"
            //   inherits true
            //   initial-value #000
            let prop_name = substitute_vars(rest, &ctx.variables);
            let mut syntax = String::from("\"*\"");
            let mut inherits = String::from("false");
            let mut initial_value = String::new();

            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    if let Some((key, value)) = trimmed.split_once(' ') {
                        let value = substitute_vars(value.trim(), &ctx.variables);
                        match key.trim() {
                            "syntax" => syntax = value,
                            "inherits" => inherits = value,
                            "initial-value" | "initial_value" => initial_value = value,
                            _ => {}
                        }
                    }
                }
                self.pos += 1;
            }

            let mut rule = format!("@property {} {{", prop_name);
            rule.push_str(&format!("syntax:{};", syntax));
            rule.push_str(&format!("inherits:{};", inherits));
            if !initial_value.is_empty() {
                rule.push_str(&format!("initial-value:{};", initial_value));
            }
            rule.push('}');
            ctx.custom_css.push(rule);
            return Ok(None);
        }

        // --- @data (load JSON file into variables) ---

        if let Some(rest) = content.strip_prefix("@data ") {
            let rest = rest.trim();
            // @data $prefix file.json  OR  @data file.json (no prefix, top-level keys become vars)
            let (prefix, filename) = if rest.starts_with('$') {
                if let Some((p, f)) = rest.split_once(' ') {
                    (
                        p.strip_prefix('$').unwrap_or(p).to_string(),
                        f.trim().to_string(),
                    )
                } else {
                    return Err(ParseError {
                        line: line_num,
                        message: "@data requires: @data $prefix file.json or @data file.json"
                            .to_string(),
                    });
                }
            } else {
                (String::new(), rest.to_string())
            };

            let filename = substitute_vars(&filename, &ctx.variables);
            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            let json_text = match std::fs::read_to_string(&resolved) {
                Ok(text) => text,
                Err(e) => {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("cannot load data '{}': {}", filename, e),
                        severity: Severity::Error,
                        source_line: Some(content.clone()),
                    });
                    return Ok(None);
                }
            };

            match parse_json_with_error(&json_text) {
                Ok(json) => {
                    if prefix.is_empty() {
                        // No prefix: top-level object keys become variables directly
                        if let JsonValue::Object(pairs) = &json {
                            for (key, val) in pairs {
                                let mut sub = HashMap::new();
                                flatten_json(key, val, &mut sub);
                                for (k, v) in sub {
                                    ctx.variables.insert(k, v);
                                }
                            }
                        } else {
                            ctx.diagnostics.push(Diagnostic {
                                line: line_num,
                                column: None,
                                message: "@data without prefix requires a JSON object at top level"
                                    .to_string(),
                                severity: Severity::Error,
                                source_line: Some(content.clone()),
                            });
                        }
                    } else {
                        let mut sub = HashMap::new();
                        flatten_json(&prefix, &json, &mut sub);
                        for (k, v) in sub {
                            ctx.variables.insert(k, v);
                        }
                    }
                }
                Err(detail) => {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("invalid JSON in '{}': {}", filename, detail),
                        severity: Severity::Error,
                        source_line: Some(content.clone()),
                    });
                }
            }

            ctx.included_files.push(resolved);
            return Ok(None);
        }

        // --- @use (selective import) ---

        if let Some(rest) = content.strip_prefix("@use ") {
            let rest = rest.trim();
            // @use "./file.hl" fn1, fn2, define1
            let (filename, names_str) = if let Some(after_quote) = rest.strip_prefix('"') {
                if let Some(end_quote) = after_quote.find('"') {
                    let filename = &after_quote[..end_quote];
                    let names = after_quote[end_quote + 1..].trim();
                    (filename.to_string(), names.to_string())
                } else {
                    return Err(ParseError {
                        line: line_num,
                        message: "@use requires: @use \"file.hl\" name1, name2".to_string(),
                    });
                }
            } else if let Some((filename, names)) = rest.split_once(' ') {
                (filename.to_string(), names.to_string())
            } else {
                return Err(ParseError {
                    line: line_num,
                    message: "@use requires: @use file.hl name1, name2".to_string(),
                });
            };

            let wanted: HashSet<String> = names_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let filename = substitute_vars(&filename, &ctx.variables);
            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            if ctx.include_stack.contains(&resolved) {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("circular use '{}'", filename),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                });
                return Ok(None);
            }

            let use_text = if let Some(cached) = ctx.file_cache.get(&resolved) {
                cached.clone()
            } else {
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => {
                        ctx.file_cache.insert(resolved.clone(), text.clone());
                        text
                    }
                    Err(e) => {
                        ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!("cannot use '{}': {}", filename, e),
                            severity: Severity::Error,
                            source_line: Some(content.clone()),
                        });
                        return Ok(None);
                    }
                }
            };

            ctx.included_files.push(resolved.clone());
            ctx.include_stack.push(resolved.clone());
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            // Parse the file fully, then filter to only keep wanted definitions
            let saved_fns = ctx.functions.clone();
            let saved_defines = ctx.defines.clone();

            let use_lines = preprocess(&use_text);
            let mut use_parser = Parser {
                lines: use_lines,
                pos: 0,
            };
            let _discarded = use_parser.parse_children(0, ctx);

            // Keep only the wanted functions/defines, restore everything else
            let mut new_fns = HashMap::new();
            let mut new_defines = HashMap::new();
            for name in &wanted {
                if let Some(f) = ctx.functions.get(name) {
                    new_fns.insert(name.clone(), f.clone());
                }
                if let Some(d) = ctx.defines.get(name) {
                    new_defines.insert(name.clone(), d.clone());
                }
            }
            ctx.functions = saved_fns;
            ctx.defines = saved_defines;
            ctx.functions.extend(new_fns);
            ctx.defines.extend(new_defines);

            ctx.base_path = saved_base;
            ctx.include_stack.pop();
            return Ok(None);
        }

        // --- @collection (load multiple JSON files matching a glob pattern) ---

        if let Some(rest) = content.strip_prefix("@collection ") {
            let rest = rest.trim();
            // @collection $varname "pattern" OR @collection "pattern" as varname
            let (var_name, pattern) = if rest.starts_with('$') {
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let name = parts[0].strip_prefix('$').unwrap_or(parts[0]);
                    let pat = parts[1].trim().trim_matches('"');
                    (name.to_string(), pat.to_string())
                } else {
                    return Err(ParseError {
                        line: line_num,
                        message: "@collection requires: @collection $var \"pattern\"".to_string(),
                    });
                }
            } else {
                let pat = rest.trim_matches('"');
                ("_items".to_string(), pat.to_string())
            };

            let pattern = substitute_vars(&pattern, &ctx.variables);
            let base = ctx.base_path.clone().unwrap_or_else(|| PathBuf::from("."));
            let glob_pattern = base.join(&pattern);

            // Simple glob: support * in filename part
            let parent = glob_pattern.parent().unwrap_or(Path::new("."));
            let file_pattern = glob_pattern
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            let mut items = Vec::new();
            if let Ok(entries) = std::fs::read_dir(parent) {
                let mut paths: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                            if file_pattern.contains('*') {
                                let parts: Vec<&str> = file_pattern.split('*').collect();
                                if parts.len() == 2 {
                                    name.starts_with(parts[0]) && name.ends_with(parts[1])
                                } else {
                                    true
                                }
                            } else {
                                name == file_pattern
                            }
                        } else {
                            false
                        }
                    })
                    .collect();
                paths.sort();
                for path in &paths {
                    if let Ok(text) = std::fs::read_to_string(path) {
                        // Store the file content as a JSON string for each item
                        let stem = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();
                        items.push((stem, text));
                    }
                }
            }

            // Store item count and serialized data as variables
            ctx.variables
                .insert(format!("{}_count", var_name), items.len().to_string());
            // Store as space-separated list of stems for @each
            let stems: Vec<String> = items.iter().map(|(stem, _)| stem.clone()).collect();
            ctx.variables.insert(var_name.clone(), stems.join(" "));
            // Also parse each JSON file and store keys as prefix_stem_key
            for (stem, text) in &items {
                if let Some(json) = parse_json(text.trim())
                    && let JsonValue::Object(ref obj) = json
                {
                    for (key, val) in obj {
                        let var_key = format!("{}_{}", stem, key);
                        ctx.variables.insert(var_key, json_value_to_string(val));
                    }
                }
            }

            return Ok(None);
        }

        // --- @if / @else ---

        if let Some(rest) = content.strip_prefix("@if ") {
            let rest = rest.trim();
            track_var_refs(rest, &mut ctx.used_variables);
            let condition = substitute_vars(rest, &ctx.variables);
            let result = evaluate_condition(&condition);

            // Collect then-body lines
            let mut then_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                then_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            // Build branches: [(condition_result, body_lines), ...]
            let mut branches: Vec<(bool, Vec<Line>)> = vec![(result, then_lines)];

            // Check for @else if / @else chains at same indent
            loop {
                if self.pos >= self.lines.len() || self.lines[self.pos].indent != current_indent {
                    break;
                }
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    if let Some(else_if_cond) = trimmed.strip_prefix("@else if ") {
                        self.pos += 1; // consume @else if
                        track_var_refs(else_if_cond, &mut ctx.used_variables);
                        let cond = substitute_vars(else_if_cond.trim(), &ctx.variables);
                        let cond_result = evaluate_condition(&cond);
                        let mut body = Vec::new();
                        while self.pos < self.lines.len()
                            && self.lines[self.pos].indent > current_indent
                        {
                            body.push(self.lines[self.pos].clone());
                            self.pos += 1;
                        }
                        branches.push((cond_result, body));
                    } else if trimmed == "@else" {
                        self.pos += 1; // consume @else
                        let mut body = Vec::new();
                        while self.pos < self.lines.len()
                            && self.lines[self.pos].indent > current_indent
                        {
                            body.push(self.lines[self.pos].clone());
                            self.pos += 1;
                        }
                        // @else is always true (fallback)
                        branches.push((true, body));
                        break;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Pick the first branch whose condition is true
            let body_lines = branches
                .into_iter()
                .find(|(cond, _)| *cond)
                .map(|(_, lines)| lines)
                .unwrap_or_default();

            if body_lines.is_empty() {
                return Ok(None);
            }

            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();

            // Scope variables: @let inside @if doesn't leak out
            let saved_vars = ctx.variables.clone();
            let mut body_parser = Parser {
                lines: adjusted,
                pos: 0,
            };
            let nodes = body_parser.parse_children(0, ctx);
            ctx.variables = saved_vars;
            return Ok(Some(nodes));
        }

        if let Some(rest) = content.strip_prefix("@unless ") {
            let rest = rest.trim();
            track_var_refs(rest, &mut ctx.used_variables);
            let condition = substitute_vars(rest, &ctx.variables);
            let result = !evaluate_condition(&condition);
            let mut body_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                body_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }
            if !result || body_lines.is_empty() {
                return Ok(None);
            }
            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();
            let saved_vars = ctx.variables.clone();
            let mut body_parser = Parser {
                lines: adjusted,
                pos: 0,
            };
            let nodes = body_parser.parse_children(0, ctx);
            ctx.variables = saved_vars;
            return Ok(Some(nodes));
        }

        if content.trim() == "@else" || content.trim().starts_with("@else if ") {
            return Err(ParseError {
                line: line_num,
                message: "@else without matching @if".to_string(),
            });
        }

        // --- @each loop ---

        if let Some(rest) = content.strip_prefix("@each ") {
            let rest = rest.trim();
            // Support: @each $var in list  OR  @each $var, $index in list
            // OR  @each $name, $url in Alice /alice, Bob /bob (destructuring)
            let (var_names, list_str) = if let Some((before_in, after_in)) = rest.split_once(" in ")
            {
                let before_in = before_in.trim();
                let vars: Vec<String> = before_in
                    .split(',')
                    .map(|v| v.trim().strip_prefix('$').unwrap_or(v.trim()).to_string())
                    .collect();
                (vars, after_in.trim().to_string())
            } else {
                return Err(ParseError {
                    line: line_num,
                    message: "@each requires: @each $var in list".to_string(),
                });
            };
            let var_name = var_names[0].clone();
            let index_var = var_names.get(1).cloned();

            // Strip a trailing `[page N]` pagination suffix so it doesn't end
            // up in the last item; it's parsed from the raw line below.
            let list_str = match list_str.rfind("[page ") {
                Some(pos) if list_str.trim_end().ends_with(']') => {
                    list_str[..pos].trim_end().to_string()
                }
                _ => list_str,
            };
            track_var_refs(&list_str, &mut ctx.used_variables);
            let list_str = substitute_vars(&list_str, &ctx.variables);
            // Support range syntax: @each $i in 1..5  or  @each $i in 0..100 step 10
            let items: Vec<String> = if let Some((start_s, rest)) = list_str.split_once("..") {
                let (end_s, step) = if let Some((e, s)) = rest.split_once(" step ") {
                    (e.trim(), s.trim().parse::<i64>().unwrap_or(1).max(1))
                } else {
                    (rest.trim(), 1i64)
                };
                if let (Ok(start), Ok(end)) = (start_s.trim().parse::<i64>(), end_s.parse::<i64>())
                {
                    numeric_range(start, end, step)
                } else {
                    list_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                }
            } else {
                list_str
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };

            // Pagination: check for [page N] or [page N per P] suffix
            // Parse from the original rest before substitution
            let (items, pagination_info) = {
                // Check if list_str ended with a page directive (already consumed in items)
                // Instead, check the raw rest for [page ...] syntax
                let raw_rest = rest;
                let page_size = if let Some(page_pos) = raw_rest.find("[page ") {
                    let after = &raw_rest[page_pos + 6..];
                    if let Some(close) = after.find(']') {
                        let page_spec = after[..close].trim();
                        page_spec.parse::<usize>().ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(size) = page_size {
                    let current_page: usize = ctx
                        .variables
                        .get("_page")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1)
                        .max(1);
                    let size = size.max(1);
                    let total_items = items.len();
                    let total_pages = total_items.div_ceil(size);
                    let start = (current_page - 1).saturating_mul(size);
                    let end = start.saturating_add(size).min(total_items);
                    let page_items: Vec<String> = if start < total_items {
                        items[start..end].to_vec()
                    } else {
                        Vec::new()
                    };
                    (
                        page_items,
                        Some((current_page, total_pages, size, total_items)),
                    )
                } else {
                    (items, None)
                }
            };

            // Collect body lines
            let mut body_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                body_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            // Check for @else block (empty-state fallback)
            let mut else_lines = Vec::new();
            if self.pos < self.lines.len()
                && self.lines[self.pos].indent == current_indent
                && let LineContent::Normal(ref s) = self.lines[self.pos].content
                && s.trim() == "@else"
            {
                self.pos += 1; // consume @else
                while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                    else_lines.push(self.lines[self.pos].clone());
                    self.pos += 1;
                }
            }

            if body_lines.is_empty() {
                return Ok(None);
            }

            // If list is empty, render @else body
            if items.is_empty() {
                if else_lines.is_empty() {
                    return Ok(None);
                }
                let min_indent = else_lines.iter().map(|l| l.indent).min().unwrap_or(0);
                let adjusted: Vec<Line> = else_lines
                    .iter()
                    .map(|l| Line {
                        indent: l.indent - min_indent,
                        content: l.content.clone(),
                        line_num: l.line_num,
                    })
                    .collect();
                let mut body_parser = Parser {
                    lines: adjusted,
                    pos: 0,
                };
                let saved_vars = ctx.variables.clone();
                let nodes = body_parser.parse_children(0, ctx);
                ctx.variables = saved_vars;
                return Ok(Some(nodes));
            }

            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();

            let saved_vars = ctx.variables.clone();
            let mut all_nodes = Vec::new();

            // Inject pagination variables if pagination is active
            if let Some((page, total_pages, page_size, total_items)) = pagination_info {
                ctx.variables.insert("_page".to_string(), page.to_string());
                ctx.variables
                    .insert("_total_pages".to_string(), total_pages.to_string());
                ctx.variables
                    .insert("_page_size".to_string(), page_size.to_string());
                ctx.variables
                    .insert("_total_items".to_string(), total_items.to_string());
            }

            let has_extra_vars = var_names.len() > 2
                || (var_names.len() == 2 && items.first().is_some_and(|it| it.contains(' ')));

            for (i, item) in items.iter().enumerate() {
                // Always expose $_index for the current iteration
                ctx.variables.insert("_index".to_string(), i.to_string());
                if has_extra_vars {
                    // Destructuring: split item by spaces and assign to each variable
                    let parts: Vec<&str> = item.splitn(var_names.len(), ' ').collect();
                    for (vi, vn) in var_names.iter().enumerate() {
                        let val = parts.get(vi).unwrap_or(&"").to_string();
                        ctx.variables.insert(vn.clone(), val);
                    }
                } else {
                    ctx.variables.insert(var_name.clone(), item.clone());
                    if let Some(ref idx_name) = index_var {
                        ctx.variables.insert(idx_name.clone(), i.to_string());
                    }
                }
                let mut body_parser = Parser {
                    lines: adjusted.clone(),
                    pos: 0,
                };
                let nodes = body_parser.parse_children(0, ctx);
                all_nodes.extend(nodes);
            }

            ctx.variables = saved_vars;
            return Ok(Some(all_nodes));
        }

        // --- @for numeric loop ---

        if let Some(rest) = content.strip_prefix("@for ") {
            let rest = rest.trim();
            // @for $var in start..end  OR  @for $var in start..end step N
            let (var_name, range_str) = if let Some((before_in, after_in)) = rest.split_once(" in ")
            {
                let var = before_in
                    .trim()
                    .strip_prefix('$')
                    .unwrap_or(before_in.trim());
                (var.to_string(), after_in.trim().to_string())
            } else {
                return Err(ParseError {
                    line: line_num,
                    message: "@for requires: @for $var in start..end".to_string(),
                });
            };
            track_var_refs(&range_str, &mut ctx.used_variables);
            let range_str = substitute_vars(&range_str, &ctx.variables);

            let items: Vec<String> = if let Some((start_s, rest_range)) = range_str.split_once("..")
            {
                let (end_s, step) = if let Some((e, s)) = rest_range.split_once(" step ") {
                    (e.trim(), s.trim().parse::<i64>().unwrap_or(1).max(1))
                } else {
                    (rest_range.trim(), 1i64)
                };
                if let (Ok(start), Ok(end)) = (start_s.trim().parse::<i64>(), end_s.parse::<i64>())
                {
                    numeric_range(start, end, step)
                } else {
                    return Err(ParseError {
                        line: line_num,
                        message: "@for range requires numeric bounds: @for $i in 1..10".to_string(),
                    });
                }
            } else {
                return Err(ParseError {
                    line: line_num,
                    message: "@for requires range syntax: @for $i in 1..10".to_string(),
                });
            };

            // Collect body lines
            let mut body_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                body_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            if body_lines.is_empty() || items.is_empty() {
                return Ok(None);
            }

            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();

            let saved_vars = ctx.variables.clone();
            let mut all_nodes = Vec::new();
            for item in &items {
                ctx.variables.insert(var_name.clone(), item.clone());
                let mut body_parser = Parser {
                    lines: adjusted.clone(),
                    pos: 0,
                };
                let nodes = body_parser.parse_children(0, ctx);
                all_nodes.extend(nodes);
            }
            ctx.variables = saved_vars;
            return Ok(Some(all_nodes));
        }

        // --- @repeat N (simple repetition) ---

        if let Some(rest) = content.strip_prefix("@repeat ") {
            let rest = rest.trim();
            let count_str = substitute_vars(rest, &ctx.variables);
            track_var_refs(rest, &mut ctx.used_variables);
            let count: usize = count_str.parse().unwrap_or(0);

            // Collect body lines
            let mut body_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                body_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            if body_lines.is_empty() || count == 0 {
                return Ok(None);
            }

            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();

            let mut all_nodes = Vec::new();
            let saved_vars = ctx.variables.clone();
            for i in 0..count {
                ctx.variables.insert("_index".to_string(), i.to_string());
                ctx.variables
                    .insert("_count".to_string(), count.to_string());
                let mut iter_parser = Parser {
                    lines: adjusted.clone(),
                    pos: 0,
                };
                let nodes = iter_parser.parse_children(0, ctx);
                all_nodes.extend(nodes);
            }
            ctx.variables = saved_vars;
            return Ok(Some(all_nodes));
        }

        // --- @defer (lazy-load below-fold content via IntersectionObserver) ---

        if content.trim() == "@defer" || content.starts_with("@defer ") {
            let placeholder_text = content.strip_prefix("@defer").unwrap_or("").trim();

            // Collect body lines
            let mut body_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                body_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            if body_lines.is_empty() {
                return Ok(None);
            }

            // Parse the body to get the actual content nodes
            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();

            let saved_vars = ctx.variables.clone();
            let mut body_parser = Parser {
                lines: adjusted,
                pos: 0,
            };
            let inner_nodes = body_parser.parse_children(0, ctx);
            ctx.variables = saved_vars;

            // Wrap content in a div with data-defer attribute + placeholder
            let _placeholder = if placeholder_text.is_empty() {
                "Loading...".to_string()
            } else {
                substitute_vars(placeholder_text, &ctx.variables)
            };
            let mut wrapper_attrs = vec![
                Attribute {
                    key: "data-hl-defer".to_string(),
                    value: None,
                },
                Attribute {
                    key: "class".to_string(),
                    value: Some("hl-defer-placeholder".to_string()),
                },
            ];
            // Hidden until intersection observer triggers
            wrapper_attrs.push(Attribute {
                key: "hidden".to_string(),
                value: None,
            });

            let wrapper = Element {
                kind: ElementKind::El,
                attrs: wrapper_attrs,
                argument: None,
                children: inner_nodes,
                line_num,
            };

            // Also emit the IntersectionObserver script as a sibling raw node
            let script = Node::Raw("<script>(function(){var d=document.querySelectorAll('[data-hl-defer]');if(!d.length)return;var o=new IntersectionObserver(function(e){e.forEach(function(i){if(i.isIntersecting){i.target.removeAttribute('hidden');i.target.classList.remove('hl-defer-placeholder');o.unobserve(i.target)}});},{rootMargin:'200px'});d.forEach(function(el){o.observe(el)})})()</script>".to_string());

            return Ok(Some(vec![Node::Element(wrapper), script]));
        }

        // --- @match ---

        if let Some(rest) = content.strip_prefix("@match ") {
            let match_val = substitute_vars(rest.trim(), &ctx.variables);
            track_var_refs(rest.trim(), &mut ctx.used_variables);

            // Collect all child lines
            let mut match_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                match_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            if match_lines.is_empty() {
                return Ok(None);
            }

            let case_indent = match_lines[0].indent;

            // Group into cases: (Some(value), body) or (None, body) for @default
            let mut cases: Vec<(Option<String>, Vec<Line>)> = Vec::new();
            let mut mi = 0;
            while mi < match_lines.len() {
                if match_lines[mi].indent == case_indent {
                    if let LineContent::Normal(ref s) = match_lines[mi].content {
                        let trimmed = s.trim();
                        if let Some(case_val) = trimmed.strip_prefix("@case ") {
                            let case_val = substitute_vars(case_val.trim(), &ctx.variables);
                            mi += 1;
                            let mut body = Vec::new();
                            while mi < match_lines.len() && match_lines[mi].indent > case_indent {
                                body.push(match_lines[mi].clone());
                                mi += 1;
                            }
                            cases.push((Some(case_val), body));
                        } else if trimmed == "@default" {
                            mi += 1;
                            let mut body = Vec::new();
                            while mi < match_lines.len() && match_lines[mi].indent > case_indent {
                                body.push(match_lines[mi].clone());
                                mi += 1;
                            }
                            cases.push((None, body));
                        } else {
                            mi += 1;
                        }
                    } else {
                        mi += 1;
                    }
                } else {
                    mi += 1;
                }
            }

            // Find first matching case or @default
            let body_lines = cases
                .into_iter()
                .find(|(case_val, _)| match case_val {
                    Some(v) => *v == match_val,
                    None => true,
                })
                .map(|(_, lines)| lines)
                .unwrap_or_default();

            if body_lines.is_empty() {
                return Ok(None);
            }

            let min_indent = body_lines.iter().map(|l| l.indent).min().unwrap_or(0);
            let adjusted: Vec<Line> = body_lines
                .iter()
                .map(|l| Line {
                    indent: l.indent - min_indent,
                    content: l.content.clone(),
                    line_num: l.line_num,
                })
                .collect();

            let saved_vars = ctx.variables.clone();
            let mut body_parser = Parser {
                lines: adjusted,
                pos: 0,
            };
            let nodes = body_parser.parse_children(0, ctx);
            ctx.variables = saved_vars;
            return Ok(Some(nodes));
        }

        // --- @switch (variant-based attribute switching) ---
        // @switch $variant
        //   @case primary [background #3b82f6, color white]
        //   @case danger [background #ef4444, color white]
        //   @default [background #gray, color black]
        // Registers matching @define and emits the matching case's children

        if let Some(rest) = content.strip_prefix("@switch ") {
            let switch_val = substitute_vars(rest.trim(), &ctx.variables);
            track_var_refs(rest.trim(), &mut ctx.used_variables);

            let mut switch_lines = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                switch_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }

            if switch_lines.is_empty() {
                return Ok(None);
            }

            let case_indent = switch_lines[0].indent;
            let mut si = 0;
            let mut matched_attrs: Option<Vec<Attribute>> = None;
            let mut matched_body: Vec<Line> = Vec::new();

            while si < switch_lines.len() {
                if switch_lines[si].indent == case_indent {
                    if let LineContent::Normal(ref s) = switch_lines[si].content {
                        let trimmed = s.trim();
                        if let Some(case_rest) = trimmed.strip_prefix("@case ") {
                            let case_rest = case_rest.trim();
                            // Extract case value and optional [attrs]
                            let (case_val, case_attrs_str) =
                                if let Some(bracket_pos) = case_rest.find('[') {
                                    (
                                        substitute_vars(
                                            case_rest[..bracket_pos].trim(),
                                            &ctx.variables,
                                        ),
                                        Some(&case_rest[bracket_pos..]),
                                    )
                                } else {
                                    (substitute_vars(case_rest, &ctx.variables), None)
                                };
                            si += 1;
                            // Collect body lines
                            let mut body = Vec::new();
                            while si < switch_lines.len() && switch_lines[si].indent > case_indent {
                                body.push(switch_lines[si].clone());
                                si += 1;
                            }
                            if matched_attrs.is_none() && case_val == switch_val {
                                if let Some(attrs_str) = case_attrs_str {
                                    let (attrs, _) = parse_attr_brackets(attrs_str, line_num, ctx)?;
                                    matched_attrs = Some(attrs);
                                }
                                matched_body = body;
                            }
                        } else if trimmed == "@default" || trimmed.starts_with("@default ") {
                            let default_rest =
                                trimmed.strip_prefix("@default").unwrap_or("").trim();
                            si += 1;
                            let mut body = Vec::new();
                            while si < switch_lines.len() && switch_lines[si].indent > case_indent {
                                body.push(switch_lines[si].clone());
                                si += 1;
                            }
                            if matched_attrs.is_none() {
                                if !default_rest.is_empty() && default_rest.starts_with('[') {
                                    let (attrs, _) =
                                        parse_attr_brackets(default_rest, line_num, ctx)?;
                                    matched_attrs = Some(attrs);
                                }
                                matched_body = body;
                            }
                        } else {
                            si += 1;
                        }
                    } else {
                        si += 1;
                    }
                } else {
                    si += 1;
                }
            }

            // Store matched attrs as a temporary define for use with $__switch
            if let Some(attrs) = matched_attrs {
                ctx.defines.insert("__switch".to_string(), attrs);
            }

            // Parse matched body if any
            if !matched_body.is_empty() {
                let min_indent = matched_body.iter().map(|l| l.indent).min().unwrap_or(0);
                let adjusted: Vec<Line> = matched_body
                    .iter()
                    .map(|l| Line {
                        indent: l.indent - min_indent,
                        content: l.content.clone(),
                        line_num: l.line_num,
                    })
                    .collect();
                let saved_vars = ctx.variables.clone();
                let mut body_parser = Parser {
                    lines: adjusted,
                    pos: 0,
                };
                let nodes = body_parser.parse_children(0, ctx);
                ctx.variables = saved_vars;
                return Ok(Some(nodes));
            }

            return Ok(None);
        }

        // --- @warn / @debug ---

        if let Some(rest) = content.strip_prefix("@warn ") {
            let msg = substitute_vars(rest.trim(), &ctx.variables);
            ctx.diagnostics.push(Diagnostic {
                line: line_num,
                column: None,
                message: msg,
                severity: Severity::Warning,
                source_line: Some(content.clone()),
            });
            return Ok(None);
        }

        if let Some(rest) = content.strip_prefix("@debug ") {
            let msg = substitute_vars(rest.trim(), &ctx.variables);
            eprintln!("debug: line {}: {}", line_num, msg);
            return Ok(None);
        }

        // --- @log (compile-time variable inspection) ---

        if let Some(rest) = content.strip_prefix("@log ") {
            let rest = rest.trim();
            // @log $var — shows variable name, value, and type info
            let var_names: Vec<&str> = rest.split_whitespace().collect();
            for var_name in &var_names {
                let name = var_name.strip_prefix('$').unwrap_or(var_name);
                ctx.used_variables.insert(name.to_string());
                if let Some(val) = ctx.variables.get(name) {
                    let kind = if val.parse::<f64>().is_ok() {
                        "number"
                    } else if val.starts_with('#') && is_valid_hex_color(val) {
                        "color"
                    } else if val.contains('/') || val.ends_with(".hl") || val.ends_with(".html") {
                        "path"
                    } else {
                        "string"
                    };
                    eprintln!("log: line {}: ${} = \"{}\" ({})", line_num, name, val, kind);
                } else if ctx.functions.contains_key(name) {
                    let def = &ctx.functions[name];
                    let params: Vec<String> =
                        def.params.iter().map(|p| format!("${}", p)).collect();
                    eprintln!(
                        "log: line {}: @{} ({}) — {} line(s)",
                        line_num,
                        name,
                        params.join(", "),
                        def.body_lines.len()
                    );
                } else if ctx.defines.contains_key(name) {
                    let attrs = &ctx.defines[name];
                    let attr_strs: Vec<String> = attrs
                        .iter()
                        .map(|a| match &a.value {
                            Some(v) => format!("{} {}", a.key, v),
                            None => a.key.clone(),
                        })
                        .collect();
                    eprintln!(
                        "log: line {}: ${} = [{}]",
                        line_num,
                        name,
                        attr_strs.join(", ")
                    );
                } else {
                    eprintln!("log: line {}: ${} is undefined", line_num, name);
                }
            }
            return Ok(None);
        }

        // --- @keyframes directive ---

        if let Some(rest) = content.strip_prefix("@keyframes ") {
            let name = rest.trim().to_string();
            if name.is_empty() {
                return Err(ParseError {
                    line: line_num,
                    message: "@keyframes requires a name".to_string(),
                });
            }
            // Collect body lines (indented deeper)
            let mut body = String::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    // Support htmlang-style: from [opacity 0] / to [opacity 1] / 50% [transform scale(1.5)]
                    if let Some(kf_css) = parse_keyframe_line(trimmed) {
                        body.push_str(&kf_css);
                    } else {
                        body.push_str(trimmed);
                    }
                }
                self.pos += 1;
            }
            ctx.keyframes.push((name, body));
            return Ok(None);
        }

        // --- @theme directive ---

        if content.trim() == "@theme" {
            let mut tokens = Vec::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    if let Some((name, value)) = trimmed.split_once(' ') {
                        let name = name.trim().to_string();
                        let value = value.trim().to_string();
                        tokens.push((name.clone(), value.clone()));
                        // Set as regular variable
                        ctx.variables.insert(name.clone(), value.clone());
                        // Also set as CSS custom property
                        let css_name = format!("--{}", name);
                        ctx.css_vars.push((css_name, value));
                    }
                }
                self.pos += 1;
            }
            ctx.theme_tokens = tokens;
            return Ok(None);
        }

        // --- @canonical directive ---

        if let Some(rest) = content.strip_prefix("@canonical ") {
            ctx.canonical = Some(substitute_vars(rest.trim(), &ctx.variables));
            return Ok(None);
        }

        // --- @base directive ---

        if let Some(rest) = content.strip_prefix("@base ") {
            ctx.base_url = Some(substitute_vars(rest.trim(), &ctx.variables));
            return Ok(None);
        }

        // --- @font-face directive ---

        if let Some(rest) = content.strip_prefix("@font-face ") {
            let rest = rest.trim();
            if let Some((name, url)) = rest.split_once(' ') {
                ctx.font_faces.push((
                    substitute_vars(name.trim(), &ctx.variables),
                    substitute_vars(url.trim(), &ctx.variables),
                ));
            }
            return Ok(None);
        }

        // --- @json-ld block ---

        if content.trim() == "@json-ld" {
            let mut block = String::new();
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                match &self.lines[self.pos].content {
                    LineContent::Normal(s) => {
                        block.push_str(s.trim());
                        block.push('\n');
                    }
                    LineContent::Raw(s) => {
                        block.push_str(s);
                        block.push('\n');
                    }
                }
                self.pos += 1;
            }
            let trimmed = block.trim().to_string();
            if !trimmed.is_empty() {
                ctx.json_ld_blocks.push(trimmed);
            }
            return Ok(None);
        }

        // --- @translations (i18n) ---

        if content.trim() == "@translations" || content.starts_with("@translations ") {
            let locale = content
                .strip_prefix("@translations")
                .unwrap_or("")
                .trim()
                .to_string();
            let active_locale = if locale.is_empty() {
                ctx.lang.clone().unwrap_or_else(|| "en".to_string())
            } else {
                locale
            };

            // Collect indented body: key value pairs, grouped by locale headers
            let mut current_locale = active_locale.clone();
            let mut first_locale_indent: Option<usize> = None;
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    // Check for locale sub-header: e.g., "en:" or "fr:" or "es:"
                    if trimmed.ends_with(':') && trimmed.len() <= 10 && !trimmed.contains(' ') {
                        current_locale = trimmed[..trimmed.len() - 1].to_string();
                        first_locale_indent = Some(self.lines[self.pos].indent);
                        self.pos += 1;
                        continue;
                    }
                    // key value pair
                    if let Some((key, value)) = trimmed.split_once(' ') {
                        let key = key.trim().to_string();
                        let value = substitute_vars(value.trim(), &ctx.variables);
                        ctx.translations
                            .entry(current_locale.clone())
                            .or_default()
                            .insert(key, value);
                    }
                }
                self.pos += 1;
            }
            // Set the active locale
            if ctx.active_locale.is_none() {
                ctx.active_locale = Some(active_locale.clone());
            }
            let _ = first_locale_indent; // suppress unused
            // Inject active locale's translations as $t.key variables
            if let Some(strings) = ctx.translations.get(&active_locale) {
                for (key, value) in strings {
                    ctx.variables.insert(format!("t.{}", key), value.clone());
                }
            }
            return Ok(None);
        }

        // --- @deprecated annotation ---

        if let Some(rest) = content.strip_prefix("@deprecated ") {
            let message = rest.trim().to_string();
            // Peek at the next line to get the function name
            if self.pos < self.lines.len()
                && let LineContent::Normal(ref s) = self.lines[self.pos].content
                && let Some(fn_rest) = s.trim().strip_prefix("@let ")
            {
                let parts: Vec<&str> = fn_rest.split_whitespace().collect();
                if let Some(&fn_name) = parts.first() {
                    ctx.deprecated_fns.insert(fn_name.to_string(), message);
                }
            }
            return Ok(None);
        }

        // --- @extends (template inheritance) ---

        if let Some(rest) = content.strip_prefix("@extends ") {
            let filename = substitute_vars(rest.trim(), &ctx.variables);
            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            if ctx.include_stack.contains(&resolved) {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("circular extends '{}'", filename),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                });
                return Ok(None);
            }

            let extends_text = if let Some(cached) = ctx.file_cache.get(&resolved) {
                cached.clone()
            } else {
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => {
                        ctx.file_cache.insert(resolved.clone(), text.clone());
                        text
                    }
                    Err(e) => {
                        ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!("cannot extend '{}': {}", filename, e),
                            severity: Severity::Error,
                            source_line: Some(content.clone()),
                        });
                        return Ok(None);
                    }
                }
            };

            // Collect slot blocks defined in the extending file
            let mut slot_contents: HashMap<String, Vec<Line>> = HashMap::new();
            while self.pos < self.lines.len() {
                let line_indent = self.lines[self.pos].indent;
                if line_indent < current_indent {
                    break;
                }
                if let LineContent::Normal(ref s) = self.lines[self.pos].content {
                    let trimmed = s.trim();
                    if let Some(slot_name) = trimmed.strip_prefix("@slot ") {
                        let slot_name = slot_name.trim().to_string();
                        self.pos += 1;
                        let mut slot_lines = Vec::new();
                        while self.pos < self.lines.len()
                            && self.lines[self.pos].indent > line_indent
                        {
                            slot_lines.push(self.lines[self.pos].clone());
                            self.pos += 1;
                        }
                        slot_contents.insert(slot_name, slot_lines);
                        continue;
                    }
                }
                self.pos += 1;
            }

            // Parse slot contents into nodes
            let mut slot_nodes: HashMap<String, Vec<Node>> = HashMap::new();
            for (name, lines) in &slot_contents {
                if lines.is_empty() {
                    continue;
                }
                let min_indent = lines.iter().map(|l| l.indent).min().unwrap_or(0);
                let adjusted: Vec<Line> = lines
                    .iter()
                    .map(|l| Line {
                        indent: l.indent - min_indent,
                        content: l.content.clone(),
                        line_num: l.line_num,
                    })
                    .collect();
                let mut slot_parser = Parser {
                    lines: adjusted,
                    pos: 0,
                };
                let nodes = slot_parser.parse_children(0, ctx);
                slot_nodes.insert(name.clone(), nodes);
            }

            // Parse the base layout file
            ctx.included_files.push(resolved.clone());
            ctx.include_stack.push(resolved.clone());
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            let extends_lines = preprocess(&extends_text);
            let mut extends_parser = Parser {
                lines: extends_lines,
                pos: 0,
            };
            let layout_nodes = extends_parser.parse_children(0, ctx);

            ctx.base_path = saved_base;
            ctx.include_stack.pop();

            // Replace @slot placeholders in layout with provided content
            let result_nodes = replace_extends_slots(layout_nodes, &slot_nodes);
            return Ok(Some(result_nodes));
        }

        // --- @layout (lightweight layout wrapper using @children) ---

        if let Some(rest) = content.strip_prefix("@layout ") {
            let filename = substitute_vars(rest.trim(), &ctx.variables);
            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

            if ctx.include_stack.contains(&resolved) {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("circular layout '{}'", filename),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                });
                return Ok(None);
            }

            let layout_text = if let Some(cached) = ctx.file_cache.get(&resolved) {
                cached.clone()
            } else {
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => {
                        ctx.file_cache.insert(resolved.clone(), text.clone());
                        text
                    }
                    Err(e) => {
                        ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!("cannot load layout '{}': {}", filename, e),
                            severity: Severity::Error,
                            source_line: Some(content.clone()),
                        });
                        return Ok(None);
                    }
                }
            };

            // Parse the body content (children of @layout)
            let caller_children = self.parse_children(current_indent + 1, ctx);

            // Parse the layout file
            ctx.included_files.push(resolved.clone());
            ctx.include_stack.push(resolved.clone());
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            let layout_lines = preprocess(&layout_text);
            let mut layout_parser = Parser {
                lines: layout_lines,
                pos: 0,
            };
            let layout_nodes = layout_parser.parse_children(0, ctx);

            ctx.base_path = saved_base;
            ctx.include_stack.pop();

            // Replace @children in layout with the caller's body content
            let empty_slots: HashMap<String, Vec<Node>> = HashMap::new();
            let result_nodes =
                replace_children_and_slots(layout_nodes, &caller_children, &empty_slots);
            return Ok(Some(result_nodes));
        }

        // --- @component definition (scoped @fn) ---

        if let Some(rest) = content.strip_prefix("@component ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() {
                return Err(ParseError {
                    line: line_num,
                    message: "@component requires a name".to_string(),
                });
            }
            let name = parts[0].to_string();
            let mut params = Vec::new();
            let mut defaults = HashMap::new();
            for part in &parts[1..] {
                let part = part.strip_prefix('$').unwrap_or(part);
                if let Some((param_name, default_val)) = part.split_once('=') {
                    params.push(param_name.to_string());
                    defaults.insert(param_name.to_string(), default_val.to_string());
                } else {
                    params.push(part.to_string());
                }
            }

            // Collect body lines and separate @style blocks from element content
            let mut body_lines = Vec::new();
            let mut style_lines = Vec::new();
            let mut in_style = false;
            let mut style_indent = 0;
            while self.pos < self.lines.len() && self.lines[self.pos].indent > current_indent {
                if let LineContent::Normal(ref s) = self.lines[self.pos].content
                    && s.trim() == "@style"
                {
                    in_style = true;
                    style_indent = self.lines[self.pos].indent;
                    self.pos += 1;
                    continue;
                }
                if in_style && self.lines[self.pos].indent > style_indent {
                    style_lines.push(self.lines[self.pos].clone());
                } else {
                    in_style = false;
                    body_lines.push(self.lines[self.pos].clone());
                }
                self.pos += 1;
            }

            // Register scoped CSS from @style blocks
            if !style_lines.is_empty() {
                let scope_class = format!("hl-{}", name);
                let mut scoped_css = String::new();
                for line in &style_lines {
                    if let LineContent::Normal(ref s) = line.content {
                        let trimmed = s.trim();
                        // Scope selectors by prepending .hl-componentname
                        if !trimmed.is_empty() {
                            scoped_css.push_str(&format!(".{} {}\n", scope_class, trimmed));
                        }
                    }
                }
                if !scoped_css.is_empty() {
                    ctx.custom_css.push(scoped_css);
                }
            }

            ctx.fn_lines.entry(name.clone()).or_insert(line_num);
            ctx.functions.insert(
                name.clone(),
                FnDef {
                    params,
                    defaults,
                    body_lines,
                },
            );
            // Mark that this function should wrap output with a scoped class
            ctx.variables.insert(
                format!("__component_scope_{}", name),
                format!("hl-{}", name),
            );
            return Ok(None);
        }

        // --- Function call ---

        if content.starts_with('@') {
            let name = extract_element_name(&content);
            if ctx.functions.contains_key(name) {
                ctx.used_functions.insert(name.to_string());
                // Emit deprecation warning if function is marked @deprecated
                if let Some(msg) = ctx.deprecated_fns.get(name) {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("@{} is deprecated: {}", name, msg),
                        severity: Severity::Warning,
                        source_line: Some(content.clone()),
                    });
                }
                let nodes = self.expand_fn_call(name, &content, current_indent, line_num, ctx)?;
                return Ok(Some(nodes));
            }
        }

        // --- Elements ---

        if content.starts_with('@') || content.starts_with('[') {
            let node = self.parse_element_line(&content, current_indent, line_num, ctx)?;
            return Ok(Some(vec![node]));
        }

        // --- Bare text ---

        let var_warnings =
            check_undefined_vars(&content, &ctx.variables, line_num, current_indent);
        ctx.diagnostics.extend(var_warnings);
        track_var_refs(&content, &mut ctx.used_variables);
        let segments = parse_text_segments(&content, ctx);
        Ok(Some(vec![Node::Text(segments)]))
    }

    fn expand_fn_call(
        &mut self,
        name: &str,
        content: &str,
        current_indent: usize,
        line_num: usize,
        ctx: &mut ParseContext,
    ) -> Result<Vec<Node>, ParseError> {
        // Recursive function cycle detection
        if ctx.fn_call_stack.contains(&name.to_string()) {
            return Err(ParseError {
                line: line_num,
                message: format!(
                    "recursive function call to @{} (call stack: {})",
                    name,
                    ctx.fn_call_stack.join(" -> ")
                ),
            });
        }
        ctx.fn_call_stack.push(name.to_string());

        // Parse [param value, ...] arguments
        let rest = &content[1 + name.len()..];
        let rest = rest.trim_start();

        let args = if rest.starts_with('[') {
            let (attrs, _) = parse_attr_brackets_no_validate(rest, line_num, ctx)?;
            attrs
        } else {
            Vec::new()
        };

        // Clone function definition (releases borrow on ctx). If the function is
        // missing (shouldn't normally happen — callers check ctx.functions.contains_key
        // first — but we avoid panicking on malformed state).
        let fn_def = match ctx.functions.get(name) {
            Some(def) => def.clone(),
            None => {
                ctx.fn_call_stack.pop();
                return Err(ParseError {
                    line: line_num,
                    message: format!("undefined function @{}", name),
                });
            }
        };

        // Parse caller's children, separating named slots from default children
        let all_caller_children = self.parse_children(current_indent + 1, ctx);
        let mut slot_contents: HashMap<String, Vec<Node>> = HashMap::new();
        let mut caller_children = Vec::new();
        for child in all_caller_children {
            if let Node::Element(ref elem) = child
                && let ElementKind::Slot(ref slot_name) = elem.kind
                && !slot_name.is_empty()
            {
                slot_contents
                    .entry(slot_name.clone())
                    .or_default()
                    .extend(elem.children.clone());
                continue;
            }
            caller_children.push(child);
        }

        // Save variable state, inject function parameters
        let saved_vars = ctx.variables.clone();
        for (i, param) in fn_def.params.iter().enumerate() {
            let value = args
                .iter()
                .find(|a| a.key == *param)
                .and_then(|a| a.value.clone())
                .or_else(|| {
                    // Positional fallback, unless that argument is named for
                    // a different parameter.
                    args.get(i)
                        .filter(|a| !fn_def.params.contains(&a.key))
                        .and_then(|a| a.value.clone())
                })
                .or_else(|| fn_def.defaults.get(param).cloned())
                .unwrap_or_default();
            ctx.variables.insert(param.clone(), value);
        }

        // Normalize body indentation so it parses from indent 0
        let min_indent = fn_def
            .body_lines
            .iter()
            .map(|l| l.indent)
            .min()
            .unwrap_or(0);
        let adjusted: Vec<Line> = fn_def
            .body_lines
            .iter()
            .map(|l| Line {
                indent: l.indent - min_indent,
                content: l.content.clone(),
                line_num: l.line_num,
            })
            .collect();

        // Parse body with params in scope
        let mut body_parser = Parser {
            lines: adjusted,
            pos: 0,
        };
        let body_nodes = body_parser.parse_children(0, ctx);

        // Restore variables and call stack
        ctx.variables = saved_vars;
        ctx.fn_call_stack.pop();

        // Replace @children with caller's children and @slot with slot content
        let mut result_nodes =
            replace_children_and_slots(body_nodes, &caller_children, &slot_contents);

        // If this is a @component, wrap output in a scoped container
        let scope_key = format!("__component_scope_{}", name);
        if let Some(scope_class) = ctx.variables.get(&scope_key).cloned() {
            let wrapper = Element {
                kind: ElementKind::El,
                attrs: vec![Attribute {
                    key: "class".to_string(),
                    value: Some(scope_class),
                }],
                argument: None,
                children: result_nodes,
                line_num,
            };
            result_nodes = vec![Node::Element(wrapper)];
        }

        Ok(result_nodes)
    }

    fn parse_element_line(
        &mut self,
        content: &str,
        current_indent: usize,
        line_num: usize,
        ctx: &mut ParseContext,
    ) -> Result<Node, ParseError> {
        let segments = split_chain(content);

        // Parse each segment into an Element
        let mut elements: Vec<Element> = Vec::new();
        for seg in &segments {
            let elem = parse_single_element(seg.trim(), line_num, ctx)?;
            elements.push(elem);
        }

        // Parse indented children (belong to the innermost element)
        let children = self.parse_children(current_indent + 1, ctx);

        // Build chain right-to-left: rightmost gets children, each wraps the next
        let mut current_children = children;
        for mut elem in elements.into_iter().rev() {
            elem.children.extend(current_children);
            current_children = vec![Node::Element(elem)];
        }

        current_children.into_iter().next().ok_or(ParseError {
            line: line_num,
            message: "element chain produced no nodes (this is a parser bug)".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// @children replacement
// ---------------------------------------------------------------------------

fn replace_children_and_slots(
    nodes: Vec<Node>,
    caller_children: &[Node],
    slot_contents: &HashMap<String, Vec<Node>>,
) -> Vec<Node> {
    let mut result = Vec::new();
    for node in nodes {
        match node {
            Node::Element(elem) if elem.kind == ElementKind::Children => {
                if caller_children.is_empty() && !elem.children.is_empty() {
                    // Use @children's own children as default/fallback content
                    result.extend(elem.children);
                } else {
                    result.extend(caller_children.iter().cloned());
                }
            }
            Node::Element(elem) if matches!(&elem.kind, ElementKind::Slot(name) if !name.is_empty()) =>
            {
                if let ElementKind::Slot(ref name) = elem.kind {
                    if let Some(content) = slot_contents.get(name) {
                        result.extend(content.iter().cloned());
                    }
                    // If no content provided for this slot, use the slot's own children as default
                    else if !elem.children.is_empty() {
                        result.extend(elem.children);
                    }
                }
            }
            Node::Element(mut elem) => {
                elem.children =
                    replace_children_and_slots(elem.children, caller_children, slot_contents);
                result.push(Node::Element(elem));
            }
            other => result.push(other),
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Element parsing
// ---------------------------------------------------------------------------

fn extract_element_name(content: &str) -> &str {
    let without_at = &content[1..];
    match without_at.find([' ', '[']) {
        Some(i) => &without_at[..i],
        None => without_at,
    }
}

fn parse_single_element(
    content: &str,
    line_num: usize,
    ctx: &mut ParseContext,
) -> Result<Element, ParseError> {
    let (kind, rest) = if content.starts_with('[') {
        // Implicit @el
        (ElementKind::El, content.to_string())
    } else if let Some(without_at) = content.strip_prefix('@') {
        match without_at.find([' ', '[']) {
            Some(i) => {
                let kind_str = &without_at[..i];
                let rest = if without_at.as_bytes()[i] == b'[' {
                    without_at[i..].to_string()
                } else {
                    without_at[i + 1..].to_string()
                };
                match parse_element_kind(kind_str, line_num) {
                    Ok(kind) => (kind, rest),
                    Err(mut e) => {
                        // Also suggest user-defined functions
                        if let Some(fn_suggestion) = suggest_fn_name(kind_str, ctx) {
                            e.message =
                                format!("{}, or did you mean @{}?", e.message, fn_suggestion);
                        }
                        return Err(e);
                    }
                }
            }
            None => match parse_element_kind(without_at, line_num) {
                Ok(kind) => (kind, String::new()),
                Err(mut e) => {
                    if let Some(fn_suggestion) = suggest_fn_name(without_at, ctx) {
                        e.message = format!("{}, or did you mean @{}?", e.message, fn_suggestion);
                    }
                    return Err(e);
                }
            },
        }
    } else {
        return Err(ParseError {
            line: line_num,
            message: format!("expected @element or [attrs], got: {}", content),
        });
    };

    // Parse optional [attrs]
    let (attrs, rest) = if rest.starts_with('[') {
        parse_attr_brackets(&rest, line_num, ctx)?
    } else {
        (Vec::new(), rest)
    };

    let rest = rest.trim().to_string();
    track_var_refs(&rest, &mut ctx.used_variables);

    // For @link, first token of rest is URL, remainder is inline text
    let mut children = Vec::new();
    let argument = if rest.is_empty() {
        None
    } else if kind == ElementKind::Link {
        let rest_sub = substitute_vars(&rest, &ctx.variables);
        if let Some((url, text)) = rest_sub.split_once(' ') {
            let text = text.trim();
            if !text.is_empty() {
                children.push(Node::Text(parse_text_segments(text, ctx)));
            }
            Some(url.to_string())
        } else {
            Some(rest_sub)
        }
    } else {
        Some(substitute_vars(&rest, &ctx.variables))
    };

    // For @slot, the argument is the slot name
    let kind = if let ElementKind::Slot(_) = kind {
        ElementKind::Slot(argument.clone().unwrap_or_default())
    } else {
        kind
    };

    Ok(Element {
        kind,
        attrs,
        argument,
        children,
        line_num,
    })
}

const KNOWN_ELEMENTS: &[&str] = &[
    "row",
    "column",
    "col",
    "el",
    "text",
    "paragraph",
    "p",
    "image",
    "img",
    "link",
    "children",
    "input",
    "button",
    "select",
    "textarea",
    "option",
    "label",
    "slot",
    "nav",
    "header",
    "footer",
    "main",
    "section",
    "article",
    "aside",
    "list",
    "item",
    "li",
    "table",
    "thead",
    "tbody",
    "tr",
    "td",
    "th",
    "video",
    "audio",
    "form",
    "details",
    "summary",
    "blockquote",
    "cite",
    "code",
    "pre",
    "hr",
    "divider",
    "figure",
    "figcaption",
    "progress",
    "meter",
    "fragment",
    "btn",
    "ul",
    "dialog",
    "dl",
    "dt",
    "dd",
    "fieldset",
    "legend",
    "picture",
    "source",
    "time",
    "mark",
    "kbd",
    "abbr",
    "datalist",
    "iframe",
    "output",
    "canvas",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "grid",
    "stack",
    "spacer",
    "badge",
    "tooltip",
    "avatar",
    "carousel",
    "chip",
    "tag",
    "script",
    "noscript",
    "address",
    "search",
    "breadcrumb",
    "in-front",
    "behind",
];

const KNOWN_DIRECTIVES: &[&str] = &[
    "page",
    "let",
    "include",
    "import",
    "raw",
    "keyframes",
    "if",
    "else",
    "each",
    "meta",
    "head",
    "style",
    "match",
    "case",
    "default",
    "warn",
    "debug",
    "unless",
    "og",
    "breakpoint",
    "lang",
    "favicon",
    "use",
    "theme",
    "deprecated",
    "extends",
    "canonical",
    "base",
    "font-face",
    "json-ld",
    "assert",
    "for",
    "component",
    "switch",
    "log",
    "markdown",
    "repeat",
    "with",
    "layout",
    "collection",
    "manifest",
    "scope",
    "starting-style",
    "translations",
];

fn parse_element_kind(s: &str, line_num: usize) -> Result<ElementKind, ParseError> {
    match s {
        "row" => Ok(ElementKind::Row),
        "column" | "col" => Ok(ElementKind::Column),
        "el" => Ok(ElementKind::El),
        "text" => Ok(ElementKind::Text),
        "paragraph" | "p" => Ok(ElementKind::Paragraph),
        "image" | "img" => Ok(ElementKind::Image),
        "link" => Ok(ElementKind::Link),
        "children" => Ok(ElementKind::Children),
        "input" => Ok(ElementKind::Input),
        "button" | "btn" => Ok(ElementKind::Button),
        "select" => Ok(ElementKind::Select),
        "textarea" => Ok(ElementKind::Textarea),
        "option" | "opt" => Ok(ElementKind::Option),
        "label" => Ok(ElementKind::Label),
        "slot" => Ok(ElementKind::Slot(String::new())), // slot name filled in by parse_single_element
        "nav" => Ok(ElementKind::Nav),
        "header" => Ok(ElementKind::Header),
        "footer" => Ok(ElementKind::Footer),
        "main" => Ok(ElementKind::Main),
        "section" => Ok(ElementKind::Section),
        "article" => Ok(ElementKind::Article),
        "aside" => Ok(ElementKind::Aside),
        "list" => Ok(ElementKind::List),
        "item" | "li" => Ok(ElementKind::ListItem),
        "table" => Ok(ElementKind::Table),
        "thead" => Ok(ElementKind::TableHead),
        "tbody" => Ok(ElementKind::TableBody),
        "tr" => Ok(ElementKind::TableRow),
        "td" => Ok(ElementKind::TableCell),
        "th" => Ok(ElementKind::TableHeaderCell),
        "video" => Ok(ElementKind::Video),
        "audio" => Ok(ElementKind::Audio),
        "form" => Ok(ElementKind::Form),
        "details" => Ok(ElementKind::Details),
        "summary" => Ok(ElementKind::Summary),
        "blockquote" => Ok(ElementKind::Blockquote),
        "cite" => Ok(ElementKind::Cite),
        "code" => Ok(ElementKind::Code),
        "pre" => Ok(ElementKind::Pre),
        "hr" | "divider" => Ok(ElementKind::HorizontalRule),
        "figure" => Ok(ElementKind::Figure),
        "figcaption" => Ok(ElementKind::FigCaption),
        "progress" => Ok(ElementKind::Progress),
        "meter" => Ok(ElementKind::Meter),
        "fragment" => Ok(ElementKind::Fragment),
        "dialog" => Ok(ElementKind::Dialog),
        "dl" => Ok(ElementKind::DefinitionList),
        "dt" => Ok(ElementKind::DefinitionTerm),
        "dd" => Ok(ElementKind::DefinitionDescription),
        "fieldset" => Ok(ElementKind::Fieldset),
        "legend" => Ok(ElementKind::Legend),
        "picture" => Ok(ElementKind::Picture),
        "source" => Ok(ElementKind::Source),
        "time" => Ok(ElementKind::Time),
        "mark" => Ok(ElementKind::Mark),
        "kbd" => Ok(ElementKind::Kbd),
        "abbr" => Ok(ElementKind::Abbr),
        "h1" => Ok(ElementKind::H1),
        "h2" => Ok(ElementKind::H2),
        "h3" => Ok(ElementKind::H3),
        "h4" => Ok(ElementKind::H4),
        "h5" => Ok(ElementKind::H5),
        "h6" => Ok(ElementKind::H6),
        "datalist" => Ok(ElementKind::Datalist),
        "iframe" => Ok(ElementKind::Iframe),
        "output" => Ok(ElementKind::Output),
        "canvas" => Ok(ElementKind::Canvas),
        "ul" => Ok(ElementKind::List),
        "grid" => Ok(ElementKind::Grid),
        "stack" => Ok(ElementKind::Stack),
        "spacer" => Ok(ElementKind::Spacer),
        "badge" => Ok(ElementKind::Badge),
        "tooltip" => Ok(ElementKind::Tooltip),
        "avatar" => Ok(ElementKind::Avatar),
        "carousel" => Ok(ElementKind::Carousel),
        "chip" => Ok(ElementKind::Chip),
        "tag" => Ok(ElementKind::Tag),
        "script" => Ok(ElementKind::Script),
        "noscript" => Ok(ElementKind::Noscript),
        "address" => Ok(ElementKind::Address),
        "search" => Ok(ElementKind::Search),
        "breadcrumb" => Ok(ElementKind::Breadcrumb),
        "in-front" => Ok(ElementKind::InFront),
        "behind" => Ok(ElementKind::Behind),
        _ => {
            let all_known: Vec<&str> = KNOWN_ELEMENTS
                .iter()
                .chain(KNOWN_DIRECTIVES.iter())
                .copied()
                .collect();
            let suggestion = suggest_closest(s, &all_known);
            let msg = match suggestion {
                Some(closest) => format!("unknown element @{}, did you mean @{}?", s, closest),
                None => format!("unknown element @{}", s),
            };
            Err(ParseError {
                line: line_num,
                message: msg,
            })
        }
    }
}

/// Format include/import stack as a readable chain for error messages.
fn format_include_chain(stack: &[PathBuf]) -> String {
    stack
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

/// Levenshtein distance with a rolling two-row buffer (O(min(a,b)) memory) and an
/// early-exit cutoff: if every cell in a row exceeds `cutoff`, no further cell can
/// be <= `cutoff`, so we return `cutoff + 1` immediately. Used for typo-suggestion
/// hot paths where we only care about distances <= 2.
fn levenshtein_bounded(a: &[char], b: &[char], cutoff: usize) -> usize {
    let (a, b) = if a.len() > b.len() { (b, a) } else { (a, b) };
    if b.len() - a.len() > cutoff {
        return cutoff + 1;
    }
    if a.is_empty() {
        return b.len();
    }

    let mut prev: Vec<usize> = (0..=a.len()).collect();
    let mut curr: Vec<usize> = vec![0; a.len() + 1];

    for (j, &bc) in b.iter().enumerate() {
        curr[0] = j + 1;
        let mut row_min = curr[0];
        for (i, &ac) in a.iter().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            curr[i + 1] = (prev[i + 1] + 1).min(curr[i] + 1).min(prev[i] + cost);
            row_min = row_min.min(curr[i + 1]);
        }
        if row_min > cutoff {
            return cutoff + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[a.len()]
}

fn suggest_closest<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let input_chars: Vec<char> = input.chars().collect();
    let max_allowed = 2usize.min(input_chars.len().saturating_sub(1));
    let mut best = None;
    let mut best_dist = usize::MAX;
    for &candidate in candidates {
        // Cheap length-based prune before allocating chars.
        let clen = candidate.chars().count();
        let diff = clen.abs_diff(input_chars.len());
        if diff > max_allowed {
            continue;
        }
        let cand_chars: Vec<char> = candidate.chars().collect();
        let dist = levenshtein_bounded(&input_chars, &cand_chars, max_allowed);
        if dist < best_dist && dist <= max_allowed {
            best_dist = dist;
            best = Some(candidate);
        }
    }
    best
}

/// Suggest the closest user-defined function name for typos.
fn suggest_fn_name(input: &str, ctx: &ParseContext) -> Option<String> {
    let input_chars: Vec<char> = input.chars().collect();
    let max_allowed = 2usize.min(input_chars.len().saturating_sub(1));
    let mut best: Option<String> = None;
    let mut best_dist = usize::MAX;
    for name in ctx.functions.keys() {
        let nlen = name.chars().count();
        if nlen.abs_diff(input_chars.len()) > max_allowed {
            continue;
        }
        let name_chars: Vec<char> = name.chars().collect();
        let dist = levenshtein_bounded(&input_chars, &name_chars, max_allowed);
        if dist < best_dist && dist <= max_allowed {
            best_dist = dist;
            best = Some(name.clone());
        }
    }
    best
}

/// Suggest the closest variable name for undefined `$var` references.
fn suggest_var_name(input: &str, vars: &HashMap<String, String>) -> Option<String> {
    let input_chars: Vec<char> = input.chars().collect();
    let max_allowed = 2usize.min(input_chars.len().saturating_sub(1));
    let mut best: Option<String> = None;
    let mut best_dist = usize::MAX;
    for name in vars.keys() {
        let nlen = name.chars().count();
        if nlen.abs_diff(input_chars.len()) > max_allowed {
            continue;
        }
        let name_chars: Vec<char> = name.chars().collect();
        let dist = levenshtein_bounded(&input_chars, &name_chars, max_allowed);
        if dist < best_dist && dist <= max_allowed {
            best_dist = dist;
            best = Some(name.clone());
        }
    }
    best
}

/// Check for undefined `$var` references and return "did you mean?" diagnostics.
/// `indent` is the line's leading whitespace, which `input` has had trimmed;
/// reported columns and source lines include it.
fn check_undefined_vars(
    input: &str,
    vars: &HashMap<String, String>,
    line_num: usize,
    indent: usize,
) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    if !input.contains('$') {
        return warnings;
    }
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$'
            && i + 1 < chars.len()
            && (chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' || chars[i + 1] == '-')
        {
            let col = i;
            let start = i + 1;
            let mut end = start;
            while end < chars.len()
                && (chars[end].is_alphanumeric()
                    || chars[end] == '-'
                    || chars[end] == '_'
                    || chars[end] == '.')
            {
                end += 1;
            }
            while end > start && chars[end - 1] == '.' {
                end -= 1;
            }
            let name: String = chars[start..end].iter().collect();
            if !name.is_empty()
                && !vars.contains_key(&name)
                && let Some(closest) = suggest_var_name(&name, vars)
            {
                warnings.push(Diagnostic {
                    line: line_num,
                    column: Some(indent + col),
                    message: format!(
                        "undefined variable '${}', did you mean '${}'?",
                        name, closest
                    ),
                    severity: Severity::Warning,
                    source_line: Some(format!("{}{}", " ".repeat(indent), input)),
                });
            }
            i = end;
        } else {
            i += 1;
        }
    }
    warnings
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

const KNOWN_ATTRS: &[&str] = &[
    // Layout
    "spacing",
    "gap",
    "padding",
    "padding-x",
    "padding-y",
    "padding-top",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "width",
    "height",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "center-x",
    "center-y",
    "align-left",
    "align-right",
    "align-top",
    "align-bottom",
    // Style
    "background",
    "color",
    "border",
    "border-top",
    "border-bottom",
    "border-left",
    "border-right",
    "rounded",
    "bold",
    "italic",
    "underline",
    "size",
    "font",
    "transition",
    "cursor",
    "opacity",
    "text-align",
    "line-height",
    "letter-spacing",
    "text-transform",
    "white-space",
    "overflow",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "z-index",
    "shadow",
    "wrap",
    "gap-x",
    "gap-y",
    // Display & visibility
    "display",
    "visibility",
    // Transform & filters
    "transform",
    "backdrop-filter",
    // Grid
    "grid",
    "grid-cols",
    "grid-rows",
    "col-span",
    "row-span",
    // Container queries
    "container",
    "container-name",
    "container-type",
    // Identity
    "id",
    "class",
    // Animation
    "animation",
    // Form
    "type",
    "placeholder",
    "name",
    "value",
    "disabled",
    "required",
    "checked",
    "for",
    "action",
    "method",
    "autocomplete",
    "min",
    "max",
    "step",
    "pattern",
    "maxlength",
    "rows",
    "cols",
    "multiple",
    // Accessibility
    "alt",
    "role",
    "tabindex",
    "title",
    "autofocus",
    // CSS: aspect-ratio, outline, logical properties, scroll-snap
    "aspect-ratio",
    "outline",
    "padding-inline",
    "padding-block",
    "margin-inline",
    "margin-block",
    "padding-inline-start",
    "padding-inline-end",
    "padding-block-start",
    "padding-block-end",
    "margin-inline-start",
    "margin-inline-end",
    "margin-block-start",
    "margin-block-end",
    "inset-inline",
    "inset-block",
    "inset-inline-start",
    "inset-inline-end",
    "inset-block-start",
    "inset-block-end",
    "border-inline",
    "border-block",
    "border-inline-start",
    "border-inline-end",
    "border-block-start",
    "border-block-end",
    "border-start-start-radius",
    "border-start-end-radius",
    "border-end-start-radius",
    "border-end-end-radius",
    "scroll-margin-inline",
    "scroll-margin-block",
    "scroll-padding-inline",
    "scroll-padding-block",
    "inline-size",
    "block-size",
    "min-inline-size",
    "max-inline-size",
    "min-block-size",
    "max-block-size",
    "scroll-snap-type",
    "scroll-snap-align",
    // Media attributes
    "controls",
    "autoplay",
    "loop",
    "muted",
    "poster",
    "preload",
    "loading",
    "decoding",
    // List
    "ordered",
    // Media src (explicit attribute form)
    "src",
    // New elements
    "open",
    "novalidate",
    "low",
    "high",
    "optimum",
    "colspan",
    "rowspan",
    "scope",
    // Margin
    "margin",
    "margin-x",
    "margin-y",
    // Filter & object
    "filter",
    "object-fit",
    "object-position",
    // Text extras
    "text-shadow",
    "text-overflow",
    // Interaction
    "pointer-events",
    "user-select",
    // Flexbox/grid alignment
    "justify-content",
    "align-items",
    // Flex item
    "order",
    // Background extras
    "background-size",
    "background-position",
    "background-repeat",
    // Text wrapping
    "word-break",
    "overflow-wrap",
    // Asset inlining
    "inline",
    // Hidden
    "hidden",
    // Overflow directional
    "overflow-x",
    "overflow-y",
    // Inset shorthand
    "inset",
    // Modern form theming
    "accent-color",
    "caret-color",
    // Color scheme & appearance
    "color-scheme",
    "appearance",
    // List styling
    "list-style",
    // Table styling
    "border-collapse",
    "border-spacing",
    // Text decoration variants
    "text-decoration",
    "text-decoration-color",
    "text-decoration-thickness",
    "text-decoration-style",
    "text-underline-offset",
    "column-width",
    "column-rule",
    // Grid/flex placement
    "place-items",
    "place-self",
    // Scroll behavior
    "scroll-behavior",
    // Resize
    "resize",
    // New CSS properties
    "clip-path",
    "mix-blend-mode",
    "background-blend-mode",
    "writing-mode",
    "column-count",
    "column-gap",
    "text-indent",
    "hyphens",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "isolation",
    "place-content",
    "background-image",
    "datetime",
    "direction",
    // New CSS properties (batch 2)
    "font-weight",
    "font-style",
    "text-wrap",
    "will-change",
    "touch-action",
    "vertical-align",
    "contain",
    "content-visibility",
    "scroll-margin",
    "scroll-margin-top",
    "scroll-margin-bottom",
    "scroll-margin-left",
    "scroll-margin-right",
    "scroll-padding",
    "scroll-padding-top",
    "scroll-padding-bottom",
    "scroll-padding-left",
    "scroll-padding-right",
    // Iframe/output attrs
    "sandbox",
    "allow",
    "allowfullscreen",
    "referrerpolicy",
    "formaction",
    "formmethod",
    "formtarget",
    "target",
    // Popover API
    "popover",
    "popovertarget",
    "popovertargetaction",
    // Modern form/input hints
    "inputmode",
    "enterkeyhint",
    // Performance hints
    "fetchpriority",
    "blocking",
    // Global attrs
    "translate",
    "spellcheck",
    // Script attributes
    "defer",
    "async",
    "crossorigin",
    "integrity",
    "nomodule",
    // Pseudo-element content
    "content",
    // CSS shorthands
    "truncate",
    "line-clamp",
    "blur",
    "backdrop-blur",
    "no-scrollbar",
    "skeleton",
    "gradient",
    // Grid areas
    "grid-template-areas",
    "grid-area",
    // View transitions
    "view-transition-name",
    // Animate shorthand
    "animate",
    // Critical CSS hint
    "critical",
    // CSS subgrid
    "grid-template-columns",
    "grid-template-rows",
    // Scroll-driven animations
    "animation-timeline",
    "animation-range",
    "view-timeline-name",
    "view-timeline-axis",
    "scroll-timeline-name",
    "scroll-timeline-axis",
    // Anchor positioning
    "anchor-name",
    "position-anchor",
    "position-area",
    "inset-area",
    // Drop caps
    "initial-letter",
    // Responsive images
    "responsive",
];

/// Attributes that expect purely numeric values (px-based) or values with CSS units.
const NUMERIC_ATTRS: &[&str] = &[
    "spacing",
    "gap",
    "padding",
    "padding-x",
    "padding-y",
    "padding-top",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "rounded",
    "size",
    "gap-x",
    "gap-y",
    "top",
    "right",
    "bottom",
    "left",
    "letter-spacing",
];

const CSS_UNIT_SUFFIXES: &[&str] = &[
    "px", "%", "rem", "em", "vh", "vw", "vmin", "vmax", "dvh", "svh", "lvh", "ch", "ex", "cm",
    "mm", "in", "pt", "pc", "fr",
];

fn has_css_unit(value: &str) -> bool {
    CSS_UNIT_SUFFIXES.iter().any(|u| value.ends_with(u))
        || value.starts_with("var(")
        || value.starts_with("calc(")
        || value.starts_with("clamp(")
        || value.starts_with("min(")
        || value.starts_with("max(")
}

/// Attributes that accept numeric OR keyword values.
const NUMERIC_OR_KEYWORD_ATTRS: &[&str] = &["width", "height"];
const SIZE_KEYWORDS: &[&str] = &["fill", "shrink"];

fn validate_attr_value(attr: &Attribute, line_num: usize, ctx: &mut ParseContext) {
    let base_key = strip_all_prefixes(attr.key.as_str());

    if let Some(val) = &attr.value {
        if NUMERIC_ATTRS.contains(&base_key) {
            // All space-separated parts must be numeric or have a CSS unit
            for part in val.split_whitespace() {
                if part.parse::<f64>().is_err() && !has_css_unit(part) {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!(
                            "'{}' expects a numeric value (with optional unit), got '{}'",
                            attr.key, val
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                    return;
                }
            }
        } else if NUMERIC_OR_KEYWORD_ATTRS.contains(&base_key) {
            let is_keyword = SIZE_KEYWORDS.contains(&val.as_str());
            let is_numeric = val.parse::<f64>().is_ok();
            let has_unit = has_css_unit(val);
            if !is_keyword && !is_numeric && !has_unit {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "'{}' expects a number or one of [{}], got '{}'",
                        attr.key,
                        SIZE_KEYWORDS.join(", "),
                        val
                    ),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "opacity" {
            if let Ok(v) = val.parse::<f64>() {
                if !(0.0..=1.0).contains(&v) {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("'opacity' should be between 0 and 1, got '{}'", val),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            } else {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("'opacity' expects a numeric value, got '{}'", val),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "z-index" {
            if val.parse::<i32>().is_err() {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("'z-index' expects an integer, got '{}'", val),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "display" {
            const DISPLAY_VALUES: &[&str] = &[
                "none",
                "block",
                "inline",
                "inline-block",
                "flex",
                "inline-flex",
                "grid",
                "inline-grid",
                "table",
                "table-row",
                "table-cell",
                "contents",
                "flow-root",
                "list-item",
            ];
            if !DISPLAY_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                let suggestion = suggest_closest(val, DISPLAY_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown display value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown display value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: msg,
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "position" {
            const POSITION_VALUES: &[&str] = &["static", "relative", "absolute", "fixed", "sticky"];
            if !POSITION_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                let suggestion = suggest_closest(val, POSITION_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown position value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown position value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: msg,
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "overflow" || base_key == "overflow-x" || base_key == "overflow-y" {
            const OVERFLOW_VALUES: &[&str] = &["visible", "hidden", "scroll", "auto", "clip"];
            if !OVERFLOW_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                let suggestion = suggest_closest(val, OVERFLOW_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown overflow value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown overflow value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: msg,
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "text-align" {
            const TEXT_ALIGN_VALUES: &[&str] =
                &["left", "right", "center", "justify", "start", "end"];
            if !TEXT_ALIGN_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                let suggestion = suggest_closest(val, TEXT_ALIGN_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown text-align value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown text-align value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: msg,
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "cursor" {
            const CURSOR_VALUES: &[&str] = &[
                "auto",
                "default",
                "none",
                "pointer",
                "wait",
                "text",
                "move",
                "not-allowed",
                "crosshair",
                "grab",
                "grabbing",
                "help",
                "progress",
                "col-resize",
                "row-resize",
                "n-resize",
                "s-resize",
                "e-resize",
                "w-resize",
                "zoom-in",
                "zoom-out",
                "context-menu",
                "cell",
                "copy",
                "alias",
                "no-drop",
            ];
            if !CURSOR_VALUES.contains(&val.as_str())
                && !val.starts_with("url(")
                && !val.starts_with("var(")
            {
                let suggestion = suggest_closest(val, CURSOR_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown cursor value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown cursor value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: msg,
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "font-weight" {
            const WEIGHT_VALUES: &[&str] = &[
                "normal", "bold", "bolder", "lighter", "100", "200", "300", "400", "500", "600",
                "700", "800", "900",
            ];
            if !WEIGHT_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "'font-weight' expects a weight keyword or number 100-900, got '{}'",
                        val
                    ),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        } else if base_key == "color" || base_key == "background" {
            // Validate named CSS colors (only if not hex, rgb, hsl, var, etc.)
            if !val.starts_with('#')
                && !val.starts_with("rgb")
                && !val.starts_with("hsl")
                && !val.starts_with("var(")
                && !val.starts_with("linear-gradient")
                && !val.starts_with("radial-gradient")
                && !val.starts_with("conic-gradient")
                && !val.starts_with("oklch")
                && !val.starts_with("oklab")
                && !val.starts_with("color(")
                && !val.starts_with("light-dark(")
                && !val.contains("url(")
                && !val.contains(' ')
            // skip shorthand multi-value
            {
                const NAMED_COLORS: &[&str] = &[
                    "transparent",
                    "currentcolor",
                    "inherit",
                    "initial",
                    "unset",
                    "black",
                    "white",
                    "red",
                    "green",
                    "blue",
                    "yellow",
                    "orange",
                    "purple",
                    "pink",
                    "brown",
                    "gray",
                    "grey",
                    "cyan",
                    "magenta",
                    "lime",
                    "olive",
                    "navy",
                    "teal",
                    "aqua",
                    "fuchsia",
                    "maroon",
                    "silver",
                    "coral",
                    "salmon",
                    "tomato",
                    "crimson",
                    "firebrick",
                    "darkred",
                    "indigo",
                    "violet",
                    "plum",
                    "orchid",
                    "thistle",
                    "lavender",
                    "gold",
                    "khaki",
                    "wheat",
                    "tan",
                    "sienna",
                    "chocolate",
                    "peru",
                    "beige",
                    "ivory",
                    "linen",
                    "snow",
                    "seashell",
                    "mintcream",
                    "skyblue",
                    "steelblue",
                    "royalblue",
                    "dodgerblue",
                    "cornflowerblue",
                    "slategray",
                    "slategrey",
                    "dimgray",
                    "dimgrey",
                    "lightgray",
                    "lightgrey",
                    "darkgray",
                    "darkgrey",
                    "gainsboro",
                    "whitesmoke",
                ];
                if !NAMED_COLORS.contains(&val.to_lowercase().as_str()) {
                    let lower = val.to_lowercase();
                    let suggestion = suggest_closest(&lower, NAMED_COLORS);
                    let msg = match suggestion {
                        Some(s) => format!("unknown color '{}', did you mean '{}'?", val, s),
                        None => format!("unknown color '{}'", val),
                    };
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: msg,
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            }
        }
    }
}

fn parse_attr_brackets(
    input: &str,
    line_num: usize,
    ctx: &mut ParseContext,
) -> Result<(Vec<Attribute>, String), ParseError> {
    parse_attr_brackets_inner(input, line_num, ctx, true)
}

fn parse_attr_brackets_no_validate(
    input: &str,
    line_num: usize,
    ctx: &mut ParseContext,
) -> Result<(Vec<Attribute>, String), ParseError> {
    parse_attr_brackets_inner(input, line_num, ctx, false)
}

fn parse_attr_brackets_inner(
    input: &str,
    line_num: usize,
    ctx: &mut ParseContext,
    validate: bool,
) -> Result<(Vec<Attribute>, String), ParseError> {
    let mut depth = 0;
    let mut end_pos = 0;

    for (i, c) in input.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end_pos = i;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(ParseError {
            line: line_num,
            message: "unclosed '[' in attribute list".to_string(),
        });
    }

    let attrs_inner = &input[1..end_pos];
    let remaining = input[end_pos + 1..].trim().to_string();
    let attrs = parse_attr_list(attrs_inner, line_num, ctx, validate);

    Ok((attrs, remaining))
}

fn is_valid_hex_color(s: &str) -> bool {
    if !s.starts_with('#') {
        return true; // Not a hex color, skip
    }
    let hex = &s[1..];
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_attr_list(
    input: &str,
    line_num: usize,
    ctx: &mut ParseContext,
    validate: bool,
) -> Vec<Attribute> {
    let mut attrs = Vec::new();
    let mut seen_keys: Vec<String> = Vec::new();

    for part in split_commas(input) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // ...$name spread — expand define/attribute bundle
        if let Some(name) = part.strip_prefix("...$")
            && let Some(define_attrs) = ctx.defines.get(name)
        {
            ctx.used_defines.insert(name.to_string());
            attrs.extend(define_attrs.clone());
            continue;
        }

        // $define reference — expand attribute bundle
        if let Some(name) = part.strip_prefix('$')
            && let Some(define_attrs) = ctx.defines.get(name)
        {
            ctx.used_defines.insert(name.to_string());
            attrs.extend(define_attrs.clone());
            continue;
        }

        // Substitute variables in value
        track_var_refs(part, &mut ctx.used_variables);
        let part = substitute_vars(part, &ctx.variables);

        // Conditional attribute: `key if condition` or `key value if condition`
        let (part, is_conditional) = {
            // Check for " if " not inside parentheses
            let check = part.as_str();
            let mut found_if = None;
            let mut depth = 0;
            for (i, c) in check.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                if depth == 0 && check[i..].starts_with(" if ") {
                    found_if = Some(i);
                    break;
                }
            }
            if let Some(if_pos) = found_if {
                let attr_part = &check[..if_pos];
                let condition = &check[if_pos + 4..];
                let cond_result = evaluate_condition(condition.trim());
                if cond_result {
                    (attr_part.to_string(), false)
                } else {
                    (String::new(), true)
                }
            } else {
                (part.to_string(), false)
            }
        };

        // Skip this attribute if the condition was false
        if is_conditional {
            continue;
        }

        let attr = if let Some((key, value)) = part.split_once(' ') {
            let value = evaluate_if_expr(value.trim());
            let value = substitute_ternary(&value);
            Attribute {
                key: key.trim().to_string(),
                value: Some(value),
            }
        } else {
            Attribute {
                key: part.to_string(),
                value: None,
            }
        };

        // Warn on duplicate attributes (compare full key so pseudo-class
        // variants like `border` and `hover:border` are not conflated)
        if validate {
            if seen_keys.contains(&attr.key) {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("duplicate attribute '{}'", attr.key),
                    severity: Severity::Warning,
                    source_line: None,
                });
            } else {
                seen_keys.push(attr.key.clone());
            }

            // Color validation for hex colors
            if matches!(strip_all_prefixes(&attr.key), "background" | "color")
                && let Some(ref val) = attr.value
                && val.starts_with('#')
                && !is_valid_hex_color(val)
            {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("invalid hex color '{}'", val),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
        }

        // Warn on unknown attributes
        if validate {
            let base_key = strip_all_prefixes(attr.key.as_str());
            if !KNOWN_ATTRS.contains(&base_key)
                && !base_key.starts_with("aria-")
                && !base_key.starts_with("data-")
            {
                let suggestion = suggest_closest(base_key, KNOWN_ATTRS);
                let msg = match suggestion {
                    Some(closest) => {
                        format!(
                            "unknown attribute '{}', did you mean '{}'?",
                            attr.key, closest
                        )
                    }
                    None => format!("unknown attribute '{}'", attr.key),
                };
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: msg,
                    severity: Severity::Warning,
                    source_line: None,
                });
            } else {
                validate_attr_value(&attr, line_num, ctx);
            }
        }

        attrs.push(attr);
    }

    attrs
}

fn split_commas(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut in_quotes = false;

    for (i, c) in input.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            _ if in_quotes => {}
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

// ---------------------------------------------------------------------------
// Chain splitting (the > operator)
// ---------------------------------------------------------------------------

fn split_chain(content: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let mut bracket_depth = 0;

    while i < chars.len() {
        if chars[i] == '[' {
            bracket_depth += 1;
        }
        if chars[i] == ']' {
            bracket_depth -= 1;
        }

        // Match " > @" outside brackets
        if bracket_depth == 0
            && i + 3 < chars.len()
            && chars[i] == ' '
            && chars[i + 1] == '>'
            && chars[i + 2] == ' '
            && chars[i + 3] == '@'
        {
            segments.push(current.trim().to_string());
            current = String::new();
            i += 3; // skip " > ", keep the "@"
            continue;
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.trim().is_empty() {
        segments.push(current.trim().to_string());
    }

    segments
}

// ---------------------------------------------------------------------------
// Text segment parsing (inline {...} elements)
// ---------------------------------------------------------------------------

fn parse_text_segments(input: &str, ctx: &mut ParseContext) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut current_text = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '{' && i + 1 < chars.len() && chars[i + 1] == '@' {
            // Flush accumulated plain text
            if !current_text.is_empty() {
                segments.push(TextSegment::Plain(substitute_vars(
                    &current_text,
                    &ctx.variables,
                )));
                current_text.clear();
            }

            // Find matching }
            let mut depth = 0;
            let start = i + 1; // after {
            i += 1;
            loop {
                if i >= chars.len() {
                    break;
                }
                if chars[i] == '{' {
                    depth += 1;
                } else if chars[i] == '}' {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                i += 1;
            }

            let inner: String = chars[start..i].iter().collect();
            i += 1; // skip }

            if let Ok(elem) = parse_single_element(&inner, 0, ctx) {
                segments.push(TextSegment::Inline(elem));
            } else {
                current_text.push('{');
                current_text.push_str(&inner);
                current_text.push('}');
            }
        } else {
            current_text.push(chars[i]);
            i += 1;
        }
    }

    if !current_text.is_empty() {
        segments.push(TextSegment::Plain(substitute_vars(
            &current_text,
            &ctx.variables,
        )));
    }

    segments
}

// ---------------------------------------------------------------------------
// Variable substitution
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Condition evaluation for @if
// ---------------------------------------------------------------------------

/// Inclusive integer range from `start` to `end` (counting down when
/// `start > end`), stepping by `step`. Stops instead of overflowing.
fn numeric_range(start: i64, end: i64, step: i64) -> Vec<String> {
    let mut items = Vec::new();
    let mut n = start;
    if start <= end {
        while n <= end {
            items.push(n.to_string());
            match n.checked_add(step) {
                Some(next) => n = next,
                None => break,
            }
        }
    } else {
        while n >= end {
            items.push(n.to_string());
            match n.checked_sub(step) {
                Some(next) => n = next,
                None => break,
            }
        }
    }
    items
}

fn evaluate_condition(condition: &str) -> bool {
    // Ternary expression support: condition ? true_val : false_val
    // (just check the condition part, the ternary is handled in substitute_ternary)

    if let Some((left, right)) = condition.split_once("!=") {
        left.trim() != right.trim()
    } else if let Some((left, right)) = condition.split_once("==") {
        left.trim() == right.trim()
    } else if let Some((left, right)) = condition.split_once(">=") {
        // Numeric comparison with fallback to string comparison
        let l = left.trim();
        let r = right.trim();
        match (l.parse::<f64>(), r.parse::<f64>()) {
            (Ok(ln), Ok(rn)) => ln >= rn,
            _ => l >= r,
        }
    } else if let Some((left, right)) = condition.split_once("<=") {
        let l = left.trim();
        let r = right.trim();
        match (l.parse::<f64>(), r.parse::<f64>()) {
            (Ok(ln), Ok(rn)) => ln <= rn,
            _ => l <= r,
        }
    } else if let Some((left, right)) = condition.split_once('>') {
        let l = left.trim();
        let r = right.trim();
        match (l.parse::<f64>(), r.parse::<f64>()) {
            (Ok(ln), Ok(rn)) => ln > rn,
            _ => l > r,
        }
    } else if let Some((left, right)) = condition.split_once('<') {
        let l = left.trim();
        let r = right.trim();
        match (l.parse::<f64>(), r.parse::<f64>()) {
            (Ok(ln), Ok(rn)) => ln < rn,
            _ => l < r,
        }
    } else if let Some((left, right)) = condition.split_once(" contains ") {
        left.trim().contains(right.trim())
    } else if let Some((left, right)) = condition.split_once(" starts-with ") {
        left.trim().starts_with(right.trim())
    } else if let Some((left, right)) = condition.split_once(" ends-with ") {
        left.trim().ends_with(right.trim())
    } else {
        // Truthy check: non-empty, not "false", not "0"
        let trimmed = condition.trim();
        !trimmed.is_empty() && trimmed != "false" && trimmed != "0"
    }
}

/// Evaluate ternary expressions: `condition ? true_val : false_val` in attribute values.
fn substitute_ternary(input: &str) -> String {
    if !input.contains(" ? ") {
        return input.to_string();
    }
    // Find the ternary operator pattern
    if let Some(q_pos) = input.find(" ? ") {
        let condition = input[..q_pos].trim();
        let rest = &input[q_pos + 3..];
        if let Some(c_pos) = rest.find(" : ") {
            let true_val = rest[..c_pos].trim();
            let false_val = rest[c_pos + 3..].trim();
            if evaluate_condition(condition) {
                return true_val.to_string();
            } else {
                return false_val.to_string();
            }
        }
    }
    input.to_string()
}

/// Evaluate `if(condition, true_val, false_val)` expressions in attribute values.
fn evaluate_if_expr(input: &str) -> String {
    let trimmed = input.trim();
    if !trimmed.starts_with("if(") || !trimmed.ends_with(')') {
        return input.to_string();
    }
    let inner = &trimmed[3..trimmed.len() - 1];
    let parts = split_if_args(inner);
    if parts.len() != 3 {
        return input.to_string();
    }
    let condition = parts[0].trim();
    let true_val = parts[1].trim();
    let false_val = parts[2].trim();

    if evaluate_condition(condition) {
        true_val.to_string()
    } else {
        false_val.to_string()
    }
}

fn split_if_args(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (i, c) in input.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

fn evaluate_arithmetic(input: &str) -> String {
    let input = input.trim();

    // String concatenation with ~ operator: @let full $first ~ " " ~ $last
    if input.contains(" ~ ") {
        let parts: Vec<&str> = input.split(" ~ ").collect();
        if parts.len() >= 2 {
            return parts
                .iter()
                .map(|p| {
                    let t = p.trim();
                    // Strip quotes from string literals
                    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                        &t[1..t.len() - 1]
                    } else {
                        t
                    }
                })
                .collect::<Vec<_>>()
                .join("");
        }
    }

    for op in &[" * ", " / ", " + ", " - "] {
        if let Some((left, right)) = input.split_once(op) {
            let left = left.trim();
            let right = right.trim();
            let Some(op_char) = op.trim().chars().next() else {
                continue;
            };
            if let (Ok(l), Ok(r)) = (left.parse::<f64>(), right.parse::<f64>()) {
                let result = match op_char {
                    '+' => l + r,
                    '-' => l - r,
                    '*' => l * r,
                    '/' if r != 0.0 => l / r,
                    _ => return input.to_string(),
                };
                if result == result.floor() && result.abs() < i64::MAX as f64 {
                    return format!("{}", result as i64);
                }
                return format!("{}", result);
            }
        }
    }
    input.to_string()
}

// ---------------------------------------------------------------------------
// Post-parse validation (context-dependent warnings)
// ---------------------------------------------------------------------------

fn strip_all_prefixes(key: &str) -> &str {
    // Strip pseudo-state prefixes
    let key = key
        .strip_prefix("hover:")
        .or_else(|| key.strip_prefix("active:"))
        .or_else(|| key.strip_prefix("focus:"))
        .or_else(|| key.strip_prefix("focus-visible:"))
        .or_else(|| key.strip_prefix("focus-within:"))
        .or_else(|| key.strip_prefix("disabled:"))
        .or_else(|| key.strip_prefix("checked:"))
        .or_else(|| key.strip_prefix("placeholder:"))
        .or_else(|| key.strip_prefix("first:"))
        .or_else(|| key.strip_prefix("last:"))
        .or_else(|| key.strip_prefix("odd:"))
        .or_else(|| key.strip_prefix("even:"))
        .or_else(|| key.strip_prefix("selection:"))
        .or_else(|| key.strip_prefix("before:"))
        .or_else(|| key.strip_prefix("after:"))
        .or_else(|| key.strip_prefix("visited:"))
        .or_else(|| key.strip_prefix("empty:"))
        .or_else(|| key.strip_prefix("target:"))
        .or_else(|| key.strip_prefix("valid:"))
        .or_else(|| key.strip_prefix("invalid:"))
        .or_else(|| {
            // Handle nth:EXPR: prefix (e.g., nth:3:background -> background)
            if let Some(rest) = key.strip_prefix("nth:") {
                rest.find(':').map(|pos| &rest[pos + 1..])
            } else {
                None
            }
        })
        .or_else(|| {
            // Handle has(...): prefix (e.g., has(.active):background -> background)
            if key.starts_with("has(") {
                if let Some(close) = key.find("):") {
                    Some(&key[close + 2..])
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or(key);
    // Strip responsive prefixes
    let key = key
        .strip_prefix("sm:")
        .or_else(|| key.strip_prefix("md:"))
        .or_else(|| key.strip_prefix("lg:"))
        .or_else(|| key.strip_prefix("xl:"))
        .or_else(|| key.strip_prefix("2xl:"))
        .unwrap_or(key);
    // Strip media prefixes
    let key = key
        .strip_prefix("dark:")
        .or_else(|| key.strip_prefix("print:"))
        .unwrap_or(key);
    let key = key
        .strip_prefix("motion-safe:")
        .or_else(|| key.strip_prefix("motion-reduce:"))
        .or_else(|| key.strip_prefix("landscape:"))
        .or_else(|| key.strip_prefix("portrait:"))
        .unwrap_or(key);
    // Strip container query prefixes (cq-sm:, cq-md:, etc.)

    (key.strip_prefix("cq-sm:")
        .or_else(|| key.strip_prefix("cq-md:"))
        .or_else(|| key.strip_prefix("cq-lg:"))
        .or_else(|| key.strip_prefix("cq-xl:"))
        .or_else(|| key.strip_prefix("cq-2xl:"))
        .unwrap_or(key)) as _
}

/// Attributes that only make sense on container elements (@row, @column, @el).
const CONTAINER_ONLY_ATTRS: &[&str] = &[
    "spacing",
    "gap",
    "gap-x",
    "gap-y",
    "wrap",
    "grid",
    "grid-cols",
    "grid-rows",
    "container",
    "container-name",
    "container-type",
];

fn element_kind_name(kind: &ElementKind) -> &'static str {
    match kind {
        ElementKind::Row => "@row",
        ElementKind::Column => "@column",
        ElementKind::El => "@el",
        ElementKind::Text => "@text",
        ElementKind::Paragraph => "@paragraph",
        ElementKind::Image => "@image",
        ElementKind::Link => "@link",
        ElementKind::Children => "@children",
        ElementKind::Input => "@input",
        ElementKind::Button => "@button",
        ElementKind::Select => "@select",
        ElementKind::Textarea => "@textarea",
        ElementKind::Option => "@option",
        ElementKind::Label => "@label",
        ElementKind::Slot(_) => "@slot",
        ElementKind::Nav => "@nav",
        ElementKind::Header => "@header",
        ElementKind::Footer => "@footer",
        ElementKind::Main => "@main",
        ElementKind::Section => "@section",
        ElementKind::Article => "@article",
        ElementKind::Aside => "@aside",
        ElementKind::List => "@list",
        ElementKind::ListItem => "@item",
        ElementKind::Table => "@table",
        ElementKind::TableHead => "@thead",
        ElementKind::TableBody => "@tbody",
        ElementKind::TableRow => "@tr",
        ElementKind::TableCell => "@td",
        ElementKind::TableHeaderCell => "@th",
        ElementKind::Video => "@video",
        ElementKind::Audio => "@audio",
        ElementKind::Form => "@form",
        ElementKind::Details => "@details",
        ElementKind::Summary => "@summary",
        ElementKind::Blockquote => "@blockquote",
        ElementKind::Cite => "@cite",
        ElementKind::Code => "@code",
        ElementKind::Pre => "@pre",
        ElementKind::HorizontalRule => "@hr",
        ElementKind::Figure => "@figure",
        ElementKind::FigCaption => "@figcaption",
        ElementKind::Progress => "@progress",
        ElementKind::Meter => "@meter",
        ElementKind::Fragment => "@fragment",
        ElementKind::Dialog => "@dialog",
        ElementKind::DefinitionList => "@dl",
        ElementKind::DefinitionTerm => "@dt",
        ElementKind::DefinitionDescription => "@dd",
        ElementKind::Fieldset => "@fieldset",
        ElementKind::Legend => "@legend",
        ElementKind::Picture => "@picture",
        ElementKind::Source => "@source",
        ElementKind::Time => "@time",
        ElementKind::Mark => "@mark",
        ElementKind::Kbd => "@kbd",
        ElementKind::Abbr => "@abbr",
        ElementKind::H1 => "@h1",
        ElementKind::H2 => "@h2",
        ElementKind::H3 => "@h3",
        ElementKind::H4 => "@h4",
        ElementKind::H5 => "@h5",
        ElementKind::H6 => "@h6",
        ElementKind::Datalist => "@datalist",
        ElementKind::Iframe => "@iframe",
        ElementKind::Output => "@output",
        ElementKind::Canvas => "@canvas",
        ElementKind::Grid => "@grid",
        ElementKind::Stack => "@stack",
        ElementKind::Spacer => "@spacer",
        ElementKind::Badge => "@badge",
        ElementKind::Tooltip => "@tooltip",
        ElementKind::Avatar => "@avatar",
        ElementKind::Carousel => "@carousel",
        ElementKind::Chip => "@chip",
        ElementKind::Tag => "@tag",
        ElementKind::Script => "@script",
        ElementKind::Noscript => "@noscript",
        ElementKind::Address => "@address",
        ElementKind::Search => "@search",
        ElementKind::Breadcrumb => "@breadcrumb",
        ElementKind::InFront => "@in-front",
        ElementKind::Behind => "@behind",
    }
}

fn is_container(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Row
            | ElementKind::Column
            | ElementKind::El
            | ElementKind::Nav
            | ElementKind::Header
            | ElementKind::Footer
            | ElementKind::Main
            | ElementKind::Section
            | ElementKind::Article
            | ElementKind::Aside
            | ElementKind::List
            | ElementKind::ListItem
            | ElementKind::Form
            | ElementKind::Details
            | ElementKind::Figure
            | ElementKind::Blockquote
            | ElementKind::Dialog
            | ElementKind::DefinitionList
            | ElementKind::DefinitionTerm
            | ElementKind::DefinitionDescription
            | ElementKind::Fieldset
            | ElementKind::Datalist
            | ElementKind::Iframe
            | ElementKind::Canvas
            | ElementKind::Grid
            | ElementKind::Stack
            | ElementKind::Carousel
            | ElementKind::Noscript
            | ElementKind::Address
            | ElementKind::Search
            | ElementKind::Breadcrumb
            | ElementKind::InFront
            | ElementKind::Behind
    )
}

fn validate_tree(
    nodes: &[Node],
    parent_kind: Option<&ElementKind>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for node in nodes {
        if let Node::Element(elem) = node {
            for attr in &elem.attrs {
                let base = strip_all_prefixes(&attr.key);
                if base == "width"
                    && attr.value.as_deref() == Some("fill")
                    && !matches!(parent_kind, Some(ElementKind::Row))
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: "'width fill' works best inside @row; using 100% as fallback"
                            .to_string(),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
                if base == "height"
                    && attr.value.as_deref() == Some("fill")
                    && !matches!(parent_kind, Some(ElementKind::Column))
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: "'height fill' works best inside @column; using 100% as fallback"
                            .to_string(),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }

                // Container-only attributes on non-container elements
                if CONTAINER_ONLY_ATTRS.contains(&base) && !is_container(&elem.kind) {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "'{}' has no effect on {} (only works on @row, @column, @el)",
                            base,
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }

                // Form-specific: placeholder only on @input/@textarea
                if base == "placeholder"
                    && !matches!(elem.kind, ElementKind::Input | ElementKind::Textarea)
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "'placeholder' has no effect on {} (only works on @input, @textarea)",
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }

                // 'for' only on @label
                if base == "for" && !matches!(elem.kind, ElementKind::Label) {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "'for' has no effect on {} (only works on @label)",
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }

                // 'rows'/'cols' only on @textarea
                if (base == "rows" || base == "cols") && !matches!(elem.kind, ElementKind::Textarea)
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "'{}' has no effect on {} (only works on @textarea)",
                            base,
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }

                // 'ordered' only on @list
                if base == "ordered" && !matches!(elem.kind, ElementKind::List) {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "'ordered' has no effect on {} (only works on @list)",
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }

                // Media-specific attributes only on @video/@audio
                if matches!(
                    base,
                    "controls"
                        | "autoplay"
                        | "loop"
                        | "muted"
                        | "playsinline"
                        | "poster"
                        | "preload"
                ) && !matches!(elem.kind, ElementKind::Video | ElementKind::Audio)
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "'{}' has no effect on {} (only works on @video, @audio)",
                            base,
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            }
            // Missing alt text on @image
            if matches!(elem.kind, ElementKind::Image) && !elem.attrs.iter().any(|a| a.key == "alt")
            {
                diagnostics.push(Diagnostic {
                    line: elem.line_num,
                    column: None,
                    message: "@image missing 'alt' attribute (accessibility)".to_string(),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
            if matches!(elem.kind, ElementKind::Input)
                && !elem.attrs.iter().any(|a| a.key == "type")
            {
                diagnostics.push(Diagnostic {
                    line: elem.line_num,
                    column: None,
                    message: "@input missing 'type' attribute (defaults to 'text')".to_string(),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }
            if matches!(elem.kind, ElementKind::Link) {
                // For @link, argument is the URL, not text content
                let has_text = !elem.children.is_empty();
                if !has_text
                    && !elem
                        .attrs
                        .iter()
                        .any(|a| a.key == "aria-label" || a.key == "title")
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: "@link has no visible text or aria-label (accessibility)"
                            .to_string(),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            }

            // Contrast ratio check for hex color pairs
            {
                let bg_color = elem
                    .attrs
                    .iter()
                    .find(|a| strip_all_prefixes(&a.key) == "background")
                    .and_then(|a| a.value.as_deref());
                let fg_color = elem
                    .attrs
                    .iter()
                    .find(|a| strip_all_prefixes(&a.key) == "color")
                    .and_then(|a| a.value.as_deref());
                if let (Some(bg), Some(fg)) = (bg_color, fg_color)
                    && let (Some(bg_rgb), Some(fg_rgb)) = (parse_hex_rgb(bg), parse_hex_rgb(fg))
                {
                    let ratio = contrast_ratio(bg_rgb, fg_rgb);
                    if ratio < 4.5 {
                        diagnostics.push(Diagnostic {
                                line: elem.line_num,
                                column: None,
                                message: format!(
                                    "low contrast ratio {:.1}:1 between '{}' and '{}' (WCAG AA requires 4.5:1)",
                                    ratio, fg, bg
                                ),
                                severity: Severity::Warning,
                                source_line: None,
                            });
                    }
                }
            }

            // @form inputs should have associated @label
            if matches!(
                elem.kind,
                ElementKind::Input | ElementKind::Select | ElementKind::Textarea
            ) {
                let has_id = elem.attrs.iter().any(|a| a.key == "id");
                let has_aria_label = elem
                    .attrs
                    .iter()
                    .any(|a| a.key == "aria-label" || a.key == "aria-labelledby");
                let has_title = elem.attrs.iter().any(|a| a.key == "title");
                let in_label = matches!(parent_kind, Some(ElementKind::Label));
                if !has_id && !has_aria_label && !has_title && !in_label {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: format!(
                            "{} should have an 'id' (with matching @label[for]), 'aria-label', or be wrapped in @label (accessibility)",
                            element_kind_name(&elem.kind)
                        ),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            }

            // @iframe should have title attribute
            if matches!(elem.kind, ElementKind::Iframe)
                && !elem.attrs.iter().any(|a| a.key == "title")
            {
                diagnostics.push(Diagnostic {
                    line: elem.line_num,
                    column: None,
                    message: "@iframe missing 'title' attribute (accessibility)".to_string(),
                    severity: Severity::Warning,
                    source_line: None,
                });
            }

            // @button should have accessible text
            if matches!(elem.kind, ElementKind::Button) {
                let has_text = elem.argument.is_some() || !elem.children.is_empty();
                let has_aria = elem.attrs.iter().any(|a| a.key == "aria-label");
                if !has_text && !has_aria {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: "@button has no text content or aria-label (accessibility)"
                            .to_string(),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            }

            // @video should have captions or aria-label
            if matches!(elem.kind, ElementKind::Video) {
                let has_aria = elem
                    .attrs
                    .iter()
                    .any(|a| a.key == "aria-label" || a.key == "aria-describedby");
                let has_track = elem
                    .children
                    .iter()
                    .any(|c| matches!(c, Node::Element(e) if e.kind == ElementKind::Source));
                if !has_aria && !has_track {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: "@video should have aria-label or captions for accessibility"
                            .to_string(),
                        severity: Severity::Warning,
                        source_line: None,
                    });
                }
            }

            // Tabindex > 0 is an anti-pattern
            if let Some(tabindex_attr) = elem.attrs.iter().find(|a| a.key == "tabindex")
                && let Some(ref val) = tabindex_attr.value
                && let Ok(n) = val.parse::<i32>()
                && n > 0
            {
                diagnostics.push(Diagnostic {
                                line: elem.line_num,
                                column: None,
                                message: format!("tabindex {} is positive — avoid positive tabindex values as they disrupt natural tab order", n),
                                severity: Severity::Warning,
                                source_line: None,
                            });
            }

            validate_tree(&elem.children, Some(&elem.kind), diagnostics);
        }
    }
}

fn parse_hex_rgb(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match s.len() {
        3 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()?;
            let g = u8::from_str_radix(&s[1..2], 16).ok()?;
            let b = u8::from_str_radix(&s[2..3], 16).ok()?;
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn relative_luminance(r: u8, g: u8, b: u8) -> f64 {
    fn linearize(c: u8) -> f64 {
        let s = c as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(r) + 0.7152 * linearize(g) + 0.0722 * linearize(b)
}

fn contrast_ratio(c1: (u8, u8, u8), c2: (u8, u8, u8)) -> f64 {
    let l1 = relative_luminance(c1.0, c1.1, c1.2);
    let l2 = relative_luminance(c2.0, c2.1, c2.2);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

fn lighten_color(rgb: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let r = rgb.0 as f64 + (255.0 - rgb.0 as f64) * amount.clamp(0.0, 1.0);
    let g = rgb.1 as f64 + (255.0 - rgb.1 as f64) * amount.clamp(0.0, 1.0);
    let b = rgb.2 as f64 + (255.0 - rgb.2 as f64) * amount.clamp(0.0, 1.0);
    (r.round() as u8, g.round() as u8, b.round() as u8)
}

fn darken_color(rgb: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let factor = 1.0 - amount.clamp(0.0, 1.0);
    let r = (rgb.0 as f64 * factor).round() as u8;
    let g = (rgb.1 as f64 * factor).round() as u8;
    let b = (rgb.2 as f64 * factor).round() as u8;
    (r, g, b)
}

fn mix_colors(c1: (u8, u8, u8), c2: (u8, u8, u8), weight: f64) -> (u8, u8, u8) {
    let w = weight.clamp(0.0, 1.0);
    let r = (c1.0 as f64 * (1.0 - w) + c2.0 as f64 * w).round() as u8;
    let g = (c1.1 as f64 * (1.0 - w) + c2.1 as f64 * w).round() as u8;
    let b = (c1.2 as f64 * (1.0 - w) + c2.2 as f64 * w).round() as u8;
    (r, g, b)
}

// ---------------------------------------------------------------------------
// Variable substitution
// ---------------------------------------------------------------------------

fn substitute_vars(input: &str, vars: &HashMap<String, String>) -> String {
    if !input.contains('$') {
        return input.to_string();
    }

    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$'
            && i + 1 < chars.len()
            && (chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' || chars[i + 1] == '-')
        {
            let start = i + 1;
            let mut end = start;
            while end < chars.len()
                && (chars[end].is_alphanumeric()
                    || chars[end] == '-'
                    || chars[end] == '_'
                    || chars[end] == '.')
            {
                end += 1;
            }
            // Strip trailing dot (not part of name if at end)
            while end > start && chars[end - 1] == '.' {
                end -= 1;
            }
            let name: String = chars[start..end].iter().collect();

            // Collect pipe filters: $name|filter1|filter2:arg
            let mut filters: Vec<String> = Vec::new();
            while end < chars.len() && chars[end] == '|' {
                end += 1; // skip '|'
                let filter_start = end;
                while end < chars.len()
                    && chars[end] != '|'
                    && chars[end] != ' '
                    && chars[end] != ','
                    && chars[end] != ']'
                    && chars[end] != '}'
                {
                    end += 1;
                }
                let filter: String = chars[filter_start..end].iter().collect();
                if !filter.is_empty() {
                    filters.push(filter);
                }
            }

            if let Some(value) = vars.get(&name) {
                let mut val = value.clone();
                for filter in &filters {
                    val = apply_filter(&val, filter);
                }
                result.push_str(&val);
            } else {
                result.push('$');
                result.push_str(&name);
            }
            i = end;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Parse a keyframe line in htmlang syntax: `from [opacity 0]` / `50% [transform scale(1.5)]`
fn parse_keyframe_line(line: &str) -> Option<String> {
    let (selector, rest) = if let Some(rest) = line.strip_prefix("from") {
        ("from", rest.trim())
    } else if let Some(rest) = line.strip_prefix("to") {
        ("to", rest.trim())
    } else if let Some(pct_end) = line.find('%') {
        let rest = line[pct_end + 1..].trim();
        let selector = &line[..pct_end + 1];
        (selector, rest)
    } else {
        return None;
    };

    if !rest.starts_with('[') || !rest.ends_with(']') {
        return None;
    }

    let inner = &rest[1..rest.len() - 1];
    // Parse comma-separated key-value pairs into CSS
    let mut css = String::new();
    for part in split_commas(inner) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((key, value)) = part.split_once(' ') {
            css.push_str(key.trim());
            css.push(':');
            css.push_str(value.trim());
            css.push(';');
        }
    }

    if css.is_empty() {
        return None;
    }

    Some(format!("{}{{{}}}", selector, css))
}

/// Replace @slot placeholders in the base layout with content from @extends
fn replace_extends_slots(nodes: Vec<Node>, slot_nodes: &HashMap<String, Vec<Node>>) -> Vec<Node> {
    let mut result = Vec::new();
    for node in nodes {
        match node {
            Node::Element(elem) if matches!(&elem.kind, ElementKind::Slot(name) if !name.is_empty()) =>
            {
                if let ElementKind::Slot(ref name) = elem.kind {
                    if let Some(content) = slot_nodes.get(name) {
                        result.extend(content.iter().cloned());
                    } else if !elem.children.is_empty() {
                        // Use slot's own children as default
                        result.extend(elem.children);
                    }
                }
            }
            Node::Element(mut elem) => {
                elem.children = replace_extends_slots(elem.children, slot_nodes);
                result.push(Node::Element(elem));
            }
            other => result.push(other),
        }
    }
    result
}

fn apply_filter(value: &str, filter: &str) -> String {
    if let Some(arg) = filter.strip_prefix("truncate:") {
        if let Ok(n) = arg.parse::<usize>()
            && value.chars().count() > n
        {
            return format!("{}...", value.chars().take(n).collect::<String>());
        }
        return value.to_string();
    }
    if let Some(rest) = filter.strip_prefix("replace:") {
        if let Some((old, new)) = rest.split_once(':') {
            return value.replace(old, new);
        }
        return value.to_string();
    }
    if let Some(arg) = filter.strip_prefix("default:") {
        if value.is_empty() {
            return arg.to_string();
        }
        return value.to_string();
    }
    // Color functions: lighten:N, darken:N, alpha:N, mix:COLOR:N
    if let Some(arg) = filter.strip_prefix("lighten:") {
        if let Ok(amount) = arg.parse::<f64>()
            && let Some(rgb) = parse_hex_rgb(value)
        {
            let (r, g, b) = lighten_color(rgb, amount / 100.0);
            return format!("#{:02x}{:02x}{:02x}", r, g, b);
        }
        return value.to_string();
    }
    if let Some(arg) = filter.strip_prefix("darken:") {
        if let Ok(amount) = arg.parse::<f64>()
            && let Some(rgb) = parse_hex_rgb(value)
        {
            let (r, g, b) = darken_color(rgb, amount / 100.0);
            return format!("#{:02x}{:02x}{:02x}", r, g, b);
        }
        return value.to_string();
    }
    if let Some(arg) = filter.strip_prefix("alpha:") {
        if let Ok(a) = arg.parse::<f64>()
            && let Some((r, g, b)) = parse_hex_rgb(value)
        {
            let a8 = (a.clamp(0.0, 1.0) * 255.0) as u8;
            return format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a8);
        }
        return value.to_string();
    }
    if let Some(arg) = filter.strip_prefix("mix:") {
        // mix:COLOR:PERCENTAGE (e.g., mix:#ffffff:50)
        let parts: Vec<&str> = arg.splitn(2, ':').collect();
        if parts.len() == 2
            && let (Some(c1), Some(c2), Ok(pct)) = (
                parse_hex_rgb(value),
                parse_hex_rgb(parts[0]),
                parts[1].parse::<f64>(),
            )
        {
            let (r, g, b) = mix_colors(c1, c2, pct / 100.0);
            return format!("#{:02x}{:02x}{:02x}", r, g, b);
        }
        return value.to_string();
    }
    match filter {
        "uppercase" | "upper" => value.to_uppercase(),
        "lowercase" | "lower" => value.to_lowercase(),
        "capitalize" | "cap" => {
            let mut chars = value.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
        "trim" => value.trim().to_string(),
        "length" | "len" => value.chars().count().to_string(),
        "reverse" => value.chars().rev().collect(),
        _ => value.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Variable usage tracking
// ---------------------------------------------------------------------------

/// Scan a string for $name references and record them in the used set.
fn track_var_refs(input: &str, used: &mut HashSet<String>) {
    if !input.contains('$') {
        return;
    }
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$'
            && i + 1 < chars.len()
            && (chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' || chars[i + 1] == '-')
        {
            let start = i + 1;
            let mut end = start;
            while end < chars.len()
                && (chars[end].is_alphanumeric() || chars[end] == '-' || chars[end] == '_')
            {
                end += 1;
            }
            let name: String = chars[start..end].iter().collect();
            used.insert(name);
            i = end;
        } else {
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Unused definition warnings
// ---------------------------------------------------------------------------

fn check_unused(ctx: &mut ParseContext) {
    // Check unused @let variables
    for (name, &line) in &ctx.let_lines {
        if name.starts_with("--") {
            continue; // CSS vars are always used
        }
        if !ctx.used_variables.contains(name) {
            ctx.diagnostics.push(Diagnostic {
                line,
                column: None,
                message: format!("unused variable '${}' (defined but never referenced)", name),
                severity: Severity::Warning,
                source_line: None,
            });
        }
    }

    // Check unused attribute bundles (@let name [...])
    for (name, &line) in &ctx.define_lines {
        if !ctx.used_defines.contains(name) {
            ctx.diagnostics.push(Diagnostic {
                line,
                column: None,
                message: format!(
                    "unused attribute bundle '${}' (defined but never referenced)",
                    name
                ),
                severity: Severity::Warning,
                source_line: None,
            });
        }
    }

    // Check unused functions (@let name ... with body)
    for (name, &line) in &ctx.fn_lines {
        if !ctx.used_functions.contains(name) {
            ctx.diagnostics.push(Diagnostic {
                line,
                column: None,
                message: format!("unused function '@{}' (defined but never called)", name),
                severity: Severity::Warning,
                source_line: None,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal JSON parser for @data directive
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

fn parse_json(input: &str) -> Option<JsonValue> {
    let trimmed = input.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let (val, _) = parse_json_value(&chars, 0)?;
    Some(val)
}

/// Parse JSON with an error message indicating where parsing failed.
fn parse_json_with_error(input: &str) -> Result<JsonValue, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty input".to_string());
    }
    let chars: Vec<char> = trimmed.chars().collect();
    match parse_json_value(&chars, 0) {
        Some((val, _)) => Ok(val),
        None => {
            // Find approximate error position by parsing as far as possible
            let mut deepest = 0usize;
            fn probe(chars: &[char], pos: usize, deepest: &mut usize) {
                let mut p = pos;
                while p < chars.len() {
                    if p > *deepest {
                        *deepest = p;
                    }
                    match chars[p] {
                        '{' | '[' => {
                            p += 1;
                            probe(chars, p, deepest);
                            return;
                        }
                        '"' => {
                            p += 1;
                            while p < chars.len() && chars[p] != '"' {
                                if chars[p] == '\\' {
                                    p += 1;
                                }
                                p += 1;
                            }
                            if p < chars.len() {
                                p += 1;
                            }
                            if p > *deepest {
                                *deepest = p;
                            }
                            return;
                        }
                        _ => {
                            p += 1;
                        }
                    }
                }
            }
            probe(&chars, 0, &mut deepest);
            // Convert char position to line:col
            let prefix: String = chars[..deepest.min(chars.len())].iter().collect();
            let line = prefix.chars().filter(|&c| c == '\n').count() + 1;
            let col = prefix
                .rfind('\n')
                .map_or(prefix.len(), |p| prefix.len() - p - 1)
                + 1;
            let context: String = chars[deepest.saturating_sub(20)..deepest.min(chars.len())]
                .iter()
                .collect();
            Err(format!(
                "at line {}:{} near \"{}\"",
                line,
                col,
                context.trim()
            ))
        }
    }
}

fn parse_json_value(chars: &[char], mut pos: usize) -> Option<(JsonValue, usize)> {
    pos = skip_ws(chars, pos);
    if pos >= chars.len() {
        return None;
    }
    match chars[pos] {
        '"' => {
            let (s, p) = parse_json_string(chars, pos)?;
            Some((JsonValue::Str(s), p))
        }
        '{' => parse_json_object(chars, pos),
        '[' => parse_json_array(chars, pos),
        't' => {
            if chars.get(pos..pos + 4)?.iter().collect::<String>() == "true" {
                Some((JsonValue::Bool(true), pos + 4))
            } else {
                None
            }
        }
        'f' => {
            if chars.get(pos..pos + 5)?.iter().collect::<String>() == "false" {
                Some((JsonValue::Bool(false), pos + 5))
            } else {
                None
            }
        }
        'n' => {
            if chars.get(pos..pos + 4)?.iter().collect::<String>() == "null" {
                Some((JsonValue::Null, pos + 4))
            } else {
                None
            }
        }
        c if c == '-' || c.is_ascii_digit() => {
            let start = pos;
            if chars[pos] == '-' {
                pos += 1;
            }
            while pos < chars.len()
                && (chars[pos].is_ascii_digit()
                    || chars[pos] == '.'
                    || chars[pos] == 'e'
                    || chars[pos] == 'E'
                    || chars[pos] == '+'
                    || chars[pos] == '-')
            {
                if (chars[pos] == '+' || chars[pos] == '-')
                    && pos > start + 1
                    && chars[pos - 1] != 'e'
                    && chars[pos - 1] != 'E'
                {
                    break;
                }
                pos += 1;
            }
            let num: String = chars[start..pos].iter().collect();
            Some((JsonValue::Number(num), pos))
        }
        _ => None,
    }
}

fn parse_json_string(chars: &[char], mut pos: usize) -> Option<(String, usize)> {
    if chars.get(pos) != Some(&'"') {
        return None;
    }
    pos += 1;
    let mut s = String::new();
    while pos < chars.len() && chars[pos] != '"' {
        if chars[pos] == '\\' && pos + 1 < chars.len() {
            pos += 1;
            match chars[pos] {
                '"' | '\\' | '/' => s.push(chars[pos]),
                'n' => s.push('\n'),
                't' => s.push('\t'),
                'r' => s.push('\r'),
                'b' => s.push('\u{8}'),
                'f' => s.push('\u{c}'),
                'u' => {
                    let hex4 = |at: usize| -> Option<u32> {
                        let digits: String = chars.get(at..at + 4)?.iter().collect();
                        u32::from_str_radix(&digits, 16).ok()
                    };
                    let hi = hex4(pos + 1)?;
                    pos += 4;
                    let code = if (0xD800..0xDC00).contains(&hi)
                        && chars.get(pos + 1) == Some(&'\\')
                        && chars.get(pos + 2) == Some(&'u')
                        && let Some(lo) = hex4(pos + 3)
                        && (0xDC00..0xE000).contains(&lo)
                    {
                        pos += 6;
                        0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                    } else {
                        hi
                    };
                    s.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                }
                _ => {
                    s.push('\\');
                    s.push(chars[pos]);
                }
            }
        } else {
            s.push(chars[pos]);
        }
        pos += 1;
    }
    if pos < chars.len() {
        pos += 1; // closing quote
    }
    Some((s, pos))
}

fn parse_json_object(chars: &[char], mut pos: usize) -> Option<(JsonValue, usize)> {
    pos += 1; // skip '{'
    pos = skip_ws(chars, pos);
    let mut pairs = Vec::new();
    if pos < chars.len() && chars[pos] == '}' {
        return Some((JsonValue::Object(pairs), pos + 1));
    }
    loop {
        pos = skip_ws(chars, pos);
        let (key, p) = parse_json_string(chars, pos)?;
        pos = skip_ws(chars, p);
        if pos >= chars.len() || chars[pos] != ':' {
            return None;
        }
        pos += 1;
        let (val, p) = parse_json_value(chars, pos)?;
        pos = p;
        pairs.push((key, val));
        pos = skip_ws(chars, pos);
        if pos >= chars.len() {
            break;
        }
        if chars[pos] == '}' {
            pos += 1;
            break;
        }
        if chars[pos] == ',' {
            pos += 1;
        }
    }
    Some((JsonValue::Object(pairs), pos))
}

fn parse_json_array(chars: &[char], mut pos: usize) -> Option<(JsonValue, usize)> {
    pos += 1; // skip '['
    pos = skip_ws(chars, pos);
    let mut items = Vec::new();
    if pos < chars.len() && chars[pos] == ']' {
        return Some((JsonValue::Array(items), pos + 1));
    }
    loop {
        let (val, p) = parse_json_value(chars, pos)?;
        pos = p;
        items.push(val);
        pos = skip_ws(chars, pos);
        if pos >= chars.len() {
            break;
        }
        if chars[pos] == ']' {
            pos += 1;
            break;
        }
        if chars[pos] == ',' {
            pos += 1;
        }
    }
    Some((JsonValue::Array(items), pos))
}

fn skip_ws(chars: &[char], mut pos: usize) -> usize {
    while pos < chars.len() && chars[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

/// Flatten a JSON value into variable assignments.
/// - Top-level object: each key becomes `prefix.key`
/// - Top-level array: `prefix` becomes comma-separated, `prefix._count` set
/// - Array of objects: each item becomes space-separated values for @each destructuring
fn flatten_json(prefix: &str, value: &JsonValue, vars: &mut HashMap<String, String>) {
    match value {
        JsonValue::Str(s) => {
            vars.insert(prefix.to_string(), s.clone());
        }
        JsonValue::Number(n) => {
            vars.insert(prefix.to_string(), n.clone());
        }
        JsonValue::Bool(b) => {
            vars.insert(prefix.to_string(), b.to_string());
        }
        JsonValue::Null => {
            vars.insert(prefix.to_string(), String::new());
        }
        JsonValue::Object(pairs) => {
            for (key, val) in pairs {
                flatten_json(&format!("{}.{}", prefix, key), val, vars);
            }
        }
        JsonValue::Array(items) => {
            vars.insert(format!("{}._count", prefix), items.len().to_string());
            // Check if all items are objects with the same keys
            let all_objects = items.iter().all(|v| matches!(v, JsonValue::Object(_)));
            if all_objects && !items.is_empty() {
                // Collect keys from first object for destructuring
                if let JsonValue::Object(first_pairs) = &items[0] {
                    let keys: Vec<String> = first_pairs.iter().map(|(k, _)| k.clone()).collect();
                    vars.insert(format!("{}._keys", prefix), keys.join(","));
                }
                // Each item becomes space-separated values, items comma-separated
                let csv: Vec<String> = items
                    .iter()
                    .map(|item| {
                        if let JsonValue::Object(pairs) = item {
                            pairs
                                .iter()
                                .map(|(_, v)| json_value_to_string(v))
                                .collect::<Vec<_>>()
                                .join(" ")
                        } else {
                            json_value_to_string(item)
                        }
                    })
                    .collect();
                vars.insert(prefix.to_string(), csv.join(","));
            } else {
                // Primitive array: comma-separated
                let csv: Vec<String> = items.iter().map(json_value_to_string).collect();
                vars.insert(prefix.to_string(), csv.join(","));
            }
            // Also set indexed access: prefix.0, prefix.1, etc.
            for (i, item) in items.iter().enumerate() {
                flatten_json(&format!("{}.{}", prefix, i), item, vars);
            }
        }
    }
}

fn json_value_to_string(v: &JsonValue) -> String {
    match v {
        JsonValue::Str(s) => s.clone(),
        JsonValue::Number(n) => n.clone(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => String::new(),
        JsonValue::Array(_) | JsonValue::Object(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// HTTP fetch helper (blocking, minimal, no dependencies)
// ---------------------------------------------------------------------------

fn fetch_url_blocking(url: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let is_https = url.starts_with("https://");
    let url_without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "URL must start with http:// or https://".to_string())?;

    let (host_port, path) = match url_without_scheme.find('/') {
        Some(pos) => (&url_without_scheme[..pos], &url_without_scheme[pos..]),
        None => (url_without_scheme, "/"),
    };

    let (host, port) = match host_port.find(':') {
        Some(pos) => (
            &host_port[..pos],
            host_port[pos + 1..]
                .parse::<u16>()
                .map_err(|e| format!("invalid port: {}", e))?,
        ),
        None => (host_port, if is_https { 443 } else { 80 }),
    };

    if is_https {
        return Err("@fetch does not support https:// (no TLS in std). Use @data with a local JSON file, or set up a build script to fetch data before compilation.".to_string());
    }

    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connection failed: {}", e))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: application/json, text/plain, */*\r\n\r\n",
        path, host
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write failed: {}", e))?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| format!("read failed: {}", e))?;

    // Split headers and body (on bytes, so chunk sizes stay byte counts)
    if let Some(body_start) = response.windows(4).position(|w| w == b"\r\n\r\n") {
        let headers = String::from_utf8_lossy(&response[..body_start]);
        let body = &response[body_start + 4..];

        // Check status code
        if let Some(first_line) = headers.lines().next()
            && let Some(code_str) = first_line.split_whitespace().nth(1)
        {
            let code: u16 = code_str.parse().unwrap_or(0);
            if code >= 400 {
                return Err(format!("HTTP {}", code));
            }
        }

        // Handle chunked transfer encoding
        if headers
            .to_lowercase()
            .contains("transfer-encoding: chunked")
        {
            return Ok(String::from_utf8_lossy(&decode_chunked(body)).into_owned());
        }

        Ok(String::from_utf8_lossy(body).into_owned())
    } else {
        Err("malformed HTTP response".to_string())
    }
}

fn decode_chunked(body: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    let mut rest = body;
    while let Some(size_end) = rest.windows(2).position(|w| w == b"\r\n") {
        let size_line = String::from_utf8_lossy(&rest[..size_end]);
        // Chunk extensions (`;name=value`) may follow the size.
        let size_str = size_line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_str, 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        let chunk = &rest[size_end + 2..];
        if size > chunk.len() {
            result.extend_from_slice(chunk);
            break;
        }
        result.extend_from_slice(&chunk[..size]);
        rest = chunk[size..].strip_prefix(b"\r\n").unwrap_or(&chunk[size..]);
    }
    result
}

// ---------------------------------------------------------------------------
// SVG attribute injection helper
// ---------------------------------------------------------------------------

fn set_svg_attr(svg: &str, attr_name: &str, value: &str) -> String {
    // Only the opening <svg ...> tag is touched, so child attributes such as
    // `stroke-width` are left alone.
    let Some(tag_start) = svg.find("<svg") else {
        return svg.to_string();
    };
    let tag_end = svg[tag_start..]
        .find('>')
        .map(|p| tag_start + p)
        .unwrap_or(svg.len());
    let tag = &svg[tag_start..tag_end];

    // Replace an existing attribute (must be preceded by whitespace so that
    // `width` doesn't match `stroke-width`).
    let pattern = format!("{}=\"", attr_name);
    let existing = tag.match_indices(&pattern).find(|(pos, _)| {
        tag[..*pos]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    });
    if let Some((pos, _)) = existing {
        let value_start = tag_start + pos + pattern.len();
        if let Some(end) = svg[value_start..tag_end].find('"') {
            let mut result = String::with_capacity(svg.len());
            result.push_str(&svg[..value_start]);
            result.push_str(value);
            result.push_str(&svg[value_start + end..]);
            return result;
        }
    }

    // Otherwise inject it into the opening tag (before a self-closing `/`).
    let insert_at = if svg[..tag_end].ends_with('/') {
        tag_end - 1
    } else {
        tag_end
    };
    let mut result = String::with_capacity(svg.len() + attr_name.len() + value.len() + 4);
    result.push_str(svg[..insert_at].trim_end());
    result.push(' ');
    result.push_str(attr_name);
    result.push_str("=\"");
    result.push_str(value);
    result.push('"');
    result.push_str(&svg[insert_at..]);
    result
}

// ---------------------------------------------------------------------------
// Markdown → HTML conversion (minimal subset)
// ---------------------------------------------------------------------------

fn markdown_to_html(lines: &[String]) -> String {
    let mut html = String::new();
    // Tag of the list currently open ("ul" or "ol"), if any.
    let mut list: Option<&'static str> = None;
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut para_buf = String::new();

    let flush_para = |para: &mut String, out: &mut String| {
        let trimmed = para.trim();
        if !trimmed.is_empty() {
            out.push_str("<p>");
            out.push_str(&md_inline(trimmed));
            out.push_str("</p>\n");
        }
        para.clear();
    };
    let close_list = |list: &mut Option<&'static str>, out: &mut String| {
        if let Some(tag) = list.take() {
            out.push_str(&format!("</{}>\n", tag));
        }
    };
    let open_list = |list: &mut Option<&'static str>, tag: &'static str, out: &mut String| {
        if *list != Some(tag) {
            if let Some(prev) = list.take() {
                out.push_str(&format!("</{}>\n", prev));
            }
            out.push_str(&format!("<{}>\n", tag));
            *list = Some(tag);
        }
    };

    for line in lines {
        let trimmed = line.trim();

        // Fenced code blocks
        if let Some(after_fence) = trimmed.strip_prefix("```") {
            if in_code_block {
                html.push_str("<pre><code");
                if !code_lang.is_empty() {
                    html.push_str(&format!(" class=\"language-{}\"", code_lang));
                }
                html.push('>');
                html.push_str(&html_escape_md(&code_buf));
                html.push_str("</code></pre>\n");
                code_buf.clear();
                code_lang.clear();
                in_code_block = false;
            } else {
                flush_para(&mut para_buf, &mut html);
                close_list(&mut list, &mut html);
                code_lang = after_fence.trim().to_string();
                in_code_block = true;
            }
            continue;
        }
        if in_code_block {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            // Keep indentation inside code blocks.
            code_buf.push_str(line.trim_end());
            continue;
        }

        // Blank line ends paragraph
        if trimmed.is_empty() {
            flush_para(&mut para_buf, &mut html);
            close_list(&mut list, &mut html);
            continue;
        }

        // Headings
        let heading_level = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&heading_level) && trimmed.as_bytes().get(heading_level) == Some(&b' ')
        {
            flush_para(&mut para_buf, &mut html);
            close_list(&mut list, &mut html);
            let text = &trimmed[heading_level + 1..];
            html.push_str(&format!(
                "<h{}>{}</h{}>\n",
                heading_level,
                md_inline(text),
                heading_level
            ));
            continue;
        }

        // Horizontal rule (checked before list items so `---`/`***` aren't
        // mistaken for bullets)
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            flush_para(&mut para_buf, &mut html);
            close_list(&mut list, &mut html);
            html.push_str("<hr>\n");
            continue;
        }

        // Unordered list items
        if (trimmed.starts_with("- ") || trimmed.starts_with("* ")) && trimmed.len() > 2 {
            flush_para(&mut para_buf, &mut html);
            open_list(&mut list, "ul", &mut html);
            html.push_str(&format!("<li>{}</li>\n", md_inline(&trimmed[2..])));
            continue;
        }

        // Ordered list items
        if let Some(dot_pos) = trimmed.find(". ")
            && (1..=3).contains(&dot_pos)
            && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit())
        {
            flush_para(&mut para_buf, &mut html);
            open_list(&mut list, "ol", &mut html);
            html.push_str(&format!(
                "<li>{}</li>\n",
                md_inline(&trimmed[dot_pos + 2..])
            ));
            continue;
        }

        // Blockquote
        if let Some(quote_content) = trimmed.strip_prefix("> ") {
            flush_para(&mut para_buf, &mut html);
            close_list(&mut list, &mut html);
            html.push_str(&format!(
                "<blockquote><p>{}</p></blockquote>\n",
                md_inline(quote_content)
            ));
            continue;
        }

        // Otherwise, accumulate paragraph text
        close_list(&mut list, &mut html);
        if !para_buf.is_empty() {
            para_buf.push(' ');
        }
        para_buf.push_str(trimmed);
    }

    // Flush remaining
    close_list(&mut list, &mut html);
    flush_para(&mut para_buf, &mut html);

    html
}

fn md_inline(text: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Bold: **text** or __text__
        if i + 1 < chars.len()
            && ((chars[i] == '*' && chars[i + 1] == '*')
                || (chars[i] == '_' && chars[i + 1] == '_'))
        {
            let marker = chars[i];
            if let Some(end) = find_closing_double(&chars, i + 2, marker) {
                let inner: String = chars[i + 2..end].iter().collect();
                result.push_str("<strong>");
                result.push_str(&md_inline(&inner));
                result.push_str("</strong>");
                i = end + 2;
                continue;
            }
        }
        // Italic: *text* or _text_
        if (chars[i] == '*' || chars[i] == '_') && i + 1 < chars.len() && chars[i + 1] != chars[i] {
            let marker = chars[i];
            if let Some(end) = find_closing_single(&chars, i + 1, marker) {
                let inner: String = chars[i + 1..end].iter().collect();
                result.push_str("<em>");
                result.push_str(&md_inline(&inner));
                result.push_str("</em>");
                i = end + 1;
                continue;
            }
        }
        // Inline code: `text`
        if chars[i] == '`'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == '`')
        {
            let inner: String = chars[i + 1..i + 1 + end].iter().collect();
            result.push_str("<code>");
            result.push_str(&html_escape_md(&inner));
            result.push_str("</code>");
            i = i + 2 + end;
            continue;
        }
        // Links: [text](url)
        if chars[i] == '['
            && let Some(close_bracket) = chars[i + 1..].iter().position(|&c| c == ']')
        {
            let after = i + 1 + close_bracket + 1;
            if after < chars.len()
                && chars[after] == '('
                && let Some(close_paren) = chars[after + 1..].iter().position(|&c| c == ')')
            {
                let text: String = chars[i + 1..i + 1 + close_bracket].iter().collect();
                let url: String = chars[after + 1..after + 1 + close_paren].iter().collect();
                result.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    html_escape_md(&url),
                    md_inline(&text)
                ));
                i = after + 2 + close_paren;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn find_closing_double(chars: &[char], start: usize, marker: char) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == marker && chars[i + 1] == marker {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_closing_single(chars: &[char], start: usize, marker: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == marker)
}

/// Simple glob matching supporting `*` as wildcard for any characters and `?` for a single character.
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();
    let mut star_pi = usize::MAX;
    let mut star_ti = 0;
    while ti < tb.len() {
        if pi < pb.len() && (pb[pi] == b'?' || pb[pi] == tb[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < pb.len() && pb[pi] == b'*' {
            star_pi = pi;
            star_ti = ti;
            pi += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < pb.len() && pb[pi] == b'*' {
        pi += 1;
    }
    pi == pb.len()
}

fn html_escape_md(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> ParseResult {
        let r = parse(src);
        assert!(
            r.diagnostics.iter().all(|d| d.severity != Severity::Error),
            "unexpected error diagnostics: {:?}",
            r.diagnostics,
        );
        r
    }

    #[test]
    fn parses_empty_input() {
        let r = parse("");
        assert!(r.document.nodes.is_empty());
        assert!(r.diagnostics.is_empty());
    }

    #[test]
    fn parses_bare_text_node() {
        let r = parse_ok("Hello world\n");
        assert_eq!(r.document.nodes.len(), 1);
    }

    #[test]
    fn parses_page_directive() {
        let r = parse_ok("@page My Title\n");
        assert_eq!(r.document.page_title.as_deref(), Some("My Title"));
    }

    #[test]
    fn let_variable_substitution() {
        let r = parse_ok("@let color red\n@text [color $color] hi\n");
        assert_eq!(
            r.document.variables.get("color").map(|s| s.as_str()),
            Some("red")
        );
    }

    #[test]
    fn error_recovery_continues_after_unknown_element() {
        // Intentionally malformed line followed by a valid one — recovery must
        // let us still see the good line in diagnostics / output.
        let r = parse("@notareal\n@text hi\n");
        let has_error = r.diagnostics.iter().any(|d| d.severity == Severity::Error);
        assert!(has_error, "expected at least one error for @notareal");
    }

    #[test]
    fn undefined_fn_is_reported_not_panics() {
        // Prior to the unwrap fix this panicked.
        let r = parse("@missing\n");
        assert!(r.diagnostics.iter().any(|d| d.severity == Severity::Error));
    }

    #[test]
    fn scope_without_selector_does_not_panic() {
        let r = parse("@scope\n  body { background red; }\n");
        // Should parse — may or may not emit diagnostics, but must not panic.
        let _ = r;
    }

    #[test]
    fn levenshtein_bounded_respects_cutoff() {
        let a: Vec<char> = "hello".chars().collect();
        let b: Vec<char> = "world".chars().collect();
        // Full distance is 4, but asking for cutoff 2 should early-exit.
        let d = levenshtein_bounded(&a, &b, 2);
        assert!(d > 2, "expected early exit above cutoff, got {}", d);
    }

    #[test]
    fn severity_has_all_variants() {
        // This compiles only if Severity has Error, Warning, Info, Help.
        let all = [
            Severity::Error,
            Severity::Warning,
            Severity::Info,
            Severity::Help,
        ];
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn arithmetic_empty_operator_does_not_panic() {
        // Guards the `op.trim().chars().next().unwrap()` fix.
        let r = parse("@let x 1 + 2\n");
        assert_eq!(r.document.variables.get("x").map(|s| s.as_str()), Some("3"));
    }

    #[test]
    fn decode_chunked_handles_bytes_and_malformed_input() {
        // Chunk sizes are byte counts: "é" is 2 bytes.
        let body = "3\r\naé\r\n2\r\nbc\r\n0\r\n\r\n".as_bytes();
        assert_eq!(decode_chunked(body), "aébc".as_bytes());
        // No CRLF after the size line — must not panic.
        assert_eq!(decode_chunked(b"5"), b"");
        // Declared size larger than the data — take what's there.
        assert_eq!(decode_chunked(b"a\r\nabc"), b"abc");
    }
}
