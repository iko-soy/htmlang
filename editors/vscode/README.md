# htmlang for VS Code

Syntax highlighting, snippets, and language server support for
[htmlang](https://github.com/iko-soy/htmlang) (`.hl`) files.

## Features

- **Syntax highlighting**: `@` elements and directives, `$variables`,
  `[attribute]` lists with state and media prefixes, `if(...)` and its
  `[groups]`, `key=value` HTML attributes, quoted text, escapes (`\,`, `\$`, `\]`), `{@inline}` elements,
  colors, numbers and `-- comments`.
- **Diagnostics**: the compiler's own, each with its code: parse errors,
  unknown elements and attributes with "did you mean" suggestions, undefined
  and unused definitions, and accessibility warnings. The whole file is
  checked, including branches and functions that don't run, and relative
  `@include` and `@data` paths resolve from the file's folder.
- **Completion** of elements, directives, layout attributes, CSS properties,
  HTML attributes, prefixes, `if()` (and attributes inside its branches),
  variables, functions and the parameters a function call hasn't passed yet, the slot names of a call's `@slot`
  blocks, and elements and functions inside `{@...}` in text. It triggers on `@`, `$`, `[` and `,`.
- **Hover** documentation for elements, directives, attributes (CSS
  properties link to MDN) and `if()`, function signatures with their slots and
  whether they take content, variable values, and color swatches.
- **Navigation**: go to definition, find references and rename for
  `$variables`, bundles and `@let @name` functions. `@include`, `@markdown` and
  `@data` file paths are links.
- **Code actions**: fixes keyed on diagnostic codes (replace a misspelled
  name or slot name, write a slot name as one word, add a missing `alt` or `type`, include the file that defines an
  unknown function, write a quoted font stack as `A\, B`, drop the `$` from
  `@let $x` and add it to `@each x`, pass a parameter as `name value`
  instead of `name=value`), removing unused
  definitions, and extracting a selection
  into a `@let` function or attribute bundle.
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
`if(` (attributes chosen by a condition), `@layout` and more. Type a prefix and press `Tab` to expand it.

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
