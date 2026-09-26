# htmlang for VS Code

Syntax highlighting, snippets, and language server support for
[htmlang](https://github.com/iko-soy/htmlang) (`.hl`) files.

## Features

- **Syntax highlighting**: `@` elements and directives, `$variables`,
  `[attribute]` lists (`@page`'s included) with state and media prefixes
  (stacked, `md:hover:`, and on a group or bundle, `md:[...]`, `dark:$card`)
  and element prefixes (`@td:`, `@link:hover:`),
  `if(...)` and its `[groups]`, `key=value` HTML attributes (`xml:lang=` too), the leading argument of
  `@link`, `@image`, `@script` and the other elements that take one, quoted text, escapes (`\,`, `\$`, `\]`), `{@inline}` elements
  (shown as text in `@code` and `@textarea`), the one-line body of `@raw`,
  `@style` and `@head` as raw text,
  `@each $item in` headers and ranges (`1..5`), custom and vendor-prefixed
  property names (`--gap`, `md:--gap`, `-webkit-text-stroke`), colors,
  numbers and `-- comments` (`--` followed by a space, also between the lines
  of an attribute list).
- **Diagnostics**: the compiler's own, each with its code: parse errors,
  unknown elements, prefixes and CSS properties with "did you mean"
  suggestions, anything that would be left out of the page, undefined and
  unused definitions, and accessibility warnings. The whole file is
  checked, including branches and functions that don't run, and relative
  `@include` and `@data` paths resolve from the file's folder.
- **Completion** of elements, directives, layout attributes (`spacing`,
  `wrap` and `grid-cols` only on a row, column or grid), CSS properties,
  HTML attributes (not the one an element's leading argument already
  gives, as `href=` in `@link [] /about About`; on `@page`, `lang=`,
  `dir=`, `class=` and `favicon` first), the custom properties the file
  names (`--surface`, and `dark:--surface` after a prefix), prefixes (CSS's
  pseudo-classes and pseudo-elements, one with an argument placing the
  cursor between its parentheses, as `nth-child(|):`; and
  after one, those that can follow it, as `md:hover:`; after a
  pseudo-element such as `before:`, only media, width and container
  prefixes; after `@`, the element prefixes, `@td:`, of the elements with
  an HTML tag of their own, and after one, CSS properties only), only
  styles inside a prefixed group (`md:[...]`), `if()` (and attributes inside its branches),
  the variables visible where you type (definitions above in the block and
  the blocks around it, `@each` variables and a function's parameters),
  functions and the parameters a function call hasn't passed yet, the slot names of a call's `@slot`
  blocks, and elements and functions inside `{@...}` in text. It triggers on `@`, `$`, `[` and `,`.
- **Hover** documentation for elements (with their layout: column, row,
  grid, text, native or void), directives, attributes (CSS
  properties link to MDN), prefixes and stacks of them (`md:hover:`, `@td:`) and `if()`, function signatures with their slots and
  whether they take content, variable values, and color swatches.
- **Navigation**: go to definition (the definition a name means at that
  line), find references and rename for
  `$variables`, bundles and `@let @name` functions. `@include`, `@markdown` and
  `@data` file paths are links.
- **Code actions**: fixes keyed on diagnostic codes (replace a misspelled
  name, prefix (`@tdd:` as `@td:`, `@a:` as `@link:`) or slot name, put a pseudo-element's prefix last
  (`before:hover:` as `hover:before:`), replace `@a` with `@link`, write a slot name as one word, add a missing `alt` or `type`, include the file that defines an
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
`@let-tokens`, `@navbar`, `@hero`, `@form`, `@grid`, `@table` (its cells
padded by `@td:`), `@markdown-styled`, `@each`, `@if`,
`if(` (attributes chosen by a condition), `@code-sample` (`@pre > @code`
over a block shown as written), `@layout` and more. Type a prefix and press `Tab` to expand it.

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
