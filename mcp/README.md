# Project-specific MCP strategy

`python3 scripts/configure_mcp.py` generates `.cursor/mcp.json` only after detecting `npx` / `uvx`, and only for the current repository. The generated file is excluded from Git because it contains host absolute paths. It never updates home-wide MCP configuration or installs MCP servers unprompted. Cursor CLI automatically reads project MCP settings according to its official documentation.

### Baseline

1. Filesystem MCP: official reference `@modelcontextprotocol/server-filesystem@2.3.0` confined to this project root; `npx` might download it on first invocation, requiring existing network authorization.
2. Git MCP: reference `uvx mcp-server-git --repository <root>` if `uvx` exists and this is a Git repository. `uvx` may download packages; verify upstream package/version and trust policy.
3. Documentation fetch MCP: optional, off by default; use built-in web/search where present instead of additional network tools.

Other integrations (GitHub PR MCP, macOS accessibility/computer-use, cloud AI) are **not inherently necessary** for local TUI IDE development. Do not install third-party macOS control servers requiring global Accessibility permissions by default. Terminal interaction is tested through tmux/PTY and captures; true macOS UI automation is optional and subject to OS permissions.

### Transport/capability concerns

MCP tool access can exceed the code agent's sandbox depending on host implementation. Use project-scoped directories, no auth keys in JSON, no unrestricted shell MCP, and explicit capability checks. For remote MCP use authenticated endpoints with trusted TLS. Do not assume all MCP servers speak the same protocol version without negotiation.

Official references: https://github.com/modelcontextprotocol/servers and https://modelcontextprotocol.io/specification
