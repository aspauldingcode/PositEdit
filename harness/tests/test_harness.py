import importlib.util, os, pathlib, sqlite3, tempfile, unittest
ROOT=pathlib.Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('ide_harness',ROOT/'scripts/harness.py')
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
class GraphTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.original=h.DB
        h.DB=pathlib.Path(self.tmp.name)/'test.sqlite'
        self.conn=h.connection();h.init(self.conn)
    def tearDown(self):
        self.conn.close();h.DB=self.original;self.tmp.cleanup()
    def test_acyclic_seed_and_idempotent_init(self):
        before=self.conn.execute('select count(*) from nodes').fetchone()[0]
        h.init(self.conn)
        self.assertEqual(before,self.conn.execute('select count(*) from nodes').fetchone()[0])
    def test_only_root_ready_at_start(self):
        self.assertEqual([r['id'] for r in h.ready_tasks(self.conn)],['TASK-000'])
    def test_dependency_unlock(self):
        self.conn.execute("UPDATE nodes SET status='done' WHERE id='TASK-000'");self.conn.commit()
        ready={r['id'] for r in h.ready_tasks(self.conn)}
        self.assertIn('TASK-010',ready);self.assertIn('TASK-020',ready)
        self.assertNotIn('TASK-030',ready)
    def test_law_is_accepted(self):
        self.assertEqual(self.conn.execute("select status from nodes where id='LAW-001'").fetchone()[0],'accepted')
    def test_recursion_guard_marker_is_in_agent_prompt(self):
        task=h.task_data(self.conn.execute("select * from nodes where id='TASK-000'").fetchone())
        self.assertIn('NEVER spawn',h.task_prompt(task))
    def test_scrub_bearer(self):
        self.assertNotIn('secrettext',h.scrub('Authorization: Bearer secrettext'))
    def test_process_log_and_exit_code(self):
        import sys
        out=pathlib.Path(self.tmp.name)/'run.log'
        code, seconds, tail=h.run_command([sys.executable,'-c','print("GOOD")'],out,10)
        self.assertEqual(code,0)
        self.assertIn('GOOD',out.read_text())
        self.assertGreaterEqual(seconds,0)
    def test_process_timeout_is_not_green(self):
        import sys
        out=pathlib.Path(self.tmp.name)/'timeout.log'
        code,seconds,tail=h.run_command([sys.executable,'-c','import time;time.sleep(5)'],out,.15)
        self.assertEqual(code,124)
        self.assertIn('TIMEOUT',out.read_text())
if __name__=='__main__':unittest.main()
