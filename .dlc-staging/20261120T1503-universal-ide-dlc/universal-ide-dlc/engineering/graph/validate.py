#!/usr/bin/env python3
"""Read-only DAG validation for the additive engineering graph."""
import json
from pathlib import Path
root=Path(__file__).parent
nodes=json.loads((root/'nodes.json').read_text())
edges=json.loads((root/'edges.json').read_text())
ids=[n['id'] for n in nodes]
assert len(ids)==len(set(ids)), 'Duplicate graph ID'
node_map={n['id']:n for n in nodes}
assert all(n['status'] in ('planned','candidate','passive-prototype','verified') for n in nodes)
assert all(e['from'] in ids and e['to'] in ids for e in edges)
assert {(e['from'],e['to']) for e in edges} == {(p,n['id']) for n in nodes for p in n['depends_on']}
seen,active=set(),set()
def walk(key):
    if key in active: raise AssertionError('Cycle: '+key)
    if key in seen: return
    active.add(key)
    for dep in node_map[key]['depends_on']: walk(dep)
    active.remove(key); seen.add(key)
for key in ids: walk(key)
print(f'Graph valid: {len(nodes)} nodes, {len(edges)} dependency edges, no cycles')
