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
