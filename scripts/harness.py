#!/usr/bin/env python3
"""Persistent, bounded, headless agent-task DAG supervisor (Python stdlib only).

Not a sandbox and not an AI model. Child agent uses its own provider sandbox.
"""
import argparse, contextlib, datetime as dt, json, os, pathlib, re, shlex, shutil, signal, sqlite3, subprocess, sys, time, hashlib
ROOT = pathlib.Path(__file__).resolve().parents[1]
POLICY = json.loads((ROOT/'engineering/policy.json').read_text())
SEED = json.loads((ROOT/'engineering/graph/seed.json').read_text())
STATE = ROOT/'engineering/state'
DB = pathlib.Path(os.environ.get('IDE_GRAPH_DB', str(STATE/'graph.sqlite')))

def connection():
    DB.parent.mkdir(parents=True, exist_ok=True)
    c=sqlite3.connect(str(DB),timeout=30)
    c.row_factory=sqlite3.Row
    c.execute('PRAGMA busy_timeout=10000')
    c.executescript("""
    CREATE TABLE IF NOT EXISTS nodes(
      id TEXT PRIMARY KEY, type TEXT NOT NULL, title TEXT NOT NULL,
      phase INTEGER, priority INTEGER, risk TEXT, status TEXT NOT NULL,
      attempts INTEGER NOT NULL DEFAULT 0, data TEXT NOT NULL,
      updated TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS edges(source TEXT NOT NULL,target TEXT NOT NULL,type TEXT NOT NULL,
      PRIMARY KEY(source,target,type));
    CREATE TABLE IF NOT EXISTS evidence(id INTEGER PRIMARY KEY AUTOINCREMENT,
      node_id TEXT NOT NULL, occurred TEXT NOT NULL, kind TEXT NOT NULL,
      status TEXT NOT NULL, detail TEXT NOT NULL, file TEXT);
    CREATE TABLE IF NOT EXISTS runs(id INTEGER PRIMARY KEY AUTOINCREMENT,
      started TEXT NOT NULL, finished TEXT, provider TEXT, result TEXT);
    """)
    return c

def now(): return dt.datetime.now(dt.timezone.utc).isoformat()

def init(conn):
    ids={x['id'] for x in SEED['nodes']}
    if len(ids)!=len(SEED['nodes']): raise ValueError('Duplicate graph ids')
    for edge in SEED['edges']:
        if edge['source'] not in ids or edge['target'] not in ids: raise ValueError('Dangling edge: '+repr(edge))
    for n in SEED['nodes']:
        conn.execute("""INSERT OR IGNORE INTO nodes(id,type,title,phase,priority,risk,status,data,updated)
             VALUES (?,?,?,?,?,?,?,?,?)""",(n['id'],n['type'],n['title'],n.get('phase'),n.get('priority'),n.get('risk'),n.get('status','pending'),json.dumps(n),now()))
    for edge in SEED['edges']:
        conn.execute('INSERT OR IGNORE INTO edges VALUES (?,?,?)',(edge['source'],edge['target'],edge['type']))
    conn.commit()
    # Validate acyclicity (DEPENDs_ON points from node to prerequisite).
    deps={x['id']:set() for x in SEED['nodes']}
    for edge in SEED['edges']:
        if edge['type']=='DEPENDS_ON':deps[edge['source']].add(edge['target'])
    pending=set(deps)
    while pending:
        ready={t for t in pending if not deps[t].intersection(pending)}
        if not ready:raise ValueError('Dependency graph cycle: '+repr(sorted(pending)))
        pending-=ready

def task_data(row): return json.loads(row['data'])

def ready_tasks(conn, excluded=()):
    result=[]
    for row in conn.execute("SELECT * FROM nodes WHERE type='task' AND status='pending' ORDER BY priority DESC, id"):
        if row['id'] in excluded:continue
        deps=conn.execute("SELECT target FROM edges WHERE source=? AND type='DEPENDS_ON'",(row['id'],)).fetchall()
        if all((r:=conn.execute('SELECT status FROM nodes WHERE id=?',(dep[0],)).fetchone()) and r[0]=='done' for dep in deps):
            result.append(row)
    return result

def record(conn, node_id, kind, status, detail, path=None):
    conn.execute('INSERT INTO evidence(node_id,occurred,kind,status,detail,file) VALUES (?,?,?,?,?,?)',
                 (node_id,now(),kind,status,detail,str(path) if path else None))
    conn.commit()

def status(conn):
    print('Posit Edit graph:',str(DB))
    for row in conn.execute("SELECT status,COUNT(*) cnt FROM nodes WHERE type='task' GROUP BY status ORDER BY status"):
        print('  %-12s %d'%(row['status'],row['cnt']))
    ready=ready_tasks(conn)
    print('Ready:',', '.join(x['id'] for x in ready[:8]) or '(none)')
    for row in conn.execute("SELECT id,title,attempts FROM nodes WHERE type='task' AND status='blocked' ORDER BY id"):
        print('BLOCKED:',row['id'],row['title'],f"attempts={row['attempts']}")

def scrub(text):
    # Best effort only; never log raw environment variables or authentication tokens.
    for pat in (r'(?i)(authorization\s*[:=]\s*bearer\s+)\S+',
                r'(?i)((?:api[_-]?key|access[_-]?token|password|secret)\s*[:=]\s*)\S+',
                r'(?i)(ghp_|github_pat_|sk-)[a-zA-Z0-9_-]{12,}'):
        text=re.sub(pat,lambda m:(m.group(1) if m.lastindex else '')+'[REDACTED]',text)
    return text

def provider_command(provider, prompt):
    if provider=='cursor':
        return ['agent','-p',prompt,'--output-format','text','--sandbox','enabled']
    if provider=='codex':
        return ['codex','exec','--sandbox','workspace-write',prompt]
    if provider=='claude':
        return ['claude','-p',prompt,'--output-format','text','--permission-mode','auto']
    raise ValueError('Unknown provider: '+provider)

def detect_provider():
    requested=os.environ.get('IDE_AGENT','auto').strip().lower()
    if requested=='auto':
        for name,binary in [('cursor','agent'),('codex','codex'),('claude','claude')]:
            if shutil.which(binary):return name
        return None
    if requested not in ('cursor','codex','claude'):raise ValueError('IDE_AGENT must be auto/cursor/codex/claude')
    binary={'cursor':'agent','codex':'codex','claude':'claude'}[requested]
    return requested if shutil.which(binary) else None

def run_command(cmd, log_path, timeout, env=None):
    """Run with bounded time and disk-backed output; terminate Unix process group on timeout."""
    import tempfile
    log_path.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    try:
        with tempfile.TemporaryFile() as output:
            proc = subprocess.Popen(cmd, cwd=ROOT, env=env, stdin=subprocess.DEVNULL,
                                    stdout=output, stderr=subprocess.STDOUT,
                                    start_new_session=(os.name == 'posix'))
            timed_out = False
            try:
                proc.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                if os.name == 'posix':
                    try: os.killpg(proc.pid, signal.SIGTERM)
                    except ProcessLookupError: pass
                else:
                    proc.terminate()
                try: proc.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    if os.name == 'posix':
                        try: os.killpg(proc.pid, signal.SIGKILL)
                        except ProcessLookupError: pass
                    else: proc.kill()
                    proc.wait(timeout=4)
            output.seek(0)
            raw=output.read(int(POLICY['max_log_bytes'])).decode('utf-8','replace')
            tail_note='\n[OUTPUT TRUNCATED TO POLICY LIMIT]' if output.read(1) else ''
            payload=scrub(raw+tail_note)
            if timed_out:payload+='\n[TASK TIMEOUT; PROCESS GROUP TERMINATED]'
            log_path.write_text(payload,encoding='utf-8')
            os.chmod(log_path,0o600)
            return (124 if timed_out else proc.returncode),round(time.monotonic()-started,2),payload[-2000:]
    except OSError as exc:
        log_path.write_text('EXEC FAILURE: '+str(exc),encoding='utf-8')
        os.chmod(log_path,0o600)
        return 127,round(time.monotonic()-started,2),str(exc)

def task_prompt(data):
    return f"""Read AGENTS.md, ONE_SHOT_PROMPT.md, ARCHITECTURE.md, engineering/VERIFICATION.md.
You are a CHILD TASK WORKER inside the outer harness. IDE_HARNESS_CHILD=1. NEVER spawn a coding agent or run harness.py run.
Assigned graph task: {data['id']} — {data['title']}.
Scope: {json.dumps(data.get('file_scope'))}.
Requirements: {data.get('description')}.
Acceptance commands: {json.dumps(data.get('acceptance_commands'))}.
Requires verified Bend proof: {data.get('requires_proof')}.
Implement and verify the feature in this codebase, not merely a plan. Preserve accepted LAWS.bend and SPEC.bend. Run targeted tests, record concrete outcomes, do not claim tools you did not run. Do not push/publish/spend/escalate OS permissions. If blocked, report exact root cause. End with a concise list of changed paths and test results."""

_TOOLCHAIN=None
def toolchain():
    """Load scripts/toolchain.py by path so file-location imports in tests still work."""
    global _TOOLCHAIN
    if _TOOLCHAIN is None:
        import importlib.util
        path=ROOT/'scripts'/'toolchain.py'
        spec=importlib.util.spec_from_file_location('axiom_toolchain', path)
        mod=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        _TOOLCHAIN=mod
    return _TOOLCHAIN

def verify_task(conn,row,log_prefix,timeout):
    data=task_data(row)
    tests=list(data.get('acceptance_commands',[]))
    if data.get('requires_proof') and 'bend PROOF.bend --verdict' not in tests:
        tests.append('bend PROOF.bend --verdict')
    ok=True
    for crate in data.get('required_implemented_crates',[]):
        path=ROOT/'crates'/crate/'src/lib.rs'
        source=path.read_text() if path.exists() else ''
        if not re.search(r'pub\s+const\s+IMPLEMENTED\s*:\s*bool\s*=\s*true\s*;',source):
            record(conn,row['id'],'completion','fail',f'{crate} is missing IMPLEMENTED = true; implementation incomplete')
            ok=False
    if 'python3 scripts/check_governance.py' not in tests:
        tests.append('python3 scripts/check_governance.py')
    for idx,script in enumerate(tests):
        original=shlex.split(script)
        argv=toolchain().wrap(original)
        if not argv or not shutil.which(argv[0]):
            record(conn,row['id'],'acceptance','blocked','Missing executable: '+(original[0] if original else '<empty>'))
            ok=False;continue
        rc,seconds,tail=run_command(argv,log_prefix.with_name(log_prefix.name+f'.check{idx}.log'),timeout)
        detail=f'{script} => rc={rc} seconds={seconds}'
        if argv!=original:
            detail+=' via='+' '.join(argv[:8])
        if original[:3]==['bend','PROOF.bend','--verdict']:
            if 'ALL PROOFS CHECK' not in tail: rc=1;detail+=' verdict missing'
        record(conn,row['id'],'acceptance','pass' if rc==0 else 'fail',detail,log_prefix.with_name(log_prefix.name+f'.check{idx}.log'))
        if rc!=0:ok=False
    return ok

def execute_task(conn,row,provider):
    data=task_data(row); task_id=row['id']
    attempts=row['attempts']+1
    conn.execute("UPDATE nodes SET status='running',attempts=?,updated=? WHERE id=?",(attempts,now(),task_id));conn.commit()
    logdir=STATE/'logs';logdir.mkdir(parents=True,exist_ok=True)
    logfile=logdir/(task_id+'-'+str(attempts)+'.log')
    env=os.environ.copy();env['IDE_HARNESS_CHILD']='1';env['IDE_TASK_ID']=task_id
    cmd=provider_command(provider,task_prompt(data))
    rc,seconds,tail=run_command(cmd,logfile,int(POLICY['task_timeout_seconds']),env)
    record(conn,task_id,'agent','pass' if rc==0 else 'fail',f'provider={provider} rc={rc} duration={seconds}',logfile)
    if rc==0:
        ok=verify_task(conn,row,logfile, min(1200,int(POLICY['task_timeout_seconds'])))
    else:ok=False
    if ok:
        conn.execute("UPDATE nodes SET status='done',updated=? WHERE id=?",(now(),task_id))
        print(f'PASS {task_id} ({seconds}s)')
    else:
        retries=int(data.get('max_attempts',POLICY['max_attempts_per_task']))
        next_status='blocked' if attempts>=retries else 'pending'
        conn.execute('UPDATE nodes SET status=?,updated=? WHERE id=?',(next_status,now(),task_id))
        record(conn,task_id,'result',next_status,'Agent or acceptance gate failed; see logfile',logfile)
        print(f'{next_status.upper()} {task_id} ({seconds}s) log={logfile}')
    conn.commit()
    return ok

@contextlib.contextmanager
def run_lock():
    STATE.mkdir(parents=True,exist_ok=True)
    path=STATE/'run.lock'
    with path.open('a+') as f:
        if os.name=='posix':
            import fcntl
            try:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
            except BlockingIOError:raise RuntimeError('Another harness run is active')
        yield

def report(conn):
    status(conn)
    print('\nRecent evidence:')
    for e in conn.execute('SELECT * FROM evidence ORDER BY id DESC LIMIT 20'):
        print(f"{e['occurred']} {e['node_id']} {e['kind']}/{e['status']}: {e['detail']}")

def main():
    ap=argparse.ArgumentParser(description=__doc__)
    sub=ap.add_subparsers(dest='action',required=True)
    for name in ('init','status','next','report'):sub.add_parser(name)
    rp=sub.add_parser('run');rp.add_argument('--max-tasks',type=int,default=int(os.environ.get('IDE_MAX_TASKS',POLICY['max_tasks_per_run'])))
    args=ap.parse_args()
    if args.action=='run' and os.environ.get('IDE_HARNESS_CHILD')=='1':
        sys.exit('Refusing recursive harness launch from agent worker')
    with connection() as conn:
        init(conn)
        if args.action=='init':print('Graph initialized:',str(DB))
        elif args.action=='status':status(conn)
        elif args.action=='next':
            ready=ready_tasks(conn)
            print(json.dumps(task_data(ready[0]) if ready else {'status':'no_ready_tasks'},indent=2))
        elif args.action=='report':report(conn)
        elif args.action=='run':
            if args.max_tasks<1 or args.max_tasks>100:sys.exit('max-tasks must be 1..100')
            provider=detect_provider()
            if not provider:sys.exit('No authenticated agent CLI found. Install/login separately, then resume. Graph preserved.')
            with run_lock():
                rid=conn.execute('INSERT INTO runs(started,provider,result) VALUES (?,?,?)',(now(),provider,'running')).lastrowid
                conn.commit();excluded=set();count=0
                started=time.monotonic()
                while count<args.max_tasks and time.monotonic()-started<int(POLICY.get('max_run_wall_seconds',28800)):
                    ready=ready_tasks(conn,excluded)
                    if not ready:
                        # All available nodes have been tried this pass: reset exclusions
                        # so failed but retriable nodes receive bounded further attempts.
                        excluded=set()
                        ready=ready_tasks(conn)
                        if not ready:break
                    row=ready[0];excluded.add(row['id']);count+=1
                    execute_task(conn,row,provider)
                conn.execute('UPDATE runs SET finished=?,result=? WHERE id=?',(now(),'stopped_bounded',rid));conn.commit()
                status(conn)
            print('Run checkpointed. Invoke again to resume. This is not a background daemon.')
if __name__=='__main__':main()
