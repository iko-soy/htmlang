# htmlang

A minimalist layout language inspired by [elm-ui](https://package.elm-lang.org/packages/mdgriffith/elm-ui/latest/) that compiles to static HTML.

`@` means structure. Bare lines mean content. No CSS required.

## Example

```
@page My Site
@let primary #3b82f6

@fn card $title
  @el [padding 20, background white, rounded 8, border 1 #e5e7eb, hover:border 1 $primary, transition all 0.15s ease]
    @text [bold] $title
    @children

@column [max-width 800, center-x, padding 40, spacing 20]
  @text [bold, size 32] Hello

  @paragraph
    Built with {@text [bold, color $primary] htmlang}.

  @row [wrap, spacing 10]
    @card [title Simple]
      Write layouts without CSS
    @card [title Fast]
      Compiles to a single HTML file
```

Compiles to a self-contained `.html` file with flexbox layout and generated CSS classes. No JavaScript, no external dependencies.

## Install

```
cargo install --path .
```

## Usage

Everyday workflow:

```
htmlang init                 # scaffold a new project
htmlang page.hl              # compile page.hl -> page.html
htmlang --watch page.hl      # recompile on change
htmlang serve .              # dev server with live reload
htmlang serve --https --cert cert.pem --key key.pem .
htmlang fmt page.hl          # format a file in place
htmlang check page.hl        # parse only, report diagnostics
```

More commands (run with `--help` for flags):

| Command            | Purpose                                        |
|--------------------|------------------------------------------------|
| `build [dir]`      | Compile every `.hl` under a directory          |
| `watch [path]`     | Watch mode without a server                    |
| `serve [path]`     | Dev server with live reload (`--https` ready)  |
| `convert file.html`| Convert HTML back to htmlang                   |
| `new name`         | Scaffold a new page from a template            |
| `components`       | List `@fn` / `@component` definitions          |
| `deps file.hl`     | Print the include / import graph               |
| `dead-code [dir]`  | Report unused `@fn` / `@let` / `@define`       |
| `feed [dir]`       | Generate an Atom feed from `@article` pages    |
| `sitemap [dir]`    | Generate `sitemap.xml`                         |
| `lint [path]`      | Run warnings-only checks, optional JSON output |
| `stats file.hl`    | Compile-time statistics                        |
| `preview file.hl`  | Open a one-off preview                         |
| `diff a.hl b.hl`   | Diff two files at the AST level                |
| `export file.hl`   | Export to plain HTML (`--format pdf` planned)  |
| `repl`             | Interactive REPL for expressions               |
| `deploy`           | Preset-driven deploy helper                    |
| `playground`       | Write a self-contained playground HTML         |
| `clean [dir]`      | Remove generated `.html` files                 |
| `upgrade`          | Apply mechanical migrations to out-of-date `.hl` |
| `outline file.hl`  | Print the document tree                        |
| `explain CODE`     | Longer explanation of a diagnostic code        |
| `lsp`              | Launch the language server (stdio)             |
| `benchmark [dir]`  | Measure parse / codegen timing                 |

## Editor support

A VS Code extension with syntax highlighting and LSP integration is available in [`editors/vscode`](editors/vscode). The language server (`htmlang-lsp`) provides diagnostics, completions, and hover documentation.

## Language overview

### Elements

| Element      | Purpose                      |
|--------------|------------------------------|
| `@row`       | Horizontal flex layout       |
| `@column`    | Vertical flex layout         |
| `@el`        | Generic container            |
| `@text`      | Styled inline text           |
| `@paragraph` | Flowing text with inline elements |
| `@image`     | Image                        |
| `@link`      | Anchor                       |
| `@raw`       | Verbatim HTML escape hatch   |
| `@form`      | Form container               |
| `@details`   | Disclosure widget            |
| `@summary`   | Summary for `@details`       |
| `@blockquote`| Block quotation              |
| `@cite`      | Citation reference           |
| `@code`      | Inline code (monospace)      |
| `@pre`       | Preformatted text            |
| `@hr`        | Horizontal rule / divider    |
| `@figure`    | Figure with caption          |
| `@figcaption`| Caption for `@figure`        |
| `@progress`  | Progress bar                 |
| `@meter`     | Meter/gauge                  |
| `@in-front`  | Overlay layer on top of parent |
| `@behind`    | Layer behind parent content  |

### Layout attributes

```
@row [spacing 20]                  -- gap between children
@row [gap-x 10, gap-y 20]         -- separate horizontal/vertical gaps
@el [padding 20]                   -- uniform padding (also padding-x, padding-y)
@el [width fill]                   -- take remaining space (also width 200, width shrink)
@el [center-x]                     -- center horizontally (also align-left, align-right)
@el [overflow hidden]              -- overflow behavior (hidden, scroll, auto)
```

### Style attributes

```
@el [background #3b82f6, color white, rounded 8, border 1 #e5e7eb]
@text [bold, italic, underline, size 18, font Inter]
@el [opacity 0.5, cursor pointer, transition all 0.15s ease]
@el [shadow 0 2px 4px rgba(0,0,0,0.1)]
@paragraph [text-align center, line-height 1.5]
```

### Positioning

```
@el [position relative]
  @el [position absolute, z-index 10]
    Overlay content
```

For overlays, prefer `@in-front` and `@behind` — they make the parent a
positioning context and stretch to its bounds automatically:

```
@el [width 200, height 200, background blue]
  @text [color white] Main content
  @in-front
    @text [color yellow] Top layer
  @behind
    @el [background red, width fill, height fill]
```

### Pseudo-states and media prefixes

Prefix any style attribute with `hover:`, `active:`, or `focus:`:

```
@el [background #3b82f6, hover:background #2563eb, active:background #1d4ed8]
```

Use `dark:` for dark mode and `print:` for print styles:

```
@el [background white, dark:background #1a1a2e, print:display none]
```

### Variables and defines

```
@let primary #3b82f6              -- simple variable, used as $primary
@let greeting "Hello $name"       -- quoted string interpolation
@define card [padding 20, rounded 8]   -- attribute bundle, used as [$card]
```

### Functions

```
@fn button $label
  @el [padding 12, background $primary, rounded 8]
    @text [color white, bold] $label
    @children                      -- slot for caller's children

@button [label Click me]
```

### File includes

```
@include header.hl                -- inline another .hl file
```

### Multi-line attributes

Attribute lists can span multiple lines:

```
@el [
  padding 20,
  background white,
  rounded 8,
  shadow 0 2px 4px rgba(0,0,0,0.1)
]
  Content here
```

### Other features

- `--` comments
- `{@text [bold] inline}` elements in text
- `@el [attrs] > @link url` single-child chaining
- `[padding 20]` bare attributes as implicit `@el`
- `@raw """..."""` for embedding arbitrary HTML/CSS/JS
- `@image [inline] logo.svg` inlines SVG content
- `@each $name, $url in Home /, About /about` destructuring
- `@let large = $base * 2` computed variables
- `[grid-template-areas "header main", grid-area header]` named grid areas
- `[animate fade-in 0.3s ease]` animation shorthand
- `[view-transition-name hero]` View Transitions API
- `[has(.active):background blue]` `:has()` pseudo-selector
- `@slot header` / `@slot content` named slots in `@fn`
- `@theme` design tokens as runtime CSS custom properties
- VS Code snippets for common patterns

## Documentation

See [DESIGN.md](DESIGN.md) for the full language specification.
