//! Regression tests for bugs found in review. Each test names the behavior
//! that used to be wrong.

use std::path::PathBuf;

use htmlang::codegen;
use htmlang::parser::{self, Severity};

fn compile(input: &str) -> String {
    let result = parser::parse(input);
    assert!(
        result.diagnostics.iter().all(|d| d.severity != Severity::Error),
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
    compile_in(&dir, "@data d.json\n@text ok");
    std::fs::write(dir.join("d.json"), "{").unwrap();
    compile_in(&dir, "@data d.json\n@text ok");
}

#[test]
fn json_unicode_escapes_are_decoded() {
    let dir = scratch_dir("json_unicode");
    std::fs::write(
        dir.join("d.json"),
        r#"{"name": "Caf\u00e9", "emoji": "\ud83d\ude00"}"#,
    )
    .unwrap();
    let out = compile_in(&dir, "@data d.json\n@text $name $emoji");
    assert!(out.contains("Café 😀"), "{}", out);
}

#[test]
fn page_zero_does_not_underflow() {
    let out = compile("@let _page 0\n@each $x in a, b, c [page 2]\n  @text $x");
    assert!(out.contains(">a<") && out.contains(">b<"), "{}", out);
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
    let out = compile("@page T\n@link [color red, underline] /x Home");
    assert!(
        out.contains("@layer hl-reset,htmlang;@layer hl-reset{"),
        "reset must be layered so class rules can override it: {}",
        out
    );
}

#[test]
fn px_values_are_not_doubled() {
    let out = compile("@el [padding 10px, margin 0 auto, max-width none, blur 4px] x");
    assert!(out.contains("padding:10px"), "{}", out);
    assert!(out.contains("margin:0 auto"), "{}", out);
    assert!(out.contains("max-width:none"), "{}", out);
    assert!(!out.contains("pxpx"), "{}", out);
}

#[test]
fn minify_keeps_significant_spaces() {
    let result = parser::parse("@page T\n@paragraph\n  Built with {@text [bold] htmlang}.");
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
        "@style\n  @scope (.card) { .title { font-weight: bold; } }\n@keyframes k\n  from [opacity 0]\n@text hi",
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
    let out = compile(&format!("@image [width 200, alt x] {}", svg.display()));
    assert!(!out.contains("height=\"1000\""), "{}", out);
}

#[test]
fn source_map_uses_real_output_lines() {
    let html = "<!DOCTYPE html>\n<html>\n<div data-hl-line=\"3\">\n<span data-hl-line=\"4\">x</span>\n";
    let map = codegen::source_map_for_html(html, "a.hl");
    // Lines 1-2 have no mapping; line 3 maps to source line 3 (0-based 2 =
    // VLQ "E"), line 4 to source line 4 (delta 1 = "C").
    assert!(map.contains("\"mappings\":\";;AAEA;AACA\""), "{}", map);
}

// --- Parser logic ---

#[test]
fn bracket_in_text_does_not_join_lines() {
    let out = compile("@column\n  Use [ to open a list\n  @text [bold] Second\n  @text Third");
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
fn triple_quoted_let_is_a_string_not_a_function() {
    let out = compile("@let msg \"\"\"\n  hello\n  world\n  \"\"\"\n@text $msg");
    assert!(out.contains("hello"), "{}", out);
    assert!(!out.contains("\"\"\""), "{}", out);
    assert!(!out.contains("$msg"), "{}", out);
}

#[test]
fn pagination_suffix_is_not_an_item() {
    let out = compile("@each $x in a, b [page 5]\n  @text $x");
    assert!(!out.contains("[page"), "{}", out);
}

#[test]
fn each_else_does_not_leak_variables() {
    let out = compile(
        "@let empty \"\"\n@each $x in $empty\n  @text $x\n@else\n  @let leaked yes\n  @text none\n@text v=$leaked",
    );
    assert!(!out.contains("v=yes"), "{}", out);
}

#[test]
fn named_argument_is_not_used_positionally_for_another_param() {
    let out = compile(
        "@let card $title $variant=primary\n  @text t=$title v=$variant\n@card [variant danger]",
    );
    assert!(out.contains("v=danger"), "{}", out);
    assert!(!out.contains("t=danger"), "{}", out);
}

#[test]
fn keyframe_values_keep_commas_inside_parens() {
    let out = compile("@keyframes k\n  from [transform translate(0, 0)]\n  to [opacity 1]\n@text x");
    assert!(out.contains("transform:translate(0, 0)"), "{}", out);
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
    assert!(out.contains("width=\"48\""), "{}", out);
}

#[test]
fn markdown_keeps_paragraphs_rules_and_ordered_lists() {
    let out = compile(
        "@markdown\n  First para.\n\n  Second para.\n\n  ---\n\n  1. one\n  2. two\n\n  ```\n  if x:\n      y\n  ```",
    );
    assert!(out.contains("<p>First para.</p>"), "{}", out);
    assert!(out.contains("<p>Second para.</p>"), "{}", out);
    assert!(out.contains("<hr>"), "{}", out);
    assert!(out.contains("<ol>\n<li>one</li>\n<li>two</li>\n</ol>"), "{}", out);
    assert!(out.contains("if x:\n    y"), "{}", out);
}

#[test]
fn quoted_let_values_are_not_evaluated_as_arithmetic() {
    let out = compile("@let span \"1 / -1\"\n@el [grid-area $span] x");
    assert!(out.contains("grid-area:1 / -1"), "{}", out);
}

#[test]
fn quoted_font_stack_is_one_attribute() {
    let out = compile("@el [font \"Inter, sans-serif\", bold] x");
    assert!(out.contains("font-family:Inter, sans-serif"), "{}", out);
    assert!(out.contains("font-weight:bold"), "{}", out);
}

#[test]
fn diagnostic_column_includes_indentation() {
    let result = parser::parse("@let name x\n@column\n    Hello $nmae");
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
    assert!(out.contains("<a href=\"/more\">more</a>"), "{}", out);
}

#[test]
fn html_attributes_use_equals_and_are_all_emitted() {
    let out = compile("@link [target=_blank, rel=me] /x Home\n@iframe [sandbox, allow=camera] https://e.com");
    assert!(out.contains("target=\"_blank\""), "{}", out);
    assert!(out.contains("rel=\"me\""), "{}", out);
    assert!(out.contains(" sandbox"), "{}", out);
    assert!(out.contains("allow=\"camera\""), "{}", out);
}

#[test]
fn html_attribute_and_style_with_the_same_name_are_distinct() {
    let out = compile("@select [size=4, size 20]\n  @option A");
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
    let out = compile("@let n 5\n@if $n > 2 and not $missing\n  @text big");
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
    let result = parser::parse("@row [size 18, wrap, hidden]\n  @text x");
    assert!(
        result.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn data_glob_loads_each_file_under_its_stem() {
    let dir = scratch_dir("data_glob");
    std::fs::create_dir_all(dir.join("posts")).unwrap();
    std::fs::write(dir.join("posts/a.json"), r#"{"title": "First"}"#).unwrap();
    std::fs::write(dir.join("posts/b.json"), r#"{"title": "Second"}"#).unwrap();
    let out = compile_in(
        &dir,
        "@data $posts posts/*.json\n@text $posts._count: $posts.a.title, $posts.b.title\n@each $p in $posts\n  @text [class=item] $p",
    );
    assert!(out.contains("2: First, Second"), "{}", out);
    assert!(out.contains(">a<") && out.contains(">b<"), "{}", out);
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
    assert_eq!(messages.iter().filter(|m| m.contains("'alt'")).count(), 1, "{:?}", messages);
    assert!(messages.iter().any(|m| m.contains("empty container")), "{:?}", messages);
    assert!(messages.iter().any(|m| m.contains("'type'")), "{:?}", messages);
}

#[test]
fn inline_svg_resolves_from_the_page_and_keeps_attributes() {
    let dir = scratch_dir("inline_svg");
    std::fs::create_dir_all(dir.join("icons")).unwrap();
    std::fs::write(dir.join("icons/a.svg"), r#"<svg viewBox="0 0 24 24"></svg>"#).unwrap();
    let out = compile_in(&dir, "@image [inline, width 24, class=icon] icons/a.svg");
    assert!(out.contains(r#"<svg viewBox="0 0 24 24" width="24" class="icon">"#), "{}", out);
}
