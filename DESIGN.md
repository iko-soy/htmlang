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
  border-radius 8,
  box-shadow 0 2px 4px rgba(0,0,0,0.1)
]
  Content
```

An attribute is either a **style**, written `key value` (`padding 20`,
`color red`), or an **HTML attribute**, written `key=value` (`id=main`,
`type=email`, `aria-label=Close`). Boolean HTML attributes are written bare
(`required`, `disabled`, `open`). A comma inside `(...)` or `"..."` does not
split attributes, so a font stack is written `font-family "Inter, sans-serif"`.

### Text

Any line that doesn't start with `@` is text. Text after an element's
attributes is its content, and `{...}` puts elements inside a line of text:

```
@paragraph
  This is {@text [font-weight bold] important} and this is a {@link https://example.com link}.
@text [font-weight bold, font-size 24, color #333] Hello world
@section [padding 8] Text after the attributes is content too.
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

`>` chains single-child elements on one line; the last element in the chain
gets the indented children.

```
@el [padding 16, background blue, border-radius 8] > @link https://example.com
  @text [color white] Get Started
```

### Raw content

`@raw` pastes HTML into the output verbatim: the rest of its line, or its
indented block. The bodies of `@style`, `@head`, `@script` and `@markdown` are
verbatim too (a `--` line in them is not a comment).

```
@raw <hr class="fancy">
@raw
  <div class="custom-widget">
    <span>Hand-written HTML</span>
  </div>
```

## Elements

The layout and text elements come from elm-ui; every other element has its
HTML name. Every container lays its children out in a column, except `@row`.

| Element | Output | Purpose |
|---|---|---|
| `@row` | div, flex row | Horizontal layout |
| `@el` | div, flex column | The container: children laid out top to bottom |
| `@grid` | div, grid | Grid container (use `grid-cols`) |
| `@in-front` / `@behind` | div, absolute | Overlay layers filling the parent (see below) |
| `@text` | span | Styled inline text |
| `@paragraph` | p | Flowing text with inline elements |
| `@link url` | a | Link; text after the URL becomes its content |
| `@image src` | img | Image |
| `@h1` … `@h6` | h1 … h6 | Headings |

Semantic containers, all laid out as columns: `@nav`, `@header`, `@footer`,
`@main`, `@section`, `@article`, `@aside`, `@address`, `@search`, `@form`,
`@details` / `@summary`, `@dialog`, `@figure` / `@figcaption`, `@blockquote` /
`@cite`, `@fieldset` / `@legend`, `@noscript`.

Content: `@ul` / `@ol` / `@li`, `@dl` / `@dt` /
`@dd`, `@table` / `@thead` / `@tbody` / `@tr` / `@th` / `@td`, `@code`, `@pre`,
`@hr`, `@mark`, `@kbd`, `@abbr`, `@time`, `@progress`, `@meter`, `@output`,
`@canvas`, `@iframe src`, `@video src`, `@audio src`, `@picture` / `@source`,
`@script`, `@fragment` (children without a wrapper).

Form controls: `@input`, `@button`, `@select` / `@option`, `@textarea`,
`@label`, `@datalist`. `@form URL` sets the form's `action`.

```
@ol
  @li First
  @li Second
@form [method=post] /subscribe
  @label [for=email] Email
  @input [type=email, name=email, id=email, required]
  @button [type=submit] Send
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

### Standard library

These components are written in htmlang (`std.hl`) and available in every file.
Use them like elements; your own `@let` with the same name takes precedence.

| Component | Purpose |
|---|---|
| `@badge` | Small pill for counts and statuses |
| `@tag` | Label with slightly rounded corners |
| `@chip` | Outlined, fully rounded label |
| `@avatar` | Circular frame for an image or initials |
| `@spacer` | Takes up the remaining space in a row or column |
| `@tooltip [tip TEXT]` | Text that shows `TEXT` when hovered |
| `@carousel` | Horizontally scrolling row that snaps to each child |
| `@breadcrumb` | Breadcrumb trail of `@li`s |
| `$skeleton`, `$truncate`, `$no-scrollbar` | Attribute bundles: loading placeholder, one-line ellipsis, hidden scrollbars |

```
@row [spacing 8, align-items center]
  @badge [background #ef4444, color white] 3
  @chip Rust
  @spacer
  @tooltip [tip Opens in a new tab] Help
@breadcrumb
  @li > @link / Home
  @li Docs
@el [$skeleton, height 20, width fill]
```

## Attributes

### Layout

htmlang's own attributes are about layout, in the spirit of elm-ui: how an
element sits in its row or column.

| Attribute | Effect |
|---|---|
| `spacing N` | Gap between children |
| `width fill` / `width N` / `width shrink` | Take remaining space, exact size, or fit content |
| `height fill` / `height N` / `height shrink` | Same for height |
| `center-x`, `center-y` | Center within the parent |
| `align-left`, `align-right`, `align-top`, `align-bottom` | Align within the parent |
| `wrap` | Let a row wrap |
| `grid-cols N`, `grid-rows N`, `col-span N`, `row-span N` | Grid layout |

### Style

Everything else is CSS: **any standard CSS property** is an attribute, with
the same name and value: `padding 20`, `font-weight bold`, `font-size 18`,
`border 1 solid #e5e7eb`, `border-radius 8`, `background red`,
`display none`, `grid-template-areas "a b"`, and so on. `line-clamp N` also
adds the `-webkit-box` declarations browsers still need to cut text off after
N lines.

**Units.** For lengths, a bare number is pixels (`padding 20` is `20px`,
`border 1 solid red` is `1px solid red`). Values with a unit, keywords and CSS
functions are passed through: `width 50%`, `margin 0 auto`,
`max-width min(100%, 800px)`.

### HTML attributes

Write HTML attributes as `key=value`: `id=main`, `class=note`, `href=/about`,
`type=email`, `alt=Logo`, `target=_blank`, `aria-label=Close menu`,
`data-id=42`. Any name works. Booleans are written bare: `required`,
`disabled`, `checked`, `open`, `sandbox`. A style and an HTML attribute may
share a name without clashing:

```
@image [width=800, width 200, alt=A photo] photo.jpg
@select [size=4, font-size 18]
  @option One
```

### State and media prefixes

Prefix a style attribute to apply it conditionally:

| Prefix | Applies |
|---|---|
| `hover:`, `active:`, `focus:`, `focus-visible:`, `focus-within:`, `disabled:`, `checked:`, `visited:`, `target:`, `valid:`, `invalid:`, `empty:`, `placeholder:`, `selection:` | In that state |
| `first:`, `last:`, `odd:`, `even:`, `nth:EXPR:` | By position among siblings |
| `children:` | To each direct child |
| `before:`, `after:` | On the `::before` / `::after` pseudo-element (with `content`) |
| `has(SELECTOR):` | When the element contains a match |
| `sm:`, `md:`, `lg:`, `xl:`, `2xl:` | From that viewport width up (640–1536px) |
| `cq-sm:` … `cq-2xl:` | Container queries |
| `dark:`, `print:`, `motion-safe:`, `motion-reduce:`, `landscape:`, `portrait:` | Media conditions |

```
@el [padding 16, background #3b82f6, hover:background #2563eb, md:padding 32, dark:background #1e3a8a]
  @text [color white] Click me
@row [spacing 4, children:flex 1, nth:2n:background #f3f4f6]
  @el A
  @el B
@el [before:content "→ ", before:color red]
  Item with an arrow
```

### Conditional attributes

`if(CONDITION, A, B)` picks `A` or `B` by the condition. It works as a value or
as a whole attribute, and an empty choice (the `B` can be left out) leaves the
attribute out.

```
@let active true
@el [background if($active, blue, gray), if($active, font-weight bold), padding if($active, 12)]
  Conditionally styled
```

## Variables and functions

`@let` defines everything reusable. What it defines depends on its shape:

```
-- A value, used as $primary
@let primary #3b82f6
-- A computed value: `=` makes it an expression
@let gap = 8 * 2
-- A quoted string, with $variables interpolated
@let greeting "Hello $primary"
-- An attribute bundle, used as [$card]
@let card [padding 20, background white, border-radius 8]
-- A function: a @let with an indented body
@let panel $title $tone=neutral
  @el [$card]
    @text [font-weight bold] $title
    @children

@panel [title Welcome]
  Body text goes into @children.
@el [$card, background #f9fafb]
  Attributes after a bundle override it.
```

A function is called like an element:

- Its parameters are passed as attributes; parameters with `=default` may be
  omitted.
- Other attributes style its root element, so `@panel [title Hi, padding 40]`
  works like styling a built-in element.
- Text after the attributes and indented children replace `@children`, and a
  caller's `@slot name` block replaces `@slot name` (the slot's own children are
  the default).
- An `@style` block at the top of the body is scoped to the function: its rules
  apply inside the function's root element (`&` is the root itself). The
  body needs a single root element.

```
@let note $kind=info
  @style
    .title { font-weight: bold; }
  @el [padding 12, border-radius 6, background #eff6ff]
    @text [class=title] $kind
    @children

@note [kind Tip, padding 20] Scoped styles and forwarded attributes.
```

A multi-line string uses triple quotes; its indented lines are the value:

```
@let intro """
  First line
  Second line
  """
@text $intro
```

`@let --name value` also declares the CSS custom property `--name` on
`:root`. `$--name` is its value at compile time; `var(--name)` refers to it at
run time, so a stylesheet can override it:

```
@let --brand #3b82f6
@el [background var(--brand), border 1 solid ${darken($--brand, 10)}] Themed
```

## Expressions

Conditions and computed values (`@let x = ...`) are expressions:

| | |
|---|---|
| Values | numbers, `"strings"` (with `$var` interpolation), `$variables`, `true`, `false`, and bare words (`dark`, `#fff`) as strings |
| Arithmetic | `+ - * / %` with the usual precedence, unary `-`, `( )` |
| Comparison | `== != < > <= >=` (numeric when both sides are numbers), `contains` (in text, or as an item of a list), `starts-with`, `ends-with` |
| Logic | `and`, `or`, `not`; empty, `false` and `0` are false |
| Choice | `if(CONDITION, A, B)` |
| Text functions | `uppercase(s)`, `lowercase(s)`, `capitalize(s)`, `trim(s)`, `length(s)` (a list's items, or a text's characters), `reverse(s)`, `truncate(s, n)`, `replace(s, old, new)`, `default(s, fallback)` |
| Color functions | `lighten(c, pct)`, `darken(c, pct)`, `alpha(c, a)`, `mix(c1, c2, pct)` |

Variables are looked up while evaluating, so a value containing `==` or spaces
is still one value. An invalid expression is a compile error.

In text and attribute values, `$name` inserts a variable and `${EXPR}` inserts
the value of any expression:

```
@let name htmlang
@let base #3b82f6
@text [color ${darken($base, 10)}] ${uppercase($name)} has ${length($name)} letters
```

## Control flow

All control flow runs at compile time.

```
@let items apple, banana, cherry
@let count 3

@if $count > 2 and not $hidden
  @text Many
@else if $count == 0
  @text None
@else
  @text Few

@each $item, $i in $items
  @text $i: $item
@else
  @text The list is empty.

@each $n in 1..10 step 3
  @text $n
```

`@each $item in LIST` loops over a list: a list loaded with `@data`, text
split on commas, or a range (which counts down when its start is greater than
its end). An optional second variable is the index, from 0. An item loaded from
JSON can be a record, whose fields are `$item.key`:

```
@data $links [
  {"label": "About us", "url": "/about"},
  {"label": "Blog", "url": "/blog"}
]
@row [spacing 12]
  @each $link in $links
    @link $link.url $link.label
@text ${length($links)} links
```

## Files and data

| Directive | Effect |
|---|---|
| `@include file.hl` | Insert another file: its content and definitions (a library of `@let`s emits nothing) |
| `@data $name file.json` | Load JSON values as variables (`$name.key`; arrays are lists) |
| `@data $name [...]` / `@data $name {...}` | Inline JSON (an array may span lines) |
| `@data $name dir/*.json` | A list with one record per file, in name order; `$item.file` is the file's name |
| `@data $name env:NAME [default]` | Read an environment variable |
| `@markdown` / `@markdown file.md` | Markdown, converted to HTML |
| `@image [inline] file.svg` | Inline an SVG file (`width`, `height`, `color`, `class=`, `id=` apply to it) |

A layout is an ordinary function in its own file. It marks where content goes
with `@slot name` (named blocks, with default content) and `@children`
(everything in the call outside `@slot` blocks):

```
-- layout.hl
@let layout
  @page My Site
  @el [max-width 800, center-x]
    @slot header
      @text Default header
    @children
```

```
-- page.hl
@include layout.hl
@layout
  @slot header
    @text About us
  @paragraph This fills @children.
```

## Page and head

`@page TITLE` produces a full HTML document; without it the output is a fragment.
Its attributes set `lang`, `favicon`, `canonical` and `base`. `@meta NAME VALUE`
adds a meta tag (`og:` names become Open Graph `property` tags), and
`@head` holds any other raw HTML for the `<head>` (fonts, JSON-LD, a manifest
link). Translations are a JSON file per locale: `@data $t locales/$lang.json`.

```
@page [lang en, favicon /favicon.png] My Site
@meta description A small site
@meta og:title My Site
```

## CSS

- `@style` holds raw CSS, including at-rules such as `@keyframes`, `@scope`,
  `@property` and `@starting-style`.
- Generated rules live in `@layer htmlang` (after a `hl-reset` layer), so any
  CSS in `@style` or `@raw` overrides them.

```
@style
  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .note { color: gray; }
@el [animation fade-in 0.3s ease, class=note] Fades in
```

## Upgrading older files

`htmlang upgrade [dir|file]` rewrites removed syntax; the compiler reports each
removed form with its replacement.

| Removed | Use instead |
|---|---|
| `@fn`, `@define`, `@mixin`, `@component` | `@let` (function, bundle; `@style` in a function body is scoped) |
| `@unless COND` | `@if not COND` |
| `@for $i in A..B`, `@repeat N` | `@each $i in A..B`, `@each $_ in 1..N` |
| `grid` (attribute) | `@grid`, or `display grid` |
| `@switch`, `@match` | `@if $x == a` / `@else if` / `@else` |
| `@use "file" names`, `@import file` | `@include file` |
| `@collection`, `@env`, `@translations`, `@fetch` | `@data $name SOURCE` (glob, `env:NAME`, or a JSON file per locale) |
| `@theme` | `@let --name value` lines |
| `@json-ld`, `@font-face`, `@manifest` | Raw HTML in `@head`, CSS in `@style` |
| `@breakpoint`, `@deprecated` | A media query in `@style`; nothing |
| `@svg [attrs] file.svg` | `@image [inline, attrs] file.svg` |
| `gap-x`, `gap-y`, `shadow`, `blur N`, `truncate`, `critical` | `column-gap`, `row-gap`, `box-shadow`, `filter blur(N)`, `$truncate`, nothing |
| `@with $x as y` | `@let y $x` |
| `@extends layout.hl`, `@layout file` | `@include layout.hl` and a call to the layout, now a function (both files are converted) |
| `@scope`, `@starting-style`, `@css-property` | The CSS rule in `@style` |
| `@lang`, `@favicon`, `@canonical`, `@base` | `@page [lang ..., favicon ..., canonical ..., base ...] Title` |
| `@og KEY VALUE` | `@meta og:KEY VALUE` |
| `@assert`, `@warn`, `@debug`, `@log` | Nothing (the lines are removed) |
| `@defer` | Its content, directly |
| `@col`, `@p`, `@img`, `@li`, `@btn`, `@ul`, `@divider`, `@opt` | `@el`, `@paragraph`, `@image`, `@li`, `@button`, `@ul`, `@hr`, `@option` |
| `type email`, `id main` (HTML attributes written as styles) | `type=email`, `id=main` |
| `skeleton`, `no-scrollbar`, `gradient A B` | `$skeleton`, `$no-scrollbar`, `background linear-gradient(A, B)` |
| `@tooltip TEXT` | `@tooltip [tip TEXT] TEXT` |
| `@let x $a + 1` (computed without `=`) | `@let x = $a + 1` |
| `COND ? A : B` | `if(COND, A, B)` |
| `key if COND` (in attributes) | `if(COND, key)` |
| `$a ~ " " ~ $b` | `"$a $b"` |
| `@each $x in LIST [page N]` | Split the list, or filter it with `@if` |
| `...$bundle` | `$bundle` |
| `bold`, `italic`, `underline`, `hidden` | `font-weight bold`, `font-style italic`, `text-decoration underline`, `display none` |
| `size`, `font`, `rounded`, `padding-x/y`, `margin-x/y` | `font-size`, `font-family`, `border-radius`, `padding-inline/block`, `margin-inline/block` |
| `border N COLOR` (solid implied) | `border N solid COLOR` |
| `@each $a, $b in A x, B y` (items split on spaces) | Records: `@data $rows [{"a": "A", "b": "x"}, ...]` and `@each $row in $rows` with `$row.a` |
| `$_index` | `@each $item, $i in ...` |
| `$posts._count`, `$posts._keys`, `$posts.STEM.key` | `length($posts)`; nothing; `@each $post in $posts` with `$post.file` |
| `@raw """ ... """` | `@raw` with an indented body |
| `@keyframes NAME` with `from [opacity 0]` | The `@keyframes` rule in `@style` |
| `@include lib.hl as ui` | `@include lib.hl`, without the `ui.` prefix |
| `@data file.json` (keys as `$key`) | `@data $name file.json` (keys as `$name.key`) |
| `[attrs]` alone on a line | `@el [attrs]` |
| `animate`, `inset-area` | `animation`, `position-area` |
| `$x\|uppercase`, `$c\|darken:10` (filters) | `${uppercase($x)}`, `${darken($c, 10)}` |
