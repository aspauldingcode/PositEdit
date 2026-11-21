#!/usr/bin/env python3
"""Opt-in, non-overwriting DLC overlay installer for a Rust workspace.

Does not wire the toolbar into the renderer; that's a deliberate host-specific
integration step. Does not copy or duplicate old source files into this DLC.
"""
from pathlib import Path
import argparse
import shutil
import sys


def apply(host:Path, *, execute:bool=False)->list[str]:
    source=Path(__file__).resolve().parents[1]
    host=host.resolve(strict=True)
    if not (host/'Cargo.toml').is_file():raise ValueError('Host is not an existing Cargo workspace')
    tui=host/'crates'/'ide-tui'/'Cargo.toml'
    if not tui.is_file(): raise ValueError('Host IDE TUI crate missing: crates/ide-tui/Cargo.toml')
    source_crate=source/'integration'/'rust'/'ide-dlc-ui'
    dest_crate=host/'crates'/'ide-dlc-ui'
    target_pkg=host/'extensions'/'project-dlc'
    if dest_crate.exists() or target_pkg.exists():
        raise ValueError('DLC files already present; avoid overwriting or duplicating existing changes')
    manifest=tui.read_text('utf-8')
    if 'ide-dlc-ui' in manifest:raise ValueError('Dependency already present in target TUI manifest')
    header='[dependencies]'
    if header not in manifest:raise ValueError('TUI manifest missing [dependencies] section')
    backup=tui.with_name('Cargo.toml.before-dlc')
    if backup.exists():raise ValueError('Backup already exists: '+str(backup))
    changed=manifest.replace(header,header+'\nide-dlc-ui = { path = "../ide-dlc-ui" }',1)
    actions=[f'COPY {source_crate} -> {dest_crate}',f'COPY DLC Python package -> {target_pkg}',f'EDIT {tui} (add dependency only)']
    if execute:
        shutil.copytree(source_crate,dest_crate)
        target_pkg.mkdir(parents=True)
        shutil.copytree(source/'project_dlc',target_pkg/'project_dlc',ignore=shutil.ignore_patterns('__pycache__','*.pyc'))
        shutil.copy2(source/'dlc.py',target_pkg/'dlc.py')
        shutil.copy2(tui,backup)
        tui.write_text(changed,encoding='utf-8')
        actions.append(f'BACKUP {backup}')
    return actions

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--host',required=True);p.add_argument('--apply',action='store_true')
    a=p.parse_args()
    try:
        for line in apply(Path(a.host),execute=a.apply):print(line)
        print('Overlay staged. Next: integrate WorkbenchToolbar render/events in host source; no automatic host rewrites.')
    except (ValueError,OSError) as e:
        print('Error: '+str(e),file=sys.stderr);sys.exit(2)
