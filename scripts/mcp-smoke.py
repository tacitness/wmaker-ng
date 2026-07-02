#!/usr/bin/env python3
"""Runtime smoke for ai-mcp against a live X display.

This is intentionally dependency-free: it speaks MCP JSON-RPC over stdio,
launches a disposable X client, and verifies the tools a daily-driver desktop
needs before replacing the operator's normal Window Maker session.
"""

import argparse
import json
import os
import select
import shutil
import signal
import subprocess
import sys
import time


REQUIRED_TOOLS = {
    "changed_regions",
    "changed_regions_fast",
    "click",
    "focus",
    "key",
    "list_windows",
    "move_mouse",
    "move_resize",
    "screenshot",
    "tile",
    "type",
}


class Mcp:
    def __init__(self, argv, env, timeout):
        self.timeout = timeout
        self.next_id = 1
        self.proc = subprocess.Popen(
            argv,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            universal_newlines=True,
            bufsize=1,
            env=env,
        )

    def close(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=2)

    def request(self, method, params=None):
        msg_id = self.next_id
        self.next_id += 1
        payload = {"jsonrpc": "2.0", "id": msg_id, "method": method}
        if params is not None:
            payload["params"] = params
        self._send(payload)
        response = self._read_response(msg_id)
        if "error" in response:
            raise RuntimeError("{} failed: {}".format(method, response["error"]))
        return response["result"]

    def notify(self, method, params=None):
        payload = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            payload["params"] = params
        self._send(payload)

    def _send(self, payload):
        if self.proc.poll() is not None:
            raise RuntimeError("ai-mcp exited before request")
        self.proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()

    def _read_response(self, msg_id):
        deadline = time.time() + self.timeout
        while time.time() < deadline:
            ready, _, _ = select.select([self.proc.stdout], [], [], 0.2)
            if not ready:
                if self.proc.poll() is not None:
                    raise RuntimeError("ai-mcp exited: {}".format(self.stderr_tail()))
                continue
            line = self.proc.stdout.readline()
            if not line:
                continue
            msg = json.loads(line)
            if msg.get("id") == msg_id:
                return msg
        raise TimeoutError("timed out waiting for {}: {}".format(msg_id, self.stderr_tail()))

    def stderr_tail(self):
        ready, _, _ = select.select([self.proc.stderr], [], [], 0)
        if not ready:
            return "<no stderr>"
        return self.proc.stderr.read()[-1200:]


def call_tool(mcp, name, arguments=None):
    result = mcp.request(
        "tools/call",
        {"name": name, "arguments": arguments or {}},
    )
    if result.get("isError"):
        raise RuntimeError("tool {} returned error: {}".format(name, result))
    return result


def text_json(result):
    for item in result.get("content", []):
        if item.get("type") == "text":
            return json.loads(item["text"])
    raise RuntimeError("tool result has no text JSON content: {}".format(result))


def image_size(result):
    for item in result.get("content", []):
        if item.get("type") == "image" and item.get("mimeType") == "image/png":
            return len(item.get("data", ""))
    raise RuntimeError("tool result has no image/png content")


def launch_client(display, title):
    candidates = [
        ["xclock", "-name", title, "-title", title],
        ["xterm", "-T", title, "-n", title, "-geometry", "42x8+40+40", "-e", "sh", "-c", "sleep 120"],
        ["zenity", "--info", "--title", title, "--text", title],
    ]
    env = dict(os.environ, DISPLAY=display)
    for argv in candidates:
        if shutil.which(argv[0]):
            return subprocess.Popen(argv, env=env)
    raise RuntimeError("no disposable X client found; install xclock, xterm, or zenity")


def find_window(mcp, title, timeout):
    deadline = time.time() + timeout
    while time.time() < deadline:
        windows = mcp.request("tools/call", {"name": "list_windows", "arguments": {}})
        data = text_json(windows)
        for window in data.get("windows", []):
            if title in window.get("title", ""):
                return window
        time.sleep(0.25)
    raise RuntimeError("test window did not appear in list_windows")


def assert_delta(update, screen_area):
    if update["kind"] != "delta":
        raise RuntimeError("expected delta update, got {}".format(summarize_update(update)))
    if not update.get("regions"):
        raise RuntimeError("delta update had no regions: {}".format(summarize_update(update)))
    if update.get("dirty_area", 0) >= screen_area:
        raise RuntimeError("delta was not smaller than the full screen: {}".format(summarize_update(update)))


def summarize_update(update):
    return {
        "kind": update.get("kind"),
        "width": update.get("width"),
        "height": update.get("height"),
        "dirty_area": update.get("dirty_area"),
        "rebaseline_reason": update.get("rebaseline_reason"),
        "regions": [
            {
                "x": region.get("x"),
                "y": region.get("y"),
                "width": region.get("width"),
                "height": region.get("height"),
                "png_base64_bytes": len(region.get("png_base64", "")),
            }
            for region in update.get("regions", [])
        ],
    }


def next_delta(mcp, screen_area, window_id):
    attempts = [
        ("move_resize", {"window": window_id, "x": 80, "y": 80, "width": 480, "height": 220}),
        ("type", {"text": "wmng smoke"}),
        ("move_resize", {"window": window_id, "x": 220, "y": 180, "width": 500, "height": 240}),
        ("type", {"text": "delta"}),
    ]
    last = None
    for name, args in attempts:
        call_tool(mcp, name, args)
        time.sleep(0.5)
        started = time.monotonic()
        update = text_json(call_tool(mcp, "changed_regions"))
        update["_call_ms"] = int((time.monotonic() - started) * 1000)
        if update["kind"] == "delta" and update.get("regions"):
            assert_delta(update, screen_area)
            return update
        last = update
    raise RuntimeError("could not produce a non-empty delta: {}".format(summarize_update(last or {})))


def next_fast_delta(mcp, screen_area, window_id):
    attempts = [
        ("move_resize", {"window": window_id, "x": 120, "y": 120, "width": 500, "height": 230}),
        ("type", {"text": " fast"}),
        ("move_resize", {"window": window_id, "x": 260, "y": 210, "width": 520, "height": 250}),
    ]
    last = None
    for name, args in attempts:
        call_tool(mcp, name, args)
        time.sleep(0.25)
        started = time.monotonic()
        update = text_json(call_tool(mcp, "changed_regions_fast"))
        update["_call_ms"] = int((time.monotonic() - started) * 1000)
        if update["kind"] == "delta" and update.get("regions"):
            if update.get("dirty_area", 0) >= screen_area:
                raise RuntimeError("fast delta was not smaller than the full screen: {}".format(update))
            return update
        last = update
    raise RuntimeError("could not produce a non-empty fast delta: {}".format(last or {}))


def main():
    parser = argparse.ArgumentParser(description="Smoke-test ai-mcp against a live X display.")
    parser.add_argument("--display", default=os.environ.get("DISPLAY", ":9"))
    parser.add_argument("--ai-mcp", default="./target/debug/ai-mcp")
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--title", default="wmng-mcp-smoke")
    args = parser.parse_args()

    env = dict(os.environ, DISPLAY=args.display)
    client = None
    mcp = None
    try:
        subprocess.check_call([args.ai_mcp, "--check"], env=env)
        mcp = Mcp([args.ai_mcp], env, args.timeout)
        init = mcp.request(
            "initialize",
            {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "wmng-mcp-smoke", "version": "0"},
            },
        )
        mcp.notify("notifications/initialized")

        tools = mcp.request("tools/list").get("tools", [])
        names = {tool.get("name") for tool in tools}
        missing = sorted(REQUIRED_TOOLS - names)
        if missing:
            raise RuntimeError("missing MCP tools: {}".format(", ".join(missing)))

        first = text_json(call_tool(mcp, "changed_regions"))
        if first["kind"] != "keyframe" or not first.get("regions"):
            raise RuntimeError("first changed_regions was not a keyframe: {}".format(first))
        screen_area = first["width"] * first["height"]

        client = launch_client(args.display, args.title)
        window = find_window(mcp, args.title, args.timeout)
        window_id = window["id"]

        call_tool(mcp, "focus", {"window": window_id})
        first_delta = next_delta(mcp, screen_area, window_id)

        screenshot_bytes = image_size(call_tool(mcp, "screenshot"))
        if screenshot_bytes <= 0:
            raise RuntimeError("screenshot returned an empty PNG payload")
        fast_delta = next_fast_delta(mcp, screen_area, window_id)

        print(
            "mcp smoke ok display={} protocol={} window=0x{:x} keyframe_regions={} delta_regions={} delta_png_b64_bytes={} delta_call_ms={} screenshot_b64_bytes={} fast_regions={} fast_b64_bytes={} fast_call_ms={} fast_total_ms={} fast_capture_ms={} fast_encode_ms={}".format(
                args.display,
                init.get("protocolVersion", "<unknown>"),
                window_id,
                len(first["regions"]),
                len(first_delta["regions"]),
                sum(len(region.get("png_base64", "")) for region in first_delta.get("regions", [])),
                first_delta.get("_call_ms"),
                screenshot_bytes,
                len(fast_delta["regions"]),
                fast_delta.get("encoded_bytes"),
                fast_delta.get("_call_ms"),
                fast_delta.get("timings", {}).get("total_ms"),
                fast_delta.get("timings", {}).get("capture_ms"),
                fast_delta.get("timings", {}).get("encode_ms"),
            )
        )
        return 0
    finally:
        if client and client.poll() is None:
            client.send_signal(signal.SIGTERM)
            try:
                client.wait(timeout=2)
            except subprocess.TimeoutExpired:
                client.kill()
        if mcp:
            mcp.close()


if __name__ == "__main__":
    sys.exit(main())
