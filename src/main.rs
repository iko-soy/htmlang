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
    compat: bool,
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
                    compat: cfg.compat,
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
                // match even with --compat or --minify.
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

/// Rename attribute key `from` to `to` wherever it appears as a whole key:
/// preceded by `[`, `,`, whitespace or a `prefix:` and followed by a space.
fn rename_attr_key(line: &str, from: &str, to: &str) -> String {
    let needle = format!("{from} ");
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(pos) = rest.find(&needle) {
        let whole_key = rest[..pos]
            .chars()
            .next_back()
            .is_none_or(|c| matches!(c, '[' | ',' | ':') || c.is_whitespace());
        out.push_str(&rest[..pos]);
        out.push_str(if whole_key { to } else { from });
        out.push(' ');
        rest = &rest[pos + needle.len()..];
    }
    out.push_str(rest);
    out
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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

fn kebab_to_title(s: &str) -> String {
    s.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => {
                    let mut title = c.to_uppercase().to_string();
                    title.extend(chars);
                    title
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

fn bundle_assets(html: &str, base_dir: &Path) -> String {
    use std::io::Read;
    let mut result = html.to_string();

    // Inline images referenced as src="..." in <img> tags
    let img_re_patterns = ["src=\"", "url("];
    for pattern in &img_re_patterns {
        let mut output = String::new();
        let mut remaining = result.as_str();
        while let Some(pos) = remaining.find(pattern) {
            output.push_str(&remaining[..pos + pattern.len()]);
            remaining = &remaining[pos + pattern.len()..];

            // Find the closing delimiter
            let close_char = if *pattern == "src=\"" { '"' } else { ')' };
            if let Some(end) = remaining.find(close_char) {
                let path_str = remaining[..end].trim_matches(|c| c == '\'' || c == '"');

                // Skip data URIs, remote URLs, and anchors
                if !path_str.starts_with("data:")
                    && !path_str.starts_with("http://")
                    && !path_str.starts_with("https://")
                    && !path_str.starts_with('#')
                    && !path_str.is_empty()
                {
                    let asset_path = base_dir.join(path_str);
                    if asset_path.exists()
                        && let Ok(mut file) = std::fs::File::open(&asset_path)
                    {
                        let mut buf = Vec::new();
                        if file.read_to_end(&mut buf).is_ok() {
                            let mime = match asset_path.extension().and_then(|e| e.to_str()) {
                                Some("png") => "image/png",
                                Some("jpg") | Some("jpeg") => "image/jpeg",
                                Some("gif") => "image/gif",
                                Some("svg") => "image/svg+xml",
                                Some("webp") => "image/webp",
                                Some("ico") => "image/x-icon",
                                Some("woff2") => "font/woff2",
                                Some("woff") => "font/woff",
                                Some("ttf") => "font/ttf",
                                Some("otf") => "font/otf",
                                Some("avif") => "image/avif",
                                _ => "application/octet-stream",
                            };
                            use std::fmt::Write as FmtWrite;
                            let mut b64 = String::new();
                            // Simple base64 encoding
                            let encoded = base64_encode(&buf);
                            let _ = write!(b64, "data:{};base64,{}", mime, encoded);
                            output.push_str(&b64);
                            remaining = &remaining[end..];
                            continue;
                        }
                    }
                }
                // If we couldn't inline, keep original path
                output.push_str(&remaining[..end]);
                remaining = &remaining[end..];
            }
        }
        output.push_str(remaining);
        result = output;
    }
    result
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
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

fn init_project(dir: &str, template_name: Option<&str>) {
    let dir = Path::new(dir);
    if dir.to_str() != Some(".")
        && let Err(e) = fs::create_dir_all(dir)
    {
        eprintln!("error: cannot create directory '{}': {}", dir.display(), e);
        process::exit(1);
    }

    let index_path = dir.join("index.hl");
    if index_path.exists() {
        eprintln!("error: {} already exists", index_path.display());
        process::exit(1);
    }

    let template = match template_name {
        Some("blog") => {
            r#"@page My Blog
@let primary #3b82f6
@let bg-dark #1a1a2e

@column [max-width 720, center-x, padding 40, spacing 30]
  @header [spacing 10]
    @text [bold, size 36] My Blog
    @paragraph [color #666, line-height 1.6] Thoughts and ideas.

  @main [spacing 40]
    @article [spacing 12, padding-bottom 30, border-bottom 1 #eee]
      @text [bold, size 24] First Post
      @text [color #888, size 14] 2024-01-15
      @paragraph [line-height 1.8]
        Welcome to my blog. This is the first post.

    @article [spacing 12, padding-bottom 30, border-bottom 1 #eee]
      @text [bold, size 24] Another Post
      @text [color #888, size 14] 2024-01-10
      @paragraph [line-height 1.8]
        Here is another post with more content.

  @footer [padding-top 20, border-top 1 #eee]
    @text [color #888, size 14, text-align center] Built with htmlang
"#
        }
        Some("docs") => {
            r#"@page Documentation
@let primary #3b82f6
@let sidebar-width 250

@row [min-height 100vh]
  @aside [width $sidebar-width, padding 20, background #f8f9fa, border-right 1 #e0e0e0, spacing 8]
    @text [bold, size 18, padding-bottom 10] Docs
    @nav [spacing 4]
      @link # Getting Started
      @link # Installation
      @link # Configuration
      @link # API Reference

  @main [width fill, padding 40, max-width 800, spacing 20]
    @text [bold, size 32] Getting Started
    @paragraph [line-height 1.8]
      Welcome to the documentation. Use the sidebar to navigate.

    @text [bold, size 24] Installation
    @code [padding 16, background #f5f5f5, rounded 8]
      npm install my-package

    @text [bold, size 24] Usage
    @paragraph [line-height 1.8]
      Import and use the library in your project.
"#
        }
        Some("portfolio") => {
            r#"@page Portfolio
@let primary #3b82f6
@let accent #8b5cf6

@column [min-height 100vh, spacing 0]
  @header [padding 20 40, background white, border-bottom 1 #eee]
    @row [max-width 1200, center-x, width fill, justify-content space-between, align-items center]
      @text [bold, size 20] Jane Doe
      @nav
        @row [spacing 20]
          @link # Work
          @link # About
          @link # Contact

  @main [max-width 1200, center-x, padding 60 40, spacing 60]
    @column [spacing 10, center-x, text-align center, max-width 600]
      @text [bold, size 48] Designer & Developer
      @paragraph [color #666, size 18, line-height 1.6]
        I create beautiful, functional digital experiences.

    @grid [grid-cols 3, gap 20]
      @el [aspect-ratio 1, background #f0f0f0, rounded 12, padding 20, spacing 10]
        @text [bold] Project One
        @text [color #666, size 14] Web Design

      @el [aspect-ratio 1, background #f0f0f0, rounded 12, padding 20, spacing 10]
        @text [bold] Project Two
        @text [color #666, size 14] Branding

      @el [aspect-ratio 1, background #f0f0f0, rounded 12, padding 20, spacing 10]
        @text [bold] Project Three
        @text [color #666, size 14] Development

  @footer [padding 30 40, background #1a1a2e, color white, text-align center]
    @text [size 14] Built with htmlang
"#
        }
        _ => {
            r#"@page My Site
@let primary #3b82f6

@column [max-width 800, center-x, padding 40, spacing 20]
  @text [bold, size 32] Hello, htmlang!

  @paragraph [line-height 1.6]
    Edit {@text [bold, color $primary] index.hl} and run
    {@text [font monospace, size 14] htmlang -s .} to get started.

  @row [spacing 10]
    @el [padding 12 24, background $primary, rounded 8, cursor pointer, hover:background #2563eb, transition all 0.15s ease] > @link https://github.com/nicholasgasior/htmlang
      @text [color white, bold] Documentation
"#
        }
    };

    match fs::write(&index_path, template) {
        Ok(()) => eprintln!("created {}", index_path.display()),
        Err(e) => {
            eprintln!("error: {}: {}", index_path.display(), e);
            process::exit(1);
        }
    }
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

fn collect_all_files_recursive(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, files);
                } else if path.is_file() {
                    files.push(path);
                }
            }
        }
    }
    walk(dir, &mut files);
    files.sort();
    files
}

fn generate_sitemap(dir: &str, base_url: &str) {
    let dir = Path::new(dir);
    let hl_files = collect_hl_files_recursive(dir);
    if hl_files.is_empty() {
        eprintln!("no .hl files found in {}", dir.display());
        process::exit(1);
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n",
    );
    for file in &hl_files {
        let rel = file.strip_prefix(dir).unwrap_or(file);
        let url_path = rel
            .with_extension("html")
            .to_string_lossy()
            .replace('\\', "/");
        let url = if url_path == "index.html" {
            format!("{}/", base_url.trim_end_matches('/'))
        } else {
            format!("{}/{}", base_url.trim_end_matches('/'), url_path)
        };

        // Get file modification time for <lastmod>
        let lastmod = file
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let secs = t
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let days = secs / 86400;
                // Simple date calculation from epoch days
                let (y, m, d) = epoch_days_to_date(days);
                format!("{:04}-{:02}-{:02}", y, m, d)
            });

        // Heuristic priority: index pages get higher priority
        let priority = if url_path == "index.html" {
            "1.0"
        } else if rel.components().count() <= 2 {
            "0.8"
        } else {
            "0.5"
        };

        xml.push_str("  <url>\n");
        xml.push_str(&format!("    <loc>{}</loc>\n", xml_escape(&url)));
        if let Some(ref date) = lastmod {
            xml.push_str(&format!("    <lastmod>{}</lastmod>\n", date));
        }
        xml.push_str(&format!("    <priority>{}</priority>\n", priority));
        xml.push_str("  </url>\n");
    }
    xml.push_str("</urlset>\n");
    let out_path = dir.join("sitemap.xml");
    match fs::write(&out_path, &xml) {
        Ok(()) => eprintln!("wrote {} ({} URLs)", out_path.display(), hl_files.len()),
        Err(e) => {
            eprintln!("error: {}: {}", out_path.display(), e);
            process::exit(1);
        }
    }
}

fn epoch_days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn lint_file(path: &str) -> Vec<String> {
    let input = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return vec![format!("error: {}: {}", path, e)],
    };
    let base = Path::new(path).parent();
    let result = htmlang::parser::parse_with_base(&input, base);
    let mut warnings = Vec::new();

    // Report parser diagnostics
    for d in &result.diagnostics {
        let prefix = severity_label(d.severity);
        warnings.push(format!("{}:{}:{}: {}", path, d.line, prefix, d.message));
    }

    // Additional lint checks on the AST
    lint_nodes(&result.document.nodes, path, 0, &mut warnings);
    warnings
}

fn lint_nodes(nodes: &[htmlang::ast::Node], path: &str, depth: usize, warnings: &mut Vec<String>) {
    for node in nodes {
        if let htmlang::ast::Node::Element(elem) = node {
            // Deeply nested elements (>10 levels)
            if depth > 10 {
                warnings.push(format!(
                    "{}:{}:lint: deeply nested element ({} levels) — consider simplifying",
                    path, elem.line_num, depth
                ));
            }

            // @image without alt
            if elem.kind == htmlang::ast::ElementKind::Image
                && !elem.attrs.iter().any(|a| a.key == "alt")
            {
                warnings.push(format!(
                    "{}:{}:lint: @image missing 'alt' attribute (accessibility)",
                    path, elem.line_num
                ));
            }

            // @link without content or aria-label
            if elem.kind == htmlang::ast::ElementKind::Link {
                let has_aria = elem.attrs.iter().any(|a| a.key == "aria-label");
                let has_children = !elem.children.is_empty();
                let has_arg_text = elem.argument.as_ref().is_some_and(|_| false);
                if !has_aria && !has_children && !has_arg_text {
                    warnings.push(format!(
                        "{}:{}:lint: @link has no visible text or aria-label (accessibility)",
                        path, elem.line_num
                    ));
                }
            }

            // @input without type
            if elem.kind == htmlang::ast::ElementKind::Input
                && !elem.attrs.iter().any(|a| a.key == "type")
            {
                warnings.push(format!(
                    "{}:{}:lint: @input missing 'type' attribute",
                    path, elem.line_num
                ));
            }

            // Empty containers (no children, no text)
            if matches!(
                elem.kind,
                htmlang::ast::ElementKind::Row
                    | htmlang::ast::ElementKind::Column
                    | htmlang::ast::ElementKind::El
            ) && elem.children.is_empty()
            {
                warnings.push(format!(
                    "{}:{}:lint: empty container (@{}) has no children",
                    path,
                    elem.line_num,
                    match elem.kind {
                        htmlang::ast::ElementKind::Row => "row",
                        htmlang::ast::ElementKind::Column => "column",
                        _ => "el",
                    }
                ));
            }

            // @button without type
            if elem.kind == htmlang::ast::ElementKind::Button
                && !elem.attrs.iter().any(|a| a.key == "type")
            {
                warnings.push(format!(
                    "{}:{}:lint: @button missing 'type' attribute (defaults to submit)",
                    path, elem.line_num
                ));
            }

            lint_nodes(&elem.children, path, depth + 1, warnings);
        }
    }
}

fn stats_file(path: &str) {
    let input = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}: {}", path, e);
            process::exit(1);
        }
    };
    let base = Path::new(path).parent();
    let result = htmlang::parser::parse_with_base(&input, base);
    let html = htmlang::codegen::generate(&result.document);

    let mut element_count = 0;
    let mut colors = std::collections::HashSet::new();
    let mut fonts = std::collections::HashSet::new();
    count_elements(
        &result.document.nodes,
        &mut element_count,
        &mut colors,
        &mut fonts,
    );

    // Count CSS rules (approximate from generated style block)
    let css_rules = html.matches('{').count().saturating_sub(1); // subtract the html/head/body structure

    let source_bytes = input.len();
    let output_bytes = html.len();

    eprintln!("--- {} ---", path);
    eprintln!(
        "  source size:    {} bytes ({} lines)",
        source_bytes,
        input.lines().count()
    );
    eprintln!("  output size:    {} bytes", output_bytes);
    eprintln!("  elements:       {}", element_count);
    eprintln!("  CSS rules:      ~{}", css_rules);
    eprintln!("  unique colors:  {}", colors.len());
    if !colors.is_empty() {
        let mut sorted: Vec<_> = colors.iter().collect();
        sorted.sort();
        eprintln!(
            "    {}",
            sorted
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    eprintln!("  unique fonts:   {}", fonts.len());
    if !fonts.is_empty() {
        let mut sorted: Vec<_> = fonts.iter().collect();
        sorted.sort();
        eprintln!(
            "    {}",
            sorted
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if result
        .diagnostics
        .iter()
        .any(|d| d.severity == htmlang::parser::Severity::Error)
    {
        eprintln!(
            "  errors:         {}",
            result
                .diagnostics
                .iter()
                .filter(|d| d.severity == htmlang::parser::Severity::Error)
                .count()
        );
    }
    let warn_count = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == htmlang::parser::Severity::Warning)
        .count();
    if warn_count > 0 {
        eprintln!("  warnings:       {}", warn_count);
    }
}

fn count_elements(
    nodes: &[htmlang::ast::Node],
    count: &mut usize,
    colors: &mut std::collections::HashSet<String>,
    fonts: &mut std::collections::HashSet<String>,
) {
    for node in nodes {
        if let htmlang::ast::Node::Element(elem) = node {
            *count += 1;
            for attr in &elem.attrs {
                let key = attr.key.as_str();
                // Strip pseudo/media prefixes for color/font detection
                let base_key = key.split(':').next_back().unwrap_or(key);
                if matches!(base_key, "color" | "background")
                    && let Some(ref v) = attr.value
                {
                    colors.insert(v.clone());
                }
                if base_key == "font"
                    && let Some(ref v) = attr.value
                {
                    fonts.insert(v.clone());
                }
            }
            count_elements(&elem.children, count, colors, fonts);
        }
    }
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
    compat: Option<bool>,
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
        compat: None,
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
                    "compat" => config.compat = Some(value == "true"),
                    "strict" => config.strict = Some(value == "true"),
                    _ => {
                        eprintln!(
                            "warning: {}:{}: unknown build key '{}' (expected: dev, minify, compat, strict)",
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

fn find_wasm_pkg() -> Option<(Vec<u8>, String)> {
    // Look for pre-built WASM binary and JS glue in known locations
    let candidates = [
        // Relative to current executable
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("..").join("wasm-pkg"))),
        // In the project build directory (development)
        Some(PathBuf::from("target/wasm-pkg")),
    ];
    for dir in candidates.iter().flatten() {
        let wasm = dir.join("htmlang_wasm_bg.wasm");
        let js = dir.join("htmlang_wasm.js");
        if let (Ok(wasm_bytes), Ok(js_str)) = (fs::read(&wasm), fs::read_to_string(&js)) {
            return Some((wasm_bytes, js_str));
        }
    }
    None
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut watch = false;
    let mut serve = false;
    let mut dev = false;
    let mut check = false;
    let mut format_json = false;
    let mut compat = false;
    let mut strict = false;
    let mut open_browser = false;
    let mut partial = false;
    let mut port: u16 = 3000;
    let mut output_path: Option<String> = None;
    let mut input_path = None;

    // Handle "lsp" subcommand — launch the LSP server from the main binary
    if args.len() >= 2 && args[1] == "lsp" {
        cli::run_lsp();
        return;
    }

    // Handle "completions" subcommand — generate shell completions
    if args.len() >= 2 && args[1] == "completions" {
        let shell = args.get(2).map(|s| s.as_str()).unwrap_or("bash");
        cli::print_shell_completions(shell);
        return;
    }

    // Handle "init" subcommand
    if args.len() >= 2 && args[1] == "init" {
        let mut init_dir = ".";
        let mut init_template: Option<&str> = None;
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--template" | "-t" => {
                    i += 1;
                    init_template = args.get(i).map(|s| s.as_str());
                }
                _ if init_dir == "." => init_dir = &args[i],
                _ => {}
            }
            i += 1;
        }
        init_project(init_dir, init_template);
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
        let mut build_compat = false;
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
                "--compat" => build_compat = true,
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
        let build_compat = build_compat || config.compat.unwrap_or(false);
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
                        (build_minify, build_compat, build_strict, effective_out).hash(&mut hasher);
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
                            compat: build_compat,
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

    // Handle "sitemap" subcommand
    if args.len() >= 2 && args[1] == "sitemap" {
        let dir = if args.len() >= 3 { &args[2] } else { "." };
        let base_url = args
            .iter()
            .position(|a| a == "--base-url" || a == "-b")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("https://example.com");
        generate_sitemap(dir, base_url);
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

    // Handle "stats" subcommand
    if args.len() >= 2 && args[1] == "stats" {
        let target = if args.len() >= 3 { &args[2] } else { "." };
        let path = Path::new(target);
        if path.is_dir() {
            let hl_files = collect_hl_files_recursive(path);
            if hl_files.is_empty() {
                eprintln!("no .hl files found in {}", target);
                process::exit(1);
            }
            for file in &hl_files {
                stats_file(&file.to_string_lossy());
            }
        } else {
            stats_file(target);
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

    // Handle "test" subcommand (run @assert directives across a project)
    if args.len() >= 2 && args[1] == "test" {
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
        let mut total_asserts = 0usize;
        let mut failed_asserts = 0usize;
        let mut files_with_errors = 0usize;
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {}: {}", file.display(), e);
                    files_with_errors += 1;
                    continue;
                }
            };
            let base = file.parent();
            let result = htmlang::parser::parse_with_base(&input, base);
            let file_errors: Vec<_> = result
                .diagnostics
                .iter()
                .filter(|d| d.severity == htmlang::parser::Severity::Error)
                .collect();
            let assert_failures: Vec<_> = file_errors
                .iter()
                .filter(|d| d.message.starts_with("assertion failed:"))
                .collect();
            total_asserts += assert_failures.len();
            // Count @assert lines (passes + failures)
            let assert_count = input
                .lines()
                .filter(|l| l.trim().starts_with("@assert "))
                .count();
            total_asserts += assert_count.saturating_sub(assert_failures.len());
            if !assert_failures.is_empty() {
                files_with_errors += 1;
                failed_asserts += assert_failures.len();
                for d in &assert_failures {
                    eprintln!("FAIL {}: line {}: {}", file.display(), d.line, d.message);
                }
            }
        }
        let passed = total_asserts.saturating_sub(failed_asserts);
        eprintln!(
            "\n{} assertions: {} passed, {} failed ({} files)",
            total_asserts,
            passed,
            failed_asserts,
            hl_files.len()
        );
        if failed_asserts > 0 || files_with_errors > 0 {
            process::exit(1);
        }
        return;
    }

    // Handle "new" subcommand
    if args.len() >= 2 && args[1] == "new" {
        if args.len() < 3 {
            eprintln!("usage: htmlang new <page-name>");
            process::exit(1);
        }
        let page_name = &args[2];
        let file_name = format!("{}.hl", page_name);
        let file_path = Path::new(&file_name);
        if file_path.exists() {
            eprintln!("error: {} already exists", file_name);
            process::exit(1);
        }
        let title = kebab_to_title(page_name);
        let template = format!(
            "@page {title}\n\
             @let primary #3b82f6\n\
             \n\
             @column [max-width 800, center-x, padding 40, spacing 20]\n\
             \x20\x20@text [bold, size 32] {title}\n\
             \x20\x20@paragraph [line-height 1.6]\n\
             \x20\x20\x20\x20Content goes here.\n",
            title = title,
        );
        match fs::write(file_path, &template) {
            Ok(()) => eprintln!("created {}", file_name),
            Err(e) => {
                eprintln!("error: {}: {}", file_name, e);
                process::exit(1);
            }
        }
        return;
    }

    // Handle "preview" subcommand
    if args.len() >= 2 && args[1] == "preview" {
        if args.len() < 3 {
            eprintln!("usage: htmlang preview <file.hl>");
            process::exit(1);
        }
        let file = &args[2];
        let tmp_dir = env::temp_dir().join("htmlang-preview");
        let _ = fs::create_dir_all(&tmp_dir);
        let out_path = tmp_dir.join("preview.html");
        let (has_errors, _) = compile(
            file,
            &CompileConfig {
                dev: true,
                output_path: Some(&out_path.to_string_lossy()),
                ..Default::default()
            },
        );
        if has_errors {
            process::exit(1);
        }
        let url = format!("file://{}", out_path.display());
        let cmd = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(cmd).arg(&url).spawn();
        eprintln!("opened {}", out_path.display());
        return;
    }

    // Handle "diff" subcommand
    if args.len() >= 2 && args[1] == "diff" {
        if args.len() < 4 {
            eprintln!("usage: htmlang diff <file1.hl> <file2.hl>");
            process::exit(1);
        }
        let file1 = &args[2];
        let file2 = &args[3];
        let compile_to_string = |path: &str| -> Result<String, String> {
            let input = fs::read_to_string(path).map_err(|e| format!("error: {}: {}", path, e))?;
            let base = Path::new(path).parent();
            let result = htmlang::parser::parse_with_base(&input, base);
            if result
                .diagnostics
                .iter()
                .any(|d| d.severity == htmlang::parser::Severity::Error)
            {
                return Err(format!("error: {} has parse errors", path));
            }
            Ok(htmlang::codegen::generate_dev(&result.document))
        };
        let html1 = match compile_to_string(file1) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("{}", e);
                process::exit(1);
            }
        };
        let html2 = match compile_to_string(file2) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("{}", e);
                process::exit(1);
            }
        };
        let lines1: Vec<&str> = html1.lines().collect();
        let lines2: Vec<&str> = html2.lines().collect();
        let mut has_diff = false;
        let max_len = lines1.len().max(lines2.len());
        for i in 0..max_len {
            let l1 = lines1.get(i).unwrap_or(&"");
            let l2 = lines2.get(i).unwrap_or(&"");
            if l1 != l2 {
                has_diff = true;
                if !l1.is_empty() {
                    println!("- {}", l1);
                }
                if !l2.is_empty() {
                    println!("+ {}", l2);
                }
            }
        }
        if !has_diff {
            eprintln!("no differences");
        }
        return;
    }

    // Handle "export" subcommand
    if args.len() >= 2 && args[1] == "export" {
        let mut src_dir = None;
        let mut out_file = None;
        let mut ei = 2;
        while ei < args.len() {
            match args[ei].as_str() {
                "-o" | "--output" => {
                    ei += 1;
                    out_file = args.get(ei).map(|s| s.as_str());
                }
                _ if src_dir.is_none() => src_dir = Some(args[ei].as_str()),
                _ => {
                    eprintln!("unknown argument: {}", args[ei]);
                    process::exit(1);
                }
            }
            ei += 1;
        }
        let src = src_dir.unwrap_or(".");
        let dir = Path::new(src);
        if !dir.is_dir() {
            eprintln!("error: '{}' is not a directory", src);
            process::exit(1);
        }
        let hl_files = collect_hl_files_recursive(dir);
        if hl_files.is_empty() {
            eprintln!("no .hl files found in {}", src);
            process::exit(1);
        }
        let zip_path = out_file.unwrap_or("site.zip");
        // Build to temp directory, then create zip
        let tmp_dir = env::temp_dir().join("htmlang-export");
        let _ = fs::remove_dir_all(&tmp_dir);
        let _ = fs::create_dir_all(&tmp_dir);
        let mut any_errors = false;
        for file in &hl_files {
            let rel = file.strip_prefix(dir).unwrap_or(file);
            let out_path = tmp_dir.join(rel).with_extension("html");
            if let Some(parent) = out_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let path_str = file.to_string_lossy().to_string();
            let (has_errors, _) = compile(
                &path_str,
                &CompileConfig {
                    output_path: Some(&out_path.to_string_lossy()),
                    ..Default::default()
                },
            );
            if has_errors {
                any_errors = true;
            }
        }
        // Copy non-.hl assets
        copy_non_hl_files(dir, &tmp_dir);
        if any_errors {
            eprintln!("export completed with errors");
        }
        // Create a simple tar-like zip (concatenated files with headers)
        // Since we can't use external crates, we'll create a self-contained directory listing
        let export_files = collect_all_files_recursive(&tmp_dir);
        let zip_file = std::fs::File::create(zip_path);
        match zip_file {
            Ok(mut f) => {
                use std::io::Write;
                // Write a simple archive format: filename\0length\0content
                for file in &export_files {
                    let rel = file.strip_prefix(&tmp_dir).unwrap_or(file);
                    if let Ok(data) = fs::read(file) {
                        let _ = write!(f, "FILE:{}:{}:", rel.display(), data.len());
                        let _ = f.write_all(&data);
                    }
                }
                eprintln!("exported {} files to {}", export_files.len(), zip_path);
            }
            Err(e) => {
                eprintln!("error: {}: {}", zip_path, e);
                // Fallback: just report the built files in tmp_dir
                eprintln!("files available in {}", tmp_dir.display());
            }
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

    // Handle "repl" subcommand
    if args.len() >= 2 && args[1] == "repl" {
        use std::io::{self, BufRead, Write};
        eprintln!("htmlang repl — type .hl code, then a blank line to compile (Ctrl+D to exit)");
        let stdin = io::stdin();
        let stdout = io::stdout();
        loop {
            eprint!("> ");
            let _ = io::stderr().flush();
            let mut buffer = String::new();
            loop {
                let mut line = String::new();
                match stdin.lock().read_line(&mut line) {
                    Ok(0) => {
                        if buffer.is_empty() {
                            return;
                        }
                        break;
                    }
                    Ok(_) => {
                        if line.trim().is_empty() && !buffer.is_empty() {
                            break;
                        }
                        buffer.push_str(&line);
                    }
                    Err(_) => return,
                }
            }
            if buffer.trim().is_empty() {
                continue;
            }
            let result = htmlang::parser::parse(&buffer);
            for d in &result.diagnostics {
                let prefix = severity_label(d.severity);
                eprintln!("{}: line {}: {}", prefix, d.line, d.message);
            }
            if !result
                .diagnostics
                .iter()
                .any(|d| d.severity == htmlang::parser::Severity::Error)
            {
                let html = htmlang::codegen::generate_dev(&result.document);
                let _ = stdout.lock().write_all(html.as_bytes());
                let _ = stdout.lock().write_all(b"\n");
            }
        }
    }

    // Handle "feed" subcommand
    if args.len() >= 2 && args[1] == "feed" {
        let dir = if args.len() >= 3 && !args[2].starts_with('-') {
            &args[2]
        } else {
            "."
        };
        let base_url = args
            .iter()
            .position(|a| a == "--base-url" || a == "-b")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("https://example.com");
        let dir_path = Path::new(dir);
        let hl_files = collect_hl_files_recursive(dir_path);
        if hl_files.is_empty() {
            eprintln!("no .hl files found in {}", dir);
            process::exit(1);
        }
        let mut items = Vec::new();
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let base = file.parent();
            let result = htmlang::parser::parse_with_base(&input, base);
            if let Some(ref title) = result.document.page_title {
                let rel = file.strip_prefix(dir_path).unwrap_or(file);
                let url_path = rel
                    .with_extension("html")
                    .to_string_lossy()
                    .replace('\\', "/");
                let url = if url_path == "index.html" {
                    format!("{}/", base_url.trim_end_matches('/'))
                } else {
                    format!("{}/{}", base_url.trim_end_matches('/'), url_path)
                };
                // Get description from meta tags if available
                let description = result
                    .document
                    .meta_tags
                    .iter()
                    .find(|(k, _)| k == "description")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                items.push((title.clone(), url, description));
            }
        }
        // Generate RSS 2.0 feed
        let site_title = items
            .first()
            .map(|(t, _, _)| t.clone())
            .unwrap_or_else(|| "My Site".to_string());
        let mut rss = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <rss version=\"2.0\">\n\
             <channel>\n\
             <title>{}</title>\n\
             <link>{}</link>\n\
             <description>RSS feed generated by htmlang</description>\n",
            xml_escape(&site_title),
            xml_escape(base_url)
        );
        for (title, url, description) in &items {
            rss.push_str(&format!(
                "<item>\n<title>{}</title>\n<link>{}</link>\n<description>{}</description>\n</item>\n",
                xml_escape(title),
                xml_escape(url),
                xml_escape(description)
            ));
        }
        rss.push_str("</channel>\n</rss>\n");
        let out_path = dir_path.join("feed.xml");
        match fs::write(&out_path, &rss) {
            Ok(()) => eprintln!("wrote {} ({} items)", out_path.display(), items.len()),
            Err(e) => {
                eprintln!("error: {}: {}", out_path.display(), e);
                process::exit(1);
            }
        }
        return;
    }

    // Handle "components" subcommand
    if args.len() >= 2 && args[1] == "components" {
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
        let mut total = 0usize;
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut found = Vec::new();
            for (line_num, line) in input.lines().enumerate() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("@let ") {
                    // Detect function definitions: @let name with indented body
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if let Some(&name) = parts.first() {
                        // Check if next line is indented (indicating a function body)
                        let next_line = input.lines().nth(line_num + 1);
                        let is_fn = next_line
                            .map(|l| l.starts_with("  ") || l.starts_with('\t'))
                            .unwrap_or(false);
                        // Also include if it has $params (even without checking body)
                        let has_params =
                            parts.len() > 1 && parts[1..].iter().any(|p| p.starts_with('$'));
                        if is_fn || has_params {
                            let params: Vec<&str> = parts[1..]
                                .iter()
                                .filter(|p| p.starts_with('$'))
                                .map(|p| p.strip_prefix('$').unwrap_or(p))
                                .collect();
                            let param_str = if params.is_empty() {
                                String::new()
                            } else {
                                format!(" ({})", params.join(", "))
                            };
                            found.push((line_num + 1, name.to_string(), param_str));
                        }
                    }
                }
            }
            if !found.is_empty() {
                let rel = file.strip_prefix(path).unwrap_or(file);
                for (line, name, params) in &found {
                    println!("  @{}{} — {}:{}", name, params, rel.display(), line);
                    total += 1;
                }
            }
        }
        if total == 0 {
            eprintln!("no component definitions found");
        } else {
            eprintln!("\n{} component(s) found", total);
        }
        return;
    }

    // Handle "deps" subcommand — show dependency graph
    if args.len() >= 2 && args[1] == "deps" {
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
        let mut graph: Vec<(String, Vec<String>)> = Vec::new();
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rel = file.strip_prefix(path).unwrap_or(file);
            let rel_str = rel.display().to_string();
            let mut deps = Vec::new();
            for line in input.lines() {
                let trimmed = line.trim();
                for directive in &["@include ", "@import ", "@extends ", "@use "] {
                    if let Some(rest) = trimmed.strip_prefix(directive) {
                        let dep = rest.split_whitespace().next().unwrap_or("").to_string();
                        if !dep.is_empty() {
                            deps.push(dep);
                        }
                    }
                }
            }
            graph.push((rel_str, deps));
        }
        for (file, deps) in &graph {
            if deps.is_empty() {
                println!("{} (no dependencies)", file);
            } else {
                println!("{}", file);
                for (i, dep) in deps.iter().enumerate() {
                    let prefix = if i == deps.len() - 1 {
                        "└── "
                    } else {
                        "├── "
                    };
                    println!("  {}{}", prefix, dep);
                }
            }
        }
        return;
    }

    // Handle "dead-code" subcommand — project-wide unused definitions
    if args.len() >= 2 && args[1] == "dead-code" {
        let mut dc_json = false;
        let mut dc_target = ".".to_string();
        let mut i = 2;
        while i < args.len() {
            match args[i].as_str() {
                "--format" => {
                    i += 1;
                    if args.get(i).map(|s| s.as_str()) == Some("json") {
                        dc_json = true;
                    }
                }
                _ if dc_target == "." => dc_target = args[i].clone(),
                _ => {}
            }
            i += 1;
        }
        let target = dc_target.as_str();
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
        // Pass 1: collect all definitions and usages across all files
        let mut all_let_defs: Vec<(String, String, usize)> = Vec::new(); // (name, file, line)
        let mut all_refs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rel = file
                .strip_prefix(path)
                .unwrap_or(file)
                .display()
                .to_string();
            for (line_num, line) in input.lines().enumerate() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("@let ")
                    && let Some(name) = rest.split_whitespace().next()
                {
                    let name = name.trim_end_matches('[');
                    all_let_defs.push((name.to_string(), rel.clone(), line_num + 1));
                }
                // Collect references: @name calls and $name usages
                if trimmed.starts_with('@')
                    && !trimmed.starts_with("@let ")
                    && !trimmed.starts_with("@include ")
                    && !trimmed.starts_with("@import ")
                    && !trimmed.starts_with("@extends ")
                    && !trimmed.starts_with("@page")
                    && !trimmed.starts_with("@meta ")
                    && !trimmed.starts_with("@og ")
                    && !trimmed.starts_with("@head")
                    && !trimmed.starts_with("@style")
                    && !trimmed.starts_with("@keyframes ")
                    && !trimmed.starts_with("@theme")
                    && !trimmed.starts_with("@deprecated ")
                    && !trimmed.starts_with("@breakpoint ")
                    && !trimmed.starts_with("@use ")
                    && !trimmed.starts_with("@slot ")
                    && !trimmed.starts_with("@lang ")
                    && !trimmed.starts_with("@favicon ")
                    && !trimmed.starts_with("--")
                    && let Some(name) = trimmed[1..].split([' ', '[']).next()
                {
                    all_refs.insert(name.to_string());
                }
                // Collect $var references
                let mut rest = trimmed;
                while let Some(pos) = rest.find('$') {
                    let after = &rest[pos + 1..];
                    let end = after
                        .find(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .unwrap_or(after.len());
                    if end > 0 {
                        all_refs.insert(after[..end].to_string());
                    }
                    rest = if end < after.len() { &after[end..] } else { "" };
                }
            }
        }
        // Pass 2: report unused
        let mut unused: Vec<(String, String, String, usize)> = Vec::new(); // (kind, name, file, line)
        for (name, file, line) in &all_let_defs {
            if !all_refs.contains(name) && !name.starts_with("--") {
                unused.push(("@let".to_string(), name.clone(), file.clone(), *line));
            }
        }
        if dc_json {
            let arr = json_array(unused.iter().map(|(kind, name, file, line)| {
                json_object(&[
                    ("kind", json_escape_string(kind)),
                    ("name", json_escape_string(name)),
                    ("file", json_escape_string(file)),
                    ("line", line.to_string()),
                ])
            }));
            println!("{}", json_object(&[("unused", arr)]));
        } else if unused.is_empty() {
            eprintln!("no unused definitions found");
        } else {
            for (kind, name, file, line) in &unused {
                println!("unused {} '{}' — {}:{}", kind, name, file, line);
            }
            eprintln!("\n{} unused definition(s) found", unused.len());
        }
        return;
    }

    // Handle "deploy" subcommand — deploy to GitHub Pages or other providers
    if args.len() >= 2 && args[1] == "deploy" {
        let mut deploy_target = ".";
        let mut provider = "github-pages";
        let mut di = 2;
        while di < args.len() {
            match args[di].as_str() {
                "--provider" | "-P" => {
                    di += 1;
                    if let Some(p) = args.get(di) {
                        provider = match p.as_str() {
                            "netlify" | "vercel" | "cloudflare" | "github-pages" => {
                                // Leak to get &str lifetime — fine for CLI
                                args[di].as_str()
                            }
                            _ => {
                                eprintln!(
                                    "unknown provider: {} (supported: github-pages, netlify, vercel, cloudflare)",
                                    p
                                );
                                process::exit(1);
                            }
                        };
                    }
                }
                _ if deploy_target == "." => deploy_target = &args[di],
                _ => {}
            }
            di += 1;
        }
        let target = deploy_target;
        let path = Path::new(target);
        if !path.is_dir() {
            eprintln!("error: '{}' is not a directory", target);
            process::exit(1);
        }
        // Build first
        let hl_files = collect_hl_files_recursive(path);
        if hl_files.is_empty() {
            eprintln!("no .hl files found in {}", target);
            process::exit(1);
        }
        let deploy_dir = path.join("_deploy");
        let _ = fs::create_dir_all(&deploy_dir);
        let any_errors = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            for file in &hl_files {
                let any_errors = &any_errors;
                let deploy_dir = &deploy_dir;
                s.spawn(move || {
                    let rel = file.strip_prefix(path).unwrap_or(file);
                    let out_path = deploy_dir.join(rel).with_extension("html");
                    if let Some(parent) = out_path.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let out_str = out_path.to_string_lossy().to_string();
                    let path_str = file.to_string_lossy().to_string();
                    let (has_errors, _) = compile(
                        &path_str,
                        &CompileConfig {
                            output_path: Some(&out_str),
                            ..Default::default()
                        },
                    );
                    if has_errors {
                        any_errors.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
            }
        });
        if any_errors.load(std::sync::atomic::Ordering::Relaxed) {
            eprintln!("error: build failed, not deploying");
            process::exit(1);
        }
        copy_non_hl_files(path, &deploy_dir);
        eprintln!("built {} files to {}", hl_files.len(), deploy_dir.display());

        // Provider-specific deploy
        match provider {
            "netlify" => {
                eprintln!("deploying to Netlify...");
                let status = process::Command::new("npx")
                    .args([
                        "netlify-cli",
                        "deploy",
                        "--prod",
                        "--dir",
                        &deploy_dir.to_string_lossy(),
                    ])
                    .status();
                match status {
                    Ok(s) if s.success() => eprintln!("deployed to Netlify!"),
                    _ => {
                        eprintln!(
                            "netlify deploy failed — ensure netlify-cli is installed (npx netlify-cli deploy --prod --dir {})",
                            deploy_dir.display()
                        );
                        process::exit(1);
                    }
                }
                return;
            }
            "vercel" => {
                eprintln!("deploying to Vercel...");
                let status = process::Command::new("npx")
                    .args(["vercel", "--prod", deploy_dir.to_string_lossy().as_ref()])
                    .status();
                match status {
                    Ok(s) if s.success() => eprintln!("deployed to Vercel!"),
                    _ => {
                        eprintln!(
                            "vercel deploy failed — ensure vercel CLI is installed (npx vercel --prod {})",
                            deploy_dir.display()
                        );
                        process::exit(1);
                    }
                }
                return;
            }
            "cloudflare" => {
                eprintln!("deploying to Cloudflare Pages...");
                let status = process::Command::new("npx")
                    .args([
                        "wrangler",
                        "pages",
                        "deploy",
                        deploy_dir.to_string_lossy().as_ref(),
                    ])
                    .status();
                match status {
                    Ok(s) if s.success() => eprintln!("deployed to Cloudflare Pages!"),
                    _ => {
                        eprintln!(
                            "cloudflare deploy failed — ensure wrangler is installed (npx wrangler pages deploy {})",
                            deploy_dir.display()
                        );
                        process::exit(1);
                    }
                }
                return;
            }
            _ => {} // fall through to GitHub Pages
        }

        // Deploy via gh-pages push
        let init_ok = matches!(
            process::Command::new("git")
                .args(["init"])
                .current_dir(&deploy_dir)
                .status(),
            Ok(s) if s.success()
        );
        if !init_ok {
            eprintln!("error: git init failed in deploy directory");
            process::exit(1);
        }
        let _ = process::Command::new("git")
            .args(["checkout", "-b", "gh-pages"])
            .current_dir(&deploy_dir)
            .status();
        let _ = process::Command::new("git")
            .args(["add", "."])
            .current_dir(&deploy_dir)
            .status();
        let commit_ok = matches!(
            process::Command::new("git")
                .args(["commit", "-m", "deploy"])
                .current_dir(&deploy_dir)
                .status(),
            Ok(s) if s.success()
        );
        if !commit_ok {
            eprintln!("error: git commit failed");
            process::exit(1);
        }
        // Check if remote origin exists in parent
        let remote_output = process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .output();
        if let Ok(output) = remote_output {
            if output.status.success() {
                let remote_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let _ = process::Command::new("git")
                    .args(["remote", "add", "origin", &remote_url])
                    .current_dir(&deploy_dir)
                    .status();
                eprintln!("pushing to gh-pages branch at {}...", remote_url);
                let push_ok = matches!(
                    process::Command::new("git")
                        .args(["push", "-f", "origin", "gh-pages"])
                        .current_dir(&deploy_dir)
                        .status(),
                    Ok(s) if s.success()
                );
                if !push_ok {
                    eprintln!(
                        "error: push failed — run 'git push -f origin gh-pages' manually from {}",
                        deploy_dir.display()
                    );
                    process::exit(1);
                }
                eprintln!("deployed to GitHub Pages!");
            } else {
                eprintln!(
                    "no git remote found — deploy directory ready at {}",
                    deploy_dir.display()
                );
                eprintln!(
                    "push manually: cd {} && git remote add origin <url> && git push -f origin gh-pages",
                    deploy_dir.display()
                );
            }
        }
        return;
    }

    // Handle "playground" subcommand — generate self-contained HTML playground
    if args.len() >= 2 && args[1] == "playground" {
        let out = if args.len() >= 3 {
            &args[2]
        } else {
            "playground.html"
        };

        // Load pre-built WASM binary and JS glue
        let (wasm_bytes, js_glue) = match find_wasm_pkg() {
            Some(pkg) => pkg,
            None => {
                eprintln!("error: WASM module not found. Build it first:");
                eprintln!(
                    "  cargo build --release --target wasm32-unknown-unknown -p htmlang-wasm"
                );
                eprintln!(
                    "  wasm-bindgen --target no-modules --no-typescript --out-dir target/wasm-pkg \\"
                );
                eprintln!("    target/wasm32-unknown-unknown/release/htmlang_wasm.wasm");
                process::exit(1);
            }
        };
        let b64 = base64_encode(&wasm_bytes);
        let wasm_section = format!(
            "<script>\n{}\n</script>\n<script>\n\
            (function(){{\n\
              var b64=\"{}\";\n\
              var raw=atob(b64);\n\
              var bytes=new Uint8Array(raw.length);\n\
              for(var i=0;i<raw.length;i++)bytes[i]=raw.charCodeAt(i);\n\
              wasm_bindgen.initSync(bytes);\n\
              window._wasmCompile=wasm_bindgen.compile;\n\
            }})();\n\
            </script>",
            js_glue, b64,
        );
        let compile_fn = "function compileSource(src){return window._wasmCompile(src);}";
        let editor_content = include_str!("playground_default.hl");

        let playground_html = format!(
            r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>htmlang Playground</title>
<style>
*{{box-sizing:border-box;margin:0}}
body{{font-family:system-ui,-apple-system,sans-serif;display:flex;height:100vh;background:#1a1a2e;color:#eee}}
.panel{{flex:1;display:flex;flex-direction:column;min-width:0}}
.header{{padding:12px 16px;background:#16213e;border-bottom:1px solid #0f3460;display:flex;align-items:center;gap:12px}}
.header h1{{font-size:14px;font-weight:600;color:#e94560}}
.header .tag{{font-size:11px;padding:2px 8px;background:#0f3460;border-radius:4px;color:#a8b2d1}}
.editor-wrap{{flex:1;position:relative;overflow:hidden}}
textarea{{width:100%;height:100%;resize:none;border:none;outline:none;padding:16px;font-family:ui-monospace,monospace;font-size:14px;line-height:1.6;background:#1a1a2e;color:#e6e6e6;tab-size:2;position:absolute;top:0;left:0}}
iframe{{flex:1;border:none;background:white}}
.divider{{width:1px;background:#0f3460;cursor:col-resize}}
.divider:hover{{background:#e94560}}
.toolbar{{padding:8px 16px;background:#16213e;border-top:1px solid #0f3460;display:flex;gap:8px;align-items:center}}
button{{padding:6px 16px;border:none;border-radius:4px;cursor:pointer;font-size:13px;font-weight:500}}
.btn-run{{background:#e94560;color:white}}
.btn-run:hover{{background:#c81e45}}
.btn-copy{{background:#0f3460;color:#a8b2d1}}
.btn-share{{background:#0f3460;color:#a8b2d1}}
.status{{margin-left:auto;font-size:12px;color:#666}}
</style>
</head>
<body>
<div class="panel" id="editor-panel">
<div class="header"><h1>htmlang</h1><span class="tag">playground</span></div>
<div class="editor-wrap">
<textarea id="editor" spellcheck="false">{editor_content}</textarea>
</div>
<div class="toolbar">
<button class="btn-run" onclick="compile()">&#9654; Run (Ctrl+Enter)</button>
<button class="btn-copy" onclick="copyOutput()">Copy HTML</button>
<button class="btn-share" onclick="share()">Share</button>
<span class="status" id="status">Ready</span>
</div>
</div>
<div class="divider" id="divider"></div>
<div class="panel" id="preview-panel">
<div class="header"><span class="tag">preview</span></div>
<iframe id="preview"></iframe>
</div>
{wasm_section}
<script>
{compile_fn}
var lastOutput='';
function compile(){{
  var src=document.getElementById('editor').value;
  var status=document.getElementById('status');
  var start=performance.now();
  var html=compileSource(src);
  var ms=(performance.now()-start).toFixed(1);
  lastOutput=html;
  document.getElementById('preview').srcdoc=html;
  status.textContent='Compiled in '+ms+'ms';
}}
function copyOutput(){{if(lastOutput)navigator.clipboard.writeText(lastOutput).then(function(){{document.getElementById('status').textContent='Copied to clipboard!'}});}}
function share(){{
  var src=document.getElementById('editor').value;
  var encoded=btoa(unescape(encodeURIComponent(src)));
  var url=location.origin+location.pathname+'#'+encoded;
  navigator.clipboard.writeText(url).then(function(){{document.getElementById('status').textContent='Share URL copied to clipboard!'}});
  history.replaceState(null,'','#'+encoded);
}}
function loadFromHash(){{
  if(location.hash.length>1){{
    try{{
      var decoded=decodeURIComponent(escape(atob(location.hash.slice(1))));
      document.getElementById('editor').value=decoded;
    }}catch(e){{}}
  }}
}}
// Resizable divider
var divider=document.getElementById('divider');
var editorPanel=document.getElementById('editor-panel');
var dragging=false;
divider.addEventListener('mousedown',function(){{dragging=true;document.body.style.cursor='col-resize';document.body.style.userSelect='none';}});
document.addEventListener('mousemove',function(e){{if(!dragging)return;var pct=(e.clientX/window.innerWidth)*100;editorPanel.style.flex='none';editorPanel.style.width=pct+'%';}});
document.addEventListener('mouseup',function(){{if(dragging){{dragging=false;document.body.style.cursor='';document.body.style.userSelect='';}}}});
// Keyboard shortcuts and auto-compile
var editor=document.getElementById('editor');
editor.addEventListener('keydown',function(e){{
  if(e.key==='Enter'&&(e.ctrlKey||e.metaKey)){{e.preventDefault();compile();}}
  if(e.key==='Tab'){{e.preventDefault();var s=e.target;var start=s.selectionStart;s.value=s.value.substring(0,start)+'  '+s.value.substring(s.selectionEnd);s.selectionStart=s.selectionEnd=start+2;}}
}});
loadFromHash();
compile();
</script>
</body>
</html>"##,
            wasm_section = wasm_section,
            compile_fn = compile_fn
        );
        if let Err(e) = fs::write(out, playground_html) {
            eprintln!("error writing {}: {}", out, e);
            process::exit(1);
        }
        eprintln!("open in browser: open {}", out);
        return;
    }

    // Handle "clean" subcommand — remove generated .html files
    if args.len() >= 2 && args[1] == "clean" {
        let target = if args.len() >= 3 { &args[2] } else { "." };
        let dir = Path::new(target);
        if !dir.is_dir() {
            eprintln!("error: '{}' is not a directory", target);
            process::exit(1);
        }
        let hl_files = collect_hl_files_recursive(dir);
        let mut removed = 0;
        for hl_file in &hl_files {
            let html_file = hl_file.with_extension("html");
            if html_file.exists() {
                match fs::remove_file(&html_file) {
                    Ok(()) => {
                        eprintln!("removed {}", html_file.display());
                        removed += 1;
                    }
                    Err(e) => eprintln!("error: {}: {}", html_file.display(), e),
                }
            }
        }
        // Also remove sitemap.xml if present
        let sitemap = dir.join("sitemap.xml");
        if sitemap.exists()
            && let Ok(()) = fs::remove_file(&sitemap)
        {
            eprintln!("removed {}", sitemap.display());
            removed += 1;
        }
        eprintln!(
            "cleaned {} file{}",
            removed,
            if removed == 1 { "" } else { "s" }
        );
        return;
    }

    // Handle "outline" subcommand — show document structure
    if args.len() >= 2 && args[1] == "outline" {
        if args.len() < 3 {
            eprintln!("usage: htmlang outline <file.hl>");
            process::exit(1);
        }
        let path = &args[2];
        let input = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {}: {}", path, e);
                process::exit(1);
            }
        };
        let base = Path::new(path).parent();
        let result = htmlang::parser::parse_with_base(&input, base);
        if let Some(title) = &result.document.page_title {
            eprintln!("@page {}", title);
        }
        fn print_outline(nodes: &[htmlang::ast::Node], depth: usize) {
            let indent = "  ".repeat(depth);
            for node in nodes {
                match node {
                    htmlang::ast::Node::Element(elem) => {
                        let kind = format!("{:?}", elem.kind);
                        let kind = kind.split('(').next().unwrap_or(&kind);
                        let label = if let Some(arg) = &elem.argument {
                            format!("@{} {}", kind.to_lowercase(), arg)
                        } else {
                            format!("@{}", kind.to_lowercase())
                        };
                        eprintln!("{}{}", indent, label);
                        print_outline(&elem.children, depth + 1);
                    }
                    htmlang::ast::Node::Text(segs) => {
                        let text: String = segs
                            .iter()
                            .filter_map(|s| match s {
                                htmlang::ast::TextSegment::Plain(t) => Some(t.as_str()),
                                _ => None,
                            })
                            .collect();
                        if !text.trim().is_empty() {
                            let display = if text.chars().count() > 40 {
                                format!("{}...", text.chars().take(37).collect::<String>())
                            } else {
                                text
                            };
                            eprintln!("{}\"{}\"", indent, display.trim());
                        }
                    }
                    htmlang::ast::Node::Raw(_) => {
                        eprintln!("{}@raw ...", indent);
                    }
                }
            }
        }
        print_outline(&result.document.nodes, 0);
        return;
    }

    // Handle "doctor" subcommand — check toolchain health
    if args.len() >= 2 && args[1] == "doctor" {
        eprintln!("htmlang doctor — checking toolchain health\n");
        let mut ok = true;

        // Check htmlang version
        eprintln!("  htmlang:     v{}", env!("CARGO_PKG_VERSION"));

        // Check if htmlang-lsp binary is available
        let lsp_status = process::Command::new("htmlang-lsp")
            .arg("--version")
            .output();
        match lsp_status {
            Ok(output) if output.status.success() => {
                let ver = String::from_utf8_lossy(&output.stdout);
                eprintln!("  htmlang-lsp: {}", ver.trim());
            }
            _ => {
                eprintln!("  htmlang-lsp: not found (optional — needed for editor integration)");
            }
        }

        // Check if cargo is available (for building from source)
        let cargo_status = process::Command::new("cargo").arg("--version").output();
        match cargo_status {
            Ok(output) if output.status.success() => {
                let ver = String::from_utf8_lossy(&output.stdout);
                eprintln!("  cargo:       {}", ver.trim());
            }
            _ => {
                eprintln!("  cargo:       not found");
                ok = false;
            }
        }

        // Check if git is available (for deploy)
        let git_status = process::Command::new("git").arg("--version").output();
        match git_status {
            Ok(output) if output.status.success() => {
                let ver = String::from_utf8_lossy(&output.stdout);
                eprintln!("  git:         {}", ver.trim());
            }
            _ => {
                eprintln!("  git:         not found (optional — needed for deploy)");
            }
        }

        // Check for htmlang.toml in current directory
        if Path::new("htmlang.toml").exists() {
            eprintln!("  config:      htmlang.toml found");
        } else {
            eprintln!("  config:      no htmlang.toml (using defaults)");
        }

        // Check for .hl files
        let hl_count = collect_hl_files_recursive(Path::new(".")).len();
        eprintln!("  .hl files:   {} found in current directory", hl_count);

        eprintln!();
        if ok {
            eprintln!("all checks passed");
        } else {
            eprintln!("some checks failed — see above");
            process::exit(1);
        }
        return;
    }

    // Handle "migrate" subcommand — auto-upgrade deprecated syntax
    if args.len() >= 2 && args[1] == "migrate" {
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
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut output = String::new();
            let mut changes = 0usize;
            for line in input.lines() {
                let mut migrated = line.to_string();
                // Migrate @divider -> @hr
                if migrated.trim().starts_with("@divider") {
                    migrated = migrated.replace("@divider", "@hr");
                    changes += 1;
                }
                // Migrate @ul -> @list
                if migrated.trim().starts_with("@ul") && !migrated.trim().starts_with("@unless") {
                    migrated = migrated.replacen("@ul", "@list", 1);
                    changes += 1;
                }
                // Migrate align-center -> center-x (common mistake)
                if migrated.contains("align-center") {
                    migrated = migrated.replace("align-center", "center-x");
                    changes += 1;
                }
                output.push_str(&migrated);
                output.push('\n');
            }
            // Remove trailing extra newline if original didn't end with one
            if !input.ends_with('\n') && output.ends_with('\n') {
                output.pop();
            }
            if changes > 0 {
                match fs::write(file, &output) {
                    Ok(()) => {
                        eprintln!(
                            "migrated {} ({} change{})",
                            file.display(),
                            changes,
                            if changes == 1 { "" } else { "s" }
                        );
                        total_changes += changes;
                    }
                    Err(e) => eprintln!("error: {}: {}", file.display(), e),
                }
            }
        }
        if total_changes == 0 {
            eprintln!("no migrations needed");
        } else {
            eprintln!(
                "\n{} total change(s) across {} file(s)",
                total_changes,
                hl_files.len()
            );
        }
        return;
    }

    // Handle "bundle" subcommand — inline images/fonts as data URIs
    if args.len() >= 2 && args[1] == "bundle" {
        if args.len() < 3 {
            eprintln!("usage: htmlang bundle <file.hl | dir> [-o <out>]");
            process::exit(1);
        }
        let mut bundle_target = None;
        let mut bundle_out = None;
        let mut bi = 2;
        while bi < args.len() {
            match args[bi].as_str() {
                "-o" | "--output" => {
                    bi += 1;
                    bundle_out = args.get(bi).map(|s| s.as_str());
                }
                _ if bundle_target.is_none() => bundle_target = Some(args[bi].as_str()),
                _ => {
                    eprintln!("unknown argument: {}", args[bi]);
                    process::exit(1);
                }
            }
            bi += 1;
        }
        let target = bundle_target.unwrap_or(".");
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
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {}: {}", file.display(), e);
                    continue;
                }
            };
            let base = file.parent();
            let result = htmlang::parser::parse_with_base(&input, base);
            if result
                .diagnostics
                .iter()
                .any(|d| d.severity == htmlang::parser::Severity::Error)
            {
                for d in &result.diagnostics {
                    if d.severity == htmlang::parser::Severity::Error {
                        eprintln!("error: line {}: {}", d.line, d.message);
                    }
                }
                continue;
            }
            let html = htmlang::codegen::generate(&result.document);
            // Inline external assets: images and fonts referenced in the HTML
            let bundled = bundle_assets(&html, base.unwrap_or(Path::new(".")));
            let out_path = match bundle_out {
                Some(o) => {
                    let p = Path::new(o);
                    if p.is_dir() || hl_files.len() > 1 {
                        let _ = fs::create_dir_all(p);
                        let rel = file.strip_prefix(path).unwrap_or(file);
                        p.join(rel).with_extension("html")
                    } else {
                        PathBuf::from(o)
                    }
                }
                None => file.with_extension("html"),
            };
            match fs::write(&out_path, &bundled) {
                Ok(()) => eprintln!("bundled {}", out_path.display()),
                Err(e) => eprintln!("error: {}: {}", out_path.display(), e),
            }
        }
        return;
    }

    // Handle "size" subcommand — report output sizes with gzip estimates
    if args.len() >= 2 && args[1] == "size" {
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
        let mut total_source = 0usize;
        let mut total_output = 0usize;
        let mut total_minified = 0usize;
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            total_source += input.len();
            let base = file.parent();
            let result = htmlang::parser::parse_with_base(&input, base);
            if result
                .diagnostics
                .iter()
                .any(|d| d.severity == htmlang::parser::Severity::Error)
            {
                continue;
            }
            let html = htmlang::codegen::generate(&result.document);
            let minified = htmlang::codegen::generate_minified(&result.document);
            let html_len = html.len();
            let min_len = minified.len();
            // Estimate gzip size as ~30% of minified (rough heuristic for HTML)
            let gzip_est = min_len * 30 / 100;
            total_output += html_len;
            total_minified += min_len;
            eprintln!(
                "  {}:  {} → {} (minified: {}, ~gzip: {})",
                file.display(),
                format_bytes(input.len()),
                format_bytes(html_len),
                format_bytes(min_len),
                format_bytes(gzip_est),
            );
        }
        eprintln!("---");
        let total_gzip = total_minified * 30 / 100;
        eprintln!(
            "  total:  {} source → {} output (minified: {}, ~gzip: {})",
            format_bytes(total_source),
            format_bytes(total_output),
            format_bytes(total_minified),
            format_bytes(total_gzip),
        );
        return;
    }

    // Handle "explain" subcommand — show CSS mapping for each line
    if args.len() >= 2 && args[1] == "explain" {
        if args.len() < 3 {
            eprintln!("usage: htmlang explain <file.hl>");
            process::exit(1);
        }
        let file = &args[2];
        let input = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {}: {}", file, e);
                process::exit(1);
            }
        };
        let base = Path::new(file).parent();
        let result = htmlang::parser::parse_with_base(&input, base);
        for d in &result.diagnostics {
            let prefix = severity_label(d.severity);
            eprintln!("{}: line {}: {}", prefix, d.line, d.message);
        }
        if result
            .diagnostics
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error)
        {
            process::exit(1);
        }
        let html = htmlang::codegen::generate_dev(&result.document);
        // Extract CSS class mappings from <style> block
        let mut class_css: HashMap<String, String> = HashMap::new();
        if let Some(style_start) = html.find("<style>")
            && let Some(style_end) = html[style_start..].find("</style>")
        {
            let css_block = &html[style_start + 7..style_start + style_end];
            // Parse each CSS rule: .className { rules }
            let mut pos = 0;
            let css_bytes = css_block.as_bytes();
            while pos < css_bytes.len() {
                if css_bytes[pos] == b'.' {
                    let rule_start = pos;
                    // Find class name end
                    pos += 1;
                    while pos < css_bytes.len()
                        && css_bytes[pos] != b' '
                        && css_bytes[pos] != b'{'
                        && css_bytes[pos] != b':'
                        && css_bytes[pos] != b','
                    {
                        pos += 1;
                    }
                    let class_name = &css_block[rule_start + 1..pos];
                    // Find rule body
                    while pos < css_bytes.len() && css_bytes[pos] != b'{' {
                        pos += 1;
                    }
                    if pos < css_bytes.len() {
                        pos += 1; // skip '{'
                        let body_start = pos;
                        let mut depth = 1;
                        while pos < css_bytes.len() && depth > 0 {
                            if css_bytes[pos] == b'{' {
                                depth += 1;
                            }
                            if css_bytes[pos] == b'}' {
                                depth -= 1;
                            }
                            pos += 1;
                        }
                        let body = css_block[body_start..pos.saturating_sub(1)].trim();
                        if !body.is_empty() {
                            class_css
                                .entry(class_name.to_string())
                                .and_modify(|existing| {
                                    existing.push_str("; ");
                                    existing.push_str(body);
                                })
                                .or_insert_with(|| body.to_string());
                        }
                    }
                } else {
                    pos += 1;
                }
            }
        }
        // Extract data-hl-line attributes from dev HTML to map classes to source lines
        let mut line_classes: HashMap<usize, Vec<String>> = HashMap::new();
        let html_body = if let Some(body_start) = html.find("<body") {
            &html[body_start..]
        } else {
            &html
        };
        // Scan for elements with data-hl-line="N" and class="X"
        // Dev mode outputs: <div class="a" data-hl-line="2" data-hl-el="el">
        let mut scan_pos = 0;
        while scan_pos < html_body.len() {
            // Find each opening tag
            if html_body.as_bytes()[scan_pos] == b'<'
                && scan_pos + 1 < html_body.len()
                && html_body.as_bytes()[scan_pos + 1].is_ascii_alphabetic()
            {
                // Find end of tag
                let tag_end = html_body[scan_pos..]
                    .find('>')
                    .map(|p| scan_pos + p)
                    .unwrap_or(html_body.len());
                let tag = &html_body[scan_pos..tag_end];
                // Extract data-hl-line
                if let Some(hl_pos) = tag.find("data-hl-line=\"") {
                    let after = &tag[hl_pos + 14..];
                    if let Some(end) = after.find('"')
                        && let Ok(ln) = after[..end].parse::<usize>()
                    {
                        // Extract class
                        if let Some(cls_pos) = tag.find("class=\"") {
                            let cls_after = &tag[cls_pos + 7..];
                            if let Some(cls_end) = cls_after.find('"') {
                                for cls in cls_after[..cls_end].split_whitespace() {
                                    line_classes.entry(ln).or_default().push(cls.to_string());
                                }
                            }
                        }
                    }
                }
                scan_pos = tag_end;
            } else {
                scan_pos += 1;
            }
        }
        // Print explanation
        for (line_num, line) in input.lines().enumerate() {
            let ln = line_num + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("--") {
                continue;
            }
            print!("{:>4} | {}", ln, line);
            if let Some(classes) = line_classes.get(&ln) {
                let mut css_parts: Vec<String> = Vec::new();
                for cls in classes {
                    if let Some(css) = class_css.get(cls) {
                        css_parts.push(css.clone());
                    }
                }
                if !css_parts.is_empty() {
                    println!();
                    println!("       → {}", css_parts.join("; "));
                } else {
                    println!();
                }
            } else {
                println!();
            }
        }
        return;
    }

    // Handle "benchmark" subcommand — measure compile time and output size
    if args.len() >= 2 && args[1] == "benchmark" {
        if args.len() < 3 {
            eprintln!("usage: htmlang benchmark <file.hl | dir>");
            process::exit(1);
        }
        let target = &args[2];
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

        let mut total_source = 0usize;
        let mut total_output = 0usize;
        let mut total_css_rules = 0usize;
        let mut total_elements = 0usize;
        let mut total_errors = 0usize;

        let start = std::time::Instant::now();
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            total_source += input.len();
            let base = file.parent();
            let result = htmlang::parser::parse_with_base(&input, base);
            let has_errors = result
                .diagnostics
                .iter()
                .any(|d| d.severity == htmlang::parser::Severity::Error);
            if has_errors {
                total_errors += 1;
                continue;
            }
            let html = htmlang::codegen::generate(&result.document);
            total_output += html.len();
            total_css_rules += html.matches('{').count().saturating_sub(1);
            let mut ec = 0;
            let mut colors = std::collections::HashSet::new();
            let mut fonts = std::collections::HashSet::new();
            count_elements(&result.document.nodes, &mut ec, &mut colors, &mut fonts);
            total_elements += ec;
        }
        let elapsed = start.elapsed();

        eprintln!("--- benchmark results ---");
        eprintln!("  files:          {}", hl_files.len());
        eprintln!("  compile time:   {:.1}ms", elapsed.as_secs_f64() * 1000.0);
        eprintln!("  source size:    {} bytes", total_source);
        eprintln!("  output size:    {} bytes", total_output);
        if total_source > 0 {
            eprintln!(
                "  ratio:          {:.1}x",
                total_output as f64 / total_source as f64
            );
        }
        eprintln!("  elements:       {}", total_elements);
        eprintln!("  CSS rules:      ~{}", total_css_rules);
        if total_errors > 0 {
            eprintln!("  errors:         {} file(s) had errors", total_errors);
        }
        if hl_files.len() > 1 {
            let per_file = elapsed.as_secs_f64() * 1000.0 / hl_files.len() as f64;
            eprintln!("  per file:       {:.2}ms", per_file);
        }
        return;
    }

    // Handle "upgrade" subcommand — comprehensive version-to-version migration
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
        for file in &hl_files {
            let input = match fs::read_to_string(file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut output = String::new();
            let mut changes = 0usize;
            for line in input.lines() {
                let mut migrated = line.to_string();
                // Migrate @divider -> @hr
                if migrated.trim().starts_with("@divider") {
                    migrated = migrated.replace("@divider", "@hr");
                    changes += 1;
                }
                // Migrate @ul -> @list
                if migrated.trim().starts_with("@ul") && !migrated.trim().starts_with("@unless") {
                    migrated = migrated.replacen("@ul", "@list", 1);
                    changes += 1;
                }
                // Migrate @col -> @column (full form)
                if migrated.trim().starts_with("@col ") || migrated.trim() == "@col" {
                    migrated = migrated.replacen("@col", "@column", 1);
                    changes += 1;
                }
                // Migrate @img -> @image (full form)
                if migrated.trim().starts_with("@img ") || migrated.trim() == "@img" {
                    migrated = migrated.replacen("@img", "@image", 1);
                    changes += 1;
                }
                // Migrate @p -> @paragraph (full form)
                if migrated.trim().starts_with("@p ") || migrated.trim() == "@p" {
                    migrated = migrated.replacen("@p ", "@paragraph ", 1);
                    changes += 1;
                }
                // Migrate @btn -> @button (full form)
                if migrated.trim().starts_with("@btn ") || migrated.trim() == "@btn" {
                    migrated = migrated.replacen("@btn", "@button", 1);
                    changes += 1;
                }
                // Migrate @li -> @item (full form)
                if migrated.trim().starts_with("@li ") || migrated.trim() == "@li" {
                    migrated = migrated.replacen("@li", "@item", 1);
                    changes += 1;
                }
                // Migrate align-center -> center-x
                if migrated.contains("align-center") {
                    migrated = migrated.replace("align-center", "center-x");
                    changes += 1;
                }
                // Migrate spacing -> gap (modern naming). Only the whole
                // attribute key: `letter-spacing` must stay as it is.
                if migrated.contains('[') {
                    let renamed = rename_attr_key(&migrated, "spacing", "gap");
                    if renamed != migrated {
                        migrated = renamed;
                        changes += 1;
                    }
                }
                output.push_str(&migrated);
                output.push('\n');
            }
            if !input.ends_with('\n') && output.ends_with('\n') {
                output.pop();
            }
            if changes > 0 {
                match fs::write(file, &output) {
                    Ok(()) => {
                        eprintln!(
                            "upgraded {} ({} change{})",
                            file.display(),
                            changes,
                            if changes == 1 { "" } else { "s" }
                        );
                        total_changes += changes;
                    }
                    Err(e) => eprintln!("error: {}: {}", file.display(), e),
                }
            }
        }
        if total_changes == 0 {
            eprintln!("no upgrades needed — all files are up to date");
        } else {
            eprintln!("\n{} total change(s) applied", total_changes);
        }
        return;
    }

    // Handle "create-component" subcommand — scaffold a new component
    if args.len() >= 2 && args[1] == "create-component" {
        if args.len() < 3 {
            eprintln!("usage: htmlang create-component <name> [param1] [param2] ...");
            process::exit(1);
        }
        let comp_name = &args[2];
        let params: Vec<&str> = args[3..].iter().map(|s| s.as_str()).collect();
        let file_name = format!("{}.hl", comp_name);
        let file_path = Path::new(&file_name);
        if file_path.exists() {
            eprintln!("error: {} already exists", file_name);
            process::exit(1);
        }

        let mut template = String::new();
        template.push_str(&format!("-- {} component\n", comp_name));
        template.push_str("-- Usage: @");
        template.push_str(comp_name);
        if !params.is_empty() {
            template.push_str(" [");
            for (i, p) in params.iter().enumerate() {
                if i > 0 {
                    template.push_str(", ");
                }
                template.push_str(p);
                template.push_str(" value");
            }
            template.push(']');
        }
        template.push('\n');
        template.push('\n');

        // Generate @let component definition
        template.push_str("@let ");
        template.push_str(comp_name);
        for p in &params {
            template.push(' ');
            template.push('$');
            template.push_str(p);
        }
        template.push('\n');
        template.push_str("  @el [padding 16, rounded 8, border 1 #e5e7eb]\n");
        if !params.is_empty() {
            for p in &params {
                template.push_str(&format!("    @text ${}\n", p));
            }
        }
        template.push_str("    @children\n");

        match fs::write(file_path, &template) {
            Ok(()) => {
                eprintln!("created {}", file_name);
                eprintln!("import with: @use \"{}\" {}", file_name, comp_name);
            }
            Err(e) => {
                eprintln!("error: {}: {}", file_name, e);
                process::exit(1);
            }
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
            "--compat" => compat = true,
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
                    compat,
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
                compat,
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
            compat,
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
            compat,
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
    compat: bool,
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
                    compat: build.compat,
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
    fn rename_attr_key_only_touches_whole_keys() {
        assert_eq!(
            rename_attr_key(
                "@row [spacing 10, letter-spacing 2, hover:spacing 4]",
                "spacing",
                "gap"
            ),
            "@row [gap 10, letter-spacing 2, hover:gap 4]"
        );
    }

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
