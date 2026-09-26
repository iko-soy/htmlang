use htmlang::ast::ElementKind;
use htmlang::syntax::{DefinitionKind, VisibleKind};
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

    // Nothing in a verbatim body (CSS, JavaScript, HTML, a code sample
    // under `@code`) is htmlang, so there is nothing to offer
    let tree = htmlang::syntax::parse(text);
    if crate::tree::verbatim_lines(&tree).contains(&position.line) {
        return vec![];
    }

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

        // After prefixes (`hover:`, `md:hover:`, `nth:2n:`), offer the
        // styles they can apply to, and the prefixes that can follow
        if let Some(colon) = current_word.rfind(':') {
            let prefix = &current_word[..=colon];
            if vocab::is_prefixed(prefix) {
                let element = owning_element(text, position);
                let group = attr_context(before)
                    .and_then(|c| c.group)
                    .unwrap_or_default();
                let mut items =
                    state_attr_completions(prefix, &group, edit_range, element.as_deref());
                items.extend(prefix_completions(prefix, &group, edit_range));
                items.extend(custom_property_completions(
                    text,
                    prefix,
                    current_word,
                    edit_range,
                ));
                return items;
            }
        }

        // In a prefixed group (`md:[...]`) every attribute is a style
        if let Some(group) = attr_context(before).and_then(|c| c.group) {
            let element = owning_element(text, position);
            let mut items = state_attr_completions("", &group, edit_range, element.as_deref());
            items.extend(prefix_completions("", &group, edit_range));
            items.extend(custom_property_completions(
                text,
                "",
                current_word,
                edit_range,
            ));
            return items;
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
        let given = given_by_the_argument(text, position);
        items.extend(
            attr_completions(edit_range, element.as_deref())
                .into_iter()
                .filter(|item| !given.contains(&item.label.trim_end_matches('='))),
        );
        items.extend(custom_property_completions(
            text,
            "",
            current_word,
            edit_range,
        ));
        return items;
    }

    // $ variable reference outside brackets
    let current_word = &before[word_start..];
    if current_word.starts_with('$') {
        return variable_completions(text, edit_range);
    }

    // An inline element or call in text, `{@name ...}`: elements and
    // functions, but no directives. In the text of `@code` or `@textarea`,
    // `{@` is text, so there is nothing to offer.
    if current_word.starts_with('@') && before[..word_start].ends_with('{') {
        if !starts_inline_element(text, position.line, word_start) {
            return vec![];
        }
        let mut items = element_completions(edit_range);
        items.extend(function_completions(text, edit_range));
        return items;
    }

    // The name of a `@slot` block under a call: the slots of its function
    let trimmed = before.trim_start();
    if let Some(name) = trimmed.strip_prefix("@slot ")
        && !name.contains(char::is_whitespace)
    {
        return slot_completions(text, position.line, edit_range);
    }

    // @ element/directive or start of line
    if trimmed.is_empty() || trimmed.starts_with('@') {
        let mut items = element_completions(edit_range);
        items.extend(directive_completions(edit_range));
        items.extend(function_completions(text, edit_range));
        items.extend(snippet_completions(edit_range));
        return items;
    }

    vec![]
}

/// Whether the `@` at byte `column` of `line` starts an inline element,
/// as the syntax tree reads it: not in text shown as written.
fn starts_inline_element(text: &str, line: u32, column: usize) -> bool {
    let tree = htmlang::syntax::parse(text);
    crate::tree::node_at(&tree, line).is_some_and(|node| {
        node.heads()
            .iter()
            .any(|head| head.name_span.line == line as usize + 1 && head.name_span.column == column)
    })
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
    /// In a prefixed group (`md:[...]`, also inside another one): the
    /// groups' prefixes as written, which apply to the attribute.
    pub group: Option<String>,
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
/// close or split it, and neither do commas inside `(...)` or `{...}`. In a
/// whole-attribute `if(CONDITION, A, B)`, the attribute being written is
/// the branch (a `[group]` is a list of its own).
pub(crate) fn attr_context(before: &str) -> Option<AttrContext<'_>> {
    // Each open list: where its current attribute starts, the `(`/`{`
    // depth inside it, the attributes before it, and the depths at which
    // an `if(` is open with where its current part starts
    struct List<'a> {
        start: usize,
        depth: i32,
        previous: Vec<&'a str>,
        ifs: Vec<(i32, usize)>,
        /// The prefixes before the `[` of a prefixed group
        prefix: Option<&'a str>,
    }
    let mut lists: Vec<List> = Vec::new();
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
            '[' => {
                // `md:[`: a group whose attributes take the prefixes
                let prefix = lists.last().and_then(|list| {
                    let from = list.ifs.last().map_or(list.start, |(_, at)| *at);
                    let written = before[from..i].trim_start();
                    let prefix = written.strip_suffix(':').map(|_| written)?;
                    let (known, rest) = vocab::split_prefixes(prefix);
                    (!known.is_empty() && rest.is_empty()).then_some(prefix)
                });
                lists.push(List {
                    start: i + 1,
                    depth: 0,
                    previous: Vec::new(),
                    ifs: Vec::new(),
                    prefix,
                })
            }
            ']' => {
                lists.pop();
            }
            '(' | '{' => {
                if let Some(list) = lists.last_mut() {
                    list.depth += 1;
                    let part = list.ifs.last().map_or(list.start, |(_, at)| *at);
                    if c == '(' && before[part..i].trim_start() == "if" {
                        list.ifs.push((list.depth, i + 1));
                    }
                }
            }
            ')' | '}' => {
                if let Some(list) = lists.last_mut() {
                    if list
                        .ifs
                        .last()
                        .is_some_and(|(depth, _)| *depth == list.depth)
                    {
                        list.ifs.pop();
                    }
                    list.depth -= 1;
                }
            }
            ',' => {
                if let Some(list) = lists.last_mut() {
                    if let Some(open) = list.ifs.last_mut()
                        && open.0 == list.depth
                    {
                        open.1 = i + 1;
                    } else if list.depth <= 0 {
                        list.previous.push(&before[list.start..i]);
                        list.start = i + 1;
                    }
                }
            }
            _ => {}
        }
        i += c.len_utf8();
    }
    // The prefixes of every group the attribute is in, outermost first
    let prefixes: String = lists.iter().filter_map(|list| list.prefix).collect();
    let group = (!prefixes.is_empty()).then_some(prefixes);
    let list = lists.pop()?;
    let start = list.ifs.last().map_or(list.start, |(_, at)| *at);
    Some(AttrContext {
        segment: &before[start..],
        previous: list.previous,
        group,
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
            "@el [width fill, max-width 800, center-x, padding 40, spacing 20]",
        ),
        (
            "nav bar",
            "Navigation bar with horizontal items",
            "@nav [flex-direction row, spacing 20, align-items center, padding 16, background #1a1a2e]",
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
    owning_list(text, position).map(|(_, _, name)| name)
}

/// The attributes not to offer in the list at `position`: the leading
/// attribute of an element whose leading argument is written after the
/// list (`href` in `@link [|] /about About`), which would give it twice.
fn given_by_the_argument(text: &str, position: Position) -> Vec<&'static str> {
    let Some((line, bracket, name)) = owning_list(text, position) else {
        return Vec::new();
    };
    let Some(kind) = ElementKind::from_name(&name) else {
        return Vec::new();
    };
    let leading = kind.arg().attributes();
    if leading.is_empty() {
        return leading;
    }
    // Find the list's `]` and look at what follows it on its line
    let lines: Vec<&str> = text.lines().collect();
    let mut depth = 0;
    let mut quoted = false;
    for (index, l) in lines.iter().enumerate().skip(line) {
        let start = if index == line { bracket } else { 0 };
        let mut chars = l[start..].char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => quoted = !quoted,
                _ if quoted => {}
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        let after = l[start + i + 1..].trim();
                        let chain = after.starts_with('>');
                        return if after.is_empty() || chain {
                            Vec::new()
                        } else {
                            leading
                        };
                    }
                }
                _ => {}
            }
        }
    }
    Vec::new()
}

/// The line and column of the `[` of the list around `position`, and the
/// name of the element before it.
fn owning_list(text: &str, position: Position) -> Option<(usize, usize, String)> {
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
                    // A group in `if(CONDITION, [a, b])` belongs to the
                    // list around it
                    let group = line[..col].trim_end().ends_with([',', '(']);
                    if depth > 0 {
                        depth -= 1;
                    } else if !group {
                        bracket_line = Some(line_idx);
                        bracket_col = col;
                        break 'outer;
                    }
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
    Some((line_idx, bracket_col, name.to_string()))
}

/// Attributes the LSP knows are specifically meaningful for a given element.
/// Universal styling attributes (padding, color, etc.) aren't listed here —
/// they remain available to every element via `attr_completions`.
fn element_specific_attrs(element: &str) -> &'static [&'static str] {
    match element {
        // The page is the root element: HTML attributes for `<html>`, and
        // its own word
        "page" => &["lang", "dir", "class", "favicon"],
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
        "source" => &["src", "srcset", "sizes", "media", "type"],
        "meta" => &["name", "content", "charset"],
        "blockquote" | "q" => &["cite"],
        "ins" | "del" => &["cite", "datetime"],
        "bdo" => &["dir"],
        "col" | "colgroup" => &["span"],
        "optgroup" => &["label", "disabled"],
        "track" => &["src", "kind", "srclang", "label", "default"],
        "embed" => &["src", "type", "width", "height"],
        "object" => &["data", "type", "name", "width", "height"],
        "map" => &["name"],
        "area" => &[
            "href", "alt", "shape", "coords", "target", "rel", "download",
        ],
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
    let lays_out_children = lays_out_children(element);
    if element == Some("page") {
        for name in vocab::PAGE_WORDS {
            let detail = "@page: the page's icon, put into the page";
            push(name.to_string(), format!("{} ", name), detail, "1", name);
        }
    }
    for name in vocab::HTMLANG_ATTRIBUTES {
        if !lays_out_children && vocab::CONTAINER_ATTRIBUTES.contains(name) {
            continue;
        }
        let doc = docs::attribute(name);
        let insert = if !vocab::HTMLANG_FLAGS.contains(name) {
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
    let mut choice = item(
        "if()",
        CompletionItemKind::SNIPPET,
        docs::IF_SUMMARY,
        "if(${1:\\$condition}, ${2:attribute})",
        range,
    );
    choice.insert_text_format = Some(InsertTextFormat::SNIPPET);
    choice.sort_text = Some("4_if".to_string());
    items.push(choice);
    items
}

/// The custom properties the file names (`@let --brand`, `[--gap 8px]`,
/// `var(--gap)`), to set on an element: `--gap `, or `md:--gap ` after a
/// prefix. The name being written is left out.
fn custom_property_completions(
    text: &str,
    prefix: &str,
    current: &str,
    range: Range,
) -> Vec<CompletionItem> {
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let mut names: Vec<&str> = Vec::new();
    for (at, _) in text.match_indices("--") {
        if text[..at].ends_with(is_name) {
            continue;
        }
        let rest = &text[at + 2..];
        if !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let end = rest.find(|c: char| !is_name(c)).unwrap_or(rest.len());
        let name = &text[at..at + 2 + end];
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
        .into_iter()
        .map(|name| format!("{}{}", prefix, name))
        .filter(|full| full != current)
        .map(|full| {
            let mut completion = item(
                &full,
                CompletionItemKind::VARIABLE,
                "Custom property: set on this element and everything inside it",
                &format!("{} ", full),
                range,
            );
            completion.sort_text = Some(format!("1_{}", full));
            completion
        })
        .collect()
}

/// Does `element` lay out its children, so `spacing`, `wrap` and
/// `grid-cols` work on it: a row, column or grid. A function's root isn't
/// known, so a call (or no element) gets them all.
fn lays_out_children(element: Option<&str>) -> bool {
    element
        .and_then(htmlang::ast::ElementKind::from_name)
        .is_none_or(|kind| kind.layout().is_container())
}

/// Styles that can follow prefixes: `hover:color`, `md:padding`, also in a
/// group under the prefixes `group` (`md:[`). `spacing`, `wrap` and
/// `grid-cols` only where `element` lays out its children, or under
/// `children:`, which puts them on the children, where the words that
/// place an element in its parent (`center-x`, `align-*`) can't go.
fn state_attr_completions(
    prefix: &str,
    group: &str,
    range: Range,
    element: Option<&str>,
) -> Vec<CompletionItem> {
    let chain = format!("{}{}", group, prefix);
    let (prefixes, _) = vocab::split_prefixes(&chain);
    let on_children = prefixes.contains(&"children:");
    let lays_out_children = on_children || lays_out_children(element);
    let htmlang = vocab::HTMLANG_ATTRIBUTES
        .iter()
        .filter(|name| lays_out_children || !vocab::CONTAINER_ATTRIBUTES.contains(name))
        .filter(|name| !(on_children && vocab::places_in_parent(name, None)))
        .filter(|name| **name != "inline")
        .map(|name| (*name, !vocab::HTMLANG_FLAGS.contains(name)));
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

/// The prefixes that can follow `prefix` (under the group prefixes
/// `group`): any at-rule prefix, and a selector prefix unless a
/// pseudo-element (`before:`), which comes last, is already there.
fn prefix_completions(prefix: &str, group: &str, range: Range) -> Vec<CompletionItem> {
    let chain = format!("{}{}", group, prefix);
    let (written, _) = vocab::split_prefixes(&chain);
    let after_element = written.iter().any(|p| vocab::is_pseudo_element(p));
    vocab::PSEUDO_PREFIXES
        .iter()
        .map(|(p, _)| *p)
        .filter(|_| !after_element)
        .chain(vocab::at_rule_prefixes())
        .filter(|p| !written.contains(p))
        .map(|p| {
            let label = format!("{}{}", prefix, p);
            let detail = docs::prefix_selector(p).unwrap_or_default();
            let mut completion = item(&label, CompletionItemKind::KEYWORD, &detail, &label, range);
            completion.sort_text = Some(format!("6_{}", label));
            completion
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

/// The `$names` visible where the cursor is (see
/// `htmlang::syntax::Tree::visible_at`): definitions above it in its
/// block and the blocks around it, the variables of the `@each` and the
/// parameters of the function it is in, and the standard library's
/// bundles.
fn variable_completions(text: &str, range: Range) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let tree = htmlang::syntax::parse(text);
    let visible = tree.visible_at(range.start.line as usize + 1);
    let hidden = |name: &str| visible.iter().any(|v| v.name == name);

    for doc in docs::BUNDLES.iter().filter(|doc| !hidden(doc.name)) {
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

    // What each `@let` says, by where its name is written
    let defs = crate::tree::definitions(text);
    let mut seen = std::collections::HashSet::new();
    // The innermost of each name
    for name in visible.iter().rev() {
        if !seen.insert(name.name) {
            continue;
        }
        let label = format!("${}", name.name);
        let (kind, detail) = match name.kind {
            VisibleKind::Let(DefinitionKind::Function) => continue,
            VisibleKind::Let(DefinitionKind::Bundle) => {
                (CompletionItemKind::CONSTANT, "Attribute bundle".to_string())
            }
            VisibleKind::Let(DefinitionKind::Value) => {
                let value = defs
                    .iter()
                    .find(|d| d.name_range == crate::tree::range(name.span))
                    .and_then(|d| d.value.clone())
                    .unwrap_or_default();
                (
                    CompletionItemKind::VARIABLE,
                    format!("= {}", value.trim_start_matches("= ")),
                )
            }
            VisibleKind::Parameter => (CompletionItemKind::VARIABLE, "Parameter".to_string()),
            VisibleKind::Loop => (CompletionItemKind::VARIABLE, "@each variable".to_string()),
            VisibleKind::Data => (CompletionItemKind::VARIABLE, "@data".to_string()),
        };
        items.push(item(&label, kind, &detail, &label, range));
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

/// The slots of the function called on the line that a `@slot` block on
/// `line` is under (through `@if`, `@else` and `@each`).
fn slot_completions(text: &str, line: u32, range: Range) -> Vec<CompletionItem> {
    let lines: Vec<&str> = text.lines().collect();
    let indent = |l: &str| l.len() - l.trim_start().len();
    let Some(mut level) = lines.get(line as usize).map(|l| indent(l)) else {
        return Vec::new();
    };
    let mut parent = None;
    for i in (0..line as usize).rev() {
        let l = lines[i];
        let trimmed = l.trim_start();
        if trimmed.is_empty()
            || htmlang::syntax::is_comment(trimmed.trim_end())
            || indent(l) >= level
        {
            continue;
        }
        level = indent(l);
        let directive = trimmed.split_whitespace().next().unwrap_or("");
        if !matches!(directive, "@if" | "@else" | "@each") {
            parent = Some(i as u32);
            break;
        }
    }
    let Some(parent) = parent else {
        return Vec::new();
    };
    let tree = htmlang::syntax::parse(text);
    let Some(htmlang::syntax::NodeKind::Element(element)) =
        crate::tree::node_at(&tree, parent).map(|n| &n.kind)
    else {
        return Vec::new();
    };
    let Some(head) = element.chain.last() else {
        return Vec::new();
    };
    let Some(def) = crate::tree::definitions(text)
        .into_iter()
        .find(|d| d.kind == DefinitionKind::Function && d.name == head.name)
    else {
        return Vec::new();
    };
    def.slots
        .iter()
        .map(|slot| {
            let detail = format!("Slot of @{}", head.name);
            item(slot, CompletionItemKind::FIELD, &detail, slot, range)
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
        assert_eq!(insert("opacity"), "opacity ");
        assert_eq!(insert("type="), "type=");
        assert_eq!(insert("required"), "required");
        assert_eq!(insert("hover:"), "hover:");
        let boosted = items.iter().find(|i| i.label == "type=").unwrap();
        assert!(boosted.sort_text.as_deref().unwrap().starts_with("0_"));
        let column = attr_completions(Range::default(), Some("ul"));
        let insert =
            |label: &str| match &column.iter().find(|i| i.label == label).unwrap().text_edit {
                Some(CompletionTextEdit::Edit(edit)) => edit.new_text.clone(),
                _ => panic!("no edit for {}", label),
            };
        assert_eq!(insert("spacing"), "spacing ");
        assert_eq!(insert("wrap"), "wrap");
    }

    #[test]
    fn page_offers_its_own_word_and_the_attributes_of_html_first() {
        let labels = |text: &str, line: u32, character: u32| -> Vec<CompletionItem> {
            completions(text, Position::new(line, character))
        };
        let items = labels("@page [] Home", 0, 7);
        let rank = |label: &str| {
            items
                .iter()
                .find(|i| i.label == label)
                .and_then(|i| i.sort_text.clone())
                .unwrap_or_else(|| panic!("{} not offered", label))
        };
        let favicon = items.iter().find(|i| i.label == "favicon").unwrap();
        match &favicon.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => assert_eq!(edit.new_text, "favicon "),
            _ => panic!("no edit for favicon"),
        }
        assert!(rank("favicon").starts_with("0_"));
        assert!(rank("lang=").starts_with("0_"));
        assert!(rank("dir=").starts_with("0_"));
        // It styles <body>, a column
        assert!(items.iter().any(|i| i.label == "spacing"));
        assert!(items.iter().any(|i| i.label == "background"));
        // Only @page has the word
        let items = labels("@el [] x", 0, 5);
        assert!(!items.iter().any(|i| i.label == "favicon"));
    }

    #[test]
    fn the_attribute_a_leading_argument_fills_is_not_offered_again() {
        let labels = |text: &str, line: u32, character: u32| -> Vec<String> {
            completions(text, Position::new(line, character))
                .into_iter()
                .map(|i| i.label)
                .collect()
        };
        let has = |labels: &[String], label: &str| labels.iter().any(|l| l == label);
        // The argument is written: `href=` would give it twice
        let text = "@link [] /about About";
        assert!(!has(&labels(text, 0, 7), "href="));
        assert!(has(&labels(text, 0, 7), "target="));
        let text = "@image [alt=x, ] logo.png";
        assert!(!has(&labels(text, 0, 15), "src="));
        let text = "@source [] a.webp";
        let offered = labels(text, 0, 9);
        assert!(!has(&offered, "src=") && !has(&offered, "srcset="));
        // Across lines, with a `]` inside quotes
        let text = "@form [\n  title=\"a]\",\n  \n] /subscribe";
        assert!(!has(&labels(text, 2, 2), "action="));
        // No argument: the attribute is how the value is given
        let text = "@link []\n  About";
        assert!(has(&labels(text, 0, 7), "href="));
        let text = "@image [";
        assert!(has(&labels(text, 0, 8), "src="));
        // A chain is not an argument
        let text = "@link [] > @b About";
        assert!(has(&labels(text, 0, 7), "href="));
        // Elements without a leading argument are unaffected
        let text = "@input [] x";
        assert!(has(&labels(text, 0, 8), "type="));
    }

    #[test]
    fn spacing_is_offered_only_where_children_are_laid_out() {
        let offers = |element: Option<&str>, name: &str| {
            attr_completions(Range::default(), element)
                .iter()
                .any(|i| i.label == name)
        };
        for container in ["el", "row", "grid", "section", "ol", "li"] {
            assert!(offers(Some(container), "spacing"), "{}", container);
        }
        for other in ["h2", "button", "paragraph", "table", "input", "image"] {
            assert!(!offers(Some(other), "spacing"), "{}", other);
            assert!(!offers(Some(other), "grid-cols"), "{}", other);
            assert!(offers(Some(other), "padding"), "{}", other);
        }
        // A function's root isn't known
        assert!(offers(Some("card"), "spacing"));
        assert!(offers(None, "spacing"));
        // After a prefix too, except `children:`, which styles the children
        let prefixed = |prefix: &str, element: &str, name: &str| {
            state_attr_completions(prefix, "", Range::default(), Some(element))
                .iter()
                .any(|i| i.label == format!("{}{}", prefix, name))
        };
        assert!(prefixed("md:", "row", "spacing"));
        assert!(!prefixed("md:", "h2", "spacing"));
        assert!(!prefixed("hover:", "button", "wrap"));
        assert!(prefixed("hover:", "button", "padding"));
        assert!(prefixed("children:", "paragraph", "spacing"));
    }

    #[test]
    fn the_file_s_custom_properties_are_offered_as_attributes() {
        let text = "@let --surface white\n@el [background var(--muted), --gap 8px]\n@el [--s";
        let items = completions(text, Position::new(2, 8));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        for name in ["--surface", "--muted", "--gap"] {
            assert!(labels.contains(&name), "{name}: {labels:?}");
        }
        // The name being written isn't one of them
        assert!(!labels.contains(&"--s"));
        let text = "@let --surface white\n@el [dark:";
        let items = completions(text, Position::new(1, 10));
        let item = items.iter().find(|i| i.label == "dark:--surface").unwrap();
        match &item.text_edit {
            Some(CompletionTextEdit::Edit(edit)) => assert_eq!(edit.new_text, "dark:--surface "),
            _ => panic!("no edit"),
        }
    }

    #[test]
    fn prefixed_attribute_completions() {
        let items = completions("@el [hover:", Position::new(0, 11));
        assert!(items.iter().any(|i| i.label == "hover:background"));
        let items = completions("@el [md:", Position::new(0, 8));
        assert!(items.iter().any(|i| i.label == "md:padding"));
        // Prefixes stack: styles and further prefixes after a chain
        let items = completions("@el [md:hover:", Position::new(0, 14));
        let has = |items: &[CompletionItem], label: &str| items.iter().any(|i| i.label == label);
        assert!(has(&items, "md:hover:color"));
        assert!(has(&items, "md:hover:dark:"));
        assert!(has(&items, "md:hover:before:"));
        assert!(!has(&items, "md:hover:md:"));
        // A pseudo-element is the last selector prefix
        let items = completions("@el [before:", Position::new(0, 12));
        assert!(has(&items, "before:dark:") && !has(&items, "before:hover:"));
        // Under `children:`, no word that places an element in its parent
        let items = completions("@row [children:", Position::new(0, 15));
        assert!(has(&items, "children:padding") && !has(&items, "children:center-x"));
        // In a prefixed group, only styles (and prefixes)
        let items = completions("@el [md:[pad", Position::new(0, 12));
        assert!(has(&items, "padding") && has(&items, "hover:"));
        assert!(!has(&items, "id=") && !has(&items, "disabled"));
        let items = completions("@row [dark:[children:[", Position::new(0, 22));
        assert!(has(&items, "padding") && !has(&items, "center-x"));
        assert_eq!(
            attr_context("@el [dark:[hover:[co").and_then(|c| c.group),
            Some("dark:hover:".to_string())
        );
        // A value's brackets are not a group
        assert_eq!(
            attr_context("@el [grid-template-columns [a").and_then(|c| c.group),
            None
        );
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
    fn a_branch_of_if_is_the_attribute_being_written() {
        fn context(before: &str) -> Option<(&str, usize)> {
            attr_context(before).map(|c| (c.segment, c.previous.len()))
        }
        assert_eq!(context("@el [padding 4, if($on, col"), Some((" col", 1)));
        assert_eq!(context("@el [if($on, color red, back"), Some((" back", 0)));
        assert_eq!(context("@el [if($a, if($b, x, marg"), Some((" marg", 0)));
        assert_eq!(context("@el [if($on, [padding 4, mar"), Some((" mar", 1)));
        assert_eq!(context("@el [if($on, padding 4), col"), Some((" col", 1)));
        // CSS's own if() inside a value is part of the value
        assert_eq!(
            context("@el [width if(media(print): 1px; else: 2px), col"),
            Some((" col", 1))
        );
        let items = completions("@el [if($on, col", Position::new(0, 16));
        assert!(items.iter().any(|i| i.label == "color"));
        let items = completions("@el [pad", Position::new(0, 8));
        assert!(items.iter().any(|i| i.label == "if()"));
        // A group's element is the one whose list it is in
        let text = "@input [\n  if($on, [ty";
        assert_eq!(
            owning_element(text, Position::new(1, 14)).as_deref(),
            Some("input")
        );
        let text = "@input [if($a, [id=x], [ty";
        assert_eq!(
            owning_element(text, Position::new(0, 26)).as_deref(),
            Some("input")
        );
    }

    #[test]
    fn value_completions_after_html_attribute() {
        let items = completions("@input [type=", Position::new(0, 13));
        assert!(items.iter().any(|i| i.label == "email"));
    }

    #[test]
    fn a_slot_block_under_a_call_offers_the_function_s_slots() {
        let text = "@let @card\n  @el\n    @slot actions\n    @children\n    @slot footer\n@card\n  @if true\n    @slot fo";
        let items = completions(text, pos(7, 12));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["actions", "footer"]);
        assert_eq!(items[1].detail.as_deref(), Some("Slot of @card"));
        // Under an element, not a call: nothing to offer
        let text = "@el\n  @slot ";
        assert!(completions(text, pos(1, 8)).is_empty());
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
    fn inline_elements_are_offered_in_every_text_but_literal_text() {
        // An element's argument is text like any other
        let text = "@h2 Meet {@t";
        let items = completions(text, pos(0, text.len() as u32));
        assert!(items.iter().any(|i| i.label == "@text"), "{:?}", items);
        // In `@code` and `@textarea`, `{@` is text
        for text in [
            "@code Write {@l",
            "@paragraph Write {@code {@l",
            "@code\n  Write {@l",
            // The lines under `@code` are a verbatim sample: `@` is text
            "@pre > @code\n  @",
            "@style\n  @",
        ] {
            let line = text.lines().count() as u32 - 1;
            let column = text.lines().last().unwrap().len() as u32;
            assert!(completions(text, pos(line, column)).is_empty(), "{}", text);
        }
    }

    #[test]
    fn variables_offered_are_those_visible_at_the_cursor() {
        let text = "@let a 1\n@let @card [title]\n  @let inner 2\n  @text $\n@each $x in 1, 2\n  @text $\n@text $\n@let later 3\n";
        let labels = |line| -> Vec<String> {
            completions(text, pos(line, 9))
                .into_iter()
                .map(|c| c.label)
                .collect()
        };
        let body = labels(3);
        for name in ["$a", "$title", "$inner", "$truncate"] {
            assert!(body.iter().any(|l| l == name), "{}: {:?}", name, body);
        }
        assert!(
            !body.iter().any(|l| l == "$later" || l == "$x"),
            "{:?}",
            body
        );
        let each = labels(5);
        assert!(each.iter().any(|l| l == "$x"), "{:?}", each);
        assert!(
            !each.iter().any(|l| l == "$inner" || l == "$title"),
            "{:?}",
            each
        );
        let after = labels(6);
        assert!(
            !after.iter().any(|l| l == "$x" || l == "$later"),
            "{:?}",
            after
        );
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
