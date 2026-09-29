---
classification: internal
project: proj-komun
doc_type: standard
---

# Standard: how a documentation claim is settled

What makes a sentence a claim, and what must sit inside its parenthetical?

A claim asserts something about the repository, and every claim needs authority (`docs/DOC-STYLE.md:15` `needs authority (R2). A sentence that only describes the document itself is not a claim.`). A bare location is not enough: naming an artifact without quoting it fails R2 (`docs/DOC-STYLE.md:40` `A bare location is a v1 form and fails R2 in v2`). The acceptable authority forms are three, and each names a literal (`docs/DOC-STYLE.md:36` `a location and the text at it`).

## Which rules govern the prose?

- Open every section with the question it answers (`docs/DOC-STYLE.md:26` `A section opens with the question it answers.`).
- Quote the artifact and the literal text that settles the claim, in the parenthetical (`docs/DOC-STYLE.md:27` `Every claim carries its authority in parentheses, naming the artifact **and** the literal text, value or count that settles it.`).
- Keep a sentence to at most 35 words (`docs/DOC-STYLE.md:28` `A sentence is at most 35 words.`).
- Write rules as imperatives and never hedge (`docs/DOC-STYLE.md:29` `A rule is an imperative, and never hedges`).

## What happens to a claim nobody can trace?

Do not delete it and do not invent authority for it; mark it and list it (`docs/DOC-STYLE.md:72` `Do not delete the claim, and do not invent authority for it.`). The mark is `[UNVERIFIED]`, and the entry belongs in that section's `Claims needing verification` list (`docs/DOC-STYLE.md:73` `and add it to a `Claims needing verification` list at the end of that section`). That list names the artifact that would settle the claim. A revision is a new version with a dated history entry, never an in-place edit (`docs/DOC-STYLE.md:5` `version number plus a dated entry in the history at the bottom — never an in-place edit`).
