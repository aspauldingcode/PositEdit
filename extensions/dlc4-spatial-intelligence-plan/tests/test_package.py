import json,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class PackageTests(unittest.TestCase):
 def test_tasks(self):
  x=json.loads((ROOT/'engineering/tasks.json').read_text()); self.assertEqual(len(x['tasks']),12)
 def test_contract(self):
  x=json.loads((ROOT/'contracts/spatial-contract.schema.json').read_text());self.assertEqual(x['properties']['version']['const'],1)
 def test_agent_prompt(self):
  s=(ROOT/'DLC4_ONE_SHOT_AGENT_PROMPT.md').read_text();self.assertIn('bottom-right',s);self.assertIn('unzip -o',s)
 def test_no_host_code(self):
  self.assertFalse((ROOT/'Cargo.toml').exists())
if __name__=='__main__':unittest.main()
