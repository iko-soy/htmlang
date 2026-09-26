use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::ast::*;
use crate::vocab::with_px;

// ---------------------------------------------------------------------------
// Style collector: deduplicates CSS and assigns class names
// ---------------------------------------------------------------------------

/// (min-width breakpoint, prefix)
const BREAKPOINTS: &[(&str, &str)] = &[
    ("sm", "640px"),
    ("md", "768px"),
    ("lg", "1024px"),
    ("xl", "1280px"),
    ("2xl", "1536px"),
];

/// The prefix of every generated class (`hl-a`, `hl-b`, ...), which keeps
/// them out of the author's own class names.
pub const CLASS_PREFIX: &str = "hl-";

/// Generate short CSS class names: a..z, then aa..a9, ba..b9, ..., z9, then
/// aaa, ... The first character is always a letter; later ones are drawn from
/// [a-z0-9]. The mapping is a bijection, so distinct indices never collide.
pub(crate) fn short_class_name(idx: usize) -> String {
    const REST: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    // Find the name length: 26 names of length 1, 26*36 of length 2, ...
    let mut n = idx;
    let mut len = 1;
    let mut count = 26usize;
    while n >= count {
        n -= count;
        len += 1;
        count = count.saturating_mul(36);
    }
    let mut tail = Vec::with_capacity(len - 1);
    for _ in 1..len {
        tail.push(REST[n % 36]);
        n /= 36;
    }
    let mut name = String::with_capacity(len);
    name.push((b'a' + n as u8) as char);
    name.extend(tail.iter().rev().map(|&b| b as char));
    name
}

/// When a style applies: the at-rule prefixes it is under (their ranks,
/// see [`crate::vocab::at_rule_rank`], sorted, so `dark:md:` and `md:dark:`
/// are one condition) and its selector prefixes, left to right as the
/// selector reads (`hover:children:` is `.a:hover > *`).
#[derive(Clone, Default, PartialEq, Eq, Hash, Debug)]
struct Condition {
    at: Vec<usize>,
    selector: Vec<String>,
}

impl Condition {
    /// The condition of an attribute's key, and the name after its
    /// prefixes.
    fn of(key: &str) -> (Condition, &str) {
        let (prefixes, name) = crate::vocab::split_prefixes(key);
        let mut condition = Condition::default();
        for prefix in prefixes {
            match crate::vocab::at_rule_rank(prefix) {
                Some(rank) => condition.at.push(rank),
                None => condition.selector.push(prefix.to_string()),
            }
        }
        condition.at.sort_unstable();
        condition.at.dedup();
        (condition, name)
    }

    /// Only the at-rule part: a direction under `hover:` is the one without
    /// it.
    fn at_only(at: &[usize]) -> Condition {
        Condition {
            at: at.to_vec(),
            selector: Vec::new(),
        }
    }

    /// The styles go on the element's children (`children:`).
    fn on_children(&self) -> bool {
        self.selector.iter().any(|p| p == crate::vocab::CHILDREN)
    }

    /// The styles go on the elements of one kind inside the element
    /// (`@td:`): where the element prefix is in the selector chain.
    fn inside(&self) -> Option<usize> {
        self.selector
            .iter()
            .position(|p| crate::vocab::element_prefix(p).is_some())
    }

    /// Whether this condition holds wherever `other` does: its at-rules
    /// are implied by `other`'s, and it has no selector or `other`'s.
    fn holds_at(&self, other: &Condition) -> bool {
        (self.selector.is_empty() || self.selector == other.selector)
            && self.at.iter().all(|&a| {
                other
                    .at
                    .iter()
                    .any(|&b| crate::vocab::at_rule_implied(a, b))
            })
    }

    /// The order the rules of each condition are written in, which the
    /// cascade follows: no prefix, then selectors, then each at-rule
    /// block (see [`at_order`]), with its selectors inside it.
    fn order(&self) -> (AtOrder, SelectorOrder) {
        (at_order(&self.at), selector_order(&self.selector))
    }
}

/// Where a block of at-rules is written: after the rules without one; by
/// its last (innermost) prefix in the order of
/// [`crate::vocab::at_rule_prefixes`] (widths, media, container widths), so
/// `md:dark:` comes with `dark:`, after it; then by length.
type AtOrder = (bool, usize, usize, Vec<usize>);

fn at_order(at: &[usize]) -> AtOrder {
    (
        !at.is_empty(),
        at.last().copied().unwrap_or(0),
        at.len(),
        at.to_vec(),
    )
}

/// Selector chains in the order of [`crate::vocab::PSEUDOS`] (position,
/// then form state, then `hover:`, `focus:`, `active:`, `disabled:`), and
/// by their argument's text for the same pseudo-class; a chain before the
/// longer ones it starts. A chain through `children:` comes after the
/// others, ordered by what follows its last `children:` and then by what
/// comes before it: only that tail counts toward its specificity (see
/// [`selector`]), so `hover:children:` follows `children:` and wins over it
/// on hover.
type SelectorOrder = (bool, Vec<(usize, String)>, Vec<(usize, String)>);

fn selector_order(selector: &[String]) -> SelectorOrder {
    let table = crate::vocab::PSEUDOS;
    let ranked = |chain: &[String]| -> Vec<(usize, String)> {
        chain
            .iter()
            .map(|p| {
                let name = crate::vocab::pseudo_name(p);
                let rank = table
                    .iter()
                    .position(|(q, _)| *q == name)
                    .unwrap_or(table.len());
                (rank, p.clone())
            })
            .collect()
    };
    match selector.iter().rposition(|p| p == crate::vocab::CHILDREN) {
        None => (false, ranked(selector), Vec::new()),
        Some(i) => (true, ranked(&selector[i + 1..]), ranked(&selector[..i])),
    }
}

/// The order of the chains through an element prefix (`@td:`, see
/// [`scoped_selector`]): by the prefixes before it, which select the root
/// of the scope and add nothing to the specificity, so `hover:@td:` comes
/// after `@td:` and wins on hover; then by the element; then by the
/// prefixes after it.
fn inside_order(chain: &[String]) -> (SelectorOrder, String, SelectorOrder) {
    let at = chain
        .iter()
        .position(|p| crate::vocab::element_prefix(p).is_some())
        .unwrap_or(chain.len());
    (
        selector_order(&chain[..at]),
        chain.get(at).cloned().unwrap_or_default(),
        selector_order(chain.get(at + 1..).unwrap_or_default()),
    )
}

/// The selector for `class` under a chain of selector prefixes, read left
/// to right. Everything before the last `children:` goes in `:where()`, so
/// a parent's `children:` styles have no specificity, and a child's own
/// attributes win over them: `children:hover:` is `:where(.a)>*:hover`,
/// `hover:children:` is `:where(.a:hover)>*`.
fn selector(class: &str, chain: &[String]) -> String {
    let last_children = chain.iter().rposition(|p| p == crate::vocab::CHILDREN);
    let mut out = format!(".{}", class);
    for (i, prefix) in chain.iter().enumerate() {
        if prefix == crate::vocab::CHILDREN {
            if Some(i) == last_children {
                out = format!(":where({})>*", out);
            } else {
                out.push_str(">*");
            }
        } else if let Some(pseudo) = crate::vocab::pseudo_selector(prefix) {
            out.push_str(&pseudo);
        }
    }
    out
}

/// The rule for the elements an element prefix picks out inside the
/// element with the class `class` (see [`Condition::inside`]): the scope's
/// root, from the selector prefixes before it (`hover:@td:` is inside a
/// hovered element), and the selector for the elements, from those after
/// it (`@td:hover:` is a hovered cell). `:scope td` leaves out the root
/// itself, so `@ul [@ul:...]` styles the lists inside, not its own. `None`
/// when the name has no tag of its own (the parser reports it).
fn scoped_selector(class: &str, chain: &[String], at: usize) -> Option<(String, String)> {
    let name = crate::vocab::element_prefix(&chain[at])?;
    let tag = ElementKind::from_name(name)?.own_tag()?;
    let root = selector(class, &chain[..at]);
    let mut target = format!(":scope {}", tag);
    for prefix in &chain[at + 1..] {
        if let Some(pseudo) = crate::vocab::pseudo_selector(prefix) {
            target.push_str(&pseudo);
        }
    }
    Some((root, target))
}

/// The `@media` or `@container` rule an at-rule prefix stands for, as
/// written in readable and in compact output.
fn at_rule(rank: usize, dev: bool) -> String {
    let prefix = crate::vocab::at_rule_prefixes()
        .nth(rank)
        .unwrap_or_default();
    let name = prefix.trim_end_matches(':');
    let width = |name: &str| {
        BREAKPOINTS
            .iter()
            .find(|(n, _)| *n == name)
            .map_or("0", |(_, w)| *w)
    };
    let (kind, feature, value) = if let Some(size) = name.strip_prefix("cq-") {
        ("@container", "min-width", width(size))
    } else {
        match name {
            "print" => {
                return "@media print".to_string();
            }
            "dark" => ("@media", "prefers-color-scheme", "dark"),
            "motion-safe" => ("@media", "prefers-reduced-motion", "no-preference"),
            "motion-reduce" => ("@media", "prefers-reduced-motion", "reduce"),
            "landscape" | "portrait" => ("@media", "orientation", name),
            _ => ("@media", "min-width", width(name)),
        }
    };
    if dev {
        format!("{} ({}: {})", kind, feature, value)
    } else {
        format!("{}({}:{})", kind, feature, value)
    }
}

struct StyleEntry {
    class_name: String,
    /// The defaults of the element's kind (its layout, a heading's
    /// `margin:0`, ...), less what its own attributes set. They are written
    /// as `:where(.hl-a)`, with no specificity, so a parent's `children:`
    /// styles win over them, as its attributes win over those.
    defaults: String,
    /// The element's rules: under each condition, its declarations, in the
    /// order they are written
    rules: Vec<(Condition, String)>,
    /// What else tells two elements with the same CSS apart: the rules
    /// keyed on this class (see [`StyleCollector::keyed`]). Not written.
    distinct: String,
}

struct StyleCollector {
    entries: Vec<StyleEntry>,
    /// Maps a pre-hashed style signature to an index into `entries`.
    /// Using u64 as the key keeps lookups allocation-free; on the rare case of
    /// a hash collision we fall back to a full equality check against the entry.
    index: HashMap<u64, Vec<usize>>,
    /// Rules for an element's children, keyed on its class and written in
    /// the block of an at-rule condition: `(at-rules, selector, body)`, e.g.
    /// `([md], ":where(.a)>.b", "flex:1;")` (see [`Flow`]).
    keyed: Vec<(Vec<usize>, String, String)>,
    /// The keyed rules already added
    keyed_seen: std::collections::HashSet<(Vec<usize>, String, String)>,
}

impl StyleCollector {
    fn new() -> Self {
        StyleCollector {
            entries: Vec::new(),
            index: HashMap::new(),
            keyed: Vec::new(),
            keyed_seen: std::collections::HashSet::new(),
        }
    }

    /// Add a rule for `selector` under the at-rules `at`, once.
    fn add_keyed(&mut self, at: Vec<usize>, selector: String, body: String) {
        let rule = (at, selector, body);
        if self.keyed_seen.insert(rule.clone()) {
            self.keyed.push(rule);
        }
    }

    /// Returns a class name for this style combination, or None if all empty.
    fn get_class(
        &mut self,
        defaults: String,
        mut rules: Vec<(Condition, String)>,
        distinct: String,
    ) -> Option<String> {
        rules.retain(|(_, body)| !body.is_empty());
        if defaults.is_empty() && rules.is_empty() {
            return None;
        }
        rules.sort_by_cached_key(|(condition, _)| condition.order());
        use std::collections::hash_map::DefaultHasher;
        let mut h = DefaultHasher::new();
        defaults.hash(&mut h);
        rules.hash(&mut h);
        distinct.hash(&mut h);
        let sig = h.finish();

        if let Some(indices) = self.index.get(&sig) {
            for &idx in indices {
                let e = &self.entries[idx];
                if e.defaults == defaults && e.rules == rules && e.distinct == distinct {
                    return Some(e.class_name.clone());
                }
            }
        }
        let idx = self.entries.len();
        let name = format!("{}{}", CLASS_PREFIX, short_class_name(idx));
        self.entries.push(StyleEntry {
            class_name: name.clone(),
            defaults,
            rules,
            distinct,
        });
        self.index.entry(sig).or_default().push(idx);
        Some(name)
    }

    /// All generated rules, in three layers (see [`LAYERS`]): the element
    /// kinds' defaults in `hl-kind` (`:where(.hl-a)`), the rules for the
    /// elements an element prefix picks out inside an element (`@td:`), and
    /// a function's scoped `@style`, in `hl-inside`, and the elements' own
    /// rules in `htmlang`. They are written in a fixed order, so which rule
    /// wins never depends on where in the page an element is: the rules
    /// without a prefix, then those under selector prefixes, then each block
    /// of at-rules (see [`at_order`]), in which the rules keyed on a parent
    /// come first, then the element's own, then its selectors (see
    /// [`selector_order`]).
    fn to_css_formatted(&self, dev: bool, scoped: &[String]) -> String {
        // The defaults: `:where(.hl-a,.hl-b){...}`
        let mut kind = String::new();
        let mut defaults: Vec<(&str, Vec<&str>)> = Vec::new();
        for e in self.entries.iter().filter(|e| !e.defaults.is_empty()) {
            match defaults.iter_mut().find(|(body, _)| *body == e.defaults) {
                Some((_, classes)) => classes.push(&e.class_name),
                None => defaults.push((&e.defaults, vec![&e.class_name])),
            }
        }
        for (body, classes) in defaults {
            let classes: Vec<String> = classes.iter().map(|c| format!(".{}", c)).collect();
            let selector = format!(":where({})", classes.join(","));
            if dev {
                kind.push_str(&format!("  {} {{{}}}\n", selector, body));
            } else {
                kind.push_str(&format!("{}{{{}}}", selector, body));
            }
        }

        let mut inside = self.blocks(true, dev);
        // A function's scoped `@style`, nested under its class
        for block in scoped {
            if dev {
                inside.push_str(block);
                inside.push('\n');
            } else {
                let minified: String = block.lines().map(str::trim).collect();
                inside.push_str(&minified);
            }
        }

        let mut css = String::new();
        for (name, rules) in [
            ("hl-kind", kind),
            ("hl-inside", inside),
            ("htmlang", self.blocks(false, dev)),
        ] {
            if rules.is_empty() {
                continue;
            }
            if dev {
                css.push_str(&format!("@layer {} {{\n{}}}\n", name, rules));
            } else {
                css.push_str(&format!("@layer {}{{{}}}", name, rules));
            }
        }
        css
    }

    /// The rules of every block of at-rules, in order: those for the
    /// elements inside (`inside`: rules under an element prefix, each in
    /// an `@scope` on its element's class), or the elements' own.
    fn blocks(&self, inside: bool, dev: bool) -> String {
        let mut css = String::new();
        let rules = || {
            self.entries.iter().flat_map(move |e| {
                e.rules
                    .iter()
                    .filter(move |(c, _)| c.inside().is_some() == inside)
                    .map(move |(c, body)| (e, c, body))
            })
        };
        let mut blocks: Vec<&[usize]> = vec![&[]];
        let keyed = self.keyed.iter().filter(|_| !inside);
        let every = rules()
            .map(|(_, c, _)| c.at.as_slice())
            .chain(keyed.map(|(at, _, _)| at.as_slice()));
        for at in every {
            if !blocks.contains(&at) {
                blocks.push(at);
            }
        }
        blocks.sort_by_key(|at| at_order(at));

        for at in blocks {
            let depth = at.len();
            // Readable output indents a rule inside `@layer` and inside
            // each at-rule but the first
            let indent = if dev {
                "  ".repeat(depth.max(1))
            } else {
                String::new()
            };
            let mut inner = String::new();
            // Rules keyed on a parent's class come first: they have the
            // specificity of one class, like a class rule, so the child's
            // own rules in the block still win
            if !inside {
                let keyed: Vec<(&str, &str)> = self
                    .keyed
                    .iter()
                    .filter(|(a, _, _)| a.as_slice() == at)
                    .map(|(_, selector, body)| (selector.as_str(), body.as_str()))
                    .collect();
                emit_selector_rules(&mut inner, &keyed, &indent, dev);
            }
            // The selector chains of this block, in order
            let mut chains: Vec<&[String]> = Vec::new();
            for (_, condition, _) in rules() {
                if condition.at == at && !chains.contains(&condition.selector.as_slice()) {
                    chains.push(&condition.selector);
                }
            }
            if inside {
                chains.sort_by_cached_key(|chain| inside_order(chain));
                // One `@scope` for each root, its rules in the order of
                // the chains: rules for one element under two roots (`.a`
                // and `.a:hover`) tie on specificity and proximity, so the
                // later one wins
                let mut scopes: Vec<(String, Vec<(String, &str)>)> = Vec::new();
                for chain in chains {
                    let Some(element) = chain
                        .iter()
                        .position(|p| crate::vocab::element_prefix(p).is_some())
                    else {
                        continue;
                    };
                    for (e, _, body) in
                        rules().filter(|(_, c, _)| c.at == at && c.selector == chain)
                    {
                        let Some((root, target)) = scoped_selector(&e.class_name, chain, element)
                        else {
                            continue;
                        };
                        match scopes.iter_mut().find(|(r, _)| *r == root) {
                            Some((_, targets)) => targets.push((target, body)),
                            None => scopes.push((root, vec![(target, body)])),
                        }
                    }
                }
                let inner_indent = if dev {
                    format!("{}  ", indent)
                } else {
                    String::new()
                };
                for (root, targets) in scopes {
                    let targets: Vec<(&str, &str)> =
                        targets.iter().map(|(t, b)| (t.as_str(), *b)).collect();
                    inner.push_str(&indent);
                    inner.push_str(&format!("@scope ({})", root));
                    inner.push_str(if dev { " {\n" } else { "{" });
                    emit_selector_rules(&mut inner, &targets, &inner_indent, dev);
                    inner.push_str(&indent);
                    inner.push_str(if dev { "}\n" } else { "}" });
                }
            } else {
                chains.sort_by_cached_key(|chain| selector_order(chain));
                for chain in chains {
                    let pairs: Vec<(String, &str)> = rules()
                        .filter(|(_, c, _)| c.at == at && c.selector == chain)
                        .map(|(e, _, body)| (selector(&e.class_name, chain), body.as_str()))
                        .collect();
                    emit_grouped_rules(&mut inner, &pairs, &indent, dev);
                }
            }
            if inner.is_empty() {
                continue;
            }
            // Each at-rule wraps the next, outermost first
            for (level, &rank) in at.iter().enumerate().rev() {
                let pad = if dev {
                    "  ".repeat(level)
                } else {
                    String::new()
                };
                inner = if dev {
                    format!("{pad}{} {{\n{inner}{pad}}}\n", at_rule(rank, true))
                } else {
                    format!("{}{{{inner}}}", at_rule(rank, false))
                };
            }
            css.push_str(&inner);
        }
        css
    }
}

/// Emit CSS rules from `(selector, body)` pairs, merging identical bodies
/// into a single selector-list rule (e.g. `.a,.b{body}`). The first occurrence
/// of each distinct body determines ordering, so output stays deterministic
/// across runs. `indent` is prepended to each rule line.
fn emit_grouped_rules(out: &mut String, pairs: &[(String, &str)], indent: &str, dev: bool) {
    if pairs.is_empty() {
        return;
    }
    // Group in first-occurrence order.
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<&str>> = HashMap::new();
    for (selector, body) in pairs {
        if body.is_empty() {
            continue;
        }
        if !groups.contains_key(body) {
            order.push(body);
        }
        groups.entry(body).or_default().push(selector);
    }
    let sp = if dev { " " } else { "" };
    let nl = if dev { "\n" } else { "" };
    for body in &order {
        out.push_str(indent);
        out.push_str(&groups[body].join(","));
        out.push_str(sp);
        out.push('{');
        out.push_str(body);
        out.push('}');
        out.push_str(nl);
    }
}

/// Emit `(selector, body)` rules in their order, merging a run of rules
/// with the same body into one rule with a selector list (the order
/// matters: two rules may apply to one element).
fn emit_selector_rules(out: &mut String, rules: &[(&str, &str)], indent: &str, dev: bool) {
    let mut i = 0;
    while i < rules.len() {
        let body = rules[i].1;
        let run = rules[i..].iter().take_while(|(_, b)| *b == body).count();
        if !body.is_empty() {
            let selectors: Vec<&str> = rules[i..i + run].iter().map(|(s, _)| *s).collect();
            out.push_str(indent);
            out.push_str(&selectors.join(","));
            if dev {
                out.push(' ');
            }
            out.push('{');
            out.push_str(body);
            out.push('}');
            if dev {
                out.push('\n');
            }
        }
        i += run;
    }
}

struct GenContext {
    dev: bool,
    depth: usize,
    has_interactive: bool,
    /// Writing inside a text element, at any depth: htmlang's own `<div>`s
    /// are `<span>`s there, which text can hold.
    in_text: bool,
    /// How the element whose children are being written lays them out
    flow: Flow,
    /// Writing the children of a `@picture`, where `@source`'s leading
    /// argument is its `srcset`
    in_picture: bool,
}

impl GenContext {
    fn indent(&self) -> String {
        if self.dev {
            "  ".repeat(self.depth)
        } else {
            String::new()
        }
    }

    fn nl(&self) -> &str {
        if self.dev { "\n" } else { "" }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct CodegenOptions {
    pub dev: bool,
    pub partial: bool,
    pub minify: bool,
}

/// Generate HTML from a parsed document using the given options.
pub fn generate_with(doc: &Document, opts: &CodegenOptions) -> String {
    let mut html = if opts.partial {
        generate_partial_inner(doc, opts.dev)
    } else {
        generate_full_inner(doc, opts.dev)
    };
    if opts.minify {
        html = minify_html(&html);
    }
    html
}

pub fn generate(doc: &Document) -> String {
    generate_with(doc, &CodegenOptions::default())
}

pub fn generate_dev(doc: &Document) -> String {
    generate_with(
        doc,
        &CodegenOptions {
            dev: true,
            ..Default::default()
        },
    )
}

pub fn generate_partial(doc: &Document) -> String {
    generate_with(
        doc,
        &CodegenOptions {
            partial: true,
            ..Default::default()
        },
    )
}

pub fn generate_partial_dev(doc: &Document) -> String {
    generate_with(
        doc,
        &CodegenOptions {
            dev: true,
            partial: true,
            ..Default::default()
        },
    )
}

pub fn generate_minified(doc: &Document) -> String {
    generate_with(
        doc,
        &CodegenOptions {
            minify: true,
            ..Default::default()
        },
    )
}

fn minify_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_pre = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut prev_was_space = false;
    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Track <pre>, <script>, <style> contexts
        if i + 4 < chars.len() && chars[i] == '<' {
            let rest: String = chars[i..].iter().take(10).collect();
            let rest_lower = rest.to_lowercase();
            if rest_lower.starts_with("<pre") || rest_lower.starts_with("<textarea") {
                in_pre = true;
            } else if rest_lower.starts_with("</pre") || rest_lower.starts_with("</textarea") {
                in_pre = false;
            } else if rest_lower.starts_with("<script") {
                in_script = true;
            } else if rest_lower.starts_with("</script") {
                in_script = false;
            } else if rest_lower.starts_with("<style") {
                in_style = true;
            } else if rest_lower.starts_with("</style") {
                in_style = false;
            }
        }

        // Strip HTML comments (<!-- ... -->)
        if !in_script
            && !in_style
            && i + 3 < chars.len()
            && chars[i] == '<'
            && chars[i + 1] == '!'
            && chars[i + 2] == '-'
            && chars[i + 3] == '-'
        {
            // Skip to -->
            let mut j = i + 4;
            while j + 2 < chars.len() {
                if chars[j] == '-' && chars[j + 1] == '-' && chars[j + 2] == '>' {
                    j += 3;
                    break;
                }
                j += 1;
            }
            i = j;
            continue;
        }

        // In <pre>, preserve everything
        if in_pre || in_script || in_style {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Collapse whitespace runs to a single space. Spaces are never
        // dropped entirely: next to inline elements they are significant
        // ("Built with <span>" must keep its space).
        if chars[i].is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
            i += 1;
            continue;
        }

        prev_was_space = false;
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn generate_full_inner(doc: &Document, dev: bool) -> String {
    // Without `@page` the output is a fragment
    let Some(page) = &doc.page else {
        return generate_partial_inner(doc, dev);
    };
    let mut styles = StyleCollector::new();
    let mut ctx = GenContext {
        dev,
        depth: 0,
        has_interactive: false,
        in_text: false,
        flow: Flow::default(),
        in_picture: false,
    };

    // The page is the root element: `<body>` is a column (the reset makes
    // it one, filling the viewport), `@page`'s styles are its class, and
    // the page's top-level elements are its children
    let root = Element {
        kind: ElementKind::El,
        attrs: page.styles.clone(),
        argument: None,
        children: Vec::new(),
        line_num: 0,
        function: None,
    };
    let mut own = Flow::of(&root);
    let top = Flow::default();
    let site = Site {
        kind: &root.kind,
        parent: &top,
        has_overlay_children: holds_overlays(&doc.nodes),
        inline: false,
        root: true,
    };
    let body_class = element_class(&root, &site, &mut styles);
    own.class = body_class.clone();
    ctx.flow = own;

    let mut body = String::new();
    generate_children(
        &doc.nodes,
        Some(Layout::Column),
        &mut body,
        &mut styles,
        &mut ctx,
    );

    let element_css = build_element_css(doc, &styles, dev);
    let nl = if dev { "\n" } else { "" };

    let mut meta_html = String::new();
    // A `@meta viewport` of the page's own replaces the usual one
    if !doc.meta_tags.iter().any(|(name, _)| name == "viewport") {
        meta_html.push_str(&format!(
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">{}",
            nl
        ));
    }
    meta_html.push_str(&format!(
        "<title>{}</title>{}",
        html_escape(&page.title),
        nl
    ));
    for (name, content) in &doc.meta_tags {
        meta_html.push_str(&format!(
            "<meta name=\"{}\" content=\"{}\">{}",
            html_escape(name),
            html_escape(content),
            nl
        ));
    }
    for (property, content) in &doc.og_tags {
        meta_html.push_str(&format!(
            "<meta property=\"og:{}\" content=\"{}\">{}",
            html_escape(property),
            html_escape(content),
            nl
        ));
    }
    if let Some(path) = &page.favicon {
        meta_html.push_str(&format!(
            "<link rel=\"icon\" href=\"{}\">{}",
            favicon_href(path),
            nl
        ));
    }
    for block in &doc.head_blocks {
        meta_html.push_str(block);
        meta_html.push_str(nl);
    }

    // `@page`'s HTML attributes go on `<html>`
    let mut html_attrs = String::new();
    for attr in &page.html_attrs {
        html_attrs.push(' ');
        html_attrs.push_str(&attr.key);
        if attr.html {
            html_attrs.push_str("=\"");
            html_attrs.push_str(&html_escape(attr.value.as_deref().unwrap_or("")));
            html_attrs.push('"');
        }
    }
    let mut body_attrs = String::new();
    emit_class_attr(&mut body_attrs, body_class.as_deref(), None);

    // Focus-visible CSS for interactive elements (accessibility)
    let focus_visible_css = if ctx.has_interactive {
        let focusable = ":where(a,button,input,select,textarea):focus-visible";
        if dev {
            format!(
                "{}{} {{ outline: 2px solid currentColor; outline-offset: 2px; }}\n",
                OWN_ELEMENTS, focusable
            )
        } else {
            format!(
                "{}{}{{outline:2px solid currentColor;outline-offset:2px}}",
                OWN_ELEMENTS, focusable
            )
        }
    } else {
        String::new()
    };
    let reset_css = reset_css(dev, true, &focus_visible_css);

    format!(
        "<!DOCTYPE html>{nl}<html{html_attrs}>{nl}<head>{nl}<meta charset=\"utf-8\">{nl}\
         {meta_html}<style>{nl}{reset_css}{element_css}</style>{nl}</head>{nl}\
         <body{body_attrs}>{nl}{body}</body>{nl}</html>{nl}",
    )
}

/// A favicon's `href`: the file itself as a `data:` URI when it can be
/// read, else the path as written.
fn favicon_href(path: &str) -> String {
    match std::fs::read(path) {
        Ok(data) => {
            let mime = if path.ends_with(".png") {
                "image/png"
            } else if path.ends_with(".svg") {
                "image/svg+xml"
            } else {
                "image/x-icon"
            };
            format!("data:{};base64,{}", mime, base64_encode(&data))
        }
        Err(_) => html_escape(path),
    }
}

/// Assemble every CSS block the document needs (font faces, custom
/// properties, generated class rules, keyframes, and user CSS). Shared by
/// full-page and partial output so both emit the same styles.
fn build_element_css(doc: &Document, styles: &StyleCollector, dev: bool) -> String {
    let mut element_css = String::new();

    // Collect the CSS custom properties declared with `@let --name` so they
    // can be emitted in a single `:root` block below.
    let mut root_vars: Vec<(String, String)> = Vec::new();
    for (name, value) in &doc.css_vars {
        root_vars.push((name.clone(), value.clone()));
    }

    // Generated rules always go in htmlang's layers, so unlayered user CSS
    // (`@style`, `@raw`) overrides them regardless of specificity.
    let styles_css = styles.to_css_formatted(dev, &doc.scoped_css);

    // Emit the :root block first so the cascade picks up the custom
    // properties before the class rules consume them.
    if !root_vars.is_empty() {
        if dev {
            element_css.push_str(":root {\n");
            for (name, value) in &root_vars {
                element_css.push_str(&format!("  {}: {};\n", name, value));
            }
            element_css.push_str("}\n");
        } else {
            element_css.push_str(":root{");
            for (name, value) in &root_vars {
                element_css.push_str(name);
                element_css.push(':');
                element_css.push_str(value);
                element_css.push(';');
            }
            element_css.push('}');
        }
    }

    element_css.push_str(&styles_css);

    // @style blocks (custom CSS)
    for block in &doc.custom_css {
        if dev {
            element_css.push_str(block);
            element_css.push('\n');
        } else {
            let minified: String = block.lines().map(|l| l.trim()).collect::<Vec<_>>().join("");
            element_css.push_str(&minified);
        }
    }

    element_css
}

/// htmlang's own elements: those with a generated class, or the class of a
/// function's scoped `@style`. The reset's rules for elements are kept to
/// them, so `@raw` and `@markdown` content, and the page a fragment is put
/// into, keep their own.
const OWN_ELEMENTS: &str = ":where([class^=\"hl-\"],[class*=\" hl-\"])";

/// htmlang's cascade layers, from the one that loses to the one that wins:
/// the reset; the defaults of each element's kind (a heading's `margin:0`,
/// a column's `display:flex`); the styles an element gets from an
/// element prefix on an element around it (`@td:padding 8`) and from a
/// function's scoped `@style`; and the element's own styles. So a
/// scope's styles win over the defaults, and what an element says about
/// itself wins over both. CSS outside any layer (`@style`, `@raw`) wins
/// over all of them.
const LAYERS: [&str; 4] = ["hl-reset", "hl-kind", "hl-inside", "htmlang"];

/// Built-in reset rules, in a layer before `htmlang`: unlayered CSS beats
/// every layer, so an unlayered rule here would override the generated
/// ones. A page resets `box-sizing` everywhere and makes `<body>` a column
/// that fills the window; a fragment, which goes into a page it doesn't
/// own, touches only htmlang's own elements. In both, an element htmlang
/// lays out stays hidden while it is `hidden`, a closed `<dialog>` or a
/// popover that isn't showing: its generated `display` would otherwise
/// beat the browser's `display: none`.
fn reset_css(dev: bool, page: bool, focus_visible_css: &str) -> String {
    let own = OWN_ELEMENTS;
    let mut rules = Vec::new();
    if page {
        rules.push(if dev {
            "*, *::before, *::after { box-sizing: border-box; }".to_string()
        } else {
            "*,*::before,*::after{box-sizing:border-box}".to_string()
        });
        rules.push(if dev {
            "body { margin: 0; font-family: system-ui, -apple-system, sans-serif; display: flex; flex-direction: column; min-height: 100dvh; }".to_string()
        } else {
            "body{margin:0;font-family:system-ui,-apple-system,sans-serif;display:flex;flex-direction:column;min-height:100dvh}".to_string()
        });
    } else {
        rules.push(if dev {
            format!("{own}, {own}::before, {own}::after {{ box-sizing: border-box; }}")
        } else {
            format!("{own},{own}::before,{own}::after{{box-sizing:border-box}}")
        });
    }
    let hidden = "[hidden]:not([hidden=\"until-found\"]),dialog:not([open]):not(:popover-open),\
                  [popover]:not(:popover-open):not(dialog[open])";
    rules.push(if dev {
        format!("{own}:where({hidden}) {{ display: none !important; }}")
    } else {
        format!("{own}:where({hidden}){{display:none!important}}")
    });
    let mut rules = rules.join(if dev { "\n" } else { "" });
    if dev {
        rules.push('\n');
    }
    rules.push_str(focus_visible_css);
    if dev {
        format!(
            "@layer {};\n@layer hl-reset {{\n{}}}\n",
            LAYERS.join(", "),
            rules
        )
    } else {
        format!("@layer {};@layer hl-reset{{{}}}", LAYERS.join(","), rules)
    }
}

/// Generate an HTML fragment: body + optional <style>, no <html>/<head>/<body> wrapper.
fn generate_partial_inner(doc: &Document, dev: bool) -> String {
    let mut styles = StyleCollector::new();
    let mut ctx = GenContext {
        dev,
        depth: 0,
        has_interactive: false,
        in_text: false,
        flow: Flow::default(),
        in_picture: false,
    };
    let mut body = String::new();

    generate_children(&doc.nodes, None, &mut body, &mut styles, &mut ctx);

    let mut element_css = build_element_css(doc, &styles, dev);
    // htmlang's own elements get the reset's rules for elements (which
    // also puts htmlang's layers in order)
    if !styles.entries.is_empty() || !doc.scoped_css.is_empty() {
        element_css.insert_str(0, &reset_css(dev, false, ""));
    }

    if element_css.is_empty() {
        body
    } else if dev {
        format!("<style>\n{}</style>\n{}", element_css, body)
    } else {
        format!("<style>{}</style>{}", element_css, body)
    }
}

// ---------------------------------------------------------------------------
// Node generation
// ---------------------------------------------------------------------------

/// Write `node`, which is inside an element with the layout `parent`
/// (`None` at the top of the page).
fn generate_node(
    node: &Node,
    parent: Option<Layout>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    match node {
        Node::Element(elem) => generate_element(elem, parent, out, styles, ctx),
        Node::Text(segments) => {
            // In a row, column or grid each line of text is a child
            let needs_wrap = parent.is_some_and(Layout::is_container);
            if needs_wrap {
                out.push_str(&ctx.indent());
                out.push_str("<span>");
            }
            generate_text_segments(segments, out, styles, ctx);
            if needs_wrap {
                out.push_str("</span>");
                out.push_str(ctx.nl());
            }
        }
        Node::Raw(content) => {
            out.push_str(&ctx.indent());
            out.push_str(content);
            out.push_str(ctx.nl());
        }
    }
}

/// What goes between two things written one after the other in an element
/// with this layout (`None`: the top of the page): text flows, so its lines
/// and children are joined with a space; in HTML's own layout, and at the
/// top of the page, two lines of text are separated by a line break (a
/// space, except where whitespace is kept, as in `@pre`).
fn separator(layout: Option<Layout>, previous_is_text: bool, next: &Node) -> Option<char> {
    match layout {
        Some(Layout::Text) => Some(' '),
        Some(Layout::Native) | None if previous_is_text && matches!(next, Node::Text(_)) => {
            Some('\n')
        }
        _ => None,
    }
}

/// Write the children of an element (or of a `@fragment`, which has no
/// element of its own) laid out as `layout` (`None`: the top of the page).
/// The first line of an element's text is its first child.
fn generate_children(
    children: &[Node],
    layout: Option<Layout>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    generate_run(children, layout, &mut None, out, styles, ctx);
}

/// [`generate_children`], with `previous` saying whether what was written
/// last was a line of text (`None`: nothing yet). A `@fragment`'s children
/// are written as if they stood in its place, so its lines are separated
/// from the lines around it like any other lines.
fn generate_run(
    children: &[Node],
    layout: Option<Layout>,
    previous: &mut Option<bool>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    for child in children {
        if let Node::Element(elem) = child
            && elem.kind == ElementKind::Fragment
        {
            generate_run(&elem.children, layout, previous, out, styles, ctx);
            continue;
        }
        let start = out.len();
        if let Some(previous_is_text) = *previous
            && let Some(sep) = separator(layout, previous_is_text, child)
        {
            out.push(sep);
        }
        let body = out.len();
        generate_node(child, layout, out, styles, ctx);
        // A child that writes nothing (an empty `@fragment`) takes no
        // separator either, so text never gets two spaces in a row
        if out.len() == body {
            out.truncate(start);
            continue;
        }
        *previous = Some(matches!(child, Node::Text(_)));
    }
}

/// Emit the element's leading argument as its attribute (`@iframe URL` →
/// `src="URL"`, `@form /submit` → `action="/submit"`, `@link /a` →
/// `href="/a"`), first, before the class. `@image [inline] photo.png` puts
/// the file itself into the page.
fn emit_argument_attr(out: &mut String, elem: &Element, in_picture: bool) {
    let (Some(attr), Some(value)) = (elem.kind.leading_attribute(in_picture), &elem.argument)
    else {
        return;
    };
    let inline =
        elem.kind == ElementKind::Image && elem.attrs.iter().any(|a| !a.html && a.key == "inline");
    let value = match inline.then(|| image_data_uri(value)).flatten() {
        Some(data) => data,
        None => html_escape(value),
    };
    out.push(' ');
    out.push_str(attr);
    out.push_str("=\"");
    out.push_str(&value);
    out.push('"');
}

/// A raster image's file as a `data:` URI (an SVG is put into the page
/// as markup by the parser instead); `None` when it can't be read.
fn image_data_uri(src: &str) -> Option<String> {
    if src.is_empty() || src.ends_with(".svg") {
        return None;
    }
    let mime = if src.ends_with(".png") {
        "image/png"
    } else if src.ends_with(".jpg") || src.ends_with(".jpeg") {
        "image/jpeg"
    } else if src.ends_with(".gif") {
        "image/gif"
    } else if src.ends_with(".webp") {
        "image/webp"
    } else if src.ends_with(".avif") {
        "image/avif"
    } else {
        "application/octet-stream"
    };
    let data = std::fs::read(src).ok()?;
    Some(format!("data:{};base64,{}", mime, base64_encode(&data)))
}

/// Emit HTML attributes: `key=value` ones (except `id` / `class`, which
/// are emitted with the generated class) and bare booleans like `required`.
fn emit_html_attrs(out: &mut String, attrs: &[Attribute]) {
    for attr in attrs {
        let key = attr.key.as_str();
        if attr.html && key != "id" && key != "class" {
            out.push(' ');
            out.push_str(key);
            out.push_str("=\"");
            // Quoted text has already lost its quotes (see parser.rs)
            out.push_str(&html_escape(attr.value.as_deref().unwrap_or("")));
            out.push('"');
        } else if !attr.html
            && attr.value.is_none()
            && crate::vocab::BOOLEAN_HTML_ATTRS.contains(&key)
        {
            out.push(' ');
            out.push_str(key);
        }
    }
}

// True if any direct child is `@in-front` or `@behind`. Such children render
// as absolutely positioned overlays, so the parent automatically becomes a
// positioning context (position:relative + isolation:isolate).
fn has_overlay_children(elem: &Element) -> bool {
    holds_overlays(&elem.children)
}

fn holds_overlays(children: &[Node]) -> bool {
    children.iter().any(|child| {
        matches!(
            child,
            Node::Element(e) if e.kind.is_tag("in-front") || e.kind.is_tag("behind")
        )
    })
}

fn generate_element(
    elem: &Element,
    parent: Option<Layout>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    // Self-closing elements
    if elem.kind.layout() == Layout::Void {
        generate_self_closing(elem, parent, out, styles, ctx);
        return;
    }
    // A verbatim element (@script) writes its body as it is, with no HTML
    // escaping; it isn't shown, so it has no class (its styles are an error)
    if elem.kind.is_verbatim() {
        let tag = elem.kind.spec().map_or("script", |spec| spec.html);
        out.push_str(&ctx.indent());
        out.push('<');
        out.push_str(tag);
        emit_argument_attr(out, elem, false);
        let (id, class) = extract_id_class(&elem.attrs);
        for (key, value) in [("id", id), ("class", class)] {
            if let Some(value) = value {
                out.push_str(&format!(" {}=\"{}\"", key, html_escape(&value)));
            }
        }
        emit_html_attrs(out, &elem.attrs);
        out.push('>');
        for child in &elem.children {
            match child {
                Node::Text(segments) => {
                    for seg in segments {
                        if let TextSegment::Plain(text) = seg {
                            out.push_str(text)
                        }
                    }
                }
                Node::Raw(content) => out.push_str(content),
                _ => {}
            }
        }
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        out.push_str(ctx.nl());
        return;
    }
    if elem.kind == ElementKind::Children {
        return;
    }
    if matches!(elem.kind, ElementKind::Slot(_)) {
        return;
    }
    if elem.kind == ElementKind::Fragment {
        // Render children without a wrapper element, as if they were
        // written where the fragment is
        generate_children(&elem.children, parent, out, styles, ctx);
        return;
    }

    let mut own = Flow::of(elem);
    let tag = match &elem.kind {
        ElementKind::Row | ElementKind::El => "div",
        ElementKind::Text => "span",
        ElementKind::Paragraph => "p",
        ElementKind::Link => "a",
        ElementKind::Tag(spec) => spec.html,
        _ => "",
    };
    // Inside text, htmlang's own `<div>` is a `<span>`, which text can
    // hold (directly in text, a row, column or grid is also laid out
    // inline: see `Site::inline`)
    let in_text = ctx.in_text || parent == Some(Layout::Text);
    let tag = if in_text && tag == "div" { "span" } else { tag };
    let kind_label = elem.kind.name();

    // Track interactive elements for focus-visible CSS
    if elem.kind == ElementKind::Link
        || matches!(kind_label, "button" | "input" | "select" | "textarea")
    {
        ctx.has_interactive = true;
    }

    // Compute CSS for each state and get a class name
    let gen_class = element_class(elem, &Site::new(elem, parent, &ctx.flow), styles);
    own.class = gen_class.clone();
    let (id, user_class) = extract_id_class(&elem.attrs);

    if ctx.dev && elem.line_num > 0 {
        out.push_str(&ctx.indent());
        out.push_str(&format!(
            "<!-- @{} line {} -->\n",
            kind_label, elem.line_num
        ));
    }
    out.push_str(&ctx.indent());
    out.push('<');
    out.push_str(tag);

    emit_argument_attr(out, elem, ctx.in_picture);

    emit_class_attr(out, gen_class.as_deref(), user_class.as_deref());

    if let Some(id) = id {
        out.push_str(" id=\"");
        out.push_str(&html_escape(&id));
        out.push('"');
    }

    emit_html_attrs(out, &elem.attrs);

    // Source map attributes in dev mode
    if ctx.dev && elem.line_num > 0 {
        out.push_str(&format!(
            " data-hl-line=\"{}\" data-hl-el=\"{}\"",
            elem.line_num, kind_label
        ));
    }

    out.push('>');
    // In `<pre>` and `<textarea>` every space and line break shows, so
    // readable (dev) output adds none there: a code sample's lines stay
    // exactly as written
    let outer_dev = ctx.dev;
    ctx.dev &= !matches!(tag, "pre" | "textarea");
    out.push_str(ctx.nl());

    let layout = elem.kind.layout();
    ctx.depth += 1;
    let outer_in_text = ctx.in_text;
    ctx.in_text = in_text || layout == Layout::Text;
    let outer_flow = std::mem::replace(&mut ctx.flow, own);
    let outer_picture = std::mem::replace(&mut ctx.in_picture, elem.kind.is_tag("picture"));
    generate_children(&elem.children, Some(layout), out, styles, ctx);
    ctx.in_picture = outer_picture;
    ctx.flow = outer_flow;
    ctx.in_text = outer_in_text;
    ctx.depth -= 1;

    out.push_str(&ctx.indent());
    ctx.dev = outer_dev;
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    out.push_str(ctx.nl());
}

fn generate_self_closing(
    elem: &Element,
    parent: Option<Layout>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    let gen_class = element_class(elem, &Site::new(elem, parent, &ctx.flow), styles);
    let (id, user_class) = extract_id_class(&elem.attrs);

    let (tag, kind_label) = match &elem.kind {
        ElementKind::Image => ("img", "image"),
        ElementKind::Tag(spec) => (spec.html, spec.name),
        _ => unreachable!("not a void element: {:?}", elem.kind),
    };

    if ctx.dev && elem.line_num > 0 {
        out.push_str(&ctx.indent());
        out.push_str(&format!(
            "<!-- @{} line {} -->\n",
            kind_label, elem.line_num
        ));
    }
    out.push_str(&ctx.indent());
    out.push('<');
    out.push_str(tag);
    emit_argument_attr(out, elem, ctx.in_picture);

    emit_class_attr(out, gen_class.as_deref(), user_class.as_deref());

    if let Some(id) = id {
        out.push_str(" id=\"");
        out.push_str(&html_escape(&id));
        out.push('"');
    }

    emit_html_attrs(out, &elem.attrs);

    // Source map attributes in dev mode (self-closing)
    if ctx.dev && elem.line_num > 0 {
        out.push_str(&format!(
            " data-hl-line=\"{}\" data-hl-el=\"{}\"",
            elem.line_num, kind_label
        ));
    }

    out.push('>');
    out.push_str(ctx.nl());
}

fn generate_text_segments(
    segments: &[TextSegment],
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    for segment in segments {
        match segment {
            TextSegment::Plain(text) => out.push_str(&html_escape(text)),
            TextSegment::Inline(elem) => {
                let mut buf = String::new();
                // An element inside a line of text is in text
                let outer_flow = std::mem::take(&mut ctx.flow);
                generate_element(elem, Some(Layout::Text), &mut buf, styles, ctx);
                ctx.flow = outer_flow;
                out.push_str(buf.trim_end());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

fn compute_class(
    attrs: &[Attribute],
    site: &Site,
    styles: &mut StyleCollector,
    distinct: String,
) -> Option<String> {
    // Every condition an attribute is under, and none
    let mut conditions = vec![Condition::default()];
    for attr in attrs.iter().filter(|a| !a.html) {
        let (condition, _) = Condition::of(&attr.key);
        if !conditions.contains(&condition) {
            conditions.push(condition);
        }
    }
    // Dedupe: if a property is declared twice within a single rule, keep only
    // the last occurrence.
    let rules: Vec<(Condition, String)> = conditions
        .into_iter()
        .map(|condition| {
            let css = dedupe_declarations(&attrs_to_css(attrs, &condition, site));
            (condition, css)
        })
        .collect();
    // The kind's defaults, less what the element sets itself (a user
    // `[list-style disc]` wins over the default `list-style:none` anyway,
    // so there is no need to ship both)
    let own = rules
        .iter()
        .find(|(condition, _)| *condition == Condition::default())
        .map_or("", |(_, css)| css.as_str());
    let own: Vec<&str> = declarations(own).map(|(name, _)| name).collect();
    let defaults: String = declarations(&dedupe_declarations(&default_css(site)))
        .filter(|(name, _)| !own.contains(name))
        .map(|(_, declaration)| declaration)
        .collect();
    styles.get_class(defaults, rules, distinct)
}

/// The defaults of an element's kind where it is: an overlay's positioning
/// context, its layout, and its kind's own CSS (a heading's `margin:0`).
fn default_css(site: &Site) -> String {
    let mut css = String::new();
    // Elements with @in-front / @behind children become positioning
    // contexts
    if site.has_overlay_children {
        css.push_str("position:relative;isolation:isolate;");
    }
    if !site.root {
        css.push_str(layout_css(site.kind.layout(), site.inline));
    }
    css.push_str(site.kind.css());
    css
}

/// The declarations of a rule body, as (property, `property:value;`), for
/// bodies htmlang wrote: every declaration ends in `;`, and a `;` inside
/// quotes or parentheses doesn't end one.
fn declarations(css: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut rest = css;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let (mut depth, mut quote, mut escaped) = (0i32, None, false);
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                c if Some(c) == quote => quote = None,
                _ if quote.is_some() => {}
                '"' | '\'' => quote = Some(c),
                '(' => depth += 1,
                ')' => depth -= 1,
                ';' if depth == 0 => {
                    end = i + 1;
                    break;
                }
                _ => {}
            }
        }
        let (declaration, after) = rest.split_at(end);
        rest = after;
        let name = declaration.split(':').next().unwrap_or("").trim();
        Some((name, declaration))
    })
}

/// Dedupe CSS declarations within a single rule body: for any property
/// declared more than once, keep only the last occurrence. Unparseable
/// segments (no `:`) are preserved as-is. Semicolons inside parentheses or
/// quoted strings are treated as part of a value, not as declaration
/// separators.
fn dedupe_declarations(css: &str) -> String {
    if css.is_empty() {
        return String::new();
    }
    // Quick path: no chance of duplicates if there's fewer than 2 declarations.
    if css.matches(';').count() < 2 {
        return css.to_string();
    }

    // Split into (property_name_opt, full_decl_with_semi) preserving whatever
    // terminator the input used. We split on `;` at depth 0 (ignoring parens).
    let mut decls: Vec<(Option<String>, String)> = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    // A quoted string's `;` (`content "a;b"`) is text; a backslash escapes
    // the character after it
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in css.chars() {
        if escaped {
            escaped = false;
            current.push(ch);
        } else if ch == '\\' {
            escaped = true;
            current.push(ch);
        } else if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            current.push(ch);
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            current.push(ch);
        } else if ch == '(' {
            depth += 1;
            current.push(ch);
        } else if ch == ')' {
            depth -= 1;
            current.push(ch);
        } else if ch == ';' && depth == 0 {
            current.push(';');
            let trimmed = current.trim();
            if !trimmed.is_empty() && trimmed != ";" {
                let prop = trimmed
                    .trim_end_matches(';')
                    .split_once(':')
                    .map(|(p, _)| p.trim().to_ascii_lowercase());
                decls.push((prop, std::mem::take(&mut current)));
            } else {
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        let prop = current
            .split_once(':')
            .map(|(p, _)| p.trim().to_ascii_lowercase());
        decls.push((prop, std::mem::take(&mut current)));
    }

    if decls.len() < 2 {
        return css.to_string();
    }

    // Find index of last occurrence of each property.
    use std::collections::HashMap;
    let mut last: HashMap<String, usize> = HashMap::new();
    for (i, (prop, _)) in decls.iter().enumerate() {
        if let Some(p) = prop {
            last.insert(p.clone(), i);
        }
    }

    let mut out = String::with_capacity(css.len());
    for (i, (prop, raw)) in decls.iter().enumerate() {
        let keep = match prop {
            Some(p) => last.get(p) == Some(&i),
            None => true,
        };
        if keep {
            out.push_str(raw);
        }
    }
    out
}

fn emit_class_attr(out: &mut String, gen_class: Option<&str>, user_class: Option<&str>) {
    match (gen_class, user_class) {
        (Some(g), Some(u)) => {
            out.push_str(" class=\"");
            out.push_str(g);
            out.push(' ');
            out.push_str(&html_escape(u));
            out.push('"');
        }
        (Some(g), None) => {
            out.push_str(" class=\"");
            out.push_str(g);
            out.push('"');
        }
        (None, Some(u)) => {
            out.push_str(" class=\"");
            out.push_str(&html_escape(u));
            out.push('"');
        }
        (None, None) => {}
    }
}

// ---------------------------------------------------------------------------
// Attribute → CSS mapping
// ---------------------------------------------------------------------------

/// Where an element is written, which its CSS depends on.
struct Site<'a> {
    kind: &'a ElementKind,
    /// How its parent lays out its children: its `width fill`, `height
    /// fill` and `shrink` compile against it.
    parent: &'a Flow,
    /// It has `@in-front` / `@behind` children.
    has_overlay_children: bool,
    /// A row, column or grid inside text, laid out inline.
    inline: bool,
    /// The page's `<body>`, which the reset already makes a column.
    root: bool,
}

impl Site<'_> {
    /// Where an element in a parent with the layout `layout` and the flow
    /// `parent` is.
    fn new<'a>(elem: &'a Element, layout: Option<Layout>, parent: &'a Flow) -> Site<'a> {
        Site {
            kind: &elem.kind,
            parent,
            has_overlay_children: has_overlay_children(elem),
            inline: layout == Some(Layout::Text) && elem.kind.layout().is_container(),
            root: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Direction: what a child's `fill` and `shrink` compile against
// ---------------------------------------------------------------------------

/// The way a flex row or column lays out its children.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    Row,
    Column,
}

impl Axis {
    /// The direction a `flex-direction` or `flex-flow` value sets, if it
    /// names one (not `var(--dir)`, which only the browser knows).
    fn of_value(value: &str) -> Option<Axis> {
        value.split_whitespace().find_map(|word| match word {
            "row" | "row-reverse" => Some(Axis::Row),
            "column" | "column-reverse" => Some(Axis::Column),
            _ => None,
        })
    }
}

/// How an element lays out its children, as its CSS sets it: the
/// direction its children's `width`/`height` `fill` and `shrink` compile
/// against. That is the direction of `@row` or of a column, or the one its
/// own `flex-direction` (or `flex-flow`) sets, with or without media or
/// container prefixes. Like elm-ui's `.r > .wf`, the rules for a direction
/// set under a prefix are keyed on the element's class: `@media (...) {
/// :where(.a)>.b {...} }`.
#[derive(Clone, Default)]
struct Flow {
    /// Without a prefix; `None` when the element isn't a flex row or column
    /// (a grid, text, HTML's own layout, the top of the page)
    base: Option<Axis>,
    /// The direction under each chain of at-rule prefixes that sets one, in
    /// the order their blocks are written
    changes: Vec<(Vec<usize>, Axis)>,
    /// The element's generated class, which the rules for the changes are
    /// keyed on
    class: Option<String>,
}

impl Flow {
    fn of(elem: &Element) -> Flow {
        let mut base = match elem.kind.layout() {
            Layout::Row => Axis::Row,
            Layout::Column => Axis::Column,
            _ => return Flow::default(),
        };
        let mut changes: Vec<(Vec<usize>, Axis)> = Vec::new();
        for attr in elem.attrs.iter().filter(|a| !a.html) {
            let (condition, name) = Condition::of(&attr.key);
            if !matches!(name, "flex-direction" | "flex-flow") || !condition.selector.is_empty() {
                continue;
            }
            let Some(axis) = attr.value.as_deref().and_then(Axis::of_value) else {
                continue;
            };
            if condition.at.is_empty() {
                base = axis;
            } else {
                // The later one wins, as in the CSS
                changes.retain(|(at, _)| *at != condition.at);
                changes.push((condition.at, axis));
            }
        }
        changes.sort_by_key(|(at, _)| at_order(at));
        Flow {
            base: Some(base),
            changes,
            class: None,
        }
    }

    /// The direction under the at-rules `at`: the latest one set among
    /// the conditions that hold there (`md:` holds at `lg:`), else the one
    /// without a prefix.
    fn at(&self, at: &[usize]) -> Option<Axis> {
        let base = self.base?;
        let here = Condition::at_only(at);
        let changed = self
            .changes
            .iter()
            .rev()
            .find(|(chain, _)| Condition::at_only(chain).holds_at(&here))
            .map(|&(_, axis)| axis);
        Some(changed.unwrap_or(base))
    }
}

/// `width` or `height`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Dim {
    Width,
    Height,
}

/// What a `width` or `height` says.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Size {
    Fill,
    Shrink,
    /// A size of its own (`width 300`)
    Set,
}

impl Size {
    fn of(value: &str) -> Size {
        match value.trim() {
            "fill" => Size::Fill,
            "shrink" => Size::Shrink,
            _ => Size::Set,
        }
    }
}

/// The declarations `fill` and `shrink` make in a parent laid out along
/// `axis` (`None`: not a flex row or column). Along the parent's direction,
/// `fill` takes the remaining space and `shrink` keeps the content's size;
/// across it (or outside flex), `fill` is the full size and `shrink` fits
/// the content.
fn sizing(dim: Dim, size: Size, axis: Option<Axis>) -> &'static [(&'static str, &'static str)] {
    match (dim, size, axis) {
        (_, Size::Set, _) => &[],
        (Dim::Width, Size::Fill, Some(Axis::Row)) => &[("flex", "1"), ("min-width", "0")],
        (Dim::Width, Size::Fill, _) => &[("width", "100%")],
        (Dim::Width, Size::Shrink, Some(Axis::Row)) => &[("flex-shrink", "0")],
        (Dim::Width, Size::Shrink, _) => &[("width", "fit-content")],
        (Dim::Height, Size::Fill, Some(Axis::Column)) => &[("flex", "1"), ("min-height", "0")],
        (Dim::Height, Size::Fill, _) => &[("height", "100%")],
        (Dim::Height, Size::Shrink, Some(Axis::Column)) => &[("flex-shrink", "0")],
        (Dim::Height, Size::Shrink, _) => &[("height", "fit-content")],
    }
}

/// Every property `fill` and `shrink` of `dim` can set, with its initial
/// value (`flex` covers `flex-shrink`).
fn sizing_properties(dim: Dim) -> [(&'static str, &'static str); 3] {
    match dim {
        Dim::Width => [
            ("flex", "0 1 auto"),
            ("min-width", "auto"),
            ("width", "auto"),
        ],
        Dim::Height => [
            ("flex", "0 1 auto"),
            ("min-height", "auto"),
            ("height", "auto"),
        ],
    }
}

/// What an element's attributes under some prefixes say about its size.
#[derive(Default)]
struct Sizes {
    /// The last `width` and `height`
    width: Option<Size>,
    height: Option<Size>,
    /// A `fill` or `shrink` for the width / height is among them (a later
    /// size may replace it)
    width_word: bool,
    height_word: bool,
    /// It writes `flex`, one of its longhands (`flex-grow`, `flex-shrink`,
    /// `flex-basis`), `min-width` or `min-height` itself
    flex: bool,
    flex_grow: bool,
    flex_shrink: bool,
    flex_basis: bool,
    min_width: bool,
    min_height: bool,
}

impl Sizes {
    /// Read the attributes under the conditions that hold wherever `at`
    /// does, a later condition over an earlier one.
    fn of(attrs: &[Attribute], at: &Condition) -> Sizes {
        let mut sizes = Sizes::default();
        let mut read: Vec<(Condition, &str, &str)> = Vec::new();
        for attr in attrs.iter().filter(|a| !a.html) {
            let (condition, name) = Condition::of(&attr.key);
            let Some(value) = attr.value.as_deref().filter(|v| !v.trim().is_empty()) else {
                continue;
            };
            if condition.holds_at(at) {
                read.push((condition, name, value));
            }
        }
        read.sort_by_cached_key(|(condition, _, _)| condition.order());
        for (_, name, value) in read {
            match name {
                "width" => {
                    let size = Size::of(value);
                    sizes.width = Some(size);
                    sizes.width_word |= size != Size::Set;
                }
                "height" => {
                    let size = Size::of(value);
                    sizes.height = Some(size);
                    sizes.height_word |= size != Size::Set;
                }
                "flex" => sizes.flex = true,
                "flex-grow" => sizes.flex_grow = true,
                "flex-shrink" => sizes.flex_shrink = true,
                "flex-basis" => sizes.flex_basis = true,
                "min-width" => sizes.min_width = true,
                "min-height" => sizes.min_height = true,
                _ => {}
            }
        }
        sizes
    }

    /// The element writes `property` itself: a layout word leaves it alone.
    fn writes(&self, property: &str) -> bool {
        match property {
            "flex" => self.flex,
            "flex-grow" => self.flex || self.flex_grow,
            "flex-shrink" => self.flex || self.flex_shrink,
            "flex-basis" => self.flex || self.flex_basis,
            "min-width" => self.min_width,
            "min-height" => self.min_height,
            "width" => self.width == Some(Size::Set),
            "height" => self.height == Some(Size::Set),
            _ => false,
        }
    }

    /// Write a layout word's `property: value`, unless the element writes
    /// the property itself. When it writes some of `flex`'s longhands, a
    /// `flex` is written as the others (`flex-shrink 0, width fill` still
    /// grows).
    fn push(&self, css: &mut String, property: &str, value: &str) {
        if property == "flex" && !self.flex {
            let longhands = match value {
                "1" => [
                    ("flex-grow", "1"),
                    ("flex-shrink", "1"),
                    ("flex-basis", "0%"),
                ],
                // The reset, `0 1 auto`
                _ => [
                    ("flex-grow", "0"),
                    ("flex-shrink", "1"),
                    ("flex-basis", "auto"),
                ],
            };
            if longhands.iter().any(|&(p, _)| self.writes(p)) {
                for (longhand, value) in longhands {
                    if !self.writes(longhand) {
                        push_css(css, longhand, value);
                    }
                }
                return;
            }
        }
        if !self.writes(property) {
            push_css(css, property, value);
        }
    }
}

/// The rule that gives an element's `fill` and `shrink`, as its attributes
/// under the at-rules `at` set them, their meaning in a parent laid out
/// along `axis`. Every other property a layout word could have set is put
/// back to its initial value, so the rule is right whichever earlier rule
/// it overrides. Properties the element writes itself are left alone.
fn sizing_rule(attrs: &[Attribute], at: &[usize], axis: Axis) -> String {
    let sizes = Sizes::of(attrs, &Condition::at_only(at));
    let mut sets: Vec<(&str, &str)> = Vec::new();
    let mut resets: Vec<(&str, &str)> = Vec::new();
    for (dim, size, word) in [
        (Dim::Width, sizes.width, sizes.width_word),
        (Dim::Height, sizes.height, sizes.height_word),
    ] {
        if !word {
            continue;
        }
        if let Some(size) = size {
            sets.extend_from_slice(sizing(dim, size, Some(axis)));
        }
        resets.extend_from_slice(&sizing_properties(dim));
    }
    let mut css = String::new();
    let mut written: Vec<&str> = Vec::new();
    for &(property, value) in &resets {
        if sets.iter().any(|&(p, _)| p == property) || written.contains(&property) {
            continue;
        }
        written.push(property);
        sizes.push(&mut css, property, value);
    }
    for &(property, value) in &sets {
        sizes.push(&mut css, property, value);
    }
    css
}

/// The rules an element's `fill` and `shrink` need under each chain of
/// at-rules where the direction of `flow` (its parent's) changes: `(at,
/// body)`.
fn sizing_changes(attrs: &[Attribute], flow: &Flow) -> Vec<(Vec<usize>, String)> {
    flow.changes
        .iter()
        .map(|(at, axis)| (at.clone(), sizing_rule(attrs, at, *axis)))
        .filter(|(_, body)| !body.is_empty())
        .collect()
}

/// An element's generated class, with the rules keyed on its parent's
/// class that its `fill` and `shrink` need where the parent's direction
/// changes under a prefix (see [`Flow`]): `:where(.a)>.b`, one class of
/// specificity like any class rule.
fn element_class(elem: &Element, site: &Site, styles: &mut StyleCollector) -> Option<String> {
    let as_child = match site.parent.class {
        Some(_) => sizing_changes(&elem.attrs, site.parent),
        None => Vec::new(),
    };
    // Two elements with the same CSS but different rules keyed on them
    // need two classes
    let mut distinct = String::new();
    for (at, body) in &as_child {
        distinct.push_str(&format!("<{:?}{}", at, body));
    }
    let class = compute_class(&elem.attrs, site, styles, distinct)?;
    if let Some(parent) = &site.parent.class {
        for (at, body) in as_child {
            styles.add_keyed(at, format!(":where(.{})>.{}", parent, class), body);
        }
    }
    Some(class)
}

/// The `display` of a layout (nothing for text, native and void elements,
/// which keep HTML's own).
fn layout_css(layout: Layout, inline: bool) -> &'static str {
    match (layout, inline) {
        (Layout::Column, false) => "display:flex;flex-direction:column;",
        (Layout::Column, true) => "display:inline-flex;flex-direction:column;",
        (Layout::Row, false) => "display:flex;flex-direction:row;",
        (Layout::Row, true) => "display:inline-flex;flex-direction:row;",
        (Layout::Grid, false) => "display:grid;",
        (Layout::Grid, true) => "display:inline-grid;",
        _ => "",
    }
}

/// The declarations of the attributes under `condition` (the defaults of
/// the element's kind are [`default_css`]'s).
fn attrs_to_css(attrs: &[Attribute], condition: &Condition, site: &Site) -> String {
    let mut css = String::new();
    // The auto margins of `center-x`, `align-left`, ..., written at the end
    let mut aligned = String::new();
    let kind = site.kind;
    // Under `children:` the styles go on the children, whose parent is this
    // element: htmlang's words that place an element in its parent are an
    // error there (the parser reports them), and left out
    let on_children = condition.on_children();
    let inside = condition.inside().is_some();
    // The direction `fill` and `shrink` compile against
    let axis = site.parent.at(&condition.at);

    // htmlang's words for laying out children (`spacing`, `wrap`,
    // `grid-cols`) mean nothing on an element that doesn't lay out its
    // children, which the parser reports; they are left out. Under
    // `children:` they go on the children, whose layout isn't known here.
    let lays_out_children = kind.layout().is_container() || on_children;
    fn under<'k>(key: &'k str, condition: &Condition) -> Option<&'k str> {
        let (c, name) = Condition::of(key);
        (c == *condition).then_some(name)
    }

    for (index, attr) in attrs.iter().enumerate() {
        if attr.html {
            continue;
        }
        let Some(effective_key) = under(&attr.key, condition) else {
            continue;
        };
        if on_children && crate::vocab::places_in_parent(effective_key, attr.value.as_deref()) {
            continue;
        }
        // Under an element prefix (`@td:`) the styles go on elements whose
        // layout and parent aren't known here: htmlang's own words are an
        // error there (the parser reports them), and left out
        if inside && crate::vocab::is_htmlang_word(effective_key, attr.value.as_deref()) {
            continue;
        }

        let val = attr.value.as_deref();
        // A style whose value came out empty (a field a record doesn't
        // have, `${if()}` without its other branch) is left out
        if val.is_some_and(|v| v.trim().is_empty()) {
            continue;
        }

        match effective_key {
            // htmlang's own words: they lay out children or place the
            // element in its parent, and mean more than one CSS property
            "spacing" if !lays_out_children => {}
            "spacing" => {
                if let Some(v) = val {
                    push_css(&mut css, "gap", &with_px("gap", v));
                }
            }

            // Sizing: `fill` and `shrink` compile against the parent's
            // direction. Only the last `width` (`height`) counts, as for
            // any style written twice.
            "width" | "height" if val.is_some_and(|v| Size::of(v) != Size::Set) => {
                let later = attrs[index + 1..].iter().any(|a| {
                    !a.html
                        && under(&a.key, condition) == Some(effective_key)
                        && a.value.as_deref().is_some_and(|v| !v.trim().is_empty())
                });
                if later {
                    continue;
                }
                let dim = if effective_key == "width" {
                    Dim::Width
                } else {
                    Dim::Height
                };
                let size = Size::of(val.unwrap_or_default());
                // What the element writes itself, which a layout word
                // leaves alone
                let sizes = Sizes::of(attrs, condition);
                for &(property, value) in sizing(dim, size, axis) {
                    sizes.push(&mut css, property, value);
                }
            }

            // Alignment: auto margins, which work along either direction.
            // They are written last, so a `margin` on the same element
            // keeps its other sides (`center-x, margin 20`)
            "center-x" => {
                push_css(&mut aligned, "margin-left", "auto");
                push_css(&mut aligned, "margin-right", "auto");
            }
            "center-y" => {
                push_css(&mut aligned, "margin-top", "auto");
                push_css(&mut aligned, "margin-bottom", "auto");
            }
            "align-left" => push_css(&mut aligned, "margin-right", "auto"),
            "align-right" => push_css(&mut aligned, "margin-left", "auto"),
            "align-top" => push_css(&mut aligned, "margin-bottom", "auto"),
            "align-bottom" => push_css(&mut aligned, "margin-top", "auto"),

            "wrap" if lays_out_children => push_css(&mut css, "flex-wrap", "wrap"),
            "wrap" => {}

            // Grid
            "grid-cols" | "grid-rows" if !lays_out_children => {}
            "grid-cols" | "grid-rows" => {
                if let Some(v) = val {
                    let property = if effective_key == "grid-cols" {
                        "grid-template-columns"
                    } else {
                        "grid-template-rows"
                    };
                    match v.parse::<u32>() {
                        Ok(n) => push_css(&mut css, property, &format!("repeat({},1fr)", n)),
                        Err(_) => push_css(&mut css, property, &with_px(property, v)),
                    }
                }
            }
            "col-span" | "row-span" => {
                if let Some(v) = val {
                    let property = if effective_key == "col-span" {
                        "grid-column"
                    } else {
                        "grid-row"
                    };
                    push_css(&mut css, property, &format!("span {}", v));
                }
            }

            // CSS's `line-clamp` has no support yet without its
            // `-webkit-` fallback, which needs a box to clamp
            "line-clamp" => {
                if let Some(v) = val {
                    push_css(&mut css, "display", "-webkit-box");
                    push_css(&mut css, "-webkit-line-clamp", v);
                    push_css(&mut css, "-webkit-box-orient", "vertical");
                    push_css(&mut css, "overflow", "hidden");
                }
            }

            // Every other name is a CSS property (standard, custom,
            // vendor-prefixed, or one htmlang doesn't know, which the
            // parser warned about), written under its own name with the
            // value as written, except for `px` after a bare number where
            // the property takes a length
            key if crate::vocab::is_property_name(key) => {
                if let Some(v) = val {
                    push_css(&mut css, key, &with_px(key, v));
                }
            }

            _ => {}
        }
    }

    css.push_str(&aligned);
    css
}

fn push_css(css: &mut String, prop: &str, value: &str) {
    css.push_str(prop);
    css.push(':');
    css.push_str(value);
    css.push(';');
}

fn extract_id_class(attrs: &[Attribute]) -> (Option<String>, Option<String>) {
    let mut id = None;
    let mut class = None;
    for attr in attrs.iter().filter(|a| a.html) {
        match attr.key.as_str() {
            "id" => id = attr.value.clone(),
            "class" => class = attr.value.clone(),
            _ => {}
        }
    }
    (id, class)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Source map generation (standard v3 format with VLQ-encoded mappings)
// ---------------------------------------------------------------------------

/// Generate a standard v3 source map with VLQ-encoded mappings.
/// Compatible with browser devtools and source map tooling.
pub fn generate_source_map(doc: &Document, source_file: &str) -> String {
    source_map_for_html(&generate_dev(doc), source_file)
}

/// Build a source map for dev-mode HTML by reading the `data-hl-line`
/// markers each element carries, so generated line numbers are exact.
pub fn source_map_for_html(html: &str, source_file: &str) -> String {
    let mut mappings: Vec<(usize, usize)> = Vec::new(); // (html_line, hl_line)
    for (idx, line) in html.lines().enumerate() {
        if let Some(pos) = line.find("data-hl-line=\"") {
            let digits: String = line[pos + "data-hl-line=\"".len()..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(hl_line) = digits.parse::<usize>() {
                mappings.push((idx + 1, hl_line));
            }
        }
    }

    // Build VLQ-encoded mappings string.
    // Each generated line is separated by ';'. Each segment within a line is
    // separated by ','. A segment has 4 fields: generated column, source index,
    // source line, source column — all VLQ-encoded as deltas.
    let max_gen_line = mappings.last().map(|m| m.0).unwrap_or(0);
    let mut vlq = String::new();
    let mut prev_source_line: i64 = 0;
    let mut mapping_idx = 0;

    for gen_line in 1..=max_gen_line {
        if gen_line > 1 {
            vlq.push(';');
        }
        if mapping_idx < mappings.len() && mappings[mapping_idx].0 == gen_line {
            let source_line = mappings[mapping_idx].1 as i64 - 1; // 0-based
            // Segment: gen_col=0, source_idx=0, source_line=delta, source_col=0
            vlq_encode(0, &mut vlq); // generated column (always 0)
            vlq_encode(0, &mut vlq); // source file index (always 0)
            vlq_encode(source_line - prev_source_line, &mut vlq); // source line delta
            vlq_encode(0, &mut vlq); // source column (always 0)
            prev_source_line = source_line;
            mapping_idx += 1;
        }
    }

    let escaped_file = source_file
        .replace(".hl", ".html")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let escaped_source = source_file.replace('\\', "\\\\").replace('"', "\\\"");

    format!(
        "{{\"version\":3,\"file\":\"{}\",\"sourceRoot\":\"\",\"sources\":[\"{}\"],\"names\":[],\"mappings\":\"{}\"}}",
        escaped_file, escaped_source, vlq
    )
}

/// Encode a single signed integer as a VLQ base64 string, appending to `out`.
fn vlq_encode(value: i64, out: &mut String) {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut v = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    } as u64;
    loop {
        let mut digit = (v & 0x1F) as u8; // 5-bit chunk
        v >>= 5;
        if v > 0 {
            digit |= 0x20; // continuation bit
        }
        out.push(B64[digit as usize] as char);
        if v == 0 {
            break;
        }
    }
}
