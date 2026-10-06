---
classification: public
project: proj-komun
doc_type: runbook
---

# Runbook: deploy topology

How does the code in this repository become a running Komun instance with a public URL?

Build the artifacts in order, starting with the wasm package (`docs/DEPLOY.md:14` `wasm first (the frontend depends on crates/wasm/pkg), then the frontend, then the server`). The release artifacts are the server binary and the static frontend (`docs/DEPLOY.md:20` `target/release/komun-server` and `web/build/`).

## Where do the artifacts live on the host?

Install the binary and the config under `/opt/komun` (`docs/DEPLOY.md:42` `install -m 0755 target/release/komun-server /opt/komun/`). The service runs there, because the server reads its config from the working directory (`docs/DEPLOY.md:58` `The server reads `config.toml` from its working directory`).

## How is the service started on the Alpine host?

Alpine uses OpenRC, and the repository ships a working init script (`` `docs/DEPLOY.md:61` `OpenRC (Alpine)` — `deploy/komun.initd` is a ready starting point ``). The script names the binary and the run directory (`deploy/komun.initd:4` `command="/opt/komun/komun-server"`; `:9` `directory="/opt/komun"`), and enabling it is one command (`docs/DEPLOY.md:66` `rc-update add komun`). A restart applies pending migrations at startup (`docs/DEPLOY.md:133` `migrator applies any new additive migrations on boot.`).

## What answers on the public URL?

A reverse proxy terminates TLS in front of the plain-HTTP server (`deploy/nginx-komun.conf:3` `Komun does not terminate TLS itself; run this behind a TLS listener`). The proxy forwards the API and media to loopback (`deploy/nginx-komun.conf:14` `proxy_pass http://127.0.0.1:3000;`) and serves the SPA from its own root (`deploy/nginx-komun.conf:32` `root /opt/komun/frontend;`). The shipped name is a placeholder an operator replaces (`deploy/nginx-komun.conf:10` `server_name komun.example.org;`).
