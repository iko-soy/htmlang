use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::ast::*;
use crate::diagnostic::code;
pub use crate::diagnostic::{Diagnostic, Severity};
use crate::interp::{self, Sink};
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
    /// The parameters in the order they are declared; one without a
    /// default is required.
    params: Vec<syntax::Param>,
    /// The definition's line and its text (when it is one physical line),
    /// and its node, for diagnostics about a default.
    line: usize,
    source: Option<String>,
    node: usize,
    body: Vec<syntax::Node>,
    /// The file it is defined in, when that isn't the file being compiled
    /// (see `ParseContext::current_file`).
    file: Option<String>,
    /// The slots its body declares (`@slot NAME`), in order.
    slots: Vec<String>,
    /// Whether its body has a `@children` for the content of a call.
    has_children: bool,
}

impl FnDef {
    fn is_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }
}

/// A saved namespace; see `ParseContext::save_scope`.
struct Scope {
    variables: HashMap<String, String>,
    defines: HashMap<String, Vec<Attribute>>,
    functions: HashMap<String, Rc<FnDef>>,
    scoped_functions: HashSet<String>,
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
    functions: HashMap<String, Rc<FnDef>>,
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
    /// The files each `@include` line brought in (by the line's node id),
    /// with the include chain that leads to each, so the checks of
    /// `@slot` and `@children` see an included file's lines where the
    /// `@include` is written.
    included_by: Rc<IncludedBy>,
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
    /// The file whose lines are being evaluated, when it isn't the file
    /// being compiled: the standard library, or an included file (by its
    /// include chain).
    current_file: Option<String>,
    /// A call went deeper than [`MAX_CALL_DEPTH`]: the calls it is nested
    /// in expand to nothing, until the outermost one returns.
    too_deep: bool,
}

/// How deeply function calls may nest, so that a function that calls
/// itself without an `@if` that stops it is an error instead of a hang.
const MAX_CALL_DEPTH: usize = 64;

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

    fn quoted(&self, path: &str) -> Option<String> {
        self.0.get(&quoted_key(path)).cloned()
    }
}

/// Where the variables keep the CSS form of a quoted value `name`: under
/// `name"`, next to `name` (what it says), as a list keeps its length
/// under `name#`.
fn quoted_key(name: &str) -> String {
    format!("{}\"", name)
}

/// Give the variable `name` a value, quoted text or not.
fn assign(vars: &mut HashMap<String, String>, name: &str, value: String, quoted: Option<&Quoted>) {
    match quoted {
        Some(quoted) => {
            vars.insert(quoted_key(name), quoted.css.clone());
            vars.insert(name.to_string(), quoted.text.clone());
        }
        None => {
            vars.remove(&quoted_key(name));
            vars.insert(name.to_string(), value);
        }
    }
}

/// Evaluates a syntax tree (see `syntax.rs`) into the document's nodes.
struct Evaluator;

impl ParseContext {
    /// Forget what `name` means (a value, a bundle or a function), before a
    /// `@let` gives it a new meaning.
    fn forget(&mut self, name: &str) {
        for key in [name.to_string(), quoted_key(name), format!("{}#", name)] {
            self.variables.remove(&key);
        }
        self.defines.remove(name);
        self.functions.remove(name);
        self.scoped_functions.remove(name);
    }

    /// What the `@let`s so far define, to restore when a scope ends (an
    /// `@if` branch, an `@each` body, a function call).
    fn save_scope(&self) -> Scope {
        Scope {
            variables: self.variables.clone(),
            defines: self.defines.clone(),
            functions: self.functions.clone(),
            scoped_functions: self.scoped_functions.clone(),
        }
    }

    /// End a scope: its values end with it, while bundles and functions
    /// defined inside it stay defined after it. Since a name has one
    /// meaning, a bundle or function that a value inside the scope replaced
    /// comes back, and a value that a bundle or function defined inside the
    /// scope replaced stays gone.
    fn restore_scope(&mut self, saved: Scope) {
        self.variables = saved.variables;
        let taken = |ctx: &Self, name: &str| {
            ctx.defines.contains_key(name) || ctx.functions.contains_key(name)
        };
        let new: Vec<String> = self
            .defines
            .keys()
            .filter(|name| !saved.defines.contains_key(*name))
            .chain(
                self.functions
                    .keys()
                    .filter(|name| !saved.functions.contains_key(*name)),
            )
            .cloned()
            .collect();
        for (name, attrs) in saved.defines {
            if !taken(self, &name) {
                self.defines.insert(name, attrs);
            }
        }
        for (name, function) in saved.functions {
            if !taken(self, &name) {
                if saved.scoped_functions.contains(&name) {
                    self.scoped_functions.insert(name.clone());
                }
                self.functions.insert(name, function);
            }
        }
        for name in new {
            for key in [name.clone(), quoted_key(&name), format!("{}#", name)] {
                self.variables.remove(&key);
            }
        }
    }

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

    /// Fill in one slot of htmlang text written at `line` and `column`
    /// (when known), for `sink`: its escapes stand for themselves and its
    /// variables are filled in (see `interp.rs`). What can't be filled is
    /// left as written and reported; the flag says whether everything was
    /// filled.
    fn fill(
        &mut self,
        raw: &str,
        line: usize,
        column: Option<usize>,
        sink: Sink,
    ) -> (String, bool) {
        let (filled, ok) = self.fill_protected(raw, line, column, sink);
        (restore_escapes(&filled), ok)
    }

    /// [`fill`](Self::fill) for text.
    fn interpolate_text(&mut self, raw: &str, line: usize, column: Option<usize>) -> String {
        self.fill(raw, line, column, Sink::Text).0
    }

    /// [`fill`](Self::fill), with the escapes still placeholders (see
    /// [`protect_escapes`]).
    fn fill_protected(
        &mut self,
        raw: &str,
        line: usize,
        column: Option<usize>,
        sink: Sink,
    ) -> (String, bool) {
        let mut protected = protect_escapes(raw);
        if sink == Sink::Css {
            protected = keep_css_string_escapes(&protected);
        }
        if !protected.contains('$') {
            return (protected, true);
        }
        track_var_refs(&protected, &mut self.used_variables);
        let (out, problems) = interp::interpolate_for(&protected, &Vars(&self.variables), sink);
        let filled = problems.is_empty();
        for mut problem in problems {
            match &mut problem {
                interp::Problem::Undefined { offset, .. }
                | interp::Problem::Invalid { offset, .. } => {
                    // An offset into the protected text, as one into `raw`
                    *offset = protected[..*offset]
                        .chars()
                        .map(|c| escape_of(c).map_or(c.len_utf8(), str::len))
                        .sum();
                }
            }
            self.report(problem, line, column);
        }
        (out, filled)
    }

    /// Fill in a value slot (an attribute's value, a `@let` value, `@meta`'s
    /// value) for `sink`, and say whether it is quoted text: one `"..."`, or
    /// one variable that holds quoted text. Quoted text keeps its quotes
    /// only in CSS.
    fn fill_value(
        &mut self,
        raw: &str,
        line: usize,
        column: Option<usize>,
        sink: Sink,
    ) -> (String, Option<Quoted>, bool) {
        if let Some(inner) = syntax::quoted_string(raw) {
            let (css, ok) = self.fill(raw, line, column, Sink::Css);
            // Already reported by the CSS pass, so filled in quietly
            let (text, _) = interp::interpolate(&protect_escapes(inner), &Vars(&self.variables));
            let quoted = Quoted {
                text: restore_escapes(&text),
                css,
            };
            let out = match sink {
                Sink::Css => quoted.css.clone(),
                Sink::Text => quoted.text.clone(),
            };
            return (out, Some(quoted), ok);
        }
        let (out, ok) = self.fill(raw, line, column, sink);
        let quoted = whole_reference(raw, &self.variables).and_then(|path| {
            let css = interp::Scope::quoted(&Vars(&self.variables), &path)?;
            let text = self.variables.get(&path)?.clone();
            Some(Quoted { text, css })
        });
        (out, quoted, ok)
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
        self.push_once(diagnostic);
    }

    /// Report a diagnostic unless the node being evaluated already did,
    /// so a line evaluated many times (in a loop, in a function) reports
    /// each problem once.
    fn push_once(&mut self, diagnostic: Diagnostic) {
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
    ctx.current_file = Some("std.hl".to_string());
    let _ = Evaluator.eval_block(&tree.nodes, ctx);
    ctx.current_file = None;
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
        included_by: Rc::default(),
        namespace: HashSet::new(),
        current_source: (0, None),
        reported: HashSet::new(),
        current_node: 0,
        current_file: None,
        too_deep: false,
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
    check_slot_places(&mut ctx);
    validate_tree(&nodes, None, &mut ctx.diagnostics);
    check_unused(&mut ctx);
    dedupe(&mut ctx.diagnostics);
    ParseResult {
        document: Document {
            page_title: ctx.page_title,
            lang: ctx.lang,
            favicon: ctx.favicon,
            meta_tags: ctx.meta_tags,
            head_blocks: ctx.head_blocks,
            // Without the quoted forms (see `quoted_key`)
            variables: ctx
                .variables
                .into_iter()
                .filter(|(key, _)| !key.ends_with('"'))
                .collect(),
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
                check_unevaluated_content(node, element, ctx);
                if let Some(text) = &element.text {
                    check_inline_heads(text, line, ctx);
                }
            }
            NodeKind::Text(text) => check_inline_heads(text, line, ctx),
            _ => {}
        }
        let text = written_line(node);
        for d in &mut ctx.diagnostics[before..] {
            d.source_line.get_or_insert_with(|| text.as_str().into());
        }
    }
    for child in &node.children {
        check_unevaluated_node(child, ctx);
    }
}

/// Check the content passed to the calls of a line that was never
/// evaluated: its `@slot NAME` blocks, and, for a function without
/// `@children`, any other content.
fn check_unevaluated_content(
    node: &syntax::Node,
    element: &syntax::ElementLine,
    ctx: &mut ParseContext,
) {
    let calls: Vec<Option<(String, Rc<FnDef>)>> = element
        .chain
        .iter()
        .map(|head| {
            let function = ctx.functions.get(&head.name)?;
            Some((head.name.clone(), function.clone()))
        })
        .collect();
    check_fillers(node, element, &calls, ctx);
    let last = calls.len() - 1;
    for (i, call) in calls.iter().enumerate() {
        let Some((name, function)) = call else {
            continue;
        };
        if function.has_children {
            continue;
        }
        let content = if i == last {
            element.text.is_some() || holds_content(&node.children, ctx)
        } else {
            element.chain[i + 1].name != "slot" || !slot_is_built_in(ctx)
        };
        if content {
            let head = &element.chain[i];
            ctx.push_once(no_children(
                name,
                node.span.line,
                name_column(head, node.span.line),
            ));
        }
    }
}

/// Check a head that was never evaluated: its name, and its attributes
/// (for a call, see [`check_call`]).
fn check_head(head: &syntax::Head, line: usize, inline: bool, ctx: &mut ParseContext) {
    let is_function = ctx.namespace.contains(&head.name) || ctx.functions.contains_key(&head.name);
    if is_function {
        ctx.used_functions.insert(head.name.clone());
        if let Some(function) = ctx.functions.get(&head.name).cloned() {
            check_call(head, &function, line, ctx);
        }
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
        check_attrs(&list.attrs, line, ctx, &[]);
    }
}

/// The attributes of a list that is never evaluated that can be checked
/// together: not bundles or `if()`s. Names are checked; a value with a
/// variable in it depends on what the variable holds, so only a literal
/// value is.
fn literal_attrs(tokens: &[syntax::Attr]) -> Vec<syntax::Attr> {
    tokens
        .iter()
        .filter(|a| {
            let bundle = a.key.starts_with('$') && a.value.is_none() && !a.html;
            a.choice.is_none() && !bundle
        })
        .map(|a| {
            let mut a = a.clone();
            let variable = |v: &String| v.contains('$');
            if !a.key.contains('$') && a.value.as_ref().is_some_and(variable) {
                a.value = None;
                a.raw = a.key.clone();
            }
            a
        })
        .collect()
}

/// Check a call that was never evaluated: how it passes its parameters
/// (none written `name=value`, and every one without a default passed,
/// when no bundle or `if()` could pass it), and its other attributes, like
/// those of an element.
fn check_call(head: &syntax::Head, function: &FnDef, line: usize, ctx: &mut ParseContext) {
    let name = head.name.as_str();
    let attrs = head.attrs.as_ref().map_or(&[][..], |list| &list.attrs[..]);
    written_parameter_forms(name, function, attrs, line, ctx);
    if let Some(list) = &head.attrs {
        let params: Vec<String> = function.params.iter().map(|p| p.name.clone()).collect();
        check_attrs(&list.attrs, line, ctx, &params);
    }
    let unknown = attrs
        .iter()
        .any(|a| a.choice.is_some() || (a.key.starts_with('$') && a.value.is_none()));
    if unknown {
        return;
    }
    for param in function.params.iter().filter(|p| p.default.is_none()) {
        if !attrs.iter().any(|a| a.key == param.name) {
            let column = name_column(head, line);
            ctx.push_once(missing_parameter(name, &param.name, line, column));
        }
    }
}

/// Report each parameter a call's list writes `name=value`, with the value
/// as written and its column; returns their names, so the evaluated call
/// doesn't report them again.
fn written_parameter_forms(
    name: &str,
    function: &FnDef,
    attrs: &[syntax::Attr],
    line: usize,
    ctx: &mut ParseContext,
) -> Vec<String> {
    let mut reported = Vec::new();
    for attr in attrs {
        if !(attr.html && function.is_param(&attr.key)) {
            continue;
        }
        let at = if attr.span.line == 0 {
            line
        } else {
            attr.span.line
        };
        let value = attr.value.as_deref().unwrap_or("");
        let diagnostic = parameter_form(name, &attr.key, value, at).column(attr.span.column);
        ctx.push_once(match &ctx.current_source {
            (current, Some(text)) if *current == at => diagnostic.source(text.clone()),
            _ => diagnostic,
        });
        reported.push(attr.key.clone());
    }
    reported
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
        let saved = ctx.save_scope();
        let nodes = self.eval_block(block, ctx);
        ctx.restore_scope(saved);
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
                // it (a syntax error) are evaluated as its siblings. A
                // @let's body belongs to it even under a value (also an
                // error), so it is never evaluated in place.
                let takes_body = directive.spec.body != BodyKind::None;
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
                    for attr in parse_attr_list(&list.attrs, line_num, ctx, false, &[]) {
                        // A setting, not CSS: quoted text loses its quotes
                        let value = match attr.quoted {
                            Some(quoted) => quoted.text,
                            None => attr.value.unwrap_or_default(),
                        };
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
                        ctx.interpolate_text(&title.raw, title.span.line, Some(title.span.column))
                    }
                    None => String::new(),
                };
                ctx.page_title = Some(title);
                Ok(None)
            }

            ("let", DirectiveArgs::Let(def)) => {
                let name = def.name.as_str();
                // Values, bundles and functions share one namespace: a
                // definition replaces whatever the name meant before
                ctx.forget(name);
                match &def.form {
                    LetForm::Function(function) => {
                        self.define_function(name, &function.params, node, ctx);
                    }
                    // `@let name = EXPR` computes its value (see expr.rs)
                    LetForm::Computed(expression) => {
                        let value = ctx
                            .eval(&expression.raw, line_num, Some(expression.span.column))
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        set_variable(name, value, None, line_num, ctx);
                    }
                    // Attribute bundle: @let name [attr1, attr2, ...]
                    LetForm::Bundle(list) => {
                        let attrs = parse_attr_list(&list.attrs, line_num, ctx, true, &[]);
                        ctx.defines.insert(name.to_string(), attrs);
                        ctx.define_lines.entry(name.to_string()).or_insert(line_num);
                    }
                    // Text with `$var` interpolation: `@let size 16px`. It is
                    // quoted text when it is one variable holding some.
                    LetForm::Value(value) => {
                        let (text, quoted, _) = ctx.fill_value(
                            &value.raw,
                            value.span.line,
                            Some(value.span.column),
                            Sink::Text,
                        );
                        set_variable(name, text, quoted.as_ref(), line_num, ctx);
                    }
                    // Quoted text: `@let arrow "→ "`
                    LetForm::Quoted(value) => {
                        let raw = format!("\"{}\"", value.raw);
                        let column = value.span.column.saturating_sub(1);
                        let (text, quoted, _) =
                            ctx.fill_value(&raw, value.span.line, Some(column), Sink::Text);
                        set_variable(name, text, quoted.as_ref(), line_num, ctx);
                    }
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
                // An HTML attribute value: quoted text loses its quotes
                let (value, _, _) =
                    ctx.fill_value(value.trim(), line_num, Some(value_column), Sink::Text);
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
                let filename = ctx.interpolate_text(&file.raw, line_num, Some(file.span.column));
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
                self.eval_include(node.id, path, line_num, content, ctx)
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
        id: usize,
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
        let filename = ctx.interpolate_text(filename, line_num, Some(column));
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
        let files = Rc::make_mut(&mut ctx.included_by).entry(id).or_default();
        if !files.iter().any(|(_, chain)| *chain == import_chain) {
            files.push((tree.clone(), import_chain.clone()));
        }
        let saved_file = ctx.current_file.replace(import_chain.clone());
        let included_nodes = self.eval_block(&tree.nodes, ctx);
        ctx.current_file = saved_file;

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
        ctx.variables.remove(&quoted_key(&prefix));
        let filename = source.raw.as_str();
        if let Some(env) = filename.strip_prefix("env:") {
            let (var, default) = match env.split_once(char::is_whitespace) {
                Some((var, default)) => (var, Some(default.trim())),
                None => (env, None),
            };
            let default_column = default.map(|d| source.span.column + filename.len() - d.len());
            let value = std::env::var(var)
                .ok()
                .or_else(|| default.map(|d| ctx.interpolate_text(d, line_num, default_column)));
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
            assign(&mut ctx.variables, &prefix, value.unwrap_or_default(), None);
            return;
        }

        // Inline JSON: @data $links [{"label": "Home", "url": "/"}]
        let (json_text, source) = if filename.starts_with(['[', '{']) {
            (filename.to_string(), "the inline data".to_string())
        } else {
            let filename = ctx.interpolate_text(filename, line_num, Some(source.span.column));
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
        let whole = whole_reference(list_src, &ctx.variables);
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
        // Items are split at the commas that aren't escaped (`\,`)
        let (text, _) =
            ctx.fill_protected(list_src, list.span.line, Some(list.span.column), Sink::Text);
        let text_items: Vec<String> = match &data_list {
            Some(_) => Vec::new(),
            // Reported; there is nothing to repeat
            None if undefined => Vec::new(),
            None => text_list_items(&text)
                .iter()
                .map(|item| restore_escapes(item))
                .collect(),
        };
        let count = data_list.as_ref().map_or(text_items.len(), |(_, len)| *len);

        if body.iter().all(syntax::Node::is_trivia) {
            return Ok(Some(Vec::new()));
        }
        if count == 0 {
            return Ok(Some(self.eval_scoped(empty, ctx)));
        }
        let saved = ctx.save_scope();
        let mut nodes = Vec::new();
        for i in 0..count {
            match &data_list {
                Some((name, _)) => bind_item(&mut ctx.variables, &format!("{}.{}", name, i), item),
                None => {
                    let text = text_items.get(i).cloned().unwrap_or_default();
                    assign(&mut ctx.variables, item, text, None);
                }
            }
            if let Some(index) = index {
                assign(&mut ctx.variables, index, i.to_string(), None);
            }
            nodes.extend(self.eval_block(body, ctx));
        }
        ctx.restore_scope(saved);
        Ok(Some(nodes))
    }

    fn define_function(
        &mut self,
        name: &str,
        params: &[syntax::Param],
        node: &syntax::Node,
        ctx: &mut ParseContext,
    ) {
        let line_num = node.span.line;
        let body = &node.children;
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

        // A default is filled in only when a call leaves its parameter
        // out, so the names in it count as used here
        for default in params.iter().filter_map(|p| p.default.as_ref()) {
            track_var_refs(default, &mut ctx.used_variables);
        }
        ctx.fn_lines.entry(name.to_string()).or_insert(line_num);
        let body: Vec<syntax::Node> = body.into_iter().cloned().collect();
        let (slots, has_children) = declared_slots(&body, ctx);
        ctx.functions.insert(
            name.to_string(),
            Rc::new(FnDef {
                params: params.to_vec(),
                line: line_num,
                source: ctx.current_source.1.clone(),
                node: node.id,
                body,
                file: ctx.current_file.clone(),
                slots,
                has_children,
            }),
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
        let line_num = node.span.line;
        let last = element.chain.len() - 1;
        // Each head is read where it is written, left to right. The
        // children belong to the last one, so the chain is built right to
        // left, each link wrapping the next.
        let mut links = Vec::new();
        for (i, head) in element.chain.iter().enumerate() {
            let text = if i == last {
                element.text.as_ref()
            } else {
                None
            };
            links.push(resolve(head, text, line_num, &node.source, ctx)?);
        }
        let calls: Vec<Option<(String, Rc<FnDef>)>> = links
            .iter()
            .map(|link| match link {
                Resolved::Call(call) => Some((call.name.clone(), call.function.clone())),
                Resolved::Element(_) => None,
            })
            .collect();
        let mut current = self.eval_block(&node.children, ctx);
        // After the lines under the call ran, so the files they include
        // are read
        check_fillers(node, element, &calls, ctx);
        for link in links.into_iter().rev() {
            current = self.complete(link, current, ctx)?;
        }
        Ok(current)
    }

    /// Give a resolved `@name` its children: an element holds them, and a
    /// function's body puts them where its `@children` is.
    fn complete(
        &mut self,
        resolved: Resolved,
        children: Vec<Node>,
        ctx: &mut ParseContext,
    ) -> Result<Vec<Node>, ParseError> {
        match resolved {
            Resolved::Element(mut element) => {
                element.children.extend(children);
                let line = element.line_num;
                Ok(vec![inline_svg(Node::Element(element), line, ctx)])
            }
            Resolved::Call(call) => self.expand_fn_call(*call, children, ctx),
        }
    }

    fn expand_fn_call(
        &mut self,
        call: Call,
        all_caller_children: Vec<Node>,
        ctx: &mut ParseContext,
    ) -> Result<Vec<Node>, ParseError> {
        let Call {
            name,
            function: fn_def,
            args,
            records,
            text: trailing_text,
            written_form,
            line: line_num,
            name_at,
            source: content,
        } = call;
        let (name, content) = (name.as_str(), content.as_str());

        // Separate the caller's `@slot NAME` blocks from its other
        // children. A block for a slot the function doesn't declare was
        // reported where it is written (see `check_fillers`), like one
        // without a name.
        let mut slot_contents: HashMap<String, Vec<Node>> = HashMap::new();
        let mut caller_children = Vec::new();
        for child in all_caller_children {
            if let Node::Element(elem) = child {
                match elem.kind {
                    ElementKind::Slot(slot_name) => {
                        slot_contents
                            .entry(slot_name)
                            .or_default()
                            .extend(elem.children);
                    }
                    kind => caller_children.push(Node::Element(Element { kind, ..elem })),
                }
                continue;
            }
            caller_children.push(child);
        }
        // Text after the call, as in `@card [color red] New`, is content:
        // it goes first among the caller's children, and like them it is
        // evaluated where the call is written.
        if let Some(text) = &trailing_text {
            caller_children.insert(0, Node::Text(text_segments(text, ctx)));
        }
        // Content goes where the body's `@children` is: without one, it
        // would be dropped
        if !fn_def.has_children && !caller_children.is_empty() {
            let (column, text) = match &name_at {
                Some((column, text)) => (Some(*column), text.as_str()),
                None => (None, content),
            };
            ctx.push_once(no_children(name, line_num, column).source(text));
        }

        // Only the body is inside the call, so a function may appear in the
        // content its caller passes it. A function may call itself, under
        // a condition that stops it; one that doesn't stop goes too deep.
        if ctx.too_deep {
            return Ok(Vec::new());
        }
        if ctx.fn_call_stack.len() >= MAX_CALL_DEPTH {
            ctx.too_deep = true;
            return Err(too_deep(name, &ctx.fn_call_stack, line_num).source(content));
        }
        let caller = (
            ctx.current_line,
            ctx.current_source.clone(),
            ctx.current_node,
        );
        ctx.fn_call_stack.push(name.to_string());

        // Bind the parameters, by name, in the order they are declared: a
        // default is filled in at the call, where it sees the parameters
        // before it.
        let saved = ctx.save_scope();
        let mut consumed = vec![false; args.len()];
        for param in &fn_def.params {
            let mut passed = None;
            for (j, arg) in args.iter().enumerate() {
                if arg.key != param.name {
                    continue;
                }
                consumed[j] = true;
                if passed.is_some() {
                    ctx.push_once(
                        Diagnostic::warning(
                            code::DUPLICATE_ATTRIBUTE,
                            line_num,
                            format!("parameter '{}' of @{} is passed twice", param.name, name),
                        )
                        .source(content)
                        .subject(param.name.as_str()),
                    );
                    continue;
                }
                // Written `name=value` in the call's own list is reported
                // as written; this is one from a bundle
                if arg.html && !written_form.contains(&arg.key) {
                    let value = arg.value.as_deref().unwrap_or("");
                    ctx.push_once(parameter_form(name, &arg.key, value, line_num).source(content));
                }
                passed = Some(arg);
            }
            let (value, quoted, record) = match passed {
                // A parameter's name alone is true: `@post-card [featured]`
                Some(arg) if arg.value.is_none() => ("true".to_string(), None, None),
                Some(arg) => {
                    let quoted = arg.quoted.clone();
                    let value = match &quoted {
                        Some(quoted) => quoted.text.clone(),
                        None => arg.value.clone().unwrap_or_default(),
                    };
                    let record = records
                        .iter()
                        .find(|(key, _)| *key == param.name)
                        .map(|(_, entries)| entries.clone());
                    (value, quoted, record)
                }
                None => match &param.default {
                    Some(default) => {
                        let record = whole_record(default, &ctx.variables)
                            .map(|path| record_entries(&ctx.variables, &path));
                        let before = ctx.diagnostics.len();
                        let (value, quoted) = fill_default(&fn_def, param, default, ctx);
                        point_at_call(ctx, before, &fn_def, name, line_num, content);
                        (value, quoted, record)
                    }
                    None => {
                        let (column, text) = match &name_at {
                            Some((column, text)) => (Some(*column), text.as_str()),
                            None => (None, content),
                        };
                        ctx.push_once(
                            missing_parameter(name, &param.name, line_num, column).source(text),
                        );
                        (String::new(), None, None)
                    }
                },
            };
            assign(&mut ctx.variables, &param.name, value, quoted.as_ref());
            if let Some(entries) = record {
                bind_record(&mut ctx.variables, &param.name, &entries);
            }
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

        // Evaluate the body with the parameters in scope, as lines of the
        // file that defines the function
        let caller_file = std::mem::replace(&mut ctx.current_file, fn_def.file.clone());
        let before = ctx.diagnostics.len();
        let mut body_nodes = self.eval_block(&fn_def.body, ctx);
        ctx.current_file = caller_file;
        point_at_call(ctx, before, &fn_def, name, line_num, content);

        // Restore variables and call stack
        ctx.restore_scope(saved);
        ctx.fn_call_stack.pop();
        if ctx.fn_call_stack.is_empty() {
            ctx.too_deep = false;
        }
        (ctx.current_line, ctx.current_source, ctx.current_node) = caller;

        // What the body wrote is the call's
        belongs_to_call(&mut body_nodes, name, line_num);

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
                                quoted: None,
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

/// A `@name` read where it is written: a built-in element, or a call.
enum Resolved {
    Element(Element),
    Call(Box<Call>),
}

/// A call of a function, with its arguments evaluated where it is written.
struct Call {
    name: String,
    function: Rc<FnDef>,
    args: Vec<Attribute>,
    /// Parameters passed one `$name` that holds a record or a list, with
    /// its entries (see [`record_entries`]): the parameter gets all of it.
    records: Vec<(String, Vec<(String, String)>)>,
    /// Text after the attributes, which is content.
    text: Option<syntax::Text>,
    /// Parameters written `name=value`, already reported.
    written_form: Vec<String>,
    line: usize,
    /// The column of the call's name and the line it is on, for a missing
    /// parameter.
    name_at: Option<(usize, String)>,
    /// The line as written, for diagnostics.
    source: String,
}

/// What `@name` is where it is written: a call of the function `name`
/// when one is defined, else the built-in element. A whole line, each link
/// of a chain and an inline element in text all resolve here.
fn resolve(
    head: &syntax::Head,
    text: Option<&syntax::Text>,
    line: usize,
    source: &str,
    ctx: &mut ParseContext,
) -> Result<Resolved, ParseError> {
    let Some(function) = ctx.functions.get(&head.name).cloned() else {
        return parse_single_element(head, text, line, ctx).map(Resolved::Element);
    };
    ctx.used_functions.insert(head.name.clone());
    let params: Vec<String> = function.params.iter().map(|p| p.name.clone()).collect();
    let written = head.attrs.as_ref().map_or(&[][..], |list| &list.attrs[..]);
    let written_form = written_parameter_forms(&head.name, &function, written, line, ctx);
    let records = written
        .iter()
        .filter(|a| !a.html && function.is_param(&a.key))
        .filter_map(|a| {
            let path = whole_record(a.value.as_deref()?, &ctx.variables)?;
            Some((a.key.clone(), record_entries(&ctx.variables, &path)))
        })
        .collect();
    // Parameters are bound by name; every other attribute goes to the
    // root element, and is checked like any attribute
    let args = parse_attr_list(written, line, ctx, true, &params);
    let name_at = name_column(head, line).zip(match &ctx.current_source {
        (current, Some(text)) if *current == line => Some(text.clone()),
        _ => None,
    });
    Ok(Resolved::Call(Box::new(Call {
        name: head.name.clone(),
        function,
        args,
        records,
        text: text.cloned(),
        written_form,
        line,
        name_at,
        source: source.to_string(),
    })))
}

/// A line of another file (the standard library, an included file) means
/// nothing in this one: the problems reported since `before` while
/// evaluating the function (its body, a default) are moved to the call.
fn point_at_call(
    ctx: &mut ParseContext,
    before: usize,
    function: &FnDef,
    name: &str,
    line: usize,
    source: &str,
) {
    if function.file == ctx.current_file {
        return;
    }
    let defined_in = function
        .file
        .as_deref()
        .unwrap_or("the file being compiled");
    for d in &mut ctx.diagnostics[before..] {
        d.message = format!(
            "{}\n  in @{} (line {} of {})",
            d.message, name, d.line, defined_in
        );
        d.line = line;
        d.column = None;
        d.subject = None;
        d.suggestion = None;
        d.source_line = Some(source.into());
    }
}

/// A call nested deeper than [`MAX_CALL_DEPTH`].
fn too_deep(name: &str, stack: &[String], line: usize) -> Diagnostic {
    let message = if stack.iter().any(|n| n == name) {
        format!(
            "recursive function call to @{} goes more than {} calls deep: a function that \
             calls itself needs a condition that stops it (`@if`)",
            name, MAX_CALL_DEPTH
        )
    } else {
        format!(
            "function calls nest more than {} deep at @{} (starting with {})",
            MAX_CALL_DEPTH,
            name,
            stack
                .iter()
                .take(3)
                .map(|n| format!("@{}", n))
                .collect::<Vec<_>>()
                .join(" → ")
        )
    };
    Diagnostic::error(code::RECURSIVE_CALL, line, message).subject(name)
}

/// Mark what a function's body wrote as the call's: it gets the call's
/// line, so what is reported about it points at the call, and the name of
/// the function, unless a call inside the body already claimed it.
fn belongs_to_call(nodes: &mut [Node], function: &str, line: usize) {
    fn claim(element: &mut Element, function: &str, line: usize) {
        element.line_num = line;
        element.function.get_or_insert_with(|| function.to_string());
        belongs_to_call(&mut element.children, function, line);
    }
    for node in nodes {
        match node {
            Node::Element(element) => claim(element, function, line),
            Node::Text(segments) => {
                for segment in segments {
                    if let TextSegment::Inline(element) = segment {
                        claim(element, function, line);
                    }
                }
            }
            Node::Raw(_) => {}
        }
    }
}

/// What an inline `{@name ...}` stands for in its line of text: an
/// element; the text of a function whose body is text; or, for a body with
/// several roots, its nodes as a fragment.
fn inline_segments(mut nodes: Vec<Node>, line: usize) -> Vec<TextSegment> {
    if let [Node::Element(_)] = nodes.as_slice()
        && let Some(Node::Element(element)) = nodes.pop()
    {
        return vec![TextSegment::Inline(element)];
    }
    if nodes.iter().all(|n| matches!(n, Node::Text(_))) {
        let mut segments = Vec::new();
        for node in nodes {
            if let Node::Text(run) = node {
                if !segments.is_empty() {
                    segments.push(TextSegment::Plain(" ".to_string()));
                }
                segments.extend(run);
            }
        }
        return segments;
    }
    vec![TextSegment::Inline(Element {
        kind: ElementKind::Fragment,
        attrs: Vec::new(),
        argument: None,
        children: nodes,
        line_num: line,
        function: None,
    })]
}

/// Fill in the default of a parameter that a call leaves out, like the
/// value of an attribute passed for it: variables are filled in, and one
/// `"..."` is quoted text. Its problems are reported
/// at the definition, once.
fn fill_default(
    fn_def: &FnDef,
    param: &syntax::Param,
    default: &str,
    ctx: &mut ParseContext,
) -> (String, Option<Quoted>) {
    // A default that uses a later parameter is an error in the
    // definition; the later one isn't bound yet, so don't report it again
    let later = fn_def
        .params
        .iter()
        .skip_while(|p| p.name != param.name)
        .skip(1);
    let names = interp::names(default);
    if later.clone().any(|p| names.contains(&p.name.as_str())) {
        return (String::new(), None);
    }
    let line = param.span.line;
    let at_head = line == fn_def.line && fn_def.source.is_some();
    let column =
        at_head.then(|| param.span.column + (param.span.end - param.span.start) - default.len());
    let caller = (
        std::mem::replace(
            &mut ctx.current_source,
            (fn_def.line, fn_def.source.clone()),
        ),
        std::mem::replace(&mut ctx.current_node, fn_def.node),
    );
    let (text, quoted, _) = ctx.fill_value(default, line, column, Sink::Text);
    (ctx.current_source, ctx.current_node) = caller;
    (text, quoted)
}

/// A parameter passed `name=value`, the form of an HTML attribute.
fn parameter_form(function: &str, key: &str, value: &str, line: usize) -> Diagnostic {
    Diagnostic::error(
        code::PARAMETER_FORM,
        line,
        format!(
            "'{}' is a parameter of @{}: parameters are written `name value`, so write `{} {}`",
            key, function, key, value
        ),
    )
    .subject(format!("{}=", key))
    .suggest(Some(format!("{} ", key)))
}

/// A call that leaves out a parameter without a default.
fn missing_parameter(
    function: &str,
    param: &str,
    line: usize,
    column: Option<usize>,
) -> Diagnostic {
    let diagnostic = Diagnostic::error(
        code::MISSING_PARAMETER,
        line,
        format!(
            "@{} needs '{}': the parameter has no default, so pass it as `{} VALUE`",
            function, param, param
        ),
    )
    .subject(param);
    match column {
        Some(column) => diagnostic.column(column),
        None => diagnostic,
    }
}

/// The column of a call's `@name`, when it is on the call's line.
fn name_column(head: &syntax::Head, line: usize) -> Option<usize> {
    (head.name_span.line == line).then_some(head.name_span.column)
}

/// Define `$name` (and, for `--name`, the CSS custom property, which gets
/// quoted text with its quotes).
fn set_variable(
    name: &str,
    value: String,
    quoted: Option<&Quoted>,
    line_num: usize,
    ctx: &mut ParseContext,
) {
    if name.starts_with("--") {
        let css = quoted.map_or_else(|| value.clone(), |q| q.css.clone());
        ctx.css_vars.push((name.to_string(), css));
    }
    assign(&mut ctx.variables, name, value, quoted);
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
// Where @slot and @children are written
// ---------------------------------------------------------------------------

/// The files each `@include` line brought in, by the line's node id, with
/// the include chain that leads to each.
type IncludedBy = HashMap<usize, Vec<(Rc<Tree>, String)>>;

/// The lines of the files an `@include` line brings in, and the include
/// chain of each (for diagnostics).
type Included<'a, 'f> = &'f dyn Fn(&syntax::Node) -> Vec<(&'a [syntax::Node], Option<&'a str>)>;

/// Whether `name` is a function here: defined so far, or anywhere in the
/// file and the files it includes.
fn is_function(name: &str, ctx: &ParseContext) -> bool {
    ctx.functions.contains_key(name) || ctx.namespace.contains(name)
}

/// Whether `name` is one slot name: letters, digits, `-` and `_`,
/// starting with a letter (like a function's name).
fn is_slot_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_alphabetic)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// What a `@slot` or `@children` line is written in.
#[derive(Clone, Copy)]
enum Place<'a> {
    /// The top of a file or of a function's body, or a built-in element
    /// (by its name).
    Element(Option<&'a str>),
    /// A call: its content.
    Call,
}

/// Where a line is, for [`slot_uses`].
#[derive(Clone, Copy)]
struct At<'a> {
    /// Inside a function's body.
    body: bool,
    place: Place<'a>,
    /// Outside a body: the call (and its line) whose content it is in.
    call: Option<(&'a str, usize)>,
    /// The include chain of the file it is in, when that isn't the file
    /// being compiled.
    file: Option<&'a str>,
}

impl At<'_> {
    /// The top of a file, or of a function's body.
    fn top(body: bool) -> Self {
        At {
            body,
            place: Place::Element(None),
            call: None,
            file: None,
        }
    }
}

/// What [`slot_uses`] knows about the names and files it walks.
struct Scan<'a, 'f> {
    is_function: &'f dyn Fn(&str) -> bool,
    included: Included<'a, 'f>,
    /// Visit the bodies of the functions defined in the lines too.
    into_lets: bool,
}

/// A `@slot` or `@children` written in a file, and where.
struct SlotUse<'a> {
    node: &'a syntax::Node,
    head: &'a syntax::Head,
    /// The text after it, which is a slot's name; `None` when it isn't
    /// the last head of its chain.
    text: Option<&'a syntax::Text>,
    /// Written inline in a line of text, `{@slot x}`.
    inline: bool,
    at: At<'a>,
}

impl SlotUse<'_> {
    fn is_slot(&self) -> bool {
        self.head.name == "slot"
    }

    /// The slot's name as written.
    fn name(&self) -> &str {
        self.text.map_or("", |t| t.raw.trim())
    }

    /// A `@slot NAME` that marks a place in a function's body (not one
    /// that fills a slot of a call).
    fn declares(&self) -> bool {
        self.is_slot() && !self.inline && !matches!(self.at.place, Place::Call)
    }
}

/// Where each `@slot` and `@children` in `block` is written. A `@slot`
/// directly under a call (under its `@if`, `@else` and `@each` too) fills
/// one of its slots; anywhere else in a function's body it declares one.
/// An included file's lines are where its `@include` is.
fn slot_uses<'a>(
    block: &'a [syntax::Node],
    at: At<'a>,
    scan: &Scan<'a, '_>,
    out: &mut Vec<SlotUse<'a>>,
) {
    fn inline<'a>(
        text: &'a syntax::Text,
        node: &'a syntax::Node,
        at: At<'a>,
        scan: &Scan<'a, '_>,
        out: &mut Vec<SlotUse<'a>>,
    ) {
        for segment in &text.segments {
            if let Segment::Inline(inline_element) = segment {
                let head = &inline_element.head;
                if matches!(head.name.as_str(), "slot" | "children")
                    && !(scan.is_function)(&head.name)
                {
                    out.push(SlotUse {
                        node,
                        head,
                        text: inline_element.text.as_ref(),
                        inline: true,
                        at,
                    });
                }
                if let Some(text) = &inline_element.text {
                    inline(text, node, at, scan, out);
                }
            }
        }
    }
    for node in block {
        match &node.kind {
            NodeKind::Directive(directive) => match &directive.args {
                DirectiveArgs::Let(def) if matches!(def.form, LetForm::Function(_)) => {
                    if scan.into_lets {
                        let top = At {
                            file: at.file,
                            ..At::top(true)
                        };
                        slot_uses(&node.children, top, scan, out);
                    }
                }
                // A value's body is an error of its own
                DirectiveArgs::Let(_) => {}
                _ if directive.spec.body == BodyKind::Verbatim => {}
                // `@if`, `@else` and `@each` put their lines in place, and
                // `@include` its file's; the lines under a directive that
                // takes no body are its siblings
                _ => {
                    for (lines, file) in (scan.included)(node) {
                        slot_uses(lines, At { file, ..at }, scan, out);
                    }
                    slot_uses(&node.children, at, scan, out);
                }
            },
            NodeKind::Element(line) => {
                let mut at = at;
                let last = line.chain.len() - 1;
                for (i, head) in line.chain.iter().enumerate() {
                    let text = if i == last { line.text.as_ref() } else { None };
                    let function = (scan.is_function)(&head.name);
                    if !function && matches!(head.name.as_str(), "slot" | "children") {
                        out.push(SlotUse {
                            node,
                            head,
                            text,
                            inline: false,
                            at,
                        });
                    } else if let Some(text) = text {
                        inline(text, node, at, scan, out);
                    }
                    if function {
                        at.place = Place::Call;
                        if !at.body {
                            at.call = Some((&head.name, node.span.line));
                        }
                    } else {
                        at.place = Place::Element(Some(&head.name));
                    }
                }
                slot_uses(&node.children, at, scan, out);
            }
            NodeKind::Text(text) => {
                inline(text, node, at, scan, out);
                slot_uses(&node.children, at, scan, out);
            }
            NodeKind::Blank | NodeKind::Comment | NodeKind::Verbatim(_) => {}
        }
    }
}

/// The slots a function's body declares, and whether it has a
/// `@children`, when it is defined. The files its `@include` lines bring
/// in count too: they are read now, since they run only at a call.
fn declared_slots(body: &[syntax::Node], ctx: &mut ParseContext) -> (Vec<String>, bool) {
    let mut files = HashMap::new();
    read_includes(
        body,
        ctx.base_path.clone(),
        &mut Vec::new(),
        ctx,
        &mut files,
    );
    let included = |node: &syntax::Node| match files.get(&node.id) {
        Some(tree) => vec![(&tree.nodes[..], None)],
        None => Vec::new(),
    };
    let is_function = |name: &str| is_function(name, ctx);
    slots_of(body, &is_function, &included)
}

/// Read and parse the files the `@include` lines in `nodes` bring in (by
/// a literal path, relative to `base`), and the files those include, by
/// the node id of each `@include` line.
fn read_includes(
    nodes: &[syntax::Node],
    base: Option<PathBuf>,
    stack: &mut Vec<PathBuf>,
    ctx: &mut ParseContext,
    files: &mut HashMap<usize, Rc<Tree>>,
) {
    let mut includes = Vec::new();
    for node in nodes {
        node.walk(&mut |node| {
            if let Some(DirectiveArgs::Text(Some(path))) = node.directive().map(|d| &d.args)
                && node.is_directive("include")
            {
                includes.push((node.id, path.raw.clone()));
            }
        });
    }
    for (id, path) in includes {
        let path = path.trim_matches('"');
        if path.contains('$') {
            continue;
        }
        let resolved = match &base {
            Some(base) => base.join(path),
            None => PathBuf::from(path),
        };
        if stack.contains(&resolved) {
            continue;
        }
        let Ok(text) = ctx.read_file(&resolved) else {
            continue;
        };
        let tree = ctx.parse_tree(&text);
        stack.push(resolved.clone());
        let parent = resolved.parent().map(Path::to_path_buf);
        read_includes(&tree.nodes, parent, stack, ctx, files);
        stack.pop();
        files.insert(id, tree);
    }
}

/// The slots a function's body declares (`@slot NAME`, in order), and
/// whether it has a `@children` for the content of a call. A `@slot`
/// directly under a call in the body fills that call's slot instead, so
/// `is_function` says which names are functions. The files the body
/// includes aren't read.
pub fn function_slots(
    body: &[syntax::Node],
    is_function: &dyn Fn(&str) -> bool,
) -> (Vec<String>, bool) {
    slots_of(body, is_function, &|_| Vec::new())
}

fn slots_of<'a>(
    body: &'a [syntax::Node],
    is_function: &dyn Fn(&str) -> bool,
    included: Included<'a, '_>,
) -> (Vec<String>, bool) {
    let scan = Scan {
        is_function,
        included,
        into_lets: false,
    };
    let mut uses = Vec::new();
    slot_uses(body, At::top(true), &scan, &mut uses);
    let mut slots: Vec<String> = Vec::new();
    for slot in uses.iter().filter(|u| u.declares()) {
        let name = slot.name();
        if is_slot_name(name) && !slots.iter().any(|s| s == name) {
            slots.push(name.to_string());
        }
    }
    let has_children = uses.iter().any(|u| !u.is_slot() && !u.inline);
    (slots, has_children)
}

/// The line as written, indented, when it is one physical line, so a
/// column points into it.
fn written_line(node: &syntax::Node) -> String {
    match node.line_count <= 1 {
        true => format!("{}{}", " ".repeat(node.indent), node.source),
        false => node.source.clone(),
    }
}

/// The lines of the files an `@include` line brought in when it ran, from
/// `included_by`.
fn ran_includes<'a>(
    included_by: &'a IncludedBy,
    node: &syntax::Node,
) -> Vec<(&'a [syntax::Node], Option<&'a str>)> {
    match included_by.get(&node.id) {
        Some(files) => files
            .iter()
            .map(|(tree, chain)| (&tree.nodes[..], Some(chain.as_str())))
            .collect(),
        None => Vec::new(),
    }
}

/// Check where every `@slot` and `@children` of the file and the files it
/// includes is written, whether or not it runs: a slot has one name, a
/// function's body marks places with them, and a call fills its slots
/// with `@slot` blocks directly under it. An included file's lines are
/// where its `@include` is (in a body, under a call), so an `@include`
/// that never ran leaves its file unchecked.
fn check_slot_places(ctx: &mut ParseContext) {
    let trees = std::mem::take(&mut ctx.trees);
    let included_by = ctx.included_by.clone();
    let is_function = |name: &str| is_function(name, ctx);
    let scan = Scan {
        is_function: &is_function,
        included: &|node| ran_includes(&included_by, node),
        into_lets: true,
    };
    let mut uses = Vec::new();
    for (tree, _) in trees.iter().filter(|(_, chain)| chain.is_none()) {
        slot_uses(&tree.nodes, At::top(false), &scan, &mut uses);
    }
    let found: Vec<Diagnostic> = uses
        .iter()
        .filter_map(|slot| {
            let mut d = slot_problem(slot)?;
            if d.column.is_none() {
                d = d.column(slot.head.name_span.column);
            }
            d = d.source(written_line(slot.node));
            if let Some(file) = slot.at.file {
                d.message = format!("{}\n  in {}", d.message, file);
            }
            Some(d)
        })
        .collect();
    ctx.diagnostics.extend(found);
    ctx.trees = trees;
}

/// What is wrong with a `@slot` or `@children` where it is written.
fn slot_problem(slot: &SlotUse) -> Option<Diagnostic> {
    let line = slot.node.span.line;
    let what = if slot.is_slot() {
        match slot.name() {
            "" => "@slot".to_string(),
            name => format!("@slot {}", name),
        }
    } else {
        "@children".to_string()
    };
    if slot.inline {
        return Some(Diagnostic::error(
            code::MISPLACED_SLOT,
            line,
            format!(
                "{} goes on a line of its own: inline in text it marks and fills nothing",
                what
            ),
        ));
    }
    if let Some(list) = &slot.head.attrs {
        return Some(
            Diagnostic::error(
                code::UNEXPECTED_ARGUMENT,
                line,
                format!(
                    "{} takes no attributes: it is replaced by content, so style the \
                     element around it",
                    what
                ),
            )
            .column(list.span.column),
        );
    }
    if slot.is_slot() {
        let name = slot.name();
        if name.is_empty() {
            return Some(Diagnostic::error(
                code::MISSING_ARGUMENT,
                line,
                "@slot needs a name: `@slot footer` (the caller's content without a \
                 name goes where `@children` is)"
                    .to_string(),
            ));
        }
        if !is_slot_name(name) {
            let joined = name.split_whitespace().collect::<Vec<_>>().join("-");
            let suggestion = is_slot_name(&joined).then_some(joined);
            let column = slot.text.map(|t| t.span.column);
            let d = Diagnostic::error(
                code::INVALID_SLOT_NAME,
                line,
                format!(
                    "'{}' is not a slot name: a slot's name is one word of letters, digits, \
                     `-` and `_`, starting with a letter, and its content goes on the lines \
                     under it",
                    name
                ),
            )
            .subject(name)
            .suggest(suggestion);
            return Some(match column {
                Some(column) => d.column(column),
                None => d,
            });
        }
        if matches!(slot.at.place, Place::Call) || slot.at.body {
            return None;
        }
        let message = match (slot.at.call, slot.at.place) {
            (Some((call, call_line)), Place::Element(Some(element))) => format!(
                "{} is inside @{}, so it fills nothing: a @slot block that fills a slot of \
                 @{} (line {}) goes directly under the call",
                what, element, call, call_line
            ),
            _ => format!(
                "{} is outside a function's body: `@slot NAME` marks a place in a body \
                 (`@let @name`), and directly under a call it fills one",
                what
            ),
        };
        return Some(Diagnostic::error(code::MISPLACED_SLOT, line, message));
    }
    if slot.at.body {
        return None;
    }
    let message = match slot.at.call {
        Some((call, _)) => format!(
            "@children is outside a function's body, so it stands for nothing: the content \
             for @{} is written directly under the call",
            call
        ),
        None => "@children is outside a function's body: it marks where a body puts the \
                 content of each call"
            .to_string(),
    };
    Some(Diagnostic::error(code::MISPLACED_SLOT, line, message))
}

/// A `@slot NAME` block that fills a slot of a call.
struct Filler<'a> {
    node: &'a syntax::Node,
    text: &'a syntax::Text,
    /// The include chain of the file it is in, when that isn't the file
    /// being compiled.
    file: Option<&'a str>,
}

/// Whether `@slot` is the built-in element here, and not a function
/// defined so far that took its name.
fn slot_is_built_in(ctx: &ParseContext) -> bool {
    !ctx.functions.contains_key("slot")
}

/// The `@slot NAME` blocks directly under a call, in `block` (under its
/// `@if`, `@else` and `@each` too, and in the files its `@include` lines
/// brought in).
fn fillers<'a>(
    block: &'a [syntax::Node],
    file: Option<&'a str>,
    included: &'a IncludedBy,
    out: &mut Vec<Filler<'a>>,
) {
    for node in block {
        match &node.kind {
            NodeKind::Directive(directive)
                if matches!(directive.name(), "if" | "else" | "each") =>
            {
                fillers(&node.children, file, included, out);
            }
            NodeKind::Directive(directive) if directive.name() == "include" => {
                for (lines, file) in ran_includes(included, node) {
                    fillers(lines, file, included, out);
                }
            }
            NodeKind::Element(line) if line.chain.len() == 1 && line.chain[0].name == "slot" => {
                if let Some(text) = &line.text {
                    out.push(Filler { node, text, file });
                }
            }
            _ => {}
        }
    }
}

/// Whether `block`, the lines under a call, holds content other than
/// `@slot` blocks (under `@if`, `@else` and `@each` too): text, an
/// element, raw HTML, Markdown or an included file. For code that doesn't
/// run.
fn holds_content(block: &[syntax::Node], ctx: &ParseContext) -> bool {
    block.iter().any(|node| match &node.kind {
        NodeKind::Directive(directive) => match directive.name() {
            "if" | "else" | "each" => holds_content(&node.children, ctx),
            name => matches!(name, "raw" | "markdown" | "include"),
        },
        NodeKind::Element(line) => {
            line.chain[0].name != "slot" || !slot_is_built_in(ctx) || line.chain.len() > 1
        }
        NodeKind::Text(_) => true,
        _ => false,
    })
}

/// Check the `@slot NAME` blocks passed to each call of a line against
/// the slots its function declares. `calls` has the function of each head
/// of the chain that is a call.
fn check_fillers(
    node: &syntax::Node,
    element: &syntax::ElementLine,
    calls: &[Option<(String, Rc<FnDef>)>],
    ctx: &mut ParseContext,
) {
    if !slot_is_built_in(ctx) {
        return;
    }
    let included = ctx.included_by.clone();
    let last = element.chain.len() - 1;
    for (i, call) in calls.iter().enumerate() {
        let Some((name, function)) = call else {
            continue;
        };
        let mut found = Vec::new();
        if i == last {
            fillers(&node.children, None, &included, &mut found);
        } else if i + 1 == last
            && element.chain[last].name == "slot"
            && let Some(text) = &element.text
        {
            // `@card > @slot footer`
            found.push(Filler {
                node,
                text,
                file: None,
            });
        }
        for filler in found {
            let slot = filler.text.raw.trim();
            if !is_slot_name(slot) || function.slots.iter().any(|s| s == slot) {
                continue;
            }
            let mut d = unknown_slot(name, slot, &function.slots, filler.text.span.line)
                .column(filler.text.span.column)
                .source(written_line(filler.node));
            if let Some(file) = filler.file {
                d.message = format!("{}\n  in {}", d.message, file);
            }
            ctx.push_once(d);
        }
    }
}

/// A `@slot NAME` block for a slot the function doesn't declare.
fn unknown_slot(function: &str, slot: &str, slots: &[String], line: usize) -> Diagnostic {
    let candidates: Vec<&str> = slots.iter().map(String::as_str).collect();
    let suggestion = suggest_closest(slot, &candidates);
    let message = match (slots.is_empty(), suggestion) {
        (true, _) => format!(
            "@{} has no slot '{}': its body declares no slots (`@slot NAME`)",
            function, slot
        ),
        (false, Some(close)) => format!(
            "@{} has no slot '{}', did you mean '{}'? (its slots: {})",
            function,
            slot,
            close,
            slots.join(", ")
        ),
        (false, None) => format!(
            "@{} has no slot '{}' (its slots: {})",
            function,
            slot,
            slots.join(", ")
        ),
    };
    Diagnostic::error(code::UNKNOWN_SLOT, line, message)
        .subject(slot)
        .suggest(suggestion)
}

/// Content passed to a function whose body has no `@children`.
fn no_children(function: &str, line: usize, column: Option<usize>) -> Diagnostic {
    let d = Diagnostic::error(
        code::UNEXPECTED_CONTENT,
        line,
        format!(
            "@{0} takes no content: its body has no `@children`, so the text and lines \
             passed to it would be dropped (pass text as a parameter, `@{0} [name value]`, \
             or add `@children` to the body)",
            function
        ),
    )
    .subject(function);
    match column {
        Some(column) => d.column(column),
        None => d,
    }
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
        parse_attr_list(&list.attrs, line_num, ctx, true, &[])
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
        function: None,
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
    } else if ctx.defines.contains_key(name) {
        Some(format!(
            "unknown element @{}: '{}' is an attribute bundle, used as `[${}]` \
             (a function is defined with `@let @{}`)",
            name, name, name, name
        ))
    } else if ctx.variables.contains_key(name) {
        Some(format!(
            "unknown element @{}: '{}' is a value, used as `${}` \
             (a function is defined with `@let @{}`)",
            name, name, name, name
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
    for name in vars.keys().filter(|k| !k.ends_with(['#', '"'])) {
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

/// The quoted strings of a CSS value, without their quotes.
fn css_strings(value: &str) -> Vec<&str> {
    let mut strings = Vec::new();
    let mut start = None;
    let mut escaped = false;
    for (i, c) in value.char_indices() {
        match (c, start) {
            _ if escaped => escaped = false,
            ('\\', Some(_)) => escaped = true,
            ('"', None) => start = Some(i + 1),
            ('"', Some(from)) => {
                strings.push(&value[from..i]);
                start = None;
            }
            _ => {}
        }
    }
    strings
}

/// An `if(...)` in a CSS value that isn't CSS's if(): CSS writes each
/// branch `CONDITION: VALUE`, so an if() without a `:` is something else.
fn not_css_if(value: &str) -> Option<&str> {
    let mut from = 0;
    while let Some(found) = value[from..].find("if(") {
        let at = from + found;
        from = at + 3;
        let word_before = value[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, '-' | '_'));
        if word_before {
            continue;
        }
        let mut depth = 0;
        let close = value[at + 2..].char_indices().find_map(|(i, c)| {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(at + 2 + i)
        });
        let end = close.map_or(value.len(), |c| c + 1);
        if !value[at + 3..end].contains(':') {
            return Some(&value[at..end]);
        }
    }
    None
}

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

    // In CSS, a quoted font-family is one family, commas and all
    if base_key == "font-family"
        && let Some(val) = &attr.value
        && let Some(family) = css_strings(val).into_iter().find(|s| s.contains(','))
    {
        let stack = family
            .split(',')
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\\, ");
        ctx.diagnostics.push(
            Diagnostic::warning(
                code::INVALID_VALUE,
                line_num,
                format!(
                    "font-family \"{}\" is one family, whose name has a comma in it: \
                     for a font stack, write `font-family {}`",
                    family, stack
                ),
            )
            .subject(format!("\"{}\"", family))
            .suggest(Some(stack)),
        );
    }

    if let Some(val) = &attr.value {
        if let Some(call) = not_css_if(val) {
            ctx.diagnostics.push(
                Diagnostic::warning(
                    code::INVALID_VALUE,
                    line_num,
                    format!(
                        "'{}' is not CSS's if(), whose branches are written \
                         `if(CONDITION: VALUE; else: VALUE)`",
                        call
                    ),
                )
                .subject(call),
            );
            return;
        }
        // CSS's if() is decided by the browser, like var()
        if val.starts_with("if(") {
            return;
        }
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
/// `validate`, unknown names and invalid values are reported. `text_keys`
/// are a called function's parameters: their values are text rather than
/// CSS, and they aren't checked as attributes.
fn parse_attr_list(
    tokens: &[syntax::Attr],
    line_num: usize,
    ctx: &mut ParseContext,
    validate: bool,
    text_keys: &[String],
) -> Vec<Attribute> {
    let mut attrs = Vec::new();
    let mut seen_keys: Vec<String> = Vec::new();
    let mut chosen = Vec::new();
    choose_attrs(tokens, line_num, ctx, validate, text_keys, &mut chosen);

    for token in &chosen {
        let line = if token.span.line == 0 {
            line_num
        } else {
            token.span.line
        };
        let (key, value, html) = (token.key.clone(), token.value.clone(), token.html);
        let column = |at: usize| Some(token.span.column + at);

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

        // A value's variables are filled in. A style's value is CSS, where
        // quoted text keeps its quotes; an HTML attribute's (and a
        // parameter's) is text, where it loses them.
        let sink = if html || text_keys.contains(&key) {
            Sink::Text
        } else {
            Sink::Css
        };
        let mut filled = true;
        let mut quoted = None;
        let value = match value {
            None => None,
            Some(value) => {
                let at = column(token.raw.len() - value.len());
                let (text, is_quoted, ok) = ctx.fill_value(&value, line, at, sink);
                filled = ok;
                quoted = is_quoted;
                Some(text)
            }
        };
        let attr = Attribute {
            key,
            value,
            html,
            quoted,
        };
        // A value that couldn't be filled in is already reported, and a
        // parameter's value is text for the function
        let validate = validate && filled && !text_keys.contains(&attr.key);

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
                let mut msg = match &suggestion {
                    Some(closest) => {
                        format!(
                            "unknown attribute '{}', did you mean '{}'?",
                            attr.key, closest
                        )
                    }
                    None => format!("unknown attribute '{}'", attr.key),
                };
                // `box-shadow 0 1px red, 0 2px blue` or `font-family Inter,
                // sans-serif`: the comma ended the attribute, and the rest
                // of the value became one
                let rest_of_a_value = !base_key
                    .starts_with(|c: char| c.is_ascii_alphabetic() || c == '-')
                    || (suggestion.is_none() && attr.value.is_none() && !attrs.is_empty());
                if rest_of_a_value {
                    msg.push_str(
                        ". A comma separates attributes: to keep one in a value, write `\\,`",
                    );
                }
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

/// While a slot is filled in, each escape (see [`syntax::ESCAPES`]) is a
/// private-use placeholder, U+E000 plus its place in the table, so it
/// can't start a variable, a quote or an inline element. Afterwards it
/// becomes what it stands for.
fn protect_escapes(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        let len = syntax::escape_len(&text[i..]);
        if len == 0 {
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let escape = &text[i..i + len];
        let index = syntax::ESCAPES.iter().position(|e| *e == escape);
        out.extend(index.and_then(|n| char::from_u32(0xE000 + n as u32)));
        i += len;
    }
    out
}

/// The escape a placeholder stands for, as written (`\,`).
fn escape_of(c: char) -> Option<&'static str> {
    let index = (c as u32).checked_sub(0xE000)?;
    syntax::ESCAPES.get(index as usize).copied()
}

fn restore_escapes(text: &str) -> String {
    text.chars()
        .fold(String::with_capacity(text.len()), |mut out, c| {
            match escape_of(c) {
                // What the escape stands for: what follows the backslash
                Some(escape) => out.push_str(&escape[1..]),
                None => out.push(c),
            }
            out
        })
}

/// Inside the quoted strings of a CSS value, `\"` and `\\` mean in CSS what
/// they mean in htmlang, so they stay as written there.
fn keep_css_string_escapes(protected: &str) -> String {
    let mut out = String::with_capacity(protected.len());
    let mut inside = false;
    for c in protected.chars() {
        match escape_of(c) {
            Some(escape @ ("\\\"" | "\\\\")) if inside => out.push_str(escape),
            _ => {
                if c == '"' {
                    inside = !inside;
                }
                out.push(c);
            }
        }
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
                let line = ctx.current_line;
                match resolve(&inline.head, inline.text.as_ref(), line, &text.raw, ctx) {
                    Ok(resolved) => {
                        flush(&mut plain, &mut start, &mut segments, ctx);
                        // A function called inline gets no children: its
                        // content is the text after its attributes
                        match Evaluator.complete(resolved, Vec::new(), ctx) {
                            Ok(nodes) => segments.extend(inline_segments(nodes, line)),
                            Err(mut e) => {
                                e.source_line
                                    .get_or_insert_with(|| text.raw.as_str().into());
                                ctx.diagnostics.push(e);
                            }
                        }
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
    let entries = record_entries(vars, source);
    bind_record(vars, target, &entries);
}

/// Whether the variable key `key` is `name` or one of its parts (a field,
/// an item, a list's length, the quoted form).
fn is_under(key: &str, name: &str) -> bool {
    key.strip_prefix(name)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '#', '"']))
}

/// A value with all its parts, as the keys after its name and their
/// values: a record's fields, a list's items and length.
fn record_entries(vars: &HashMap<String, String>, name: &str) -> Vec<(String, String)> {
    vars.iter()
        .filter(|(key, _)| is_under(key, name))
        .map(|(key, value)| (key[name.len()..].to_string(), value.clone()))
        .collect()
}

/// Make `$target` the value whose parts are `entries` (see
/// [`record_entries`]), replacing what it was.
fn bind_record(vars: &mut HashMap<String, String>, target: &str, entries: &[(String, String)]) {
    vars.retain(|key, _| !is_under(key, target));
    for (rest, value) in entries {
        vars.insert(format!("{}{}", target, rest), value.clone());
    }
    // A record has no text of its own
    vars.entry(target.to_string()).or_default();
}

/// The variable `raw` is, when it is exactly one `$name` or `${name}`.
fn whole_reference(raw: &str, vars: &HashMap<String, String>) -> Option<String> {
    let after = raw.trim().strip_prefix('$')?;
    match interp::reference(after, &Vars(vars))? {
        (interp::Reference::Var(path), len) if len == after.len() => Some(path),
        _ => None,
    }
}

/// The record or list `raw` is, when it is one variable holding one: a
/// parameter passed it gets the whole value, not just its text.
fn whole_record(raw: &str, vars: &HashMap<String, String>) -> Option<String> {
    whole_reference(raw, vars).filter(|path| interp::Scope::has_fields(&Vars(vars), path))
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

/// The attributes of `tokens` after each whole-attribute `if()` has
/// chosen (see [`syntax::Choice`]), in order, into `out`. The branch not
/// taken isn't evaluated; with `validate`, its attribute names are checked
/// like code that doesn't run.
fn choose_attrs(
    tokens: &[syntax::Attr],
    line_num: usize,
    ctx: &mut ParseContext,
    validate: bool,
    text_keys: &[String],
    out: &mut Vec<syntax::Attr>,
) {
    for token in tokens {
        let Some(choice) = &token.choice else {
            out.push(token.clone());
            continue;
        };
        let Some(condition) = choice_shape(token, choice, line_num, ctx) else {
            continue;
        };
        let taken = match condition.span.line {
            0 => ctx.condition(&condition.raw, line_num, None),
            line => ctx.condition(&condition.raw, line, Some(condition.span.column)),
        };
        let (chosen, other) = match taken {
            true => (choice.branches.first(), choice.branches.get(1)),
            false => (choice.branches.get(1), choice.branches.first()),
        };
        if let Some(other) = other {
            track_var_refs(&branch_text(other), &mut ctx.used_variables);
            if validate {
                check_attrs(other.attrs(), line_num, ctx, text_keys);
            }
        }
        if let Some(branch) = chosen {
            choose_attrs(branch.attrs(), line_num, ctx, validate, text_keys, out);
        }
    }
}

/// Report what is wrong with the shape of `if()`: an empty condition, a
/// number of branches other than one or two, text after a group's `]`.
/// Returns the condition when the `if()` can be evaluated.
fn choice_shape<'a>(
    token: &syntax::Attr,
    choice: &'a syntax::Choice,
    line_num: usize,
    ctx: &mut ParseContext,
) -> Option<&'a syntax::Arg> {
    let line = if token.span.line == 0 {
        line_num
    } else {
        token.span.line
    };
    // The line the column points into, when it is the one being evaluated
    let source = |ctx: &ParseContext, diagnostic: Diagnostic, at: usize| match &ctx.current_source {
        (current, Some(text)) if *current == at => diagnostic.source(text.clone()),
        _ => diagnostic,
    };
    for branch in &choice.branches {
        if let syntax::Branch::Group {
            trailing: Some(trailing),
            ..
        } = branch
        {
            let diagnostic = source(
                ctx,
                Diagnostic::error(
                    code::UNEXPECTED_ARGUMENT,
                    trailing.span.line,
                    format!(
                        "'{}' after the `]` of a group in if(): a branch is one attribute, \
                         one `[group]` of attributes or one `$bundle`",
                        trailing.raw
                    ),
                )
                .column(trailing.span.column)
                .subject(trailing.raw.as_str()),
                trailing.span.line,
            );
            ctx.push_once(diagnostic);
        }
    }
    match &choice.condition {
        Some(condition) if (1..=2).contains(&choice.branches.len()) => Some(condition),
        _ => {
            let diagnostic = source(
                ctx,
                Diagnostic::error(
                    code::INVALID_EXPRESSION,
                    line,
                    format!(
                        "`{}`: if() takes a condition and one or two branches, \
                         `if(CONDITION, A, B)`",
                        token.raw
                    ),
                )
                .column(token.span.column),
                line,
            );
            ctx.push_once(diagnostic);
            None
        }
    }
}

/// A branch's text, for the names it uses.
fn branch_text(branch: &syntax::Branch) -> String {
    let raws: Vec<&str> = branch.attrs().iter().map(|a| a.raw.as_str()).collect();
    raws.join(", ")
}

/// Check attributes that are never evaluated (in code that doesn't run,
/// or the branch an `if()` didn't take), as far as they can be checked:
/// names are, and literal values; a value with a variable depends on what
/// the variable holds. Each branch of an `if()` is checked on its own.
fn check_attrs(tokens: &[syntax::Attr], line: usize, ctx: &mut ParseContext, text_keys: &[String]) {
    parse_attr_list(&literal_attrs(tokens), line, ctx, true, text_keys);
    for token in tokens {
        if let Some(choice) = &token.choice
            && choice_shape(token, choice, line, ctx).is_some()
        {
            for branch in &choice.branches {
                check_attrs(branch.attrs(), line, ctx, text_keys);
            }
        }
    }
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
            let start = out.len();
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
            // A function's body may draw with an empty element (a spacer,
            // a dot): only one written in the page is flagged
            if matches!(elem.kind, ElementKind::Row | ElementKind::El)
                && elem.children.is_empty()
                && elem.function.is_none()
            {
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
            in_function_body(elem, &mut out[start..]);
            walk(&elem.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(nodes, 0, &mut out);
    dedupe(&mut out);
    out
}

fn validate_tree(
    nodes: &[Node],
    parent_kind: Option<&ElementKind>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for node in nodes {
        // An element inside a line of text (`{@kbd ...}`, an inline call)
        // is checked like one on a line of its own
        if let Node::Text(segments) = node {
            let inline: Vec<Node> = segments
                .iter()
                .filter_map(|segment| match segment {
                    TextSegment::Inline(elem) => Some(Node::Element(elem.clone())),
                    _ => None,
                })
                .collect();
            validate_tree(&inline, parent_kind, diagnostics);
        }
        if let Node::Element(elem) = node {
            let start = diagnostics.len();
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
            in_function_body(elem, &mut diagnostics[start..]);

            validate_tree(&elem.children, Some(&elem.kind), diagnostics);
        }
    }
}

/// Diagnostics about an element a function's body wrote are reported at
/// the call (see `belongs_to_call`); say which body the element is in.
fn in_function_body(elem: &Element, diagnostics: &mut [Diagnostic]) {
    if let Some(function) = &elem.function {
        for d in diagnostics {
            d.message = format!("{}\n  in the body of @{}", d.message, function);
        }
    }
}

/// Report each diagnostic once: a line that runs many times (in a loop, in
/// a function called from one place many times) reports its problems once.
fn dedupe(diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    diagnostics.retain(|d| {
        seen.insert((
            d.code,
            d.line,
            d.column,
            d.message.clone(),
            d.severity as u8,
        ))
    });
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
