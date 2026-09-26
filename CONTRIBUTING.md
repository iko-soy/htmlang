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
    server takes definitions, directives and verbatim bodies from it.
  - `parser.rs` — evaluates the tree: definitions, data, loops and
    conditions, and the checks over code that doesn't run.
  - `diagnostic.rs` — `Diagnostic` and the stable diagnostic codes.
  - `ast.rs` — the element kinds; every plain HTML element is one row in
    `TAGS`, and every directive one row in `DIRECTIVES` (its argument, its
    kind of body, and whether it takes the rest of the line).
  - `vocab.rs` — the attribute vocabulary (htmlang attributes, CSS properties,
    HTML attributes) and the state/media prefixes.
  - `expr.rs` — the expression language for conditions and computed values.
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
  and every page in `examples/`.
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
   element is a row in `TAGS`, and a CSS property needs no code at all. Only
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
