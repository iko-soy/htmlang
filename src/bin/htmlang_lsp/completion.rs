use htmlang::ast::ElementKind;
use htmlang::syntax::DefinitionKind;
use htmlang::vocab;
use tower_lsp::lsp_types::*;

use crate::docs;

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
        // A function's parameter list names its parameters: there are no
        // attributes or values to offer
        if in_parameter_list(text, position.line) {
            return vec![];
        }
        let current_word = &before[word_start..];

        // $ variable/define reference
        if current_word.starts_with('$') {
            return variable_completions(text, edit_range);
        }

        // After a state/media prefix (`hover:`, `md:`, `nth:2n:`), offer
        // the styles it can apply to.
        if let Some(colon) = current_word.rfind(':') {
            let prefix = &current_word[..=colon];
            if vocab::is_prefixed(prefix) {
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
        let mut items = element
            .as_deref()
            .map(|name| param_completions(text, name, before, edit_range))
            .unwrap_or_default();
        items.extend(attr_completions(edit_range, element.as_deref()));
        return items;
    }

    // $ variable reference outside brackets
    let current_word = &before[word_start..];
    if current_word.starts_with('$') {
        return variable_completions(text, edit_range);
    }

    // An inline element or call in text, `{@name ...}`: elements and
    // functions, but no directives
    if current_word.starts_with('@') && before[..word_start].ends_with('{') {
        let mut items = element_completions(edit_range);
        items.extend(function_completions(text, edit_range));
        return items;
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

/// Whether `line` is (part of) a function definition's head,
/// `@let @card [title, tone info]`.
fn in_parameter_list(text: &str, line: u32) -> bool {
    let tree = htmlang::syntax::parse(text);
    crate::tree::node_at(&tree, line)
        .and_then(|node| node.directive())
        .is_some_and(|directive| {
            matches!(&directive.args, htmlang::syntax::DirectiveArgs::Let(def)
                if matches!(def.form, htmlang::syntax::LetForm::Function(_)))
        })
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
    attr_context(text).is_some()
}

/// The attribute being written at the end of `before`, inside an attribute
/// list that is still open there.
pub(crate) struct AttrContext<'a> {
    /// The attribute's text so far: what follows the list's last comma.
    pub segment: &'a str,
    /// The attributes before it in the list, as written.
    pub previous: Vec<&'a str>,
}

impl AttrContext<'_> {
    /// The names of the attributes before this one (`title` of
    /// `title Hi` or `title=Hi`).
    pub fn previous_keys(&self) -> impl Iterator<Item = &str> {
        self.previous.iter().map(|attr| attr_key(attr))
    }
}

/// The name an attribute starts with: up to a space or `=`.
pub(crate) fn attr_key(attr: &str) -> &str {
    let attr = attr.trim_start();
    let end = attr
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(attr.len());
    &attr[..end]
}

/// Where `before` ends inside an open attribute list, read as the compiler
/// reads one: escapes (`\,`, `\]`) and quoted text (`"a, b"`) don't open,
/// close or split it, and neither do commas inside `(...)` or `{...}`.
pub(crate) fn attr_context(before: &str) -> Option<AttrContext<'_>> {
    // Each open list: where its current attribute starts, the `(`/`{`
    // depth inside it, and the attributes before it
    let mut lists: Vec<(usize, i32, Vec<&str>)> = Vec::new();
    let mut quoted = false;
    let mut i = 0;
    while i < before.len() {
        let rest = &before[i..];
        let escape = htmlang::syntax::escape_len(rest);
        if escape > 0 {
            i += escape;
            continue;
        }
        let Some(c) = rest.chars().next() else { break };
        match c {
            '"' if !lists.is_empty() => quoted = !quoted,
            _ if quoted => {}
            '[' => lists.push((i + 1, 0, Vec::new())),
            ']' => {
                lists.pop();
            }
            '(' | '{' => {
                if let Some(list) = lists.last_mut() {
                    list.1 += 1;
                }
            }
            ')' | '}' => {
                if let Some(list) = lists.last_mut() {
                    list.1 -= 1;
                }
            }
            ',' => {
                if let Some(list) = lists.last_mut()
                    && list.1 <= 0
                {
                    list.2.push(&before[list.0..i]);
                    list.0 = i + 1;
                }
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    let (start, _, previous) = lists.pop()?;
    Some(AttrContext {
        segment: &before[start..],
        previous,
    })
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

/// Elements (from the compiler) and standard-library components.
fn element_completions(range: Range) -> Vec<CompletionItem> {
    let elements = ElementKind::all_names().map(|name| {
        let detail = docs::element_summary(name).unwrap_or_default();
        (name, detail)
    });
    let components = docs::COMPONENTS
        .iter()
        .map(|doc| (doc.name, format!("{} (standard library)", doc.summary)));
    elements
        .chain(components)
        .map(|(name, detail)| {
            let label = format!("@{}", name);
            item(&label, CompletionItemKind::KEYWORD, &detail, &label, range)
        })
        .collect()
}

/// Directives, from the compiler's directive table.
fn directive_completions(range: Range) -> Vec<CompletionItem> {
    htmlang::ast::DIRECTIVES
        .iter()
        .map(|spec| spec.name)
        .map(|name| {
            let label = format!("@{}", name);
            let detail = docs::directive(name).map_or("Directive", |d| d.summary);
            item(
                &label,
                CompletionItemKind::SNIPPET,
                detail,
                &format!("{} ", label),
                range,
            )
        })
        .collect()
}

fn snippet_completions(range: Range) -> Vec<CompletionItem> {
    let snippets: &[(&str, &str, &str)] = &[
        (
            "function",
            "Define a reusable function",
            "@let @${1:name} [${2:title}]\n  @el [${3:padding 16}]\n    @h3 \\$${2}\n    @children",
        ),
        (
            "responsive layout",
            "Centered responsive column layout",
            "@el [max-width 800, center-x, padding 40, spacing 20]",
        ),
        (
            "nav bar",
            "Navigation bar with horizontal items",
            "@nav [padding 16, background #1a1a2e]\n  @row [spacing 20, align-items center]",
        ),
        (
            "each with else",
            "Loop with empty-state fallback",
            "@each \\$${1:item} in ${2:list}\n  @text \\$${1:item}\n@else\n  @text [color #888] No items found.",
        ),
        (
            "if / else",
            "Conditional rendering block",
            "@if ${1:condition}\n  ${2:content}\n@else\n  ${3:fallback}",
        ),
        (
            "form with inputs",
            "Form with a labeled input and a submit button",
            "@form [spacing 16] ${1:/submit}\n  @label [for=${2:email}] ${3:Email}\n  @input [type=email, name=$2, id=$2, required]\n  @button [type=submit] Submit",
        ),
        (
            "grid layout",
            "Grid with equal columns",
            "@grid [grid-cols ${1:3}, spacing ${2:20}]\n  @el [padding 20]\n    ${3:Item}",
        ),
        (
            "dark mode",
            "Element with light and dark styles",
            "@el [background ${1:white}, dark:background ${2:#1a1a2e}, color ${3:#333}, dark:color ${4:#eee}]\n  ${5:Content}",
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
    let cursor_col =
        (position.character as usize).min(lines.get(cursor_line).map(|l| l.len()).unwrap_or(0));
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
            // `\[` and `\]` are brackets in a value (see `attr_context`)
            let backslashes = col - line[..col].trim_end_matches('\\').len();
            if backslashes % 2 == 1 {
                continue;
            }
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
        "button" => &[
            "type",
            "disabled",
            "name",
            "value",
            "autofocus",
            "popovertarget",
            "popovertargetaction",
        ],
        "select" => &[
            "name",
            "multiple",
            "required",
            "disabled",
            "size",
            "autofocus",
        ],
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
        "option" => &["value", "selected", "disabled", "label"],
        "form" => &[
            "action",
            "method",
            "novalidate",
            "target",
            "autocomplete",
            "enctype",
            "name",
        ],
        "image" => &[
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
        "link" => &[
            "href",
            "target",
            "rel",
            "download",
            "referrerpolicy",
            "type",
        ],
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
        "ol" => &["type", "start", "reversed"],
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
    let segment = attr_context(before)?.segment.trim_start();
    // A style (`cursor `) or an HTML attribute (`type=`) with no value typed yet.
    let attr = segment
        .strip_suffix('=')
        .filter(|a| !a.contains(char::is_whitespace))
        .or_else(|| {
            let (attr, rest) = segment.split_once(' ')?;
            rest.trim().is_empty().then_some(attr)
        })?;

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
        "white-space" => &[
            "normal",
            "nowrap",
            "pre",
            "pre-line",
            "pre-wrap",
            "break-spaces",
        ],
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
            "text", "numeric", "decimal", "email", "search", "tel", "url", "none",
        ],
        "enterkeyhint" => &["enter", "done", "go", "next", "previous", "search", "send"],
        "fetchpriority" => &["high", "low", "auto"],
        "spellcheck" | "translate" => &["true", "false"],
        "color-scheme" => &["light", "dark", "light dark", "normal"],
        "appearance" => &["none", "auto"],
        "autocomplete" => &[
            "on",
            "off",
            "name",
            "email",
            "username",
            "current-password",
            "new-password",
        ],
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
        "contain" => &[
            "none", "strict", "content", "size", "layout", "style", "paint",
        ],
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

/// Attributes, from the compiler's vocabulary: htmlang attributes and CSS
/// properties (`key value`), HTML attributes (`key=value`, or bare for
/// booleans), and state/media prefixes. Attributes the owning element
/// specifically uses sort first.
fn attr_completions(range: Range, element: Option<&str>) -> Vec<CompletionItem> {
    let boosted = element.map_or(&[][..], element_specific_attrs);
    let mut items = Vec::new();
    let mut push = |label: String, insert: String, detail: &str, rank: &str, name: &str| {
        let mut completion = item(&label, CompletionItemKind::PROPERTY, detail, &insert, range);
        let rank = if boosted.contains(&name) { "0" } else { rank };
        completion.sort_text = Some(format!("{}_{}", rank, label));
        items.push(completion);
    };
    for name in vocab::HTMLANG_ATTRIBUTES {
        let doc = docs::attribute(name);
        let insert = if doc.is_none_or(|d| d.takes_value()) {
            format!("{} ", name)
        } else {
            name.to_string()
        };
        let detail = doc.map_or("htmlang attribute", |d| d.summary);
        push(name.to_string(), insert, detail, "2", name);
    }
    for name in vocab::CSS_PROPERTIES {
        let detail = docs::attribute(name).map_or("CSS property", |d| d.summary);
        push(name.to_string(), format!("{} ", name), detail, "5", name);
    }
    for name in vocab::BOOLEAN_HTML_ATTRS {
        if !vocab::is_style_attribute(name) {
            push(
                name.to_string(),
                name.to_string(),
                "HTML attribute (boolean)",
                "3",
                name,
            );
        }
    }
    for name in vocab::HTML_ATTRIBUTES {
        if !vocab::BOOLEAN_HTML_ATTRS.contains(name) {
            push(
                format!("{}=", name),
                format!("{}=", name),
                "HTML attribute",
                "3",
                name,
            );
        }
    }
    let prefixes = vocab::PSEUDO_PREFIXES
        .iter()
        .map(|(p, _)| *p)
        .chain(vocab::RESPONSIVE_PREFIXES.iter().copied())
        .chain(vocab::MEDIA_PREFIXES.iter().copied())
        .chain(vocab::CONTAINER_QUERY_PREFIXES.iter().copied());
    for prefix in prefixes {
        let detail = docs::prefix_selector(prefix).unwrap_or_default();
        push(prefix.to_string(), prefix.to_string(), &detail, "6", prefix);
    }
    items
}

/// Styles that can follow a state/media prefix: `hover:color`, `md:padding`.
fn state_attr_completions(prefix: &str, range: Range) -> Vec<CompletionItem> {
    let htmlang = vocab::HTMLANG_ATTRIBUTES
        .iter()
        .map(|name| (*name, docs::attribute(name).is_none_or(|d| d.takes_value())));
    let css = vocab::CSS_PROPERTIES.iter().map(|name| (*name, true));
    htmlang
        .chain(css)
        .map(|(name, takes_value)| {
            let full = format!("{}{}", prefix, name);
            let insert = if takes_value {
                format!("{} ", full)
            } else {
                full.clone()
            };
            let detail = docs::attribute(name).map_or("CSS property", |d| d.summary);
            item(&full, CompletionItemKind::PROPERTY, detail, &insert, range)
        })
        .collect()
}

fn color_value_completions(before: &str, range: Range) -> Option<Vec<CompletionItem>> {
    // Find the preceding attribute name before the cursor value position.
    // Inside brackets, attributes are comma-separated. Look for the last attribute token
    // before the current value position. Pattern: "attr value" or "attr " at end.
    let segment = attr_context(before)?.segment.trim();

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

    for doc in docs::BUNDLES {
        let label = format!("${}", doc.name);
        let detail = format!("{} (standard-library bundle)", doc.summary);
        items.push(item(
            &label,
            CompletionItemKind::CONSTANT,
            &detail,
            &label,
            range,
        ));
    }

    for def in crate::tree::definitions(text) {
        let label = format!("${}", def.name);
        match def.kind {
            DefinitionKind::Bundle => {
                items.push(item(
                    &label,
                    CompletionItemKind::CONSTANT,
                    "Attribute bundle",
                    &label,
                    range,
                ));
            }
            DefinitionKind::Value => {
                let value = def.value.unwrap_or_default();
                let detail = format!("= {}", value.trim_start_matches("= "));
                items.push(item(
                    &label,
                    CompletionItemKind::VARIABLE,
                    &detail,
                    &label,
                    range,
                ));
            }
            DefinitionKind::Function => {}
        }
    }

    items
}

/// The parameters of the function `name` that a call's list doesn't pass
/// yet, first among its attributes.
fn param_completions(text: &str, name: &str, before: &str, range: Range) -> Vec<CompletionItem> {
    let Some(def) = crate::tree::definitions(text)
        .into_iter()
        .find(|d| d.kind == DefinitionKind::Function && d.name == name)
    else {
        return Vec::new();
    };
    let passed: Vec<String> = attr_context(before)
        .map(|args| args.previous_keys().map(String::from).collect())
        .unwrap_or_default();
    def.params
        .iter()
        .filter(|p| !passed.contains(&p.name))
        .map(|p| {
            let detail = match &p.default {
                Some(default) => format!("Parameter of @{} (default: {})", name, default),
                None => format!("Parameter of @{} (required)", name),
            };
            let mut completion = item(
                &p.name,
                CompletionItemKind::VARIABLE,
                &detail,
                &format!("{} ", p.name),
                range,
            );
            completion.sort_text = Some(format!("0_{}", p.name));
            completion
        })
        .collect()
}

fn function_completions(text: &str, range: Range) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for def in crate::tree::definitions(text) {
        if def.kind != DefinitionKind::Function {
            continue;
        }
        let name = &def.name;
        let detail = if def.params.is_empty() {
            "Function".to_string()
        } else {
            format!("Function {}", crate::analysis::param_list(&def.params))
        };
        // A snippet with a tab stop for each required parameter. One with
        // a default is left out: its default is filled in at the call,
        // where it may use the parameters before it, and completion in
        // the list offers it.
        let required: Vec<&str> = def
            .params
            .iter()
            .filter(|p| p.default.is_none())
            .map(|p| p.name.as_str())
            .collect();
        let insert_text = if required.is_empty() {
            format!("@{}", name)
        } else {
            let param_snippets: Vec<String> = required
                .iter()
                .enumerate()
                .map(|(i, p)| format!("{} ${{{}:{}}}", p, i + 1, p))
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
        if !required.is_empty() {
            ci.insert_text_format = Some(tower_lsp::lsp_types::InsertTextFormat::SNIPPET);
        }
        items.push(ci);
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
        let text = "@input [type=text, ";
        // Cursor at end of line — inside the unclosed `[`.
        assert_eq!(
            owning_element(text, pos(0, text.len() as u32)),
            Some("input".to_string())
        );
    }

    #[test]
    fn owning_element_finds_element_across_lines() {
        let text = "@button [\n  padding 10,\n  ";
        assert_eq!(owning_element(text, pos(2, 2)), Some("button".to_string()));
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
    fn element_and_directive_completions_come_from_the_compiler() {
        let range = Range::default();
        let elements = element_completions(range);
        for name in ElementKind::all_names() {
            let label = format!("@{}", name);
            assert!(
                elements.iter().any(|i| i.label == label),
                "missing {}",
                label
            );
        }
        assert!(elements.iter().any(|i| i.label == "@spacer"));
        let directives = directive_completions(range);
        for spec in htmlang::ast::DIRECTIVES {
            assert!(
                directives
                    .iter()
                    .any(|i| i.label == format!("@{}", spec.name))
            );
        }
    }

    #[test]
    fn attribute_completions_use_the_right_form() {
        let items = attr_completions(Range::default(), Some("input"));
        let insert = |label: &str| {
            let item = items.iter().find(|i| i.label == label).unwrap();
            match &item.text_edit {
                Some(CompletionTextEdit::Edit(edit)) => edit.new_text.clone(),
                _ => panic!("no edit for {}", label),
            }
        };
        assert_eq!(insert("spacing"), "spacing ");
        assert_eq!(insert("wrap"), "wrap");
        assert_eq!(insert("opacity"), "opacity ");
        assert_eq!(insert("type="), "type=");
        assert_eq!(insert("required"), "required");
        assert_eq!(insert("hover:"), "hover:");
        let boosted = items.iter().find(|i| i.label == "type=").unwrap();
        assert!(boosted.sort_text.as_deref().unwrap().starts_with("0_"));
    }

    #[test]
    fn prefixed_attribute_completions() {
        let items = completions("@el [hover:", Position::new(0, 11));
        assert!(items.iter().any(|i| i.label == "hover:background"));
        let items = completions("@el [md:", Position::new(0, 8));
        assert!(items.iter().any(|i| i.label == "md:padding"));
    }

    #[test]
    fn escaped_commas_and_quotes_do_not_start_an_attribute() {
        fn context(before: &str) -> Option<(&str, usize)> {
            attr_context(before).map(|c| (c.segment, c.previous.len()))
        }
        assert_eq!(
            context(r"@el [transition opacity 1s\, color 1s, cursor "),
            Some((" cursor ", 1))
        );
        assert_eq!(context(r#"@el [content "a, b", "#), Some((" ", 1)));
        assert_eq!(
            context("@el [box-shadow 0 0 rgba(0,0,0,1), "),
            Some((" ", 1))
        );
        // `\]` and `"]"` don't close the list, `]` does
        assert!(in_brackets(r"@el [width 4\], "));
        assert!(in_brackets(r#"@el [content "]"#));
        assert!(!in_brackets("@el [width 4] text"));
        // An escaped `]` is not a list the cursor is inside of
        let text = r"@input [pattern=\d\], ";
        assert_eq!(
            owning_element(text, pos(0, text.len() as u32)),
            Some("input".to_string())
        );
        // The value `cursor` is still offered after an escaped comma
        let text = r"@el [transition opacity 1s\, color 1s, cursor ";
        let items = completions(text, pos(0, text.len() as u32));
        assert!(items.iter().any(|i| i.label == "pointer"), "{:?}", items);
    }

    #[test]
    fn value_completions_after_html_attribute() {
        let items = completions("@input [type=", Position::new(0, 13));
        assert!(items.iter().any(|i| i.label == "email"));
    }

    #[test]
    fn a_call_offers_the_parameters_it_does_not_pass_yet() {
        let text = "@let @card [title, tone info]\n  @el $title\n@card [tone x, ";
        let items = completions(text, pos(2, 15));
        let title = items.iter().find(|i| i.label == "title").expect("title");
        assert_eq!(
            title.detail.as_deref(),
            Some("Parameter of @card (required)")
        );
        assert!(!items.iter().any(|i| i.label == "tone"));
        assert!(items.iter().any(|i| i.label == "padding"));
    }

    #[test]
    fn a_parameter_list_offers_no_attributes() {
        let text = "@let @card [title, pad";
        assert!(completions(text, pos(0, text.len() as u32)).is_empty());
        let text = "@let @card [\n  title,\n  pa";
        assert!(completions(text, pos(2, 4)).is_empty());
        // A call's list still does
        let text = "@let @card [title]\n  @el $title\n@card [pad";
        let items = completions(text, pos(2, 10));
        assert!(items.iter().any(|i| i.label == "padding"), "{:?}", items);
    }

    #[test]
    fn an_inline_element_offers_elements_and_functions() {
        let text = "@let @key\n  @kbd\n    @children\n@paragraph\n  Press {@k";
        let labels: Vec<String> = completions(text, pos(4, 11))
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.iter().any(|l| l == "@key"), "{:?}", labels);
        assert!(labels.iter().any(|l| l == "@kbd"), "{:?}", labels);
        assert!(!labels.iter().any(|l| l == "@each"), "{:?}", labels);
    }

    #[test]
    fn a_call_snippet_passes_the_required_parameters() {
        let snippet = |text: &str| {
            let items = function_completions(text, Range::default());
            let card = items.iter().find(|i| i.label == "@card").expect("@card");
            let Some(CompletionTextEdit::Edit(edit)) = &card.text_edit else {
                panic!("{:?}", card);
            };
            edit.new_text.clone()
        };
        // A default is filled in at the call, where `$title` is the
        // parameter; passing its text would read the caller's `$title`
        let text = "@let @card [title, heading \"About $title\", kind]\n  @el $heading\n";
        assert_eq!(snippet(text), "@card [title ${1:title}, kind ${2:kind}]");
        let text = "@let @card [tone $brand, list a\\, b]\n  @el $tone\n";
        assert_eq!(snippet(text), "@card");
    }
}
