# Browser MCP Server Schema

Which server, endpoint, and runtime files does this document describe?

The server is named `browser` (`mcp/browser/server.py:100` `mcp = FastMCP("browser")`), and it serves
streamable HTTP at `http://localhost:8004/mcp`. It is the governed role `beta-tester`'s only
window onto the running application, and the driver it uses is `mcp/browser/driver.py`.

Runtime files inside the sandbox container:

| File | Default path | Authority |
| --- | --- | --- |
| Base URL | `http://rt-app-web/` | `mcp/browser/server.py:64` `os.getenv("BETA_BASE_URL", "http://rt-app-web/")` |
| Chromium binary | `/usr/bin/chromium` | `mcp/browser/server.py:65` `os.getenv("BROWSER_BINARY", "/usr/bin/chromium")` |
| Evidence directory | `/workspace/.memory/beta-evidence` | `mcp/browser/server.py:67` `os.getenv("BROWSER_EVIDENCE_DIR", str(Path(MEMORY_DIR) / "beta-evidence"))` |
| Audit journal | `/workspace/.memory/browser-audit.log` | `mcp/browser/server.py:69` `os.getenv("BROWSER_AUDIT_PATH", str(Path(MEMORY_DIR) / "browser-audit.log"))` |
| Per-call timeout | `30` seconds | `mcp/browser/server.py:70` `float(os.getenv("BROWSER_TIMEOUT_SECONDS", "30"))` |
| Role allow-list | `/workspace/mcp/browser/allow-list.json` | `mcp/browser/server.py:76` `os.getenv("BROWSER_ALLOW_LIST_PATH", ...)` |

The audit journal defaults under `MEMORY_DIR`, which defaults to `/workspace/.memory`
(`mcp/browser/server.py:63` `MEMORY_DIR = os.getenv("MEMORY_DIR", "/workspace/.memory")`). Both
configuration files sit in the repository — the allow-list beside this server and the routing map
under `docs/` — so each resolves from any working directory (`mcp/browser/server.py:81`
`str(Path(__file__).resolve().parents[2] / "docs" / "routing-and-tool-grant-map.json")`).

Which start command does the container use?

```bash
python3 mcp/browser/server.py --port 8004 --host 0.0.0.0
python3 mcp/browser/server.py --port 8004 --allowlist-path /tmp/other-allow-list.json   # optional
```

The port defaults to `8004` (`mcp/browser/server.py:589`
`parser.add_argument("--port", type=int, default=8004, help="HTTP port (default 8004)")`) and the
host defaults to `0.0.0.0` (`mcp/browser/server.py:590`
`parser.add_argument("--host", default="0.0.0.0", help="bind address (default 0.0.0.0)")`). The
CLI also overrides `--base-url`, `--binary`, `--evidence-dir`, `--audit-path`, `--timeout-seconds`,
`--allowlist-path` and `--routing-map-path` for a local run, plus `--disable-javascript` for the
self-test's negative control.

The server prints its grants as it starts, so the run states what it will allow
(`mcp/browser/server.py:640` `print(f"browser grants: {json.dumps(ALLOW_LIST, sort_keys=True)}"`),
which printed `browser grants: {"beta-tester": ["browser_open", ...], "tester": []}` on a live start.

## Which eight tools does the server expose, and what does each return?

Exactly eight, no more and no fewer (`mcp/browser/server.py:88` `OPERATIONS = (`). Each is marked by
`@mcp.tool`, and the tool names are the operation names.

| Tool | Definition | Returns |
| --- | --- | --- |
| `browser_open(url, calling_role)` | `mcp/browser/server.py:423` `async def browser_open(` | `{url, title, status, ok}` |
| `browser_snapshot(calling_role)` | `mcp/browser/server.py:453` `async def browser_snapshot(` | `{url, title, text, links[], fields[], buttons[]}` |
| `browser_click(selector, calling_role)` | `mcp/browser/server.py:466` `async def browser_click(` | `{clicked, selector, url_after}` |
| `browser_type(selector, text, calling_role)` | `mcp/browser/server.py:485` `async def browser_type(` | `{typed, selector}` |
| `browser_press(key, calling_role)` | `mcp/browser/server.py:503` `async def browser_press(` | `{pressed}` |
| `browser_diagnostics(calling_role)` | `mcp/browser/server.py:514` `async def browser_diagnostics(` | `{console[], page_errors[], requests[]}` |
| `browser_screenshot(name, calling_role)` | `mcp/browser/server.py:531` `async def browser_screenshot(` | `{path}` |
| `browser_close(calling_role)` | `mcp/browser/server.py:559` `async def browser_close(` | `{closed}` |

Every tool takes a `calling_role` string that defaults to `unknown`; the role is bound to the
container's `AGENT_ROLE`, so the argument only corroborates the identity the harness set.

## Which URLs may `browser_open` reach?

Only the base origin's. A bare path is resolved against `BETA_BASE_URL`, and an absolute URL is
accepted only when its scheme, host and port equal the base's (`mcp/browser/server.py:355`
`def resolve_target(url: str)`). Any other origin is refused with a `url_refused` reason naming the
base origin, and the refusal is journalled before it is raised
(`mcp/browser/server.py:429` `reason=refusal,`). A live call as `beta-tester` returned

```
Error calling tool 'browser_open': url_refused: 'http://example.com/evil' is outside the base origin http://127.0.0.1:35899; only the base origin is reachable
```

The empty target is refused the same way, because there is no page to open and no origin to check
(`mcp/browser/server.py:363` `"url_refused: the target is empty; pass a path such as '/aid' or an absolute URL whose "`).

## Which role may call which tool, and where is that grant written?

Where does the grant live, and which guard reads it?

The grant is data, not code: it lives in `mcp/browser/allow-list.json`, one entry per role, naming
exactly the tools that role may call. The server reads it at startup (`mcp/browser/server.py:344`
`ALLOW_LIST: dict[str, list[str]] = load_allow_list(ALLOW_LIST_PATH)`), and `_authorize` is the first
statement of every tool (`mcp/browser/server.py:300` `def _authorize(calling_role: str | None, tool: str) -> str:`),
so a refused call launches no browser and writes nothing:

| Operation | The guard, as the first statement of the tool |
| --- | --- |
| `browser_open` | `mcp/browser/server.py:425` `role = _authorize(calling_role, "browser_open")` |
| `browser_snapshot` | `mcp/browser/server.py:455` `role = _authorize(calling_role, "browser_snapshot")` |
| `browser_click` | `mcp/browser/server.py:468` `role = _authorize(calling_role, "browser_click")` |
| `browser_type` | `mcp/browser/server.py:487` `role = _authorize(calling_role, "browser_type")` |
| `browser_press` | `mcp/browser/server.py:505` `role = _authorize(calling_role, "browser_press")` |
| `browser_diagnostics` | `mcp/browser/server.py:516` `role = _authorize(calling_role, "browser_diagnostics")` |
| `browser_screenshot` | `mcp/browser/server.py:533` `role = _authorize(calling_role, "browser_screenshot")` |
| `browser_close` | `mcp/browser/server.py:561` `role = _authorize(calling_role, "browser_close")` |

Which grants does the file hold?

Only `beta-tester` holds a browser tool; every other role's list is empty. `beta-tester` holds all
eight, and the file records why each denial exists in its `denial_note_by_role` and
`denial_note_by_tool` fields.

| Role | The eight `browser_*` tools |
| --- | --- |
| `beta-tester` | granted, all eight |
| `orchestrator`, `planner`, `implementer`, `tester`, `reviewer`, `project-manager`, `researcher` | denied, every one |

That table is the file's own content, and the file is the projection of
`docs/routing-and-tool-grant-map.json`: every `mcp__browser__<tool>` string under `grants.<role>`
becomes a grant and nothing else does (`mcp/browser/server.py:98` `GRANT_PREFIX = "mcp__browser__"`).

What does a refusal look like?

It raises `AuthorizationDenied` (`mcp/browser/server.py:156` `class AuthorizationDenied(PermissionError):`)
with a message carrying the literal token `authorization_denied`, the role, the tool and the roles
that ARE allowed, and it is journalled before it is raised. A live call as `tester` while the
container was bound to `beta-tester` returned

```
Error calling tool 'browser_open': authorization_denied: calling_role 'tester' disagrees with the bound AGENT_ROLE 'beta-tester' ...
```

## How is the allow-list checked against the routing map at startup?

The map's `grants[role]` filtered to `mcp__browser__*` must equal the allow-list's entry for the
role, or the server refuses to start (`mcp/browser/server.py:228`
`def check_configuration_agrees(`). A live start with a map that dropped one browser grant printed

```
ERROR: /workspace/mcp/browser/allow-list.json lists [... 'browser_open', ...] for role 'beta-tester' while /tmp/disagreeing-map.json projects [...]; the two files must agree
```

and exited `2`, with nothing listening on its port afterwards. A map that predates the browser
server carries no projection to disagree with, so it is a warning and the allow-list governs
(`mcp/browser/server.py:243` `if not mentions_browser:`).

What happens when the allow-list file is missing?

The server stops before it binds a port (`mcp/browser/server.py:168` `raise AllowListError(`), because
starting without the file would let every role call every tool. A live run with
`BROWSER_ALLOW_LIST_PATH` set to a path that does not exist printed

```
ERROR: allow-list file not found at /workspace/mcp/browser/missing-allow-list.json; refusing to start, because a server without its allow-list would let every role call every tool
```

and exited `2`. The loader also refuses a file that grants a tool this server does not expose
(`mcp/browser/server.py:187` `raise AllowListError(`), so a typo cannot silently disable or invent a
grant.

## What shape does one journal record have?

How is each record written to the journal?

Hand each record to the one hash-chain module all the MCP journals share
(`mcp/browser/server.py:121` `hashchain.append_journal_record(AUDIT_PATH, record)`). Write one JSON
object per line, with keys in sorted order, and fsync each record so a following `tail -n 1` sees it
immediately (`mcp/hashchain.py:262` `os.fsync(handle.fileno())`).

| Key | Meaning |
| --- | --- |
| `timestamp` | ISO-8601 UTC instant (`mcp/browser/server.py:140` `"timestamp": utc_now(),`) |
| `tool` | The tool name, one of the eight (`mcp/browser/server.py:141` `"tool": tool,`) |
| `calling_role` | The bound role, defaulting to `unknown` (`mcp/browser/server.py:142` `"calling_role": calling_role or "unknown",`) |
| `target` | The resolved URL, selector or key, or `null` (`mcp/browser/server.py:143` `"target": target if isinstance(target, str) else None,`) |
| `allowed` | `true` for a permitted call, `false` for a denial (`mcp/browser/server.py:144` `"allowed": allowed,`) |
| `ok` | Whether the call succeeded; `false` for a refused URL or a missed selector (`mcp/browser/server.py:145` `"ok": ok,`) |
| `reason` | `null` when the call succeeded, a short cause otherwise (`mcp/browser/server.py:146` `"reason": reason,`) |
| `chain` | The line's hash-chain block: `seq`, `prev`, `head` and `seeded` (`mcp/hashchain.py:229` `return {"seq": seq, "prev": prev_head, "head": head, "seeded": seeded}, add_newline`) |

One live refusal row (`browser_open` of `http://example.com/evil` as `beta-tester`) carries
`"allowed": true`, `"ok": false` and a `"reason"` beginning `url_refused:`. One authorization denial
carries `"allowed": false` and a `"reason"` beginning `authorization_denied:`. Every allowed call
writes exactly one row too, so the journal is one line per call, not one line per state change.

## How does the driver launch Chromium and read the chosen port?

The driver launches the container's own Chromium with `--headless=new`
(`mcp/browser/driver.py:31` `"--headless=new",`), and as root it adds `--no-sandbox` and
`--disable-dev-shm-usage` (`mcp/browser/driver.py:32` `"--no-sandbox",`), because the container's
`/dev/shm` is 64MB and a heavy page would otherwise kill the renderer. With
`--remote-debugging-port=0` the chosen port and browser path are written to `DevToolsActivePort`
inside `--user-data-dir`, and the driver reads them there (`mcp/browser/driver.py:133`
`port_file = Path(self._profile or "") / "DevToolsActivePort"`).

The driver then speaks CDP over that WebSocket: `Target.createTarget` and `Target.attachToTarget`
with `flatten` give one page session, and `Page`, `Runtime`, `Log` and `Network` are enabled on it
(`mcp/browser/driver.py:124` `for domain in ("Page", "Runtime", "Log", "Network"):`). There is no
Playwright download and no Node process at call time.

How is the negative control's JavaScript switch implemented?

`--disable-javascript` (or `BROWSER_DISABLE_JAVASCRIPT=1`) makes the driver call
`Emulation.setScriptExecutionDisabled` on the page session
(`mcp/browser/driver.py:128` `"Emulation.setScriptExecutionDisabled", {"value": True}, session_id=self._session`),
so the page's own scripts never run while the debugger's evaluations still do.

## How does `browser_diagnostics` drain its evidence?

The driver records console messages, page errors and per-request status in memory as the page runs,
and `diagnostics()` returns them and clears the three lists in one step
(`mcp/browser/driver.py:386` `def diagnostics(self) -> dict:`). Draining on read is the point: a beta
tester's evidence is the errors the page produced, so a second `browser_diagnostics` after the first
returns empty lists, which the self-test asserts (`mcp/browser/selftest.py:272`
`"browser_diagnostics_drains_on_read"`).

## How is the server proved with a positive and a negative control?

The self-test starts the server against a page it serves, whose inline
`<img src=x onerror="document.title='PWNED'">` sets the title when page JavaScript runs
(`mcp/browser/selftest.py:58` `PWNED_PAGE = b"""<!doctype html>`) and expects exactly the eight tool
names (`mcp/browser/selftest.py:44` `TOOL_NAMES = {`). The positive control reads `PWNED` back. The
negative control is `--negative-control` (`mcp/browser/selftest.py:383` `"--negative-control",`): the
same run with page JavaScript disabled, so the title stays `before` and the process exits non-zero.
The missing image also 404s, so a failed-resource console entry is expected noise, never a failure.

```bash
python3 mcp/browser/selftest.py                    # positive: exits 0
python3 mcp/browser/selftest.py --negative-control # negative: exits non-zero
```

## Design notes

Does the server expose a page's raw HTML, a screenshot inside a tool result, or a filesystem path?

Answer: no to the first two, and the third is confined.

- Expose named operations only, so a caller cannot widen its own access with a query string
  (`mcp/browser/server.py:88` `OPERATIONS = (` names the only eight callable entries).
- Keep `browser_snapshot` text-first (`mcp/browser/driver.py:53` `PAGE_STATE_JS` returns text, links,
  fields and buttons and no image data), so the result reads in a text-only agent context.
- Write PNGs under the evidence dir and return only the path (`mcp/browser/server.py:551`
  `path.write_bytes(png)`), so image bytes never enter the agent context.
- Sanitise the screenshot name to a bare `*.png` file name (`mcp/browser/server.py:379`
  `def safe_evidence_name(name: str) -> str:`), so a name cannot escape the evidence dir.
- Bind the caller to the container's `AGENT_ROLE` (`mcp/browser/server.py:278` `def bind_role(`, the
  storage server's guard unchanged), so a caller cannot escalate by typing another role's name.
- Chain the journal like the other three (`mcp/browser/server.py:121` `hashchain.append_journal_record(`),
  so the browser evidence joins the same append-only chain.