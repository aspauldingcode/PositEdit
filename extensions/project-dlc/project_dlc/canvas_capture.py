#!/usr/bin/env python3
"""Canvas capture worker: PNG + meta.json for the host Kitty interactive Canvas.

Writes frames for the IDE to paint and map clicks into. Does not use half-blocks.

Performance rules:
- Navigate / phone-up / viewport / describe only when needed (not every poll).
- Skip rewriting the frame when the PNG bytes are unchanged.
- Idle interval is intentionally slow; a nudge file speeds up after interaction.
"""
from __future__ import annotations
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# Idle capture cadence. Interaction sets a nudge file for one faster pass.
BROWSER_IDLE_S = 2.0
PHONE_IDLE_S = 1.5
NUDGE_S = 0.35


def which(*names: str) -> str | None:
    for name in names:
        found = shutil.which(name)
        if found:
            return found
    return None


def vendor_bin(name: str) -> str | None:
    here = Path(__file__).resolve()
    for parent in here.parents:
        candidate = parent / "vendor" / "bin" / name
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
        if (parent / "extensions" / "project-dlc").is_dir() and (parent / "Cargo.toml").is_file():
            break
    return None


def find_tool(name: str, env_key: str) -> str | None:
    override = os.environ.get(env_key)
    if override and Path(override).is_file() and os.access(override, os.X_OK):
        return override
    return vendor_bin(name) or which(name)


def png_size(path: Path) -> tuple[int, int]:
    try:
        data = path.read_bytes()
        if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n":
            return (0, 0)
        w, h = struct.unpack(">II", data[16:24])
        return (int(w), int(h))
    except Exception:
        return (0, 0)


def write_meta(
    meta: Path,
    kind: str,
    target: str,
    png: Path,
    *,
    scale: float = 1.0,
    coordinate_space: str = "css-pixels",
    coord_width: int | None = None,
    coord_height: int | None = None,
    native_scale: float | None = None,
) -> None:
    w, h = png_size(png)
    body: dict = {
        "kind": kind,
        "target": target,
        "image_width": w,
        "image_height": h,
        # Action mapping divisor. Phone unframed PNGs are already display-points → 1.0.
        "scale": scale,
        "coordinate_space": coordinate_space,
        "coord_width": int(coord_width or w),
        "coord_height": int(coord_height or h),
    }
    if native_scale is not None:
        body["native_scale"] = native_scale
    meta.parent.mkdir(parents=True, exist_ok=True)
    meta.write_text(json.dumps(body) + "\n")


def atomic_replace_if_changed(src: Path, dest: Path) -> bool:
    """Replace dest only when bytes differ. Returns True when dest changed."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    new_bytes = src.read_bytes()
    if not new_bytes:
        return False
    if dest.is_file():
        try:
            if dest.read_bytes() == new_bytes:
                return False
        except Exception:
            pass
    tmp = dest.with_suffix(dest.suffix + ".tmp")
    tmp.write_bytes(new_bytes)
    tmp.replace(dest)
    return True


def read_geom() -> dict:
    """Canvas pane geometry from the IDE (not the full terminal window)."""
    raw = os.environ.get("POSIT_CANVAS_GEOM") or ""
    if not raw:
        return {}
    path = Path(raw)
    try:
        data = json.loads(path.read_text())
        if isinstance(data, dict):
            return data
    except Exception:
        pass
    return {}


def nudge_path() -> Path | None:
    raw = os.environ.get("POSIT_CANVAS_NUDGE") or ""
    return Path(raw) if raw else None


def consume_nudge() -> bool:
    path = nudge_path()
    if path is None:
        return False
    try:
        if path.is_file():
            path.unlink(missing_ok=True)
            return True
    except Exception:
        return True
    return False


_last_viewport: tuple[int, int] | None = None
_browser_opened_for: str | None = None
_phone_up_for: str | None = None


def apply_browser_viewport(browser: str) -> None:
    global _last_viewport
    geom = read_geom()
    try:
        width = int(geom.get("pixel_width") or 0)
        height = int(geom.get("pixel_height") or 0)
    except Exception:
        return
    if width < 160 or height < 120:
        return
    if _last_viewport == (width, height):
        return
    subprocess.run(
        [browser, "set", "viewport", str(width), str(height)],
        check=False,
        timeout=10,
        capture_output=True,
    )
    _last_viewport = (width, height)


def capture_browser(url: str, dest: Path, meta: Path, *, navigate: bool) -> bool:
    """Screenshot the live agent-browser session."""
    global _browser_opened_for
    browser = which("agent-browser")
    if not browser:
        return False
    try:
        if navigate or _browser_opened_for != url:
            subprocess.run(
                [browser, "open", url], check=False, timeout=25, capture_output=True
            )
            _browser_opened_for = url
        apply_browser_viewport(browser)
        shot = Path(tempfile.gettempdir()) / f"posit-canvas-browser-{os.getpid()}.png"
        subprocess.run(
            [browser, "screenshot", str(shot)],
            check=True,
            timeout=25,
            capture_output=True,
        )
        if shot.is_file() and shot.stat().st_size > 0:
            if atomic_replace_if_changed(shot, dest):
                write_meta(
                    meta,
                    "browser",
                    url,
                    dest,
                    scale=1.0,
                    coordinate_space="css-pixels",
                )
            return True
    except Exception:
        return False
    return False


def ensure_phone_up(phone: str, session: str) -> None:
    global _phone_up_for
    if _phone_up_for == session:
        return
    subprocess.run(
        [phone, "up", session, "--no-view"],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        timeout=40,
    )
    _phone_up_for = session


def capture_phone(session: str, dest: Path, meta: Path) -> bool:
    """Screenshot via terminal-phone; meta matches its display-points contract."""
    phone = find_tool("terminal-phone", "IDE_TERMINAL_PHONE")
    if not phone:
        return False
    ensure_phone_up(phone, session)
    shot = Path(tempfile.gettempdir()) / f"posit-canvas-phone-{os.getpid()}.png"
    try:
        proc = subprocess.run(
            [phone, "action", session, "screenshot", "--output", str(shot)],
            check=False,
            capture_output=True,
            text=True,
            timeout=40,
        )
        if proc.returncode != 0 or not shot.is_file() or shot.stat().st_size <= 0:
            return False
        coord_w = None
        coord_h = None
        native_scale = None
        try:
            info = json.loads(proc.stdout or "{}")
            if isinstance(info, dict) and info.get("ok"):
                # terminal-phone: width/height are display-points; PNG matches.
                if isinstance(info.get("width"), (int, float)):
                    coord_w = int(info["width"])
                if isinstance(info.get("height"), (int, float)):
                    coord_h = int(info["height"])
                if isinstance(info.get("scale"), (int, float)):
                    native_scale = float(info["scale"])
        except Exception:
            pass
        if atomic_replace_if_changed(shot, dest):
            write_meta(
                meta,
                "phone",
                session,
                dest,
                scale=1.0,
                coordinate_space="display-points",
                coord_width=coord_w,
                coord_height=coord_h,
                native_scale=native_scale,
            )
        return True
    except Exception:
        return False
    return False


def main() -> int:
    kind = os.environ.get("POSIT_CANVAS_KIND") or (sys.argv[1] if len(sys.argv) > 1 else "")
    frame = Path(os.environ.get("POSIT_CANVAS_FRAME") or "")
    meta = Path(os.environ.get("POSIT_CANVAS_META") or (str(frame) + ".json" if frame else ""))
    if not kind or not str(frame):
        sys.stderr.write("POSIT_CANVAS_KIND and POSIT_CANVAS_FRAME are required\n")
        return 2
    target = sys.argv[2] if len(sys.argv) > 2 else ""
    if kind == "browser":
        url = target or "https://example.com"
        first = True
        while True:
            started = time.monotonic()
            capture_browser(url, frame, meta, navigate=first)
            first = False
            # Don't stack captures if screenshot already took longer than the interval.
            elapsed = time.monotonic() - started
            if consume_nudge():
                time.sleep(max(0.0, NUDGE_S - elapsed))
            else:
                time.sleep(max(0.0, BROWSER_IDLE_S - elapsed))
    if kind == "phone":
        session = target or "posit"
        if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", session):
            sys.stderr.write(f"unsafe session: {session!r}\n")
            return 2
        while True:
            started = time.monotonic()
            capture_phone(session, frame, meta)
            elapsed = time.monotonic() - started
            if consume_nudge():
                time.sleep(max(0.0, NUDGE_S - elapsed))
            else:
                time.sleep(max(0.0, PHONE_IDLE_S - elapsed))
    sys.stderr.write(f"unknown canvas kind: {kind}\n")
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(0)
