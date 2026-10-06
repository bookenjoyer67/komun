#!/usr/bin/env python3
"""Headless-Chromium driver for the browser MCP server, over the DevTools Protocol.

The server is Python, so the driver is Python: it launches the container's own ``chromium`` with
``--headless=new`` and speaks CDP over the browser's DevTools WebSocket. No Playwright download and
no Node process is involved at call time, and the distro Chromium is the only browser used.

The eight browser tools need five things from a page: navigation and its HTTP status, a text-first
snapshot of its DOM, the console/network diagnostics it produced, input events, and a PNG. Each is
one CDP domain call or a small ``Runtime.evaluate``, and this file is the only place that knows the
wire.
"""

from __future__ import annotations

import asyncio
import base64
import json
import os
import shutil
import tempfile
import time
from pathlib import Path
from typing import Any

import websockets

# Chromium as root needs --no-sandbox, and the container's /dev/shm is 64MB, so a heavy page would
# kill the renderer without --disable-dev-shm-usage. Both are non-negotiable in this image.
LAUNCH_FLAGS = (
    "--headless=new",
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--disable-gpu",
    "--hide-scrollbars",
    "--mute-audio",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    "--disable-component-update",
)
KEY_CODES = {
    "Enter": ("Enter", 13),
    "Tab": ("Tab", 9),
    "Escape": ("Escape", 27),
    "Backspace": ("Backspace", 8),
    "ArrowDown": ("ArrowDown", 40),
    "ArrowUp": ("ArrowUp", 38),
    " ": ("Space", 32),
}
# A text-first snapshot: links, form fields and buttons, plus the visible text. No image data and no
# screenshot bytes, so the result is readable in a text-only agent context.
PAGE_STATE_JS = """(() => {
  const text = (node) => (node && node.innerText ? node.innerText : '').trim();
  const links = [...document.querySelectorAll('a[href]')].map((a) => ({ text: text(a), href: a.href }));
  const fields = [...document.querySelectorAll('input, textarea, select')].map((el) => ({
    name: el.name || el.id || '',
    type: (el.type || el.tagName).toLowerCase(),
    value: 'value' in el ? el.value : null,
  }));
  const buttons = [...document.querySelectorAll('button, input[type=submit], input[type=button], [role=button]')]
    .map((b) => ({ text: text(b) || b.value || '', type: (b.type || 'button').toLowerCase() }));
  return {
    url: location.href,
    title: document.title,
    text: document.body ? document.body.innerText.trim() : '',
    links: links, fields: fields, buttons: buttons,
  };
})()"""


class DriverError(RuntimeError):
    """A CDP call, a page evaluation or the browser launch failed in a way the caller must see."""


class CDPError(DriverError):
    """The browser answered a command with a protocol error object."""


class Chrome:
    """One headless Chromium process and the single page target the tools operate on."""

    def __init__(self, binary: str, timeout: float = 30.0, disable_javascript: bool = False) -> None:
        self.binary = binary
        self.timeout = timeout
        self.disable_javascript = disable_javascript
        self._profile: str | None = None
        self._process: asyncio.subprocess.Process | None = None
        self._ws: Any = None
        self._reader: asyncio.Task | None = None
        self._session: str | None = None
        self._target: str | None = None
        self._next_id = 0
        self._pending: dict[int, asyncio.Future] = {}
        self._load = asyncio.Event()
        self._document_status: int | None = None
        self._console: list[dict] = []
        self._page_errors: list[str] = []
        self._requests: dict[str, dict] = {}

    # --- lifecycle --------------------------------------------------------------------------
    async def start(self) -> None:
        """Launch Chromium, attach to a fresh page target and enable the four domains used."""
        self._profile = tempfile.mkdtemp(prefix="browser-profile-")
        argv = [
            self.binary,
            *LAUNCH_FLAGS,
            "--remote-debugging-port=0",
            f"--user-data-dir={self._profile}",
            "about:blank",
        ]
        self._process = await asyncio.create_subprocess_exec(
            *argv, stdout=asyncio.subprocess.DEVNULL, stderr=asyncio.subprocess.DEVNULL
        )
        port, path = await self._devtools_endpoint()
        self._ws = await websockets.connect(f"ws://127.0.0.1:{port}{path}", max_size=2**26)
        self._reader = asyncio.create_task(self._read_loop())
        created = await self._send("Target.createTarget", {"url": "about:blank"})
        self._target = created["targetId"]
        attached = await self._send(
            "Target.attachToTarget", {"targetId": self._target, "flatten": True}
        )
        self._session = attached["sessionId"]
        for domain in ("Page", "Runtime", "Log", "Network"):
            await self._send(f"{domain}.enable", session_id=self._session)
        if self.disable_javascript:
            await self._send(
                "Emulation.setScriptExecutionDisabled", {"value": True}, session_id=self._session
            )

    async def _devtools_endpoint(self) -> tuple[int, str]:
        """Read the chosen port and browser path from DevToolsActivePort inside the profile dir."""
        port_file = Path(self._profile or "") / "DevToolsActivePort"
        deadline = time.monotonic() + self.timeout
        while True:
            if self._process is not None and self._process.returncode is not None:
                raise DriverError(
                    f"chromium exited with code {self._process.returncode} before DevTools was ready"
                )
            if port_file.exists():
                lines = port_file.read_text(encoding="utf-8").splitlines()
                if len(lines) >= 2 and lines[0].strip().isdigit():
                    return int(lines[0]), lines[1].strip()
            if time.monotonic() > deadline:
                raise DriverError(
                    f"chromium did not write DevToolsActivePort within {self.timeout}s"
                )
            await asyncio.sleep(0.05)

    async def close(self) -> None:
        """Cancel the reader, drop the WebSocket, kill Chromium and remove its profile."""
        if self._reader is not None:
            self._reader.cancel()
            try:
                await self._reader
            except asyncio.CancelledError:
                pass
            except Exception:  # noqa: BLE001 - closing must not raise over a reader fault
                pass
            self._reader = None
        if self._ws is not None:
            await self._ws.close()
            self._ws = None
        if self._process is not None and self._process.returncode is None:
            self._process.terminate()
            try:
                await asyncio.wait_for(self._process.wait(), timeout=10)
            except asyncio.TimeoutError:
                self._process.kill()
                await self._process.wait()
        if self._profile and os.path.isdir(self._profile):
            shutil.rmtree(self._profile, ignore_errors=True)

    def kill_sync(self) -> None:
        """Best-effort synchronous teardown for process exit; ``close()`` is the normal path."""
        if self._process is not None and self._process.returncode is None:
            try:
                self._process.kill()
            except ProcessLookupError:
                pass
        if self._profile and os.path.isdir(self._profile):
            shutil.rmtree(self._profile, ignore_errors=True)

    # --- the wire ---------------------------------------------------------------------------
    async def _send(
        self, method: str, params: dict | None = None, *, session_id: str | None = None
    ) -> dict:
        """Send one CDP command and await its result, matched by message id."""
        if self._ws is None:
            raise DriverError("the browser is not started")
        self._next_id += 1
        message_id = self._next_id
        message: dict[str, Any] = {"id": message_id, "method": method}
        if params:
            message["params"] = params
        if session_id:
            message["sessionId"] = session_id
        future = asyncio.get_running_loop().create_future()
        self._pending[message_id] = future
        await self._ws.send(json.dumps(message))
        try:
            return await asyncio.wait_for(future, timeout=self.timeout)
        except asyncio.TimeoutError as error:
            self._pending.pop(message_id, None)
            raise DriverError(f"CDP {method} timed out after {self.timeout}s") from error

    async def _read_loop(self) -> None:
        """Dispatch every inbound frame: responses to their future, events to the recorders."""
        try:
            async for raw in self._ws:
                message = json.loads(raw)
                if "id" in message:
                    future = self._pending.pop(message["id"], None)
                    if future is None or future.done():
                        continue
                    error = message.get("error")
                    if error:
                        future.set_exception(
                            CDPError(f"{error.get('message')} ({error.get('code')})")
                        )
                    else:
                        future.set_result(message.get("result", {}))
                else:
                    self._dispatch(message)
        except websockets.exceptions.ConnectionClosed:
            for future in self._pending.values():
                if not future.done():
                    future.set_exception(DriverError("the DevTools connection closed"))
            self._pending.clear()

    def _dispatch(self, message: dict) -> None:
        """Record the console, page-error, request and load events the tools report on."""
        if message.get("sessionId") not in (None, self._session):
            return
        method = message.get("method", "")
        params = message.get("params", {}) or {}
        if method == "Page.loadEventFired":
            self._load.set()
        elif method == "Runtime.consoleAPICalled":
            self._console.append(
                {"type": params.get("type", "log"), "text": self._join_args(params.get("args", []))}
            )
        elif method == "Runtime.exceptionThrown":
            self._page_errors.append(self._exception_text(params.get("exceptionDetails", {})))
        elif method == "Log.entryAdded":
            entry = params.get("entry", {})
            self._console.append(
                {"type": entry.get("level", "info"), "text": entry.get("text", "")}
            )
        elif method == "Network.requestWillBeSent":
            request = params.get("request", {})
            self._requests[params.get("requestId", "")] = {
                "url": request.get("url", ""),
                "status": None,
                "failed": False,
            }
        elif method == "Network.responseReceived":
            record = self._requests.setdefault(
                params.get("requestId", ""), {"url": "", "status": None, "failed": False}
            )
            response = params.get("response", {})
            record["url"] = response.get("url", record["url"])
            record["status"] = response.get("status")
            if params.get("type") == "Document":
                self._document_status = response.get("status")
        elif method == "Network.loadingFailed":
            record = self._requests.setdefault(
                params.get("requestId", ""), {"url": "", "status": None, "failed": False}
            )
            record["failed"] = True

    @staticmethod
    def _join_args(args: list[dict]) -> str:
        parts: list[str] = []
        for arg in args:
            if "value" in arg:
                parts.append(str(arg["value"]))
            elif arg.get("description") is not None:
                parts.append(str(arg["description"]))
            else:
                parts.append(arg.get("type", ""))
        return " ".join(parts)

    @staticmethod
    def _exception_text(details: dict) -> str:
        exception = details.get("exception", {}) or {}
        text = exception.get("description") or details.get("text") or "page error"
        line = details.get("lineNumber")
        return f"{text} (line {line})" if line is not None else text

    # --- the five capabilities the tools need ------------------------------------------------
    async def navigate(self, url: str) -> dict:
        """Navigate, wait for load, and return ``{ok, status, title, error}``."""
        self._load.clear()
        self._document_status = None
        try:
            result = await self._send("Page.navigate", {"url": url}, session_id=self._session)
        except DriverError as error:
            return {"ok": False, "status": None, "title": None, "error": str(error)}
        if result.get("errorText"):
            return {"ok": False, "status": None, "title": None, "error": result["errorText"]}
        try:
            await asyncio.wait_for(self._load.wait(), timeout=self.timeout)
        except asyncio.TimeoutError:
            return {
                "ok": False,
                "status": self._document_status,
                "title": None,
                "error": f"navigation did not finish within {self.timeout}s",
            }
        # A short grace so an onerror/onload handler that runs after the load event is visible.
        await asyncio.sleep(0.2)
        title = await self.evaluate("document.title")
        return {"ok": True, "status": self._document_status, "title": title, "error": None}

    async def evaluate(self, expression: str) -> Any:
        """Evaluate an expression in the page and return its value by value."""
        result = await self._send(
            "Runtime.evaluate",
            {"expression": expression, "returnByValue": True, "awaitPromise": True},
            session_id=self._session,
        )
        if result.get("exceptionDetails"):
            raise DriverError(self._exception_text(result["exceptionDetails"]))
        return (result.get("result") or {}).get("value")

    async def page_state(self) -> dict:
        """Return the text-first snapshot the ``browser_snapshot`` tool publishes."""
        return await self.evaluate(PAGE_STATE_JS)

    async def click(self, selector: str) -> bool:
        """Click the first element matching ``selector``; return whether it was found."""
        found = await self.evaluate(
            "(() => { const el = document.querySelector("
            f"{json.dumps(selector)}"
            "); if (!el) return false; el.scrollIntoView({block: 'center'}); el.click(); return true; })()"
        )
        await asyncio.sleep(0.2)
        return bool(found)

    async def type_text(self, selector: str, text: str) -> bool:
        """Set the value of a form field through its own setter and fire input/change."""
        script = (
            "(() => { const el = document.querySelector("
            f"{json.dumps(selector)}"
            "); if (!el) return false; el.focus();"
            " const descriptor = Object.getOwnPropertyDescriptor(el.constructor.prototype, 'value');"
            f" if (descriptor && descriptor.set) {{ descriptor.set.call(el, {json.dumps(text)}); }}"
            f" else {{ el.value = {json.dumps(text)}; }}"
            " el.dispatchEvent(new Event('input', {bubbles: true}));"
            " el.dispatchEvent(new Event('change', {bubbles: true})); return true; })()"
        )
        return bool(await self.evaluate(script))

    async def press(self, key: str) -> None:
        """Press a key through the input domain, or dispatch a synthetic key event for the rest."""
        if key in KEY_CODES:
            code, virtual = KEY_CODES[key]
            for event_type in ("keyDown", "keyUp"):
                await self._send(
                    "Input.dispatchKeyEvent",
                    {
                        "type": event_type,
                        "key": key,
                        "code": code,
                        "windowsVirtualKeyCode": virtual,
                        "nativeVirtualKeyCode": virtual,
                    },
                    session_id=self._session,
                )
            return
        await self.evaluate(
            "(() => { const el = document.activeElement || document.body;"
            f" el.dispatchEvent(new KeyboardEvent('keydown', {{key: {json.dumps(key)}, bubbles: true}}));"
            f" el.dispatchEvent(new KeyboardEvent('keyup', {{key: {json.dumps(key)}, bubbles: true}}));"
            " return true; })()"
        )

    async def screenshot(self) -> bytes:
        """Capture the visible viewport as PNG bytes."""
        result = await self._send(
            "Page.captureScreenshot", {"format": "png"}, session_id=self._session
        )
        return base64.b64decode(result["data"])

    def diagnostics(self) -> dict:
        """Return the console, page-error and request evidence since the previous drain."""
        drained = {
            "console": self._console,
            "page_errors": self._page_errors,
            "requests": [
                {"url": record["url"], "status": record["status"], "failed": record["failed"]}
                for record in self._requests.values()
            ],
        }
        self._console = []
        self._page_errors = []
        self._requests = {}
        return drained
