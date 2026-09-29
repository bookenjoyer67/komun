---
classification: internal
project: proj-komun
doc_type: reference
---

# Reference: the three migration files

What does each file in `migrations/` change, and which of them is frozen?

`migrations/` holds the frozen baseline plus two additive migrations (`docs/DATABASE.md:94` `Schema changes are additive files: `002_*.sql`, `003_*.sql`, …`). The frozen file is `001_schema.sql`, checksum-bookmarked in every provisioned database (`docs/DEVELOPMENT.md:115` `It is checksum-bookmarked in every existing`).

## What does `002_directory_open_registration` add?

It adds one boolean column advertising whether a directory entry's server accepts public peer registration (`migrations/002_directory_open_registration.sql:7` `ADD COLUMN open_registration BOOLEAN NOT NULL DEFAULT true;`). The flag is deliberately separate from user signup (`migrations/002_directory_open_registration.sql:4` `open_registration` is deliberately separate from). Its default is `true`, so existing rows advertise open registration until an operator changes them.

## What does `003_drop_matches_message` remove?

It drops the leftover plaintext column from the pre-ciphertext design (`migrations/003_drop_matches_message.sql:22` `ALTER TABLE matches DROP COLUMN message;`). Nothing had written that column since the reshape, and every read enumerates its columns (`migrations/003_drop_matches_message.sql:7` `no query selects it`). A plaintext message column that no code writes is a standing invitation for a future write (`migrations/003_drop_matches_message.sql:11` `So the column goes.`).

## What must an operator check before applying `003` to a long-lived database?

Run the file's own pre-check first (`migrations/003_drop_matches_message.sql:18` `SELECT count(*) FROM matches WHERE message IS NOT NULL;`). Any non-zero result is pre-reshape data and should be dumped before the migration runs (`migrations/003_drop_matches_message.sql:19` `any non-zero result is pre-reshape data and`).
