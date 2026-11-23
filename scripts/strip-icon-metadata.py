#!/usr/bin/env python3
"""Rewrite PNGs as clean RGBA (drop caBX / tEXt / eXIf / etc.). Requires Pillow."""
from __future__ import annotations

import argparse
import struct
import sys
from pathlib import Path

from PIL import Image

KEEP = {b"IHDR", b"IDAT", b"IEND", b"PLTE", b"tRNS"}


def chunk_types(path: Path) -> list[str]:
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"not a PNG: {path}")
    i = 8
    types: list[str] = []
    while i < len(data):
        length = struct.unpack(">I", data[i : i + 4])[0]
        types.append(data[i + 4 : i + 8].decode("latin1"))
        i += 12 + length
    return types


def strip(path: Path) -> None:
    image = Image.open(path).convert("RGBA")
    clean = Image.frombytes("RGBA", image.size, image.tobytes())
    clean.save(path, format="PNG", optimize=True)
    types = chunk_types(path)
    bad = [t for t in types if t.encode("latin1") not in KEEP]
    if bad:
        # pHYs/sBIT are harmless; reject generative/attribution chunks.
        fatal = [t for t in bad if t not in {"pHYs", "sBIT", "sRGB", "gAMA"}]
        if fatal:
            raise RuntimeError(f"{path} still has chunks: {fatal}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path)
    args = parser.parse_args()
    for path in args.paths:
        if path.is_dir():
            files = sorted(path.rglob("*.png"))
        else:
            files = [path]
        for file in files:
            before = chunk_types(file)
            strip(file)
            after = chunk_types(file)
            print(f"{file}: {before} -> {after}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
