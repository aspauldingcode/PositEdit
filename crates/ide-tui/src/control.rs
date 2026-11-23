//! Unix control socket so MCP / agents can drive a live Posit Edit session.
use crate::{draw, EditorApp};
use ide_mcp::{control_socket_path, BufferInfo, ControlRequest, ControlResponse, StatusInfo};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct ControlServer {
    listener: UnixListener,
    path: PathBuf,
}

impl ControlServer {
    pub fn bind() -> io::Result<Self> {
        let path = control_socket_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _ = fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        // Advertise for shells / MCP launchers.
        std::env::set_var("POSIT_EDIT_CONTROL_SOCKET", &path);
        // Stable pointer under the lock directory (and home cache) for MCP discovery.
        if let Some(parent) = path.parent() {
            let _ = fs::write(parent.join("control.path"), path.to_string_lossy().as_bytes());
        }
        let home_ptr = dirs_control_pointer();
        if let Some(parent) = home_ptr.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&home_ptr, path.to_string_lossy().as_bytes());
        Ok(Self { listener, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn poll(&self, app: &mut EditorApp) {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = handle_client(stream, app) {
                        app.notice = format!("control: {error}");
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    app.notice = format!("control accept: {error}");
                    break;
                }
            }
        }
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_file(parent.join("control.path"));
        }
        let _ = fs::remove_file(dirs_control_pointer());
    }
}

fn dirs_control_pointer() -> PathBuf {
    let mut dir = std::env::temp_dir();
    // Also publish under a predictable home-relative cache for MCP hosts whose TMPDIR differs.
    if let Ok(home) = std::env::var("HOME") {
        dir = PathBuf::from(home).join(".cache").join("posit-edit");
    }
    dir.join("control.path")
}

fn handle_client(stream: UnixStream, app: &mut EditorApp) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Ok(());
    }
    let request: ControlRequest = serde_json::from_str(line.trim())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let response = dispatch(app, &request);
    let mut writer = stream;
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&response)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
    )?;
    Ok(())
}

fn dispatch(app: &mut EditorApp, request: &ControlRequest) -> ControlResponse {
    match request.method.as_str() {
        "ping" => ok(
            request.id,
            serde_json::json!({"pid": std::process::id(), "socket": control_socket_path()}),
        ),
        "status" => ok(request.id, serde_json::to_value(status_info(app)).unwrap()),
        "buffers" => ok(
            request.id,
            serde_json::to_value(buffer_list(app)).unwrap(),
        ),
        "read_buffer" => match read_buffer(app, &request.params) {
            Ok(text) => ok(request.id, serde_json::json!({"text": text})),
            Err(error) => err(request.id, error),
        },
        "snapshot" => {
            let width = request
                .params
                .get("width")
                .and_then(|v| v.as_u64())
                .unwrap_or(app.width.max(40) as u64) as u16;
            let height = request
                .params
                .get("height")
                .and_then(|v| v.as_u64())
                .unwrap_or(app.height.max(20) as u64) as u16;
            ok(
                request.id,
                serde_json::json!({
                    "width": width,
                    "height": height,
                    "text": render_snapshot(app, width, height)
                }),
            )
        }
        "open" => match request.params.get("path").and_then(|v| v.as_str()) {
            Some(path) => {
                app.control_open(PathBuf::from(path));
                ok(request.id, serde_json::json!({"notice": app.notice()}))
            }
            None => err(request.id, "path required"),
        },
        "key" => match request.params.get("key").and_then(|v| v.as_str()) {
            Some(key) => match app.control_key(key) {
                Ok(()) => ok(request.id, serde_json::json!({"notice": app.notice()})),
                Err(error) => err(request.id, error),
            },
            None => err(request.id, "key required"),
        },
        "command" => match request.params.get("name").and_then(|v| v.as_str()) {
            Some(name) => match app.control_command(name) {
                Ok(()) => ok(request.id, serde_json::json!({"notice": app.notice()})),
                Err(error) => err(request.id, error),
            },
            None => err(request.id, "name required"),
        },
        "runtime" => match request.params.get("kind").and_then(|v| v.as_str()) {
            Some(kind) => match app.control_runtime(kind) {
                Ok(()) => ok(
                    request.id,
                    serde_json::json!({
                        "notice": app.notice(),
                        "runtime_dock": app.runtime_dock_id()
                    }),
                ),
                Err(error) => err(request.id, error),
            },
            None => err(request.id, "kind required (browser|phone|android)"),
        },
        "pointer" => {
            let kind = request
                .params
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("down");
            let column = request
                .params
                .get("column")
                .or_else(|| request.params.get("x"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;
            let row = request
                .params
                .get("row")
                .or_else(|| request.params.get("y"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u16;
            match app.control_pointer(kind, column, row) {
                Ok(()) => ok(
                    request.id,
                    serde_json::json!({
                        "notice": app.notice(),
                        "runtime_dock": app.runtime_dock_id()
                    }),
                ),
                Err(error) => err(request.id, error),
            }
        }
        "capture" => match capture(app, &request.params) {
            Ok(path) => ok(request.id, serde_json::json!({"path": path})),
            Err(error) => err(request.id, error),
        },
        other => err(request.id, format!("unknown method: {other}")),
    }
}

fn ok(id: u64, result: serde_json::Value) -> ControlResponse {
    ControlResponse {
        id,
        ok: true,
        error: None,
        result: Some(result),
    }
}

fn err(id: u64, error: impl Into<String>) -> ControlResponse {
    ControlResponse {
        id,
        ok: false,
        error: Some(error.into()),
        result: None,
    }
}

fn status_info(app: &EditorApp) -> StatusInfo {
    let buffer = app.focused_buffer_meta();
    StatusInfo {
        pid: std::process::id(),
        workspace: app.workspace_path().display().to_string(),
        notice: app.notice().to_string(),
        focus: format!("{:?}", app.pane_focus()),
        editing: app.is_editing(),
        buffer_name: buffer.name,
        language: buffer.language,
        cursor: buffer.cursor,
        revision: buffer.revision,
        dirty: buffer.dirty,
        buffer_count: app.buffer_count(),
        terminal_open: app.terminals_open(),
        runtime_dock: app.runtime_dock_id(),
        width: app.frame_width(),
        height: app.frame_height(),
        relative_numbers: app.relative_line_numbers(),
        minimap_on: app.minimap_enabled(),
        canvas_frozen: app.canvas_is_frozen(),
        control_socket: control_socket_path().display().to_string(),
    }
}

fn buffer_list(app: &EditorApp) -> Vec<BufferInfo> {
    app.list_buffers()
}

fn read_buffer(app: &EditorApp, params: &serde_json::Value) -> Result<String, String> {
    if let Some(index) = params.get("index").and_then(|v| v.as_u64()) {
        return app.buffer_text(index as usize);
    }
    Ok(app.text())
}

fn render_snapshot(app: &EditorApp, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width.max(20), height.max(8));
    let mut terminal = Terminal::new(backend).expect("test backend");
    let _ = terminal.draw(|frame| draw(frame, app));
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol().to_string())
        .collect::<Vec<_>>()
        .chunks(width.max(20) as usize)
        .map(|row| row.join("").trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn capture(app: &EditorApp, params: &serde_json::Value) -> Result<String, String> {
    let width = params
        .get("width")
        .and_then(|v| v.as_u64())
        .unwrap_or(app.frame_width().max(80) as u64) as u16;
    let height = params
        .get("height")
        .and_then(|v| v.as_u64())
        .unwrap_or(app.frame_height().max(24) as u64) as u16;
    let text = render_snapshot(app, width, height);
    let path = if let Some(raw) = params.get("path").and_then(|v| v.as_str()) {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            app.workspace_path().join(candidate)
        }
    } else {
        let dir = app
            .workspace_path()
            .join("engineering/evidence/mcp-captures");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        dir.join(format!(
            "posit-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ))
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Stay inside workspace when possible.
    let workspace = app.workspace_path().canonicalize().unwrap_or_else(|_| app.workspace_path().to_path_buf());
    if let Ok(canon) = path.canonicalize().or_else(|_| {
        path.parent()
            .map(|p| p.canonicalize().map(|c| c.join(path.file_name().unwrap_or_default())))
            .unwrap_or_else(|| Ok(path.clone()))
    }) {
        if !canon.starts_with(&workspace) {
            return Err("capture path escapes the workspace".into());
        }
    }
    fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn control_socket_answers_ping_and_status() {
        let dir = std::env::temp_dir().join(format!(
            "posit-mcp-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("control.sock");
        std::env::set_var("POSIT_EDIT_CONTROL_SOCKET", &sock);
        let server = ControlServer::bind().unwrap();
        let mut app = EditorApp::new("fn main() {}\n");
        app.note_frame(80, 24);

        let path = server.path().to_path_buf();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            let mut stream = UnixStream::connect(&path).unwrap();
            writeln!(stream, r#"{{"id":1,"method":"ping"}}"#).unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line
        });
        for _ in 0..50 {
            server.poll(&mut app);
            if handle.is_finished() {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let line = handle.join().unwrap();
        let response: ControlResponse = serde_json::from_str(line.trim()).unwrap();
        assert!(response.ok, "{response:?}");

        // Direct dispatch coverage for status / snapshot / runtime key mapping.
        let status = dispatch(
            &mut app,
            &ControlRequest {
                id: 2,
                method: "status".into(),
                params: serde_json::json!({}),
            },
        );
        assert!(status.ok);
        let status_body = status.result.as_ref().unwrap();
        assert_eq!(status_body["minimap_on"], true);
        assert_eq!(status_body["canvas_frozen"], false);
        app.control_command("minimap").unwrap();
        let status2 = dispatch(
            &mut app,
            &ControlRequest {
                id: 22,
                method: "status".into(),
                params: serde_json::json!({}),
            },
        );
        assert_eq!(status2.result.unwrap()["minimap_on"], false);
        let snap = dispatch(
            &mut app,
            &ControlRequest {
                id: 3,
                method: "snapshot".into(),
                params: serde_json::json!({"width": 60, "height": 20}),
            },
        );
        assert!(snap.ok);
        let result = snap.result.unwrap();
        let text = result["text"].as_str().unwrap();
        assert!(text.contains("main") || text.contains("Posit") || text.contains("fn"));
        let _ = fs::remove_dir_all(dir);
    }
}
