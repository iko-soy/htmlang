# htmlang language reference

htmlang is a small layout language that compiles to static HTML. A page is a
tree of elements written one per line, with indentation for nesting:

```
@page Hello
@el [max-width 640, center-x, padding 40, spacing 16]
  @h1 Hello
  @paragraph
    This page was written in {@text [font-weight bold] htmlang}.
```

Every example block in this file is compiled by the test suite
(`tests/docs.rs`), so the examples stay correct.

## Principles

- **`@` starts structure, and any other line is content.** An element is `@name`,
  and a line without `@` is text.
- **htmlang's own vocabulary is about layout.** It comes from elm-ui. An
  element is a row or a column, `spacing` sets the gap between its children,
  and each child says how it sits in its parent (`width fill`, `center-x`,
  `align-right`). The layout attributes are the only styling words htmlang
  adds.
- **Everything else is CSS or HTML, under its own name.** A style is any CSS
  property with a CSS value. An element that isn't about layout has its HTML
  name, and an HTML attribute is written `key=value`. Nothing is renamed or
  abbreviated.
- **Each thing is done one way.** `@let` defines every reusable piece, `@data`
  loads data, `@include` brings in a file, and `if()` makes an attribute
  conditional. A layout is an ordinary function.
- **The output is what you wrote.** Each page compiles to one self-contained
  HTML file. Its CSS is inside, and it has no JavaScript unless you write
  some. The compiler doesn't add attributes, tags or rewritten values that
  the source doesn't ask for.
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
  box-shadow 0 2px 4px rgba(0,0,0,0.1)
]
  Content
```

An attribute can take one of three forms:

- `key value` is a **style**: a layout attribute or a CSS property
  (`spacing 20`, `color red`).
- `key=value` is an **HTML attribute** (`id=main`, `type=email`,
  `aria-label=Close`).
- A bare word is a **flag**: a layout attribute such as `center-x`, or a
  boolean HTML attribute such as `required`, `disabled` or `open`.

A comma inside `(...)` or `"..."` doesn't split attributes, so a font stack
is written `font-family "Inter, sans-serif"`.

A variable fills an attribute's value (`padding $gap`, `alt=$title`), never
its name or a whole attribute: attributes come from a
[bundle](#definitions), written `[$card]`.

The attribute list belongs to the element name right before it. Anywhere
else, `[` is an ordinary character, so a line of text can contain one:

```
@text Use [ to open a list
@paragraph Arrays look like [1, 2, 3].
```

### Text

A line that doesn't start with `@` is text. Text after an element's
attributes is the element's content. Inside a line of text, `{...}` holds an
inline element:

```
@paragraph
  This is {@text [font-weight bold] important}, and this is {@link https://example.com a link}.
@text [font-weight bold, font-size 24, color #333] Hello world
@section [padding 8] Text after the attributes is content too.
```

A backslash makes the next character literal:

- `\@` and `\--` let a line of text start with `@` or `--`.
- `\$` writes a `$` that would otherwise start a variable. A `$` that isn't
  followed by a letter or `_` is text anyway: `$5`, `$$`.
- `\{` writes a brace that doesn't start an inline element.
- `\\` writes a backslash.

```
\@htmlang on social media
@let price 5
@text \$price is $price dollars
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
elements (`@name [attributes]`); in text it is just a character.

```
@el [padding 16, background blue, border-radius 8] > @link https://example.com
  @text [color white] Get Started
```

### Verbatim bodies

`@raw` writes HTML into the output exactly as given, either the rest of its
line or its indented block. The bodies of `@style`, `@head`, `@script` and
`@markdown` are also kept exactly as written: nothing in them is parsed as
htmlang, and a `--` line in them is not a comment.

```
@raw <hr class="fancy">
@raw
  <div class="custom-widget">
    <span>Hand-written HTML</span>
  </div>
```

### Directives

A directive is a built-in word that isn't an element. Each one takes a
fixed kind of argument and a fixed kind of body:

| Directive | Argument | Body |
|---|---|---|
| `@page` | attributes and a title | none |
| `@let` | a name and a value, bundle or parameters | a function's body |
| `@include` | a file | none |
| `@data` | a variable and a source | none |
| `@meta` | a name and a value | none |
| `@if`, `@else`, `@each` | a condition or a loop | htmlang |
| `@style`, `@head` | none | verbatim |
| `@raw`, `@markdown` | the rest of the line (or a file, for `@markdown`) | verbatim, instead of the argument |

An indented line under a directive that takes no body is an error, because
it would otherwise silently become a sibling.

### Diagnostics

The compiler checks the whole file, including code that doesn't run: the
branch of an `@if` that isn't taken, the body of a function that is never
called and the body of a loop over an empty list. Unknown elements,
functions and attributes are reported there too. Code that doesn't run may
name a function defined anywhere in the file or in an included file, so a
function's body can call a function defined further down; code that runs
needs the function's `@let` to have run first. Only a name that depends on
data, such as a variable a loop fills in, is checked just where the code
runs.

Every diagnostic has a stable code, such as `unknown-element` or
`unused-variable`. The command line prints it as `error[unknown-element]`,
`--format json` has it in a `code` field, and the editor's quick fixes are
keyed on it.

## Layout

### Rows and columns

`@el` is the container, and it lays out its children in a column. `@row`
lays them out side by side. Every other container (`@section`, `@nav`,
`@article`, `@form`, and so on) is a column too, so `@row` is the only
element that changes direction. `@grid` is a CSS grid.

These attributes are htmlang's own. They describe how an element lays out
its children, and how it sits in its parent:

| Attribute | Effect |
|---|---|
| `spacing N` | Gap between children |
| `width fill` / `width shrink` / `width N` | Take the remaining space in a row (the full width in a column), fit the content, or an exact size |
| `height fill` / `height shrink` / `height N` | Take the remaining space in a column, fit the content, or an exact size |
| `center-x`, `center-y` | Center the element in its parent |
| `align-left`, `align-right`, `align-top`, `align-bottom` | Align the element in its parent |
| `wrap` | Let a row wrap onto more lines |
| `grid-cols N`, `grid-rows N` | Equal grid columns or rows (on `@grid`) |
| `col-span N`, `row-span N` | Cells a grid child spans |

```
@row [spacing 8]
  @el [width fill, padding 12, background #f3f4f6] Fills the row
  @el [width 120, padding 12, background #e5e7eb] 120px
@el [height 200, spacing 8, background #f9fafb]
  @text [center-x] Centered
  @el [height fill, background #dbeafe] Fills the rest of the column
  @text [align-right] Right-aligned
@grid [grid-cols 3, spacing 8, md:grid-cols 4]
  @el [col-span 2, background #fef3c7] Two columns wide
  @el [background #fde68a] One
  @el [background #fcd34d] One
```

Everything else about layout is plain CSS: `justify-content`, `align-items`,
`flex 1 1 240px`, `grid-template-areas`, `position`.

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

Values mean what they mean in CSS, with one addition: **a bare number in a
length is pixels**. So `padding 20` is `20px`, `border 1 solid red` is
`1px solid red`, and `margin 0 auto` is `0 auto`. Values that have a unit,
keywords and CSS functions pass through as written (`width 50%`,
`max-width min(100%, 800px)`), and so do numbers that aren't lengths
(`opacity 0.5`, `z-index 2`, `line-height 1.5`, `flex 1`).

`line-clamp N` also adds the `-webkit-box` declarations that browsers still
need to cut text off after N lines.

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

`if(CONDITION, A, B)` chooses `A` or `B` depending on the condition. It works
as a value or as a whole attribute. If the chosen side is empty, the attribute
is left out, and `B` itself can be omitted.

```
@let active true
@el [background if($active, blue, gray), if($active, font-weight bold), padding if($active, 12)]
  Conditionally styled
```

## HTML

### Elements

The layout and text elements have elm-ui's names. Every other element has
its HTML name.

| Element | Output | Purpose |
|---|---|---|
| `@el` | div, flex column | The container: children laid out top to bottom |
| `@row` | div, flex row | Children laid out side by side |
| `@grid` | div, grid | Grid container (with `grid-cols` or `grid-template-*`) |
| `@in-front` / `@behind` | div, absolute | Overlay layers that fill the parent |
| `@text` | span | Styled inline text |
| `@paragraph` | p | Flowing text with inline elements |
| `@link URL` | a | Link, whose content is the text after the URL |
| `@image SRC` | img | Image |
| `@fragment` | (none) | Its children, without a wrapper element |

The HTML elements, by kind:

- **Semantic containers**, all laid out as columns: `@nav`, `@header`,
  `@footer`, `@main`, `@section`, `@article`, `@aside`, `@address`,
  `@search`, `@form`, `@details` / `@summary`, `@dialog`, `@figure` /
  `@figcaption`, `@blockquote` / `@cite`, `@fieldset` / `@legend`,
  `@noscript`.
- **Text**: `@h1` … `@h6`, `@code`, `@pre`, `@mark`, `@kbd`, `@abbr`,
  `@time`.
- **Lists and tables**: `@ul` / `@ol` / `@li`, `@dl` / `@dt` / `@dd`,
  `@table` / `@thead` / `@tbody` / `@tr` / `@th` / `@td`.
- **Forms**: `@input`, `@button`, `@select` / `@option`, `@textarea`,
  `@label`, `@datalist`, `@progress`, `@meter`, `@output`. For `@form URL`,
  the URL is the form's `action`.
- **Media and embeds**: `@picture` / `@source`, `@video SRC`, `@audio SRC`,
  `@iframe SRC`, `@canvas`, `@script`, `@hr`.

Browser default margins on headings, paragraphs, lists and figures are reset
to 0, so `spacing` controls the gaps. List items are columns like other
containers, so a list shows no markers. To bring them back, write
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

### HTML attributes

HTML attributes are written `key=value`: `id=main`, `class=note`,
`href=/about`, `type=email`, `alt=Logo`, `target=_blank`,
`aria-label=Close menu`, `data-id=42`. Any name works. Boolean attributes are
written bare: `required`, `disabled`, `checked`, `open`, `popover`. A style
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
-- A string, with $variables interpolated
@let greeting "Hello from $primary"
-- An attribute bundle, used as [$card]
@let card [padding 20, background white, border-radius 8]
-- A function: a @let with an indented body
@let panel $title
  @el [$card, spacing $gap]
    @text [font-weight bold, color $primary] $title
    @children

@panel [title Welcome]
  $greeting
@el [$card, background #f9fafb]
  Attributes after a bundle override it.
```

A definition applies from its own line onward.

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
is empty, so optional fields of `@data` records work: `@if $post.draft` is
false, and `${default($post.tag, none)}` gives `none`.

### Functions

A function is called like an element:

- Its parameters are passed as attributes. A parameter with a default
  (`$tone=info`) can be left out.
- Any other attributes style the function's root element, so
  `@panel [title Hi, padding 40]` works the same as styling a built-in
  element.
- Text after the attributes and the indented children replace `@children`.
  A caller's `@slot NAME` block replaces the function's `@slot NAME`, and
  the slot's own children are the default content.

```
@let card $title $tone=#f9fafb
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

An `@style` block at the top of a function body is **scoped** to the
function. Its rules apply inside the function's root element, and `&` is the
root itself. This requires the body to have a single root element.

```
@let note $kind=Note
  @style
    & { border-left: 4px solid #3b82f6; }
    .title { font-weight: bold; }
  @el [padding 12, border-radius 6, background #eff6ff]
    @text [class=title] $kind
    @children

@note [kind Tip, padding 20] Scoped styles and forwarded attributes.
```

A function whose body is text holds multi-line content:

```
@let intro
  htmlang is a layout language.
  It compiles to {@text [font-weight bold] static HTML}.

@paragraph
  @intro
```

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
| Values | numbers, `"strings"` (with `$var` interpolation), `$variables`, `$record.field`, `true`, `false`, and bare words (`dark`, `#fff`) as strings |
| Arithmetic | `+ - * / %` with the usual precedence, unary `-`, `( )` |
| Comparison | `== != < > <= >=` (numeric when both sides are numbers) |
| Logic | `and`, `or`, `not`. An empty value, `false` and `0` are false |
| Choice | `if(CONDITION, A, B)` |
| Tests | `contains(s, x)` (in a text, or as an item of a list), `starts-with(s, x)`, `ends-with(s, x)` |
| Text | `uppercase(s)`, `lowercase(s)`, `capitalize(s)`, `trim(s)`, `length(s)` (the items of a list, or the characters of a text), `reverse(s)`, `truncate(s, n)`, `replace(s, old, new)`, `default(s, fallback)` |
| Color | `lighten(c, pct)`, `darken(c, pct)`, `alpha(c, a)`, `mix(c1, c2, pct)` |

Variables are looked up during evaluation, so a value that contains `==` or
spaces is still one value. An invalid expression is a compile error, and so
is an undefined variable.

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

`@each $item in LIST` repeats its body for each item. A list can be a list
loaded with `@data`, a text split on commas, or a range. A range counts down
when its start is greater than its end, and `step` sets the increment. An
optional second variable is the index, starting from 0. `@else` gives the
content to show when the list is empty.

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
| `@include file.hl` | Insert another file: its content and its definitions. A file that holds only `@let`s outputs nothing |
| `@markdown` / `@markdown file.md` | Markdown (an indented body or a file), converted to HTML |
| `@image [inline] file.svg` | Put the file inside the page: SVG as markup (`width`, `height`, `color`, `class=` and `id=` apply to it), other images as base64 |

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
goes with `@slot NAME` (named blocks, with default content) and `@children`
(everything in the call that is outside a `@slot` block).

```
-- layout.hl
@let layout
  @page My Site
  @el [max-width 800, center-x, spacing 24]
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

## Page and head

`@page TITLE` makes the output a full HTML document. Without it, the output
is a fragment. The attributes of `@page` set `lang`, and `favicon` (the file
is embedded in the page). `@meta NAME VALUE` adds a meta tag, and names that
start with `og:` become Open Graph `property` tags. `@head` holds any other
raw HTML for the `<head>`, such as a font link, a canonical URL or JSON-LD.

```
@page [lang en, favicon favicon.png] My Site
@meta description A small site
@meta og:title My Site
@head
  <link rel="canonical" href="https://example.com/">
```

Translations are a JSON file per locale: `@data $t locales/$lang.json`
loads `locales/fr.json` when `$lang` is `fr`.

## CSS

`@style` holds raw CSS, including at-rules such as `@keyframes`,
`@font-face`, `@property` and `@starting-style`:

```
@style
  @keyframes fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  .note { color: gray; }
@el [animation fade-in 0.3s ease, class=note] Fades in
```

Each element gets a short generated class for its styles, and elements with
the same styles share one. The generated rules live in `@layer htmlang`. A
small reset (`box-sizing`, body margin, block images, unstyled links, a
focus outline) lives in `@layer hl-reset` before it. CSS outside any layer
takes precedence over both, so rules in `@style` or `@raw` override the
generated ones.
