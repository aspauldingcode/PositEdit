#!/usr/bin/env python3
"""Strict additive planning-bundle overlay. Does NOT implement IDE features."""
import argparse, pathlib, shutil, sys
p=argparse.ArgumentParser()
p.add_argument('--host',required=True)
p.add_argument('--apply',action='store_true')
a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[1]
host=pathlib.Path(a.host).expanduser().resolve()
if not host.is_dir() or not (host/'Cargo.toml').is_file(): sys.exit('Host must be an existing Cargo workspace; no files changed')
if host.is_symlink(): sys.exit('Refusing symlink host')
dest=host/'extensions'/'dlc4-spatial-intelligence-plan'
if dest.exists() or dest.is_symlink(): sys.exit(f'Destination exists; refusing overwrite: {dest}')
sources=['README.md','DLC4_ONE_SHOT_AGENT_PROMPT.md','architecture','contracts','engineering','scripts','tests']
print('Additive overlay destination:',dest)
for name in sources: print('  +',name)
if not a.apply:
 print('DRY RUN. No files changed. Apply with --apply after review.')
 sys.exit(0)
dest.parent.mkdir(parents=True,exist_ok=True)
try:
 dest.mkdir(exist_ok=False)
 for name in sources:
  src=root/name; target=dest/name
  if src.is_dir():
   shutil.copytree(src,target,symlinks=False)
  else:
   with src.open('rb') as inp, target.open('xb') as out: shutil.copyfileobj(inp,out)
except Exception as e:
 print('Partial additive overlay may exist; inspect before retry:',e,file=sys.stderr)
 sys.exit(1)
print('Planning bundle installed. No Cargo manifests or existing source files modified. Agent must implement features.')
