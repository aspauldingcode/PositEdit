# Project-specific MCP strategy

`python3 scripts/configure_mcp.py` generates `.cursor/mcp.json` only for this repository. It never updates home-wide MCP configuration. Cursor CLI reads project MCP settings from `.cursor/mcp.json`.

### Baseline

1. **posit-edit** (preferred): `target/debug/posit-edit-mcp` proxies agent tools to a live Posit Edit session over a Unix control socket. Build with `cargo build -p ide-mcp -p ide`. Launch the IDE first; the session binds `$XDG_RUNTIME_DIR/posit-edit.control.sock` (override with `POSIT_EDIT_CONTROL_SOCKET`).
2. Filesystem MCP: official `@modelcontextprotocol/server-filesystem` confined to this project root.
3. Git MCP: optional `uvx mcp-server-git --repository <root>` when `uvx` exists.
4. Documentation fetch MCP: optional, off by default (`IDE_ENABLE_FETCH_MCP=1`).

Re-run configure with merge:

```bash
cargo build -p ide-mcp -p ide
python3 scripts/configure_mcp.py
# or force rewrite: IDE_FORCE_MCP_CONFIG=1 python3 scripts/configure_mcp.py
```

### Posit Edit agent tools

| Tool | Purpose |
|------|---------|
| `posit_ping` | Reachability of the live IDE control socket |
| `posit_status` | Workspace, focus, notice, buffer, runtime dock, size |
| `posit_buffers` | Open buffers (dirty / cursor / revision) |
| `posit_read_buffer` | Focused (or indexed) buffer text |
| `posit_snapshot` | Plain-text UI grid — no Ghostty / Computer Use |
| `posit_open` | Open a file in the live session |
| `posit_key` | Send keys (`esc`, `tab`, `6`, `ctrl-s`, …) |
| `posit_command` | Named commands (`save`, `terminal.toggle`, `focus.runtime`, …) |
| `posit_runtime` | Open `browser` / `phone` / `android` dock panels |
| `posit_capture` | Write a text snapshot under `engineering/evidence/` |

Example (stdio JSON-RPC once the IDE is up):

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"cli","version":"0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"posit_status","arguments":{}}}' \
  | ./target/debug/posit-edit-mcp
```

### Transport / capability concerns

MCP tool access can exceed the code agent's sandbox depending on host implementation. Use project-scoped directories, no auth keys in JSON, no unrestricted shell MCP, and explicit capability checks. The Posit control socket is local-only (Unix domain). Do not expose it over TCP.

Official references: https://github.com/modelcontextprotocol/servers and https://modelcontextprotocol.io/specification
