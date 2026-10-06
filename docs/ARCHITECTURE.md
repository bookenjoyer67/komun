# Architecture

## What Komun is

A single-server marketplace and local listings hub. The server **is** the community: there is no
multi-tenant `communities` table, no federation, and no relay. Users sign up with email and
password; post needs/offers/resources and marketplace listings/wants; negotiate over
end-to-end encrypted match threads; and, once a deal is completed, leave a star review. The
only outbound integration is an optional public directory listing and the OSM/Nominatim map.

## System overview

```
┌──────────────────────────────────────────────────────────┐
│ Browser (SvelteKit 5 SPA, ssr=false)                     │
│  web/src/  ── UI, routes, stores                         │
│  komun-wasm ── x25519 ECDH + XChaCha20Poly1305 + Argon2  │
└───────────────┬──────────────────────────────────────────┘
                │ HTTPS (JSON; ciphertext for messages)
                ▼
┌──────────────────────────────────────────────────────────┐
│ Axum server (crates/server, :3000)                       │
│  api/   REST handlers (flat /api/posts, /api/auth, …)    │
│  auth/  signup, signin, sessions, password, email        │
│  db/    sqlx runtime queries                             │
│  tasks/ expiry, health, directory registration           │
│  repl.rs, security_headers.rs, rate_limit.rs, sessions.rs│
└───────────────┬──────────────────────────────────────────┘
                │
                ▼
        ┌───────────────┐        ┌────────────────────────┐
        │ PostgreSQL 16 │        │ optional: public       │
        │ (one node)    │        │ directory + OSM tiles  │
        └───────────────┘        └────────────────────────┘
```

## Crates

| Crate | Role |
|---|---|
| `crates/core` | Shared models and the `db_enum!` macro (plain data + serde; no DB or HTTP) |
| `crates/server` | Axum HTTP API, auth/sessions, sqlx queries, background tasks, REPL |
| `crates/wasm` | Client-side crypto compiled to WASM (x25519, XChaCha20Poly1305, Argon2, recovery codes) |

`komun-relay` and the `federation/` module were deleted in the reshape; the federation
config section and the alliances API are gone.

## Server module tree (`crates/server/src/`)

```
main.rs              bootstrap: config, pool, migrations, router, tasks, REPL
config.rs            TOML config, env overrides, startup validation
sessions.rs          opaque session tokens (random raw, SHA-256 stored)
rate_limit.rs        per-IP token buckets for auth routes
security_headers.rs  response security headers
repl.rs              interactive admin CLI when stdin is a terminal

api/
  mod.rs             router composition
  health.rs          GET  /api/health
  node.rs            GET  /api/node         (server identity + discovery flags)
  posts.rs           GET/POST /api/posts, GET/PATCH/DELETE /api/posts/{id}, image upload
  conversations.rs   responses, /api/me/conversations, messages, offers, status
  search.rs          GET  /api/search, /api/search/users
  users.rs           GET  /api/users/{id}
  endorsements.rs    GET/POST/DELETE /api/users/{id}/endorse(ments)
  notifications.rs   /api/me/notifications*
  admin.rs           /api/admin/* (superadmin/admin)
  categories.rs      GET /api/categories, POST/PATCH /api/admin/categories (admin+)
  reviews.rs         POST /api/matches/{id}/reviews, GET /api/users/{id}/reviews
  reports.rs         report/hide a post; /api/admin/reports*
  directory.rs       optional /api/directory* (mounted only when enabled)
  geocode.rs         hardened Nominatim proxy (rate-limited, cached)
  link_preview.rs    GET  /api/link-preview
  error.rs           shared StatusError -> JSON error mapping
  geocode/           mod.rs + limiter.rs + cache.rs

auth/
  mod.rs             routes + middleware (require_auth/require_admin/require_superadmin)
  password.rs        Argon2id verifier hashing + policy
  email.rs           SMTP (lettre) for verify / reset mail

db/                  categories, conversations, endorsements, notifications, posts, reports, reviews, sessions, users
tasks/               expiry, health, directory registration, bundle cleanup
```

## Frontend route tree (`web/src/routes/`)

```
+page.svelte                 home / feed
+layout.svelte, +layout.ts   shell; ssr=false, prerender=false
account/{login,signup,verify,forgot,reset}/   account lifecycle
aid/+page.svelte             aid post list
aid/new/+page.svelte         create a post (incl. click-to-place coordinates)
market/+page.svelte          browse listings and wanted ads (category/price/condition filters)
market/new/+page.svelte      create a listing or a want
p/[id]/+page.svelte          flat post permalink (listing detail + respond)
map/+page.svelte             OSM map of located posts
search/+page.svelte          post/user search
messages/+page.svelte        conversation list
messages/[id]/+page.svelte   conversation thread + offers + review
notifications/+page.svelte
users/[id]/+page.svelte      profile + endorsements + reviews
admin/+page.svelte, admin/users/+page.svelte
```

The old `c/**`, `community/**` and `federation/**` route trees were deleted with the
community model.

## Request lifecycle

1. The SPA calls the API through `web/src/lib/api/**` (the single client hub).
2. Axum matches the route in `api/mod.rs`, applies CORS and the security-headers/trace layers.
3. Protected routes run `require_auth`, which loads the session row (user id **and** role, so a
   demotion takes effect immediately) from `sessions`.
4. Handlers validate input and call `db/*`, which issues parameterized sqlx **runtime**
   queries (`sqlx::query` / `query_as`, not the compile-time macros — so no `sqlx prepare`).
5. The handler returns JSON; errors go through `api/error.rs`.

## Data and trust boundaries

- **Server-visible:** email, password verifier, wrapped key bundles, session hashes, directory
  entries. **Never server-visible:** the plaintext password, the password-derived key, the x25519 secret
  in the clear, and message plaintext (the `messages` table stores ciphertext only).
- **Config** (`config.rs`, `config.example.toml`) covers `[server]`, `[database]`, `[node]`,
  `[discovery]`, `[auth]`, `[security]`, `[posts]`, `[admin]`, `[media]`, `[email]`,
  `[registration]`, `[geocode]`, `[market]`. Environment variables override specific fields.
  There is no `jwt_secret` and no `[relay]`/`[federation]`.
- **Map**: `LocationMap.svelte` (Leaflet) is read-only on `/map` and opt-in *pickable* on
  `aid/new`; the tile URL is a component default (operator-configurable later). No relay, no
  map-community credentials.

## Marketplace

The marketplace is a facet on the existing flat `posts` table, not a second system: a post is an
`listing` or a `want`, the negotiation reuses the encrypted match thread, and a review anchors to
the match that was completed. Money changes hands in person — there are no payment rails.

### Kinds and price fields

`PostKind` has five values. Three are aid — `resource`, `need`, `offer` — and two are marketplace —
`listing` (something offered for sale) and `want` (a request to buy). `PostKind::is_market()` is the
single source of that split.

The market columns live on `posts`. `chk_posts_market_fields` keeps `market_listed`, `price_cents`
and `item_condition` off every non-market kind, and `chk_posts_currency` pins the code to exactly
three uppercase letters:

| Column | Meaning |
|---|---|
| `market_listed` | the browse flag a marketplace view keys on |
| `price_cents` | whole cents (`BIGINT`); `0` is "free", negative is rejected |
| `currency` | ISO-4217 alphabetic code: exactly three uppercase letters |
| `price_negotiable` | whether the seller will take an offer |
| `item_condition` | `new`, `like_new`, `good`, `fair`, `poor`, `for_parts` |

A non-market kind carrying any of those three is a `400` ("price, condition and market_listed
belong to 'listing' and 'want' posts only"), which turns what would be a constraint violation (a
500) into a usable error.

**Currency precedence.** On create, a market post keeps its own `currency`; if it has none and
`[market] default_currency` is set, the server fills that in; otherwise the post keeps no currency
at all (allowed — a price you negotiate in person may not need one yet). At negotiation time the
rule is stricter, because a deal needs a unit: the offer's own `currency`, else the post's, else
`[market] default_currency`, and with none of the three the offer is refused with a `400` naming
the missing currency. An offer MAY name a currency different from the post's and that currency then
governs the deal. An `accept` must restate `amount_cents`.

### The deal lifecycle

A match thread (`matches`) is `proposed` when a response opens it. The negotiation is a trail of
`match_offers` rows; the status moves only through this matrix (`check_transition`, shared by the
handler and the locked transaction):

```
proposed  → proposed   (no-op)
proposed  → accepted   accept an offer
proposed  → withdrawn  decline, or withdraw
accepted  → completed  finish the deal
accepted  → withdrawn  back out after agreeing
```

Every other move is a `409` that names the status the thread is actually in. Going back from
`accepted` to `proposed` is illegal on purpose: `agreed_price_cents` would survive on a thread that
no longer has an agreement.

What each write actually does:

- **accept** (an `accept` offer) is atomic — the `match_offers` accept row, `matches.agreed_price_cents`,
  `matches.currency` and `status = 'accepted'` commit together under a row lock. Only the
  counterparty of the last `offer`/`counter` may accept; accepting your own number is a `409`
  (`SELF_ACCEPT`), and an accept with no offer to accept is a `409` (`NOTHING_TO_ACCEPT`).
- **decline** records the `decline` row first, then sets `status = 'withdrawn'` and `resolved_at`.
- **complete** is `PATCH /api/conversations/{id}/status` with `{"status":"completed"}` from
  `accepted`. It sets `matches.status = 'completed'` and `matches.resolved_at`, and in the same
  transaction marks the post `status = 'fulfilled'`; on a `listing`/`want` it also writes
  `posts.sold_at = now()` and `posts.buyer_id` = the responder. A listing that already has a
  `sold_at` refuses a second completion with a `409` ("this listing is already sold") — one listing,
  one sale — and the `FOR UPDATE OF p` lock makes that check race-safe.
- **withdraw** from either live state sets `resolved_at`; the two terminal states (`completed`,
  `withdrawn`) take no further offers and no further status change.

### The append-only offer trail

`match_offers` is the negotiation, and it is append-only: `id`, `match_id`, `actor_id`,
`kind` (`offer`/`counter`/`accept`/`decline`), `amount_cents`, `currency`, `note`, `created_at`.
Nothing in the codebase updates or deletes a row (a unit test reads `db/conversations.rs` and
asserts it), and the list comes back oldest-first with `created_at ASC, id ASC` so two rows written
in the same microsecond still have a stable order. A `note` is a plaintext, server-readable
sentence attached to a number (a counter to the end-to-end-encrypted messages on the same thread),
bounded to 500 characters; a `decline` carries no amount.

### Reviews and the profile aggregate

`deal_reviews` is the trust record: `id`, `match_id`, `reviewer_id`, `reviewee_id`, `rating`
(`1..5`), `body`, `created_at`, with `UNIQUE (match_id, reviewer_id)`. The rules:

- **only a `completed` deal is reviewable** — a proposal or a withdrawn thread is a `409` naming
  the status. This is the trust property: a review says "we actually dealt", not "we talked".
- **only a participant may review**, and the `reviewee_id` is derived from the thread
  (`other_participant`), never supplied by the client.
- **one review per reviewer per deal**, enforced by the unique constraint and mapped to a `409`
  ("you have already reviewed this deal"), never a 500.
- reviews are **immutable** — no update or delete path exists.
- `rating` is a whole number `1`–`5`; `body` is optional and ≤ 2000 characters.

Reviews are attributed: `GET /api/users/{id}/reviews` carries the reviewer's display name. The
profile (`GET /api/users/{id}`) returns `rating_avg` — the mean to one decimal, `null` when there
are none — and `rating_count`, computed from `deal_reviews` on every read. There is no
denormalised counter column: a counter is a second answer to the same question whose only ability
is to disagree with the rows.

### Categories are data, not code

`categories(slug PK, label, scope, sort_order, active)` is a runtime-editable table, seeded by
`migrations/001_schema.sql` with **23 rows**: **15** `scope = market`, **6** `scope = both`, **2**
`scope = aid`. `GET /api/categories?scope=` is a **union**, not an equality — asking for `market`
returns the 15 market-only rows **and** the 6 `both` rows (**21**), and `aid` returns 2 + 6 (**8**);
`scope=both` narrows to the 6 shared rows, and omitting `scope` returns all 23. That is what lets
one flat list serve both UIs: a `both` row is usable from either form, a `market` row only from the
marketplace form, an `aid` row only from the aid form.

`posts.category` is a foreign key onto `slug` with `ON DELETE RESTRICT`, so a category in use
cannot be deleted; an admin retires one with `active = false` (inactive rows are hidden from the
public list and shown only to `GET /api/admin/categories`). The full-text-search trigger indexes the
category **label**, so renaming a category re-runs the search vector for every post in it. Admins
edit the list at runtime through `POST`/`PATCH /api/admin/categories` (admin or superadmin),
audited as `admin.category_create` / `admin.category_update` with the slug in `audit_events.detail`
(the `subject_id` column is a UUID and a category is keyed by a text slug).

## HTTP API

Everything is mounted under `/api`. Auth is `Authorization: Bearer <session token>`, where the
token is the opaque value returned by `POST /api/auth/signin`. `require_auth` demands a session and
refuses every mutating method from an account whose email is not verified when
`[registration] require_email_verification = true`, answering `403` with
`{"code":"email_unverified"}`. Admin routes need `users.role` `admin` or `superadmin`; superadmin
routes need `superadmin`.

`GET /api/posts` and `GET /api/users/{id}/reviews` are **paginated**: `limit` defaults to **100**
and caps at **200**, and `offset` is supported. Both reject an out-of-range value with a `400`
rather than clamping it.

### Marketplace endpoints

**Categories**

| Method & path | Auth | Success | Errors |
|---|---|---|---|
| `GET /api/categories?scope=aid\|market\|both` | public | `200` active rows `{slug,label,scope}` (union rule above) | `400` unknown `scope` |
| `GET /api/admin/categories?scope=` | admin | `200` full rows incl. inactive | `400`, `401`, `403` |
| `POST /api/admin/categories` | admin | `201` `{slug,label,scope,sort_order?,…}` | `400` bad slug/label, `409` slug exists, `401`, `403` |
| `PATCH /api/admin/categories/{slug}` | admin | `200` updated row (label rename re-indexes posts) | `400` empty body/bad label, `404`, `401`, `403` |

**Market filters on the post list**

| Method & path | Auth | Notes |
|---|---|---|
| `GET /api/posts` | public | filters `kind` (`resource\|need\|offer\|listing\|want`), `category`, `status`, `q`, `min_price_cents`, `max_price_cents`, `currency`, `item_condition` (`new\|like_new\|good\|fair\|poor\|for_parts`), `limit`, `offset`. `200` `Post[]` with `category_label`; `400` for any unparseable parameter (unknown enum value, non-integer or negative price, `min > max`, bad currency, bad slug, bad limit/offset). A price filter drops posts with `NULL` price, so `?min_price_cents=` is implicitly a market query. |
| `POST /api/posts` | session + verified | market kinds accept `market_listed`, `price_cents`, `currency`, `price_negotiable`, `item_condition` (a market post with no currency takes `[market] default_currency`, else keeps none); setting `market_listed`, `price_cents` or `item_condition` on an aid kind is a `400`. `200` created `Post`; `403` unverified; `429` rate limit. |

**Negotiation** (all participant-only; a missing thread is `404`, a thread that is not yours is
`403`)

| Method & path | Success | Errors |
|---|---|---|
| `POST /api/posts/{post_id}/respond` | `200` `{match_id,status:"proposed"}`, sealed opening message stored | `400` bad/empty base64 ciphertext, `403` unverified, `429` |
| `POST /api/conversations/{id}/offers` | `201` the new `match_offers` row | `400` bad `kind`/`amount_cents`/`currency`/`note`, an offer on an aid thread, or an amount with no currency anywhere; `403`; `404`; `409` thread closed / accept on a non-`proposed` thread / self-accept / nothing to accept |
| `GET /api/conversations/{id}/offers` | `200` the trail, oldest first | `403`, `404` |
| `PATCH /api/conversations/{id}/status` | `200` `{status}`; `{"status":"completed"}` from `accepted` also fulfils/sells the post | `400` unknown status; `403`; `404`; `409` illegal transition or "this listing is already sold" |

The `POST .../offers` body is `{"kind":"offer"\|"counter"\|"accept"\|"decline","amount_cents":…,
"currency":…,"note":…}`. `amount_cents` is required for `offer`, `counter` and `accept`, must be a
non-negative whole number of cents, and is forbidden on `decline`. `note` is ≤ 500 characters.

**Reviews**

| Method & path | Auth | Success | Errors |
|---|---|---|---|
| `POST /api/matches/{id}/reviews` | session + participant | `201` the review; body `{"rating":1..5,"body"?}` | `400` rating missing/not a whole number/out of range, body > 2000 chars; `403` non-participant or unverified; `404`; `409` deal not `completed`, or "you have already reviewed this deal" |
| `GET /api/users/{id}/reviews?limit=&offset=` | public | `200` attributed rows newest-first | `400` bad page |
| `GET /api/users/{id}` | public | `200` profile incl. `rating_avg` (1 decimal or `null`) and `rating_count` | `404` |

**Plainly: a review is only possible against a completed deal.** The server refuses a review on a
`proposed`, `accepted` or `withdrawn` thread with a `409`, and the reviewee is always the other
participant of that deal — never a field the client supplies. That is the trust property: a rating
attests to a deal that actually finished, and it cannot be pointed at a stranger.

See `docs/DATABASE.md`, `docs/CRYPTO.md`, `docs/CONVENTIONS.md`, `docs/DEVELOPMENT.md` and
`docs/DEPLOY.md`.
