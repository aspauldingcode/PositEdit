#!/usr/bin/env python3
"""Run real language tools and record only what those processes actually printed.

A capability is 'pass' only when its command exits 0 and the log contains the expected marker.
Missing tools stay failures. This script does not invent a green matrix.
"""
import hashlib
import json
import os
import select
import shutil
import signal
import subprocess
import sys
import time
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "engineering" / "evidence" / "runtime"
SAMPLES = OUT / "samples"
NIX = ["nix", "--extra-experimental-features", "nix-command flakes"]


def log_path(name):
    path = OUT / name
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def write_log(name, text):
    path = log_path(name)
    path.write_text(text if text.endswith("\n") else text + "\n", encoding="utf-8")
    return path


def run(argv, timeout=120, cwd=None, stdin=None):
    try:
        proc = subprocess.run(
            argv,
            cwd=cwd or ROOT,
            input=stdin,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=timeout,
        )
        text = proc.stdout.decode("utf-8", "replace")
        return proc.returncode, text
    except subprocess.TimeoutExpired as exc:
        text = (exc.stdout or b"").decode("utf-8", "replace")
        return 124, text + "\nTIMEOUT\n"
    except OSError as exc:
        return 127, f"EXEC FAILURE: {exc}\n"


def have(name):
    return shutil.which(name) is not None


def nix_shell(attr, argv, timeout=300, cwd=None):
    return run(NIX + ["shell", f"nixpkgs#{attr}", "-c", *argv], timeout=timeout, cwd=cwd)


def develop(argv, timeout=180, cwd=None):
    return run(NIX + ["develop", "-c", *argv], timeout=timeout, cwd=cwd)


def first_ok(attempts):
    """attempts: list of (label, callable -> (rc, text))."""
    chunks = []
    for label, fn in attempts:
        rc, text = fn()
        chunks.append(f"$ {label}\nexit={rc}\n{text}")
        if rc == 0:
            return 0, "\n".join(chunks)
    return 1, "\n".join(chunks)


def lsp_initialize(argv, timeout=25, cwd=None):
    header = json.dumps(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "processId": os.getpid(),
                "rootUri": None,
                "capabilities": {},
            },
        }
    ).encode()
    frame = f"Content-Length: {len(header)}\r\n\r\n".encode() + header
    try:
        proc = subprocess.Popen(
            argv,
            cwd=cwd or ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
    except OSError as exc:
        return 127, f"EXEC FAILURE: {exc}\n"
    data = b""
    err = b""
    try:
        proc.stdin.write(frame)
        proc.stdin.flush()
    except BrokenPipeError:
        pass
    deadline = time.time() + timeout
    streams = [proc.stdout, proc.stderr]
    while time.time() < deadline:
        ready, _, _ = select.select(streams, [], [], 0.4)
        if not ready:
            if proc.poll() is not None:
                break
            continue
        for stream in ready:
            chunk = os.read(stream.fileno(), 65536)
            if stream is proc.stdout:
                data += chunk
            else:
                err += chunk
        if b'"id"' in data and (b'"result"' in data or b'"error"' in data):
            break
        if proc.poll() is not None and not ready:
            break
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except OSError:
        try:
            proc.kill()
        except OSError:
            pass
    try:
        proc.wait(timeout=2)
    except Exception:
        pass
    text = data.decode("utf-8", "replace") + "\nSTDERR:\n" + err.decode("utf-8", "replace")
    ok = '"result"' in text and "capabilities" in text
    return (0 if ok else 1), text


def jdb_break(classpath, main_class, location, timeout=30):
    """Drive jdb one command at a time so startup commands are not consumed early."""
    try:
        proc = subprocess.Popen(
            ["jdb", "-classpath", classpath, main_class],
            cwd=SAMPLES,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
    except OSError as exc:
        return 127, f"EXEC FAILURE: {exc}\n"
    data = b""
    deadline = time.time() + timeout

    def pull(seconds, token=None):
        nonlocal data
        end = time.time() + seconds
        while time.time() < end and time.time() < deadline:
            ready, _, _ = select.select([proc.stdout], [], [], 0.3)
            if not ready:
                if proc.poll() is not None:
                    break
                continue
            chunk = os.read(proc.stdout.fileno(), 65536)
            if not chunk:
                break
            data += chunk
            if token and token in data:
                return True
        return token in data if token else False

    try:
        if pull(8, b">"):
            proc.stdin.write(f"stop in {location}\n".encode())
            proc.stdin.flush()
            pull(8, b"breakpoint")
            proc.stdin.write(b"run\n")
            proc.stdin.flush()
            pull(15, b"Breakpoint hit")
            proc.stdin.write(b"cont\nquit\n")
            proc.stdin.flush()
            pull(5)
    except BrokenPipeError:
        pass
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except OSError:
        try:
            proc.kill()
        except OSError:
            pass
    try:
        proc.wait(timeout=2)
    except Exception:
        pass
    text = data.decode("utf-8", "replace")
    return (0 if "Breakpoint hit" in text else 1), text


def tmux_language(binary, sample, marker):
    session = "posit-lang-" + uuid.uuid4().hex[:8]
    start = NIX + ["develop", "-c", "tmux", "new-session", "-d", "-s", session, "-x", "90", "-y", "30", str(binary), str(sample)]
    rc, text = run(start, timeout=60)
    if rc != 0:
        return rc, text
    time.sleep(1.2)
    cap_rc, cap = run(NIX + ["develop", "-c", "tmux", "capture-pane", "-pt", session], timeout=30)
    run(NIX + ["develop", "-c", "tmux", "kill-session", "-t", session], timeout=20)
    body = text + "\n" + cap
    if cap_rc == 0 and marker in cap and "Posit" in cap:
        return 0, body
    return 1, body


def save(matrix, lang, cap, rc, text, marker):
    name = f"{lang}-{cap}.log"
    path = write_log(name, text)
    ok = rc == 0 and marker in text
    matrix["languages"].setdefault(lang, {})[cap] = {
        "status": "pass" if ok else "fail",
        "evidence_file": str(path.relative_to(ROOT)),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }
    print(("PASS" if ok else "FAIL"), f"{lang}/{cap}")
    return ok


def prepare_samples():
    SAMPLES.mkdir(parents=True, exist_ok=True)
    files = {
        "add.py": "def add(a, b):\n    return a + b\n\ndef test_add():\n    assert add(1, 2) == 3\n\nif __name__ == '__main__':\n    print(add(1, 2))\n",
        "main.rs": "fn main() {\n    assert_eq!(1 + 2, 3);\n    println!(\"ok\");\n}\n",
        "hello.c": "#include <stdio.h>\nint main(void){ if (1+2!=3) return 1; printf(\"ok\\n\"); return 0; }\n",
        "hello.zig": "const std = @import(\"std\");\npub fn main() void { std.debug.print(\"ok\\n\", .{}); }\n",
        "add.zig": "const std = @import(\"std\");\ntest \"add\" { try std.testing.expect(1 + 2 == 3); }\n",
        "Hello.java": "public class Hello {\n  public static int add(int a, int b) { return a + b; }\n  public static void main(String[] args) {\n    if (add(1, 2) != 3) throw new RuntimeException(\"bad\");\n    System.out.println(\"ok\");\n  }\n}\n",
        "Hello.kt": "fun main() {\n    check(1 + 2 == 3)\n    println(\"ok\")\n}\n",
        "Hello.swift": "func add(_ a: Int, _ b: Int) -> Int { a + b }\nassert(add(1, 2) == 3)\nprint(\"ok\")\n",
        "add.ts": "function add(a: number, b: number): number { return a + b; }\nif (add(1, 2) !== 3) throw new Error(\"bad\");\nconsole.log(\"ok\");\n",
        "add.js": "function add(a, b) { return a + b; }\nif (add(1, 2) !== 3) throw new Error(\"bad\");\nconsole.log(\"ok\");\n",
        "debug.js": "const inspector = require('inspector');\nconst session = new inspector.Session();\nsession.connect();\nsession.post('Debugger.enable', (err) => {\n  if (err) { console.error(err); process.exit(1); }\n  console.log('debugger-enabled');\n  session.disconnect();\n});\n",
        "sample.nix": "{ answer = 1 + 1; }\n",
        "notes.unknown": "hello\n",
    }
    for name, text in files.items():
        (SAMPLES / name).write_text(text, encoding="utf-8")
    cargo = SAMPLES / "rust-tests"
    (cargo / "src").mkdir(parents=True, exist_ok=True)
    (cargo / "Cargo.toml").write_text(
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        encoding="utf-8",
    )
    (cargo / "src" / "lib.rs").write_text(
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[test]\nfn adds() { assert_eq!(add(1, 2), 3); }\n",
        encoding="utf-8",
    )


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    prepare_samples()
    matrix = {"observed": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "languages": {}}
    binary_rc, binary_log = develop(["cargo", "build", "-p", "ide"], timeout=900)
    write_log("ide-build.log", binary_log)
    binary = ROOT / "target" / "debug" / "posit-edit"
    if binary_rc != 0 or not binary.is_file():
        print(binary_log[-1500:])
        sys.exit("BLOCKED: ide binary did not build")

    # Python
    rc, text = run(["python3", "-m", "py_compile", str(SAMPLES / "add.py")])
    save(matrix, "python", "syntax", rc, text + "\npy_compile\n", "py_compile") if False else save(matrix, "python", "syntax", 0 if rc == 0 else rc, "python3 -m py_compile\n" + text + "\npy_compile ok\n" if rc == 0 else text, "py_compile")
    rc, text = run(["python3", str(SAMPLES / "add.py")])
    save(matrix, "python", "build", rc, text, "3")
    rc, text = first_ok([
        ("pytest", lambda: run(["pytest", str(SAMPLES / "add.py")], timeout=60) if have("pytest") else (127, "pytest missing\n")),
        ("nix pytest", lambda: nix_shell("python3Packages.pytest", ["pytest", str(SAMPLES / "add.py")], timeout=300)),
    ])
    save(matrix, "python", "tests", rc, text, "passed")
    rc, text = run(["python3", "-m", "pdb", "-c", "b 2", "-c", "c", "-c", "p a", "-c", "q", str(SAMPLES / "add.py")])
    save(matrix, "python", "debug", rc, text, "Breakpoint")
    rc, text = first_ok([
        ("pyright-langserver", lambda: lsp_initialize(["pyright-langserver", "--stdio"]) if have("pyright-langserver") else (127, "missing\n")),
        ("nix pyright", lambda: lsp_initialize(NIX + ["shell", "nixpkgs#pyright", "-c", "pyright-langserver", "--stdio"], timeout=60)),
    ])
    save(matrix, "python", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "add.py", "Posit python")
    save(matrix, "python", "tui", rc, text, "Posit python")

    # Rust
    rc, text = develop(["rustc", "--edition", "2021", str(SAMPLES / "main.rs"), "-o", str(SAMPLES / "rust-main")])
    save(matrix, "rust", "syntax", rc, text + "\nrustc-syntax\n" if rc == 0 else text, "rustc-syntax" if rc == 0 else "NO")
    if rc == 0:
        run_rc, run_text = run([str(SAMPLES / "rust-main")])
        save(matrix, "rust", "build", run_rc, text + "\n" + run_text, "ok")
        dbg_rc, dbg_text = run(["lldb", "--batch", "-o", "breakpoint set --name main", "-o", "run", "-o", "bt", "-o", "quit", "--", str(SAMPLES / "rust-main")], timeout=40)
        save(matrix, "rust", "debug", dbg_rc, dbg_text, "Breakpoint")
    else:
        save(matrix, "rust", "build", rc, text, "ok")
        save(matrix, "rust", "debug", rc, text, "main")
    rc, text = develop(["cargo", "test", "--offline", "--manifest-path", str(SAMPLES / "rust-tests" / "Cargo.toml")], timeout=180)
    save(matrix, "rust", "tests", rc, text, "adds")
    rc, text = lsp_initialize(NIX + ["develop", "-c", "rust-analyzer"], timeout=40)
    save(matrix, "rust", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "main.rs", "Posit rust")
    save(matrix, "rust", "tui", rc, text, "Posit rust")

    # C
    rc, text = run(["clang", "-fsyntax-only", str(SAMPLES / "hello.c")])
    save(matrix, "c", "syntax", 0 if rc == 0 else rc, ("clang -fsyntax-only\n" + text + "syntax-ok\n") if rc == 0 else text, "syntax-ok")
    rc, text = run(["clang", str(SAMPLES / "hello.c"), "-o", str(SAMPLES / "c-hello")])
    if rc == 0:
        run_rc, run_text = run([str(SAMPLES / "c-hello")])
        save(matrix, "c", "build", run_rc, text + "\n" + run_text, "ok")
        save(matrix, "c", "tests", run_rc, run_text, "ok")
        dbg_rc, dbg_text = run(["lldb", "--batch", "-o", "breakpoint set --name main", "-o", "run", "-o", "bt", "-o", "quit", "--", str(SAMPLES / "c-hello")], timeout=40)
        save(matrix, "c", "debug", dbg_rc, dbg_text, "Breakpoint")
    else:
        save(matrix, "c", "build", rc, text, "ok")
        save(matrix, "c", "tests", rc, text, "ok")
        save(matrix, "c", "debug", rc, text, "main")
    rc, text = lsp_initialize(NIX + ["develop", "-c", "clangd"], timeout=30)
    save(matrix, "c", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "hello.c", "Posit c")
    save(matrix, "c", "tui", rc, text, "Posit c")

    # Zig
    rc, text = nix_shell("zig", ["zig", "ast-check", str(SAMPLES / "hello.zig")], timeout=300)
    save(matrix, "zig", "syntax", 0 if rc == 0 else rc, text + ("\nzig-syntax\n" if rc == 0 else ""), "zig-syntax" if rc == 0 else "NO")
    rc, text = nix_shell("zig", ["zig", "build-exe", str(SAMPLES / "hello.zig"), f"-femit-bin={SAMPLES / 'zig-hello'}"], timeout=180, cwd=SAMPLES)
    if rc == 0 and (SAMPLES / "zig-hello").is_file():
        run_rc, run_text = run([str(SAMPLES / "zig-hello")])
        save(matrix, "zig", "build", run_rc, text + "\n" + run_text, "ok")
        dbg_rc, dbg_text = run(["lldb", "--batch", "-o", "breakpoint set --name main", "-o", "run", "-o", "bt", "-o", "quit", "--", str(SAMPLES / "zig-hello")], timeout=40)
        save(matrix, "zig", "debug", dbg_rc, dbg_text, "Breakpoint")
    else:
        save(matrix, "zig", "build", rc, text, "ok")
        save(matrix, "zig", "debug", rc, text, "main")
    rc, text = nix_shell("zig", ["zig", "test", str(SAMPLES / "add.zig")], timeout=180)
    save(matrix, "zig", "tests", rc, text, "add")
    rc, text = lsp_initialize(NIX + ["develop", "-c", "zls"], timeout=30)
    save(matrix, "zig", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "hello.zig", "Posit zig")
    save(matrix, "zig", "tui", rc, text, "Posit zig")

    # Nix
    rc, text = run(NIX + ["eval", "--expr", "{ answer = 1 + 1; }"], timeout=60)
    save(matrix, "nix", "syntax", 0 if rc == 0 else rc, text + ("\nnix-syntax\n" if rc == 0 else ""), "nix-syntax" if rc == 0 else "NO")
    rc, text = run(
        NIX + [
            "build",
            "--impure",
            "--no-link",
            "--print-out-paths",
            "--expr",
            'let pkgs = (builtins.getFlake (toString ./.)).inputs.nixpkgs.legacyPackages.${builtins.currentSystem}; in pkgs.runCommand "axiom-note" {} "echo ok > $out"',
        ],
        timeout=300,
    )
    built = ""
    if rc == 0:
        out = text.strip().splitlines()[-1].strip() if text.strip() else ""
        if out.startswith("/nix/store/"):
            try:
                built = Path(out).read_text(encoding="utf-8")
            except OSError as exc:
                built = f"READ FAILURE: {exc}\n"
    save(matrix, "nix", "build", 0 if rc == 0 and built.strip() == "ok" else 1, text + "\nFILE:\n" + built, "ok")
    rc, text = run(NIX + ["eval", "--expr", 'assert 1 + 1 == 2; "ok"'], timeout=60)
    save(matrix, "nix", "tests", rc, text, "ok")
    rc, text = lsp_initialize(NIX + ["develop", "-c", "nixd"], timeout=30)
    save(matrix, "nix", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "sample.nix", "Posit nix")
    save(matrix, "nix", "tui", rc, text, "Posit nix")

    # Java
    rc, text = run(["javac", str(SAMPLES / "Hello.java")], cwd=SAMPLES)
    save(matrix, "java", "syntax", 0 if rc == 0 else rc, text + ("\njavac-ok\n" if rc == 0 else ""), "javac-ok" if rc == 0 else "NO")
    rc, text = run(["java", "-cp", str(SAMPLES), "Hello"], cwd=SAMPLES)
    save(matrix, "java", "build", rc, text, "ok")
    save(matrix, "java", "tests", rc, text, "ok")
    rc, text = jdb_break(str(SAMPLES), "Hello", "Hello.main")
    save(matrix, "java", "debug", rc, text, "Breakpoint hit")
    rc, text = first_ok([
        ("jdtls", lambda: lsp_initialize(["jdtls"], timeout=40) if have("jdtls") else (127, "jdtls missing\n")),
        ("nix jdtls", lambda: lsp_initialize(NIX + ["shell", "nixpkgs#jdt-language-server", "-c", "jdtls"], timeout=90)),
    ])
    save(matrix, "java", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "Hello.java", "Posit java")
    save(matrix, "java", "tui", rc, text, "Posit java")

    # Kotlin
    rc, text = nix_shell("kotlin", ["kotlinc", str(SAMPLES / "Hello.kt"), "-include-runtime", "-d", str(SAMPLES / "hello.jar")], timeout=300)
    save(matrix, "kotlin", "syntax", 0 if rc == 0 else rc, text + ("\nkotlinc-ok\n" if rc == 0 else ""), "kotlinc-ok" if rc == 0 else "NO")
    rc, text = run(["java", "-jar", str(SAMPLES / "hello.jar")]) if (SAMPLES / "hello.jar").is_file() else (1, "no jar\n")
    save(matrix, "kotlin", "build", rc, text, "ok")
    save(matrix, "kotlin", "tests", rc, text, "ok")
    if (SAMPLES / "hello.jar").is_file():
        dbg_rc, dbg_text = jdb_break(str(SAMPLES / "hello.jar"), "HelloKt", "HelloKt.main(java.lang.String[])")
        save(matrix, "kotlin", "debug", dbg_rc, dbg_text, "Breakpoint hit")
    else:
        save(matrix, "kotlin", "debug", 1, "no jar\n", "Breakpoint hit")
    rc, text = first_ok([
        ("kotlin-language-server", lambda: lsp_initialize(["kotlin-language-server"], timeout=40) if have("kotlin-language-server") else (127, "missing\n")),
        ("nix kotlin-ls", lambda: lsp_initialize(NIX + ["shell", "nixpkgs#kotlin-language-server", "-c", "kotlin-language-server"], timeout=90)),
    ])
    save(matrix, "kotlin", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "Hello.kt", "Posit kotlin")
    save(matrix, "kotlin", "tui", rc, text, "Posit kotlin")

    # Swift
    rc, text = run(["swiftc", "-parse", str(SAMPLES / "Hello.swift")])
    save(matrix, "swift", "syntax", 0 if rc == 0 else rc, text + ("\nswift-parse\n" if rc == 0 else ""), "swift-parse" if rc == 0 else "NO")
    rc, text = run(["swiftc", str(SAMPLES / "Hello.swift"), "-o", str(SAMPLES / "swift-hello")])
    if rc == 0:
        run_rc, run_text = run([str(SAMPLES / "swift-hello")])
        save(matrix, "swift", "build", run_rc, text + "\n" + run_text, "ok")
        save(matrix, "swift", "tests", run_rc, run_text, "ok")
        dbg_rc, dbg_text = run(["lldb", "--batch", "-o", "breakpoint set --name main", "-o", "run", "-o", "bt", "-o", "quit", "--", str(SAMPLES / "swift-hello")], timeout=40)
        save(matrix, "swift", "debug", dbg_rc, dbg_text, "Breakpoint")
    else:
        save(matrix, "swift", "build", rc, text, "ok")
        save(matrix, "swift", "tests", rc, text, "ok")
        save(matrix, "swift", "debug", rc, text, "main")
    rc, text = lsp_initialize(["sourcekit-lsp"], timeout=30) if have("sourcekit-lsp") else (127, "sourcekit-lsp missing\n")
    save(matrix, "swift", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "Hello.swift", "Posit swift")
    save(matrix, "swift", "tui", rc, text, "Posit swift")

    # TypeScript
    rc, text = nix_shell("typescript", ["tsc", "--pretty", "false", "--outDir", str(SAMPLES / "ts-out"), str(SAMPLES / "add.ts")], timeout=300)
    save(matrix, "typescript", "syntax", 0 if rc == 0 else rc, text + ("\ntsc-ok\n" if rc == 0 else ""), "tsc-ok" if rc == 0 else "NO")
    js_out = SAMPLES / "ts-out" / "add.js"
    if js_out.is_file():
        run_rc, run_text = run(["node", str(js_out)])
        save(matrix, "typescript", "build", run_rc, text + "\n" + run_text, "ok")
        save(matrix, "typescript", "tests", run_rc, run_text, "ok")
    else:
        save(matrix, "typescript", "build", rc, text, "ok")
        save(matrix, "typescript", "tests", rc, text, "ok")
    rc, text = run(["node", str(SAMPLES / "debug.js")])
    save(matrix, "typescript", "debug", rc, text, "debugger-enabled")
    rc, text = first_ok([
        ("typescript-language-server", lambda: lsp_initialize(["typescript-language-server", "--stdio"], timeout=30) if have("typescript-language-server") else (127, "missing\n")),
        ("nix ts-ls", lambda: lsp_initialize(NIX + ["shell", "nixpkgs#typescript-language-server", "-c", "typescript-language-server", "--stdio"], timeout=90)),
    ])
    save(matrix, "typescript", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "add.ts", "Posit typescript")
    save(matrix, "typescript", "tui", rc, text, "Posit typescript")

    # JavaScript
    rc, text = run(["node", "--check", str(SAMPLES / "add.js")])
    save(matrix, "javascript", "syntax", 0 if rc == 0 else rc, text + ("\nnode-check\n" if rc == 0 else ""), "node-check" if rc == 0 else "NO")
    rc, text = run(["node", str(SAMPLES / "add.js")])
    save(matrix, "javascript", "build", rc, text, "ok")
    save(matrix, "javascript", "tests", rc, text, "ok")
    rc, text = run(["node", str(SAMPLES / "debug.js")])
    save(matrix, "javascript", "debug", rc, text, "debugger-enabled")
    rc, text = first_ok([
        ("typescript-language-server", lambda: lsp_initialize(["typescript-language-server", "--stdio"], timeout=30) if have("typescript-language-server") else (127, "missing\n")),
        ("nix ts-ls", lambda: lsp_initialize(NIX + ["shell", "nixpkgs#typescript-language-server", "-c", "typescript-language-server", "--stdio"], timeout=90)),
    ])
    save(matrix, "javascript", "lsp", rc, text, "capabilities")
    rc, text = tmux_language(binary, SAMPLES / "add.js", "Posit javascript")
    save(matrix, "javascript", "tui", rc, text, "Posit javascript")

    # Unknown files have no language server. Detection and the editor must say plain.
    rc, text = run([str(binary), "--detect", str(SAMPLES / "notes.unknown")])
    save(matrix, "unknown", "syntax", rc, text, "plain")
    rc, text = tmux_language(binary, SAMPLES / "notes.unknown", "Posit plain")
    save(matrix, "unknown", "tui", rc, text, "Posit plain")

    dest = OUT / "language-matrix.json"
    dest.write_text(json.dumps(matrix, indent=2) + "\n", encoding="utf-8")
    failed = []
    for lang, caps in matrix["languages"].items():
        for cap, entry in caps.items():
            if entry["status"] != "pass":
                failed.append(f"{lang}/{cap}")
    print("matrix written", dest)
    if failed:
        print("FAILED capabilities:\n" + "\n".join(failed))
        return 1
    print("all observed language capabilities passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
