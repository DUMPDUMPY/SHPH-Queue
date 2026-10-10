"""Fixtures for browser tests: a fresh server per test and a Chromium browser.

Run:  cargo build && pip install pytest playwright && python -m playwright install chromium
      pytest tests/e2e
Env:  SHPH_BIN       path to the server binary (default target/debug/shph-queue)
      CHROMIUM_PATH  use an existing Chromium instead of Playwright's download
"""

import json
import os
import pathlib
import socket
import subprocess
import time
import urllib.error
import urllib.request

import pytest
from playwright.sync_api import sync_playwright

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = os.environ.get("SHPH_BIN") or str(ROOT / "target" / "debug" / ("shph-queue.exe" if os.name == "nt" else "shph-queue"))


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Server:
    def __init__(self, base_dir):
        self.dir = base_dir
        self.port = free_port()
        self.proc = None
        self.cookie = None

    @property
    def url(self):
        return f"http://127.0.0.1:{self.port}"

    def start(self):
        self.proc = subprocess.Popen(
            [BIN, "--dir", str(self.dir), "--port", str(self.port)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        deadline = time.time() + 15
        while True:
            try:
                urllib.request.urlopen(self.url + "/", timeout=1)
                return self
            except OSError:
                if time.time() > deadline:
                    raise RuntimeError("server did not start")
                time.sleep(0.05)

    def stop(self):
        if self.proc:
            self.proc.kill()
            self.proc.wait()
            self.proc = None

    def api(self, path, body=None, method=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.url + "/api" + path, data=data, method=method or ("GET" if body is None else "POST"))
        req.add_header("Content-Type", "application/json")
        if self.cookie:
            req.add_header("Cookie", self.cookie)
        try:
            with urllib.request.urlopen(req) as r:
                if path == "/admin/login":
                    self.cookie = r.headers["Set-Cookie"].split(";")[0]
                return json.load(r)
        except urllib.error.HTTPError as e:
            raise AssertionError(f"{path}: {e.code} {e.read().decode()}")

    def state(self):
        return self.api("/state")


@pytest.fixture
def server(tmp_path):
    s = Server(tmp_path).start()
    yield s
    s.stop()


@pytest.fixture(scope="session")
def browser():
    with sync_playwright() as p:
        kwargs = {"args": ["--autoplay-policy=no-user-gesture-required"]}
        if os.environ.get("CHROMIUM_PATH"):
            kwargs["executable_path"] = os.environ["CHROMIUM_PATH"]
        b = p.chromium.launch(**kwargs)
        yield b
        b.close()


@pytest.fixture
def open_page(browser, server):
    """open_page(path, width=1280, height=900) -> Page. Fails the test on any console error.

    A test that expects an error message adds part of it to open_page.allow.
    """
    pages, errors = [], []

    def _open(path, width=1280, height=900):
        ctx = browser.new_context(viewport={"width": width, "height": height})
        page = ctx.new_page()
        page.on("console", lambda m: errors.append(f"{path}: {m.text}") if m.type == "error" else None)
        page.on("pageerror", lambda e: errors.append(f"{path}: {e}"))
        page.goto(server.url + path)
        page.wait_for_load_state("networkidle")
        pages.append(ctx)
        return page

    _open.allow = []
    yield _open
    for ctx in pages:
        ctx.close()
    # A dropped connection while a test restarts the server is expected.
    allowed = ["WebSocket", "ERR_CONNECTION_REFUSED", *_open.allow]
    errors = [e for e in errors if not any(a in e for a in allowed)]
    assert not errors, "console errors:\n" + "\n".join(errors)
