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
    /// Source line currently being parsed, for diagnostics raised deep
    /// inside helpers that don't take a line number.
    current_line: usize,
    /// Functions whose body has a scoped @style block.
    scoped_functions: std::collections::HashSet<String>,
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
    /// @manifest configuration
    manifest: Option<crate::ast::ManifestConfig>,
    /// Track @import paths for circular dependency detection
    import_stack: Vec<PathBuf>,
}

struct Parser {
    lines: Vec<Line>,
    pos: usize,
}

impl ParseContext {
    /// Evaluate an expression (see `expr.rs`), reporting errors at `line`.
    fn eval(&mut self, src: &str, line: usize) -> Option<crate::expr::Value> {
        track_var_refs(src, &mut self.used_variables);
        let vars = &self.variables;
        let resolve = |reference: &str| {
            let name = reference.split('|').next().unwrap_or(reference);
            vars.contains_key(name)
                .then(|| substitute_vars(&format!("${}", reference), vars))
        };
        let result = crate::expr::eval(src, &resolve);
        match result {
            Ok(value) => Some(value),
            Err(message) => {
                self.diagnostics.push(Diagnostic {
                    line,
                    column: None,
                    message: format!("invalid expression: {}", message),
                    severity: Severity::Error,
                    source_line: Some(src.to_string()),
                });
                None
            }
        }
    }

    /// Evaluate a condition; an invalid one is reported and counts as false.
    fn condition(&mut self, src: &str, line: usize) -> bool {
        self.eval(src, line).is_some_and(|value| value.truthy())
    }
}

/// The standard library (`std.hl`): components and bundles defined in
/// htmlang itself and available in every file.
const PRELUDE: &str = include_str!("std.hl");

fn load_prelude(ctx: &mut ParseContext) {
    let mut prelude = Parser {
        lines: preprocess(PRELUDE),
        pos: 0,
    };
    let _ = prelude.parse_children(0, ctx);
    // Library definitions aren't the file's own: never report them unused.
    ctx.fn_lines.clear();
    ctx.define_lines.clear();
    ctx.let_lines.clear();
}

pub fn parse(input: &str) -> ParseResult {
    parse_with_base(input, None)
}

pub fn parse_with_base(input: &str, base_path: Option<&Path>) -> ParseResult {
    let lines = preprocess(input);
    let mut parser = Parser { lines, pos: 0 };
    let mut ctx = ParseContext {
        current_line: 0,
        scoped_functions: std::collections::HashSet::new(),
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
        manifest: None,
        import_stack: Vec::new(),
    };
    load_prelude(&mut ctx);
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
        ctx.current_line = line_num;
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

        // Removed syntax gets a pointer to its replacement — unless the
        // name is a user function (e.g. `@let divider`).
        if content.starts_with('@')
            && !ctx.functions.contains_key(extract_element_name(&content))
            && let Some(hint) = removed_syntax_hint(&content)
        {
            return Err(ParseError {
                line: line_num,
                message: hint,
            });
        }

        // @page [lang en, favicon /f.png, canonical URL, base URL] Title
        if let Some(rest) = content.strip_prefix("@page ") {
            let rest = rest.trim_start();
            let title = if rest.starts_with('[') {
                let (attrs, title) = parse_attr_brackets_no_validate(rest, line_num, ctx)?;
                for attr in attrs {
                    let value = attr.value.unwrap_or_default();
                    match attr.key.as_str() {
                        "lang" => ctx.lang = Some(value),
                        "favicon" => ctx.favicon = Some(value),
                        "canonical" => ctx.canonical = Some(value),
                        "base" => ctx.base_url = Some(value),
                        other => ctx.diagnostics.push(Diagnostic {
                            line: line_num,
                            column: None,
                            message: format!(
                                "unknown @page attribute '{}' (expected lang, favicon, canonical or base)",
                                other
                            ),
                            severity: Severity::Warning,
                            source_line: Some(content.clone()),
                        }),
                    }
                }
                title
            } else {
                rest.to_string()
            };
            ctx.page_title = Some(substitute_vars(title.trim(), &ctx.variables));
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

                // An @style block at the top of the body is scoped to the
                // function: its rules apply inside a `.hl-NAME` wrapper.
                let (body_lines, style_lines) = split_style_block(body_lines);
                if !style_lines.is_empty() {
                    let scope_class = format!("hl-{}", name);
                    let scoped_css: String = style_lines
                        .iter()
                        .filter_map(|line| match &line.content {
                            LineContent::Normal(s) if !s.trim().is_empty() => {
                                Some(format!(".{} {}\n", scope_class, s.trim()))
                            }
                            _ => None,
                        })
                        .collect();
                    if !scoped_css.is_empty() {
                        ctx.custom_css.push(scoped_css);
                    }
                    ctx.scoped_functions.insert(name.clone());
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
                // `@let name = EXPR` computes its value (see expr.rs); any
                // other value is literal text with `$var` interpolation.
                if let Some(expression) = value.strip_prefix('=') {
                    let value = ctx
                        .eval(expression.trim(), line_num)
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    if name.starts_with("--") {
                        ctx.css_vars.push((name.to_string(), value.clone()));
                    }
                    ctx.variables.insert(name.to_string(), value);
                    ctx.let_lines.entry(name.to_string()).or_insert(line_num);
                    return Ok(None);
                }

                // Attribute bundle: @let name [attr1, attr2, ...]
                if value.starts_with('[') {
                    let (attrs, _) = parse_attr_brackets(value, line_num, ctx)?;
                    ctx.defines.insert(name.to_string(), attrs);
                    ctx.define_lines.entry(name.to_string()).or_insert(line_num);
                    return Ok(None);
                }

                // Multi-line @let with triple quotes: @let name """..."""
                let value_str;
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
                if name.starts_with("--") {
                    // CSS custom property
                    ctx.css_vars.push((name.to_string(), value.clone()));
                }
                ctx.variables.insert(name.to_string(), value);
                ctx.let_lines.entry(name.to_string()).or_insert(line_num);
            }
            return Ok(None);
        }

        // @meta NAME VALUE; `og:` names become Open Graph property tags
        if let Some(rest) = content.strip_prefix("@meta ") {
            let rest = rest.trim();
            if let Some((name, value)) = rest.split_once(' ') {
                let value = substitute_vars(value.trim(), &ctx.variables);
                match name.trim().strip_prefix("og:") {
                    Some(property) => ctx.og_tags.push((property.to_string(), value)),
                    None => ctx.meta_tags.push((name.trim().to_string(), value)),
                }
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


        // --- @assert directive (compile-time assertions) ---

        if let Some(rest) = content.strip_prefix("@assert ") {
            let rest = rest.trim();
            if !ctx.condition(rest, line_num) {
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
            let rest = rest.trim();
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
                let mut matched_nodes = Vec::new();
                for file_path in matched_files {
                    let rel_name = file_path
                        .strip_prefix(&base_dir)
                        .unwrap_or(&file_path)
                        .to_string_lossy()
                        .to_string();
                    // Synthesize an @include line for each matched file
                    let import_line = match &alias {
                        Some(pfx) => {
                            let stem = file_path
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            format!("@include \"{}\" as {}.{}", rel_name, pfx, stem)
                        }
                        None => format!("@include \"{}\"", rel_name),
                    };
                    let synth_lines = preprocess(&import_line);
                    let mut synth_parser = Parser {
                        lines: synth_lines,
                        pos: 0,
                    };
                    matched_nodes.extend(synth_parser.parse_children(0, ctx));
                }
                return Ok(Some(matched_nodes));
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
                        "circular include '{}' (cycle: {} → {})",
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
            ctx.import_stack.push(resolved.clone());
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            let diag_count_before = ctx.diagnostics.len();
            let mut included_nodes = Vec::new();

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
                let included_lines = preprocess(&imported_text);
                let mut included_parser = Parser {
                    lines: included_lines,
                    pos: 0,
                };
                included_nodes = included_parser.parse_children(0, ctx);
            }

            // Annotate new diagnostics with import chain
            let import_chain = format_include_chain(&ctx.include_stack);
            for d in &mut ctx.diagnostics[diag_count_before..] {
                d.message = format!("{}\n  in {}", d.message, import_chain);
            }

            ctx.base_path = saved_base;
            ctx.include_stack.pop();
            ctx.import_stack.pop();
            return Ok(Some(included_nodes));
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


        // --- @data (load JSON file into variables) ---

        // @data file.json               top-level keys become variables
        // @data $name file.json         values as $name.key
        // @data $name dir/*.json        each file as $name.STEM.key; $name
        //                               lists the stems, $name._count counts them
        // @data $name env:NAME [DEFAULT] an environment variable
        if let Some(rest) = content.strip_prefix("@data ") {
            let rest = rest.trim();
            let (prefix, filename) = match rest.strip_prefix('$') {
                Some(named) => match named.split_once(char::is_whitespace) {
                    Some((name, source)) => (name.to_string(), source.trim().to_string()),
                    None => {
                        return Err(ParseError {
                            line: line_num,
                            message: "@data requires: @data $name SOURCE or @data file.json"
                                .to_string(),
                        });
                    }
                },
                None => (String::new(), rest.to_string()),
            };

            if let Some(env) = filename.strip_prefix("env:") {
                let (var, default) = match env.split_once(char::is_whitespace) {
                    Some((var, default)) => (var, Some(default.trim())),
                    None => (env, None),
                };
                if prefix.is_empty() {
                    return Err(ParseError {
                        line: line_num,
                        message: format!("@data env:{} needs a name: @data $name env:{}", var, var),
                    });
                }
                let value = std::env::var(var)
                    .ok()
                    .or_else(|| default.map(|d| substitute_vars(d, &ctx.variables)));
                if value.is_none() {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!(
                            "environment variable '{}' is not set and has no default",
                            var
                        ),
                        severity: Severity::Warning,
                        source_line: Some(content.clone()),
                    });
                }
                ctx.variables.insert(prefix, value.unwrap_or_default());
                return Ok(None);
            }

            let filename = substitute_vars(&filename, &ctx.variables);
            if filename.contains('*') {
                if prefix.is_empty() {
                    return Err(ParseError {
                        line: line_num,
                        message: format!("@data {} needs a name: @data $name {}", filename, filename),
                    });
                }
                self.load_data_glob(&prefix, &filename, line_num, &content, ctx);
                return Ok(None);
            }

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


        // --- @if / @else ---

        if let Some(rest) = content.strip_prefix("@if ") {
            let result = ctx.condition(rest.trim(), line_num);

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
                        let cond_result =
                            ctx.condition(else_if_cond.trim(), self.lines[self.pos - 1].line_num);
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

            // Collect slot blocks defined in the extending file. Everything
            // else becomes the default content, filling the layout's
            // @children.
            let mut slot_contents: HashMap<String, Vec<Line>> = HashMap::new();
            let mut default_lines: Vec<Line> = Vec::new();
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
                default_lines.push(self.lines[self.pos].clone());
                self.pos += 1;
            }
            let default_nodes = if default_lines.is_empty() {
                Vec::new()
            } else {
                let min_indent = default_lines.iter().map(|l| l.indent).min().unwrap_or(0);
                let adjusted: Vec<Line> = default_lines
                    .iter()
                    .map(|l| Line {
                        indent: l.indent - min_indent,
                        content: l.content.clone(),
                        line_num: l.line_num,
                    })
                    .collect();
                let mut default_parser = Parser {
                    lines: adjusted,
                    pos: 0,
                };
                default_parser.parse_children(0, ctx)
            };

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

            // Fill @slot placeholders and @children in the layout
            let result_nodes = replace_children_and_slots(layout_nodes, &default_nodes, &slot_nodes);
            return Ok(Some(result_nodes));
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

    /// `@data $name dir/*.json`: load every matching file as
    /// `$name.STEM.key`, list the stems in `$name` and count them.
    fn load_data_glob(
        &mut self,
        name: &str,
        pattern: &str,
        line_num: usize,
        content: &str,
        ctx: &mut ParseContext,
    ) {
        let base = ctx.base_path.clone().unwrap_or_else(|| PathBuf::from("."));
        let pattern_path = Path::new(pattern);
        let dir = match pattern_path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => base.join(dir),
            _ => base,
        };
        let file_pattern = pattern_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| glob_match(&file_pattern, &n.to_string_lossy()))
                })
                .collect(),
            Err(e) => {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("cannot read directory for '{}': {}", pattern, e),
                    severity: Severity::Error,
                    source_line: Some(content.to_string()),
                });
                return;
            }
        };
        files.sort();
        let mut stems = Vec::new();
        for file in files {
            let stem = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let text = match std::fs::read_to_string(&file) {
                Ok(text) => text,
                Err(e) => {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("cannot load data '{}': {}", file.display(), e),
                        severity: Severity::Error,
                        source_line: Some(content.to_string()),
                    });
                    continue;
                }
            };
            match parse_json_with_error(&text) {
                Ok(json) => {
                    let mut vars = HashMap::new();
                    flatten_json(&format!("{}.{}", name, stem), &json, &mut vars);
                    ctx.variables.extend(vars);
                    stems.push(stem);
                }
                Err(detail) => ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("invalid JSON in '{}': {}", file.display(), detail),
                    severity: Severity::Error,
                    source_line: Some(content.to_string()),
                }),
            }
            ctx.included_files.push(file);
        }
        ctx.variables
            .insert(format!("{}._count", name), stems.len().to_string());
        ctx.variables.insert(name.to_string(), stems.join(", "));
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

        let (args, trailing_text) = if rest.starts_with('[') {
            parse_attr_brackets_no_validate(rest, line_num, ctx)?
        } else {
            (Vec::new(), rest.to_string())
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
        // Text after the call, as in `@badge [color red] New`, is content:
        // it goes first among the caller's children.
        let trailing_text = trailing_text.trim();
        if !trailing_text.is_empty() {
            caller_children.insert(0, Node::Text(parse_text_segments(trailing_text, ctx)));
        }

        // Save variable state, inject function parameters
        let saved_vars = ctx.variables.clone();
        let mut consumed = vec![false; args.len()];
        for (i, param) in fn_def.params.iter().enumerate() {
            let named = args.iter().position(|a| a.key == *param);
            // Positional fallback, unless that argument is named for a
            // different parameter.
            let positional = args
                .get(i)
                .filter(|a| !fn_def.params.contains(&a.key) && a.value.is_some())
                .map(|_| i);
            let value = named
                .filter(|&j| args[j].value.is_some())
                .or(positional)
                .and_then(|j| {
                    consumed[j] = true;
                    args[j].value.clone()
                })
                .or_else(|| fn_def.defaults.get(param).cloned())
                .unwrap_or_default();
            if let Some(j) = named {
                consumed[j] = true;
            }
            ctx.variables.insert(param.clone(), value);
        }
        // Arguments that aren't parameters are attributes for the
        // function's root element, so a function can be styled like an
        // element: `@badge [background red] New`.
        let forwarded: Vec<Attribute> = args
            .iter()
            .zip(&consumed)
            .filter(|&(_, &used)| !used)
            .map(|(a, _)| a.clone())
            .collect();

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

        if !forwarded.is_empty() {
            let mut roots = result_nodes.iter_mut().filter_map(|n| match n {
                Node::Element(e) => Some(e),
                _ => None,
            });
            match (roots.next(), roots.next()) {
                (Some(root), None) => root.attrs.extend(forwarded),
                _ => ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "attributes {} on @{} are not parameters, and its body has no single \
                         root element to receive them",
                        forwarded
                            .iter()
                            .map(|a| format!("'{}'", a.key))
                            .collect::<Vec<_>>()
                            .join(", "),
                        name
                    ),
                    severity: Severity::Warning,
                    source_line: Some(content.to_string()),
                }),
            }
        }

        // A function with a scoped @style wraps its output in the scope class
        if ctx.scoped_functions.contains(name) {
            let wrapper = Element {
                kind: ElementKind::El,
                attrs: vec![Attribute {
                    key: "class".to_string(),
                    value: Some(format!("hl-{}", name)),
                    html: true,
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

    // Every other element treats its argument as leading text content, as
    // in `@el [padding 8] Hello` or `@paragraph Read {@link /more more}`.
    let argument = match argument {
        Some(_) if !argument_is_special(&kind) && !renders_argument_as_text(&kind) => {
            children.insert(0, Node::Text(parse_text_segments(&rest, ctx)));
            None
        }
        other => other,
    };

    Ok(Element {
        kind,
        attrs,
        argument,
        children,
        line_num,
    })
}

/// Elements whose argument is not text content: a URL, a source, an action,
/// or a slot name.
fn argument_is_special(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Link | ElementKind::Image | ElementKind::Script | ElementKind::Slot(_)
    ) || kind
        .spec()
        .is_some_and(|spec| matches!(spec.arg, TagArg::Attr(_)))
}

const KNOWN_DIRECTIVES: &[&str] = &[
    "page",
    "let",
    "include",
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
    "breakpoint",
    "theme",
    "deprecated",
    "extends",
    "font-face",
    "json-ld",
    "assert",
    "markdown",
    "manifest",
];

fn parse_element_kind(s: &str, line_num: usize) -> Result<ElementKind, ParseError> {
    if let Some(kind) = ElementKind::from_name(s) {
        return Ok(kind);
    }
    if let Some(hint) = removed_syntax_hint(&format!("@{}", s)) {
        return Err(ParseError {
            line: line_num,
            message: hint,
        });
    }
    let all_known: Vec<&str> = ElementKind::all_names()
        .chain(KNOWN_DIRECTIVES.iter().copied())
        .collect();
    let message = match suggest_closest(s, &all_known) {
        Some(closest) => format!("unknown element @{}, did you mean @{}?", s, closest),
        None => format!("unknown element @{}", s),
    };
    Err(ParseError {
        line: line_num,
        message,
    })
}

/// Directives and element names removed when the language was simplified,
/// with what replaces each. `htmlang upgrade` rewrites all of them.
const REMOVED_SYNTAX: &[(&str, &str)] = &[
    ("@fn", "use `@let name $param` with an indented body"),
    ("@define", "use `@let name [attributes]`"),
    ("@mixin", "use `@let name [attributes]`"),
    ("@unless", "use `@if not <condition>`"),
    ("@for", "use `@each $i in 1..10`"),
    ("@repeat", "use `@each $_ in 1..N`"),
    (
        "@switch",
        "use `@match`, with `@let __switch [...]` inside a case for its attributes",
    ),
    ("@use", "use `@include`"),
    (
        "@import",
        "use `@include` (a file with only definitions emits no content)",
    ),
    ("@with", "use `@let alias $source`"),
    (
        "@layout",
        "use `@extends` (page content outside `@slot` blocks fills `@children`)",
    ),
    ("@scope", "write the `@scope` rule in an `@style` block"),
    (
        "@starting-style",
        "write the `@starting-style` rule in an `@style` block",
    ),
    ("@css-property", "write an `@property` rule in an `@style` block"),
    ("@lang", "use `@page [lang ...] Title`"),
    ("@favicon", "use `@page [favicon ...] Title`"),
    ("@canonical", "use `@page [canonical ...] Title`"),
    ("@base", "use `@page [base ...] Title`"),
    ("@og", "use `@meta og:NAME VALUE`"),
    ("@debug", "use `@warn`"),
    (
        "@collection",
        "use `@data $name dir/*.json` (each file becomes `$name.STEM.key`)",
    ),
    ("@env", "use `@data $name env:NAME [default]`"),
    (
        "@fetch",
        "download the data before building and use `@data $name file.json`",
    ),
    (
        "@translations",
        "put each locale's strings in a JSON file and use `@data $t locales/$lang.json`",
    ),
    ("@defer", "remove it: the content is already in the page"),
    ("@log", "use `@warn`"),
    (
        "@component",
        "use `@let`: an `@style` block in a function body is scoped to it",
    ),
    ("@col", "use `@column`"),
    ("@p", "use `@paragraph`"),
    ("@img", "use `@image`"),
    ("@li", "use `@item`"),
    ("@btn", "use `@button`"),
    ("@ul", "use `@list`"),
    ("@divider", "use `@hr`"),
    ("@opt", "use `@option`"),
];

/// Attributes that became standard-library bundles or plain CSS.
fn removed_attribute_hint(name: &str) -> Option<&'static str> {
    match name {
        "skeleton" => Some("use the `$skeleton` bundle"),
        "no-scrollbar" => Some("use the `$no-scrollbar` bundle"),
        "gradient" => Some("use `background linear-gradient(...)`"),
        "animate" => Some("use `animation`"),
        "inset-area" => Some("use `position-area`"),
        _ => None,
    }
}

/// If `content` starts with removed syntax, the error message for it.
fn removed_syntax_hint(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    REMOVED_SYNTAX.iter().find_map(|(name, replacement)| {
        let rest = trimmed.strip_prefix(name)?;
        if !(rest.is_empty() || rest.starts_with([' ', '['])) {
            return None;
        }
        Some(format!(
            "`{}` was removed: {} (run `htmlang upgrade` to rewrite it automatically)",
            name, replacement
        ))
    })
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
    let base_key = crate::vocab::base_attribute(attr.key.as_str());

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

        // $define reference — expand attribute bundle
        if let Some(name) = part.strip_prefix('$')
            && let Some(define_attrs) = ctx.defines.get(name)
        {
            ctx.used_defines.insert(name.to_string());
            attrs.extend(define_attrs.clone());
            continue;
        }

        track_var_refs(part, &mut ctx.used_variables);

        // Conditional attribute: `key if condition` or `key value if condition`.
        // Conditions are evaluated on the source text, before substitution.
        let part = match split_trailing_if(part) {
            (attr, Some(condition)) => {
                if !ctx.condition(condition, line_num) {
                    continue;
                }
                attr
            }
            (attr, None) => attr,
        };
        // A value `if(cond, a, b)` picks `a` or `b` (free text) by `cond`.
        let part = choose_if_value(part, ctx, line_num);
        let part = substitute_vars(&part, &ctx.variables);

        let attr = if let Some((key, value)) = split_html_attribute(&part) {
            Attribute {
                key: key.to_string(),
                value: Some(value.to_string()),
                html: true,
            }
        } else if let Some((key, value)) = part.split_once(' ') {
            let value = value.trim().to_string();
            Attribute {
                key: key.trim().to_string(),
                value: Some(value),
                html: false,
            }
        } else {
            Attribute {
                key: part.to_string(),
                value: None,
                html: false,
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
            if !attr.html
                && matches!(crate::vocab::base_attribute(&attr.key), "background" | "color")
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

        // Warn on unknown attributes. `key=value` HTML attributes may use any
        // name; everything else must be a known style.
        if validate && !attr.html {
            let base_key = crate::vocab::base_attribute(attr.key.as_str());
            let is_boolean_html =
                attr.value.is_none() && crate::vocab::BOOLEAN_HTML_ATTRS.contains(&base_key);
            if is_boolean_html || crate::vocab::is_style_attribute(base_key) {
                validate_attr_value(&attr, line_num, ctx);
            } else if crate::vocab::HTML_ATTRIBUTES.contains(&base_key)
                || base_key.starts_with("aria-")
                || base_key.starts_with("data-")
            {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "'{}' is an HTML attribute: write `{}={}` (run `htmlang upgrade`)",
                        attr.key,
                        attr.key,
                        attr.value.as_deref().unwrap_or("")
                    ),
                    severity: Severity::Warning,
                    source_line: None,
                });
            } else if let Some(hint) = removed_attribute_hint(base_key) {
                ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!(
                        "attribute '{}' was removed: {} (run `htmlang upgrade`)",
                        base_key, hint
                    ),
                    severity: Severity::Warning,
                    source_line: None,
                });
            } else {
                let suggestion = suggest_closest(base_key, &crate::vocab::all_attributes());
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
            }
        }

        attrs.push(attr);
    }

    attrs
}

/// Split an HTML attribute written `key=value` (`alt=`, `type=email`,
/// `aria-label=Close menu`). Returns `None` for style attributes.
fn split_html_attribute(part: &str) -> Option<(&str, &str)> {
    let first_token = part.split(char::is_whitespace).next()?;
    let eq = first_token.find('=')?;
    let key = &part[..eq];
    let valid_key = key.starts_with(|c: char| c.is_ascii_alphabetic())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    valid_key.then(|| (key, part[eq + 1..].trim()))
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

            match parse_single_element(&inner, ctx.current_line, ctx) {
                Ok(elem) => segments.push(TextSegment::Inline(elem)),
                Err(e) => {
                    // Keep the braces as literal text, but flag what looks
                    // like a mistyped inline element.
                    if inner.trim_start().starts_with('@') {
                        ctx.diagnostics.push(Diagnostic {
                            line: ctx.current_line,
                            column: None,
                            message: e.message,
                            severity: Severity::Warning,
                            source_line: Some(input.to_string()),
                        });
                    }
                    current_text.push('{');
                    current_text.push_str(&inner);
                    current_text.push('}');
                }
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



/// Evaluate `if(condition, true_val, false_val)` expressions in attribute values.
/// Split `key value if condition` into the attribute and its condition
/// (ignoring ` if ` inside parentheses).
fn split_trailing_if(part: &str) -> (&str, Option<&str>) {
    let mut depth = 0;
    for (i, c) in part.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0 && part[i..].starts_with(" if ") => {
                return (&part[..i], Some(part[i + 4..].trim()));
            }
            _ => {}
        }
    }
    (part, None)
}

/// Resolve a value written `if(cond, a, b)`: evaluate `cond` and keep the
/// chosen branch's text. Other attributes are returned unchanged.
fn choose_if_value(part: &str, ctx: &mut ParseContext, line: usize) -> String {
    let (head, value) = match part.find(['=', ' ']) {
        Some(pos) => part.split_at(pos + 1),
        None => return part.to_string(),
    };
    let value = value.trim();
    let Some(inner) = value.strip_prefix("if(").and_then(|v| v.strip_suffix(')')) else {
        return part.to_string();
    };
    let args = split_if_args(inner);
    if args.len() != 3 {
        return part.to_string();
    }
    let branch = if ctx.condition(args[0].trim(), line) {
        args[1]
    } else {
        args[2]
    };
    format!("{}{}", head, branch.trim())
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

// ---------------------------------------------------------------------------
// Post-parse validation (context-dependent warnings)
// ---------------------------------------------------------------------------

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

fn element_kind_name(kind: &ElementKind) -> String {
    format!("@{}", kind.name())
}

fn is_container(kind: &ElementKind) -> bool {
    matches!(kind, ElementKind::Row | ElementKind::Column | ElementKind::El)
        || kind.spec().is_some_and(|spec| spec.container)
}

fn validate_tree(
    nodes: &[Node],
    parent_kind: Option<&ElementKind>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for node in nodes {
        if let Node::Element(elem) = node {
            for attr in &elem.attrs {
                let base = crate::vocab::base_attribute(&attr.key);
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
                    && !(elem.kind.is_tag("input") || elem.kind.is_tag("textarea"))
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
                if base == "for" && !elem.kind.is_tag("label") {
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
                if (base == "rows" || base == "cols") && !elem.kind.is_tag("textarea")
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
                if base == "ordered" && !elem.kind.is_tag("list") {
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
                ) && !matches!(elem.kind.name(), "video" | "audio")
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
            if elem.kind.is_tag("input")
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
                    .find(|a| crate::vocab::base_attribute(&a.key) == "background")
                    .and_then(|a| a.value.as_deref());
                let fg_color = elem
                    .attrs
                    .iter()
                    .find(|a| crate::vocab::base_attribute(&a.key) == "color")
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
            if matches!(elem.kind.name(), "input" | "select" | "textarea") {
                let has_id = elem.attrs.iter().any(|a| a.key == "id");
                let has_aria_label = elem
                    .attrs
                    .iter()
                    .any(|a| a.key == "aria-label" || a.key == "aria-labelledby");
                let has_title = elem.attrs.iter().any(|a| a.key == "title");
                let in_label = parent_kind.is_some_and(|k| k.is_tag("label"));
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
            if elem.kind.is_tag("iframe")
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
            if elem.kind.is_tag("button") {
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
            if elem.kind.is_tag("video") {
                let has_aria = elem
                    .attrs
                    .iter()
                    .any(|a| a.key == "aria-label" || a.key == "aria-describedby");
                let has_track = elem
                    .children
                    .iter()
                    .any(|c| matches!(c, Node::Element(e) if e.kind.is_tag("source")));
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
        "uppercase" => value.to_uppercase(),
        "lowercase" => value.to_lowercase(),
        "capitalize" => {
            let mut chars = value.chars();
            match chars.next() {
                Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        }
        "trim" => value.trim().to_string(),
        "length" => value.chars().count().to_string(),
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

/// Split a function body into its content and an @style block at the
/// body's top level (the block's CSS lines, without the `@style` line).
fn split_style_block(body: Vec<Line>) -> (Vec<Line>, Vec<Line>) {
    let top = body.iter().map(|l| l.indent).min().unwrap_or(0);
    let mut content = Vec::new();
    let mut style = Vec::new();
    let mut in_style = false;
    for line in body {
        let is_style_header =
            line.indent == top && matches!(&line.content, LineContent::Normal(s) if s.trim() == "@style");
        if is_style_header {
            in_style = true;
        } else if in_style && line.indent > top {
            style.push(line);
        } else {
            in_style = false;
            content.push(line);
        }
    }
    (content, style)
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
        let r = parse("@let x = 1 + 2\n");
        assert_eq!(r.document.variables.get("x").map(|s| s.as_str()), Some("3"));
    }
}
