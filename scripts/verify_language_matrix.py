#!/usr/bin/env python3
"""Check independently recorded runtime observations for all promised languages.

Does not *create* tests or accept an entirely synthetic checked box: requires raw logs.
"""
import json, pathlib, sys, hashlib
ROOT=pathlib.Path(__file__).resolve().parents[1]
FILE=ROOT/'engineering/evidence/runtime/language-matrix.json'
EXPECTED=['python','rust','c','zig','nix','java','kotlin','swift','typescript','javascript','unknown']
if not FILE.exists(): sys.exit('BLOCKED: no observed language-matrix.json; run real test drivers')
try: data=json.loads(FILE.read_text())
except (ValueError,OSError) as exc:sys.exit('FAIL: invalid matrix: '+str(exc))
fail=[]
for name in EXPECTED:
    report=data.get('languages',{}).get(name)
    if not report:fail.append(name+': missing');continue
    capabilities=['syntax','lsp','build','tests','tui']
    if name=='unknown':capabilities=['syntax','tui']
    for capability in capabilities:
        entry=report.get(capability, {})
        if entry.get('status')!='pass':fail.append(f'{name}/{capability}: not pass');continue
        raw=entry.get('evidence_file','');sha=entry.get('sha256','')
        if not raw or not sha:fail.append(f'{name}/{capability}: no immutable raw evidence');continue
        path=(ROOT/raw).resolve()
        if not path.is_relative_to(ROOT.resolve()) or not path.is_file():
            fail.append(f'{name}/{capability}: invalid evidence path');continue
        if hashlib.sha256(path.read_bytes()).hexdigest()!=sha:
            fail.append(f'{name}/{capability}: mismatch evidence hash')
    if name!='nix' and name!='unknown':
        entry=report.get('debug',{})
        if entry.get('status')!='pass':fail.append(f'{name}/debug: not pass')
if fail:
    print('BLOCKED matrix verification:\n'+'\n'.join(fail));sys.exit(1)
print('Language matrix evidence references are complete and hash-valid. Human/tool review still required to establish observations are genuine.')
