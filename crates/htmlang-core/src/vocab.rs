//! The attribute vocabulary.
//!
//! An attribute in `[...]` is one of three things:
//! - one of htmlang's own attributes, which mean more than a single CSS
//!   property (`spacing`, `center-x`, `width fill`, ...), handled in codegen;
//! - a standard CSS property, copied into the generated CSS unchanged;
//! - an HTML attribute, written `key=value` (or bare, for booleans such as
//!   `required`) and emitted on the element.

/// htmlang's own attributes (those that are neither a CSS property nor an
/// HTML attribute).
#[rustfmt::skip]
pub const HTMLANG_ATTRIBUTES: &[&str] = &[
    "align-bottom", "align-left", "align-right", "align-top", "center-x", "center-y",
    "col-span", "grid-cols", "grid-rows", "inline", "row-span",
    "spacing", "wrap",
];

/// htmlang's own attributes about how an element lays out its children:
/// they work on a row, column or grid (`ast::Layout::is_container`).
pub const CONTAINER_ATTRIBUTES: &[&str] = &["spacing", "wrap", "grid-cols", "grid-rows"];

/// The words of htmlang's own that `@page` takes besides its styles and
/// HTML attributes: `favicon FILE` puts the file into the page as its icon.
pub const PAGE_WORDS: &[&str] = &["favicon"];

/// htmlang's own attributes that are flags, written without a value
/// (`center-x`); the others take one (`spacing 8`).
pub const HTMLANG_FLAGS: &[&str] = &[
    "align-bottom",
    "align-left",
    "align-right",
    "align-top",
    "center-x",
    "center-y",
    "inline",
    "wrap",
];

/// Standard CSS properties, sorted. Any of these can be written as an
/// attribute and is copied into the generated CSS as-is.
#[rustfmt::skip]
pub const CSS_PROPERTIES: &[&str] = &[
    "accent-color", "align-content", "align-items", "align-self", "all", "anchor-name",
    "animation", "animation-composition", "animation-delay", "animation-direction",
    "animation-duration", "animation-fill-mode", "animation-iteration-count", "animation-name",
    "animation-play-state", "animation-range", "animation-range-end", "animation-range-start",
    "animation-timeline", "animation-timing-function", "appearance", "aspect-ratio",
    "backdrop-filter", "backface-visibility", "background", "background-attachment",
    "background-blend-mode", "background-clip", "background-color", "background-image",
    "background-origin", "background-position", "background-position-x",
    "background-position-y", "background-repeat", "background-size", "block-size", "border",
    "border-block", "border-block-color", "border-block-end", "border-block-end-color",
    "border-block-end-style", "border-block-end-width", "border-block-start",
    "border-block-start-color", "border-block-start-style", "border-block-start-width",
    "border-block-style", "border-block-width", "border-bottom", "border-bottom-color",
    "border-bottom-left-radius", "border-bottom-right-radius", "border-bottom-style",
    "border-bottom-width", "border-collapse", "border-color", "border-end-end-radius",
    "border-end-start-radius", "border-image", "border-image-outset", "border-image-repeat",
    "border-image-slice", "border-image-source", "border-image-width", "border-inline",
    "border-inline-color", "border-inline-end", "border-inline-end-color",
    "border-inline-end-style", "border-inline-end-width", "border-inline-start",
    "border-inline-start-color", "border-inline-start-style", "border-inline-start-width",
    "border-inline-style", "border-inline-width", "border-left", "border-left-color",
    "border-left-style", "border-left-width", "border-radius", "border-right",
    "border-right-color", "border-right-style", "border-right-width", "border-spacing",
    "border-start-end-radius", "border-start-start-radius", "border-style", "border-top",
    "border-top-color", "border-top-left-radius", "border-top-right-radius", "border-top-style",
    "border-top-width", "border-width", "bottom", "box-decoration-break", "box-shadow",
    "box-sizing", "break-after", "break-before", "break-inside", "caption-side", "caret-color",
    "clear", "clip", "clip-path", "color", "color-scheme", "column-count", "column-fill",
    "column-gap", "column-rule", "column-rule-color", "column-rule-style", "column-rule-width",
    "column-span", "column-width", "columns", "contain", "contain-intrinsic-block-size",
    "contain-intrinsic-height", "contain-intrinsic-inline-size", "contain-intrinsic-size",
    "contain-intrinsic-width", "container", "container-name", "container-type", "content",
    "content-visibility", "counter-increment", "counter-reset", "counter-set", "cursor",
    "direction", "display", "empty-cells", "field-sizing", "filter", "flex", "flex-basis",
    "flex-direction", "flex-flow", "flex-grow", "flex-shrink", "flex-wrap", "float", "font",
    "font-family", "font-feature-settings", "font-kerning", "font-language-override",
    "font-optical-sizing", "font-palette", "font-size", "font-size-adjust", "font-stretch",
    "font-style", "font-synthesis", "font-variant", "font-variant-alternates",
    "font-variant-caps", "font-variant-east-asian", "font-variant-ligatures",
    "font-variant-numeric", "font-variant-position", "font-variation-settings", "font-weight",
    "forced-color-adjust", "gap", "grid", "grid-area", "grid-auto-columns", "grid-auto-flow",
    "grid-auto-rows", "grid-column", "grid-column-end", "grid-column-start", "grid-row",
    "grid-row-end", "grid-row-start", "grid-template", "grid-template-areas",
    "grid-template-columns", "grid-template-rows", "hanging-punctuation", "height",
    "hyphenate-character", "hyphens", "image-orientation", "image-rendering", "initial-letter",
    "inline-size", "inset", "inset-block", "inset-block-end", "inset-block-start",
    "inset-inline", "inset-inline-end", "inset-inline-start", "interpolate-size", "isolation",
    "justify-content", "justify-items", "justify-self", "left", "letter-spacing", "line-break",
    "line-clamp", "line-height", "list-style", "list-style-image", "list-style-position",
    "list-style-type", "margin", "margin-block", "margin-block-end", "margin-block-start",
    "margin-bottom", "margin-inline", "margin-inline-end", "margin-inline-start", "margin-left",
    "margin-right", "margin-top", "mask", "mask-clip", "mask-composite", "mask-image",
    "mask-mode", "mask-origin", "mask-position", "mask-repeat", "mask-size", "mask-type",
    "math-depth", "math-style", "max-block-size", "max-height", "max-inline-size", "max-width",
    "min-block-size", "min-height", "min-inline-size", "min-width", "mix-blend-mode",
    "object-fit", "object-position", "offset", "offset-anchor", "offset-distance",
    "offset-path", "offset-position", "offset-rotate", "opacity", "order", "orphans", "outline",
    "outline-color", "outline-offset", "outline-style", "outline-width", "overflow",
    "overflow-anchor", "overflow-block", "overflow-clip-margin", "overflow-inline",
    "overflow-wrap", "overflow-x", "overflow-y", "overscroll-behavior",
    "overscroll-behavior-block", "overscroll-behavior-inline", "overscroll-behavior-x",
    "overscroll-behavior-y", "padding", "padding-block", "padding-block-end",
    "padding-block-start", "padding-bottom", "padding-inline", "padding-inline-end",
    "padding-inline-start", "padding-left", "padding-right", "padding-top", "page",
    "page-break-after", "page-break-before", "page-break-inside", "paint-order", "perspective",
    "perspective-origin", "place-content", "place-items", "place-self", "pointer-events",
    "position", "position-anchor", "position-area", "position-try", "position-try-fallbacks",
    "position-visibility", "print-color-adjust", "quotes", "resize", "right", "rotate",
    "row-gap", "ruby-align", "ruby-position", "scale", "scroll-behavior", "scroll-margin",
    "scroll-margin-block", "scroll-margin-block-end", "scroll-margin-block-start",
    "scroll-margin-bottom", "scroll-margin-inline", "scroll-margin-inline-end",
    "scroll-margin-inline-start", "scroll-margin-left", "scroll-margin-right",
    "scroll-margin-top", "scroll-padding", "scroll-padding-block", "scroll-padding-block-end",
    "scroll-padding-block-start", "scroll-padding-bottom", "scroll-padding-inline",
    "scroll-padding-inline-end", "scroll-padding-inline-start", "scroll-padding-left",
    "scroll-padding-right", "scroll-padding-top", "scroll-snap-align", "scroll-snap-stop",
    "scroll-snap-type", "scroll-timeline", "scroll-timeline-axis", "scroll-timeline-name",
    "scrollbar-color", "scrollbar-gutter", "scrollbar-width", "shape-image-threshold",
    "shape-margin", "shape-outside", "tab-size", "table-layout", "text-align",
    "text-align-last", "text-anchor", "text-box", "text-box-edge", "text-box-trim",
    "text-combine-upright", "text-decoration", "text-decoration-color", "text-decoration-line",
    "text-decoration-skip-ink", "text-decoration-style", "text-decoration-thickness",
    "text-emphasis", "text-emphasis-color", "text-emphasis-position", "text-emphasis-style",
    "text-indent", "text-justify", "text-orientation", "text-overflow", "text-rendering",
    "text-shadow", "text-transform", "text-underline-offset", "text-underline-position",
    "text-wrap", "text-wrap-mode", "text-wrap-style", "timeline-scope", "top", "touch-action",
    "transform", "transform-box", "transform-origin", "transform-style", "transition",
    "transition-behavior", "transition-delay", "transition-duration", "transition-property",
    "transition-timing-function", "translate", "unicode-bidi", "user-select", "vertical-align",
    "view-timeline", "view-timeline-axis", "view-timeline-inset", "view-timeline-name",
    "view-transition-class", "view-transition-name", "visibility", "white-space",
    "white-space-collapse", "widows", "width", "will-change", "word-break", "word-spacing",
    "writing-mode", "z-index", "zoom",
];

/// Boolean HTML attributes, written bare (`[required]`) and rendered
/// without a value (`<input required>`). HTML attributes with a value are
/// written `key=value`.
#[rustfmt::skip]
pub const BOOLEAN_HTML_ATTRS: &[&str] = &[
    "allowfullscreen", "async", "autofocus", "autoplay", "checked", "controls", "default",
    "defer", "disabled", "download", "formnovalidate", "hidden", "inert", "ismap", "itemscope",
    "loop", "multiple", "muted", "nomodule", "novalidate", "open", "playsinline", "popover",
    "readonly", "required", "reversed", "sandbox", "selected",
];

/// Common HTML attribute names, used to suggest `key=value` when one is
/// written like a style (`type email`). Any name works with `=`.
#[rustfmt::skip]
pub const HTML_ATTRIBUTES: &[&str] = &[
    "abbr", "accept", "action", "allow", "allowfullscreen", "alt", "aria-atomic", "aria-live",
    "aria-relevant", "async", "autocomplete", "autofocus", "autoplay", "blocking", "charset",
    "checked", "cite", "class", "cols", "colspan", "content", "contenteditable", "controls",
    "coords", "crossorigin", "datetime", "decoding", "defer", "dir", "disabled", "download",
    "draggable", "enctype", "enterkeyhint", "fetchpriority", "for", "form", "formaction",
    "formmethod", "formtarget", "headers", "height", "hidden", "high", "href", "hreflang",
    "http-equiv", "id", "inert", "inputmode", "integrity", "ismap", "kind", "label", "lang",
    "list", "loading", "loop", "low", "max", "maxlength", "media", "method", "min", "multiple",
    "muted", "name", "nomodule", "novalidate", "open", "optimum", "pattern", "placeholder",
    "playsinline", "popover", "popovertarget", "popovertargetaction", "poster", "preload",
    "readonly", "referrerpolicy", "rel", "required", "reversed", "role", "rows", "rowspan",
    "sandbox", "scope", "selected", "shape", "size", "sizes", "span", "spellcheck", "src",
    "srclang", "srcset", "start", "step", "tabindex", "target", "title", "translate", "type",
    "usemap", "value", "width", "wrap",
];

/// The CSS properties whose values are lengths. In them, and only in them,
/// a bare number is pixels: `padding 8 16`, `box-shadow 0 2 4 black`. Every
/// other property, custom properties included, takes its value as written,
/// so the numbers of `flex 1 1 240px`, `grid-column 1 / 3`, `opacity 0.5`
/// or `border-image-width 2` stay numbers. Sorted, for the binary search.
#[rustfmt::skip]
pub const LENGTH_PROPERTIES: &[&str] = &[
    "background-position", "background-position-x", "background-position-y",
    "background-size", "block-size", "border", "border-block", "border-block-end",
    "border-block-end-width", "border-block-start", "border-block-start-width",
    "border-block-width", "border-bottom", "border-bottom-left-radius",
    "border-bottom-right-radius", "border-bottom-width", "border-end-end-radius",
    "border-end-start-radius", "border-inline", "border-inline-end", "border-inline-end-width",
    "border-inline-start", "border-inline-start-width", "border-inline-width", "border-left",
    "border-left-width", "border-radius", "border-right", "border-right-width",
    "border-spacing", "border-start-end-radius", "border-start-start-radius", "border-top",
    "border-top-left-radius", "border-top-right-radius", "border-top-width", "border-width",
    "bottom", "box-shadow", "column-gap", "column-rule", "column-rule-width", "column-width",
    "contain-intrinsic-block-size", "contain-intrinsic-height", "contain-intrinsic-inline-size",
    "contain-intrinsic-size", "contain-intrinsic-width", "flex-basis", "font-size", "gap",
    "grid-auto-columns", "grid-auto-rows", "grid-template-columns", "grid-template-rows",
    "height", "inline-size", "inset", "inset-block", "inset-block-end", "inset-block-start",
    "inset-inline", "inset-inline-end", "inset-inline-start", "left", "letter-spacing",
    "margin", "margin-block", "margin-block-end", "margin-block-start", "margin-bottom",
    "margin-inline", "margin-inline-end", "margin-inline-start", "margin-left", "margin-right",
    "margin-top", "mask-position", "mask-size", "max-block-size", "max-height",
    "max-inline-size", "max-width", "min-block-size", "min-height", "min-inline-size",
    "min-width", "object-position", "outline", "outline-offset", "outline-width", "padding",
    "padding-block", "padding-block-end", "padding-block-start", "padding-bottom",
    "padding-inline", "padding-inline-end", "padding-inline-start", "padding-left",
    "padding-right", "padding-top", "perspective", "perspective-origin", "right", "row-gap",
    "scroll-margin", "scroll-margin-block", "scroll-margin-block-end",
    "scroll-margin-block-start", "scroll-margin-bottom", "scroll-margin-inline",
    "scroll-margin-inline-end", "scroll-margin-inline-start", "scroll-margin-left",
    "scroll-margin-right", "scroll-margin-top", "scroll-padding", "scroll-padding-block",
    "scroll-padding-block-end", "scroll-padding-block-start", "scroll-padding-bottom",
    "scroll-padding-inline", "scroll-padding-inline-end", "scroll-padding-inline-start",
    "scroll-padding-left", "scroll-padding-right", "scroll-padding-top", "text-decoration-thickness",
    "text-indent", "text-shadow", "text-underline-offset", "top", "transform-origin",
    "translate", "width", "word-spacing",
];

/// A property whose value is a length (see [`LENGTH_PROPERTIES`]).
pub fn is_length_property(name: &str) -> bool {
    LENGTH_PROPERTIES.binary_search(&name).is_ok()
}

/// The value of `property` as it goes into the CSS. In a length property,
/// every bare number outside parentheses and quotes gets `px`: `0 2 4
/// rgba(0,0,0,.1)` is `0 2px 4px rgba(0,0,0,.1)`. A zero stays `0`, and
/// numbers inside a function (`calc(100% - 20)`, `rgb(255 128 0)`) are
/// the function's. Any other property's value is written as it is.
pub fn with_px(property: &str, value: &str) -> String {
    let value = value.trim();
    if !is_length_property(property) {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 8);
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    // Where the current word started in `value`, while one is open
    let mut word: Option<usize> = None;
    let end_word = |out: &mut String, word: &mut Option<usize>, at: usize| {
        if let Some(start) = word.take()
            && is_bare_number(&value[start..at])
            && value[start..at].parse::<f64>().is_ok_and(|n| n != 0.0)
        {
            out.push_str("px");
        }
    };
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth = depth.saturating_sub(1);
        }
        // Words are separated by spaces, commas and slashes outside
        // parentheses and quotes, so a function or a string is part of a
        // word, which is then not a bare number
        if depth == 0 && quote.is_none() && (c.is_whitespace() || c == ',' || c == '/') {
            end_word(&mut out, &mut word, i);
        } else if word.is_none() {
            word = Some(i);
        }
        out.push(c);
    }
    end_word(&mut out, &mut word, value.len());
    out
}

/// `12`, `-4`, `+1.5`, `.5`: a sign, digits and at most one decimal point,
/// with a digit after it. Nothing else (`10px`, `1e3`, `#333`, `1.`).
fn is_bare_number(word: &str) -> bool {
    let digits = word.strip_prefix(['-', '+']).unwrap_or(word);
    let (int, frac) = match digits.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (digits, None),
    };
    int.bytes().all(|b| b.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        && (!int.is_empty() || frac.is_some())
}

/// State prefixes and the selector each adds: `hover:color red` styles
/// `.x:hover`, `children:flex-shrink 0` styles `.x > *`.
pub const PSEUDO_PREFIXES: &[(&str, &str)] = &[
    ("hover:", ":hover"),
    ("active:", ":active"),
    ("focus:", ":focus"),
    ("focus-visible:", ":focus-visible"),
    ("focus-within:", ":focus-within"),
    ("disabled:", ":disabled"),
    ("checked:", ":checked"),
    ("placeholder:", "::placeholder"),
    ("first:", ":first-child"),
    ("last:", ":last-child"),
    ("odd:", ":nth-child(odd)"),
    ("even:", ":nth-child(even)"),
    ("before:", "::before"),
    ("after:", "::after"),
    ("selection:", "::selection"),
    ("visited:", ":visited"),
    ("empty:", ":empty"),
    ("target:", ":target"),
    ("valid:", ":valid"),
    ("invalid:", ":invalid"),
    ("children:", " > *"),
];

/// Viewport-width prefixes (`md:padding 32`).
pub const RESPONSIVE_PREFIXES: &[&str] = &["sm:", "md:", "lg:", "xl:", "2xl:"];
pub const MEDIA_PREFIXES: &[&str] = &[
    "dark:",
    "print:",
    "motion-safe:",
    "motion-reduce:",
    "landscape:",
    "portrait:",
];
pub const CONTAINER_QUERY_PREFIXES: &[&str] = &["cq-sm:", "cq-md:", "cq-lg:", "cq-xl:", "cq-2xl:"];

/// Does `key` carry any state, media, responsive or container prefix?
pub fn is_prefixed(key: &str) -> bool {
    prefix_len(key).is_some()
}

/// The length of the prefix `key` starts with (its `:` included), when it
/// starts with one: `hover:`, `md:`, `nth:2n+1:`, `has(> img):`.
pub fn prefix_len(key: &str) -> Option<usize> {
    let known = PSEUDO_PREFIXES
        .iter()
        .map(|&(p, _)| p)
        .chain(RESPONSIVE_PREFIXES.iter().copied())
        .chain(MEDIA_PREFIXES.iter().copied())
        .chain(CONTAINER_QUERY_PREFIXES.iter().copied())
        .find(|p| key.starts_with(p));
    if let Some(prefix) = known {
        return Some(prefix.len());
    }
    if let Some(rest) = key.strip_prefix("nth:") {
        return rest.find(':').map(|colon| 4 + colon + 1);
    }
    if key.starts_with("has(") {
        // The selector may hold parentheses and colons of its own
        let mut depth = 0;
        for (i, c) in key.char_indices().skip(3) {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return key[i + 1..].starts_with(':').then_some(i + 2);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// The prefixes `key` starts with, and the rest: `md:hover:color` →
/// `(["md:", "hover:"], "color")`.
pub fn split_prefixes(key: &str) -> (Vec<&str>, &str) {
    let mut prefixes = Vec::new();
    let mut rest = key;
    while let Some(len) = prefix_len(rest) {
        prefixes.push(&rest[..len]);
        rest = &rest[len..];
    }
    (prefixes, rest)
}

/// Every prefix's name, for "did you mean" suggestions.
pub fn all_prefixes() -> Vec<&'static str> {
    PSEUDO_PREFIXES
        .iter()
        .map(|&(p, _)| p)
        .chain(RESPONSIVE_PREFIXES.iter().copied())
        .chain(MEDIA_PREFIXES.iter().copied())
        .chain(CONTAINER_QUERY_PREFIXES.iter().copied())
        .chain(["nth:", "has("])
        .collect()
}

/// The attribute name without its prefixes: `hover:md:background` →
/// `background`, `nth:2n:color` → `color`, `has(.x):padding` → `padding`.
pub fn base_attribute(key: &str) -> &str {
    split_prefixes(key).1
}

/// A CSS custom property's name: `--brand`.
pub fn is_custom_property(name: &str) -> bool {
    name.strip_prefix("--").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    })
}

/// A vendor-prefixed property's name: `-webkit-tap-highlight-color`.
pub fn is_vendor_property(name: &str) -> bool {
    name.strip_prefix('-').is_some_and(|rest| {
        rest.split_once('-').is_some_and(|(vendor, property)| {
            !vendor.is_empty()
                && vendor.chars().all(|c| c.is_ascii_lowercase())
                && is_ident(property)
        })
    })
}

/// A word CSS could have as a property's name: letters, digits and `-`,
/// starting with a letter, or a custom or vendor-prefixed name. Such a name
/// is passed to the CSS as written, even when htmlang doesn't know it.
pub fn is_property_name(name: &str) -> bool {
    is_ident(name) || is_custom_property(name) || is_vendor_property(name)
}

fn is_ident(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && !name.ends_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub fn is_css_property(name: &str) -> bool {
    CSS_PROPERTIES.binary_search(&name).is_ok()
}

/// A style attribute: one of htmlang's own or a CSS property.
pub fn is_style_attribute(name: &str) -> bool {
    HTMLANG_ATTRIBUTES.contains(&name) || is_css_property(name)
}

/// All known attribute names, for "did you mean" suggestions.
pub fn all_attributes() -> Vec<&'static str> {
    HTMLANG_ATTRIBUTES
        .iter()
        .chain(CSS_PROPERTIES)
        .chain(HTML_ATTRIBUTES)
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bare_number_in_a_length_is_px() {
        use super::with_px;
        assert_eq!(with_px("padding", "8 16"), "8px 16px");
        assert_eq!(with_px("width", "200"), "200px");
        assert_eq!(with_px("margin", "0 auto"), "0 auto");
        assert_eq!(with_px("margin", "-10 +2.5 .5"), "-10px +2.5px .5px");
        assert_eq!(
            with_px("box-shadow", "0 2 4 rgba(0,0,0,.1)"),
            "0 2px 4px rgba(0,0,0,.1)"
        );
        assert_eq!(
            with_px("box-shadow", "0 1 red, inset 0 0 0 1 blue"),
            "0 1px red, inset 0 0 0 1px blue"
        );
        assert_eq!(
            with_px("border", "1 solid rgb(255 128 0)"),
            "1px solid rgb(255 128 0)"
        );
        assert_eq!(with_px("border-radius", "8/4"), "8px/4px");
        assert_eq!(with_px("grid-template-columns", "200 1fr"), "200px 1fr");
        assert_eq!(
            with_px("grid-template-columns", "repeat(3, 100) minmax(0, 1fr)"),
            "repeat(3, 100) minmax(0, 1fr)"
        );
        assert_eq!(with_px("inset", "0 10"), "0 10px");
        assert_eq!(with_px("outline", "2 dashed red"), "2px dashed red");
        assert_eq!(with_px("width", "calc(100% - 20)"), "calc(100% - 20)");
        assert_eq!(with_px("width", "10px"), "10px");
        assert_eq!(with_px("font-size", "1.2em"), "1.2em");
        assert_eq!(
            with_px("margin", "1e3 1. #333 10 !important"),
            "1e3 1. #333 10px !important"
        );
        assert_eq!(with_px("width", "var(--w, 10)"), "var(--w, 10)");
        // Zero stays zero, however it is written
        assert_eq!(with_px("padding", "0 0.0 -0"), "0 0.0 -0");
        // Numbers in quotes are text
        assert_eq!(with_px("text-shadow", "0 0 2 \"4\""), "0 0 2px \"4\"");
        // Number-valued properties and custom properties are as written
        for (property, value) in [
            ("flex", "1 1 240"),
            ("grid-column", "1 / 3"),
            ("grid-row", "span 2"),
            ("initial-letter", "3"),
            ("columns", "3"),
            ("opacity", "0.5"),
            ("z-index", "2"),
            ("line-height", "1.5"),
            ("font-weight", "700"),
            ("aspect-ratio", "16 / 9"),
            ("scale", "2"),
            ("border-image-width", "2"),
            ("border-image-slice", "30"),
            ("animation-iteration-count", "3"),
            ("--gap", "12"),
            ("-webkit-margin-start", "4"),
            ("corner-radius", "4"),
        ] {
            assert_eq!(with_px(property, value), value, "{property}");
        }
    }

    #[test]
    fn every_length_property_is_a_css_property() {
        let table = super::LENGTH_PROPERTIES;
        assert!(
            table.windows(2).all(|w| w[0] < w[1]),
            "sorted, no duplicates"
        );
        for name in table {
            assert!(super::is_css_property(name), "{name}");
            assert!(!name.starts_with("border-image"), "{name}");
        }
        // Every border shorthand, width and radius is a length
        for name in super::CSS_PROPERTIES
            .iter()
            .filter(|n| n.starts_with("border"))
        {
            let takes_length = !name.starts_with("border-image")
                && !name.ends_with("-color")
                && !name.ends_with("-style")
                && *name != "border-collapse";
            assert_eq!(super::is_length_property(name), takes_length, "{name}");
        }
    }

    #[test]
    fn base_attribute_strips_every_prefix() {
        assert_eq!(super::base_attribute("hover:md:background"), "background");
        assert_eq!(super::base_attribute("nth:2n:color"), "color");
        assert_eq!(super::base_attribute("has(.a):dark:padding"), "padding");
        assert_eq!(super::base_attribute("children:flex-shrink"), "flex-shrink");
        assert_eq!(super::base_attribute("width"), "width");
        assert_eq!(super::base_attribute("has(:not(.a)):color"), "color");
        assert_eq!(super::base_attribute("hovr:color"), "hovr:color");
    }

    #[test]
    fn prefixes_split_one_by_one() {
        assert_eq!(
            super::split_prefixes("md:hover:color"),
            (vec!["md:", "hover:"], "color")
        );
        assert_eq!(
            super::split_prefixes("nth:2n+1:padding"),
            (vec!["nth:2n+1:"], "padding")
        );
        assert_eq!(
            super::split_prefixes("has(a:hover)"),
            (vec![], "has(a:hover)")
        );
    }

    #[test]
    fn property_names_are_checked_by_form() {
        for name in [
            "corner-shape",
            "--brand",
            "--x_1",
            "-webkit-tap-highlight-color",
            "-moz-x",
        ] {
            assert!(super::is_property_name(name), "{}", name);
        }
        for name in [
            "20",
            "a!b",
            "-x",
            "--",
            "colr-",
            "hovr:color",
            "-Webkit-x",
            "",
        ] {
            assert!(!super::is_property_name(name), "{}", name);
        }
        assert!(super::is_custom_property("--brand"));
        assert!(!super::is_vendor_property("--brand"));
    }

    #[test]
    fn css_properties_are_sorted() {
        assert!(super::CSS_PROPERTIES.windows(2).all(|w| w[0] < w[1]));
    }
}
