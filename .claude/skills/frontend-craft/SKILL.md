---
name: frontend-craft
description: >
  Build or review a Komun interface so it reads as designed rather than generated. Use whenever a
  change touches anything under web/ — a page, a component, a style block or a token.
---

# Skill: Frontend Craft

## Why does an interface read as generated?

Because every surface was written in isolation, each one inventing a value that a neighbour had
already chosen. The result is not ugly, it is inconsistent, and inconsistency is what a reader
notices first. Measured on this repository before the standard existed: two display faces in
conflict, fourteen distinct radius declarations across 33 components, and thirteen separate patches
of `font-family: inherit` (`rg -c 'font-family: inherit' web/src --glob '*.svelte'` -> `13`).

## What must you read before touching a surface?

Read these three, in this order, before writing a line of markup or style:

- `docs/UI-STYLE.md` — the rules and, for each one, how a violation is detected.
- `web/src/lib/design/tokens.css` — the spacing, type, radius, shadow and transition vocabulary.
- `web/src/app.css` — the base element styles and the colour tokens. It also defines `--radius` as an
  alias for `--radius-md` (`web/src/app.css:12`), which is why that name appears where a literal
  would otherwise.

## How do you build a surface?

1. **Find the nearest existing pattern.** A card, a list row, a form field and an empty state almost
   certainly already exist. Copy the pattern, not the pixels. If nothing close exists, say so in the
   handoff rather than inventing a third variant.
2. **Take every value from a token.** Colour, radius, spacing, type size, shadow and duration all have
   names. A literal beside a token is the defect this standard exists to catch.
3. **Design the unhappy states first.** Empty, loading, error and disabled are part of the surface, not
   an afterthought. A surface that fetches and has no empty branch is unfinished.
4. **Give every interactive element four states.** Hover, focus-visible, disabled and busy. Focus must
   be visible without a mouse, because it is the only navigation some users have.
5. **Run the detection column.** Before claiming compliance, run each detector in `docs/UI-STYLE.md`
   that applies to the files you touched. A claim of compliance without a detector output is an
   assertion, and this repository does not accept those.

## What must you never do?

- Declare a colour, radius or spacing literal in a component instead of using its token.
- Add a `font-family` in a component. Inheritance is already handled once, in the base layer.
- Use an emoji where an icon belongs.
- Ship a shape or gradient used in exactly one place. One use is not a style, it is an accident.
- Leave an interactive element with no visible focus state.
- Style a surface and call it done while its empty and error paths render nothing.

## How does this interact with the repository's other standards?

`docs/DOC-STYLE.md` governs prose and `docs/UI-STYLE.md` governs the interface; both apply to a
frontend change, and the conformance gate covers the prose half. Where this skill and a rule in
`UI-STYLE.md` disagree, the rule wins and this skill is corrected to match it — the same order of
authority the routing map holds over the agent definitions.

## Activation Scope

Permitted for `implementer` and `reviewer`, because those are the roles that write and judge
interface code.

Permitted for `planner` when the change's file list includes `web/`, so the plan can account for the
states and tokens the work will need.

Denied for `orchestrator`, `tester`, `project-manager` and `researcher`, none of which edit or judge
a surface.
