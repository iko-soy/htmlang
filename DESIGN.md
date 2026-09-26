# htmlang language reference

htmlang is a small layout language that compiles to static HTML. A page is a
tree of elements written one per line, with indentation for nesting:

```
@page Hello
@el [width fill, max-width 640, center-x, padding 40, spacing 16]
  @h1 Hello
  @paragraph
    This page was written in {@b htmlang}.
```

Every example block in this file is compiled by the test suite
(`tests/docs.rs`), so the examples stay correct.

## Principles

- **`@` starts structure, and any other line is content.** An element is `@name`,
  and a line without `@` is text.
- **htmlang's own vocabulary is about layout.** It comes from elm-ui. An
  element is a row, a column or text, `spacing` sets the gap between its children,
  and each child says how it sits in its parent (`width fill`, `center-x`,
  `align-right`). The layout attributes are the only styling words htmlang
  adds.
- **Everything else is CSS or HTML, under its own name.** A style is any CSS
  property with a CSS value. An element that isn't about layout or text has
  its HTML name, and an HTML attribute is written `key=value`. Nothing is
  renamed or abbreviated.
- **Each thing is done one way.** `@let` defines every reusable piece, `@data`
  loads data, `@include` brings in a file, and `if()` makes an attribute
  conditional. A layout is an ordinary function.
- **The output is what you wrote.** Each page compiles to one self-contained
  HTML file. Its CSS is inside, and it has no JavaScript unless you write
  some. The compiler doesn't add attributes, tags or rewritten values that
  the source doesn't ask for, and it doesn't leave out anything you wrote:
  what can't go into the page is an error.
- **Everything runs at compile time.** Variables, expressions, loops and
  conditions are resolved when the page is built.

## Syntax

### Elements

```
@element [attributes] argument
  children
```

Children are indented under their parent. Attributes go inside `[...]`,
separated by commas, and the list may span several lines:

```
@el [
  padding 20,
  background white,
  border-radius 8,
  box-shadow 0 2 4 rgba(0,0,0,0.1)
]
  Content
```

An attribute can take one of three forms:

- `key value` is a **style**: a layout attribute or a CSS property
  (`spacing 20`, `color red`).
- `key=value` is an **HTML attribute** (`id=main`, `type=email`,
  `aria-label=Close`).
- A bare word is a **flag**: a layout attribute such as `center-x`, or a
  boolean HTML attribute such as `required`, `disabled`, `hidden` or `open`.

A comma inside `(...)`, `[...]` or `"..."` doesn't split attributes. Anywhere else,
`\,` keeps a comma in the value:

```
@el [transition opacity 0.3s\, transform 0.3s, font-family "Open Sans"\, sans-serif]
```

A variable fills an attribute's value (`padding $gap`, `alt=$title`), never
its name or a whole attribute: attributes come from a
[bundle](#definitions), written `[$card]`, and a condition chooses whole
attributes with [`if()`](#conditional-attributes).

The attribute list belongs to the element name right before it. Anywhere
else, `[` is an ordinary character, so a line of text can contain one:

```
@text Use [ to open a list
@paragraph Arrays look like [1, 2, 3].
```

### Text

A line that doesn't start with `@` is text. Text after an element's
attributes is the element's content: its first line of text, read like any
other. Inside a line of text, `{...}` holds an inline element, and
[escapes](#escapes) and [`$names`](#variables) work:

```
@paragraph
  This is {@strong important}, and this is {@link https://example.com a link}.
@text [font-weight bold, font-size 24, color #333] Hello world
@h2 Welcome to {@text [color #3b82f6] htmlang}
@ul
  @li Read {@link /docs the docs}
@section [padding 8] Text after the attributes is content too.
```

How an element's lines combine is its [layout](#rows-columns-and-text)'s
business: in a row or column each line is a child; in a text element lines
flow. Text that `@if`, `@each`, `@fragment` or a function writes counts as
lines written in its place, so in a text element it flows with the rest,
joined with a space, and in a native element it is on lines of its own.

The text of `@code` and `@textarea` is shown as written. On its line and
inline, a `{@...}` in it is text, while escapes and `$names` still work.
The lines indented under it are a [verbatim](#verbatim-bodies) sample:
nothing in them is htmlang (`@`, `$`, `{`, `\` and `--` are text), and
they are HTML-escaped with their line breaks and indentation kept. This
is how a page shows code, htmlang included. `@pre` is an ordinary element,
so `@pre > @code` is HTML's `<pre><code>`, and a function can wrap `@pre`
around a sample. A sample is text on the line or lines in the block, not
both.

```
@paragraph
  Write {@code {@link /docs docs}} for a link.
@pre [padding 16] > @code
  @el [padding 40]
    <b>shown as text</b>
@textarea [aria-label=Notes]
  First line
    Second line, indented
```

A sample in the block has no `$names`, so a version number in it can't
come from a variable; a one-line sample on `@code`'s own line can
(`@code cargo install htmlang@$version`).

### Escapes

A backslash before one of these characters stands for the character itself.
The table is the same in every htmlang string: text, arguments, attribute
values, `@let`, `@page` and `@meta`.

| Escape | Writes | For |
|---|---|---|
| `\@`, `\--` | `@`, `--` | a line of text that starts with `@` or `--` |
| `\$` | `$` | a `$` before a name, which would start a variable |
| `\{`, `\}` | `{`, `}` | braces that don't start or end an inline element |
| `\[`, `\]` | `[`, `]` | brackets that don't open or close an attribute list |
| `\,` | `,` | a comma in an attribute's value |
| `\"` | `"` | a quote that doesn't start or end quoted text |
| `\\` | `\` | a backslash before one of these characters |

A backslash before any other character is kept as written, so CSS's own
escapes and patterns pass through: `before:content "\201C"`,
`pattern=\d{3}`. A `$` that isn't followed by a letter or `_` is text
anyway: `$5`, `$$`.

```
\@htmlang on social media
@let price 5
@paragraph \$price is $price dollars: {@mark \{braces\}}, \[brackets\] and a comma\, too
@input [type=text, pattern=\d{3}-\d{4}, title=Costs \$5]
```

### Comments

A line starting with `--` is a comment. A comment must be on its own line,
because `--` later in a line is ordinary text.

```
-- this is a comment
@row [spacing 10]
  -- todo: more nav items
  @link / Home
```

### Chains

`>` puts elements that have one child each on one line. The last element in
the chain gets the indented children. A `>` is a chain only between two
elements (`@name [attributes]`); in text it is just a character. A
function call can be a link of a chain, like any element.

```
@el [padding 16, background blue, border-radius 8] > @link https://example.com
  @text [color white] Get Started
```

### Verbatim bodies

The bodies of `@raw`, `@style`, `@head`, `@markdown`, `@script`, `@code`
and `@textarea` are foreign text, kept exactly as written: nothing in them
is parsed as htmlang, and a `--` line in them is not a comment. `@raw`
writes its HTML into the page, `@style` its CSS and `@head` its HTML into
the `<head>`; `@code` and `@textarea` show theirs as text. Which lines are
verbatim is decided by the element or directive on the line, also at the
end of a chain (`@pre > @code`, `@el > @script`).

The header's line follows one rule. For `@raw`, `@style` and `@head`, text
on the line is a one-line body; for `@markdown` it is the file and for
`@script` the `src`. Text on the line and an indented body together are an
error. Only `@script` takes attributes: an attribute list on `@raw`,
`@style`, `@head` or `@markdown` is an error, so content that starts with
`[` (a CSS attribute selector) goes in the indented block.

```
@raw <hr class="fancy">
@raw
  <div class="custom-widget">
    <span>Hand-written HTML</span>
  </div>
@style .note { color: gray; }
```

### Directives

A directive is a built-in word that isn't an element. Each one takes a
fixed kind of argument and a fixed kind of body:

| Directive | Argument | Body |
|---|---|---|
| `@page` | attributes (styles for `<body>`, `key=value` for `<html>`) and a title | none |
| `@let` | a name and a value or bundle, or `@name` and parameters | a function's body |
| `@include` | a file | none |
| `@data` | a variable and a source | none |
| `@meta` | a name and a value | none |
| `@if`, `@else`, `@each` | a condition or a loop | htmlang |
| `@raw`, `@style`, `@head` | the rest of the line: a one-line body | verbatim, instead of the line |
| `@markdown` | a file | verbatim Markdown, instead of the file |

An indented line under a directive that takes no body is an error, because
it would otherwise silently become a sibling.

### Diagnostics

The compiler checks the whole file, including code that doesn't run: the
branch of an `@if` or an `if()` that isn't taken, the body of a function that is never
called and the body of a loop over an empty list. Unknown elements,
functions and attributes are reported there too. Code that doesn't run may
name a function defined anywhere in the file or in an included file, so a
function's body can call a function defined further down; code that runs
needs the function's `@let` to have run first. Only a name that depends on
data, such as a variable a loop fills in, is checked just where the code
runs.

**Names are checked, and values are CSS's.** A misspelled element, prefix,
parameter or slot is an error, and so is an undefined variable. A CSS
property htmlang doesn't know, but whose name CSS could have, is written to
the CSS as it is, with a warning that suggests the closest known name
(`corner-shape`, or `colr`, which asks "did you mean `color`?"). Values go to
the CSS as written, so the browser decides what `z-index auto` or `color
rebeccapurple` means; only what is wrong in any CSS is reported (see
[CSS properties](#css-properties)).

**Nothing you write is left out silently.** An error means something
couldn't go into the page: a flag htmlang doesn't know, an attribute
`@fragment` has no element for, content for a function without
`@children`. A warning means it went into the page as written, but may not
be what you meant. A warning is reported once, even from a line that runs
many times in a loop or a function.

Every diagnostic has a stable code, such as `unknown-element` or
`unused-variable`. The command line prints it as `error[unknown-element]`,
`--format json` has it in a `code` field, and the editor's quick fixes are
keyed on it.

## Layout

### Rows, columns and text

Every element has one layout, which decides what the lines written inside
it are. **In a row or column each line is a child; in a text element lines
flow.**

| Layout | Elements | What is inside it |
|---|---|---|
| column | `@el`, and the containers: `@section`, `@nav`, `@form`, `@ul`, `@li`, ... | Each line of text is a child of its own, and the children are laid out top to bottom (side by side with `flex-direction row`) |
| row | `@row` | The same, side by side |
| grid | `@grid` | Each line of text is a cell |
| text | `@paragraph`, `@text`, `@link`, `@h1` … `@h6`, `@button`, `@label`, `@td`, `@strong`, `@em`, ... | The argument, the lines and the children flow as one run of text, joined with spaces |
| native | `@table`, `@caption`, `@select`, `@pre`, `@textarea`, `@video`, ... | HTML's own layout, which htmlang leaves alone |
| void | `@input`, `@hr`, `@br`, `@image`, `@source` | Nothing: it takes no content |

The table under [Elements](#elements) gives every element's layout. The
page itself is a column too: `@page` is the [root element](#page-and-head),
`<body>`, so what is written at the top of a page stacks like the children
of an `@el`, `height fill` there takes the rest of the window and
`center-y` centres in it.

```
@el [spacing 4]
  First line
  Second line
@button [type=button]
  Save
  changes
```

The column shows two lines 4px apart; the button says "Save changes".

These attributes are htmlang's own. They describe how an element lays out
its children, and how it sits in its parent:

| Attribute | Effect |
|---|---|
| `spacing N` | Gap between children |
| `width fill` / `width shrink` / `width N` | In a row, take the remaining width or keep the content's width; in a column or anywhere else, take the full width or fit the content; or an exact size |
| `height fill` / `height shrink` / `height N` | In a column, take the remaining height or keep the content's height; in a row or anywhere else, take the full height or fit the content; or an exact size |
| `center-x`, `center-y` | Center the element in its parent (auto margins) |
| `align-left`, `align-right`, `align-top`, `align-bottom` | Align the element in its parent (an auto margin on the other side) |
| `wrap` | Let a row wrap onto more lines |
| `grid-cols N`, `grid-rows N` | Equal grid columns or rows (on `@grid`) |
| `col-span N`, `row-span N` | Cells a grid child spans |

`spacing`, `wrap`, `grid-cols` and `grid-rows` lay out an element's
children, so they go on a row, column or grid. On a text, native or void
element they are an error: text has no gap between its lines. For a flex
layout of your own on such an element, write the CSS (`@label [display
flex, gap 8]`).

A child's `fill` and `shrink` follow the direction its parent's CSS
actually sets: a row for `@row`, a column for `@el` and the other columns,
or whatever the parent's own `flex-direction` (or `flex-flow`) says. Along
that direction, `fill` takes the remaining space and `shrink` keeps the
content's size; across it, `fill` is the full size and `shrink` fits the
content. `center-x`, `center-y` and `align-*` are auto margins, which work
in either direction; a `margin` on the same element keeps its other sides
(`[center-x, margin 20]`). A layout word never overrides a CSS property
the element writes itself: `[width fill, min-width 200]` keeps its 200px,
and `[flex-shrink 0, width fill]` still grows but never shrinks.

```
@row [spacing 8]
  @el [width fill, padding 12, background #f3f4f6] Fills the row
  @el [width 120, padding 12, background #e5e7eb] 120px
@el [height 200, spacing 8, background #f9fafb]
  @text [center-x] Centered
  @el [height fill, background #dbeafe] Fills the rest of the column
  @text [align-right] Right-aligned
@nav [flex-direction row, spacing 20]
  @link /features Features
  @el [width fill]
  @link /signup Sign up
@grid [grid-cols 3, spacing 8, md:grid-cols 4]
  @el [col-span 2, background #fef3c7] Two columns wide
  @el [background #fde68a] One
  @el [background #fcd34d] One
```

A direction set under a media or container prefix (`md:flex-direction
row`, `print:`, `cq-md:`) switches the children's `fill` and `shrink` in
that same condition. As in elm-ui, whose child rules are keyed on the
parent's class (`.r > .wf`), each child gets a rule keyed on its parent's
class in the parent's `@media` or `@container` block. A width prefix holds
from its width up, so `sm:flex-direction column` still applies at `lg:`
unless `lg:` sets a direction of its own. A direction under a state prefix
(`hover:`, `first:`, `children:`) leaves the children's layout words as
they are.

```
@header [spacing 16, md:flex-direction row, md:align-items center]
  @text [font-weight 800] Launchpad
  @el [width fill]
  @nav [flex-direction row, spacing 20]
    @link /features Features
    @link /pricing Pricing
@section [padding 24]
  @el [width fill, max-width 800, center-x, spacing 16]
    The full width up to 800px, in the middle
```

On a phone the header is a column and its `@el [width fill]` is the full
width; from md up it is a row and the `@el` takes the free space, pushing
the navigation to the right. The section's `@el` is the usual centred
column: the full width, at most 800px, with auto margins on both sides.

Everything else about layout is plain CSS: `justify-content`, `align-items`,
`flex 1 1 240px`, `grid-template-areas`, `position`.

### Layout inside text

A row, column or grid written inside text, inline in a line (`{@el ...}`) or
as a child of a text element, is laid out inline (`display: inline-flex`, or
`inline-grid`), and keeps its attributes. Anywhere inside text, `@el`, `@row`
and `@grid` (and `@in-front` and `@behind`) are written as a `<span>` rather
than a `<div>`, since text can't hold a `<div>`.

```
@paragraph
  Price: {@el [padding 2 6, background #fef3c7, border-radius 4] $9} a month.
@h2
  Plans
  @row [spacing 4, font-size 14] > @text New
```

Any other element keeps its HTML element. One whose HTML ends a paragraph,
such as `@section`, `@ul` or `@h2`, is a warning inside `@paragraph`
(`block-in-paragraph`): the browser would move it, and what follows it, out
of the `<p>`.

### Overlays: `@in-front` and `@behind`

These layers work like elm-ui's `inFront` and `behind`. Their children fill
the parent's bounds, and the parent becomes a positioning context
(`position: relative; isolation: isolate`). An explicit `position` on the
parent takes precedence.

```
@el [width 200, height 200, background blue]
  @text [color white] Main content
  @in-front
    @text [color yellow, align-right, align-bottom] Painted on top
  @behind
    @el [background red, height fill]
```

### Standard library

Two definitions are written in htmlang (`std.hl`) and are available in every
file. A `@let` of your own with the same name replaces them. `@spacer` takes
up the remaining space in a row or column. The `$truncate` bundle cuts text
off at one line with an ellipsis. You define any other components yourself
with `@let`.

```
@row [spacing 8, align-items center]
  @text Logo
  @spacer
  @text [$truncate, max-width 200] A long title that gets cut off
```

## Style

### CSS properties

Any attribute that isn't a layout attribute is a **CSS property**, with the
same name and value as in CSS: `padding 20`, `font-weight bold`,
`border 1 solid #e5e7eb`, `border-radius 8`, `background red`,
`grid-template-areas "a b"`, `display none`.

A quoted string in a value is CSS's own, so it keeps its quotes and its
commas: `font-family "Inter, sans-serif"` names one family called
"Inter, sans-serif", and the compiler warns about it. A font stack is
several values with commas between them, written `\,`:
`font-family Inter\, sans-serif`.

Values mean what they mean in CSS, with one addition: **in a length
property, every bare number outside parentheses and quotes is pixels**. So
`padding 20` is `20px`, `border 1 solid red` is `1px solid red`,
`box-shadow 0 2 4 rgba(0,0,0,0.1)` is `0 2px 4px rgba(0,0,0,0.1)`, and
`margin 0 auto` is `0 auto` (a zero stays `0`). Numbers inside a function
belong to it (`rgb(255 128 0)`, `calc(100% - 20px)`), and values that have
a unit or are keywords pass through as written (`width 50%`, `max-width
min(100%, 800px)`). Every other property takes numbers as CSS does, so
`opacity 0.5`, `z-index 2`, `line-height 1.5`, `flex 1 1 240px`,
`grid-column 1 / 3`, `initial-letter 3` and `border-image-width 2` are
written as they are, and so are custom properties (`--cols 3`), which have
no type to go by.

The length properties are these, and only these:

| Group | Properties |
|---|---|
| Size | `width`, `height`, `min-*` and `max-*` of both, `block-size`, `inline-size` and their `min-`/`max-`, `flex-basis`, `column-width`, `contain-intrinsic-*` |
| Space | `margin`, `padding`, `scroll-margin`, `scroll-padding` and every side of them, `gap`, `row-gap`, `column-gap`, `border-spacing` |
| Position | `top`, `right`, `bottom`, `left`, `inset` and `inset-*`, `translate`, `transform-origin`, `perspective`, `perspective-origin` |
| Border | `border` and every `border-*` shorthand, width and radius (not `border-image-*`), `outline`, `outline-width`, `outline-offset`, `column-rule`, `column-rule-width` |
| Shadow | `box-shadow`, `text-shadow` |
| Grid | `grid-template-columns`, `grid-template-rows`, `grid-auto-columns`, `grid-auto-rows` |
| Background | `background-position` (and `-x`, `-y`), `background-size`, `object-position`, `mask-position`, `mask-size` |
| Text | `font-size`, `letter-spacing`, `word-spacing`, `text-indent`, `text-decoration-thickness`, `text-underline-offset` |

Nothing else is added to a value, and a shorthand means what it means in
CSS: `outline 2 solid red` is `2px solid red`, while `outline 2 red` names
no line style, so CSS draws no outline. `container card / inline-size`
and `contain content` are CSS's own shorthands too, and a bare `contain` is
an error, like any style without a value. `line-clamp N` alone also adds
the `-webkit-box` declarations that browsers still need to cut text off
after N lines.

A property htmlang doesn't know is written to the CSS as it is, so new CSS
(`corner-shape squircle`) works before htmlang lists it; the compiler warns
and suggests the closest known name, in case it is a typo. Custom
properties (`--gap`) and vendor-prefixed ones (`-webkit-tap-highlight-color`)
pass without a warning. The value of a property htmlang doesn't know is
written exactly as it is, without pixels added. A style needs a value:
`[padding]` is an error. In a list that spans lines, a line that starts with
`--` is a comment, so a custom property goes after another attribute on its
line.

```
@el [--gap 12px, gap var(--gap), -webkit-tap-highlight-color transparent]
  Passed through as written
```

A value is checked only for what is wrong in any CSS. A `;`, `{` or `}`
(outside a quoted string, and for `;` outside parentheses, as in CSS's own
`if()`) or an unclosed quote or parenthesis would break out of the rule, and
a `</style` (even quoted) would end the page's style element, so each is an
error, including in a value that comes from a variable or from `@data`. A hex color that doesn't have 3, 4, 6 or 8 digits is a warning. A
style whose value comes out empty, such as a field that a record doesn't
have, is left out.

### Prefixes

A prefix applies a style only in some condition:

| Prefix | Applies |
|---|---|
| `hover:`, `active:`, `focus:`, `focus-visible:`, `focus-within:`, `disabled:`, `checked:`, `visited:`, `target:`, `valid:`, `invalid:`, `empty:`, `placeholder:`, `selection:` | In that state |
| `first:`, `last:`, `odd:`, `even:`, `nth:EXPR:` | By the element's position among its siblings |
| `children:` | To each direct child |
| `before:`, `after:` | To the `::before` / `::after` pseudo-element (together with `content`) |
| `has(SELECTOR):` | When the element contains a match |
| `sm:`, `md:`, `lg:`, `xl:`, `2xl:` | From that viewport width up (640, 768, 1024, 1280, 1536px) |
| `cq-sm:` … `cq-2xl:` | From that container width up (the same widths, for an ancestor with `container-type inline-size`) |
| `dark:`, `print:`, `motion-safe:`, `motion-reduce:`, `landscape:`, `portrait:` | Under that media condition |

A prefix goes on a style or a layout flag, and an attribute takes one
prefix: a misspelled prefix (`hovr:`) and a second one (`md:hover:`) are
errors, not styles that silently never apply.

```
@el [padding 16, background #3b82f6, hover:background #2563eb, md:padding 32, dark:background #1e3a8a]
  @text [color white] Click me
@row [spacing 4, children:flex 1]
  @el [odd:background #f3f4f6] A
  @el [odd:background #f3f4f6] B
  @el [odd:background #f3f4f6] C
@el [before:content "→ ", before:color red]
  Item with an arrow
```

### Conditional attributes

`if(CONDITION, A, B)`, written as a whole attribute, is `A` when the
condition holds and `B` otherwise. Each branch is an attribute, a `[group]`
of attributes or a `$bundle`. An empty branch leaves the attribute out, and
`B` itself can be left out. Several attributes that depend on one condition
are one `if()` with a group:

```
@let active true
@el [if($active, background blue, background gray), if($active, font-weight bold)]
  Conditionally styled
@link [if($active, [background #eef2ff, font-weight 600, aria-current=page])] /docs Docs
```

A value that depends on a condition is an [expression](#expressions),
`${if(CONDITION, A, B)}`. Its branches are expressions, so a number or a
single word is written as it is, and other CSS text is quoted:
`padding ${if($active, 24, 0)}`, `color ${if($active, #10b981, "var(--muted)")}`.
An `if()` written in a value without `${...}` is CSS's own `if()`, which the
browser decides, and it is passed through like any CSS function:
`width if(media(width > 40em): 50%; else: 100%)`.

## HTML

### Elements

The layout and text elements have elm-ui's names. Every other element has
its HTML name: `@strong`, `@em`, `@small`, `@br`, `@sub`, `@caption`,
`@tfoot`, `@optgroup`, `@hgroup` and the others in the table of layouts
below.

| Element | Output | Purpose |
|---|---|---|
| `@el` | div, flex column | The container: children laid out top to bottom |
| `@row` | div, flex row | Children laid out side by side |
| `@grid` | div, grid | Grid container (with `grid-cols` or `grid-template-*`) |
| `@in-front` / `@behind` | div, absolute | Overlay layers that fill the parent |
| `@text` | span | Styled inline text |
| `@paragraph` | p | A paragraph of flowing text |
| `@link URL` | a | Link: the URL is its `href`, and the text after it is its content |
| `@image SRC` | img | Image: the one word after its attributes is its `src` |
| `@fragment` | (none) | Its children, without a wrapper element (so it takes no attributes) |

Every element has one [layout](#rows-columns-and-text):

| Layout | Elements |
|---|---|
| column | `@el`, `@in-front`, `@behind`, `@nav`, `@header`, `@footer`, `@main`, `@section`, `@article`, `@aside`, `@address`, `@search`, `@noscript`, `@form`, `@details`, `@dialog`, `@figure`, `@blockquote`, `@fieldset`, `@hgroup`, `@ul`, `@ol`, `@menu`, `@li`, `@dl`, `@dd` |
| row | `@row` |
| grid | `@grid` |
| text | `@text`, `@paragraph`, `@link`, `@h1`, `@h2`, `@h3`, `@h4`, `@h5`, `@h6`, `@button`, `@label`, `@legend`, `@summary`, `@figcaption`, `@cite`, `@dt`, `@td`, `@th`, `@code`, `@kbd`, `@mark`, `@abbr`, `@time`, `@b`, `@i`, `@strong`, `@em`, `@small`, `@s`, `@u`, `@sub`, `@sup`, `@q`, `@var`, `@samp`, `@dfn`, `@bdi`, `@bdo`, `@ins`, `@del` |
| native | `@table`, `@caption`, `@colgroup`, `@thead`, `@tbody`, `@tfoot`, `@tr`, `@select`, `@optgroup`, `@option`, `@datalist`, `@textarea`, `@progress`, `@meter`, `@output`, `@pre`, `@ruby`, `@rt`, `@rp`, `@picture`, `@video`, `@audio`, `@iframe`, `@object`, `@map`, `@canvas`, `@script` |
| void | `@image`, `@input`, `@hr`, `@br`, `@wbr`, `@col`, `@source`, `@track`, `@embed`, `@area` |

`@fragment`, `@children` and `@slot` have no element of their own: what
they hold takes the layout of the element they are in. The top of a page is
a column, `<body>` (see [Page and head](#page-and-head)). In a native
element, and at the top of a fragment, lines of text are separated by a line break,
which HTML shows as a space except where whitespace is kept, as in `@pre`
and `@textarea`.

The text-level elements (`@strong`, `@em`, `@b`, `@i`, `@small`, `@sub`,
`@q`, `@del`, ...) are text with the browser's own style and no CSS of
htmlang's, so they work inside a line like any inline element. Table parts,
`@optgroup` and `@ruby` keep HTML's own layout:

```
@paragraph
  {@strong Note:} water is H{@sub 2}O,{@br}
  and {@em this} is {@small fine print}.
@table
  @caption Team
  @thead
    @tr
      @th Name
  @tbody
    @tr
      @td Ada
  @tfoot
    @tr
      @td One person
@select [aria-label=Fruit]
  @optgroup Citrus
    @option Lemon
```

The list of elements is fixed: a name that isn't in it is an unknown
element, not a new HTML tag, since a function is called with the same `@`
and a misspelled one would otherwise become a tag. HTML's `<div>`, `<span>`,
`<p>`, `<a>` and `<img>` are htmlang's own `@el`, `@text`, `@paragraph`,
`@link` and `@image`, so `@a` is an unknown element whose error suggests
`@link`. The head's elements (`<meta>`, `<style>`, `<title>`, `<link>`) are
written with [directives](#page-and-head). `<data>` and `<slot>` share their
names with `@data` and `@slot`, and `<template>` holds content that only a
script uses; these, and `<svg>` and `<math>`, are written with `@raw`.

An element without a closing tag (`@input`, `@hr`, `@br`, `@image`,
`@source`, ...) takes no content.

Browser default margins on headings, paragraphs, lists and figures are reset
to 0 (in the element's own class, so Markdown and raw HTML keep theirs; see
[CSS](#css)), so `spacing` controls the gaps. Lists (`@ul`, `@ol`, `@menu`) and list
items are columns like other containers, so `spacing` on a list is the gap between its items, and
a list shows no markers. To bring them back, write
`[list-style disc, padding-inline-start 20, children:display list-item]` on
the list.

```
@ol [spacing 4]
  @li First
  @li Second
@form [method=post, spacing 8] /subscribe
  @label [for=email] Email
  @input [type=email, name=email, id=email, required]
  @button [type=submit] Send
@details [open]
  @summary Question
  @text The answer.
```

### The leading argument

An element that points at a URL or a file takes it as the first word after
its attributes, its leading argument, which fills one HTML attribute:

| Element | Leading argument |
|---|---|
| `@link`, `@area` | `href` |
| `@image`, `@script`, `@iframe`, `@video`, `@audio`, `@track`, `@embed` | `src` |
| `@source` | `srcset` directly inside `@picture`, `src` inside `@video` and `@audio` |
| `@form` | `action` |
| `@object` | `data` |
| `@optgroup` | `label` |

The first word is taken before any `$name` in it is filled in, and it ends
at the first space that isn't inside `"..."` or `${...}`: `@link $url More`
and `@link /page/${$n + 1} Next` take the whole `$url` and
`/page/${$n + 1}`, and `@link "/my page" Open` takes `/my page`. After it,
the rest of the text is the element's content, read like any text; an
element without content (`@image`, `@source`, `@track`, `@embed`, `@area`)
takes nothing more, and a word after its argument is an error (write a
text alternative as `alt=...`). So is a word after an `@optgroup`'s label,
since it holds only the `@option` lines under it. A value with a space in
it is quoted, as such a label: `@optgroup "Citrus fruits"`. The first word is
always the argument, so on an element whose attribute is often left out
(`@form`, `@video`, `@audio`), text goes on the lines under it: `@form Sign
in` would make `Sign` the form's action.

The attribute can be written as an attribute instead, and it means the same:
`@link [href=/about]` with its text on the lines under it, or
`@image [src=logo.svg, inline]`. Both at once, as in
`@link [href=/a] About`, is an error that says which word was taken as the
`href`.

```
@nav [flex-direction row, spacing 16]
  @link /docs The docs
  @link [aria-current=page] /pricing Pricing
@image [alt=The team, width 320] "team photo.jpg"
@picture
  @source [type=image/avif] hero.avif
  @image [alt=The office] hero.jpg
@video [controls] intro.mp4
  @track [kind=captions, srclang=en] intro.vtt
  Your browser doesn't play this video.
```

`@script` is an element like the others: its leading argument is its `src`,
any HTML attribute passes through (`@script [type=module, defer] app.js`),
and without a `src` its indented body is its JavaScript, kept exactly as
written. A `src` and a body together are an error, since the browser runs
only the file. It isn't shown, so a style on it is an error.

```
@script [defer] analytics.js
@script
  document.body.dataset.ready = "yes"
```

### HTML attributes

HTML attributes are written `key=value`: `id=main`, `class=note`,
`href=/about`, `type=email`, `alt=Logo`, `target=_blank`,
`aria-label=Close menu`, `data-id=42`. Any name works. Boolean attributes are
written bare: `required`, `disabled`, `checked`, `hidden`, `open`, `popover`.
An HTML attribute written like a style (`type email`) is an error that
shows the `key=value` form. A style
and an HTML attribute can share a name, because the `=` tells them apart:

```
@image [width=800, width 200, alt=A photo] photo.jpg
@label
  Size
  @select [size=4, font-size 18]
    @option One
```

## Definitions

`@let` defines everything reusable. What it defines depends on its form:

```
-- A value, used as $primary
@let primary #3b82f6
-- A computed value: `=` makes the rest an expression
@let gap = 8 * 2
-- Quoted text, with $variables filled in (see Variables for its quotes)
@let greeting "Hello from $primary"
-- An attribute bundle, used as [$card]
@let card [padding 20, background white, border-radius 8]
-- A function: `@` before its name, its parameters in brackets, and a body
@let @panel [title]
  @el [$card, spacing $gap]
    @text [font-weight bold, color $primary] $title
    @children

@panel [title Welcome]
  $greeting
@el [$card, background #f9fafb]
  Attributes after a bundle override it.
```

The line alone says what a `@let` defines: an `@` before the name makes a
function, a `[` after the name an attribute bundle, and anything else a
value. A value or a bundle is named without `$` and used with it; a function
is named with `@` and called with it. Only a function has a body, so an
indented block under a value or a bundle is an error, and so are `@let $x`
(write `@let x`) and a `@let` without a value.

Values, bundles and functions share one namespace, and one rule says where
a name is visible: **from the line that defines it to the end of its
block**. A block is the file, the lines indented under an element, a branch
of `@if`, one repetition of `@each` or a function's body. A `@let` gives its
name a new meaning in its block, whatever the name meant before, and hides
what the name means around the block until the block ends:

```
@let gap 8
@el [spacing $gap]
  @let gap 16
  @el [padding $gap] Sixteen
@el [padding $gap] Eight
```

Parameters, `@each` variables and `@data` names follow the same rule, and
each repetition of `@each` starts afresh, so a `@let` in one doesn't carry
over to the next. A function's body sees its parameters and what is
visible where the function is defined: not the names at a call, and not a
definition further down, so a function means the same wherever it is
called. `@let t.greeting Hello` gives the record `$t` the field `greeting`
(making `$t` a record when it isn't defined), and `@let items.0 first`
replaces the first item of the list `$items`; a field of any other value is
an error.

### Variables

`$name` inserts a value wherever htmlang holds text: a line of text, an
attribute's value, an element's argument, a `@let` value, a file path, the
title of `@page` and the value of `@meta`. Each line is read first, and then
`$name` fills the one place it is written in. What it inserts is never read
again as htmlang, so a value can't become an attribute, an attribute's name,
a comma between attributes, an inline element or another `$name`. Attributes
come from a bundle:

```
@let link-style [color #2563eb, text-decoration underline]
@let label Docs
@link [$link-style, aria-label=$label] /docs Read the $label
```

A name starts with a letter or `_` and goes on with letters, digits, `_`
and `-`. It ends at the first other character, so after `@let lang fr`,
`$lang.json` is `fr.json`. A `.field` continues the name only when the value
is a record or a list (`$post.title`, `$tags.0`). `${name}` is the same name
with explicit ends, for text that follows it directly, and `${EXPR}` inserts
the value of an [expression](#expressions). A `$` followed by anything else
is text.

```
@let lang fr
@let size 4
@data $post {"title": "Hello"}
@text locales/$lang.json, ${size}px, $post.title, $5
```

A name that isn't defined is an error. A field that a record doesn't have
is empty, and so is any field of it, so optional fields of `@data` records
work: `@if $post.draft` is false, `$post.author.name` is empty when there is
no `author`, and `${default($post.tag, none)}` gives `none`.

### Values

A value is text, a number, `true` or `false`, a list or a record, and a
name holds a whole value: a `@let`, a parameter, `@each` and `@data` bind
one, and binding the name again replaces it. What a `@let` holds is what it
says:

```
-- Text, as written
@let size 16px
-- Commas make a list, which prints as it was written
@let fruits apple, banana, cherry
-- Quoted, or with `\,`, text with commas in it is one text
@let tagline "Fast, simple"
@let motto small\, fast
-- A range of whole numbers
@let steps 1..9 step 4
-- One `$name` or `${...}` keeps the type of what it holds
@data $posts [{"title": "Hello"}, {"title": "Again"}]
@let first $posts.0
@let backwards ${reverse($fruits)}
@text [font-size $size] $fruits: ${length($fruits)}. $tagline: ${length($tagline)}. $motto. $steps
@text $first.title, $backwards
```

The last lines give `apple, banana, cherry: 3. Fast, simple: 12. small,
fast. 1..9 step 4` and `Hello, cherry, banana, apple`. A value that is
exactly one `$name` or `${...}` keeps its type wherever it goes: a `@let`, a
function's parameter or default, the list of `@each`. So a record can be
passed to a function whole (`@post-card [post $p]`), and a list stays a
list.

- Text written without quotes reads as a number when it is one (`3`,
  `0.5`), and as false when it is `false` or `0`: that is how htmlang writes
  numbers and flags (`@let gap 8`, `@card [dot false]`). Quoted text is
  always text.
- **False** is `false`, empty text, `0` and an empty list (or record).
  Everything else is true, including `no` and quoted `"0"`.
- `==` and `!=` compare numbers as numbers (`1.0 == 1`), lists and records
  item by item, and anything else as text, exactly (`#FFF` isn't `#fff`).
  `<`, `>`, `<=` and `>=` compare numbers as numbers and text by character
  order.
- A computed number prints without float noise: whole numbers without
  decimals, others with at most four (`${100 / 3}` is `33.3333`).
- A list prints as written when the source wrote it, and otherwise as its
  items joined with `, `. A record has no text of its own, so writing one
  where text goes is an error: write one of its fields.

**Quoted text remembers that it was quoted.** A value written `"..."` (in a
`@let`, an attribute or a function's argument) keeps its quotes in a CSS
value, where CSS needs them, and loses them in text, in HTML attribute
values and in parameters. It stays quoted when it passes through another
`@let` or a parameter. Inside a quoted string of a CSS value, it inserts
what it says, without its own quotes:

```
@let arrow "→ "
@let label "Next, please"
@el [before:content $arrow, after:content " ($label)", aria-label=$label] $label
```

This gives `content:"→ "`, `content:" (Next, please)"`,
`aria-label="Next, please"` and the text `Next, please`. A value written
without quotes is inserted as written everywhere.

### Functions

A function's parameters are listed in brackets after its name and written
the way a call passes them: a name alone (`title`) is a required parameter,
and a name and a value (`tone #f9fafb`) is a parameter with that default.
The body uses them as `$title` and `$tone`. `@let @name` without brackets
takes no parameters.

A default is filled in at each call that leaves its parameter out, like a
value passed for it: it may hold spaces, quotes, escapes, `${...}` and
`$variables`. It sees what the body sees: the names visible where the
function is defined and the parameters declared before it
(`[title, heading "About $title"]`). A default that uses a parameter
declared after it is an error.

A function is called like an element, wherever an element can be: on a
line of its own, in a [chain](#chains) (`@el > @card [title Hi]`) or
inline in text (`{@key Ctrl+K}`):

- Its parameters are passed by name, as attributes: `name value`, or the
  name alone for `true`. A parameter with a default can be left out;
  leaving out one without a default is an error that names it. `=` writes
  an HTML attribute, so `title=Hi` for a parameter is an error too.
- Every other attribute goes to the function's root element: a style, a
  flag, or a `key=value` HTML attribute such as `id=intro`. It is checked
  like an attribute written on that element, so `@panel [title Hi,
  padding 40]` works the same as styling a built-in element, and
  `[paddin 40]` gets the same warning. A name close to one of the
  parameters (`titel Hi`) is a misspelled parameter, which is an error.
- Text after the attributes and the indented lines are the call's
  content, and replace `@children` in the body. A `@slot NAME` block
  directly under the call (or under an `@if`, `@else` or `@each` there)
  fills the body's `@slot NAME` instead.
- The lines indented under `@children` or `@slot NAME` in the body are its
  fallback: they are shown when a call passes no content, or no block for
  that slot.

```
@let @card [title, tone #f9fafb]
  @article [padding 16, spacing 8, background $tone, border-radius 8]
    @row [align-items center]
      @h3 $title
      @spacer
      @slot actions
    @children
    @slot footer
      @text [font-size 12, color gray] No footer

@card [title Plain]
  Just children.
@card [title Full, tone #eff6ff, padding 24]
  @slot actions
    @link /edit Edit
  The children go here.
  @slot footer
    @text Updated today
```

`@children` takes a fallback the same way:

```
@let @notice
  @el [padding 12, background #fef3c7]
    @children
      @text Nothing to report.

@notice
@notice Deploys are paused today.
```

Content that would go nowhere is an error, so a mistake can't drop it:

- a `@slot NAME` block for a slot the function doesn't have (the message
  lists the slots it has);
- text or lines passed to a function whose body has no `@children`;
- a `@slot NAME` block inside an element at the call, instead of directly
  under the call;
- `@slot` or `@children` outside a function's body, or inline in text.

A slot's name is one word of letters, digits, `-` and `_`, starting with a
letter, like a function's. Inside a body, a call can pass on what the
function got: `@children` under the call passes the content, and a
`@slot footer` block holding `@slot footer` passes that slot.

A parameter that is off unless the call names it is a flag: give it the
default `false`, and name it alone to turn it on.

```
@let @post-card [post, featured false, label "About $post"]
  @article [spacing 8, padding ${if($featured, 24, 0)}]
    @if $featured
      @text [font-weight bold] Featured
    @h3 $label

@post-card [post htmlang, featured]
@post-card [post CSS]
```

An `@style` block at the top of a function body is **scoped** to the
function. Its rules apply inside the function's root element, and `&` is the
root itself. This requires the body to have a single root element.

```
@let @note [kind Note]
  @style
    & { border-left: 4px solid #3b82f6; }
    .title { font-weight: bold; }
  @el [padding 12, border-radius 6, background #eff6ff]
    @text [class=title] $kind
    @children

@note [kind Tip, padding 20] Scoped styles and forwarded attributes.
```

A call in text or in a chain whose body has several roots, or is text,
puts them in its place, like `@fragment`. A function whose body is text
holds multi-line content, and a small function works inside a sentence:

```
@let @intro
  htmlang is a layout language.
  It compiles to {@strong static HTML}.
@let @key
  @kbd [padding 1 6, border 1 solid #d1d5db, border-radius 4]
    @children

@paragraph
  @intro
@paragraph
  In short: {@intro} Press {@key Ctrl+K} to search.
```

A function may call itself, under a condition that stops it, so it can
render nested data such as a menu. A parameter passed one `$name` that
holds a record or a list gets the whole value, fields and items
included. Calls nest at most 64 deep: a function that calls itself with
nothing to stop it is an error.

```
@data $menu [
  {"label": "Guide", "children": [{"label": "Install"}, {"label": "Usage"}]},
  {"label": "Reference"}
]
@let @tree [items]
  @ul [padding-left 16]
    @each $item in $items
      @li
        @text $item.label
        @if $item.children
          @tree [items $item.children]

@tree [items $menu]
```

The elements a function's body writes are the call's: a problem with
one of them, such as low contrast or `spacing` on an element that isn't a
row, column or grid, is reported at the call, once, and names the function. A
problem in the text of a body is reported on its own line, or at the call
when the function comes from `std.hl` or another file.

A function can't take the name of a built-in element or a directive:
`@let @button` would replace `@button` in every call after it, and a
function named `@if` could never be called, so both are warnings.

### CSS custom properties

`@let --name value` declares the CSS custom property `--name` on `:root`.
`var(--name)` refers to it at run time, so a stylesheet can still override
it. `$--name` is its value at compile time, for use in expressions:

```
@let --brand #3b82f6
@el [background var(--brand), border 1 solid ${darken($--brand, 10)}] Themed
```

## Expressions

Conditions and computed values (`@let x = ...`) are expressions:

| | |
|---|---|
| Values | numbers, `"strings"` (with `$var` interpolation), `$variables` (with their type: a list or a record too), `$record.field`, `true`, `false`, ranges `A..B` and `A..B step N`, and bare words (`dark`, `#fff`) as text |
| Arithmetic | `+ - * / %` with the usual precedence, unary `-`, `( )` |
| Comparison | `== != < > <= >=`: numbers as numbers, text exactly, lists and records item by item (see [Values](#values)) |
| Logic | `and`, `or`, `not`. `false`, empty text, `0` and an empty list are false |
| Choice | `if(CONDITION, A, B)`, where `B` may be left out (empty) |
| Tests | `contains(s, x)` (in a text, or as an item of a list), `starts-with(s, x)`, `ends-with(s, x)` |
| Text | `uppercase(s)`, `lowercase(s)`, `capitalize(s)`, `trim(s)`, `length(s)` (the items of a list, or the characters of a text), `reverse(s)` (a list's items, or a text's characters), `truncate(s, n)`, `replace(s, old, new)`, `default(s, fallback)` (the fallback when `s` is empty text or an empty list) |
| Color | `lighten(c, pct)`, `darken(c, pct)`, `alpha(c, a)`, `mix(c1, c2, pct)` |

Variables are looked up during evaluation, so a value that contains `==` or
spaces is still one value. An invalid expression is a compile error, and so
is an undefined variable. A text function given a list is an error too,
instead of a guess: `uppercase()` of `Fast, simple` (a list, because of its
comma) says so, and the text is written `"Fast, simple"` or `Fast\, simple`.
A variable's name ends at `..`, so `${$from..$to}` is a range.

Only what decides the result is evaluated: `if()` evaluates the branch it
takes, and `and` and `or` stop at the side that decides. So
`${if($n != 0, 10 / $n, 0)}` and `@if $n != 0 and 10 / $n > 1` work when
`$n` is 0. The rest is still read, and its syntax and function names are
checked.

In text, values and file paths, `${EXPR}` inserts the value of any
expression (see [Variables](#variables)):

```
@let name htmlang
@let base #3b82f6
@text [color ${darken($base, 10)}] ${uppercase($name)} has ${length($name)} letters
```

## Control flow

All control flow runs at compile time.

```
@let count 3
@let hidden false

@if $count > 2 and not $hidden
  @text Many
@else if $count == 0
  @text None
@else
  @text Few
```

`@each $item in LIST` repeats its body for each item. Its variables are
written with `$`, like `@data $name`. LIST is a value, read like the value
of a `@let`: items written with commas (a quoted item or one with `\,` is
one text), a range `A..B` (which counts down when its start is greater than
its end, and takes `step N`), or one `$name` or `${...}` that holds a list,
such as one loaded with `@data` or `${reverse(1..5)}`. A text is one item,
and empty text none. Each item is bound whole, so a record keeps its fields.
An optional second variable is the index, starting from 0. `@else` gives
the content to show when the list is empty.

```
@let fruits apple, banana, cherry
@each $fruit, $i in $fruits
  @text ${$i + 1}. $fruit
@else
  @text No fruit.

@row [spacing 8]
  @each $n in 10..0 step 5
    @text $n
```

## Data and files

| Directive | Effect |
|---|---|
| `@data $name file.json` | Load a JSON file. Objects are records (`$name.key`) and arrays are lists |
| `@data $name [...]` / `@data $name {...}` | Inline JSON, which may span several lines |
| `@data $name dir/*.json` | A list with one record per file, in name order. `$item.file` is the file's name |
| `@data $name env:NAME [default]` | An environment variable |
| `@include file.hl` | Insert another file: its content and its definitions. A file that holds only `@let`s outputs nothing (a [library](#layouts)) |
| `@markdown` / `@markdown file.md` | Markdown (an indented body or a file), converted to HTML |
| `@image [inline] file.svg` | Put the file inside the page: SVG as markup (`width`, `height`, `color`, `class=` and `id=` apply to it), other images as base64. `inline` goes only on `@image`, without a prefix |

Records keep their values whole, even when a value contains spaces or
commas:

```
@data $team [
  {"name": "Ada", "role": "Engineering, research"},
  {"name": "Grace", "role": "Design"}
]
@ul [spacing 4]
  @each $person in $team
    @li
      @text [font-weight bold] $person.name
      @text $person.role
@text ${length($team)} people
```

### Layouts

A layout is an ordinary function in a file of its own. It marks where content
goes with `@slot NAME` (named blocks) and `@children` (everything in the call
that is outside a `@slot` block), each with fallback content indented under
it.

```
-- layout.hl
@let @layout
  @page [lang=en, background #f8fafc] My Site
  @el [width fill, max-width 800, center-x, spacing 24]
    @header
      @slot header
        @text Default header
    @main
      @children
```

```
-- page.hl
@include layout.hl
@layout
  @slot header
    @h1 About us
  @paragraph This fills @children.
```

The layout holds the page's `@page`, whose attributes can come from its
parameters (`@page [background $bg] $title`). A page has one `@page`, so a
second one is an error (`duplicate-page`): the page's own next to its
layout's, or a layout called twice. The same `@meta` tag twice is written
once. A file that holds only `@let`s, such as `layout.hl`, is a library:
its definitions aren't reported unused, in it or in a page that includes
it and uses only some of them, and `htmlang build`, `serve` and `watch`
don't build it into a page of its own.

## Page and head

`@page TITLE` makes the output a full HTML document. Without it, the output
is a fragment. The page is the root element, and `@page`'s attributes are
checked like any element's:

- Styles style `<body>`, prefixes included, so a page's colour, font and
  dark background go on `@page` and cover the whole window.
- `key=value` attributes go on `<html>`: `lang=en`, `dir=rtl`, `class=x`.
- `favicon FILE` is its one word of htmlang's own: the file is put into
  the page as its icon (or linked, when it can't be read).

`<body>` is a column that fills the window (`display: flex;
flex-direction: column; min-height: 100dvh`, from the reset), so the
page's top-level elements are laid out like the children of an `@el`:
they stack, even a line of text or an inline element such as `@text` or
`@link`, and each takes the full width unless it says otherwise. A
centred column at the top writes `width fill` like anywhere else (see
[Rows, columns and text](#rows-columns-and-text)).

`@meta NAME VALUE` adds a meta tag, and names that start with `og:` become
Open Graph `property` tags; a `@meta viewport` replaces the usual one.
`@head` holds any other raw HTML for the `<head>`, such as a font link, a
canonical URL or JSON-LD, on its line or in its indented block.

```
@page [lang=en, favicon favicon.png, color #1f2937, dark:background #0b1220, dark:color #e5e7eb] My Site
@meta description A small site
@meta og:title My Site
@head <link rel="canonical" href="https://example.com/">
@head
  <script type="application/ld+json">{"@type": "WebSite"}</script>
@header [padding 16] My Site
@main [height fill, padding 16]
  The main part takes the rest of the window.
@footer [padding 16] © 2026
```

Translations are a JSON file per locale: `@data $t locales/$lang.json`
loads `locales/fr.json` when `$lang` is `fr`.

## CSS

`@style` holds raw CSS, including at-rules such as `@keyframes`,
`@font-face`, `@property` and `@starting-style`, in its indented block or,
for a single rule, on its line:

```
@style
  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .note { color: gray; }
@style .quiet { opacity: 0.6; }
@el [animation fade-in 0.3s ease, class=note] Fades in
```

Each element with styles gets a short generated class, `hl-a`, `hl-b`, ...,
and elements with the same styles share one. The `hl-` prefix is htmlang's:
a class of your own that starts with it gets a warning, since the generated
rules would apply to it too. Every style goes into the class under its own
name, with its value as written (plus pixels, see
[CSS properties](#css-properties)); only the layout words, and
`line-clamp`'s fallback, write more than one declaration.

An element's defaults are part of its own class: the margins browsers give
headings, paragraphs, lists, figures and blockquotes are 0, lists have no
markers, `@fieldset` has a thin border and padding, `@code`, `@kbd` and
`@pre` are monospace, `@link` has no underline and takes its parent's text
colour, and `@image` is a block. They apply only to htmlang's own elements,
so HTML that comes from `@markdown` or `@raw` keeps the browser's defaults:
a Markdown list keeps its bullets and a Markdown link its underline.

The generated rules live in `@layer htmlang`, after a small reset in
`@layer hl-reset`. A page's reset sets `box-sizing: border-box` everywhere
and makes `<body>` a column that fills the window. A fragment (a file
without `@page`, or `--partial` output) goes into a page it doesn't own, so
its reset touches only htmlang's own elements (those with an `hl-` class),
which get `box-sizing: border-box`; no other element on the page is
touched. In both, an element
htmlang lays out stays hidden while it has `hidden`, while an `@dialog` is
closed, and while a popover isn't showing, even though its generated
`display` would otherwise beat the browser's `display: none`. A page with
links, buttons or form fields also gives htmlang's ones a focus outline.
CSS outside any layer takes precedence over both layers, so rules in
`@style` or `@raw` override the generated ones.
