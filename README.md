# htmlang

A small layout language that compiles to static HTML. The layout model comes
from [elm-ui](https://package.elm-lang.org/packages/mdgriffith/elm-ui/latest/):
rows and columns, spacing between children, and elements that fill or center
themselves. Styling uses CSS properties under their CSS names, and every other
element uses its HTML name.

```
@page My Site
@let --brand #3b82f6

@let card $title
  @article [padding 20, spacing 8, border 1 solid #e5e7eb, border-radius 8, hover:border-color var(--brand)]
    @h3 $title
    @children

@el [max-width 800, center-x, padding 40, spacing 24]
  @h1 [font-size 32] Hello
  @paragraph
    Built with {@text [font-weight bold, color var(--brand)] htmlang}.
  @row [spacing 16, wrap]
    @card [title Simple]
      Layout without writing a stylesheet.
    @card [title Static]
      One HTML file, with its CSS inside.
```

Each page compiles to one self-contained `.html` file. It has no JavaScript
unless you write some, and nothing is added that you didn't ask for.

## The language in brief

- **`@` starts structure, and any other line is content.** Indentation nests
  elements. Text after an element's attributes is its content, and `{...}`
  puts an element inside a line of text.
- **Layout comes from elm-ui.** `@el` lays out its children in a column and
  `@row` in a row, and every other container is a column too. Layout
  attributes (`spacing`, `width fill`, `center-x`, `align-right`, `wrap`) say
  how an element sits inside its parent.
- **Styling is CSS.** Any other attribute is a CSS property with its CSS
  name and value: `padding 20`, `border 1 solid #eee`,
  `grid-template-columns 1fr 2fr`. In lengths, a bare number means pixels.
  Prefixes make a style conditional: `hover:`, `md:`, `dark:`, `first:`.
- **HTML stays HTML.** Elements have their HTML names (`@nav`, `@ul`, `@form`,
  `@details`), and HTML attributes are written `key=value` (`id=main`,
  `type=email`) or bare (`required`).
- **`@let` defines everything.** It defines values, computed values,
  attribute bundles and functions. A layout is just a function with slots.
- **Data comes in as lists and records.** `@data` loads JSON, and `@each` and
  `@if` run at compile time.

## Install

```
cargo install --path .
```

## Usage

```
htmlang page.hl              # compile page.hl to page.html
htmlang page.hl -o out.html  # choose the output file
htmlang serve .              # dev server with live reload
htmlang build src -o dist    # compile a whole site
```

| Command | Purpose |
|---|---|
| `build <dir> [-o out] [--minify] [--strict]` | Compile every `.hl` file under a directory (into `out/` by default) |
| `serve [dir\|file] [-p PORT] [--open]` | Dev server with live reload |
| `watch [dir\|file] [-o out]` | Recompile on change, without a server |
| `check <file\|dir> [--format json]` | Report diagnostics without writing output |
| `lint <file\|dir> [--format json]` | Stricter checks (accessibility, nesting) |
| `fmt <file.hl>` | Format a file in place |
| `lsp` | Run the language server over stdio |

Compiling a file directly also takes `-w` / `--watch`, `--dev` (readable
output and source maps), `--strict`, `--partial` (a fragment, without the
document wrapper), `--format json`, `-s` / `--serve`, `-p` / `--port` and
`--open`.

## A short tour

Layout attributes place an element. Everything else is CSS, and `key=value`
is an HTML attribute:

```
@row [spacing 12, align-items center, padding 12 20, border-bottom 1 solid #e5e7eb, id=toolbar]
  @text [font-weight 700] Acme
  @spacer
  @input [type=search, placeholder=Search, aria-label=Search, width 240, padding 6 10]
  @button [type=submit, padding 6 14, hover:background #f3f4f6] Go
```

A prefix applies a style only in a state, from a screen width up, or under a
media condition:

```
@el [padding 16, md:padding 32, background #3b82f6, hover:background #2563eb, dark:background #1e3a8a]
  @text [color white] Click me
```

`@let` defines values, bundles and functions. A function is called like an
element: its parameters are attributes, and any other attributes style its
root element.

```
@let --primary #3b82f6
@let gap = 8 * 2
@let rounded [border-radius 8, overflow hidden]

@let button $label $href=#
  @link [$rounded, padding 10 16, background var(--primary), color white] $href
    $label

@row [spacing $gap]
  @button [label Sign up]
  @button [label Learn more, href /about, background #64748b]
```

Data, loops and conditions run at compile time:

```
@data $links [
  {"label": "Home", "url": "/"},
  {"label": "Blog", "url": "/blog"},
  {"label": "About us", "url": "/about"}
]
@let current /blog

@nav
  @row [spacing 16]
    @each $link in $links
      @link [if($link.url == $current, font-weight bold)] $link.url $link.label
```

The [`examples/`](examples) directory has complete pages (a landing page, a
blog, a docs page, and a tour of the whole language). [DESIGN.md](DESIGN.md)
is the language reference.

## Editor support

The VS Code extension in [`editors/vscode`](editors/vscode) provides syntax
highlighting, snippets, and the language server (`htmlang-lsp`). The server
gives diagnostics, completions, hover documentation, go to definition, rename
and formatting.

The compiler, the formatter and the language server read a file with the
same parser, so the editor sees exactly what the compiler sees, relative
`@include` and `@data` paths included. Every diagnostic has a stable code
(`error[unknown-element]` on the command line, a `code` field in
`--format json`), and the whole file is checked, including branches and
functions that don't run.
