use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let result = htmlang_core::parser::parse(source);

    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == htmlang_core::parser::Severity::Error)
        .map(|d| {
            let loc = if let Some(col) = d.column {
                format!("{}:{}", d.line, col + 1)
            } else {
                d.line.to_string()
            };
            format!("Line {}: {}", loc, d.message)
        })
        .collect();

    if !errors.is_empty() {
        return format!(
            "<!DOCTYPE html><html><body style=\"font-family:ui-monospace,monospace;color:#e94560;padding:20px;background:#1a1a2e\">\
            <h3 style=\"margin:0 0 12px\">Compilation Errors</h3><pre style=\"white-space:pre-wrap\">{}</pre></body></html>",
            errors.join("\n")
        );
    }

    htmlang_core::codegen::generate(&result.document)
}
