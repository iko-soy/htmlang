//! The attribute vocabulary.
//!
//! An attribute in `[...]` is one of three things:
//! - one of htmlang's own attributes, which mean more than a single CSS
//!   property (`spacing`, `center-x`, `width fill`, ...), handled in codegen;
//! - a standard CSS property, copied into the generated CSS unchanged;
//! - an HTML attribute, written `key=value` (or bare, for booleans such as
//!   `required` and for `data-` and `hx-` attributes) and emitted on the
//!   element.

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
    "accent-color", "align-content", "align-items", "align-self", "alignment-baseline", "all",
    "anchor-name", "anchor-scope", "animation", "animation-composition", "animation-delay",
    "animation-direction", "animation-duration", "animation-fill-mode",
    "animation-iteration-count", "animation-name", "animation-play-state", "animation-range",
    "animation-range-end", "animation-range-start", "animation-timeline",
    "animation-timing-function", "appearance", "aspect-ratio", "backdrop-filter",
    "backface-visibility", "background", "background-attachment", "background-blend-mode",
    "background-clip", "background-color", "background-image", "background-origin",
    "background-position", "background-position-x", "background-position-y", "background-repeat",
    "background-size", "baseline-shift", "baseline-source", "block-size", "border", "border-block",
    "border-block-color", "border-block-end", "border-block-end-color", "border-block-end-style",
    "border-block-end-width", "border-block-start", "border-block-start-color",
    "border-block-start-style", "border-block-start-width", "border-block-style",
    "border-block-width", "border-bottom", "border-bottom-color", "border-bottom-left-radius",
    "border-bottom-right-radius", "border-bottom-style", "border-bottom-width", "border-collapse",
    "border-color", "border-end-end-radius", "border-end-start-radius", "border-image",
    "border-image-outset", "border-image-repeat", "border-image-slice", "border-image-source",
    "border-image-width", "border-inline", "border-inline-color", "border-inline-end",
    "border-inline-end-color", "border-inline-end-style", "border-inline-end-width",
    "border-inline-start", "border-inline-start-color", "border-inline-start-style",
    "border-inline-start-width", "border-inline-style", "border-inline-width", "border-left",
    "border-left-color", "border-left-style", "border-left-width", "border-radius", "border-right",
    "border-right-color", "border-right-style", "border-right-width", "border-spacing",
    "border-start-end-radius", "border-start-start-radius", "border-style", "border-top",
    "border-top-color", "border-top-left-radius", "border-top-right-radius", "border-top-style",
    "border-top-width", "border-width", "bottom", "box-decoration-break", "box-shadow",
    "box-sizing", "break-after", "break-before", "break-inside", "caption-side", "caret",
    "caret-animation", "caret-color", "caret-shape", "clear", "clip", "clip-path", "clip-rule",
    "color", "color-interpolation", "color-interpolation-filters", "color-scheme", "column-count",
    "column-fill", "column-gap", "column-rule", "column-rule-color", "column-rule-style",
    "column-rule-width", "column-span", "column-width", "columns", "contain",
    "contain-intrinsic-block-size", "contain-intrinsic-height", "contain-intrinsic-inline-size",
    "contain-intrinsic-size", "contain-intrinsic-width", "container", "container-name",
    "container-type", "content", "content-visibility", "corner-shape", "counter-increment",
    "counter-reset", "counter-set", "cursor", "direction", "display", "dominant-baseline",
    "dynamic-range-limit", "empty-cells", "field-sizing", "fill", "fill-opacity", "fill-rule",
    "filter", "flex", "flex-basis", "flex-direction", "flex-flow", "flex-grow", "flex-shrink",
    "flex-wrap", "float", "flood-color", "flood-opacity", "font", "font-family",
    "font-feature-settings", "font-kerning", "font-language-override", "font-optical-sizing",
    "font-palette", "font-size", "font-size-adjust", "font-stretch", "font-style",
    "font-synthesis", "font-synthesis-position", "font-synthesis-small-caps",
    "font-synthesis-style", "font-synthesis-weight", "font-variant", "font-variant-alternates",
    "font-variant-caps", "font-variant-east-asian", "font-variant-emoji", "font-variant-ligatures",
    "font-variant-numeric", "font-variant-position", "font-variation-settings", "font-weight",
    "forced-color-adjust", "gap", "grid", "grid-area", "grid-auto-columns", "grid-auto-flow",
    "grid-auto-rows", "grid-column", "grid-column-end", "grid-column-start", "grid-row",
    "grid-row-end", "grid-row-start", "grid-template", "grid-template-areas",
    "grid-template-columns", "grid-template-rows", "hanging-punctuation", "height",
    "hyphenate-character", "hyphenate-limit-chars", "hyphens", "image-orientation",
    "image-rendering", "initial-letter", "inline-size", "inset", "inset-block", "inset-block-end",
    "inset-block-start", "inset-inline", "inset-inline-end", "inset-inline-start",
    "interpolate-size", "isolation", "justify-content", "justify-items", "justify-self", "left",
    "letter-spacing", "lighting-color", "line-break", "line-clamp", "line-height", "list-style",
    "list-style-image", "list-style-position", "list-style-type", "margin", "margin-block",
    "margin-block-end", "margin-block-start", "margin-bottom", "margin-inline",
    "margin-inline-end", "margin-inline-start", "margin-left", "margin-right", "margin-top",
    "margin-trim", "marker", "marker-end", "marker-mid", "marker-start", "mask", "mask-border",
    "mask-border-mode", "mask-border-outset", "mask-border-repeat", "mask-border-slice",
    "mask-border-source", "mask-border-width", "mask-clip", "mask-composite", "mask-image",
    "mask-mode", "mask-origin", "mask-position", "mask-repeat", "mask-size", "mask-type",
    "math-depth", "math-shift", "math-style", "max-block-size", "max-height", "max-inline-size",
    "max-width", "min-block-size", "min-height", "min-inline-size", "min-width", "mix-blend-mode",
    "object-fit", "object-position", "object-view-box", "offset", "offset-anchor",
    "offset-distance", "offset-path", "offset-position", "offset-rotate", "opacity", "order",
    "orphans", "outline", "outline-color", "outline-offset", "outline-style", "outline-width",
    "overflow", "overflow-anchor", "overflow-block", "overflow-clip-margin", "overflow-inline",
    "overflow-wrap", "overflow-x", "overflow-y", "overlay", "overscroll-behavior",
    "overscroll-behavior-block", "overscroll-behavior-inline", "overscroll-behavior-x",
    "overscroll-behavior-y", "padding", "padding-block", "padding-block-end",
    "padding-block-start", "padding-bottom", "padding-inline", "padding-inline-end",
    "padding-inline-start", "padding-left", "padding-right", "padding-top", "page",
    "page-break-after", "page-break-before", "page-break-inside", "paint-order", "perspective",
    "perspective-origin", "place-content", "place-items", "place-self", "pointer-events",
    "position", "position-anchor", "position-area", "position-try", "position-try-fallbacks",
    "position-try-order", "position-visibility", "print-color-adjust", "quotes", "reading-flow",
    "reading-order", "resize", "right", "rotate", "row-gap", "ruby-align", "ruby-overhang",
    "ruby-position", "scale", "scroll-behavior", "scroll-margin", "scroll-margin-block",
    "scroll-margin-block-end", "scroll-margin-block-start", "scroll-margin-bottom",
    "scroll-margin-inline", "scroll-margin-inline-end", "scroll-margin-inline-start",
    "scroll-margin-left", "scroll-margin-right", "scroll-margin-top", "scroll-marker-group",
    "scroll-padding", "scroll-padding-block", "scroll-padding-block-end",
    "scroll-padding-block-start", "scroll-padding-bottom", "scroll-padding-inline",
    "scroll-padding-inline-end", "scroll-padding-inline-start", "scroll-padding-left",
    "scroll-padding-right", "scroll-padding-top", "scroll-snap-align", "scroll-snap-stop",
    "scroll-snap-type", "scroll-timeline", "scroll-timeline-axis", "scroll-timeline-name",
    "scrollbar-color", "scrollbar-gutter", "scrollbar-width", "shape-image-threshold",
    "shape-margin", "shape-outside", "shape-rendering", "stop-color", "stop-opacity", "stroke",
    "stroke-dasharray", "stroke-dashoffset", "stroke-linecap", "stroke-linejoin",
    "stroke-miterlimit", "stroke-opacity", "stroke-width", "tab-size", "table-layout",
    "text-align", "text-align-last", "text-anchor", "text-autospace", "text-box", "text-box-edge",
    "text-box-trim", "text-combine-upright", "text-decoration", "text-decoration-color",
    "text-decoration-line", "text-decoration-skip", "text-decoration-skip-ink",
    "text-decoration-style", "text-decoration-thickness", "text-emphasis", "text-emphasis-color",
    "text-emphasis-position", "text-emphasis-style", "text-indent", "text-justify",
    "text-orientation", "text-overflow", "text-rendering", "text-shadow", "text-size-adjust",
    "text-spacing-trim", "text-transform", "text-underline-offset", "text-underline-position",
    "text-wrap", "text-wrap-mode", "text-wrap-style", "timeline-scope", "top", "touch-action",
    "transform", "transform-box", "transform-origin", "transform-style", "transition",
    "transition-behavior", "transition-delay", "transition-duration", "transition-property",
    "transition-timing-function", "translate", "unicode-bidi", "user-select", "vector-effect",
    "vertical-align", "view-timeline", "view-timeline-axis", "view-timeline-inset",
    "view-timeline-name", "view-transition-class", "view-transition-name", "visibility",
    "white-space", "white-space-collapse", "widows", "width", "will-change", "word-break",
    "word-spacing", "word-wrap", "writing-mode", "z-index", "zoom",
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

/// The families of HTML attributes whose names are open: ARIA's, the
/// author's own `data-` attributes, and htmx's `hx-` attributes.
pub const HTML_ATTRIBUTE_FAMILIES: &[&str] = &["aria-", "data-", "hx-"];

/// An HTML attribute by its name: a common one, or one of a family whose
/// names are open (`aria-label`, `data-id`, `hx-get`).
pub fn is_html_attribute(name: &str) -> bool {
    HTML_ATTRIBUTES.contains(&name)
        || HTML_ATTRIBUTE_FAMILIES
            .iter()
            .any(|family| in_family(name, family))
}

/// An HTML attribute written bare, as a flag, and rendered without a value:
/// a boolean one (`required`), or a `data-` or `hx-` attribute, whose
/// presence is what it says (`data-open`, htmx's `hx-preserve`). ARIA's
/// attributes always take a value.
pub fn is_html_flag(name: &str) -> bool {
    BOOLEAN_HTML_ATTRS.contains(&name) || in_family(name, "data-") || in_family(name, "hx-")
}

fn in_family(name: &str, family: &str) -> bool {
    name.len() > family.len() && name.starts_with(family)
}

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
    "background-position", "background-position-x", "background-position-y", "background-size",
    "baseline-shift", "block-size", "border", "border-block", "border-block-end",
    "border-block-end-width", "border-block-start", "border-block-start-width",
    "border-block-width", "border-bottom",
    "border-bottom-left-radius", "border-bottom-right-radius", "border-bottom-width",
    "border-end-end-radius", "border-end-start-radius", "border-inline", "border-inline-end",
    "border-inline-end-width", "border-inline-start", "border-inline-start-width",
    "border-inline-width", "border-left", "border-left-width", "border-radius", "border-right",
    "border-right-width", "border-spacing", "border-start-end-radius",
    "border-start-start-radius", "border-top", "border-top-left-radius",
    "border-top-right-radius", "border-top-width", "border-width", "bottom", "box-shadow",
    "column-gap", "column-rule", "column-rule-width", "column-width",
    "contain-intrinsic-block-size", "contain-intrinsic-height", "contain-intrinsic-inline-size",
    "contain-intrinsic-size", "contain-intrinsic-width", "flex-basis", "font-size", "gap",
    "grid-auto-columns", "grid-auto-rows", "grid-template-columns", "grid-template-rows",
    "height", "inline-size", "inset", "inset-block", "inset-block-end", "inset-block-start",
    "inset-inline", "inset-inline-end", "inset-inline-start", "left", "letter-spacing",
    "margin", "margin-block", "margin-block-end", "margin-block-start", "margin-bottom",
    "margin-inline", "margin-inline-end", "margin-inline-start", "margin-left", "margin-right",
    "margin-top", "mask-position", "mask-size", "max-block-size", "max-height",
    "max-inline-size", "max-width", "min-block-size", "min-height", "min-inline-size",
    "min-width", "object-position", "offset-anchor", "offset-distance", "offset-position",
    "outline", "outline-offset", "outline-width", "overflow-clip-margin", "padding",
    "padding-block", "padding-block-end", "padding-block-start", "padding-bottom",
    "padding-inline", "padding-inline-end", "padding-inline-start", "padding-left",
    "padding-right", "padding-top", "perspective", "perspective-origin", "right", "row-gap",
    "scroll-margin", "scroll-margin-block", "scroll-margin-block-end",
    "scroll-margin-block-start", "scroll-margin-bottom", "scroll-margin-inline",
    "scroll-margin-inline-end", "scroll-margin-inline-start", "scroll-margin-left",
    "scroll-margin-right", "scroll-margin-top", "scroll-padding", "scroll-padding-block",
    "scroll-padding-block-end", "scroll-padding-block-start", "scroll-padding-bottom",
    "scroll-padding-inline", "scroll-padding-inline-end", "scroll-padding-inline-start",
    "scroll-padding-left", "scroll-padding-right", "scroll-padding-top", "shape-margin",
    "text-decoration-thickness", "text-indent", "text-shadow", "text-underline-offset", "top",
    "transform-origin", "translate", "vertical-align", "view-timeline-inset", "width",
    "word-spacing",
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
        // word, which is then not a bare number. `!important` starts a
        // word of its own, also right after a number (`10!important`)
        let outside = depth == 0 && quote.is_none();
        if outside && (c.is_whitespace() || c == ',' || c == '/') {
            end_word(&mut out, &mut word, i);
        } else if outside && c == '!' {
            end_word(&mut out, &mut word, i);
            word = Some(i);
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

/// How CSS writes a pseudo-class or pseudo-element that a selector prefix
/// names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pseudo {
    /// A pseudo-class, `:hover`: the prefix `hover:`.
    Class,
    /// A pseudo-class that takes an argument, `:nth-child(odd)`: the
    /// prefix `nth-child(odd):`.
    Function,
    /// A pseudo-element, `::marker`: the prefix `marker:`.
    Element,
}

/// The pseudo-classes and pseudo-elements a selector prefix can name, under
/// their CSS names (`first-child:` is `:first-child`, `marker:` is
/// `::marker`), and whether each is written `:` or `::` in CSS. A name not
/// here is an unknown prefix, as a property not in [`CSS_PROPERTIES`] is an
/// unknown property. The order is the order the rules are written in, so a
/// later one wins where both hold: position, then form state, then the
/// states a pointer or the keyboard sets (`hover:`, `focus:`, `active:`),
/// then `disabled:`, as in CSS's usual `:link` … `:hover` … `:active` order.
pub const PSEUDOS: &[(&str, Pseudo)] = &[
    ("link", Pseudo::Class),
    ("visited", Pseudo::Class),
    ("first-child", Pseudo::Class),
    ("last-child", Pseudo::Class),
    ("only-child", Pseudo::Class),
    ("first-of-type", Pseudo::Class),
    ("last-of-type", Pseudo::Class),
    ("only-of-type", Pseudo::Class),
    ("nth-child", Pseudo::Function),
    ("nth-last-child", Pseudo::Function),
    ("nth-of-type", Pseudo::Function),
    ("nth-last-of-type", Pseudo::Function),
    ("empty", Pseudo::Class),
    ("target", Pseudo::Class),
    ("open", Pseudo::Class),
    ("popover-open", Pseudo::Class),
    ("default", Pseudo::Class),
    ("checked", Pseudo::Class),
    ("indeterminate", Pseudo::Class),
    ("placeholder-shown", Pseudo::Class),
    ("autofill", Pseudo::Class),
    ("required", Pseudo::Class),
    ("optional", Pseudo::Class),
    ("valid", Pseudo::Class),
    ("invalid", Pseudo::Class),
    ("user-valid", Pseudo::Class),
    ("user-invalid", Pseudo::Class),
    ("read-only", Pseudo::Class),
    ("read-write", Pseudo::Class),
    ("not", Pseudo::Function),
    ("is", Pseudo::Function),
    ("where", Pseudo::Function),
    ("has", Pseudo::Function),
    ("focus-within", Pseudo::Class),
    ("hover", Pseudo::Class),
    ("focus", Pseudo::Class),
    ("focus-visible", Pseudo::Class),
    ("active", Pseudo::Class),
    ("enabled", Pseudo::Class),
    ("disabled", Pseudo::Class),
    ("before", Pseudo::Element),
    ("after", Pseudo::Element),
    ("marker", Pseudo::Element),
    ("placeholder", Pseudo::Element),
    ("selection", Pseudo::Element),
    ("backdrop", Pseudo::Element),
    ("first-line", Pseudo::Element),
    ("first-letter", Pseudo::Element),
    ("file-selector-button", Pseudo::Element),
];

/// htmlang's one combinator word: `children:flex-shrink 0` styles each
/// direct child, `:where(.x)>*`.
pub const CHILDREN: &str = "children:";

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

/// The place of an at-rule prefix (a width, media or container prefix,
/// whose styles go in an `@media` or `@container` block) in the order the
/// blocks are written: the widths, then `dark:` … `portrait:`, then the
/// container widths. `None` for a selector prefix.
pub fn at_rule_rank(prefix: &str) -> Option<usize> {
    at_rule_prefixes().position(|p| p == prefix)
}

/// The at-rule prefixes, in the order their blocks are written.
pub fn at_rule_prefixes() -> impl Iterator<Item = &'static str> {
    RESPONSIVE_PREFIXES
        .iter()
        .chain(MEDIA_PREFIXES)
        .chain(CONTAINER_QUERY_PREFIXES)
        .copied()
}

/// Whether the at-rule prefix of rank `a` holds wherever the one of rank
/// `b` does: itself, or a smaller width of the same kind (`sm:` wherever
/// `md:` holds).
pub fn at_rule_implied(a: usize, b: usize) -> bool {
    let widths = RESPONSIVE_PREFIXES.len();
    let media = widths + MEDIA_PREFIXES.len();
    a == b || (a < b && ((b < widths) || (a >= media && b >= media)))
}

/// The pseudo-class or pseudo-element `name` names, and how CSS writes it.
pub fn pseudo(name: &str) -> Option<Pseudo> {
    PSEUDOS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, kind)| kind)
}

/// The CSS name of a selector prefix, without its argument:
/// `nth-child(odd):` → `nth-child`, `hover:` → `hover`.
pub fn pseudo_name(prefix: &str) -> &str {
    let name = prefix.strip_suffix(':').unwrap_or(prefix);
    name.split_once('(').map_or(name, |(name, _)| name)
}

/// What a selector prefix adds to the selector: `hover:` → `:hover`,
/// `marker:` → `::marker`, `nth-child(2n+1):` → `:nth-child(2n+1)`. `None`
/// for `children:` and the at-rule prefixes.
pub fn pseudo_selector(prefix: &str) -> Option<String> {
    let written = prefix.strip_suffix(':')?;
    match pseudo(pseudo_name(prefix))? {
        Pseudo::Element => Some(format!("::{}", written)),
        Pseudo::Class | Pseudo::Function => Some(format!(":{}", written)),
    }
}

/// A selector prefix that selects a pseudo-element (`before:`, `marker:`,
/// `backdrop:`, ...), which comes last among the selector prefixes:
/// `hover:before:` is the `::before` of a hovered element, and CSS has no
/// `::before:hover`.
pub fn is_pseudo_element(prefix: &str) -> bool {
    prefix
        .strip_suffix(':')
        .is_some_and(|name| pseudo(name) == Some(Pseudo::Element))
}

/// The element an element prefix names: `@td:` → `td`. `None` for every
/// other prefix.
pub fn element_prefix(prefix: &str) -> Option<&str> {
    prefix.strip_prefix('@')?.strip_suffix(':')
}

/// htmlang's words that place an element in its parent, so they mean
/// something only against the parent: `width`/`height` `fill` or
/// `shrink`, `center-x`, `center-y`, `align-*`.
pub fn places_in_parent(name: &str, value: Option<&str>) -> bool {
    match name {
        "width" | "height" => value.is_some_and(|v| matches!(v.trim(), "fill" | "shrink")),
        _ => name.starts_with("center-") || name.starts_with("align-"),
    }
}

/// One of htmlang's own words rather than a CSS property: the layout words
/// (`spacing`, `wrap`, `grid-cols`, `col-span`, ...), `inline`, and the
/// words that place an element in its parent (`width fill`, `center-x`).
pub fn is_htmlang_word(name: &str, value: Option<&str>) -> bool {
    HTMLANG_ATTRIBUTES.contains(&name) || places_in_parent(name, value)
}

/// Does `key` carry any state, media, responsive or container prefix?
pub fn is_prefixed(key: &str) -> bool {
    prefix_len(key).is_some()
}

/// The length of the prefix `key` starts with (its `:` included), when it
/// starts with a known one: `hover:`, `md:`, `children:`, a pseudo-class
/// with its argument in balanced parentheses, which may hold spaces and
/// colons of its own (`nth-child(2n+1):`, `has(> img):`), or an element
/// prefix, `@td:` (any name: the parser checks it names an element).
pub fn prefix_len(key: &str) -> Option<usize> {
    if let Some(rest) = key.strip_prefix('@') {
        let name_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .filter(|&len| len > 0)?;
        let name_starts_well = rest.starts_with(|c: char| c.is_ascii_alphabetic());
        return (name_starts_well && rest[name_len..].starts_with(':')).then_some(name_len + 2);
    }
    let name_len = key
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|&len| len > 0)?;
    let name = &key[..name_len];
    match key[name_len..].chars().next()? {
        ':' => {
            let prefix = &key[..=name_len];
            let known = prefix == CHILDREN
                || at_rule_rank(prefix).is_some()
                || matches!(pseudo(name), Some(Pseudo::Class | Pseudo::Element));
            known.then_some(name_len + 1)
        }
        '(' if pseudo(name) == Some(Pseudo::Function) => {
            let close = closing_paren(key, name_len)?;
            key[close + 1..].starts_with(':').then_some(close + 2)
        }
        _ => None,
    }
}

/// The `)` that closes the `(` at `open` in `text`. Parentheses inside
/// quotes, and a character after a backslash, don't count.
pub fn closing_paren(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut chars = text.char_indices().skip_while(|&(i, _)| i < open);
    while let Some((i, c)) = chars.next() {
        match (quote, c) {
            (_, '\\') => {
                chars.next();
            }
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
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

/// Every prefix written without an argument (`hover:`, `marker:`, `md:`,
/// `children:`), for "did you mean" suggestions.
pub fn all_prefixes() -> Vec<String> {
    PSEUDOS
        .iter()
        .filter(|(_, kind)| *kind != Pseudo::Function)
        .map(|(name, _)| format!("{}:", name))
        .chain(at_rule_prefixes().map(str::to_string))
        .chain([CHILDREN.to_string()])
        .collect()
}

/// The pseudo-classes that take an argument (`nth-child`, `has`), for "did
/// you mean" suggestions.
pub fn functional_pseudos() -> Vec<&'static str> {
    PSEUDOS
        .iter()
        .filter(|(_, kind)| *kind == Pseudo::Function)
        .map(|&(name, _)| name)
        .collect()
}

/// The attribute name without its prefixes: `hover:md:background` →
/// `background`, `nth-child(2n):color` → `color`, `has(.x):padding` → `padding`.
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
        assert_eq!(with_px("margin", "10!important"), "10px!important");
        assert_eq!(with_px("margin", "0 4 ! important"), "0 4px ! important");
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
        assert_eq!(super::base_attribute("nth-child(2n):color"), "color");
        assert_eq!(super::base_attribute("has(> img):padding"), "padding");
        assert_eq!(super::base_attribute("nth:2n:color"), "nth:2n:color");
        assert_eq!(super::base_attribute("first:color"), "first:color");
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
            super::split_prefixes("nth-child(2n+1):hover:padding"),
            (vec!["nth-child(2n+1):", "hover:"], "padding")
        );
        assert_eq!(
            super::split_prefixes("is(:hover, :focus):not(.a):marker:color"),
            (vec!["is(:hover, :focus):", "not(.a):", "marker:"], "color")
        );
        assert_eq!(
            super::split_prefixes("has([title=\"a)\"]):color"),
            (vec!["has([title=\"a)\"]):"], "color")
        );
        assert_eq!(
            super::split_prefixes("hover(x):first-child(2):color"),
            (vec![], "hover(x):first-child(2):color")
        );
        assert_eq!(
            super::split_prefixes("has(a:hover)"),
            (vec![], "has(a:hover)")
        );
    }

    #[test]
    fn an_element_prefix_is_an_at_sign_a_name_and_a_colon() {
        use super::{element_prefix, split_prefixes};
        assert_eq!(split_prefixes("@td:padding"), (vec!["@td:"], "padding"));
        assert_eq!(
            split_prefixes("md:@link:hover:color"),
            (vec!["md:", "@link:", "hover:"], "color")
        );
        assert_eq!(
            split_prefixes("@in-front:@h2:color"),
            (vec!["@in-front:", "@h2:"], "color")
        );
        // Not a name: no prefix
        assert_eq!(split_prefixes("@:color"), (vec![], "@:color"));
        assert_eq!(split_prefixes("@2x:color"), (vec![], "@2x:color"));
        assert_eq!(split_prefixes("@td padding"), (vec![], "@td padding"));
        assert_eq!(element_prefix("@td:"), Some("td"));
        assert_eq!(element_prefix("hover:"), None);
        assert!(super::is_htmlang_word("spacing", Some("8")));
        assert!(super::is_htmlang_word("width", Some("fill")));
        assert!(!super::is_htmlang_word("width", Some("200")));
        assert!(!super::is_htmlang_word("padding", Some("8")));
    }

    #[test]
    fn at_rule_prefixes_are_ranked_in_block_order() {
        use super::{at_rule_implied, at_rule_rank};
        let rank = |p| at_rule_rank(p).unwrap();
        assert!(rank("sm:") < rank("2xl:") && rank("2xl:") < rank("dark:"));
        assert!(rank("portrait:") < rank("cq-sm:"));
        assert_eq!(at_rule_rank("hover:"), None);
        assert!(at_rule_implied(rank("sm:"), rank("md:")));
        assert!(!at_rule_implied(rank("md:"), rank("sm:")));
        assert!(at_rule_implied(rank("cq-sm:"), rank("cq-lg:")));
        assert!(!at_rule_implied(rank("lg:"), rank("cq-xl:")));
        assert!(!at_rule_implied(rank("dark:"), rank("print:")));
        assert!(at_rule_implied(rank("dark:"), rank("dark:")));
        assert!(super::is_pseudo_element("before:") && !super::is_pseudo_element("hover:"));
        assert!(super::is_pseudo_element("marker:") && !super::is_pseudo_element("has(a):"));
    }

    #[test]
    fn pseudo_prefixes_are_written_as_css_writes_them() {
        use super::pseudo_selector as selector;
        assert_eq!(selector("hover:").as_deref(), Some(":hover"));
        assert_eq!(selector("marker:").as_deref(), Some("::marker"));
        assert_eq!(selector("backdrop:").as_deref(), Some("::backdrop"));
        assert_eq!(
            selector("nth-child(odd):").as_deref(),
            Some(":nth-child(odd)")
        );
        assert_eq!(selector("has(> img):").as_deref(), Some(":has(> img)"));
        assert_eq!(selector("children:"), None);
        assert_eq!(selector("md:"), None);
        // Every name is written once, and none is also an at-rule prefix
        for (i, (name, _)) in super::PSEUDOS.iter().enumerate() {
            assert!(
                !super::PSEUDOS[..i].iter().any(|(n, _)| n == name),
                "{name}"
            );
            assert_eq!(super::at_rule_rank(&format!("{name}:")), None, "{name}");
        }
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
