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
        css.push_str(if dev { "@layer htmlang {\n" } else { "@layer htmlang{" });

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
    image_count: usize,
    has_interactive: bool,
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
        image_count: 0,
        has_interactive: false,
    };

    // Check if document has @main for skip-to-content link
    let has_main = has_tag(&doc.nodes, "main");

    let mut body = String::new();

    // Skip-to-content link for accessibility (only when @main exists)
    if has_main {
        if dev {
            body.push_str("<a href=\"#hl-main\" class=\"hl-skip\">Skip to content</a>\n");
        } else {
            body.push_str("<a href=\"#hl-main\" class=\"hl-skip\">Skip to content</a>");
        }
    }

    for node in &doc.nodes {
        generate_node(node, None, &mut body, &mut styles, &mut ctx);
    }


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

    // Canonical URL
    let canonical_html = match &doc.canonical {
        Some(url) => {
            if dev {
                format!("<link rel=\"canonical\" href=\"{}\">\n", html_escape(url))
            } else {
                format!("<link rel=\"canonical\" href=\"{}\">", html_escape(url))
            }
        }
        None => String::new(),
    };

    // Base URL
    let base_html = match &doc.base_url {
        Some(url) => {
            if dev {
                format!("<base href=\"{}\">\n", html_escape(url))
            } else {
                format!("<base href=\"{}\">", html_escape(url))
            }
        }
        None => String::new(),
    };

    // Preload hints
    let mut preload_html = String::new();
    // Explicit preload hints from the document
    for hint in &doc.preload_hints {
        if dev {
            preload_html.push_str(&format!(
                "<link rel=\"preload\" href=\"{}\" as=\"{}\"{}>\n",
                html_escape(&hint.href),
                hint.as_type,
                if hint.crossorigin { " crossorigin" } else { "" }
            ));
        } else {
            preload_html.push_str(&format!(
                "<link rel=\"preload\" href=\"{}\" as=\"{}\"{}>",
                html_escape(&hint.href),
                hint.as_type,
                if hint.crossorigin { " crossorigin" } else { "" }
            ));
        }
    }

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

    // Skip-to-content CSS (visually hidden but accessible)
    let skip_link_css = if has_main {
        if dev {
            ".hl-skip { position: absolute; left: -9999px; top: auto; width: 1px; height: 1px; overflow: hidden; z-index: 9999; padding: 8px 16px; background: #000; color: #fff; text-decoration: none; font-size: 14px; }\n.hl-skip:focus { left: 8px; top: 8px; width: auto; height: auto; overflow: visible; }\n"
        } else {
            ".hl-skip{position:absolute;left:-9999px;top:auto;width:1px;height:1px;overflow:hidden;z-index:9999;padding:8px 16px;background:#000;color:#fff;text-decoration:none;font-size:14px}.hl-skip:focus{left:8px;top:8px;width:auto;height:auto;overflow:visible}"
        }
    } else {
        ""
    };

    let reset_css = reset_css(dev, focus_visible_css, skip_link_css);

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
{base_html}{canonical_html}{preload_html}{meta_html}{og_html}{favicon_html}{head_html}\
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
                    base_html = base_html,
                    canonical_html = canonical_html,
                    preload_html = preload_html,
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
                    "<!DOCTYPE html><html{lang_attr}><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title>{base_html}{canonical_html}{preload_html}{meta_html}{og_html}{favicon_html}{head_html}<style>{reset_css}{element_css}</style></head><body>{body}</body></html>",
                    title = html_escape(title),
                    lang_attr = lang_attr,
                    base_html = base_html,
                    canonical_html = canonical_html,
                    preload_html = preload_html,
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

    // Collect all CSS custom properties (explicit `@let --name` / `@theme`
    // tokens, plus any auto-extracted repeats) so they can be emitted in a
    // single `:root` block below.
    let mut root_vars: Vec<(String, String)> = Vec::new();
    for (name, value) in &doc.css_vars {
        root_vars.push((name.clone(), value.clone()));
    }

    // Generated rules always go in `@layer htmlang`, so unlayered user CSS
    // (`@style`, `@raw`) overrides them regardless of specificity.
    let styles_css = styles.to_css_formatted(dev);
    // Fold literal values declared via `@theme` / `@let --name` back into
    // `var(--name)` references so the generated CSS actually uses the
    // custom properties emitted in `:root`.
    let styles_css = substitute_css_vars(&styles_css, &root_vars);

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

    // @keyframes
    for (name, kf_body) in &doc.keyframes {
        if dev {
            element_css.push_str(&format!("@keyframes {} {{\n{}\n}}\n", name, kf_body));
        } else {
            element_css.push_str(&format!("@keyframes {}{{{}}}", name, kf_body));
        }
    }

    // Keyframes for the standard library's `$skeleton` bundle, when used
    if element_css.contains("hl-skeleton") {
        if dev {
            element_css.push_str("@keyframes hl-skeleton {\n  0% { background-position: 200% 0; }\n  100% { background-position: -200% 0; }\n}\n");
        } else {
            element_css.push_str("@keyframes hl-skeleton{0%{background-position:200% 0}100%{background-position:-200% 0}}");
        }
    }

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
fn reset_css(dev: bool, focus_visible_css: &str, skip_link_css: &str) -> String {
    let base = if dev {
        "*, *::before, *::after { box-sizing: border-box; }\nbody { margin: 0; font-family: system-ui, -apple-system, sans-serif; }\nimg { display: block; }\na { text-decoration: none; color: inherit; }\n"
    } else {
        "*,*::before,*::after{box-sizing:border-box}body{margin:0;font-family:system-ui,-apple-system,sans-serif}img{display:block}a{text-decoration:none;color:inherit}"
    };
    let rules = format!("{}{}{}", base, focus_visible_css, skip_link_css);
    if dev {
        format!("@layer hl-reset, htmlang;\n@layer hl-reset {{\n{}}}\n", rules)
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
        image_count: 0,
        has_interactive: false,
    };
    let mut body = String::new();

    for node in &doc.nodes {
        generate_node(node, None, &mut body, &mut styles, &mut ctx);
    }

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

fn generate_node(
    node: &Node,
    parent_kind: Option<&ElementKind>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    match node {
        Node::Element(elem) => generate_element(elem, parent_kind, out, styles, ctx),
        Node::Text(segments) => {
            let needs_wrap = matches!(
                parent_kind,
                Some(ElementKind::Row | ElementKind::Column | ElementKind::El)
            ) || parent_kind
                .and_then(ElementKind::spec)
                .is_some_and(|spec| spec.wraps_text);
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
            out.push_str(&html_escape(strip_string_quotes(
                attr.value.as_deref().unwrap_or(""),
            )));
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

/// If the value is wrapped in a single pair of matching quotes (e.g.
/// `"Avatar"`), return the inner content. Quotes act as source-level
/// delimiters in the `.hl` syntax and shouldn't leak into HTML attribute
/// values. Multi-quoted values like `"h h" "s m"` are left unchanged —
/// those are real string tokens (used e.g. by CSS `grid-template-areas`).
fn strip_string_quotes(val: &str) -> &str {
    let bytes = val.as_bytes();
    if bytes.len() < 2 {
        return val;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
        let inner = &val[1..val.len() - 1];
        if !inner.as_bytes().contains(&first) {
            return inner;
        }
    }
    val
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
    parent_kind: Option<&ElementKind>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    // Self-closing elements
    if elem.kind == ElementKind::Image || elem.kind.spec().is_some_and(|spec| spec.void) {
        generate_self_closing(elem, parent_kind, out, styles, ctx);
        return;
    }
    // @script renders as <script> with raw body content (no HTML escaping)
    if elem.kind == ElementKind::Script {
        out.push_str(&ctx.indent());
        out.push_str("<script");
        // Pass through src, type, defer, async, etc.
        for attr in &elem.attrs {
            let key = attr.key.as_str();
            if matches!(
                key,
                "src"
                    | "type"
                    | "defer"
                    | "async"
                    | "crossorigin"
                    | "integrity"
                    | "nomodule"
                    | "id"
            ) {
                if let Some(val) = &attr.value {
                    out.push(' ');
                    out.push_str(key);
                    out.push_str("=\"");
                    out.push_str(&html_escape(val));
                    out.push('"');
                } else {
                    out.push(' ');
                    out.push_str(key);
                }
            }
        }
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
        // Render children without a wrapper element
        for child in &elem.children {
            generate_node(child, parent_kind, out, styles, ctx);
        }
        return;
    }

    let tag = match &elem.kind {
        ElementKind::Row | ElementKind::Column | ElementKind::El => "div",
        ElementKind::Text => "span",
        ElementKind::Paragraph => "p",
        ElementKind::Link => "a",
        ElementKind::Tag(spec)
            if spec.name == "list" && elem.attrs.iter().any(|a| a.key == "ordered") =>
        {
            "ol"
        }
        ElementKind::Tag(spec) => spec.html,
        _ => "",
    };
    let kind_label = elem.kind.name();

    // Track interactive elements for focus-visible CSS
    if elem.kind == ElementKind::Link
        || matches!(kind_label, "button" | "input" | "select" | "textarea")
    {
        ctx.has_interactive = true;
    }

    // Compute CSS for each state and get a class name
    let overlay_children = has_overlay_children(elem);
    let gen_class = compute_class(
        &elem.attrs,
        &elem.kind,
        parent_kind,
        styles,
        overlay_children,
    );
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

    // @main gets id="hl-main" for skip-to-content link (unless user set an id)
    if elem.kind.is_tag("main") && id.is_none() {
        out.push_str(" id=\"hl-main\"");
    }

    if elem.kind == ElementKind::Link
        && let Some(url) = &elem.argument
    {
        out.push_str(" href=\"");
        out.push_str(&html_escape(url));
        out.push('"');
        // Auto rel="noopener noreferrer" and target="_blank" for external links
        let is_external = url.starts_with("http://") || url.starts_with("https://");
        if is_external {
            let has_rel = elem.attrs.iter().any(|a| a.key == "rel");
            let has_target = elem.attrs.iter().any(|a| a.key == "target");
            if !has_rel {
                out.push_str(" rel=\"noopener noreferrer\"");
            }
            if !has_target {
                out.push_str(" target=\"_blank\"");
            }
        }
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

    // Inline text argument
    if renders_argument_as_text(&elem.kind)
        && let Some(text) = &elem.argument
    {
        out.push_str(&html_escape(text));
    }

    // Children
    ctx.depth += 1;
    let is_paragraph = elem.kind == ElementKind::Paragraph
        || elem.kind.spec().is_some_and(|spec| spec.inline);
    for (i, child) in elem.children.iter().enumerate() {
        generate_node(child, Some(&elem.kind), out, styles, ctx);
        if is_paragraph && i < elem.children.len() - 1 {
            out.push(' ');
        }
    }
    ctx.depth -= 1;

    out.push_str(&ctx.indent());
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    out.push_str(ctx.nl());
}

fn generate_self_closing(
    elem: &Element,
    parent_kind: Option<&ElementKind>,
    out: &mut String,
    styles: &mut StyleCollector,
    ctx: &mut GenContext,
) {
    let gen_class = compute_class(&elem.attrs, &elem.kind, parent_kind, styles, false);
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

    // Image optimization: auto-add loading="lazy" and decoding="async"
    if elem.kind == ElementKind::Image {
        // SVG inlining: @image [inline] logo.svg
        if elem.attrs.iter().any(|a| a.key == "inline")
            && let Some(src) = &elem.argument
            && src.ends_with(".svg")
            && let Ok(svg_content) = std::fs::read_to_string(src)
        {
            // Close the tag we opened, then emit inline SVG instead
            out.truncate(out.rfind('<').unwrap_or(0));
            out.push_str(&ctx.indent());
            out.push_str(svg_content.trim());
            out.push_str(ctx.nl());
            return;
        }
        // Responsive srcset: @image photo.jpg [responsive 400 800 1200]
        let responsive_attr = elem.attrs.iter().find(|a| a.key == "responsive");
        if let Some(resp) = responsive_attr
            && let Some(ref sizes_str) = resp.value
        {
            let widths: Vec<&str> = sizes_str.split_whitespace().collect();
            if !widths.is_empty() {
                let src = elem.argument.as_deref().unwrap_or("");
                if !src.is_empty() {
                    // Generate srcset with width descriptors
                    // Convention: file-{width}.ext (e.g., photo-400.jpg)
                    let dot_pos = src.rfind('.').unwrap_or(src.len());
                    let base = &src[..dot_pos];
                    let ext = &src[dot_pos..];
                    let mut srcset_parts = Vec::new();
                    for w in &widths {
                        srcset_parts.push(format!("{}-{}{} {}w", base, w, ext, w));
                    }
                    out.push_str(" srcset=\"");
                    out.push_str(&srcset_parts.join(", "));
                    out.push('"');
                    // Generate sizes attribute
                    let max_width = widths.last().unwrap_or(&"100vw");
                    out.push_str(&format!(
                        " sizes=\"(max-width: {}px) 100vw, {}px\"",
                        max_width, max_width
                    ));
                }
            }
        }

        // Auto image dimensions: read local image file to inject width/height + aspect-ratio
        let has_width = elem.attrs.iter().any(|a| a.key == "width");
        let has_height = elem.attrs.iter().any(|a| a.key == "height");
        if (!has_width || !has_height)
            && let Some(ref src) = elem.argument
            && !src.starts_with("http://")
            && !src.starts_with("https://")
            && !src.starts_with("data:")
            && let Some((w, h)) = read_image_dimensions(src)
        {
            // Intrinsic size attributes only when neither dimension is set in
            // CSS; with one CSS dimension, a leftover intrinsic attribute for
            // the other would distort the image (200 wide but 1000 tall).
            if !has_width && !has_height {
                out.push_str(&format!(" width=\"{}\" height=\"{}\"", w, h));
            }
            // Auto aspect-ratio to prevent CLS
            if !elem.attrs.iter().any(|a| a.key == "aspect-ratio") {
                out.push_str(&format!(" style=\"aspect-ratio:{}/{}\"", w, h));
            }
        }

        // Smart image loading: first 3 images get fetchpriority="high" (above the fold),
        // subsequent images get loading="lazy" + decoding="async"
        ctx.image_count += 1;
        if ctx.image_count <= 3 {
            // Above-the-fold: eager loading with high priority
            if !elem.attrs.iter().any(|a| a.key == "fetchpriority") {
                out.push_str(" fetchpriority=\"high\"");
            }
        } else {
            // Below-the-fold: lazy loading
            if !elem.attrs.iter().any(|a| a.key == "loading") {
                out.push_str(" loading=\"lazy\"");
            }
            if !elem.attrs.iter().any(|a| a.key == "decoding") {
                out.push_str(" decoding=\"async\"");
            }
        }
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
                generate_element(elem, None, &mut buf, styles, ctx);
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
    kind: &ElementKind,
    parent_kind: Option<&ElementKind>,
    styles: &mut StyleCollector,
    has_overlay_children: bool,
) -> Option<String> {
    let base = attrs_to_css(attrs, "", kind, parent_kind, has_overlay_children);

    // Collect pseudo-state overrides
    let mut pseudo = Vec::new();
    for &(prefix, selector) in crate::vocab::PSEUDO_PREFIXES {
        let css = attrs_to_css(attrs, prefix, kind, parent_kind, has_overlay_children);
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
        let css = attrs_to_css(attrs, prefix, kind, parent_kind, has_overlay_children);
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
        let css = attrs_to_css(attrs, prefix, kind, parent_kind, has_overlay_children);
        if !css.is_empty() {
            pseudo.push((selector, css));
        }
    }

    // Collect responsive overrides
    let mut responsive = Vec::new();
    for &(bp_name, _) in BREAKPOINTS {
        let prefix = format!("{}:", bp_name);
        let css = attrs_to_css(attrs, &prefix, kind, parent_kind, has_overlay_children);
        if !css.is_empty() {
            responsive.push((bp_name.to_string(), css));
        }
    }

    // Collect container query overrides
    let mut container = Vec::new();
    for &(bp_name, _) in BREAKPOINTS {
        let prefix = format!("cq-{}:", bp_name);
        let css = attrs_to_css(attrs, &prefix, kind, parent_kind, has_overlay_children);
        if !css.is_empty() {
            container.push((bp_name.to_string(), css));
        }
    }

    let dark = attrs_to_css(attrs, "dark:", kind, parent_kind, has_overlay_children);
    let print = attrs_to_css(attrs, "print:", kind, parent_kind, has_overlay_children);
    let motion_safe = attrs_to_css(attrs, "motion-safe:", kind, parent_kind, has_overlay_children);
    let motion_reduce =
        attrs_to_css(attrs, "motion-reduce:", kind, parent_kind, has_overlay_children);
    let landscape = attrs_to_css(attrs, "landscape:", kind, parent_kind, has_overlay_children);
    let portrait = attrs_to_css(attrs, "portrait:", kind, parent_kind, has_overlay_children);

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
/// segments (no `:`) are preserved as-is. Semicolons inside parentheses are
/// treated as part of a value, not as declaration separators.
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
    for ch in css.chars() {
        if ch == '(' {
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

/// (htmlang prefix, CSS selector suffix)
/// `display:flex;flex-direction:column;` — base layout for `@el` and every
/// semantic wrapper that behaves like a column.
const FLEX_COLUMN: &str = "display:flex;flex-direction:column;";



fn attrs_to_css(
    attrs: &[Attribute],
    state_prefix: &str,
    kind: &ElementKind,
    parent_kind: Option<&ElementKind>,
    has_overlay_children: bool,
) -> String {
    let mut css = String::new();

    // Base element styles only for the default (non-state) pass
    if state_prefix.is_empty() {
        // Elements with @in-front / @behind children become positioning
        // contexts. Pushed before user attrs so an explicit `position` wins
        // via dedupe (only the last declaration of a property is kept).
        if has_overlay_children {
            css.push_str("position:relative;isolation:isolate;");
        }
        match kind {
            ElementKind::Row => css.push_str("display:flex;flex-direction:row;"),
            ElementKind::Column | ElementKind::El => css.push_str(FLEX_COLUMN),
            ElementKind::Paragraph => css.push_str("margin:0;"),
            ElementKind::Tag(spec) => css.push_str(spec.css),
            _ => {}
        }
    }

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

        match effective_key {
            // Layout
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
            "padding-x" => {
                if let Some(v) = val {
                    // Logical property covers both inline sides in one
                    // declaration; for symmetric values this is visually
                    // identical to padding-left/right in LTR and RTL.
                    push_css(&mut css, "padding-inline", &css_px(v));
                }
            }
            "padding-y" => {
                if let Some(v) = val {
                    push_css(&mut css, "padding-block", &css_px(v));
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
                        "fill" => match parent_kind {
                            Some(ElementKind::Row) => {
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
                        "fill" => match parent_kind {
                            Some(ElementKind::Column) => {
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
            "center-x" => match parent_kind {
                Some(ElementKind::Column) | Some(ElementKind::El) => {
                    push_css(&mut css, "align-self", "center");
                }
                _ => {
                    push_css(&mut css, "margin-left", "auto");
                    push_css(&mut css, "margin-right", "auto");
                }
            },
            "center-y" => match parent_kind {
                Some(ElementKind::Row) => push_css(&mut css, "align-self", "center"),
                _ => {
                    push_css(&mut css, "margin-top", "auto");
                    push_css(&mut css, "margin-bottom", "auto");
                }
            },
            "align-left" => match parent_kind {
                Some(ElementKind::Column) | Some(ElementKind::El) => {
                    push_css(&mut css, "align-self", "flex-start");
                }
                _ => push_css(&mut css, "margin-right", "auto"),
            },
            "align-right" => match parent_kind {
                Some(ElementKind::Column) | Some(ElementKind::El) => {
                    push_css(&mut css, "align-self", "flex-end");
                }
                _ => push_css(&mut css, "margin-left", "auto"),
            },
            "align-top" => match parent_kind {
                Some(ElementKind::Row) => push_css(&mut css, "align-self", "flex-start"),
                _ => push_css(&mut css, "margin-bottom", "auto"),
            },
            "align-bottom" => match parent_kind {
                Some(ElementKind::Row) => push_css(&mut css, "align-self", "flex-end"),
                _ => push_css(&mut css, "margin-top", "auto"),
            },

            // Style
            "border" => {
                if let Some(v) = val {
                    let parts: Vec<&str> = v.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        push_css(
                            &mut css,
                            "border",
                            &format!("{} solid {}", css_px(parts[0]), parts[1]),
                        );
                    } else {
                        push_css(
                            &mut css,
                            "border",
                            &format!("{} solid currentColor", css_px(parts[0])),
                        );
                    }
                }
            }
            "border-top" | "border-bottom" | "border-left" | "border-right" => {
                if let Some(v) = val {
                    let parts: Vec<&str> = v.splitn(2, ' ').collect();
                    if parts.len() == 2 {
                        push_css(
                            &mut css,
                            effective_key,
                            &format!("{} solid {}", css_px(parts[0]), parts[1]),
                        );
                    } else {
                        push_css(
                            &mut css,
                            effective_key,
                            &format!("{} solid currentColor", css_px(parts[0])),
                        );
                    }
                }
            }
            "rounded" => {
                if let Some(v) = val {
                    push_css(&mut css, "border-radius", &css_px(v));
                }
            }
            "bold" => push_css(&mut css, "font-weight", "bold"),
            "italic" => push_css(&mut css, "font-style", "italic"),
            "underline" => push_css(&mut css, "text-decoration", "underline"),
            "size" => {
                if let Some(v) = val {
                    push_css(&mut css, "font-size", &css_px(v));
                }
            }
            "font" => {
                if let Some(v) = val {
                    // `font "Inter, sans-serif"` quotes a whole font stack so
                    // its commas don't split the attribute list.
                    let v = match v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                        Some(stack) if stack.contains(',') => stack,
                        _ => v,
                    };
                    push_css(&mut css, "font-family", v);
                }
            }

            // Typography
            "line-height" => {
                if let Some(v) = val {
                    push_css(&mut css, "line-height", &css_line_height(v));
                }
            }
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

            // Display & visibility
            "hidden" => push_css(&mut css, "display", "none"),

            // Effects

            // Flow
            "wrap" => push_css(&mut css, "flex-wrap", "wrap"),

            // Grid
            "grid" => {
                push_css(&mut css, "display", "grid");
            }
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

            // CSS containment for rendering performance
            "contain" => {
                if let Some(v) = val {
                    push_css(&mut css, "contain", v);
                } else {
                    push_css(&mut css, "contain", "layout style paint");
                }
            }
            "content-visibility" => {
                if let Some(v) = val {
                    push_css(&mut css, "content-visibility", v);
                } else {
                    push_css(&mut css, "content-visibility", "auto");
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
            "margin-x" => {
                if let Some(v) = val {
                    push_css(&mut css, "margin-inline", &css_px(v));
                }
            }
            "margin-y" => {
                if let Some(v) = val {
                    push_css(&mut css, "margin-block", &css_px(v));
                }
            }

            // Container queries
            "container" => {
                push_css(&mut css, "container-type", "inline-size");
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

            "content" => {
                if let Some(v) = val {
                    // Wrap in quotes if not already quoted and not a CSS keyword
                    if v.starts_with('"')
                        || v.starts_with('\'')
                        || v == "none"
                        || v == "normal"
                        || v.starts_with("attr(")
                        || v.starts_with("counter(")
                    {
                        push_css(&mut css, "content", v);
                    } else {
                        push_css(&mut css, "content", &format!("\"{}\"", v));
                    }
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
        && v.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+'))
        && v.parse::<f64>().is_ok();
    if is_bare_number {
        format!("{}px", v)
    } else {
        v.to_string()
    }
}

/// Rewrite literal values that match a declared CSS custom property (from
/// `@theme` or `@let --name value`) to `var(--name)` references. Matches are
/// anchored to CSS value boundaries so e.g. `#3b82f6` inside a longer hex or
/// inside an identifier is not replaced. The `:root` block is emitted before
/// this call runs, so its declarations are not affected.
fn substitute_css_vars(css: &str, vars: &[(String, String)]) -> String {
    if css.is_empty() || vars.is_empty() {
        return css.to_string();
    }
    // Prefer longer values first so that if two vars share a prefix, the
    // longer (more specific) match wins.
    let mut pairs: Vec<(&str, String)> = vars
        .iter()
        .filter(|(name, value)| name.starts_with("--") && !value.is_empty())
        .map(|(name, value)| (value.as_str(), format!("var({})", name)))
        .collect();
    pairs.sort_by_key(|(v, _)| std::cmp::Reverse(v.len()));
    if pairs.is_empty() {
        return css.to_string();
    }

    let is_boundary_before = |b: Option<u8>| match b {
        None => true,
        Some(c) => matches!(c, b':' | b' ' | b',' | b'(' | b';' | b'{' | b'\n' | b'\t'),
    };
    let is_boundary_after = |b: Option<u8>| match b {
        None => true,
        Some(c) => matches!(c, b';' | b'}' | b',' | b' ' | b')' | b'\n' | b'\t'),
    };

    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    let mut prev: Option<u8> = None;
    // Only declaration values are rewritten — never selectors or at-rule
    // preludes such as `@media (min-width:768px)`. `blocks` records, for
    // each open `{`, whether it holds declarations (a style rule) or nested
    // rules (an at-rule like `@media` / `@layer`).
    let mut blocks: Vec<bool> = Vec::new();
    let mut prelude_start = 0;
    let mut in_value = false;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                let prelude = css[prelude_start..i].trim_start();
                blocks.push(!prelude.starts_with('@'));
                in_value = false;
                prelude_start = i + 1;
            }
            b'}' => {
                blocks.pop();
                in_value = false;
                prelude_start = i + 1;
            }
            b';' => {
                in_value = false;
                prelude_start = i + 1;
            }
            b':' if blocks.last() == Some(&true) => in_value = true,
            _ => {}
        }
        if in_value && is_boundary_before(prev) {
            let mut matched = false;
            for (val, repl) in &pairs {
                let vb = val.as_bytes();
                if i + vb.len() <= bytes.len() && &bytes[i..i + vb.len()] == vb {
                    let next = bytes.get(i + vb.len()).copied();
                    if is_boundary_after(next) {
                        out.push_str(repl);
                        prev = repl.as_bytes().last().copied();
                        i += vb.len();
                        matched = true;
                        break;
                    }
                }
            }
            if matched {
                continue;
            }
        }
        // Advance by one UTF-8 code point.
        let start = i;
        i += 1;
        while i < bytes.len() && (bytes[i] & 0xC0) == 0x80 {
            i += 1;
        }
        out.push_str(&css[start..i]);
        prev = bytes.get(i - 1).copied();
    }
    out
}


/// Format a `line-height` value. CSS accepts either a unitless multiplier
/// (e.g. `1.5`) or a length (e.g. `24px`). Plain integers in htmlang source
/// are ambiguous: `[line-height 24]` was historically emitted as `24` (which
/// CSS interprets as 24× font-size — almost never what anyone wants). Treat
/// integers ≥ 2 as pixel lengths; anything with a decimal, an existing unit,
/// or the value `0`/`1` passes through unchanged.
fn css_line_height(value: &str) -> String {
    let v = value.trim();
    if v == "0" || v == "1" {
        return v.to_string();
    }
    // Plain integer — treat as pixel length. Decimals, units, keywords and
    // functions pass through.
    if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}px", v);
    }
    v.to_string()
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

/// Read image dimensions from a local file by parsing the header bytes.
/// Supports PNG, JPEG, GIF, WebP, AVIF, and SVG.
fn read_image_dimensions(path: &str) -> Option<(u32, u32)> {
    // SVG: parse as text for viewBox/width/height attributes
    if path.ends_with(".svg") || path.ends_with(".SVG") {
        let text = std::fs::read_to_string(path).ok()?;
        return read_svg_dimensions(&text);
    }

    let data = std::fs::read(path).ok()?;
    if data.len() < 12 {
        return None;
    }

    // PNG: 8-byte signature, then IHDR chunk with width/height at bytes 16-23
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        return Some((w, h));
    }

    // GIF: "GIF87a" or "GIF89a", width/height at bytes 6-9 (little-endian)
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        let w = u16::from_le_bytes([data[6], data[7]]) as u32;
        let h = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some((w, h));
    }

    // JPEG: scan for SOF0/SOF2 marker (0xFF 0xC0 or 0xFF 0xC2)
    if data.starts_with(b"\xff\xd8") {
        let mut i = 2;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if marker == 0xC0 || marker == 0xC2 {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((w, h));
            }
            if i + 3 < data.len() {
                let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                i += 2 + len;
            } else {
                break;
            }
        }
    }

    // WebP: "RIFF" ... "WEBP", VP8 header at byte 20
    if data.len() >= 30 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        if &data[12..16] == b"VP8 " && data.len() >= 30 {
            let w = u16::from_le_bytes([data[26], data[27]]) as u32 & 0x3FFF;
            let h = u16::from_le_bytes([data[28], data[29]]) as u32 & 0x3FFF;
            return Some((w, h));
        }
        if &data[12..16] == b"VP8L" && data.len() >= 25 && data[21] == 0x2F {
            let bits = u32::from_le_bytes([data[22], data[23], data[24], data[25]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            return Some((w, h));
        }
    }

    // AVIF: ISOBMFF container with "ftyp" box containing "avif"/"avis" brand,
    // then "ispe" box with width/height
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let brand = &data[8..12];
        if brand == b"avif" || brand == b"avis" || brand == b"mif1" {
            return read_avif_dimensions(&data);
        }
    }

    None
}

/// Parse SVG viewBox or width/height attributes to get dimensions.
fn read_svg_dimensions(text: &str) -> Option<(u32, u32)> {
    // Try viewBox first: viewBox="minX minY width height"
    if let Some(vb_start) = text.find("viewBox=\"") {
        let rest = &text[vb_start + 9..];
        if let Some(end) = rest.find('"') {
            let parts: Vec<&str> = rest[..end].split_whitespace().collect();
            if parts.len() == 4
                && let (Ok(w), Ok(h)) = (parts[2].parse::<f64>(), parts[3].parse::<f64>())
                && w > 0.0
                && h > 0.0
            {
                return Some((w.round() as u32, h.round() as u32));
            }
        }
    }
    // Fall back to width/height attributes on <svg>
    let svg_tag = text.find("<svg")?;
    let tag_end = text[svg_tag..].find('>')? + svg_tag;
    let tag = &text[svg_tag..tag_end];
    let w = extract_svg_attr(tag, "width")?;
    let h = extract_svg_attr(tag, "height")?;
    Some((w, h))
}

fn extract_svg_attr(tag: &str, attr: &str) -> Option<u32> {
    let needle = format!("{}=\"", attr);
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    let val = rest[..end].trim_end_matches("px");
    val.parse::<f64>().ok().map(|v| v.round() as u32)
}

/// Parse AVIF (ISOBMFF) container to find ispe box with image dimensions.
fn read_avif_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    // Walk ISOBMFF boxes looking for "ispe" (image spatial extents)
    let mut i = 0;
    while i + 8 <= data.len() {
        let box_size =
            u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let box_type = &data[i + 4..i + 8];
        if box_size < 8 {
            break;
        }
        let box_end = (i + box_size).min(data.len());
        // ispe box: 4 bytes version/flags + 4 bytes width + 4 bytes height
        if box_type == b"ispe" && box_end >= i + 20 {
            let w = u32::from_be_bytes([data[i + 12], data[i + 13], data[i + 14], data[i + 15]]);
            let h = u32::from_be_bytes([data[i + 16], data[i + 17], data[i + 18], data[i + 19]]);
            return Some((w, h));
        }
        // Recurse into container boxes (meta, iprp, ipco)
        if matches!(
            box_type,
            b"meta" | b"iprp" | b"ipco" | b"moov" | b"trak" | b"mdia"
        ) {
            let header_size = if box_type == b"meta" { 12 } else { 8 };
            if i + header_size < box_end
                && let Some(dims) = read_avif_dimensions(&data[i + header_size..box_end])
            {
                return Some(dims);
            }
        }
        i = box_end;
    }
    None
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


/// Does the tree contain the table element `name` (e.g. `"main"`)?
fn has_tag(nodes: &[Node], name: &str) -> bool {
    nodes.iter().any(|node| match node {
        Node::Element(elem) => elem.kind.is_tag(name) || has_tag(&elem.children, name),
        _ => false,
    })
}


