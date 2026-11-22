//! The operating system clipboard. There is no editor-local clipboard.
//!
//! Set `POSIT_EDIT_CLIPBOARD=memory` for an in-process pasteboard (tests / CI).
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

fn pasteboard_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn memory_clip() -> &'static Mutex<Option<String>> {
    static CLIP: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    CLIP.get_or_init(|| Mutex::new(None))
}

fn use_memory_clipboard() -> bool {
    cfg!(test)
        || std::env::var_os("POSIT_EDIT_CLIPBOARD").is_some_and(|value| value == "memory")
}

pub fn set(text: &str) -> Result<(), String> {
    let _guard = pasteboard_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if use_memory_clipboard() {
        *memory_clip()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text.to_string());
        return Ok(());
    }
    set_unlocked(text)
}

pub fn get() -> Result<String, String> {
    let _guard = pasteboard_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if use_memory_clipboard() {
        return memory_clip()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| "memory clipboard empty".into());
    }
    get_unlocked()
}

/// Write then confirm round-trip, retrying briefly against external pasteboard noise.
#[cfg_attr(not(test), allow(dead_code))]
pub fn set_confirmed(text: &str) -> Result<(), String> {
    set(text)?;
    if get().ok().as_deref() == Some(text) {
        return Ok(());
    }
    if use_memory_clipboard() {
        return Err("memory clipboard round-trip failed".into());
    }
    for _ in 0..8 {
        set(text)?;
        if get().ok().as_deref() == Some(text) {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    Err("clipboard round-trip failed under contention".into())
}

fn set_unlocked(text: &str) -> Result<(), String> {
    run_first(copy_tools(), |program, args| write_tool(program, args, text))
}

fn get_unlocked() -> Result<String, String> {
    run_first(paste_tools(), read_tool)
}

fn copy_tools() -> Vec<(&'static str, Vec<&'static str>)> {
    if cfg!(target_os = "macos") {
        return vec![("pbcopy", vec![])];
    }
    if cfg!(target_os = "windows") {
        return vec![("clip", vec![])];
    }
    vec![
        ("wl-copy", vec![]),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ]
}

fn paste_tools() -> Vec<(&'static str, Vec<&'static str>)> {
    if cfg!(target_os = "macos") {
        return vec![("pbpaste", vec![])];
    }
    if cfg!(target_os = "windows") {
        return vec![(
            "powershell",
            vec!["-NoProfile", "-Command", "Get-Clipboard -Raw"],
        )];
    }
    vec![
        ("wl-paste", vec!["-n"]),
        ("xclip", vec!["-selection", "clipboard", "-out"]),
        ("xsel", vec!["--clipboard", "--output"]),
    ]
}

fn run_first<T>(
    tools: Vec<(&'static str, Vec<&'static str>)>,
    attempt: impl Fn(&str, &[&str]) -> io::Result<T>,
) -> Result<T, String> {
    let mut missing = Vec::new();
    for (program, args) in tools {
        match attempt(program, &args) {
            Ok(value) => return Ok(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(program),
            Err(error) => return Err(format!("{program}: {error}")),
        }
    }
    Err(format!(
        "system clipboard tool not found ({})",
        missing.join(", ")
    ))
}

fn write_tool(program: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("clipboard tool has no stdin"))?;
        stdin.write_all(text.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(tool_detail(&output.stderr)))
    }
}

fn read_tool(program: &str, args: &[&str]) -> io::Result<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|error| io::Error::other(error.to_string()))
    } else {
        Err(io::Error::other(tool_detail(&output.stderr)))
    }
}

fn tool_detail(stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        "clipboard tool failed".to_string()
    } else {
        detail.to_string()
    }
}
