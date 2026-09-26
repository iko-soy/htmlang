# Contributing

Thanks for your interest in improving htmlang. This file covers the practical
bits: how to run the project locally, where the code lives, and what style of
changes land smoothly.

## Development

```
cargo build                     # build the CLI + LSP binaries
cargo test                      # run all tests (unit + snapshot + integration)
cargo run -- page.hl            # compile a .hl file to page.html
cargo run -- serve examples     # run the dev server at http://127.0.0.1:3000
```

CI runs `cargo test`, `cargo fmt --check`, and `cargo clippy -- -D warnings` on
Linux, macOS, and Windows. Your change should pass all three.

## Repository layout

- `crates/htmlang-core/` — parser, AST, code generator. No I/O lives here.
  - `syntax.rs` — the syntax tree: every line as written, with spans. The
    compiler evaluates it, the formatter prints it back, and the language
    server takes definitions, directives and verbatim bodies from it. It
    also holds `ESCAPES`, the one escape table of every htmlang string,
    and decides where `{@...}` is an inline element: in every line of text
    except the text of a `literal` element.
  - `parser.rs` — evaluates the tree: definitions, data, loops and
    conditions, and the checks over code that doesn't run. Names live in
    `Env`, one frame per block, so a definition is visible to the end of
    its block and a function keeps the frames of its definition.
  - `diagnostic.rs` — `Diagnostic` and the stable diagnostic codes.
  - `codegen.rs` — writes the HTML and the CSS. A parent's `Flow` (the
    direction its CSS sets, with the changes under media and container
    prefixes) is what its children's `fill` and `shrink` compile against;
    the rules for a change are keyed on the parent's class in that block.
    A page's `<body>` is written like an `@el` whose styles are `@page`'s
    (the reset makes it the column), so the top level of a page is laid
    out like any column; a fragment's top level has no layout of its own.
    Every CSS property goes through one generic path (its name, its value
    as written, with `vocab::with_px`'s pixels); only htmlang's layout
    words and `line-clamp` have code of their own. Generated classes are
    `hl-` + a short name; an element's defaults (its layout,
    `ElementKind::css`) are part of its class, written first as
    `:where(.hl-a)` so its own styles and a parent's `children:` styles
    override them, never a rule on a global element selector, so
    `@markdown`/`@raw` HTML and a page embedding a fragment are untouched.
    A style's prefixes are a `Condition`: its at-rule prefixes (sorted, so
    `dark:md:` is `md:dark:`, and written as nested `@media`/`@container`
    blocks) and its selector chain (read left to right). Each class keeps
    one list of (condition, declarations), and every block and chain is
    written in one fixed order, never the order of the source.
  - `ast.rs` — the element kinds; every plain HTML element is one row in
    `TAGS` with its one `Layout` (column, row, grid, text, native or void,
    which decides its text lines, `spacing` and its children's layout
    words), whether its text is shown as written (`literal`: `@code`,
    `@textarea`, whose indented body is verbatim text, HTML-escaped), the
    one HTML attribute its first word fills (`arg`: `src`, `action`, ...;
    `ElementKind::arg` gives `@link`'s and `@image`'s too) and whether its
    body is foreign text written into the page as it is (`verbatim`:
    `@script`), and every directive one row in `DIRECTIVES` (its argument
    and its kind of body; a verbatim directive's line is its one-line body
    or file, and it takes no attributes). The list of
    elements is fixed; `HTML_NAMES_WRITTEN_OTHERWISE` maps HTML's `a`, `img`,
    `span`, `p` and `div` to htmlang's own names for the unknown-element
    suggestion only.
  - `vocab.rs` — the attribute vocabulary (htmlang attributes, CSS properties,
    HTML attributes, `@page`'s own `favicon`), the pixel rule
    (`LENGTH_PROPERTIES`, the one table of properties whose bare numbers
    are px, and `with_px`, the one function that applies it) and the
    state/media prefixes, with the rank that orders the at-rule blocks.
  - `expr.rs` — the expression language for conditions, computed values and
    `${...}`. It evaluates only what decides the result (the branch `if()`
    takes, the side of `and`/`or` that decides) and only reads the rest.
  - `value.rs` — the value types (text, number, true/false, list,
    record) and the rules defined once on them: truthiness, comparison,
    how numbers and lists print, ranges.
  - `interp.rs` — `$name` and `${...}`: where a name ends and how one slot
    of text (a text run, an attribute's value, an argument, a path) is
    filled in, including what quoted text inserts in CSS and in text. The
    one place variables are interpolated.
  - `std.hl` — the standard library, written in htmlang and loaded before
    every file.
- `crates/htmlang-wasm/` — thin wrapper exposing `compile` to the web playground.
- `src/` — CLI, dev server and formatter (`fmt.rs`, which prints the syntax
  tree back with its comments and blank lines).
- `src/bin/htmlang_lsp/` — language server binary (`htmlang-lsp`). Its
  completions come from the compiler's tables; hover and completion text live
  in `docs.rs`.
- `editors/vscode/` — VS Code extension.
- `tests/snapshots.rs` — integration / snapshot tests for the compiler.
- `tests/regressions.rs` — one test per fixed bug.
- `tests/docs.rs` — compiles every example in `DESIGN.md` and `README.md`,
  and every page in `examples/`: any diagnostic fails it.
- `tests/fmt.rs` — formats every example and snapshot input: the result is
  stable, compiles to the same HTML and keeps every comment.
- `examples/` — complete pages: a landing page, a blog, a docs page, and a
  tour of the whole language (`demo.hl`).

## Adding a feature

1. Add a test first. Most language features fit as a new `#[test]` in
   `tests/snapshots.rs`; prefer integration tests that exercise the full
   parser-to-HTML pipeline. Pure parser / codegen helpers can live as unit
   tests alongside the code.
2. Prefer the smallest mechanism: a component belongs in `std.hl`, an HTML
   element is a row in `TAGS` (and a line in DESIGN.md's table of layouts
   and a name in the VS Code grammar's element list, which tests check), and
   a CSS property needs no code at all. Only
   thread a feature through the parser and codegen when it needs to be, and
   describe it in the LSP's `docs.rs`.
3. Document it in `DESIGN.md` (the examples there are compiled by the tests).
   If it's user-facing, also update `README.md`, and show it in
   `examples/demo.hl` if the tour has a section it belongs in.
4. If it changes the CLI surface, update the `--help` output.

## Style

- Prefer enum-based ASTs and pattern matching over stringly-typed dispatch.
- Keep `@` prefixes and bracket-attribute syntax consistent with existing
  directives. A new `@foo` is a row in `ast::DIRECTIVES`; the parser, the
  formatter and the language server read that table, and `tests/editor.rs`
  fails when the VS Code grammar's word lists drift from it.
- Every diagnostic has a code from `diagnostic::code`; add one there when no
  existing code fits, and key tool behavior (quick fixes, tests) on the code
  rather than on the message. Put the name the diagnostic is about in
  `subject` and a replacement in `suggestion`.
- Diagnostics should include `line` and, when practical, `column` and a
  `source_line` excerpt. Use `Severity::Help` for suggestions, not `Warning`.
- Check names, not values, and never drop input silently. When something
  the author wrote can't go into the page, report an error; a warning means
  it went into the page as written. Check a CSS value only for what is
  wrong in any CSS (see `css_breakout` in `parser.rs`), not against a list
  of values the browser may know better.
- No unwrap() on parsed user input. Use `Result<_, ParseError>` and record a
  diagnostic so the compiler keeps going.

## Reporting bugs

Bug reports are most useful when they include:

- The exact `.hl` input that reproduces the issue (minimized if possible).
- The command you ran and the output you saw.
- The output you expected instead.
- `htmlang --version` and your OS.

## License

By contributing, you agree that your changes will be licensed under the same
terms as the rest of the project.
