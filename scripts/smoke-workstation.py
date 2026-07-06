#!/usr/bin/env python3
"""Smoke-test the opt-in workstation profile (#90).

Boots `wmaker-ai-workstation`, drives deterministic launchers through MCP, and
verifies the workstation app contract plus the `/profile` and `/workspace`
mount persistence split.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
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


def docker_capture(argv, check=True):
    proc = subprocess.run(argv, check=False, text=True, capture_output=True)
    if check and proc.returncode != 0:
        raise RuntimeError(
            "docker command failed ({}): {}".format(
                proc.returncode, (proc.stderr or proc.stdout).strip()[-4000:]
            )
        )
    return proc.stdout.strip(), proc.stderr.strip(), proc.returncode


def wait_for_window(mcp, predicate, timeout, label):
    deadline = time.time() + timeout
    last = []
    while time.time() < deadline:
        windows = text_json(call(mcp, "list_windows")).get("windows", [])
        last = windows
        for window in windows:
            if predicate(window):
                return window
        time.sleep(1.0)
    raise TimeoutError(
        "timed out waiting for {} window; last titles={}".format(
            label, [window.get("title", "") for window in last]
        )
    )


def route_command(mcp, text, wait_ms=1500):
    return text_json(
        call(
            mcp,
            "route_command",
            {
                "text": text,
                "source": "typed",
                "wait_ms": wait_ms,
            },
        )
    )


def lower_window_fields(window):
    values = [
        window.get("title", ""),
        window.get("class", "") or "",
        window.get("instance", "") or "",
    ]
    return " ".join(values).lower()


def run_blender_checks(image, workspace_dir):
    version_out, _, _ = docker_capture(
        ["docker", "run", "--rm", image, "blender", "--version"]
    )
    render_out, _, _ = docker_capture(
        [
            "docker",
            "run",
            "--rm",
            "-v",
            workspace_dir + ":/workspace",
            "-e",
            "WMAKER_BLENDER_OUTPUT_DIR=/workspace/blender",
            image,
            "sh",
            "-lc",
            (
                "mkdir -p /workspace/blender "
                "&& blender --background --python /usr/share/wmaker-ng/blender-cylinder.py "
                "&& identify -format '%m %wx%h\\n' /workspace/blender/cylinder.png "
                "&& file /workspace/blender/cylinder.blend"
            ),
        ]
    )
    return {
        "version_first_line": next(
            (line for line in version_out.splitlines() if line.startswith("Blender ")),
            "",
        ),
        "render_report": render_out.splitlines()[-2:],
    }


def run_media_checks(image, workspace_dir):
    media_out, _, _ = docker_capture(
        [
            "docker",
            "run",
            "--rm",
            "-v",
            workspace_dir + ":/workspace",
            image,
            "sh",
            "-lc",
            (
                "ffmpeg -loglevel error -f lavfi -i color=c=red:s=16x16:d=1 -frames:v 1 /workspace/ffmpeg.png "
                "&& if command -v magick >/dev/null 2>&1; then "
                "magick /workspace/ffmpeg.png -resize 8x8 /workspace/magick.png; "
                "else convert /workspace/ffmpeg.png -resize 8x8 /workspace/magick.png; fi "
                "&& identify -format '%m %wx%h' /workspace/magick.png"
            ),
        ]
    )
    return media_out.splitlines()[-1] if media_out else ""


def run_persistence_checks(image, profile_dir, workspace_dir):
    browser_marker = os.path.join(profile_dir, "wmaker-browser-marker.txt")
    terminalrc = os.path.join(
        workspace_dir, "home", ".config", "xfce4", "terminal", "terminalrc"
    )
    init_cmd = (
        "mkdir -p /profile \"$HOME/.config/xfce4/terminal\" "
        "&& printf 'browser-marker\\n' >/profile/wmaker-browser-marker.txt "
        "&& printf '[Configuration]\\nColorBackground=#000000\\n' >"
        "\"$HOME/.config/xfce4/terminal/terminalrc\""
    )
    verify_cmd = (
        "test -f /profile/wmaker-browser-marker.txt "
        "&& grep -q browser-marker /profile/wmaker-browser-marker.txt "
        "&& test -f \"$HOME/.config/xfce4/terminal/terminalrc\" "
        "&& grep -q ColorBackground \"$HOME/.config/xfce4/terminal/terminalrc\""
    )
    base_argv = [
        "docker",
        "run",
        "--rm",
        "-e",
        "HOME=/workspace/home",
        "-e",
        "XDG_CONFIG_HOME=/workspace/home/.config",
        "-v",
        profile_dir + ":/profile",
        "-v",
        workspace_dir + ":/workspace",
        image,
        "sh",
        "-lc",
    ]
    docker_capture(base_argv + [init_cmd])
    docker_capture(base_argv + [verify_cmd])
    return {
        "browser_marker": browser_marker,
        "terminalrc": terminalrc,
    }


def verify_terminal_nonce(workspace_dir, terminal_nonce):
    nonce_file = os.path.join(workspace_dir, "terminal", "nonce.txt")
    deadline = time.time() + 20.0
    while time.time() < deadline:
        if os.path.exists(nonce_file):
            with open(nonce_file, "r", encoding="utf-8") as handle:
                observed = handle.read().strip()
            if observed != terminal_nonce:
                raise RuntimeError(
                    "terminal nonce mismatch: expected {!r}, got {!r}".format(
                        terminal_nonce, observed
                    )
                )
            return nonce_file
        time.sleep(0.5)
    raise TimeoutError("timed out waiting for terminal nonce file {}".format(nonce_file))


def restore_mount_permissions(image, profile_dir, workspace_dir):
    docker_capture(
        [
            "docker",
            "run",
            "--rm",
            "--entrypoint",
            "sh",
            "-v",
            profile_dir + ":/profile",
            "-v",
            workspace_dir + ":/workspace",
            image,
            "-lc",
            "chmod -R a+rwX /profile /workspace",
        ],
        check=False,
    )


def main():
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip())
    parser.add_argument("--image", default="wmaker-ai-workstation")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--render-wait", type=float, default=12.0)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="wmaker-workstation-profile-") as profile_dir, tempfile.TemporaryDirectory(
        prefix="wmaker-workstation-home-"
    ) as workspace_dir:
        terminal_nonce = "wmaker-nonce-{}".format(int(time.time()))
        terminal_title = "wmaker terminal {}".format(terminal_nonce)
        argv = [
            "docker",
            "run",
            "-i",
            "--rm",
            "-v",
            profile_dir + ":/profile",
            "-v",
            workspace_dir + ":/workspace",
            "-e",
            "START_URL=about:blank",
            "-e",
            "WMAKER_TERMINAL_NONCE=" + terminal_nonce,
            "-e",
            "WMAKER_TERMINAL_NONCE_FILE=/workspace/terminal/nonce.txt",
            "-e",
            "WMAKER_TERMINAL_TITLE=" + terminal_title,
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
                    "clientInfo": {"name": "workstation-smoke", "version": "0"},
                },
            )
            mcp.notify("notifications/initialized")
            time.sleep(args.render_wait)

            terminal_result = route_command(mcp, "open terminal")
            terminal_window = wait_for_window(
                mcp,
                lambda window: terminal_title.lower() in lower_window_fields(window),
                args.timeout,
                "terminal",
            )
            terminal_nonce_file = verify_terminal_nonce(workspace_dir, terminal_nonce)

            browser_result = route_command(mcp, "open browser")
            browser_window = wait_for_window(
                mcp,
                lambda window: "brave" in lower_window_fields(window),
                args.timeout,
                "brave",
            )

            chrome_result = route_command(mcp, "open chrome")
            chrome_window = wait_for_window(
                mcp,
                lambda window: "chromium" in lower_window_fields(window),
                args.timeout,
                "chromium",
            )

            blender_result = route_command(mcp, "open blender", wait_ms=2500)
            blender_window = wait_for_window(
                mcp,
                lambda window: "blender" in lower_window_fields(window),
                args.timeout,
                "blender",
            )

            libreoffice_result = route_command(mcp, "open libreoffice", wait_ms=2500)
            libreoffice_window = wait_for_window(
                mcp,
                lambda window: "libreoffice" in lower_window_fields(window)
                or "writer" in lower_window_fields(window),
                args.timeout,
                "libreoffice",
            )

            gimp_result = route_command(mcp, "open gimp", wait_ms=2500)
            gimp_window = wait_for_window(
                mcp,
                lambda window: "gimp" in lower_window_fields(window),
                args.timeout,
                "gimp",
            )

            inkscape_result = route_command(mcp, "open inkscape", wait_ms=2500)
            inkscape_window = wait_for_window(
                mcp,
                lambda window: "inkscape" in lower_window_fields(window),
                args.timeout,
                "inkscape",
            )

            accessibility = text_json(
                call(
                    mcp,
                    "accessibility_tree",
                    {"max_depth": 2, "max_children_per_node": 16},
                )
            )
            if not accessibility.get("available"):
                raise RuntimeError("AT-SPI tree unavailable: {}".format(accessibility))
            if accessibility.get("node_count", 0) <= 0:
                raise RuntimeError("AT-SPI returned no nodes: {}".format(accessibility))

            blender_checks = run_blender_checks(args.image, workspace_dir)
            media_report = run_media_checks(args.image, workspace_dir)
            persistence = run_persistence_checks(args.image, profile_dir, workspace_dir)

            report = {
                "protocol": init.get("protocolVersion", "?"),
                "terminal": {
                    "result_status": terminal_result["result"]["status"],
                    "title": terminal_window.get("title"),
                    "nonce_file": terminal_nonce_file,
                },
                "brave": {
                    "result_status": browser_result["result"]["status"],
                    "title": browser_window.get("title"),
                },
                "chromium": {
                    "result_status": chrome_result["result"]["status"],
                    "title": chrome_window.get("title"),
                },
                "blender": {
                    "result_status": blender_result["result"]["status"],
                    "title": blender_window.get("title"),
                    "version": blender_checks["version_first_line"],
                    "render_report": blender_checks["render_report"],
                },
                "libreoffice": {
                    "result_status": libreoffice_result["result"]["status"],
                    "title": libreoffice_window.get("title"),
                },
                "gimp": {
                    "result_status": gimp_result["result"]["status"],
                    "title": gimp_window.get("title"),
                },
                "inkscape": {
                    "result_status": inkscape_result["result"]["status"],
                    "title": inkscape_window.get("title"),
                },
                "accessibility": {
                    "available": accessibility.get("available"),
                    "node_count": accessibility.get("node_count"),
                },
                "media_transform": media_report,
                "persistence": persistence,
            }
            print(json.dumps(report, indent=2))
            return 0
        finally:
            mcp.close()
            restore_mount_permissions(args.image, profile_dir, workspace_dir)


if __name__ == "__main__":
    sys.exit(main())
