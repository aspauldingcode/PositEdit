#!/usr/bin/env python3
"""Basic end-to-end PTY/tmux smoke check. Missing tmux is not success."""
import importlib.util
import pathlib
import shutil
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("axiom_toolchain", ROOT / "scripts" / "toolchain.py")
toolchain = importlib.util.module_from_spec(spec)
spec.loader.exec_module(toolchain)


def run(cmd, **kwargs):
    wrapped = toolchain.wrap(cmd)
    if not wrapped or shutil.which(wrapped[0]) is None:
        sys.exit("BLOCKED: missing executable " + (cmd[0] if cmd else "<empty>"))
    return subprocess.run(wrapped, cwd=ROOT, **kwargs)


if shutil.which("tmux") is None and shutil.which("nix") is None:
    sys.exit("BLOCKED: tmux not available")
if shutil.which("cargo") is None and shutil.which("nix") is None:
    sys.exit("BLOCKED: cargo not available")

proc = run(["cargo", "build", "-p", "ide"], timeout=900)
if proc.returncode:
    sys.exit(proc.returncode)
binary = ROOT / "target" / "debug" / "posit-edit"
session = "posit-test-" + uuid.uuid4().hex[:8]
try:
    run(["tmux", "new-session", "-d", "-s", session, "-x", "90", "-y", "30", str(binary)], check=True)
    time.sleep(1.2)
    cap = run(["tmux", "capture-pane", "-pt", session], capture_output=True, text=True, check=True).stdout
    if "Posit" not in cap:
        sys.exit("FAIL: editor not visible in actual tmux capture\n" + cap)
    run(["tmux", "send-keys", "-t", session, "C-q"], check=True)
    print("PASS: interactive tmux capture contained Posit and Ctrl-Q was sent")
finally:
    run(["tmux", "kill-session", "-t", session], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
