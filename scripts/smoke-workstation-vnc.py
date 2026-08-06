#!/usr/bin/env python3
"""Smoke-test the supervised workstation VNC overlay (#92).

Boots `wmaker-ai-workstation-vnc` detached, verifies the VNC and MCP bridge
ports are reachable, confirms the desktop-side processes stay alive, and
ensures the configured VNC password does not appear in container logs.
"""

import argparse
import json
import os
import secrets
import socket
import subprocess
import sys
import tempfile
import time


def docker_capture(argv, check=True):
    proc = subprocess.run(argv, check=False, text=True, capture_output=True)
    if check and proc.returncode != 0:
        raise RuntimeError(
            "docker command failed ({}): {}".format(
                proc.returncode, (proc.stderr or proc.stdout).strip()[-4000:]
            )
        )
    return proc.stdout.strip(), proc.stderr.strip(), proc.returncode


def wait_for_port(host, port, timeout, label):
    deadline = time.time() + timeout
    last_error = None
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=1.0):
                return
        except OSError as exc:
            last_error = exc
            time.sleep(1.0)
    raise TimeoutError(
        "timed out waiting for {} on {}:{} ({})".format(label, host, port, last_error)
    )


def inspect_running(container):
    out, _, _ = docker_capture(
        [
            "docker",
            "inspect",
            "-f",
            "{{.State.Running}}",
            container,
        ]
    )
    return out == "true"


def mapped_port(container, port_spec):
    out, _, _ = docker_capture(["docker", "port", container, port_spec])
    _, host_port = out.rsplit(":", 1)
    return int(host_port)


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
    parser.add_argument("--image", default="wmaker-ai-workstation-vnc")
    parser.add_argument("--timeout", type=float, default=60.0)
    parser.add_argument("--startup-wait", type=float, default=8.0)
    args = parser.parse_args()

    password = "wmaker-vnc-{}".format(secrets.token_hex(8))

    with tempfile.TemporaryDirectory(prefix="wmaker-workstation-vnc-profile-") as profile_dir, tempfile.TemporaryDirectory(
        prefix="wmaker-workstation-vnc-home-"
    ) as workspace_dir, tempfile.NamedTemporaryFile(
        "w", prefix="wmaker-workstation-vnc-secret-", delete=False
    ) as password_file:
        container = None
        password_path = password_file.name
        try:
            password_file.write(password + "\n")
            password_file.flush()
            os.chmod(password_path, 0o600)

            argv = [
                "docker",
                "run",
                "-d",
                "--rm",
                "-p",
                "127.0.0.1::5900",
                "-p",
                "127.0.0.1::8090",
                "-v",
                profile_dir + ":/profile",
                "-v",
                workspace_dir + ":/workspace",
                "-v",
                password_path + ":/run/secrets/wmaker-vnc-password:ro",
                "-e",
                "VNC_PASSWORD_FILE=/run/secrets/wmaker-vnc-password",
                args.image,
            ]
            print("launching:", " ".join(argv), file=sys.stderr)
            container, _, _ = docker_capture(argv)

            time.sleep(args.startup_wait)
            if not inspect_running(container):
                logs, _, _ = docker_capture(["docker", "logs", container], check=False)
                raise RuntimeError("container exited early: {}".format(logs[-4000:]))

            vnc_port = mapped_port(container, "5900/tcp")
            mcp_port = mapped_port(container, "8090/tcp")
            wait_for_port("127.0.0.1", vnc_port, args.timeout, "VNC listener")
            wait_for_port("127.0.0.1", mcp_port, args.timeout, "MCP listener")

            logs, _, _ = docker_capture(["docker", "logs", container], check=False)
            if password in logs:
                raise RuntimeError("VNC password leaked into container logs")

            # Restart the same container so its writable /tmp layer, including
            # Xvfb runtime artifacts, is retained just like an emptyDir-backed
            # Kubernetes container restart inside one Pod.
            docker_capture(["docker", "restart", container])
            time.sleep(args.startup_wait)
            if not inspect_running(container):
                logs, _, _ = docker_capture(["docker", "logs", container], check=False)
                raise RuntimeError("container exited after restart: {}".format(logs[-4000:]))
            # Docker may allocate a different host port for an ephemeral
            # publication when the container is restarted.
            vnc_port = mapped_port(container, "5900/tcp")
            mcp_port = mapped_port(container, "8090/tcp")
            try:
                wait_for_port("127.0.0.1", vnc_port, args.timeout, "restarted VNC listener")
                wait_for_port("127.0.0.1", mcp_port, args.timeout, "restarted MCP listener")
            except TimeoutError as exc:
                container_logs, _, _ = docker_capture(
                    ["docker", "logs", container], check=False
                )
                vnc_logs, _, _ = docker_capture(
                    ["docker", "exec", container, "tail", "-80", "/tmp/x11vnc.log"],
                    check=False,
                )
                raise RuntimeError(
                    "{}\ncontainer logs:\n{}\nx11vnc logs:\n{}".format(
                        exc, container_logs[-4000:], vnc_logs[-4000:]
                    )
                ) from exc

            logs, _, _ = docker_capture(["docker", "logs", container], check=False)
            if password in logs:
                raise RuntimeError("VNC password leaked into container logs after restart")

            proc_report, _, _ = docker_capture(
                [
                    "docker",
                    "exec",
                    container,
                    "sh",
                    "-lc",
                    (
                        "pgrep -a x11vnc && "
                        "pgrep -a -f '/opt/mcp-proxy/bin/mcp-proxy' && "
                        "pgrep -a -f '/usr/local/bin/wmaker-ai-workstation'"
                    ),
                ]
            )

            print(
                json.dumps(
                    {
                        "container": container,
                        "image": args.image,
                        "vnc_port": vnc_port,
                        "mcp_port": mcp_port,
                        "restart_verified": True,
                        "processes": proc_report.splitlines(),
                    },
                    indent=2,
                )
            )
            return 0
        finally:
            if container:
                docker_capture(["docker", "rm", "-f", container], check=False)
            restore_mount_permissions(args.image, profile_dir, workspace_dir)
            try:
                os.unlink(password_path)
            except FileNotFoundError:
                pass


if __name__ == "__main__":
    sys.exit(main())
