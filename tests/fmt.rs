//! The formatter over every htmlang file in the repository: formatting is
//! idempotent, keeps every comment, and never changes what a file compiles
//! to.

use std::path::{Path, PathBuf};

use htmlang::{codegen, fmt, parser};

fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["examples", "tests/snapshots"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "hl") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn compile(source: &str, dir: &Path) -> String {
    codegen::generate(&parser::parse_with_base(source, Some(dir)).document)
}

#[test]
fn formatting_is_idempotent_and_keeps_the_output() {
    for path in sources() {
        let source = std::fs::read_to_string(&path).unwrap();
        let dir = path.parent().unwrap();
        let once = fmt::format(&source);
        assert_eq!(
            fmt::format(&once),
            once,
            "not idempotent: {}",
            path.display()
        );
        assert_eq!(
            compile(&once, dir),
            compile(&source, dir),
            "formatting changed the output of {}",
            path.display()
        );
        let comments = |s: &str| {
            s.lines()
                .filter(|l| l.trim_start().starts_with("--"))
                .map(str::trim)
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(comments(&once), comments(&source), "{}", path.display());
    }
}
