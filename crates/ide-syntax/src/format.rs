//! Manual formatting. Nothing in this module runs unless a caller asks.
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatter {
    pub program: &'static str,
    pub args: &'static [&'static str],
    pub extension: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatOutcome {
    Applied(String),
    Unchanged,
    Unavailable { program: String },
    Failed { program: String, detail: String },
}

pub fn formatter_for(language: &str) -> Option<Formatter> {
    formatters_for(language).into_iter().next()
}

/// Preferred tool first, then practical fallbacks when the preferred binary is missing.
fn formatters_for(language: &str) -> Vec<Formatter> {
    match language {
        "rust" => vec![Formatter {
            program: "rustfmt",
            args: &["--edition", "2021"],
            extension: "rs",
        }],
        "python" => vec![Formatter {
            program: "ruff",
            args: &["format"],
            extension: "py",
        }],
        "c" => vec![Formatter {
            program: "clang-format",
            args: &["-i"],
            extension: "c",
        }],
        "objc" | "objective-c" | "objective-cpp" | "objcpp" => vec![Formatter {
            program: "clang-format",
            args: &["-i", "-assume-filename=file.m"],
            extension: "m",
        }],
        "zig" => vec![Formatter {
            program: "zig",
            args: &["fmt"],
            extension: "zig",
        }],
        // Prefer official nixfmt only — do not fall back to alejandra/nixpkgs-fmt.
        "nix" => vec![
            Formatter {
                program: "nixfmt",
                args: &[],
                extension: "nix",
            },
            Formatter {
                program: "nixfmt-rfc-style",
                args: &[],
                extension: "nix",
            },
        ],
        "java" => vec![
            Formatter {
                program: "google-java-format",
                args: &["--replace"],
                extension: "java",
            },
            // Widely installed; clang-format understands Java when given a .java name.
            Formatter {
                program: "clang-format",
                args: &["-i", "-assume-filename=file.java"],
                extension: "java",
            },
        ],
        "kotlin" => vec![Formatter {
            program: "ktlint",
            args: &["--format"],
            extension: "kt",
        }],
        "swift" => vec![Formatter {
            program: "swift-format",
            args: &["--in-place"],
            extension: "swift",
        }],
        "javascript" => vec![Formatter {
            program: "prettier",
            args: &["--write"],
            extension: "js",
        }],
        "typescript" => vec![Formatter {
            program: "prettier",
            args: &["--write"],
            extension: "ts",
        }],
        _ => Vec::new(),
    }
}

pub fn format_manually(language: &str, source: &str) -> FormatOutcome {
    let formatters = formatters_for(language);
    if formatters.is_empty() {
        return FormatOutcome::Unavailable {
            program: "none".to_string(),
        };
    }
    let timeout = Duration::from_secs(5);
    let mut missing = Vec::new();
    for formatter in formatters {
        match format_with(
            formatter.program,
            formatter.args,
            formatter.extension,
            source,
            timeout,
        ) {
            FormatOutcome::Unavailable { program } => missing.push(program),
            other => return other,
        }
    }
    FormatOutcome::Unavailable {
        program: missing.join(" / "),
    }
}

pub fn format_with(
    program: &str,
    args: &[&str],
    extension: &str,
    source: &str,
    timeout: Duration,
) -> FormatOutcome {
    let scratch = match scratch_file(extension, source) {
        Ok(path) => path,
        Err(error) => {
            return FormatOutcome::Failed {
                program: program.to_string(),
                detail: error.to_string(),
            };
        }
    };
    let outcome = run_formatter(program, args, &scratch, source, timeout);
    let _ = fs::remove_file(&scratch);
    if let Some(parent) = scratch.parent() {
        let _ = fs::remove_dir(parent);
    }
    outcome
}

fn scratch_file(extension: &str, source: &str) -> io::Result<PathBuf> {
    static TICK: AtomicU64 = AtomicU64::new(0);
    let n = TICK.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "axiom-format-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("buffer.{extension}"));
    fs::write(&path, source)?;
    Ok(path)
}

fn run_formatter(
    program: &str,
    args: &[&str],
    file: &PathBuf,
    source: &str,
    timeout: Duration,
) -> FormatOutcome {
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return FormatOutcome::Unavailable {
                program: program.to_string(),
            };
        }
        Err(error) => {
            return FormatOutcome::Failed {
                program: program.to_string(),
                detail: error.to_string(),
            };
        }
    };
    let started = Instant::now();
    let status = loop {
        if started.elapsed() >= timeout {
            kill_child(&mut child);
            let _ = child.wait();
            return FormatOutcome::Failed {
                program: program.to_string(),
                detail: "formatter timed out".to_string(),
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                return FormatOutcome::Failed {
                    program: program.to_string(),
                    detail: error.to_string(),
                };
            }
        }
    };
    if !status.success() {
        let mut detail = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut detail);
        }
        let detail = detail.trim();
        return FormatOutcome::Failed {
            program: program.to_string(),
            detail: if detail.is_empty() {
                format!("exit {}", status.code().unwrap_or(-1))
            } else {
                detail.chars().take(400).collect()
            },
        };
    }
    match fs::read_to_string(file) {
        Ok(formatted) if formatted == source => FormatOutcome::Unchanged,
        Ok(formatted) => FormatOutcome::Applied(formatted),
        Err(error) => FormatOutcome::Failed {
            program: program.to_string(),
            detail: error.to_string(),
        },
    }
}

fn kill_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
    }
    let _ = child.kill();
}
