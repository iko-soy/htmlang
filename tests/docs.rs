//! Compile every htmlang example in the docs, so they can't drift from the
//! language. A fenced block without a language tag is htmlang unless it is a
//! shell session (lines starting with `htmlang ` / `cargo `) or the syntax
//! template in DESIGN.md. Blocks whose first line is `-- name.hl` are written
//! to disk first so `@include` between them resolves.

use std::path::Path;

use htmlang::parser::{self, Severity};

fn code_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    // Inside a fence: Some(true) collects an untagged block, Some(false)
    // skips a tagged one (```json etc.).
    let mut fence: Option<bool> = None;
    let mut current = String::new();
    for line in markdown.lines() {
        if line.starts_with("```") {
            match fence.take() {
                Some(true) => blocks.push(std::mem::take(&mut current)),
                Some(false) => {}
                None => fence = Some(line.trim() == "```"),
            }
            continue;
        }
        if fence == Some(true) {
            current.push_str(line);
            current.push('\n');
        }
    }
    blocks
}

fn is_htmlang(block: &str) -> bool {
    let first = block
        .lines()
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with("--"))
        .unwrap_or("");
    !(first.starts_with("htmlang ")
        || first.starts_with("cargo ")
        || first.starts_with("@element "))
}

fn check_doc(file: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let markdown = std::fs::read_to_string(root.join(file)).unwrap();
    let dir = std::env::temp_dir().join(format!("htmlang_docs_{}", file.replace('.', "_")));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let blocks: Vec<String> = code_blocks(&markdown)
        .into_iter()
        .filter(|b| is_htmlang(b))
        .collect();
    // Named blocks become files, so blocks can refer to each other.
    for block in &blocks {
        if let Some(name) = block.lines().next().and_then(|l| l.strip_prefix("-- "))
            && name.ends_with(".hl")
        {
            std::fs::write(dir.join(name.trim()), block).unwrap();
        }
    }

    let mut failures = Vec::new();
    for block in &blocks {
        let result = parser::parse_with_base(block, Some(&dir));
        let problems: Vec<String> = result
            .diagnostics
            .iter()
            .filter(|d| {
                d.severity == Severity::Error
                    || d.message.contains("unknown")
                    || d.message.contains("undefined variable")
                    || d.message.contains("is an HTML attribute")
                    || d.message.contains("was removed")
                    || d.message.contains("no single root")
                    || d.message.contains("filters are functions now")
            })
            .map(|d| format!("line {}: {}", d.line, d.message))
            .collect();
        if !problems.is_empty() {
            failures.push(format!("{}\n=> {}", block, problems.join("\n=> ")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} example(s) in {} don't compile cleanly:\n\n{}",
        failures.len(),
        file,
        failures.join("\n\n")
    );
}

#[test]
fn design_md_examples_compile() {
    check_doc("DESIGN.md");
}

#[test]
fn readme_examples_compile() {
    check_doc("README.md");
}
