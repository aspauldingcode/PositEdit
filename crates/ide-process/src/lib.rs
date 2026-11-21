//! Supervised child processes. Commands are argv arrays, never a shell.
//! Output is capped, and a timeout or cancel kills the child.
use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const COMPONENT: &str = "ide-process";
pub const IMPLEMENTED: bool = true;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    NotInstalled,
    Ready,
    Unsupported,
}

pub fn status() -> Availability {
    Availability::Ready
}

#[derive(Clone, Debug)]
pub struct JobRequest {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Option<Duration>,
    pub max_output: usize,
}

impl JobRequest {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            timeout: None,
            max_output: 1024 * 1024,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn max_output(mut self, max_output: usize) -> Self {
        self.max_output = max_output;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobOutput {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub truncated: bool,
}

pub fn run_job(request: &JobRequest, cancel: &AtomicBool) -> io::Result<JobOutput> {
    let mut command = Command::new(&request.program);
    command
        .args(&request.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let cap = request.max_output;
    let stdout_handle = thread::spawn(move || read_capped(stdout, cap));
    let stderr_handle = thread::spawn(move || read_capped(stderr, cap));
    let started = Instant::now();
    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
            kill_child(&mut child);
            break child.wait()?;
        }
        if request
            .timeout
            .is_some_and(|limit| started.elapsed() >= limit)
        {
            timed_out = true;
            kill_child(&mut child);
            break child.wait()?;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let (stdout, stdout_cut) = stdout_handle.join().unwrap_or_default();
    let (stderr, stderr_cut) = stderr_handle.join().unwrap_or_default();
    Ok(JobOutput {
        exit_code: status.code(),
        stdout,
        stderr,
        timed_out,
        cancelled,
        truncated: stdout_cut || stderr_cut,
    })
}

fn read_capped(pipe: Option<impl Read>, cap: usize) -> (Vec<u8>, bool) {
    let mut reader = match pipe {
        Some(reader) => reader,
        None => return (Vec::new(), false),
    };
    let mut buffer = [0_u8; 4096];
    let mut output = Vec::new();
    let mut truncated = false;
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => return (output, truncated),
            Ok(count) => {
                if output.len() >= cap {
                    truncated = true;
                    continue;
                }
                let room = cap - output.len();
                let take = count.min(room);
                output.extend_from_slice(&buffer[..take]);
                if take < count {
                    truncated = true;
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_without_a_shell() {
        let cancel = AtomicBool::new(false);
        let request = JobRequest::new("python3")
            .arg("-c")
            .arg("import sys; print(sys.argv[1])")
            .arg("a;echo injected");
        let output = run_job(&request, &cancel).unwrap();
        assert_eq!(output.exit_code, Some(0));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "a;echo injected"
        );
        assert!(!output.timed_out);
    }

    #[test]
    fn timeout_is_not_success() {
        let cancel = AtomicBool::new(false);
        let request = JobRequest::new("python3")
            .arg("-c")
            .arg("import time; time.sleep(30)")
            .timeout(Duration::from_millis(200));
        let started = Instant::now();
        let output = run_job(&request, &cancel).unwrap();
        assert!(output.timed_out);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn cancel_stops_the_child() {
        let cancel = AtomicBool::new(true);
        let request = JobRequest::new("python3")
            .arg("-c")
            .arg("import time; time.sleep(30)");
        let output = run_job(&request, &cancel).unwrap();
        assert!(output.cancelled);
    }

    #[test]
    fn output_is_capped() {
        let cancel = AtomicBool::new(false);
        let request = JobRequest::new("python3")
            .arg("-c")
            .arg("print('x'*100000, end='')")
            .max_output(64);
        let output = run_job(&request, &cancel).unwrap();
        assert!(output.truncated);
        assert!(output.stdout.len() <= 64);
    }
}
