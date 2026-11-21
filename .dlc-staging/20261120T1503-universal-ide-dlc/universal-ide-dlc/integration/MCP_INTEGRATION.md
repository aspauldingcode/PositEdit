# MCP and external tool bridge policy

The template `mcp.json.example` contains **only** an officially documented terminal-phone MCP stdio server (`terminal-phone mcp`). It requires the upstream CLI to be installed and configured on a compatible Apple Silicon macOS host. The README documents `terminal-phone mcp install --cursor / --claude / --codex`; choose one registration approach, not duplicates.

Do not invent a terminal-browser MCP endpoint. Its documented `terminal-browser action` command and embedded mode may be exposed through a new, independently implemented MCP bridge once their precise versioned API has been verified. Android Emulator uses an emulator gRPC interface, not MCP; wrap it in an explicitly scoped agent bridge if necessary. Gradle uses a Tooling API JVM process, not MCP. Xcode JSON uses Apple's Swift library; xtool/omarchy use their own CLIs.

An optional MCP broker for this DLC may expose:
- `project_scan` (passive read)
- `project_inspect_passive` (passive read)
- `project_plan` (read-only preview)
- `project_native_sync` (trust-gated, may execute project code)
- `project_config_preview` (read-only diff)
- `project_config_apply` (write + owner approval + reimport)
- `runtime_discover` (read-only)
- `runtime_observe` (authorized session)
- `runtime_input` (explicit owner consent)
- `runtime_launch` (installation and credentials restrictions)

An MCP tool must not create a user trust grant or bypass device authorization. All destructive actions require authorization, scopes, audit logs, cancellation, explicit cwd, and bounded output. Forward tool stderr and facts; never mark an unexecuted test passed.
