# Deployment (self-hosting)

How does an operator run Komun on a machine of their own?

Generic guidance for running Komun on your own machine. These are instructions, not a
script — nothing here is performed automatically, and nothing here touches DNS, Cloudflare or
any other edge layer (that is the operator's to manage).

## 1. Build the release binary and frontend

How are the release binary and the frontend built, and in which order?

```bash
# wasm first (the frontend depends on crates/wasm/pkg), then the frontend, then the server
wasm-pack build crates/wasm --target web
cd web && npm ci && npm run build && cd ..
cargo build --release --bin komun-server
```

Artifacts: `target/release/komun-server` and `web/build/`, which the reverse proxy serves: the
router mounts only `/api`, `/avatars` and `/post-images` (`crates/server/src/main.rs:124` `.nest("/api", api::router(state.clone()))`).

## 2. Provision the database

How is the PostgreSQL database prepared before the server boots for the first time?

Follow the exact order in `docs/DEVELOPMENT.md` ("Provisioning a database"): create the
database and role, create `_sqlx_migrations`, load `migrations/001_schema.sql`, insert the
`001` bookmark with its real `sha384`, then boot once so the migrator applies `002+`. Never
edit `001_schema.sql`.

Use a dedicated, least-privilege database user and a strong password. Put the connection
string in the server's `config.toml` (`[database] url`) or the `DATABASE_URL` environment
variable.

## 3. Install and configure

Where do the binary, the data directories and the configuration file go?

```bash
install -d -o komun -g komun /opt/komun /opt/komun/data/avatars /opt/komun/data/post-images
install -m 0755 target/release/komun-server /opt/komun/
install -m 0644 config.example.toml /opt/komun/config.toml
# edit /opt/komun/config.toml: [database] url, [node] name/public_url,
# [registration] (leave require_email_verification = false unless [email] is configured),
# [email] if you want verification/reset mail, [discovery] for directory listing,
# [market] default_currency only if this server should fall back to one for listings.
chown -R komun:komun /opt/komun
```

Directory routes are only mounted when `[discovery] directory_enabled = true`; with it false
`/api/directory*` returns 404 by design (`crates/server/src/api/mod.rs:71` `if state.config.discovery.directory_enabled {`).

## 4. Run it as a service

How does the server run as a supervised service under OpenRC or systemd?

The server reads `config.toml` from its working directory, so the service must run in
`/opt/komun` as the `komun` user.

**OpenRC (Alpine)** — `deploy/komun.initd` is a ready starting point:

```sh
cp deploy/komun.initd /etc/init.d/komun
chmod +x /etc/init.d/komun
rc-update add komun
rc-service komun start
```

**systemd (Debian/Ubuntu)** — equivalent unit:

```ini
[Unit]
Description=Komun marketplace and local listings server
After=network-online.target postgresql.service
Wants=network-online.target

[Service]
User=komun
Group=komun
WorkingDirectory=/opt/komun
ExecStart=/opt/komun/komun-server
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

## 5. Terminate TLS in front

How is TLS put in front of a server that speaks only plain HTTP?

Komun speaks plain HTTP; put a reverse proxy in front for TLS. `deploy/nginx-komun.conf` is a
starting point (proxy `/api/`, `/avatars/`, `/post-images/` to the server; serve the SPA with
an `index.html` fallback). No WebSocket or relay route exists to proxy (`crates/server/src/main.rs:124` `.nest("/api", api::router(state.clone()))`).

```nginx
server {
    listen 443 ssl;
    server_name komun.example.org;

    ssl_certificate     /etc/letsencrypt/live/komun/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/komun/privkey.pem;

    location /api/          { proxy_pass http://127.0.0.1:3000; proxy_set_header Host $host; proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for; }
    location /avatars/      { proxy_pass http://127.0.0.1:3000; proxy_set_header Host $host; }
    location /post-images/  { proxy_pass http://127.0.0.1:3000; proxy_set_header Host $host; }
    location /              { root /opt/komun/frontend; try_files $uri $uri/ /index.html; }
}
```

If the server sits behind a trusted proxy, list that proxy in `[security] trusted_proxies`
(as IP literals) so the rate limiter can believe `X-Forwarded-For`; otherwise the header is
ignored, which is the safe default.

## 6. Operate

What does a running server need from its operator day to day?

- **Health:** `curl -s http://127.0.0.1:3000/api/health` → `{"service":"komun","status":"ok",...}` (`crates/server/src/api/mod.rs:43` `.merge(health::router())`).
- **Media:** avatars and post images live under `[media]` paths inside the working directory —
  include them in backups.
- **Database:** back up PostgreSQL (the schema, plus the tables in `docs/DATABASE.md`).
- **Seed (optional):** `psql "$DATABASE_URL" -f deploy/seed.sql` adds demo accounts and posts;
  the 23 categories come from `001_schema.sql` and are not in the seed file (`crates/core/src/tests.rs:257` `assert_eq!(rows.len(), 23, "expected 23 seeded categories");`).
- **Categories:** the taxonomy is a runtime-editable table (`docs/ARCHITECTURE.md`, "Categories
  are data"). An admin adds, relabels, reorders or retires a category through
  `POST`/`PATCH /api/admin/categories`; retiring means `active = false`, never a delete.
- **Mail (optional):** verification and password-reset mail need `[email] smtp_host` + `from`.
  If you do not run SMTP, keep `[registration] require_email_verification = false`; the server
  otherwise refuses to start.
- **Upgrades:** stop the service, install the new binary and `web/build`, start it — the
  migrator applies any new additive migrations on boot. (`crates/server/src/main.rs:75` `sqlx::migrate!("../../migrations")`).

## 7. Map tiles and the geocode contact

Which settings point the map's tiles and the geocoder's contact at the operator's own choices?

The tile URL and its CSP entry are read from the environment of the frontend build
(`web/svelte.config.js:5` `const mapTiles = resolveMapTiles(process.env);`). Changing either
one takes a rebuild of `web/build/`
(`web/vite.config.ts:10` `__KOMUN_TILE_URL__: JSON.stringify(resolveMapTiles(process.env).tileUrl)`).

- Export `KOMUN_TILE_URL` in the shell that runs `npm run build` to replace the default template
  (`web/map-tiles.config.js:4` `export const OSM_TILE_URL = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png';`).
- Export `KOMUN_TILE_CSP_ORIGINS` in the same shell, as a comma-separated list of the tile hosts'
  origins (`web/map-tiles.config.js:20` `const listed = (env.KOMUN_TILE_CSP_ORIGINS ?? '')`).
- Set the two together, because a URL template is not parsed into an origin
  (`web/map-tiles.config.js:13` `cannot be parsed into the origin the CSP needs. A blank value counts as unset.`).
- Set the geocode contact in `[geocode] contact` or in `KOMUN_GEOCODE_CONTACT`, which wins when
  both are set (`crates/server/src/api/geocode/mod.rs:139` `if let Ok(value) = std::env::var("KOMUN_GEOCODE_CONTACT") {`).

Without a contact, geocode requests carry the generic agent
(`crates/server/src/api/geocode/mod.rs:46` `" (nominatim proxy; local-listings app)"`). The server
warns at startup when no contact is set
(`crates/server/src/main.rs:131` `if api::geocode::contact_is_missing() {`).

## 8. Automatic updates from a release

How does a host update itself without a deploy pipeline reaching into it?

The pipeline publishes, the host pulls. `release` in `.github/workflows/ci.yml` runs only for a push to
the default branch and only when the gating jobs passed, so the host can only ever run a revision the
pipeline verified. It builds the server inside an Alpine image (production's libc; every TLS dependency
here is rustls, so the build needs no OpenSSL), builds the frontend, regenerates the
Content-Security-Policy for that frontend, and attaches `komun-prod.tar.gz` plus `SHA256SUMS` to a
release marked latest. The host asks for `releases/latest/download/komun-prod.tar.gz` on a timer.
Nothing connects to the host, no port is opened for it, and no GitHub runner executes on it — which
matters, because this repository is public and a self-hosted runner would run workflow code from fork
pull requests on the host itself.

What is in the tarball:

| Path | Where it lands | Why it travels with the release |
|:--|:--|:--|
| `komun-server` | `/opt/komun/komun-server` | the Alpine build |
| `frontend/` | `/opt/komun/frontend` | the SPA the proxy serves |
| `csp.conf` | `/opt/komun/csp.conf` | the policy pins the *build's* inline bootstrap hash |
| `VERSION` | compare only | the commit the host records as deployed |
| `BUILD` | read by a human | the run that produced it |

`csp.conf` is not optional. A SvelteKit build carries one inline bootstrap script and the served policy
allows scripts only from `'self'` plus that script's `sha256`; a new frontend under an old policy is a
blank page for every visitor, with nothing in the server log to say why. `scripts/csp-hash.sh` prints
the policy for the file it is given, and the release job runs it over the build it just made.

Install the updater once, as root:

```sh
install -m 0755 deploy/komun-update /usr/local/sbin/komun-update
: >/var/log/komun-update.log
echo '*/10 * * * * /usr/local/sbin/komun-update' >>/etc/crontabs/root
```

cron re-reads a changed crontab, so nothing needs restarting on Alpine's cronie. If a host's daemon
does not, restart the one it runs under (`rc-service crond restart` on OpenRC, `sv restart cron` under
runit). Make sure the appended line ends with a newline: a crontab whose last line has none loses it.

Then run it by hand once (`/usr/local/sbin/komun-update`) and watch `/var/log/komun-update.log` for the
cutover; the first run replaces whatever the host is running with the newest release. On a host whose
proxy reads a different port or path, set `KOMUN_HEALTH_URL`, `KOMUN_APP_DIR` or `KOMUN_SERVICE` in the
cron line rather than editing the script. `KOMUN_UPDATE_BASE` points the updater at another source
(file:// or a local mirror), which is how a host is tested without publishing a release.

It is idempotent — a tick whose `VERSION` matches `/opt/komun/.deployed-sha` does nothing — and it keeps
one previous generation of each file (`.prev`), so a build that fails its health probe rolls back and
restarts the old one on the spot. It never touches `config.toml`, `data/`, the database or
`/etc/nginx`: it only reloads nginx, and only after `nginx -t` accepts the new policy.

Two limits worth knowing. The checksum in `SHA256SUMS` comes from the same origin as the tarball, so it
catches a corrupted download, not a compromised release — what it does guarantee is that the host only
ever runs a revision this pipeline published. And the updater asks for *latest*: if you ever publish
releases for something other than the server, give the updater a tag filter first.

## 9. The server and directory defaults a build resolves

Which server does a first-time visitor talk to, and what may a build set to change that?

A release build sets neither variable, so both defaults resolve to the origin that served the app
(`web/src/lib/stores/server.ts:39` `return servingOrigin();`;
`web/src/lib/stores/directories.ts:10` `return typeof window === 'undefined' ? [] : [window.location.origin];`).

- Set `VITE_DEFAULT_SERVER` to name a different server explicitly (`web/src/lib/stores/server.ts:36`
  `const configured = String(import.meta.env.VITE_DEFAULT_SERVER ?? '').trim();`).
- Set `VITE_DEFAULT_SERVER=none` to send a visitor to Connect before any search
  (`web/src/lib/stores/server.ts:37` `if (configured === 'none') return null;`).
- Set `VITE_DEFAULT_DIRECTORY` only to point discovery at a directory that is not the server's own
  (`web/src/lib/stores/directories.ts:8` `const configured = String(import.meta.env.VITE_DEFAULT_DIRECTORY ?? '').trim();`).
- Bake no hostname into the bundle, because the served policy allows only the app's own origin
  (`web/svelte.config.js:36` `'connect-src': ['self', ...devConnectSources],`).

The pipeline refuses a bundle that carries a hostname of its own
(`.github/workflows/ci.yml:936` `if grep -rl 'komun\.buzz' web/build >/dev/null; then`).

Name the deployment's canonical host in `[node] public_url` instead
(`config.example.toml:21` `# public_url = "https://komun.example.org"`).

## 10. Moderation, appeals and migration 004

Which moderation routes does the server expose, who may call each, and what does the operator apply?

Every route below is mounted under `/api` (`crates/server/src/main.rs:125` `.nest("/api", api::router(state.clone()))`).

| Route | Who may call it | Authority |
|:--|:--|:--|
| `POST /api/posts/{post_id}/hide` | a superadmin | (`crates/server/src/api/reports.rs:18` `.route("/posts/{post_id}/hide", post(hide_post))`; `:21` `require_superadmin,`) |
| `POST /api/posts/{post_id}/appeal` | the post's author | (`crates/server/src/api/reports.rs:42` `.route("/posts/{post_id}/appeal", post(file_appeal))`; `:44` `.layer(middleware::from_fn_with_state(state.clone(), require_auth))`; `crates/server/src/db/reports.rs:299` `if owner != author_id {`) |
| `GET /api/me/moderation` | a signed-in user, about their own hidden posts | (`crates/server/src/api/reports.rs:43` `.route("/me/moderation", get(my_moderation))`; `crates/server/src/db/reports.rs:389` `WHERE p.author_id = $1 AND p.status = 'hidden'`) |
| `GET /api/admin/appeals` | a superadmin | (`crates/server/src/api/reports.rs:50` `.route("/admin/appeals", get(list_appeals))`; `:54` `require_superadmin,`) |
| `PATCH /api/admin/appeals/{appeal_id}` | a superadmin | (`crates/server/src/api/reports.rs:51` `.route("/admin/appeals/{appeal_id}", patch(resolve_appeal))`; `:54` `require_superadmin,`) |

- Send a non-empty `reason` with every hide, because a missing or blank one is refused with 400
  (`crates/server/src/api/reports.rs:123` `let reason = required_text("reason", input.reason.as_deref()).map_err(bad_request)?;`;
  `crates/server/src/api/categories.rs:319` `StatusError::with_status(StatusCode::BAD_REQUEST, message)`).
- Expect every hide to notify the post's author with the reason as the notice body
  (`crates/server/src/db/reports.rs:259` `Some(reason),`).
- Expect an appeal to be accepted for 14 days from the removal
  (`crates/server/src/db/reports.rs:99` `pub const APPEAL_WINDOW_DAYS: i64 = 14;`).
- Expect a second appeal on the same removal to get 409
  (`crates/server/src/db/reports.rs:104` `pub const ALREADY_APPEALED: &str = "this removal has already been appealed";`;
  `crates/server/src/api/reports.rs:227` `StatusError::with_status(axum::http::StatusCode::CONFLICT, message)`).
- Expect anyone but the post's author to get 404 from the appeal route
  (`crates/server/src/db/reports.rs:299` `if owner != author_id {`).
- Send `admin_notes` with every denial; a grant needs none
  (`crates/server/src/api/reports.rs:213` `AppealDecision::Denied => Ok((decision, Some(required_text("admin_notes", note)?))),`).
- Expect a grant to restore the status the post had before the hide
  (`crates/server/src/db/reports.rs:178` `Some(prior) => prior,`).
- Expect a grant to set `active` instead when that earlier status was itself hidden or flagged
  (`crates/server/src/db/reports.rs:177` `Some(PostStatus::Hidden | PostStatus::Flagged) | None => PostStatus::Active,`).

### Apply migration 004

Who applies `004`, when does it run, and what must never happen to it afterwards?

- Apply `migrations/004_moderation_path.sql` as the operator's own step: the server runs every pending
  migration on boot (`crates/server/src/main.rs:76` `sqlx::migrate!("../../migrations")`).
- Expect it to create two tables (`migrations/004_moderation_path.sql:2` `CREATE TABLE moderation_actions (`;
  `:16` `CREATE TABLE appeals (`).
- Expect every removal row to record the status the hide replaced
  (`migrations/004_moderation_path.sql:10` `prior_status TEXT NOT NULL,`).
- Expect that column to accept exactly the post statuses of `001`
  (`migrations/004_moderation_path.sql:12` `CONSTRAINT chk_moderation_actions_prior_status CHECK`;
  `migrations/001_schema.sql:162` `CONSTRAINT chk_posts_status CHECK`).
- Expect a post hidden before `004` to have no removal its author can appeal
  (`crates/server/src/db/reports.rs:103` `"this post was hidden before removals recorded a reason, so there is no removal to appeal";`).
- Leave `004` unedited once it has run, because each applied migration is stored with its checksum
  (`docs/DEVELOPMENT.md:71` `checksum       BYTEA NOT NULL,`; `docs/DEVELOPMENT.md:117` `Never edit an applied`).
