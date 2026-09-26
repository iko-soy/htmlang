# htmlang

A minimalist layout language inspired by [elm-ui](https://package.elm-lang.org/packages/mdgriffith/elm-ui/latest/) that compiles to static HTML.

`@` means structure. Bare lines mean content. No CSS required.

## Example

```
@page My Site
@let primary #3b82f6

@let card $title
  @el [padding 20, background white, border-radius 8, border 1 solid #e5e7eb, hover:border 1 solid $primary, transition all 0.15s ease]
    @text [font-weight bold] $title
    @children

@el [max-width 800, center-x, padding 40, spacing 20]
  @text [font-weight bold, font-size 32] Hello

  @paragraph
    Built with {@text [font-weight bold, color $primary] htmlang}.

  @row [wrap, spacing 10]
    @card [title Simple]
      Write layouts without CSS
    @card [title Fast]
      Compiles to a single HTML file
```

This compiles to one self-contained `.html` file with flexbox layout and
generated CSS classes. No JavaScript, no external dependencies.

## Install

```
cargo install --path .
```

## Usage

```
htmlang page.hl              # compile page.hl -> page.html
htmlang page.hl -o out.html  # choose the output file
htmlang -w page.hl           # recompile on change
htmlang serve .              # dev server with live reload
```

| Command | Purpose |
|---|---|
| `build <dir> [-o out] [--minify] [--strict]` | Compile every `.hl` file under a directory |
| `serve [dir\|file] [-p PORT] [--open]` | Dev server with live reload |
| `watch [dir\|file] [-o out]` | Recompile on change, without a server |
| `check <file\|dir> [--format json]` | Report diagnostics without writing output |
| `lint <file\|dir> [--format json]` | Stricter checks (accessibility, nesting) |
| `fmt <file.hl>` | Format a file in place |
| `lsp` | Run the language server over stdio |

Compiling directly also takes `--dev`, `--strict`, `--partial`,
`--format json`, `-s` / `--serve`, `-p` / `--port` and `--open`.

## Editor support

A VS Code extension with syntax highlighting and LSP integration is in
[`editors/vscode`](editors/vscode). The language server (`htmlang-lsp`) provides
diagnostics, completions, hover documentation, go to definition, rename and
formatting.

## Language tour

Elements are laid out with `@el` (a column) and `@row`; `@text`, `@paragraph`,
`@link` and `@image` hold content, and every other HTML element is available by
name (`@nav`, `@section`, `@table`, `@input`, ...).

Attributes are styles, written `key value`, or HTML attributes, written
`key=value`:

```
-- gap between children, padding, and an HTML id
@row [spacing 20, padding 16, id=toolbar]
  -- take the remaining space; center the text
  @el [width fill, text-align center] Search
  @input [type=search, placeholder=Find..., width 240]
```

Any standard CSS property works as a style (`opacity 0.5`, `margin-top 16`).
Prefixes apply a style conditionally:

```
@el [background #3b82f6, hover:background #2563eb, md:padding 32, dark:background #1e3a8a]
  @text [color white] Click me
```

`@let` defines values, computed values, attribute bundles and functions:

```
@let primary #3b82f6
@let gap = 8 * 2
@let card [padding 20, border-radius 8]
@let button $label
  @el [$card, background $primary]
    @text [color white, font-weight bold] $label
    @children

@button [label Click me, padding 12]
```

Functions are used like elements: extra attributes (here `padding 12`) style
their root element.

Control flow runs at compile time:

```
@let items Home, About, Contact
@row [spacing 8]
  @each $item in $items
    @if $item != About
      @link /${lowercase($item)} $item
```

See [DESIGN.md](DESIGN.md) for the full language: files and data (`@include`,
layouts, `@data`), page metadata, expressions and CSS.
