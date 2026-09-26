//! Compile every htmlang example in the docs and in `examples/`, so they
//! can't drift from the language. A fenced block without a language tag is htmlang unless it is a
//! shell session (lines starting with `htmlang ` / `cargo `) or the syntax
//! template in DESIGN.md. Blocks whose first line is `-- name.hl` are written
//! to disk first so `@include` between them resolves.

use std::path::Path;

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

/// Every diagnostic makes an example wrong: the docs show pages that
/// compile cleanly. (A file of definitions only, such as a layout, is a
/// library: the compiler doesn't report its definitions unused.)
fn problems(result: &parser::ParseResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
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
        let problems = problems(&parser::parse_with_base(block, Some(&dir)));
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
        let problems = problems(&parser::parse_with_base(&source, Some(&dir)));
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

/// The table of layouts under DESIGN.md's Elements lists every element
/// once, with the layout the compiler gives it.
#[test]
fn design_md_gives_every_element_its_layout() {
    use htmlang::ast::{ElementKind, Layout};
    let design =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("DESIGN.md")).unwrap();
    let start = design
        .find("Every element has one [layout]")
        .expect("DESIGN.md has the table of layouts");
    let mut listed: Vec<(String, &str)> = Vec::new();
    for line in design[start..].lines().skip(4) {
        let Some(row) = line.strip_prefix("| ") else {
            break;
        };
        let (layout, names) = row.split_once(" | ").unwrap();
        for name in names.trim_end_matches(" |").split(", ") {
            let name = name.trim_matches('`').trim_start_matches('@');
            listed.push((name.to_string(), layout));
        }
    }
    let placeholders = ["fragment", "children", "slot"];
    for name in ElementKind::all_names().filter(|n| !placeholders.contains(n)) {
        let rows: Vec<&str> = listed
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, l)| *l)
            .collect();
        let layout: Layout = ElementKind::from_name(name).unwrap().layout();
        assert_eq!(
            rows,
            [layout.name()],
            "@{} in DESIGN.md's table of layouts",
            name
        );
    }
    for (name, _) in &listed {
        assert!(
            ElementKind::from_name(name).is_some(),
            "@{} isn't an element",
            name
        );
    }
}
