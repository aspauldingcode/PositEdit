#!/usr/bin/env python3
"""Read-only host/tooling inventory; writes a report with no secrets."""
import json, os, pathlib, platform, shutil, subprocess, sys, datetime
ROOT=pathlib.Path(__file__).resolve().parents[1]
def version(command):
    if not shutil.which(command[0]):return None
    try:
        p=subprocess.run(command,capture_output=True,text=True,timeout=8,stdin=subprocess.DEVNULL)
        return (p.stdout or p.stderr).strip().split('\n')[0][:250]
    except Exception as ex:return f'unavailable: {type(ex).__name__}'
def run():
    names={'git':['git','--version'],'nix':['nix','--version'],'rustc':['rustc','--version'],
      'cargo':['cargo','--version'],'bend':['bend','version'],'xcode-select':['xcode-select','-p'],
      'clang':['clang','--version'],'agent':['agent','--version'],'codex':['codex','--version'],
      'claude':['claude','--version'],'node':['node','--version'],'npm':['npm','--version'],
      'uvx':['uvx','--version'],'tmux':['tmux','-V'],'python3':['python3','--version']}
    record={'checked':datetime.datetime.now(datetime.timezone.utc).isoformat(),
      'os':platform.system(),'machine':platform.machine(),'release':platform.release(),
      'tools':{k:{'path':shutil.which(k),'version':version(v)} for k,v in names.items()},
      'blockers':[]}
    if platform.system()=='Darwin' and not shutil.which('xcode-select'):
        record['blockers'].append('Apple CLT missing; privileged/user installation required')
    if not shutil.which('cargo') and not shutil.which('nix'):
        record['blockers'].append('Neither cargo nor Nix installed; required toolchain provisioning external')
    if not shutil.which('rustc'):
        record['blockers'].append('rustc not on PATH; compile via Nix devShell (nix develop) or a host rustc. cargo alone cannot build')
    if not shutil.which('bend'):
        if shutil.which('nix'):
            record['blockers'].append('Bend 2 not on PATH; formal proof gate uses the pinned Nix devShell until a host bend exists')
        else:
            record['blockers'].append('Bend 2 not installed; formal proof gate blocked until verified installation')
    if not shutil.which('tmux'):
        record['blockers'].append('tmux not on PATH; PTY checks need the Nix devShell or a host tmux')
    if not any(shutil.which(x) for x in ('agent','codex','claude')):
        record['blockers'].append('No agent CLI; unattended AI runs unavailable until CLI installation and login')
    output=ROOT/'engineering/state/preflight.json'
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(record,indent=2)+'\n')
    os.chmod(output,0o600)
    print(json.dumps(record,indent=2))
    return 0
if __name__=='__main__':sys.exit(run())
