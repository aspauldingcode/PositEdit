#!/usr/bin/env python3
"""Resolve host tools, or the pinned Nix devShell when the host tool is incomplete.

Does not install packages, change Nix profiles, or invent a successful tool run.
"""
import shutil

NIX_PREFIX = [
    "nix",
    "--extra-experimental-features",
    "nix-command flakes",
    "develop",
    "-c",
]
# Tools declared by flake.nix devShells.default. Other commands stay on PATH.
NIX_TOOLS = {"bend", "cargo", "rustc", "rustfmt", "cargo-clippy", "tmux"}


def wrap(argv, which=shutil.which):
    """Return argv, prefixed with `nix develop -c` when the host cannot run it."""
    argv = list(argv)
    if not argv:
        return argv
    tool = argv[0]
    if which(tool) is None:
        if tool in NIX_TOOLS and which("nix") is not None:
            return NIX_PREFIX + argv
        return argv
    # A cargo binary without rustc cannot compile. The devShell ships both.
    if tool == "cargo" and which("rustc") is None and which("nix") is not None:
        return NIX_PREFIX + argv
    return argv
