#!/usr/bin/env python3
"""Strict initial release checklist. Placeholder crates or missing locks block release."""
import pathlib, sys,re
ROOT=pathlib.Path(__file__).resolve().parents[1]
CRATES=['ide-core','ide-tui','ide-syntax','ide-process','ide-lsp','ide-dap',
        'ide-navigation','ide-git','ide-test','ide-profile','ide-ai','ide-mcp','ide-remote','ide-packages']
missing=[]
for name in CRATES:
    p=ROOT/'crates'/name/'src/lib.rs'; content=p.read_text() if p.exists() else ''
    if not re.search(r'pub\s+const\s+IMPLEMENTED\s*:\s*bool\s*=\s*true\s*;',content):
        missing.append(f'{name}: full implementation not declared')
for name in ('Cargo.lock','flake.lock','engineering/state/verification-full.json',
             'engineering/evidence/runtime/language-matrix.json',
             'engineering/evidence/runtime/performance.json'):
    if not (ROOT/name).is_file():missing.append(name+': missing')
if missing:
    print('RELEASE BLOCKED:\n'+'\n'.join(missing));sys.exit(1)
print('Manifest presence checks passed; real tests and proof verdicts are still mandatory.')
