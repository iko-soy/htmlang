use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::ast::*;

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

struct StyleEntry {
    class_name: String,
    base: String,
    /// (CSS selector suffix, css_rules) — e.g. (":hover", "color:red;")
    pseudo: Vec<(String, String)>,
    /// Responsive overrides: (breakpoint_prefix, css)
    responsive: Vec<(String, String)>,
    /// Dark mode overrides
    dark: String,
    /// Print overrides
    print: String,
    motion_safe: String,
    motion_reduce: String,
    landscape: String,
    portrait: String,
    /// Container query overrides: (breakpoint_prefix, css)
    container: Vec<(String, String)>,
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
    /// the block of a condition: `(prefix, selector, body)`, e.g.
    /// `("md:", ":where(.a)>.b", "flex:1;")` (see [`Flow`]).
    keyed: Vec<(&'static str, String, String)>,
    /// The keyed rules already added
    keyed_seen: std::collections::HashSet<(&'static str, String, String)>,
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

    /// Add a rule for `selector` under the condition `prefix`, once.
    fn add_keyed(&mut self, prefix: &'static str, selector: String, body: String) {
        let rule = (prefix, selector, body);
        if self.keyed_seen.insert(rule.clone()) {
            self.keyed.push(rule);
        }
    }

    /// The keyed rules under `prefix`, as `(selector, body)`.
    fn keyed_under(&self, prefix: &str) -> Vec<(&str, &str)> {
        self.keyed
            .iter()
            .filter(|(p, _, _)| *p == prefix)
            .map(|(_, selector, body)| (selector.as_str(), body.as_str()))
            .collect()
    }

    /// Returns a class name for this style combination, or None if all empty.
    #[allow(clippy::too_many_arguments)]
    fn get_class(
        &mut self,
        base: String,
        pseudo: Vec<(String, String)>,
        responsive: Vec<(String, String)>,
        dark: String,
        print: String,
        motion_safe: String,
        motion_reduce: String,
        landscape: String,
        portrait: String,
        container: Vec<(String, String)>,
        distinct: String,
    ) -> Option<String> {
        if base.is_empty()
            && pseudo.is_empty()
            && responsive.is_empty()
            && dark.is_empty()
            && print.is_empty()
            && motion_safe.is_empty()
            && motion_reduce.is_empty()
            && landscape.is_empty()
            && portrait.is_empty()
            && container.is_empty()
        {
            return None;
        }
        use std::collections::hash_map::DefaultHasher;
        let mut h = DefaultHasher::new();
        base.hash(&mut h);
        pseudo.hash(&mut h);
        responsive.hash(&mut h);
        dark.hash(&mut h);
        print.hash(&mut h);
        motion_safe.hash(&mut h);
        motion_reduce.hash(&mut h);
        landscape.hash(&mut h);
        portrait.hash(&mut h);
        container.hash(&mut h);
        distinct.hash(&mut h);
        let sig = h.finish();

        if let Some(indices) = self.index.get(&sig) {
            for &idx in indices {
                let e = &self.entries[idx];
                if e.base == base
                    && e.pseudo == pseudo
                    && e.responsive == responsive
                    && e.dark == dark
                    && e.print == print
                    && e.motion_safe == motion_safe
                    && e.motion_reduce == motion_reduce
                    && e.landscape == landscape
                    && e.portrait == portrait
                    && e.container == container
                    && e.distinct == distinct
                {
                    return Some(e.class_name.clone());
                }
            }
        }
        let idx = self.entries.len();
        let name = short_class_name(idx);
        self.entries.push(StyleEntry {
            class_name: name.clone(),
            base,
            pseudo,
            responsive,
            dark,
            print,
            motion_safe,
            motion_reduce,
            landscape,
            portrait,
            container,
            distinct,
        });
        self.index.entry(sig).or_default().push(idx);
        Some(name)
    }

    /// All generated rules, wrapped in `@layer htmlang`.
    fn to_css_formatted(&self, dev: bool) -> String {
        let mut css = String::new();
        let inner_indent = if dev { "  " } else { "" };
        css.push_str(if dev {
            "@layer htmlang {\n"
        } else {
            "@layer htmlang{"
        });

        // Non-responsive rules. Base and pseudo rules are merged separately by
        // identical body so that e.g. `.a,.b{display:flex;flex-direction:column;}`
        // replaces two identical rules. Pseudo variants are grouped per selector
        // suffix (`:hover` with `:hover`, etc.) to keep each pseudo's cascade
        // position independent of others.
        let mut base_pairs: Vec<(&str, &str)> = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            if !e.base.is_empty() {
                base_pairs.push((e.class_name.as_str(), e.base.as_str()));
            }
        }
        emit_grouped_rules(&mut css, &base_pairs, "", inner_indent, dev);

        // Collect pseudo rules grouped by selector suffix, preserving the order
        // in which suffixes first appear across entries.
        let mut pseudo_order: Vec<&str> = Vec::new();
        let mut pseudo_buckets: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for e in &self.entries {
            for (selector, body) in &e.pseudo {
                if body.is_empty() {
                    continue;
                }
                let key = selector.as_str();
                if !pseudo_buckets.contains_key(key) {
                    pseudo_order.push(key);
                }
                pseudo_buckets
                    .entry(key)
                    .or_default()
                    .push((e.class_name.as_str(), body.as_str()));
            }
        }
        for selector in &pseudo_order {
            if let Some(pairs) = pseudo_buckets.get(selector) {
                emit_grouped_rules(&mut css, pairs, selector, inner_indent, dev);
            }
        }

        // Responsive rules grouped by breakpoint
        for &(bp_name, bp_width) in BREAKPOINTS {
            let mut bp_pairs: Vec<(&str, &str)> = Vec::new();
            for e in &self.entries {
                for (bp, rule_css) in &e.responsive {
                    if bp == bp_name && !rule_css.is_empty() {
                        bp_pairs.push((e.class_name.as_str(), rule_css.as_str()));
                    }
                }
            }
            emit_media_block(
                &mut css,
                &format!("@media (min-width: {})", bp_width),
                &format!("@media(min-width:{})", bp_width),
                &self.keyed_under(&format!("{}:", bp_name)),
                &bp_pairs,
                dev,
            );
        }

        // Dark mode rules
        let dark_pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|e| !e.dark.is_empty())
            .map(|e| (e.class_name.as_str(), e.dark.as_str()))
            .collect();
        emit_media_block(
            &mut css,
            "@media (prefers-color-scheme: dark)",
            "@media(prefers-color-scheme:dark)",
            &self.keyed_under("dark:"),
            &dark_pairs,
            dev,
        );

        // Print rules
        let print_pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|e| !e.print.is_empty())
            .map(|e| (e.class_name.as_str(), e.print.as_str()))
            .collect();
        emit_media_block(
            &mut css,
            "@media print",
            "@media print",
            &self.keyed_under("print:"),
            &print_pairs,
            dev,
        );

        // Motion safe rules
        let motion_safe_pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|e| !e.motion_safe.is_empty())
            .map(|e| (e.class_name.as_str(), e.motion_safe.as_str()))
            .collect();
        emit_media_block(
            &mut css,
            "@media (prefers-reduced-motion: no-preference)",
            "@media(prefers-reduced-motion:no-preference)",
            &self.keyed_under("motion-safe:"),
            &motion_safe_pairs,
            dev,
        );

        // Motion reduce rules
        let motion_reduce_pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|e| !e.motion_reduce.is_empty())
            .map(|e| (e.class_name.as_str(), e.motion_reduce.as_str()))
            .collect();
        emit_media_block(
            &mut css,
            "@media (prefers-reduced-motion: reduce)",
            "@media(prefers-reduced-motion:reduce)",
            &self.keyed_under("motion-reduce:"),
            &motion_reduce_pairs,
            dev,
        );

        // Landscape rules
        let landscape_pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|e| !e.landscape.is_empty())
            .map(|e| (e.class_name.as_str(), e.landscape.as_str()))
            .collect();
        emit_media_block(
            &mut css,
            "@media (orientation: landscape)",
            "@media(orientation:landscape)",
            &self.keyed_under("landscape:"),
            &landscape_pairs,
            dev,
        );

        // Portrait rules
        let portrait_pairs: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|e| !e.portrait.is_empty())
            .map(|e| (e.class_name.as_str(), e.portrait.as_str()))
            .collect();
        emit_media_block(
            &mut css,
            "@media (orientation: portrait)",
            "@media(orientation:portrait)",
            &self.keyed_under("portrait:"),
            &portrait_pairs,
            dev,
        );

        // Container query rules grouped by breakpoint
        for &(bp_name, bp_width) in BREAKPOINTS {
            let mut cq_pairs: Vec<(&str, &str)> = Vec::new();
            for e in &self.entries {
                for (bp, rule_css) in &e.container {
                    if bp == bp_name && !rule_css.is_empty() {
                        cq_pairs.push((e.class_name.as_str(), rule_css.as_str()));
                    }
                }
            }
            emit_media_block(
                &mut css,
                &format!("@container (min-width: {})", bp_width),
                &format!("@container(min-width:{})", bp_width),
                &self.keyed_under(&format!("cq-{}:", bp_name)),
                &cq_pairs,
                dev,
            );
        }

        css.push_str(if dev { "}\n" } else { "}" });
        css
    }
}

/// Emit CSS rules from `(class_name, body)` pairs, merging identical bodies
/// into a single selector-list rule (e.g. `.a,.b{body}`). The first occurrence
/// of each distinct body determines ordering, so output stays deterministic
/// across runs. `selector_suffix` is appended to each class (e.g. `":hover"`,
/// or `""` for plain class rules). `indent` is prepended to each rule line.
fn emit_grouped_rules(
    out: &mut String,
    pairs: &[(&str, &str)],
    selector_suffix: &str,
    indent: &str,
    dev: bool,
) {
    if pairs.is_empty() {
        return;
    }
    // Group in first-occurrence order.
    let mut order: Vec<&str> = Vec::new();
    let mut groups: HashMap<&str, Vec<&str>> = HashMap::new();
    for &(name, body) in pairs {
        if body.is_empty() {
            continue;
        }
        if !groups.contains_key(body) {
            order.push(body);
        }
        groups.entry(body).or_default().push(name);
    }
    let sp = if dev { " " } else { "" };
    let nl = if dev { "\n" } else { "" };
    for body in &order {
        let names = &groups[body];
        out.push_str(indent);
        for (i, n) in names.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('.');
            out.push_str(n);
            out.push_str(selector_suffix);
        }
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

/// Emit an `@media` / `@container` block containing the rules keyed on a
/// parent's class (`(selector, body)`), then grouped class rules. Skips the
/// block entirely if no non-empty bodies are present.
fn emit_media_block(
    out: &mut String,
    header_dev: &str,
    header_min: &str,
    keyed: &[(&str, &str)],
    pairs: &[(&str, &str)],
    dev: bool,
) {
    if keyed.is_empty() && pairs.iter().all(|(_, body)| body.is_empty()) {
        return;
    }
    let mut inner = String::new();
    let inner_indent = if dev { "  " } else { "" };
    // Keyed rules come first: they have the specificity of one class, like
    // a class rule, so the child's own rules in the block still win
    emit_selector_rules(&mut inner, keyed, inner_indent, dev);
    emit_grouped_rules(&mut inner, pairs, "", inner_indent, dev);
    if inner.is_empty() {
        return;
    }
    if dev {
        out.push_str(&format!("{} {{\n{}}}\n", header_dev, inner));
    } else {
        out.push_str(&format!("{}{{{}}}", header_min, inner));
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
        own: &own,
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
        if dev {
            "a:focus-visible, button:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible { outline: 2px solid currentColor; outline-offset: 2px; }\n"
        } else {
            "a:focus-visible,button:focus-visible,input:focus-visible,select:focus-visible,textarea:focus-visible{outline:2px solid currentColor;outline-offset:2px}"
        }
    } else {
        ""
    };
    let reset_css = reset_css(dev, focus_visible_css);

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

    // Generated rules always go in `@layer htmlang`, so unlayered user CSS
    // (`@style`, `@raw`) overrides them regardless of specificity.
    let styles_css = styles.to_css_formatted(dev);

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

/// Built-in reset rules, in a layer before `htmlang`: unlayered CSS beats
/// every layer, so an unlayered `a{color:inherit}` would override
/// `@link [color red]`.
fn reset_css(dev: bool, focus_visible_css: &str) -> String {
    let base = if dev {
        "*, *::before, *::after { box-sizing: border-box; }\nbody { margin: 0; font-family: system-ui, -apple-system, sans-serif; display: flex; flex-direction: column; min-height: 100dvh; }\nimg { display: block; }\na { text-decoration: none; color: inherit; }\n"
    } else {
        "*,*::before,*::after{box-sizing:border-box}body{margin:0;font-family:system-ui,-apple-system,sans-serif;display:flex;flex-direction:column;min-height:100dvh}img{display:block}a{text-decoration:none;color:inherit}"
    };
    let rules = format!("{}{}", base, focus_visible_css);
    if dev {
        format!(
            "@layer hl-reset, htmlang;\n@layer hl-reset {{\n{}}}\n",
            rules
        )
    } else {
        format!("@layer hl-reset,htmlang;@layer hl-reset{{{}}}", rules)
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

    let element_css = build_element_css(doc, &styles, dev);

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
    let gen_class = element_class(elem, &Site::new(elem, parent, &ctx.flow, &own), styles);
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
    let own = Flow::default();
    let gen_class = element_class(elem, &Site::new(elem, parent, &ctx.flow, &own), styles);
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
    let base = attrs_to_css(attrs, "", site);

    // Collect pseudo-state overrides
    let mut pseudo = Vec::new();
    for &(prefix, selector) in crate::vocab::PSEUDO_PREFIXES {
        let css = attrs_to_css(attrs, prefix, site);
        if !css.is_empty() {
            pseudo.push((selector.to_string(), css));
        }
    }

    // Collect nth:EXPR: dynamic pseudo selectors
    let mut nth_prefixes: Vec<String> = Vec::new();
    for attr in attrs {
        if attr.key.starts_with("nth:") {
            let rest = &attr.key[4..];
            if let Some(colon_pos) = rest.find(':') {
                let prefix = format!("nth:{}:", &rest[..colon_pos]);
                if !nth_prefixes.contains(&prefix) {
                    nth_prefixes.push(prefix);
                }
            }
        }
    }
    for prefix in &nth_prefixes {
        let expr = &prefix[4..prefix.len() - 1];
        let selector = format!(":nth-child({})", expr);
        let css = attrs_to_css(attrs, prefix, site);
        if !css.is_empty() {
            pseudo.push((selector, css));
        }
    }

    // Collect has(...): dynamic pseudo selectors
    let mut has_prefixes: Vec<String> = Vec::new();
    for attr in attrs {
        if attr.key.starts_with("has(")
            && let Some(close) = attr.key.find("):")
        {
            let prefix = format!("{}:", &attr.key[..close + 1]);
            if !has_prefixes.contains(&prefix) {
                has_prefixes.push(prefix);
            }
        }
    }
    for prefix in &has_prefixes {
        let inner = &prefix[4..prefix.len() - 2]; // extract selector from has(selector):
        let selector = format!(":has({})", inner);
        let css = attrs_to_css(attrs, prefix, site);
        if !css.is_empty() {
            pseudo.push((selector, css));
        }
    }

    // Collect responsive overrides
    let mut responsive = Vec::new();
    for &(bp_name, _) in BREAKPOINTS {
        let prefix = format!("{}:", bp_name);
        let css = attrs_to_css(attrs, &prefix, site);
        if !css.is_empty() {
            responsive.push((bp_name.to_string(), css));
        }
    }

    // Collect container query overrides
    let mut container = Vec::new();
    for &(bp_name, _) in BREAKPOINTS {
        let prefix = format!("cq-{}:", bp_name);
        let css = attrs_to_css(attrs, &prefix, site);
        if !css.is_empty() {
            container.push((bp_name.to_string(), css));
        }
    }

    let dark = attrs_to_css(attrs, "dark:", site);
    let print = attrs_to_css(attrs, "print:", site);
    let motion_safe = attrs_to_css(attrs, "motion-safe:", site);
    let motion_reduce = attrs_to_css(attrs, "motion-reduce:", site);
    let landscape = attrs_to_css(attrs, "landscape:", site);
    let portrait = attrs_to_css(attrs, "portrait:", site);

    // Dedupe: if a property is declared twice within a single rule, keep only
    // the last occurrence (element-kind defaults are written before
    // user attributes, so a user [list-style disc] correctly overrides the
    // default list-style:none, and we don't need to ship both).
    let base = dedupe_declarations(&base);
    let pseudo: Vec<(String, String)> = pseudo
        .into_iter()
        .map(|(sel, css)| (sel, dedupe_declarations(&css)))
        .collect();
    let responsive: Vec<(String, String)> = responsive
        .into_iter()
        .map(|(bp, css)| (bp, dedupe_declarations(&css)))
        .collect();
    let dark = dedupe_declarations(&dark);
    let print = dedupe_declarations(&print);
    let motion_safe = dedupe_declarations(&motion_safe);
    let motion_reduce = dedupe_declarations(&motion_reduce);
    let landscape = dedupe_declarations(&landscape);
    let portrait = dedupe_declarations(&portrait);
    let container: Vec<(String, String)> = container
        .into_iter()
        .map(|(bp, css)| (bp, dedupe_declarations(&css)))
        .collect();

    styles.get_class(
        base,
        pseudo,
        responsive,
        dark,
        print,
        motion_safe,
        motion_reduce,
        landscape,
        portrait,
        container,
        distinct,
    )
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
    /// How it lays out its own children, for its `children:` styles.
    own: &'a Flow,
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
    fn new<'a>(
        elem: &'a Element,
        layout: Option<Layout>,
        parent: &'a Flow,
        own: &'a Flow,
    ) -> Site<'a> {
        Site {
            kind: &elem.kind,
            parent,
            own,
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

/// The prefixes whose styles are written in a block of their own (`@media`
/// or `@container`), in the order the blocks are written. A parent's
/// `flex-direction` under one of them changes its children's layout words
/// in that block.
fn block_prefixes() -> impl Iterator<Item = &'static str> {
    crate::vocab::RESPONSIVE_PREFIXES
        .iter()
        .chain(crate::vocab::MEDIA_PREFIXES)
        .chain(crate::vocab::CONTAINER_QUERY_PREFIXES)
        .copied()
}

/// The prefixes whose styles apply whenever `prefix`'s do, in the order
/// the cascade applies them: no prefix, the smaller widths of the same kind
/// (`md:` holds from 768px up, so `sm:` holds there too), then `prefix`.
fn in_effect(prefix: &str) -> Vec<&str> {
    if prefix.is_empty() {
        return vec![""];
    }
    // `children:` styles are another element's
    if prefix == "children:" {
        return vec![prefix];
    }
    let mut prefixes = vec![""];
    for widths in [
        crate::vocab::RESPONSIVE_PREFIXES,
        crate::vocab::CONTAINER_QUERY_PREFIXES,
    ] {
        if let Some(i) = widths.iter().position(|p| *p == prefix) {
            prefixes.extend_from_slice(&widths[..i]);
        }
    }
    prefixes.push(prefix);
    prefixes
}

/// An attribute's prefix (`""` for none) and the rest of its key.
fn split_prefix(key: &str) -> (&str, &str) {
    match crate::vocab::prefix_len(key) {
        Some(len) => key.split_at(len),
        None => ("", key),
    }
}

/// How an element lays out its children, as its CSS sets it: the
/// direction its children's `width`/`height` `fill` and `shrink` compile
/// against. That is the direction of `@row` or of a column, or the one its
/// own `flex-direction` (or `flex-flow`) sets, with or without a media or
/// container prefix. Like elm-ui's `.r > .wf`, the rules for a direction
/// set under a prefix are keyed on the element's class: `@media (...) {
/// :where(.a)>.b {...} }`.
#[derive(Clone, Default)]
struct Flow {
    /// Without a prefix; `None` when the element isn't a flex row or column
    /// (a grid, text, HTML's own layout, the top of the page)
    base: Option<Axis>,
    /// The direction under each block prefix that sets one, in block order
    changes: Vec<(&'static str, Axis)>,
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
        let mut changes: Vec<(&'static str, Axis)> = Vec::new();
        for attr in elem.attrs.iter().filter(|a| !a.html) {
            let (prefix, name) = split_prefix(&attr.key);
            if !matches!(name, "flex-direction" | "flex-flow") {
                continue;
            }
            let Some(axis) = attr.value.as_deref().and_then(Axis::of_value) else {
                continue;
            };
            if prefix.is_empty() {
                base = axis;
            } else if let Some(prefix) = block_prefixes().find(|p| *p == prefix) {
                // The later one wins, as in the CSS
                changes.retain(|(p, _)| *p != prefix);
                changes.push((prefix, axis));
            }
        }
        changes.sort_by_key(|(p, _)| block_prefixes().position(|q| q == *p));
        Flow {
            base: Some(base),
            changes,
            class: None,
        }
    }

    /// The direction under `prefix`: the latest one set among the prefixes
    /// in effect there (a state such as `hover:` keeps the direction without
    /// a prefix).
    fn at(&self, prefix: &str) -> Option<Axis> {
        let base = self.base?;
        let changed = in_effect(prefix).into_iter().rev().find_map(|p| {
            self.changes
                .iter()
                .find(|(q, _)| *q == p)
                .map(|&(_, axis)| axis)
        });
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
    /// Read `attrs` under `prefixes`, a later prefix over an earlier one.
    fn of(attrs: &[Attribute], prefixes: &[&str]) -> Sizes {
        let mut sizes = Sizes::default();
        for &want in prefixes {
            for attr in attrs.iter().filter(|a| !a.html) {
                let (prefix, name) = split_prefix(&attr.key);
                let Some(value) = attr.value.as_deref().filter(|v| !v.trim().is_empty()) else {
                    continue;
                };
                if prefix != want {
                    continue;
                }
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
/// under `prefixes` set them, their meaning in a parent laid out along
/// `axis`. Every other property a layout word could have set is put back to
/// its initial value, so the rule is right whichever earlier rule it
/// overrides. Properties the element writes itself are left alone.
fn sizing_rule(attrs: &[Attribute], prefixes: &[&str], axis: Axis) -> String {
    let sizes = Sizes::of(attrs, prefixes);
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

/// The rules an element's `fill` and `shrink` need under each prefix where
/// the direction of `flow` (its parent's) changes: `(prefix, body)`. With
/// `children`, for the `children:` styles of the element whose flow it is.
fn sizing_changes(attrs: &[Attribute], flow: &Flow, children: bool) -> Vec<(&'static str, String)> {
    flow.changes
        .iter()
        .map(|&(prefix, axis)| {
            let body = if children {
                sizing_rule(attrs, &["children:"], axis)
            } else {
                sizing_rule(attrs, &in_effect(prefix), axis)
            };
            (prefix, body)
        })
        .filter(|(_, body)| !body.is_empty())
        .collect()
}

/// An element's generated class, with the rules keyed on classes that its
/// `fill` and `shrink` need where a direction changes under a prefix (see
/// [`Flow`]): its own, keyed on its parent's class (`:where(.a)>.b`, one
/// class of specificity like any class rule), and its `children:` styles',
/// keyed on its own (`.b>*`, like `children:`'s own `.b > *`).
fn element_class(elem: &Element, site: &Site, styles: &mut StyleCollector) -> Option<String> {
    let as_child = match site.parent.class {
        Some(_) => sizing_changes(&elem.attrs, site.parent, false),
        None => Vec::new(),
    };
    let for_children = sizing_changes(&elem.attrs, site.own, true);
    // Two elements with the same CSS but different rules keyed on them
    // need two classes
    let mut distinct = String::new();
    for (list, mark) in [(&as_child, '<'), (&for_children, '>')] {
        for (prefix, body) in list {
            distinct.push(mark);
            distinct.push_str(prefix);
            distinct.push_str(body);
        }
    }
    let class = compute_class(&elem.attrs, site, styles, distinct)?;
    if let Some(parent) = &site.parent.class {
        for (prefix, body) in as_child {
            styles.add_keyed(prefix, format!(":where(.{})>.{}", parent, class), body);
        }
    }
    for (prefix, body) in for_children {
        styles.add_keyed(prefix, format!(".{}>*", class), body);
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

fn attrs_to_css(attrs: &[Attribute], state_prefix: &str, site: &Site) -> String {
    let mut css = String::new();
    // The auto margins of `center-x`, `align-left`, ..., written at the end
    let mut aligned = String::new();
    let kind = site.kind;
    // The direction `fill` and `shrink` compile against: `children:` styles
    // go on the children, whose parent is this element
    let axis = if state_prefix == "children:" {
        site.own.base
    } else {
        site.parent.at(state_prefix)
    };

    // Base element styles only for the default (non-state) pass
    if state_prefix.is_empty() {
        // Elements with @in-front / @behind children become positioning
        // contexts. Pushed before user attrs so an explicit `position` wins
        // via dedupe (only the last declaration of a property is kept).
        if site.has_overlay_children {
            css.push_str("position:relative;isolation:isolate;");
        }
        if !site.root {
            css.push_str(layout_css(kind.layout(), site.inline));
        }
        css.push_str(kind.css());
    }
    // htmlang's words for laying out children (`spacing`, `wrap`,
    // `grid-cols`) mean nothing on an element that doesn't lay out its
    // children, which the parser reports; they are left out. Under
    // `children:` they go on the children, whose layout isn't known here.
    let lays_out_children = kind.layout().is_container() || state_prefix == "children:";

    for (index, attr) in attrs.iter().enumerate() {
        if attr.html {
            continue;
        }
        // Determine the effective key for this pass
        let effective_key = if state_prefix.is_empty() {
            if crate::vocab::is_prefixed(&attr.key) {
                continue;
            }
            attr.key.as_str()
        } else {
            match attr.key.strip_prefix(state_prefix) {
                Some(k) => k,
                None => continue,
            }
        };

        let val = attr.value.as_deref();
        // A style whose value came out empty (a field a record doesn't
        // have, `${if()}` without its other branch) is left out
        if val.is_some_and(|v| v.trim().is_empty()) {
            continue;
        }

        match effective_key {
            // Layout
            "spacing" if !lays_out_children => {}
            "spacing" | "gap" => {
                if let Some(v) = val {
                    push_css(&mut css, "gap", &css_px(v));
                }
            }
            "padding" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding", &css_px_multi(v));
                }
            }
            "padding-top" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-top", &css_px(v));
                }
            }
            "padding-bottom" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-bottom", &css_px(v));
                }
            }
            "padding-left" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-left", &css_px(v));
                }
            }
            "padding-right" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-right", &css_px(v));
                }
            }

            // Sizing: `fill` and `shrink` compile against the parent's
            // direction. Only the last `width` (`height`) counts, as for
            // any style written twice.
            "width" | "height" if val.is_some_and(|v| Size::of(v) != Size::Set) => {
                let later = attrs[index + 1..].iter().any(|a| {
                    !a.html
                        && a.key.strip_prefix(state_prefix) == Some(effective_key)
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
                let sizes = Sizes::of(attrs, &in_effect(state_prefix));
                for &(property, value) in sizing(dim, size, axis) {
                    sizes.push(&mut css, property, value);
                }
            }
            "width" | "height" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }
            "min-width" => {
                if let Some(v) = val {
                    push_css(&mut css, "min-width", &css_px(v));
                }
            }
            "max-width" => {
                if let Some(v) = val {
                    push_css(&mut css, "max-width", &css_px(v));
                }
            }
            "min-height" => {
                if let Some(v) = val {
                    push_css(&mut css, "min-height", &css_px(v));
                }
            }
            "max-height" => {
                if let Some(v) = val {
                    push_css(&mut css, "max-height", &css_px(v));
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

            // Typography
            "letter-spacing" => {
                if let Some(v) = val {
                    push_css(&mut css, "letter-spacing", &css_px(v));
                }
            }

            // Overflow & positioning
            "top" => {
                if let Some(v) = val {
                    push_css(&mut css, "top", &css_px(v));
                }
            }
            "right" => {
                if let Some(v) = val {
                    push_css(&mut css, "right", &css_px(v));
                }
            }
            "bottom" => {
                if let Some(v) = val {
                    push_css(&mut css, "bottom", &css_px(v));
                }
            }
            "left" => {
                if let Some(v) = val {
                    push_css(&mut css, "left", &css_px(v));
                }
            }

            // Effects

            // Flow
            "wrap" if lays_out_children => push_css(&mut css, "flex-wrap", "wrap"),
            "wrap" => {}

            // Grid
            "grid-cols" | "grid-rows" if !lays_out_children => {}
            "grid-cols" => {
                if let Some(v) = val {
                    if let Ok(n) = v.parse::<u32>() {
                        push_css(
                            &mut css,
                            "grid-template-columns",
                            &format!("repeat({},1fr)", n),
                        );
                    } else {
                        push_css(&mut css, "grid-template-columns", v);
                    }
                }
            }
            "grid-rows" => {
                if let Some(v) = val {
                    if let Ok(n) = v.parse::<u32>() {
                        push_css(
                            &mut css,
                            "grid-template-rows",
                            &format!("repeat({},1fr)", n),
                        );
                    } else {
                        push_css(&mut css, "grid-template-rows", v);
                    }
                }
            }
            "col-span" => {
                if let Some(v) = val {
                    push_css(&mut css, "grid-column", &format!("span {}", v));
                }
            }
            "row-span" => {
                if let Some(v) = val {
                    push_css(&mut css, "grid-row", &format!("span {}", v));
                }
            }

            // Outline (like border but doesn't affect layout)
            "outline" => {
                if let Some(v) = val {
                    let parts: Vec<&str> = v.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        push_css(
                            &mut css,
                            "outline",
                            &format!("{} solid {}", css_px(parts[0]), parts[1]),
                        );
                    } else {
                        push_css(
                            &mut css,
                            "outline",
                            &format!("{} solid currentColor", css_px(parts[0])),
                        );
                    }
                }
            }

            // Logical properties (i18n-aware)
            "padding-inline" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-inline", &css_px_multi(v));
                }
            }
            "padding-block" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-block", &css_px_multi(v));
                }
            }
            "margin-inline" => {
                if let Some(v) = val {
                    push_css(&mut css, "margin-inline", &css_px_multi(v));
                }
            }
            "margin-block" => {
                if let Some(v) = val {
                    push_css(&mut css, "margin-block", &css_px_multi(v));
                }
            }

            // Logical property start/end variants
            "padding-inline-start"
            | "padding-inline-end"
            | "padding-block-start"
            | "padding-block-end"
            | "margin-inline-start"
            | "margin-inline-end"
            | "margin-block-start"
            | "margin-block-end" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }

            // Logical inset
            "inset-inline" | "inset-block" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px_multi(v));
                }
            }
            "inset-inline-start" | "inset-inline-end" | "inset-block-start" | "inset-block-end" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }

            // Logical border
            "border-inline"
            | "border-block"
            | "border-inline-start"
            | "border-inline-end"
            | "border-block-start"
            | "border-block-end" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, v);
                }
            }

            // Logical border-radius
            "border-start-start-radius"
            | "border-start-end-radius"
            | "border-end-start-radius"
            | "border-end-end-radius" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }

            // Logical scroll margins & padding
            "scroll-margin-inline"
            | "scroll-margin-block"
            | "scroll-padding-inline"
            | "scroll-padding-block" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px_multi(v));
                }
            }

            // Logical sizing
            "inline-size" | "block-size" | "min-inline-size" | "max-inline-size"
            | "min-block-size" | "max-block-size" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }

            // Margin
            "margin" => {
                if let Some(v) = val {
                    push_css(&mut css, "margin", &css_px_multi(v));
                }
            }
            // Inset (shorthand for top/right/bottom/left)
            "inset" => {
                if let Some(v) = val {
                    push_css(&mut css, "inset", &css_px(v));
                }
            }

            // Table
            "border-spacing" => {
                if let Some(v) = val {
                    push_css(&mut css, "border-spacing", &css_px(v));
                }
            }

            // Text decoration
            "text-decoration-thickness" => {
                if let Some(v) = val {
                    push_css(&mut css, "text-decoration-thickness", &css_px(v));
                }
            }
            "text-underline-offset" => {
                if let Some(v) = val {
                    push_css(&mut css, "text-underline-offset", &css_px(v));
                }
            }

            // Multi-column
            "column-width" => {
                if let Some(v) = val {
                    push_css(&mut css, "column-width", &css_px(v));
                }
            }

            // New CSS properties
            "column-gap" => {
                if let Some(v) = val {
                    push_css(&mut css, "column-gap", &css_px(v));
                }
            }
            "text-indent" => {
                if let Some(v) = val {
                    push_css(&mut css, "text-indent", &css_px(v));
                }
            }
            "flex-basis" => {
                if let Some(v) = val {
                    push_css(&mut css, "flex-basis", &css_px(v));
                }
            }
            "scroll-margin" => {
                if let Some(v) = val {
                    push_css(&mut css, "scroll-margin", &css_px(v));
                }
            }
            "scroll-margin-top"
            | "scroll-margin-bottom"
            | "scroll-margin-left"
            | "scroll-margin-right" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }
            "scroll-padding" => {
                if let Some(v) = val {
                    push_css(&mut css, "scroll-padding", &css_px(v));
                }
            }
            "scroll-padding-top"
            | "scroll-padding-bottom"
            | "scroll-padding-left"
            | "scroll-padding-right" => {
                if let Some(v) = val {
                    push_css(&mut css, effective_key, &css_px(v));
                }
            }

            // --- CSS Shorthands ---
            "line-clamp" => {
                if let Some(v) = val {
                    push_css(&mut css, "display", "-webkit-box");
                    push_css(&mut css, "-webkit-line-clamp", v);
                    push_css(&mut css, "-webkit-box-orient", "vertical");
                    push_css(&mut css, "overflow", "hidden");
                }
            }

            // Any other standard CSS property is copied through, with `px`
            // added to bare numbers where the property takes a length.
            key if crate::vocab::is_css_property(key) => {
                if let Some(v) = val {
                    if crate::vocab::is_length_property(key) {
                        push_css(&mut css, key, &css_px_multi(v));
                    } else {
                        push_css(&mut css, key, v);
                    }
                }
            }

            // A name htmlang doesn't know but CSS could have (a custom
            // property, a vendor-prefixed or a new property) is written as
            // it is; the parser warned about an unknown one
            key if crate::vocab::is_property_name(key) => {
                if let Some(v) = val {
                    push_css(&mut css, key, v);
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

/// Known CSS units — if a value ends with one, skip appending `px`.
/// Format a length value: a bare number gets `px` appended; anything else
/// (values with units, keywords like `auto`, functions like `calc(...)`) is
/// passed through unchanged.
fn css_px(value: &str) -> String {
    let v = value.trim();
    if v == "0" {
        return "0".to_string();
    }
    let is_bare_number = !v.is_empty()
        && v.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+'))
        && v.parse::<f64>().is_ok();
    if is_bare_number {
        format!("{}px", v)
    } else {
        v.to_string()
    }
}

/// Format multiple space-separated values, each getting px if needed.
fn css_px_multi(value: &str) -> String {
    value
        .split_whitespace()
        .map(css_px)
        .collect::<Vec<_>>()
        .join(" ")
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
