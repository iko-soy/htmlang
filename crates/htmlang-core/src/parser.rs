use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::ast::*;
use crate::syntax::{self, Syntax};

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
struct FnDef {
    params: Vec<String>,
    defaults: HashMap<String, String>,
    body: Vec<Syntax>,
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
    css_vars: Vec<(String, String)>,
    custom_css: Vec<String>,
    og_tags: Vec<(String, String)>,
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
    /// Canonical URL
    canonical: Option<String>,
    /// Base URL for relative links
    base_url: Option<String>,
}

/// Evaluates a syntax tree (see `syntax.rs`) into the document's nodes.
struct Evaluator;

impl ParseContext {
    /// Evaluate an expression (see `expr.rs`), reporting errors at `line`.
    fn eval(&mut self, src: &str, line: usize) -> Option<crate::expr::Value> {
        track_var_refs(src, &mut self.used_variables);
        let vars = &self.variables;
        let result = crate::expr::eval(src, &|name: &str| lookup(vars, name));
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
    let _ = Evaluator.eval_block(&syntax::parse(PRELUDE), ctx);
    // Library definitions aren't the file's own: never report them unused.
    ctx.fn_lines.clear();
    ctx.define_lines.clear();
    ctx.let_lines.clear();
}

pub fn parse(input: &str) -> ParseResult {
    parse_with_base(input, None)
}

pub fn parse_with_base(input: &str, base_path: Option<&Path>) -> ParseResult {
    let tree = syntax::parse(input);
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
        css_vars: Vec::new(),
        custom_css: Vec::new(),
        og_tags: Vec::new(),
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
        canonical: None,
        base_url: None,
    };
    load_prelude(&mut ctx);
    let nodes = Evaluator.eval_block(&tree, &mut ctx);
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
            css_vars: ctx.css_vars,
            custom_css: ctx.custom_css,
            og_tags: ctx.og_tags,
            canonical: ctx.canonical,
            base_url: ctx.base_url,
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

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

impl Evaluator {
    fn eval_block(&mut self, block: &[Syntax], ctx: &mut ParseContext) -> Vec<Node> {
        let mut nodes = Vec::new();
        for node in block {
            match self.eval(node, ctx) {
                Ok(Some(new_nodes)) => nodes.extend(new_nodes),
                Ok(None) => {}
                // Record the error and go on, to report several in one pass
                Err(e) => ctx.diagnostics.push(Diagnostic {
                    line: e.line,
                    column: None,
                    message: e.message,
                    severity: Severity::Error,
                    source_line: Some(node.source()),
                }),
            }
        }
        nodes
    }

    /// Evaluate a block in its own scope: `@let` inside doesn't leak out.
    fn eval_scoped(&mut self, block: &[Syntax], ctx: &mut ParseContext) -> Vec<Node> {
        let saved_vars = ctx.variables.clone();
        let nodes = self.eval_block(block, ctx);
        ctx.variables = saved_vars;
        nodes
    }

    fn eval(&mut self, node: &Syntax, ctx: &mut ParseContext) -> Result<Option<Vec<Node>>, ParseError> {
        let line_num = node.line();
        ctx.current_line = line_num;
        if !matches!(node, Syntax::Raw { .. })
            && let Some(filter) = old_filter_syntax(&node.source())
        {
            ctx.diagnostics.push(Diagnostic {
                line: line_num,
                column: None,
                message: format!(
                    "`${}|{}` filters are functions now: write `${{{}(${})}}` (run `htmlang upgrade`)",
                    filter.0, filter.1, filter.1, filter.0
                ),
                severity: Severity::Warning,
                source_line: Some(node.source()),
            });
        }
        let (content, current_indent, children) = match node {
            Syntax::Raw { text, .. } => return Ok(Some(vec![Node::Raw(text.clone())])),
            Syntax::Function { name, params, defaults, body, .. } => {
                self.define_function(name, params, defaults, body, line_num, ctx);
                return Ok(None);
            }
            Syntax::If { branches } => {
                // Every condition is checked (and its errors reported);
                // the first that holds picks the branch.
                let mut chosen = None;
                for branch in branches {
                    let holds = match &branch.condition {
                        Some(condition) => ctx.condition(condition, branch.line),
                        None => true,
                    };
                    if holds && chosen.is_none() {
                        chosen = Some(&branch.body);
                    }
                }
                return Ok(chosen.map(|body| self.eval_scoped(body, ctx)));
            }
            Syntax::Each { header, body, empty, .. } => {
                return self.eval_each(header, body, empty, line_num, ctx).map(Some);
            }
            Syntax::Line { text, indent, children, .. } => (text.clone(), *indent, children),
        };

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

            if !children.is_empty() {
                return Err(ParseError {
                    line: line_num,
                    message: "@let with body requires a name".to_string(),
                });
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

                // Triple quotes: @let name """...""" (syntax.rs joins the
                // lines of a multi-line string)
                let value = if let Some(after_open) = value.strip_prefix("\"\"\"") {
                    after_open.strip_suffix("\"\"\"").unwrap_or(after_open)
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


        if content == "@head" || content.starts_with("@head ") {
            let trimmed = block_text(children).trim().to_string();
            if !trimmed.is_empty() {
                ctx.head_blocks.push(trimmed);
            }
            return Ok(None);
        }

        // --- @style block (raw CSS) ---
        if content.trim() == "@style" {
            let trimmed = block_text(children).trim().to_string();
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
                let md_lines: Vec<String> = block_text(children).lines().map(String::from).collect();
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



        if let Some(rest) = content.strip_prefix("@include ") {
            let rest = rest.trim();
            if let Some((file, alias)) = rest.rsplit_once(" as ") {
                return Err(ParseError {
                    line: line_num,
                    message: format!(
                        "`@include ... as` was removed: write `@include {}` and drop the `{}.` \
                         prefix (run `htmlang upgrade`)",
                        file.trim(),
                        alias.trim()
                    ),
                });
            }
            let filename = rest
                .strip_prefix('"')
                .and_then(|f| f.strip_suffix('"'))
                .unwrap_or(rest);
            let filename = substitute_vars(filename, &ctx.variables);

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
                    let import_line = format!("@include \"{}\"", rel_name);
                    matched_nodes.extend(self.eval_block(&syntax::parse(&import_line), ctx));
                }
                return Ok(Some(matched_nodes));
            }

            let resolved = match &ctx.base_path {
                Some(base) => base.join(&filename),
                None => PathBuf::from(&filename),
            };

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
            let saved_base = ctx.base_path.clone();
            ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

            let diag_count_before = ctx.diagnostics.len();

            let included_nodes = self.eval_block(&syntax::parse(&imported_text), ctx);

            // Annotate new diagnostics with import chain
            let import_chain = format_include_chain(&ctx.include_stack);
            for d in &mut ctx.diagnostics[diag_count_before..] {
                d.message = format!("{}\n  in {}", d.message, import_chain);
            }

            ctx.base_path = saved_base;
            ctx.include_stack.pop();
            return Ok(Some(included_nodes));
        }



        // --- @data (load JSON file into variables) ---

        // @data $name file.json         values as $name.key
        // @data $name dir/*.json        a list of the files' records
        // @data $name [...] / {...}     inline JSON
        // @data $name env:NAME [DEFAULT] an environment variable
        if let Some(rest) = content.strip_prefix("@data ") {
            let rest = rest.trim();
            let Some((prefix, filename)) = rest
                .strip_prefix('$')
                .and_then(|named| named.split_once(char::is_whitespace))
                .map(|(name, source)| (name.to_string(), source.trim().to_string()))
            else {
                return Err(ParseError {
                    line: line_num,
                    message: format!(
                        "@data needs a name: write `@data $name {}` and use `$name.key`",
                        rest.trim_start_matches('$')
                    ),
                });
            };

            if let Some(env) = filename.strip_prefix("env:") {
                let (var, default) = match env.split_once(char::is_whitespace) {
                    Some((var, default)) => (var, Some(default.trim())),
                    None => (env, None),
                };
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

            // Inline JSON: @data $links [{"label": "Home", "url": "/"}]
            let (json_text, source) = if filename.starts_with(['[', '{']) {
                (filename, "the inline data".to_string())
            } else {
                let filename = substitute_vars(&filename, &ctx.variables);
                if filename.contains('*') {
                    self.load_data_glob(&prefix, &filename, line_num, &content, ctx);
                    return Ok(None);
                }
                let resolved = match &ctx.base_path {
                    Some(base) => base.join(&filename),
                    None => PathBuf::from(&filename),
                };
                match std::fs::read_to_string(&resolved) {
                    Ok(text) => {
                        ctx.included_files.push(resolved);
                        (text, format!("'{}'", filename))
                    }
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
                }
            };
            match parse_json_with_error(&json_text) {
                Ok(json) => flatten_json(&prefix, &json, &mut ctx.variables),
                Err(detail) => ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("invalid JSON in {}: {}", source, detail),
                    severity: Severity::Error,
                    source_line: Some(content.clone()),
                }),
            }
            return Ok(None);
        }

        // --- @if / @else ---

        if content.trim() == "@else" || content.trim().starts_with("@else if ") {
            return Err(ParseError {
                line: line_num,
                message: "@else without matching @if".to_string(),
            });
        }

        // --- @each loop ---

        // @raw: its indented body, or the rest of the line, verbatim
        if content == "@raw" {
            let text = block_text(children);
            let text = text.trim_end_matches('\n');
            return Ok((!text.is_empty()).then(|| vec![Node::Raw(text.to_string())]));
        }
        if let Some(rest) = content.strip_prefix("@raw ") {
            if rest.starts_with("\"\"\"") {
                return Err(ParseError {
                    line: line_num,
                    message: "`@raw \"\"\"` was removed: put the content in an indented block \
                              under `@raw` (run `htmlang upgrade`)"
                        .to_string(),
                });
            }
            return Ok(Some(vec![Node::Raw(rest.to_string())]));
        }

        // --- Function call ---

        if content.starts_with('@') {
            let name = extract_element_name(&content);
            if ctx.functions.contains_key(name) {
                ctx.used_functions.insert(name.to_string());
                let nodes = self.expand_fn_call(name, &content, children, line_num, ctx)?;
                return Ok(Some(nodes));
            }
        }

        // --- Elements ---

        if content.starts_with('@') {
            let node = self.parse_element_line(&content, children, line_num, ctx)?;
            return Ok(Some(vec![inline_svg(node, line_num, ctx)]));
        }

        // --- Bare text ---

        // `[attrs]` alone on a line used to be an anonymous @el
        if let Some(list) = content.strip_prefix('[')
            && (list.is_empty()
                || list
                    .split([',', ']'])
                    .next()
                    .and_then(|first| first.split_whitespace().next())
                    .is_some_and(crate::vocab::is_style_attribute))
        {
            ctx.diagnostics.push(Diagnostic {
                line: line_num,
                column: None,
                message: "a line starting with `[` is text: for an element, write `@el [...]` \
                          (run `htmlang upgrade`)"
                    .to_string(),
                severity: Severity::Warning,
                source_line: Some(content.clone()),
            });
        }

        let var_warnings =
            check_undefined_vars(&content, &ctx.variables, line_num, current_indent);
        ctx.diagnostics.extend(var_warnings);
        track_var_refs(&content, &mut ctx.used_variables);
        let segments = parse_text_segments(&content, ctx);
        Ok(Some(vec![Node::Text(segments)]))
    }

    /// `@data $name dir/*.json`: a list with one record per matching file
    /// (in name order), whose `file` is the file's name without extension.
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
        let mut records = Vec::new();
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
                Ok(JsonValue::Object(mut pairs)) => {
                    if !pairs.iter().any(|(key, _)| key == "file") {
                        pairs.push(("file".to_string(), JsonValue::Str(stem)));
                    }
                    records.push(JsonValue::Object(pairs));
                }
                Ok(_) => ctx.diagnostics.push(Diagnostic {
                    line: line_num,
                    column: None,
                    message: format!("'{}' should hold a JSON object", file.display()),
                    severity: Severity::Error,
                    source_line: Some(content.to_string()),
                }),
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
        flatten_json(name, &JsonValue::Array(records), &mut ctx.variables);
    }

    /// `@each $item in LIST` or `@each $item, $index in LIST`. A LIST from
    /// JSON binds each record or value to `$item`; any other list is text,
    /// split on commas, or a range `A..B [step N]`.
    fn eval_each(
        &mut self,
        header: &str,
        body: &[Syntax],
        empty: &[Syntax],
        line_num: usize,
        ctx: &mut ParseContext,
    ) -> Result<Vec<Node>, ParseError> {
        let Some((names, list_src)) = header.split_once(" in ") else {
            return Err(ParseError {
                line: line_num,
                message: "@each requires: @each $item in LIST".to_string(),
            });
        };
        let names: Vec<&str> = names.split(',').map(|v| v.trim().trim_start_matches('$')).collect();
        if names.len() > 2 {
            return Err(ParseError {
                line: line_num,
                message: "@each takes `$item` or `$item, $index`: to loop over records, \
                          load them with @data and use `$item.key`"
                    .to_string(),
            });
        }
        let (item, index) = (names[0], names.get(1).copied());
        let list_src = list_src.trim();
        if list_src.ends_with(']') && list_src.contains("[page ") {
            return Err(ParseError {
                line: line_num,
                message: "@each pagination (`[page N]`) was removed: split the list or \
                          filter it with @if"
                    .to_string(),
            });
        }
        track_var_refs(list_src, &mut ctx.used_variables);

        // A list loaded from JSON, by name
        let data_list = list_src.strip_prefix('$').and_then(|name| {
            let len = ctx.variables.get(&format!("{}#", name))?.parse::<usize>().ok()?;
            Some((name.to_string(), len))
        });
        let undefined = list_src.strip_prefix('$').filter(|name| {
            name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
                && !ctx.variables.contains_key(*name)
        });
        let text_items: Vec<String> = match (&data_list, undefined) {
            (Some(_), _) => Vec::new(),
            // A missing variable is an empty list: quietly for a record's
            // missing field (`$post.tags`), with a warning otherwise
            (None, Some(name)) => {
                let root = name.split('.').next().unwrap_or(name);
                if !ctx.variables.contains_key(root) {
                    ctx.diagnostics.push(Diagnostic {
                        line: line_num,
                        column: None,
                        message: format!("undefined variable '${}'", name),
                        severity: Severity::Warning,
                        source_line: Some(format!("@each {}", header)),
                    });
                }
                Vec::new()
            }
            (None, None) => text_list_items(&substitute_vars(list_src, &ctx.variables)),
        };
        let count = data_list.as_ref().map_or(text_items.len(), |(_, len)| *len);

        if body.is_empty() {
            return Ok(Vec::new());
        }
        if count == 0 {
            return Ok(self.eval_scoped(empty, ctx));
        }
        let saved_vars = ctx.variables.clone();
        let mut nodes = Vec::new();
        for i in 0..count {
            match &data_list {
                Some((name, _)) => bind_item(&mut ctx.variables, &format!("{}.{}", name, i), item),
                None => {
                    let text = text_items.get(i).cloned().unwrap_or_default();
                    ctx.variables.insert(item.to_string(), text);
                }
            }
            if let Some(index) = index {
                ctx.variables.insert(index.to_string(), i.to_string());
            }
            nodes.extend(self.eval_block(body, ctx));
        }
        ctx.variables = saved_vars;
        Ok(nodes)
    }

    fn define_function(
        &mut self,
        name: &str,
        params: &[String],
        defaults: &HashMap<String, String>,
        body: &[Syntax],
        line_num: usize,
        ctx: &mut ParseContext,
    ) {
        // An @style block at the top of the body is scoped to the
        // function: its rules apply inside a `.hl-NAME` wrapper.
        let (style, body): (Vec<&Syntax>, Vec<&Syntax>) = body
            .iter()
            .partition(|node| matches!(node, Syntax::Line { text, .. } if text == "@style"));
        if !style.is_empty() {
            let css: String = style
                .iter()
                .filter_map(|node| match node {
                    Syntax::Line { children, .. } => Some(block_text(children)),
                    _ => None,
                })
                .collect();
            // Nested under the scope class, so any CSS works (multi-line
            // rules, at-rules)
            if !css.trim().is_empty() {
                ctx.custom_css.push(format!(".hl-{} {{\n{}}}", name, css));
            }
            ctx.scoped_functions.insert(name.to_string());
        }

        ctx.fn_lines.entry(name.to_string()).or_insert(line_num);
        ctx.functions.insert(
            name.to_string(),
            FnDef {
                params: params.to_vec(),
                defaults: defaults.clone(),
                body: body.into_iter().cloned().collect(),
            },
        );
    }

    fn expand_fn_call(
        &mut self,
        name: &str,
        content: &str,
        children: &[Syntax],
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
        let all_caller_children = self.eval_block(children, ctx);
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

        // Evaluate the body with the parameters in scope
        let body_nodes = self.eval_block(&fn_def.body, ctx);

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
        children: &[Syntax],
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
        let children = self.eval_block(children, ctx);

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
    let (kind, rest) = if let Some(without_at) = content.strip_prefix('@') {
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

/// The directive names (without `@`) the parser recognizes, for tools such
/// as the language server.
pub fn known_directives() -> &'static [&'static str] {
    KNOWN_DIRECTIVES
}

const KNOWN_DIRECTIVES: &[&str] = &[
    "page",
    "let",
    "include",
    "raw",
    "if",
    "else",
    "each",
    "meta",
    "head",
    "style",
    "markdown",
    "data",
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
        "make the layout a function (`@let layout` with `@children`) and call it",
    ),
    (
        "@extends",
        "make the layout a function (`@let layout` with `@children` and `@slot`), \
         `@include` its file and call it",
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
    ("@debug", "removed: a layout needs no compile-time messages"),
    ("@svg", "use `@image [inline] file.svg`"),
    ("@match", "use `@if $x == a` / `@else if $x == b` / `@else`"),
    ("@case", "use `@if $x == a` / `@else if $x == b` / `@else`"),
    ("@default", "use `@else` in an `@if` chain"),
    ("@theme", "use a `@let --name value` line per token"),
    (
        "@json-ld",
        "put a `<script type=\"application/ld+json\">` in `@head`",
    ),
    ("@font-face", "write the `@font-face` rule in an `@style` block"),
    (
        "@manifest",
        "write a manifest.json file and link it from `@head`",
    ),
    ("@breakpoint", "write the media query in an `@style` block"),
    ("@deprecated", "remove it"),
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
    ("@keyframes", "write the CSS `@keyframes` rule in `@style`"),
    ("@log", "removed: a layout needs no compile-time messages"),
    ("@warn", "removed: a layout needs no compile-time messages"),
    ("@assert", "removed: a layout needs no compile-time checks"),
    (
        "@component",
        "use `@let`: an `@style` block in a function body is scoped to it",
    ),
    ("@col", "use `@el`"),
    ("@p", "use `@paragraph`"),
    ("@img", "use `@image`"),
    ("@btn", "use `@button`"),
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
        "gap-x" => Some("use `column-gap`"),
        "gap-y" => Some("use `row-gap`"),
        "shadow" => Some("use `box-shadow`"),
        "blur" => Some("use `filter blur(...)`"),
        "backdrop-blur" => Some("use `backdrop-filter blur(...)`"),
        "truncate" => Some("use the `$truncate` bundle"),
        "critical" => Some("remove it"),
        "grid" => Some("use `@grid`, or `display grid`"),
        "bold" => Some("use `font-weight bold`"),
        "italic" => Some("use `font-style italic`"),
        "underline" => Some("use `text-decoration underline`"),
        "size" => Some("use `font-size`"),
        "rounded" => Some("use `border-radius`"),
        "hidden" => Some("use `display none`"),
        "padding-x" => Some("use `padding-inline`"),
        "padding-y" => Some("use `padding-block`"),
        "margin-x" => Some("use `margin-inline`"),
        "margin-y" => Some("use `margin-block`"),
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
    for name in vars.keys().filter(|k| !k.ends_with('#')) {
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
    "padding-top",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "min-width",
    "max-width",
    "min-height",
    "max-height",
    "border-radius",
    "font-size",
    "column-gap",
    "row-gap",
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
        // A whole attribute `if(cond, a, b)` is the chosen branch's text.
        let chosen;
        let part = match choose_if(part, ctx, line_num) {
            Some(branch) => {
                chosen = branch;
                chosen.as_str()
            }
            None => part,
        };
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

        if let Some((attr, condition)) = split_trailing_if(part) {
            ctx.diagnostics.push(Diagnostic {
                line: line_num,
                column: None,
                message: format!(
                    "`KEY if CONDITION` was removed: write `if({}, {})`",
                    condition, attr
                ),
                severity: Severity::Error,
                source_line: None,
            });
            continue;
        }
        // A value `if(cond, a, b)` picks `a` or `b` (free text) by `cond`;
        // an empty choice leaves the attribute out.
        let Some(part) = choose_if_value(part, ctx, line_num) else {
            continue;
        };
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
            // A removed attribute that shares its name with a CSS property
            // (bare `grid`) still gets its hint.
            let removed = removed_attribute_hint(base_key)
                .filter(|_| attr.value.is_none() || !crate::vocab::is_css_property(base_key));
            if let Some(hint) = removed {
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
            } else if is_boolean_html || crate::vocab::is_style_attribute(base_key) {
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
/// The items of a text list: a range `A..B [step N]`, or comma-separated.
fn text_list_items(list: &str) -> Vec<String> {
    if let Some((start, rest)) = list.split_once("..") {
        let (end, step) = match rest.split_once(" step ") {
            Some((end, step)) => (end.trim(), step.trim().parse::<i64>().unwrap_or(1).max(1)),
            None => (rest.trim(), 1),
        };
        if let (Ok(start), Ok(end)) = (start.trim().parse::<i64>(), end.parse::<i64>()) {
            return numeric_range(start, end, step);
        }
    }
    list.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Make `$target` (and `$target.key`, and nested lists) a copy of the list
/// item stored under `source`.
fn bind_item(vars: &mut HashMap<String, String>, source: &str, target: &str) {
    let under = |key: &str, name: &str| {
        key.strip_prefix(name)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '#']))
    };
    vars.retain(|key, _| !under(key, target));
    let copies: Vec<(String, String)> = vars
        .iter()
        .filter(|(key, _)| under(key, source))
        .map(|(key, value)| (format!("{}{}", target, &key[source.len()..]), value.clone()))
        .collect();
    vars.extend(copies);
    // A record has no text of its own
    vars.entry(target.to_string()).or_default();
}

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



/// Find a leftover `key if condition` (ignoring ` if ` inside parentheses
/// or quotes), to point at `if()`.
fn split_trailing_if(part: &str) -> Option<(&str, &str)> {
    let mut depth = 0;
    let mut quote = None;
    for (i, c) in part.char_indices() {
        match c {
            '"' | '\'' if quote == Some(c) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(c),
            _ if quote.is_some() => {}
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0 && part[i..].starts_with(" if ") => {
                return Some((&part[..i], part[i + 4..].trim()));
            }
            _ => {}
        }
    }
    None
}

/// Evaluate `if(cond, a)` or `if(cond, a, b)` when it is all of `text`,
/// returning the chosen branch (empty when `cond` fails and there is no `b`).
fn choose_if(text: &str, ctx: &mut ParseContext, line: usize) -> Option<String> {
    let inner = text.strip_prefix("if(")?.strip_suffix(')')?;
    let args = split_if_args(inner);
    // `if(a) (b)` isn't one call
    if split_trailing_paren(inner) || !(2..=3).contains(&args.len()) {
        return None;
    }
    let branch = if ctx.condition(args[0].trim(), line) {
        args[1]
    } else {
        args.get(2).copied().unwrap_or("")
    };
    Some(branch.trim().to_string())
}

/// Whether the parentheses in `inner` close before its end, as in the
/// inside of `if(a)(b)`.
fn split_trailing_paren(inner: &str) -> bool {
    let mut depth = 0i32;
    let mut quote = None;
    for c in inner.chars() {
        match c {
            '"' | '\'' if quote == Some(c) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(c),
            _ if quote.is_some() => {}
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// Resolve a value written `if(cond, a, b)`: evaluate `cond` and keep the
/// chosen branch's text, or `None` when the choice is empty. Other
/// attributes are returned unchanged.
fn choose_if_value(part: &str, ctx: &mut ParseContext, line: usize) -> Option<String> {
    let Some(pos) = part.find(['=', ' ']) else {
        return Some(part.to_string());
    };
    let (head, value) = part.split_at(pos + 1);
    match choose_if(value.trim(), ctx, line) {
        Some(branch) if branch.is_empty() => None,
        Some(branch) => Some(format!("{}{}", head, branch)),
        None => Some(part.to_string()),
    }
}

fn split_if_args(input: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    let mut quote = None;
    for (i, c) in input.char_indices() {
        match c {
            '"' | '\'' if quote == Some(c) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(c),
            _ if quote.is_some() => {}
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

/// Attributes that only make sense on container elements (@row, @el, @el).
const CONTAINER_ONLY_ATTRS: &[&str] = &[
    "spacing",
    "gap",
    "wrap",
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
    matches!(kind, ElementKind::Row | ElementKind::El)
        || kind.spec().is_some_and(|spec| spec.container)
}

/// Stricter checks for `htmlang lint`, on top of the diagnostics every
/// compile reports: deep nesting, empty layout containers, and buttons
/// without an explicit `type`.
pub fn lint(nodes: &[Node]) -> Vec<Diagnostic> {
    fn walk(nodes: &[Node], depth: usize, out: &mut Vec<Diagnostic>) {
        for node in nodes {
            let Node::Element(elem) = node else { continue };
            let mut warn = |message: String| {
                out.push(Diagnostic {
                    line: elem.line_num,
                    column: None,
                    message,
                    severity: Severity::Warning,
                    source_line: None,
                })
            };
            if depth > 10 {
                warn(format!(
                    "deeply nested element ({} levels): consider simplifying",
                    depth
                ));
            }
            if matches!(elem.kind, ElementKind::Row | ElementKind::El)
                && elem.children.is_empty()
            {
                warn(format!("empty container (@{}) has no children", elem.kind.name()));
            }
            if elem.kind.is_tag("button") && !elem.attrs.iter().any(|a| a.key == "type") {
                warn("@button missing 'type' attribute (defaults to submit)".to_string());
            }
            walk(&elem.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(nodes, 0, &mut out);
    out
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
                    && !parent_kind.is_some_and(ElementKind::is_column)
                {
                    diagnostics.push(Diagnostic {
                        line: elem.line_num,
                        column: None,
                        message: "'height fill' works best inside @el; using 100% as fallback"
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
                            "'{}' has no effect on {} (only works on @row, @el, @el)",
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
                    && let (Some(bg_rgb), Some(fg_rgb)) = (crate::expr::parse_hex_rgb(bg), crate::expr::parse_hex_rgb(fg))
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

// ---------------------------------------------------------------------------
// Variable substitution
// ---------------------------------------------------------------------------

/// Interpolate `$name` variables and `${expr}` expressions into text.
/// Undefined variables, and expressions that don't evaluate, are left as
/// written so they show up in the output.
fn substitute_vars(input: &str, vars: &HashMap<String, String>) -> String {
    if !input.contains('$') {
        return input.to_string();
    }
    let is_name_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find('$') {
        result.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if after.starts_with('{')
            && let Some(close) = matching_brace(after)
        {
            let source = &after[1..close];
            match crate::expr::eval(source, &|name: &str| lookup(vars, name)) {
                Ok(value) => result.push_str(&value.to_string()),
                Err(_) => result.push_str(&rest[pos..pos + 1 + close + 1]),
            }
            rest = &after[close + 1..];
            continue;
        }
        let mut end = after.find(|c: char| !is_name_char(c)).unwrap_or(after.len());
        while end > 0 && after[..end].ends_with('.') {
            end -= 1;
        }
        let name = &after[..end];
        match vars.get(name) {
            Some(value) if !name.is_empty() => result.push_str(value),
            _ => {
                result.push('$');
                result.push_str(name);
            }
        }
        rest = &after[end..];
    }
    result.push_str(rest);
    result
}

/// `@image [inline] icon.svg` becomes the SVG's markup, with `width`,
/// `height`, `color` / `fill`, `class=` and `id=` applied to the `<svg>`
/// tag. Other nodes are returned unchanged.
fn inline_svg(node: Node, line_num: usize, ctx: &mut ParseContext) -> Node {
    let Node::Element(elem) = &node else {
        return node;
    };
    let is_inline_svg = elem.kind == ElementKind::Image
        && elem.attrs.iter().any(|a| a.key == "inline" && !a.html)
        && elem.argument.as_deref().is_some_and(|src| src.ends_with(".svg"));
    if !is_inline_svg {
        return node;
    }
    let filename = elem.argument.clone().unwrap_or_default();
    let resolved = match &ctx.base_path {
        Some(base) => base.join(&filename),
        None => PathBuf::from(&filename),
    };
    let mut svg = match std::fs::read_to_string(&resolved) {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            ctx.diagnostics.push(Diagnostic {
                line: line_num,
                column: None,
                message: format!("cannot load SVG '{}': {}", filename, e),
                severity: Severity::Error,
                source_line: None,
            });
            return node;
        }
    };
    ctx.included_files.push(resolved);
    for attr in &elem.attrs {
        let Some(value) = &attr.value else { continue };
        let target = match (attr.key.as_str(), attr.html) {
            ("width", false) | ("height", false) => attr.key.as_str(),
            ("color", false) | ("fill", false) => "fill",
            ("class", true) | ("id", true) => attr.key.as_str(),
            _ => continue,
        };
        svg = set_svg_attr(&svg, target, value);
    }
    Node::Raw(svg)
}

/// The first `$name|filter` (old filter syntax) in `text`, as (name, filter).
fn old_filter_syntax(text: &str) -> Option<(String, String)> {
    const FILTERS: &[&str] = &[
        "uppercase", "lowercase", "capitalize", "trim", "length", "reverse", "truncate",
        "replace", "default", "lighten", "darken", "alpha", "mix",
    ];
    let mut rest = text;
    while let Some(pos) = rest.find('$') {
        let after = &rest[pos + 1..];
        let end = after
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.')))
            .unwrap_or(after.len());
        if let Some(filter) = after[end..].strip_prefix('|') {
            let name_end = filter.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(filter.len());
            if end > 0 && FILTERS.contains(&&filter[..name_end]) {
                return Some((after[..end].to_string(), filter[..name_end].to_string()));
            }
        }
        rest = &after[end..];
    }
    None
}

/// Index of the `}` matching the `{` that `s` starts with (skipping
/// braces inside string literals).
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_string = !in_string,
            _ if in_string => {}
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
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

/// Visit the lines of a block in source order, children after their line.
fn for_each_line<'a>(block: &'a [Syntax], f: &mut impl FnMut(&'a Syntax)) {
    for node in block {
        f(node);
        if let Syntax::Line { children, .. } = node {
            for_each_line(children, f);
        }
    }
}

/// The text of a block whose lines are content, not htmlang (`@head`,
/// `@style`, `@markdown`): each line trimmed, verbatim text as is.
fn block_text(block: &[Syntax]) -> String {
    let mut text = String::new();
    for_each_line(block, &mut |node| match node {
        Syntax::Line { text: line, .. } => {
            text.push_str(line.trim());
            text.push('\n');
        }
        Syntax::Raw { text: raw, .. } => {
            text.push_str(raw);
            text.push('\n');
        }
        _ => {}
    });
    text
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
            // A list: its length under `NAME#` (see `lookup`), its text
            // items joined as its value, and each item as `NAME.INDEX`
            vars.insert(format!("{}#", prefix), items.len().to_string());
            let text: Vec<String> = items
                .iter()
                .map(json_value_to_string)
                .filter(|s| !s.is_empty())
                .collect();
            vars.insert(prefix.to_string(), text.join(", "));
            // Also set indexed access: prefix.0, prefix.1, etc.
            for (i, item) in items.iter().enumerate() {
                flatten_json(&format!("{}.{}", prefix, i), item, vars);
            }
        }
    }
}

/// Resolve `$name` for an expression: a list if `name` was loaded from a
/// JSON array, else its text.
fn lookup(vars: &HashMap<String, String>, name: &str) -> Option<crate::expr::Value> {
    use crate::expr::Value;
    match vars.get(&format!("{}#", name)).and_then(|n| n.parse::<usize>().ok()) {
        Some(len) => Some(Value::List(
            (0..len)
                .map(|i| vars.get(&format!("{}.{}", name, i)).cloned().unwrap_or_default())
                .collect(),
        )),
        None => vars.get(name).cloned().map(Value::Str),
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
