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
}

struct StyleCollector {
    entries: Vec<StyleEntry>,
    /// Maps a pre-hashed style signature to an index into `entries`.
    /// Using u64 as the key keeps lookups allocation-free; on the rare case of
    /// a hash collision we fall back to a full equality check against the entry.
    index: HashMap<u64, Vec<usize>>,
}

impl StyleCollector {
    fn new() -> Self {
        StyleCollector {
            entries: Vec::new(),
            index: HashMap::new(),
        }
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
                &bp_pairs,
                "",
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
            &dark_pairs,
            "",
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
            &print_pairs,
            "",
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
            &motion_safe_pairs,
            "",
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
            &motion_reduce_pairs,
            "",
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
            &landscape_pairs,
            "",
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
            &portrait_pairs,
            "",
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
                &cq_pairs,
                "",
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

/// Emit an `@media` / `@container` block containing grouped class rules.
/// Skips the block entirely if no non-empty bodies are present.
fn emit_media_block(
    out: &mut String,
    header_dev: &str,
    header_min: &str,
    pairs: &[(&str, &str)],
    selector_suffix: &str,
    dev: bool,
) {
    if pairs.iter().all(|(_, body)| body.is_empty()) {
        return;
    }
    let mut inner = String::new();
    let inner_indent = if dev { "  " } else { "" };
    emit_grouped_rules(&mut inner, pairs, selector_suffix, inner_indent, dev);
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
    let mut styles = StyleCollector::new();
    let mut ctx = GenContext {
        dev,
        depth: 0,
        has_interactive: false,
        in_text: false,
    };

    let mut body = String::new();

    generate_children(&doc.nodes, None, &mut body, &mut styles, &mut ctx);

    let element_css = build_element_css(doc, &styles, dev);

    // Build meta tags string
    let meta_html = if doc.meta_tags.is_empty() {
        String::new()
    } else {
        let mut m = String::new();
        for (name, content) in &doc.meta_tags {
            if dev {
                m.push_str(&format!(
                    "<meta name=\"{}\" content=\"{}\">\n",
                    html_escape(name),
                    html_escape(content)
                ));
            } else {
                m.push_str(&format!(
                    "<meta name=\"{}\" content=\"{}\">",
                    html_escape(name),
                    html_escape(content)
                ));
            }
        }
        m
    };

    // Build OG meta tags
    let og_html = if doc.og_tags.is_empty() {
        String::new()
    } else {
        let mut o = String::new();
        for (property, content) in &doc.og_tags {
            if dev {
                o.push_str(&format!(
                    "<meta property=\"og:{}\" content=\"{}\">\n",
                    html_escape(property),
                    html_escape(content)
                ));
            } else {
                o.push_str(&format!(
                    "<meta property=\"og:{}\" content=\"{}\">",
                    html_escape(property),
                    html_escape(content)
                ));
            }
        }
        o
    };

    // Build head blocks string
    let head_html = if doc.head_blocks.is_empty() {
        String::new()
    } else {
        let mut h = String::new();
        for block in &doc.head_blocks {
            h.push_str(block);
            if dev {
                h.push('\n');
            }
        }
        h
    };

    let lang_attr = match &doc.lang {
        Some(lang) => format!(" lang=\"{}\"", html_escape(lang)),
        None => String::new(),
    };

    let favicon_html = match &doc.favicon {
        Some(path) => {
            // Try to read and inline the favicon
            if let Ok(data) = std::fs::read(path) {
                let mime = if path.ends_with(".ico") {
                    "image/x-icon"
                } else if path.ends_with(".png") {
                    "image/png"
                } else if path.ends_with(".svg") {
                    "image/svg+xml"
                } else {
                    "image/x-icon"
                };
                let b64 = base64_encode(&data);
                if dev {
                    format!(
                        "<link rel=\"icon\" href=\"data:{};base64,{}\">\n",
                        mime, b64
                    )
                } else {
                    format!("<link rel=\"icon\" href=\"data:{};base64,{}\">", mime, b64)
                }
            } else {
                // Fall back to href
                if dev {
                    format!("<link rel=\"icon\" href=\"{}\">\n", html_escape(path))
                } else {
                    format!("<link rel=\"icon\" href=\"{}\">", html_escape(path))
                }
            }
        }
        None => String::new(),
    };

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

    match &doc.page_title {
        Some(title) => {
            if dev {
                format!(
                    "\
<!DOCTYPE html>
<html{lang_attr}>
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>{title}</title>
{meta_html}{og_html}{favicon_html}{head_html}\
<style>
{reset_css}{element_css}\
</style>
</head>
<body>
{body}\
</body>
</html>
",
                    title = html_escape(title),
                    lang_attr = lang_attr,
                    meta_html = meta_html,
                    favicon_html = favicon_html,
                    head_html = head_html,
                    og_html = og_html,
                    reset_css = reset_css,
                    element_css = element_css,
                    body = body,
                )
            } else {
                format!(
                    "<!DOCTYPE html><html{lang_attr}><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>{meta_html}{og_html}{favicon_html}{head_html}<style>{reset_css}{element_css}</style></head><body>{body}</body></html>",
                    title = html_escape(title),
                    lang_attr = lang_attr,
                    meta_html = meta_html,
                    og_html = og_html,
                    favicon_html = favicon_html,
                    head_html = head_html,
                    reset_css = reset_css,
                    element_css = element_css,
                    body = body,
                )
            }
        }
        None => {
            if element_css.is_empty() {
                body
            } else if dev {
                format!("<style>\n{}</style>\n{}", element_css, body)
            } else {
                format!("<style>{}</style>{}", element_css, body)
            }
        }
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
        "*, *::before, *::after { box-sizing: border-box; }\nbody { margin: 0; font-family: system-ui, -apple-system, sans-serif; }\nimg { display: block; }\na { text-decoration: none; color: inherit; }\n"
    } else {
        "*,*::before,*::after{box-sizing:border-box}body{margin:0;font-family:system-ui,-apple-system,sans-serif}img{display:block}a{text-decoration:none;color:inherit}"
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
    let mut previous: Option<bool> = None;
    for child in children {
        let start = out.len();
        if let Some(previous_is_text) = previous
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
        previous = Some(matches!(child, Node::Text(_)));
    }
}

/// Emit the argument of an element whose argument is an HTML attribute
/// (`@iframe URL` → `src="URL"`, `@form /submit` → `action="/submit"`).
fn emit_argument_attr(out: &mut String, elem: &Element) {
    if let Some(TagSpec {
        arg: TagArg::Attr(attr),
        ..
    }) = elem.kind.spec()
        && let Some(value) = &elem.argument
    {
        out.push(' ');
        out.push_str(attr);
        out.push_str("=\"");
        out.push_str(&html_escape(value));
        out.push('"');
    }
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
    elem.children.iter().any(|child| {
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
    // @script renders as <script> with raw body content (no HTML escaping)
    if elem.kind == ElementKind::Script {
        out.push_str(&ctx.indent());
        out.push_str("<script");
        // Its HTML attributes (`src=`, `type=module`, `defer`, ...); it has
        // no styles, which the parser reports
        let (id, class) = extract_id_class(&elem.attrs);
        for (key, value) in [("id", id), ("class", class)] {
            if let Some(value) = value {
                out.push_str(&format!(" {}=\"{}\"", key, html_escape(&value)));
            }
        }
        emit_html_attrs(out, &elem.attrs);
        out.push('>');
        // Children are raw JS code, not HTML
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
        out.push_str("</script>");
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

    let site = Site::new(elem, parent);
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
    let gen_class = compute_class(&elem.attrs, &site, styles);
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

    if elem.kind == ElementKind::Link
        && let Some(url) = &elem.argument
    {
        out.push_str(" href=\"");
        out.push_str(&html_escape(url));
        out.push('"');
    }

    emit_argument_attr(out, elem);

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
    out.push_str(ctx.nl());

    let layout = elem.kind.layout();
    ctx.depth += 1;
    let outer_in_text = ctx.in_text;
    ctx.in_text = in_text || layout == Layout::Text;
    generate_children(&elem.children, Some(layout), out, styles, ctx);
    ctx.in_text = outer_in_text;
    ctx.depth -= 1;

    out.push_str(&ctx.indent());
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
    let gen_class = compute_class(&elem.attrs, &Site::new(elem, parent), styles);
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
    emit_argument_attr(out, elem);

    // Image src (with optional non-SVG base64 inlining)
    if elem.kind == ElementKind::Image {
        let src = elem.argument.as_deref().unwrap_or("");
        let is_inline = elem.attrs.iter().any(|a| a.key == "inline");
        if is_inline && !src.is_empty() && !src.ends_with(".svg") {
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
            if let Ok(data) = std::fs::read(src) {
                let b64 = base64_encode(&data);
                out.push_str(" src=\"data:");
                out.push_str(mime);
                out.push_str(";base64,");
                out.push_str(&b64);
                out.push('"');
            } else {
                out.push_str(" src=\"");
                out.push_str(&html_escape(src));
                out.push('"');
            }
        } else {
            out.push_str(" src=\"");
            out.push_str(&html_escape(src));
            out.push('"');
        }
    }

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
                generate_element(elem, Some(Layout::Text), &mut buf, styles, ctx);
                out.push_str(buf.trim_end());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

fn compute_class(attrs: &[Attribute], site: &Site, styles: &mut StyleCollector) -> Option<String> {
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
    /// The layout of what contains it: its children's `width fill`,
    /// `center-x` and `align-*` compile against it. `None` at the top of
    /// the page.
    parent: Option<Layout>,
    /// It has `@in-front` / `@behind` children.
    has_overlay_children: bool,
    /// A row, column or grid inside text, laid out inline.
    inline: bool,
}

impl Site<'_> {
    /// Where an element in `parent` is.
    fn new<'a>(elem: &'a Element, parent: Option<Layout>) -> Site<'a> {
        Site {
            kind: &elem.kind,
            parent,
            has_overlay_children: has_overlay_children(elem),
            inline: parent == Some(Layout::Text) && elem.kind.layout().is_container(),
        }
    }
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
    let kind = site.kind;
    // `children:` styles go on the children, whose parent is this element
    let parent = if state_prefix == "children:" {
        Some(kind.layout())
    } else {
        site.parent
    };

    // Base element styles only for the default (non-state) pass
    if state_prefix.is_empty() {
        // Elements with @in-front / @behind children become positioning
        // contexts. Pushed before user attrs so an explicit `position` wins
        // via dedupe (only the last declaration of a property is kept).
        if site.has_overlay_children {
            css.push_str("position:relative;isolation:isolate;");
        }
        css.push_str(layout_css(kind.layout(), site.inline));
        css.push_str(kind.css());
    }
    // htmlang's words for laying out children (`spacing`, `wrap`,
    // `grid-cols`) mean nothing on an element that doesn't lay out its
    // children, which the parser reports; they are left out. Under
    // `children:` they go on the children, whose layout isn't known here.
    let lays_out_children = kind.layout().is_container() || state_prefix == "children:";

    for attr in attrs {
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

            // Sizing
            "width" => {
                if let Some(v) = val {
                    match v {
                        "fill" => match parent {
                            Some(Layout::Row) => {
                                push_css(&mut css, "flex", "1");
                                push_css(&mut css, "min-width", "0");
                            }
                            _ => push_css(&mut css, "width", "100%"),
                        },
                        "shrink" => push_css(&mut css, "flex-shrink", "0"),
                        _ => push_css(&mut css, "width", &css_px(v)),
                    }
                }
            }
            "height" => {
                if let Some(v) = val {
                    match v {
                        "fill" => match parent {
                            Some(Layout::Column) => {
                                push_css(&mut css, "flex", "1");
                                push_css(&mut css, "min-height", "0");
                            }
                            _ => push_css(&mut css, "height", "100%"),
                        },
                        "shrink" => push_css(&mut css, "flex-shrink", "0"),
                        _ => push_css(&mut css, "height", &css_px(v)),
                    }
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

            // Alignment
            "center-x" => match parent {
                Some(Layout::Column) => {
                    push_css(&mut css, "align-self", "center");
                }
                _ => {
                    push_css(&mut css, "margin-left", "auto");
                    push_css(&mut css, "margin-right", "auto");
                }
            },
            "center-y" => match parent {
                Some(Layout::Row) => push_css(&mut css, "align-self", "center"),
                _ => {
                    push_css(&mut css, "margin-top", "auto");
                    push_css(&mut css, "margin-bottom", "auto");
                }
            },
            "align-left" => match parent {
                Some(Layout::Column) => {
                    push_css(&mut css, "align-self", "flex-start");
                }
                _ => push_css(&mut css, "margin-right", "auto"),
            },
            "align-right" => match parent {
                Some(Layout::Column) => {
                    push_css(&mut css, "align-self", "flex-end");
                }
                _ => push_css(&mut css, "margin-left", "auto"),
            },
            "align-top" => match parent {
                Some(Layout::Row) => push_css(&mut css, "align-self", "flex-start"),
                _ => push_css(&mut css, "margin-bottom", "auto"),
            },
            "align-bottom" => match parent {
                Some(Layout::Row) => push_css(&mut css, "align-self", "flex-end"),
                _ => push_css(&mut css, "margin-top", "auto"),
            },

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
