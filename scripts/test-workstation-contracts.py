#!/usr/bin/env python3
"""Dependency-free regression tests for workstation restart/profile contracts."""

from __future__ import annotations

import os
import pathlib
import subprocess
import tempfile
import time


ROOT = pathlib.Path(__file__).resolve().parents[1]
ENTRYPOINT = ROOT / "sandbox/workstation/entrypoint.sh"
BROWSER_LAUNCH = ROOT / "sandbox/browser/launch.sh"


def executable(path: pathlib.Path, body: str) -> None:
    path.write_text(body, encoding="utf-8")
    path.chmod(0o755)


def test_stale_x_runtime_is_removed() -> None:
    with tempfile.TemporaryDirectory() as temp:
        root = pathlib.Path(temp)
        (root / ".X11-unix").mkdir()
        (root / ".X99-lock").write_text("999999\n", encoding="utf-8")
        (root / ".X11-unix/X99").write_text("stale", encoding="utf-8")
        fake_bin = root / "bin"
        fake_bin.mkdir()
        marker = root / "entrypoint-ran"
        executable(
            fake_bin / "wmaker-headless",
            f"#!/bin/sh\nprintf ran >'{marker}'\n",
        )
        installed = pathlib.Path("/usr/local/bin/wmaker-headless")
        script = ENTRYPOINT.read_text(encoding="utf-8").replace(
            str(installed), str(fake_bin / "wmaker-headless")
        )
        wrapper = root / "entrypoint.sh"
        executable(wrapper, script)
        subprocess.run(
            [str(wrapper)],
            env={**os.environ, "DISPLAY": ":99", "WMAKER_X_RUNTIME_DIR": str(root)},
            check=True,
        )
        assert marker.exists()
        assert not (root / ".X99-lock").exists()
        assert not (root / ".X11-unix/X99").exists()


def test_live_x_lock_is_preserved() -> None:
    with tempfile.TemporaryDirectory() as temp:
        root = pathlib.Path(temp)
        lock = root / ".X99-lock"
        lock.write_text(f"{os.getpid()}\n", encoding="utf-8")
        result = subprocess.run(
            [str(ENTRYPOINT)],
            env={**os.environ, "DISPLAY": ":99", "WMAKER_X_RUNTIME_DIR": str(root)},
            text=True,
            capture_output=True,
            check=False,
        )
        assert result.returncode == 70, result.stderr
        assert lock.exists()


def run_browser(fake_name: str, extra_env: dict[str, str] | None = None) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    with tempfile.TemporaryDirectory() as temp:
        root = pathlib.Path(temp)
        bin_dir = root / "bin"
        bin_dir.mkdir()
        args_file = root / "browser-args"
        executable(
            bin_dir / fake_name,
            f"#!/bin/sh\nprintf '%s\\n' \"$@\" >'{args_file}'\n",
        )
        executable(bin_dir / "ai-mcp", "#!/bin/sh\nsleep 0.1\n")
        profile = root / "profile"
        env = {
            **os.environ,
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "DISPLAY": ":99",
            "BROWSER": fake_name,
            "USER_DATA_DIR": str(profile),
        }
        if extra_env:
            env.update(extra_env)
        result = subprocess.run(
            ["/bin/sh", str(BROWSER_LAUNCH)],
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )
        for _ in range(20):
            if args_file.exists():
                break
            time.sleep(0.02)
        args = args_file.read_text(encoding="utf-8").splitlines() if args_file.exists() else []
        return result, args


def test_firefox_uses_native_stable_profile() -> None:
    result, args = run_browser("firefox")
    assert result.returncode == 0, result.stderr
    assert "-profile" in args
    profile = pathlib.Path(args[args.index("-profile") + 1])
    assert profile.name == "firefox"
    assert not any(arg.startswith("--user-data-dir") for arg in args)


def test_firefox_rejects_profile_outside_root() -> None:
    result, _ = run_browser("firefox", {"FIREFOX_PROFILE_DIR": "/outside/profile"})
    assert result.returncode == 64
    assert "must be inside USER_DATA_DIR" in result.stderr


def test_chromium_keeps_user_data_dir() -> None:
    result, args = run_browser("chromium")
    assert result.returncode == 0, result.stderr
    assert any(arg.startswith("--user-data-dir=") for arg in args)


def main() -> None:
    tests = [value for name, value in globals().items() if name.startswith("test_")]
    for test in sorted(tests, key=lambda item: item.__name__):
        test()
        print(f"ok: {test.__name__}")


if __name__ == "__main__":
    main()
