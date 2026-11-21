"""Supervised execution of reviewed argv plans. No shell evaluation."""
from __future__ import annotations
import os
import shutil
import signal
import subprocess
import threading
import time
from pathlib import Path
from .core import Plan, CapabilityError

def execute(plan: Plan, *, timeout: float = 600, allow: bool = False, log=None) -> dict:
    if not allow:
        raise CapabilityError('Refused to execute untrusted project code without explicit authorization')
    if not plan.argv or not Path(plan.cwd).is_dir():
        raise CapabilityError('Malformed executable plan')
    cmd = plan.argv[0]
    if not Path(cmd).is_file() and not shutil.which(cmd):
        raise CapabilityError(f'Required tool missing: {cmd}')
    if Path(cmd).is_file() and not os.access(cmd,os.X_OK):
        raise CapabilityError(f'Executable permission missing: {cmd}')
    kwargs = {'cwd':plan.cwd,'stdout':subprocess.PIPE,'stderr':subprocess.STDOUT,
              'text':True,'bufsize':1}
    if os.name=='posix': kwargs['start_new_session']=True
    start=time.monotonic()
    p=subprocess.Popen(plan.argv,**kwargs)
    buffer=[]
    seen = [0]
    assert p.stdout is not None
    def reader():
        for line in p.stdout:
            seen[0] += 1
            if log: log(line.rstrip('\n'))
            if len(buffer)<500: buffer.append(line)
    thread=threading.Thread(target=reader,daemon=True); thread.start()
    expired=False
    try:
        p.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        expired=True
        if os.name=='posix':
            try: os.killpg(p.pid,signal.SIGTERM)
            except ProcessLookupError: pass
        else: p.terminate()
        try: p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            if os.name=='posix':
                try: os.killpg(p.pid,signal.SIGKILL)
                except ProcessLookupError: pass
            else: p.kill()
            p.wait()
    thread.join(timeout=2)
    p.stdout.close()
    return {'exit_code':p.returncode,'timed_out':expired,'elapsed_seconds':round(time.monotonic()-start,3),
            'captured_output':''.join(buffer),'output_truncated':seen[0]>500,'plan':plan.asdict()}


def execute_interactive(plan: Plan, *, allow: bool = False) -> dict:
    """For third-party viewers that must own their terminal/stdin; no stdout capture.

    Launch commands only after explicit authorization. The full-screen viewer should
    use a native terminal split or dedicated pane rather than sharing TUI stdout.
    """
    if not allow:
        raise CapabilityError('Interactive execution requires explicit authorization')
    if not plan.argv or not Path(plan.cwd).is_dir():
        raise CapabilityError('Malformed interactive execution plan')
    if not Path(plan.argv[0]).is_file() and not shutil.which(plan.argv[0]):
        raise CapabilityError(f'Required tool missing: {plan.argv[0]}')
    start=time.monotonic()
    process=subprocess.run(plan.argv,cwd=plan.cwd,check=False)
    return {'exit_code':process.returncode,'elapsed_seconds':round(time.monotonic()-start,3),
            'mode':'interactive_tty','plan':plan.asdict()}
