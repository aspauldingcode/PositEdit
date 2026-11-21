#!/usr/bin/env python3
"""Write project-scoped Cursor MCP configuration, never a home-wide config."""
import json, os, pathlib, shutil, sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
def main():
    out=ROOT/'.cursor/mcp.json';out.parent.mkdir(parents=True,exist_ok=True)
    if out.exists():
        print('Existing MCP config unchanged:',out)
        return 0
    servers={}
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
    out.write_text(json.dumps({'mcpServers':servers},indent=2)+'\n')
    os.chmod(out,0o600)
    print('Project-scoped config:',out)
    print('Enabled:',', '.join(servers) or '(none; existing CLI tools still work)')
    print('Note: installed MCP packages and tool permissions must still be verified at runtime.')
    return 0
if __name__=='__main__':sys.exit(main())
