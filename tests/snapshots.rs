use std::fs;
use std::path::Path;

fn compile(input: &str) -> String {
    let result = htmlang::parser::parse(input);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "unexpected parse errors: {:?}",
        result.diagnostics
    );
    htmlang::codegen::generate(&result.document)
}

fn compile_with_base(input: &str, base: &Path) -> String {
    let result = htmlang::parser::parse_with_base(input, Some(base));
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "unexpected parse errors: {:?}",
        result.diagnostics
    );
    htmlang::codegen::generate(&result.document)
}

fn parse_diagnostics(input: &str) -> Vec<htmlang::parser::Diagnostic> {
    htmlang::parser::parse(input).diagnostics
}

fn snapshot_test(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let hl_path = dir.join(format!("{}.hl", name));
    let html_path = dir.join(format!("{}.html", name));

    let input = fs::read_to_string(&hl_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", hl_path.display(), e));
    let actual = compile_with_base(&input, &dir);

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        fs::write(&html_path, &actual).unwrap();
        return;
    }

    if !html_path.exists() {
        fs::write(&html_path, &actual).unwrap();
        eprintln!("created snapshot: {}", html_path.display());
        return;
    }

    let expected = fs::read_to_string(&html_path).unwrap();
    assert_eq!(actual, expected, "snapshot mismatch for {}", name);
}

#[test]
fn snapshot_basic_elements() {
    snapshot_test("basic_elements");
}

#[test]
fn snapshot_attributes() {
    snapshot_test("attributes");
}

#[test]
fn snapshot_alignment() {
    snapshot_test("alignment");
}

#[test]
fn snapshot_variables_defines() {
    snapshot_test("variables_defines");
}

#[test]
fn snapshot_functions() {
    snapshot_test("functions");
}

#[test]
fn snapshot_pseudo_states() {
    snapshot_test("pseudo_states");
}

#[test]
fn snapshot_chain_operator() {
    snapshot_test("chain_operator");
}

#[test]
fn snapshot_inline_text() {
    snapshot_test("inline_text");
}

#[test]
fn snapshot_raw_html() {
    snapshot_test("raw_html");
}

#[test]
fn snapshot_sizing() {
    snapshot_test("sizing");
}

#[test]
fn snapshot_no_page() {
    snapshot_test("no_page");
}

#[test]
fn snapshot_implicit_el() {
    snapshot_test("implicit_el");
}

#[test]
fn snapshot_responsive() {
    snapshot_test("responsive");
}

#[test]
fn snapshot_animations() {
    snapshot_test("animations");
}

#[test]
fn snapshot_css_vars() {
    snapshot_test("css_vars");
}

#[test]
fn snapshot_form_elements() {
    snapshot_test("form_elements");
}

#[test]
fn snapshot_conditionals() {
    snapshot_test("conditionals");
}

#[test]
fn snapshot_loops() {
    snapshot_test("loops");
}

#[test]
fn snapshot_accessibility() {
    snapshot_test("accessibility");
}

#[test]
fn snapshot_css_units() {
    snapshot_test("css_units");
}

#[test]
fn snapshot_positional() {
    snapshot_test("positional");
}

#[test]
fn snapshot_else_if() {
    snapshot_test("else_if");
}

#[test]
fn snapshot_meta_head() {
    snapshot_test("meta_head");
}

#[test]
fn snapshot_extra_css() {
    snapshot_test("extra_css");
}

#[test]
fn snapshot_fn_defaults() {
    snapshot_test("fn_defaults");
}

#[test]
fn snapshot_each_index() {
    snapshot_test("each_index");
}

#[test]
fn snapshot_style_block() {
    snapshot_test("style_block");
}

#[test]
fn snapshot_named_slots() {
    snapshot_test("named_slots");
}

#[test]
fn snapshot_each_range() {
    snapshot_test("each_range");
}

#[test]
fn snapshot_container_queries() {
    snapshot_test("container_queries");
}

// ---------------------------------------------------------------------------
// Error case tests
// ---------------------------------------------------------------------------

#[test]
fn error_unknown_element() {
    let diags = parse_diagnostics("@unknown");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unknown element @unknown")),
        "expected unknown element error, got: {:?}",
        diags
    );
}

#[test]
fn error_unknown_element_suggestion() {
    let diags = parse_diagnostics("@ro");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("did you mean @row")),
        "expected suggestion, got: {:?}",
        diags
    );
}

#[test]
fn error_unknown_attribute() {
    let diags = parse_diagnostics("@el [bakground red]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unknown attribute") && d.message.contains("background")),
        "expected unknown attribute with suggestion, got: {:?}",
        diags
    );
}

#[test]
fn error_unclosed_bracket() {
    let diags = parse_diagnostics("@el [padding 10");
    assert!(
        diags.iter().any(|d| d.message.contains("unclosed")),
        "expected unclosed bracket error, got: {:?}",
        diags
    );
}

#[test]
fn error_recursive_function() {
    let input = "@let loop\n  @loop\n@loop";
    let diags = parse_diagnostics(input);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("recursive function call")),
        "expected recursive function error, got: {:?}",
        diags
    );
}

#[test]
fn error_else_without_if() {
    let diags = parse_diagnostics("@else");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("@else without matching @if")),
        "expected @else error, got: {:?}",
        diags
    );
}

#[test]
fn error_each_bad_syntax() {
    let diags = parse_diagnostics("@each $x");
    assert!(
        diags.iter().any(|d| d.message.contains("@each requires")),
        "expected @each syntax error, got: {:?}",
        diags
    );
}

#[test]
fn error_numeric_validation() {
    let diags = parse_diagnostics("@el [padding abc]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("expects a numeric value")),
        "expected numeric validation warning, got: {:?}",
        diags
    );
}

#[test]
fn error_opacity_range() {
    let diags = parse_diagnostics("@el [opacity 2.0]");
    assert!(
        diags.iter().any(|d| d.message.contains("between 0 and 1")),
        "expected opacity range warning, got: {:?}",
        diags
    );
}

#[test]
fn warning_fill_outside_row() {
    let diags = parse_diagnostics("@column\n  @el [width fill]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("width fill") && d.message.contains("@row")),
        "expected fill context warning, got: {:?}",
        diags
    );
}

#[test]
fn error_else_if_without_if() {
    let diags = parse_diagnostics("@else if $x == 1");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("@else without matching @if")),
        "expected @else error, got: {:?}",
        diags
    );
}

#[test]
fn else_if_chain() {
    let output = compile(
        "@let x 2\n@if $x == 1\n  @text one\n@else if $x == 2\n  @text two\n@else\n  @text other",
    );
    assert!(output.contains("two"));
    assert!(!output.contains("one"));
    assert!(!output.contains("other"));
}

#[test]
fn fn_default_used() {
    let output = compile("@let test $x=hello\n  @text $x\n@test");
    assert!(output.contains("hello"));
}

#[test]
fn fn_default_overridden() {
    let output = compile("@let test $x=hello\n  @text $x\n@test [x world]");
    assert!(output.contains("world"));
    assert!(!output.contains("hello"));
}

#[test]
fn each_with_index() {
    let output = compile("@each $item, $i in a,b,c\n  @text $i");
    assert!(output.contains("0"));
    assert!(output.contains("1"));
    assert!(output.contains("2"));
}

#[test]
fn css_unit_passthrough() {
    let output = compile("@page T\n@el [width 50%, padding 2rem]");
    assert!(output.contains("width:50%"));
    assert!(output.contains("padding:2rem"));
}

#[test]
fn warning_fill_inside_row_ok() {
    let diags = parse_diagnostics("@row\n  @el [width fill]");
    assert!(
        !diags.iter().any(|d| d.message.contains("width fill")),
        "should not warn about width fill inside @row, got: {:?}",
        diags
    );
}

#[test]
fn if_condition_truthy() {
    let output = compile("@let x hello\n@if $x\n  @text visible");
    assert!(output.contains("visible"));
}

#[test]
fn if_condition_falsy() {
    let output = compile("@let x false\n@if $x\n  @text hidden");
    assert!(!output.contains("hidden"));
}

#[test]
fn each_loop_expansion() {
    let output = compile("@each $n in a,b,c\n  @text $n");
    assert!(output.contains("a"));
    assert!(output.contains("b"));
    assert!(output.contains("c"));
}

#[test]
fn aria_data_attrs_accepted() {
    let diags = parse_diagnostics("@el [aria-label=Test, data-id=42]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "aria-*/data-* should not produce warnings, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// New feature tests
// ---------------------------------------------------------------------------

// --- Unused variable/function/define warnings ---

#[test]
fn warning_unused_variable() {
    let diags = parse_diagnostics("@let color red\n@el [padding 10]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unused variable") && d.message.contains("color")),
        "expected unused variable warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_used_variable() {
    let diags = parse_diagnostics("@let color red\n@el [background $color]");
    assert!(
        !diags.iter().any(|d| d.message.contains("unused variable")),
        "should not warn about used variable, got: {:?}",
        diags
    );
}

#[test]
fn warning_unused_function() {
    let diags = parse_diagnostics("@let card\n  @el [padding 10]\n@el");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unused function") && d.message.contains("card")),
        "expected unused function warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_used_function() {
    let diags = parse_diagnostics("@let card\n  @el [padding 10]\n@card");
    assert!(
        !diags.iter().any(|d| d.message.contains("unused function")),
        "should not warn about used function, got: {:?}",
        diags
    );
}

#[test]
fn warning_unused_define() {
    let diags = parse_diagnostics("@let card-style [padding 10]\n@el");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unused attribute bundle")),
        "expected unused attribute bundle warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_used_define() {
    let diags = parse_diagnostics("@let card-style [padding 10]\n@el [$card-style]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unused attribute bundle")),
        "should not warn about used define, got: {:?}",
        diags
    );
}

// --- Element-specific attribute validation ---

#[test]
fn warning_spacing_on_text() {
    let diags = parse_diagnostics("@text [spacing 10] hello");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("spacing") && d.message.contains("no effect")),
        "expected spacing on @text warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_spacing_on_row() {
    let diags = parse_diagnostics("@row [spacing 10]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("spacing") && d.message.contains("no effect")),
        "should not warn about spacing on @row, got: {:?}",
        diags
    );
}

#[test]
fn warning_placeholder_on_row() {
    let diags = parse_diagnostics("@row [placeholder=test]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("placeholder") && d.message.contains("no effect")),
        "expected placeholder on @row warning, got: {:?}",
        diags
    );
}

#[test]
fn warning_for_on_non_label() {
    let diags = parse_diagnostics("@el [for=email]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("'for'") && d.message.contains("@label")),
        "expected 'for' on non-label warning, got: {:?}",
        diags
    );
}

// --- @each ranges ---

#[test]
fn each_range_basic() {
    let output = compile("@each $i in 1..3\n  @text $i");
    assert!(output.contains("1"));
    assert!(output.contains("2"));
    assert!(output.contains("3"));
}

#[test]
fn each_range_with_index() {
    let output = compile("@each $n, $i in 1..3\n  @text $i");
    assert!(output.contains("0"));
    assert!(output.contains("1"));
    assert!(output.contains("2"));
}

// --- Named slots ---

#[test]
fn named_slot_basic() {
    let output = compile(
        "@let card\n  @el\n    @slot header\n    @children\n@card\n  @slot header\n    @text Title\n  @text Body",
    );
    assert!(output.contains("Title"));
    assert!(output.contains("Body"));
}

#[test]
fn named_slot_default_content() {
    let output = compile(
        "@let card\n  @el\n    @slot header\n      @text Default\n    @children\n@card\n  @text Body",
    );
    assert!(output.contains("Default"));
    assert!(output.contains("Body"));
}

// --- @style block ---

#[test]
fn style_block_output() {
    let output = compile(
        "@page Test\n@style\n  .custom { color: red; }\n@el [class=custom]\n  @text styled",
    );
    assert!(output.contains(".custom{color:red;}") || output.contains(".custom { color: red; }"));
    assert!(output.contains("styled"));
}

// --- Container queries ---

#[test]
fn container_attr() {
    let output = compile("@page T\n@el [container]");
    assert!(output.contains("container-type:inline-size"));
}

#[test]
fn container_name_attr() {
    let output = compile("@page T\n@el [container-name sidebar]");
    assert!(output.contains("container-name:sidebar"));
}

// --- Variable scoping in @if ---

#[test]
fn if_block_scopes_variables() {
    let output = compile("@let x before\n@if true\n  @let x inside\n@text $x");
    // $x should be "before" outside the @if block
    assert!(output.contains("before"));
}

// --- CSS custom vars not warned as unused ---

#[test]
fn css_var_not_warned_unused() {
    let diags = parse_diagnostics("@let --primary blue");
    assert!(
        !diags.iter().any(|d| d.message.contains("unused")),
        "CSS vars should not be warned as unused, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Snapshot tests for new features
// ---------------------------------------------------------------------------

#[test]
fn snapshot_semantic_elements() {
    snapshot_test("semantic_elements");
}

#[test]
fn snapshot_list_elements() {
    snapshot_test("list_elements");
}

#[test]
fn snapshot_table_elements() {
    snapshot_test("table_elements");
}

#[test]
fn snapshot_media_elements() {
    snapshot_test("media_elements");
}

#[test]
fn snapshot_match_directive() {
    snapshot_test("match_directive");
}

#[test]
fn snapshot_new_css_attrs() {
    snapshot_test("new_css_attrs");
}

// ---------------------------------------------------------------------------
// Feature tests: semantic elements
// ---------------------------------------------------------------------------

#[test]
fn semantic_nav_renders_nav_tag() {
    let output = compile("@page T\n@nav\n  @text Links");
    assert!(output.contains("<nav"));
    assert!(output.contains("</nav>"));
}

#[test]
fn semantic_header_renders_header_tag() {
    let output = compile("@page T\n@header\n  @text Title");
    assert!(output.contains("<header"));
    assert!(output.contains("</header>"));
}

#[test]
fn semantic_footer_renders_footer_tag() {
    let output = compile("@page T\n@footer\n  @text Footer");
    assert!(output.contains("<footer"));
    assert!(output.contains("</footer>"));
}

#[test]
fn semantic_main_renders_main_tag() {
    let output = compile("@page T\n@main\n  @text Content");
    assert!(output.contains("<main"));
    assert!(output.contains("</main>"));
}

#[test]
fn semantic_section_renders_section_tag() {
    let output = compile("@page T\n@section\n  @text Section");
    assert!(output.contains("<section"));
    assert!(output.contains("</section>"));
}

#[test]
fn semantic_article_renders_article_tag() {
    let output = compile("@page T\n@article\n  @text Article");
    assert!(output.contains("<article"));
    assert!(output.contains("</article>"));
}

#[test]
fn semantic_aside_renders_aside_tag() {
    let output = compile("@page T\n@aside\n  @text Sidebar");
    assert!(output.contains("<aside"));
    assert!(output.contains("</aside>"));
}

// ---------------------------------------------------------------------------
// Feature tests: list elements
// ---------------------------------------------------------------------------

#[test]
fn list_renders_ul_by_default() {
    let output = compile("@page T\n@list\n  @item Hello");
    assert!(output.contains("<ul"));
    assert!(output.contains("<li"));
    assert!(output.contains("Hello"));
}

#[test]
fn list_renders_ol_with_ordered() {
    let output = compile("@page T\n@list [ordered]\n  @item First\n  @item Second");
    assert!(output.contains("<ol"));
    assert!(output.contains("<li"));
}

#[test]
fn removed_aliases_point_to_upgrade() {
    for (src, replacement) in [
        ("@li Works", "@item"),
        ("@divider", "@hr"),
        ("@el > @col x", "@column"),
        ("Say {@img a.png}", "@image"),
    ] {
        let diags = parse_diagnostics(src);
        assert!(
            diags.iter().any(|d| d.message.contains(replacement)
                && d.message.contains("htmlang upgrade")),
            "{:?} should point to {}: {:?}",
            src,
            replacement,
            diags
        );
    }
}

#[test]
fn removed_directives_point_to_upgrade() {
    for src in [
        "@unless $x\n  hi",
        "@for $i in 1..2\n  $i",
        "@repeat 2\n  hi",
        "@switch $v\n  @case a\n    hi",
        "@use \"a.hl\" x",
        "@import a.hl",
        "@with $a as b\n  hi",
        "@layout base.hl\n  hi",
        "@scope .c\n  .t {}",
        "@starting-style\n  .t {}",
        "@css-property --x\n  syntax \"*\"",
        "@fn card $t\n  @text $t",
        "@define c [bold]",
    ] {
        let diags = parse_diagnostics(src);
        assert!(
            diags.iter().any(|d| d.message.contains("was removed")
                && d.message.contains("htmlang upgrade")),
            "{:?} should report removed syntax: {:?}",
            src,
            diags
        );
    }
}

#[test]
fn user_function_may_reuse_a_removed_name() {
    let output = compile("@page T\n@let divider\n  @hr\n@divider");
    assert!(output.contains("<hr"), "{}", output);
}

// ---------------------------------------------------------------------------
// Feature tests: table elements
// ---------------------------------------------------------------------------

#[test]
fn table_renders_proper_tags() {
    let output = compile(
        "@page T\n@table\n  @thead\n    @tr\n      @th Header\n  @tbody\n    @tr\n      @td Cell",
    );
    assert!(output.contains("<table"));
    assert!(output.contains("<thead"));
    assert!(output.contains("<tbody"));
    assert!(output.contains("<tr"));
    assert!(output.contains("<th"));
    assert!(output.contains("<td"));
    assert!(output.contains("Header"));
    assert!(output.contains("Cell"));
}

// ---------------------------------------------------------------------------
// Feature tests: media elements
// ---------------------------------------------------------------------------

#[test]
fn video_renders_with_src() {
    let output = compile("@page T\n@video [controls] demo.mp4");
    assert!(output.contains("<video"));
    assert!(output.contains("src=\"demo.mp4\""));
    assert!(output.contains("controls"));
    assert!(output.contains("</video>"));
}

#[test]
fn audio_renders_with_src() {
    let output = compile("@page T\n@audio [controls] song.mp3");
    assert!(output.contains("<audio"));
    assert!(output.contains("src=\"song.mp3\""));
    assert!(output.contains("controls"));
}

#[test]
fn video_with_multiple_attrs() {
    let output = compile("@page T\n@video [controls, muted, autoplay, loop] clip.mp4");
    assert!(output.contains("controls"));
    assert!(output.contains("muted"));
    assert!(output.contains("autoplay"));
    assert!(output.contains("loop"));
}

// ---------------------------------------------------------------------------
// Feature tests: @match directive
// ---------------------------------------------------------------------------

#[test]
fn match_selects_correct_case() {
    let output = compile(
        "@let x b\n@if $x == \"a\"\n  @text A\n@else if $x == \"b\"\n  @text B\n@else\n  @text D",
    );
    assert!(output.contains("B"));
    assert!(!output.contains(">A<"));
    assert!(!output.contains(">D<"));
}

#[test]
fn match_falls_to_default() {
    let output =
        compile("@let x z\n@if $x == \"a\"\n  @text A\n@else\n  @text Default");
    assert!(output.contains("Default"));
    assert!(!output.contains(">A<"));
}

#[test]
fn match_no_match_no_default() {
    let output = compile("@let x z\n@if $x == \"a\"\n  @text A\n@else if $x == \"b\"\n  @text B");
    assert!(!output.contains(">A<"));
    assert!(!output.contains(">B<"));
}

// ---------------------------------------------------------------------------
// Feature tests: new CSS attributes
// ---------------------------------------------------------------------------

#[test]
fn css_aspect_ratio() {
    let output = compile("@page T\n@el [aspect-ratio 16/9]");
    assert!(output.contains("aspect-ratio:16/9"));
}

#[test]
fn css_outline() {
    let output = compile("@page T\n@el [outline 2 red]");
    assert!(output.contains("outline:2px solid red"));
}

#[test]
fn css_outline_no_color() {
    let output = compile("@page T\n@el [outline 3]");
    assert!(output.contains("outline:3px solid currentColor"));
}

#[test]
fn css_padding_inline() {
    let output = compile("@page T\n@el [padding-inline 20]");
    assert!(output.contains("padding-inline:20px"));
}

#[test]
fn css_padding_block() {
    let output = compile("@page T\n@el [padding-block 10]");
    assert!(output.contains("padding-block:10px"));
}

#[test]
fn css_margin_inline() {
    let output = compile("@page T\n@el [margin-inline 20]");
    assert!(output.contains("margin-inline:20px"));
}

#[test]
fn css_margin_block() {
    let output = compile("@page T\n@el [margin-block 10]");
    assert!(output.contains("margin-block:10px"));
}

#[test]
fn css_scroll_snap_type() {
    let output = compile("@page T\n@el [scroll-snap-type x mandatory]");
    assert!(output.contains("scroll-snap-type:x mandatory"));
}

#[test]
fn css_scroll_snap_align() {
    let output = compile("@page T\n@el [scroll-snap-align center]");
    assert!(output.contains("scroll-snap-align:center"));
}

// ---------------------------------------------------------------------------
// Feature tests: @warn / @debug
// ---------------------------------------------------------------------------

#[test]
fn warn_produces_diagnostic() {
    let diags = parse_diagnostics("@warn Something is wrong");
    assert!(
        diags.iter().any(|d| d.message == "Something is wrong"
            && d.severity == htmlang::parser::Severity::Warning),
        "expected @warn diagnostic, got: {:?}",
        diags
    );
}

#[test]
fn warn_substitutes_variables() {
    let diags = parse_diagnostics("@let name test\n@warn Missing $name value");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Missing test value")),
        "expected substituted @warn, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Feature tests: image optimization hints
// ---------------------------------------------------------------------------

#[test]
fn image_auto_lazy_loading() {
    // First 3 images get fetchpriority="high" (above-the-fold), subsequent get loading="lazy"
    let output = compile(
        "@page T\n@image https://example.com/1.jpg\n@image https://example.com/2.jpg\n@image https://example.com/3.jpg\n@image https://example.com/4.jpg",
    );
    // First 3 images: fetchpriority="high", no lazy loading
    assert!(output.contains("fetchpriority=\"high\""));
    // 4th image: loading="lazy" + decoding="async"
    assert!(output.contains("loading=\"lazy\""));
    assert!(output.contains("decoding=\"async\""));
}

#[test]
fn image_explicit_loading_not_doubled() {
    let output = compile("@page T\n@image [loading=eager] https://example.com/photo.jpg");
    assert!(output.contains("loading=\"eager\""));
    assert!(!output.contains("loading=\"lazy\""));
}

// ---------------------------------------------------------------------------
// Feature tests: new improvements
// ---------------------------------------------------------------------------

#[test]
fn ternary_expression_in_attrs() {
    let output = compile("@page T\n@let active true\n@el [color if($active, green, gray)]\n  test");
    assert!(output.contains("color:green"));
}

#[test]
fn ternary_expression_false() {
    let output = compile("@page T\n@let active false\n@el [color if($active, green, gray)]\n  test");
    assert!(output.contains("color:gray"));
}

#[test]
fn multiline_let_triple_quotes() {
    let output = compile("@page T\n@let bio \"\"\"Hello World\"\"\"\n@text $bio");
    assert!(output.contains("Hello World"));
}

#[test]
fn comparison_operators_gt() {
    let output = compile("@page T\n@let count 5\n@if $count > 3\n  @text big");
    assert!(output.contains("big"));
}

#[test]
fn comparison_operators_lt() {
    let output = compile("@page T\n@let count 2\n@if $count < 3\n  @text small");
    assert!(output.contains("small"));
}

#[test]
fn comparison_operators_contains() {
    let output = compile("@page T\n@let name hello world\n@if $name contains world\n  @text found");
    assert!(output.contains("found"));
}

#[test]
fn comparison_operators_starts_with() {
    let output = compile(
        "@page T\n@let url https://example.com\n@if $url starts-with https\n  @text secure",
    );
    assert!(output.contains("secure"));
}

#[test]
fn string_interpolation_joins_values() {
    let output = compile(
        "@page T\n@let first Hello\n@let last World\n@let full \"$first $last\"\n@text $full",
    );
    assert!(output.contains("Hello World"), "got: {}", output);
}

#[test]
fn css_contain_attribute() {
    let output = compile("@page T\n@el [contain layout, width 200, height 100]\n  test");
    assert!(output.contains("contain:layout"));
}

#[test]
fn css_contain_default() {
    let output = compile("@page T\n@el [contain, width 200]\n  test");
    assert!(output.contains("contain:layout style paint"));
}

#[test]
fn content_visibility_attribute() {
    let output = compile("@page T\n@el [content-visibility auto]\n  test");
    assert!(output.contains("content-visibility:auto"));
}

#[test]
fn focus_visible_css_with_interactive() {
    let output = compile("@page T\n@button Click");
    assert!(output.contains("focus-visible"));
}

#[test]
fn focus_visible_css_without_interactive() {
    let output = compile("@page T\n@el\n  text");
    assert!(!output.contains("focus-visible"));
}

#[test]
fn skip_to_content_with_main() {
    let output = compile("@page T\n@main\n  content");
    assert!(output.contains("hl-skip"));
    assert!(output.contains("hl-main"));
    assert!(output.contains("Skip to content"));
}

#[test]
fn no_skip_to_content_without_main() {
    let output = compile("@page T\n@el\n  content");
    assert!(!output.contains("hl-skip"));
}

#[test]
fn external_link_noopener() {
    let output = compile("@page T\n@link https://example.com\n  External");
    assert!(output.contains("rel=\"noopener noreferrer\""));
    assert!(output.contains("target=\"_blank\""));
}

#[test]
fn internal_link_no_noopener() {
    let output = compile("@page T\n@link /about\n  About");
    assert!(!output.contains("noopener"));
    assert!(!output.contains("target=\"_blank\""));
}

#[test]
fn theme_color_meta_from_theme() {
    let output = compile("@page T\n@let primary #3b82f6\n@let --primary #3b82f6\n@meta theme-color #3b82f6\n@el\n  test");
    assert!(output.contains("theme-color"));
    assert!(output.contains("#3b82f6"));
}

#[test]
fn aria_live_passthrough() {
    let output = compile("@page T\n@el [aria-live=polite]\n  updating");
    assert!(output.contains("aria-live=\"polite\""));
}

#[test]
fn defer_directive_was_removed() {
    let diags = parse_diagnostics("@page T\n@defer\n  @el\n    Lazy content");
    assert!(
        diags.iter().any(|d| d.message.contains("`@defer` was removed")),
        "{:?}",
        diags
    );
}

#[test]
fn comparison_gte_lte() {
    let output = compile("@page T\n@let x 5\n@if $x >= 5\n  @text gte\n@if $x <= 5\n  @text lte");
    assert!(output.contains("gte"));
    assert!(output.contains("lte"));
}

#[test]
fn comparison_ends_with() {
    let output = compile("@page T\n@let file photo.jpg\n@if $file ends-with .jpg\n  @text image");
    assert!(output.contains("image"));
}

// ---------------------------------------------------------------------------
// Feature tests: element-specific attribute validation
// ---------------------------------------------------------------------------

#[test]
fn warning_ordered_on_non_list() {
    let diags = parse_diagnostics("@el [ordered]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("ordered") && d.message.contains("@list")),
        "expected ordered on non-list warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_ordered_on_list() {
    let diags = parse_diagnostics("@list [ordered]\n  @item x");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("ordered") && d.message.contains("no effect")),
        "should not warn about ordered on @list, got: {:?}",
        diags
    );
}

#[test]
fn warning_controls_on_non_media() {
    let diags = parse_diagnostics("@el [controls]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("controls") && d.message.contains("@video")),
        "expected controls on non-media warning, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Feature tests: formatter
// ---------------------------------------------------------------------------

#[test]
fn fmt_normalizes_indentation() {
    let input = "@row\n      @column\n            @text hello\n      @column\n            @text world";
    let formatted = htmlang::fmt::format(input);
    assert_eq!(
        formatted,
        "@row\n  @column\n    @text hello\n  @column\n    @text world\n"
    );
}

#[test]
fn fmt_preserves_blank_lines() {
    let input = "@page Test\n\n@row\n    @text hello";
    let formatted = htmlang::fmt::format(input);
    assert_eq!(formatted, "@page Test\n\n@row\n  @text hello\n");
}

// ---------------------------------------------------------------------------
// Feature tests: string interpolation (already exists, verify)
// ---------------------------------------------------------------------------

#[test]
fn string_interpolation_in_text() {
    let output = compile("@let name World\n@text Hello $name!");
    assert!(output.contains("Hello World!"));
}

#[test]
fn string_interpolation_in_bare_text() {
    let output = compile("@let greeting Hi\n@el\n  $greeting there");
    assert!(output.contains("Hi there"));
}

// ---------------------------------------------------------------------------
// Snapshot tests for batch 3 features
// ---------------------------------------------------------------------------

#[test]
fn snapshot_new_elements() {
    snapshot_test("new_elements");
}

#[test]
fn snapshot_dark_mode() {
    snapshot_test("dark_mode");
}

#[test]
fn snapshot_new_css() {
    snapshot_test("new_css");
}

#[test]
fn snapshot_string_interpolation() {
    snapshot_test("string_interpolation");
}

#[test]
fn snapshot_each_destructuring() {
    snapshot_test("each_destructuring");
}

// ---------------------------------------------------------------------------
// Feature tests: new elements
// ---------------------------------------------------------------------------

#[test]
fn form_renders_form_tag() {
    let output = compile("@page T\n@form [method=post] /submit\n  @input [type=text]");
    assert!(output.contains("<form"));
    assert!(output.contains("action=\"/submit\""));
    assert!(output.contains("method=\"post\""));
}

#[test]
fn details_summary_renders() {
    let output = compile("@page T\n@details [open]\n  @summary Click me\n  @text Content");
    assert!(output.contains("<details"));
    assert!(output.contains(" open"));
    assert!(output.contains("<summary"));
    assert!(output.contains("Click me"));
}

#[test]
fn blockquote_renders() {
    let output = compile("@page T\n@blockquote\n  @text A quote\n  @cite Source");
    assert!(output.contains("<blockquote"));
    assert!(output.contains("<cite"));
    assert!(output.contains("Source"));
}

#[test]
fn code_renders_monospace() {
    let output = compile("@page T\n@code hello");
    assert!(output.contains("<code"));
    assert!(output.contains("font-family:ui-monospace,monospace"));
}

#[test]
fn pre_renders_with_whitespace() {
    let output = compile("@page T\n@pre\n  @text formatted");
    assert!(output.contains("<pre"));
    assert!(output.contains("white-space:pre"));
}

#[test]
fn hr_renders_self_closing() {
    let output = compile("@page T\n@hr");
    assert!(output.contains("<hr"));
}

#[test]
fn figure_figcaption_renders() {
    let output = compile("@page T\n@figure\n  @image [alt=test] photo.jpg\n  @figcaption Caption");
    assert!(output.contains("<figure"));
    assert!(output.contains("<figcaption"));
    assert!(output.contains("Caption"));
}

#[test]
fn progress_renders() {
    let output = compile("@page T\n@progress [value=70, max=100]");
    assert!(output.contains("<progress"));
    assert!(output.contains("value=\"70\""));
    assert!(output.contains("max=\"100\""));
}

#[test]
fn meter_renders() {
    let output = compile("@page T\n@meter [value=6, min=0, max=10]");
    assert!(output.contains("<meter"));
    assert!(output.contains("value=\"6\""));
}

// ---------------------------------------------------------------------------
// Feature tests: dark mode
// ---------------------------------------------------------------------------

#[test]
fn dark_mode_generates_media_query() {
    let output = compile("@page T\n@el [background white, dark:background #333]");
    assert!(output.contains("prefers-color-scheme:dark"));
    assert!(output.contains("background:#333"));
}

#[test]
fn print_generates_media_query() {
    let output = compile("@page T\n@el [display flex, print:display none]");
    assert!(output.contains("@media print"));
    assert!(output.contains("display:none"));
}

// ---------------------------------------------------------------------------
// Feature tests: new CSS attributes
// ---------------------------------------------------------------------------

#[test]
fn css_margin() {
    let output = compile("@page T\n@el [margin 20]");
    assert!(output.contains("margin:20px"));
}

#[test]
fn css_margin_x() {
    // margin-x emits the `margin-inline` logical shorthand (single property,
    // covers both inline sides, stays symmetric under RTL).
    let output = compile("@page T\n@el [margin-x 10]");
    assert!(output.contains("margin-inline:10px"));
}

#[test]
fn css_margin_y() {
    let output = compile("@page T\n@el [margin-y 10]");
    assert!(output.contains("margin-block:10px"));
}

#[test]
fn css_filter() {
    let output = compile("@page T\n@el [filter blur(5px)]");
    assert!(output.contains("filter:blur(5px)"));
}

#[test]
fn css_object_fit() {
    let output = compile("@page T\n@image [object-fit cover, width 200] test.jpg");
    assert!(output.contains("object-fit:cover"));
}

#[test]
fn css_text_shadow() {
    let output = compile("@page T\n@text [text-shadow 1px 1px black] Hello");
    assert!(output.contains("text-shadow:1px 1px black"));
}

#[test]
fn css_text_overflow() {
    let output = compile("@page T\n@text [text-overflow ellipsis] Hello");
    assert!(output.contains("text-overflow:ellipsis"));
}

#[test]
fn css_pointer_events() {
    let output = compile("@page T\n@el [pointer-events none]");
    assert!(output.contains("pointer-events:none"));
}

#[test]
fn css_user_select() {
    let output = compile("@page T\n@el [user-select none]");
    assert!(output.contains("user-select:none"));
}

#[test]
fn css_justify_content() {
    let output = compile("@page T\n@row [justify-content space-between]");
    assert!(output.contains("justify-content:space-between"));
}

#[test]
fn css_align_items() {
    let output = compile("@page T\n@row [align-items center]");
    assert!(output.contains("align-items:center"));
}

#[test]
fn css_order() {
    let output = compile("@page T\n@el [order 2]");
    assert!(output.contains("order:2"));
}

#[test]
fn css_background_size() {
    let output = compile("@page T\n@el [background-size cover]");
    assert!(output.contains("background-size:cover"));
}

#[test]
fn css_word_break() {
    let output = compile("@page T\n@el [word-break break-all]");
    assert!(output.contains("word-break:break-all"));
}

#[test]
fn css_overflow_wrap() {
    let output = compile("@page T\n@el [overflow-wrap break-word]");
    assert!(output.contains("overflow-wrap:break-word"));
}

// ---------------------------------------------------------------------------
// Feature tests: string interpolation
// ---------------------------------------------------------------------------

#[test]
fn let_string_interpolation() {
    let output = compile("@let name World\n@let greeting \"Hello $name\"\n@text $greeting");
    assert!(output.contains("Hello World"));
}

// ---------------------------------------------------------------------------
// Feature tests: @each destructuring
// ---------------------------------------------------------------------------

#[test]
fn each_destructuring_pairs() {
    let output = compile("@each $name, $role in Alice Admin, Bob User\n  @text $name is $role");
    assert!(output.contains("Alice is Admin"));
    assert!(output.contains("Bob is User"));
}

// ---------------------------------------------------------------------------
// Feature tests: diagnostics
// ---------------------------------------------------------------------------

#[test]
fn warning_missing_alt_on_image() {
    let diags = parse_diagnostics("@image photo.jpg");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("alt") && d.message.contains("accessibility")),
        "expected missing alt warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_with_alt_on_image() {
    let diags = parse_diagnostics("@image [alt=A photo] photo.jpg");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("missing") && d.message.contains("alt")),
        "should not warn when alt is present, got: {:?}",
        diags
    );
}

#[test]
fn warning_invalid_hex_color() {
    let diags = parse_diagnostics("@el [color #ggg]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("invalid hex color")),
        "expected invalid hex color warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_valid_hex_color() {
    let diags = parse_diagnostics("@el [color #ff0000]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("invalid hex color")),
        "should not warn on valid hex color, got: {:?}",
        diags
    );
}

#[test]
fn warning_duplicate_attribute() {
    let diags = parse_diagnostics("@el [padding 10, padding 20]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("duplicate attribute")),
        "expected duplicate attribute warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_different_attributes() {
    let diags = parse_diagnostics("@el [padding 10, margin 20]");
    assert!(
        !diags.iter().any(|d| d.message.contains("duplicate")),
        "should not warn on different attributes, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Feature tests: formatter improvements
// ---------------------------------------------------------------------------

#[test]
fn fmt_preserves_attribute_order() {
    // Later attributes may override earlier ones, so order is significant.
    let formatted = htmlang::fmt::format("@el [color red,width 200,  padding 10]");
    assert!(formatted.contains("[color red, width 200, padding 10]"));
}

#[test]
fn fmt_multiline_brackets() {
    let input = "@el [\n  color red,\n  width 200\n]\n  @text hello";
    let formatted = htmlang::fmt::format(input);
    assert!(formatted.contains("[color red, width 200]"));
    assert!(formatted.contains("  @text hello"));
}

#[test]
fn fmt_examples_idempotent_and_semantics_preserving() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "hl") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let once = htmlang::fmt::format(&src);
        let twice = htmlang::fmt::format(&once);
        assert_eq!(once, twice, "formatter not idempotent on {}", path.display());
        assert_eq!(
            compile_with_base(&src, &dir),
            compile_with_base(&once, &dir),
            "formatting changed the output of {}",
            path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// New features: pseudo-states, child selectors, fragment, hidden, CSS props
// ---------------------------------------------------------------------------

#[test]
fn snapshot_pseudo_states_extended() {
    snapshot_test("pseudo_states_extended");
}

#[test]
fn snapshot_fragment() {
    snapshot_test("fragment");
}

#[test]
fn snapshot_hidden_attr() {
    snapshot_test("hidden_attr");
}

#[test]
fn snapshot_new_css_properties() {
    snapshot_test("new_css_properties");
}

#[test]
fn snapshot_lang_directive() {
    snapshot_test("lang_directive");
}

// --- Assertion tests for new pseudo-states ---

#[test]
fn focus_visible_generates_pseudo() {
    let output = compile("@page T\n@el [focus-visible:border 2 blue]");
    assert!(output.contains(":focus-visible"));
    assert!(output.contains("border:2px solid blue"));
}

#[test]
fn focus_within_generates_pseudo() {
    let output = compile("@page T\n@el [focus-within:background #eee]");
    assert!(output.contains(":focus-within"));
    assert!(output.contains("background:#eee"));
}

#[test]
fn disabled_generates_pseudo() {
    let output = compile("@page T\n@el [disabled:opacity 0.5]");
    assert!(output.contains(":disabled"));
    assert!(output.contains("opacity:0.5"));
}

#[test]
fn checked_generates_pseudo() {
    let output = compile("@page T\n@el [checked:background green]");
    assert!(output.contains(":checked"));
    assert!(output.contains("background:green"));
}

#[test]
fn placeholder_generates_pseudo() {
    let output = compile("@page T\n@input [type=text, placeholder:color #999]");
    assert!(output.contains("::placeholder"));
    assert!(output.contains("color:#999"));
}

#[test]
fn first_child_generates_pseudo() {
    let output = compile("@page T\n@el [first:border-top 0]");
    assert!(output.contains(":first-child"));
}

#[test]
fn last_child_generates_pseudo() {
    let output = compile("@page T\n@el [last:border-bottom 0]");
    assert!(output.contains(":last-child"));
}

#[test]
fn odd_generates_pseudo() {
    let output = compile("@page T\n@el [odd:background #f5f5f5]");
    assert!(output.contains(":nth-child(odd)"));
}

#[test]
fn even_generates_pseudo() {
    let output = compile("@page T\n@el [even:background white]");
    assert!(output.contains(":nth-child(even)"));
}

// --- Assertion tests for fragment ---

#[test]
fn fragment_no_wrapper() {
    let output = compile("@page T\n@column\n  @fragment\n    @text A\n    @text B");
    // Fragment should NOT add any div wrapper
    assert!(!output.contains("<div class=\"_1\"><span"));
    // But children should still be present
    assert!(output.contains("A"));
    assert!(output.contains("B"));
}

// --- Assertion tests for hidden ---

#[test]
fn hidden_generates_display_none() {
    let output = compile("@page T\n@el [hidden]");
    assert!(output.contains("display:none"));
}

// --- Assertion tests for new CSS properties ---

#[test]
fn css_overflow_x() {
    let output = compile("@page T\n@el [overflow-x hidden]");
    assert!(output.contains("overflow-x:hidden"));
}

#[test]
fn css_overflow_y() {
    let output = compile("@page T\n@el [overflow-y auto]");
    assert!(output.contains("overflow-y:auto"));
}

#[test]
fn css_inset() {
    let output = compile("@page T\n@el [inset 0]");
    assert!(output.contains("inset:0"));
}

#[test]
fn css_accent_color() {
    let output = compile("@page T\n@input [type=checkbox, accent-color blue]");
    assert!(output.contains("accent-color:blue"));
}

#[test]
fn css_caret_color() {
    let output = compile("@page T\n@input [type=text, caret-color red]");
    assert!(output.contains("caret-color:red"));
}

#[test]
fn css_list_style() {
    let output = compile("@page T\n@list [list-style disc]");
    assert!(output.contains("list-style:disc"));
}

#[test]
fn css_border_collapse() {
    let output = compile("@page T\n@table [border-collapse collapse]");
    assert!(output.contains("border-collapse:collapse"));
}

#[test]
fn css_text_decoration_full() {
    let output = compile(
        "@page T\n@text [text-decoration underline, text-decoration-color red, text-decoration-style wavy] Hello",
    );
    assert!(output.contains("text-decoration:underline"));
    assert!(output.contains("text-decoration-color:red"));
    assert!(output.contains("text-decoration-style:wavy"));
}

#[test]
fn css_place_items() {
    let output = compile("@page T\n@el [display grid, place-items center]");
    assert!(output.contains("place-items:center"));
}

#[test]
fn css_place_self() {
    let output = compile("@page T\n@el [place-self center]");
    assert!(output.contains("place-self:center"));
}

#[test]
fn css_scroll_behavior() {
    let output = compile("@page T\n@el [scroll-behavior smooth]");
    assert!(output.contains("scroll-behavior:smooth"));
}

#[test]
fn css_resize() {
    let output = compile("@page T\n@textarea [resize vertical]");
    assert!(output.contains("resize:vertical"));
}

// --- Assertion tests for @lang ---

#[test]
fn lang_sets_html_attr() {
    let output = compile("@page [lang en] T\n@text Hello");
    assert!(output.contains("<html lang=\"en\">"));
}

#[test]
fn lang_not_present_without_directive() {
    let output = compile("@page T\n@text Hello");
    assert!(output.contains("<html>"));
    assert!(!output.contains("lang="));
}

// --- Assertion test for @favicon ---

#[test]
fn favicon_fallback_href() {
    // Nonexistent file should fall back to href
    let output = compile("@page [favicon nonexistent.png] T\n@text Hello");
    assert!(output.contains("<link rel=\"icon\" href=\"nonexistent.png\">"));
}

// --- No warnings for new attrs ---

#[test]
fn no_warning_new_pseudo_prefixes() {
    let diags = parse_diagnostics(
        "@el [focus-visible:border 2 blue, disabled:opacity 0.5, checked:background green]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new pseudo-state prefixes should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_child_selectors() {
    let diags = parse_diagnostics(
        "@el [first:padding 0, last:padding 0, odd:background #eee, even:background white]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "child selectors should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_new_css_attrs() {
    let diags = parse_diagnostics(
        "@el [overflow-x hidden, overflow-y auto, inset 0, accent-color blue, hidden]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new CSS attrs should be recognized, got: {:?}",
        diags
    );
}

// =========================================================================
// New elements snapshot
// =========================================================================

#[test]
fn snapshot_new_elements_2() {
    snapshot_test("new_elements_2");
}

// =========================================================================
// New CSS properties snapshot
// =========================================================================

#[test]
fn snapshot_new_css_properties_2() {
    snapshot_test("new_css_properties_2");
}

// =========================================================================
// Extended media queries snapshot
// =========================================================================

#[test]
fn snapshot_media_extended() {
    snapshot_test("media_extended");
}

// =========================================================================
// @unless directive snapshot
// =========================================================================

#[test]
fn snapshot_unless_directive() {
    snapshot_test("unless_directive");
}

// =========================================================================
// @og directive snapshot
// =========================================================================

#[test]
fn snapshot_og_directive() {
    snapshot_test("og_directive");
}

// =========================================================================
// Arithmetic in @let snapshot
// =========================================================================

#[test]
fn snapshot_arithmetic() {
    snapshot_test("arithmetic");
}

// =========================================================================
// New element assertion tests
// =========================================================================

#[test]
fn element_dialog() {
    let output = compile("@page T\n@dialog [id=modal, open]\n  @text Hello");
    assert!(output.contains("<dialog"));
    assert!(output.contains("id=\"modal\""));
    assert!(output.contains("open"));
    assert!(output.contains("</dialog>"));
}

#[test]
fn element_definition_list() {
    let output = compile("@page T\n@dl\n  @dt Term\n  @dd Definition");
    assert!(output.contains("<dl"));
    assert!(output.contains("<dt>Term</dt>"));
    assert!(output.contains("<dd"));
    assert!(output.contains(">Definition</dd>"));
}

#[test]
fn element_fieldset_legend() {
    let output = compile("@page T\n@fieldset\n  @legend Info\n  @input [type=text, name=n]");
    assert!(output.contains("<fieldset"));
    assert!(output.contains("<legend>Info</legend>"));
    assert!(output.contains("</fieldset>"));
}

#[test]
fn element_picture_source() {
    let output = compile(
        "@page T\n@picture\n  @source [srcset=wide.jpg, media=(min-width: 800px)]\n  @image [alt=Photo] photo.jpg",
    );
    assert!(output.contains("<picture"));
    assert!(output.contains("<source"));
    assert!(output.contains("srcset=\"wide.jpg\""));
    assert!(output.contains("</picture>"));
}

#[test]
fn element_time() {
    let output = compile("@page T\n@time [datetime=2026-04-15] April 15");
    assert!(output.contains("<time"));
    assert!(output.contains("datetime=\"2026-04-15\""));
    assert!(output.contains("April 15"));
    assert!(output.contains("</time>"));
}

#[test]
fn element_mark() {
    let output = compile("@page T\n@mark highlighted");
    assert!(output.contains("<mark>highlighted</mark>"));
}

#[test]
fn element_kbd() {
    let output = compile("@page T\n@kbd Ctrl+C");
    assert!(output.contains("<kbd"));
    assert!(output.contains("Ctrl+C</kbd>"));
}

#[test]
fn element_abbr() {
    let output = compile("@page T\n@abbr [title=HyperText Markup Language] HTML");
    assert!(output.contains("<abbr"));
    assert!(output.contains("title=\"HyperText Markup Language\""));
    assert!(output.contains("HTML"));
}

#[test]
fn element_datalist() {
    let output = compile("@page T\n@datalist [id=browsers]\n  @option Chrome\n  @option Firefox");
    assert!(output.contains("<datalist"));
    assert!(output.contains("id=\"browsers\""));
    assert!(output.contains("</datalist>"));
}

// =========================================================================
// New CSS property assertion tests
// =========================================================================

#[test]
fn css_clip_path() {
    let output = compile("@page T\n@el [clip-path circle(50%)]");
    assert!(output.contains("clip-path:circle(50%)"));
}

#[test]
fn css_mix_blend_mode() {
    let output = compile("@page T\n@el [mix-blend-mode multiply]");
    assert!(output.contains("mix-blend-mode:multiply"));
}

#[test]
fn css_background_blend_mode() {
    let output = compile("@page T\n@el [background-blend-mode overlay]");
    assert!(output.contains("background-blend-mode:overlay"));
}

#[test]
fn css_writing_mode() {
    let output = compile("@page T\n@el [writing-mode vertical-rl]");
    assert!(output.contains("writing-mode:vertical-rl"));
}

#[test]
fn css_column_count() {
    let output = compile("@page T\n@column [column-count 3]");
    assert!(output.contains("column-count:3"));
}

#[test]
fn css_column_gap() {
    let output = compile("@page T\n@column [column-gap 20]");
    assert!(output.contains("column-gap:20px"));
}

#[test]
fn css_text_indent() {
    let output = compile("@page T\n@paragraph [text-indent 2em]");
    assert!(output.contains("text-indent:2em"));
}

#[test]
fn css_hyphens() {
    let output = compile("@page T\n@paragraph [hyphens auto]");
    assert!(output.contains("hyphens:auto"));
}

#[test]
fn css_flex_grow() {
    let output = compile("@page T\n@el [flex-grow 2]");
    assert!(output.contains("flex-grow:2"));
}

#[test]
fn css_flex_shrink() {
    let output = compile("@page T\n@el [flex-shrink 0]");
    assert!(output.contains("flex-shrink:0"));
}

#[test]
fn css_flex_basis() {
    let output = compile("@page T\n@el [flex-basis 200]");
    assert!(output.contains("flex-basis:200px"));
}

#[test]
fn css_isolation() {
    let output = compile("@page T\n@el [isolation isolate]");
    assert!(output.contains("isolation:isolate"));
}

#[test]
fn css_place_content() {
    let output = compile("@page T\n@el [display grid, place-content center]");
    assert!(output.contains("place-content:center"));
}

#[test]
fn css_background_image() {
    let output = compile("@page T\n@el [background-image linear-gradient(red, blue)]");
    assert!(output.contains("background-image:linear-gradient(red, blue)"));
}

// =========================================================================
// Media prefix assertion tests
// =========================================================================

#[test]
fn media_2xl_breakpoint() {
    let output = compile("@page T\n@el [2xl:padding 40]");
    assert!(output.contains("@media(min-width:1536px)"));
    assert!(output.contains("padding:40px"));
}

#[test]
fn media_motion_reduce() {
    let output = compile("@page T\n@el [motion-reduce:transition none]");
    assert!(output.contains("@media(prefers-reduced-motion:reduce)"));
    assert!(output.contains("transition:none"));
}

#[test]
fn media_motion_safe() {
    let output = compile("@page T\n@el [motion-safe:animation spin 1s]");
    assert!(output.contains("@media(prefers-reduced-motion:no-preference)"));
    assert!(output.contains("animation:spin 1s"));
}

#[test]
fn media_landscape() {
    let output = compile("@page T\n@el [landscape:padding 10]");
    assert!(output.contains("@media(orientation:landscape)"));
    assert!(output.contains("padding:10px"));
}

#[test]
fn media_portrait() {
    let output = compile("@page T\n@el [portrait:padding 40]");
    assert!(output.contains("@media(orientation:portrait)"));
    assert!(output.contains("padding:40px"));
}

// =========================================================================
// @unless assertion tests
// =========================================================================

#[test]
fn unless_false_shows_content() {
    let output = compile("@page T\n@let show false\n@if not $show\n  @text Visible");
    assert!(output.contains("Visible"));
}

#[test]
fn unless_true_hides_content() {
    let output = compile("@page T\n@let show true\n@if not $show\n  @text Hidden");
    assert!(!output.contains("Hidden"));
}

// =========================================================================
// @og assertion tests
// =========================================================================

#[test]
fn og_tags_in_output() {
    let output = compile(
        "@page T\n@meta og:title My Page\n@meta og:image https://example.com/img.png\n@text Hello",
    );
    assert!(output.contains("og:title"));
    assert!(output.contains("My Page"));
    assert!(output.contains("og:image"));
    assert!(output.contains("https://example.com/img.png"));
}

// =========================================================================
// Arithmetic assertion tests
// =========================================================================

#[test]
fn let_arithmetic_multiply() {
    let output = compile("@page T\n@let x 10\n@let y = $x * 2\n@el [width $y]\n  @text test");
    assert!(output.contains("width:20px"));
}

#[test]
fn let_arithmetic_add() {
    let output = compile("@page T\n@let a 10\n@let b = $a + 5\n@el [padding $b]\n  @text test");
    assert!(output.contains("padding:15px"));
}

#[test]
fn let_arithmetic_divide() {
    let output = compile("@page T\n@let x = 200 / 4\n@el [height $x]\n  @text test");
    assert!(output.contains("height:50px"));
}

// =========================================================================
// New diagnostics assertion tests
// =========================================================================

#[test]
fn warning_missing_input_type() {
    let diags = parse_diagnostics("@input [name=email]");
    assert!(
        diags.iter().any(|d| d.message.contains("missing 'type'")),
        "should warn about missing type on @input, got: {:?}",
        diags
    );
}

#[test]
fn warning_link_without_text() {
    let diags = parse_diagnostics("@link https://example.com");
    assert!(
        diags.iter().any(|d| d.message.contains("no visible text")),
        "should warn about @link without visible text, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_link_with_text() {
    let diags = parse_diagnostics("@link https://example.com Click here");
    assert!(
        !diags.iter().any(|d| d.message.contains("no visible text")),
        "should not warn when @link has text, got: {:?}",
        diags
    );
}

// =========================================================================
// No warnings for new CSS attributes
// =========================================================================

#[test]
fn no_warning_new_css_properties_2() {
    let diags = parse_diagnostics(
        "@el [clip-path circle(50%), mix-blend-mode multiply, writing-mode vertical-rl, isolation isolate]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new CSS properties should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_new_css_properties_3() {
    let diags = parse_diagnostics(
        "@el [column-count 3, column-gap 20, text-indent 2em, hyphens auto, flex-grow 1, flex-shrink 0, flex-basis 200, place-content center, background-image url(x)]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new CSS properties should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_new_media_prefixes() {
    let diags = parse_diagnostics(
        "@el [2xl:padding 40, motion-safe:animation none, motion-reduce:transition none, landscape:width 100%, portrait:padding 20]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new media prefixes should be recognized, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// Feature: if() conditional attribute values
// ---------------------------------------------------------------------------

#[test]
fn snapshot_if_expr() {
    snapshot_test("if_expr");
}

#[test]
fn if_expr_true_branch() {
    let output = compile("@let x true\n@el [background if($x, blue, gray)]\n  test");
    assert!(
        output.contains("blue"),
        "should use true branch, got: {}",
        output
    );
    assert!(!output.contains("gray"), "should not contain false branch");
}

#[test]
fn if_expr_false_branch() {
    let output = compile("@let x false\n@el [background if($x, blue, gray)]\n  test");
    assert!(
        output.contains("gray"),
        "should use false branch, got: {}",
        output
    );
    assert!(!output.contains("blue"), "should not contain true branch");
}

#[test]
fn if_expr_equality_condition() {
    let output = compile("@let theme dark\n@el [color if($theme == dark, white, black)]\n  test");
    assert!(
        output.contains("white"),
        "should match equality, got: {}",
        output
    );
}

#[test]
fn if_expr_inequality_condition() {
    let output = compile("@let mode light\n@el [color if($mode != dark, green, red)]\n  test");
    assert!(
        output.contains("green"),
        "should match inequality, got: {}",
        output
    );
}

#[test]
fn if_expr_passthrough_non_if() {
    // Regular values should not be affected
    let output = compile("@el [background blue]\n  test");
    assert!(output.contains("blue"));
}

// ---------------------------------------------------------------------------
// New improvement tests (batch 3)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_new_elements_3() {
    snapshot_test("new_elements_3");
}

#[test]
fn snapshot_new_css_properties_3() {
    snapshot_test("new_css_properties_3");
}

#[test]
fn snapshot_pseudo_elements() {
    snapshot_test("pseudo_elements");
}

#[test]
fn snapshot_each_else() {
    snapshot_test("each_else");
}

#[test]
fn each_else_empty_list() {
    // Variable that resolves to empty string: @each produces no items → @else fires
    let output =
        compile("@let empty \"\"\n@each $item in $empty\n  @text $item\n@else\n  @text fallback");
    assert!(
        output.contains("fallback"),
        "should render @else block when list is empty: {}",
        output
    );
}

#[test]
fn each_else_non_empty_list() {
    let output = compile("@each $x in a,b\n  @text $x\n@else\n  @text empty");
    assert!(output.contains("a"), "should render loop items: {}", output);
    assert!(output.contains("b"), "should render loop items: {}", output);
    assert!(
        !output.contains("empty"),
        "should not render @else block when list is non-empty: {}",
        output
    );
}

#[test]
fn pseudo_element_before_content() {
    let output = compile("@el [before:content arrow, before:color red]\n  Hello");
    assert!(
        output.contains("::before"),
        "should generate ::before CSS: {}",
        output
    );
    assert!(
        output.contains("content:\"arrow\""),
        "should generate content property: {}",
        output
    );
    assert!(
        output.contains("color:red"),
        "should generate color in ::before: {}",
        output
    );
}

#[test]
fn pseudo_element_after_content() {
    let output = compile("@el [after:content ✓]\n  Done");
    assert!(
        output.contains("::after"),
        "should generate ::after CSS: {}",
        output
    );
    assert!(
        output.contains("content:\"✓\""),
        "should generate content: {}",
        output
    );
}

#[test]
fn css_font_weight_numeric() {
    let output = compile("@text [font-weight 300] Light text");
    assert!(
        output.contains("font-weight:300"),
        "should generate font-weight CSS: {}",
        output
    );
}

#[test]
fn css_text_wrap_balance() {
    let output = compile("@text [text-wrap balance] Balanced");
    assert!(
        output.contains("text-wrap:balance"),
        "should generate text-wrap CSS: {}",
        output
    );
}

#[test]
fn css_touch_action() {
    let output = compile("@el [touch-action none]\n  No touch");
    assert!(
        output.contains("touch-action:none"),
        "should generate touch-action CSS: {}",
        output
    );
}

#[test]
fn css_content_visibility() {
    let output = compile("@el [content-visibility auto]\n  Lazy");
    assert!(
        output.contains("content-visibility:auto"),
        "should generate content-visibility CSS: {}",
        output
    );
}

#[test]
fn css_scroll_margin() {
    let output = compile("@el [scroll-margin-top 80]\n  Offset");
    assert!(
        output.contains("scroll-margin-top:80px"),
        "should generate scroll-margin-top CSS: {}",
        output
    );
}

#[test]
fn element_iframe() {
    let output = compile("@iframe [width fill, height 400] https://example.com");
    assert!(
        output.contains("<iframe"),
        "should generate iframe tag: {}",
        output
    );
    assert!(
        output.contains("src=\"https://example.com\""),
        "should have src: {}",
        output
    );
}

#[test]
fn element_canvas() {
    let output = compile("@canvas [width 400, height 300, id=myCanvas]");
    assert!(
        output.contains("<canvas"),
        "should generate canvas tag: {}",
        output
    );
    assert!(
        output.contains("id=\"myCanvas\""),
        "should have id: {}",
        output
    );
}

#[test]
fn element_output() {
    let output = compile("@output [for=a b]\n  42");
    assert!(
        output.contains("<output"),
        "should generate output tag: {}",
        output
    );
}

// =========================================================================
// Batch 4: New elements, @each step, pseudo selectors, container queries
// =========================================================================

#[test]
fn snapshot_new_elements_4() {
    snapshot_test("new_elements_4");
}

#[test]
fn snapshot_each_step() {
    snapshot_test("each_step");
}

#[test]
fn snapshot_selection_pseudo() {
    snapshot_test("selection_pseudo");
}

#[test]
fn snapshot_nth_pseudo() {
    snapshot_test("nth_pseudo");
}

#[test]
fn snapshot_direction_attr() {
    snapshot_test("direction_attr");
}

// --- Grid element ---

#[test]
fn element_grid() {
    let output = compile("@page T\n@grid [grid-cols 3, gap 16]\n  @el\n    @text A");
    assert!(
        output.contains("display:grid"),
        "grid should have display:grid: {}",
        output
    );
    assert!(
        output.contains("grid-template-columns:repeat(3,1fr)"),
        "should have 3 cols: {}",
        output
    );
}

// --- Stack element ---

#[test]
fn element_stack() {
    let output = compile("@page T\n@stack [width 200, height 200]\n  @el\n    @text Layer");
    assert!(
        output.contains("position:relative"),
        "stack should have position:relative: {}",
        output
    );
}

// --- @in-front / @behind overlay layers ---

#[test]
fn element_in_front() {
    let output = compile(
        "@page T\n@el [width 200, height 200]\n  Main\n  @in-front\n    @text Overlay",
    );
    assert!(
        output.contains("position:absolute"),
        "@in-front should have position:absolute: {}",
        output
    );
    assert!(
        output.contains("inset:0"),
        "@in-front should have inset:0: {}",
        output
    );
}

#[test]
fn element_behind() {
    let output =
        compile("@page T\n@el [width 200, height 200]\n  @behind\n    @text Bg\n  Foreground");
    assert!(
        output.contains("z-index:-1"),
        "@behind should have z-index:-1: {}",
        output
    );
    assert!(
        output.contains("position:absolute"),
        "@behind should have position:absolute: {}",
        output
    );
}

#[test]
fn in_front_makes_parent_positioning_context() {
    let output =
        compile("@page T\n@el [width 200, height 200]\n  Main\n  @in-front\n    Overlay");
    assert!(
        output.contains("position:relative"),
        "parent of @in-front should be position:relative: {}",
        output
    );
    assert!(
        output.contains("isolation:isolate"),
        "parent of @in-front should isolate stacking context: {}",
        output
    );
}

#[test]
fn in_front_does_not_override_explicit_position() {
    let output = compile(
        "@page T\n@el [position absolute, top 0, left 0]\n  Main\n  @in-front\n    Overlay",
    );
    assert!(
        !output.contains("position:relative"),
        "explicit position:absolute should win over auto position:relative: {}",
        output
    );
    assert!(
        output.contains("position:absolute"),
        "explicit position:absolute should be present: {}",
        output
    );
}

#[test]
fn parent_without_overlay_children_stays_static() {
    let output = compile("@page T\n@el [width 200, height 200]\n  Main");
    assert!(
        !output.contains("isolation:isolate"),
        "elements without @in-front/@behind children should not get isolation: {}",
        output
    );
}

// --- Spacer element ---

#[test]
fn element_spacer() {
    let output = compile("@page T\n@row\n  @text Left\n  @spacer\n  @text Right");
    assert!(
        output.contains("flex:1"),
        "spacer should have flex:1: {}",
        output
    );
}

// --- Badge element ---

#[test]
fn element_badge() {
    let output = compile("@page T\n@badge [background red, color white] 3");
    assert!(
        output.contains("<span"),
        "badge renders as span: {}",
        output
    );
    assert!(
        output.contains("border-radius:9999px"),
        "badge should be pill-shaped: {}",
        output
    );
    assert!(output.contains("3"), "badge content: {}", output);
}

// --- Tooltip element ---

#[test]
fn element_tooltip() {
    let output = compile("@page T\n@tooltip [tip Hover for info]\n  @text Help");
    assert!(
        output.contains("title=\"Hover for info\""),
        "tooltip should have title attr: {}",
        output
    );
    assert!(
        output.contains("cursor:help"),
        "tooltip should have cursor:help: {}",
        output
    );
}

// --- @each step ---

#[test]
fn each_step_basic() {
    let output = compile("@each $i in 0..20 step 5\n  @text $i");
    assert!(output.contains(">0<"), "should include 0: {}", output);
    assert!(output.contains(">5<"), "should include 5: {}", output);
    assert!(output.contains(">10<"), "should include 10: {}", output);
    assert!(output.contains(">15<"), "should include 15: {}", output);
    assert!(output.contains(">20<"), "should include 20: {}", output);
    assert!(!output.contains(">3<"), "should not include 3: {}", output);
}

#[test]
fn each_step_reverse() {
    let output = compile("@each $i in 10..1 step 3\n  @text $i");
    assert!(output.contains(">10<"), "should include 10: {}", output);
    assert!(output.contains(">7<"), "should include 7: {}", output);
    assert!(output.contains(">4<"), "should include 4: {}", output);
    assert!(output.contains(">1<"), "should include 1: {}", output);
}

// --- selection: pseudo ---

#[test]
fn selection_pseudo_generates_css() {
    let output =
        compile("@page T\n@text [selection:background blue, selection:color white] Select me");
    assert!(
        output.contains("::selection"),
        "should generate ::selection: {}",
        output
    );
    assert!(
        output.contains("background:blue"),
        "should have bg: {}",
        output
    );
}

// --- nth: pseudo ---

#[test]
fn nth_pseudo_generates_css() {
    let output = compile("@page T\n@el [nth:3:background red]\n  @text test");
    assert!(
        output.contains(":nth-child(3)"),
        "should generate :nth-child(3): {}",
        output
    );
    assert!(
        output.contains("background:red"),
        "should have bg: {}",
        output
    );
}

#[test]
fn nth_pseudo_formula() {
    let output = compile("@page T\n@el [nth:2n:background #eee]\n  @text test");
    assert!(
        output.contains(":nth-child(2n)"),
        "should generate :nth-child(2n): {}",
        output
    );
}

// --- container query prefix ---

#[test]
fn container_query_generates_css() {
    let output = compile("@page T\n@el [container]\n  @el [cq-sm:padding 20]\n    @text test");
    assert!(
        output.contains("@container(min-width:640px)"),
        "should generate container query: {}",
        output
    );
    assert!(
        output.contains("padding:20px"),
        "should have padding: {}",
        output
    );
}

// --- direction attribute ---

#[test]
fn direction_rtl() {
    let output = compile("@page T\n@el [direction rtl]\n  @text RTL text");
    assert!(
        output.contains("direction:rtl"),
        "should generate direction:rtl: {}",
        output
    );
}

// --- contrast checker ---

#[test]
fn warning_low_contrast() {
    let diags = parse_diagnostics("@el [background #ffffff, color #cccccc]\n  @text test");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("low contrast ratio")),
        "should warn about low contrast, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_good_contrast() {
    let diags = parse_diagnostics("@el [background #ffffff, color #000000]\n  @text test");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("low contrast ratio")),
        "should not warn about good contrast, got: {:?}",
        diags
    );
}

// --- no warnings for new features ---

#[test]
fn no_warning_selection_prefix() {
    let diags = parse_diagnostics("@el [selection:background blue]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "selection: prefix should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_nth_prefix() {
    let diags = parse_diagnostics("@el [nth:3:background red]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "nth: prefix should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_cq_prefix() {
    let diags = parse_diagnostics("@el [cq-sm:padding 20]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "cq- prefix should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_direction_attr() {
    let diags = parse_diagnostics("@el [direction rtl]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "direction should be recognized, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_new_elements() {
    // Grid, stack, spacer, badge, tooltip should all parse without errors
    let diags = parse_diagnostics("@grid\n  @text A");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "grid should parse, got: {:?}",
        diags
    );
    let diags = parse_diagnostics("@stack\n  @text A");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "stack should parse, got: {:?}",
        diags
    );
    let diags = parse_diagnostics("@row\n  @spacer");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "spacer should parse, got: {:?}",
        diags
    );
    let diags = parse_diagnostics("@badge [background red] 5");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "badge should parse, got: {:?}",
        diags
    );
    let diags = parse_diagnostics("@tooltip Help text\n  @text Hover");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "tooltip should parse, got: {:?}",
        diags
    );
}

// --- New feature tests ---

#[test]
fn snapshot_variable_filters() {
    snapshot_test("variable_filters");
}

#[test]
fn snapshot_new_elements_5() {
    snapshot_test("new_elements_5");
}

#[test]
fn snapshot_css_shorthands() {
    snapshot_test("css_shorthands");
}

#[test]
fn test_variable_filters() {
    let result = htmlang::parser::parse("@let name hello\n@text ${uppercase($name)}");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error)
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("HELLO"),
        "uppercase filter should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@let name HELLO\n@text ${lowercase($name)}");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("hello"),
        "lowercase filter should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@let name hello\n@text ${capitalize($name)}");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("Hello"),
        "capitalize filter should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@let name hello\n@text ${length($name)}");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("5"),
        "length filter should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@let name hello\n@text ${reverse($name)}");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("olleh"),
        "reverse filter should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@let name hello world\n@text ${truncate($name, 5)}");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("hello..."),
        "truncate filter should work, got: {}",
        html
    );
}

#[test]
fn test_new_elements_parse() {
    // Avatar
    let diags = parse_diagnostics("@avatar [width 48, height 48]\n  @text AB");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "avatar: {:?}",
        diags
    );

    // Carousel
    let diags = parse_diagnostics("@carousel [gap 16]\n  @el Slide 1");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "carousel: {:?}",
        diags
    );

    // Chip
    let diags = parse_diagnostics("@chip [background #eee] Tag");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "chip: {:?}",
        diags
    );

    // Tag
    let diags = parse_diagnostics("@tag [color blue] v1.0");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "tag: {:?}",
        diags
    );
}

#[test]
fn test_css_shorthands_output() {
    let result = htmlang::parser::parse("@text [$truncate] Hello");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("text-overflow:ellipsis"),
        "truncate should add ellipsis, got: {}",
        html
    );
    assert!(
        html.contains("white-space:nowrap"),
        "truncate should add nowrap"
    );

    let result = htmlang::parser::parse("@paragraph [line-clamp 3] Text");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("-webkit-line-clamp:3"),
        "line-clamp should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@el [filter blur(4px)] Content");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("filter:blur(4px)"),
        "blur should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@el [backdrop-filter blur(10px)] Content");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("backdrop-filter:blur(10px)"),
        "backdrop-blur should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@el [$no-scrollbar] Content");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("scrollbar-width:none"),
        "no-scrollbar should work, got: {}",
        html
    );

    let result = htmlang::parser::parse("@el [$skeleton, width 100, height 20] Content");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("hl-skeleton"),
        "skeleton should add animation, got: {}",
        html
    );
    assert!(
        html.contains("@keyframes hl-skeleton"),
        "skeleton should add keyframes, got: {}",
        html
    );

    // `gradient` became plain CSS: the compiler points to the replacement.
    let result = htmlang::parser::parse("@el [gradient #fff #000] Content");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("linear-gradient") && d.message.contains("upgrade")),
        "gradient should point to linear-gradient: {:?}",
        result.diagnostics
    );
}

#[test]
fn test_carousel_children_snap() {
    let result = htmlang::parser::parse("@carousel\n  @el A\n  @el B");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("scroll-snap-align:start"),
        "carousel children should have snap-align, got: {}",
        html
    );
}

#[test]
fn test_import_missing_file() {
    // We can't test @use with actual files in unit tests easily, but we can verify
    // the parser recognizes the directive without errors when it can't find the file
    let result = htmlang::parser::parse("@include nonexistent.hl");
    let has_use_error = result.diagnostics.iter().any(|d| {
        d.message.contains("cannot include") && d.severity == htmlang::parser::Severity::Error
    });
    assert!(
        has_use_error,
        "@include should report error for missing file, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn test_enhanced_keyframes() {
    let result = htmlang::parser::parse(
        "@keyframes fade-in\n  from [opacity 0]\n  to [opacity 1]\n@el [animation fade-in 0.3s] Content",
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("@keyframes fade-in{from{opacity:0;}to{opacity:1;}}"),
        "keyframes should parse htmlang syntax, got: {}",
        html
    );
}

#[test]
fn test_keyframes_percentage() {
    let result = htmlang::parser::parse(
        "@keyframes slide\n  0% [transform translateX(-100%)]\n  100% [transform translateX(0)]",
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("0%{transform:translateX(-100%);}"),
        "keyframe percentage should work, got: {}",
        html
    );
}

#[test]
fn test_theme_directive() {
    let result = htmlang::parser::parse(
        "@let primary #3b82f6\n@let --primary #3b82f6\n@meta theme-color #3b82f6\n@let spacing-md 16\n@let --spacing-md 16\n\n@el [background $primary, padding $spacing-md] Content",
    );
    let diags = &result.diagnostics;
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == htmlang::parser::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "theme should not cause errors: {:?}",
        errors
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("--primary:#3b82f6"),
        "theme should emit CSS vars, got: {}",
        html
    );
    assert!(
        html.contains("--spacing-md:16"),
        "theme should emit spacing var, got: {}",
        html
    );
    // Theme tokens collapse to `var(--name)` references so runtime theming
    // actually takes effect (users can override `--primary` with CSS).
    assert!(
        html.contains("background:var(--primary)"),
        "theme var should resolve to var(--primary), got: {}",
        html
    );
}

#[test]
fn deprecated_was_removed() {
    let diags = parse_diagnostics("@deprecated old\n@let card\n  @text x");
    assert!(diags.iter().any(|d| d.message.contains("`@deprecated` was removed")), "{:?}", diags);
}

#[test]
fn test_extends_was_removed() {
    let result = htmlang::parser::parse("@extends layout.hl\n@slot content\n  Hello");
    let has_error = result.diagnostics.iter().any(|d| {
        d.message.contains("was removed")
            && d.message.contains("function")
            && d.severity == htmlang::parser::Severity::Error
    });
    assert!(
        has_error,
        "@extends should point at layout functions, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn test_color_filter_lighten() {
    let result = htmlang::parser::parse(
        "@let primary #3b82f6\n@el [background ${lighten($primary, 20)}] Content",
    );
    let html = htmlang::codegen::generate(&result.document);
    // Lighten #3b82f6 by 20% should produce a lighter blue
    assert!(
        html.contains("background:#"),
        "lighten filter should produce hex color, got: {}",
        html
    );
    // Verify it's not the original color
    assert!(
        !html.contains("background:#3b82f6"),
        "lighten should change the color"
    );
}

#[test]
fn test_color_filter_darken() {
    let result =
        htmlang::parser::parse("@let primary #ffffff\n@el [background ${darken($primary, 50)}] Content");
    let html = htmlang::codegen::generate(&result.document);
    // Darken white by 50% should produce gray (#808080 approximately)
    assert!(
        html.contains("background:#"),
        "darken filter should produce hex color, got: {}",
        html
    );
    assert!(
        !html.contains("background:#ffffff"),
        "darken should change the color"
    );
}

#[test]
fn test_color_filter_alpha() {
    let result =
        htmlang::parser::parse("@let primary #3b82f6\n@el [background ${alpha($primary, 0.5)}] Content");
    let html = htmlang::codegen::generate(&result.document);
    // Should produce 8-digit hex with alpha
    assert!(
        html.contains("background:#3b82f67f"),
        "alpha filter should add alpha channel, got: {}",
        html
    );
}

#[test]
fn test_color_filter_mix() {
    let result = htmlang::parser::parse(
        "@let primary #000000\n@el [background ${mix($primary, #ffffff, 50)}] Content",
    );
    let html = htmlang::codegen::generate(&result.document);
    // Mix black and white at 50% should produce gray
    assert!(
        html.contains("background:#808080") || html.contains("background:#7f7f7f"),
        "mix filter should blend colors, got: {}",
        html
    );
}

#[test]
fn test_autofocus_attribute() {
    let result = htmlang::parser::parse("@input [type=text, autofocus]");
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("autofocus"),
        "autofocus should be in output, got: {}",
        html
    );
    // Should not produce unknown attribute warning
    let unknown_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unknown attribute") && d.message.contains("autofocus"))
        .collect();
    assert!(
        unknown_warnings.is_empty(),
        "autofocus should not warn as unknown"
    );
}

#[test]
fn test_repl_components_feed_subcommands_recognized() {
    // Just verify that the parser and codegen work for content that these commands would process
    let result = htmlang::parser::parse(
        "@page Test Site\n@meta description A test\n@let card $title\n  @text $title",
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error)
    );
}

// =========================================================================
// Batch 6: Grid areas, view transitions, animate, :has(), computed @let,
//          @layer wrapping, named slots in @fn
// =========================================================================

#[test]
fn snapshot_grid_areas() {
    snapshot_test("grid_areas");
}

#[test]
fn snapshot_view_transitions() {
    snapshot_test("view_transitions");
}

#[test]
fn snapshot_animate_shorthand() {
    snapshot_test("animate_shorthand");
}

#[test]
fn snapshot_has_pseudo() {
    snapshot_test("has_pseudo");
}

#[test]
fn snapshot_computed_let() {
    snapshot_test("computed_let");
}

#[test]
fn snapshot_layer_wrapping() {
    snapshot_test("layer_wrapping");
}

// --- Grid area assertions ---

#[test]
fn grid_template_areas_passthrough() {
    let output =
        compile("@page T\n@el [display grid, grid-template-areas \"a b\"]\n  @el [grid-area a]\n    A");
    assert!(
        output.contains("grid-template-areas:\"a b\""),
        "grid-template-areas should pass through: {}",
        output
    );
    assert!(
        output.contains("grid-area:a"),
        "grid-area should pass through: {}",
        output
    );
}

// --- View transition assertions ---

#[test]
fn view_transition_name_passthrough() {
    let output = compile("@page T\n@el [view-transition-name hero]\n  Content");
    assert!(
        output.contains("view-transition-name:hero"),
        "view-transition-name should pass through: {}",
        output
    );
}

// --- Animate shorthand assertions ---

#[test]
fn animate_generates_animation_css() {
    let output = compile(
        "@page T\n@keyframes fade\n  from [opacity 0]\n  to [opacity 1]\n@el [animation fade 0.3s ease]\n  Content",
    );
    assert!(
        output.contains("animation:fade 0.3s ease"),
        "animate should generate animation CSS: {}",
        output
    );
}

// --- :has() pseudo assertions ---

#[test]
fn has_pseudo_generates_css() {
    let output = compile("@page T\n@el [has(.active):background blue]\n  Content");
    assert!(
        output.contains(":has(.active)"),
        "should generate :has() selector: {}",
        output
    );
    assert!(
        output.contains("background:blue"),
        "should have background:blue in :has() rule: {}",
        output
    );
}

#[test]
fn has_pseudo_no_warning() {
    let diags = parse_diagnostics("@el [has(.child):background red]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "has() prefix should not produce unknown attribute warning: {:?}",
        diags
    );
}

// --- Computed @let assertions ---

#[test]
fn computed_let_equals_syntax() {
    let output =
        compile("@page T\n@let base 10\n@let doubled = $base * 2\n@el [width $doubled]\n  test");
    assert!(
        output.contains("width:20px"),
        "computed @let with = should work: {}",
        output
    );
}

// --- @layer wrapping assertions ---

#[test]
fn output_contains_layer_wrapping() {
    let output = compile("@page T\n@el [padding 10]\n  test");
    assert!(
        output.contains("@layer htmlang{"),
        "output should contain @layer htmlang wrapper: {}",
        output
    );
}

// --- Named slots in @let assertions ---

#[test]
fn fn_named_slots() {
    let output = compile(
        "@let layout\n  @column\n    @slot header\n      Default Header\n    @slot content\n@layout\n  @slot header\n    Custom Header\n  @slot content\n    Page body",
    );
    assert!(
        output.contains("Custom Header"),
        "named slot should be filled: {}",
        output
    );
    assert!(
        output.contains("Page body"),
        "content slot should be filled: {}",
        output
    );
    assert!(
        !output.contains("Default Header"),
        "default should be overridden: {}",
        output
    );
}

#[test]
fn fn_named_slot_default() {
    let output = compile(
        "@let layout\n  @column\n    @slot header\n      Default Header\n    @slot content\n@layout\n  @slot content\n    Only content",
    );
    assert!(
        output.contains("Default Header"),
        "unfilled slot should use default: {}",
        output
    );
    assert!(
        output.contains("Only content"),
        "filled slot should render: {}",
        output
    );
}

// --- No warnings for new attributes ---

#[test]
fn no_warning_new_attrs_batch6() {
    let diags = parse_diagnostics(
        "@el [grid-template-areas \"a b\", grid-area a, view-transition-name hero, animation fade 1s]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new attributes should be recognized: {:?}",
        diags
    );
}

// --- New elements (batch 6) ---

#[test]
fn snapshot_new_elements_6() {
    snapshot_test("new_elements_6");
}

#[test]
fn snapshot_script_element() {
    snapshot_test("script_element");
}

#[test]
fn snapshot_breadcrumb() {
    snapshot_test("breadcrumb");
}

#[test]
fn snapshot_new_directives() {
    snapshot_test("new_directives");
}

#[test]
fn snapshot_new_pseudos() {
    snapshot_test("new_pseudos");
}

#[test]
fn snapshot_new_css_properties_4() {
    snapshot_test("new_css_properties_4");
}

// --- Assertion tests for new features ---

#[test]
fn script_element_with_src() {
    let output = compile("@script [src=app.js, defer]");
    assert!(
        output.contains("<script src=\"app.js\" defer>"),
        "script src: {}",
        output
    );
    assert!(output.contains("</script>"), "script close: {}", output);
}

#[test]
fn script_element_inline() {
    let output = compile("@script\n  console.log(42);");
    assert!(
        output.contains("<script>console.log(42);</script>"),
        "inline script: {}",
        output
    );
}

#[test]
fn noscript_element() {
    let output = compile("@noscript\n  @text Fallback");
    assert!(output.contains("<noscript>"), "noscript open: {}", output);
    assert!(output.contains("</noscript>"), "noscript close: {}", output);
}

#[test]
fn address_element() {
    let output = compile("@address\n  @text Contact");
    assert!(output.contains("<address>"), "address: {}", output);
}

#[test]
fn search_element() {
    let output = compile("@search\n  @input [type=search]");
    assert!(output.contains("<search>"), "search: {}", output);
}

#[test]
fn breadcrumb_generates_nav_ol() {
    let output = compile("@breadcrumb\n  @item > @text Home\n  @item About");
    assert!(output.contains("aria-label=\"breadcrumb\""), "nav: {}", output);
    assert!(output.contains("<ol class="), "ol: {}", output);
    assert!(output.contains("<li class="), "li: {}", output);
}

#[test]
fn canonical_directive() {
    let output = compile("@page [canonical https://example.com/page] T\n@text Hello");
    assert!(
        output.contains("<link rel=\"canonical\" href=\"https://example.com/page\">"),
        "canonical: {}",
        output
    );
}

#[test]
fn base_directive() {
    let output = compile("@page [base https://example.com/] T\n@text Hello");
    assert!(
        output.contains("<base href=\"https://example.com/\">"),
        "base: {}",
        output
    );
}

#[test]
fn font_face_directive() {
    let output = compile("@page T\n@style\n  @font-face { font-family: 'Inter'; src: url('fonts/inter.woff2') format('woff2'); font-display: swap; }\n@head\n  <link rel=\"preload\" href=\"fonts/inter.woff2\" as=\"font\" crossorigin>\n@text Hello");
    assert!(output.contains("@font-face"), "font-face: {}", output);
    assert!(
        output.contains("font-family: 'Inter'"),
        "font name: {}",
        output
    );
    assert!(output.contains("fonts/inter.woff2"), "font url: {}", output);
    assert!(output.contains("woff2"), "format hint: {}", output);
}

#[test]
fn json_ld_directive() {
    let output = compile("@page T\n@head\n  <script type=\"application/ld+json\">\n    {\"@type\": \"WebPage\"}\n  </script>\n@text Hello");
    assert!(
        output.contains("application/ld+json"),
        "json-ld type: {}",
        output
    );
    assert!(output.contains("WebPage"), "json-ld content: {}", output);
}

#[test]
fn visited_pseudo() {
    let output = compile("@link [visited:color purple] https://example.com\n  Test");
    assert!(output.contains(":visited"), "visited pseudo: {}", output);
    assert!(output.contains("color:purple"), "visited color: {}", output);
}

#[test]
fn empty_pseudo() {
    let output = compile("@el [empty:display none]\n  Content");
    assert!(output.contains(":empty"), "empty pseudo: {}", output);
}

#[test]
fn target_pseudo() {
    let output = compile("@el [target:background yellow]\n  Content");
    assert!(output.contains(":target"), "target pseudo: {}", output);
}

#[test]
fn valid_invalid_pseudo() {
    let output = compile("@input [type=email, valid:border 2 green]");
    assert!(output.contains(":valid"), "valid pseudo: {}", output);
}

#[test]
fn text_underline_offset_property() {
    let output = compile("@text [underline, text-underline-offset 4] Link");
    assert!(
        output.contains("text-underline-offset:4px"),
        "text-underline-offset: {}",
        output
    );
}

#[test]
fn column_width_property() {
    let output = compile("@el [column-width 200]\n  Content");
    assert!(
        output.contains("column-width:200px"),
        "column-width: {}",
        output
    );
}

#[test]
fn column_rule_property() {
    let output = compile("@el [column-rule 1px solid #ccc]\n  Content");
    assert!(
        output.contains("column-rule:1px solid #ccc"),
        "column-rule: {}",
        output
    );
}

#[test]
fn no_warning_new_attrs_batch7() {
    let diags = parse_diagnostics(
        "@el [text-underline-offset 4, column-width 200, column-rule 1px solid gray]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "new CSS properties should be recognized: {:?}",
        diags
    );
}

#[test]
fn no_warning_script_attrs() {
    let diags = parse_diagnostics(
        "@script [src=app.js, defer, async, crossorigin anonymous, integrity sha384-abc, nomodule]",
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unknown attribute")),
        "script attributes should be recognized: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// @let attribute bundles (spread attributes)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_mixin_spread() {
    snapshot_test("mixin_spread");
}

#[test]
fn mixin_expands_in_attrs() {
    let output = compile("@let card [padding 20, rounded 8]\n@el [$card]\n  Hi");
    assert!(
        output.contains("padding:20px"),
        "mixin should expand padding: {}",
        output
    );
    assert!(
        output.contains("border-radius:8px"),
        "mixin should expand rounded: {}",
        output
    );
}

#[test]
fn mixin_with_dollar_syntax() {
    let output = compile("@let card [padding 20, rounded 8]\n@el [$card]\n  Hi");
    assert!(
        output.contains("padding:20px"),
        "mixin with $ syntax should expand: {}",
        output
    );
}

#[test]
fn mixin_compose_with_extra_attrs() {
    let output = compile("@let base [padding 10]\n@el [$base, background red]\n  Hi");
    assert!(
        output.contains("padding:10px"),
        "mixin should expand: {}",
        output
    );
    assert!(
        output.contains("background:red"),
        "extra attrs should work: {}",
        output
    );
}

#[test]
fn warning_unused_mixin() {
    let diags = parse_diagnostics("@let card [padding 10]\n@el [background red]");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unused attribute bundle")),
        "expected unused attribute bundle warning, got: {:?}",
        diags
    );
}

#[test]
fn no_warning_used_mixin() {
    let diags = parse_diagnostics("@let card [padding 10]\n@el [$card]");
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("unused attribute bundle")),
        "should not warn about used mixin, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// @assert directive
// ---------------------------------------------------------------------------

#[test]
fn snapshot_assert_directive() {
    snapshot_test("assert_directive");
}

#[test]
fn assert_passes_no_error() {
    let diags = parse_diagnostics("@let x hello\n@assert $x == hello\n@el [padding 10]");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "passing assertion should produce no error, got: {:?}",
        diags
    );
}

#[test]
fn assert_fails_produces_error() {
    let diags = parse_diagnostics("@let x hello\n@assert $x == world");
    assert!(
        diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error
                && d.message.contains("assertion failed")),
        "failing assertion should produce error, got: {:?}",
        diags
    );
}

#[test]
fn assert_not_equal() {
    let diags = parse_diagnostics("@let x hello\n@assert $x != world\n@el");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "!= assertion should pass when values differ, got: {:?}",
        diags
    );
}

#[test]
fn assert_truthy() {
    let diags = parse_diagnostics("@let x true\n@assert $x\n@el");
    assert!(
        !diags
            .iter()
            .any(|d| d.severity == htmlang::parser::Severity::Error),
        "truthy assertion should pass, got: {:?}",
        diags
    );
}

// ---------------------------------------------------------------------------
// clamp() / min() / max() CSS functions
// ---------------------------------------------------------------------------

#[test]
fn snapshot_clamp_css() {
    snapshot_test("clamp_css");
}

#[test]
fn clamp_passthrough() {
    let output = compile("@el [size clamp(16px, 2vw, 24px)]");
    assert!(
        output.contains("font-size:clamp(16px, 2vw, 24px)"),
        "clamp should pass through: {}",
        output
    );
}

#[test]
fn min_passthrough() {
    let output = compile("@el [width min(100%, 800px)]");
    assert!(
        output.contains("width:min(100%, 800px)"),
        "min() should pass through: {}",
        output
    );
}

#[test]
fn max_passthrough() {
    let output = compile("@el [padding max(10px, 2vw)]");
    assert!(
        output.contains("padding:max(10px, 2vw)"),
        "max() should pass through: {}",
        output
    );
}

// -----------------------------------------------------------------------
// @for numeric loop tests
// -----------------------------------------------------------------------

#[test]
fn for_basic_range() {
    let html = compile("@each $i in 1..3\n  @text $i\n");
    assert!(html.contains("1"));
    assert!(html.contains("2"));
    assert!(html.contains("3"));
}

#[test]
fn for_with_step() {
    let html = compile("@each $i in 0..10 step 5\n  @text $i\n");
    assert!(html.contains("0"));
    assert!(html.contains("5"));
    assert!(html.contains("10"));
}

#[test]
fn for_reverse_range() {
    let html = compile("@each $i in 3..1\n  @text $i\n");
    assert!(html.contains("3"));
    assert!(html.contains("2"));
    assert!(html.contains("1"));
}

#[test]
fn for_with_variable_bounds() {
    let html = compile("@let start 1\n@let end 3\n@each $i in $start..$end\n  @text $i\n");
    assert!(html.contains("1"));
    assert!(html.contains("2"));
    assert!(html.contains("3"));
}

// -----------------------------------------------------------------------
// Conditional attribute tests
// -----------------------------------------------------------------------

#[test]
fn conditional_attr_true() {
    let html = compile("@let show true\n@el [padding 10 if $show]\n  test\n");
    assert!(html.contains("padding:10px"));
}

#[test]
fn conditional_attr_false() {
    let html = compile("@let show false\n@el [padding 10 if $show]\n  test\n");
    assert!(!html.contains("padding:10px"));
}

#[test]
fn conditional_attr_boolean_true() {
    let html = compile("@let loading true\n@button [disabled if $loading] Click\n");
    assert!(html.contains("disabled"));
}

#[test]
fn conditional_attr_boolean_false() {
    let html = compile("@let loading false\n@button [disabled if $loading] Click\n");
    assert!(!html.contains("disabled"));
}

// -----------------------------------------------------------------------
// @component tests
// -----------------------------------------------------------------------

#[test]
fn function_with_style_is_scoped() {
    let html = compile(
        "@let card $title\n  @style\n    .t { color: red; }\n  @text [class=t] $title\n@card [title Hello]\n",
    );
    assert!(html.contains("hl-card\"><span class=\"t\">Hello"), "{}", html);
    assert!(html.contains(".hl-card .t { color: red; }"), "{}", html);
}

#[test]
fn function_without_style_has_no_wrapper() {
    let html =
        compile("@let box\n  @el [padding 10]\n    @children\n\n@box\n  @text Inside\n");
    assert!(!html.contains("hl-box"), "{}", html);
    assert!(html.contains("Inside"));
}

// -----------------------------------------------------------------------
// @switch tests
// -----------------------------------------------------------------------

#[test]
fn switch_matches_case() {
    let html = compile(
        "@let variant primary\n@if $variant == \"primary\"\n  @text Primary\n@else if $variant == \"danger\"\n  @text Danger\n",
    );
    assert!(html.contains("Primary"));
    assert!(!html.contains("Danger"));
}

#[test]
fn switch_falls_to_default() {
    let html = compile(
        "@let variant unknown\n@if $variant == \"primary\"\n  @text Primary\n@else\n  @text Default\n",
    );
    assert!(!html.contains("Primary"));
    assert!(html.contains("Default"));
}

#[test]
fn switch_with_attrs() {
    let _html = compile(
        "@let variant primary\n@if $variant == \"primary\"\n  @let __switch [background blue, color white]\n@else if $variant == \"danger\"\n  @let __switch [background red, color white]\n",
    );
    // The @switch should register matched attrs as __switch define
    let result = htmlang::parser::parse(
        "@let variant primary\n@if $variant == \"primary\"\n  @let __switch [background blue, color white]\n@else if $variant == \"danger\"\n  @let __switch [background red, color white]\n",
    );
    assert!(result.document.defines.contains_key("__switch"));
}

// -----------------------------------------------------------------------
// HTML minification test
// -----------------------------------------------------------------------

#[test]
fn minified_output_is_smaller() {
    let input = "@page Test\n@column [padding 20]\n  @text [bold] Hello World\n  @paragraph\n    Some text here\n";
    let result = htmlang::parser::parse(input);
    let normal = htmlang::codegen::generate(&result.document);
    let minified = htmlang::codegen::generate_minified(&result.document);
    assert!(
        minified.len() <= normal.len(),
        "minified ({}) should be <= normal ({})",
        minified.len(),
        normal.len()
    );
    assert!(minified.contains("Hello World"));
}

#[test]
fn minified_strips_comments() {
    let input = "@page Test\n@column\n  @text Hello\n";
    let result = htmlang::parser::parse(input);
    let dev = htmlang::codegen::generate_dev(&result.document);
    let minified = htmlang::codegen::generate_minified(&result.document);
    // Dev mode has comments, minified should not
    assert!(dev.contains("<!--"));
    assert!(!minified.contains("<!--"));
}

// -----------------------------------------------------------------------
// Critical CSS test
// -----------------------------------------------------------------------

#[test]
fn critical_attr_was_removed() {
    let diags = parse_diagnostics("@el [critical, padding 20]\n  test\n");
    assert!(
        diags.iter().any(|d| d.message.contains("'critical' was removed")),
        "{:?}",
        diags
    );
}

// -----------------------------------------------------------------------
// Enhanced a11y warnings
// -----------------------------------------------------------------------

#[test]
fn warning_input_without_label() {
    let diags = parse_diagnostics("@input [type=text]\n");
    let has_label_warning = diags
        .iter()
        .any(|d| d.message.contains("aria-label") || d.message.contains("@label"));
    assert!(
        has_label_warning,
        "should warn about input without label association"
    );
}

#[test]
fn warning_iframe_without_title() {
    let diags = parse_diagnostics("@iframe https://example.com\n");
    let has_title_warning = diags.iter().any(|d| d.message.contains("title"));
    assert!(has_title_warning, "should warn about iframe without title");
}

#[test]
fn warning_button_without_text() {
    let diags = parse_diagnostics("@button [background red]\n");
    let has_warning = diags
        .iter()
        .any(|d| d.message.contains("text content") || d.message.contains("aria-label"));
    assert!(
        has_warning,
        "should warn about button without accessible text"
    );
}

#[test]
fn warning_positive_tabindex() {
    let diags = parse_diagnostics("@el [tabindex=5]\n  test\n");
    let has_warning = diags.iter().any(|d| d.message.contains("tabindex"));
    assert!(has_warning, "should warn about positive tabindex");
}

#[test]
fn no_warning_input_with_aria_label() {
    let diags = parse_diagnostics("@input [type=text, aria-label=Search]\n");
    let has_label_warning = diags
        .iter()
        .any(|d| d.message.contains("should have an") && d.message.contains("@label"));
    assert!(
        !has_label_warning,
        "should not warn when aria-label is present"
    );
}

#[test]
fn no_warning_input_in_label() {
    let diags = parse_diagnostics("@label\n  @input [type=text]\n");
    let has_label_warning = diags
        .iter()
        .any(|d| d.message.contains("should have an") && d.message.contains("@label"));
    assert!(
        !has_label_warning,
        "should not warn when input is inside @label"
    );
}

// --- New feature tests ---

#[test]
fn snapshot_popover_api() {
    snapshot_test("popover_api");
}

#[test]
fn snapshot_new_html_attrs() {
    snapshot_test("new_html_attrs");
}

#[test]
fn snapshot_color_scheme() {
    snapshot_test("color_scheme");
}

#[test]
fn snapshot_data_directive() {
    snapshot_test("data_directive");
}

#[test]
fn test_popover_in_output() {
    let html = compile(
        "@button [popovertarget=my-pop] Open\n@el [popover, id=my-pop, padding 10]\n  Hello",
    );
    assert!(
        html.contains("popovertarget=\"my-pop\""),
        "should have popovertarget attr"
    );
    assert!(
        html.contains(" popover"),
        "should have popover boolean attr"
    );
}

#[test]
fn test_color_scheme_css() {
    let html = compile("@el [color-scheme light dark]\n  Test");
    assert!(
        html.contains("color-scheme:light dark"),
        "should generate color-scheme CSS"
    );
}

#[test]
fn test_appearance_css() {
    let html = compile("@input [appearance none, padding 10]");
    assert!(
        html.contains("appearance:none"),
        "should generate appearance CSS"
    );
}

#[test]
fn test_inputmode_attr() {
    let html = compile("@input [type=search, inputmode=search]");
    assert!(
        html.contains("inputmode=\"search\""),
        "should pass through inputmode"
    );
}

#[test]
fn test_fetchpriority_attr() {
    let html = compile("@image [fetchpriority=high, width 100] hero.jpg");
    assert!(
        html.contains("fetchpriority=\"high\""),
        "should pass through fetchpriority"
    );
}

// ---------------------------------------------------------------------------
// Error snapshot tests — verify expected error messages on invalid input
// ---------------------------------------------------------------------------

#[test]
fn test_error_unknown_element() {
    let diags = parse_diagnostics("@bogus\n  Hello");
    assert!(
        diags.iter().any(|d| d.message.contains("unknown element")),
        "should report unknown element error"
    );
}

#[test]
fn test_error_unclosed_brackets() {
    let diags = parse_diagnostics("@el [padding 10\n  Hello");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("unclosed '['") || d.message.contains("unclosed")),
        "should report unclosed bracket error"
    );
}

#[test]
fn test_error_each_missing_in() {
    let diags = parse_diagnostics("@each $item\n  @text $item");
    assert!(
        diags.iter().any(|d| d.message.contains("@each requires")),
        "should report @each syntax error"
    );
}

#[test]
fn test_error_for_missing_range() {
    let diags = parse_diagnostics("@each $i\n  @text $i");
    assert!(
        diags.iter().any(|d| d.message.contains("@each requires")),
        "should report @for syntax error"
    );
}

#[test]
fn test_error_circular_include() {
    // A file including itself would be circular, but we test via in-memory parse
    // by testing that the parser detects self-referential definitions
    let diags = parse_diagnostics("@let recursive $x\n  @recursive [$x]\n\n@recursive [hello]");
    assert!(
        diags.iter().any(|d| d.message.contains("recursive")),
        "should report recursive function call"
    );
}

#[test]
fn test_error_assert_failure() {
    let diags = parse_diagnostics("@let x 5\n@assert $x == 10");
    assert!(
        diags.iter().any(|d| d.message.contains("assertion failed")
            && d.severity == htmlang::parser::Severity::Error),
        "should report assertion failure as error"
    );
}

#[test]
fn test_error_duplicate_attribute() {
    let diags = parse_diagnostics("@el [padding 10, padding 20]\n  Hello");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("duplicate attribute")),
        "should warn on duplicate attribute"
    );
}

#[test]
fn test_warning_unused_variable() {
    let diags = parse_diagnostics("@let unused_var hello\n@text Hello");
    assert!(
        diags.iter().any(|d| d.message.contains("unused variable")
            && d.severity == htmlang::parser::Severity::Warning),
        "should warn about unused variable"
    );
}

#[test]
fn test_warning_unused_function() {
    let diags = parse_diagnostics("@let unused_fn\n  @text Hello\n\n@text World");
    assert!(
        diags.iter().any(|d| d.message.contains("unused function")
            && d.severity == htmlang::parser::Severity::Warning),
        "should warn about unused function"
    );
}

// ---------------------------------------------------------------------------
// New feature tests
// ---------------------------------------------------------------------------

#[test]
fn test_each_index_variable() {
    let html = compile("@each $item in A, B, C\n  @text $_index");
    assert!(html.contains(">0<"), "first item should have $_index = 0");
    assert!(html.contains(">1<"), "second item should have $_index = 1");
    assert!(html.contains(">2<"), "third item should have $_index = 2");
}

#[test]
fn test_children_fallback_content() {
    let html = compile(
        "@let wrapper\n  @el [padding 10]\n    @children\n      @text Default content\n\n@wrapper",
    );
    assert!(
        html.contains("Default content"),
        "should use @children fallback when no children provided"
    );
}

#[test]
fn test_spread_define() {
    let html = compile("@let btn [padding 12, bold]\n@el [$btn]\n  Click");
    assert!(
        html.contains("padding:12px"),
        "spread define should apply padding"
    );
    assert!(
        html.contains("font-weight:bold") || html.contains("font-weight:700"),
        "spread define should apply bold"
    );
}

#[test]
fn test_log_directive() {
    // @log should not produce errors and should be consumed without output nodes
    let result = htmlang::parser::parse("@let x hello\n@warn x = $x\n@text $x");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "@warn should = $should, not = $not, produce = $produce, errors = $errors"
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(!html.contains("@log"), "@warn should = $should, not = $not, appear = $appear, in = $in, output = $output");
}

#[test]
fn test_short_class_names() {
    let html = compile("@el [padding 10]\n  @el [padding 20]\n    Hello");
    // Class names should be short single letters, not _0, _1
    assert!(
        !html.contains("class=\"_0\""),
        "should use short class names, not _0"
    );
    assert!(
        html.contains("class=\"a\"") || html.contains("class=\"b\""),
        "should use short alphabetic class names"
    );
}

// ---------------------------------------------------------------------------
// New improvement tests (batch)
// ---------------------------------------------------------------------------

#[test]
fn snapshot_markdown_block() {
    snapshot_test("markdown_block");
}

#[test]
fn snapshot_repeat_directive() {
    snapshot_test("repeat_directive");
}

#[test]
fn snapshot_with_directive() {
    snapshot_test("with_directive");
}

#[test]
fn snapshot_scope_css() {
    snapshot_test("scope_css");
}

#[test]
fn snapshot_starting_style() {
    snapshot_test("starting_style");
}

#[test]
fn snapshot_manifest_directive() {
    snapshot_test("manifest_directive");
}

#[test]
fn snapshot_subgrid_css() {
    snapshot_test("subgrid_css");
}

#[test]
fn snapshot_anchor_positioning() {
    snapshot_test("anchor_positioning");
}

#[test]
fn snapshot_scroll_driven_animations() {
    snapshot_test("scroll_driven_animations");
}

#[test]
fn snapshot_initial_letter() {
    snapshot_test("initial_letter");
}

// --- Inline unit tests for new features ---

#[test]
fn repeat_directive_basic() {
    let output = compile("@each $_ in 1..3\n  @text hello");
    // Should contain 3 spans with "hello"
    let count = output.matches("hello").count();
    assert_eq!(count, 3, "expected 3 repetitions, got {}", count);
}

#[test]
fn with_directive_rebinding() {
    let output = compile("@let x hello\n@let y $x\n@text $y");
    assert!(
        output.contains("hello"),
        "expected @with to rebind variable"
    );
}

#[test]
fn markdown_renders_heading() {
    let output = compile("@markdown\n  # Title\n  Some text");
    assert!(
        output.contains("<h1>Title</h1>"),
        "markdown should render # as <h1>"
    );
    assert!(
        output.contains("<p>Some text</p>"),
        "markdown should render paragraphs"
    );
}

#[test]
fn markdown_renders_bold_italic() {
    let output = compile("@markdown\n  This is **bold** and *italic*");
    assert!(
        output.contains("<strong>bold</strong>"),
        "markdown should render **bold**"
    );
    assert!(
        output.contains("<em>italic</em>"),
        "markdown should render *italic*"
    );
}

#[test]
fn markdown_renders_code() {
    let output = compile("@markdown\n  Use `code` here");
    assert!(
        output.contains("<code>code</code>"),
        "markdown should render `code`"
    );
}

#[test]
fn markdown_renders_link() {
    let output = compile("@markdown\n  Visit [example](https://example.com)");
    assert!(
        output.contains("<a href=\"https://example.com\">example</a>"),
        "markdown should render links"
    );
}

#[test]
fn markdown_renders_list() {
    let output = compile("@markdown\n  - one\n  - two\n  - three");
    assert!(
        output.contains("<ul>"),
        "markdown should render unordered list"
    );
    assert!(
        output.contains("<li>one</li>"),
        "markdown should render list items"
    );
}

#[test]
fn scope_block_generates_css() {
    let output = compile("@page Test\n@style\n  @scope (.card) {\n    .title { color: red; }\n  }\n@text hello");
    assert!(
        output.contains("@scope (.card)"),
        "should generate @scope CSS block"
    );
}

#[test]
fn starting_style_generates_css() {
    let output = compile("@page Test\n@style\n  @starting-style {\n    .fade { opacity: 0; }\n  }\n@text hello");
    assert!(
        output.contains("@starting-style"),
        "should generate @starting-style CSS block"
    );
}

#[test]
fn manifest_was_removed() {
    let diags = parse_diagnostics("@page App\n@manifest My App\n  display standalone\n@text hi");
    assert!(diags.iter().any(|d| d.message.contains("`@manifest` was removed")), "{:?}", diags);
}

#[test]
fn subgrid_support() {
    let output = compile("@el [grid-template-columns subgrid]");
    assert!(
        output.contains("grid-template-columns:subgrid"),
        "should support CSS subgrid"
    );
}

#[test]
fn anchor_positioning_support() {
    let output = compile("@el [anchor-name --my-anchor]\n  @text anchor");
    assert!(
        output.contains("anchor-name:--my-anchor"),
        "should support anchor-name CSS property"
    );
}

#[test]
fn scroll_driven_animation_support() {
    let output = compile("@el [animation-timeline scroll()]\n  @text scroll");
    assert!(
        output.contains("animation-timeline:scroll()"),
        "should support animation-timeline CSS property"
    );
}

#[test]
fn initial_letter_support() {
    let output = compile("@text [initial-letter 3] O");
    assert!(
        output.contains("initial-letter:3"),
        "should support initial-letter CSS property"
    );
}

#[test]
fn position_area_support() {
    let output = compile("@el [position-area top]\n  @text tooltip");
    assert!(
        output.contains("position-area:top"),
        "should support position-area CSS property"
    );
}

// --- Snapshot tests for batch 2 ---

#[test]
fn snapshot_translations_i18n() {
    snapshot_test("translations_i18n");
}

#[test]
fn each_pagination_was_removed() {
    let diags = parse_diagnostics("@each $item in a,b,c [page 2]\n  @text $item");
    assert!(diags.iter().any(|d| d.message.contains("pagination")), "{:?}", diags);
}

#[test]
fn snapshot_env_directive() {
    snapshot_test("env_directive");
}

#[test]
fn snapshot_css_property() {
    snapshot_test("css_property");
}

#[test]
fn snapshot_responsive_images() {
    snapshot_test("responsive_images");
}

// --- Inline tests for batch 2 ---

#[test]
fn translations_point_to_data_files() {
    let diags = parse_diagnostics("@translations\n  en:\n    hello Hi");
    assert!(
        diags.iter().any(|d| d.message.contains("@data $t locales/$lang.json")),
        "{:?}",
        diags
    );
}

#[test]
fn image_auto_preload() {
    let output = compile("@page Test\n@image logo.png\n@image hero.jpg");
    assert!(
        output.contains("rel=\"preload\""),
        "should auto-preload images"
    );
    assert!(output.contains("logo.png"), "should preload logo.png");
}

#[test]
fn source_map_generation() {
    let input = "@page Test\n@text [bold] Hello";
    let result = htmlang::parser::parse(input);
    let map = htmlang::codegen::generate_source_map(&result.document, "test.hl");
    assert!(
        map.contains("\"version\":3"),
        "source map should have version 3"
    );
    assert!(
        map.contains("test.hl"),
        "source map should reference source file"
    );
}

#[test]
fn parser_multiple_errors() {
    let diags = parse_diagnostics("@unknown1\n@text hello\n@unknown2");
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == htmlang::parser::Severity::Error)
        .collect();
    assert!(
        errors.len() >= 2,
        "parser should report multiple errors, got {}",
        errors.len()
    );
}

#[test]
fn repeat_with_index() {
    let output = compile("@each $_ in 1..3\n  @text $_index");
    assert!(output.contains("0"), "should have index 0");
    assert!(output.contains("1"), "should have index 1");
    assert!(output.contains("2"), "should have index 2");
}

// ---------------------------------------------------------------------------
// New feature tests
// ---------------------------------------------------------------------------

#[test]
fn env_directive_with_default() {
    let output =
        compile("@data $fallback env:HTMLANG_TEST_NONEXISTENT fallback_value\n@text $fallback");
    assert!(
        output.contains("fallback_value"),
        "env: data should use the default when unset, got: {}",
        output
    );
}

#[test]
fn env_directive_from_environment() {
    // Set an env var and check it's picked up
    unsafe {
        std::env::set_var("HTMLANG_TEST_VAR", "hello_world");
    }
    let output = compile("@data $var env:HTMLANG_TEST_VAR\n@text $var");
    assert!(
        output.contains("hello_world"),
        "env: data should read the variable, got: {}",
        output
    );
    unsafe {
        std::env::remove_var("HTMLANG_TEST_VAR");
    }
}

#[test]
fn env_directive_warning_when_missing() {
    let result = htmlang::parser::parse("@data $x env:HTMLANG_DEFINITELY_NOT_SET_12345");
    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == htmlang::parser::Severity::Warning && d.message.contains("not set")
        })
        .collect();
    assert!(
        !warnings.is_empty(),
        "env: data should warn when unset, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn fetch_was_removed() {
    let diags = parse_diagnostics("@fetch $data https://example.com/api");
    assert!(
        diags.iter().any(|d| d.message.contains("`@fetch` was removed")),
        "{:?}",
        diags
    );
}

#[test]
fn svg_directive_inline() {
    // Create a temporary SVG file
    let dir = std::env::temp_dir().join("htmlang_test_svg");
    let _ = std::fs::create_dir_all(&dir);
    let svg_path = dir.join("test.svg");
    std::fs::write(&svg_path, r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/></svg>"#).unwrap();

    let input = format!("@image [inline] {}", svg_path.display());
    let result = htmlang::parser::parse(&input);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "should parse without errors: {:?}",
        result.diagnostics
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("<svg"),
        "should inline SVG content, got: {}",
        html
    );
    assert!(
        html.contains("<circle"),
        "should contain SVG elements, got: {}",
        html
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn svg_directive_with_attrs() {
    let dir = std::env::temp_dir().join("htmlang_test_svg_attrs");
    let _ = std::fs::create_dir_all(&dir);
    let svg_path = dir.join("icon.svg");
    std::fs::write(
        &svg_path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="48"><rect/></svg>"#,
    )
    .unwrap();

    let input = format!("@image [inline, width 24, color red] {}", svg_path.display());
    let result = htmlang::parser::parse(&input);
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("width=\"24\""),
        "should override width, got: {}",
        html
    );
    assert!(
        html.contains("fill=\"red\""),
        "should set fill from color attr, got: {}",
        html
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn svg_directive_missing_file() {
    let result = htmlang::parser::parse("@image [inline] /nonexistent/missing.svg");
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == htmlang::parser::Severity::Error && d.message.contains("cannot load SVG")
        })
        .collect();
    assert!(
        !errors.is_empty(),
        "should error on missing SVG, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn css_property_directive() {
    let input = "@style\n  @property --my-color {\n    syntax:\"<color>\";\n    inherits:true;\n    initial-value:#000;\n  }\n\n@el [background var(--my-color)] Content";
    let result = htmlang::parser::parse(input);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "should parse without errors: {:?}",
        result.diagnostics
    );
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("@property --my-color"),
        "@property rule should be emitted, got: {}",
        html
    );
    assert!(
        html.contains("syntax:\"<color>\""),
        "should include syntax, got: {}",
        html
    );
    assert!(
        html.contains("inherits:true"),
        "should include inherits, got: {}",
        html
    );
    assert!(
        html.contains("initial-value:#000"),
        "should include initial-value, got: {}",
        html
    );
}

#[test]
fn partial_output() {
    let result = htmlang::parser::parse("@page Test\n@el [padding 20]\n  Hello");
    let html = htmlang::codegen::generate_partial(&result.document);
    assert!(
        !html.contains("<!DOCTYPE"),
        "partial should not have doctype, got: {}",
        html
    );
    assert!(
        !html.contains("<html"),
        "partial should not have html tag, got: {}",
        html
    );
    assert!(
        !html.contains("<head"),
        "partial should not have head tag, got: {}",
        html
    );
    assert!(
        !html.contains("<body"),
        "partial should not have body tag, got: {}",
        html
    );
    assert!(
        html.contains("<style>"),
        "partial should still have CSS, got: {}",
        html
    );
    assert!(
        html.contains("Hello"),
        "partial should have content, got: {}",
        html
    );
}

#[test]
fn partial_output_dev() {
    let result = htmlang::parser::parse("@page Test\n@el [padding 20]\n  Hello");
    let html = htmlang::codegen::generate_partial_dev(&result.document);
    assert!(
        !html.contains("<!DOCTYPE"),
        "partial dev should not have doctype"
    );
    assert!(html.contains("<style>"), "partial dev should have style");
    assert!(html.contains("Hello"), "partial dev should have content");
}

#[test]
fn responsive_srcset_on_image() {
    let output = compile("@image [responsive 400 800 1200, alt=Photo] photo.jpg");
    assert!(
        output.contains("srcset=\""),
        "should generate srcset, got: {}",
        output
    );
    assert!(
        output.contains("photo-400.jpg 400w"),
        "should have 400w source, got: {}",
        output
    );
    assert!(
        output.contains("photo-800.jpg 800w"),
        "should have 800w source, got: {}",
        output
    );
    assert!(
        output.contains("photo-1200.jpg 1200w"),
        "should have 1200w source, got: {}",
        output
    );
    assert!(
        output.contains("sizes="),
        "should generate sizes attribute, got: {}",
        output
    );
}

#[test]
fn auto_image_dimensions_png() {
    // Create a minimal valid PNG (1x1 pixel)
    let dir = std::env::temp_dir().join("htmlang_test_img_dim");
    let _ = std::fs::create_dir_all(&dir);
    let png_path = dir.join("test.png");
    // Minimal 1x1 PNG file
    let png_data: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, // IHDR length
        0x49, 0x48, 0x44, 0x52, // IHDR
        0x00, 0x00, 0x00, 0x01, // width = 1
        0x00, 0x00, 0x00, 0x01, // height = 1
        0x08, 0x02, 0x00, 0x00, 0x00, // bit depth, color type, etc.
        0x90, 0x77, 0x53, 0xDE, // CRC
        0x00, 0x00, 0x00, 0x0C, // IDAT length
        0x49, 0x44, 0x41, 0x54, // IDAT
        0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC,
        0x33, // CRC
        0x00, 0x00, 0x00, 0x00, // IEND length
        0x49, 0x45, 0x4E, 0x44, // IEND
        0xAE, 0x42, 0x60, 0x82, // CRC
    ];
    std::fs::write(&png_path, &png_data).unwrap();

    let input = format!("@image [alt=test] {}", png_path.display());
    let result = htmlang::parser::parse(&input);
    let html = htmlang::codegen::generate(&result.document);
    assert!(
        html.contains("width=\"1\""),
        "should auto-detect width=1 from PNG, got: {}",
        html
    );
    assert!(
        html.contains("height=\"1\""),
        "should auto-detect height=1 from PNG, got: {}",
        html
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn auto_image_dimensions_not_for_urls() {
    // Remote URLs should not trigger dimension detection
    let output = compile("@image [alt=test] https://example.com/photo.png");
    // Should not crash or add dimensions for remote URLs
    assert!(
        output.contains("src=\"https://example.com/photo.png\""),
        "should keep URL src, got: {}",
        output
    );
}

#[test]
fn auto_image_dimensions_respects_explicit() {
    // If width/height are explicitly set, don't override them
    let dir = std::env::temp_dir().join("htmlang_test_img_explicit");
    let _ = std::fs::create_dir_all(&dir);
    let png_path = dir.join("test2.png");
    let png_data: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    std::fs::write(&png_path, &png_data).unwrap();

    let input = format!(
        "@image [width 100, height 100, alt=test] {}",
        png_path.display()
    );
    let result = htmlang::parser::parse(&input);
    let html = htmlang::codegen::generate(&result.document);
    // When width/height are set as CSS attrs, auto-dimensions should not add HTML width/height
    assert!(
        html.contains("width:100px"),
        "should have CSS width, got: {}",
        html
    );
    assert!(
        html.contains("height:100px"),
        "should have CSS height, got: {}",
        html
    );
    assert!(
        !html.contains("width=\"1\""),
        "should NOT auto-detect dimensions when CSS size is set, got: {}",
        html
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_stress_large() {
    snapshot_test("stress_large");
}

#[test]
fn snapshot_stress_deeply_nested() {
    snapshot_test("stress_deeply_nested");
}

#[test]
fn perf_large_document() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let input = std::fs::read_to_string(dir.join("stress_large.hl")).unwrap();
    let start = std::time::Instant::now();
    let result = htmlang::parser::parse(&input);
    let _ = htmlang::codegen::generate(&result.document);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 5000,
        "compilation took {}ms, expected < 5000ms",
        elapsed.as_millis()
    );
}

// ---------------------------------------------------------------------------
// Filesystem-based error tests for @include / @data
// ---------------------------------------------------------------------------

#[test]
fn error_circular_include_filesystem() {
    // a.hl includes b.hl, which includes a.hl — expect a cycle diagnostic.
    let dir = std::env::temp_dir().join("htmlang_test_circular_include");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let a_path = dir.join("a.hl");
    let b_path = dir.join("b.hl");
    std::fs::write(&a_path, "@include b.hl\n").unwrap();
    std::fs::write(&b_path, "@include a.hl\n").unwrap();

    let input = std::fs::read_to_string(&a_path).unwrap();
    let result = htmlang::parser::parse_with_base(&input, Some(&dir));
    let has_cycle = result
        .diagnostics
        .iter()
        .any(|d| d.severity == htmlang::parser::Severity::Error && d.message.contains("circular"));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        has_cycle,
        "expected circular include error, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn import_with_alias_prefixes_definitions() {
    // Verify that @include with alias registers imported fns under `alias.name`.
    let dir = std::env::temp_dir().join("htmlang_test_import_alias");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lib_path = dir.join("lib.hl");
    std::fs::write(&lib_path, "@let card\n  @el\n    @text card-body\n").unwrap();

    let input = "@include lib.hl as ui\n@ui.card\n";
    let result = htmlang::parser::parse_with_base(input, Some(&dir));
    let html = htmlang::codegen::generate(&result.document);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "no errors expected, got: {:?}",
        result.diagnostics
    );
    assert!(
        html.contains("card-body"),
        "alias prefixed call should expand, got: {}",
        html
    );
}

#[test]
fn error_invalid_json_in_data_directive() {
    let dir = std::env::temp_dir().join("htmlang_test_bad_json");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data_path = dir.join("bad.json");
    std::fs::write(&data_path, "{ not: valid json }").unwrap();

    let input = "@data bad.json\n";
    let result = htmlang::parser::parse_with_base(input, Some(&dir));
    let has_err = result.diagnostics.iter().any(|d| {
        d.severity == htmlang::parser::Severity::Error && d.message.contains("invalid JSON")
    });
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        has_err,
        "expected invalid JSON error, got: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// @markdown file embedding tests
// ---------------------------------------------------------------------------

#[test]
fn markdown_file_renders_content() {
    let dir = std::env::temp_dir().join("htmlang_test_markdown_file");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let md_path = dir.join("article.md");
    std::fs::write(&md_path, "# Hello\n\nThis is **bold** text.\n").unwrap();

    let input = "@markdown article.md\n";
    let result = htmlang::parser::parse_with_base(input, Some(&dir));
    let html = htmlang::codegen::generate(&result.document);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "no errors expected, got: {:?}",
        result.diagnostics
    );
    assert!(
        html.contains("<h1>Hello</h1>"),
        "should render heading from md file, got: {}",
        html
    );
    assert!(
        html.contains("<strong>bold</strong>"),
        "should render bold from md file, got: {}",
        html
    );
}

#[test]
fn markdown_file_with_variable_path() {
    let dir = std::env::temp_dir().join("htmlang_test_markdown_var");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let md_path = dir.join("post.md");
    std::fs::write(&md_path, "# Post Title\n").unwrap();

    let input = "@let file post.md\n@markdown $file\n";
    let result = htmlang::parser::parse_with_base(input, Some(&dir));
    let html = htmlang::codegen::generate(&result.document);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.severity != htmlang::parser::Severity::Error),
        "no errors expected, got: {:?}",
        result.diagnostics
    );
    assert!(
        html.contains("<h1>Post Title</h1>"),
        "should resolve variable path for markdown file, got: {}",
        html
    );
}

#[test]
fn markdown_file_missing_reports_error() {
    let dir = std::env::temp_dir().join("htmlang_test_markdown_missing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let input = "@markdown nonexistent.md\n";
    let result = htmlang::parser::parse_with_base(input, Some(&dir));
    let _ = std::fs::remove_dir_all(&dir);

    let has_err = result.diagnostics.iter().any(|d| {
        d.severity == htmlang::parser::Severity::Error && d.message.contains("cannot read markdown")
    });
    assert!(
        has_err,
        "expected missing file error, got: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Standard library and function calls behaving like elements
// ---------------------------------------------------------------------------

#[test]
fn function_call_text_becomes_children() {
    let output = compile("@let box\n  @el [padding 4]\n    @children\n@box Hello {@text [bold] world}");
    assert!(output.contains("Hello"), "{}", output);
    assert!(output.contains(">world</span>"), "{}", output);
}

#[test]
fn function_call_extra_attributes_style_the_root() {
    let output = compile("@let box $label\n  @el [padding 4] $label\n@box [label Hi, background red, hover:color blue]");
    assert!(output.contains("background:red"), "{}", output);
    assert!(output.contains(":hover{color:blue"), "{}", output);
    assert!(output.contains("Hi"), "{}", output);
}

#[test]
fn function_call_extra_attributes_need_a_single_root() {
    let diags = parse_diagnostics("@let two\n  @text A\n  @text B\n@two [background red]");
    assert!(
        diags.iter().any(|d| d.message.contains("no single root element")),
        "{:?}",
        diags
    );
}

#[test]
fn children_prefix_styles_direct_children() {
    let output = compile("@row [children:flex-shrink 0]\n  @el A");
    assert!(output.contains(" > *{flex-shrink:0;}"), "{}", output);
}

#[test]
fn standard_library_components_compile_cleanly() {
    let src = "@badge [background red] 3\n@tag v2\n@chip Rust\n@avatar\n  AB\n@spacer\n@tooltip [tip More] Hover\n@carousel\n  @el A\n@breadcrumb\n  @item Home\n@el [$skeleton]\n@el [$no-scrollbar]";
    let diags = parse_diagnostics(src);
    assert!(diags.is_empty(), "standard library produced diagnostics: {:?}", diags);
    let output = compile(src);
    assert!(output.contains("<span class=\"a\">3</span>"), "{}", output);
    assert!(output.contains("title=\"More\""), "{}", output);
    assert!(output.contains("@keyframes hl-skeleton"), "{}", output);
}

#[test]
fn own_definition_overrides_standard_library() {
    let output = compile("@let badge $label\n  @text [color green] $label\n@badge [label Mine]");
    assert!(output.contains("color:green"), "{}", output);
    assert!(!output.contains("border-radius:9999px"), "{}", output);
}
