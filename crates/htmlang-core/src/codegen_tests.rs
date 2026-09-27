#[cfg(test)]
mod tests {
    use crate::codegen::{
        CodegenOptions, generate, generate_dev, generate_partial, generate_with_classes,
    };
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
        let src =
            "@row [spacing 10]\n  @text [font-weight bold] a\n  @text [font-style italic] b\n";
        let r1 = parse(src);
        let r2 = parse(src);
        assert_eq!(generate_dev(&r1.document), generate_dev(&r2.document));
    }

    /// The classes of `src`, compiled as a fragment
    fn classes(src: &str) -> Vec<crate::codegen::Class> {
        let options = CodegenOptions {
            partial: true,
            ..Default::default()
        };
        generate_with_classes(&parse(src).document, &options).1
    }

    #[test]
    fn a_style_has_one_name_in_every_file() {
        // A fragment's classes mean in a page what they mean in the
        // fragment, whatever else either holds
        let page = classes("@el [padding 10, background red] a\n@el [gap 4] b\n");
        let fragment = classes("@row [margin 2] x\n@el [padding 10, background red] y\n");
        assert_eq!(page[0], fragment[1]);
        assert_ne!(page[1].style, fragment[0].style);
        assert_ne!(page[1].name, fragment[0].name);
        for class in page.iter().chain(&fragment) {
            let digits = class.name.strip_prefix("hl-").expect("hl-");
            assert_eq!(digits.len(), crate::codegen::CLASS_DIGITS, "{}", class.name);
            assert!(
                digits
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()),
                "{}",
                class.name
            );
        }
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
