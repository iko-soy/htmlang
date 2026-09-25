/// Converts HTML to htmlang (.hl) syntax.
///
/// This is a best-effort converter: it handles well-formed HTML and maps known
/// elements and CSS properties to their htmlang equivalents. Unknown elements
/// are wrapped in `@raw """..."""`.

// ---------------------------------------------------------------------------
// Simple recursive-descent HTML parser
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum HtmlNode {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<HtmlNode>,
        self_closing: bool,
    },
    Text(String),
    Comment(String),
    /// For DOCTYPE, processing instructions, etc. we just keep the raw text.
    Raw(String),
}

struct HtmlParser<'a> {
    src: &'a [u8],
    pos: usize,
}

/// Tags that are self-closing in HTML (void elements).
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

impl<'a> HtmlParser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
        }
    }

    fn advance(&mut self) {
        if self.pos < self.src.len() {
            self.pos += 1;
        }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.src.len() && self.src[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s.as_bytes())
    }

    /// Whether the `<` at the current position opens markup (a tag, closing
    /// tag, comment or doctype) rather than being a literal `<` in text.
    fn at_markup(&self) -> bool {
        self.src.get(self.pos) == Some(&b'<')
            && self
                .src
                .get(self.pos + 1)
                .is_some_and(|&c| c.is_ascii_alphabetic() || c == b'/' || c == b'!')
    }

    /// Consume up to and including `needle`, returning the text before it.
    /// Without a match, consumes the rest of the input.
    fn consume_until(&mut self, needle: &str) -> String {
        self.consume_until_by(needle, |a, b| a == b)
    }

    /// Case-insensitive consume-until.
    fn consume_until_ci(&mut self, needle: &str) -> String {
        self.consume_until_by(needle, |a, b| a.eq_ignore_ascii_case(b))
    }

    fn consume_until_by(&mut self, needle: &str, eq: impl Fn(&[u8], &[u8]) -> bool) -> String {
        let start = self.pos;
        let needle = needle.as_bytes();
        let found = self.src[start..]
            .windows(needle.len())
            .position(|w| eq(w, needle));
        let end = found.map_or(self.src.len(), |i| start + i);
        self.pos = found.map_or(self.src.len(), |_| end + needle.len());
        String::from_utf8_lossy(&self.src[start..end]).into_owned()
    }

    fn parse_nodes(&mut self, stop_tag: Option<&str>) -> Vec<HtmlNode> {
        let mut nodes = Vec::new();
        loop {
            if self.pos >= self.src.len() {
                break;
            }
            // Check for closing tag
            if let Some(stop) = stop_tag
                && self.starts_with("</")
            {
                let saved = self.pos;
                self.pos += 2;
                let tag = self.parse_tag_name();
                if tag.eq_ignore_ascii_case(stop) {
                    // Consume the rest of the closing tag
                    self.consume_until(">");
                    break;
                }
                // Not our closing tag, restore and let it be handled
                self.pos = saved;
                // This closing tag doesn't match; break out to let the
                // parent handle it.
                break;
            }

            if self.starts_with("<!--") {
                self.pos += 4;
                let text = self.consume_until("-->");
                nodes.push(HtmlNode::Comment(text));
            } else if self.starts_with("<!") {
                // DOCTYPE or similar
                let raw = self.consume_until(">");
                nodes.push(HtmlNode::Raw(format!("<!{}>", raw)));
            } else if self.starts_with("</") {
                // Stray closing tag (no matching open) -- skip it
                self.consume_until(">");
            } else if self.at_markup() {
                if let Some(node) = self.parse_element() {
                    nodes.push(node);
                }
            } else {
                // Text content
                let text = self.parse_text();
                if !text.is_empty() {
                    nodes.push(HtmlNode::Text(text));
                }
            }
        }
        nodes
    }

    fn parse_text(&mut self) -> String {
        let start = self.pos;
        // A `<` that doesn't open markup (`a < b`, `1 <2`) is literal text.
        while self.pos < self.src.len() && (self.pos == start || !self.at_markup()) {
            self.pos += 1;
        }
        let raw = String::from_utf8_lossy(&self.src[start..self.pos]);
        decode_entities(&raw)
    }

    fn parse_tag_name(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.src.len() {
            let ch = self.src[self.pos];
            if ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' || ch == b':' {
                self.pos += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.src[start..self.pos])
            .unwrap_or("")
            .to_ascii_lowercase()
    }

    fn parse_element(&mut self) -> Option<HtmlNode> {
        // Consume '<'
        self.advance();
        let tag = self.parse_tag_name();
        if tag.is_empty() {
            return None;
        }

        let mut attrs = Vec::new();
        loop {
            self.skip_whitespace();
            if self.pos >= self.src.len() {
                break;
            }
            if self.starts_with("/>") {
                self.pos += 2;
                return Some(HtmlNode::Element {
                    tag,
                    attrs,
                    children: Vec::new(),
                    self_closing: true,
                });
            }
            if self.starts_with(">") {
                self.advance();
                break;
            }
            // Parse attribute
            let key = self.parse_attr_name();
            if key.is_empty() {
                self.advance(); // skip unknown char
                continue;
            }
            self.skip_whitespace();
            let value = if self.starts_with("=") {
                self.advance();
                self.skip_whitespace();
                self.parse_attr_value()
            } else {
                String::new()
            };
            attrs.push((key, value));
        }

        // Void / self-closing tags
        if VOID_TAGS.contains(&tag.as_str()) {
            return Some(HtmlNode::Element {
                tag,
                attrs,
                children: Vec::new(),
                self_closing: true,
            });
        }

        // Raw content tags (script, style) -- consume until closing tag
        if tag == "script" || tag == "style" {
            let closer = format!("</{}>", tag);
            let content = self.consume_until_ci(&closer);
            let children = if content.trim().is_empty() {
                Vec::new()
            } else {
                vec![HtmlNode::Text(content)]
            };
            return Some(HtmlNode::Element {
                tag,
                attrs,
                children,
                self_closing: false,
            });
        }

        let children = self.parse_nodes(Some(&tag));
        Some(HtmlNode::Element {
            tag,
            attrs,
            children,
            self_closing: false,
        })
    }

    fn parse_attr_name(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.src.len() {
            let ch = self.src[self.pos];
            if ch.is_ascii_alphanumeric()
                || ch == b'-'
                || ch == b'_'
                || ch == b':'
                || ch == b'.'
                || ch == b'@'
            {
                self.pos += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.src[start..self.pos])
            .unwrap_or("")
            .to_string()
    }

    fn parse_attr_value(&mut self) -> String {
        if self.pos >= self.src.len() {
            return String::new();
        }
        let quote = self.src[self.pos];
        if quote == b'"' || quote == b'\'' {
            self.advance();
            let start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos] != quote {
                self.pos += 1;
            }
            let val = std::str::from_utf8(&self.src[start..self.pos])
                .unwrap_or("")
                .to_string();
            if self.pos < self.src.len() {
                self.advance(); // closing quote
            }
            decode_entities(&val)
        } else {
            // Unquoted value
            let start = self.pos;
            while self.pos < self.src.len()
                && !self.src[self.pos].is_ascii_whitespace()
                && self.src[self.pos] != b'>'
            {
                self.pos += 1;
            }
            std::str::from_utf8(&self.src[start..self.pos])
                .unwrap_or("")
                .to_string()
        }
    }

}

/// Decode HTML character references in one pass (so `&amp;lt;` stays `&lt;`),
/// including numeric (`&#8212;`, `&#x2014;`) and common named entities.
/// Unknown references are kept as written.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest[1..].find(';').filter(|&end| end <= 32).and_then(|end| {
            let name = &rest[1..1 + end];
            decode_entity(name).map(|c| (c, end + 2))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "bull" => '•',
        "middot" => '·',
        "deg" => '°',
        "times" => '×',
        "divide" => '÷',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// CSS style parsing (inline style="" attribute)
// ---------------------------------------------------------------------------

fn parse_inline_style(style: &str) -> Vec<(String, String)> {
    let mut props = Vec::new();
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let key = decl[..colon].trim().to_string();
            let val = decl[colon + 1..].trim().to_string();
            props.push((key, val));
        }
    }
    props
}

/// Strips a trailing "px" and returns just the number, if the value is a plain
/// px value. Otherwise returns the value as-is.
fn strip_px(v: &str) -> &str {
    v.strip_suffix("px").unwrap_or(v)
}

/// Strips px from a multi-value string like "10px 20px".
fn strip_px_multi(v: &str) -> String {
    v.split_whitespace()
        .map(strip_px)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Converts CSS properties to htmlang attribute key-value pairs.
/// Returns (attrs, leftover_css) where leftover_css is anything we
/// could not convert to a known htmlang attribute.
type HlAttr = (String, Option<String>);
type CssDecl = (String, String);

fn css_to_hl_attrs(styles: &[CssDecl]) -> (Vec<HlAttr>, Vec<CssDecl>) {
    let mut attrs: Vec<(String, Option<String>)> = Vec::new();
    let mut leftover: Vec<(String, String)> = Vec::new();

    // Collect all styles into a map so we can detect flex patterns
    let style_map: std::collections::HashMap<&str, &str> = styles
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // Detect flex row/column -- these affect the element kind, not attributes
    // We skip display:flex and flex-direction here; the caller checks them.

    for (key, val) in styles {
        match key.as_str() {
            // Skip flex layout props -- handled at element-kind level
            "display" if val == "flex" => {}
            "flex-direction" => {}

            "display" if val == "grid" => {
                attrs.push(("grid".into(), None));
            }
            "display" if val == "none" => {
                attrs.push(("hidden".into(), None));
            }

            "padding" => {
                attrs.push(("padding".into(), Some(strip_px_multi(val))));
            }
            "padding-left" | "padding-right" => {
                // Check for padding-x pattern
                if key == "padding-left"
                    && let Some(&pr) = style_map.get("padding-right")
                    && pr == val
                {
                    attrs.push(("padding-x".into(), Some(strip_px(val).into())));
                    continue;
                }
                if key == "padding-right" && style_map.get("padding-left") == Some(&val.as_str()) {
                    // Already emitted as padding-x
                    continue;
                }
                leftover.push((key.clone(), val.clone()));
            }
            "padding-top" | "padding-bottom" => {
                if key == "padding-top"
                    && let Some(&pb) = style_map.get("padding-bottom")
                    && pb == val
                {
                    attrs.push(("padding-y".into(), Some(strip_px(val).into())));
                    continue;
                }
                if key == "padding-bottom" && style_map.get("padding-top") == Some(&val.as_str()) {
                    continue;
                }
                leftover.push((key.clone(), val.clone()));
            }
            "padding-inline" => {
                attrs.push(("padding-inline".into(), Some(strip_px_multi(val))));
            }
            "padding-block" => {
                attrs.push(("padding-block".into(), Some(strip_px_multi(val))));
            }

            "margin" => {
                attrs.push(("margin".into(), Some(strip_px_multi(val))));
            }
            "margin-left" | "margin-right" => {
                if key == "margin-left"
                    && let Some(&mr) = style_map.get("margin-right")
                    && mr == val
                {
                    attrs.push(("margin-x".into(), Some(strip_px(val).into())));
                    continue;
                }
                if key == "margin-right" && style_map.get("margin-left") == Some(&val.as_str()) {
                    continue;
                }
                leftover.push((key.clone(), val.clone()));
            }
            "margin-top" | "margin-bottom" => {
                if key == "margin-top"
                    && let Some(&mb) = style_map.get("margin-bottom")
                    && mb == val
                {
                    attrs.push(("margin-y".into(), Some(strip_px(val).into())));
                    continue;
                }
                if key == "margin-bottom" && style_map.get("margin-top") == Some(&val.as_str()) {
                    continue;
                }
                leftover.push((key.clone(), val.clone()));
            }
            "margin-inline" => {
                attrs.push(("margin-inline".into(), Some(strip_px_multi(val))));
            }
            "margin-block" => {
                attrs.push(("margin-block".into(), Some(strip_px_multi(val))));
            }

            "width" => attrs.push(("width".into(), Some(strip_px(val).into()))),
            "height" => attrs.push(("height".into(), Some(strip_px(val).into()))),
            "min-width" => attrs.push(("min-width".into(), Some(strip_px(val).into()))),
            "max-width" => attrs.push(("max-width".into(), Some(strip_px(val).into()))),
            "min-height" => attrs.push(("min-height".into(), Some(strip_px(val).into()))),
            "max-height" => attrs.push(("max-height".into(), Some(strip_px(val).into()))),

            "gap" => attrs.push(("spacing".into(), Some(strip_px(val).into()))),
            "column-gap" => attrs.push(("gap-x".into(), Some(strip_px(val).into()))),
            "row-gap" => attrs.push(("gap-y".into(), Some(strip_px(val).into()))),

            "background" | "background-color" => {
                attrs.push(("background".into(), Some(val.clone())));
            }
            "color" => attrs.push(("color".into(), Some(val.clone()))),

            "border" => {
                // Try to simplify "1px solid #ccc" -> "1 #ccc"
                attrs.push(("border".into(), Some(simplify_border(val))));
            }
            "border-top" => attrs.push(("border-top".into(), Some(simplify_border(val)))),
            "border-bottom" => attrs.push(("border-bottom".into(), Some(simplify_border(val)))),
            "border-left" => attrs.push(("border-left".into(), Some(simplify_border(val)))),
            "border-right" => attrs.push(("border-right".into(), Some(simplify_border(val)))),

            "border-radius" => {
                attrs.push(("rounded".into(), Some(strip_px(val).into())));
            }

            "font-weight" if val == "bold" || val == "700" => {
                attrs.push(("bold".into(), None));
            }
            "font-weight" => {
                attrs.push(("font-weight".into(), Some(val.clone())));
            }
            "font-style" if val == "italic" => {
                attrs.push(("italic".into(), None));
            }
            "font-style" => {
                attrs.push(("font-style".into(), Some(val.clone())));
            }
            "text-decoration" if val.contains("underline") => {
                attrs.push(("underline".into(), None));
            }
            "text-decoration" => {
                leftover.push((key.clone(), val.clone()));
            }
            "font-size" => attrs.push(("size".into(), Some(strip_px(val).into()))),
            "font-family" => attrs.push(("font".into(), Some(val.clone()))),

            "text-align" => attrs.push(("text-align".into(), Some(val.clone()))),
            "line-height" => attrs.push(("line-height".into(), Some(val.clone()))),
            "letter-spacing" => {
                attrs.push(("letter-spacing".into(), Some(strip_px(val).into())));
            }
            "text-transform" => attrs.push(("text-transform".into(), Some(val.clone()))),
            "white-space" => attrs.push(("white-space".into(), Some(val.clone()))),

            "overflow" => attrs.push(("overflow".into(), Some(val.clone()))),
            "position" => attrs.push(("position".into(), Some(val.clone()))),
            "top" => attrs.push(("top".into(), Some(strip_px(val).into()))),
            "right" => attrs.push(("right".into(), Some(strip_px(val).into()))),
            "bottom" => attrs.push(("bottom".into(), Some(strip_px(val).into()))),
            "left" => attrs.push(("left".into(), Some(strip_px(val).into()))),
            "z-index" => attrs.push(("z-index".into(), Some(val.clone()))),

            "display" => attrs.push(("display".into(), Some(val.clone()))),
            "visibility" => attrs.push(("visibility".into(), Some(val.clone()))),

            "opacity" => attrs.push(("opacity".into(), Some(val.clone()))),
            "cursor" => attrs.push(("cursor".into(), Some(val.clone()))),
            "transition" => attrs.push(("transition".into(), Some(val.clone()))),
            "transform" => attrs.push(("transform".into(), Some(val.clone()))),
            "backdrop-filter" => attrs.push(("backdrop-filter".into(), Some(val.clone()))),
            "box-shadow" => attrs.push(("shadow".into(), Some(val.clone()))),
            "flex-wrap" if val == "wrap" => attrs.push(("wrap".into(), None)),
            "animation" => attrs.push(("animation".into(), Some(val.clone()))),
            "aspect-ratio" => attrs.push(("aspect-ratio".into(), Some(val.clone()))),
            "outline" => attrs.push(("outline".into(), Some(simplify_border(val)))),
            "filter" => attrs.push(("filter".into(), Some(val.clone()))),
            "object-fit" => attrs.push(("object-fit".into(), Some(val.clone()))),
            "object-position" => attrs.push(("object-position".into(), Some(val.clone()))),
            "text-shadow" => attrs.push(("text-shadow".into(), Some(val.clone()))),
            "text-overflow" => attrs.push(("text-overflow".into(), Some(val.clone()))),
            "pointer-events" => attrs.push(("pointer-events".into(), Some(val.clone()))),
            "user-select" => attrs.push(("user-select".into(), Some(val.clone()))),
            "justify-content" => attrs.push(("justify-content".into(), Some(val.clone()))),
            "align-items" => attrs.push(("align-items".into(), Some(val.clone()))),

            "grid-template-columns" => {
                attrs.push(("grid-cols".into(), Some(simplify_grid_repeat(val))));
            }
            "grid-template-rows" => {
                attrs.push(("grid-rows".into(), Some(simplify_grid_repeat(val))));
            }
            "grid-column" => {
                if let Some(rest) = val.strip_prefix("span ") {
                    attrs.push(("col-span".into(), Some(rest.trim().into())));
                } else {
                    leftover.push((key.clone(), val.clone()));
                }
            }
            "grid-row" => {
                if let Some(rest) = val.strip_prefix("span ") {
                    attrs.push(("row-span".into(), Some(rest.trim().into())));
                } else {
                    leftover.push((key.clone(), val.clone()));
                }
            }

            "scroll-snap-type" => attrs.push(("scroll-snap-type".into(), Some(val.clone()))),
            "scroll-snap-align" => attrs.push(("scroll-snap-align".into(), Some(val.clone()))),

            "flex" if val == "1" => attrs.push(("width".into(), Some("fill".into()))),
            "flex-shrink" if val == "0" => attrs.push(("width".into(), Some("shrink".into()))),

            "text-wrap" => attrs.push(("text-wrap".into(), Some(val.clone()))),
            "will-change" => attrs.push(("will-change".into(), Some(val.clone()))),
            "touch-action" => attrs.push(("touch-action".into(), Some(val.clone()))),
            "vertical-align" => attrs.push(("vertical-align".into(), Some(val.clone()))),
            "contain" => attrs.push(("contain".into(), Some(val.clone()))),
            "content-visibility" => attrs.push(("content-visibility".into(), Some(val.clone()))),
            "scroll-margin" => attrs.push(("scroll-margin".into(), Some(strip_px(val).into()))),
            "scroll-padding" => attrs.push(("scroll-padding".into(), Some(strip_px(val).into()))),

            _ => {
                leftover.push((key.clone(), val.clone()));
            }
        }
    }

    (attrs, leftover)
}

/// Simplifies a CSS border value like "1px solid #ccc" to "1 #ccc".
fn simplify_border(val: &str) -> String {
    let parts: Vec<&str> = val.split_whitespace().collect();
    // Common pattern: "1px solid #ccc"
    if parts.len() == 3 && parts[1] == "solid" {
        let width = strip_px(parts[0]);
        let color = parts[2];
        if color == "currentColor" {
            width.to_string()
        } else {
            format!("{} {}", width, color)
        }
    } else if parts.len() == 2 && parts[0].ends_with("px") {
        // "1px #ccc" (no style)
        format!("{} {}", strip_px(parts[0]), parts[1])
    } else {
        val.to_string()
    }
}

/// Simplifies "repeat(3, 1fr)" to "3".
fn simplify_grid_repeat(val: &str) -> String {
    if let Some(inner) = val
        .strip_prefix("repeat(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.splitn(2, ',').collect();
        if parts.len() == 2 && parts[1].trim() == "1fr" {
            return parts[0].trim().to_string();
        }
    }
    val.to_string()
}

// ---------------------------------------------------------------------------
// HTML-to-htmlang conversion
// ---------------------------------------------------------------------------

/// Converts an HTML string to htmlang (.hl) syntax.
pub fn convert(html: &str) -> String {
    let mut parser = HtmlParser::new(html);
    let nodes = parser.parse_nodes(None);

    // Unwrap boilerplate: look inside html > body for the real content.
    let content_nodes = unwrap_boilerplate(&nodes);

    let mut out = String::new();
    for node in content_nodes {
        emit_node(node, 0, &mut out);
    }

    // Trim trailing whitespace but keep a final newline
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{}\n", trimmed)
    }
}

/// Walks through html/head/body wrappers and returns references to the
/// meaningful content nodes.
fn unwrap_boilerplate(nodes: &[HtmlNode]) -> Vec<&HtmlNode> {
    // First pass: skip DOCTYPE, comments, whitespace-only text at top level
    let meaningful: Vec<&HtmlNode> = nodes
        .iter()
        .filter(|n| match n {
            HtmlNode::Raw(_) => false,
            HtmlNode::Comment(_) => false,
            HtmlNode::Text(t) => !t.trim().is_empty(),
            HtmlNode::Element { .. } => true,
        })
        .collect();

    // If there's a single <html> element, unwrap it
    if meaningful.len() == 1
        && let HtmlNode::Element { tag, children, .. } = meaningful[0]
        && tag == "html"
    {
        return unwrap_html_children(children);
    }

    // Otherwise, check if there are any html/head/body tags mixed in
    let mut result = Vec::new();
    for node in nodes {
        match node {
            HtmlNode::Raw(_) => {}
            HtmlNode::Comment(_) => {}
            HtmlNode::Text(t) if t.trim().is_empty() => {}
            HtmlNode::Element { tag, children, .. }
                if tag == "html" || tag == "head" || tag == "body" =>
            {
                if tag == "body" || tag == "html" {
                    for child in children {
                        match child {
                            HtmlNode::Element { tag: t, .. } if t == "head" => {}
                            HtmlNode::Text(t) if t.trim().is_empty() => {}
                            _ => result.push(child),
                        }
                    }
                }
                // Skip <head> entirely
            }
            _ => result.push(node),
        }
    }

    if result.is_empty() {
        // Fallback: return everything that's not boilerplate
        nodes
            .iter()
            .filter(|n| !matches!(n, HtmlNode::Raw(_)))
            .collect()
    } else {
        result
    }
}

fn unwrap_html_children(children: &[HtmlNode]) -> Vec<&HtmlNode> {
    let mut result = Vec::new();
    for child in children {
        match child {
            HtmlNode::Element { tag, children, .. } if tag == "head" => {
                // Skip <head> entirely
                let _ = children;
            }
            HtmlNode::Element { tag, children, .. } if tag == "body" => {
                for body_child in children {
                    match body_child {
                        HtmlNode::Text(t) if t.trim().is_empty() => {}
                        _ => result.push(body_child),
                    }
                }
            }
            HtmlNode::Text(t) if t.trim().is_empty() => {}
            _ => result.push(child),
        }
    }
    result
}

fn indent_str(depth: usize) -> String {
    "  ".repeat(depth)
}

fn emit_node(node: &HtmlNode, depth: usize, out: &mut String) {
    match node {
        HtmlNode::Text(text) => {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                for line in trimmed.lines() {
                    let l = line.trim();
                    if !l.is_empty() {
                        out.push_str(&indent_str(depth));
                        // A bare line starting with `@`, `[` or `--` would be
                        // read as an element, attribute list or comment.
                        if l.starts_with('@') || l.starts_with('[') || l.starts_with("--") {
                            out.push_str("@text ");
                        }
                        out.push_str(l);
                        out.push('\n');
                    }
                }
            }
        }
        HtmlNode::Comment(text) => {
            for line in text.trim().lines() {
                out.push_str(&indent_str(depth));
                out.push_str("-- ");
                out.push_str(line.trim());
                out.push('\n');
            }
        }
        HtmlNode::Raw(_) => {}
        HtmlNode::Element {
            tag,
            attrs,
            children,
            ..
        } => {
            emit_element(tag, attrs, children, depth, out);
        }
    }
}

fn emit_element(
    tag: &str,
    attrs: &[(String, String)],
    children: &[HtmlNode],
    depth: usize,
    out: &mut String,
) {
    let indent = indent_str(depth);

    // Collect HTML attributes
    let style_str = attr_val(attrs, "style").unwrap_or_default();
    let class_str = attr_val(attrs, "class").unwrap_or_default();
    let id_str = attr_val(attrs, "id");
    let href = attr_val(attrs, "href");
    let src = attr_val(attrs, "src");
    let alt = attr_val(attrs, "alt");

    // Parse inline styles
    let css_props = parse_inline_style(&style_str);

    // Detect flex layout from styles
    let is_flex = css_props.iter().any(|(k, v)| k == "display" && v == "flex");
    let flex_dir = css_props
        .iter()
        .find(|(k, _)| k == "flex-direction")
        .map(|(_, v)| v.as_str());

    // Map HTML tag to htmlang element
    let hl_tag = match tag {
        "div" => {
            if is_flex {
                match flex_dir {
                    Some("column") => "@column",
                    _ => "@row",
                }
            } else {
                "@el"
            }
        }
        "span" => "@text",
        "p" => "@paragraph",
        "a" => "@link",
        "img" => "@image",
        "nav" => "@nav",
        "header" => "@header",
        "footer" => "@footer",
        "main" => "@main",
        "section" => "@section",
        "article" => "@article",
        "aside" => "@aside",
        "ul" => "@list",
        "ol" => "@list",
        "li" => "@item",
        "table" => "@table",
        "thead" => "@thead",
        "tbody" => "@tbody",
        "tr" => "@tr",
        "td" => "@td",
        "th" => "@th",
        "form" => "@form",
        "input" => "@input",
        "button" => "@button",
        "select" => "@select",
        "label" => "@label",
        "details" => "@details",
        "summary" => "@summary",
        "blockquote" => "@blockquote",
        "cite" => "@cite",
        "code" => "@code",
        "hr" => "@hr",
        "figure" => "@figure",
        "figcaption" => "@figcaption",
        "video" => "@video",
        "audio" => "@audio",
        "dialog" => "@dialog",
        "dl" => "@dl",
        "dt" => "@dt",
        "dd" => "@dd",
        "fieldset" => "@fieldset",
        "legend" => "@legend",
        "picture" => "@picture",
        "source" => "@source",
        "mark" => "@mark",
        "kbd" => "@kbd",
        "abbr" => "@abbr",
        "time" => "@time",
        "progress" => "@progress",
        "meter" => "@meter",
        "h1" => "@h1",
        "h2" => "@h2",
        "h3" => "@h3",
        "h4" => "@h4",
        "h5" => "@h5",
        "h6" => "@h6",
        "b" | "strong" => "@text",
        "i" | "em" => "@text",
        "u" => "@text",
        // Whitespace-sensitive content: htmlang text lines are trimmed and
        // joined, so keep these as HTML.
        "br" | "pre" | "textarea" => {
            emit_raw_element(tag, attrs, children, depth, out);
            return;
        }
        "iframe" => "@iframe",
        "output" => "@output",
        "canvas" => "@canvas",
        "noscript" => "@noscript",
        "address" => "@address",
        "search" => "@search",
        // Unknown / raw elements
        "script" | "style" | "svg" | "object" | "embed" => {
            emit_raw_element(tag, attrs, children, depth, out);
            return;
        }
        _ => {
            // Try to emit as raw
            emit_raw_element(tag, attrs, children, depth, out);
            return;
        }
    };

    // Build htmlang attributes from CSS + HTML attrs
    let (mut hl_attrs, leftover_css) = css_to_hl_attrs(&css_props);

    // Only a div can become @row/@column. For other flex containers
    // (`<nav style="display:flex">`) keep the semantic element and lay the
    // children out in a nested @row/@column that takes the flex attributes.
    let nested_flex = (is_flex && tag != "div").then(|| {
        let kind = match flex_dir {
            Some("column") | Some("column-reverse") => "@column",
            _ => "@row",
        };
        let (flex_attrs, rest): (Vec<HlAttr>, Vec<HlAttr>) =
            hl_attrs.drain(..).partition(|(k, _)| {
                matches!(
                    k.as_str(),
                    "spacing" | "gap-x" | "gap-y" | "wrap" | "justify-content" | "align-items"
                )
            });
        hl_attrs = rest;
        (kind, flex_attrs)
    });

    // Add id if present
    if let Some(id) = &id_str {
        hl_attrs.insert(0, ("id".into(), Some(id.clone())));
    }

    // Bold/italic for strong/em/b/i/u
    match tag {
        "b" | "strong" if !hl_attrs.iter().any(|(k, _)| k == "bold") => {
            hl_attrs.push(("bold".into(), None));
        }
        "i" | "em" if !hl_attrs.iter().any(|(k, _)| k == "italic") => {
            hl_attrs.push(("italic".into(), None));
        }
        "u" if !hl_attrs.iter().any(|(k, _)| k == "underline") => {
            hl_attrs.push(("underline".into(), None));
        }
        _ => {}
    }

    // Ordered list
    if tag == "ol" {
        hl_attrs.push(("ordered".into(), None));
    }

    // Alt text for images
    if tag == "img"
        && let Some(alt_text) = &alt
        && !alt_text.is_empty()
    {
        hl_attrs.push(("alt".into(), Some(alt_text.clone())));
    }

    // Passthrough HTML attributes that htmlang supports
    for (key, val) in attrs {
        match key.as_str() {
            "style" | "class" | "id" | "href" | "src" | "alt" => {}
            "type" | "name" | "value" | "placeholder" | "disabled" | "required" | "checked"
            | "readonly" | "maxlength" | "min" | "max" | "step" | "action" | "method"
            | "target" | "rel" | "title" | "role" | "aria-label" | "aria-hidden" | "tabindex"
            | "for" | "open" | "autoplay" | "controls" | "loop" | "muted" | "preload" | "width"
            | "height" | "loading" | "decoding" | "srcset" | "sizes" | "media" | "datetime"
            | "cite" | "download" => {
                if val.is_empty() {
                    hl_attrs.push((key.clone(), None));
                } else {
                    hl_attrs.push((key.clone(), Some(val.clone())));
                }
            }
            _ if key.starts_with("data-") || key.starts_with("aria-") => {
                if val.is_empty() {
                    hl_attrs.push((key.clone(), None));
                } else {
                    hl_attrs.push((key.clone(), Some(val.clone())));
                }
            }
            _ => {}
        }
    }

    // Build the line
    out.push_str(&indent);
    out.push_str(hl_tag);

    // Argument (link href, image src)
    if tag == "a" {
        if let Some(url) = &href {
            out.push(' ');
            out.push_str(url);
        }
    } else if tag == "img"
        && let Some(src_val) = &src
    {
        out.push(' ');
        out.push_str(src_val);
    }

    push_attr_block(&hl_attrs, out);
    out.push('\n');

    // Comment with original classes (if any)
    if !class_str.is_empty() {
        out.push_str(&indent_str(depth + 1));
        out.push_str("-- classes: ");
        out.push_str(&class_str);
        out.push('\n');
    }

    // Comment with leftover CSS that couldn't be mapped
    if !leftover_css.is_empty() {
        let css_text: Vec<String> = leftover_css
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        out.push_str(&indent_str(depth + 1));
        out.push_str("-- unmapped css: ");
        out.push_str(&css_text.join("; "));
        out.push('\n');
    }

    // Emit children
    let child_depth = match &nested_flex {
        Some((kind, flex_attrs)) => {
            out.push_str(&indent_str(depth + 1));
            out.push_str(kind);
            push_attr_block(flex_attrs, out);
            out.push('\n');
            depth + 2
        }
        None => depth + 1,
    };
    for child in children {
        emit_node(child, child_depth, out);
    }
}

/// Append ` [key value, ...]` for a non-empty attribute list. Values that
/// contain a `,` or `]` are quoted so they stay one attribute.
fn push_attr_block(attrs: &[HlAttr], out: &mut String) {
    if attrs.is_empty() {
        return;
    }
    out.push_str(" [");
    for (i, (key, val)) in attrs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(key);
        if let Some(v) = val {
            out.push(' ');
            if v.contains(',') || v.contains(']') {
                out.push('"');
                out.push_str(&v.replace('"', "'"));
                out.push('"');
            } else {
                out.push_str(v);
            }
        }
    }
    out.push(']');
}

fn emit_raw_element(
    tag: &str,
    attrs: &[(String, String)],
    children: &[HtmlNode],
    depth: usize,
    out: &mut String,
) {
    let indent = indent_str(depth);

    // Reconstruct the HTML for this element
    let mut html = String::new();
    html.push('<');
    html.push_str(tag);
    for (key, val) in attrs {
        html.push(' ');
        html.push_str(key);
        if !val.is_empty() {
            html.push_str("=\"");
            html.push_str(&html_escape(val));
            html.push('"');
        }
    }
    if VOID_TAGS.contains(&tag) {
        html.push('>');
    } else if children.is_empty() {
        html.push_str(&format!("></{}>", tag));
    } else {
        html.push('>');
        reconstruct_html(children, is_raw_text_tag(tag), &mut html);
        html.push_str(&format!("</{}>", tag));
    }

    let trimmed = html.trim();
    if trimmed.contains('\n') || trimmed.len() > 60 {
        out.push_str(&indent);
        out.push_str("@raw \"\"\"\n");
        // Raw content is emitted verbatim by the compiler, so indenting it
        // here would change the output (e.g. inside <pre>).
        for line in trimmed.lines() {
            out.push_str(line);
            out.push('\n');
        }
        out.push_str(&indent);
        out.push_str("\"\"\"\n");
    } else {
        out.push_str(&indent);
        out.push_str("@raw \"\"\"");
        out.push_str(trimmed);
        out.push_str("\"\"\"\n");
    }
}

/// Elements whose text content is not HTML (and was not entity-decoded).
fn is_raw_text_tag(tag: &str) -> bool {
    tag == "script" || tag == "style"
}

fn reconstruct_html(nodes: &[HtmlNode], raw_text: bool, out: &mut String) {
    for node in nodes {
        match node {
            HtmlNode::Text(t) if raw_text => out.push_str(t),
            HtmlNode::Text(t) => out.push_str(&html_escape(t)),
            HtmlNode::Comment(c) => {
                out.push_str("<!--");
                out.push_str(c);
                out.push_str("-->");
            }
            HtmlNode::Raw(r) => out.push_str(r),
            HtmlNode::Element {
                tag,
                attrs,
                children,
                self_closing,
            } => {
                out.push('<');
                out.push_str(tag);
                for (key, val) in attrs {
                    out.push(' ');
                    out.push_str(key);
                    if !val.is_empty() {
                        out.push_str("=\"");
                        out.push_str(&html_escape(val));
                        out.push('"');
                    }
                }
                if *self_closing {
                    out.push_str(" />");
                } else {
                    out.push('>');
                    reconstruct_html(children, is_raw_text_tag(tag), out);
                    out.push_str(&format!("</{}>", tag));
                }
            }
        }
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn attr_val(attrs: &[(String, String)], key: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_div() {
        let html =
            r#"<div style="padding: 20px; background-color: #fff; border-radius: 8px">Hello</div>"#;
        let result = convert(html);
        assert!(result.contains("@el [padding 20, background #fff, rounded 8]"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn flex_row() {
        let html = r#"<div style="display: flex; flex-direction: row; gap: 10px"><span>A</span><span>B</span></div>"#;
        let result = convert(html);
        assert!(result.contains("@row [spacing 10]"));
    }

    #[test]
    fn flex_column() {
        let html = r#"<div style="display: flex; flex-direction: column"><p>One</p></div>"#;
        let result = convert(html);
        assert!(result.contains("@column"));
    }

    #[test]
    fn link_element() {
        let html = r#"<a href="https://example.com">Click me</a>"#;
        let result = convert(html);
        assert!(result.contains("@link https://example.com"));
        assert!(result.contains("Click me"));
    }

    #[test]
    fn image_element() {
        let html = r#"<img src="photo.jpg" alt="A photo">"#;
        let result = convert(html);
        assert!(result.contains("@image photo.jpg"));
    }

    #[test]
    fn nested_structure() {
        let html = r#"<div><p>Hello</p><p>World</p></div>"#;
        let result = convert(html);
        assert!(result.contains("@el\n"));
        assert!(result.contains("  @paragraph\n"));
        assert!(result.contains("    Hello\n"));
    }

    #[test]
    fn strips_boilerplate() {
        let html = r#"<!DOCTYPE html><html><head><title>Test</title></head><body><div>Content</div></body></html>"#;
        let result = convert(html);
        assert!(!result.contains("html"));
        assert!(!result.contains("head"));
        assert!(!result.contains("body"));
        assert!(result.contains("@el"));
        assert!(result.contains("Content"));
    }

    #[test]
    fn script_to_raw() {
        let html = r#"<script>alert("hi");</script>"#;
        let result = convert(html);
        assert!(result.contains("@raw"));
        assert!(result.contains("alert"));
    }

    #[test]
    fn classes_as_comments() {
        let html = r#"<div class="container main-content">Text</div>"#;
        let result = convert(html);
        assert!(result.contains("-- classes: container main-content"));
    }

    #[test]
    fn heading_elements() {
        let h1 = convert("<h1>Title</h1>");
        assert!(h1.contains("@h1"));
        let h3 = convert("<h3>Sub</h3>");
        assert!(h3.contains("@h3"));
    }

    #[test]
    fn strong_bold() {
        let result = convert("<strong>Important</strong>");
        assert!(result.contains("@text [bold]"));
    }

    #[test]
    fn ordered_list() {
        let result = convert("<ol><li>First</li><li>Second</li></ol>");
        assert!(result.contains("@list [ordered]"));
        assert!(result.contains("@item"));
    }

    #[test]
    fn semantic_elements() {
        let result = convert("<nav><a href=\"/\">Home</a></nav>");
        assert!(result.contains("@nav"));
        assert!(result.contains("@link /"));
    }

    #[test]
    fn table_structure() {
        let html = "<table><thead><tr><th>Name</th></tr></thead><tbody><tr><td>Alice</td></tr></tbody></table>";
        let result = convert(html);
        assert!(result.contains("@table"));
        assert!(result.contains("@thead"));
        assert!(result.contains("@th"));
        assert!(result.contains("@tbody"));
        assert!(result.contains("@td"));
    }

    #[test]
    fn form_elements() {
        let html = r#"<form action="/submit"><input type="text" placeholder="Name"><button>Go</button></form>"#;
        let result = convert(html);
        assert!(result.contains("@form [action /submit]"));
        assert!(result.contains("@input [type text, placeholder Name]"));
        assert!(result.contains("@button"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(convert("").trim(), "");
        assert_eq!(convert("   ").trim(), "");
    }

    #[test]
    fn border_simplification() {
        let result = convert(r#"<div style="border: 1px solid #ccc">X</div>"#);
        assert!(result.contains("border 1 #ccc"));
    }

    #[test]
    fn rounded_shorthand() {
        let result = convert(r#"<div style="border-radius: 8px">X</div>"#);
        assert!(result.contains("rounded 8"));
    }

    #[test]
    fn bare_less_than_is_text() {
        let result = convert("<p>a < b and 1 <2 ok</p><p>after</p>");
        assert!(result.contains("a < b and 1 <2 ok"), "{result}");
        assert!(result.contains("after"), "{result}");
        assert!(!result.contains("@raw"), "{result}");
    }

    #[test]
    fn script_content_is_not_escaped() {
        let result = convert("<script>if (a < b && c > d) {}</script>");
        assert!(result.contains("if (a < b && c > d) {}"), "{result}");
    }

    #[test]
    fn entities_decode_once_and_fully() {
        assert_eq!(decode_entities("&amp;lt;tag&amp;gt;"), "&lt;tag&gt;");
        assert_eq!(decode_entities("&copy; &#8212; &#x2014; &bogus; a&b"), "© — — &bogus; a&b");
    }

    #[test]
    fn values_with_commas_are_quoted() {
        let result = convert(r#"<input type="text" placeholder="Name, email">"#);
        assert!(result.contains(r#"placeholder "Name, email""#), "{result}");
    }

    #[test]
    fn text_that_looks_like_syntax_is_escaped() {
        let result = convert("<p>-- not a comment</p><p>@mention</p>");
        assert!(result.contains("@text -- not a comment"), "{result}");
        assert!(result.contains("@text @mention"), "{result}");
    }

    #[test]
    fn whitespace_sensitive_elements_stay_raw() {
        let result = convert("<div>line1<br>line2</div><pre>  indented\n    more</pre>");
        assert!(result.contains(r#"@raw """<br>""""#), "{result}");
        assert!(result.contains("<pre>  indented\n    more</pre>"), "{result}");
    }

    #[test]
    fn flex_on_semantic_element_nests_a_row() {
        let result = convert(r#"<nav style="display:flex; gap: 8px"><a href="/">Home</a></nav>"#);
        assert!(result.contains("@nav\n  @row [spacing 8]\n    @link /"), "{result}");
    }

    #[test]
    fn large_script_converts_quickly() {
        let body = "x".repeat(200_000);
        let start = std::time::Instant::now();
        let result = convert(&format!("<script>{body}</script>"));
        assert!(result.contains(&body[..100]));
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}
