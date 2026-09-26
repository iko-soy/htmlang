# htmlang for VS Code

Syntax highlighting, snippets, and LSP-backed intelligence for [htmlang](https://github.com/iko/htmlang)
(`.hl`) files.

## Features

- **Syntax highlighting** — `@` directives, `$variables`, `[attribute]` brackets,
  `-- comments`, verbatim `@raw` / `@style` / `@script` bodies.
- **Diagnostics** — parse errors, missing variables/functions, unused bindings,
  CSS / a11y warnings, and typo suggestions.
- **Completion** — directives, attribute keys and values, variable names, and
  function calls. Trigger characters: `@`, `$`, `[`, `,`.
- **Hover** — documentation for directives, function signatures, variable
  values, and color swatches.
- **Go to definition / rename** — for `$variables`, `@fn` calls, `@define`
  bundles, and `@include` targets.
- **Document links** — cmd-click on `@include`.
- **Code lens** — per-definition reference counts for `@fn` / `@let` / `@define`.
- **Document symbols + workspace symbols** — outline view and `Ctrl-T` search
  scan both open documents and every `.hl` file in the workspace.
- **Formatter** — `Format Document` / `Format Selection` invokes the same
  formatter as `htmlang fmt`.
- **Color picker, folding ranges, semantic tokens, inlay hints, signature help**.

## Requirements

The extension launches the `htmlang-lsp` binary. Install it from the repo root:

```
cargo install --path . --bin htmlang-lsp
```

Make sure `htmlang-lsp` is on your `PATH`, or set `htmlang.serverPath` in
settings to an absolute path.

## Snippets

A set of snippets is included for common patterns (`card`, `row`, `column`,
`fn`, `each`, `component`, …). Type the prefix and `Tab` to expand.

## Development

```
cd editors/vscode
npm install
npm run build          # compile TypeScript to ./out
```

Open this folder in VS Code and run the "Extension" launch configuration to
try it out.

## Reporting issues

Please file issues in the main [htmlang repository](https://github.com/iko/htmlang/issues),
including a minimal `.hl` snippet and your `htmlang-lsp --version`.
