#!/usr/bin/env python3
"""Write project-scoped Cursor MCP configuration, never a home-wide config."""
import json, os, pathlib, shutil, sys
ROOT=pathlib.Path(__file__).resolve().parents[1]

def _posit_edit_mcp(servers: dict) -> None:
    """Prefer the in-repo MCP bridge that drives a live Posit Edit session."""
    binary = ROOT / 'target' / 'debug' / 'posit-edit-mcp'
    if not binary.is_file():
        binary = ROOT / 'target' / 'release' / 'posit-edit-mcp'
    if not binary.is_file():
        return
    # Do not hard-code the socket path: the IDE publishes ~/.cache/posit-edit/control.path
    # and posit-edit-mcp resolves it (TMPDIR can differ between Cursor and Ghostty).
    servers['posit-edit'] = {
        'command': str(binary),
        'args': [],
    }

def main():
    out=ROOT/'.cursor/mcp.json';out.parent.mkdir(parents=True,exist_ok=True)
    force = os.environ.get('IDE_FORCE_MCP_CONFIG') == '1' or '--force' in sys.argv
    servers={}
    if out.exists() and not force:
        # Merge posit-edit into an existing config when missing.
        try:
            existing=json.loads(out.read_text())
            servers=dict(existing.get('mcpServers') or {})
        except Exception:
            print('Existing MCP config unchanged:',out)
            return 0
        if 'posit-edit' in servers:
            print('Existing MCP config unchanged:',out)
            return 0
        _posit_edit_mcp(servers)
        if 'posit-edit' not in servers:
            print('Existing MCP config unchanged:',out)
            return 0
        out.write_text(json.dumps({'mcpServers':servers},indent=2)+'\n')
        os.chmod(out,0o600)
        print('Merged posit-edit into',out)
        return 0
    if shutil.which('npx'):
        servers['project-filesystem']={
            'command':shutil.which('npx'),
            'args':['--yes','@modelcontextprotocol/server-filesystem@2.3.0',str(ROOT)]}
    if shutil.which('uvx') and (ROOT/'.git').exists():
        servers['project-git']={
            'command':shutil.which('uvx'),
            'args':['mcp-server-git','--repository',str(ROOT)]}
        if os.environ.get('IDE_ENABLE_FETCH_MCP')=='1':
            servers['documentation-fetch']={'command':shutil.which('uvx'),'args':['mcp-server-fetch']}
    _posit_edit_mcp(servers)
    out.write_text(json.dumps({'mcpServers':servers},indent=2)+'\n')
    os.chmod(out,0o600)
    print('Project-scoped config:',out)
    print('Enabled:',', '.join(servers) or '(none; existing CLI tools still work)')
    print('Note: installed MCP packages and tool permissions must still be verified at runtime.')
    print('Launch Posit Edit first so posit_* tools can reach the control socket.')
    return 0
if __name__=='__main__':sys.exit(main())
