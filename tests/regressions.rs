//! Regression tests for bugs found in review. Each test names the behavior
//! that used to be wrong.

use std::path::PathBuf;

use htmlang::codegen;
use htmlang::parser::{self, Severity};

fn compile(input: &str) -> String {
    let result = parser::parse(input);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "unexpected parse errors: {:?}",
        result.diagnostics
    );
    codegen::generate(&result.document)
}

/// A fresh scratch directory for tests that need files on disk.
fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("htmlang_regress_{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile_in(dir: &std::path::Path, input: &str) -> String {
    let result = parser::parse_with_base(input, Some(dir));
    codegen::generate(&result.document)
}

// --- Panics ---

#[test]
fn truncate_filter_handles_multibyte_text() {
    let out = compile("@let s héllo wörld\n@text ${truncate($s, 2)}");
    assert!(out.contains("hé..."), "{}", out);
}

#[test]
fn length_filter_counts_characters() {
    let out = compile("@let s héllo\n@text ${length($s)}");
    assert!(out.contains(">5<"), "{}", out);
}

#[test]
fn non_ascii_hex_color_does_not_panic() {
    compile("@el [background #é1, color #fff] hi");
}

#[test]
fn truncated_json_data_does_not_panic() {
    let dir = scratch_dir("json_trunc");
    std::fs::write(dir.join("d.json"), "{\"a\": 1,").unwrap();
    compile_in(&dir, "@data $d d.json\n@text ok");
    std::fs::write(dir.join("d.json"), "{").unwrap();
    compile_in(&dir, "@data $d d.json\n@text ok");
}

#[test]
fn json_unicode_escapes_are_decoded() {
    let dir = scratch_dir("json_unicode");
    std::fs::write(
        dir.join("d.json"),
        r#"{"name": "Caf\u00e9", "emoji": "\ud83d\ude00"}"#,
    )
    .unwrap();
    let out = compile_in(&dir, "@data $d d.json\n@text $d.name $d.emoji");
    assert!(out.contains("Café 😀"), "{}", out);
}

#[test]
fn range_at_integer_limit_terminates() {
    let out = compile("@each $i in 9223372036854775806..9223372036854775807\n  @text $i");
    assert!(out.contains("9223372036854775807"), "{}", out);
    let out = compile("@each $i in -9223372036854775807..-9223372036854775808\n  @text $i");
    assert!(out.contains("-9223372036854775808"), "{}", out);
}

// --- CSS / codegen ---

#[test]
fn reset_css_is_layered_below_generated_rules() {
    let out = compile("@page T\n@link [color red, text-decoration underline] /x Home");
    assert!(
        out.contains("@layer hl-reset,hl-kind,hl-inside,htmlang;@layer hl-reset{"),
        "reset must be layered so class rules can override it: {}",
        out
    );
}

#[test]
fn px_values_are_not_doubled() {
    let out = compile("@el [padding 10px, margin 0 auto, max-width none, filter blur(4px)] x");
    assert!(out.contains("padding:10px"), "{}", out);
    assert!(out.contains("margin:0 auto"), "{}", out);
    assert!(out.contains("max-width:none"), "{}", out);
    assert!(!out.contains("pxpx"), "{}", out);
}

#[test]
fn minify_keeps_significant_spaces() {
    let result =
        parser::parse("@page T\n@paragraph\n  Built with {@text [font-weight bold] htmlang}.");
    let out = codegen::generate_minified(&result.document);
    assert!(out.contains("Built with <span"), "{}", out);
}

#[test]
fn css_vars_are_not_substituted_into_media_queries() {
    let out = compile("@let --bp 768px\n@el [md:padding 20] x");
    assert!(!out.contains("min-width:var("), "{}", out);
    assert!(out.contains("768px"), "{}", out);
}

#[test]
fn partial_output_includes_style_and_keyframes() {
    let result = parser::parse(
        "@style\n  @scope (.card) { .title { font-weight: bold; } }\n  @keyframes k { from { opacity: 0 } }\n@text hi",
    );
    let out = codegen::generate_partial(&result.document);
    assert!(out.contains(".title"), "{}", out);
    assert!(out.contains("@keyframes k"), "{}", out);
}

#[test]
fn script_body_is_verbatim() {
    let out = compile("@script\n  let a = 1\n  let b = `${a}`\n  if (a) { go(); }");
    assert!(
        out.contains("<script>let a = 1\nlet b = `${a}`\nif (a) { go(); }</script>"),
        "{}",
        out
    );
}

#[test]
fn image_with_css_width_gets_no_intrinsic_height() {
    let dir = scratch_dir("image_dims");
    let svg = dir.join("big.svg");
    std::fs::write(
        &svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="1000"></svg>"#,
    )
    .unwrap();
    let out = compile(&format!("@image [width 200, alt=x] {}", svg.display()));
    assert!(!out.contains("height=\"1000\""), "{}", out);
}

#[test]
fn source_map_uses_real_output_lines() {
    let html =
        "<!DOCTYPE html>\n<html>\n<div data-hl-line=\"3\">\n<span data-hl-line=\"4\">x</span>\n";
    let map = codegen::source_map_for_html(html, "a.hl");
    // Lines 1-2 have no mapping; line 3 maps to source line 3 (0-based 2 =
    // VLQ "E"), line 4 to source line 4 (delta 1 = "C").
    assert!(map.contains("\"mappings\":\";;AAEA;AACA\""), "{}", map);
}

// --- Parser logic ---

#[test]
fn bracket_in_text_does_not_join_lines() {
    let out =
        compile("@el\n  Use [ to open a list\n  @text [font-weight bold] Second\n  @text Third");
    assert!(out.contains("Use [ to open a list"), "{}", out);
    assert!(out.contains(">Second<"), "{}", out);
    assert!(out.contains(">Third<"), "{}", out);
}

#[test]
fn multiline_attributes_still_join() {
    let out = compile("@el [\n  padding 20,\n  background white\n]\n  Content");
    assert!(out.contains("padding:20px"), "{}", out);
    assert!(out.contains("background:white"), "{}", out);
}

#[test]
fn multi_line_content_is_a_function() {
    let out = compile(
        "@let @intro\n  First line\n  Second {@text [font-weight bold] line}\n@paragraph\n  @intro\n",
    );
    assert!(out.contains("First line"), "{}", out);
    assert!(out.contains(">line</span>"), "{}", out);
}

#[test]
fn each_else_does_not_leak_variables() {
    // Outside the @else, `$leaked` is undefined: an error, not "yes"
    let result = parser::parse(
        "@let empty \"\"\n@each $x in $empty\n  @text $x\n@else\n  @let leaked yes\n  @text none\n@text v=$leaked",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "undefined-variable" && d.line == 7),
        "{:?}",
        result.diagnostics
    );
    assert!(!codegen::generate(&result.document).contains("v=yes"));
}

#[test]
fn named_argument_is_not_used_positionally_for_another_param() {
    let out = compile(
        "@let @card [title none, variant primary]\n  @text t=$title v=$variant\n@card [variant danger]",
    );
    assert!(out.contains("v=danger"), "{}", out);
    assert!(out.contains("t=none"), "{}", out);
}

#[test]
fn svg_width_does_not_touch_stroke_width() {
    let dir = scratch_dir("svg_attr");
    std::fs::write(
        dir.join("icon.svg"),
        r#"<svg viewBox="0 0 24 24"><path stroke-width="2" d="M0 0"/></svg>"#,
    )
    .unwrap();
    let out = compile_in(&dir, "@image [inline, width 48] icon.svg");
    assert!(out.contains("stroke-width=\"2\""), "{}", out);
    assert!(out.contains("style=\"width:48px\""), "{}", out);
}

#[test]
fn markdown_keeps_paragraphs_rules_and_ordered_lists() {
    let out = compile(
        "@markdown\n  First para.\n\n  Second para.\n\n  ---\n\n  1. one\n  2. two\n\n  ```\n  if x:\n      y\n  ```",
    );
    assert!(out.contains("<p>First para.</p>"), "{}", out);
    assert!(out.contains("<p>Second para.</p>"), "{}", out);
    assert!(out.contains("<hr>"), "{}", out);
    assert!(
        out.contains("<ol>\n<li>one</li>\n<li>two</li>\n</ol>"),
        "{}",
        out
    );
    assert!(out.contains("if x:\n    y"), "{}", out);
}

#[test]
fn let_values_are_not_evaluated_as_arithmetic() {
    // Quoted text would keep its quotes in CSS: a value is written bare
    let out = compile("@let span 1 / -1\n@el [grid-area $span] x");
    assert!(out.contains("grid-area:1 / -1"), "{}", out);
}

#[test]
fn font_stack_keeps_its_commas_escaped() {
    let out = compile(r#"@el [font-family "Open Sans"\, Inter\, sans-serif, font-weight bold] x"#);
    assert!(
        out.contains(r#"font-family:"Open Sans", Inter, sans-serif"#),
        "{}",
        out
    );
    assert!(out.contains("font-weight:bold"), "{}", out);
}

#[test]
fn quoted_font_family_is_one_family_and_warns() {
    let result = parser::parse("@el [font-family \"Inter, sans-serif\"] x");
    let warning = result
        .diagnostics
        .iter()
        .find(|d| d.code == "invalid-value")
        .expect("a warning about the quoted font stack");
    assert!(
        warning.message.contains(r"font-family Inter\, sans-serif"),
        "{}",
        warning.message
    );
    assert_eq!(warning.suggestion.as_deref(), Some(r"Inter\, sans-serif"));
    // CSS means what CSS says: one family
    let out = codegen::generate(&result.document);
    assert!(
        out.contains(r#"font-family:"Inter, sans-serif""#),
        "{}",
        out
    );
}

#[test]
fn diagnostic_column_includes_indentation() {
    let result = parser::parse("@let name x\n@el\n    Hello $nmae");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("nmae"))
        .expect("undefined-variable warning");
    assert_eq!(d.column, Some(10));
}

#[test]
fn variables_used_in_loops_conditions_and_text_are_not_unused() {
    for src in [
        "@let items a, b\n@each $x in $items\n  @text $x",
        "@let on true\n@if $on\n  @text y",
        "@let n 3\n@each $i in 1..$n\n  @text $i",
        "@let score \"10 - 2\"\n@text Score: $score",
        "@let who World\nHello $who",
    ] {
        let result = parser::parse(src);
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("unused variable")),
            "false unused warning for {:?}: {:?}",
            src,
            result.diagnostics
        );
    }
}

#[test]
fn container_arguments_are_rendered_as_text() {
    for src in [
        "@el [padding 8] Hello",
        "@paragraph Hello",
        "@row Hello",
        "@section Hello",
        "@nav [id=x] Hello",
    ] {
        let out = compile(src);
        assert!(out.contains("Hello"), "{:?} dropped its text: {}", src, out);
    }
    let out = compile("@paragraph Read {@link /more more}");
    assert!(
        out.contains("<a href=\"/more\" class=\"hl-b\">more</a>"),
        "{}",
        out
    );
}

#[test]
fn html_attributes_use_equals_and_are_all_emitted() {
    let out = compile(
        "@link [target=_blank, rel=me] /x Home\n@iframe [sandbox, allow=camera] https://e.com",
    );
    assert!(out.contains("target=\"_blank\""), "{}", out);
    assert!(out.contains("rel=\"me\""), "{}", out);
    assert!(out.contains(" sandbox"), "{}", out);
    assert!(out.contains("allow=\"camera\""), "{}", out);
}

#[test]
fn html_attribute_and_style_with_the_same_name_are_distinct() {
    let out = compile("@select [size=4, font-size 20]\n  @option A");
    assert!(out.contains("size=\"4\""), "{}", out);
    assert!(out.contains("font-size:20px"), "{}", out);
    let out = compile("@image [width=800, width 200, alt=x] a.png");
    assert!(out.contains("width=\"800\""), "{}", out);
    assert!(out.contains("width:200px"), "{}", out);
}

#[test]
fn space_form_html_attribute_points_to_equals() {
    let result = parser::parse("@input [type email]");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("`type=email`")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn conditions_do_not_reparse_variable_values() {
    // The value contains `==`; the condition compares it as a whole.
    let out = compile("@let v \"a == b\"\n@if $v == \"a == b\"\n  @text yes");
    assert!(out.contains(">yes<"), "{}", out);
    let out = compile("@let n 5\n@let none \"\"\n@if $n > 2 and not $none\n  @text big");
    assert!(out.contains(">big<"), "{}", out);
}

#[test]
fn computed_values_need_equals_and_have_precedence() {
    let out = compile("@let x = 2 + 3 * 4\n@text $x");
    assert!(out.contains(">14<"), "{}", out);
    // Without `=`, a value is literal text: `1 / 3` stays as written.
    let out = compile("@let area 1 / 3\n@el [grid-row $area] x");
    assert!(out.contains("grid-row:1 / 3"), "{}", out);
}

#[test]
fn invalid_expressions_are_errors() {
    let result = parser::parse("@let x = dark * 2\n@if (1\n  @text y");
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("invalid expression"))
        .collect();
    assert_eq!(errors.len(), 2, "{:?}", result.diagnostics);
}

#[test]
fn htmlang_attributes_sharing_html_names_do_not_warn() {
    let result = parser::parse("@row [font-size 18, wrap, display none]\n  @text x");
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn data_glob_loads_a_list_of_records() {
    let dir = scratch_dir("data_glob");
    std::fs::create_dir_all(dir.join("posts")).unwrap();
    std::fs::write(dir.join("posts/a.json"), r#"{"title": "First"}"#).unwrap();
    std::fs::write(dir.join("posts/b.json"), r#"{"title": "Second"}"#).unwrap();
    let out = compile_in(
        &dir,
        "@data $posts posts/*.json\n@text ${length($posts)} posts\n@each $p, $i in $posts\n  @text $i $p.title ($p.file)",
    );
    assert!(out.contains("2 posts"), "{}", out);
    assert!(
        out.contains(">0 First (a)<") && out.contains(">1 Second (b)<"),
        "{}",
        out
    );
}

#[test]
fn lint_adds_only_checks_the_compiler_does_not_already_make() {
    let result = parser::parse("@row\n@button Go\n@image a.png");
    let lint = parser::lint(&result.document.nodes);
    let messages: Vec<&str> = result
        .diagnostics
        .iter()
        .chain(&lint)
        .map(|d| d.message.as_str())
        .collect();
    // Each problem is reported once, whichever pass finds it.
    assert_eq!(
        messages.iter().filter(|m| m.contains("'alt'")).count(),
        1,
        "{:?}",
        messages
    );
    assert!(
        messages.iter().any(|m| m.contains("empty container")),
        "{:?}",
        messages
    );
    assert!(
        messages.iter().any(|m| m.contains("'type'")),
        "{:?}",
        messages
    );
}

#[test]
fn inline_svg_resolves_from_the_page_and_keeps_attributes() {
    let dir = scratch_dir("inline_svg");
    std::fs::create_dir_all(dir.join("icons")).unwrap();
    std::fs::write(
        dir.join("icons/a.svg"),
        r#"<svg viewBox="0 0 24 24"></svg>"#,
    )
    .unwrap();
    let out = compile_in(&dir, "@image [inline, width 24, class=icon] icons/a.svg");
    assert!(
        out.contains(r#"<svg viewBox="0 0 24 24" class="icon" style="width:24px">"#),
        "{}",
        out
    );
}

#[test]
fn inline_element_attributes_can_continue_on_the_next_line() {
    let result = htmlang::parser::parse(
        "@paragraph\n  Press {@kbd [\n    padding 2 6, border-radius 4\n  ] Ctrl+K} to search.\n",
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let html = htmlang::codegen::generate(&result.document);
    assert!(html.contains(">Ctrl+K</kbd> to search."), "{}", html);
}

#[test]
fn lines_under_text_are_its_siblings() {
    let html = htmlang::codegen::generate(
        &htmlang::parser::parse("@el\n  Some text\n    more text\n").document,
    );
    assert!(
        html.contains("<span>Some text</span><span>more text</span>"),
        "{}",
        html
    );
}

#[test]
fn style_keeps_css_custom_properties() {
    let out = compile("@style\n  :root {\n    --brand: red;\n  }\n@el [color var(--brand)] x\n");
    assert!(out.contains("--brand: red;"), "{}", out);
}

#[test]
fn raw_takes_an_indented_body() {
    let out = compile("@raw\n  <div>\n\n    <p>x</p>\n  </div>\n@raw <hr>\n");
    assert!(out.contains("<div>\n\n  <p>x</p>\n</div><hr>"), "{}", out);
}

#[test]
fn border_shorthands_get_px() {
    let out = compile("@el [border 1 solid red, border-radius 4 4 0 0] x");
    assert!(out.contains("border:1px solid red"), "{}", out);
    assert!(out.contains("border-radius:4px 4px 0 0"), "{}", out);
}

#[test]
fn backslash_escapes_in_text() {
    let out = compile(
        "@let price 9\n\\@user says\n\\-- not a comment\n@text \\$price is $price, \\{@b} \\\\\n@paragraph\n  Now {@text \\$5} only\n",
    );
    assert!(out.contains("@user says"), "{}", out);
    assert!(out.contains("-- not a comment"), "{}", out);
    assert!(out.contains("$price is 9, {@b} \\<"), "{}", out);
    assert!(out.contains(">$5</span> only"), "{}", out);
}

#[test]
fn links_and_images_are_output_as_written() {
    let out = compile("@main\n  @link https://example.com Out\n  @image [alt=a] a.png\n");
    assert!(!out.contains("target="), "{}", out);
    assert!(!out.contains("rel="), "{}", out);
    assert!(!out.contains("preload"), "{}", out);
    assert!(!out.contains("fetchpriority"), "{}", out);
    assert!(!out.contains("Skip to content"), "{}", out);
    assert!(!out.contains("hl-main"), "{}", out);
}

#[test]
fn css_values_pass_through() {
    let out = compile("@el [line-height 24, before:content \"→ \", after:content attr(title)] x");
    assert!(out.contains("line-height:24;"), "{}", out);
    assert!(out.contains("content:\"→ \""), "{}", out);
    assert!(out.contains("content:attr(title)"), "{}", out);
}

#[test]
fn page_attributes_are_checked_like_any_element_s() {
    // `canonical` belongs in a `@head` link: on @page it is an unknown CSS
    // property (for <body>), reported rather than dropped. Right after an
    // HTML attribute, it may be the rest of that one's value too.
    for (source, code) in [
        (
            "@page [canonical https://x.dev, lang=en] T\n",
            "unknown-attribute",
        ),
        (
            "@page [lang=en, canonical https://x.dev] T\n",
            "split-value",
        ),
    ] {
        let result = parser::parse(source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == code && d.message.contains("'canonical'")),
            "{:?}",
            result.diagnostics
        );
    }
}

// --- One grammar: the syntax tree and the checks built on it ---

fn codes(input: &str) -> Vec<(usize, &'static str, Severity)> {
    parser::parse(input)
        .diagnostics
        .iter()
        .map(|d| (d.line, d.code, d.severity))
        .collect()
}

fn has_code(input: &str, line: usize, code: &str) -> bool {
    codes(input)
        .iter()
        .any(|(l, c, _)| *l == line && *c == code)
}

#[test]
fn untaken_branches_are_checked() {
    let src = "@let on true\n@if $on\n  @text ok\n@else\n  @nosuch [paddin 4]\n  @el [paddin 4]\n";
    let result = parser::parse(src);
    let nosuch = result
        .diagnostics
        .iter()
        .find(|d| d.message.contains("unknown element @nosuch"))
        .expect("unknown element in an untaken @else");
    assert_eq!((nosuch.line, nosuch.severity), (5, Severity::Error));
    assert_eq!(nosuch.code, "unknown-element");
    assert!(has_code(src, 6, "unknown-attribute"), "{:?}", codes(src));
}

#[test]
fn uncalled_functions_and_empty_loops_are_checked() {
    // A function body can call a function defined later in the file
    let src = "@let @a\n  @later\n  @nosuch\n@let @later\n  @text hi\n@later\n";
    let found = codes(src);
    assert!(has_code(src, 3, "unknown-element"), "{:?}", found);
    assert!(!found.iter().any(|(l, _, _)| *l == 2), "{:?}", found);
    // A loop over an empty list, and its `@else` when the list isn't empty
    let src = "@data $e []\n@each $x in $e\n  @bogus\n@each $y in 1..2\n  $y\n@else\n  @nope\n";
    assert!(has_code(src, 3, "unknown-element"), "{:?}", codes(src));
    assert!(has_code(src, 7, "unknown-element"), "{:?}", codes(src));
    // An inline element in code that doesn't run is an error, as it is
    // when it runs (a misspelled element is never a warning)
    let src = "@if false\n  Text with {@nosuch x}\n";
    assert!(
        codes(src).contains(&(2, "unknown-element", Severity::Error)),
        "{:?}",
        codes(src)
    );
    let src = "@paragraph\n  Text with {@nosuch x}\n";
    assert!(
        codes(src).contains(&(2, "unknown-element", Severity::Error)),
        "{:?}",
        codes(src)
    );
}

#[test]
fn names_used_only_in_code_that_does_not_run_are_used() {
    let src = "@let color red\n@let @card\n  @el hi\n@if false\n  @card [color $color]\n";
    let found = codes(src);
    assert!(
        !found
            .iter()
            .any(|(_, c, _)| *c == "unused-variable" || *c == "unused-function"),
        "{:?}",
        found
    );
}

#[test]
fn unevaluated_names_include_functions_from_included_files() {
    let dir = scratch_dir("static_include");
    std::fs::write(dir.join("lib.hl"), "@let @helper\n  @el\n    @children\n").unwrap();
    std::fs::write(dir.join("bad.hl"), "@if false\n  @oops\n").unwrap();
    let result = parser::parse_with_base(
        "@let @page-body\n  @helper x\n@include lib.hl\n@include bad.hl\n",
        Some(&dir),
    );
    let unknown: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "unknown-element")
        .collect();
    assert_eq!(unknown.len(), 1, "{:?}", result.diagnostics);
    assert!(unknown[0].message.contains("@oops"), "{:?}", unknown);
    assert!(unknown[0].message.contains("in bad.hl"), "{:?}", unknown);
}

#[test]
fn a_bracket_in_text_does_not_open_a_list() {
    let out = compile("@text Use [ to open\n@text [color red] next\n");
    assert!(out.contains("Use [ to open"), "{}", out);
    assert!(out.contains(">next<"), "{}", out);
    let out = compile("@link /a[x,y] T\n");
    assert!(out.contains("href=\"/a[x,y]\""), "{}", out);
}

#[test]
fn a_chain_is_only_between_element_heads() {
    let out = compile("@text Ask me > @support\n");
    assert!(
        out.contains("Ask me &gt; @support") || out.contains("Ask me > @support"),
        "{}",
        out
    );
}

#[test]
fn indented_lines_under_a_directive_without_a_body_are_errors() {
    for src in [
        "@page Home\n  @text x\n",
        "@meta description A site\n  @text x\n",
        "@include x.hl\n  @text x\n",
        "@data $d [1]\n  @text x\n",
        "@let x = 1\n  @text x\n",
        "@let b [padding 4]\n  @text x\n",
    ] {
        assert!(
            has_code(src, 2, "unexpected-body"),
            "{}: {:?}",
            src,
            codes(src)
        );
    }
    // Comments and blank lines may be indented anywhere
    assert!(codes("@page Home\n  -- note\n\n@text x\n").is_empty());
}

#[test]
fn verbatim_directives_take_a_line_or_a_body() {
    let src = "@raw <hr>\n  <br>\n";
    assert!(has_code(src, 1, "unexpected-body"), "{:?}", codes(src));
    let src = "@style .a { color: red; }\n  .b {}\n";
    assert!(has_code(src, 1, "unexpected-body"), "{:?}", codes(src));
    // The line is a one-line body: `@style X` was "unknown element @style,
    // did you mean @style?", `@head X` was dropped
    let src =
        "@page T\n@style .a { color: red; }\n@head <meta name=\"x\" content=\"y\">\n@text z\n";
    assert!(codes(src).is_empty(), "{:?}", codes(src));
    let out = compile(src);
    assert!(out.contains(".a { color: red; }</style>"), "{}", out);
    assert!(out.contains("<meta name=\"x\" content=\"y\">"), "{}", out);
    // Attributes on a verbatim directive were text (`@raw [id=x]` wrote
    // "[id=x]"), a file name (`@markdown [padding 8]`) or dropped
    // (`@head [id=x]`): now an error
    for src in [
        "@raw [id=x] <hr>\n",
        "@markdown [padding 8]\n",
        "@head [id=x]\n  <meta name=x>\n",
        "@style [media=print]\n  a {}\n",
    ] {
        assert!(
            has_code(src, 1, "unexpected-argument"),
            "{}: {:?}",
            src,
            codes(src)
        );
    }
}

#[test]
fn multi_line_inline_json_object() {
    let out = compile(
        "@data $site {\n  \"name\": \"Demo\",\n  \"tags\": [\"a\", \"b\"]\n}\n@text $site.name\n",
    );
    assert!(out.contains(">Demo<"), "{}", out);
}

#[test]
fn script_attribute_list_may_span_lines() {
    let out = compile("@script [\n  type=module,\n  defer\n]\n  const a = [1, 2];\n");
    assert!(out.contains("<script type=\"module\" defer>"), "{}", out);
    assert!(out.contains("const a = [1, 2];"), "{}", out);
}

#[test]
fn every_diagnostic_has_a_known_code() {
    let src = "@page Home\n  @x\n@el [paddin 4, color #zz, opacity 3] > @link /x\n@let unused 1\n@if\n@else\n@each $x\n@text $undefinedd\n@let undefined 1\n@text $undefined\n";
    let result = parser::parse(src);
    assert!(!result.diagnostics.is_empty());
    for d in &result.diagnostics {
        assert!(
            htmlang::diagnostic::code::ALL.contains(&d.code),
            "unknown code {:?} for {}",
            d.code,
            d.message
        );
    }
}

#[test]
fn directives_that_need_their_argument_say_so() {
    for src in ["@include\n", "@meta\n", "@meta description\n"] {
        assert!(
            has_code(src, 1, "missing-argument"),
            "{}: {:?}",
            src,
            codes(src)
        );
    }
    // `@raw` and `@markdown` may take their content from a body instead
    assert!(codes("@raw\n  <hr>\n").is_empty());
}

#[test]
fn a_known_name_in_the_wrong_place_is_not_its_own_suggestion() {
    // Called before its `@let` has run: no "did you mean @later?", and
    // `@later` isn't reported unused
    let result = parser::parse("@let @a\n  @el\n    @later\n@a\n@let @later\n  @text hi\n");
    let unknown = result
        .diagnostics
        .iter()
        .find(|d| d.code == "unknown-element")
        .expect("@later is not defined when @a runs");
    assert!(
        unknown.message.contains("isn't visible here"),
        "{}",
        unknown.message
    );
    assert!(unknown.suggestion.is_none());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == "unused-function"),
        "{:?}",
        result.diagnostics
    );
    // A directive in a chain, and a function inside text called before
    // its `@let` has run
    for (src, says) in [
        ("@el > @if true\n  x\n", "is a directive"),
        ("Call {@f}\n@let @f\n  @el\n", "isn't visible here"),
    ] {
        let d = parser::parse(src)
            .diagnostics
            .into_iter()
            .find(|d| d.code == "unknown-element")
            .unwrap_or_else(|| panic!("{}", src));
        assert!(d.message.contains(says), "{}", d.message);
        assert!(d.suggestion.is_none(), "{:?}", d.suggestion);
    }
}

#[test]
fn syntax_errors_are_reported_in_line_order() {
    let lines: Vec<usize> = codes("@else\n  a\n@each $x\n  b\n@if\n  c\n")
        .into_iter()
        .map(|(line, _, _)| line)
        .collect();
    assert_eq!(lines, [1, 3, 5]);
}

#[test]
fn an_escaped_quote_does_not_split_an_attribute() {
    let out = compile("@el [content \"a\\\", b\", padding 4] x\n");
    assert!(out.contains("content:\"a\\\", b\""), "{}", out);
    assert!(out.contains("padding:4px"), "{}", out);
}

// --- Variables fill the slot they are written in ---

fn codes_of(src: &str) -> Vec<(&'static str, usize)> {
    parser::parse(src)
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.code, d.line))
        .collect()
}

#[test]
fn a_variable_never_makes_an_attribute() {
    // Text in attribute position used to become an HTML attribute
    let src = "@let attr href=/x\n@el [$attr] y";
    assert_eq!(codes_of(src), [("attribute-from-variable", 2)]);
    assert!(!compile_in(&std::env::temp_dir(), src).contains("href"));
    // ... or build an attribute's name
    assert_eq!(
        codes_of("@let key padding\n@el [$key 8] y"),
        [("attribute-from-variable", 2)]
    );
    // A bundle is the way to reuse attributes
    let out = compile("@let attr [href=/x]\n@el [$attr] y");
    assert!(out.contains("href=\"/x\""), "{}", out);
}

#[test]
fn a_value_with_commas_stays_one_value() {
    let out = compile("@let fonts Inter, sans-serif\n@el [font-family $fonts, color red] x");
    assert!(out.contains("font-family:Inter, sans-serif;"), "{}", out);
    assert!(out.contains("color:red"), "{}", out);
}

#[test]
fn a_variable_name_ends_before_a_dot_of_text() {
    let out = compile("@let lang fr\n@text file $lang.json, ${lang}uage");
    assert!(out.contains("file fr.json, fruage"), "{}", out);
    // In a path too
    let dir = scratch_dir("lang_path");
    std::fs::create_dir_all(dir.join("locales")).unwrap();
    std::fs::write(dir.join("locales/fr.json"), r#"{"hello": "Bonjour"}"#).unwrap();
    let result = parser::parse_with_base(
        "@let lang fr\n@data $t locales/$lang.json\n@text $t.hello",
        Some(&dir),
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(codegen::generate(&result.document).contains("Bonjour"));
}

#[test]
fn a_dollar_before_anything_but_a_name_is_text() {
    let out = compile("@text costs $5, $$ and $\n@text [id=a$] x");
    assert!(out.contains("costs $5, $$ and $"), "{}", out);
    assert!(out.contains("id=\"a$\""), "{}", out);
}

#[test]
fn an_undefined_variable_is_an_error_everywhere() {
    for src in [
        "@text Hello $nobody",
        "@el [padding $nobody] x",
        "@let x = $nobody + 1\n@text $x",
        "@if $nobody\n  @text y",
        "@each $x in $nobody\n  @text $x",
        "@page $nobody",
        "@link /$nobody x",
    ] {
        let result = parser::parse(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "undefined-variable" && d.severity == Severity::Error),
            "{}: {:?}",
            src,
            result.diagnostics
        );
    }
    // A typo gets a suggestion and a column
    let result = parser::parse("@let gap 8\n@el [padding $gpa] x");
    let d = &result.diagnostics[0];
    assert_eq!(d.suggestion.as_deref(), Some("gap"));
    assert_eq!(d.column, Some(13));
}

#[test]
fn a_missing_field_of_a_record_is_empty() {
    let out = compile(
        "@data $post {\"title\": \"T\"}\n@text x${post.draft}y ${default($post.tag, none)}\n@if $post.draft\n  @text draft\n@each $t in $post.tags\n  @text $t",
    );
    assert!(out.contains("<span>xy none</span>"), "{}", out);
    assert!(!out.contains("draft"), "{}", out);
}

#[test]
fn data_is_never_read_as_markup_or_variables() {
    let out =
        compile("@let x no\n@data $d {\"t\": \"$x {@b bold}\"}\n@link /a $d.t\n@paragraph $d.t");
    assert!(!out.contains("<b"), "{}", out);
    assert!(!out.contains(">no"), "{}", out);
    assert!(out.contains("$x {@b bold}</a>"), "{}", out);
}

#[test]
fn attribute_names_are_checked_in_code_that_does_not_run() {
    let result = parser::parse("@let @card [t]\n  @el [paddin $t, $k 4] $t\n");
    let codes: Vec<_> = result.diagnostics.iter().map(|d| d.code).collect();
    assert!(codes.contains(&"unknown-attribute"), "{:?}", codes);
    assert!(codes.contains(&"attribute-from-variable"), "{:?}", codes);
}

#[test]
fn a_problem_in_a_loop_is_reported_once() {
    let result = parser::parse("@each $i in 1..3\n  @text $nobody");
    assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
}

#[test]
fn a_column_after_an_escape_points_at_the_variable() {
    let result = parser::parse("@text \\@ \\$ $nobody \\-- $other");
    let columns: Vec<_> = result.diagnostics.iter().map(|d| d.column).collect();
    assert_eq!(columns, [Some(12), Some(24)], "{:?}", result.diagnostics);
}

#[test]
fn an_invalid_expression_in_a_loop_is_reported_once() {
    let result = parser::parse("@each $i in 1..3\n  @if $i +\n    @text x\n  @text ${$i +}");
    let lines: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| (d.code, d.line))
        .collect();
    assert_eq!(
        lines,
        [("invalid-expression", 2), ("invalid-expression", 4)],
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn a_bundle_can_be_written_with_braces() {
    let out = compile("@let card [padding 8]\n@el [${card}] x");
    assert!(out.contains("padding:8px"), "{}", out);
    // A bundle used as a value is reported once, not also as unused
    let result = parser::parse("@let card [padding 8]\n@el [padding $card] x");
    let codes: Vec<_> = result.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, ["undefined-variable"], "{:?}", result.diagnostics);
}

#[test]
fn the_same_problem_in_an_included_file_is_reported_too() {
    let dir = scratch_dir("include_undefined");
    std::fs::write(dir.join("part.hl"), "@text $nobody\n").unwrap();
    let result = parser::parse_with_base("@text $nobody\n@include part.hl", Some(&dir));
    let undefined = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "undefined-variable")
        .count();
    assert_eq!(undefined, 2, "{:?}", result.diagnostics);
}

// --- Commas, quotes and backslashes ---

#[test]
fn escaped_comma_stays_in_the_value() {
    let out = compile(r"@el [transition opacity 0.3s\, transform 0.3s, color red] x");
    assert!(
        out.contains("transition:opacity 0.3s, transform 0.3s;color:red"),
        "{}",
        out
    );
}

#[test]
fn escapes_work_in_every_string() {
    let out = compile(
        "@page A \\$5\n@meta description costs \\$5\n@let v a\\$b\n@input [type=text, title=a\\$b, placeholder=$v]\n",
    );
    assert!(out.contains("<title>A $5</title>"), "{}", out);
    assert!(out.contains(r#"content="costs $5""#), "{}", out);
    assert!(out.contains(r#"title="a$b""#), "{}", out);
    assert!(out.contains(r#"placeholder="a$b""#), "{}", out);
}

#[test]
fn a_backslash_before_anything_else_is_kept() {
    let out = compile(
        "@el [before:content \"\\201C\"] x\n@input [type=text, pattern=\\d{3}-\\d{4}]\n@text C:\\temp\\",
    );
    assert!(out.contains(r#"content:"\201C""#), "{}", out);
    assert!(out.contains(r#"pattern="\d{3}-\d{4}""#), "{}", out);
    assert!(out.contains(r"C:\temp\</span>"), "{}", out);
}

#[test]
fn escaped_brackets_and_braces_do_not_close() {
    let out = compile("@el [content \"x\\\"]\", width 4] a\\]b\n@paragraph {@mark a\\}b} c");
    assert!(out.contains(r#"content:"x\"]""#), "{}", out);
    assert!(out.contains("width:4px"), "{}", out);
    assert!(out.contains("a]b"), "{}", out);
    assert!(out.contains("<mark>a}b</mark> c"), "{}", out);
}

#[test]
fn quoted_text_keeps_its_quotes_only_in_css() {
    let out = compile(
        "@let arrow \"→ \"\n@let alias $arrow\n@el [before:content $arrow, after:content $alias, aria-label=$arrow] Go $arrow",
    );
    assert!(out.contains(r#"::before{content:"→ ";}"#), "{}", out);
    assert!(out.contains(r#"::after{content:"→ ";}"#), "{}", out);
    assert!(out.contains(r#"aria-label="→ ""#), "{}", out);
    assert!(out.contains("Go → </"), "{}", out);
}

#[test]
fn quoted_text_inside_a_css_string_is_what_it_says() {
    let out = compile("@let q \"say \\\"hi\\\"\"\n@el [after:content \"« $q »\"] x\n@text $q");
    assert!(out.contains(r#"content:"« say \"hi\" »""#), "{}", out);
    assert!(out.contains("say &quot;hi&quot;"), "{}", out);
}

#[test]
fn quoted_arguments_lose_their_quotes_in_text() {
    let out = compile(
        "@let @card [lede]\n  @el [after:content $lede]\n    $lede\n@card [lede \"Fast, simple\"]",
    );
    assert!(out.contains("<span>Fast, simple</span>"), "{}", out);
    assert!(!out.contains("&quot;"), "{}", out);
    // and keep them in CSS, after passing through the parameter
    assert!(out.contains(r#"content:"Fast, simple""#), "{}", out);
}

#[test]
fn a_let_with_several_quoted_strings_is_kept_whole() {
    let out = compile(
        "@let areas \"head head\" \"side main\"\n@el [display grid, grid-template-areas $areas] x",
    );
    assert!(
        out.contains(r#"grid-template-areas:"head head" "side main""#),
        "{}",
        out
    );
}

#[test]
fn quoted_custom_property_keeps_its_quotes() {
    let out = compile("@let --font \"Inter\"\n@el [font-family var(--font)] x");
    assert!(out.contains(r#"--font:"Inter""#), "{}", out);
}

#[test]
fn an_escaped_comma_in_an_if_branch_stays_in_the_branch() {
    let out = compile(
        "@let on = true\n@el [if($on, transition opacity 1s\\, color 1s, transition none), if($on, font-family Inter\\, serif, color red)] x",
    );
    assert!(out.contains("transition:opacity 1s, color 1s;"), "{}", out);
    assert!(out.contains("font-family:Inter, serif"), "{}", out);
}

#[test]
fn a_backslash_at_the_end_of_a_value_keeps_a_css_string_closed() {
    let out =
        compile("@let dir C:\\\\\n@el [before:content \"$dir\", after:content \"« $dir »\"] x");
    assert!(out.contains(r#"::before{content:"C:\\";}"#), "{}", out);
    assert!(out.contains(r#"::after{content:"« C:\\ »";}"#), "{}", out);
}

#[test]
fn text_list_items_split_at_unescaped_commas() {
    let out = compile("@each $x in a\\, b, c\n  @text item $x.");
    assert!(out.contains("<span>item a, b.</span>"), "{}", out);
    assert!(out.contains("<span>item c.</span>"), "{}", out);
}

#[test]
fn a_loop_variable_forgets_that_a_name_was_quoted() {
    let out = compile("@let x \"q\"\n@each $x in a, b\n  @el [after:content $x] x");
    assert!(out.contains("content:a"), "{}", out);
    assert!(!out.contains(r#"content:"q""#), "{}", out);
}

#[test]
fn a_comma_split_value_says_how_to_keep_the_comma() {
    let result = parser::parse("@el [box-shadow 0 1px 2px red, 0 2px 4px blue] x");
    let warning = result
        .diagnostics
        .iter()
        .find(|d| d.code == "unknown-attribute")
        .expect("the rest of the value is an unknown attribute");
    assert!(
        warning.message.contains(r"write `\,`"),
        "{}",
        warning.message
    );
    // An unquoted font stack: the rest of the value is a bare word
    let result = parser::parse("@el [font-family Inter, sans-serif] x");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "unknown-attribute" && d.message.contains(r"write `\,`")),
        "{:?}",
        result.diagnostics
    );
    // A misspelled attribute of its own gets no such hint
    let result = parser::parse("@el [color red, paddin 4] x");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("comma")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn a_required_parameter_left_out_is_not_silently_empty() {
    let result = parser::parse("@let @greet [who]\n  @text Hello $who\n@greet\n");
    let missing: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "missing-parameter")
        .collect();
    assert_eq!(missing.len(), 1, "{:?}", result.diagnostics);
    assert_eq!(missing[0].severity, Severity::Error);
    assert!(
        missing[0].message.contains("@greet needs 'who'"),
        "{}",
        missing[0].message
    );
}

#[test]
fn a_parameter_default_fills_in_its_variables() {
    // The default `$brand` used to be passed on as the text `$brand`
    let out =
        compile("@let brand #123456\n@let @card [tone $brand]\n  @el [color $tone] x\n@card\n");
    assert!(out.contains("color:#123456"), "{}", out);
    assert!(!out.contains("$brand"), "{}", out);
}

// --- Slot mistakes used to drop content silently ---

#[test]
fn slot_mistakes_no_longer_drop_content_silently() {
    let card = "@let @card [title]\n  @article\n    @h3 $title\n    @children\n    @slot footer\n      No footer\n";
    let box_ = "@let @box\n  @el Box\n";
    for (source, code) in [
        ("@card [title A]\n  @slot foter\n    Hi\n", "unknown-slot"),
        (
            "@card [title A]\n  @el\n    @slot footer\n      Hi\n",
            "misplaced-slot",
        ),
        ("@box Hi\n", "unexpected-content"),
        ("@box\n  Hi\n", "unexpected-content"),
        ("@slot footer\n  Hi\n", "misplaced-slot"),
        ("@children\n", "misplaced-slot"),
    ] {
        let result = parser::parse(&format!("{}{}{}", card, box_, source));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == code && d.severity == Severity::Error),
            "{}: {:?}",
            source,
            result.diagnostics
        );
    }
    // A slot name with a space used to name the slot `my footer`
    let result = parser::parse("@let @c\n  @el\n    @slot my footer\n@c\n");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "invalid-slot-name"),
        "{:?}",
        result.diagnostics
    );
}

// --- One value model (typed values, real lists, lexical scope) ---

#[test]
fn a_comma_list_counts_its_items_not_its_characters() {
    let out = compile("@let fruits apple, banana, cherry\n@text ${length($fruits)} fruits\n");
    assert!(out.contains("3 fruits"), "{}", out);
    // Quoted or escaped, text with commas is one text
    let out = compile(
        "@let tagline \"Fast, simple\"\n@let motto a\\, b\n@text ${length($tagline)} ${length($motto)}\n",
    );
    assert!(out.contains("12 4"), "{}", out);
}

#[test]
fn a_list_prints_as_it_was_written() {
    let out = compile("@let price 1,299\n@let rgb red,green,blue\n@text $price $rgb\n");
    assert!(out.contains("1,299 red,green,blue"), "{}", out);
}

#[test]
fn rebinding_a_name_leaves_no_stale_fields() {
    let out = compile(
        "@data $x [\"a\", \"b\"]\n@let x hello, world, there\n@text ${length($x)} $x.0\n@each $w in $x\n  @text ($w)\n",
    );
    assert!(out.contains("3 hello"), "{}", out);
    assert!(out.contains("(there)"), "{}", out);
    let out = compile("@data $post {\"title\": \"T\"}\n@let post plain\n@text <$post.title>\n");
    // `$post` is text now: `.title` isn't a field of it
    assert!(out.contains("&lt;plain.title&gt;"), "{}", out);
}

#[test]
fn a_let_in_an_each_does_not_carry_over() {
    let src =
        "@each $i in 1, 2\n  @if $i == 2\n    @text prev=$last\n  @let last $i\n  @text $last\n";
    assert!(has_code(src, 3, "undefined-variable"), "{:?}", codes(src));
}

#[test]
fn bundles_and_functions_do_not_leak_out_of_blocks() {
    for src in [
        "@if true\n  @let b [padding 4]\n@el [$b] x\n",
        "@each $i in 1\n  @let b [padding 4]\n@el [$b] x\n",
        "@let @f\n  @let b [padding 4]\n  @el [$b] in\n@f\n@el [$b] x\n",
    ] {
        let last = src.lines().count();
        assert!(
            has_code(src, last, "undefined-variable"),
            "{}: {:?}",
            src,
            codes(src)
        );
    }
    for src in [
        "@if true\n  @let @g\n    @text g\n@g\n",
        "@el\n  @let @g\n    @text g\n@g\n",
    ] {
        let last = src.lines().count();
        assert!(
            has_code(src, last, "unknown-element"),
            "{}: {:?}",
            src,
            codes(src)
        );
    }
}

#[test]
fn values_in_an_element_s_children_stay_there() {
    let src = "@el\n  @let v one\n  @text $v\n@text v=$v\n";
    assert!(has_code(src, 4, "undefined-variable"), "{:?}", codes(src));
}

#[test]
fn a_record_goes_to_a_parameter_of_any_name() {
    let out = compile(
        "@data $posts [{\"title\": \"A\", \"slug\": \"a\"}]\n@let @post-card [post]\n  @h3 $post.title / $post.slug\n@each $p in $posts\n  @post-card [post $p]\n",
    );
    assert!(out.contains("A / a"), "{}", out);
}

#[test]
fn a_function_body_does_not_see_the_names_at_its_call() {
    let src = "@let @f\n  @text $v\n@el\n  @let v 1\n  @f\n";
    assert!(has_code(src, 2, "undefined-variable"), "{:?}", codes(src));
    // Nor a definition further down
    let src = "@let @f\n  @text $later\n@let later 1\n@f\n";
    assert!(has_code(src, 2, "undefined-variable"), "{:?}", codes(src));
}

#[test]
fn a_range_is_a_list_inside_expressions() {
    let out = compile("@text ${reverse(1..5)}\n@each $n in ${reverse(1..3)}\n  @text <$n>\n");
    assert!(out.contains("5, 4, 3, 2, 1"), "{}", out);
    assert!(out.contains("&lt;3&gt;"), "{}", out);
    let out = compile("@let a 2\n@let b 4\n@text ${$a..$b}\n");
    assert!(out.contains("2, 3, 4"), "{}", out);
}

#[test]
fn zero_is_false_however_it_is_written() {
    let out = compile(
        "@let z 0.0\n@let n = 1 - 1\n@text ${if($z, yes, no)} ${if($n, yes, no)} ${if(0, yes, no)}\n",
    );
    assert!(out.contains("no no no"), "{}", out);
}

#[test]
fn computed_numbers_print_without_float_noise() {
    let out = compile("@el [width ${100 / 3}%] ${0.1 + 0.2}\n");
    assert!(out.contains("width:33.3333%"), "{}", out);
    assert!(out.contains(">0.3<"), "{}", out);
}

#[test]
fn list_items_keep_escaped_commas_and_quotes() {
    let out = compile(
        "@let items a\\, b, c\n@each $y in $items\n  @text ($y)\n@each $z in \"c, d\", e\n  @text <$z>\n",
    );
    assert!(out.contains("(a, b)") && out.contains("(c)"), "{}", out);
    assert!(
        out.contains("&lt;c, d&gt;") && out.contains("&lt;e&gt;"),
        "{}",
        out
    );
}

#[test]
fn a_text_function_given_a_list_is_an_error() {
    let result = parser::parse("@let tagline Fast, simple\n@text ${uppercase($tagline)}\n");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == "invalid-expression")
        .expect("an error");
    assert!(d.message.contains("a list of 2 items"), "{}", d.message);
    assert!(d.message.contains("quote it"), "{}", d.message);
}

#[test]
fn each_needs_a_list() {
    let src = "@each $x in [1, 2]\n  @text $x\n@else\n  @text none\n";
    let found = parser::parse(src).diagnostics;
    let d = found
        .iter()
        .find(|d| d.code == "invalid-loop")
        .expect("error");
    assert_eq!(d.suggestion.as_deref(), Some("1, 2"));
    let src = "@data $post {\"title\": \"T\"}\n@each $x in $post\n  @text $x\n";
    assert!(has_code(src, 2, "invalid-loop"), "{:?}", codes(src));
    let src = "@each $i in 1..5 step 0\n  @text $i\n";
    assert!(has_code(src, 1, "invalid-expression"), "{:?}", codes(src));
}

#[test]
fn a_field_set_with_let_makes_a_record() {
    let result = parser::parse(
        "@let t.greeting Hello\n@let t.farewell Bye\n@text $t.greeting $t.farewell\n",
    );
    // Used through its fields: not reported unused
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code == "unused-variable"),
        "{:?}",
        result.diagnostics
    );
    let out = codegen::generate(&result.document);
    assert!(out.contains("Hello Bye"), "{}", out);
}

#[test]
fn a_field_set_with_let_keeps_the_kind_of_value() {
    // An index of a list replaces that item: the list stays a list
    let out = compile(
        "@let fruits apple, banana\n@let fruits.1 kiwi\n@text $fruits ${length($fruits)}\n",
    );
    assert!(out.contains("apple, kiwi 2"), "{}", out);
    // A field of a record in a list
    let out = compile("@data $ps [{\"t\": \"x\"}]\n@let ps.0.t y\n@text $ps.0.t\n");
    assert!(out.contains(">y<"), "{}", out);
    // Text has no fields, and a list has no item past its end: errors,
    // instead of turning the value into a record
    let src = "@let s hello\n@let s.x 1\n@text $s\n";
    assert!(has_code(src, 2, "invalid-definition"), "{:?}", codes(src));
    let src = "@let l a, b\n@let l.5 z\n@text $l\n";
    assert!(has_code(src, 2, "invalid-definition"), "{:?}", codes(src));
}

#[test]
fn a_name_defined_out_of_sight_says_where_names_are_visible() {
    for src in [
        "@el\n  @let v one\n@text $v\n",
        "@let @card\n  @text $later\n@let later L\n@card\n",
    ] {
        let d = parser::parse(src)
            .diagnostics
            .into_iter()
            .find(|d| d.code == "undefined-variable")
            .expect("an error");
        assert!(
            d.message.contains("to the end of its block"),
            "{}",
            d.message
        );
    }
    // A name never defined gets the plain message
    let d = parser::parse("@text $nothing\n")
        .diagnostics
        .into_iter()
        .find(|d| d.code == "undefined-variable")
        .expect("an error");
    assert!(!d.message.contains("block"), "{}", d.message);
}

// --- Input that was dropped without a word ---

#[test]
fn a_semicolon_from_data_no_longer_escapes_the_css_rule() {
    let result =
        parser::parse("@data $d {\"c\": \"red;} body{display:none\"}\n@el [color $d.c] x\n");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "invalid-value" && d.severity == Severity::Error),
        "{:?}",
        result.diagnostics
    );
    let html = codegen::generate(&result.document);
    assert!(!html.contains("body{display:none"), "{}", html);
}

#[test]
fn stacked_prefixes_are_no_longer_dropped_silently() {
    // Codegen stripped one prefix and dropped the rest, with no diagnostic
    // (P5 made them errors); they compose now
    let result = parser::parse(
        "@el [md:hover:color red, dark:hover:background black, children:nth-child(odd):color blue, first-child:before:content \"x\"] x\n",
    );
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let html = codegen::generate(&result.document);
    for css in [
        "@media(min-width:768px){.hl-a:hover{color:red;}}",
        "@media(prefers-color-scheme:dark){.hl-a:hover{background:black;}}",
        ":where(.hl-a)>*:nth-child(odd){color:blue;}",
        ".hl-a:first-child::before{content:\"x\";}",
    ] {
        assert!(html.contains(css), "{} in {}", css, html);
    }
}

#[test]
fn script_keeps_every_html_attribute() {
    // It used to keep only src, type, defer, async, crossorigin, integrity,
    // nomodule and id
    let html = compile("@script [src=a.js, referrerpolicy=no-referrer, data-x=1, nonce=abc]\n");
    assert!(
        html.contains(r#"<script src="a.js" referrerpolicy="no-referrer" data-x="1" nonce="abc">"#),
        "{}",
        html
    );
}

#[test]
fn text_on_an_element_without_a_closing_tag_is_reported() {
    // `@input hello` dropped the text
    let d = parser::parse("@input [type=text, aria-label=x] hello\n").diagnostics;
    assert!(
        d.iter()
            .any(|d| d.code == "unexpected-content" && d.severity == Severity::Error),
        "{:?}",
        d
    );
}

#[test]
fn a_duplicate_width_across_forms_is_not_reported() {
    // DESIGN's own `@image [width=800, width 200]` warned "duplicate"
    let d = parser::parse("@image [width=800, width 200, alt=A] a.png\n").diagnostics;
    assert!(d.is_empty(), "{:?}", d);
}

#[test]
fn a_quoted_semicolon_survives_a_duplicate_property() {
    // The later `content` replaced the earlier one by splitting the rule at
    // every `;`, so the quoted `;` cut `content:"a;b"` in two and `b"` was
    // left in the CSS
    let html = compile("@el [content \"a;b\", color red, content \"c;d\"] x\n");
    assert!(html.contains("color:red;content:\"c;d\";"), "{}", html);
    assert!(!html.contains("b\""), "{}", html);
}

#[test]
fn a_value_cannot_end_the_style_element() {
    let result = parser::parse(
        "@data $d {\"c\": \"</style><script>alert(1)</script>\"}\n@el [color $d.c, content \"</STYLE>\"] x\n@let --x </style>\n",
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == "invalid-value" && d.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 3, "{:?}", result.diagnostics);
    let html = codegen::generate(&result.document);
    assert!(!html.contains("<script>"), "{}", html);
}

#[test]
fn inline_is_only_for_an_image_without_a_prefix() {
    // Both were ignored by codegen without a word
    let d = parser::parse("@el [inline] x\n").diagnostics;
    assert!(
        d.iter()
            .any(|d| d.code == "unexpected-argument" && d.severity == Severity::Error),
        "{:?}",
        d
    );
    let d = parser::parse("@image [md:inline, alt=A] a.png\n").diagnostics;
    assert!(
        d.iter()
            .any(|d| d.code == "invalid-prefix" && d.severity == Severity::Error),
        "{:?}",
        d
    );
}

#[test]
fn a_misspelled_parameter_in_a_bundle_is_an_error_at_the_call() {
    // Written directly it was an error, but from a bundle it went to the
    // root element's CSS with a warning
    let src = "@let @card [tone]\n  @el $tone\n@let b [tnoe red]\n@card [$b, tone blue]\n";
    let d = parser::parse(src).diagnostics;
    assert!(
        d.iter().any(|d| d.severity == Severity::Error
            && d.line == 4
            && d.message
                .contains("unknown parameter 'tnoe', did you mean 'tone'?")),
        "{:?}",
        d
    );
}

// --- One layout per element (P1) ---

#[test]
fn spacing_on_a_list_was_a_gap_on_a_block() {
    // `@ol [spacing 4]` emitted gap:4px on a block <ol>, which did nothing
    let out = compile("@ol [spacing 4]\n  @li First\n  @li Second\n");
    assert!(
        out.contains("display:flex;flex-direction:column;"),
        "{}",
        out
    );
    assert!(out.contains("gap:4px"), "{}", out);
}

#[test]
fn a_button_s_lines_were_glued_together() {
    // 'Savechanges'
    let out = compile("@button [type=button]\n  Save\n  changes\n");
    assert!(out.contains(">Save changes</button>"), "{}", out);
}

#[test]
fn an_el_in_a_paragraph_was_a_div_inside_a_p() {
    // `<p>Price: <div>...` made the browser end the paragraph early
    let out = compile("@paragraph\n  Price: {@el [padding 2] 9}\n");
    assert!(!out.contains("<div"), "{}", out);
    assert!(out.contains("display:inline-flex"), "{}", out);
}

#[test]
fn canvas_and_iframe_took_spacing_without_laying_anything_out() {
    for src in [
        "@canvas [spacing 4]",
        "@iframe [spacing 4, title=x] https://x.org",
    ] {
        let result = parser::parse(src);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.code == "no-effect" && d.severity == Severity::Error),
            "{}: {:?}",
            src,
            result.diagnostics
        );
    }
}

#[test]
fn an_empty_fragment_in_text_left_two_spaces() {
    let out = compile("@paragraph\n  a\n  @fragment\n  b\n");
    assert!(out.contains(">a b</p>"), "{}", out);
    // Nor a line break in a native element
    let out = compile("@pre\n  a\n  @fragment\n  b\n");
    assert!(out.contains(">a\nb</pre>"), "{}", out);
}

#[test]
fn inline_elements_were_printed_as_text_in_a_heading() {
    // `<h1>Hello {@text [color red] world}</h1>`
    let out = compile("@h1 Hello {@text [color red] world}\n");
    assert!(!out.contains("{@"), "{}", out);
    assert!(
        out.contains("Hello <span class=\"hl-b\">world</span></h1>"),
        "{}",
        out
    );
    // `@text This is {@mark highlighted} text`
    let out = compile("@text This is {@mark highlighted} text\n");
    assert!(
        out.contains("This is <mark>highlighted</mark> text"),
        "{}",
        out
    );
}

#[test]
fn text_lines_at_the_top_of_the_page_were_glued() {
    // 'Readmore'
    let out = compile("Read\nmore\n");
    assert!(out.contains("Read\nmore"), "{}", out);
}

#[test]
fn a_fragment_s_lines_were_glued_to_the_lines_around_it() {
    // `<pre>sm\nnt</pre>`: a line before or after a @fragment in a native
    // element (or at the top of the page) had no line break
    let out = compile("@pre\n  s\n  @fragment\n    m\n    n\n  t\n");
    assert!(out.contains(">s\nm\nn\nt</pre>"), "{}", out);
    let out = compile("@let @two\n  @fragment\n    a\n    b\nx\n@two\n@two\ny\n");
    assert_eq!(out, "x\na\nb\na\nb\ny");
    // In a text element and a column nothing changes
    let out = compile("@h2 s\n  @fragment\n    m\n  t\n");
    assert!(out.contains(">s m t</h2>"), "{}", out);
    let out = compile("@el s\n  @fragment\n    m\n  t\n");
    assert!(
        out.contains("<span>s</span><span>m</span><span>t</span>"),
        "{}",
        out
    );
}

#[test]
fn a_video_s_captions_are_its_track_not_its_source() {
    // A <source> is another encoding of the video, not captions
    let src = "@video [controls] a.mp4\n  @source b.webm\n";
    assert!(has_code(src, 1, "missing-captions"), "{:?}", codes(src));
    let src = "@video [controls] a.mp4\n  @track [kind=captions, srclang=en] a.vtt\n";
    assert!(!has_code(src, 1, "missing-captions"), "{:?}", codes(src));
}

// --- Layout words and the parent's direction ---

#[test]
fn width_shrink_in_a_column_fits_the_content() {
    // It was `flex-shrink:0`, and the element still stretched to the
    // column's full width
    let out = compile("@el\n  @el [width shrink, background red] a\n");
    assert!(out.contains("width:fit-content;background:red;"), "{}", out);
    assert!(!out.contains("flex-shrink"), "{}", out);
}

#[test]
fn width_fill_in_a_row_made_a_column_is_not_flex() {
    // `flex:1` grew the child in height once the row was a column
    let out = compile("@row [flex-direction column]\n  @el [width fill] a\n");
    assert!(!out.contains("flex:1"), "{}", out);
    assert!(out.contains("width:100%"), "{}", out);
    // Also when the row turns into a column at a breakpoint
    let out = compile("@row [sm:flex-direction column]\n  @el [width fill] a\n");
    assert!(
        out.contains(
            "@media(min-width:640px){:where(.hl-a)>.hl-b{flex:0 1 auto;min-width:auto;width:100%;}"
        ),
        "{}",
        out
    );
}

#[test]
fn an_explicit_min_width_before_width_fill_is_kept() {
    // `min-width 200, width fill` in a row came out as `min-width:0`
    let out = compile("@row\n  @el [min-width 200, width fill] a\n");
    assert!(out.contains("min-width:200px;flex:1;"), "{}", out);
    assert!(!out.contains("min-width:0"), "{}", out);
}

#[test]
fn a_margin_does_not_undo_center_x() {
    // `margin 20` after `center-x` wrote over its auto margins, so the
    // element was no longer centred in a column
    let out = compile("@el\n  @el [center-x, margin 20] a\n");
    assert!(
        out.contains("margin:20px;margin-left:auto;margin-right:auto;"),
        "{}",
        out
    );
}

#[test]
fn a_flex_longhand_keeps_width_fill_growing() {
    // `flex-shrink 0` (or `flex-basis`) left out the whole `flex:1`, so the
    // element no longer filled the row
    let out = compile("@row\n  @el [flex-shrink 0, width fill] a\n");
    assert!(
        out.contains("flex-shrink:0;flex-grow:1;flex-basis:0%;min-width:0;"),
        "{}",
        out
    );
    let out = compile("@row\n  @el [flex-basis 200, width fill] a\n");
    assert!(
        out.contains("flex-basis:200px;flex-grow:1;flex-shrink:1;min-width:0;"),
        "{}",
        out
    );
    // `width shrink` keeps its `flex-shrink:0` beside a `flex-grow`
    let out = compile("@row\n  @el [flex-grow 2, width shrink] a\n");
    assert!(out.contains("flex-grow:2;flex-shrink:0;"), "{}", out);
    // Where the direction changes, the rule puts back only the longhands
    // the element doesn't write
    let out = compile("@row [md:flex-direction column]\n  @el [flex-shrink 0, width fill] a\n");
    assert!(
        out.contains(":where(.hl-a)>.hl-b{flex-grow:0;flex-basis:auto;min-width:auto;width:100%;}"),
        "{}",
        out
    );
}

#[test]
fn an_image_written_with_src_has_one_src() {
    // It used to write `src=""` for the missing argument, then `src="a.png"`
    let html = compile("@image [src=a.png, alt=x]\n");
    assert_eq!(html.matches("src=").count(), 1, "{}", html);
    assert!(
        html.contains(r#"<img src="a.png" class="hl-a" alt="x">"#),
        "{}",
        html
    );
}

#[test]
fn a_source_in_a_picture_is_a_srcset() {
    // `<source src>` inside `<picture>` is ignored by the browser
    let html = compile("@picture\n  @source [type=image/webp] a.webp\n  @image [alt=x] a.jpg\n");
    assert!(
        html.contains(r#"<source srcset="a.webp" type="image/webp">"#),
        "{}",
        html
    );
}

#[test]
fn a_form_s_action_is_its_first_word_only() {
    // `@form /subscribe Sign up` gave action="/subscribe Sign up"
    let html = compile("@form /subscribe Sign up\n");
    assert!(html.contains(r#"action="/subscribe""#), "{}", html);
    assert!(html.contains("<span>Sign up</span>"), "{}", html);
}

#[test]
fn inline_works_with_the_src_attribute_too() {
    // `@image [src=a.svg, inline]` left `inline` unused and wrote the `<img>`
    let dir = scratch_dir("inline_src_attribute");
    std::fs::write(dir.join("a.svg"), "<svg viewBox=\"0 0 1 1\"></svg>").unwrap();
    let html = compile_in(
        &dir,
        "@image [src=a.svg, inline, aria-label=x]\n@image [inline, aria-label=x] a.svg\n",
    );
    assert_eq!(html.matches("<svg").count(), 2, "{}", html);
    assert!(!html.contains("<img"), "{}", html);
}

#[test]
fn an_outline_keeps_the_style_it_was_given() {
    // `outline 2 solid red` became `outline:2px solid solid red`, and an
    // outline without a style got an invented `solid`
    let html = compile("@input [type=text, aria-label=x, focus:outline 2 solid var(--brand)]\n");
    assert!(
        html.contains(":focus{outline:2px solid var(--brand);}"),
        "{}",
        html
    );
    assert!(!html.contains("solid solid"), "{}", html);
    let html = compile("@el [outline 2 red]\n");
    assert!(html.contains("outline:2px red;"), "{}", html);
    assert!(!html.contains("solid"), "{}", html);
}

#[test]
fn css_s_container_shorthand_is_written_as_it_is() {
    // `container X` always became `container-type:inline-size`, dropping
    // the name
    let html = compile("@el [container card / inline-size]\n");
    assert!(html.contains("container:card / inline-size;"), "{}", html);
    assert!(!html.contains("container-type"), "{}", html);
}

#[test]
fn a_class_of_one_s_own_doesn_t_pick_up_generated_rules() {
    // `@el [class=a]` gave `class="j a"`, and the generated `.a` rule
    // styled it too
    let html = compile("@el [padding 1]\n@el [class=a, padding 2]\n");
    assert!(html.contains(r#"class="hl-b a""#), "{}", html);
    assert!(!html.contains(".a{"), "{}", html);
}

#[test]
fn a_closed_dialog_and_a_hidden_element_stay_hidden() {
    // htmlang's `display:flex` beat the browser's `display:none`, so a
    // closed @dialog and `@el [hidden]` showed
    let rule = "{display:none!important}";
    for src in ["@page T\n@dialog Closed\n", "@dialog Closed\n"] {
        let html = compile(src);
        assert!(html.contains(r#"<dialog class="hl-a">"#), "{}", html);
        let at = html.find(rule).unwrap_or_else(|| panic!("{}", html));
        let selector = &html[..at];
        assert!(
            selector.ends_with(
                r#":where([class^="hl-"],[class*=" hl-"]):where([hidden]:not([hidden="until-found"]),dialog:not([open]):not(:popover-open),[popover]:not(:popover-open):not(dialog[open]))"#
            ),
            "{}",
            html
        );
        // In the reset layer, before the dialog's defaults
        assert!(
            html.find("@layer hl-reset{").unwrap() < at
                && at < html.find("@layer hl-kind{").unwrap(),
            "{}",
            html
        );
    }
    let html = compile("@el [hidden] Hidden\n");
    assert!(html.contains(r#"<div class="hl-a" hidden>"#), "{}", html);
    assert!(html.contains(rule), "{}", html);
}

#[test]
fn a_fragment_without_styles_has_no_empty_layer() {
    // Every output had `<style>@layer htmlang{}</style>`
    assert_eq!(compile("Hello\n"), "Hello");
    let html = compile("@page T\nHello\n");
    assert!(!html.contains("@layer htmlang{}"), "{}", html);
}

#[test]
fn a_line_style_after_an_outline_width_is_kept() {
    let out = compile("@el [outline 2 dashed red] x\n@el [outline none] y");
    assert!(out.contains("outline:2px dashed red;"), "{}", out);
    assert!(out.contains("outline:none;"), "{}", out);
}

#[test]
fn numbers_inside_a_colour_function_get_no_px() {
    let out = compile("@el [border 1 solid rgb(255 128 0)] x");
    assert!(out.contains("border:1px solid rgb(255 128 0);"), "{}", out);
}

#[test]
fn border_image_width_is_a_number() {
    let out = compile("@el [border-image-width 2, border-image-slice 30] x");
    assert!(out.contains("border-image-width:2;"), "{}", out);
    assert!(out.contains("border-image-slice:30;"), "{}", out);
}

#[test]
fn every_shadow_gets_px() {
    let out = compile("@el [hover:box-shadow 0 4 12 rgba(0,0,0,0.1)\\, 0 1 2 red] x");
    assert!(
        out.contains("box-shadow:0 4px 12px rgba(0,0,0,0.1), 0 1px 2px red;"),
        "{}",
        out
    );
}

#[test]
fn important_right_after_a_length_keeps_its_px() {
    let out = compile("@el [margin 10!important, padding 4 8 !important] x");
    assert!(out.contains("margin:10px!important;"), "{}", out);
    assert!(out.contains("padding:4px 8px !important;"), "{}", out);
}

#[test]
fn every_length_valued_property_gets_px() {
    let out = compile(
        "@el [vertical-align -2, shape-margin 4, offset-distance 10, offset-position 10 20, offset-anchor 0 5, overflow-clip-margin content-box 8, view-timeline-inset auto 12] x",
    );
    for decl in [
        "vertical-align:-2px;",
        "shape-margin:4px;",
        "offset-distance:10px;",
        "offset-position:10px 20px;",
        "offset-anchor:0 5px;",
        "overflow-clip-margin:content-box 8px;",
        "view-timeline-inset:auto 12px;",
    ] {
        assert!(out.contains(decl), "{decl}: {out}");
    }
}

#[test]
fn a_custom_property_starting_a_line_of_a_list_is_not_dropped() {
    // `--surface white` on its own line of a list was read as a comment and
    // left out without a word
    let result = parser::parse("@el [\n  padding 4,\n  --surface white,\n  color red\n]\n  x\n");
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let html = codegen::generate(&result.document);
    assert!(html.contains("--surface:white;"), "{}", html);
}

#[test]
fn a_caller_s_attributes_win_over_a_function_s_scoped_style() {
    // The scoped @style was unlayered, so its `& { padding }` beat the
    // caller's `[padding 2]`, and it was written for a function that was
    // never called
    let function = "@let @box\n  @style\n    & { padding: 40px; }\n  @el x\n";
    let out = compile(&format!("{}@box [padding 2]\n", function));
    let order = "@layer hl-reset,hl-kind,hl-inside,htmlang;";
    assert!(out.contains(order), "{}", out);
    let scoped = out.find("@layer hl-inside{.hl-fn-box {").expect(&out);
    let own = out.find("@layer htmlang{").expect(&out);
    assert!(
        scoped < own && out[own..].contains("padding:2px"),
        "{}",
        out
    );
    // Nothing after the layers: no unlayered copy of the scoped rules
    assert_eq!(out.matches("padding: 40px").count(), 1, "{}", out);
    // Called only where the page doesn't go: not written
    let out = compile(&format!(
        "{}@let on false\n@if $on\n  @box\n@text y\n",
        function
    ));
    assert!(!out.contains("hl-fn-box"), "{}", out);
}

// --- Found in the final review of the whole branch ---

fn diagnostics_of(input: &str) -> Vec<htmlang::diagnostic::Diagnostic> {
    parser::parse(input).diagnostics
}

fn coded_in(diags: &[htmlang::diagnostic::Diagnostic], code: &str) -> bool {
    diags.iter().any(|d| d.code == code)
}

#[test]
fn a_style_value_over_two_lines_keeps_its_words_apart() {
    // Compact output joined the lines of an @style with nothing between
    // them: `margin: 0` then `auto;` became `0auto`
    let fn_style = "@let @box\n  @style\n    h2 { margin: 0\n      auto; }\n  @el x\n@box\n";
    let out = compile(&format!(
        "@style\n  .a {{ margin: 0\n    auto; }}\n{}",
        fn_style
    ));
    assert!(out.contains(".a { margin: 0 auto; }"), "{}", out);
    assert!(out.contains("h2 { margin: 0 auto; }"), "{}", out);
    assert!(!out.contains("0auto"), "{}", out);
}

#[test]
fn a_call_s_class_adds_to_the_root_s_classes_and_keeps_the_scope() {
    let src = "@let @note\n  @style\n    .t { color: red; }\n  @el [class=base]\n    @text [class=t] hi\n@note [class=extra]\n@note\n";
    let out = compile(src);
    assert!(
        out.contains("<div class=\"hl-a base extra hl-fn-note\">"),
        "{}",
        out
    );
    assert!(
        out.contains("<div class=\"hl-a base hl-fn-note\">"),
        "{}",
        out
    );
}

#[test]
fn a_parameter_passed_again_takes_the_later_value() {
    // After a bundle, silently; twice in the call's own list, with a warning
    let out = compile("@let b [t b1]\n@let @f [t]\n  @text $t\n@f [$b, t mine]\n");
    assert!(out.contains(">mine<"), "{}", out);
    let result = parser::parse("@let @f [t]\n  @text $t\n@f [t a, t b]\n");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("passed twice: the later one wins")),
        "{:?}",
        result.diagnostics
    );
    let out = codegen::generate(&result.document);
    assert!(out.contains(">b<"), "{}", out);
    let diags = diagnostics_of("@let b [t b1]\n@let @f [t]\n  @text $t\n@f [$b, t mine]\n");
    assert!(!coded_in(&diags, "duplicate-attribute"), "{:?}", diags);
}

#[test]
fn a_custom_property_is_declared_at_the_top_level_once() {
    // Inside a function it was hoisted to :root, and calls overwrote each
    // other
    let diags = diagnostics_of(
        "@let @theme [c]\n  @let --c $c\n  @el [color var(--c)] y\n@theme [c green]\n@theme [c pink]\n",
    );
    let inside: Vec<_> = diags
        .iter()
        .filter(|d| d.code == "invalid-definition")
        .collect();
    assert_eq!(inside.len(), 1, "{:?}", diags);
    assert!(
        inside[0].message.contains("[--c value]"),
        "{}",
        inside[0].message
    );
    let result = parser::parse("@let --a red\n@el [color var(--a)] x\n@let --a 12\n");
    assert!(
        coded_in(&result.diagnostics, "duplicate-attribute"),
        "{:?}",
        result.diagnostics
    );
    let out = codegen::generate(&result.document);
    assert!(
        out.contains("--a:12;") && !out.contains("--a:red"),
        "{}",
        out
    );
}

#[test]
fn an_ampersand_in_an_unscoped_style_is_reported() {
    let diags = diagnostics_of(
        "@let @note [warn false]\n  @if $warn\n    @style\n      & { border-color: red; }\n  @el x\n@note [warn]\n",
    );
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("`&` in this @style")),
        "{:?}",
        diags
    );
    // Nested under a rule, or in a scoped @style, it is fine
    let diags = diagnostics_of(
        "@style\n  .a { &:hover { color: red; } }\n@let @n\n  @style\n    & { color: red; }\n  @el x\n@n\n",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("`&` in this @style")),
        "{:?}",
        diags
    );
}

#[test]
fn a_chain_after_a_leading_argument_is_reported() {
    let result = parser::parse("@link /a > @image logo.png\n");
    let d = result
        .diagnostics
        .iter()
        .find(|d| d.code == "unexpected-argument")
        .expect("a warning");
    assert!(
        d.message.contains("@link [href=/a] > @image"),
        "{}",
        d.message
    );
    // Text with a `>` that doesn't name an element is text
    let diags = diagnostics_of("@link /a Next > more\n");
    assert!(!coded_in(&diags, "unexpected-argument"), "{:?}", diags);
}

#[test]
fn a_layout_word_on_text_is_reported_in_code_that_doesn_t_run() {
    let diags = diagnostics_of("@let @card\n  @text [spacing 4] x\n@if false\n  @text [wrap] y\n");
    let lines: Vec<usize> = diags
        .iter()
        .filter(|d| d.code == "no-effect")
        .map(|d| d.line)
        .collect();
    assert_eq!(lines, [2, 4], "{:?}", diags);
}

#[test]
fn children_after_an_element_s_attributes_is_reported() {
    let diags = diagnostics_of(
        "@let @cell [pad 8]\n  @td [padding $pad] @children\n@table > @tr\n  @cell\n",
    );
    assert!(coded_in(&diags, "misplaced-slot"), "{:?}", diags);
    let diags = diagnostics_of("@let @sample\n  @pre > @code\n    @children\n@sample\n");
    let d = diags
        .iter()
        .find(|d| d.code == "misplaced-slot")
        .expect("an error");
    assert_eq!(d.line, 3, "{:?}", d);
}

#[test]
fn an_unclosed_inline_element_is_reported() {
    let result = parser::parse("@paragraph Unclosed {@b bold\n");
    assert!(
        coded_in(&result.diagnostics, "unclosed-bracket"),
        "{:?}",
        result.diagnostics
    );
    assert!(codegen::generate(&result.document).contains("<b>bold</b>"));
}

#[test]
fn a_size_under_a_prefix_undoes_fill_in_a_row() {
    let out = compile("@row\n  @el [width fill, lg:width 200] a\n");
    assert!(
        out.contains("@media(min-width:1024px){.hl-b{flex:0 1 auto;min-width:auto;width:200px;}}"),
        "{}",
        out
    );
}

#[test]
fn name_mistakes_get_the_fix_that_fits() {
    let d = diagnostics_of("@let card [padding 8]\n@el [card] a\n");
    assert!(
        d.iter().any(|d| d.suggestion.as_deref() == Some("$card")),
        "{:?}",
        d
    );
    let d = diagnostics_of("@let @fn\n  @el x\n@fn\n@text $fn\n");
    assert!(
        d.iter().any(|d| d.message.contains("is a function")),
        "{:?}",
        d
    );
    let d = diagnostics_of("@each $i in 1..2\n  @el [id=item-$i-label] x\n");
    assert!(
        d.iter().any(|d| d.message.contains("`${i}-label`")),
        "{:?}",
        d
    );
    let d = diagnostics_of("@svg x\n");
    assert!(d.iter().any(|d| d.message.contains("@raw")), "{:?}", d);
}

#[test]
fn prefixes_in_another_order_are_the_same_key() {
    let d = diagnostics_of(
        "@el [md:dark:padding 1, dark:md:padding 2, md:hover:color red, hover:md:color blue] x\n",
    );
    let dups = d.iter().filter(|d| d.code == "duplicate-attribute").count();
    assert_eq!(dups, 2, "{:?}", d);
    // The order of selector prefixes is the selector's: not a duplicate
    let d = diagnostics_of("@table [hover:@td:color red, @td:hover:color blue]\n  @tr > @td x\n");
    assert!(!coded_in(&d, "duplicate-attribute"), "{:?}", d);
}

#[test]
fn a_style_written_as_an_html_attribute_is_reported() {
    let d =
        diagnostics_of("@el [padding=4, width=200, title=x] x\n@image [width=20, alt=x] a.png\n");
    let found: Vec<(usize, bool)> = d
        .iter()
        .filter(|d| d.code == "html-attribute-form")
        .map(|d| (d.line, d.suggestion.is_some()))
        .collect();
    assert_eq!(found, [(1, true), (1, false)], "{:?}", d);
    // `for=` is an attribute of @output too
    let d = diagnostics_of("@output [for=a] x\n");
    assert!(!coded_in(&d, "no-effect"), "{:?}", d);
}

#[test]
fn a_custom_property_s_messages_don_t_suggest_a_function() {
    let d = diagnostics_of("@let --empty\n");
    assert!(
        d.iter()
            .any(|d| d.message.contains("custom property") && !d.message.contains("@let @--")),
        "{:?}",
        d
    );
}

#[test]
fn a_function_that_wraps_the_element_it_is_named_after_is_one_error() {
    let d =
        diagnostics_of("@let @nav [items]\n  @nav [spacing 8]\n    @text $items\n@nav [items a]\n");
    let errors: Vec<_> = d.iter().filter(|d| d.severity == Severity::Error).collect();
    assert_eq!(errors.len(), 1, "{:?}", d);
    assert_eq!(errors[0].code, "recursive-call");
    // Under an @if it can stop: a tree
    let d = diagnostics_of("@let @ul [n]\n  @if $n > 0\n    @ul [n ${$n - 1}]\n@ul [n 2]\n");
    assert!(!coded_in(&d, "recursive-call"), "{:?}", d);
}

#[test]
fn inline_svg_styles_go_in_its_style_attribute() {
    let dir = scratch_dir("svg_style");
    std::fs::write(dir.join("i.svg"), "<svg viewBox=\"0 0 1 1\"></svg>").unwrap();
    let out = compile_in(&dir, "@image [inline, fill var(--c), width 24] i.svg\n");
    assert!(
        out.contains("<svg viewBox=\"0 0 1 1\" style=\"fill:var(--c);width:24px\">"),
        "{}",
        out
    );
    // The file's own `style=` stays, before the element's styles
    std::fs::write(
        dir.join("j.svg"),
        "<svg style=\"display: block;\" viewBox=\"0 0 1 1\"></svg>",
    )
    .unwrap();
    let out = compile_in(&dir, "@image [inline, width 24] j.svg\n");
    assert!(
        out.contains("<svg style=\"display: block;width:24px\" viewBox=\"0 0 1 1\">"),
        "{}",
        out
    );
}

#[test]
fn an_inlined_raster_is_read_next_to_the_file() {
    let dir = scratch_dir("raster");
    std::fs::write(dir.join("p.png"), b"\x89PNGxxxx").unwrap();
    let out = compile_in(&dir, "@page T\n@image [inline, alt=x] p.png\n");
    assert!(out.contains("src=\"data:image/png;base64,"), "{}", out);
    let result = parser::parse_with_base("@image [inline, alt=x] nothere.png\n", Some(&dir));
    assert!(
        coded_in(&result.diagnostics, "unreadable-file"),
        "{:?}",
        result.diagnostics
    );
    let out = compile_in(&dir, "@page [favicon p.png] T\n");
    assert!(
        out.contains("rel=\"icon\" href=\"data:image/png;base64,"),
        "{}",
        out
    );
}

#[test]
fn meta_in_a_fragment_is_reported() {
    let d = diagnostics_of("@meta description Hello\n@text hi\n");
    assert!(coded_in(&d, "no-effect"), "{:?}", d);
    let d = diagnostics_of("@page T\n@meta description Hello\n@text hi\n");
    assert!(!coded_in(&d, "no-effect"), "{:?}", d);
}

#[test]
fn a_list_of_records_in_text_is_an_error() {
    let d = diagnostics_of("@data $posts [{\"t\": \"a\"}, {\"t\": \"b\"}]\n@text All: $posts\n");
    assert!(
        d.iter()
            .any(|d| d.message.contains("list of records") && d.message.contains("$posts.0.t")),
        "{:?}",
        d
    );
    // A list in a list prints its items
    let out = compile("@let items a, b\n@let more $items, d\n@text $more\n");
    assert!(out.contains(">a, b, d<"), "{}", out);
}

#[test]
fn a_library_may_include_a_library_and_read_data() {
    let dir = scratch_dir("libraries");
    std::fs::write(dir.join("base.hl"), "@let gap 8\n").unwrap();
    std::fs::write(dir.join("theme.hl"), "@include base.hl\n@let accent red\n").unwrap();
    std::fs::write(dir.join("header.hl"), "@let spare 1\n@text header\n").unwrap();
    let theme = std::fs::read_to_string(dir.join("theme.hl")).unwrap();
    let result = parser::parse_with_base(&theme, Some(&dir));
    assert!(
        !coded_in(&result.diagnostics, "unused-variable"),
        "{:?}",
        result.diagnostics
    );
    // An included page's unused names are its own, not the includer's
    let result = parser::parse_with_base("@page T\n@include header.hl\n", Some(&dir));
    assert!(
        !coded_in(&result.diagnostics, "unused-variable"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn mixed_tabs_and_spaces_are_reported() {
    let d = diagnostics_of("@el\n\t@text a\n  @text b\n");
    let found: Vec<usize> = d
        .iter()
        .filter(|d| d.code == "mixed-indentation")
        .map(|d| d.line)
        .collect();
    assert_eq!(found, [3], "{:?}", d);
    assert!(!coded_in(
        &diagnostics_of("@el\n\t@text a\n\t\t@text b\n"),
        "mixed-indentation"
    ));
    // A tab inside a verbatim body, after the body's own indentation, is
    // content (a Go sample, a script), and so is a continuation line's
    let content = "@data $x [\n\t1, 2\n]\n@el\n  @code\n    func main() {\n    \tfmt.Println()\n    }\n  @script\n    if (a) {\n    \tb()\n    }\n";
    assert!(
        !coded_in(&diagnostics_of(content), "mixed-indentation"),
        "{:?}",
        diagnostics_of(content)
    );
    // A body indented with the other one than the lines around it is
    // reported, since that decides which lines are in the body
    let d = diagnostics_of("@el\n  @code\n\t\t\tx\n");
    assert!(coded_in(&d, "mixed-indentation"), "{:?}", d);
}

#[test]
fn a_leading_argument_that_can_t_be_one_is_reported() {
    let d = diagnostics_of("@link {@b x} y\n@link \"unclosed x\n");
    let lines: Vec<usize> = d
        .iter()
        .filter(|d| d.code == "unexpected-argument")
        .map(|d| d.line)
        .collect();
    assert_eq!(lines, [1, 2], "{:?}", d);
}

// --- Found while looking at htmx's attributes ---

#[test]
fn an_attribute_after_a_bundle_or_a_function_s_own_wins() {
    // The browser keeps the first of two attributes with one name, so
    // writing both made the bundle's win over the one written after it
    let out = compile("@let live [hx-get=/x, hx-trigger=load]\n@el [$live, hx-trigger=click] A\n");
    assert!(
        out.contains(r#"hx-get="/x" hx-trigger="click">"#),
        "{}",
        out
    );
    assert_eq!(out.matches("hx-trigger=").count(), 1, "{}", out);
    let out = compile(
        "@let @action [url]\n  @button [type=button, hx-post=$url, hx-target=#result] Save\n@action [url /save, hx-target=#other]\n",
    );
    assert!(out.contains(r##"hx-target="#other""##), "{}", out);
    assert_eq!(out.matches("hx-target=").count(), 1, "{}", out);
    let out = compile("@page [lang=en, lang=fr] T\n@text a\n");
    assert!(out.contains(r#"<html lang="fr">"#), "{}", out);
    let d = diagnostics_of("@el [hx-get=/a, hx-get=/b] B\n");
    assert!(
        d.iter()
            .any(|d| d.code == "duplicate-attribute" && d.message.contains("later one wins")),
        "{:?}",
        d
    );
}

#[test]
fn a_comma_that_ends_an_html_attribute_s_value_is_reported() {
    // The rest of `hx-trigger=load, every 2s` was a CSS property `every`,
    // with a warning that didn't say why
    let d = diagnostics_of("@el [hx-get=/poll, hx-trigger=load, every 2s] A\n");
    let split = d
        .iter()
        .find(|d| d.code == "split-value")
        .expect("split-value");
    assert_eq!(split.severity, Severity::Error);
    assert!(
        split.message.contains(r"`hx-trigger=load\, every 2s`"),
        "{}",
        split.message
    );
    // A bare word, as in `hx-confirm=Sure, really?`
    assert!(coded_in(
        &diagnostics_of("@el [hx-confirm=Sure, really?] A\n"),
        "split-value"
    ));
    // Close to a known name, it is that name misspelled, with a note
    let d = diagnostics_of("@el [id=x, colr red] A\n");
    let typo = d
        .iter()
        .find(|d| d.code == "unknown-attribute")
        .expect("unknown-attribute");
    assert!(typo.message.contains("'color'") && typo.message.contains(r"`id=x\, colr red`"));
    // An escaped comma, or a known attribute after the comma, is fine
    let out = compile("@el [hx-trigger=load\\, every 2s, color red] A\n");
    assert!(out.contains(r#"hx-trigger="load, every 2s""#), "{}", out);
}

#[test]
fn hx_attributes_are_html_attributes() {
    // `hx-get /items` was a CSS property, with a warning
    let d = diagnostics_of("@button [hx-get /items] B\n");
    let form = d
        .iter()
        .find(|d| d.code == "html-attribute-form")
        .expect("form");
    assert_eq!(form.severity, Severity::Error);
    assert!(form.message.contains("`hx-get=/items`"), "{}", form.message);
    // htmx's flags are written bare, like `data-` ones; ARIA's take a value
    let out = compile("@el [hx-preserve, data-open, id=player] A\n");
    assert!(
        out.contains(r#"id="player" hx-preserve data-open>"#),
        "{}",
        out
    );
    assert!(coded_in(
        &diagnostics_of("@el [aria-hidden] A\n"),
        "html-attribute-form"
    ));
    assert!(coded_in(
        &diagnostics_of("@el [md:hx-preserve] A\n"),
        "invalid-prefix"
    ));
}

#[test]
fn link_text_taken_as_the_url_is_reported() {
    // `Open` became the href, and `panel` the text, without a word
    let d = diagnostics_of("@link [hx-get=/panel, hx-target=#main] Open panel\n");
    let d = d
        .iter()
        .find(|d| d.code == "text-as-url")
        .expect("text-as-url");
    assert_eq!(d.severity, Severity::Warning);
    assert!(
        d.message.contains("'Open'") && d.message.contains("href"),
        "{}",
        d.message
    );
    assert!(coded_in(&diagnostics_of("@form Sign in\n"), "text-as-url"));
    for fine in [
        "@link /about About\n",
        "@link about About us\n",
        "@link ./Home Home\n",
        "@link https://example.com Out\n",
        "@link $url More\n",
        "@image [alt=Logo] Logo.svg\n",
        "@optgroup Citrus\n  @option Lemon\n",
    ] {
        assert!(!coded_in(&diagnostics_of(fine), "text-as-url"), "{}", fine);
    }
}

#[test]
fn a_bracket_inside_single_quotes_is_reported() {
    // The `]` in `alert(']')` ended the list, and the rest became text
    let d = diagnostics_of("@button [type=button, hx-on:click=alert(']')] Go\n");
    let d = d
        .iter()
        .find(|d| d.code == "unbalanced-quote")
        .expect("unbalanced-quote");
    assert_eq!(d.column, Some(41));
    // Escaped, it is in the value; an apostrophe in text is fine
    let out = compile("@button [type=button, hx-on:click=alert('\\]')] Go\n");
    assert!(out.contains(r#"hx-on:click="alert(']')">Go"#), "{}", out);
    assert!(!coded_in(
        &diagnostics_of("@el [title=Don't] It's fine\n"),
        "unbalanced-quote"
    ));
}
