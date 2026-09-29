---
classification: internal
project: proj-komun
doc_type: reference
---

# Reference: the category taxonomy

How many categories exist, what does a scope filter select, and how is one retired?

The taxonomy is seeded data rather than an enum, and it is editable at runtime (`AGENTS.md:184` `taxonomy is the seeded, runtime-editable `categories` table (23 rows), not an enum`). `001_schema.sql` seeds 23 rows, split 15 market-only, 6 both and 2 aid-only (`docs/DATABASE.md:29` `Seeded with 23 rows (15 market-only, 6 both, 2 aid-only) by `001_schema.sql``). The optional demo seed does not touch it (`docs/DEVELOPMENT.md:101` `it does **not** touch the taxonomy, which `001_schema.sql` already seeds`).

## How does a scope filter behave?

A scope filter is a union, not an equality, so `market` means the market rows plus the shared rows (`docs/DEVELOPMENT.md:223` `The taxonomy is a UNION, not an equality: `market` is the 15 market rows + the 6 `both` rows.`). The measured answers are 21 rows for `market`, 8 for `aid` and 6 for `both` (`docs/DEVELOPMENT.md:224` `# 21 rows`). An unknown scope is refused rather than answered with an empty list (`docs/DEVELOPMENT.md:227` `# 400, names aid, market, both`).

## How is a category retired or renamed?

The slug is an immutable identifier, because posts reference it. An admin therefore retires a row instead of deleting it (`docs/CONVENTIONS.md:176` `so an admin retires a row with `active = false``). A label rename re-runs the full-text update for that category's posts (`docs/CONVENTIONS.md:177` `a label rename re-runs the FTS update for the category's posts.`). The database refuses to delete a category in use (`docs/DATABASE.md:29` `a category in use cannot be deleted`).
