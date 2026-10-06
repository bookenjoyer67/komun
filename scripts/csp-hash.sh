#!/bin/sh
# usage: scripts/csp-hash.sh web/build/index.html > deploy/csp.conf
#
# Why the hash lives here and not in the HTML: a SvelteKit build carries exactly one inline bootstrap
# script, and the served policy allows scripts only from 'self' plus that script's sha256. The hash is
# therefore a property of the build. Ship a new index.html under an old policy and every page is blocked
# -- the site renders blank, and nothing in the server log says why. The release job regenerates this
# file for each build, and the updater installs it beside the frontend.
#
# The policy text is this deployment's, reproduced from the file nginx served before this script existed
# (`/opt/komun/csp.conf`): OpenStreetMap tiles are the only third-party image source, `connect-src` stays
# 'self', and the wasm crypto module needs 'wasm-unsafe-eval'. Change the text here and the change reaches
# production on the next release; change it only with a reason, because a policy that is too strict fails
# silently in the browser.
python3 - "$1" <<'ENDOFPYTHON'
import base64
import hashlib
import re
import sys

html = open(sys.argv[1], encoding="utf-8").read()
inline = re.findall(r"<script(?![^>]*\bsrc=)[^>]*>(.*?)</script>", html, re.S)
hashes = " ".join(
    "'sha256-" + base64.b64encode(hashlib.sha256(body.encode()).digest()).decode() + "'"
    for body in inline
)
print(f"{len(inline)} inline script(s) hashed", file=sys.stderr)

policy = (
    "default-src 'self'; "
    "frame-src 'self' https://www.openstreetmap.org; "
    "connect-src 'self'; "
    "font-src 'self'; "
    "img-src 'self' data: blob: https://tile.openstreetmap.org https://*.tile.openstreetmap.org; "
    "object-src 'none'; "
    "script-src 'self' 'wasm-unsafe-eval'" + (f" {hashes}" if hashes else "") + "; "
    "style-src 'self' 'unsafe-inline'; "
    "base-uri 'self'; "
    "form-action 'self'; "
    "frame-ancestors 'none'"
)
print(f'add_header Content-Security-Policy "{policy}" always;')
ENDOFPYTHON
