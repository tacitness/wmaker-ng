#!/usr/bin/env python3
"""Drive the wmaker-ai-browser sandbox over MCP (#20).

Boots `wmaker-ai-browser` (Xvfb + wmaker + Brave + ai-mcp) via `docker run -i`,
speaks MCP over stdio, waits for the browser window, then proves the agent can
both *observe* (screenshot + changed_regions) and *drive* (click/type) a real
browser inside the headless desktop. Dependency-free, same shape as
scripts/mcp-smoke.py.
"""

import argparse
import base64
import json
import os
import select
import subprocess
import sys
import time


class Mcp:
    def __init__(self, argv, timeout):
        self.timeout = timeout
        self.next_id = 1
        self.proc = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            universal_newlines=True,
            bufsize=1,
        )

    def close(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()

    def request(self, method, params=None):
        msg_id = self.next_id
        self.next_id += 1
        payload = {"jsonrpc": "2.0", "id": msg_id, "method": method}
        if params is not None:
            payload["params"] = params
        self._send(payload)
        return self._read(msg_id)

    def notify(self, method, params=None):
        payload = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = params
        self._send(payload)

    def _send(self, payload):
        if self.proc.poll() is not None:
            raise RuntimeError("ai-mcp exited before request: " + self._stderr())
        self.proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()

    def _read(self, msg_id):
        deadline = time.time() + self.timeout
        while time.time() < deadline:
            ready, _, _ = select.select([self.proc.stdout], [], [], 0.2)
            if not ready:
                if self.proc.poll() is not None:
                    raise RuntimeError("ai-mcp exited: " + self._stderr())
                continue
            line = self.proc.stdout.readline()
            if not line or not line.strip():
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                print("non-json stdout: " + line.rstrip()[:200], file=sys.stderr)
                continue
            if msg.get("id") == msg_id:
                if "error" in msg:
                    raise RuntimeError("{} failed: {}".format(msg_id, msg["error"]))
                return msg["result"]
        raise TimeoutError("timed out waiting for response {}".format(msg_id))

    def _stderr(self):
        ready, _, _ = select.select([self.proc.stderr], [], [], 0)
        return self.proc.stderr.read()[-1500:] if ready else "<no stderr>"


def call(mcp, name, args=None):
    res = mcp.request("tools/call", {"name": name, "arguments": args or {}})
    if res.get("isError"):
        raise RuntimeError("tool {} error: {}".format(name, res))
    return res


def text_json(res):
    for item in res.get("content", []):
        if item.get("type") == "text":
            return json.loads(item["text"])
    raise RuntimeError("no text content: {}".format(res))


def save_png(res, path):
    for item in res.get("content", []):
        if item.get("type") == "image" and item.get("mimeType") == "image/png":
            data = base64.b64decode(item["data"])
            with open(path, "wb") as fh:
                fh.write(data)
            return len(data)
    raise RuntimeError("no image content in screenshot result")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", default="wmaker-ai-browser")
    ap.add_argument("--url", default="https://example.com")
    ap.add_argument("--geometry", default="1280x800x24")
    ap.add_argument("--out-dir", default="/tmp/wmaker-ai-browser")
    ap.add_argument("--profile", default=None,
                    help="host profile dir to bind-mount at /profile (live Brave must be closed)")
    ap.add_argument("--clear-singleton", action="store_true")
    ap.add_argument("--timeout", type=float, default=30.0)
    ap.add_argument("--render-wait", type=float, default=10.0)
    args = ap.parse_args()

    os.makedirs(args.out_dir, exist_ok=True)
    argv = ["docker", "run", "-i", "--rm",
            "-e", "SCREEN_GEOMETRY=" + args.geometry,
            "-e", "START_URL=" + args.url]
    if args.profile:
        argv += ["-v", os.path.abspath(args.profile) + ":/profile"]
    if args.clear_singleton:
        argv += ["-e", "CLEAR_SINGLETON=1"]
    argv.append(args.image)

    print("launching:", " ".join(argv), file=sys.stderr)
    mcp = Mcp(argv, args.timeout)
    try:
        init = mcp.request("initialize", {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "drive-browser", "version": "0"},
        })
        mcp.notify("notifications/initialized")
        proto = init.get("protocolVersion", "?")

        # Give Brave time to create its profile and paint the page.
        print("waiting {:.0f}s for Brave to render...".format(args.render_wait), file=sys.stderr)
        time.sleep(args.render_wait)

        windows = text_json(call(mcp, "list_windows")).get("windows", [])
        titles = [w.get("title", "") for w in windows]

        # Keyframe baseline, then a full screenshot of the rendered page.
        keyframe = text_json(call(mcp, "changed_regions"))
        before = save_png(call(mcp, "screenshot"), os.path.join(args.out_dir, "01-page.png"))

        # Drive input: click into the page and type, to dirty the screen.
        call(mcp, "move_mouse", {"x": 640, "y": 400})
        call(mcp, "click", {})
        call(mcp, "type", {"text": "wmaker-ng drives brave"})
        time.sleep(1.0)
        delta = text_json(call(mcp, "changed_regions"))
        after = save_png(call(mcp, "screenshot"), os.path.join(args.out_dir, "02-after-input.png"))

        report = {
            "protocol": proto,
            "windows": titles,
            "keyframe_kind": keyframe.get("kind"),
            "keyframe_regions": len(keyframe.get("regions", [])),
            "delta_kind": delta.get("kind"),
            "delta_regions": len(delta.get("regions", [])),
            "delta_dirty_area": delta.get("dirty_area"),
            "screenshot_page_bytes": before,
            "screenshot_after_bytes": after,
            "out_dir": args.out_dir,
        }
        print(json.dumps(report, indent=2))
        return 0
    finally:
        mcp.close()


if __name__ == "__main__":
    sys.exit(main())
