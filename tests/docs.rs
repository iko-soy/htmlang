//! Compile every htmlang example in the docs and in `examples/`, so they
//! can't drift from the language. A fenced block without a language tag is htmlang unless it is a
//! shell session (lines starting with `htmlang ` / `cargo `) or the syntax
//! template in DESIGN.md. Blocks whose first line is `-- name.hl` are written
//! to disk first so `@include` between them resolves.

use std::path::Path;

use htmlang::diagnostic::code;
use htmlang::parser;

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

/// Whether a file holds only definitions (a library, such as a layout):
/// every line that isn't indented is a `@let`, a comment, or the end of a
/// list that a `@let` opened.
fn only_definitions(source: &str) -> bool {
    source
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with([' ', '\t']))
        .all(|l| l.starts_with("@let ") || l.starts_with("--") || l.starts_with(']'))
}

/// Every diagnostic makes an example wrong: the docs show pages that
/// compile cleanly. The one exception is a file of definitions only, whose
/// definitions are used by the files that include it.
fn problems(source: &str, result: &parser::ParseResult) -> Vec<String> {
    let library = only_definitions(source);
    result
        .diagnostics
        .iter()
        .filter(|d| {
            let unused = [
                code::UNUSED_VARIABLE,
                code::UNUSED_BUNDLE,
                code::UNUSED_FUNCTION,
            ]
            .contains(&d.code);
            !(library && unused)
        })
        .map(|d| format!("line {}: {} [{}]", d.line, d.message, d.code))
        .collect()
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
        let problems = problems(block, &parser::parse_with_base(block, Some(&dir)));
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

#[test]
fn examples_compile() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "hl") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let problems = problems(&source, &parser::parse_with_base(&source, Some(&dir)));
        if !problems.is_empty() {
            failures.push(format!("{}\n=> {}", path.display(), problems.join("\n=> ")));
        }
    }
    assert!(
        failures.is_empty(),
        "examples don't compile cleanly:\n\n{}",
        failures.join("\n\n")
    );
}
