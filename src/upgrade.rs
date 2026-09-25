//! `htmlang upgrade`: rewrite removed or renamed syntax to its current form.
//!
//! Every rewrite here is meant to be behavior-preserving: the upgraded file
//! compiles to the same HTML as the original did before the syntax was
//! removed. Constructs that can't be rewritten mechanically are reported in
//! [`Upgrade::manual`] and left untouched.

/// Result of upgrading one file.
pub struct Upgrade {
    pub output: String,
    pub changes: usize,
    /// Constructs that need a manual rewrite: (1-based line, message).
    pub manual: Vec<(usize, String)>,
}

/// Element aliases and their canonical names.
const ELEMENT_ALIASES: &[(&str, &str)] = &[
    ("col", "column"),
    ("p", "paragraph"),
    ("img", "image"),
    ("li", "item"),
    ("btn", "button"),
    ("ul", "list"),
    ("divider", "hr"),
    ("opt", "option"),
];

/// Attribute keys that were renamed.
const ATTR_RENAMES: &[(&str, &str)] = &[
    ("animate", "animation"),
    ("inset-area", "position-area"),
    ("align-center", "center-x"),
    ("gap-x", "column-gap"),
    ("gap-y", "row-gap"),
    ("shadow", "box-shadow"),
];

/// Variable filter aliases (`$x|upper`) and their canonical names.
const FILTER_ALIASES: &[(&str, &str)] = &[
    ("upper", "uppercase"),
    ("lower", "lowercase"),
    ("cap", "capitalize"),
    ("len", "length"),
];

/// Directives whose indented bodies are foreign text (CSS, JS, Markdown,
/// HTML, JSON) and must not be rewritten.
const VERBATIM_BODIES: &[&str] = &["@style", "@script", "@markdown", "@head"];

pub fn upgrade(input: &str) -> Upgrade {
    let mut manual = Vec::new();
    let (folded, mut changes) = fold_head_directives(input, &mut manual);
    let lines: Vec<&str> = folded.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut attr_depth = 0i32;
    // Whether the attribute list being rewritten belongs to an element (or
    // a bundle for elements), where HTML attributes now need `key=value`.
    let mut element_attrs = false;
    let mut i = 0;
    // A file may define its own function named like an old alias (e.g.
    // `@let divider`); calls to it must not be renamed.
    // (Lines inside `"""` strings don't count: they're text.)
    let mut user_defined: Vec<&str> = Vec::new();
    let mut j = 0;
    while j < lines.len() {
        if let Some(end) = triple_quote_end(&lines, j) {
            j = end;
            continue;
        }
        if let Some(name) = lines[j]
            .trim()
            .strip_prefix("@let ")
            .and_then(|rest| rest.split_whitespace().next())
        {
            user_defined.push(name);
        }
        j += 1;
    }

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        let indent = indent_of(line);

        // Copy verbatim regions unchanged.
        if let Some(end) = triple_quote_end(&lines, i) {
            out.extend(lines[i..end].iter().map(|l| l.to_string()));
            i = end;
            continue;
        }
        if attr_depth == 0 && VERBATIM_BODIES.iter().any(|d| starts_directive(trimmed, d)) {
            // The directive line is upgraded like any other; its body is not.
            let end = block_end(&lines, i);
            let mut header = rewrite_attr_regions(line, &mut attr_depth, &mut true, &user_defined);
            header = convert_filters(&header);
            if header != line {
                changes += 1;
            }
            out.push(header);
            out.extend(lines[i + 1..end].iter().map(|l| l.to_string()));
            i = end;
            continue;
        }

        // --- Block rewrites ---
        if attr_depth == 0
            && let Some(block) = rewrite_block(&lines, i, &mut manual, &user_defined)
        {
            out.extend(block.lines);
            changes += 1;
            i = block.end;
            continue;
        }

        // --- Line rewrites ---
        let mut new_line = line.to_string();
        if attr_depth == 0 {
            new_line = rewrite_directive_line(&new_line, indent, i, &mut manual);
            new_line = rename_elements(&new_line, &user_defined);
        }
        if attr_depth == 0 {
            element_attrs = takes_element_attributes(new_line.trim_start(), &user_defined);
        }
        new_line =
            rewrite_attr_regions(&new_line, &mut attr_depth, &mut element_attrs, &user_defined);
        new_line = convert_filters(&new_line);
        if new_line != line {
            changes += 1;
        }
        out.push(new_line);
        i += 1;
    }

    let mut output = out.join("\n");
    if input.ends_with('\n') {
        output.push('\n');
    }
    Upgrade {
        output,
        changes,
        manual,
    }
}

struct Block {
    lines: Vec<String>,
    end: usize,
}

/// Rewrites of directives that own an indented block.
fn rewrite_block(
    lines: &[&str],
    i: usize,
    manual: &mut Vec<(usize, String)>,
    user_defined: &[&str],
) -> Option<Block> {
    let line = lines[i];
    let trimmed = line.trim();
    let indent = indent_of(line);
    let pad = " ".repeat(indent);
    let end = block_end(lines, i);
    let body = &lines[i + 1..end];

    // @tooltip TEXT (text shown and used as the hover tip) →
    // @tooltip [tip TEXT] TEXT
    if let Some(rest) = trimmed.strip_prefix("@tooltip ")
        && !user_defined.contains(&"tooltip")
        && !has_attr_key(rest, "tip")
    {
        let (attrs, text) = match rest.strip_prefix('[') {
            Some(after) => {
                let close = after.find(']')?;
                (Some(&after[..close]), after[close + 1..].trim())
            }
            None => (None, rest.trim()),
        };
        if text.is_empty() {
            return None;
        }
        let attrs = match attrs {
            Some(a) if !a.trim().is_empty() => format!("[tip {}, {}]", text, a.trim()),
            _ => format!("[tip {}]", text),
        };
        let mut out = vec![format!("{pad}@tooltip {} {}", attrs, text)];
        out.extend(body.iter().map(|l| l.to_string()));
        return Some(Block { lines: out, end });
    }

    // @breadcrumb: each crumb is now an explicit @item
    if (trimmed == "@breadcrumb" || trimmed.starts_with("@breadcrumb "))
        && !user_defined.contains(&"breadcrumb")
    {
        let child_indent = body.iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l));
        if body.iter().all(|l| {
            l.trim().is_empty()
                || Some(indent_of(l)) != child_indent
                || l.trim().starts_with("@item")
        }) {
            return None;
        }
        let mut out = vec![line.to_string()];
        for l in body {
            let t = l.trim();
            if Some(indent_of(l)) == child_indent && !t.is_empty() && !t.starts_with("@item") {
                let wrapped = if t.starts_with('@') {
                    format!("@item > {}", t)
                } else {
                    format!("@item {}", t)
                };
                out.push(format!("{}{}", " ".repeat(indent_of(l)), wrapped));
            } else {
                out.push(l.to_string());
            }
        }
        return Some(Block { lines: out, end });
    }

    // @switch $v / @case x [attrs] → @match $v / @case x / @let __switch [attrs]
    if let Some(rest) = trimmed.strip_prefix("@switch ") {
        let mut out = vec![format!("{pad}@match {}", rest.trim())];
        let case_indent = body.iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l));
        let mut j = 0;
        while j < body.len() {
            let l = body[j];
            let t = l.trim();
            let is_case_line = Some(indent_of(l)) == case_indent
                && (t.starts_with("@case ") || t == "@default" || t.starts_with("@default "));
            let bracket = t.find('[').filter(|_| is_case_line);
            if let Some(bracket) = bracket {
                let head = t[..bracket].trim_end();
                let attrs = &t[bracket..];
                out.push(format!("{}{}", " ".repeat(indent_of(l)), head));
                let body_indent = body[j + 1..]
                    .iter()
                    .find(|b| !b.trim().is_empty())
                    .map(|b| indent_of(b))
                    .filter(|&n| n > indent_of(l))
                    .unwrap_or(indent_of(l) + 2);
                out.push(format!("{}@let __switch {}", " ".repeat(body_indent), attrs));
            } else {
                out.push(l.to_string());
            }
            j += 1;
        }
        return Some(Block { lines: out, end });
    }

    // @translations [LOCALE] with `locale:` sections of `key value` lines →
    // `@let t.key value` for the active locale
    if trimmed == "@translations" || trimmed.starts_with("@translations ") {
        let explicit = trimmed["@translations".len()..].trim();
        let page_lang = lines.iter().find_map(|l| {
            let rest = l.trim().strip_prefix("@page [")?;
            let attrs = rest.split(']').next()?;
            attrs
                .split(',')
                .find_map(|a| a.trim().strip_prefix("lang "))
                .map(|v| v.trim().to_string())
        });
        let active = if !explicit.is_empty() {
            explicit.to_string()
        } else {
            page_lang.unwrap_or_else(|| "en".to_string())
        };
        let mut locale = active.clone();
        let mut out = Vec::new();
        let mut others = Vec::new();
        for l in body {
            let t = l.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(name) = t.strip_suffix(':').filter(|n| !n.contains(' ')) {
                locale = name.to_string();
                if locale != active && !others.contains(&locale) {
                    others.push(locale.clone());
                }
                continue;
            }
            if locale == active
                && let Some((key, value)) = t.split_once(' ')
            {
                out.push(format!("{pad}@let t.{} {}", key, value.trim()));
            }
        }
        if !others.is_empty() {
            manual.push((
                i + 1,
                format!(
                    "kept the '{}' strings; move the other locales ({}) to JSON files and load \
                     them with `@data $t locales/$lang.json`",
                    active,
                    others.join(", ")
                ),
            ));
        }
        return Some(Block { lines: out, end });
    }

    // @match $v / @case a / @default → @if $v == "a" / @else if ... / @else
    if let Some(value) = trimmed.strip_prefix("@match ") {
        let value = value.trim();
        let case_indent = body.iter().find(|l| !l.trim().is_empty()).map(|l| indent_of(l));
        let mut out = Vec::new();
        let mut first = true;
        let mut j = 0;
        while j < body.len() {
            let l = body[j];
            let t = l.trim();
            if Some(indent_of(l)) == case_indent {
                let head = if let Some(case) = t.strip_prefix("@case ") {
                    let case = case.trim();
                    let quoted = if case.starts_with('"') {
                        case.to_string()
                    } else {
                        format!("\"{}\"", case)
                    };
                    let keyword = if first { "@if" } else { "@else if" };
                    first = false;
                    format!("{pad}{} {} == {}", keyword, value, quoted)
                } else if t == "@default" {
                    format!("{pad}@else")
                } else {
                    j += 1;
                    continue;
                };
                out.push(head);
                // The case body moves up one level, under the @if.
                let body_end = (j + 1..body.len())
                    .find(|&k| !body[k].trim().is_empty() && Some(indent_of(body[k])) <= case_indent)
                    .unwrap_or(body.len());
                out.extend(dedent_block(&body[j + 1..body_end], indent + 2));
                j = body_end;
            } else {
                j += 1;
            }
        }
        return Some(Block { lines: out, end });
    }

    // @theme / `name value` lines → `@let name value` and `@let --name value`
    if trimmed == "@theme" {
        let mut out = Vec::new();
        for l in body {
            if let Some((name, value)) = l.trim().split_once(' ') {
                let value = value.trim();
                out.push(format!("{pad}@let {} {}", name, value));
                out.push(format!("{pad}@let --{} {}", name, value));
                if name == "primary" || name == "theme-color" {
                    out.push(format!("{pad}@meta theme-color {}", value));
                }
            }
        }
        return Some(Block { lines: out, end });
    }

    // @json-ld with an indented JSON body → a script in @head
    if trimmed == "@json-ld" {
        let mut out = vec![
            format!("{pad}@head"),
            format!("{pad}  <script type=\"application/ld+json\">"),
        ];
        out.extend(dedent_block(body, indent).into_iter().map(|l| {
            if l.is_empty() {
                l
            } else {
                format!("{pad}    {}", l.trim_start())
            }
        }));
        out.push(format!("{pad}  </script>"));
        return Some(Block { lines: out, end });
    }

    if trimmed.starts_with("@manifest") {
        manual.push((
            i + 1,
            "@manifest was removed: write a manifest.json file and add \
             `<link rel=\"manifest\" href=\"manifest.json\">` to @head"
                .to_string(),
        ));
        return None;
    }

    // @defer → its body, dedented
    if trimmed == "@defer" || trimmed.starts_with("@defer ") {
        return Some(Block {
            lines: dedent_block(body, indent),
            end,
        });
    }

    // @with $x as y → @let y $x, body dedented
    if let Some(rest) = trimmed.strip_prefix("@with ") {
        let (source, alias) = rest.split_once(" as ")?;
        let mut out = vec![format!("{pad}@let {} {}", alias.trim(), source.trim())];
        out.extend(dedent_block(body, indent));
        return Some(Block { lines: out, end });
    }

    // @layout file with a body running to the end of the file →
    // @extends file, body dedented (its content fills @children).
    if let Some(rest) = trimmed.strip_prefix("@layout ") {
        if end < lines.len() && lines[end..].iter().any(|l| !l.trim().is_empty()) {
            manual.push((
                i + 1,
                "@layout was removed: replace it with @extends (content after the \
                 @layout block has to move into the layout)"
                    .to_string(),
            ));
            return None;
        }
        let mut out = vec![format!("{pad}@extends {}", rest.trim())];
        out.extend(dedent_block(body, indent));
        return Some(Block { lines: out, end });
    }

    // @scope sel / @starting-style → an @style block with the CSS at-rule
    if trimmed == "@scope" || trimmed.starts_with("@scope ") || trimmed == "@starting-style" {
        let prelude = if trimmed == "@starting-style" {
            "@starting-style {".to_string()
        } else {
            match trimmed["@scope".len()..].trim() {
                "" => "@scope {".to_string(),
                sel => format!("@scope ({}) {{", sel),
            }
        };
        let mut out = vec![format!("{pad}@style"), format!("{pad}  {prelude}")];
        out.extend(
            body.iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| format!("{pad}    {}", l.trim())),
        );
        out.push(format!("{pad}  }}"));
        return Some(Block { lines: out, end });
    }

    // @css-property --name / key value lines → @style with @property
    if let Some(name) = trimmed.strip_prefix("@css-property ") {
        let mut syntax = "\"*\"".to_string();
        let mut inherits = "false".to_string();
        let mut initial = None;
        for l in body {
            if let Some((key, value)) = l.trim().split_once(' ') {
                if value.contains('$') {
                    manual.push((
                        i + 1,
                        "@css-property with variables must be rewritten as @style by hand"
                            .to_string(),
                    ));
                    return None;
                }
                match key {
                    "syntax" => syntax = value.trim().to_string(),
                    "inherits" => inherits = value.trim().to_string(),
                    "initial-value" | "initial_value" => initial = Some(value.trim().to_string()),
                    _ => {}
                }
            }
        }
        let mut out = vec![
            format!("{pad}@style"),
            format!("{pad}  @property {} {{", name.trim()),
            format!("{pad}    syntax:{};", syntax),
            format!("{pad}    inherits:{};", inherits),
        ];
        if let Some(initial) = initial {
            out.push(format!("{pad}    initial-value:{};", initial));
        }
        out.push(format!("{pad}  }}"));
        return Some(Block { lines: out, end });
    }

    // @repeat N → @each $_ in 1..N (literal counts only)
    if let Some(rest) = trimmed.strip_prefix("@repeat ") {
        let Ok(count) = rest.trim().parse::<u64>() else {
            manual.push((
                i + 1,
                "@repeat with a variable count was removed: use @each $_ in 1..$n \
                 (note: 1..0 counts down, so guard a zero count with @if)"
                    .to_string(),
            ));
            return None;
        };
        if count == 0 {
            // Renders nothing: drop the block entirely.
            return Some(Block {
                lines: Vec::new(),
                end,
            });
        }
        let mut out = vec![format!("{pad}@each $_ in 1..{}", count)];
        // @repeat exposed $_count; @each doesn't, so inline it.
        out.extend(
            body.iter()
                .map(|l| replace_var(l, "_count", &count.to_string())),
        );
        return Some(Block { lines: out, end });
    }

    None
}

/// Move `@lang`, `@favicon`, `@canonical` and `@base` into the `@page`
/// line's attributes: `@page [lang en, favicon /f.png] Title`.
fn fold_head_directives(input: &str, manual: &mut Vec<(usize, String)>) -> (String, usize) {
    const FOLDED: &[&str] = &["lang", "favicon", "canonical", "base"];
    let lines: Vec<&str> = input.lines().collect();
    let mut attrs = Vec::new();
    let mut keep = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let folded = FOLDED.iter().find_map(|key| {
            let rest = line.strip_prefix(&format!("@{} ", key))?;
            Some(format!("{} {}", key, rest.trim()))
        });
        match folded {
            Some(attr) => attrs.push((i, attr)),
            None => keep.push(*line),
        }
    }
    if attrs.is_empty() {
        return (input.to_string(), 0);
    }
    let Some(page) = keep.iter().position(|l| l.starts_with("@page ")) else {
        for (i, _) in &attrs {
            manual.push((
                i + 1,
                "this directive now goes in @page's attributes, but the file has no @page line"
                    .to_string(),
            ));
        }
        return (input.to_string(), 0);
    };
    let list = attrs.iter().map(|(_, a)| a.as_str()).collect::<Vec<_>>().join(", ");
    let title = &keep[page]["@page ".len()..];
    let new_page = match title.strip_prefix('[') {
        Some(rest) => format!("@page [{}, {}", list, rest),
        None => format!("@page [{}] {}", list, title),
    };
    let mut out: Vec<String> = keep.iter().map(|l| l.to_string()).collect();
    out[page] = new_page;
    let mut text = out.join("\n");
    if input.ends_with('\n') {
        text.push('\n');
    }
    (text, attrs.len())
}

/// Single-line directive rewrites.
fn rewrite_directive_line(
    line: &str,
    indent: usize,
    idx: usize,
    manual: &mut Vec<(usize, String)>,
) -> String {
    let trimmed = line.trim_start();
    let pad = " ".repeat(indent);
    for old in ["@fn ", "@define ", "@mixin ", "@component "] {
        if let Some(rest) = trimmed.strip_prefix(old) {
            return format!("{pad}@let {}", rest);
        }
    }
    if let Some(rest) = trimmed.strip_prefix("@og ")
        && let Some((key, value)) = rest.trim().split_once(' ')
    {
        let value = value.trim();
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or(value);
        return format!("{pad}@meta og:{} {}", key, value);
    }
    if let Some(rest) = trimmed.strip_prefix("@debug ") {
        return format!("{pad}@warn {}", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("@log ") {
        let shown: Vec<String> = rest
            .split_whitespace()
            .map(|v| format!("{} = ${}", v.trim_start_matches('$'), v.trim_start_matches('$')))
            .collect();
        return format!("{pad}@warn {}", shown.join(", "));
    }
    // `@let x = $a ~ " " ~ $b` → `@let x "$a $b"` (`~` was removed:
    // interpolated strings do the same)
    if let Some(rest) = trimmed.strip_prefix("@let ")
        && let Some((name, value)) = rest.split_once(' ')
        && value.contains(" ~ ")
    {
        let expression = value.trim();
        let expression = expression.strip_prefix('=').unwrap_or(expression);
        let is_var_char = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
        let pieces: Vec<String> = expression
            .split(" ~ ")
            .map(|part| {
                let part = part.trim();
                if let Some(text) = part.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
                    text.to_string()
                } else if !part.contains(' ') && !part.contains('(') {
                    part.to_string()
                } else {
                    format!("${{{}}}", part)
                }
            })
            .collect();
        let mut joined = String::new();
        for (k, piece) in pieces.iter().enumerate() {
            // `$n` directly followed by text would read as a longer name.
            let next_continues_name = pieces
                .get(k + 1)
                .and_then(|next| next.chars().next())
                .is_some_and(is_var_char);
            if piece.starts_with('$') && !piece.starts_with("${") && next_continues_name {
                joined.push_str(&format!("${{{}}}", piece));
            } else {
                joined.push_str(piece);
            }
        }
        return format!("{pad}@let {} \"{}\"", name, joined);
    }
    // `@let x $a + 4` computed its value; computing now needs `=`.
    if let Some(rest) = trimmed.strip_prefix("@let ")
        && let Some((name, value)) = rest.split_once(' ')
        && is_old_arithmetic(value.trim())
    {
        return format!("{pad}@let {} = {}", name, value.trim());
    }
    if let Some(rest) = trimmed.strip_prefix("@env ") {
        let rest = rest.trim();
        let (var, default) = match rest.split_once(char::is_whitespace) {
            Some((var, default)) => (var, format!(" {}", default.trim())),
            None => (rest, String::new()),
        };
        let name = var.to_lowercase().replace('-', "_");
        return format!("{pad}@data ${} env:{}{}", name, var, default);
    }
    if let Some(rest) = trimmed.strip_prefix("@collection ") {
        let rest = rest.trim();
        let parsed = match rest.strip_prefix('$') {
            Some(named) => named
                .split_once(char::is_whitespace)
                .map(|(n, p)| (n.to_string(), p.trim().trim_matches('"').to_string())),
            None => rest
                .rsplit_once(" as ")
                .map(|(p, n)| (n.trim().to_string(), p.trim().trim_matches('"').to_string())),
        };
        if let Some((name, pattern)) = parsed {
            manual.push((
                idx + 1,
                format!(
                    "collection values are now `${name}.STEM.key` (they were `${name}_STEM_key`), \
                     and `${name}` lists the stems separated by commas"
                ),
            ));
            return format!("{pad}@data ${} {}", name, pattern);
        }
    }
    if trimmed.starts_with("@each ") && trimmed.trim_end().ends_with(']') && trimmed.contains("[page ") {
        manual.push((
            idx + 1,
            "@each pagination ([page N]) was removed: split the list or filter it with @if"
                .to_string(),
        ));
        return line.to_string();
    }
    if trimmed.starts_with("@fetch ") {
        manual.push((
            idx + 1,
            "@fetch was removed: download the data before building and load it with \
             `@data $name file.json`"
                .to_string(),
        ));
        return line.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("@font-face ")
        && let Some((name, url)) = rest.trim().split_once(' ')
    {
        let url = url.trim();
        let format = match url.rsplit('.').next() {
            Some("woff2") => " format('woff2')",
            Some("woff") => " format('woff')",
            Some("ttf") => " format('truetype')",
            Some("otf") => " format('opentype')",
            _ => "",
        };
        return format!(
            "{pad}@style\n{pad}  @font-face {{ font-family: '{name}'; src: url('{url}'){format}; font-display: swap; }}\n\
             {pad}@head\n{pad}  <link rel=\"preload\" href=\"{url}\" as=\"font\" crossorigin>"
        );
    }
    if trimmed.starts_with("@deprecated ") {
        return String::new();
    }
    if trimmed.starts_with("@breakpoint ") {
        manual.push((
            idx + 1,
            "@breakpoint was removed (its prefixes never took effect): write the media \
             query in an @style block"
                .to_string(),
        ));
        return line.to_string();
    }
    // @svg [attrs] file.svg → @image [inline, attrs] file.svg
    if let Some(rest) = trimmed.strip_prefix("@svg ") {
        let rest = rest.trim();
        let (attrs, file) = match rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            Some((attrs, file)) => (attrs.trim(), file.trim()),
            None => ("", rest),
        };
        let attrs: Vec<String> = split_top_level_commas(attrs)
            .into_iter()
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(|a| match a.split_once(' ') {
                Some((key @ ("class" | "id"), value)) => format!("{}={}", key, value.trim()),
                _ => a.to_string(),
            })
            .collect();
        let list = std::iter::once("inline".to_string())
            .chain(attrs)
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{pad}@image [{}] {}", list, file);
    }
    if let Some(rest) = trimmed.strip_prefix("@import ") {
        return format!("{pad}@include {}", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("@unless ") {
        return format!("{pad}@if not {}", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("@for ") {
        return format!("{pad}@each {}", rest);
    }
    if let Some(rest) = trimmed.strip_prefix("@use ") {
        let rest = rest.trim();
        let file = if let Some(q) = rest.strip_prefix('"') {
            q.split('"').next().unwrap_or("")
        } else {
            rest.split(|c: char| c.is_whitespace() || c == ',')
                .next()
                .unwrap_or("")
        };
        if file.is_empty() {
            manual.push((idx + 1, "could not read the file name in @use".to_string()));
            return line.to_string();
        }
        return format!("{pad}@include {}", file);
    }
    line.to_string()
}

/// Did the old `@let` evaluate this bare value? It did for one arithmetic
/// operator between two operands (`$base * 2`) and for `~` concatenation.
fn is_old_arithmetic(value: &str) -> bool {
    if value.starts_with(['=', '[', '"']) {
        return false;
    }
    if value.contains(" ~ ") {
        return true;
    }
    [" * ", " / ", " + ", " - "].iter().any(|op| {
        value.split_once(op).is_some_and(|(l, r)| {
            let operand = |s: &str| {
                let s = s.trim();
                s.parse::<f64>().is_ok() || (s.starts_with('$') && !s.contains(' '))
            };
            operand(l) && operand(r)
        })
    })
}

/// Rename element aliases wherever an element name can appear: at the start
/// of a line, after a `>` chain, or at the start of an inline `{@...}`.
fn rename_elements(line: &str, user_defined: &[&str]) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
        if bytes[i] == b'@' {
            let before = line[..i].trim_end();
            let at_element_position =
                before.is_empty() || before.ends_with('>') || line[..i].ends_with('{');
            if at_element_position {
                let name_end = line[i + 1..]
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                    .map_or(line.len(), |p| i + 1 + p);
                let name = &line[i + 1..name_end];
                let alias = ELEMENT_ALIASES
                    .iter()
                    .find(|(a, _)| *a == name && !user_defined.contains(a));
                if let Some((_, canonical)) = alias {
                    out.push('@');
                    out.push_str(canonical);
                    i = name_end;
                    continue;
                }
            }
        }
        let ch = line[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Apply attribute-level rewrites inside `[...]` regions. `depth` carries an
/// unclosed bracket over to the following (continuation) lines.
/// Attributes that became HTML when written `key value`, or were meant as
/// HTML but dropped; they are now written `key=value`.
const HTML_ATTRIBUTES: &[&str] = &[
    "accept", "action", "allow", "alt", "autocomplete", "blocking", "class", "cols", "colspan",
    "datetime", "decoding", "dir", "download", "enctype", "enterkeyhint", "fetchpriority", "for",
    "formaction", "formmethod", "formtarget", "headers", "high", "href", "hreflang", "id",
    "inputmode", "label", "lang", "list", "loading", "low", "max", "maxlength", "media", "method",
    "min", "name", "optimum", "pattern", "placeholder", "popover", "popovertarget",
    "popovertargetaction", "poster", "preload", "referrerpolicy", "rel", "role", "rows",
    "rowspan", "sandbox", "scope", "sizes", "span", "spellcheck", "src", "srcset", "start",
    "step", "tabindex", "target", "title", "translate", "type", "value",
];

/// Standard-library components, which forward attributes to an element.
const STD_COMPONENTS: &[&str] = &[
    "badge", "tag", "chip", "avatar", "spacer", "tooltip", "carousel", "breadcrumb",
];

/// Does this line's attribute list style an element (as opposed to passing
/// function parameters or directive options)?
fn takes_element_attributes(trimmed: &str, user_defined: &[&str]) -> bool {
    if trimmed.starts_with('[') {
        return true; // implicit @el
    }
    let Some(rest) = trimmed.strip_prefix('@') else {
        return false;
    };
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name == "let" {
        // An attribute bundle: `@let card [...]`
        return rest[name_end..]
            .trim_start()
            .split_once(' ')
            .is_some_and(|(_, v)| v.trim_start().starts_with('['));
    }
    !user_defined.contains(&name)
        && (htmlang_core::ast::ElementKind::from_name(name).is_some()
            || STD_COMPONENTS.contains(&name))
}

fn rewrite_attr_regions(
    line: &str,
    depth: &mut i32,
    element_attrs: &mut bool,
    user_defined: &[&str],
) -> String {
    let trimmed = line.trim_start();
    // In a text line, only inline elements (`{@abbr [...]}`) have attributes.
    let text_line = !trimmed.starts_with('@') && !trimmed.starts_with('[');
    if *depth == 0 && text_line && !line.contains("{@") {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut region = String::new();
    for ch in line.chars() {
        if *depth > 0 {
            match ch {
                '[' => *depth += 1,
                ']' => *depth -= 1,
                _ => {}
            }
            if *depth == 0 {
                out.push_str(&rewrite_attr_list(&region, *element_attrs));
                region.clear();
                out.push(ch);
            } else {
                region.push(ch);
            }
            continue;
        }
        if ch == '[' {
            if !text_line {
                *depth = 1;
            } else if let Some(brace) = out.rfind('{') {
                let name = out[brace + 1..].trim_end();
                if name.starts_with('@') && !name.contains(char::is_whitespace) {
                    *element_attrs = takes_element_attributes(name, user_defined);
                    *depth = 1;
                }
            }
        }
        out.push(ch);
    }
    out.push_str(&rewrite_attr_list(&region, *element_attrs));
    out
}

/// Rewrite one attribute list's contents (without the brackets).
fn rewrite_attr_list(list: &str, element_attrs: bool) -> String {
    if list.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(list.len());
    let mut first = true;
    let mut dropped_leading = false;
    for segment in split_top_level_commas(list) {
        // `critical` (inline styles) was removed without a replacement
        if segment.trim() == "critical" {
            dropped_leading |= first;
            continue;
        }
        let lead = segment.len() - segment.trim_start().len();
        let (ws, body) = segment.split_at(lead);
        if !first {
            out.push(',');
            out.push_str(ws);
        } else if !dropped_leading || ws.contains('\n') {
            out.push_str(ws);
        }
        first = false;
        // `...$bundle` spread → `$bundle`
        let body = body.strip_prefix("...$").map_or(body.to_string(), |b| format!("${b}"));
        // Built-in style attributes that became standard-library bundles
        let body = match body.trim_end() {
            "skeleton" | "no-scrollbar" | "truncate" => format!("${}", body),
            "grid" => "display grid".to_string(),
            _ => body,
        };
        // `blur N` / `backdrop-blur N` → the filter they generated
        let body = {
            let (key, value) = body.split_once(' ').unwrap_or((&body, ""));
            let (prefix, base) = key.rsplit_once(':').map_or(("", key), |(p, b)| (p, b));
            let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix}:") };
            let px = |v: &str| {
                let v = v.trim();
                if !v.is_empty() && v.parse::<f64>().is_ok() && v != "0" {
                    format!("{v}px")
                } else {
                    v.to_string()
                }
            };
            match base {
                "blur" => format!("{prefix}filter blur({})", px(value)),
                "backdrop-blur" => format!("{prefix}backdrop-filter blur({})", px(value)),
                _ => body.clone(),
            }
        };
        // `gradient A B [ANGLE]` → the `background` it generated
        let body = match body.strip_prefix("gradient ") {
            Some(v) => {
                let parts: Vec<&str> = v.split_whitespace().collect();
                let angle = parts
                    .get(2)
                    .filter(|a| a.ends_with("deg") || a.ends_with("turn") || a.ends_with("rad"));
                let gradient = match (parts.as_slice(), angle) {
                    ([a, b, ..], Some(angle)) => format!("{},{},{}", angle, a, b),
                    ([a, b, ..], None) => format!("{},{}", a, b),
                    ([a], _) => format!("{},transparent", a),
                    _ => String::new(),
                };
                format!("background linear-gradient({})", gradient)
            }
            None => body,
        };
        // `key COND ? A : B` → `key if(COND, A, B)`
        let body = match body.split_once(' ') {
            Some((key, value)) => match value.split_once(" ? ").and_then(|(cond, rest)| {
                rest.split_once(" : ").map(|(a, b)| (cond, a, b))
            }) {
                Some((cond, a, b)) => format!("{} if({}, {}, {})", key, cond.trim(), a.trim(), b.trim()),
                None => body,
            },
            None => body,
        };
        // HTML attributes with a value: `type email` → `type=email`
        // (already-converted `key=value` attributes are left alone)
        let body = match body.split_once(' ') {
            Some((key, value))
                if element_attrs
                    && !key.contains('=')
                    && (HTML_ATTRIBUTES.contains(&key)
                        || key.starts_with("aria-")
                        || key.starts_with("data-")) =>
            {
                format!("{}={}", key, value.trim())
            }
            _ => body,
        };
        // Renamed keys (keeping any `hover:` / `md:` style prefix)
        let key_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let (key, value) = body.split_at(key_end);
        let (prefix, base) = match key.rfind(':') {
            Some(p) => key.split_at(p + 1),
            None => ("", key),
        };
        match ATTR_RENAMES.iter().find(|(from, _)| *from == base) {
            Some((_, to)) => {
                out.push_str(prefix);
                out.push_str(to);
                out.push_str(value);
            }
            None => out.push_str(&body),
        }
    }
    out
}

/// Does the attribute list at the start of `rest` (if any) have `key`?
fn has_attr_key(rest: &str, key: &str) -> bool {
    let Some(list) = rest.strip_prefix('[').and_then(|r| r.split(']').next()) else {
        return false;
    };
    split_top_level_commas(list)
        .iter()
        .any(|attr| attr.split_whitespace().next() == Some(key))
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_quotes = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            _ if in_quotes => {}
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Rename filter aliases in `$var|filter` chains.
fn rename_filters(line: &str) -> String {
    if !line.contains('|') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find('|') {
        out.push_str(&rest[..=pos]);
        rest = &rest[pos + 1..];
        let name_end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if let Some((_, canonical)) = FILTER_ALIASES.iter().find(|(a, _)| *a == name) {
            out.push_str(canonical);
            rest = &rest[name_end..];
        }
    }
    out.push_str(rest);
    out
}

/// Turn `$name|filter:arg|...` chains into function calls: bare in
/// expressions (`@if`, `@else if`, `@assert`, `@let x = ...`), and as
/// `${...}` interpolation everywhere else.
fn convert_filters(line: &str) -> String {
    let line = rename_filters(line);
    if !line.contains('|') {
        return line;
    }
    let trimmed = line.trim_start();
    let expression_line = ["@if ", "@else if ", "@assert "]
        .iter()
        .any(|p| trimmed.starts_with(p))
        || trimmed
            .strip_prefix("@let ")
            .and_then(|r| r.split_once(' '))
            .is_some_and(|(_, v)| v.trim_start().starts_with('='));
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '.');
    let mut out = String::with_capacity(line.len() + 8);
    let mut rest = line.as_str();
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let name_end = after.find(|c: char| !is_name(c)).unwrap_or(after.len());
        let mut expr = format!("${}", &after[..name_end]);
        let mut tail = &after[name_end..];
        let mut converted = false;
        while name_end > 0 && tail.starts_with('|') {
            // A filter is a name (letters) with optional `:arg` parts.
            let body = &tail[1..];
            let name_len = body
                .find(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
                .unwrap_or(body.len());
            let mut filter_end = 1 + name_len;
            while tail[filter_end..].starts_with(':') {
                let arg = &tail[filter_end + 1..];
                let arg_len = arg
                    .find(|c: char| matches!(c, ':' | '|' | ',' | ']' | '}' | ')' | '!' | '?' | ';') || c.is_whitespace())
                    .unwrap_or(arg.len());
                filter_end += 1 + arg_len;
            }
            let mut parts = tail[1..filter_end].split(':');
            let name = parts.next().unwrap_or("");
            let args: Vec<String> = parts
                .map(|a| {
                    if a.parse::<f64>().is_ok() || a.starts_with('#') {
                        a.to_string()
                    } else {
                        format!("\"{}\"", a)
                    }
                })
                .collect();
            expr = if args.is_empty() {
                format!("{}({})", name, expr)
            } else {
                format!("{}({}, {})", name, expr, args.join(", "))
            };
            converted = true;
            tail = &tail[filter_end..];
        }
        if converted && !expression_line {
            out.push_str(&format!("${{{}}}", expr));
        } else {
            out.push_str(&expr);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Replace whole-token `$name` references with `value`.
fn replace_var(line: &str, name: &str, value: &str) -> String {
    let needle = format!("${name}");
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find(&needle) {
        let after = rest[pos + needle.len()..].chars().next();
        out.push_str(&rest[..pos]);
        if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            out.push_str(&needle);
        } else {
            out.push_str(value);
        }
        rest = &rest[pos + needle.len()..];
    }
    out.push_str(rest);
    out
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn starts_directive(trimmed: &str, directive: &str) -> bool {
    trimmed
        .strip_prefix(directive)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '[']))
}

/// Index just past the indented block owned by `lines[i]` (trailing blank
/// lines are not part of the block).
fn block_end(lines: &[&str], i: usize) -> usize {
    let indent = indent_of(lines[i]);
    let mut end = i + 1;
    let mut j = i + 1;
    while j < lines.len() {
        let l = lines[j];
        if l.trim().is_empty() {
            j += 1;
            continue;
        }
        if indent_of(l) <= indent {
            break;
        }
        j += 1;
        end = j;
    }
    end
}

/// If `lines[i]` opens a multi-line `"""` string (`@raw """` or
/// `@let name """`), the index just past its closing `"""`.
fn triple_quote_end(lines: &[&str], i: usize) -> Option<usize> {
    let trimmed = lines[i].trim();
    let open = trimmed.find("\"\"\"")?;
    let after = &trimmed[open + 3..];
    if after.contains("\"\"\"") {
        return None; // single-line
    }
    let mut j = i + 1;
    while j < lines.len() {
        if lines[j].trim() == "\"\"\"" {
            return Some(j + 1);
        }
        j += 1;
    }
    Some(lines.len())
}

/// Remove the extra indentation a block had under its (removed) owner.
fn dedent_block(body: &[&str], owner_indent: usize) -> Vec<String> {
    let body_indent = body
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(owner_indent);
    let shift = body_indent.saturating_sub(owner_indent);
    body.iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                l.get(shift..).unwrap_or(l.trim_start()).to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::upgrade;

    fn up(src: &str) -> String {
        upgrade(src).output
    }

    #[test]
    fn element_aliases() {
        assert_eq!(up("@col [padding 4]\n  @p hi"), "@column [padding 4]\n  @paragraph hi");
        assert_eq!(up("@el > @btn Go"), "@el > @button Go");
        assert_eq!(up("Say {@img a.png} now"), "Say {@image a.png} now");
        // Not aliases: @page, @paragraph, text mentioning @p
        assert_eq!(up("@page T\nemail me @p"), "@page T\nemail me @p");
        // A user function named like an alias is left alone.
        let src = "@let divider\n  @hr\n@divider";
        assert_eq!(up(src), src);
    }

    #[test]
    fn directives() {
        assert_eq!(up("@unless $x\n  hi"), "@if not $x\n  hi");
        assert_eq!(up("@fn card $t\n  @text $t"), "@let card $t\n  @text $t");
        assert_eq!(up("@define c [bold]\n@mixin m [italic]"), "@let c [bold]\n@let m [italic]");
        assert_eq!(up("@for $i in 1..3\n  $i"), "@each $i in 1..3\n  $i");
        assert_eq!(up("@use \"lib.hl\" a, b"), "@include lib.hl");
        assert_eq!(up("@import theme.hl\n@import \"ui.hl\" as ui"), "@include theme.hl\n@include \"ui.hl\" as ui");
        assert_eq!(
            up("@repeat 2\n  @text $_count"),
            "@each $_ in 1..2\n  @text 2"
        );
    }

    #[test]
    fn switch_to_match() {
        assert_eq!(
            up("@switch $v\n  @case a [color red]\n    @text A\n  @default [color blue]"),
            "@match $v\n  @case a\n    @let __switch [color red]\n    @text A\n  @default\n    @let __switch [color blue]"
        );
    }

    #[test]
    fn with_and_layout() {
        assert_eq!(up("@with $a as b\n  @text $b"), "@let b $a\n@text $b");
        assert_eq!(up("@defer Loading\n  @text x"), "@text x");
        assert_eq!(up("@layout base.hl\n  @text x\n"), "@extends base.hl\n@text x\n");
        let r = upgrade("@layout base.hl\n  @text x\n@text after");
        assert_eq!(r.output, "@layout base.hl\n  @text x\n@text after");
        assert_eq!(r.manual.len(), 1);
    }

    #[test]
    fn css_directives_become_style() {
        assert_eq!(
            up("@scope .card\n  .t { color: red; }"),
            "@style\n  @scope (.card) {\n    .t { color: red; }\n  }"
        );
        assert_eq!(
            up("@css-property --x\n  syntax \"<color>\"\n  initial-value #000"),
            "@style\n  @property --x {\n    syntax:\"<color>\";\n    inherits:false;\n    initial-value:#000;\n  }"
        );
    }

    #[test]
    fn attributes_and_filters() {
        assert_eq!(
            up("@el [...$card, animate fade 1s, hover:inset-area top] x"),
            "@el [$card, animation fade 1s, hover:position-area top] x"
        );
        assert_eq!(up("@el [\n  animate spin 1s\n]"), "@el [\n  animation spin 1s\n]");
        assert_eq!(up("@text $name|upper|len"), "@text ${length(uppercase($name))}");
        // Text that merely mentions a renamed word is untouched.
        assert_eq!(up("@text [bold] please animate this"), "@text [bold] please animate this");
    }

    #[test]
    fn standard_library_migrations() {
        assert_eq!(
            up("@el [skeleton, height 20]\n@el [no-scrollbar]"),
            "@el [$skeleton, height 20]\n@el [$no-scrollbar]"
        );
        assert_eq!(
            up("@el [gradient #f00 #00f 45deg, padding 4]"),
            "@el [background linear-gradient(45deg,#f00,#00f), padding 4]"
        );
        assert_eq!(up("@tooltip Hover me"), "@tooltip [tip Hover me] Hover me");
        assert_eq!(up("@tooltip A tooltip text"), "@tooltip [tip A tooltip text] A tooltip text");
        assert_eq!(up("@tooltip [tip X] Y"), "@tooltip [tip X] Y");
        assert_eq!(
            up("@breadcrumb\n  @link / Home\n  Current"),
            "@breadcrumb\n  @item > @link / Home\n  @item Current"
        );
    }

    #[test]
    fn head_directives_and_debugging() {
        assert_eq!(
            up("@page Home\n@lang en\n@favicon /f.png\n@text hi\n"),
            "@page [lang en, favicon /f.png] Home\n@text hi\n"
        );
        assert_eq!(up("@og title \"My Page\""), "@meta og:title My Page");
        assert_eq!(up("@debug hi $x\n@log $a $b"), "@warn hi $x\n@warn a = $a, b = $b");
        assert_eq!(up("@component card $t\n  @text $t"), "@let card $t\n  @text $t");
        assert_eq!(up("@el [color $on ? green : gray]"), "@el [color if($on, green, gray)]");
        let r = upgrade("@lang en\n@text hi");
        assert_eq!(r.manual.len(), 1);
    }

    #[test]
    fn html_attributes_use_equals() {
        assert_eq!(
            up("@input [type email, name e, required, padding 8]"),
            "@input [type=email, name=e, required, padding 8]"
        );
        assert_eq!(up("[id main, aria-label Close]\n  x"), "[id=main, aria-label=Close]\n  x");
        // Idempotent: a second run changes nothing.
        let once = up("@el [aria-label Main menu]");
        assert_eq!(once, "@el [aria-label=Main menu]");
        assert_eq!(up(&once), once);
        assert_eq!(up("@let card [class note, padding 4]"), "@let card [class=note, padding 4]");
        // Function parameters and directive options are left alone.
        assert_eq!(up("@let card $title\n  @text $title\n@card [title Hi]"), "@let card $title\n  @text $title\n@card [title Hi]");
        assert_eq!(up("@page [lang en] Home"), "@page [lang en] Home");
        // Inline elements in text, and `@let` inside a raw string.
        assert_eq!(
            up("Read {@abbr [title HyperText] HTML} now"),
            "Read {@abbr [title=HyperText] HTML} now"
        );
        assert_eq!(
            up("@raw \"\"\"\n@let button $x\n\"\"\"\n@button [type submit] Go"),
            "@raw \"\"\"\n@let button $x\n\"\"\"\n@button [type=submit] Go"
        );
        // Multi-line lists keep their element context.
        assert_eq!(up("@input [\n  type email,\n  padding 8\n]"), "@input [\n  type=email,\n  padding 8\n]");
    }

    #[test]
    fn computed_let_needs_equals() {
        assert_eq!(up("@let gap $base + 4"), "@let gap = $base + 4");
        assert_eq!(up("@let full $a ~ \" \" ~ $b"), "@let full \"$a $b\"");
        assert_eq!(up("@let x = $n ~ \"px\""), "@let x \"${$n}px\"");
        assert_eq!(up("@let x = 1 + 2"), "@let x = 1 + 2");
        assert_eq!(up("@let area 1 / span 2"), "@let area 1 / span 2");
        assert_eq!(up("@let card $title $tone=primary\n  @text $title"), "@let card $title $tone=primary\n  @text $title");
    }

    #[test]
    fn data_directives() {
        assert_eq!(up("@env API_URL http://x"), "@data $api_url env:API_URL http://x");
        assert_eq!(up("@collection $posts \"posts/*.json\""), "@data $posts posts/*.json");
        assert_eq!(
            up("@page [lang fr] T\n@translations\n  en:\n    hi Hello\n  fr:\n    hi Bonjour\n@text $t.hi"),
            "@page [lang fr] T\n@let t.hi Bonjour\n@text $t.hi"
        );
        assert_eq!(upgrade("@translations\n  en:\n    hi Hello\n  fr:\n    hi Salut").manual.len(), 1);
        assert_eq!(upgrade("@fetch $d http://x").manual.len(), 1);
    }

    #[test]
    fn css_alias_attributes() {
        assert_eq!(
            up("@el [gap-x 4, hover:shadow 0 1px red, blur 4, backdrop-blur 2px, truncate, critical]"),
            "@el [column-gap 4, hover:box-shadow 0 1px red, filter blur(4px), backdrop-filter blur(2px), $truncate]"
        );
        assert_eq!(up("@el [critical, padding 4]"), "@el [padding 4]");
        assert_eq!(up("@el [grid, grid-cols 3]"), "@el [display grid, grid-cols 3]");
    }

    #[test]
    fn small_directives() {
        assert_eq!(
            up("@match $v\n  @case a\n    @text A\n  @case b c\n    @text B\n  @default\n    @text D"),
            "@if $v == \"a\"\n  @text A\n@else if $v == \"b c\"\n  @text B\n@else\n  @text D"
        );
        assert_eq!(
            up("@theme\n  primary #3b82f6\n  radius 8"),
            "@let primary #3b82f6\n@let --primary #3b82f6\n@meta theme-color #3b82f6\n@let radius 8\n@let --radius 8"
        );
        assert_eq!(
            up("@json-ld\n  {\"a\": 1}"),
            "@head\n  <script type=\"application/ld+json\">\n    {\"a\": 1}\n  </script>"
        );
        assert_eq!(
            up("@font-face Inter fonts/inter.woff2"),
            "@style\n  @font-face { font-family: 'Inter'; src: url('fonts/inter.woff2') format('woff2'); font-display: swap; }\n@head\n  <link rel=\"preload\" href=\"fonts/inter.woff2\" as=\"font\" crossorigin>"
        );
        assert_eq!(up("@deprecated old\n@let x 1"), "\n@let x 1");
        assert_eq!(upgrade("@breakpoint tablet 600").manual.len(), 1);
    }

    #[test]
    fn filters_become_functions() {
        assert_eq!(up("@text $name|uppercase!"), "@text ${uppercase($name)}!");
        assert_eq!(
            up("@el [color $base|darken:10, background $base|mix:#ffffff:50]"),
            "@el [color ${darken($base, 10)}, background ${mix($base, #ffffff, 50)}]"
        );
        assert_eq!(up("@if $name|length > 3\n  x"), "@if length($name) > 3\n  x");
        assert_eq!(up("@let short = $title|truncate:5"), "@let short = truncate($title, 5)");
        assert_eq!(up("Hi $who|default:friend"), "Hi ${default($who, \"friend\")}");
        assert_eq!(up("a | b $x | c"), "a | b $x | c");
    }

    #[test]
    fn svg_becomes_inline_image() {
        assert_eq!(up("@svg icons/a.svg"), "@image [inline] icons/a.svg");
        assert_eq!(
            up("@svg [width 24, color red, class icon] a.svg"),
            "@image [inline, width 24, color red, class=icon] a.svg"
        );
    }

    #[test]
    fn verbatim_regions_are_untouched() {
        let src = "@script\n  for (x of y) {}\n@raw \"\"\"\n@unless\n\"\"\"\n@style\n  .p { }";
        assert_eq!(up(src), src);
        assert_eq!(up("@script [src app.js, defer]"), "@script [src=app.js, defer]");
    }
}
