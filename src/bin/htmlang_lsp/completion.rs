use tower_lsp::lsp_types::*;

pub(crate) fn completions(text: &str, position: Position) -> Vec<CompletionItem> {
    let lines: Vec<&str> = text.lines().collect();
    let line = match lines.get(position.line as usize) {
        Some(l) => *l,
        None => return vec![],
    };

    let col = (position.character as usize).min(line.len());
    let before = &line[..col];

    let word_start = find_word_start(before);
    let edit_range = Range::new(Position::new(position.line, word_start as u32), position);

    // Inside attribute brackets?
    if in_brackets(before) {
        let current_word = &before[word_start..];

        // $ variable/define reference
        if current_word.starts_with('$') {
            return variable_completions(text, edit_range);
        }

        // State prefix (hover:, active:, focus:) or media prefix (dark:, print:)
        if let Some(colon) = current_word.find(':') {
            let prefix = &current_word[..colon];
            if matches!(
                prefix,
                "hover"
                    | "active"
                    | "focus"
                    | "focus-visible"
                    | "focus-within"
                    | "disabled"
                    | "checked"
                    | "placeholder"
                    | "first"
                    | "last"
                    | "odd"
                    | "even"
                    | "before"
                    | "after"
                    | "dark"
                    | "print"
                    | "sm"
                    | "md"
                    | "lg"
                    | "xl"
                    | "2xl"
                    | "motion-safe"
                    | "motion-reduce"
                    | "landscape"
                    | "portrait"
            ) {
                return state_attr_completions(prefix, edit_range);
            }
        }

        // Attribute-value enums for `attr <value>` patterns (e.g. type, cursor).
        if let Some(values) = attr_value_completions(before, edit_range) {
            return values;
        }

        // Color value completions after color-related attributes.
        if let Some(colors) = color_value_completions(before, edit_range) {
            return colors;
        }

        let element = owning_element(text, position);
        return attr_completions(edit_range, element.as_deref());
    }

    // $ variable reference outside brackets
    let current_word = &before[word_start..];
    if current_word.starts_with('$') {
        return variable_completions(text, edit_range);
    }

    // @ element/directive or start of line
    let trimmed = before.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('@') {
        let mut items = element_completions(edit_range);
        items.extend(directive_completions(edit_range));
        items.extend(function_completions(text, edit_range));
        items.extend(snippet_completions(edit_range));
        return items;
    }

    vec![]
}

pub(crate) fn find_word_start(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        let c = bytes[i - 1];
        if c.is_ascii_alphanumeric()
            || c == b'@'
            || c == b'$'
            || c == b'-'
            || c == b'_'
            || c == b':'
        {
            i -= 1;
        } else {
            break;
        }
    }
    i
}

pub(crate) fn in_brackets(text: &str) -> bool {
    let mut depth: i32 = 0;
    for ch in text.chars() {
        if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
        }
    }
    depth > 0
}

/// Extract design tokens from any `@theme` blocks in the source.
///
/// `@theme` opens a block whose indented children are `name value` pairs.
/// Each pair becomes both a `$variable` reference and a CSS custom property
/// at codegen time. We surface them as completions and hover targets.
pub(crate) fn collect_theme_tokens(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed == "@theme" || trimmed.starts_with("@theme ") {
            let header_indent = line.len() - trimmed.len();
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    j += 1;
                    continue;
                }
                let indent = l.len() - l.trim_start().len();
                if indent <= header_indent {
                    break;
                }
                let body = l.trim();
                if let Some((name, value)) = body.split_once(char::is_whitespace) {
                    out.push((name.trim().to_string(), value.trim().to_string()));
                }
                j += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn item(
    label: &str,
    kind: CompletionItemKind,
    detail: &str,
    insert: &str,
    range: Range,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: Some(detail.to_string()),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
            range,
            new_text: insert.to_string(),
        })),
        ..Default::default()
    }
}

fn element_completions(range: Range) -> Vec<CompletionItem> {
    [
        ("@row", "Horizontal layout (flexbox row)"),
        ("@column", "Vertical layout (flexbox column)"),
        ("@el", "Generic container"),
        ("@text", "Styled inline text (span)"),
        ("@paragraph", "Flowing text block (p)"),
        ("@image", "Image element"),
        ("@link", "Anchor/link element"),
        ("@input", "Form input element (self-closing)"),
        ("@button", "Button element"),
        ("@select", "Select dropdown element"),
        ("@textarea", "Multi-line text input"),
        ("@option", "Option inside @select"),
        ("@label", "Label element"),
        ("@raw", "Raw HTML escape hatch"),
        ("@children", "Slot for caller's children (inside component)"),
        ("@slot", "Named slot inside component (e.g., @slot header)"),
        // Semantic elements
        ("@nav", "Navigation container (nav)"),
        ("@header", "Page/section header (header)"),
        ("@footer", "Page/section footer (footer)"),
        ("@main", "Main content area (main)"),
        ("@section", "Thematic section (section)"),
        ("@article", "Self-contained content (article)"),
        ("@aside", "Sidebar/tangential content (aside)"),
        // List elements
        ("@list", "List container (ul/ol, use [ordered] for ol)"),
        ("@item", "List item (li)"),
        // Table elements
        ("@table", "Table element"),
        ("@thead", "Table head group"),
        ("@tbody", "Table body group"),
        ("@tr", "Table row"),
        ("@td", "Table cell"),
        ("@th", "Table header cell"),
        // Media elements
        ("@video", "Video element"),
        ("@audio", "Audio element"),
        // Additional elements
        ("@form", "Form container (form)"),
        ("@details", "Disclosure widget (details)"),
        ("@summary", "Summary for @details"),
        ("@blockquote", "Block quotation"),
        ("@cite", "Citation/source reference"),
        ("@code", "Inline code (monospace)"),
        ("@pre", "Preformatted text block"),
        ("@hr", "Horizontal rule/divider"),
        ("@figure", "Figure with optional caption"),
        ("@figcaption", "Caption for @figure"),
        ("@progress", "Progress bar (value, max attributes)"),
        ("@meter", "Meter/gauge element (value, min, max)"),
        ("@fragment", "Group children without a wrapper element"),
        // Dialog & interactive
        ("@dialog", "Dialog/modal element (dialog)"),
        // Definition lists
        ("@dl", "Description list (dl)"),
        ("@dt", "Description term (dt)"),
        ("@dd", "Description details (dd)"),
        // Form grouping
        ("@fieldset", "Fieldset group (fieldset)"),
        ("@legend", "Legend for @fieldset (legend)"),
        // Picture/responsive images
        ("@picture", "Responsive image container (picture)"),
        (
            "@source",
            "Media source for @picture/@video/@audio (source)",
        ),
        // Heading elements
        ("@h1", "Heading level 1 (h1)"),
        ("@h2", "Heading level 2 (h2)"),
        ("@h3", "Heading level 3 (h3)"),
        ("@h4", "Heading level 4 (h4)"),
        ("@h5", "Heading level 5 (h5)"),
        ("@h6", "Heading level 6 (h6)"),
        // Inline semantics
        ("@time", "Date/time element (time)"),
        ("@mark", "Highlighted/marked text (mark)"),
        ("@kbd", "Keyboard input (kbd)"),
        ("@abbr", "Abbreviation (abbr)"),
        // Datalist
        ("@datalist", "Predefined options for @input (datalist)"),
        // New elements
        ("@iframe", "Embedded external page (iframe)"),
        ("@output", "Form calculation result (output)"),
        ("@canvas", "Drawing surface for scripts (canvas)"),
        ("@script", "Inline script element (script)"),
        (
            "@noscript",
            "Fallback content when scripts disabled (noscript)",
        ),
        ("@address", "Contact information (address)"),
        ("@search", "Search section (search)"),
        ("@breadcrumb", "Breadcrumb navigation (nav with aria)"),
        // Overlay positioning
        (
            "@in-front",
            "Overlay layer rendered on top of the parent (absolute, inset 0)",
        ),
        (
            "@behind",
            "Background layer rendered behind the parent's content (absolute, inset 0, z-index -1)",
        ),
    ]
    .iter()
    .map(|(name, detail)| item(name, CompletionItemKind::KEYWORD, detail, name, range))
    .collect()
}

fn directive_completions(range: Range) -> Vec<CompletionItem> {
    [
        ("@page", "Set HTML page title", "@page "),
        (
            "@let",
            "Define a variable, attribute bundle, or component",
            "@let ",
        ),
        ("@keyframes", "Define a CSS animation", "@keyframes "),
        ("@if", "Conditional rendering", "@if "),
        ("@else if", "Else-if branch", "@else if "),
        ("@else", "Else branch", "@else"),
        (
            "@each",
            "Loop over values (@each $var, $i in list)",
            "@each ",
        ),
        (
            "@include",
            "Include another .hl file (DOM + definitions)",
            "@include ",
        ),
        (
            "@import",
            "Import definitions only (no DOM nodes)",
            "@import ",
        ),
        ("@meta", "Add a <meta> tag to <head>", "@meta "),
        ("@head", "Add raw content to <head>", "@head"),
        ("@style", "Add raw CSS to stylesheet", "@style"),
        (
            "@slot",
            "Named slot in component for caller content",
            "@slot ",
        ),
        ("@match", "Pattern matching on a value", "@match "),
        ("@case", "Match case (inside @match)", "@case "),
        ("@default", "Default case (inside @match)", "@default"),
        ("@warn", "Emit a compile-time warning", "@warn "),
        (
            "@debug",
            "Print debug message during compilation",
            "@debug ",
        ),
        (
            "@lang",
            "Set document language (html lang attribute)",
            "@lang ",
        ),
        (
            "@favicon",
            "Set favicon (inlined as base64 data URI)",
            "@favicon ",
        ),
        ("@og", "Add Open Graph meta tag", "@og "),
        (
            "@breakpoint",
            "Define custom responsive breakpoint",
            "@breakpoint ",
        ),
        (
            "@theme",
            "Define design tokens (colors, spacing, fonts)",
            "@theme",
        ),
        (
            "@deprecated",
            "Mark next @let component as deprecated",
            "@deprecated ",
        ),
        (
            "@extends",
            "Inherit a layout template and fill @slot blocks",
            "@extends ",
        ),
        (
            "@canonical",
            "Set canonical URL for the page",
            "@canonical ",
        ),
        ("@base", "Set base URL for relative links", "@base "),
        ("@font-face", "Define a custom font face", "@font-face"),
        (
            "@json-ld",
            "Add JSON-LD structured data to head",
            "@json-ld",
        ),
        (
            "@assert",
            "Compile-time assertion for variable values",
            "@assert ",
        ),
        (
            "@data",
            "Load JSON data file into template variables",
            "@data ",
        ),
        ("@env", "Access compile-time environment variable", "@env "),
        ("@fetch", "Fetch data from URL at compile time", "@fetch "),
        ("@svg", "Inline SVG file with optional attributes", "@svg "),
    ]
    .iter()
    .map(|(name, detail, insert)| item(name, CompletionItemKind::SNIPPET, detail, insert, range))
    .collect()
}

fn snippet_completions(range: Range) -> Vec<CompletionItem> {
    let snippets: &[(&str, &str, &str)] = &[
        (
            "card component",
            "Reusable card with title and content",
            "@let card \\$title\n  @el [padding 20, background white, rounded 8]\n    @text [bold] \\$title\n    @children",
        ),
        (
            "responsive layout",
            "Centered responsive column layout",
            "@column [max-width 800, center-x, padding 40, spacing 20]",
        ),
        (
            "nav bar",
            "Navigation bar with horizontal items",
            "@nav [padding 16, background #1a1a2e]\n  @row [spacing 20, align-items center]",
        ),
        (
            "hero section",
            "Hero section with title and subtitle",
            "@column [padding 80, center-x, center-y, min-height 60vh]\n  @text [bold, size 48] ${1:Title}\n  @paragraph [size 18, color #666] ${2:Subtitle}",
        ),
        (
            "each with else",
            "Loop with empty-state fallback",
            "@each \\$${1:item} in ${2:list}\n  @text \\$${1:item}\n@else\n  @text [color #888] No items found.",
        ),
        (
            "button with hover",
            "Interactive button with hover effect",
            "@el [padding 12 24, background ${1:#3b82f6}, hover:background ${2:#2563eb}, rounded 8, cursor pointer, transition all 0.15s ease] > @link ${3:url}\n  @text [color white, bold] ${4:Click me}",
        ),
        (
            "form with inputs",
            "Form with labeled inputs and submit button",
            "@form [spacing 16]\n  @label ${1:Name}\n    @input [type text, name ${2:name}, placeholder ${3:Enter name}, required]\n  @label ${4:Email}\n    @input [type email, name ${5:email}, placeholder ${6:Enter email}, required]\n  @button [type submit, padding 12 24, background ${7:#3b82f6}, color white, rounded 8, bold, cursor pointer] Submit",
        ),
        (
            "grid layout",
            "Responsive grid with columns",
            "@grid [grid-cols ${1:3}, gap ${2:20}]\n  @el [padding 20, background ${3:#f3f4f6}, rounded 8]\n    ${4:Item 1}\n  @el [padding 20, background ${3:#f3f4f6}, rounded 8]\n    ${5:Item 2}\n  @el [padding 20, background ${3:#f3f4f6}, rounded 8]\n    ${6:Item 3}",
        ),
        (
            "footer section",
            "Footer with columns and copyright",
            "@footer [padding 40, background ${1:#1a1a2e}, color ${2:#ccc}]\n  @row [spacing 40, wrap]\n    @column [spacing 10, width fill]\n      @text [bold, color white] ${3:Company}\n      @link ${4:#} ${5:About}\n      @link ${6:#} ${7:Contact}\n    @column [spacing 10, width fill]\n      @text [bold, color white] ${8:Resources}\n      @link ${9:#} ${10:Documentation}\n  @text [size 14, color #888, center-x] \\u00a9 2026 ${11:Company Name}",
        ),
        (
            "avatar with image",
            "Circular avatar with fallback",
            "@avatar [width ${1:48}, height ${1:48}, background ${2:#e5e7eb}]\n  @image [width ${1:48}, height ${1:48}, object-fit cover, alt ${3:avatar}] ${4:url}",
        ),
        (
            "carousel horizontal",
            "Scroll-snap horizontal carousel",
            "@carousel [gap ${1:16}, padding ${2:16}]\n  @el [width ${3:300}, padding 20, background ${4:#f3f4f6}, rounded 8]\n    ${5:Slide 1}\n  @el [width ${3:300}, padding 20, background ${4:#f3f4f6}, rounded 8]\n    ${6:Slide 2}\n  @el [width ${3:300}, padding 20, background ${4:#f3f4f6}, rounded 8]\n    ${7:Slide 3}",
        ),
        (
            "dark mode toggle",
            "Element with light/dark mode styles",
            "@el [padding 20, background ${1:white}, dark:background ${2:#1a1a2e}, color ${3:#333}, dark:color ${4:#eee}, rounded 8, transition all 0.2s ease]\n  ${5:Content}",
        ),
        (
            "truncated text",
            "Text with ellipsis overflow",
            "@text [max-width ${1:200}, truncate] ${2:Long text that will be truncated...}",
        ),
        (
            "@let component",
            "Define a reusable component",
            "@let ${1:name} \\$${2:param}\n  @el [${3:padding 16}]\n    @children",
        ),
        (
            "@each loop",
            "Iterate over a list of items",
            "@each \\$${1:item} in ${2:items}\n  @text \\$${1:item}",
        ),
        (
            "@if conditional",
            "Conditional rendering block",
            "@if ${1:condition}\n  ${2:content}\n@else\n  ${3:fallback}",
        ),
        (
            "@match pattern",
            "Pattern matching block",
            "@match \\$${1:value}\n  @case ${2:option1}\n    ${3:content}\n  @default\n    ${4:fallback}",
        ),
        (
            "@data JSON load",
            "Load variables from a JSON file",
            "@data ${1:data.json}",
        ),
        (
            "@component scoped",
            "Define a scoped component with styles",
            "@component ${1:name} \\$${2:param}\n  @style\n    .inner { ${3:padding: 16px;} }\n  @el [class inner]\n    @children",
        ),
    ];

    snippets
        .iter()
        .map(|(label, detail, insert)| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(detail.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: insert.to_string(),
            })),
            sort_text: Some(format!("zz_{}", label)),
            ..Default::default()
        })
        .collect()
}

pub(crate) fn path_completions(uri: &Url, position: Position) -> Vec<CompletionItem> {
    let file_path = match uri.to_file_path() {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    let dir = match file_path.parent() {
        Some(d) => d,
        None => return vec![],
    };

    let col = position.character;
    let edit_range = Range::new(
        Position::new(position.line, col),
        Position::new(position.line, col),
    );

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("hl")
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            // Skip the current file itself
            if Some(name) == file_path.file_name().and_then(|n| n.to_str()) {
                continue;
            }
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FILE),
                detail: Some("htmlang file".to_string()),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: edit_range,
                    new_text: name.to_string(),
                })),
                ..Default::default()
            });
        }
    }
    items
}


/// Walk back from `position` to find the element directive that opened the
/// nearest unmatched `[`. Returns the bare name without the leading `@`
/// (e.g. `"input"`).
pub(crate) fn owning_element(text: &str, position: Position) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    // First, locate the line that contains the unmatched `[`. We scan from
    // the cursor back, tracking depth.
    let cursor_line = position.line as usize;
    let cursor_col = (position.character as usize)
        .min(lines.get(cursor_line).map(|l| l.len()).unwrap_or(0));
    let mut depth: i32 = 0;
    let mut bracket_line: Option<usize> = None;
    let mut bracket_col: usize = 0;
    'outer: for line_idx in (0..=cursor_line).rev() {
        let line = lines[line_idx];
        let last = if line_idx == cursor_line {
            cursor_col
        } else {
            line.len()
        };
        for (col, ch) in line[..last].char_indices().rev() {
            match ch {
                ']' => depth += 1,
                '[' => {
                    if depth == 0 {
                        bracket_line = Some(line_idx);
                        bracket_col = col;
                        break 'outer;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    let line_idx = bracket_line?;
    let line = lines[line_idx];
    // The owning element should appear on the same line as the `[`. Look
    // backwards for an `@name` token before the bracket. Anything else
    // (e.g. `$bundle [...]`) doesn't bind to a builtin element.
    let prefix = &line[..bracket_col];
    let at_pos = prefix.rfind('@')?;
    let after_at = &prefix[at_pos + 1..];
    let name_end = after_at
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .unwrap_or(after_at.len());
    let name = &after_at[..name_end];
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Attributes the LSP knows are specifically meaningful for a given element.
/// Universal styling attributes (padding, color, etc.) aren't listed here —
/// they remain available to every element via `attr_completions`.
fn element_specific_attrs(element: &str) -> &'static [&'static str] {
    match element {
        "input" => &[
            "type",
            "name",
            "value",
            "placeholder",
            "required",
            "disabled",
            "checked",
            "readonly",
            "pattern",
            "min",
            "max",
            "step",
            "multiple",
            "maxlength",
            "minlength",
            "autofocus",
            "autocomplete",
            "inputmode",
            "spellcheck",
            "list",
            "accept",
        ],
        "button" | "btn" => &[
            "type",
            "disabled",
            "name",
            "value",
            "autofocus",
            "popovertarget",
            "popovertargetaction",
        ],
        "select" => &["name", "multiple", "required", "disabled", "size", "autofocus"],
        "textarea" => &[
            "name",
            "rows",
            "cols",
            "placeholder",
            "required",
            "disabled",
            "readonly",
            "maxlength",
            "minlength",
            "wrap",
            "autofocus",
            "spellcheck",
        ],
        "option" | "opt" => &["value", "selected", "disabled", "label"],
        "form" => &[
            "action",
            "method",
            "novalidate",
            "target",
            "autocomplete",
            "enctype",
            "name",
        ],
        "image" | "img" => &[
            "src",
            "alt",
            "width",
            "height",
            "loading",
            "decoding",
            "fetchpriority",
            "srcset",
            "sizes",
        ],
        "link" => &["href", "target", "rel", "download", "referrerpolicy", "type"],
        "video" => &[
            "src",
            "controls",
            "autoplay",
            "loop",
            "muted",
            "poster",
            "preload",
            "width",
            "height",
            "playsinline",
        ],
        "audio" => &["src", "controls", "autoplay", "loop", "muted", "preload"],
        "iframe" => &[
            "src",
            "width",
            "height",
            "sandbox",
            "allow",
            "allowfullscreen",
            "loading",
            "referrerpolicy",
        ],
        "td" | "th" => &["colspan", "rowspan", "scope"],
        "meter" => &["value", "min", "max", "low", "high", "optimum"],
        "progress" => &["value", "max"],
        "details" => &["open"],
        "dialog" => &["open"],
        "list" => &["ordered", "type", "start"],
        "time" => &["datetime"],
        "abbr" => &["title"],
        "label" => &["for"],
        "picture" | "source" => &["src", "srcset", "sizes", "media", "type"],
        "meta" => &["name", "content", "charset"],
        _ => &[],
    }
}

/// Attributes whose value is a closed enum (e.g. `cursor`, `text-align`).
/// Returns the list of valid values when `before` ends with the attribute
/// name plus a single space and no value yet typed.
fn attr_value_completions(before: &str, range: Range) -> Option<Vec<CompletionItem>> {
    let bracket_content = before.rsplit('[').next()?;
    let segment = bracket_content.rsplit(',').next()?.trim_start();
    let attr_end = segment
        .find(|c: char| c.is_whitespace())
        .unwrap_or(segment.len());
    let attr = &segment[..attr_end];
    let after_attr = &segment[attr_end..];
    if !after_attr.starts_with(' ') {
        return None;
    }
    // Only fire when no value has been started yet.
    let typed = after_attr.trim_start();
    if !typed.is_empty() {
        return None;
    }

    // Strip state prefix to find the base attribute.
    let base_attr = if let Some(pos) = attr.rfind(':') {
        &attr[pos + 1..]
    } else {
        attr
    };

    let values: &[&str] = match base_attr {
        "type" => &[
            "text",
            "email",
            "password",
            "submit",
            "button",
            "reset",
            "checkbox",
            "radio",
            "file",
            "hidden",
            "number",
            "range",
            "search",
            "tel",
            "url",
            "date",
            "datetime-local",
            "month",
            "time",
            "week",
            "color",
        ],
        "cursor" => &[
            "auto",
            "default",
            "pointer",
            "text",
            "wait",
            "help",
            "not-allowed",
            "crosshair",
            "move",
            "grab",
            "grabbing",
            "zoom-in",
            "zoom-out",
            "ew-resize",
            "ns-resize",
            "nesw-resize",
            "nwse-resize",
        ],
        "text-align" => &["left", "center", "right", "justify", "start", "end"],
        "text-transform" => &["uppercase", "lowercase", "capitalize", "none"],
        "white-space" => &["normal", "nowrap", "pre", "pre-line", "pre-wrap", "break-spaces"],
        "overflow" | "overflow-x" | "overflow-y" => {
            &["visible", "hidden", "scroll", "auto", "clip"]
        }
        "position" => &["static", "relative", "absolute", "fixed", "sticky"],
        "display" => &[
            "block",
            "inline",
            "inline-block",
            "flex",
            "inline-flex",
            "grid",
            "inline-grid",
            "none",
            "contents",
            "list-item",
            "table",
        ],
        "visibility" => &["visible", "hidden", "collapse"],
        "justify-content" => &[
            "flex-start",
            "center",
            "flex-end",
            "space-between",
            "space-around",
            "space-evenly",
            "start",
            "end",
        ],
        "align-items" => &[
            "stretch",
            "flex-start",
            "center",
            "flex-end",
            "baseline",
            "start",
            "end",
        ],
        "align-self" => &[
            "auto",
            "stretch",
            "flex-start",
            "center",
            "flex-end",
            "baseline",
        ],
        "object-fit" => &["fill", "contain", "cover", "none", "scale-down"],
        "loading" => &["lazy", "eager"],
        "decoding" => &["async", "sync", "auto"],
        "preload" => &["auto", "metadata", "none"],
        "method" => &["get", "post", "dialog"],
        "target" => &["_self", "_blank", "_parent", "_top"],
        "scope" => &["row", "col", "rowgroup", "colgroup"],
        "wrap" => &["soft", "hard", "off"],
        "inputmode" => &[
            "text",
            "numeric",
            "decimal",
            "email",
            "search",
            "tel",
            "url",
            "none",
        ],
        "enterkeyhint" => &["enter", "done", "go", "next", "previous", "search", "send"],
        "fetchpriority" => &["high", "low", "auto"],
        "spellcheck" | "translate" => &["true", "false"],
        "color-scheme" => &["light", "dark", "light dark", "normal"],
        "appearance" => &["none", "auto"],
        "autocomplete" => &["on", "off", "name", "email", "username", "current-password", "new-password"],
        "scroll-behavior" => &["smooth", "auto"],
        "resize" => &["none", "both", "horizontal", "vertical", "block", "inline"],
        "writing-mode" => &["horizontal-tb", "vertical-rl", "vertical-lr"],
        "direction" => &["ltr", "rtl"],
        "list-style" => &["disc", "circle", "square", "decimal", "none"],
        "border-collapse" => &["collapse", "separate"],
        "text-decoration" => &["none", "underline", "overline", "line-through"],
        "text-decoration-style" => &["solid", "double", "dotted", "dashed", "wavy"],
        "text-wrap" => &["wrap", "nowrap", "balance", "pretty", "stable"],
        "font-style" => &["normal", "italic", "oblique"],
        "font-weight" => &[
            "100", "200", "300", "400", "500", "600", "700", "800", "900", "normal", "bold",
            "lighter", "bolder",
        ],
        "vertical-align" => &[
            "baseline",
            "top",
            "middle",
            "bottom",
            "text-top",
            "text-bottom",
            "sub",
            "super",
        ],
        "user-select" => &["none", "auto", "text", "all", "contain"],
        "pointer-events" => &["none", "auto"],
        "popovertargetaction" => &["toggle", "show", "hide"],
        "popover" => &["auto", "manual"],
        "hyphens" => &["none", "manual", "auto"],
        "isolation" => &["auto", "isolate"],
        "touch-action" => &[
            "none",
            "pan-x",
            "pan-y",
            "manipulation",
            "auto",
            "pinch-zoom",
        ],
        "contain" => &["none", "strict", "content", "size", "layout", "style", "paint"],
        "content-visibility" => &["visible", "auto", "hidden"],
        _ => return None,
    };

    Some(
        values
            .iter()
            .map(|v| CompletionItem {
                label: v.to_string(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: Some(format!("value for {}", base_attr)),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: v.to_string(),
                })),
                ..Default::default()
            })
            .collect(),
    )
}

fn attr_completions(range: Range, element: Option<&str>) -> Vec<CompletionItem> {
    [
        // Layout
        ("spacing", "Gap between children (supports CSS units)", true),
        ("gap", "Gap between children (alias for spacing)", true),
        (
            "padding",
            "Inner padding (1/2/3/4 values, supports CSS units)",
            true,
        ),
        ("padding-x", "Horizontal padding", true),
        ("padding-y", "Vertical padding", true),
        // Sizing
        ("width", "Width (px/% | fill | shrink)", true),
        ("height", "Height (px/% | fill | shrink)", true),
        ("min-width", "Minimum width", true),
        ("max-width", "Maximum width", true),
        ("min-height", "Minimum height", true),
        ("max-height", "Maximum height", true),
        // Alignment
        ("center-x", "Center horizontally", false),
        ("center-y", "Center vertically", false),
        ("align-left", "Align left", false),
        ("align-right", "Align right", false),
        ("align-top", "Align top", false),
        ("align-bottom", "Align bottom", false),
        // Style
        ("background", "Background color/value", true),
        ("color", "Text color", true),
        ("border", "Border (width [color])", true),
        ("border-top", "Top border (width [color])", true),
        ("border-bottom", "Bottom border (width [color])", true),
        ("border-left", "Left border (width [color])", true),
        ("border-right", "Right border (width [color])", true),
        ("rounded", "Border radius", true),
        ("bold", "Bold text", false),
        ("italic", "Italic text", false),
        ("underline", "Underlined text", false),
        ("size", "Font size", true),
        ("font", "Font family", true),
        ("transition", "CSS transition", true),
        ("cursor", "CSS cursor type", true),
        ("opacity", "Opacity (0-1)", true),
        // Typography
        (
            "text-align",
            "Text alignment (left/center/right/justify)",
            true,
        ),
        ("line-height", "Line height (unitless or px)", true),
        ("letter-spacing", "Letter spacing", true),
        (
            "text-transform",
            "Text transform (uppercase/lowercase/capitalize)",
            true,
        ),
        (
            "white-space",
            "White-space behavior (nowrap/pre/normal)",
            true,
        ),
        // Overflow & positioning
        ("overflow", "Overflow behavior (hidden/scroll/auto)", true),
        (
            "position",
            "Position type (relative/absolute/fixed/sticky)",
            true,
        ),
        ("top", "Top offset (for positioned elements)", true),
        ("right", "Right offset (for positioned elements)", true),
        ("bottom", "Bottom offset (for positioned elements)", true),
        ("left", "Left offset (for positioned elements)", true),
        ("z-index", "Stack order (integer)", true),
        // Display & visibility
        (
            "display",
            "Display mode (none/block/inline/flex/grid)",
            true,
        ),
        ("visibility", "Visibility (visible/hidden)", true),
        // Transform & filters
        ("transform", "CSS transform (e.g., rotate(45deg))", true),
        (
            "backdrop-filter",
            "Backdrop filter (e.g., blur(10px))",
            true,
        ),
        // Effects
        ("shadow", "Box shadow (CSS value)", true),
        // Flow
        ("wrap", "Enable flex-wrap", false),
        ("gap-x", "Horizontal gap between children", true),
        ("gap-y", "Vertical gap between children", true),
        // Grid
        ("grid", "Enable CSS grid layout", false),
        ("grid-cols", "Grid columns (number or template)", true),
        ("grid-rows", "Grid rows (number or template)", true),
        ("col-span", "Span columns in grid", true),
        ("row-span", "Span rows in grid", true),
        // Container queries
        ("container", "Enable container queries (inline-size)", false),
        (
            "container-name",
            "Container name for @container queries",
            true,
        ),
        (
            "container-type",
            "Container type (inline-size/size/normal)",
            true,
        ),
        // Identity
        ("id", "HTML id attribute", true),
        ("class", "HTML class attribute", true),
        // Animation
        ("animation", "CSS animation (e.g., name 0.3s ease)", true),
        // Form
        ("type", "Input type (text/email/password/submit/...)", true),
        ("placeholder", "Placeholder text", true),
        ("name", "Form field name", true),
        ("value", "Form field value", true),
        ("disabled", "Disable the element", false),
        ("required", "Mark field as required", false),
        ("checked", "Checkbox/radio checked state", false),
        ("for", "Label target (id of associated input)", true),
        ("action", "Form action URL", true),
        ("method", "Form method (get/post)", true),
        ("rows", "Textarea rows", true),
        ("cols", "Textarea columns", true),
        ("maxlength", "Maximum input length", true),
        ("multiple", "Allow multiple selections", false),
        // Accessibility
        ("alt", "Alternative text (for images)", true),
        ("role", "ARIA role", true),
        ("tabindex", "Tab order", true),
        ("title", "Tooltip text", true),
        ("aria-label", "Accessible label", true),
        ("aria-hidden", "Hide from assistive tech (true/false)", true),
        ("data-", "Custom data attribute", true),
        // CSS: aspect-ratio, outline, logical properties, scroll-snap
        ("aspect-ratio", "CSS aspect ratio (e.g., 16/9, 1)", true),
        ("outline", "Outline (width [color])", true),
        (
            "padding-inline",
            "Inline (horizontal) padding for i18n",
            true,
        ),
        ("padding-block", "Block (vertical) padding for i18n", true),
        ("margin-inline", "Inline (horizontal) margin for i18n", true),
        ("margin-block", "Block (vertical) margin for i18n", true),
        (
            "scroll-snap-type",
            "Scroll snap behavior (x/y mandatory/proximity)",
            true,
        ),
        (
            "scroll-snap-align",
            "Snap alignment (start/center/end)",
            true,
        ),
        // Media attributes
        (
            "controls",
            "Show media controls (for @video, @audio)",
            false,
        ),
        ("autoplay", "Auto-play media", false),
        ("loop", "Loop media playback", false),
        ("muted", "Mute media", false),
        ("poster", "Video poster image URL", true),
        ("preload", "Media preload hint (auto/metadata/none)", true),
        ("loading", "Loading behavior (lazy/eager)", true),
        ("decoding", "Image decoding (async/sync/auto)", true),
        // List
        ("ordered", "Use ordered list (ol instead of ul)", false),
        // Media src
        ("src", "Source URL for media elements", true),
        // Margin
        ("margin", "Outer margin (1/2/3/4 values)", true),
        ("margin-x", "Horizontal margin", true),
        ("margin-y", "Vertical margin", true),
        // Filter & object
        (
            "filter",
            "CSS filter (blur, brightness, grayscale, etc.)",
            true,
        ),
        (
            "object-fit",
            "Object fit for images (cover/contain/fill)",
            true,
        ),
        ("object-position", "Object position within container", true),
        // Text extras
        ("text-shadow", "Text shadow (CSS value)", true),
        ("text-overflow", "Text overflow (ellipsis/clip)", true),
        // Interaction
        ("pointer-events", "Pointer events (none/auto)", true),
        ("user-select", "User selection (none/text/all)", true),
        // Flexbox/grid alignment
        (
            "justify-content",
            "Main axis alignment (center/space-between/etc.)",
            true,
        ),
        (
            "align-items",
            "Cross axis alignment (center/baseline/etc.)",
            true,
        ),
        // Flex item
        ("order", "Flex/grid item order", true),
        // Background extras
        (
            "background-size",
            "Background size (cover/contain/auto)",
            true,
        ),
        (
            "background-position",
            "Background position (center/top/etc.)",
            true,
        ),
        (
            "background-repeat",
            "Background repeat (no-repeat/repeat/etc.)",
            true,
        ),
        // Text wrapping
        (
            "word-break",
            "Word break behavior (break-all/keep-all)",
            true,
        ),
        ("overflow-wrap", "Overflow wrap (break-word/anywhere)", true),
        // New element attrs
        ("open", "Details initially open", false),
        ("novalidate", "Disable form validation", false),
        ("low", "Meter low threshold", true),
        ("high", "Meter high threshold", true),
        ("optimum", "Meter optimum value", true),
        ("colspan", "Table cell column span", true),
        ("rowspan", "Table cell row span", true),
        (
            "scope",
            "Table header scope (col/row/colgroup/rowgroup)",
            true,
        ),
        ("inline", "Inline SVG images into output", false),
        // Hidden
        ("hidden", "Hide element (display:none)", false),
        // Overflow directional
        (
            "overflow-x",
            "Horizontal overflow (hidden/scroll/auto)",
            true,
        ),
        ("overflow-y", "Vertical overflow (hidden/scroll/auto)", true),
        // Inset
        ("inset", "Shorthand for top/right/bottom/left", true),
        // Modern form theming
        ("accent-color", "Accent color for form controls", true),
        ("caret-color", "Text cursor color", true),
        (
            "color-scheme",
            "Color scheme preference (light/dark/light dark)",
            true,
        ),
        (
            "appearance",
            "Form element appearance (none to reset)",
            true,
        ),
        // Popover API
        (
            "popover",
            "Make element a popover (HTML Popover API)",
            false,
        ),
        ("popovertarget", "ID of popover to toggle", true),
        (
            "popovertargetaction",
            "Popover action (toggle/show/hide)",
            true,
        ),
        // Input hints
        (
            "inputmode",
            "Virtual keyboard type (numeric/email/search/tel/url)",
            true,
        ),
        (
            "enterkeyhint",
            "Enter key label (done/go/next/search/send)",
            true,
        ),
        (
            "fetchpriority",
            "Resource fetch priority (high/low/auto)",
            true,
        ),
        (
            "translate",
            "Whether element should be translated (yes/no)",
            true,
        ),
        ("spellcheck", "Spell check mode (true/false)", true),
        // List styling
        (
            "list-style",
            "List style type (disc/circle/square/none)",
            true,
        ),
        // Table styling
        (
            "border-collapse",
            "Border collapse mode (collapse/separate)",
            true,
        ),
        ("border-spacing", "Spacing between table cell borders", true),
        // Text decoration
        (
            "text-decoration",
            "Text decoration (underline/overline/line-through)",
            true,
        ),
        ("text-decoration-color", "Text decoration color", true),
        (
            "text-decoration-thickness",
            "Text decoration thickness",
            true,
        ),
        (
            "text-decoration-style",
            "Text decoration style (solid/dashed/dotted/wavy)",
            true,
        ),
        // Grid/flex placement
        (
            "place-items",
            "Shorthand for align-items + justify-items",
            true,
        ),
        (
            "place-self",
            "Shorthand for align-self + justify-self",
            true,
        ),
        // Scroll behavior
        ("scroll-behavior", "Scroll behavior (smooth/auto)", true),
        // Resize
        (
            "resize",
            "Resize behavior (none/both/horizontal/vertical)",
            true,
        ),
        // State prefixes
        ("hover:", "Style on hover", false),
        ("active:", "Style on active/click", false),
        ("focus:", "Style on focus", false),
        // New pseudo-state prefixes
        ("focus-visible:", "Style on keyboard focus", false),
        ("focus-within:", "Style when child has focus", false),
        ("disabled:", "Style when disabled", false),
        ("checked:", "Style when checked", false),
        ("placeholder:", "Style placeholder text", false),
        // Child selectors
        ("first:", "Style first child", false),
        ("last:", "Style last child", false),
        ("odd:", "Style odd children (1st, 3rd, ...)", false),
        ("even:", "Style even children (2nd, 4th, ...)", false),
        // Responsive prefixes
        ("sm:", "Style at 640px+ (small)", false),
        ("md:", "Style at 768px+ (medium)", false),
        ("lg:", "Style at 1024px+ (large)", false),
        ("xl:", "Style at 1280px+ (extra large)", false),
        // Additional responsive prefixes
        ("2xl:", "Style at 1536px+ (2x extra large)", false),
        // Motion prefixes
        ("motion-safe:", "Style when motion is allowed", false),
        (
            "motion-reduce:",
            "Style when reduced motion preferred",
            false,
        ),
        // Orientation prefixes
        ("landscape:", "Style in landscape orientation", false),
        ("portrait:", "Style in portrait orientation", false),
        // Media prefixes
        ("dark:", "Style in dark color scheme", false),
        ("print:", "Style for print media", false),
        // Clipping & blending
        ("clip-path", "Clip path (circle, polygon, etc.)", true),
        (
            "mix-blend-mode",
            "Blend mode (multiply, screen, overlay, etc.)",
            true,
        ),
        ("background-blend-mode", "Background blend mode", true),
        // Writing mode
        (
            "writing-mode",
            "Writing mode (horizontal-tb, vertical-rl, etc.)",
            true,
        ),
        // Multi-column layout
        (
            "column-count",
            "Number of columns in multi-column layout",
            true,
        ),
        ("column-gap", "Gap between columns", true),
        // Text
        ("text-indent", "First-line text indentation", true),
        ("hyphens", "Hyphenation behavior (none/manual/auto)", true),
        // Flex item sizing
        ("flex-grow", "Flex grow factor", true),
        ("flex-shrink", "Flex shrink factor", true),
        ("flex-basis", "Flex basis (initial main size)", true),
        // Stacking context
        ("isolation", "Create stacking context (isolate/auto)", true),
        // Grid/flex placement
        (
            "place-content",
            "Shorthand for align-content + justify-content",
            true,
        ),
        // Background image
        (
            "background-image",
            "Background image (url or gradient)",
            true,
        ),
        // New CSS properties
        (
            "font-weight",
            "Font weight (100-900, bold, lighter, bolder)",
            true,
        ),
        ("font-style", "Font style (normal/italic/oblique)", true),
        ("text-wrap", "Text wrapping (balance/pretty/nowrap)", true),
        (
            "will-change",
            "Performance hint for animations (transform, opacity)",
            true,
        ),
        (
            "touch-action",
            "Touch behavior (none/pan-x/pan-y/manipulation)",
            true,
        ),
        (
            "vertical-align",
            "Vertical alignment (middle/top/bottom/baseline)",
            true,
        ),
        (
            "contain",
            "CSS containment (layout/paint/content/strict)",
            true,
        ),
        (
            "content-visibility",
            "Content visibility (auto/visible/hidden)",
            true,
        ),
        (
            "scroll-margin",
            "Scroll margin (for scroll-snap and anchor offsets)",
            true,
        ),
        ("scroll-margin-top", "Scroll margin top", true),
        (
            "scroll-padding",
            "Scroll padding (for scroll-snap containers)",
            true,
        ),
        ("scroll-padding-top", "Scroll padding top", true),
        // Pseudo-element content
        (
            "content",
            "Content for ::before/::after (use with before:/after: prefix)",
            true,
        ),
        // Iframe/form/link attributes
        ("sandbox", "Iframe sandbox restrictions", true),
        ("allow", "Iframe permissions policy", true),
        ("allowfullscreen", "Allow iframe fullscreen", false),
        (
            "target",
            "Link/form target (_blank/_self/_parent/_top)",
            true,
        ),
        // CSS shorthands
        (
            "truncate",
            "Truncate text with ellipsis (single line)",
            false,
        ),
        ("line-clamp", "Clamp text to N lines with ellipsis", true),
        ("blur", "Apply blur filter (px)", true),
        ("backdrop-blur", "Apply backdrop blur filter (px)", true),
        (
            "no-scrollbar",
            "Hide scrollbar while keeping overflow",
            false,
        ),
        ("skeleton", "Add shimmer loading skeleton animation", false),
        ("gradient", "Linear gradient (color1 color2 [angle])", true),
        // Direction
        ("direction", "Text direction (ltr/rtl)", true),
        // Container query prefixes
        ("cq-sm:", "Container query at 640px+", false),
        ("cq-md:", "Container query at 768px+", false),
        ("cq-lg:", "Container query at 1024px+", false),
        // Pseudo-elements
        ("before:", "Style ::before pseudo-element", false),
        ("after:", "Style ::after pseudo-element", false),
        ("selection:", "Style text selection", false),
        // Grid areas
        (
            "grid-template-areas",
            "Define named grid areas (quoted string)",
            true,
        ),
        ("grid-area", "Place element in a named grid area", true),
        // View transitions
        (
            "view-transition-name",
            "Assign a view transition name",
            true,
        ),
        // Has pseudo-selector
        (
            "has(",
            "Style when element has matching children :has()",
            false,
        ),
        // Critical CSS hint
        ("critical", "Mark as above-fold critical CSS", false),
        // New pseudo-state prefixes
        ("visited:", "Style visited links", false),
        ("empty:", "Style when element has no children", false),
        (
            "target:",
            "Style when element is the URL fragment target",
            false,
        ),
        ("valid:", "Style when form element is valid", false),
        ("invalid:", "Style when form element is invalid", false),
        // New CSS properties
        (
            "text-underline-offset",
            "Offset of text underline from its default position",
            true,
        ),
        (
            "column-width",
            "Ideal width of columns in multi-column layout",
            true,
        ),
        (
            "column-rule",
            "Rule between columns (width style color)",
            true,
        ),
    ]
    .iter()
    .map(|(name, detail, takes_value)| {
        let insert = if *takes_value {
            format!("{} ", name)
        } else {
            name.to_string()
        };
        let mut completion = item(name, CompletionItemKind::PROPERTY, detail, &insert, range);
        // Boost element-specific attributes to the top of the list when we
        // know which element owns the brackets. The default sort prefix is
        // "5_" so unboosted entries land below the prioritized ones, while
        // staying above snippets (which use "zz_").
        let boosted = element
            .map(|e| element_specific_attrs(e).contains(name))
            .unwrap_or(false);
        completion.sort_text = Some(if boosted {
            format!("0_{}", name)
        } else {
            format!("5_{}", name)
        });
        completion
    })
    .collect()
}

fn state_attr_completions(prefix: &str, range: Range) -> Vec<CompletionItem> {
    [
        ("background", "Background color/value", true),
        ("color", "Text color", true),
        ("border", "Border (width [color])", true),
        ("border-top", "Top border (width [color])", true),
        ("border-bottom", "Bottom border (width [color])", true),
        ("border-left", "Left border (width [color])", true),
        ("border-right", "Right border (width [color])", true),
        ("rounded", "Border radius", true),
        ("bold", "Bold text", false),
        ("italic", "Italic text", false),
        ("underline", "Underlined text", false),
        ("size", "Font size", true),
        ("opacity", "Opacity (0-1)", true),
        ("cursor", "CSS cursor type", true),
        ("shadow", "Box shadow (CSS value)", true),
        ("text-shadow", "Text shadow", true),
        ("transform", "CSS transform", true),
        ("filter", "CSS filter", true),
        ("display", "Display mode", true),
        ("visibility", "Visibility", true),
        ("pointer-events", "Pointer events", true),
        ("user-select", "User selection", true),
        // Layout attrs for pseudo-states
        ("width", "Width", true),
        ("height", "Height", true),
        ("padding", "Inner padding", true),
        ("padding-x", "Horizontal padding", true),
        ("padding-y", "Vertical padding", true),
        ("margin", "Outer margin", true),
        ("margin-x", "Horizontal margin", true),
        ("margin-y", "Vertical margin", true),
        ("outline", "Outline", true),
        ("text-decoration", "Text decoration", true),
        ("text-decoration-color", "Text decoration color", true),
    ]
    .iter()
    .map(|(name, detail, takes_value)| {
        let full = format!("{}:{}", prefix, name);
        let insert = if *takes_value {
            format!("{} ", full)
        } else {
            full.clone()
        };
        item(&full, CompletionItemKind::PROPERTY, detail, &insert, range)
    })
    .collect()
}

fn color_value_completions(before: &str, range: Range) -> Option<Vec<CompletionItem>> {
    // Find the preceding attribute name before the cursor value position.
    // Inside brackets, attributes are comma-separated. Look for the last attribute token
    // before the current value position. Pattern: "attr value" or "attr " at end.
    let bracket_content = before.rsplit('[').next()?;
    // Split by commas to get the current segment
    let segment = bracket_content.rsplit(',').next()?.trim();

    // Check if the first word in this segment is a color-related attribute
    let attr = segment.split_whitespace().next()?;

    // Strip state prefix (e.g., "hover:background" -> "background")
    let base_attr = if let Some(pos) = attr.rfind(':') {
        &attr[pos + 1..]
    } else {
        attr
    };

    if !matches!(
        base_attr,
        "background"
            | "color"
            | "border"
            | "border-top"
            | "border-bottom"
            | "border-left"
            | "border-right"
            | "accent-color"
            | "caret-color"
            | "text-decoration-color"
            | "outline"
    ) {
        return None;
    }

    // Only show colors if we're in the value position (at least one space after the attr name)
    let after_attr = &segment[attr.len()..];
    if !after_attr.starts_with(' ') {
        return None;
    }

    let colors: &[(&str, &str, &str)] = &[
        ("white", "#ffffff", "White"),
        ("black", "#000000", "Black"),
        ("red", "#ef4444", "Red"),
        ("orange", "#f97316", "Orange"),
        ("yellow", "#eab308", "Yellow"),
        ("green", "#22c55e", "Green"),
        ("blue", "#3b82f6", "Blue"),
        ("indigo", "#6366f1", "Indigo"),
        ("purple", "#a855f7", "Purple"),
        ("pink", "#ec4899", "Pink"),
        ("gray", "#6b7280", "Gray"),
        ("slate", "#64748b", "Slate"),
        ("zinc", "#71717a", "Zinc"),
        ("neutral", "#737373", "Neutral"),
        ("stone", "#78716c", "Stone"),
        ("amber", "#f59e0b", "Amber"),
        ("lime", "#84cc16", "Lime"),
        ("emerald", "#10b981", "Emerald"),
        ("teal", "#14b8a6", "Teal"),
        ("cyan", "#06b6d4", "Cyan"),
        ("sky", "#0ea5e9", "Sky"),
        ("violet", "#8b5cf6", "Violet"),
        ("fuchsia", "#d946ef", "Fuchsia"),
        ("rose", "#f43f5e", "Rose"),
        ("transparent", "transparent", "Transparent"),
        (
            "currentColor",
            "currentColor",
            "Inherit from parent text color",
        ),
    ];

    let items: Vec<CompletionItem> = colors
        .iter()
        .map(|(label, value, detail)| {
            let doc = if value.starts_with('#') {
                format!("{} (`{}`)", detail, value)
            } else {
                detail.to_string()
            };
            CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::COLOR),
                detail: Some(doc),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: value.to_string(),
                })),
                documentation: if value.starts_with('#') {
                    Some(Documentation::String(value.to_string()))
                } else {
                    None
                },
                ..Default::default()
            }
        })
        .collect();

    Some(items)
}

fn variable_completions(text: &str, range: Range) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (name, value) in collect_theme_tokens(text) {
        items.push(item(
            &format!("${}", name),
            CompletionItemKind::COLOR,
            &format!("@theme \u{2014} {}", value),
            &format!("${}", name),
            range,
        ));
    }

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ")
            && let Some((name, value)) = rest.trim().split_once(' ')
        {
            let value = value.trim();
            if value.starts_with('[') {
                // Attribute bundle
                items.push(item(
                    &format!("${}", name),
                    CompletionItemKind::CONSTANT,
                    "Attribute bundle",
                    &format!("${}", name),
                    range,
                ));
            } else if !value.starts_with('$') {
                // Scalar variable (skip function params like $param)
                items.push(item(
                    &format!("${}", name),
                    CompletionItemKind::VARIABLE,
                    &format!("= {}", value),
                    &format!("${}", name),
                    range,
                ));
            }
        }
    }

    items
}

fn function_completions(text: &str, range: Range) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let all_lines: Vec<&str> = text.lines().collect();

    for (idx, line) in all_lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@let ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if let Some(name) = parts.first() {
                // Only suggest as function if it has params or an indented body
                let has_params = parts.len() > 1 && parts[1..].iter().any(|p| p.starts_with('$'));
                let has_body = all_lines
                    .get(idx + 1)
                    .map(|l| l.starts_with("  ") || l.starts_with('\t'))
                    .unwrap_or(false);
                if !has_params && !has_body {
                    continue;
                }
                let params: Vec<&str> = parts[1..]
                    .iter()
                    .map(|p| p.strip_prefix('$').unwrap_or(p))
                    .collect();
                let detail = if params.is_empty() {
                    "Function".to_string()
                } else {
                    format!("Function({})", params.join(", "))
                };
                // Generate snippet with tab stops for parameters
                let insert_text = if params.is_empty() {
                    format!("@{}", name)
                } else {
                    let param_snippets: Vec<String> = params
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let p_name = p.split('=').next().unwrap_or(p);
                            let default = p.split('=').nth(1).unwrap_or(p_name);
                            format!("{} ${{{}:{}}}", p_name, i + 1, default)
                        })
                        .collect();
                    format!("@{} [{}]", name, param_snippets.join(", "))
                };
                let mut ci = CompletionItem {
                    label: format!("@{}", name),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(detail),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range,
                        new_text: insert_text,
                    })),
                    ..Default::default()
                };
                if !params.is_empty() {
                    ci.insert_text_format = Some(tower_lsp::lsp_types::InsertTextFormat::SNIPPET);
                }
                items.push(ci);
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, ch: u32) -> Position {
        Position::new(line, ch)
    }

    #[test]
    fn owning_element_finds_element_on_same_line() {
        let text = "@input [type text, ";
        // Cursor at end of line — inside the unclosed `[`.
        assert_eq!(
            owning_element(text, pos(0, text.len() as u32)),
            Some("input".to_string())
        );
    }

    #[test]
    fn owning_element_finds_element_across_lines() {
        let text = "@button [\n  padding 10,\n  ";
        assert_eq!(
            owning_element(text, pos(2, 2)),
            Some("button".to_string())
        );
    }

    #[test]
    fn owning_element_returns_none_when_not_in_brackets() {
        let text = "@row\n";
        assert_eq!(owning_element(text, pos(0, 4)), None);
    }

    #[test]
    fn owning_element_skips_nested_brackets() {
        let text = "@el [transform translate(10, [20, 30]), ";
        // Cursor sits inside the outermost bracket after the inner one closed.
        assert_eq!(
            owning_element(text, pos(0, text.len() as u32)),
            Some("el".to_string())
        );
    }

    #[test]
    fn theme_tokens_extracted_from_block() {
        let text = "@theme\n  primary #3b82f6\n  spacing-md 16\n@row\n";
        let tokens = collect_theme_tokens(text);
        assert_eq!(
            tokens,
            vec![
                ("primary".into(), "#3b82f6".into()),
                ("spacing-md".into(), "16".into()),
            ]
        );
    }
}
