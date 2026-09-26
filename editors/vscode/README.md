# htmlang for VS Code

Syntax highlighting, snippets, and language server support for
[htmlang](https://github.com/iko-soy/htmlang) (`.hl`) files.

## Features

- **Syntax highlighting**: `@` elements and directives, `$variables`,
  `[attribute]` lists with state and media prefixes, `key=value` HTML
  attributes, `{@inline}` elements, colors, numbers and `-- comments`.
- **Diagnostics**: parse errors, unknown elements and attributes with "did
  you mean" suggestions, undefined and unused definitions, and accessibility
  warnings.
- **Completion** of elements, directives, layout attributes, CSS properties,
  HTML attributes, prefixes, variables and functions. It triggers on `@`,
  `$`, `[` and `,`.
- **Hover** documentation for elements, directives and attributes (CSS
  properties link to MDN), function signatures, variable values, and color
  swatches.
- **Navigation**: go to definition, find references and rename for
  `$variables`, bundles and `@let` functions. `@include` paths are links.
- **Code actions**: fixes for common diagnostics, removing unused definitions,
  and extracting a selection into a `@let` function or attribute bundle.
- **Outline and symbols**: the document outline, and `Ctrl-T` workspace
  search across every `.hl` file.
- **Formatting**: `Format Document` and `Format Selection` use the same
  formatter as `htmlang fmt`.
- **Also**: reference counts as code lenses on each `@let`, variable values
  as inlay hints, a color picker, folding, semantic tokens, signature help for
  function calls, and linked editing of a variable's uses.

## Requirements

The extension starts the `htmlang-lsp` binary. Install it from the repository
root:

```
cargo install --path . --bin htmlang-lsp
```

`htmlang-lsp` must be on your `PATH`, or you can set `htmlang.server.path`
to its absolute path. `htmlang.server.args` passes extra arguments to it.

## Snippets

Snippets cover common patterns: `@page`, `@let-fn`, `@let-slots`,
`@let-tokens`, `@navbar`, `@hero`, `@form`, `@grid`, `@each`, `@if`,
`@layout` and more. Type a prefix and press `Tab` to expand it.

## Development

```
cd editors/vscode
npm install
npm run build          # compile TypeScript to ./out
```

Open this folder in VS Code and run the "Extension" launch configuration to
try it.

## Reporting issues

File issues in the main [htmlang repository](https://github.com/iko-soy/htmlang/issues).
Include a minimal `.hl` snippet and the output of `htmlang --version`.
