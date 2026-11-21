#!/usr/bin/env python3
"""Detect unintended changes to user-accepted Bend specifications."""
import hashlib, json, pathlib, sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
EXPECTED=json.loads((ROOT/'engineering/governance-hashes.json').read_text())
failed=[]
for path,sha in EXPECTED.items():
    p=ROOT/path
    observed=hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else '<missing>'
    if observed!=sha:failed.append(path)
if failed:
    print('ACCEPTED GOVERNANCE CHANGED:',', '.join(failed));sys.exit(1)
print('Accepted specification hashes match baseline.')
