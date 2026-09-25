# htmlang (.hl)

A minimalist layout language inspired by elm-ui that compiles to static HTML.

## Principles

- `@` means structure, bare lines mean content.
- Layout is explicit and compositional: every element declares its own layout
  role, and there is no CSS cascade to reason about.
- Output is one self-contained HTML file with embedded CSS (flexbox), and no
  JavaScript unless you write some.

Every example block in this file is compiled by the test suite
(`tests/docs.rs`), so the examples stay correct.

## Syntax

### Structure

```
@element [attributes] argument
  children
```

Children are indented under their parent. Attributes are comma-separated inside
`[...]` and may span several lines:

```
@el [
  padding 20,
  background white,
  rounded 8,
  shadow 0 2px 4px rgba(0,0,0,0.1)
]
  Content
```

A comma inside `(...)` or `"..."` does not split attributes, so a font stack is
written `font "Inter, sans-serif"`.

### Text

Any line that doesn't start with `@` or `[` is text. Use `@text` when text needs
attributes, and `{...}` to put elements inside a line of text:

```
@paragraph
  This is {@text [bold] important} and this is a {@link https://example.com link}.
@text [bold, size 24, color #333] Hello world
```

### Comments

`--` at the start of a line begins a comment. Comments must be on their own
line: `--` later in a line is ordinary text.

```
-- this is a comment
@row [spacing 10]
  -- todo: add nav items
```

### Shorthands

A line starting with `[` is an anonymous `@el`. `>` chains single-child
elements on one line; the last element in the chain gets the indented children.

```
[padding 20, background white]
  Hello

@el [padding 16, background blue, rounded 8] > @link https://example.com
  @text [color white] Get Started
```

### Raw content

`@raw """ ... """` is pasted into the output verbatim.

```
@raw """
<div class="custom-widget"></div>
"""
```

## Elements

Layout elements set up flexbox; the rest map to the HTML element of the same
name.

| Element | Output | Purpose |
|---|---|---|
| `@row` | div, flex row | Horizontal layout |
| `@column` | div, flex column | Vertical layout |
| `@el` | div, flex column | Generic container |
| `@grid` | div, grid | Grid container (use `grid-cols`) |
| `@stack` | div, position relative | Children layered on top of each other |
| `@in-front` / `@behind` | div, absolute | Overlay layers filling the parent (see below) |
| `@spacer` | div, flex 1 | Pushes siblings apart |
| `@text` | span | Styled inline text |
| `@paragraph` | p | Flowing text with inline elements |
| `@link url` | a | Link; text after the URL becomes its content |
| `@image src` | img | Image |
| `@h1` … `@h6` | h1 … h6 | Headings |

Semantic containers, all laid out as columns: `@nav`, `@header`, `@footer`,
`@main`, `@section`, `@article`, `@aside`, `@address`, `@search`, `@form`,
`@details` / `@summary`, `@dialog`, `@figure` / `@figcaption`, `@blockquote` /
`@cite`, `@fieldset` / `@legend`, `@noscript`.

Content: `@list` / `@item` (`@list [ordered]` for `<ol>`), `@dl` / `@dt` /
`@dd`, `@table` / `@thead` / `@tbody` / `@tr` / `@th` / `@td`, `@code`, `@pre`,
`@hr`, `@mark`, `@kbd`, `@abbr`, `@time`, `@progress`, `@meter`, `@output`,
`@canvas`, `@iframe src`, `@video`, `@audio`, `@picture` / `@source`,
`@script`, `@breadcrumb`, `@fragment` (children without a wrapper).

Form controls: `@input`, `@button`, `@select` / `@option`, `@textarea`,
`@label`, `@datalist`.

Styled components: `@badge`, `@chip`, `@tag`, `@avatar`, `@tooltip`,
`@carousel`.

```
@list [ordered]
  @item First
  @item Second
@form [method post] /subscribe
  @label [for email] Email
  @input [type email, name email, id email, required]
  @button [type submit] Send
@details [open]
  @summary Question
  @text The answer.
```

### `@in-front` / `@behind`

Overlay layers, like elm-ui's `inFront` and `behind`. Their children fill the
parent's bounds; the parent automatically becomes a positioning context
(`position: relative; isolation: isolate`). An explicit `position` on the parent
wins.

```
@el [width 200, height 200, background blue]
  @text [color white] Main content
  @in-front
    @text [color yellow] Painted on top
  @behind
    @el [background red, width fill, height fill]
```

## Attributes

### Layout and sizing

| Attribute | Effect |
|---|---|
| `spacing N` | Gap between children (also `gap-x N`, `gap-y N`) |
| `padding N` / `padding Y X` / `padding T R B L` | Padding (also `padding-x`, `padding-y`, per side) |
| `margin ...` | Margin, same forms as padding |
| `width fill` / `width N` / `width shrink` | Take remaining space, exact size, or fit content |
| `height fill` / `height N` / `height shrink` | Same for height |
| `min-width`, `max-width`, `min-height`, `max-height` | Size limits |
| `center-x`, `center-y` | Center within the parent |
| `align-left`, `align-right`, `align-top`, `align-bottom` | Align within the parent |
| `wrap` | Let a row wrap |
| `grid-cols N`, `grid-rows N`, `col-span N`, `row-span N` | Grid layout |
| `hidden` | `display: none` |

### Style

| Attribute | Effect |
|---|---|
| `background COLOR`, `color COLOR` | Colors |
| `border N COLOR` (also `border-top` etc.) | Border |
| `rounded N` | Border radius |
| `shadow VALUE` | Box shadow |
| `bold`, `italic`, `underline` | Text style |
| `size N` | Font size |
| `font NAME` | Font family |
| `transition VALUE`, `animation VALUE` | Motion |

Most CSS properties can also be written directly as attributes with the same
name and value: `opacity 0.5`, `cursor pointer`, `z-index 10`,
`text-align center`, `position absolute`, `grid-template-areas "a b"`, and so on.

**Units.** A bare number is pixels (`padding 20` is `20px`). Values with a unit,
keywords and CSS functions are passed through: `width 50%`, `margin 0 auto`,
`max-width min(100%, 800px)`.

### HTML attributes

`id`, `class`, and element attributes such as `href`, `type`, `name`, `value`,
`placeholder`, `alt`, `for`, `required`, `disabled`, `target`, `rel`, `role`,
`tabindex`, `title`, `data-*` and `aria-*` are emitted as HTML attributes.

### State and media prefixes

Prefix a style attribute to apply it conditionally:

| Prefix | Applies |
|---|---|
| `hover:`, `active:`, `focus:`, `focus-visible:`, `focus-within:`, `disabled:`, `checked:`, `visited:`, `target:`, `valid:`, `invalid:`, `empty:`, `placeholder:`, `selection:` | In that state |
| `first:`, `last:`, `odd:`, `even:`, `nth(EXPR):` | By position among siblings |
| `before:`, `after:` | On the `::before` / `::after` pseudo-element (with `content`) |
| `has(SELECTOR):` | When the element contains a match |
| `sm:`, `md:`, `lg:`, `xl:`, `2xl:` | From that viewport width up (640–1536px) |
| `cq-sm:` … `cq-2xl:` | Container queries |
| `dark:`, `print:`, `motion-safe:`, `motion-reduce:`, `landscape:`, `portrait:` | Media conditions |

`@breakpoint name WIDTH` defines a custom responsive prefix.

```
@el [padding 16, background #3b82f6, hover:background #2563eb, md:padding 32, dark:background #1e3a8a]
  @text [color white] Click me
@el [before:content "→ ", before:color red]
  Item with an arrow
```

### Conditional attributes

```
@let active true
@el [background if($active, blue, gray), bold if $active]
  Conditionally styled
```

## Variables and functions

`@let` defines everything reusable. What it defines depends on its shape:

```
-- A value, used as $primary
@let primary #3b82f6
-- A computed value (= is optional)
@let gap = 8 * 2
-- A quoted string: interpolated, never computed
@let greeting "Hello $primary"
-- An attribute bundle, used as [$card]
@let card [padding 20, background white, rounded 8]
-- A function: a @let with an indented body
@let panel $title $tone=neutral
  @el [$card]
    @text [bold] $title
    @children

@panel [title Welcome]
  Body text goes into @children.
@el [$card, background #f9fafb]
  Attributes after a bundle override it.
```

Function arguments are passed as attributes; parameters with `=default` may be
omitted. `@component name $params` defines a function whose indented `@style`
block is scoped to its output. Inside a function, `@children` is replaced by the caller's children,
and `@slot name` by the caller's `@slot name` block (the slot's own children are
the default).

A multi-line string uses triple quotes; its indented lines are the value:

```
@let intro """
  First line
  Second line
  """
@text $intro
```

`@let --name value` also emits a CSS custom property. `@theme` declares a group
of design tokens at once; each becomes both `$name` and `--name`:

```
@theme
  brand #3b82f6
  radius 8
@el [background $brand, rounded $radius] Themed
```

### Filters

`$name|filter` transforms a value: `uppercase`, `lowercase`, `capitalize`,
`trim`, `length`, `reverse`, `truncate:N`, `replace:OLD:NEW`, `default:VALUE`,
and for colors `lighten:N`, `darken:N`, `alpha:N`, `mix:COLOR:N`.

```
@let name htmlang
@let base #3b82f6
@text [color $base|darken:10] $name|uppercase
```

### Checks

`@assert CONDITION` fails the build when false. `@warn MESSAGE` emits a
warning; `@debug` and `@log` print values at compile time. `@deprecated MESSAGE`
before a function warns every caller.

## Control flow

All control flow runs at compile time.

```
@let items apple, banana, cherry
@let count 3

@if $count > 2
  @text Many
@else if $count == 0
  @text None
@else
  @text Few

@if not $count
  @text Nothing to show

@each $item in $items
  @text $_index: $item
@else
  @text The list is empty.

@each $i in 1..10 step 3
  @text $i

@each $label, $url in Home /, About /about
  @link $url $label

@match $count
  @case 3
    @text Three
  @default
    @text Other
```

`@defer` keeps its children hidden until they scroll near the viewport (they
are shown immediately when JavaScript is off).

Conditions support `==`, `!=`, `<`, `>`, `<=`, `>=`, `contains`, `starts-with`,
`ends-with`, `not`, and truthiness (empty, `false` and `0` are false). A range
counts down when its start is greater than its end.

## Files and data

| Directive | Effect |
|---|---|
| `@include file.hl` | Insert another file here, content and definitions |
| `@import file.hl` | Take only its definitions (`@let`, bundles, functions) |
| `@extends layout.hl` | Render this page inside a layout (below) |
| `@data file.json` / `@data $prefix file.json` | Load JSON values as variables |
| `@fetch $prefix http://...` | Like `@data`, fetched at build time (http only) |
| `@collection $name "glob"` | List files matching a pattern |
| `@translations` | Per-locale strings, used as `$t.key` |
| `@env NAME default` | Read an environment variable |
| `@markdown` / `@markdown file.md` | Markdown, converted to HTML |
| `@svg file.svg` | Inline an SVG file |

With `@extends`, the layout marks where content goes with `@slot name` (named
blocks) and `@children` (everything in the page outside `@slot` blocks):

```
-- layout.hl
@page My Site
@column [max-width 800, center-x]
  @slot header
    @text Default header
  @children
```

```
-- page.hl
@extends layout.hl
@slot header
  @text About us
@paragraph This fills @children.
```

## Page and head

`@page TITLE` produces a full HTML document; without it the output is a fragment.
Head content comes from `@lang`, `@favicon`, `@meta NAME VALUE`,
`@og KEY VALUE`, `@canonical URL`, `@base URL`, `@manifest NAME` (with
indented settings), `@font-face NAME URL`, `@json-ld` (indented JSON) and `@head`
(indented raw HTML).

```
@page My Site
@lang en
@meta description A small site
@og title My Site
```

## CSS

- `@style` holds raw CSS, including at-rules such as `@scope`, `@property` and
  `@starting-style`.
- `@keyframes name` takes htmlang attribute syntax (`from [opacity 0]`) or raw
  CSS.
- Generated rules live in `@layer htmlang` (after a `hl-reset` layer), so any
  CSS in `@style` or `@raw` overrides them.

```
@keyframes fade-in
  from [opacity 0]
  to [opacity 1]
@style
  .note { color: gray; }
@el [animation fade-in 0.3s ease, class note] Fades in
```

## Upgrading older files

`htmlang upgrade [dir|file]` rewrites removed syntax:

| Removed | Use instead |
|---|---|
| `@fn`, `@define`, `@mixin` | `@let` (function, bundle) |
| `@unless COND` | `@if not COND` |
| `@for $i in A..B`, `@repeat N` | `@each $i in A..B`, `@each $_ in 1..N` |
| `@switch` | `@match` |
| `@use "file" names` | `@import file` |
| `@with $x as y` | `@let y $x` |
| `@layout file` | `@extends file` |
| `@scope`, `@starting-style`, `@css-property` | The CSS rule in `@style` |
| `@col`, `@p`, `@img`, `@li`, `@btn`, `@ul`, `@divider`, `@opt` | `@column`, `@paragraph`, `@image`, `@item`, `@button`, `@list`, `@hr`, `@option` |
| `...$bundle` | `$bundle` |
| `animate`, `inset-area` | `animation`, `position-area` |
| `\|upper`, `\|lower`, `\|cap`, `\|len` | `\|uppercase`, `\|lowercase`, `\|capitalize`, `\|length` |

The compiler reports each removed form with its replacement.
