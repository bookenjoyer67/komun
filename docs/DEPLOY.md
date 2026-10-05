# Deployment (self-hosting)

Generic guidance for running Komun on your own machine. These are instructions, not a
script — nothing here is performed automatically, and nothing here touches DNS, Cloudflare or
any other edge layer (that is the operator's to manage).

## 1. Build the release binary and frontend

```bash
# wasm first (the frontend depends on crates/wasm/pkg), then the frontend, then the server
wasm-pack build crates/wasm --target web
cd web && npm ci && npm run build && cd ..
cargo build --release --bin komun-server
```

Artifacts: `target/release/komun-server` and `web/build/` (the server serves the SPA's static
files itself; you can also serve them from the reverse proxy).

## 2. Provision the database

Follow the exact order in `docs/DEVELOPMENT.md` ("Provisioning a database"): create the
database and role, create `_sqlx_migrations`, load `migrations/001_schema.sql`, insert the
`001` bookmark with its real `sha384`, then boot once so the migrator applies `002+`. Never
edit `001_schema.sql`.

Use a dedicated, least-privilege database user and a strong password. Put the connection
string in the server's `config.toml` (`[database] url`) or the `DATABASE_URL` environment
variable.

## 3. Install and configure

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
`/api/directory*` returns 404 by design.

## 4. Run it as a service

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

Komun speaks plain HTTP; put a reverse proxy in front for TLS. `deploy/nginx-komun.conf` is a
starting point (proxy `/api/`, `/avatars/`, `/post-images/` to the server; serve the SPA with
an `index.html` fallback). There is **no** WebSocket/relay route to proxy any more.

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

- **Health:** `curl -s http://127.0.0.1:3000/api/health` → `{"service":"komun","status":"ok",...}`.
- **Media:** avatars and post images live under `[media]` paths inside the working directory —
  include them in backups.
- **Database:** back up PostgreSQL (the schema, plus the tables in `docs/DATABASE.md`).
- **Seed (optional):** `psql "$DATABASE_URL" -f deploy/seed.sql` adds demo accounts and posts;
  the 23 marketplace/aid categories come from `001_schema.sql` and are not in the seed file.
- **Categories:** the taxonomy is a runtime-editable table (`docs/ARCHITECTURE.md`, "Categories
  are data"). An admin adds, relabels, reorders or retires a category through
  `POST`/`PATCH /api/admin/categories`; retiring means `active = false`, never a delete.
- **Mail (optional):** verification and password-reset mail need `[email] smtp_host` + `from`.
  If you do not run SMTP, keep `[registration] require_email_verification = false`; the server
  otherwise refuses to start.
- **Upgrades:** stop the service, install the new binary and `web/build`, start it — the
  migrator applies any new additive migrations on boot.
