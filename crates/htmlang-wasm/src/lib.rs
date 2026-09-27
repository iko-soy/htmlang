use htmlang_core::parser::{Diagnostic, Severity};
use wasm_bindgen::prelude::*;

/// One diagnostic on one line: `warning[unknown-attribute] line 3:5: ...`.
fn format_diagnostic(d: &Diagnostic) -> String {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Help => "help",
    };
    let at = match d.column {
        Some(column) => format!("{}:{}", d.line, column + 1),
        None => d.line.to_string(),
    };
    format!("{}[{}] line {}: {}", severity, d.code, at, d.message)
}

/// Every diagnostic, errors first, one per line.
fn report(diagnostics: &[Diagnostic]) -> Vec<String> {
    let errors = diagnostics.iter().filter(|d| d.severity == Severity::Error);
    let others = diagnostics.iter().filter(|d| d.severity != Severity::Error);
    errors.chain(others).map(format_diagnostic).collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The page for `source`, or, when it has errors, a page that lists them
/// (and the warnings after them).
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let result = htmlang_core::parser::parse(source);
    if result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error)
    {
        return format!(
            "<!DOCTYPE html><html><body style=\"font-family:ui-monospace,monospace;color:#e94560;padding:20px;background:#1a1a2e\">\
            <h3 style=\"margin:0 0 12px\">Compilation Errors</h3><pre style=\"white-space:pre-wrap\">{}</pre></body></html>",
            html_escape(&report(&result.diagnostics).join("\n"))
        );
    }
    htmlang_core::codegen::generate(&result.document)
}

/// Everything the compiler reports about `source`, errors first, one
/// diagnostic per line (empty when there is nothing to report). The
/// playground shows it next to the page, so a warning is seen even when the
/// page compiles.
#[wasm_bindgen]
pub fn diagnostics(source: &str) -> String {
    report(&htmlang_core::parser::parse(source).diagnostics).join("\n")
}

#[cfg(test)]
mod tests {
    #[test]
    fn warnings_are_reported_with_errors() {
        let report = super::diagnostics("@el [text-grow per-line, paddin 4]\n  $nope");
        let lines: Vec<&str> = report.lines().collect();
        assert!(
            lines[0].starts_with("error[undefined-variable] line 2:3"),
            "{}",
            report
        );
        assert!(
            lines[1].starts_with("warning[unknown-attribute] line 1:6"),
            "{}",
            report
        );
        let page = super::compile("@el [text-grow per-line, paddin 4]\n  $nope");
        assert!(
            page.contains("Compilation Errors") && page.contains("warning["),
            "{}",
            page
        );
    }

    #[test]
    fn a_page_with_warnings_still_compiles() {
        let page = super::compile("@el [text-grow per-line] Hi");
        assert!(page.contains("text-grow:per-line"), "{}", page);
        assert!(super::diagnostics("@el [text-grow per-line] Hi").starts_with("warning["));
        assert_eq!(super::diagnostics("@el Hi"), "");
    }
}
