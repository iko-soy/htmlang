# htmlang (.hl)

A minimalist layout language inspired by elm-ui that compiles to static HTML.

## Principles

- `@` means structure, bare lines mean content
- Layout is explicit and compositional — no CSS cascade
- Every element declares its own layout role
- Output is a self-contained HTML file with embedded CSS (flexbox)

## Elements

| Element      | Output           | Purpose                        |
|--------------|------------------|--------------------------------|
| `@row`       | flex row         | Horizontal layout              |
| `@column`    | flex column      | Vertical layout                |
| `@el`        | div              | Generic container              |
| `@text`      | span             | Styled inline text             |
| `@paragraph` | p with spans     | Flowing/wrapping inline text   |
| `@image`     | img              | Image                          |
| `@link`      | a                | Anchor wrapping children       |
| `@raw`       | verbatim HTML    | Escape hatch                   |
| `@nav`       | nav              | Navigation landmark            |
| `@header`    | header           | Page/section header            |
| `@footer`    | footer           | Page/section footer            |
| `@main`      | main             | Main content area              |
| `@section`   | section          | Thematic section               |
| `@article`   | article          | Self-contained content         |
| `@aside`     | aside            | Sidebar/tangential content     |
| `@list`      | ul/ol            | List (`[ordered]` for ol)      |
| `@item`      | li               | List item                      |
| `@table`     | table            | Table                          |
| `@thead`     | thead            | Table head group               |
| `@tbody`     | tbody            | Table body group               |
| `@tr`        | tr               | Table row                      |
| `@td`        | td               | Table cell                     |
| `@th`        | th               | Table header cell              |
| `@video`     | video            | Video element                  |
| `@audio`     | audio            | Audio element                  |
| `@form`      | form             | Form container                 |
| `@details`   | details          | Disclosure widget              |
| `@summary`   | summary          | Summary for `@details`         |
| `@blockquote`| blockquote       | Block quotation                |
| `@cite`      | cite             | Citation/source reference      |
| `@code`      | code             | Inline code (monospace)        |
| `@pre`       | pre              | Preformatted text              |
| `@hr`        | hr               | Horizontal rule (self-closing) |
| `@figure`    | figure           | Figure with optional caption   |
| `@figcaption`| figcaption       | Caption for `@figure`          |
| `@progress`  | progress         | Progress bar                   |
| `@meter`     | meter            | Meter/gauge element            |
| `@iframe`    | iframe           | Embedded external page         |
| `@output`    | output           | Form calculation result        |
| `@canvas`    | canvas           | Drawing surface for scripts    |

| `@script`  | script           | JavaScript (inline or external) |
| `@noscript` | noscript         | Fallback when JS is disabled   |
| `@address` | address          | Contact information             |
| `@search`  | search           | Search landmark (HTML5)         |
| `@breadcrumb` | nav>ol        | Semantic breadcrumb navigation  |
| `@in-front` | div (absolute) | Overlay layer painted on top of parent |
| `@behind`   | div (absolute) | Layer painted behind the parent's content |

Bare lines (not starting with `@` or `[`) are text nodes.

## Syntax

### Basic structure

```
@element [attributes]
  children
```

Children are indented under their parent. Attributes are optional, comma-separated inside `[...]`. Attribute lists can span multiple lines:

```
@el [
  padding 20,
  background white,
  rounded 8,
  shadow 0 2px 4px rgba(0,0,0,0.1)
]
  Content
```

### Full example

```
-- My Site
@page My Site
@let primary #3b82f6
@let gap 20
@define card [
  padding 20,
  background white,
  rounded 8,
  border 1 #e5e7eb,
  shadow 0 2px 4px rgba(0,0,0,0.05)
]

@include header.hl

@column [max-width 800, center-x, padding 40, spacing $gap]
  @row [spacing $gap]
    @column [width fill, spacing 10]
      @text [bold, size 32] Welcome
      @paragraph [line-height 1.6]
        This is a page built with {@text [bold, color $primary] htmlang}.
        Read the {@link https://docs.example.com docs} to learn more.
    @image [width 80, height 80, rounded 40] avatar.png

  -- cards
  @row [wrap, spacing 10]
    [$card]
      First card
    [$card, background #f9fafb]
      Second card

  -- footer
  @row [spacing 10]
    @el [padding 16, background $primary, rounded 8] > @link https://example.com
      @text [color white] Get Started
    @el [width fill]
    @text [color #888, text-align right] © 2026

  @raw """
  <canvas id="chart"></canvas>
  """
```

## Directives

### `@page`

Sets the HTML `<title>` and generates boilerplate (`<!DOCTYPE>`, `<html>`, `<head>`, `<body>`).

```
@page My Site
```

### `@let`

Defines a variable. Referenced with `$name`.

```
@let primary #3b82f6
@let gap 20

@el [background $primary, spacing $gap]
```

Quoted values support string interpolation:

```
@let name World
@let greeting "Hello $name"   -- "Hello World"
@let base /api
@let url "$base/users"        -- "/api/users"
```

### `@include`

Inlines another `.hl` file. The path is resolved relative to the current file. Variables defined with `@let`, attribute bundles from `@define`, and functions from `@fn` in the included file are available after the `@include` line.

```
@include header.hl
@include components/card.hl
```

Variables can be used in the filename:

```
@let component card
@include $component.hl
```

Circular includes are detected and reported as errors. Nested includes are supported.

### `@import`

Like `@include`, but only imports definitions (`@let`, `@define`, `@fn`) without emitting DOM nodes. Use this for shared theme/component libraries.

```
@import theme.hl     -- imports variables, defines, functions only
@include header.hl   -- inlines everything including DOM nodes
```

### `@data`

Loads a JSON file and makes its values available as template variables. Useful for data-driven static sites.

```
@data site.json              -- top-level object keys become variables
@data $posts data/posts.json -- load with a prefix
```

For a JSON file like:
```json
{
  "title": "My Site",
  "tags": ["rust", "html", "css"],
  "links": [
    {"label": "Home", "url": "/"},
    {"label": "About", "url": "/about"}
  ]
}
```

Without prefix (`@data site.json`):
- `$title` → `"My Site"`
- `$tags` → iterable with `@each`
- `$links` → destructurable: `@each $label, $url in $links`

With prefix (`@data $site site.json`):
- `$site.title` → `"My Site"`
- `$site.tags` → iterable
- `$site.links` → destructurable

Arrays of objects are automatically formatted for `@each` destructuring. Indexed access is also available: `$links.0.label`, `$links.1.url`, etc. The count is available as `$links._count`.

### `@meta`

Adds a `<meta>` tag to the document `<head>`. Requires `@page`.

```
@meta description A portfolio site
@meta og:image https://example.com/preview.png
```

### `@head`

Adds raw content to the document `<head>`. Use for external fonts, favicons, or custom CSS/JS.

```
@head
  <link rel="icon" href="favicon.ico">
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter">
```

### `@define`

Creates a named attribute bundle. Referenced with `$name` inside attribute lists.

```
@define card [padding 20, background white, rounded 8, border 1 #e5e7eb]

@el [$card]
  Content
@el [$card, background #f9fafb]
  Overridden background
```

Attributes listed after `$name` override those in the definition.

### `@mixin`

Creates a composable style group. Like `@define`, but intended for reusable style patterns. Referenced with `...$name` (spread syntax) or `$name` inside attribute lists.

```
@mixin card-style [padding 20, rounded 8, background white]
@mixin interactive [cursor pointer, hover:background #e5e7eb, transition all 0.15s ease]

@el [...$card-style, ...$interactive, border 1 #ccc]
  Composed styles from two mixins
```

### `@assert`

Compile-time assertion. If the condition is false, a compile error is emitted. Useful for enforcing design system constraints.

```
@let spacing 16
@let primary #3b82f6

@assert $spacing == 16
@assert $primary != red
```

### `@fn`

Defines a pure function (reusable component). Parameters are prefixed with `$`. Default values can be specified with `=`.

```
@fn card $title $variant=primary
  @el [padding 20, background white, rounded 8, border 1 #e5e7eb]
    @text [bold, size 18] $title
    @children
```

Call it like any element, passing parameters in `[...]`:

```
@card [title Hello World]
  This is the card body.
  @text [italic] With styled text.

@card [title Warning, variant danger]
  Overridden variant.
```

Parameters with defaults can be omitted in calls — the default value is used.

### `@children`

A slot inside a function body that expands to the caller's indented children.

```
@fn layout $title
  @column [max-width 800, center-x, padding 40]
    @text [bold, size 32] $title
    @children

@layout [title My Page]
  @row [spacing 10]
    Content goes here
```

Functions can call other functions. `@let` and `@define` variables are available inside function bodies. Function parameters shadow `@let` variables of the same name within the body.

## Text

### Bare text

Any line not starting with `@` or `[` is a text node.

```
@column
  Hello world
  This is just text
```

### Styled text

Use `@text` when you need attributes.

```
@text [bold, size 24, color #333] Hello world
```

### Inline elements

Use `{...}` to embed elements inside text lines. Essential for mixed-style paragraphs.

```
@paragraph
  This is {@text [bold] important} and this is a {@link https://example.com link}.
```

## Comments

`--` starts a comment. The rest of the line is ignored.

```
-- this is a comment
@row [spacing 10]
  -- todo: add nav items
```

## Implicit `@el`

A line starting with `[` (attributes) that has indented children is an anonymous container (implicit `@el`).

```
[padding 20, background white]
  Hello

-- equivalent to:
@el [padding 20, background white]
  Hello
```

## Single-child chaining

Use `>` to chain single-child elements on one line, reducing nesting depth.

```
@el [padding 16, background blue, rounded 8] > @link https://example.com
  @text [color white] Get Started

-- equivalent to:
@el [padding 16, background blue, rounded 8]
  @link https://example.com
    @text [color white] Get Started
```

The last element in the chain takes the indented children.

## `@raw`

Triple-quoted block pasted verbatim into output. Use for arbitrary HTML, CSS, or JS.

```
@raw """
<style>
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
<div class="custom-widget"></div>
"""
```

## Units

All numeric values default to pixels. You can use CSS units explicitly:

```
@el [width 50%, height 100vh, padding 2rem, max-width 80ch, size 1.2em]
```

Supported units: `%`, `rem`, `em`, `vh`, `vw`, `vmin`, `vmax`, `dvh`, `svh`, `ch`, `ex`, `cm`, `mm`, `in`, `pt`, `pc`, `fr`.

## New elements

### `@form`

Form container. Argument is the `action` URL.

```
@form [method post] /submit
  @label [for email] Email
  @input [type email, name email, required]
  @button [type submit] Send
```

### `@details` / `@summary`

Native disclosure widget. Use `[open]` to expand by default.

```
@details [open]
  @summary FAQ Question
  @text The answer is here.
```

### `@blockquote` / `@cite`

Semantic quotation with optional citation.

```
@blockquote [padding 20, border-left 4 #ccc]
  @text To be or not to be
  @cite Shakespeare
```

### `@code` / `@pre`

Code and preformatted text. `@code` renders as `<code>` with monospace font. `@pre` preserves whitespace.

```
@pre
  @code console.log("hello")
```

### `@hr`

Horizontal rule / divider (self-closing). Alias: `@divider`.

```
@hr [border-top 1 #e5e7eb]
```

### `@figure` / `@figcaption`

Figure with optional caption.

```
@figure
  @image [alt Sunset, width fill] sunset.jpg
  @figcaption A beautiful sunset
```

### `@progress` / `@meter`

Progress bar and meter elements. Use `value`, `max`, `min` attributes.

```
@progress [value 70, max 100]
@meter [value 6, min 0, max 10, low 3, high 8]
```

### `@in-front` / `@behind`

Overlay layers, inspired by elm-ui's `inFront` and `behind`. Children of these
directives are absolutely positioned to fill the parent's bounds; the parent
automatically becomes a positioning context (`position: relative` and
`isolation: isolate`).

```
@el [width 200, height 200, background blue]
  @text [color white] Main content
  @in-front
    @text [color yellow] Painted on top
  @behind
    @el [background red, width fill, height fill]
```

`@in-front` paints on top of the parent's content (DOM order ensures it
overlays static siblings). `@behind` paints behind (`z-index: -1` keeps it
under the siblings, while `isolation: isolate` on the parent prevents it from
slipping behind the parent's own background).

If you set `position` explicitly on the parent (e.g., `position absolute` for a
popup), that wins over the auto-applied `position: relative`.

## `@each` destructuring

When `@each` has more than one variable and items contain spaces, values are destructured:

```
@each $name, $url in Home /, About /about, Contact /contact
  @link $url $name
```

`var()` and `calc()` expressions are also passed through as-is.

## `@each` with `@else`

When the list is empty, the `@else` block is rendered as a fallback:

```
@each $item in $items
  @text $item
@else
  @text [color #888] No items found.
```

## `@iframe`

Embedded external page. Argument is the `src` URL.

```
@iframe [width fill, height 400, sandbox] https://example.com
```

Attributes: `sandbox`, `allow`, `allowfullscreen`, `referrerpolicy`.

## `@canvas`

Drawing surface for JavaScript. Use with `@raw` for scripts.

```
@canvas [width 400, height 300, id chart]
```

## `@output`

Form output element for displaying calculation results.

```
@output [for a b] Result
```

## Pseudo-elements

Prefix any style attribute with `before:` or `after:` to apply it to the `::before` or `::after` pseudo-element. Use with `content` to set the generated content.

```
@el [before:content "→ ", before:color red, before:font-weight bold]
  Item with arrow prefix

@el [after:content " ✓", after:color green]
  Completed item
```

Content values are automatically quoted unless they are CSS keywords (`none`, `normal`) or CSS functions (`attr()`, `counter()`).

## Conditional attribute values

Use `if(condition, true_value, false_value)` inside attribute values for conditional styling:

```
@let active true
@let theme dark

@el [background if($active, blue, gray)]
  Conditionally styled

@el [color if($theme == dark, white, black)]
  Theme-aware text
```

Supports equality (`==`), inequality (`!=`), and truthy checks.

## Attributes reference

### Layout (set on parent)

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `spacing N`            | Gap between children             |
| `gap N`               | Alias for `spacing`              |
| `gap-x N`             | Horizontal gap (column-gap)      |
| `gap-y N`             | Vertical gap (row-gap)           |
| `padding N`            | Uniform padding                  |
| `padding Y X`         | Vertical + horizontal padding    |
| `padding T H B`       | Top + horizontal + bottom        |
| `padding T R B L`     | Per-side padding                 |
| `padding-x N`         | Horizontal padding               |
| `padding-y N`         | Vertical padding                 |

### Sizing (set on child)

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `width fill`           | Take remaining space (flex: 1)   |
| `width N`              | Exact pixels                     |
| `width shrink`         | Fit to content (default)         |
| `height fill`          | Take remaining space             |
| `height N`             | Exact pixels                     |
| `height shrink`        | Fit to content (default)         |
| `min-width N`          | Minimum width                    |
| `max-width N`          | Maximum width                    |
| `min-height N`         | Minimum height                   |
| `max-height N`         | Maximum height                   |

### Alignment (set on child)

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `center-x`            | Center horizontally              |
| `center-y`            | Center vertically                |
| `align-left`          | Align to left                    |
| `align-right`         | Align to right                   |
| `align-top`           | Align to top                     |
| `align-bottom`        | Align to bottom                  |

### Style

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `background COLOR`     | Background color                 |
| `color COLOR`          | Text color                       |
| `border N COLOR`       | Border width and color           |
| `border-top N COLOR`   | Top border                       |
| `border-bottom N COLOR`| Bottom border                    |
| `border-left N COLOR`  | Left border                      |
| `border-right N COLOR` | Right border                     |
| `rounded N`            | Border radius                    |
| `shadow VALUE`         | Box shadow (CSS value)           |
| `bold`                 | Bold text                        |
| `italic`               | Italic text                      |
| `underline`            | Underlined text                  |
| `size N`               | Font size in px                  |
| `font NAME`            | Font family                      |
| `text-align VALUE`     | Text alignment (left/center/right/justify) |
| `line-height VALUE`    | Line height (unitless or px)     |
| `letter-spacing N`     | Letter spacing                   |
| `text-transform VALUE` | Transform (uppercase/lowercase/capitalize) |
| `white-space VALUE`    | White-space (nowrap/pre/normal)  |
| `transition VALUE`     | CSS transition                   |
| `cursor VALUE`         | Cursor style                     |
| `opacity VALUE`        | Opacity (0–1)                    |
| `overflow VALUE`       | Overflow (hidden/scroll/auto/visible) |
| `position VALUE`       | Position (relative/absolute/fixed/sticky) |
| `top N`                | Top offset (positioned elements) |
| `right N`              | Right offset                     |
| `bottom N`             | Bottom offset                    |
| `left N`               | Left offset                      |
| `z-index N`            | Stack order                      |
| `display VALUE`        | Display mode (none/block/flex/grid) |
| `visibility VALUE`     | Visibility (visible/hidden)      |
| `transform VALUE`      | CSS transform (e.g., rotate(45deg)) |
| `backdrop-filter VALUE`| Backdrop filter (e.g., blur(10px)) |

### Margin

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `margin N`             | Uniform margin                   |
| `margin Y X`          | Vertical + horizontal margin     |
| `margin T R B L`      | Per-side margin                  |
| `margin-x N`          | Horizontal margin                |
| `margin-y N`          | Vertical margin                  |

### Additional Style

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `filter VALUE`         | CSS filter (blur, brightness)    |
| `object-fit VALUE`     | Object fit (cover/contain/fill)  |
| `object-position VALUE`| Object position within container|
| `text-shadow VALUE`    | Text shadow                      |
| `text-overflow VALUE`  | Text overflow (ellipsis/clip)    |
| `pointer-events VALUE` | Pointer events (none/auto)      |
| `user-select VALUE`    | User selection (none/text/all)  |
| `justify-content VALUE`| Main axis alignment             |
| `align-items VALUE`    | Cross axis alignment            |
| `order N`              | Flex/grid item order            |
| `background-size VALUE`| Background size                 |
| `background-position VALUE`| Background position         |
| `background-repeat VALUE`| Background repeat             |
| `word-break VALUE`     | Word break (break-all/keep-all) |
| `overflow-wrap VALUE`  | Overflow wrap (break-word)      |
| `font-weight VALUE`   | Font weight (100-900/bold/lighter)|
| `font-style VALUE`    | Font style (normal/italic/oblique)|
| `text-wrap VALUE`      | Text wrapping (balance/pretty)  |
| `will-change VALUE`    | Performance hint (transform)    |
| `touch-action VALUE`   | Touch behavior (none/pan-x)     |
| `vertical-align VALUE` | Vertical alignment (middle/top) |
| `contain VALUE`        | CSS containment (layout/paint)  |
| `content-visibility VALUE` | Lazy rendering (auto)       |
| `scroll-margin N`      | Scroll margin (for anchors)     |
| `scroll-padding N`     | Scroll padding (scroll-snap)    |
| `content VALUE`        | Pseudo-element content (with before:/after:)|

### Pseudo-states

Prefix any style attribute with `hover:`, `active:`, or `focus:` to apply it on that state.

```
@el [padding 16, background #3b82f6, hover:background #2563eb, active:background #1d4ed8, rounded 8, transition all 0.15s ease]
  @text [color white] Click me
```

All style attributes support state prefixes: `hover:color`, `active:rounded`, `focus:border`, etc.

### Dark mode

Prefix any style attribute with `dark:` to apply it when the user's system is in dark mode.

```
@el [background white, dark:background #1a1a2e, color #333, dark:color #eee]
  @text Theme-aware content
```

This generates a `@media (prefers-color-scheme: dark)` rule.

### Print styles

Prefix any style attribute with `print:` to apply it when printing.

```
@nav [print:display none]
  @text Navigation (hidden in print)
```

This generates a `@media print` rule.

### Flow & Grid

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `wrap`                 | Enable flex-wrap (on `@row`)     |
| `grid`                 | Enable CSS grid layout           |
| `grid-cols N`          | Grid template columns (N equal)  |
| `grid-rows N`          | Grid template rows (N equal)     |
| `col-span N`           | Span N columns in grid           |
| `row-span N`           | Span N rows in grid              |

### Identity

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `id NAME`              | HTML id attribute                |
| `class NAME`           | HTML class attribute             |

### Modern CSS

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `color-scheme VALUE`   | Color scheme (`light`, `dark`, `light dark`) |
| `appearance VALUE`     | Form element appearance (`none`) |

### Popover API

Use HTML Popover API attributes for declarative popovers without JavaScript.

```
@button [popovertarget info-panel] Show Info
@el [popover, id info-panel, padding 20, background white, rounded 8]
  @text This is a popover
```

| Attribute                     | Effect                          |
|-------------------------------|---------------------------------|
| `popover`                     | Make element a popover          |
| `popovertarget ID`            | Open popover with given ID     |
| `popovertargetaction VALUE`   | Action (toggle/show/hide)      |

### Input hints

| Attribute              | Effect                           |
|------------------------|----------------------------------|
| `inputmode VALUE`      | Virtual keyboard type (numeric/email/search/tel/url) |
| `enterkeyhint VALUE`   | Enter key label (enter/done/go/next/previous/search/send) |
| `fetchpriority VALUE`  | Resource priority (high/low/auto) |
| `translate VALUE`      | Whether to translate (yes/no)    |
| `spellcheck VALUE`     | Spell check (true/false)         |

## Asset inlining

SVG images can be inlined directly into the HTML output using the `[inline]` attribute:

```
@image [inline, width 24, height 24] icon.svg
```

This reads the SVG file and embeds its content directly, keeping the output self-contained.

## CLI

```
htmlang init                 # scaffold a new project
htmlang init my-site         # scaffold in a new directory
htmlang new about-us         # create about-us.hl from template
htmlang page.hl              # compile page.hl → page.html
htmlang site/                # compile all .hl files in directory
htmlang --watch page.hl      # compile and watch for changes
htmlang -w page.hl           # short form
htmlang --serve site/        # serve a multi-page site with hot reload
htmlang -s --open site/      # serve and open browser
htmlang check page.hl        # check for errors without writing output
htmlang check src/           # check all .hl files in a directory
htmlang convert page.html    # convert HTML to .hl format (stdout)
htmlang build src/ -o dist/  # parallel compile + copy static assets
htmlang --compat page.hl     # compile with vendor prefixes
htmlang --format json page.hl  # output diagnostics as JSON
htmlang lint --format json src/  # lint with JSON output
htmlang dead-code --format json src/  # dead code report as JSON
htmlang lint page.hl         # stricter lint checks (accessibility, nesting)
htmlang lint src/            # lint all .hl files in a directory
htmlang stats page.hl        # show file statistics (elements, CSS, colors)
htmlang repl                 # interactive REPL (type .hl, get HTML)
htmlang feed src/            # generate RSS feed from @page metadata
htmlang feed src/ -b https://mysite.com  # with custom base URL
htmlang components src/      # list all @fn definitions across project
htmlang deps src/            # show dependency graph (@include/@import)
htmlang dead-code src/       # find unused @fn, @define, @let across project
htmlang deploy src/          # build and deploy to GitHub Pages
htmlang playground           # generate self-contained HTML playground
htmlang clean src/           # remove generated .html files
htmlang outline page.hl      # show document structure tree
```

Watch mode recompiles automatically when the source file or any `@include`d/`@import`ed files change.

The `build` command automatically extracts shared CSS rules across pages into a `shared.css` file when outputting to a directory.

## Compilation target

Each `.hl` file compiles to a single self-contained `.html` file:

- Elements map to `<div>`, `<span>`, `<p>`, `<a>`, `<img>` as appropriate
- Layout uses flexbox (`display: flex`, `flex-direction`, `gap`, etc.)
- Styles are scoped via generated class names in an embedded `<style>` block
- No external CSS, no JavaScript (unless injected via `@raw`)
- `@page` generates the HTML boilerplate; without it, output is an HTML fragment

## Template inheritance (`@extends`)

A page can inherit a base layout and fill named `@slot` blocks:

**layout.hl:**
```
@page My Site
@column [max-width 800, center-x, padding 40]
  @slot header
    @text [bold, size 32] Default Header
  @slot content
  @slot footer
    @text [color #888] Default Footer
```

**about.hl:**
```
@extends layout.hl
@slot header
  @text [bold, size 32] About Us
@slot content
  @paragraph We are a team of...
```

Slots not filled by the extending page use the default children from the layout.

## Design tokens (`@theme`)

Centralized design token definitions. Each token becomes both a `$variable` and a `--css-custom-property`:

```
@theme
  primary #3b82f6
  secondary #10b981
  spacing-sm 8
  spacing-md 16
  font-body system-ui, sans-serif
```

Equivalent to:
```
@let primary #3b82f6
@let --primary #3b82f6
@let secondary #10b981
@let --secondary #10b981
...
```

## Deprecation (`@deprecated`)

Mark a function as deprecated. Callers receive a compile-time warning:

```
@deprecated Use @card-v2 instead
@fn old-card $title
  @el [padding 20]
    @text $title
    @children
```

When `@old-card` is called, the compiler emits: `warning: @old-card is deprecated: Use @card-v2 instead`

## Color functions

Color manipulation via variable filters:

```
@let primary #3b82f6

@el [background $primary|lighten:20]        -- 20% lighter
@el [background $primary|darken:10]         -- 10% darker
@el [background $primary|alpha:0.5]         -- 50% transparent (#3b82f67f)
@el [background $primary|mix:#ffffff:50]    -- 50% mixed with white
```

Filters: `lighten:N` (0-100), `darken:N` (0-100), `alpha:N` (0-1), `mix:COLOR:N` (0-100).

## Enhanced `@keyframes`

Keyframes support htmlang attribute syntax:

```
@keyframes fade-in
  from [opacity 0]
  to [opacity 1]

@keyframes slide
  0% [transform translateX(-100%)]
  50% [transform translateX(0), opacity 1]
  100% [transform translateX(100%), opacity 0]
```

Raw CSS syntax is also supported for backwards compatibility.

## Additional attributes

| Attribute | Effect |
|-----------|--------|
| `autofocus` | Auto-focus element on page load (boolean) |

## Named grid areas

Use CSS Grid's named template areas with the `grid-template-areas` and `grid-area` attributes:

```
@el [grid, grid-template-areas "header header" "sidebar main" "footer footer", gap 20]
  @el [grid-area header, padding 10, background #3b82f6]
    Header
  @el [grid-area sidebar, padding 10]
    Sidebar
  @el [grid-area main, padding 10]
    Main content
  @el [grid-area footer, padding 10]
    Footer
```

## Animate shorthand

The `animate` attribute is shorthand for the CSS `animation` property:

```
@keyframes fade-in
  from [opacity 0]
  to [opacity 1]

@el [animate fade-in 0.3s ease]
  Fades in on load
```

## View transitions

The `view-transition-name` attribute enables the View Transitions API for smooth page transitions:

```
@el [view-transition-name hero]
  Hero content that transitions between pages
```

## `:has()` pseudo-selector

Style elements based on their children using the `has()` prefix:

```
@el [padding 20, has(.active):background #dbeafe, has(img):padding 0]
  Content
```

Generates CSS `:has()` selectors: `.class:has(.active) { background:#dbeafe; }`

## Computed `@let`

Variables support arithmetic expressions. The `=` sign is optional:

```
@let base 16
@let large = $base * 2     -- 32
@let gap $base + 4         -- 20 (= is optional)
```

Supported operators: `+`, `-`, `*`, `/`.

## Named slots in `@fn`

Functions support named `@slot` blocks for multi-region components:

```
@fn layout $title
  @column [max-width 800, center-x, padding 40]
    @slot header
      @text [bold, size 32] $title
    @slot content
    @slot footer
      @text [color #888] Default Footer

@layout [title My Page]
  @slot header
    @text [bold, size 32] Custom Header
  @slot content
    @paragraph Main content here
```

Slots not filled by the caller use the default children from the function definition.

## New elements

### `@script`

Embeds JavaScript. Supports inline code and external files via `src`. Children are raw JS (not HTML-escaped).

```
@script [src app.js, defer]

@script
  console.log("hello world");
  document.addEventListener("DOMContentLoaded", () => {
    init();
  });
```

Attributes: `src`, `type`, `defer`, `async`, `crossorigin`, `integrity`, `nomodule`.

### `@noscript`

Fallback content displayed when JavaScript is disabled.

```
@noscript
  @text This page requires JavaScript.
```

### `@address`

Contact information, rendered as `<address>`.

```
@address
  @text [bold] John Doe
  @link mailto:john@example.com john@example.com
```

### `@search`

HTML5 `<search>` landmark for search functionality.

```
@search
  @input [type search, placeholder Search...]
  @button [type submit] Search
```

### `@breadcrumb`

Semantic breadcrumb navigation. Generates `<nav aria-label="breadcrumb">` with an `<ol>` list. Each child becomes an `<li>`.

```
@breadcrumb [spacing 10]
  @link / Home
  @link /docs Documentation
  @text Current Page
```

## New directives

### `@canonical`

Sets the canonical URL. Generates `<link rel="canonical">` in the document head.

```
@canonical https://example.com/my-page
```

### `@base`

Sets the base URL for relative links. Generates `<base href="...">`.

```
@base https://example.com/
```

### `@font-face`

Declares a custom font. Generates a CSS `@font-face` rule with `font-display: swap`. Format is auto-detected from the file extension.

```
@font-face Inter fonts/inter.woff2
@font-face Mono fonts/jetbrains-mono.ttf
```

### `@json-ld`

Structured data for SEO. Generates a `<script type="application/ld+json">` block. Indented body is raw JSON.

```
@json-ld
  {
    "@context": "https://schema.org",
    "@type": "WebPage",
    "name": "My Site"
  }
```

## Additional pseudo-states

| Prefix | CSS Selector | Purpose |
|--------|--------------|---------|
| `visited:` | `:visited` | Visited links |
| `empty:` | `:empty` | Empty elements |
| `target:` | `:target` | URL fragment target |
| `valid:` | `:valid` | Valid form inputs |
| `invalid:` | `:invalid` | Invalid form inputs |

```
@link [visited:color purple] https://example.com
  Already visited

@input [type email, valid:border 2 green]
@el [target:background yellow, id section-1]
  Highlighted when navigated to
```

## Additional CSS properties

| Attribute | Effect |
|-----------|--------|
| `text-underline-offset N` | Offset of text underline from text |
| `column-width N` | Ideal column width for multi-column layout |
| `column-rule VALUE` | Rule between columns (e.g., `1px solid #ccc`) |

```
@text [underline, text-underline-offset 4, text-decoration-thickness 2]
  Styled underline

@el [column-count 3, column-width 200, column-rule 1px solid #ccc]
  Multi-column content
```

## CSS `@layer` wrapping

Generated styles are wrapped in `@layer htmlang { ... }` for specificity management. This allows `@style` blocks and `@raw` CSS to easily override generated styles without specificity wars.

## CLI commands

### `htmlang deps`

Show the file dependency graph (`@include`, `@import`, `@extends` relationships):

```
htmlang deps src/
```

### `htmlang dead-code`

Find unused `@fn`, `@define`, and `@let` definitions across an entire project:

```
htmlang dead-code src/
```

### `htmlang deploy`

Build and deploy to GitHub Pages:

```
htmlang deploy src/
```

Compiles all `.hl` files, copies static assets, and pushes to a `gh-pages` branch.

### `htmlang clean`

Remove generated `.html` files from a directory (matching `.hl` source files). Also removes `sitemap.xml` if present.

```
htmlang clean src/
htmlang clean       # clean current directory
```

### `htmlang outline`

Show the document structure tree — a quick overview of elements and nesting without compiling.

```
htmlang outline page.hl
```

### `htmlang playground`

Generate a self-contained HTML playground for experimenting with htmlang:

```
htmlang playground              # writes playground.html
htmlang playground my-play.html # custom output path
htmlang clean src/              # remove generated .html files
htmlang outline page.hl         # show document structure tree
```

## Editor support

A VS Code extension is available in `editors/vscode/` with:

- Syntax highlighting via TextMate grammar
- LSP integration via `htmlang-lsp` for:
  - Real-time diagnostics and error checking
  - Completions for elements, attributes, variables, and functions
  - Hover documentation for all elements and attributes
  - Go to definition for variables, defines, and functions
  - Find all references for variables and functions
  - Rename refactoring for variables and functions
  - Signature help for `@fn` parameters
  - Document formatting (format on save)
  - Code actions (quick-fixes for typos, unused variables, missing attributes)
  - Auto-import suggestions (scans project for `@fn` definitions)
  - Extract selection to `@fn` refactoring
  - Color picker for hex colors
  - Code folding and document symbols
  - Semantic tokens for syntax highlighting
  - VS Code snippets for common patterns (`@fn`, `@each`, `@grid`, `@form`, etc.)

## Additional directives reference

The compiler recognizes a number of directives that are documented only briefly
above. A concise reference:

### Content generation

- `@markdown` — indented body is parsed as Markdown and emitted as HTML.
- `@raw """..."""` — embed verbatim HTML / CSS / JS.
- `@svg path/to/icon.svg` — inline an SVG file's contents.
- `@fetch url -> $name` — fetch a JSON payload at build time and bind it.
- `@data name path.json` — load a JSON file for iteration with `@each`.

### Metadata & head

- `@lang en` — set `<html lang>`.
- `@favicon /favicon.png` — inject a favicon link.
- `@meta name value` — add a `<meta>` tag.
- `@og key value` — add an Open Graph `<meta property="og:key">` tag.
- `@canonical https://…` — set `<link rel="canonical">`.
- `@base /base/path/` — set `<base href>`.
- `@manifest [Name]` — emit a PWA `manifest.json` reference and body.
- `@breakpoint name width` — register a custom responsive prefix.

### Styling

- `@style` — indented body is raw CSS injected into `<style>`.
- `@scope (selector)` — wrap body in CSS `@scope` for scoped styles.
- `@starting-style` — CSS entry-animation starting values.
- `@font-face name url` — register a web font.
- `@theme` — design tokens emitted as CSS custom properties.
- `@keyframes name` — define keyframe animations.
- `@css-property name syntax value` — register a typed custom property.

### Control flow

- `@if cond` / `@unless cond` / `@else` / `@else if`.
- `@match $value` / `@switch $value` — exhaustive branching.
- `@each item in list` / `@each item, $_index in list` — iterate.
- `@for $i in 0..10 [step 2]` — numeric ranges.
- `@repeat N` — repeat a block N times.
- `@defer` — defer block rendering below the fold.

### Module system

- `@include file.hl` — paste file contents here.
- `@import file.hl` — import @fn / @let / @define (with optional `as` alias).
- `@use "file.hl" fn1, fn2` — import a named subset.
- `@extends template.hl` — template inheritance; override with `@slot`.
- `@layout path.hl` — wrap output in a layout template.
- `@collection path/ -> $items` — iterate over a directory.

### Functions & composition

- `@fn name $p1 $p2` — define a function; body uses `@children` / `@slot`.
- `@component name $p` — like `@fn` but emits a scoped wrapper class.
- `@mixin name [...attrs]` — attribute bundle you can merge elsewhere.
- `@define name [...attrs]` — named attribute preset, used as `[$name]`.
- `@with $source as $alias` — temporarily rebind a variable.

### Debugging

- `@assert cond [message]` — emit an error if a boolean check fails.
- `@warn message` / `@debug expr` / `@log message` — compile-time output.
- `@deprecated name [reason]` — mark a function as deprecated.
- `@translations locale` — bind i18n strings available as `$t.key`.
