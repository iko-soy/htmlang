mod cli;

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Mutex;

struct DiagnosticJson {
    file: String,
    line: usize,
    severity: String,
    message: String,
}

fn severity_label(s: htmlang::parser::Severity) -> &'static str {
    match s {
        htmlang::parser::Severity::Error => "error",
        htmlang::parser::Severity::Warning => "warning",
        htmlang::parser::Severity::Info => "info",
        htmlang::parser::Severity::Help => "help",
    }
}

#[derive(Default)]
struct CompileConfig<'a> {
    dev: bool,
    error_overlay: bool,
    check_only: bool,
    output_path: Option<&'a str>,
    format_json: bool,
    json_collector: Option<&'a Mutex<Vec<DiagnosticJson>>>,
    minify: bool,
    strict: bool,
    partial: bool,
}

fn compile(input_path: &str, cfg: &CompileConfig) -> (bool, Vec<PathBuf>) {
    let input = match fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(e) => {
            if cfg.format_json {
                if let Some(collector) = cfg.json_collector {
                    collector.lock().unwrap().push(DiagnosticJson {
                        file: input_path.to_string(),
                        line: 0,
                        severity: "error".to_string(),
                        message: format!("{}", e),
                    });
                }
            } else {
                eprintln!("error: {}: {}", input_path, e);
            }
            return (true, vec![]);
        }
    };

    let base = Path::new(input_path).parent();
    let result = htmlang::parser::parse_with_base(&input, base);

    if cfg.format_json {
        if let Some(collector) = cfg.json_collector {
            let mut collected = collector.lock().unwrap();
            for d in &result.diagnostics {
                collected.push(DiagnosticJson {
                    file: input_path.to_string(),
                    line: d.line,
                    severity: severity_label(d.severity).to_string(),
                    message: d.message.clone(),
                });
            }
        }
    } else {
        for d in &result.diagnostics {
            let prefix = severity_label(d.severity);
            if let Some(col) = d.column {
                eprintln!("{}: line {}:{}: {}", prefix, d.line, col + 1, d.message);
            } else {
                eprintln!("{}: line {}: {}", prefix, d.line, d.message);
            }
            if let Some(ref src) = d.source_line {
                eprintln!("  | {}", src);
                if let Some(col) = d.column {
                    eprintln!("  | {}^", " ".repeat(col));
                }
            }
        }
    }

    let has_errors = result.diagnostics.iter().any(|d| {
        d.severity == htmlang::parser::Severity::Error
            || (cfg.strict && d.severity == htmlang::parser::Severity::Warning)
    });

    let out_path = match cfg.output_path {
        Some(p) => PathBuf::from(p),
        None => Path::new(input_path).with_extension("html"),
    };

    if !cfg.check_only {
        if has_errors {
            if cfg.error_overlay {
                let error_html = generate_error_overlay(&result.diagnostics, input_path);
                let _ = fs::write(&out_path, &error_html);
            }
        } else {
            let html = htmlang::codegen::generate_with(
                &result.document,
                &htmlang::codegen::CodegenOptions {
                    dev: cfg.dev,
                    partial: cfg.partial,
                    minify: cfg.minify,
                },
            );
            match fs::write(&out_path, &html) {
                Ok(()) => eprintln!("wrote {}", out_path.display()),
                Err(e) => eprintln!("error: {}: {}", out_path.display(), e),
            }
            // Generate source map alongside HTML
            if cfg.dev {
                let map_path = out_path.with_extension("html.map");
                // Map the HTML that was actually written, so line numbers
                // match even with --minify.
                let source_map = htmlang::codegen::source_map_for_html(
                    &html,
                    &Path::new(input_path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                );
                let _ = fs::write(&map_path, &source_map);
            }
        }
    }

    (has_errors, result.included_files)
}

fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}


fn json_array(items: impl Iterator<Item = String>) -> String {
    let inner: Vec<String> = items.collect();
    format!("[{}]", inner.join(","))
}

fn json_object(fields: &[(&str, String)]) -> String {
    let inner: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}:{}", json_escape_string(k), v))
        .collect();
    format!("{{{}}}", inner.join(","))
}

fn print_json_diagnostics(diagnostics: &[DiagnosticJson]) {
    let arr = json_array(diagnostics.iter().map(|d| {
        json_object(&[
            ("file", json_escape_string(&d.file)),
            ("line", d.line.to_string()),
            ("severity", json_escape_string(&d.severity)),
            ("message", json_escape_string(&d.message)),
        ])
    }));
    println!("{}", json_object(&[("diagnostics", arr)]));
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

fn copy_non_hl_files(src_dir: &Path, out_dir: &Path) {
    let skip = out_dir.canonicalize().ok();
    // Building in place: the assets are already where they belong, and
    // copying a file onto itself truncates it.
    if skip.is_some() && src_dir.canonicalize().ok() == skip {
        return;
    }
    copy_non_hl_recursive(src_dir, src_dir, out_dir, skip.as_deref());
}

/// True for `page.html` / `page.html.map` files that sit next to a
/// `page.hl` source — stale compiler output, not a static asset.
fn is_generated_output(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let stem = name
        .strip_suffix(".html.map")
        .or_else(|| name.strip_suffix(".html"));
    stem.is_some_and(|stem| path.with_file_name(format!("{stem}.hl")).is_file())
}

fn copy_non_hl_recursive(base: &Path, dir: &Path, out_dir: &Path, skip_canonical: Option<&Path>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories (.htmlang-cache, .git, etc.)
                if path
                    .file_name()
                    .is_some_and(|n| n.to_str().is_some_and(|s| s.starts_with('.')))
                {
                    continue;
                }
                // Skip the output directory to avoid copying it into itself
                if let Some(skip) = skip_canonical
                    && path.canonicalize().ok().as_deref() == Some(skip)
                {
                    continue;
                }
                copy_non_hl_recursive(base, &path, out_dir, skip_canonical);
            } else if path.is_file()
                && path.extension().is_none_or(|e| e != "hl")
                && !is_generated_output(&path)
            {
                let rel = path.strip_prefix(base).unwrap_or(&path);
                let dest = out_dir.join(rel);
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if dest.canonicalize().ok() == path.canonicalize().ok() {
                    continue;
                }
                match fs::copy(&path, &dest) {
                    Ok(_) => eprintln!("copied {}", dest.display()),
                    Err(e) => eprintln!("error: copy {}: {}", dest.display(), e),
                }
            }
        }
    }
}

fn generate_error_overlay(diagnostics: &[htmlang::parser::Diagnostic], file: &str) -> String {
    let mut errors = String::new();
    for d in diagnostics {
        let prefix = severity_label(d.severity);
        let escaped = d
            .message
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let location = match d.column {
            Some(col) => format!("line {}:{}", d.line, col + 1),
            None => format!("line {}", d.line),
        };
        errors.push_str(&format!(
            "<div class=\"entry\"><span class=\"badge {}\">{}</span> <span class=\"loc\">{}</span> {}",
            prefix, prefix, location, escaped
        ));
        if let Some(ref src) = d.source_line {
            let src_esc = src
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            errors.push_str(&format!("<pre class=\"src\">{}</pre>", src_esc));
            if let Some(col) = d.column {
                // Render a caret indicator underneath the source line.
                let caret = format!("{}^", " ".repeat(col));
                errors.push_str(&format!("<pre class=\"caret\">{}</pre>", caret));
            }
        }
        errors.push_str("</div>");
    }
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Build Error</title><style>
*{{margin:0;box-sizing:border-box}}
body{{background:#1a1a2e;color:#eee;font-family:ui-monospace,monospace;padding:2rem}}
h1{{color:#ff6b6b;margin-bottom:1rem;font-size:1.5rem}}
.file{{color:#888;margin-bottom:1.5rem;font-size:0.9rem}}
.entry{{padding:0.75rem 0;border-bottom:1px solid #333}}
.loc{{color:#9ca3af;margin-right:6px}}
.badge{{display:inline-block;padding:2px 8px;border-radius:4px;font-size:0.8rem;margin-right:8px}}
.badge.error{{background:#c0392b;color:white}}
.badge.warning{{background:#f39c12;color:white}}
.badge.info{{background:#2563eb;color:white}}
.badge.help{{background:#16a34a;color:white}}
.src{{margin-top:0.5rem;padding:0.4rem 0.6rem;background:#0f0f1e;border-radius:4px;color:#ddd;white-space:pre-wrap}}
.caret{{margin:0;padding:0 0.6rem;color:#ff6b6b;white-space:pre}}
</style></head><body>
<h1>Build Error</h1>
<div class="file">{file}</div>
{errors}
</body></html>"#,
        file = file,
        errors = errors,
    )
}

fn collect_hl_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_hl_recursive_inner(dir, &mut files);
    files.sort();
    files
}

fn collect_hl_recursive_inner(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories (.htmlang-cache, .git, etc.)
                if path
                    .file_name()
                    .is_some_and(|n| n.to_str().is_some_and(|s| s.starts_with('.')))
                {
                    continue;
                }
                collect_hl_recursive_inner(&path, files);
            } else if path.is_file() && path.extension().is_some_and(|e| e == "hl") {
                files.push(path);
            }
        }
    }
}

fn lint_file(path: &str) -> Vec<String> {
    let input = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return vec![format!("error: {}: {}", path, e)],
    };
    let base = Path::new(path).parent();
    let result = htmlang::parser::parse_with_base(&input, base);
    let lint = htmlang::parser::lint(&result.document.nodes);
    result
        .diagnostics
        .iter()
        .chain(&lint)
        .map(|d| format!("{}:{}:{}: {}", path, d.line, severity_label(d.severity), d.message))
        .collect()
}

/// Whether the build cache record at `path` matches `key` and every
/// dependency it lists still has the recorded content hash.
fn build_cache_is_fresh(path: &Path, key: u64) -> bool {
    let Ok(record) = fs::read_to_string(path) else {
        return false;
    };
    let mut lines = record.lines();
    if lines.next() != Some(key.to_string().as_str()) {
        return false;
    }
    lines.all(|line| {
        line.split_once(' ').is_some_and(|(hash, dep)| {
            fs::read(dep).is_ok_and(|content| hash_bytes(&content).to_string() == hash)
        })
    })
}

/// Record a successful build: the source+flags `key`, then one
/// `<content hash> <path>` line per included file.
fn write_build_cache(path: &Path, key: u64, deps: &[PathBuf]) {
    let mut record = key.to_string();
    for dep in deps {
        if let Ok(content) = fs::read(dep) {
            record.push_str(&format!("\n{} {}", hash_bytes(&content), dep.display()));
        }
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, record);
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Split a stylesheet into top-level segments: each rule or at-rule block,
/// together with the whitespace before it, is one segment, so the segments
/// concatenate back to the original text.
fn css_segments(css: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    for (i, c) in css.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    segments.push(&css[start..=i]);
                    start = i + 1;
                }
            }
            _ => {}
        }
    }
    if start < css.len() {
        segments.push(&css[start..]);
    }
    segments
}

/// Locate the first `<style>...</style>` body in `html` as a byte range.
fn style_body_range(html: &str) -> Option<(usize, usize)> {
    let start = html.find("<style>")? + "<style>".len();
    let end = start + html[start..].find("</style>")?;
    Some((start, end))
}

/// Extract shared CSS rules across multiple HTML files and write shared.css
fn extract_shared_css(html_files: &[PathBuf], out_dir: &Path) {
    let pages: Vec<(&PathBuf, String)> = html_files
        .iter()
        .filter_map(|f| fs::read_to_string(f).ok().map(|html| (f, html)))
        .collect();
    if pages.len() < 2 {
        return;
    }

    // A rule is shared when it appears (verbatim) in every page. Keep the
    // order of the first page so the cascade stays deterministic.
    let rule_sets: Vec<std::collections::HashSet<&str>> = pages
        .iter()
        .map(|(_, html)| {
            style_body_range(html)
                .map(|(s, e)| css_segments(&html[s..e]).into_iter().map(str::trim).collect())
                .unwrap_or_default()
        })
        .collect();
    let first_css = style_body_range(&pages[0].1).map_or("", |(s, e)| &pages[0].1[s..e]);
    let mut shared_rules: Vec<&str> = Vec::new();
    for rule in css_segments(first_css).into_iter().map(str::trim) {
        if rule.ends_with('}')
            && !shared_rules.contains(&rule)
            && rule_sets.iter().all(|set| set.contains(rule))
        {
            shared_rules.push(rule);
        }
    }
    if shared_rules.is_empty() {
        return;
    }
    let shared_css_path = out_dir.join("shared.css");
    if fs::write(&shared_css_path, shared_rules.join("\n")).is_err() {
        return;
    }
    eprintln!(
        "extracted {} shared CSS rules to {}",
        shared_rules.len(),
        shared_css_path.display()
    );

    // Remove shared rules from individual files and inject <link> tag
    for (file, html) in &pages {
        let Some((style_start, style_end)) = style_body_range(html) else {
            continue;
        };
        let filtered: String = css_segments(&html[style_start..style_end])
            .into_iter()
            .filter(|seg| !shared_rules.contains(&seg.trim()))
            .collect();
        // Link relative to the page so nested pages (blog/post.html) resolve.
        let depth = file
            .strip_prefix(out_dir)
            .map_or(0, |rel| rel.components().count().saturating_sub(1));
        let href = format!("{}shared.css", "../".repeat(depth));
        let link_tag = format!("<link rel=\"stylesheet\" href=\"{}\">", href);
        let head = &html[..style_start - "<style>".len()];
        let link = if head.contains(&link_tag) { "" } else { &link_tag };
        let new_html = format!(
            "{}{}<style>{}</style>{}",
            head,
            link,
            filtered,
            &html[style_end + "</style>".len()..],
        );
        let _ = fs::write(file, new_html);
    }
}

fn open_in_browser(port: u16) {
    let url = format!("http://127.0.0.1:{}", port);
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(&url).spawn();
}

// ---------------------------------------------------------------------------
// Config file support (htmlang.toml)
// ---------------------------------------------------------------------------

struct ProjectConfig {
    output: Option<String>,
    port: u16,
    variables: Vec<(String, String)>,
    breakpoints: Vec<(String, String)>,
    // Build options (can be overridden by CLI flags)
    dev: Option<bool>,
    minify: Option<bool>,
    strict: Option<bool>,
    // Watch options
    debounce_ms: u64,
}

fn load_config(target: &Path) -> ProjectConfig {
    let mut config = ProjectConfig {
        output: None,
        port: 3000,
        variables: Vec::new(),
        breakpoints: Vec::new(),
        dev: None,
        minify: None,
        strict: None,
        debounce_ms: 50,
    };

    let config_path = if target.is_dir() {
        target.join("htmlang.toml")
    } else {
        target
            .parent()
            .unwrap_or(Path::new("."))
            .join("htmlang.toml")
    };

    let content = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return config,
    };

    let mut section = "";
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = &trimmed[1..trimmed.len() - 1];
            if !matches!(section, "variables" | "breakpoints" | "build" | "watch") {
                eprintln!(
                    "warning: {}:{}: unknown section '[{}]' (expected: variables, breakpoints, build, watch)",
                    config_path.display(),
                    line_num + 1,
                    section
                );
            }
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match section {
                "" => match key {
                    "output" => config.output = Some(value.to_string()),
                    "port" => config.port = value.parse().unwrap_or(3000),
                    _ => {
                        eprintln!(
                            "warning: {}:{}: unknown key '{}' (expected: output, port)",
                            config_path.display(),
                            line_num + 1,
                            key
                        );
                    }
                },
                "build" => match key {
                    "dev" => config.dev = Some(value == "true"),
                    "minify" => config.minify = Some(value == "true"),
                    "strict" => config.strict = Some(value == "true"),
                    _ => {
                        eprintln!(
                            "warning: {}:{}: unknown build key '{}' (expected: dev, minify, strict)",
                            config_path.display(),
                            line_num + 1,
                            key
                        );
                    }
                },
                "watch" => match key {
                    "debounce_ms" => config.debounce_ms = value.parse().unwrap_or(50),
                    _ => {
                        eprintln!(
                            "warning: {}:{}: unknown watch key '{}' (expected: debounce_ms)",
                            config_path.display(),
                            line_num + 1,
                            key
                        );
                    }
                },
                "variables" => {
                    config.variables.push((key.to_string(), value.to_string()));
                }
                "breakpoints" => {
                    config
                        .breakpoints
                        .push((key.to_string(), value.to_string()));
                }
                _ => {} // already warned about unknown section
            }
        }
    }

    config
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut watch = false;
    let mut serve = false;
    let mut dev = false;
    let mut check = false;
    let mut format_json = false;
    let mut strict = false;
    let mut open_browser = false;
    let mut partial = false;
    let mut port: u16 = 3000;
    let mut output_path: Option<String> = None;
    let mut input_path = None;

    // A bare word that isn't a path is a mistyped (or removed) subcommand.
    if let Some(first) = args.get(1)
        && !first.starts_with('-')
        && !cli::COMMANDS.contains(&first.as_str())
        && !first.ends_with(".hl")
        && !Path::new(first).exists()
    {
        eprintln!(
            "error: unknown command '{}'\ncommands: {}",
            first,
            cli::COMMANDS.join(", ")
        );
        process::exit(1);
    }

    // Handle "lsp" subcommand — launch the LSP server from the main binary
    if args.len() >= 2 && args[1] == "lsp" {
        cli::run_lsp();
        return;
    }



    // Handle "fmt" subcommand
    if args.len() >= 3 && args[1] == "fmt" {
        let file = &args[2];
        match fs::read_to_string(file) {
            Ok(input) => {
                let formatted = htmlang::fmt::format(&input);
                match fs::write(file, &formatted) {
                    Ok(()) => eprintln!("formatted {}", file),
                    Err(e) => {
                        eprintln!("error: {}: {}", file, e);
                        process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("error: {}: {}", file, e);
                process::exit(1);
            }
        }
        return;
    }

    // Handle "build" subcommand
    if args.len() >= 2 && args[1] == "build" {
        let mut src_dir = None;
        let mut out_dir = None;
        let mut build_minify = false;
        let mut build_strict = false;
        let mut shared_css = false;
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "-o" | "--output" => {
                    i += 1;
                    out_dir = args.get(i).map(|s| s.as_str());
                }
                "--minify" => build_minify = true,
                "--strict" => build_strict = true,
                "--shared-css" => shared_css = true,
                _ if src_dir.is_none() => src_dir = Some(args[i].as_str()),
                _ => {
                    eprintln!("unknown argument: {}", args[i]);
                    process::exit(1);
                }
            }
            i += 1;
        }
        let src = src_dir.unwrap_or(".");
        let dir = Path::new(src);
        if !dir.is_dir() {
            eprintln!("error: '{}' is not a directory", src);
            process::exit(1);
        }
        // Load project config — CLI flags override config file
        let config = load_config(dir);
        let build_minify = build_minify || config.minify.unwrap_or(false);
        let build_strict = build_strict || config.strict.unwrap_or(false);
        let out_dir = out_dir.or(config.output.as_deref()).or(Some("out"));
        let hl_files = collect_hl_files_recursive(dir);
        if hl_files.is_empty() {
            eprintln!("no .hl files found in {}", src);
            process::exit(1);
        }
        // Create output dir if needed
        if let Some(out) = out_dir {
            let _ = fs::create_dir_all(out);
        }
        // Pre-create output directories for each file (must be done before parallel compilation)
        let effective_outs: Vec<Option<String>> = hl_files
            .iter()
            .map(|file| {
                out_dir.map(|o| {
                    let rel = file.strip_prefix(dir).unwrap_or(file);
                    let out_path = Path::new(o).join(rel).with_extension("html");
                    if let Some(parent) = out_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    out_path.to_string_lossy().to_string()
                })
            })
            .collect();

        // Build content hash cache for incremental compilation
        let cache_dir = dir.join(".htmlang-cache");
        let _ = fs::create_dir_all(&cache_dir);

        // Compile files in parallel (with incremental skip for unchanged files)
        let build_start = std::time::Instant::now();
        let any_errors = std::sync::atomic::AtomicBool::new(false);
        let skipped = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for (file, effective_out) in hl_files.iter().zip(effective_outs.iter()) {
                let any_errors = &any_errors;
                let skipped = &skipped;
                let cache_dir = &cache_dir;
                s.spawn(move || {
                    // Content hash-based caching: skip if neither the file, the
                    // files it includes, nor the build flags have changed.
                    let rel = file.strip_prefix(dir).unwrap_or(file);
                    let hash_path = cache_dir.join(rel).with_extension("hl.hash");
                    let cache_key = fs::read(file).ok().map(|content| {
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        content.hash(&mut hasher);
                        (build_minify, build_strict, effective_out).hash(&mut hasher);
                        hasher.finish()
                    });
                    if !shared_css
                        && let Some(key) = cache_key
                        && build_cache_is_fresh(&hash_path, key)
                    {
                        // Also verify output exists
                        let out_exists = effective_out
                            .as_ref()
                            .map_or(file.with_extension("html").exists(), |p| {
                                Path::new(p.as_str()).exists()
                            });
                        if out_exists {
                            skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return;
                        }
                    }
                    let path_str = file.to_string_lossy().to_string();
                    let (has_errors, included) = compile(
                        &path_str,
                        &CompileConfig {
                            output_path: effective_out.as_deref(),
                            minify: build_minify,
                            strict: build_strict,
                            ..Default::default()
                        },
                    );
                    if has_errors {
                        any_errors.store(true, std::sync::atomic::Ordering::Relaxed);
                        // Never let a failed build be skipped next time.
                        let _ = fs::remove_file(&hash_path);
                    } else if let Some(key) = cache_key {
                        write_build_cache(&hash_path, key, &included);
                    }
                });
            }
        });
        let build_elapsed = build_start.elapsed();
        let skipped_count = skipped.load(std::sync::atomic::Ordering::Relaxed);
        let compiled_count = hl_files.len() - skipped_count;

        // Report build performance
        let total_output_size: usize = effective_outs
            .iter()
            .filter_map(|p| p.as_ref())
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len() as usize)
            .sum();
        eprintln!(
            "built {} files in {:.2}s ({}){}",
            compiled_count,
            build_elapsed.as_secs_f64(),
            format_bytes(total_output_size),
            if skipped_count > 0 {
                format!(", {} skipped", skipped_count)
            } else {
                String::new()
            },
        );
        if any_errors.load(std::sync::atomic::Ordering::Relaxed) {
            process::exit(1);
        }

        // Copy non-.hl static assets to output directory
        if let Some(out) = out_dir {
            copy_non_hl_files(dir, Path::new(out));

            // Shared CSS extraction (opt-in): find duplicate CSS rules across
            // pages. It rewrites the output, so it needs freshly compiled
            // pages — the incremental cache is bypassed when it's enabled.
            let out_path = Path::new(out);
            let html_files: Vec<PathBuf> = hl_files
                .iter()
                .map(|f| {
                    let rel = f.strip_prefix(dir).unwrap_or(f);
                    out_path.join(rel).with_extension("html")
                })
                .filter(|p| p.exists())
                .collect();
            if shared_css && html_files.len() > 1 {
                extract_shared_css(&html_files, out_path);
            }
        }
        return;
    }


    // Handle "lint" subcommand
    if args.len() >= 2 && args[1] == "lint" {
        let mut lint_target = ".";
        let mut lint_json = false;
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--format" => {
                    i += 1;
                    if args.get(i).map(|s| s.as_str()) == Some("json") {
                        lint_json = true;
                    }
                }
                _ if lint_target == "." => lint_target = &args[i],
                _ => {}
            }
            i += 1;
        }
        let path = Path::new(lint_target);
        let mut all_warnings = Vec::new();
        if path.is_dir() {
            let hl_files = collect_hl_files_recursive(path);
            if hl_files.is_empty() {
                eprintln!("no .hl files found in {}", lint_target);
                process::exit(1);
            }
            for file in &hl_files {
                let path_str = file.to_string_lossy().to_string();
                all_warnings.extend(lint_file(&path_str));
            }
        } else {
            all_warnings.extend(lint_file(lint_target));
        }
        if lint_json {
            let arr = json_array(all_warnings.iter().map(|w| {
                json_object(&[
                    ("severity", json_escape_string("warning")),
                    ("message", json_escape_string(w)),
                ])
            }));
            println!("{}", json_object(&[("diagnostics", arr)]));
        } else if all_warnings.is_empty() {
            eprintln!("no issues found");
        } else {
            for w in &all_warnings {
                eprintln!("{}", w);
            }
            process::exit(1);
        }
        return;
    }


    // Handle "check" subcommand
    if args.len() >= 2 && args[1] == "check" {
        let mut check_target = None;
        let mut check_format_json = false;
        let mut ci = 2;
        while ci < args.len() {
            match args[ci].as_str() {
                "--format" => {
                    ci += 1;
                    if args.get(ci).is_some_and(|v| v == "json") {
                        check_format_json = true;
                    }
                }
                _ if check_target.is_none() => check_target = Some(args[ci].as_str()),
                _ => {
                    eprintln!("unknown argument: {}", args[ci]);
                    process::exit(1);
                }
            }
            ci += 1;
        }
        let target = check_target.unwrap_or(".");
        let json_collector = if check_format_json {
            Some(Mutex::new(Vec::new()))
        } else {
            None
        };
        let path = Path::new(target);
        let mut any_errors = false;
        if path.is_dir() {
            let hl_files = collect_hl_files_recursive(path);
            if hl_files.is_empty() {
                eprintln!("no .hl files found in {}", target);
                process::exit(1);
            }
            for file in &hl_files {
                let path_str = file.to_string_lossy().to_string();
                let (has_errors, _) = compile(
                    &path_str,
                    &CompileConfig {
                        check_only: true,
                        format_json: check_format_json,
                        json_collector: json_collector.as_ref(),
                        ..Default::default()
                    },
                );
                if has_errors {
                    any_errors = true;
                }
            }
        } else {
            let (has_errors, _) = compile(
                target,
                &CompileConfig {
                    check_only: true,
                    format_json: check_format_json,
                    json_collector: json_collector.as_ref(),
                    ..Default::default()
                },
            );
            if has_errors {
                any_errors = true;
            }
        }
        if check_format_json && let Some(collector) = json_collector {
            print_json_diagnostics(&collector.lock().unwrap());
        }
        if any_errors {
            process::exit(1);
        }
        return;
    }






    // Handle "serve" standalone subcommand
    if args.len() >= 2 && args[1] == "serve" {
        let mut serve_target = None;
        let mut serve_port: u16 = 3000;
        let mut serve_open = false;
        let mut serve_https = false;
        let mut cert_path: Option<String> = None;
        let mut key_path: Option<String> = None;
        let mut _proxy_routes: Vec<(String, String)> = Vec::new();
        let mut si = 2;
        while si < args.len() {
            match args[si].as_str() {
                "-p" | "--port" => {
                    si += 1;
                    serve_port = args.get(si).and_then(|p| p.parse().ok()).unwrap_or(3000);
                }
                "--open" => serve_open = true,
                "--https" => serve_https = true,
                "--cert" => {
                    si += 1;
                    cert_path = args.get(si).cloned();
                }
                "--key" => {
                    si += 1;
                    key_path = args.get(si).cloned();
                }
                "--proxy" => {
                    // --proxy /api http://localhost:3001
                    si += 1;
                    let prefix = args.get(si).cloned().unwrap_or_default();
                    si += 1;
                    let target_url = args.get(si).cloned().unwrap_or_default();
                    if !prefix.is_empty() && !target_url.is_empty() {
                        _proxy_routes.push((prefix, target_url));
                        eprintln!(
                            "proxy: {} -> {}",
                            _proxy_routes.last().unwrap().0,
                            _proxy_routes.last().unwrap().1
                        );
                    }
                }
                _ if serve_target.is_none() => serve_target = Some(args[si].clone()),
                _ => {
                    eprintln!("unknown argument: {}", args[si]);
                    process::exit(1);
                }
            }
            si += 1;
        }
        let target = serve_target.unwrap_or_else(|| ".".to_string());
        let target_path = Path::new(&target);
        // Load config
        let config = load_config(target_path);
        let effective_port = if serve_port != 3000 {
            serve_port
        } else {
            config.port
        };

        // Resolve optional TLS config. Without both --cert and --key, --https is
        // an error — we intentionally do not generate self-signed certificates
        // to avoid surprising the user with unpinned trust anchors.
        let tls_config = if serve_https {
            let cert = cert_path.clone().unwrap_or_else(|| {
                eprintln!("error: --https requires --cert <path> and --key <path>");
                eprintln!("  generate a dev cert with: mkcert localhost 127.0.0.1");
                process::exit(1);
            });
            let key = key_path.clone().unwrap_or_else(|| {
                eprintln!("error: --https requires --cert <path> and --key <path>");
                process::exit(1);
            });
            match htmlang::serve::load_tls_config(Path::new(&cert), Path::new(&key)) {
                Ok(cfg) => Some(cfg),
                Err(e) => {
                    eprintln!("error: failed to load TLS config: {}", e);
                    process::exit(1);
                }
            }
        } else {
            None
        };
        let scheme = if tls_config.is_some() {
            "https"
        } else {
            "http"
        };
        let _ = scheme; // announced by watch_loop / open_in_browser downstream

        // Do initial compile
        if target_path.is_dir() {
            let hl_files = collect_hl_files_recursive(target_path);
            let out_dir = config.output.as_deref().unwrap_or("out");
            let _ = fs::create_dir_all(out_dir);
            let mut all_included: Vec<PathBuf> = Vec::new();
            for file in &hl_files {
                let path_str = file.to_string_lossy().to_string();
                let rel = file.strip_prefix(target_path).unwrap_or(file);
                let out_p = Path::new(out_dir).join(rel).with_extension("html");
                if let Some(parent) = out_p.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let effective_out = out_p.to_string_lossy().to_string();
                let (_, included) = compile(
                    &path_str,
                    &CompileConfig {
                        dev: true,
                        error_overlay: true,
                        output_path: Some(&effective_out),
                        ..Default::default()
                    },
                );
                all_included.extend(included);
            }
            copy_non_hl_files(target_path, Path::new(out_dir));
            let (tx, _) = tokio::sync::broadcast::channel::<()>(16);
            let serve_dir = PathBuf::from(out_dir);
            let server_dir = serve_dir.clone();
            let server_tx = tx.clone();
            let tls_for_thread = tls_config;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
                match tls_for_thread {
                    Some(tls) => rt.block_on(htmlang::serve::run_dir_https(
                        effective_port,
                        server_dir,
                        server_tx,
                        tls,
                    )),
                    None => {
                        rt.block_on(htmlang::serve::run_dir(effective_port, server_dir, server_tx))
                    }
                }
            });
            if serve_open {
                open_in_browser(effective_port);
            }
            watch_loop(
                target_path,
                &hl_files,
                &all_included,
                true,
                true,
                Some(tx),
                effective_port,
                config.debounce_ms,
                &WatchBuild {
                    out_dirs: Some((target_path.to_path_buf(), serve_dir)),
                    discover_new_files: true,
                    ..Default::default()
                },
            );
        } else {
            let out_dir = config.output.as_deref().unwrap_or("out");
            let _ = fs::create_dir_all(out_dir);
            let file_stem = Path::new(&target)
                .file_stem()
                .map(|s| s.to_os_string())
                .unwrap_or_default();
            let out_path = Path::new(out_dir).join(&file_stem).with_extension("html");
            let watch_out = out_path.clone();
            let out_path_str = out_path.to_string_lossy().to_string();
            let (_, included) = compile(
                &target,
                &CompileConfig {
                    dev: true,
                    error_overlay: true,
                    output_path: Some(&out_path_str),
                    ..Default::default()
                },
            );
            let (tx, _) = tokio::sync::broadcast::channel::<()>(16);
            let server_tx = tx.clone();
            let tls_for_thread = tls_config;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
                match tls_for_thread {
                    Some(tls) => rt.block_on(htmlang::serve::run_https(
                        effective_port,
                        out_path,
                        server_tx,
                        tls,
                    )),
                    None => rt.block_on(htmlang::serve::run(effective_port, out_path, server_tx)),
                }
            });
            if serve_open {
                open_in_browser(effective_port);
            }
            let files = vec![PathBuf::from(&target)];
            watch_loop(
                Path::new(&target).parent().unwrap_or(Path::new(".")),
                &files,
                &included,
                true,
                true,
                Some(tx),
                effective_port,
                config.debounce_ms,
                &WatchBuild {
                    out_file: Some(watch_out),
                    ..Default::default()
                },
            );
        }
        return;
    }

    // Handle "watch" standalone subcommand
    if args.len() >= 2 && args[1] == "watch" {
        let mut watch_target = None;
        let mut watch_output = None;
        let mut wi = 2;
        while wi < args.len() {
            match args[wi].as_str() {
                "-o" | "--output" => {
                    wi += 1;
                    watch_output = args.get(wi).cloned();
                }
                _ if watch_target.is_none() => watch_target = Some(args[wi].clone()),
                _ => {
                    eprintln!("unknown argument: {}", args[wi]);
                    process::exit(1);
                }
            }
            wi += 1;
        }
        let target = watch_target.unwrap_or_else(|| ".".to_string());
        let target_path = Path::new(&target);
        let config = load_config(target_path);
        let effective_output = watch_output.or(config.output);

        if target_path.is_dir() {
            let hl_files = collect_hl_files_recursive(target_path);
            if let Some(ref out) = effective_output {
                let _ = fs::create_dir_all(out);
            }
            let mut all_included = Vec::new();
            for file in &hl_files {
                let path_str = file.to_string_lossy().to_string();
                let effective_out = effective_output.as_ref().map(|o| {
                    let rel = file.strip_prefix(target_path).unwrap_or(file);
                    let out_p = Path::new(o).join(rel).with_extension("html");
                    if let Some(parent) = out_p.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    out_p.to_string_lossy().to_string()
                });
                let (_, included) = compile(
                    &path_str,
                    &CompileConfig {
                        output_path: effective_out.as_deref(),
                        ..Default::default()
                    },
                );
                all_included.extend(included);
            }
            watch_loop(
                target_path,
                &hl_files,
                &all_included,
                false,
                false,
                None,
                0,
                config.debounce_ms,
                &WatchBuild {
                    out_dirs: effective_output
                        .as_ref()
                        .map(|o| (target_path.to_path_buf(), PathBuf::from(o))),
                    discover_new_files: true,
                    ..Default::default()
                },
            );
        } else {
            let (_, included) = compile(
                &target,
                &CompileConfig {
                    output_path: effective_output.as_deref(),
                    ..Default::default()
                },
            );
            let files = vec![PathBuf::from(&target)];
            watch_loop(
                Path::new(&target).parent().unwrap_or(Path::new(".")),
                &files,
                &included,
                false,
                false,
                None,
                0,
                config.debounce_ms,
                &WatchBuild {
                    out_file: effective_output.as_ref().map(PathBuf::from),
                    ..Default::default()
                },
            );
        }
        return;
    }















    // Handle "upgrade" subcommand: rewrite removed or renamed syntax to its
    // current form.
    if args.len() >= 2 && args[1] == "upgrade" {
        let target = if args.len() >= 3 { &args[2] } else { "." };
        let path = Path::new(target);
        let hl_files = if path.is_dir() {
            collect_hl_files_recursive(path)
        } else {
            vec![PathBuf::from(target)]
        };
        if hl_files.is_empty() {
            eprintln!("no .hl files found in {}", target);
            process::exit(1);
        }
        let mut total_changes = 0usize;
        let mut needs_manual = false;
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let result = htmlang::upgrade::upgrade(&input);
            for (line, message) in &result.manual {
                eprintln!("{}:{}: manual change needed: {}", file.display(), line, message);
                needs_manual = true;
            }
            if result.changes > 0 {
                match fs::write(file, &result.output) {
                    Ok(()) => {
                        eprintln!(
                            "upgraded {} ({} change{})",
                            file.display(),
                            result.changes,
                            if result.changes == 1 { "" } else { "s" }
                        );
                        total_changes += result.changes;
                    }
                    Err(e) => eprintln!("error: {}: {}", file.display(), e),
                }
            }
        }
        if total_changes == 0 && !needs_manual {
            eprintln!("no upgrades needed — all files are up to date");
        } else if total_changes > 0 {
            eprintln!("\n{} total change(s) applied", total_changes);
        }
        if needs_manual {
            process::exit(1);
        }
        return;
    }


    // Handle "convert" subcommand
    if args.len() >= 2 && args[1] == "convert" {
        if args.len() < 3 {
            eprintln!("usage: htmlang convert <file.html>");
            process::exit(1);
        }
        let html_file = &args[2];
        let html = match fs::read_to_string(html_file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {}: {}", html_file, e);
                process::exit(1);
            }
        };
        let hl_output = htmlang::convert::convert(&html);
        print!("{}", hl_output);
        return;
    }

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                cli::print_help();
                process::exit(0);
            }
            "--version" | "-V" => {
                println!("htmlang {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "--watch" | "-w" => watch = true,
            "--dev" | "-d" => dev = true,
            "--check" | "-c" => check = true,
            "--strict" => strict = true,
            "--open" => open_browser = true,
            "--partial" => partial = true,
            "--format" => {
                i += 1;
                match args.get(i) {
                    Some(f) if f == "json" => format_json = true,
                    Some(f) => {
                        eprintln!("unknown format: {}", f);
                        process::exit(1);
                    }
                    None => {
                        eprintln!("--format requires a value");
                        process::exit(1);
                    }
                }
            }
            "--serve" | "-s" => {
                serve = true;
                watch = true;
            }
            "--port" | "-p" => {
                i += 1;
                match args.get(i) {
                    Some(p) => {
                        port = p.parse().unwrap_or_else(|_| {
                            eprintln!("invalid port: {}", p);
                            process::exit(1);
                        });
                    }
                    None => {
                        eprintln!("--port requires a value");
                        process::exit(1);
                    }
                }
            }
            "--output" | "-o" => {
                i += 1;
                match args.get(i) {
                    Some(p) => output_path = Some(p.clone()),
                    None => {
                        eprintln!("--output requires a value");
                        process::exit(1);
                    }
                }
            }
            _ if input_path.is_none() => input_path = Some(args[i].clone()),
            _ => {
                eprintln!("unknown argument: {}", args[i]);
                process::exit(1);
            }
        }
        i += 1;
    }

    let input_path = match input_path {
        Some(p) => p,
        None => {
            cli::print_help();
            process::exit(1);
        }
    };

    let is_dir = Path::new(&input_path).is_dir();

    // --- Directory mode: compile all .hl files ---
    if is_dir {
        let dir = Path::new(&input_path);
        let hl_files = collect_hl_files(dir);
        if hl_files.is_empty() {
            eprintln!("no .hl files found in {}", input_path);
            process::exit(1);
        }

        // Create output dir if needed
        if let Some(ref out) = output_path {
            let _ = fs::create_dir_all(out);
        }

        let json_collector = if format_json {
            Some(Mutex::new(Vec::new()))
        } else {
            None
        };
        let mut any_errors = false;
        let mut all_included: Vec<PathBuf> = Vec::new();
        for file in &hl_files {
            let path_str = file.to_string_lossy().to_string();
            let effective_out = output_path.as_ref().map(|o| {
                let rel = file.strip_prefix(dir).unwrap_or(file);
                let out_p = Path::new(o).join(rel).with_extension("html");
                if let Some(parent) = out_p.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                out_p.to_string_lossy().to_string()
            });
            let (has_errors, included) = compile(
                &path_str,
                &CompileConfig {
                    dev,
                    error_overlay: serve,
                    check_only: check,
                    output_path: effective_out.as_deref(),
                    format_json,
                    json_collector: json_collector.as_ref(),
                    strict,
                    partial,
                    ..Default::default()
                },
            );
            if has_errors {
                any_errors = true;
            }
            all_included.extend(included);
        }

        if format_json && let Some(collector) = json_collector {
            print_json_diagnostics(&collector.lock().unwrap());
        }

        if !watch {
            if any_errors {
                process::exit(1);
            }
            return;
        }

        // For directory serve mode, serve the output directory with route mapping
        let reload_tx = if serve {
            let (tx, _) = tokio::sync::broadcast::channel::<()>(16);
            let serve_dir = output_path
                .as_ref()
                .map_or_else(|| dir.to_path_buf(), PathBuf::from);
            let server_tx = tx.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
                rt.block_on(htmlang::serve::run_dir(port, serve_dir, server_tx));
            });
            if open_browser {
                open_in_browser(port);
            }
            Some(tx)
        } else {
            None
        };

        watch_loop(
            dir,
            &hl_files,
            &all_included,
            dev,
            serve,
            reload_tx,
            port,
            50,
            &WatchBuild {
                out_dirs: output_path
                    .as_ref()
                    .map(|o| (dir.to_path_buf(), PathBuf::from(o))),
                discover_new_files: true,
                strict,
                partial,
                ..Default::default()
            },
        );
        return;
    }

    // --- Single file mode ---
    let json_collector_single = if format_json {
        Some(Mutex::new(Vec::new()))
    } else {
        None
    };
    let (has_errors, included_files) = compile(
        &input_path,
        &CompileConfig {
            dev,
            error_overlay: serve,
            check_only: check,
            output_path: output_path.as_deref(),
            format_json,
            json_collector: json_collector_single.as_ref(),
            strict,
            partial,
            ..Default::default()
        },
    );
    if format_json && let Some(ref collector) = json_collector_single {
        print_json_diagnostics(&collector.lock().unwrap());
    }

    if !watch {
        if has_errors {
            process::exit(1);
        }
        return;
    }

    // Start dev server if requested
    let reload_tx = if serve {
        let (tx, _) = tokio::sync::broadcast::channel::<()>(16);
        let out_path = match output_path {
            Some(ref p) => PathBuf::from(p),
            None => Path::new(&input_path).with_extension("html"),
        };
        let server_tx = tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");
            rt.block_on(htmlang::serve::run(port, out_path, server_tx));
        });
        if open_browser {
            open_in_browser(port);
        }
        Some(tx)
    } else {
        None
    };

    let files = vec![PathBuf::from(&input_path)];
    watch_loop(
        Path::new(&input_path).parent().unwrap_or(Path::new(".")),
        &files,
        &included_files,
        dev,
        serve,
        reload_tx,
        port,
        50,
        &WatchBuild {
            out_file: output_path.as_ref().map(PathBuf::from),
            strict,
            partial,
            ..Default::default()
        },
    );
}

// (CLI help, LSP launcher, and shell completions moved to cli.rs)

fn collect_hl_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "hl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// How watch-mode recompiles are built: where each source's output goes and
/// which flags apply, so recompiles match the initial build.
#[derive(Default)]
struct WatchBuild {
    /// `(source root, output root)`: `<src>/a/b.hl` compiles to `<out>/a/b.html`.
    out_dirs: Option<(PathBuf, PathBuf)>,
    /// Output file for a single watched source (`-o page.html`).
    out_file: Option<PathBuf>,
    /// Pick up `.hl` files created in the watch directory (directory mode).
    discover_new_files: bool,
    minify: bool,
    strict: bool,
    partial: bool,
}

impl WatchBuild {
    /// Output path for `source`, or `None` to write next to the source.
    fn output_for(&self, source: &Path) -> Option<PathBuf> {
        if let Some(file) = &self.out_file {
            return Some(file.clone());
        }
        let (src_root, out_root) = self.out_dirs.as_ref()?;
        let src_root = fs::canonicalize(src_root).unwrap_or_else(|_| src_root.clone());
        let rel = source
            .strip_prefix(&src_root)
            .ok()
            .or_else(|| source.file_name().map(Path::new))?;
        Some(out_root.join(rel).with_extension("html"))
    }
}

#[allow(clippy::too_many_arguments)]
fn watch_loop(
    watch_dir: &Path,
    source_files: &[PathBuf],
    included_files: &[PathBuf],
    dev: bool,
    serve: bool,
    reload_tx: Option<tokio::sync::broadcast::Sender<()>>,
    serve_port: u16,
    debounce_ms: u64,
    build: &WatchBuild,
) {
    use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
    use std::sync::mpsc;

    // `Path::new("page.hl").parent()` is `Some("")`, which can't be watched.
    let watch_dir = if watch_dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        watch_dir
    };

    let (tx, rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default(),
    )
    .expect("failed to create file watcher");

    // Watch all source files
    for file in source_files {
        let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
        watcher
            .watch(&canonical, RecursiveMode::NonRecursive)
            .unwrap_or_else(|_| eprintln!("warning: could not watch {}", file.display()));
    }
    for inc in included_files {
        let _ = watcher.watch(inc, RecursiveMode::NonRecursive);
    }

    // Also watch the directory itself for new files
    let _ = watcher.watch(watch_dir, RecursiveMode::NonRecursive);

    if serve {
        eprintln!("watching for changes at http://127.0.0.1:{}", serve_port);
    } else {
        eprintln!("watching for changes...");
    }

    // Track content hashes for incremental rebuilds
    let mut content_hashes: HashMap<PathBuf, u64> = HashMap::new();
    // Dependency map: source file -> list of included/imported files. Maintained
    // across rebuilds so we always know the current dep graph.
    let mut dep_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    // Set of currently-watched include paths. Used to unwatch files that are
    // no longer referenced after a rebuild changes the dep graph.
    let mut watched_includes: HashSet<PathBuf> = HashSet::new();

    fn hash_file(path: &Path) -> Option<u64> {
        let content = fs::read(path).ok()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        Some(hasher.finish())
    }

    // Seed initial hashes and dependency map
    for file in source_files {
        let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
        if let Some(h) = hash_file(&canonical) {
            content_hashes.insert(canonical, h);
        }
    }
    // Seed dep_map from the initial compile's include list so that the first
    // change event correctly picks up include-file modifications even before
    // the first recompile replaces the entry.
    let initial_includes_canonical: Vec<PathBuf> = included_files
        .iter()
        .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .collect();
    for inc in &initial_includes_canonical {
        if let Some(h) = hash_file(inc) {
            content_hashes.insert(inc.clone(), h);
        }
        watched_includes.insert(inc.clone());
    }
    // Attach the initial include list to every source file so a change to any
    // of them triggers a rebuild of any source. Per-file dep refinement happens
    // on the next recompile.
    if !initial_includes_canonical.is_empty() {
        for file in source_files {
            let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
            dep_map.insert(canonical, initial_includes_canonical.clone());
        }
    }

    while rx.recv().is_ok() {
        // Drain additional events (debounce) with configurable delay
        std::thread::sleep(std::time::Duration::from_millis(debounce_ms));
        while rx.try_recv().is_ok() {}

        // Collect which files actually changed (HashSet for O(1) lookups)
        let mut changed_files: HashSet<PathBuf> = HashSet::new();
        let check_path = |path: &Path, hashes: &mut HashMap<PathBuf, u64>| -> bool {
            if let Some(h) = hash_file(path)
                && hashes.get(path) != Some(&h)
            {
                hashes.insert(path.to_path_buf(), h);
                return true;
            }
            false
        };

        for file in source_files {
            let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
            if check_path(&canonical, &mut content_hashes) {
                changed_files.insert(canonical);
            }
        }

        // Check included files for changes
        for deps in dep_map.values() {
            for dep in deps {
                if check_path(dep, &mut content_hashes) {
                    changed_files.insert(dep.clone());
                }
            }
        }

        // Check for new .hl files in directory
        if build.discover_new_files && watch_dir.is_dir() {
            let current_files = collect_hl_files(watch_dir);
            for file in &current_files {
                let canonical = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
                if check_path(&canonical, &mut content_hashes) {
                    changed_files.insert(canonical.clone());
                    let _ = watcher.watch(&canonical, RecursiveMode::NonRecursive);
                }
            }
        }

        if changed_files.is_empty() {
            continue;
        }

        eprintln!("\nrecompiling...");

        // Determine which source files need recompilation:
        // 1. Source files that changed directly
        // 2. Source files whose dependencies changed
        let mut files_to_compile: Vec<PathBuf> = Vec::new();
        let all_sources: Vec<PathBuf> = {
            let mut s: Vec<PathBuf> = source_files
                .iter()
                .map(|f| fs::canonicalize(f).unwrap_or_else(|_| f.clone()))
                .collect();
            if build.discover_new_files && watch_dir.is_dir() {
                for file in &collect_hl_files(watch_dir) {
                    let c = fs::canonicalize(file).unwrap_or_else(|_| file.clone());
                    if !s.contains(&c) {
                        s.push(c);
                    }
                }
            }
            s
        };

        for source in &all_sources {
            // Recompile if the source itself changed
            if changed_files.contains(source) {
                if !files_to_compile.contains(source) {
                    files_to_compile.push(source.clone());
                }
                continue;
            }
            // Recompile if any of its dependencies changed
            if let Some(deps) = dep_map.get(source)
                && deps.iter().any(|d| changed_files.contains(d))
                && !files_to_compile.contains(source)
            {
                files_to_compile.push(source.clone());
            }
        }

        // If no specific files identified (e.g., first run), compile all
        if files_to_compile.is_empty() {
            files_to_compile = all_sources;
        }

        let mut recompiled = 0usize;
        for file in &files_to_compile {
            let path_str = file.to_string_lossy().to_string();
            let out_path = build.output_for(file);
            if let Some(parent) = out_path.as_deref().and_then(Path::parent) {
                let _ = fs::create_dir_all(parent);
            }
            let out_str = out_path.map(|p| p.to_string_lossy().to_string());
            let (_, new_includes) = compile(
                &path_str,
                &CompileConfig {
                    dev,
                    error_overlay: serve,
                    output_path: out_str.as_deref(),
                    minify: build.minify,
                    strict: build.strict,
                    partial: build.partial,
                    ..Default::default()
                },
            );
            recompiled += 1;
            // Update dependency map
            let canonical_deps: Vec<PathBuf> = new_includes
                .iter()
                .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
                .collect();
            for inc in &canonical_deps {
                if watched_includes.insert(inc.clone()) {
                    let _ = watcher.watch(inc, RecursiveMode::NonRecursive);
                }
                if let Some(h) = hash_file(inc) {
                    content_hashes.insert(inc.clone(), h);
                }
            }
            dep_map.insert(file.clone(), canonical_deps);
        }

        // Unwatch include files that are no longer referenced by any
        // source. Prevents file-descriptor leaks when @include edges
        // are removed mid-session.
        let still_needed: HashSet<PathBuf> = dep_map.values().flatten().cloned().collect();
        let to_drop: Vec<PathBuf> = watched_includes
            .difference(&still_needed)
            .cloned()
            .collect();
        for path in to_drop {
            let _ = watcher.unwatch(&path);
            watched_includes.remove(&path);
            content_hashes.remove(&path);
        }

        eprintln!("recompiled {} file(s)", recompiled);

        if let Some(ref tx) = reload_tx {
            let _ = tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_handles_control_characters() {
        assert_eq!(
            json_escape_string("a\tb\r\u{1}\"\\"),
            "\"a\\tb\\r\\u0001\\\"\\\\\""
        );
    }

    #[test]
    fn css_segments_split_on_rule_boundaries_only() {
        let css = "body{line-height:1.5}.a{content:\"→\"}@media(x){.a{b:c}}";
        assert_eq!(
            css_segments(css),
            vec![
                "body{line-height:1.5}",
                ".a{content:\"→\"}",
                "@media(x){.a{b:c}}"
            ]
        );
    }
}
