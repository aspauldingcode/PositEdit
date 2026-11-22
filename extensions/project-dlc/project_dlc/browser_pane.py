#!/usr/bin/env python3
"""In-editor browser pane: fetch a page, screenshot when possible, paint half-blocks.

Runs inside the IDE runtime dock PTY. Prefers agent-browser for Chromium
screenshots; falls back to a text extract so the panel is never empty.
"""
from __future__ import annotations
import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def which(*names: str) -> str | None:
    for name in names:
        found = shutil.which(name)
        if found:
            return found
    return None


def halfblocks(png: Path, cols: int, rows: int) -> list[str]:
    """Coarse RGB → Unicode half-block rows. Soft-fail to []."""
    try:
        from PIL import Image  # type: ignore
    except Exception:
        # Pillow optional; try sips → ppm via raw bytes is too heavy — skip art.
        return []
    try:
        img = Image.open(png).convert("RGB")
    except Exception:
        return []
    cols = max(8, cols)
    rows = max(4, rows)
    # Two vertical pixels per character cell.
    target_h = rows * 2
    img = img.resize((cols, target_h))
    px = img.load()
    lines: list[str] = []
    for y in range(0, target_h, 2):
        parts: list[str] = []
        for x in range(cols):
            top = px[x, y]
            bot = px[x, min(y + 1, target_h - 1)]
            parts.append(
                f"\x1b[38;2;{top[0]};{top[1]};{top[2]}m"
                f"\x1b[48;2;{bot[0]};{bot[1]};{bot[2]}m▀\x1b[0m"
            )
        lines.append("".join(parts))
    return lines


def text_snapshot(url: str) -> list[str]:
    browser = which("agent-browser")
    if browser:
        try:
            out = subprocess.check_output(
                [browser, "read", url], text=True, timeout=20, stderr=subprocess.DEVNULL
            )
            lines = [line.rstrip() for line in out.splitlines() if line.strip()]
            return lines[:80] or [f"(empty read for {url})"]
        except Exception as exc:
            return [f"agent-browser read failed: {exc}"]
    try:
        import urllib.request

        with urllib.request.urlopen(url, timeout=8) as resp:  # noqa: S310
            raw = resp.read(8000).decode("utf-8", "replace")
        # Strip tags coarsely.
        import re

        text = re.sub(r"<[^>]+>", " ", raw)
        text = re.sub(r"\s+", " ", text).strip()
        return [text[i : i + 72] for i in range(0, min(len(text), 2000), 72)] or [url]
    except Exception as exc:
        return [f"fetch failed: {exc}"]


def screenshot(url: str, dest: Path) -> bool:
    browser = which("agent-browser")
    if not browser:
        return False
    try:
        subprocess.run([browser, "open", url], check=False, timeout=25, capture_output=True)
        subprocess.run(
            [browser, "screenshot", str(dest)],
            check=True,
            timeout=25,
            capture_output=True,
        )
        return dest.is_file() and dest.stat().st_size > 0
    except Exception:
        return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url", nargs="?", default="https://example.com")
    parser.add_argument("--cols", type=int, default=int(os.environ.get("COLUMNS", "60")))
    parser.add_argument("--rows", type=int, default=int(os.environ.get("LINES", "24")))
    args = parser.parse_args()
    url = args.url
    cols = max(20, args.cols - 2)
    rows = max(8, args.rows - 4)
    print("browser pane (in-editor)", flush=True)
    print(f"url={url}", flush=True)
    tmp = Path(tempfile.gettempdir()) / f"posit-browser-pane-{os.getpid()}.png"
    frame = 0
    while True:
        lines: list[str] = []
        if screenshot(url, tmp):
            lines = halfblocks(tmp, cols, rows - 3)
        if not lines:
            lines = text_snapshot(url)[: rows - 3]
        # Clear and paint.
        sys.stdout.write("\x1b[H\x1b[J")
        print(f"Browser  {url}  frame {frame}", flush=True)
        print("-" * min(cols, 72), flush=True)
        for line in lines:
            print(line[: cols * 20], flush=True)  # ANSI may be wider than cols
        frame += 1
        time.sleep(2.5)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(0)
