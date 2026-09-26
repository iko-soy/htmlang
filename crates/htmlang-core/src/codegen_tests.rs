#[cfg(test)]
mod tests {
    use crate::codegen::{generate, generate_dev, generate_partial, short_class_name};
    use crate::parser::parse;

    fn compile(src: &str) -> String {
        let r = parse(src);
        assert!(
            r.diagnostics
                .iter()
                .all(|d| d.severity != crate::parser::Severity::Error),
            "unexpected parser error for src {src:?}: {:?}",
            r.diagnostics
        );
        generate(&r.document)
    }

    #[test]
    fn non_empty_document_emits_doctype() {
        let out = compile("@page Hello\n@text hello\n");
        assert!(
            out.to_lowercase().contains("<!doctype html>"),
            "missing doctype: {out}"
        );
    }

    #[test]
    fn text_content_is_html_escaped() {
        let out = compile("@text <script>alert(1)</script>\n");
        assert!(
            !out.contains("<script>alert(1)</script>"),
            "raw script should be escaped: {out}"
        );
        assert!(
            out.contains("&lt;script&gt;"),
            "escaped entities missing: {out}"
        );
    }

    #[test]
    fn partial_output_omits_doctype_and_html_wrapper() {
        let r = parse("@text hi\n");
        let out = generate_partial(&r.document);
        assert!(!out.to_lowercase().contains("<!doctype html>"));
        assert!(!out.to_lowercase().contains("<html"));
    }

    #[test]
    fn dev_mode_is_deterministic_across_runs() {
        let src = "@row [spacing 10]\n  @text [font-weight bold] a\n  @text [font-style italic] b\n";
        let r1 = parse(src);
        let r2 = parse(src);
        assert_eq!(generate_dev(&r1.document), generate_dev(&r2.document));
    }

    #[test]
    fn short_class_name_is_stable() {
        assert_eq!(short_class_name(0), "a");
        assert_eq!(short_class_name(25), "z");
        // Higher indexes must produce unique, stable names.
        let names: std::collections::HashSet<_> = (0..50_000).map(short_class_name).collect();
        assert_eq!(names.len(), 50_000, "short_class_name collisions");
    }

    #[test]
    fn page_title_appears_in_head() {
        let out = compile("@page Welcome\n");
        assert!(
            out.contains("<title>Welcome</title>"),
            "title missing in:\n{out}"
        );
    }

    #[test]
    fn same_styles_share_one_class() {
        // Both elements request padding:10 + background red. The collector must
        // dedupe them into a single generated class rather than emitting two.
        let src =
            "@row\n  @el [padding 10, background red] a\n  @el [padding 10, background red] b\n";
        let out = compile(src);
        // Count occurrences of a `.X{` CSS class declaration that contains both
        // padding and the red color. With dedup, the rule body should appear at
        // most once in the <style> block.
        let mut count = 0;
        let needle = "padding:10px;background:red";
        let mut rest = out.as_str();
        while let Some(pos) = rest.find(needle) {
            count += 1;
            rest = &rest[pos + needle.len()..];
        }
        assert!(
            count <= 1,
            "duplicate CSS rule emitted ({count} times) in:\n{out}"
        );
    }
}
