use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::ast::*;
use crate::diagnostic::code;
pub use crate::diagnostic::{Diagnostic, Severity};
use crate::interp;
use crate::syntax::{self, DirectiveArgs, LetForm, NodeKind, Segment, Tree};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

pub struct ParseResult {
    pub document: Document,
    pub diagnostics: Vec<Diagnostic>,
    pub included_files: Vec<PathBuf>,
}

/// An error that stops one line from being evaluated.
type ParseError = Diagnostic;

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct FnDef {
    params: Vec<String>,
    defaults: HashMap<String, String>,
    body: Vec<syntax::Node>,
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
    /// Syntax nodes evaluated at least once, by id.
    visited: HashSet<usize>,
    /// The id the next parsed tree starts at, so ids are unique across
    /// the file, its includes and the standard library.
    next_id: usize,
    /// The file's tree and those of the files it includes, with the
    /// include chain that leads to each, for the checks of code that never
    /// runs.
    trees: Vec<(Rc<Tree>, Option<String>)>,
    /// Every function defined anywhere in the file or the files it
    /// includes, whether or not its definition runs.
    namespace: HashSet<String>,
    /// The line being evaluated, and its text as written when it is one
    /// physical line, for diagnostics that point at a column.
    current_source: (usize, Option<String>),
    /// Variable diagnostics already reported, so a line evaluated many
    /// times (in a loop, in a function) reports each problem once.
    /// Keyed by the syntax node (ids are unique across files), line,
    /// column and message.
    reported: HashSet<(usize, usize, Option<usize>, String)>,
    /// The id of the syntax node being evaluated.
    current_node: usize,
}

/// The variables in scope, for interpolation and expressions.
struct Vars<'a>(&'a HashMap<String, String>);

impl interp::Scope for Vars<'_> {
    fn defined(&self, path: &str) -> bool {
        self.0.contains_key(path) || self.0.contains_key(&format!("{}#", path))
    }

    fn value(&self, path: &str) -> Option<crate::expr::Value> {
        lookup(self.0, path)
    }

    fn has_fields(&self, path: &str) -> bool {
        self.0.contains_key(&format!("{}#", path))
            || self.0.keys().any(|key| {
                key.strip_prefix(path)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
    }
}

/// Evaluates a syntax tree (see `syntax.rs`) into the document's nodes.
struct Evaluator;

impl ParseContext {
    /// Evaluate an expression (see `expr.rs`) written at `line` and
    /// `column` (when known), reporting its errors there.
    fn eval(
        &mut self,
        src: &str,
        line: usize,
        column: Option<usize>,
    ) -> Option<crate::expr::Value> {
        track_var_refs(src, &mut self.used_variables);
        match crate::expr::eval(src, &Vars(&self.variables)) {
            Ok(value) => Some(value),
            Err(error) => {
                // Without the line to point into, show the expression
                let shown = matches!(&self.current_source, (current, Some(_)) if *current == line);
                let before = self.diagnostics.len();
                self.report(error.at(0), line, column);
                if !shown && let Some(diagnostic) = self.diagnostics.get_mut(before) {
                    diagnostic.source_line = Some(src.into());
                }
                None
            }
        }
    }

    /// Evaluate a condition; an invalid one is reported and counts as false.
    fn condition(&mut self, src: &str, line: usize, column: Option<usize>) -> bool {
        self.eval(src, line, column)
            .is_some_and(|value| value.truthy())
    }

    /// Fill in the variables of one slot's text (see `interp.rs`), written
    /// at `line` and `column` (when known). What can't be filled is left as
    /// written and reported.
    fn interpolate(&mut self, text: &str, line: usize, column: Option<usize>) -> String {
        self.fill(text, line, column).0
    }

    /// [`interpolate`](Self::interpolate), and whether everything was
    /// filled in.
    fn fill(&mut self, text: &str, line: usize, column: Option<usize>) -> (String, bool) {
        self.fill_mapped(text, line, column, |offset| offset)
    }

    /// Fill in a slot of text, where `\$`, `\{` and the other escapes
    /// stay literal.
    fn interpolate_text(&mut self, raw: &str, line: usize, column: Option<usize>) -> String {
        let protected = protect_escapes(raw);
        let (filled, _) = self.fill_mapped(&protected, line, column, |offset| {
            // An offset into the protected text, as one into `raw`
            protected[..offset]
                .chars()
                .map(|c| match ESCAPES.iter().find(|(_, p, _)| *p == c) {
                    Some((escape, _, _)) => escape.len(),
                    None => c.len_utf8(),
                })
                .sum()
        });
        restore_escapes(&filled)
    }

    /// [`fill`](Self::fill), where `source_offset` turns an offset into
    /// `text` into one into the text as written.
    fn fill_mapped(
        &mut self,
        text: &str,
        line: usize,
        column: Option<usize>,
        source_offset: impl Fn(usize) -> usize,
    ) -> (String, bool) {
        if !text.contains('$') {
            return (text.to_string(), true);
        }
        track_var_refs(text, &mut self.used_variables);
        let (out, problems) = interp::interpolate(text, &Vars(&self.variables));
        let filled = problems.is_empty();
        for mut problem in problems {
            match &mut problem {
                interp::Problem::Undefined { offset, .. }
                | interp::Problem::Invalid { offset, .. } => *offset = source_offset(*offset),
            }
            self.report(problem, line, column);
        }
        (out, filled)
    }

    /// Report a variable that can't be filled in, or an invalid `${...}`,
    /// at `offset` from `column`.
    fn report(&mut self, problem: interp::Problem, line: usize, column: Option<usize>) {
        let (line_text, column) = match &self.current_source {
            // The column is only shown with the line it points into
            (current, Some(text)) if *current == line => (Some(text.clone()), column),
            _ => (None, None),
        };
        let diagnostic = match problem {
            interp::Problem::Undefined { name, offset } => {
                let column = column.map(|c| c + offset);
                let mut diagnostic = self.undefined(&name, line);
                if let Some(column) = column {
                    diagnostic = diagnostic.column(column);
                }
                diagnostic
            }
            interp::Problem::Invalid { message, offset } => {
                let diagnostic = Diagnostic::error(
                    code::INVALID_EXPRESSION,
                    line,
                    format!("invalid expression: {}", message),
                );
                match column {
                    Some(column) => diagnostic.column(column + offset),
                    None => diagnostic,
                }
            }
        };
        let diagnostic = match line_text {
            Some(text) => diagnostic.source(text),
            None => diagnostic,
        };
        let key = (
            self.current_node,
            diagnostic.line,
            diagnostic.column,
            diagnostic.message.clone(),
        );
        if self.reported.insert(key) {
            self.diagnostics.push(diagnostic);
        }
    }

    /// The error for `$name` with no definition: what the name is if it is
    /// something else, or the closest defined name.
    fn undefined(&mut self, name: &str, line: usize) -> Diagnostic {
        if self.defines.contains_key(name) {
            // Reported here, so not also as unused
            self.used_defines.insert(name.to_string());
            return Diagnostic::error(
                code::UNDEFINED_VARIABLE,
                line,
                format!(
                    "'${}' is an attribute bundle, not a value: it goes in an attribute list \
                     as a whole attribute, `[${}]`",
                    name, name
                ),
            )
            .subject(name);
        }
        let suggestion = suggest_var_name(name, &self.variables);
        let message = match &suggestion {
            Some(closest) => format!(
                "undefined variable '${}', did you mean '${}'?",
                name, closest
            ),
            None => format!("undefined variable '${}'", name),
        };
        Diagnostic::error(code::UNDEFINED_VARIABLE, line, message)
            .subject(name)
            .suggest(suggestion)
    }

    /// Note the node being evaluated, for diagnostics.
    fn enter(&mut self, node: &syntax::Node) {
        self.current_line = node.span.line;
        self.current_node = node.id;
        let text =
            (node.line_count <= 1).then(|| format!("{}{}", " ".repeat(node.indent), node.source));
        self.current_source = (node.span.line, text);
    }

    /// Parse a file's text into a tree whose ids don't clash with the
    /// trees parsed before it.
    fn parse_tree(&mut self, text: &str) -> Rc<Tree> {
        let tree = syntax::parse_from(text, self.next_id);
        self.next_id = tree.end_id;
        Rc::new(tree)
    }

    /// Read a file (once), relative to the current file.
    fn read_file(&mut self, resolved: &Path) -> std::io::Result<String> {
        if let Some(cached) = self.file_cache.get(resolved) {
            return Ok(cached.clone());
        }
        let text = std::fs::read_to_string(resolved)?;
        self.file_cache.insert(resolved.to_path_buf(), text.clone());
        Ok(text)
    }

    fn resolve(&self, filename: &str) -> PathBuf {
        match &self.base_path {
            Some(base) => base.join(filename),
            None => PathBuf::from(filename),
        }
    }
}

/// The standard library (`std.hl`): components and bundles defined in
/// htmlang itself and available in every file.
const PRELUDE: &str = include_str!("std.hl");

fn load_prelude(ctx: &mut ParseContext) {
    let tree = ctx.parse_tree(PRELUDE);
    collect_namespace(&tree.nodes, None, &mut HashSet::new(), ctx);
    let _ = Evaluator.eval_block(&tree.nodes, ctx);
    // Library definitions aren't the file's own: never report them unused.
    ctx.fn_lines.clear();
    ctx.define_lines.clear();
    ctx.let_lines.clear();
}

pub fn parse(input: &str) -> ParseResult {
    parse_with_base(input, None)
}

pub fn parse_with_base(input: &str, base_path: Option<&Path>) -> ParseResult {
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
        visited: HashSet::new(),
        next_id: 0,
        trees: Vec::new(),
        namespace: HashSet::new(),
        current_source: (0, None),
        reported: HashSet::new(),
        current_node: 0,
    };
    load_prelude(&mut ctx);
    let tree = ctx.parse_tree(input);
    ctx.diagnostics.extend(tree.diagnostics.iter().cloned());
    ctx.trees.push((tree.clone(), None));
    collect_namespace(
        &tree.nodes,
        base_path.map(Path::to_path_buf),
        &mut HashSet::new(),
        &mut ctx,
    );
    let nodes = Evaluator.eval_block(&tree.nodes, &mut ctx);
    check_unevaluated(&mut ctx);
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
            nodes,
        },
        diagnostics: ctx.diagnostics,
        included_files: ctx.included_files,
    }
}

// ---------------------------------------------------------------------------
// Code that never runs: an untaken branch, an uncalled function, a loop over
// an empty list
// ---------------------------------------------------------------------------

/// Collect the functions defined anywhere in `nodes` and in the files they
/// include (by a literal path), into `ctx.namespace`.
fn collect_namespace(
    nodes: &[syntax::Node],
    base: Option<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    ctx: &mut ParseContext,
) {
    let mut includes = Vec::new();
    for node in nodes {
        node.walk(&mut |node| match node.directive().map(|d| &d.args) {
            Some(DirectiveArgs::Let(def)) if matches!(def.form, LetForm::Function(_)) => {
                ctx.namespace.insert(def.name.clone());
            }
            Some(DirectiveArgs::Text(Some(path))) if node.is_directive("include") => {
                includes.push(path.raw.clone());
            }
            _ => {}
        });
    }
    for path in includes {
        let path = path.trim_matches('"');
        if path.contains('$') {
            continue;
        }
        let resolved = match &base {
            Some(base) => base.join(path),
            None => PathBuf::from(path),
        };
        if !seen.insert(resolved.clone()) {
            continue;
        }
        let Ok(text) = ctx.read_file(&resolved) else {
            continue;
        };
        let tree = syntax::parse(&text);
        let parent = resolved.parent().map(Path::to_path_buf);
        collect_namespace(&tree.nodes, parent, seen, ctx);
    }
}

/// Check the lines that were never evaluated: element and function names,
/// and the attributes of built-in elements (those that don't depend on a
/// variable). Names that come from data are only checked when evaluated.
fn check_unevaluated(ctx: &mut ParseContext) {
    let trees = std::mem::take(&mut ctx.trees);
    let mut seen = HashSet::new();
    for (tree, chain) in &trees {
        let before = ctx.diagnostics.len();
        for node in &tree.nodes {
            check_unevaluated_node(node, ctx);
        }
        let mut found = ctx.diagnostics.split_off(before);
        if let Some(chain) = chain {
            for d in &mut found {
                d.message = format!("{}\n  in {}", d.message, chain);
            }
        }
        // A file included twice is checked twice: report each problem once
        found.retain(|d| seen.insert((d.line, d.message.clone())));
        ctx.diagnostics.extend(found);
    }
    ctx.trees = trees;
}

fn check_unevaluated_node(node: &syntax::Node, ctx: &mut ParseContext) {
    if !ctx.visited.contains(&node.id) {
        let line = node.span.line;
        // A reference from code that doesn't run still counts as a use
        track_var_refs(&node.source, &mut ctx.used_variables);
        let before = ctx.diagnostics.len();
        match &node.kind {
            NodeKind::Element(element) => {
                for head in &element.chain {
                    check_head(head, line, false, ctx);
                }
                if let Some(text) = &element.text {
                    check_inline_heads(text, line, ctx);
                }
            }
            NodeKind::Text(text) => check_inline_heads(text, line, ctx),
            _ => {}
        }
        for d in &mut ctx.diagnostics[before..] {
            d.source_line
                .get_or_insert_with(|| node.source.as_str().into());
        }
    }
    for child in &node.children {
        check_unevaluated_node(child, ctx);
    }
}

/// Check a head that was never evaluated: its name, and for a built-in
/// element, its attributes.
fn check_head(head: &syntax::Head, line: usize, inline: bool, ctx: &mut ParseContext) {
    let is_function = ctx.namespace.contains(&head.name) || ctx.functions.contains_key(&head.name);
    if is_function && !inline {
        ctx.used_functions.insert(head.name.clone());
        return;
    }
    if let Err(mut e) = parse_element_kind(&head.name, line, ctx) {
        if inline {
            e.severity = Severity::Warning;
        }
        ctx.diagnostics.push(e);
        return;
    }
    if let Some(list) = &head.attrs {
        // Names are checked; a value with a variable in it depends on
        // what the variable holds, so only a literal value is
        let literal: Vec<syntax::Attr> = list
            .attrs
            .iter()
            .filter(|a| {
                let bundle = a.key.starts_with('$') && a.value.is_none() && !a.html;
                !a.raw.starts_with("if(") && !bundle
            })
            .map(|a| {
                let mut a = a.clone();
                let variable = |v: &String| v.contains('$') || v.contains("if(");
                if !a.key.contains('$') && a.value.as_ref().is_some_and(variable) {
                    a.value = None;
                    a.raw = a.key.clone();
                }
                a
            })
            .collect();
        parse_attr_list(&literal, line, ctx, true);
    }
}

fn check_inline_heads(text: &syntax::Text, line: usize, ctx: &mut ParseContext) {
    for segment in &text.segments {
        if let Segment::Inline(inline) = segment {
            check_head(&inline.head, line, true, ctx);
            if let Some(text) = &inline.text {
                check_inline_heads(text, line, ctx);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// The `@else` lines that continue the `@if` or `@each` before `from`, and
/// the position after them.
fn else_branches(block: &[syntax::Node], from: usize, is_if: bool) -> (Vec<&syntax::Node>, usize) {
    let mut branches = Vec::new();
    let mut next = from;
    let mut i = from;
    while i < block.len() {
        let node = &block[i];
        i += 1;
        if node.is_trivia() {
            continue;
        }
        let condition = match node.directive().map(|d| &d.args) {
            Some(DirectiveArgs::Else { condition }) => condition.is_some(),
            _ => break,
        };
        if condition && !is_if {
            break;
        }
        branches.push(node);
        next = i;
        if !condition {
            break;
        }
    }
    (branches, next)
}

impl Evaluator {
    fn eval_block(&mut self, block: &[syntax::Node], ctx: &mut ParseContext) -> Vec<Node> {
        let mut nodes = Vec::new();
        let mut i = 0;
        while i < block.len() {
            let node = &block[i];
            i += 1;
            if node.is_trivia() {
                continue;
            }
            let result = match node.directive().map(|d| d.name()) {
                Some(name @ ("if" | "each")) => {
                    let (branches, next) = else_branches(block, i, name == "if");
                    i = next;
                    if name == "if" {
                        Ok(Some(self.eval_if(node, &branches, ctx)))
                    } else {
                        self.eval_each(node, branches.first().copied(), ctx)
                    }
                }
                // Reported as a syntax error
                Some("else") => {
                    ctx.visited.insert(node.id);
                    Ok(None)
                }
                _ => self.eval(node, ctx),
            };
            match result {
                Ok(Some(new_nodes)) => nodes.extend(new_nodes),
                Ok(None) => {}
                // Record the error and go on, to report several in one pass
                Err(mut e) => {
                    e.source_line
                        .get_or_insert_with(|| node.source.as_str().into());
                    ctx.diagnostics.push(e);
                }
            }
        }
        nodes
    }

    /// Evaluate a block in its own scope: `@let` inside doesn't leak out.
    fn eval_scoped(&mut self, block: &[syntax::Node], ctx: &mut ParseContext) -> Vec<Node> {
        let saved_vars = ctx.variables.clone();
        let nodes = self.eval_block(block, ctx);
        ctx.variables = saved_vars;
        nodes
    }

    /// `@if` with its `@else if` / `@else` branches. Every condition is
    /// checked (and its errors reported); the first that holds picks the
    /// branch.
    fn eval_if(
        &mut self,
        node: &syntax::Node,
        branches: &[&syntax::Node],
        ctx: &mut ParseContext,
    ) -> Vec<Node> {
        let mut chosen = None;
        for branch in std::iter::once(node).chain(branches.iter().copied()) {
            ctx.visited.insert(branch.id);
            ctx.enter(branch);
            let holds = match branch.directive().map(|d| &d.args) {
                Some(DirectiveArgs::Condition(condition))
                | Some(DirectiveArgs::Else {
                    condition: Some(condition),
                }) => {
                    !condition.raw.is_empty()
                        && ctx.condition(
                            &condition.raw,
                            branch.span.line,
                            Some(condition.span.column),
                        )
                }
                Some(DirectiveArgs::Else { condition: None }) => true,
                // A syntax error, already reported
                _ => false,
            };
            if holds && chosen.is_none() {
                chosen = Some(&branch.children);
            }
        }
        chosen.map_or_else(Vec::new, |body| self.eval_scoped(body, ctx))
    }

    fn eval(
        &mut self,
        node: &syntax::Node,
        ctx: &mut ParseContext,
    ) -> Result<Option<Vec<Node>>, ParseError> {
        ctx.visited.insert(node.id);
        ctx.enter(node);
        match &node.kind {
            NodeKind::Blank | NodeKind::Comment => Ok(None),
            NodeKind::Verbatim(body) => Ok(Some(vec![Node::Raw(body.text.clone())])),
            NodeKind::Directive(directive) => {
                let mut nodes = self.eval_directive(node, directive, ctx)?;
                // A directive that takes no body: the lines indented under
                // it (a syntax error) are evaluated as its siblings
                let takes_body = match &directive.args {
                    DirectiveArgs::Let(def) => matches!(def.form, LetForm::Function(_)),
                    _ => directive.spec.body != BodyKind::None,
                };
                if !takes_body {
                    let siblings = self.eval_block(&node.children, ctx);
                    nodes.get_or_insert_with(Vec::new).extend(siblings);
                }
                Ok(nodes)
            }
            NodeKind::Element(element) => self.eval_element(node, element, ctx).map(Some),
            NodeKind::Text(text) => {
                let mut nodes = vec![Node::Text(text_segments(text, ctx))];
                // Text takes no body: lines indented under it are its
                // siblings
                nodes.extend(self.eval_block(&node.children, ctx));
                Ok(Some(nodes))
            }
        }
    }

    fn eval_directive(
        &mut self,
        node: &syntax::Node,
        directive: &syntax::Directive,
        ctx: &mut ParseContext,
    ) -> Result<Option<Vec<Node>>, ParseError> {
        let line_num = node.span.line;
        let content = &node.source;
        match (directive.name(), &directive.args) {
            // @page [lang en, favicon /f.png] Title
            ("page", DirectiveArgs::Page { attrs, title }) => {
                if let Some(list) = attrs {
                    for attr in parse_attr_list(&list.attrs, line_num, ctx, false) {
                        let value = attr.value.unwrap_or_default();
                        match attr.key.as_str() {
                            "lang" => ctx.lang = Some(value),
                            "favicon" => ctx.favicon = Some(value),
                            other => ctx.diagnostics.push(
                                Diagnostic::warning(
                                    code::UNKNOWN_PAGE_ATTRIBUTE,
                                    line_num,
                                    format!(
                                        "unknown @page attribute '{}' (expected lang or favicon)",
                                        other
                                    ),
                                )
                                .source(content.clone())
                                .subject(other),
                            ),
                        }
                    }
                }
                let title = match title {
                    Some(title) => {
                        ctx.interpolate(&title.raw, title.span.line, Some(title.span.column))
                    }
                    None => String::new(),
                };
                ctx.page_title = Some(title);
                Ok(None)
            }

            ("let", DirectiveArgs::Let(def)) => {
                let name = def.name.as_str();
                match &def.form {
                    LetForm::Function(params) => {
                        self.define_function(name, params, &node.children, line_num, ctx);
                    }
                    // `@let name = EXPR` computes its value (see expr.rs)
                    LetForm::Computed(expression) => {
                        let value = ctx
                            .eval(&expression.raw, line_num, Some(expression.span.column))
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        set_variable(name, value, line_num, ctx);
                    }
                    // Attribute bundle: @let name [attr1, attr2, ...]
                    LetForm::Bundle(list) => {
                        let attrs = parse_attr_list(&list.attrs, line_num, ctx, true);
                        ctx.defines.insert(name.to_string(), attrs);
                        ctx.define_lines.entry(name.to_string()).or_insert(line_num);
                    }
                    // Literal text with `$var` interpolation, quoted or not:
                    // @let greeting "Hello $name"
                    LetForm::Value(Some(value)) | LetForm::Quoted(value) => {
                        let value =
                            ctx.interpolate(&value.raw, value.span.line, Some(value.span.column));
                        set_variable(name, value, line_num, ctx);
                    }
                    LetForm::Value(None) => {}
                }
                Ok(None)
            }

            // @meta NAME VALUE; `og:` names become Open Graph property tags
            ("meta", DirectiveArgs::Text(Some(arg))) => {
                let Some((name, value)) = arg.raw.split_once(char::is_whitespace) else {
                    ctx.diagnostics.push(
                        Diagnostic::error(
                            code::MISSING_ARGUMENT,
                            line_num,
                            format!("@meta needs a name and a value: `@meta {} VALUE`", arg.raw),
                        )
                        .source(content.clone()),
                    );
                    return Ok(None);
                };
                let value_column = arg.span.column + arg.raw.len() - value.trim_start().len();
                let value = ctx.interpolate(value.trim(), line_num, Some(value_column));
                match name.trim().strip_prefix("og:") {
                    Some(property) => ctx.og_tags.push((property.to_string(), value)),
                    None => ctx.meta_tags.push((name.trim().to_string(), value)),
                }
                Ok(None)
            }

            ("head", _) => {
                let text = verbatim_text(&node.children);
                if !text.trim().is_empty() {
                    ctx.head_blocks.push(text.trim().to_string());
                }
                Ok(None)
            }

            // @style: raw CSS
            ("style", _) => {
                let text = verbatim_text(&node.children);
                if !text.trim().is_empty() {
                    ctx.custom_css.push(text.trim().to_string());
                }
                Ok(None)
            }

            // @markdown: an indented block, or a file
            ("markdown", DirectiveArgs::Text(file)) => {
                let Some(file) = file else {
                    let md_lines: Vec<String> = verbatim_text(&node.children)
                        .lines()
                        .map(String::from)
                        .collect();
                    return Ok(Some(vec![Node::Raw(markdown_to_html(&md_lines))]));
                };
                let filename = ctx.interpolate(&file.raw, line_num, Some(file.span.column));
                let resolved = ctx.resolve(&filename);
                let md_text = match ctx.read_file(&resolved) {
                    Ok(text) => text,
                    Err(e) => {
                        ctx.diagnostics.push(
                            Diagnostic::error(
                                code::UNREADABLE_FILE,
                                line_num,
                                format!("cannot read markdown '{}': {}", filename, e),
                            )
                            .source(content.clone()),
                        );
                        return Ok(None);
                    }
                };
                ctx.included_files.push(resolved);
                let md_lines: Vec<String> = md_text.lines().map(|l| l.to_string()).collect();
                Ok(Some(vec![Node::Raw(markdown_to_html(&md_lines))]))
            }

            ("include", DirectiveArgs::Text(Some(path))) => {
                self.eval_include(path, line_num, content, ctx)
            }

            ("data", DirectiveArgs::Data { name, source, .. }) => {
                self.eval_data(name, source, line_num, content, ctx);
                Ok(None)
            }

            // @raw: the rest of its line, or its indented body, verbatim
            ("raw", DirectiveArgs::Text(Some(text))) => Ok(Some(vec![Node::Raw(text.raw.clone())])),
            ("raw", _) => {
                let text = verbatim_text(&node.children);
                let text = text.trim_end_matches('\n');
                Ok((!text.is_empty()).then(|| vec![Node::Raw(text.to_string())]))
            }

            // A header with a syntax error (reported), or a directive
            // without its argument
            _ => Ok(None),
        }
    }

    fn eval_include(
        &mut self,
        path: &syntax::Arg,
        line_num: usize,
        content: &str,
        ctx: &mut ParseContext,
    ) -> Result<Option<Vec<Node>>, ParseError> {
        let (filename, column) = match path.raw.strip_prefix('"').and_then(|f| f.strip_suffix('"'))
        {
            Some(quoted) => (quoted, path.span.column + 1),
            None => (path.raw.as_str(), path.span.column),
        };
        let filename = ctx.interpolate(filename, line_num, Some(column));
        let resolved = ctx.resolve(&filename);

        if ctx.include_stack.contains(&resolved) {
            let cycle_chain = format_include_chain(&ctx.include_stack);
            ctx.diagnostics.push(
                Diagnostic::error(
                    code::CIRCULAR_INCLUDE,
                    line_num,
                    format!(
                        "circular include '{}' (cycle: {} → {})",
                        filename, cycle_chain, filename
                    ),
                )
                .source(content),
            );
            return Ok(None);
        }

        let imported_text = match ctx.read_file(&resolved) {
            Ok(text) => text,
            Err(e) => {
                ctx.diagnostics.push(
                    Diagnostic::error(
                        code::UNREADABLE_FILE,
                        line_num,
                        format!("cannot include '{}': {}", filename, e),
                    )
                    .source(content),
                );
                return Ok(None);
            }
        };

        ctx.included_files.push(resolved.clone());
        ctx.include_stack.push(resolved.clone());
        let saved_base = ctx.base_path.clone();
        ctx.base_path = resolved.parent().map(|p| p.to_path_buf());

        let diag_count_before = ctx.diagnostics.len();
        let import_chain = format_include_chain(&ctx.include_stack);
        let tree = ctx.parse_tree(&imported_text);
        ctx.diagnostics.extend(tree.diagnostics.iter().cloned());
        ctx.trees.push((tree.clone(), Some(import_chain.clone())));
        let included_nodes = self.eval_block(&tree.nodes, ctx);

        // Annotate new diagnostics with import chain
        for d in &mut ctx.diagnostics[diag_count_before..] {
            d.message = format!("{}\n  in {}", d.message, import_chain);
        }

        ctx.base_path = saved_base;
        ctx.include_stack.pop();
        Ok(Some(included_nodes))
    }

    /// @data $name file.json         values as $name.key
    /// @data $name dir/*.json        a list of the files' records
    /// @data $name [...] / {...}     inline JSON
    /// @data $name env:NAME [DEFAULT] an environment variable
    fn eval_data(
        &mut self,
        prefix: &str,
        source: &syntax::Arg,
        line_num: usize,
        content: &str,
        ctx: &mut ParseContext,
    ) {
        let prefix = prefix.to_string();
        let filename = source.raw.as_str();
        if let Some(env) = filename.strip_prefix("env:") {
            let (var, default) = match env.split_once(char::is_whitespace) {
                Some((var, default)) => (var, Some(default.trim())),
                None => (env, None),
            };
            let default_column = default.map(|d| source.span.column + filename.len() - d.len());
            let value = std::env::var(var)
                .ok()
                .or_else(|| default.map(|d| ctx.interpolate(d, line_num, default_column)));
            if value.is_none() {
                ctx.diagnostics.push(
                    Diagnostic::warning(
                        code::UNSET_ENVIRONMENT,
                        line_num,
                        format!(
                            "environment variable '{}' is not set and has no default",
                            var
                        ),
                    )
                    .source(content),
                );
            }
            ctx.variables.insert(prefix, value.unwrap_or_default());
            return;
        }

        // Inline JSON: @data $links [{"label": "Home", "url": "/"}]
        let (json_text, source) = if filename.starts_with(['[', '{']) {
            (filename.to_string(), "the inline data".to_string())
        } else {
            let filename = ctx.interpolate(filename, line_num, Some(source.span.column));
            if filename.contains('*') {
                self.load_data_glob(&prefix, &filename, line_num, content, ctx);
                return;
            }
            let resolved = ctx.resolve(&filename);
            match std::fs::read_to_string(&resolved) {
                Ok(text) => {
                    ctx.included_files.push(resolved);
                    (text, format!("'{}'", filename))
                }
                Err(e) => {
                    ctx.diagnostics.push(
                        Diagnostic::error(
                            code::UNREADABLE_FILE,
                            line_num,
                            format!("cannot load data '{}': {}", filename, e),
                        )
                        .source(content),
                    );
                    return;
                }
            }
        };
        match parse_json_with_error(&json_text) {
            Ok(json) => flatten_json(&prefix, &json, &mut ctx.variables),
            Err(detail) => ctx.diagnostics.push(
                Diagnostic::error(
                    code::INVALID_JSON,
                    line_num,
                    format!("invalid JSON in {}: {}", source, detail),
                )
                .source(content),
            ),
        }
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
                ctx.diagnostics.push(
                    Diagnostic::new(
                        code::UNREADABLE_FILE,
                        Severity::Error,
                        line_num,
                        format!("cannot read directory for '{}': {}", pattern, e),
                    )
                    .source(content.to_string()),
                );
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
                    ctx.diagnostics.push(
                        Diagnostic::new(
                            code::UNREADABLE_FILE,
                            Severity::Error,
                            line_num,
                            format!("cannot load data '{}': {}", file.display(), e),
                        )
                        .source(content.to_string()),
                    );
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
                Ok(_) => ctx.diagnostics.push(
                    Diagnostic::new(
                        code::INVALID_JSON,
                        Severity::Error,
                        line_num,
                        format!("'{}' should hold a JSON object", file.display()),
                    )
                    .source(content.to_string()),
                ),
                Err(detail) => ctx.diagnostics.push(
                    Diagnostic::new(
                        code::INVALID_JSON,
                        Severity::Error,
                        line_num,
                        format!("invalid JSON in '{}': {}", file.display(), detail),
                    )
                    .source(content.to_string()),
                ),
            }
            ctx.included_files.push(file);
        }
        flatten_json(name, &JsonValue::Array(records), &mut ctx.variables);
    }

    /// `@each $item in LIST` or `@each $item, $index in LIST`, with the
    /// `@else` that follows it. A LIST from JSON binds each record or value
    /// to `$item`; any other list is text, split on commas, or a range
    /// `A..B [step N]`.
    fn eval_each(
        &mut self,
        node: &syntax::Node,
        empty_branch: Option<&syntax::Node>,
        ctx: &mut ParseContext,
    ) -> Result<Option<Vec<Node>>, ParseError> {
        ctx.visited.insert(node.id);
        if let Some(branch) = empty_branch {
            ctx.visited.insert(branch.id);
        }
        ctx.enter(node);
        // A malformed header is a syntax error, already reported
        let Some(DirectiveArgs::Each(header)) = node.directive().map(|d| &d.args) else {
            return Ok(None);
        };
        let body = &node.children;
        let empty: &[syntax::Node] = empty_branch.map_or(&[], |b| &b.children);
        let (item, index) = (header.item.as_str(), header.index.as_deref());
        let list = &header.list;
        let list_src = list.raw.as_str();
        track_var_refs(list_src, &mut ctx.used_variables);

        // A source that is one `$name` (or `${name}`) is that variable's
        // value: a list loaded from JSON, by name, or text
        let whole = list_src.strip_prefix('$').and_then(|after| {
            match interp::reference(after, &Vars(&ctx.variables))? {
                (interp::Reference::Var(path), len) if len == after.len() => Some(path),
                _ => None,
            }
        });
        let data_list = whole.as_ref().and_then(|name| {
            let len = ctx
                .variables
                .get(&format!("{}#", name))?
                .parse::<usize>()
                .ok()?;
            Some((name.clone(), len))
        });
        let undefined = whole
            .as_ref()
            .is_some_and(|path| interp::resolve(path, &Vars(&ctx.variables)).is_none());
        // A field a record doesn't have (`$post.tags`) is an empty list
        let text = ctx.interpolate(list_src, list.span.line, Some(list.span.column));
        let text_items: Vec<String> = match &data_list {
            Some(_) => Vec::new(),
            // Reported; there is nothing to repeat
            None if undefined => Vec::new(),
            None => text_list_items(&text),
        };
        let count = data_list.as_ref().map_or(text_items.len(), |(_, len)| *len);

        if body.iter().all(syntax::Node::is_trivia) {
            return Ok(Some(Vec::new()));
        }
        if count == 0 {
            return Ok(Some(self.eval_scoped(empty, ctx)));
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
        Ok(Some(nodes))
    }

    fn define_function(
        &mut self,
        name: &str,
        params: &[syntax::Param],
        body: &[syntax::Node],
        line_num: usize,
        ctx: &mut ParseContext,
    ) {
        // An @style block at the top of the body is scoped to the
        // function: its rules apply inside a `.hl-NAME` wrapper.
        let (style, body): (Vec<&syntax::Node>, Vec<&syntax::Node>) =
            body.iter().partition(|node| node.is_directive("style"));
        if !style.is_empty() {
            let css: String = style
                .iter()
                .map(|node| {
                    ctx.visited.insert(node.id);
                    verbatim_text(&node.children)
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
                params: params.iter().map(|p| p.name.clone()).collect(),
                defaults: params
                    .iter()
                    .filter_map(|p| Some((p.name.clone(), p.default.clone()?)))
                    .collect(),
                body: body.into_iter().cloned().collect(),
            },
        );
    }

    /// An element line: a chain of elements and function calls, the last of
    /// which gets the indented children.
    fn eval_element(
        &mut self,
        node: &syntax::Node,
        element: &syntax::ElementLine,
        ctx: &mut ParseContext,
    ) -> Result<Vec<Node>, ParseError> {
        // An unclosed attribute list is a syntax error, already reported
        if element
            .chain
            .iter()
            .any(|h| h.attrs.as_ref().is_some_and(|a| !a.closed))
        {
            return Ok(Vec::new());
        }
        enum Link {
            Element(Element),
            Call {
                name: String,
                args: Vec<Attribute>,
                text: Option<syntax::Text>,
            },
        }
        let line_num = node.span.line;
        let last = element.chain.len() - 1;
        let mut links = Vec::new();
        for (i, head) in element.chain.iter().enumerate() {
            let text = if i == last {
                element.text.as_ref()
            } else {
                None
            };
            if ctx.functions.contains_key(&head.name) {
                ctx.used_functions.insert(head.name.clone());
                let args = head.attrs.as_ref().map_or_else(Vec::new, |list| {
                    parse_attr_list(&list.attrs, line_num, ctx, false)
                });
                links.push(Link::Call {
                    name: head.name.clone(),
                    args,
                    text: text.cloned(),
                });
            } else {
                links.push(Link::Element(parse_single_element(
                    head, text, line_num, ctx,
                )?));
            }
        }

        // Children belong to the innermost element; build the chain
        // right-to-left, each link wrapping the next.
        let mut current = self.eval_block(&node.children, ctx);
        for link in links.into_iter().rev() {
            current = match link {
                Link::Element(mut elem) => {
                    elem.children.extend(current);
                    vec![inline_svg(Node::Element(elem), line_num, ctx)]
                }
                Link::Call { name, args, text } => self.expand_fn_call(
                    &name,
                    args,
                    text.as_ref(),
                    current,
                    line_num,
                    &node.source,
                    ctx,
                )?,
            };
        }
        Ok(current)
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_fn_call(
        &mut self,
        name: &str,
        args: Vec<Attribute>,
        trailing_text: Option<&syntax::Text>,
        all_caller_children: Vec<Node>,
        line_num: usize,
        content: &str,
        ctx: &mut ParseContext,
    ) -> Result<Vec<Node>, ParseError> {
        // Recursive function cycle detection
        if ctx.fn_call_stack.contains(&name.to_string()) {
            return Err(Diagnostic::error(
                code::RECURSIVE_CALL,
                line_num,
                format!(
                    "recursive function call to @{} (call stack: {})",
                    name,
                    ctx.fn_call_stack.join(" -> ")
                ),
            )
            .subject(name));
        }

        // Clone function definition (releases borrow on ctx)
        let Some(fn_def) = ctx.functions.get(name).cloned() else {
            return Err(Diagnostic::error(
                code::INTERNAL,
                line_num,
                format!("undefined function @{}", name),
            ));
        };
        ctx.fn_call_stack.push(name.to_string());

        // Separate the caller's named slots from its other children
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
        // Text after the call, as in `@card [color red] New`, is content:
        // it goes first among the caller's children.
        if let Some(text) = trailing_text {
            caller_children.insert(0, Node::Text(text_segments(text, ctx)));
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
        // element: `@card [background red] New`.
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

        // Attributes that aren't parameters style the function's root
        // element, and a scoped @style's class goes on it too.
        let scope_class = ctx
            .scoped_functions
            .contains(name)
            .then(|| format!("hl-{}", name));
        if !forwarded.is_empty() || scope_class.is_some() {
            let mut roots = result_nodes.iter_mut().filter_map(|n| match n {
                Node::Element(e) => Some(e),
                _ => None,
            });
            let root = match (roots.next(), roots.next()) {
                (Some(root), None) => Some(root),
                _ => None,
            };
            let mut problems = Vec::new();
            match root {
                Some(root) => {
                    root.attrs.extend(forwarded);
                    if let Some(class) = scope_class {
                        match root.attrs.iter_mut().find(|a| a.html && a.key == "class") {
                            Some(attr) => {
                                let existing = attr.value.take().unwrap_or_default();
                                attr.value =
                                    Some(format!("{} {}", existing, class).trim().to_string());
                            }
                            None => root.attrs.push(Attribute {
                                key: "class".to_string(),
                                value: Some(class),
                                html: true,
                            }),
                        }
                    }
                }
                None => {
                    if !forwarded.is_empty() {
                        problems.push(format!(
                            "attributes {} on @{} are not parameters, and its body has no single \
                             root element to receive them",
                            forwarded
                                .iter()
                                .map(|a| format!("'{}'", a.key))
                                .collect::<Vec<_>>()
                                .join(", "),
                            name
                        ));
                    }
                    if scope_class.is_some() {
                        problems.push(format!(
                            "@{} has a scoped @style, but its body has no single root element \
                             to scope it to",
                            name
                        ));
                    }
                }
            }
            for message in problems {
                ctx.diagnostics.push(
                    Diagnostic::warning(code::NO_SINGLE_ROOT, line_num, message)
                        .source(content)
                        .subject(name),
                );
            }
        }

        Ok(result_nodes)
    }
}

/// Define `$name` (and, for `--name`, the CSS custom property).
fn set_variable(name: &str, value: String, line_num: usize, ctx: &mut ParseContext) {
    if name.starts_with("--") {
        ctx.css_vars.push((name.to_string(), value.clone()));
    }
    ctx.variables.insert(name.to_string(), value);
    ctx.let_lines.entry(name.to_string()).or_insert(line_num);
}

/// The text of a verbatim body (`@head`, `@style`, `@markdown`, `@raw`).
fn verbatim_text(children: &[syntax::Node]) -> String {
    let mut text = String::new();
    for child in children {
        if let NodeKind::Verbatim(body) = &child.kind {
            text.push_str(&body.text);
            text.push('\n');
        }
    }
    text
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

/// Build the element for `@name [attrs] text` (on its own line, in a chain,
/// or inline in text).
fn parse_single_element(
    head: &syntax::Head,
    text: Option<&syntax::Text>,
    line_num: usize,
    ctx: &mut ParseContext,
) -> Result<Element, ParseError> {
    let kind = parse_element_kind(&head.name, line_num, ctx)?;
    let attrs = head.attrs.as_ref().map_or_else(Vec::new, |list| {
        parse_attr_list(&list.attrs, line_num, ctx, true)
    });

    // The argument is one slot: a URL, a source, an action, a slot name, or
    // text shown as written. For @link, the first word is the URL and the
    // rest is its text. Any other element's argument is text content, as
    // in `@el [padding 8] Hello` or `@paragraph Read {@link /more more}`.
    let mut children = Vec::new();
    let mut argument = None;
    if let Some(text) = text {
        if argument_is_special(&kind) || renders_argument_as_text(&kind) {
            let raw = if kind == ElementKind::Link {
                let (url, rest) = text.split_first_word();
                if let Some(rest) = rest {
                    children.push(Node::Text(text_segments(&rest, ctx)));
                }
                url
            } else {
                text.raw.clone()
            };
            argument = Some(ctx.interpolate_text(&raw, text.span.line, Some(text.span.column)));
        } else {
            children.push(Node::Text(text_segments(text, ctx)));
        }
    }

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

/// The element named `name`, or an "unknown element" error that suggests
/// the closest element, directive or function.
fn parse_element_kind(
    name: &str,
    line_num: usize,
    ctx: &mut ParseContext,
) -> Result<ElementKind, ParseError> {
    if let Some(kind) = ElementKind::from_name(name) {
        return Ok(kind);
    }
    // A known name in a place it can't go: say why instead of suggesting
    // the name itself.
    let misplaced = if crate::ast::directive(name).is_some() {
        Some(format!(
            "unknown element @{}: @{} is a directive, which goes at the start of its own line",
            name, name
        ))
    } else if ctx.functions.contains_key(name) {
        ctx.used_functions.insert(name.to_string());
        Some(format!(
            "unknown element @{}: a function is called on its own line (or in a chain), \
             not inside text",
            name
        ))
    } else if ctx.namespace.contains(name) {
        ctx.used_functions.insert(name.to_string());
        Some(format!(
            "unknown element @{}: the function @{} isn't defined yet when this line runs, \
             so define it above this line",
            name, name
        ))
    } else {
        None
    };
    if let Some(message) = misplaced {
        return Err(Diagnostic::error(code::UNKNOWN_ELEMENT, line_num, message).subject(name));
    }
    let all_known: Vec<&str> = ElementKind::all_names()
        .chain(DIRECTIVES.iter().map(|d| d.name))
        .collect();
    let closest = suggest_closest(name, &all_known);
    let mut message = match closest {
        Some(closest) => format!("unknown element @{}, did you mean @{}?", name, closest),
        None => format!("unknown element @{}", name),
    };
    let function = suggest_fn_name(name, ctx);
    if let Some(function) = &function {
        message = format!("{}, or did you mean @{}?", message, function);
    }
    Err(Diagnostic::error(code::UNKNOWN_ELEMENT, line_num, message)
        .subject(name)
        .suggest(closest.map(str::to_string).or(function)))
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

/// Suggest the closest user-defined function name for typos, from the
/// functions defined so far and those defined anywhere in the file.
fn suggest_fn_name(input: &str, ctx: &ParseContext) -> Option<String> {
    let input_chars: Vec<char> = input.chars().collect();
    let max_allowed = 2usize.min(input_chars.len().saturating_sub(1));
    let mut best: Option<(usize, &String)> = None;
    for name in ctx.functions.keys().chain(ctx.namespace.iter()) {
        let nlen = name.chars().count();
        if nlen.abs_diff(input_chars.len()) > max_allowed {
            continue;
        }
        let name_chars: Vec<char> = name.chars().collect();
        let dist = levenshtein_bounded(&input_chars, &name_chars, max_allowed);
        if dist <= max_allowed && best.is_none_or(|b| (dist, name) < b) {
            best = Some((dist, name));
        }
    }
    best.map(|(_, name)| name.clone())
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
                    ctx.diagnostics.push(Diagnostic::new(
                        code::INVALID_VALUE,
                        Severity::Warning,
                        line_num,
                        format!(
                            "'{}' expects a numeric value (with optional unit), got '{}'",
                            attr.key, val
                        ),
                    ));
                    return;
                }
            }
        } else if NUMERIC_OR_KEYWORD_ATTRS.contains(&base_key) {
            let is_keyword = SIZE_KEYWORDS.contains(&val.as_str());
            let is_numeric = val.parse::<f64>().is_ok();
            let has_unit = has_css_unit(val);
            if !is_keyword && !is_numeric && !has_unit {
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    format!(
                        "'{}' expects a number or one of [{}], got '{}'",
                        attr.key,
                        SIZE_KEYWORDS.join(", "),
                        val
                    ),
                ));
            }
        } else if base_key == "opacity" {
            if let Ok(v) = val.parse::<f64>() {
                if !(0.0..=1.0).contains(&v) {
                    ctx.diagnostics.push(Diagnostic::new(
                        code::INVALID_VALUE,
                        Severity::Warning,
                        line_num,
                        format!("'opacity' should be between 0 and 1, got '{}'", val),
                    ));
                }
            } else {
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    format!("'opacity' expects a numeric value, got '{}'", val),
                ));
            }
        } else if base_key == "z-index" {
            if val.parse::<i32>().is_err() {
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    format!("'z-index' expects an integer, got '{}'", val),
                ));
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
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    msg,
                ));
            }
        } else if base_key == "position" {
            const POSITION_VALUES: &[&str] = &["static", "relative", "absolute", "fixed", "sticky"];
            if !POSITION_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                let suggestion = suggest_closest(val, POSITION_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown position value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown position value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    msg,
                ));
            }
        } else if base_key == "overflow" || base_key == "overflow-x" || base_key == "overflow-y" {
            const OVERFLOW_VALUES: &[&str] = &["visible", "hidden", "scroll", "auto", "clip"];
            if !OVERFLOW_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                let suggestion = suggest_closest(val, OVERFLOW_VALUES);
                let msg = match suggestion {
                    Some(s) => format!("unknown overflow value '{}', did you mean '{}'?", val, s),
                    None => format!("unknown overflow value '{}'", val),
                };
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    msg,
                ));
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
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    msg,
                ));
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
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    msg,
                ));
            }
        } else if base_key == "font-weight" {
            const WEIGHT_VALUES: &[&str] = &[
                "normal", "bold", "bolder", "lighter", "100", "200", "300", "400", "500", "600",
                "700", "800", "900",
            ];
            if !WEIGHT_VALUES.contains(&val.as_str()) && !val.starts_with("var(") {
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_VALUE,
                    Severity::Warning,
                    line_num,
                    format!(
                        "'font-weight' expects a weight keyword or number 100-900, got '{}'",
                        val
                    ),
                ));
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
                    ctx.diagnostics.push(
                        Diagnostic::warning(code::UNKNOWN_COLOR, line_num, msg)
                            .subject(val.as_str())
                            .suggest(suggestion),
                    );
                }
            }
        }
    }
}

fn is_valid_hex_color(s: &str) -> bool {
    if !s.starts_with('#') {
        return true; // Not a hex color, skip
    }
    let hex = &s[1..];
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// Evaluate the attributes of a list: bundles are spliced in, `if()`
/// chooses, and variables fill values. A variable fills only the value it
/// is written in: attributes come from bundles, never from text. With
/// `validate`, unknown names and invalid values are reported.
fn parse_attr_list(
    tokens: &[syntax::Attr],
    line_num: usize,
    ctx: &mut ParseContext,
    validate: bool,
) -> Vec<Attribute> {
    let mut attrs = Vec::new();
    let mut seen_keys: Vec<String> = Vec::new();

    for token in tokens {
        let line = if token.span.line == 0 {
            line_num
        } else {
            token.span.line
        };
        // A whole attribute `if(cond, a, b)` is the chosen branch, as
        // written; the attribute as written otherwise.
        let (key, value, html, as_written) = match choose_if(&token.raw, ctx, line) {
            Some(branch) if branch.is_empty() => continue,
            Some(branch) => {
                let (key, value, html) = syntax::split_attribute(&branch);
                (key, value, html, false)
            }
            None => (token.key.clone(), token.value.clone(), token.html, true),
        };
        let column = |at: usize| as_written.then_some(token.span.column + at);

        // `$name` (or `${name}`) alone: an attribute bundle, spliced in
        let bundle = key.strip_prefix('$').filter(|_| value.is_none() && !html);
        let bundle =
            bundle.and_then(
                |after| match interp::reference(after, &Vars(&ctx.variables))? {
                    (interp::Reference::Var(name), len) if len == after.len() => Some(name),
                    _ => None,
                },
            );
        if let Some(name) = bundle {
            if let Some(define_attrs) = ctx.defines.get(&name) {
                ctx.used_defines.insert(name);
                attrs.extend(define_attrs.clone());
            } else {
                not_a_bundle(&name, line, column(0), ctx);
            }
            continue;
        }
        // A name never comes from a variable
        if key.contains('$') {
            track_var_refs(&key, &mut ctx.used_variables);
            ctx.diagnostics.push(
                Diagnostic::error(
                    code::ATTRIBUTE_FROM_VARIABLE,
                    line,
                    format!(
                        "'{}': an attribute's name can't come from a variable. A variable \
                         fills a value (`padding $size`), and whole attributes come from a \
                         bundle (`@let name [padding 8]`, used as `[$name]`)",
                        key
                    ),
                )
                .subject(key.as_str()),
            );
            continue;
        }

        // A value `if(cond, a, b)` is `a` or `b`, by `cond`; an empty
        // choice leaves the attribute out. Then its variables are filled.
        let mut filled = true;
        let value = match value {
            None => None,
            Some(value) => {
                let at = column(token.raw.len() - value.len());
                let (text, at) = match choose_if(&value, ctx, line) {
                    Some(branch) if branch.is_empty() => continue,
                    Some(branch) => (branch, None),
                    None => (value, at),
                };
                let (text, ok) = ctx.fill(&text, line, at);
                filled = ok;
                Some(text)
            }
        };
        let attr = Attribute { key, value, html };
        // A value that couldn't be filled in is already reported
        let validate = validate && filled;

        // Warn on duplicate attributes (compare full key so pseudo-class
        // variants like `border` and `hover:border` are not conflated)
        if validate {
            if seen_keys.contains(&attr.key) {
                ctx.diagnostics.push(Diagnostic::new(
                    code::DUPLICATE_ATTRIBUTE,
                    Severity::Warning,
                    line_num,
                    format!("duplicate attribute '{}'", attr.key),
                ));
            } else {
                seen_keys.push(attr.key.clone());
            }

            // Color validation for hex colors
            if !attr.html
                && matches!(
                    crate::vocab::base_attribute(&attr.key),
                    "background" | "color"
                )
                && let Some(ref val) = attr.value
                && val.starts_with('#')
                && !is_valid_hex_color(val)
            {
                ctx.diagnostics.push(Diagnostic::new(
                    code::INVALID_COLOR,
                    Severity::Warning,
                    line_num,
                    format!("invalid hex color '{}'", val),
                ));
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
                ctx.diagnostics.push(
                    Diagnostic::new(
                        code::HTML_ATTRIBUTE_FORM,
                        Severity::Warning,
                        line_num,
                        format!(
                            "'{}' is an HTML attribute: write `{}={}`",
                            attr.key,
                            attr.key,
                            attr.value.as_deref().unwrap_or("")
                        ),
                    )
                    .subject(attr.key.as_str()),
                );
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
                ctx.diagnostics.push(
                    Diagnostic::warning(code::UNKNOWN_ATTRIBUTE, line_num, msg)
                        .subject(base_key)
                        .suggest(suggestion),
                );
            }
        }

        attrs.push(attr);
    }

    attrs
}

/// `[$name]` where `$name` isn't an attribute bundle.
fn not_a_bundle(name: &str, line: usize, column: Option<usize>, ctx: &mut ParseContext) {
    track_var_refs(&format!("${}", name), &mut ctx.used_variables);
    let is_value =
        interp::name_len(name) > 0 && interp::resolve(name, &Vars(&ctx.variables)).is_some();
    let diagnostic = if is_value {
        Diagnostic::error(
            code::ATTRIBUTE_FROM_VARIABLE,
            line,
            format!(
                "'${}' is a value, not an attribute: attributes come from a bundle \
                 (`@let name [padding 8]`, used as `[$name]`), and a value fills an \
                 attribute's value (`padding ${}`)",
                name, name
            ),
        )
        .subject(name)
    } else {
        let bundles: Vec<&str> = ctx.defines.keys().map(String::as_str).collect();
        let suggestion = suggest_closest(name, &bundles);
        let message = match suggestion {
            Some(closest) => format!(
                "undefined attribute bundle '${}', did you mean '${}'?",
                name, closest
            ),
            None => format!("undefined attribute bundle '${}'", name),
        };
        Diagnostic::error(code::UNDEFINED_VARIABLE, line, message)
            .subject(name)
            .suggest(suggestion)
    };
    let diagnostic = match (&ctx.current_source, column) {
        ((current, Some(text)), Some(column)) if *current == line => {
            diagnostic.column(column).source(text.clone())
        }
        _ => diagnostic,
    };
    ctx.diagnostics.push(diagnostic);
}

// ---------------------------------------------------------------------------
// Text segment parsing (inline {...} elements)
// ---------------------------------------------------------------------------

/// Escapes in text: `\@`, `\$`, `\{`, `\--` and `\\` stand for the
/// character(s) themselves. While a text is processed, each is a private-use
/// placeholder, so it can't start a variable, an inline element or a line.
const ESCAPES: &[(&str, char, &str)] = &[
    ("\\\\", '\u{E000}', "\\"),
    ("\\$", '\u{E001}', "$"),
    ("\\{", '\u{E002}', "{"),
    ("\\@", '\u{E003}', "@"),
    ("\\--", '\u{E004}', "--"),
];

fn protect_escapes(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (escape, placeholder, _) in ESCAPES {
        out = out.replace(escape, &placeholder.to_string());
    }
    out
}

fn restore_escapes(text: &str) -> String {
    let mut out = text.to_string();
    for (_, placeholder, literal) in ESCAPES {
        out = out.replace(*placeholder, literal);
    }
    out
}

/// Evaluate text: fill in the variables of its plain runs and build its
/// inline elements. Each run is one slot, so a value can't make markup.
fn text_segments(text: &syntax::Text, ctx: &mut ParseContext) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    // The plain text so far, and where it starts
    let mut plain = String::new();
    let mut start: Option<syntax::Span> = None;
    let flush = |plain: &mut String,
                 start: &mut Option<syntax::Span>,
                 segments: &mut Vec<TextSegment>,
                 ctx: &mut ParseContext| {
        if !plain.is_empty() {
            let at = start.unwrap_or(text.span);
            let filled = ctx.interpolate_text(plain, at.line, Some(at.column));
            segments.push(TextSegment::Plain(filled));
            plain.clear();
        }
        *start = None;
    };
    for segment in &text.segments {
        match segment {
            Segment::Plain { raw, span } => {
                start.get_or_insert(*span);
                plain.push_str(raw);
            }
            Segment::Inline(inline) => {
                match parse_single_element(
                    &inline.head,
                    inline.text.as_ref(),
                    ctx.current_line,
                    ctx,
                ) {
                    Ok(elem) => {
                        flush(&mut plain, &mut start, &mut segments, ctx);
                        segments.push(TextSegment::Inline(elem));
                    }
                    Err(mut e) => {
                        // Keep the braces as literal text, but flag what
                        // looks like a mistyped inline element.
                        e.severity = Severity::Warning;
                        e.source_line = Some(text.raw.as_str().into());
                        ctx.diagnostics.push(e);
                        start.get_or_insert(inline.span);
                        plain.push('{');
                        plain.push_str(&inline.raw);
                        if inline.closed {
                            plain.push('}');
                        }
                    }
                }
            }
        }
    }
    flush(&mut plain, &mut start, &mut segments, ctx);
    segments
}

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

/// Evaluate `if(cond, a)` or `if(cond, a, b)` when it is all of `text`,
/// returning the chosen branch (empty when `cond` fails and there is no `b`).
fn choose_if(text: &str, ctx: &mut ParseContext, line: usize) -> Option<String> {
    let inner = text.strip_prefix("if(")?.strip_suffix(')')?;
    let args = split_if_args(inner);
    // `if(a) (b)` isn't one call
    if split_trailing_paren(inner) || !(2..=3).contains(&args.len()) {
        return None;
    }
    let branch = if ctx.condition(args[0].trim(), line, None) {
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
            let mut warn = |code: &'static str, message: String| {
                out.push(Diagnostic::warning(code, elem.line_num, message))
            };
            if depth > 10 {
                warn(
                    code::DEEP_NESTING,
                    format!(
                        "deeply nested element ({} levels): consider simplifying",
                        depth
                    ),
                );
            }
            if matches!(elem.kind, ElementKind::Row | ElementKind::El) && elem.children.is_empty() {
                warn(
                    code::EMPTY_CONTAINER,
                    format!("empty container (@{}) has no children", elem.kind.name()),
                );
            }
            if elem.kind.is_tag("button") && !elem.attrs.iter().any(|a| a.key == "type") {
                warn(
                    code::MISSING_BUTTON_TYPE,
                    "@button missing 'type' attribute (defaults to submit)".to_string(),
                );
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
                    diagnostics.push(Diagnostic::new(
                        code::FILL_FALLBACK,
                        Severity::Warning,
                        elem.line_num,
                        "'width fill' works best inside @row; using 100% as fallback".to_string(),
                    ));
                }
                if base == "height"
                    && attr.value.as_deref() == Some("fill")
                    && !parent_kind.is_some_and(ElementKind::is_column)
                {
                    diagnostics.push(Diagnostic::new(
                        code::FILL_FALLBACK,
                        Severity::Warning,
                        elem.line_num,
                        "'height fill' works best inside @el; using 100% as fallback".to_string(),
                    ));
                }

                // Container-only attributes on non-container elements
                if CONTAINER_ONLY_ATTRS.contains(&base) && !is_container(&elem.kind) {
                    diagnostics.push(Diagnostic::new(
                        code::NO_EFFECT,
                        Severity::Warning,
                        elem.line_num,
                        format!(
                            "'{}' has no effect on {} (only works on @row, @el, @el)",
                            base,
                            element_kind_name(&elem.kind)
                        ),
                    ));
                }

                // Form-specific: placeholder only on @input/@textarea
                if base == "placeholder"
                    && !(elem.kind.is_tag("input") || elem.kind.is_tag("textarea"))
                {
                    diagnostics.push(Diagnostic::new(
                        code::NO_EFFECT,
                        Severity::Warning,
                        elem.line_num,
                        format!(
                            "'placeholder' has no effect on {} (only works on @input, @textarea)",
                            element_kind_name(&elem.kind)
                        ),
                    ));
                }

                // 'for' only on @label
                if base == "for" && !elem.kind.is_tag("label") {
                    diagnostics.push(Diagnostic::new(
                        code::NO_EFFECT,
                        Severity::Warning,
                        elem.line_num,
                        format!(
                            "'for' has no effect on {} (only works on @label)",
                            element_kind_name(&elem.kind)
                        ),
                    ));
                }

                // 'rows'/'cols' only on @textarea
                if (base == "rows" || base == "cols") && !elem.kind.is_tag("textarea") {
                    diagnostics.push(Diagnostic::new(
                        code::NO_EFFECT,
                        Severity::Warning,
                        elem.line_num,
                        format!(
                            "'{}' has no effect on {} (only works on @textarea)",
                            base,
                            element_kind_name(&elem.kind)
                        ),
                    ));
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
                    diagnostics.push(Diagnostic::new(
                        code::NO_EFFECT,
                        Severity::Warning,
                        elem.line_num,
                        format!(
                            "'{}' has no effect on {} (only works on @video, @audio)",
                            base,
                            element_kind_name(&elem.kind)
                        ),
                    ));
                }
            }
            // Missing alt text on @image
            if matches!(elem.kind, ElementKind::Image) && !elem.attrs.iter().any(|a| a.key == "alt")
            {
                diagnostics.push(Diagnostic::new(
                    code::MISSING_ALT,
                    Severity::Warning,
                    elem.line_num,
                    "@image missing 'alt' attribute (accessibility)".to_string(),
                ));
            }
            if elem.kind.is_tag("input") && !elem.attrs.iter().any(|a| a.key == "type") {
                diagnostics.push(Diagnostic::new(
                    code::MISSING_INPUT_TYPE,
                    Severity::Warning,
                    elem.line_num,
                    "@input missing 'type' attribute (defaults to 'text')".to_string(),
                ));
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
                    diagnostics.push(Diagnostic::new(
                        code::MISSING_LINK_TEXT,
                        Severity::Warning,
                        elem.line_num,
                        "@link has no visible text or aria-label (accessibility)".to_string(),
                    ));
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
                    && let (Some(bg_rgb), Some(fg_rgb)) = (
                        crate::expr::parse_hex_rgb(bg),
                        crate::expr::parse_hex_rgb(fg),
                    )
                {
                    let ratio = contrast_ratio(bg_rgb, fg_rgb);
                    if ratio < 4.5 {
                        diagnostics.push(Diagnostic::new(code::LOW_CONTRAST, Severity::Warning, elem.line_num, format!(
                                    "low contrast ratio {:.1}:1 between '{}' and '{}' (WCAG AA requires 4.5:1)",
                                    ratio, fg, bg
                                )));
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
                    diagnostics.push(Diagnostic::new(code::MISSING_LABEL, Severity::Warning, elem.line_num, format!(
                            "{} should have an 'id' (with matching @label[for]), 'aria-label', or be wrapped in @label (accessibility)",
                            element_kind_name(&elem.kind)
                        )));
                }
            }

            // @iframe should have title attribute
            if elem.kind.is_tag("iframe") && !elem.attrs.iter().any(|a| a.key == "title") {
                diagnostics.push(Diagnostic::new(
                    code::MISSING_TITLE,
                    Severity::Warning,
                    elem.line_num,
                    "@iframe missing 'title' attribute (accessibility)".to_string(),
                ));
            }

            // @button should have accessible text
            if elem.kind.is_tag("button") {
                let has_text = elem.argument.is_some() || !elem.children.is_empty();
                let has_aria = elem.attrs.iter().any(|a| a.key == "aria-label");
                if !has_text && !has_aria {
                    diagnostics.push(Diagnostic::new(
                        code::MISSING_BUTTON_TEXT,
                        Severity::Warning,
                        elem.line_num,
                        "@button has no text content or aria-label (accessibility)".to_string(),
                    ));
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
                    diagnostics.push(Diagnostic::new(
                        code::MISSING_CAPTIONS,
                        Severity::Warning,
                        elem.line_num,
                        "@video should have aria-label or captions for accessibility".to_string(),
                    ));
                }
            }

            // Tabindex > 0 is an anti-pattern
            if let Some(tabindex_attr) = elem.attrs.iter().find(|a| a.key == "tabindex")
                && let Some(ref val) = tabindex_attr.value
                && let Ok(n) = val.parse::<i32>()
                && n > 0
            {
                diagnostics.push(Diagnostic::new(code::POSITIVE_TABINDEX, Severity::Warning, elem.line_num, format!("tabindex {} is positive — avoid positive tabindex values as they disrupt natural tab order", n)));
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

/// `@image [inline] icon.svg` becomes the SVG's markup, with `width`,
/// `height`, `color` / `fill`, `class=` and `id=` applied to the `<svg>`
/// tag. Other nodes are returned unchanged.
fn inline_svg(node: Node, line_num: usize, ctx: &mut ParseContext) -> Node {
    let Node::Element(elem) = &node else {
        return node;
    };
    let is_inline_svg = elem.kind == ElementKind::Image
        && elem.attrs.iter().any(|a| a.key == "inline" && !a.html)
        && elem
            .argument
            .as_deref()
            .is_some_and(|src| src.ends_with(".svg"));
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
            ctx.diagnostics.push(Diagnostic::new(
                code::UNREADABLE_FILE,
                Severity::Error,
                line_num,
                format!("cannot load SVG '{}': {}", filename, e),
            ));
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

// ---------------------------------------------------------------------------
// Variable usage tracking
// ---------------------------------------------------------------------------

/// Record the names a string refers to (by syntax) as used.
fn track_var_refs(input: &str, used: &mut HashSet<String>) {
    if input.contains('$') {
        used.extend(interp::names(input).into_iter().map(String::from));
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
            ctx.diagnostics.push(
                Diagnostic::new(
                    code::UNUSED_VARIABLE,
                    Severity::Warning,
                    line,
                    format!("unused variable '${}' (defined but never referenced)", name),
                )
                .subject(name.as_str()),
            );
        }
    }

    // Check unused attribute bundles (@let name [...])
    for (name, &line) in &ctx.define_lines {
        if !ctx.used_defines.contains(name) {
            ctx.diagnostics.push(
                Diagnostic::new(
                    code::UNUSED_BUNDLE,
                    Severity::Warning,
                    line,
                    format!(
                        "unused attribute bundle '${}' (defined but never referenced)",
                        name
                    ),
                )
                .subject(name.as_str()),
            );
        }
    }

    // Check unused functions (@let name ... with body)
    for (name, &line) in &ctx.fn_lines {
        if !ctx.used_functions.contains(name) {
            ctx.diagnostics.push(
                Diagnostic::new(
                    code::UNUSED_FUNCTION,
                    Severity::Warning,
                    line,
                    format!("unused function '@{}' (defined but never called)", name),
                )
                .subject(name.as_str()),
            );
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
    match vars
        .get(&format!("{}#", name))
        .and_then(|n| n.parse::<usize>().ok())
    {
        Some(len) => Some(Value::List(
            (0..len)
                .map(|i| {
                    vars.get(&format!("{}.{}", name, i))
                        .cloned()
                        .unwrap_or_default()
                })
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
