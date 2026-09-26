//! Documentation for htmlang's names, shared by hover and completion.
//!
//! The *names* themselves come from the compiler (`ElementKind`, `TAGS`,
//! `vocab`, the parser's directive list); this file only adds prose. A name
//! without an entry here still gets a summary derived from the compiler's
//! tables, so new elements and attributes are never missing from the editor.

use htmlang::ast::{ElementKind, TagArg};
use htmlang::vocab;

pub(crate) struct Doc {
    pub name: &'static str,
    pub summary: &'static str,
    /// An example; for attributes, `name` alone means it takes no value.
    pub usage: &'static str,
}

impl Doc {
    /// Does this attribute take a value (`spacing 20` vs `bold`)?
    pub fn takes_value(&self) -> bool {
        self.usage.trim() != self.name
    }
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
    doc("el", "The container: a flex column (only `@row` lays out horizontally).", "@el [padding 20, background white]\n  Content"),
    doc("text", "Styled inline text (`<span>`).", "@text [font-weight bold, font-size 24] Hello"),
    doc(
        "paragraph",
        "Flowing text (`<p>`); holds inline `{@...}` elements.",
        "@paragraph\n  Read the {@link /docs docs}.",
    ),
    doc("link", "Link (`<a>`); text after the URL is its content.", "@link /about About us"),
    doc("image", "Image (`<img>`); the argument is the source.", "@image [alt=Logo, width 120] logo.png"),
    doc("script", "Script; its indented body is JavaScript, kept verbatim.", "@script\n  console.log(1)"),
    doc(
        "fragment",
        "Groups children without a wrapper element.",
        "@fragment\n  @text A\n  @text B",
    ),
    doc(
        "children",
        "Inside a function body: where the caller's content goes.",
        "@let card\n  @el [padding 20]\n    @children",
    ),
    doc(
        "slot",
        "Named placeholder in a function or layout, filled by the caller's `@slot` block.",
        "@slot header",
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
    doc("ul", "Bulleted list.", "@ul\n  @li First\n  @li Second"),
    doc("ol", "Numbered list.", "@ol [list-style decimal]\n  @li First"),
    doc("li", "List item.", "@li First"),
    doc("form", "Form; the argument is its `action`.", "@form [method=post] /subscribe"),
    doc("input", "Form input (void element).", "@input [type=email, name=email, required]"),
    doc("button", "Button.", "@button [type=submit] Send"),
    doc("iframe", "Embedded page; the argument is its `src`.", "@iframe [sandbox] https://example.com"),
    doc("video", "Video; the argument is its `src`.", "@video [controls] movie.mp4"),
    doc("audio", "Audio; the argument is its `src`.", "@audio [controls] song.mp3"),
];

/// Directives (names without `@`).
#[rustfmt::skip]
pub(crate) const DIRECTIVES: &[Doc] = &[
    doc(
        "page",
        "Produces a full HTML document with this title. Attributes set `lang` and `favicon`.",
        "@page [lang en] My Site",
    ),
    doc(
        "let",
        "Defines a value, quoted text (`\"...\"`, which keeps its quotes only in CSS), a computed value (`= expr`), an attribute bundle (`[...]`) or a function (indented body).",
        "@let primary #3b82f6\n@let arrow \"→ \"\n@let gap = 8 * 2\n@let card [padding 20]\n@let button $label\n  @el [$card] $label",
    ),
    doc("include", "Inserts another `.hl` file here, with its definitions.", "@include header.hl"),
    doc("raw", "Pastes HTML into the output verbatim: the rest of the line, or an indented block.", "@raw\n  <div class=\"widget\"></div>"),
    doc("markdown", "Markdown (indented body or file), converted to HTML.", "@markdown\n  # Title"),
    doc("style", "Raw CSS; overrides generated styles.", "@style\n  .note { color: gray; }"),
    doc("head", "Raw HTML added to `<head>`.", "@head\n  <link rel=\"icon\" href=\"f.ico\">"),
    doc("meta", "A `<meta>` tag; `og:` names become Open Graph tags.", "@meta description A small site"),
    doc("if", "Renders its body when the condition holds; `@else if` / `@else` follow.", "@if $count > 2 and not $hidden\n  @text Many"),
    doc("else", "Fallback branch of `@if` or `@each`.", "@else\n  @text None"),
    doc(
        "each",
        "Repeats its body for each item of a list or range; an optional second variable is the index, from 0. Records from `@data` have fields `$item.key`.",
        "@each $item, $i in apple, banana\n  @text $i: $item\n@each $post in $posts\n  @text $post.title",
    ),
    doc("data", "Loads data as variables: a JSON file, inline JSON, a glob of files (a list of records), or `env:NAME`.", "@data $site site.json\n@data $links [{\"label\": \"Home\", \"url\": \"/\"}]"),
];

/// htmlang's own attributes, plus CSS properties htmlang treats specially.
#[rustfmt::skip]
pub(crate) const ATTRIBUTES: &[Doc] = &[
    doc("spacing", "Gap between children.", "spacing 20"),
    doc("padding", "Inner space: 1 to 4 values; bare numbers are px.", "padding 12 24"),
    doc("margin", "Outer space: 1 to 4 values; bare numbers are px.", "margin 0 auto"),
    doc("width", "`fill` takes the remaining space, `shrink` fits the content, or a size.", "width fill"),
    doc("height", "`fill` takes the remaining space, `shrink` fits the content, or a size.", "height 200"),
    doc("center-x", "Centers the element horizontally in its parent.", "center-x"),
    doc("center-y", "Centers the element vertically in its parent.", "center-y"),
    doc("align-left", "Aligns the element to the left of its parent.", "align-left"),
    doc("align-right", "Aligns the element to the right of its parent.", "align-right"),
    doc("align-top", "Aligns the element to the top of its parent.", "align-top"),
    doc("align-bottom", "Aligns the element to the bottom of its parent.", "align-bottom"),
    doc("wrap", "Lets a row's children wrap onto new lines.", "wrap"),
    doc("grid-cols", "Number of equal grid columns.", "grid-cols 3"),
    doc("grid-rows", "Number of equal grid rows.", "grid-rows 2"),
    doc("col-span", "Columns a grid child spans.", "col-span 2"),
    doc("row-span", "Rows a grid child spans.", "row-span 2"),
    doc("line-height", "Line height; a bare number is a multiplier of the font size.", "line-height 1.5"),
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

/// One-line summary of an element: from `ELEMENTS`, or derived from its
/// row in `TAGS`.
pub(crate) fn element_summary(name: &str) -> Option<String> {
    if let Some(doc) = find(ELEMENTS, name) {
        return Some(doc.summary.to_string());
    }
    let spec = ElementKind::from_name(name)?.spec()?;
    let mut summary = format!("Renders `<{}>`", spec.html);
    if spec.css.contains("flex-direction:column") {
        summary.push_str(", laid out as a column");
    }
    if spec.void {
        summary.push_str(" (void element)");
    }
    summary.push('.');
    match spec.arg {
        TagArg::Attr(attr) => summary.push_str(&format!(" The argument is its `{}`.", attr)),
        TagArg::Text | TagArg::Child => summary.push_str(" Text after it is its content."),
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
    if vocab::HTMLANG_ATTRIBUTES.contains(&name) {
        return Some(markdown(name, "htmlang attribute.", ""));
    }
    if vocab::is_css_property(name) {
        return Some(format!(
            "**{name}** \u{2014} CSS property `{name}`, passed through unchanged.\n\n[MDN](https://developer.mozilla.org/docs/Web/CSS/{name})"
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
    }
}
