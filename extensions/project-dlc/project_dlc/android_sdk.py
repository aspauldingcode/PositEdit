"""Resolve Android SDK root + ADB for emulator launches.

The Emulator UI warns when it cannot find ADB relative to the SDK. Prefer
`$ANDROID_HOME/platform-tools/adb` (or SDK_ROOT) over a random PATH entry
(e.g. a Nix-wrapped adb), and always pass `-adb-path` when starting AVDs.
"""
from __future__ import annotations

import os
import shutil
from pathlib import Path


def sdk_roots() -> list[Path]:
    roots: list[Path] = []
    for key in ("ANDROID_HOME", "ANDROID_SDK_ROOT"):
        value = os.environ.get(key)
        if value:
            roots.append(Path(value).expanduser())
    roots += [
        Path.home() / "Library" / "Android" / "sdk",
        Path.home() / "Android" / "Sdk",
    ]
    # De-dupe while preserving order.
    seen: set[Path] = set()
    out: list[Path] = []
    for root in roots:
        try:
            key = root.resolve()
        except OSError:
            key = root
        if key in seen:
            continue
        seen.add(key)
        out.append(root)
    return out


def find_sdk_root() -> Path | None:
    for root in sdk_roots():
        if (root / "platform-tools").is_dir() or (root / "emulator" / "emulator").is_file():
            return root
    return None


def find_emulator() -> str | None:
    found = shutil.which("emulator")
    if found:
        return found
    for root in sdk_roots():
        candidate = root / "emulator" / "emulator"
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
    return None


def find_adb() -> str | None:
    """Prefer SDK platform-tools ADB so the Emulator Extended Controls detect it."""
    override = os.environ.get("IDE_ADB") or os.environ.get("ANDROID_ADB")
    if override:
        path = Path(override).expanduser()
        if path.is_file() and os.access(path, os.X_OK):
            return str(path.resolve())
    for root in sdk_roots():
        candidate = root / "platform-tools" / "adb"
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate.resolve())
    found = shutil.which("adb")
    return found


def emulator_env(adb: str | None = None) -> dict[str, str]:
    """Environment for emulator / adb child processes."""
    env = os.environ.copy()
    sdk = find_sdk_root()
    if sdk is not None:
        sdk_s = str(sdk)
        env.setdefault("ANDROID_HOME", sdk_s)
        env.setdefault("ANDROID_SDK_ROOT", sdk_s)
        platform_tools = str(sdk / "platform-tools")
        emulator_dir = str(sdk / "emulator")
        path = env.get("PATH", "")
        env["PATH"] = os.pathsep.join(
            [platform_tools, emulator_dir] + ([path] if path else [])
        )
    tool = adb or find_adb()
    if tool:
        env["ANDROID_ADB"] = tool
    return env
