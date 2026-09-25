use tower_lsp::lsp_types::*;

use crate::completion::collect_theme_tokens;

pub(crate) fn hover_at(text: &str, position: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(position.line as usize)?;
    let col = (position.character as usize).min(line.len());
    let word = word_at(line, col)?;

    let doc = if let Some(var_name) = word.strip_prefix('$') {
        hover_theme_token(text, var_name).or_else(|| hover_variable(text, var_name))
    } else if let Some(fn_name) = word.strip_prefix('@') {
        hover_user_fn(text, fn_name).or_else(|| hover_builtin(&word))
    } else {
        hover_builtin(&word)
    }?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: doc,
        }),
        range: None,
    })
}

pub(crate) fn word_at(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = col;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    if start == end {
        return None;
    }
    Some(line[start..end].to_string())
}

pub(crate) fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'@' || c == b'$' || c == b'-' || c == b'_' || c == b':'
}

fn hover_theme_token(text: &str, name: &str) -> Option<String> {
    let tokens = collect_theme_tokens(text);
    let (token_name, value) = tokens.iter().find(|(n, _)| n == name)?;
    Some(format!(
        "**${}** \u{2014} `@theme` token\n\nValue: `{}`\n\nAlso available as the CSS custom property `--{}`.",
        token_name, value, token_name
    ))
}

fn hover_variable(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ")
            && let Some((n, v)) = rest.trim().split_once(' ')
            && n == name
        {
            return Some(format!("**${}** = `{}`", name, v.trim()));
        }
    }

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let rest = rest.trim();
            // Attribute bundle: @let name [...]
            if let Some(bracket) = rest.find('[') {
                let def_name = rest[..bracket].trim();
                if def_name == name {
                    return Some(format!(
                        "**${}** \u{2014} Attribute bundle\n\n`{}`",
                        name, trimmed
                    ));
                }
            }
            // Function parameter: @let fn-name $param (with body)
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(fn_name) = parts.first() {
                for param in &parts[1..] {
                    let p = param.strip_prefix('$').unwrap_or(param);
                    if p == name {
                        return Some(format!(
                            "**${}** \u{2014} Parameter of `@{}`",
                            name, fn_name
                        ));
                    }
                }
            }
        }
    }

    None
}

fn hover_user_fn(text: &str, name: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.first() == Some(&name) {
                let params = &parts[1..];

                // Collect doc-comment lines above the definition (lines starting with --)
                let mut doc_lines: Vec<&str> = Vec::new();
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    let prev = lines[j].trim();
                    if let Some(comment) = prev.strip_prefix("-- ") {
                        doc_lines.push(comment);
                    } else if let Some(comment) = prev.strip_prefix("--") {
                        doc_lines.push(comment);
                    } else {
                        break;
                    }
                }
                doc_lines.reverse();

                let doc_str = if doc_lines.is_empty() {
                    String::new()
                } else {
                    format!("\n\n{}", doc_lines.join("\n"))
                };

                // Format params showing defaults
                let params_str = if params.is_empty() {
                    String::new()
                } else {
                    let formatted: Vec<String> = params
                        .iter()
                        .map(|p| {
                            if p.contains('=') {
                                let (name, default) = p.split_once('=').unwrap();
                                format!("{} (default: {})", name, default)
                            } else {
                                p.to_string()
                            }
                        })
                        .collect();
                    format!("\n\nParameters: {}", formatted.join(", "))
                };

                return Some(format!(
                    "**@{}** \u{2014} User function{}{}",
                    name, params_str, doc_str
                ));
            }
        }
    }
    None
}

fn hover_builtin(word: &str) -> Option<String> {
    // Strip state prefix for attribute lookup
    let (state, base) = if let Some(rest) = word.strip_prefix("hover:") {
        (Some("hover"), rest)
    } else if let Some(rest) = word.strip_prefix("active:") {
        (Some("active"), rest)
    } else if let Some(rest) = word.strip_prefix("focus:") {
        (Some("focus"), rest)
    } else if let Some(rest) = word.strip_prefix("focus-visible:") {
        (Some("focus-visible"), rest)
    } else if let Some(rest) = word.strip_prefix("focus-within:") {
        (Some("focus-within"), rest)
    } else if let Some(rest) = word.strip_prefix("disabled:") {
        (Some("disabled"), rest)
    } else if let Some(rest) = word.strip_prefix("checked:") {
        (Some("checked"), rest)
    } else if let Some(rest) = word.strip_prefix("placeholder:") {
        (Some("placeholder"), rest)
    } else if let Some(rest) = word.strip_prefix("first:") {
        (Some("first"), rest)
    } else if let Some(rest) = word.strip_prefix("last:") {
        (Some("last"), rest)
    } else if let Some(rest) = word.strip_prefix("odd:") {
        (Some("odd"), rest)
    } else if let Some(rest) = word.strip_prefix("even:") {
        (Some("even"), rest)
    } else if let Some(rest) = word.strip_prefix("before:") {
        (Some("before"), rest)
    } else if let Some(rest) = word.strip_prefix("after:") {
        (Some("after"), rest)
    } else if let Some(rest) = word.strip_prefix("sm:") {
        (Some("sm"), rest)
    } else if let Some(rest) = word.strip_prefix("md:") {
        (Some("md"), rest)
    } else if let Some(rest) = word.strip_prefix("lg:") {
        (Some("lg"), rest)
    } else if let Some(rest) = word.strip_prefix("xl:") {
        (Some("xl"), rest)
    } else if let Some(rest) = word.strip_prefix("2xl:") {
        (Some("2xl"), rest)
    } else if let Some(rest) = word.strip_prefix("motion-safe:") {
        (Some("motion-safe"), rest)
    } else if let Some(rest) = word.strip_prefix("motion-reduce:") {
        (Some("motion-reduce"), rest)
    } else if let Some(rest) = word.strip_prefix("landscape:") {
        (Some("landscape"), rest)
    } else if let Some(rest) = word.strip_prefix("portrait:") {
        (Some("portrait"), rest)
    } else if let Some(rest) = word.strip_prefix("visited:") {
        (Some("visited"), rest)
    } else if let Some(rest) = word.strip_prefix("empty:") {
        (Some("empty"), rest)
    } else if let Some(rest) = word.strip_prefix("target:") {
        (Some("target"), rest)
    } else if let Some(rest) = word.strip_prefix("valid:") {
        (Some("valid"), rest)
    } else if let Some(rest) = word.strip_prefix("invalid:") {
        (Some("invalid"), rest)
    } else {
        (None, word)
    };

    let doc = match base {
        // Elements
        "@row" => {
            "**@row** \u{2014} Horizontal layout\n\nRenders as `<div>` with `display: flex; flex-direction: row`.\n\nChildren are laid out left-to-right."
        }
        "@column" => {
            "**@column** \u{2014} Vertical layout\n\nRenders as `<div>` with `display: flex; flex-direction: column`.\n\nChildren are laid out top-to-bottom."
        }
        "@el" => {
            "**@el** \u{2014} Generic container\n\nRenders as `<div>` with column flex layout."
        }
        "@text" => {
            "**@text** \u{2014} Inline text\n\nRenders as `<span>`.\n\nUsage: `@text [bold, size 24] Hello world`"
        }
        "@paragraph" => {
            "**@paragraph** \u{2014} Text block\n\nRenders as `<p>`.\n\nSupports inline elements: `{@text [bold] word}`"
        }
        "@h1" => {
            "**@h1** \u{2014} Heading level 1\n\nRenders as `<h1>`.\n\nUsage: `@h1 Page Title`"
        }
        "@h2" => {
            "**@h2** \u{2014} Heading level 2\n\nRenders as `<h2>`.\n\nUsage: `@h2 Section Title`"
        }
        "@h3" => {
            "**@h3** \u{2014} Heading level 3\n\nRenders as `<h3>`.\n\nUsage: `@h3 Subsection`"
        }
        "@h4" => "**@h4** \u{2014} Heading level 4\n\nRenders as `<h4>`.",
        "@h5" => "**@h5** \u{2014} Heading level 5\n\nRenders as `<h5>`.",
        "@h6" => "**@h6** \u{2014} Heading level 6\n\nRenders as `<h6>`.",
        "@image" => {
            "**@image** \u{2014} Image\n\nRenders as `<img>`.\n\nUsage: `@image [width 200] https://example.com/photo.jpg`"
        }
        "@link" => {
            "**@link** \u{2014} Hyperlink\n\nRenders as `<a>`.\n\nUsage: `@link [color blue] https://example.com Link text`"
        }
        "@raw" => {
            "**@raw** \u{2014} Raw HTML\n\nPasses content through without processing.\n\nUsage: `@raw \"\"\"<div>custom html</div>\"\"\"`"
        }
        "@page" => {
            "**@page** \u{2014} Page title\n\nSets the HTML `<title>` and wraps output in a full document. Attributes set `lang`, `favicon`, `canonical` and `base`.\n\nUsage: `@page [lang en, favicon /favicon.png] My Page Title`"
        }
        "@let" => {
            "**@let** \u{2014} Definition\n\nDefines a variable, attribute bundle, or component.\n\n- Variable: `@let primary #3b82f6`\n- Attribute bundle: `@let card-style [padding 20, rounded 8]`\n- Component:\n```\n@let card $title\n  @el [padding 20]\n    @text [bold] $title\n    @children\n```"
        }
        "@keyframes" => {
            "**@keyframes** \u{2014} CSS Animation\n\nDefines keyframes for CSS animations.\n\n```\n@keyframes fade-in\n  from{opacity:0}to{opacity:1}\n```\n\nUse with `animation` attribute: `[animation fade-in 0.3s ease]`"
        }
        "@children" => {
            "**@children** \u{2014} Children slot\n\nPlaceholder inside a component body replaced with the caller's children."
        }
        "@input" => {
            "**@input** \u{2014} Form input\n\nRenders as self-closing `<input>`.\n\nUsage: `@input [type text, placeholder Name, name user]`"
        }
        "@button" => {
            "**@button** \u{2014} Button\n\nRenders as `<button>`.\n\nUsage: `@button [type submit] Click me`"
        }
        "@select" => {
            "**@select** \u{2014} Select dropdown\n\nRenders as `<select>`. Use `@option` children.\n\nUsage: `@select [name color]`"
        }
        "@textarea" => {
            "**@textarea** \u{2014} Multi-line text input\n\nRenders as `<textarea>`.\n\nUsage: `@textarea [name bio, rows 4] Default text`"
        }
        "@option" => {
            "**@option** \u{2014} Select option\n\nRenders as `<option>`.\n\nUsage: `@option [value red] Red`"
        }
        "@label" => {
            "**@label** \u{2014} Form label\n\nRenders as `<label>`.\n\nUsage: `@label [for email] Email Address`"
        }
        "@if" => {
            "**@if** \u{2014} Conditional\n\nConditionally includes children at compile time.\n\n```\n@if $theme == dark\n  @el [background #333]\n@else if $theme == light\n  @el [background white]\n@else\n  @el [background gray]\n```"
        }
        "@each" => {
            "**@each** \u{2014} Loop\n\nRepeat children for each item in a comma-separated list.\nOptional index variable.\n\n```\n@each $color, $i in red,green,blue\n  @text $i: $color\n```"
        }
        "@include" => {
            "**@include** \u{2014} Include file\n\nIncludes another .hl file (DOM nodes + definitions).\n\nUsage: `@include header.hl`"
        }
        "@import" => {
            "**@import** \u{2014} Import definitions\n\nImports `@let` definitions from another .hl file without emitting DOM nodes.\n\nUsage: `@import theme.hl`"
        }
        "@meta" => {
            "**@meta** \u{2014} Meta tag\n\nAdds a `<meta>` tag to `<head>`.\n\nUsage: `@meta description A portfolio site`"
        }
        "@head" => {
            "**@head** \u{2014} Head content\n\nAdds raw content to `<head>` (fonts, icons, etc.).\n\n```\n@head\n  <link rel=\"icon\" href=\"favicon.ico\">\n```"
        }
        "@style" => {
            "**@style** \u{2014} Custom CSS\n\nAdds raw CSS to the stylesheet.\n\n```\n@style\n  .custom { border: 1px solid red; }\n  @container sidebar (min-width: 400px) { ... }\n```"
        }
        "@slot" => {
            "**@slot** \u{2014} Named slot\n\nDefines a named insertion point inside a component. Callers fill it with `@slot name` + children.\n\n```\n@let layout\n  @slot header\n  @children\n  @slot footer\n```"
        }
        // Semantic elements
        "@nav" => {
            "**@nav** \u{2014} Navigation\n\nRenders as `<nav>`. Semantic landmark for navigation links."
        }
        "@header" => {
            "**@header** \u{2014} Header\n\nRenders as `<header>`. Page or section header."
        }
        "@footer" => {
            "**@footer** \u{2014} Footer\n\nRenders as `<footer>`. Page or section footer."
        }
        "@main" => {
            "**@main** \u{2014} Main content\n\nRenders as `<main>`. Primary content of the page."
        }
        "@section" => {
            "**@section** \u{2014} Section\n\nRenders as `<section>`. Thematic grouping of content."
        }
        "@article" => {
            "**@article** \u{2014} Article\n\nRenders as `<article>`. Self-contained, independently distributable content."
        }
        "@aside" => {
            "**@aside** \u{2014} Aside\n\nRenders as `<aside>`. Content tangentially related to surrounding content."
        }
        // List elements
        "@list" => {
            "**@list** \u{2014} List\n\nRenders as `<ul>` (or `<ol>` with `[ordered]`).\n\nUsage:\n```\n@list [ordered]\n  @item First\n  @item Second\n```"
        }
        "@item" => "**@item** \u{2014} List item\n\nRenders as `<li>`. Use inside `@list`.",
        // Table elements
        "@table" => {
            "**@table** \u{2014} Table\n\nRenders as `<table>`.\n\n```\n@table\n  @thead\n    @tr\n      @th Name\n      @th Age\n  @tbody\n    @tr\n      @td Alice\n      @td 30\n```"
        }
        "@thead" => "**@thead** \u{2014} Table head\n\nRenders as `<thead>`. Groups header rows.",
        "@tbody" => "**@tbody** \u{2014} Table body\n\nRenders as `<tbody>`. Groups body rows.",
        "@tr" => "**@tr** \u{2014} Table row\n\nRenders as `<tr>`.",
        "@td" => "**@td** \u{2014} Table cell\n\nRenders as `<td>`. Regular table data cell.",
        "@th" => {
            "**@th** \u{2014} Table header cell\n\nRenders as `<th>`. Header cell (typically bold/centered)."
        }
        // Media elements
        "@video" => {
            "**@video** \u{2014} Video\n\nRenders as `<video>`.\n\nUsage: `@video [controls] demo.mp4`"
        }
        "@audio" => {
            "**@audio** \u{2014} Audio\n\nRenders as `<audio>`.\n\nUsage: `@audio [controls] song.mp3`"
        }
        // Additional elements
        "@form" => {
            "**@form** \u{2014} Form\n\nRenders as `<form>`. Container for form elements.\n\nUsage: `@form [method post] /submit`"
        }
        "@details" => {
            "**@details** \u{2014} Disclosure\n\nRenders as `<details>`. Use `[open]` for initially expanded.\n\nContains `@summary` for the toggle label."
        }
        "@summary" => {
            "**@summary** \u{2014} Summary\n\nRenders as `<summary>`. Toggle label inside `@details`.\n\nUsage: `@summary Click to expand`"
        }
        "@blockquote" => {
            "**@blockquote** \u{2014} Block quotation\n\nRenders as `<blockquote>`. Semantic quotation container."
        }
        "@cite" => {
            "**@cite** \u{2014} Citation\n\nRenders as `<cite>`. Source or reference for a quotation.\n\nUsage: `@cite The Great Gatsby`"
        }
        "@code" => {
            "**@code** \u{2014} Code\n\nRenders as `<code>` with monospace font.\n\nUsage: `@code console.log(\"hello\")`"
        }
        "@pre" => {
            "**@pre** \u{2014} Preformatted\n\nRenders as `<pre>` with preserved whitespace and monospace font."
        }
        "@hr" => {
            "**@hr** \u{2014} Horizontal Rule\n\nRenders as self-closing `<hr>`. Visual divider.\n\nUsage: `@hr [border-top 1 #ccc]`"
        }
        "@figure" => {
            "**@figure** \u{2014} Figure\n\nRenders as `<figure>`. Container for media with optional `@figcaption`.\n\n```\n@figure\n  @image photo.jpg\n  @figcaption A nice photo\n```"
        }
        "@figcaption" => {
            "**@figcaption** \u{2014} Figure caption\n\nRenders as `<figcaption>`. Caption text inside `@figure`."
        }
        "@progress" => {
            "**@progress** \u{2014} Progress bar\n\nRenders as `<progress>`.\n\nUsage: `@progress [value 70, max 100]`"
        }
        "@meter" => {
            "**@meter** \u{2014} Meter\n\nRenders as `<meter>`. Gauge for scalar measurement.\n\nUsage: `@meter [value 0.7, min 0, max 1, low 0.3, high 0.8]`"
        }
        "@fragment" => {
            "**@fragment** \u{2014} Fragment\n\nGroups children without emitting a wrapper element. Renders children directly in the parent."
        }
        // New elements
        "@dialog" => {
            "**@dialog** \u{2014} Dialog\n\nRenders as `<dialog>`. Modal or non-modal dialog box.\n\nUsage: `@dialog [open] Dialog content`"
        }
        "@dl" => {
            "**@dl** \u{2014} Description list\n\nRenders as `<dl>`. Contains `@dt` and `@dd` pairs."
        }
        "@dt" => {
            "**@dt** \u{2014} Description term\n\nRenders as `<dt>`. Term in a `@dl` description list."
        }
        "@dd" => {
            "**@dd** \u{2014} Description details\n\nRenders as `<dd>`. Details for a `@dt` term."
        }
        "@fieldset" => {
            "**@fieldset** \u{2014} Fieldset\n\nRenders as `<fieldset>`. Groups related form elements.\n\nUse `@legend` for a caption."
        }
        "@legend" => {
            "**@legend** \u{2014} Legend\n\nRenders as `<legend>`. Caption for a `@fieldset`."
        }
        "@picture" => {
            "**@picture** \u{2014} Picture\n\nRenders as `<picture>`. Container for responsive image sources.\n\nUse `@source` children for different media queries."
        }
        "@source" => {
            "**@source** \u{2014} Source\n\nRenders as `<source>`. Media source for `@picture`, `@video`, or `@audio`.\n\nUsage: `@source [src image.webp, type image/webp]`"
        }
        "@time" => {
            "**@time** \u{2014} Time\n\nRenders as `<time>`. Machine-readable date/time.\n\nUsage: `@time [datetime 2024-01-15] January 15`"
        }
        "@mark" => "**@mark** \u{2014} Mark\n\nRenders as `<mark>`. Highlighted or marked text.",
        "@kbd" => {
            "**@kbd** \u{2014} Keyboard input\n\nRenders as `<kbd>`. Represents keyboard input.\n\nUsage: `@kbd Ctrl+C`"
        }
        "@abbr" => {
            "**@abbr** \u{2014} Abbreviation\n\nRenders as `<abbr>`. Abbreviation with optional title.\n\nUsage: `@abbr [title Hypertext Markup Language] HTML`"
        }
        "@datalist" => {
            "**@datalist** \u{2014} Datalist\n\nRenders as `<datalist>`. Provides predefined options for `@input`.\n\nUsage: `@datalist [id colors]`"
        }
        // Directives
        "@match" => {
            "**@match** \u{2014} Pattern matching\n\nMatch a value against cases.\n\n```\n@match $theme\n  @case dark\n    @el [background #333]\n  @case light\n    @el [background white]\n  @default\n    @el [background gray]\n```"
        }
        "@case" => {
            "**@case** \u{2014} Match case\n\nA case inside `@match`. Matches when the value equals the case value."
        }
        "@default" => {
            "**@default** \u{2014} Default case\n\nFallback case inside `@match` when no other case matches."
        }
        "@warn" => {
            "**@warn** \u{2014} Compile warning\n\nEmit a custom warning during compilation.\n\nUsage: `@warn This value is deprecated`"
        }
        "@breakpoint" => {
            "**@breakpoint** \u{2014} Custom breakpoint\n\nDefines a custom responsive breakpoint.\n\nUsage: `@breakpoint tablet 600`"
        }
        "@theme" => {
            "**@theme** \u{2014} Design tokens\n\nDefines centralized design tokens (colors, spacing, fonts).\nEach token becomes both a `$variable` and a `--css-custom-property`.\n\n```\n@theme\n  primary #3b82f6\n  spacing-md 16\n  font-body system-ui, sans-serif\n```"
        }
        "@deprecated" => {
            "**@deprecated** `<message>`\n\nMarks the next `@let` component as deprecated. Callers get a compile-time warning.\n\n```\n@deprecated Use @new-card instead\n@let old-card $title\n  ...\n```"
        }
        "@extends" => {
            "**@extends** `<file.hl>`\n\nInherit a layout template. Fill named `@slot` blocks.\n\n```\n@extends layout.hl\n@slot content\n  My page content\n@slot sidebar\n  Sidebar content\n```"
        }
        "@font-face" => {
            "**@font-face** \u{2014} Custom font\n\nDefines a custom font face. Generates a CSS `@font-face` rule.\n\n```\n@font-face\n  family Inter\n  src url(/fonts/Inter.woff2)\n  weight 400 700\n```"
        }
        "@json-ld" => {
            "**@json-ld** \u{2014} Structured data\n\nAdds JSON-LD structured data to `<head>` as `<script type=\"application/ld+json\">`.\n\n```\n@json-ld\n  type Organization\n  name My Company\n  url https://example.com\n```"
        }
        // Attributes
        "spacing" | "gap" => {
            "**spacing** `<value>`\n\nGap between children. Supports CSS units (px, rem, em, %).\nMaps to CSS `gap`."
        }
        "padding" => {
            "**padding** `<value>` | `<y> <x>` | `<t> <h> <b>` | `<t> <r> <b> <l>`\n\nInner padding. Supports CSS units. Accepts 1\u{2013}4 values."
        }
        "padding-x" => {
            "**padding-x** `<value>`\n\nHorizontal padding (left + right). Supports CSS units."
        }
        "padding-y" => {
            "**padding-y** `<value>`\n\nVertical padding (top + bottom). Supports CSS units."
        }
        "width" => {
            "**width** `<value>` | `fill` | `shrink`\n\n- Number/unit: fixed width (e.g., `300`, `50%`, `80ch`)\n- `fill`: expand to fill parent\n- `shrink`: prevent flex shrinking"
        }
        "height" => {
            "**height** `<value>` | `fill` | `shrink`\n\n- Number/unit: fixed height (e.g., `300`, `100vh`)\n- `fill`: expand to fill parent\n- `shrink`: prevent flex shrinking"
        }
        "min-width" => "**min-width** `<value>` \u{2014} Minimum width. Supports CSS units.",
        "max-width" => "**max-width** `<value>` \u{2014} Maximum width. Supports CSS units.",
        "min-height" => "**min-height** `<value>` \u{2014} Minimum height. Supports CSS units.",
        "max-height" => "**max-height** `<value>` \u{2014} Maximum height. Supports CSS units.",
        "center-x" => {
            "**center-x**\n\nCenter horizontally.\n\nIn column parent: `align-self: center`\nOtherwise: auto margins."
        }
        "center-y" => {
            "**center-y**\n\nCenter vertically.\n\nIn row parent: `align-self: center`\nOtherwise: auto margins."
        }
        "align-left" => "**align-left** \u{2014} Align to the left edge.",
        "align-right" => "**align-right** \u{2014} Align to the right edge.",
        "align-top" => "**align-top** \u{2014} Align to the top edge.",
        "align-bottom" => "**align-bottom** \u{2014} Align to the bottom edge.",
        "background" => {
            "**background** `<color>` \u{2014} Background color or CSS background value."
        }
        "color" => "**color** `<color>` \u{2014} Text color.",
        "border" => {
            "**border** `<width> [color]`\n\nBorder. Width in pixels, color defaults to `currentColor`."
        }
        "border-top" => "**border-top** `<width> [color]` \u{2014} Top border.",
        "border-bottom" => "**border-bottom** `<width> [color]` \u{2014} Bottom border.",
        "border-left" => "**border-left** `<width> [color]` \u{2014} Left border.",
        "border-right" => "**border-right** `<width> [color]` \u{2014} Right border.",
        "rounded" => "**rounded** `<value>` \u{2014} Border radius. Supports CSS units.",
        "bold" => "**bold** \u{2014} Bold text (`font-weight: bold`).",
        "italic" => "**italic** \u{2014} Italic text (`font-style: italic`).",
        "underline" => "**underline** \u{2014} Underlined text.",
        "size" => "**size** `<value>` \u{2014} Font size in pixels.",
        "font" => "**font** `<family>` \u{2014} Font family.",
        "transition" => {
            "**transition** `<value>` \u{2014} CSS transition (e.g., `all 0.15s ease`)."
        }
        "cursor" => "**cursor** `<value>` \u{2014} CSS cursor (e.g., `pointer`).",
        "opacity" => "**opacity** `<value>` \u{2014} Opacity from 0 to 1.",
        "text-align" => {
            "**text-align** `<value>` \u{2014} Text alignment (`left`, `center`, `right`, `justify`)."
        }
        "line-height" => {
            "**line-height** `<value>` \u{2014} Line height. Unitless (e.g., `1.5`) or pixels."
        }
        "overflow" => {
            "**overflow** `<value>` \u{2014} Overflow behavior (`hidden`, `scroll`, `auto`, `visible`)."
        }
        "position" => {
            "**position** `<value>` \u{2014} Position type (`relative`, `absolute`, `fixed`, `sticky`)."
        }
        "top" => "**top** `<value>` \u{2014} Top offset for positioned elements.",
        "right" => "**right** `<value>` \u{2014} Right offset for positioned elements.",
        "bottom" => "**bottom** `<value>` \u{2014} Bottom offset for positioned elements.",
        "left" => "**left** `<value>` \u{2014} Left offset for positioned elements.",
        "z-index" => "**z-index** `<value>` \u{2014} Stack order (integer).",
        "display" => {
            "**display** `<value>` \u{2014} Display mode (`none`, `block`, `inline`, `flex`, `grid`)."
        }
        "visibility" => "**visibility** `<value>` \u{2014} Visibility (`visible`, `hidden`).",
        "transform" => {
            "**transform** `<value>` \u{2014} CSS transform (e.g., `rotate(45deg)`, `scale(1.5)`)."
        }
        "backdrop-filter" => {
            "**backdrop-filter** `<value>` \u{2014} Backdrop filter (e.g., `blur(10px)`)."
        }
        "letter-spacing" => {
            "**letter-spacing** `<value>` \u{2014} Letter spacing. Supports CSS units."
        }
        "text-transform" => {
            "**text-transform** `<value>` \u{2014} Text transform (`uppercase`, `lowercase`, `capitalize`)."
        }
        "white-space" => {
            "**white-space** `<value>` \u{2014} White-space behavior (`nowrap`, `pre`, `normal`)."
        }
        "grid" => "**grid** \u{2014} Enable CSS grid layout on this element.",
        "grid-cols" => {
            "**grid-cols** `<value>` \u{2014} Grid template columns. Number for equal columns, or CSS value."
        }
        "grid-rows" => {
            "**grid-rows** `<value>` \u{2014} Grid template rows. Number for equal rows, or CSS value."
        }
        "col-span" => "**col-span** `<value>` \u{2014} Span N columns in a grid.",
        "row-span" => "**row-span** `<value>` \u{2014} Span N rows in a grid.",
        "shadow" => {
            "**shadow** `<value>` \u{2014} Box shadow. Raw CSS value (e.g., `0 2px 4px rgba(0,0,0,0.1)`)."
        }
        "gap-x" => {
            "**gap-x** `<value>` \u{2014} Horizontal gap between children in pixels. Maps to `column-gap`."
        }
        "gap-y" => {
            "**gap-y** `<value>` \u{2014} Vertical gap between children in pixels. Maps to `row-gap`."
        }
        "wrap" => "**wrap** \u{2014} Enable flex-wrap for children.",
        "id" => "**id** `<value>` \u{2014} HTML id attribute.",
        "class" => "**class** `<value>` \u{2014} HTML class attribute.",
        "animation" => {
            "**animation** `<value>` \u{2014} CSS animation shorthand (e.g., `fade-in 0.3s ease`).\n\nDefine animations with `@keyframes`."
        }
        "container" => {
            "**container** \u{2014} Enable container queries (`container-type: inline-size`)."
        }
        "container-name" => {
            "**container-name** `<value>` \u{2014} Name this container for `@container` queries."
        }
        "container-type" => {
            "**container-type** `<value>` \u{2014} Container type (`inline-size`, `size`, `normal`)."
        }
        // Form attributes
        "type" => {
            "**type** `<value>` \u{2014} Input type (`text`, `email`, `password`, `submit`, etc.)."
        }
        "placeholder" => "**placeholder** `<value>` \u{2014} Placeholder text for inputs.",
        "name" => "**name** `<value>` \u{2014} Form field name.",
        "value" => "**value** `<value>` \u{2014} Form field value.",
        "disabled" => "**disabled** \u{2014} Disable the element.",
        "required" => "**required** \u{2014} Mark field as required.",
        "checked" => "**checked** \u{2014} Checkbox/radio checked state.",
        "for" => "**for** `<id>` \u{2014} Label target (id of the associated input).",
        "rows" => "**rows** `<value>` \u{2014} Number of visible rows for textarea.",
        "cols" => "**cols** `<value>` \u{2014} Number of visible columns for textarea.",
        "maxlength" => "**maxlength** `<value>` \u{2014} Maximum input length.",
        // Accessibility
        "alt" => "**alt** `<value>` \u{2014} Alternative text for images.",
        "role" => "**role** `<value>` \u{2014} ARIA role (e.g., `navigation`, `banner`, `main`).",
        "tabindex" => {
            "**tabindex** `<value>` \u{2014} Tab order. `0` = natural order, `-1` = skip."
        }
        "title" => "**title** `<value>` \u{2014} Tooltip text.",
        // New CSS attributes
        "aspect-ratio" => {
            "**aspect-ratio** `<value>` \u{2014} CSS aspect ratio (e.g., `16/9`, `1`, `4/3`)."
        }
        "outline" => {
            "**outline** `<width> [color]` \u{2014} Outline (like border but doesn't affect layout)."
        }
        "padding-inline" => {
            "**padding-inline** `<value>` \u{2014} Horizontal padding (logical property, i18n-aware)."
        }
        "padding-block" => {
            "**padding-block** `<value>` \u{2014} Vertical padding (logical property, i18n-aware)."
        }
        "margin-inline" => {
            "**margin-inline** `<value>` \u{2014} Horizontal margin (logical property, i18n-aware)."
        }
        "margin-block" => {
            "**margin-block** `<value>` \u{2014} Vertical margin (logical property, i18n-aware)."
        }
        "scroll-snap-type" => {
            "**scroll-snap-type** `<value>` \u{2014} Scroll snap type (`x mandatory`, `y proximity`)."
        }
        "scroll-snap-align" => {
            "**scroll-snap-align** `<value>` \u{2014} Scroll snap alignment (`start`, `center`, `end`)."
        }
        // Media/image attributes
        "loading" => {
            "**loading** `<value>` \u{2014} Loading behavior for images (`lazy`, `eager`)."
        }
        "decoding" => {
            "**decoding** `<value>` \u{2014} Image decoding mode (`async`, `sync`, `auto`)."
        }
        "controls" => "**controls** \u{2014} Show media controls (for @video, @audio).",
        "autoplay" => "**autoplay** \u{2014} Auto-play media.",
        "loop" => "**loop** \u{2014} Loop media playback.",
        "muted" => "**muted** \u{2014} Mute media.",
        "poster" => "**poster** `<url>` \u{2014} Poster image for video.",
        "preload" => {
            "**preload** `<value>` \u{2014} Media preload hint (`auto`, `metadata`, `none`)."
        }
        "ordered" => "**ordered** \u{2014} Use ordered list (`<ol>` instead of `<ul>`).",
        "src" => "**src** `<url>` \u{2014} Source URL for media elements.",
        // New CSS attributes
        "margin" => {
            "**margin** `<value>` | `<y> <x>` | `<t> <h> <b>` | `<t> <r> <b> <l>`\n\nOuter margin. Supports CSS units. Accepts 1\u{2013}4 values."
        }
        "margin-x" => "**margin-x** `<value>` \u{2014} Horizontal margin (left + right).",
        "margin-y" => "**margin-y** `<value>` \u{2014} Vertical margin (top + bottom).",
        "filter" => {
            "**filter** `<value>` \u{2014} CSS filter (e.g., `blur(5px)`, `brightness(1.2)`, `grayscale(1)`)."
        }
        "object-fit" => {
            "**object-fit** `<value>` \u{2014} How content fits its container (`cover`, `contain`, `fill`, `none`, `scale-down`)."
        }
        "object-position" => {
            "**object-position** `<value>` \u{2014} Position of content within container (e.g., `center`, `top left`)."
        }
        "text-shadow" => {
            "**text-shadow** `<value>` \u{2014} Text shadow. Raw CSS value (e.g., `1px 1px 2px rgba(0,0,0,0.3)`)."
        }
        "text-overflow" => {
            "**text-overflow** `<value>` \u{2014} Text overflow behavior (`ellipsis`, `clip`). Combine with `white-space nowrap` and `overflow hidden`."
        }
        "pointer-events" => {
            "**pointer-events** `<value>` \u{2014} Pointer event behavior (`none`, `auto`)."
        }
        "user-select" => {
            "**user-select** `<value>` \u{2014} Text selection behavior (`none`, `text`, `all`, `auto`)."
        }
        "justify-content" => {
            "**justify-content** `<value>` \u{2014} Main axis alignment (`center`, `space-between`, `space-around`, `flex-start`, `flex-end`)."
        }
        "align-items" => {
            "**align-items** `<value>` \u{2014} Cross axis alignment (`center`, `flex-start`, `flex-end`, `stretch`, `baseline`)."
        }
        "order" => "**order** `<value>` \u{2014} Flex/grid item order (integer).",
        "background-size" => {
            "**background-size** `<value>` \u{2014} Background size (`cover`, `contain`, `auto`, or dimensions)."
        }
        "background-position" => {
            "**background-position** `<value>` \u{2014} Background position (`center`, `top`, `bottom left`, etc.)."
        }
        "background-repeat" => {
            "**background-repeat** `<value>` \u{2014} Background repeat (`no-repeat`, `repeat`, `repeat-x`, `repeat-y`)."
        }
        "word-break" => {
            "**word-break** `<value>` \u{2014} Word breaking behavior (`break-all`, `keep-all`, `normal`)."
        }
        "overflow-wrap" => {
            "**overflow-wrap** `<value>` \u{2014} Overflow wrapping (`break-word`, `anywhere`, `normal`)."
        }
        // New element attributes
        "open" => "**open** \u{2014} Initially expand `@details` element.",
        "novalidate" => "**novalidate** \u{2014} Disable form validation.",
        "low" => "**low** `<value>` \u{2014} Low threshold for `@meter`.",
        "high" => "**high** `<value>` \u{2014} High threshold for `@meter`.",
        "optimum" => "**optimum** `<value>` \u{2014} Optimum value for `@meter`.",
        "colspan" => "**colspan** `<value>` \u{2014} Number of columns a cell spans.",
        "rowspan" => "**rowspan** `<value>` \u{2014} Number of rows a cell spans.",
        "scope" => {
            "**scope** `<value>` \u{2014} Header scope (`col`, `row`, `colgroup`, `rowgroup`)."
        }
        "inline" => "**inline** \u{2014} Inline SVG image content into the HTML output.",
        "color-scheme" => {
            "**color-scheme** `<value>` \u{2014} Color scheme preference (`light`, `dark`, `light dark`)."
        }
        "appearance" => {
            "**appearance** `<value>` \u{2014} Form element appearance (`none` to remove native styling)."
        }
        "popover" => {
            "**popover** \u{2014} Make element a popover (HTML Popover API). Shown/hidden declaratively."
        }
        "popovertarget" => "**popovertarget** `<id>` \u{2014} ID of the popover element to toggle.",
        "popovertargetaction" => {
            "**popovertargetaction** `<value>` \u{2014} Popover action (`toggle`, `show`, `hide`)."
        }
        "inputmode" => {
            "**inputmode** `<value>` \u{2014} Virtual keyboard type (`numeric`, `email`, `search`, `tel`, `url`)."
        }
        "enterkeyhint" => {
            "**enterkeyhint** `<value>` \u{2014} Enter key label (`done`, `go`, `next`, `search`, `send`)."
        }
        "fetchpriority" => {
            "**fetchpriority** `<value>` \u{2014} Resource fetch priority (`high`, `low`, `auto`)."
        }
        "translate" => {
            "**translate** `<value>` \u{2014} Whether element should be translated (`yes`, `no`)."
        }
        "spellcheck" => "**spellcheck** `<value>` \u{2014} Spell check mode (`true`, `false`).",
        "hidden" => "**hidden** \u{2014} Hide element (`display: none`).",
        "overflow-x" => {
            "**overflow-x** `<value>` \u{2014} Horizontal overflow (`hidden`, `scroll`, `auto`, `visible`)."
        }
        "overflow-y" => {
            "**overflow-y** `<value>` \u{2014} Vertical overflow (`hidden`, `scroll`, `auto`, `visible`)."
        }
        "inset" => {
            "**inset** `<value>` \u{2014} Shorthand for `top`, `right`, `bottom`, `left`. Maps to CSS `inset`."
        }
        "accent-color" => {
            "**accent-color** `<color>` \u{2014} Accent color for form controls (checkboxes, radios, range)."
        }
        "caret-color" => "**caret-color** `<color>` \u{2014} Color of the text input cursor.",
        "list-style" => {
            "**list-style** `<value>` \u{2014} List style type (`disc`, `circle`, `square`, `decimal`, `none`)."
        }
        "border-collapse" => {
            "**border-collapse** `<value>` \u{2014} Table border model (`collapse`, `separate`)."
        }
        "border-spacing" => {
            "**border-spacing** `<value>` \u{2014} Spacing between table cell borders (when `border-collapse: separate`)."
        }
        "text-decoration" => {
            "**text-decoration** `<value>` \u{2014} Text decoration (`underline`, `overline`, `line-through`, `none`)."
        }
        "text-decoration-color" => {
            "**text-decoration-color** `<color>` \u{2014} Color of text decoration."
        }
        "text-decoration-thickness" => {
            "**text-decoration-thickness** `<value>` \u{2014} Thickness of text decoration."
        }
        "text-decoration-style" => {
            "**text-decoration-style** `<value>` \u{2014} Style of text decoration (`solid`, `dashed`, `dotted`, `wavy`, `double`)."
        }
        "place-items" => {
            "**place-items** `<value>` \u{2014} Shorthand for `align-items` and `justify-items`."
        }
        "place-self" => {
            "**place-self** `<value>` \u{2014} Shorthand for `align-self` and `justify-self`."
        }
        "scroll-behavior" => {
            "**scroll-behavior** `<value>` \u{2014} Scroll behavior (`smooth`, `auto`)."
        }
        "resize" => {
            "**resize** `<value>` \u{2014} Resize behavior (`none`, `both`, `horizontal`, `vertical`)."
        }
        // New CSS attributes
        "clip-path" => {
            "**clip-path** `<value>` \u{2014} Clip path (`circle()`, `polygon()`, `inset()`, `url()`)."
        }
        "mix-blend-mode" => {
            "**mix-blend-mode** `<value>` \u{2014} Blend mode (`multiply`, `screen`, `overlay`, `darken`, `lighten`)."
        }
        "background-blend-mode" => {
            "**background-blend-mode** `<value>` \u{2014} Background blend mode for layered backgrounds."
        }
        "writing-mode" => {
            "**writing-mode** `<value>` \u{2014} Writing direction (`horizontal-tb`, `vertical-rl`, `vertical-lr`)."
        }
        "column-count" => {
            "**column-count** `<value>` \u{2014} Number of columns in multi-column layout."
        }
        "column-gap" => {
            "**column-gap** `<value>` \u{2014} Gap between columns. Supports CSS units."
        }
        "text-indent" => {
            "**text-indent** `<value>` \u{2014} Indentation of the first line of text."
        }
        "hyphens" => {
            "**hyphens** `<value>` \u{2014} Hyphenation behavior (`none`, `manual`, `auto`)."
        }
        "flex-grow" => {
            "**flex-grow** `<value>` \u{2014} Flex grow factor (number). Controls how much an item grows."
        }
        "flex-shrink" => {
            "**flex-shrink** `<value>` \u{2014} Flex shrink factor (number). Controls how much an item shrinks."
        }
        "flex-basis" => {
            "**flex-basis** `<value>` \u{2014} Initial main size of a flex item (e.g., `200px`, `auto`, `0`)."
        }
        "isolation" => {
            "**isolation** `<value>` \u{2014} Creates a new stacking context (`isolate`, `auto`)."
        }
        "place-content" => {
            "**place-content** `<value>` \u{2014} Shorthand for `align-content` and `justify-content`."
        }
        "background-image" => {
            "**background-image** `<value>` \u{2014} Background image (`url()` or gradient function)."
        }
        // New CSS properties (batch 2)
        "font-weight" => {
            "**font-weight** `<value>` \u{2014} Font weight (`100`\u{2013}`900`, `bold`, `lighter`, `bolder`). More precise than `bold`."
        }
        "font-style" => {
            "**font-style** `<value>` \u{2014} Font style (`normal`, `italic`, `oblique`). More precise than `italic`."
        }
        "text-wrap" => {
            "**text-wrap** `<value>` \u{2014} Text wrapping behavior (`balance`, `pretty`, `nowrap`, `wrap`). `balance` is great for headings."
        }
        "will-change" => {
            "**will-change** `<value>` \u{2014} Performance hint for upcoming changes (`transform`, `opacity`, `scroll-position`)."
        }
        "touch-action" => {
            "**touch-action** `<value>` \u{2014} Touch behavior for mobile (`none`, `pan-x`, `pan-y`, `manipulation`, `auto`)."
        }
        "vertical-align" => {
            "**vertical-align** `<value>` \u{2014} Vertical alignment for inline elements (`middle`, `top`, `bottom`, `baseline`, `text-top`)."
        }
        "contain" => {
            "**contain** `<value>` \u{2014} CSS containment for performance (`layout`, `paint`, `content`, `strict`, `size`)."
        }
        "content-visibility" => {
            "**content-visibility** `<value>` \u{2014} Content rendering optimization (`auto`, `visible`, `hidden`). `auto` enables lazy rendering."
        }
        "scroll-margin" => {
            "**scroll-margin** `<value>` \u{2014} Scroll margin (for scroll-snap and anchor link offsets). Supports CSS units."
        }
        "scroll-padding" => {
            "**scroll-padding** `<value>` \u{2014} Scroll padding for scroll-snap containers. Supports CSS units."
        }
        "content" => {
            "**content** `<value>` \u{2014} CSS content property. Use with `before:` or `after:` prefix.\n\nExample: `before:content \u{2192}, before:color red`"
        }
        // New elements
        "@iframe" => {
            "**@iframe** \u{2014} Embedded page\n\nRenders as `<iframe>`.\n\nUsage: `@iframe [width fill, height 400] https://example.com`\n\nAttributes: `sandbox`, `allow`, `allowfullscreen`"
        }
        "@output" => {
            "**@output** \u{2014} Form output\n\nRenders as `<output>`. Displays calculation results in forms.\n\nUsage: `@output [for a b] Result`"
        }
        "@canvas" => {
            "**@canvas** \u{2014} Drawing surface\n\nRenders as `<canvas>`. Use with `@raw` JavaScript for drawing.\n\nUsage: `@canvas [width 400, height 300, id myCanvas]`"
        }
        "@script" => {
            "**@script** \u{2014} Script\n\nRenders as `<script>`. Inline JavaScript.\n\nUsage: `@script console.log(\"hello\")`"
        }
        "@noscript" => {
            "**@noscript** \u{2014} NoScript fallback\n\nRenders as `<noscript>`. Shown when JavaScript is disabled.\n\nUsage: `@noscript Please enable JavaScript.`"
        }
        "@address" => {
            "**@address** \u{2014} Address\n\nRenders as `<address>`. Contact information for the nearest `@article` or `@body`.\n\nUsage: `@address hello@example.com`"
        }
        "@search" => {
            "**@search** \u{2014} Search\n\nRenders as `<search>`. Semantic container for search functionality.\n\nUsage:\n```\n@search\n  @input [type search, placeholder Search...]\n  @button Search\n```"
        }
        "@breadcrumb" => {
            "**@breadcrumb** \u{2014} Breadcrumb\n\nRenders as `<nav aria-label=\"Breadcrumb\">`. Semantic breadcrumb navigation.\n\nUsage:\n```\n@breadcrumb\n  @link / Home\n  @link /docs Docs\n  @text Current Page\n```"
        }
        // Convenience elements
        "@grid" => {
            "**@grid** \u{2014} CSS Grid container\n\nRenders as `<div>` with `display: grid`.\n\nUsage: `@grid [grid-cols 3, gap 20]`"
        }
        "@stack" => {
            "**@stack** \u{2014} Stack container\n\nRenders as `<div>` with `position: relative`. Children can be absolutely positioned on top of each other."
        }
        "@in-front" => {
            "**@in-front** \u{2014} Overlay layer\n\nRenders as `<div>` with `position: absolute; inset: 0`, painted on top of the parent's content. The parent automatically becomes a positioning context (`position: relative; isolation: isolate`).\n\nUsage:\n```\n@el [width 200, height 200, background blue]\n  Main content\n  @in-front\n    @text [color white] Overlay\n```"
        }
        "@behind" => {
            "**@behind** \u{2014} Background layer\n\nRenders as `<div>` with `position: absolute; inset: 0; z-index: -1`, painted behind the parent's content. The parent automatically becomes a positioning context (`position: relative; isolation: isolate`).\n\nUsage:\n```\n@el [padding 40]\n  @behind\n    @el [background #fef3c7, width fill, height fill]\n  Foreground text\n```"
        }
        "@spacer" => {
            "**@spacer** \u{2014} Flexible spacer\n\nRenders as `<div>` with `flex: 1`. Pushes siblings apart in flex containers."
        }
        "@badge" => {
            "**@badge** \u{2014} Badge\n\nRenders as `<span>` with pill shape, centered text.\n\nUsage: `@badge [background #3b82f6, color white] NEW`"
        }
        "@tooltip" => {
            "**@tooltip** \u{2014} Tooltip\n\nRenders as `<span>` with `title` attribute for native tooltips.\n\nUsage: `@tooltip [cursor help] Hover me`"
        }
        "@avatar" => {
            "**@avatar** \u{2014} Avatar\n\nRenders as `<div>` with circular shape, centered content, `overflow: hidden`.\n\nUsage:\n```\n@avatar [width 48, height 48, background #e5e7eb]\n  @image [object-fit cover] photo.jpg\n```"
        }
        "@carousel" => {
            "**@carousel** \u{2014} Horizontal carousel\n\nRenders as `<div>` with horizontal scroll-snap.\nChildren auto-receive `scroll-snap-align: start` and `flex-shrink: 0`.\n\nUsage:\n```\n@carousel [gap 16]\n  @el [width 300] Slide 1\n  @el [width 300] Slide 2\n```"
        }
        "@chip" => {
            "**@chip** \u{2014} Chip / pill\n\nRenders as `<span>` with rounded borders, inline-flex layout.\n\nUsage: `@chip [background #e5e7eb] Category`"
        }
        "@tag" => {
            "**@tag** \u{2014} Tag label\n\nRenders as `<span>` with subtle rounded rectangle, bold small text.\n\nUsage: `@tag [background #dbeafe, color #1e40af] v2.0`"
        }
        // CSS shorthands
        "truncate" => {
            "**truncate** \u{2014} Truncate with ellipsis\n\nShorthand for: `overflow: hidden; text-overflow: ellipsis; white-space: nowrap`\n\nUsage: `@text [max-width 200, truncate] Long text here...`"
        }
        "line-clamp" => {
            "**line-clamp** `<N>` \u{2014} Multi-line truncation\n\nClamps text to N lines with ellipsis.\n\nUsage: `@paragraph [line-clamp 3] Long paragraph...`"
        }
        "blur" => {
            "**blur** `<value>` \u{2014} Apply blur filter\n\nShorthand for `filter: blur(Npx)`.\n\nUsage: `[blur 4]` \u{2192} `filter: blur(4px)`"
        }
        "backdrop-blur" => {
            "**backdrop-blur** `<value>` \u{2014} Apply backdrop blur\n\nShorthand for `backdrop-filter: blur(Npx)`.\n\nUsage: `[backdrop-blur 10]` \u{2192} `backdrop-filter: blur(10px)`"
        }
        "direction" => "**direction** `<value>` \u{2014} Text direction (`ltr`, `rtl`).",
        // Grid areas
        "grid-template-areas" => {
            "**grid-template-areas** `<value>` \u{2014} Define named grid areas.\n\nUsage: `[grid, grid-template-areas \"header header\" \"sidebar main\"]`\n\nChildren use `grid-area` to place themselves."
        }
        "grid-area" => {
            "**grid-area** `<name>` \u{2014} Place element in a named grid area.\n\nUsage: `@el [grid-area header] ...`\n\nRequires parent with `grid-template-areas`."
        }
        // View transitions
        "view-transition-name" => {
            "**view-transition-name** `<name>` \u{2014} Assign a View Transition API name.\n\nEnables smooth transitions between pages.\n\nUsage: `@el [view-transition-name hero]`"
        }
        // Has pseudo-selector
        "has(" => {
            "**has(selector):** \u{2014} Parent selector pseudo-class.\n\nStyle an element based on its children.\n\nUsage: `@el [has(.active):background blue, has(img):padding 0]`\n\nGenerates CSS `:has()` selector."
        }
        // Critical
        "critical" => {
            "**critical** \u{2014} Mark element as above-fold.\n\nHint for build tools to prioritize this element's CSS."
        }
        // New CSS properties
        "text-underline-offset" => {
            "**text-underline-offset** `<value>` \u{2014} Offset of text underline from its default position. Supports CSS units.\n\nUsage: `[underline, text-underline-offset 4]`"
        }
        "column-width" => {
            "**column-width** `<value>` \u{2014} Ideal column width in multi-column layout. Browser determines column count.\n\nUsage: `[column-width 200]`"
        }
        "column-rule" => {
            "**column-rule** `<value>` \u{2014} Rule between columns (shorthand for width, style, color).\n\nUsage: `[column-rule 1px solid #ccc]`"
        }
        // Variable filters
        "\\$|uppercase" => {
            "**|uppercase** \u{2014} Convert variable to UPPERCASE.\n\nUsage: `$name|uppercase`"
        }
        "\\$|lowercase" => {
            "**|lowercase** \u{2014} Convert variable to lowercase.\n\nUsage: `$name|lowercase`"
        }
        "\\$|capitalize" => {
            "**|capitalize** \u{2014} Capitalize first letter.\n\nUsage: `$name|capitalize`"
        }
        "\\$|trim" => {
            "**|trim** \u{2014} Strip leading/trailing whitespace.\n\nUsage: `$name|trim`"
        }
        "\\$|length" => "**|length** \u{2014} Get string length.\n\nUsage: `$name|length`",
        "\\$|reverse" => "**|reverse** \u{2014} Reverse string.\n\nUsage: `$name|reverse`",
        _ => return None,
    };

    Some(if let Some(state) = state {
        format!("*({} state)* {}", state, doc)
    } else {
        doc.to_string()
    })
}
