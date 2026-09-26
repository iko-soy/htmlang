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
        out.contains("@layer hl-reset,htmlang;@layer hl-reset{"),
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
    assert!(out.contains("<a href=\"/more\">more</a>"), "{}", out);
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
        out.contains(r#"<svg viewBox="0 0 24 24" width="24" class="icon">"#),
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
fn page_takes_lang_and_favicon_only() {
    let result = parser::parse("@page [lang en, canonical https://x.dev] T\n");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("unknown @page attribute 'canonical'")),
        "{:?}",
        result.diagnostics
    );
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
    let src = "@let @a\n  @b\n  @nosuch\n@let @b\n  @text hi\n@b\n";
    let found = codes(src);
    assert!(has_code(src, 3, "unknown-element"), "{:?}", found);
    assert!(!found.iter().any(|(l, _, _)| *l == 2), "{:?}", found);
    // A loop over an empty list, and its `@else` when the list isn't empty
    let src = "@data $e []\n@each $x in $e\n  @bogus\n@each $y in 1..2\n  $y\n@else\n  @nope\n";
    assert!(has_code(src, 3, "unknown-element"), "{:?}", codes(src));
    assert!(has_code(src, 7, "unknown-element"), "{:?}", codes(src));
    // Inline elements in code that doesn't run warn, as they do when it runs
    let src = "@if false\n  Text with {@nosuch x}\n";
    assert!(
        codes(src).contains(&(2, "unknown-element", Severity::Warning)),
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
    let src = "@style .a { color: red; }\n";
    assert!(has_code(src, 1, "unexpected-argument"), "{:?}", codes(src));
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
    // Called before its `@let` has run: no "did you mean @b?", and `@b`
    // isn't reported unused
    let result = parser::parse("@let @a\n  @el\n    @b\n@a\n@let @b\n  @text hi\n");
    let unknown = result
        .diagnostics
        .iter()
        .find(|d| d.code == "unknown-element")
        .expect("@b is not defined when @a runs");
    assert!(
        unknown.message.contains("isn't defined yet"),
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
        ("Call {@f}\n@let @f\n  @el\n", "isn't defined yet"),
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
        "@let on = true\n@el [transition if($on, opacity 1s\\, color 1s, none), if($on, font-family Inter\\, serif, color red)] x",
    );
    assert!(out.contains("transition:opacity 1s, color 1s;"), "{}", out);
    // A whole-attribute branch is split into its name and value after choosing
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
