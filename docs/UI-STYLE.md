# UI standard

What makes a Komun surface look deliberate rather than generated, and how is each rule detected?

This is the visual counterpart to `docs/DOC-STYLE.md`. Where that governs prose, this governs the
interface. The same discipline applies: a rule earns its place only if a reviewer can detect a
violation without exercising taste.

## Why does a token file exist if nobody uses it?

`web/src/lib/design/tokens.css` declares a spacing scale, a type scale, radii, shadows and
transitions (`web/src/lib/design/tokens.css:2` `--space-1: 0.25rem;`). `web/src/app.css:12` aliases
`--radius: var(--radius-md);`. The vocabulary is present. What is missing is a rule that components
must not invent their own values beside it.

## Rules (v1)

Which rules govern the interface from 2026-10-06 onward, and how is each one detected?

| Rule | Requirement | How a violation is detected |
|---|---|---|
| U1 | A colour comes from a token. No component declares a literal hex, rgb or hsl value. | `rg -n '#[0-9a-fA-F]{3,8}\b' web/src --glob '*.svelte'` returns a line, or `rg -c 'rgba?\(' web/src --glob '*.svelte'` increases |
| U2 | A radius comes from a token. No component declares a literal length radius. | `rg -n 'border-radius:\s*[0-9]' web/src --glob '*.svelte'` returns a line |
| U3 | Spacing comes from the scale. No component declares a px or rem margin, padding or gap absent from `tokens.css`. | the value does not appear in `rg -o '\-\-space-[0-9]+' web/src/lib/design/tokens.css` |
| U4 | One display face and one body face, both declared once as tokens. | `rg -n "font-family" web/src --glob '*.svelte' --glob '*.css'` returns anything other than `var(--font-*)` |
| U5 | Form controls inherit the body face from one base rule, never per component. | `rg -c 'font-family: inherit' web/src --glob '*.svelte'` is greater than one |
| U6 | An icon is an inline SVG or a glyph in a font, never an emoji standing in for an icon. | `rg -n '[\x{1F300}-\x{1FAFF}\x{2600}-\x{27BF}]' web/src --glob '*.svelte'` returns a line outside user content |
| U7 | Every interactive element defines hover, focus-visible, disabled and busy states. | the component has a click handler and no `:focus-visible` rule |
| U8 | One `h1` per page, and heading levels descend without skipping. | `rg -o '<h[1-6]' <page> \| sort \| uniq -c` shows two `h1`, or a level rises by more than one |
| U9 | Motion respects `prefers-reduced-motion`. | a `transition` or `animation` exists with no matching media query in `app.css` |
| U10 | A shape or effect used once is a bug. Gradients, skews and asymmetric radii come from the token set or do not ship. | `rg -n 'border-radius:\s*[0-9]+px [0-9]+px' web/src` returns a line, or a gradient appears in one component only |
| U11 | Text meets WCAG AA contrast against its own background. | the computed contrast ratio is under 4.5 for body text or 3.0 for large text |
| U12 | Empty, loading and error states are designed, not omitted. | a surface that fetches has no branch for the empty case |

## What the rule set exists to prevent

Which habits made the interface read as generated?

- A font chosen per page instead of a scale chosen once. Measured before this standard: two display
  faces in conflict (`web/src/routes/+layout.svelte:166` `font-family: 'Space Grotesk', sans-serif;`
  against `web/src/app.css:26` `font-family: var(--font, 'Atkinson Hyperlegible', sans-serif);`) and
  thirteen components patching with `font-family: inherit;`.
- A value invented where a token already existed. Measured before this standard: fourteen distinct
  radius declarations across 33 components (`rg -o 'border-radius:[^;]*' web/src` -> `14`), of which
  the token set defines five.
- An emoji used as an interface element. The document title carried one until 2026-10-06.
- A surface with no designed empty or error state, because the happy path was written first and the
  unhappy path was never reached.

## How is a change checked against this standard?

Check the detection column before claiming a surface complies. A rule whose detector cannot run is
not a rule yet — say so rather than asserting compliance.

## Authority forms

What has to be inside a citation in this document?

- a location and the text at it — (`web/src/app.css:12` `--radius: var(--radius-md);`)
- a command and its output — (`rg -o 'border-radius:[^;]*' web/src` -> `14`)
- a count and the search that produced it — (`rg -c 'font-family: inherit' web/src --glob '*.svelte'` -> `13`)

## Version history

- v1, 2026-10-06. First rule set, written after measuring the interface rather than asserting a taste.
