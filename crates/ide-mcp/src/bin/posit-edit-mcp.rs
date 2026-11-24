//! MCP stdio server that proxies agent tools to a live Posit Edit control socket.
use ide_mcp::{
    control_socket_path, evaluate, tool_catalog, tool_to_method, ControlRequest, ControlResponse,
    ToolCall, CONTROL_TOOLS,
};
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    if let Err(error) = run() {
        eprintln!("posit-edit-mcp: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut line = String::new();
    let mut next_id = 1_u64;
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(value) => value,
            Err(error) => {
                write_line(
                    &mut stdout,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32700, "message": format!("parse error: {error}")}
                    }),
                )?;
                continue;
            }
        };
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match method {
            "initialize" => {
                write_line(
                    &mut stdout,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {"tools": {}},
                            "serverInfo": {"name": "posit-edit-mcp", "version": "0.1.0"}
                        }
                    }),
                )?;
            }
            "notifications/initialized" | "initialized" => {}
            "tools/list" => {
                let tools: Vec<Value> = tool_catalog()
                    .into_iter()
                    .map(|tool| {
                        let schema: Value =
                            serde_json::from_str(tool.schema).unwrap_or_else(|_| json!({}));
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "inputSchema": schema
                        })
                    })
                    .collect();
                write_line(
                    &mut stdout,
                    &json!({"jsonrpc":"2.0","id":id,"result":{"tools":tools}}),
                )?;
            }
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                let reply = handle_tool(&name, arguments, &mut next_id);
                write_line(&mut stdout, &json!({"jsonrpc":"2.0","id":id,"result":reply}))?;
            }
            "ping" => {
                write_line(
                    &mut stdout,
                    &json!({"jsonrpc":"2.0","id":id,"result":{}}),
                )?;
            }
            "" if msg.get("result").is_some() || msg.get("error").is_some() => {}
            other => {
                write_line(
                    &mut stdout,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": format!("method not found: {other}")}
                    }),
                )?;
            }
        }
    }
    Ok(())
}

fn handle_tool(name: &str, arguments: Value, next_id: &mut u64) -> Value {
    if !CONTROL_TOOLS.contains(&name) {
        return tool_error(format!("unknown tool: {name}"));
    }
    let decision = evaluate(
        &PathBuf::from("."),
        &ToolCall {
            name: name.to_string(),
            path: ".".into(),
            approved: true,
        },
    );
    if !matches!(decision, ide_ai::Decision::Allow) {
        return tool_error(format!("denied: {decision:?}"));
    }
    let Some(method) = tool_to_method(name) else {
        return tool_error(format!("no control method for {name}"));
    };
    let id = *next_id;
    *next_id += 1;
    let request = ControlRequest {
        id,
        method: method.to_string(),
        params: arguments,
    };
    match call_control(&request) {
        Ok(response) if response.ok => tool_text(
            response
                .result
                .unwrap_or_else(|| json!({"ok": true}))
                .to_string(),
        ),
        Ok(response) => tool_error(response.error.unwrap_or_else(|| "control error".into())),
        Err(error) => tool_error(format!(
            "{error} (socket {}) — launch Posit Edit first, or set POSIT_EDIT_CONTROL_SOCKET",
            control_socket_path().display()
        )),
    }
}

fn call_control(request: &ControlRequest) -> io::Result<ControlResponse> {
    let path = control_socket_path();
    let stream = UnixStream::connect(&path)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut stream = stream;
    let payload = serde_json::to_string(request)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    writeln!(stream, "{payload}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    serde_json::from_str(line.trim())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn tool_text(text: String) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": false
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

fn write_line(out: &mut impl Write, value: &Value) -> io::Result<()> {
    writeln!(out, "{value}")?;
    out.flush()
}
