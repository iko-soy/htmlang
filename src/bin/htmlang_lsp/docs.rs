//! Documentation for htmlang's names, shared by hover and completion.
//!
//! The *names* themselves come from the compiler (`ElementKind`, `TAGS`,
//! `vocab`, the parser's directive list); this file only adds prose. A name
//! without an entry here still gets a summary derived from the compiler's
//! tables, so new elements and attributes are never missing from the editor.

use htmlang::ast::{ElementKind, Layout, TagArg};
use htmlang::vocab;

pub(crate) struct Doc {
    pub name: &'static str,
    pub summary: &'static str,
    /// An example; for attributes, `name` alone means it takes no value.
    pub usage: &'static str,
}

const fn doc(name: &'static str, summary: &'static str, usage: &'static str) -> Doc {
    Doc {
        name,
        summary,
        usage,
    }
}

/// Elements (names without `@`). Elements not listed get a summary derived
/// from their row in `TAGS`.
#[rustfmt::skip]
pub(crate) const ELEMENTS: &[Doc] = &[
    doc("row", "Horizontal layout: a flex row.", "@row [spacing 10]\n  @text A\n  @text B"),
    doc("el", "The container: a flex column. `flex-direction row` lays its children out side by side, and their `fill` and `shrink` follow.", "@el [padding 20, background white]\n  Content"),
    doc("text", "Styled inline text (`<span>`).", "@text [font-weight bold, font-size 24] Hello"),
    doc(
        "paragraph",
        "A paragraph of flowing text (`<p>`), which holds inline `{@...}` elements like any text.",
        "@paragraph\n  Read the {@link /docs docs}.",
    ),
    doc("link", "Link (`<a>`): the first word after its attributes is its `href`, and the rest is its content. It has no underline and takes its parent's text colour until you style it.", "@link /about About us"),
    doc("image", "Image (`<img>`, a block): the one word after its attributes is its `src` (quote one with a space in it).", "@image [alt=Logo, width 120] logo.png"),
    doc(
        "script",
        "Script (`<script>`): the word after its attributes is its `src`; or its JavaScript as \
         an indented body, kept verbatim (not both: the browser doesn't run the body of a script \
         with a src). HTML attributes such as `defer` and `type=module` pass through. It isn't \
         shown, so it takes no styles.",
        "@script [defer] app.js",
    ),
    doc(
        "fragment",
        "Groups children without a wrapper element, so it takes no attributes.",
        "@fragment\n  @text A\n  @text B",
    ),
    doc(
        "children",
        "In a function's body: where a call's content goes (its text and the lines under \
         it, except `@slot` blocks). Lines indented under `@children` are the fallback, \
         shown when a call passes no content. Content passed to a function without \
         `@children` is an error.",
        "@let @card\n  @el [padding 20]\n    @children\n      @text Nothing yet",
    ),
    doc(
        "slot",
        "`@slot NAME` in a function's body marks a named place; lines indented under it \
         are the fallback. Directly under a call, a `@slot NAME` block fills that place. \
         A name is one word, and a block for a slot the function doesn't have is an error.",
        "@let @card\n  @el\n    @children\n    @slot footer\n      @text No footer\n\n@card\n  Body\n  @slot footer\n    @text Updated today",
    ),
    doc("grid", "Grid container (`display: grid`).", "@grid [grid-cols 3, spacing 20]"),
    doc(
        "in-front",
        "Overlay layer filling the parent, painted on top of its content.",
        "@el [width 200, height 200]\n  @in-front\n    @text Overlay",
    ),
    doc(
        "behind",
        "Layer filling the parent, painted behind its content.",
        "@el [padding 40]\n  @behind\n    @el [background #fef3c7, width fill, height fill]",
    ),
    doc("ul", "List (`<ul>`), shown without markers.", "@ul [spacing 4]\n  @li First\n  @li Second"),
    doc(
        "ol",
        "Numbered list (`<ol>`), shown without numbers; to show them, add `list-style decimal, padding-inline-start 20, children:display list-item`.",
        "@ol [spacing 4]\n  @li First",
    ),
    doc("li", "List item.", "@li First"),
    doc("form", "Form: the first word after its attributes is its `action`, and the rest is its content.", "@form [method=post] /subscribe"),
    doc("input", "Form input (void element).", "@input [type=email, name=email, required]"),
    doc("button", "Button.", "@button [type=submit] Send"),
    doc("iframe", "Embedded page: the first word after its attributes is its `src`.", "@iframe [sandbox, title=Example] https://example.com"),
    doc("video", "Video: the first word after its attributes is its `src`, and the rest is its content (the fallback, `@source` and `@track`).", "@video [controls] movie.mp4"),
    doc("audio", "Audio: the first word after its attributes is its `src`, and the rest is its content.", "@audio [controls] song.mp3"),
    doc("strong", "Important text (`<strong>`), bold in the browser's own style.", "@paragraph\n  {@strong Note:} save your work first."),
    doc("em", "Stressed text (`<em>`), italic in the browser's own style.", "@paragraph\n  I {@em did} say so."),
    doc("b", "Text set apart in bold without extra importance (`<b>`): a name, a keyword.", "@paragraph\n  Built with {@b htmlang}."),
    doc("i", "Text in another voice (`<i>`), italic: a term, a title, a foreign phrase.", "@paragraph\n  The {@i Titanic} sank in 1912."),
    doc("small", "Fine print and side comments (`<small>`).", "@small Prices include tax."),
    doc("br", "Line break (`<br>`), inside text.", "@paragraph\n  First line{@br}\n  second line"),
    doc("sub", "Subscript (`<sub>`).", "@paragraph\n  H{@sub 2}O"),
    doc("sup", "Superscript (`<sup>`).", "@paragraph\n  E = mc{@sup 2}"),
    doc("hgroup", "A heading and its subtitle (`<hgroup>`).", "@hgroup\n  @h1 htmlang\n  @paragraph A layout language"),
    doc("menu", "List of commands (`<menu>`), shown without markers like `@ul`.", "@menu [spacing 4]\n  @li > @button Copy"),
    doc("caption", "Table title (`<caption>`), the first child of `@table`.", "@table\n  @caption Team\n  @tr\n    @td Ada"),
    doc("tfoot", "Table footer rows (`<tfoot>`).", "@tfoot\n  @tr\n    @td Total"),
    doc("optgroup", "Group of options in a `@select`: the first word after its attributes is its `label` (quote one with a space in it); its options go on the lines under it.", "@select [aria-label=Fruit]\n  @optgroup \"Citrus fruits\"\n    @option Lemon"),
    doc("track", "Captions or subtitles for `@video`: the one word after its attributes is its `src`.", "@track [kind=captions, srclang=en] captions.vtt"),
    doc("source", "A media source: the one word after its attributes is its `srcset` inside `@picture`, and its `src` inside `@video` or `@audio`.", "@picture\n  @source [type=image/avif] photo.avif\n  @image [alt=Photo] photo.jpg"),
];

/// Directives (names without `@`).
#[rustfmt::skip]
pub(crate) const DIRECTIVES: &[Doc] = &[
    doc(
        "page",
        "Produces a full HTML document with this title. The page is the root element: its styles style `<body>`, a column that fills the window (so top-level elements stack, and `height fill` takes the rest of it); its `key=value` attributes go on `<html>` (`lang=en`, `dir=rtl`, `class=x`); `favicon FILE` puts the file in as the page's icon. A page has one `@page`.",
        "@page [lang=en, favicon icon.png, background #f8fafc, dark:background #0b1220] My Site",
    ),
    doc(
        "let",
        "Defines a value, quoted text (`\"...\"`, which keeps its quotes only in CSS), a computed value (`= expr`) or an attribute bundle (`[...]`), used as `$name`; or, with `@` before the name, a function with parameters `[param, param default]` (a name alone is required) and an indented body. A value is text, a list (commas make one: `a, b, c`, printed as written), a range (`1..5`), or, when it is one `$name` or `${...}`, whatever that holds (a record, a list). A function is called like an element, on its own line, in a chain or inline in text (`{@name ...}`): `@name [param value]`, or a parameter's name alone for `true`; its other attributes style its root element. They share one namespace. `@let --name VALUE` declares a CSS custom property on `:root`, for the whole page, with its value as written (no px); `[--name VALUE]` sets one on an element and everything inside it. A definition is visible from its line to the end of its block, and a function's body sees its parameters and what is visible where it is defined. A function can't take the name of a built-in element or directive.",
        "@let primary #3b82f6\n@let arrow \"→ \"\n@let gap = 8 * 2\n@let --radius 8px\n@let card [padding 20]\n@let @cta [label]\n  @el [$card] $label",
    ),
    doc("include", "Inserts another `.hl` file here, with its definitions.", "@include header.hl"),
    doc("raw", "Pastes HTML into the output verbatim: the rest of the line, or an indented block (not both). It takes no attributes.", "@raw <hr class=\"fancy\">\n@raw\n  <div class=\"widget\"></div>"),
    doc("markdown", "Markdown, converted to HTML: a file named on its line, or an indented block kept verbatim (not both). It takes no attributes.", "@markdown notes.md\n@markdown\n  # Title"),
    doc("style", "Raw CSS, which overrides generated styles: the rest of the line, or an indented block kept verbatim (not both). It takes no attributes.", "@style .note { color: gray; }\n@style\n  @keyframes fade { from { opacity: 0; } }"),
    doc("head", "Raw HTML added to `<head>`: the rest of the line, or an indented block kept verbatim (not both). It takes no attributes.", "@head <link rel=\"icon\" href=\"f.ico\">"),
    doc("meta", "A `<meta>` tag; `og:` names become Open Graph tags.", "@meta description A small site"),
    doc("if", "Renders its body when the condition holds; `@else if` / `@else` follow.", "@if $count > 2 and not $hidden\n  @text Many"),
    doc("else", "Fallback branch of `@if` or `@each`.", "@else\n  @text None"),
    doc(
        "each",
        "Repeats its body for each item of a list: items written with commas, a range `A..B` (with `step N`), or one `$name` or `${...}` that holds a list. Each item is bound whole, so a record keeps its fields (`$item.key`); an optional second variable is the index, from 0. Each repetition is a block of its own.",
        "@each $item, $i in apple, banana\n  @text $i: $item\n@each $n in 10..0 step 5\n  @text $n\n@each $post in $posts\n  @text $post.title",
    ),
    doc("data", "Loads data into a variable: a JSON file or inline JSON (objects are records, arrays lists), a glob of files (a list of records), or `env:NAME` (text).", "@data $site site.json\n@data $links [{\"label\": \"Home\", \"url\": \"/\"}]"),
];

/// htmlang's own attributes, plus CSS properties whose hover says more
/// than CSS's name does (every one of them compiles as CSS says).
#[rustfmt::skip]
pub(crate) const ATTRIBUTES: &[Doc] = &[
    doc("spacing", "Gap between children, on a row, column or grid.", "spacing 20"),
    doc("padding", "Inner space: 1 to 4 values; bare numbers are px.", "padding 12 24"),
    doc("margin", "Outer space: 1 to 4 values; bare numbers are px.", "margin 0 auto"),
    doc("width", "`fill` takes the remaining width in a row and the full width in a column (or anywhere else); `shrink` keeps the content's width in a row and fits the content elsewhere; or a size. Follows the parent's `flex-direction`, also under a prefix such as `md:`.", "width fill"),
    doc("height", "`fill` takes the remaining height in a column and the full height in a row (or anywhere else); `shrink` keeps the content's height in a column and fits the content elsewhere; or a size. Follows the parent's `flex-direction`, also under a prefix such as `md:`.", "height 200"),
    doc("flex-direction", "CSS's direction of a row or column. The children's `width`/`height` `fill` and `shrink` follow it, also when it is set under a media or container prefix.", "@header [spacing 16, md:flex-direction row]\n  @text Logo\n  @el [width fill]"),
    doc("center-x", "Centers the element horizontally in its parent (auto margins, in a row or a column).", "center-x"),
    doc("center-y", "Centers the element vertically in its parent (auto margins, in a row or a column).", "center-y"),
    doc("align-left", "Aligns the element to the left of its parent (`margin-right: auto`).", "align-left"),
    doc("align-right", "Aligns the element to the right of its parent (`margin-left: auto`).", "align-right"),
    doc("align-top", "Aligns the element to the top of its parent (`margin-bottom: auto`).", "align-top"),
    doc("align-bottom", "Aligns the element to the bottom of its parent (`margin-top: auto`).", "align-bottom"),
    doc("wrap", "Lets a row's children wrap onto new lines.", "wrap"),
    doc("grid-cols", "Number of equal grid columns, or a track list as in `grid-template-columns` (bare numbers are px).", "grid-cols 3\ngrid-cols 200 1fr"),
    doc("grid-rows", "Number of equal grid rows, or a track list as in `grid-template-rows` (bare numbers are px).", "grid-rows 2"),
    doc("col-span", "Columns a grid child spans.", "col-span 2"),
    doc("row-span", "Rows a grid child spans.", "row-span 2"),
    doc("line-height", "Line height; a bare number is a multiplier of the font size.", "line-height 1.5"),
    doc("line-clamp", "Cuts text off after N lines, with the `-webkit-box` declarations browsers still need.", "line-clamp 3"),
    doc("outline", "CSS's outline shorthand, as CSS reads it: a width, a style and a colour. Without a style (`solid`) CSS draws none.", "focus:outline 2 solid var(--brand)"),
    doc("inline", "On `@image`: embed the file (SVG markup, or other images as base64) in the page.", "inline"),
];

/// Standard-library components (`std.hl`), used like elements.
#[rustfmt::skip]
pub(crate) const COMPONENTS: &[Doc] = &[
    doc("spacer", "Takes up the remaining space in a row or column.", "@spacer"),
];

/// Standard-library attribute bundles, used as `[$name]`.
#[rustfmt::skip]
pub(crate) const BUNDLES: &[Doc] = &[
    doc("truncate", "Cuts text off at one line with an ellipsis.", "@text [$truncate] A long title"),
];

fn find(table: &'static [Doc], name: &str) -> Option<&'static Doc> {
    table.iter().find(|d| d.name == name)
}

pub(crate) fn attribute(name: &str) -> Option<&'static Doc> {
    find(ATTRIBUTES, name)
}

pub(crate) fn directive(name: &str) -> Option<&'static Doc> {
    find(DIRECTIVES, name)
}

pub(crate) fn component(name: &str) -> Option<&'static Doc> {
    find(COMPONENTS, name)
}

pub(crate) fn bundle(name: &str) -> Option<&'static Doc> {
    find(BUNDLES, name)
}

/// What an element's layout means for what is written inside it, for
/// hover: the rule that decides text lines, `spacing` and children.
pub(crate) fn layout_summary(layout: Layout) -> &'static str {
    match layout {
        Layout::Column => {
            "Layout: column. Each line of text is a child of its own, and `spacing` is the gap \
             between the children. `flex-direction row` (also as `md:flex-direction row`) lays \
             them out side by side."
        }
        Layout::Row => {
            "Layout: row. Each line of text is a child of its own, and `spacing` is the gap \
             between the children."
        }
        Layout::Grid => "Layout: grid. Each line of text is a cell, and `spacing` is the gap.",
        Layout::Text => {
            "Layout: text. Its lines and children flow together, joined with spaces, so it \
             takes no `spacing`; an `@el`, `@row` or `@grid` inside it is laid out inline."
        }
        Layout::Native => "Layout: HTML's own, which htmlang leaves alone; it takes no `spacing`.",
        Layout::Void => "It takes no content.",
    }
}

/// One-line summary of an element: from `ELEMENTS`, or derived from its
/// row in `TAGS`, followed by what its layout means.
pub(crate) fn element_summary(name: &str) -> Option<String> {
    let kind = ElementKind::from_name(name)?;
    let mut summary = if let Some(doc) = find(ELEMENTS, name) {
        doc.summary.to_string()
    } else {
        let spec = kind.spec()?;
        let mut summary = format!("Renders `<{}>`.", spec.html);
        let rest = if kind.layout() == Layout::Void {
            ""
        } else {
            ", and the rest is its content"
        };
        match spec.arg {
            TagArg::Attr(attr) => summary.push_str(&format!(
                " The first word after its attributes is its `{}`{}.",
                attr, rest
            )),
            TagArg::Source => summary.push_str(
                " The word after its attributes is its `srcset` inside `@picture`, its `src` \
                 elsewhere.",
            ),
            TagArg::Child if spec.literal => summary.push_str(
                " Text after it is its content, shown as written: a `{@...}` in it is text. \
                 Or its content is the indented block under it, verbatim: shown HTML-escaped \
                 with its lines and indentation, and nothing in it is htmlang.",
            ),
            TagArg::Child => summary.push_str(" Text after it is its content."),
        }
        summary
    };
    // @fragment, @children, @slot and @script have no layout of their own
    let placeholder = matches!(
        kind,
        ElementKind::Fragment | ElementKind::Children | ElementKind::Slot(_)
    ) || kind.is_verbatim();
    if !placeholder {
        summary.push(' ');
        summary.push_str(layout_summary(kind.layout()));
    }
    Some(summary)
}

fn markdown(title: &str, summary: &str, usage: &str) -> String {
    if usage.is_empty() {
        format!("**{}** \u{2014} {}", title, summary)
    } else {
        format!(
            "**{}** \u{2014} {}\n\n```htmlang\n{}\n```",
            title, summary, usage
        )
    }
}

/// What a whole-attribute `if()` does, for completion and hover.
pub(crate) const IF_SUMMARY: &str = "Attributes chosen by a condition";

/// Hover documentation for a whole-attribute `if(CONDITION, A, B)`.
pub(crate) fn if_attribute() -> String {
    markdown(
        "if()",
        "Attributes chosen by a condition: `A` when it holds, `B` otherwise. \
         Each branch is an attribute, a `[group]` of attributes or a `$bundle`, \
         and an empty one leaves the attribute out. For a value, write an \
         expression: `padding ${if($on, 24, 0)}`.",
        "@link [if($active, [font-weight 600, aria-current=page])] /docs Docs",
    )
}

/// Hover text for a name: `@element`, `@directive`, `@component`, an
/// attribute (possibly prefixed, like `hover:color`), or a prefix itself.
pub(crate) fn hover(word: &str) -> Option<String> {
    if let Some(name) = word.strip_prefix('@') {
        if let Some(doc) = component(name) {
            return Some(markdown(
                word,
                &format!("{} (standard library)", doc.summary),
                doc.usage,
            ));
        }
        if let Some(doc) = directive(name) {
            return Some(markdown(word, doc.summary, doc.usage));
        }
        if htmlang::ast::directive(name).is_some() {
            return Some(markdown(word, "Directive.", ""));
        }
        let summary = element_summary(name)?;
        let usage = find(ELEMENTS, name).map_or("", |d| d.usage);
        return Some(markdown(word, &summary, usage));
    }
    if let Some(name) = word.strip_prefix('$') {
        let doc = bundle(name)?;
        return Some(markdown(
            word,
            &format!("{} (standard-library bundle)", doc.summary),
            doc.usage,
        ));
    }
    if let Some(selector) = prefix_selector(word) {
        return Some(markdown(word, &selector, ""));
    }
    let base = vocab::base_attribute(word);
    let prefix = &word[..word.len() - base.len()];
    let text = attribute_hover(base)?;
    Some(if prefix.is_empty() {
        text
    } else {
        format!(
            "{}\n\n*With `{}`:* {}",
            text,
            prefix,
            prefix_selector(prefix).unwrap_or_default()
        )
    })
}

fn attribute_hover(name: &str) -> Option<String> {
    if let Some(doc) = attribute(name) {
        return Some(markdown(name, doc.summary, doc.usage));
    }
    if vocab::PAGE_WORDS.contains(&name) {
        return Some(markdown(
            name,
            "On `@page`: the page's icon. The file is put into the page (as a `data:` URI) when it can be read.",
            "@page [favicon favicon.png] My Site",
        ));
    }
    if vocab::HTMLANG_ATTRIBUTES.contains(&name) {
        return Some(markdown(name, "htmlang attribute.", ""));
    }
    if vocab::is_css_property(name) {
        let value = if vocab::is_length_property(name) {
            "a length: every bare number outside parentheses and quotes is px (`8` is `8px`)"
        } else {
            "passed through unchanged (its numbers stay numbers)"
        };
        return Some(format!(
            "**{name}** \u{2014} CSS property `{name}`, {value}.\n\n[MDN](https://developer.mozilla.org/docs/Web/CSS/{name})"
        ));
    }
    if vocab::is_custom_property(name) {
        return Some(format!(
            "**{name}** \u{2014} Custom property: sets `{name}` on this element and everything inside it, read with `var({name})`. Its value is written to the CSS as it is (no px), so a length takes its unit: `{name} 8px`. `@let {name} VALUE` declares it for the whole page, on `:root`."
        ));
    }
    if vocab::is_vendor_property(name) {
        return Some(format!(
            "**{name}** \u{2014} Vendor-prefixed CSS property `{name}`, written to the CSS as it is (no px)."
        ));
    }
    if vocab::BOOLEAN_HTML_ATTRS.contains(&name) {
        return Some(format!(
            "**{name}** \u{2014} Boolean HTML attribute, written bare: `[{name}]`."
        ));
    }
    if vocab::HTML_ATTRIBUTES.contains(&name)
        || name.starts_with("aria-")
        || name.starts_with("data-")
    {
        return Some(format!(
            "**{name}** \u{2014} HTML attribute, written `{name}=value`."
        ));
    }
    None
}

/// What a state/media prefix like `hover:` or `md:` applies to.
pub(crate) fn prefix_selector(prefix: &str) -> Option<String> {
    if !prefix.ends_with(':') {
        return None;
    }
    if let Some((_, selector)) = vocab::PSEUDO_PREFIXES.iter().find(|(p, _)| *p == prefix) {
        return Some(if selector.starts_with(" > ") {
            "Styles each direct child (`> *`).".to_string()
        } else {
            format!("Applies in the `{}` state.", selector)
        });
    }
    let name = prefix.trim_end_matches(':');
    if vocab::RESPONSIVE_PREFIXES.contains(&prefix) {
        return Some(format!("Applies from the `{}` viewport width up.", name));
    }
    if vocab::CONTAINER_QUERY_PREFIXES.contains(&prefix) {
        return Some(format!(
            "Applies when the container is at least `{}` wide.",
            name.trim_start_matches("cq-")
        ));
    }
    if vocab::MEDIA_PREFIXES.contains(&prefix) {
        return Some(format!("Applies under the `{}` media condition.", name));
    }
    if name.starts_with("nth:") {
        return Some("Applies to children matching `:nth-child(...)`.".to_string());
    }
    if name.starts_with("has(") {
        return Some("Applies when the element contains a match (`:has(...)`).".to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_element_has_a_hover() {
        for name in ElementKind::all_names() {
            let text = hover(&format!("@{}", name));
            assert!(
                text.as_deref().is_some_and(|t| !t.is_empty()),
                "no hover for @{}",
                name
            );
        }
    }

    #[test]
    fn an_element_s_hover_says_its_layout() {
        let says = |name: &str, text: &str| hover(name).is_some_and(|h| h.contains(text));
        assert!(says("@ol", "Layout: column"));
        assert!(says("@el", "Layout: column"));
        assert!(says("@row", "Layout: row"));
        assert!(says("@button", "Layout: text"));
        assert!(says("@table", "HTML's own"));
        assert!(says("@hr", "takes no content"));
        assert!(says("@strong", "Layout: text"));
        assert!(says("@sub", "Layout: text"));
        assert!(says("@caption", "HTML's own"));
        assert!(says("@br", "takes no content"));
        assert!(says("@menu", "Layout: column"));
        assert!(says("@optgroup", "`label`"));
        assert!(!says("@fragment", "Layout"));
    }

    #[test]
    fn the_hover_of_code_and_textarea_says_their_text_is_shown_as_written() {
        let says = |name: &str, text: &str| hover(name).is_some_and(|h| h.contains(text));
        assert!(says("@code", "shown as written"));
        assert!(says("@textarea", "shown as written"));
        assert!(says("@code", "indented block under it, verbatim"));
        assert!(!says("@kbd", "shown as written"));
        assert!(says("@kbd", "Text after it is its content"));
    }

    #[test]
    fn the_hover_of_a_css_shorthand_says_what_css_reads() {
        let says = |name: &str, text: &str| hover(name).is_some_and(|h| h.contains(text));
        assert!(says("outline", "CSS draws none"));
        assert!(says("line-clamp", "-webkit-box"));
        assert!(says("@link", "no underline"));
    }

    #[test]
    fn every_htmlang_attribute_has_a_hover() {
        for name in vocab::HTMLANG_ATTRIBUTES {
            let text = hover(name);
            assert!(
                text.as_deref().is_some_and(|t| !t.is_empty()),
                "no hover for {}",
                name
            );
        }
    }

    #[test]
    fn documented_names_exist_in_the_compiler() {
        for doc in ELEMENTS {
            assert!(
                ElementKind::from_name(doc.name).is_some(),
                "@{} is not an element",
                doc.name
            );
        }
        for doc in ATTRIBUTES {
            assert!(
                vocab::is_style_attribute(doc.name),
                "{} is not a style attribute",
                doc.name
            );
        }
    }

    #[test]
    fn every_directive_is_documented_once() {
        let documented: Vec<&str> = DIRECTIVES.iter().map(|d| d.name).collect();
        let compiled: Vec<&str> = htmlang::ast::DIRECTIVES.iter().map(|d| d.name).collect();
        let mut a = documented.clone();
        let mut b = compiled.clone();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(
            a, b,
            "the hover docs and the compiler's directive table differ"
        );
        for name in compiled {
            assert!(
                ElementKind::from_name(name).is_none(),
                "@{} is both a directive and an element",
                name
            );
        }
    }

    #[test]
    fn hovers_for_css_html_and_prefixes() {
        assert!(hover("opacity").unwrap().contains("developer.mozilla.org"));
        assert!(hover("placeholder").unwrap().contains("placeholder=value"));
        assert!(hover("required").unwrap().contains("Boolean"));
        assert!(hover("hover:color").unwrap().contains(":hover"));
        assert!(hover("md:").unwrap().contains("viewport"));
        assert!(hover("@spacer").unwrap().contains("standard library"));
        assert!(hover("$truncate").unwrap().contains("bundle"));
        assert!(hover("@nav").unwrap().contains("<nav>"));
        assert!(hover("hidden").unwrap().contains("Boolean"));
        assert!(hover("favicon").unwrap().contains("@page"));
        assert!(hover("@page").unwrap().contains("<body>"));
        assert!(hover("--gap").unwrap().contains("as it is"));
        assert!(hover("--gap").unwrap().contains("everything inside it"));
        assert!(hover("--gap").unwrap().contains(":root"));
        assert!(
            hover("-webkit-tap-highlight-color")
                .unwrap()
                .contains("as it is")
        );
    }
}
