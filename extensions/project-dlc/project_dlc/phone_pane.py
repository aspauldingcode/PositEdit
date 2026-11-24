#!/usr/bin/env python3
"""In-editor phone pane: ensure a session exists, then view with blocks.

Nested IDE PTYs cannot host Kitty shared-memory graphics, so we always
use `--protocol blocks` for the dock panel.
"""
from __future__ import annotations
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


def find_phone() -> str:
    override = os.environ.get("IDE_TERMINAL_PHONE")
    if override and Path(override).is_file() and os.access(override, os.X_OK):
        return override
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "vendor" / "bin" / "terminal-phone"
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
        if (parent / "extensions" / "project-dlc").is_dir() and (parent / "Cargo.toml").is_file():
            break
    found = shutil.which("terminal-phone")
    if found:
        return found
    local = Path.home() / ".local" / "bin" / "terminal-phone"
    if local.is_file() and os.access(local, os.X_OK):
        return str(local)
    sys.stderr.write("terminal-phone not found (run scripts/install-runtime-vendors.sh)\n")
    sys.exit(1)


def main() -> int:
    session = sys.argv[1] if len(sys.argv) > 1 else "posit"
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", session):
        sys.stderr.write(f"unsafe phone session name: {session!r}\n")
        return 2
    phone = find_phone()
    # Boot/attach without opening an external viewer; ignore already-up.
    subprocess.run(
        [phone, "up", session, "--no-view"],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    os.execv(phone, [phone, "view", session, "--protocol", "blocks"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
