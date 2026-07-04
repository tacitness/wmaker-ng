#!/usr/bin/env python3
"""Smoke-test the disposable browser semantic adapter (#51).

The test starts the wmaker-ai-browser image with a throwaway profile, loads the
in-repo extension/native-messaging host, navigates to a generated fixture page,
and verifies that `observe` includes browser semantic controls without pixels.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import textwrap
import time
import importlib.util


def load_drive_browser_module():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(script_dir, "drive-browser.py")
    spec = importlib.util.spec_from_file_location("drive_browser", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load {}".format(path))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


drive_browser = load_drive_browser_module()
Mcp = drive_browser.Mcp
call = drive_browser.call
text_json = drive_browser.text_json


FIXTURE = """\
<!doctype html>
<html>
  <head><title>wmaker browser adapter smoke</title></head>
  <body>
    <main>
      <h1>Adapter Smoke</h1>
      <button id="send">Send form</button>
      <label>Search <input id="search" type="text" placeholder="Search docs"></label>
      <label>Agree <input id="agree" type="checkbox" checked></label>
      <a href="https://example.com/docs">Read docs</a>
      <input id="secret" type="password" name="current-password" value="do-not-report">
    </main>
  </body>
</html>
"""


def write_fixture(tmpdir):
    path = os.path.join(tmpdir, "adapter-smoke.html")
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(FIXTURE)
    return "file://" + path


def wait_for_browser_observe(mcp, timeout):
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        observed = text_json(call(mcp, "observe"))
        browser = observed.get("browser", {})
        last = browser
        controls = browser.get("controls") or []
        labels = {control.get("label") for control in controls}
        if browser.get("connected") and {"Send form", "Search", "Agree", "Read docs"} <= labels:
            if any(control.get("label") == "Search" and "value" in control for control in controls):
                raise RuntimeError("browser adapter leaked text input value: {}".format(controls))
            if any(control.get("label") == "Password" and not control.get("redacted") for control in controls):
                raise RuntimeError("browser adapter did not mark password control redacted: {}".format(controls))
            return observed
        time.sleep(1.0)
    raise TimeoutError("browser semantic adapter did not connect; last browser block: {}".format(last))


def main():
    parser = argparse.ArgumentParser(
        formatter_class=argparse.RawDescriptionHelpFormatter,
        description=textwrap.dedent(__doc__ or "").strip(),
    )
    parser.add_argument("--image", default="wmaker-ai-browser")
    parser.add_argument("--browser", default="brave-browser")
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument("--render-wait", type=float, default=8.0)
    parser.add_argument("--keep-fixture", action="store_true")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="wmaker-browser-adapter-") as tmpdir:
        url = write_fixture(tmpdir)
        argv = [
            "docker",
            "run",
            "-i",
            "--rm",
            "-v",
            tmpdir + ":" + tmpdir + ":ro",
            "-e",
            "DISPOSABLE_PROFILE=1",
            "-e",
            "WMAKER_AI_BROWSER_ENABLE_ADAPTER=1",
            "-e",
            "BROWSER=" + args.browser,
            "-e",
            "START_URL=" + url,
            args.image,
        ]
        print("launching:", " ".join(argv), file=sys.stderr)
        mcp = Mcp(argv, args.timeout)
        try:
            init = mcp.request(
                "initialize",
                {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "browser-adapter-smoke", "version": "0"},
                },
            )
            mcp.notify("notifications/initialized")
            time.sleep(args.render_wait)

            observed = wait_for_browser_observe(mcp, args.timeout)
            browser = observed["browser"]
            report = {
                "protocol": init.get("protocolVersion", "?"),
                "connected": browser.get("connected"),
                "adapter": browser.get("adapter"),
                "extension_id": browser.get("extension_id"),
                "title": browser.get("title"),
                "url": browser.get("url"),
                "control_count": len(browser.get("controls") or []),
                "labels": [control.get("label") for control in browser.get("controls") or []],
                "pixel_payload_present": any(
                    item.get("type") == "image" for item in observed.get("content", [])
                ),
            }
            print(json.dumps(report, indent=2))
            return 0
        finally:
            mcp.close()
            if args.keep_fixture:
                subprocess.run(["cp", "-R", tmpdir, "/tmp/wmaker-browser-adapter-fixture"], check=False)


if __name__ == "__main__":
    sys.exit(main())
