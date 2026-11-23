//! One Posit Edit session per user. A second launch fails closed.
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

/// Inherited by every child of a live editor, including in-editor shells.
pub const SESSION_ENV: &str = "POSIT_EDIT_SESSION";

/// Holds the exclusive lock for the life of the process.
pub struct InstanceGuard {
    #[cfg(unix)]
    _file: File,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // Allow a subsequent acquire in the same process (tests / restart).
        std::env::remove_var(SESSION_ENV);
    }
}

/// Refuse a nested or concurrent Posit Edit before the TUI starts.
pub fn acquire() -> io::Result<InstanceGuard> {
    if std::env::var_os(SESSION_ENV).is_some() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Posit Edit is already running in this session. Only one Posit is allowed.",
        ));
    }
    #[cfg(unix)]
    {
        let path = lock_path()?;
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Posit Edit is already running. Only one session is allowed.",
            ));
        }
        let _ = writeln!(file, "{}", std::process::id());
        let _ = file.flush();
        // Children inherit this so `posit-edit` / `pst` inside the IDE terminal fail first.
        std::env::set_var(SESSION_ENV, "1");
        Ok(InstanceGuard { _file: file })
    }
    #[cfg(not(unix))]
    {
        if instance_marker_busy()? {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Posit Edit is already running. Only one session is allowed.",
            ));
        }
        std::env::set_var(SESSION_ENV, "1");
        Ok(InstanceGuard {})
    }
}

fn lock_path() -> io::Result<PathBuf> {
    if let Ok(override_path) = std::env::var("POSIT_EDIT_INSTANCE_LOCK") {
        if !override_path.is_empty() {
            return Ok(PathBuf::from(override_path));
        }
    }
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        if !runtime.is_empty() {
            return Ok(PathBuf::from(runtime).join("posit-edit.lock"));
        }
    }
    let mut dir = std::env::temp_dir();
    dir.push(format!("posit-edit-{}", user_token()));
    Ok(dir.join("instance.lock"))
}

fn user_token() -> String {
    #[cfg(unix)]
    {
        unsafe { libc::getuid() }.to_string()
    }
    #[cfg(not(unix))]
    {
        std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "user".into())
    }
}

#[cfg(not(unix))]
fn instance_marker_busy() -> io::Result<bool> {
    // Best-effort create-new marker; flock is unix-first.
    let path = lock_path()?;
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{}", std::process::id());
            // Leak the handle for process lifetime; OS cleans on exit.
            std::mem::forget(file);
            Ok(false)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(true),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env + lock path must not run in parallel across this module.
    static LOCK: Mutex<()> = Mutex::new(());

    fn with_test_lock<F: FnOnce()>(body: F) {
        let _guard = LOCK.lock().unwrap();
        let previous_session = std::env::var_os(SESSION_ENV);
        let previous_lock = std::env::var_os("POSIT_EDIT_INSTANCE_LOCK");
        let path = std::env::temp_dir().join(format!(
            "posit-edit-test-{}-{}.lock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::env::set_var("POSIT_EDIT_INSTANCE_LOCK", &path);
        std::env::remove_var(SESSION_ENV);
        body();
        let _ = std::fs::remove_file(&path);
        match previous_session {
            Some(value) => std::env::set_var(SESSION_ENV, value),
            None => std::env::remove_var(SESSION_ENV),
        }
        match previous_lock {
            Some(value) => std::env::set_var("POSIT_EDIT_INSTANCE_LOCK", value),
            None => std::env::remove_var("POSIT_EDIT_INSTANCE_LOCK"),
        }
    }

    #[test]
    fn second_acquire_is_rejected() {
        with_test_lock(|| {
            let first = acquire().expect("first session");
            // A second process outside the IDE does not inherit SESSION_ENV; the flock still blocks it.
            std::env::remove_var(SESSION_ENV);
            let second = acquire();
            assert!(second.is_err(), "second session must fail");
            let err = second.err().unwrap();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
            assert!(err.to_string().contains("already running"));
            drop(first);
            let again = (0..8).find_map(|_| match acquire() {
                Ok(guard) => Some(guard),
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    None
                }
            });
            assert!(
                again.is_some(),
                "lock releases when the first session ends"
            );
            drop(again);
        });
    }

    #[test]
    fn nested_session_env_is_rejected() {
        with_test_lock(|| {
            std::env::set_var(SESSION_ENV, "1");
            let result = acquire();
            assert!(result.is_err());
            assert!(result
                .err()
                .unwrap()
                .to_string()
                .contains("already running in this session"));
            std::env::remove_var(SESSION_ENV);
        });
    }
}
