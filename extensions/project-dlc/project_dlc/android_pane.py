#!/usr/bin/env python3
"""Headless AVD session for the in-editor Android pane.

Boots at most one emulator (windowed on macOS, -no-window elsewhere), then
streams a coarse screencap preview and recent log lines into the PTY. A second
IDE click reuses the same pane instead of launching another AVD.
"""
from __future__ import annotations
import argparse
import subprocess
import sys
import time
from pathlib import Path

# Runnable as `python android_pane.py <avd>` (DLC argv) or as a package module.
_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))
from android_sdk import emulator_env, find_adb, find_emulator  # noqa: E402


def adb() -> str:
    path = find_adb()
    if not path:
        raise SystemExit(
            "adb is not installed — set ANDROID_HOME or install platform-tools"
        )
    return path


def devices(tool: str) -> list[str]:
    env = emulator_env(tool)
    try:
        out = subprocess.check_output(
            [tool, "devices"], text=True, timeout=10, env=env
        )
    except (subprocess.SubprocessError, OSError) as exc:
        raise SystemExit(f"adb devices failed: {exc}") from exc
    serials = []
    for line in out.splitlines()[1:]:
        parts = line.split()
        if len(parts) >= 2 and parts[1] == "device":
            serials.append(parts[0])
    return serials


def ensure_avd(emulator: str, avd: str, tool: str) -> str:
    serials = devices(tool)
    emulators = [s for s in serials if s.startswith("emulator-")]
    if emulators:
        return emulators[0]
    # On macOS show the real Emulator window so the runtime is visibly functional;
    # Linux CI/headless hosts keep -no-window.
    # Always pass -adb-path so Extended Controls does not warn about missing ADB.
    args = [
        emulator,
        f"@{avd}",
        "-gpu",
        "auto",
        "-no-audio",
        "-no-boot-anim",
        "-adb-path",
        tool,
    ]
    if sys.platform != "darwin":
        args.insert(2, "-no-window")
    env = emulator_env(tool)
    print(f"starting AVD {avd}…", flush=True)
    print(f"adb={tool}", flush=True)
    subprocess.Popen(
        args,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        env=env,
    )
    deadline = time.time() + 120
    while time.time() < deadline:
        time.sleep(2)
        emulators = [s for s in devices(tool) if s.startswith("emulator-")]
        if emulators:
            print(f"device ready: {emulators[0]}", flush=True)
            return emulators[0]
        print("waiting for emulator…", flush=True)
    raise SystemExit("timed out waiting for the Android emulator")


def frame_preview(tool: str, serial: str) -> str:
    env = emulator_env(tool)
    try:
        raw = subprocess.check_output(
            [tool, "-s", serial, "exec-out", "screencap", "-p"],
            timeout=8,
            env=env,
        )
    except (subprocess.SubprocessError, OSError) as exc:
        return f"(screencap unavailable: {exc})"
    size = len(raw)
    # Coarse text preview: PNG header + size so the pane shows live activity.
    kind = "png" if raw[:8] == b"\x89PNG\r\n\x1a\n" else "raw"
    width = height = "?"
    if kind == "png" and len(raw) >= 24:
        width = int.from_bytes(raw[16:20], "big")
        height = int.from_bytes(raw[20:24], "big")
    bar = "#" * min(40, max(4, size // 8000))
    return f"frame {kind} {width}x{height} {size}B\n[{bar}]"


def recent_log(tool: str, serial: str) -> str:
    env = emulator_env(tool)
    try:
        out = subprocess.check_output(
            [tool, "-s", serial, "logcat", "-d", "-t", "8", "-v", "brief"],
            text=True,
            timeout=8,
            stderr=subprocess.DEVNULL,
            env=env,
        )
    except (subprocess.SubprocessError, OSError):
        return ""
    return out.strip()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="In-editor headless Android pane")
    parser.add_argument("avd")
    args = parser.parse_args(argv)
    emulator = find_emulator()
    if not emulator:
        print("Android emulator is not installed", file=sys.stderr)
        return 1
    tool = adb()
    serial = ensure_avd(emulator, args.avd, tool)
    print("Android AVD pane (headless). Ctrl-C closes this view.", flush=True)
    print(f"serial={serial} avd={args.avd}", flush=True)
    while True:
        print("\033[H\033[J", end="", flush=True)
        print(f"Android  {args.avd}  {serial}", flush=True)
        print(frame_preview(tool, serial), flush=True)
        print("-" * 40, flush=True)
        log = recent_log(tool, serial)
        if log:
            print(log, flush=True)
        time.sleep(1.5)


if __name__ == "__main__":
    raise SystemExit(main())
