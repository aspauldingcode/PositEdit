#!/usr/bin/env python3
from pathlib import Path
import json, sys
root=Path(__file__).resolve().parents[1]
required=['README.md','DLC4_ONE_SHOT_AGENT_PROMPT.md','architecture/SYSTEM.md','architecture/FEATURES.md','engineering/tasks.json','contracts/spatial-contract.schema.json']
for rel in required:
 if not (root/rel).is_file(): sys.exit(f'Missing: {rel}')
tasks=json.loads((root/'engineering/tasks.json').read_text())['tasks']
assert len(tasks)==12 and len({t['id'] for t in tasks})==12
schema=json.loads((root/'contracts/spatial-contract.schema.json').read_text())
assert schema['properties']['version']['const']==1
print('DLC4 package validation passed: 12 workstreams, required architecture and contract files present')
