#!/usr/bin/env python3
"""Measure binary size, startup, and idle RSS. Records only observed numbers."""
import json
import os
import statistics
import subprocess
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "engineering" / "evidence" / "runtime"
TARGET = ROOT / "target" / "perf"
NIX = ["nix", "--extra-experimental-features", "nix-command flakes", "develop", "-c"]


def run(argv, timeout=180, env=None, cwd=None):
    proc = subprocess.run(
        argv,
        cwd=cwd or ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    return proc.returncode, proc.stdout.decode("utf-8", "replace")


def percentile(samples, pct):
    ordered = sorted(samples)
    if not ordered:
        return None
    index = max(0, min(len(ordered) - 1, int(round((pct / 100.0) * (len(ordered) - 1)))))
    return ordered[index]


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(TARGET)
    rc, build_log = run(NIX + ["cargo", "build", "-p", "ide", "--release", "--offline"], timeout=1800, env=env)
    (OUT / "performance-build.log").write_text(build_log, encoding="utf-8")
    binary = TARGET / "release" / "posit-edit"
    if rc != 0 or not binary.is_file():
        report = {
            "observed": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "status": "fail",
            "detail": "release build failed",
            "build_log": "engineering/evidence/runtime/performance-build.log",
        }
        (OUT / "performance.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print("FAIL release build")
        return 1

    _, uname = run(["uname", "-a"])
    _, cpu = run(["sysctl", "-n", "machdep.cpu.brand_string"])
    _, mem = run(["sysctl", "-n", "hw.memsize"])
    _, ncpu = run(["sysctl", "-n", "hw.ncpu"])
    _, rustc = run(NIX + ["rustc", "-vV"], timeout=60)

    size = binary.stat().st_size
    stripped = None
    stripped_log = ""
    copy = TARGET / "release" / "posit-edit.stripped"
    copy.write_bytes(binary.read_bytes())
    strip_rc, stripped_log = run(["strip", "-x", str(copy)], timeout=30)
    if strip_rc == 0 and copy.is_file():
        stripped = copy.stat().st_size

    samples = []
    for _ in range(7):
        started = time.perf_counter()
        ver_rc, ver_text = run([str(binary), "--version"], timeout=20)
        elapsed = time.perf_counter() - started
        if ver_rc != 0 or "posit-edit" not in ver_text:
            samples = []
            break
        samples.append(elapsed)

    rss_samples = []
    rss_log = ""
    session = "posit-perf-" + uuid.uuid4().hex[:8]
    sample = OUT / "samples" / "add.py"
    start_rc, start_log = run(
        NIX + ["tmux", "new-session", "-d", "-s", session, "-x", "90", "-y", "30", str(binary), str(sample)],
        timeout=60,
    )
    rss_log += start_log
    if start_rc == 0:
        time.sleep(1.2)
        for _ in range(5):
            pane_rc, pane = run(NIX + ["tmux", "list-panes", "-t", session, "-F", "#{pane_pid}"], timeout=20)
            rss_log += pane
            pid = pane.strip().splitlines()[-1].strip() if pane_rc == 0 and pane.strip() else ""
            if pid.isdigit():
                ps_rc, ps = run(["ps", "-o", "pid=,ppid=,rss=,command=", "-ax"], timeout=10)
                rows = []
                for line in ps.splitlines():
                    parts = line.split(None, 3)
                    if len(parts) >= 4 and (parts[0] == pid or parts[1] == pid) and "posit-edit" in parts[3]:
                        rows.append(int(parts[2]))
                if rows:
                    rss_samples.append(max(rows))
            time.sleep(0.3)
        run(NIX + ["tmux", "kill-session", "-t", session], timeout=20)
    (OUT / "performance-rss.log").write_text(rss_log, encoding="utf-8")

    binary_budget = 10 * 1024 * 1024
    rss_budget = 30 * 1024
    over = []
    compared = stripped if stripped is not None else size
    if compared > binary_budget:
        over.append("binary")
    if rss_samples and statistics.median(rss_samples) > rss_budget:
        over.append("idle_rss")

    report = {
        "observed": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "status": "observed",
        "host": {
            "uname": uname.strip(),
            "cpu": cpu.strip(),
            "memory_bytes": int(mem.strip()) if mem.strip().isdigit() else mem.strip(),
            "ncpu": int(ncpu.strip()) if ncpu.strip().isdigit() else ncpu.strip(),
        },
        "compiler": rustc.strip(),
        "binary": {
            "path": str(binary.relative_to(ROOT)),
            "bytes": size,
            "stripped_bytes": stripped,
            "strip_exit": strip_rc,
            "strip_log": stripped_log.strip(),
        },
        "startup_version_seconds": {
            "samples": samples,
            "median": statistics.median(samples) if samples else None,
            "p95": percentile(samples, 95) if samples else None,
            "command": "ide --version",
            "repeats": len(samples),
        },
        "idle_rss_kib": {
            "samples": rss_samples,
            "median": statistics.median(rss_samples) if rss_samples else None,
            "method": "tmux pane process rss via ps, 5 samples after 1.2s",
        },
        "budgets_from_benchmarks": {
            "stripped_binary_bytes": binary_budget,
            "idle_rss_kib": rss_budget,
            "note": "Proposed targets. Over-budget measurements stay recorded.",
        },
        "over_budget": over,
        "logs": [
            "engineering/evidence/runtime/performance-build.log",
            "engineering/evidence/runtime/performance-rss.log",
        ],
    }
    (OUT / "performance.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print("performance written", OUT / "performance.json")
    print("binary", size, "stripped", stripped, "startup_median", report["startup_version_seconds"]["median"], "rss", rss_samples)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
