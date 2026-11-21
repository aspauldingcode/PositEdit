"""Standalone curses TUI with actual clickable command buttons, keyboard shortcuts,
project selection, configuration editing and live supervised job output.
It is a separate DLC demonstration; the host Rust app adopts the included UI contract.
"""
from __future__ import annotations
import curses
import queue
import subprocess
import threading
import time
from pathlib import Path
from .config import load, save
from .core import CapabilityError, detect, inspect, plan
from .execution import execute
from .runtimes import runtime_plan

BUTTONS=[('1','Build'),('2','Run'),('3','Configure'),('4','Test'),('5','Refresh'),('6','Browser'),('7','Phone'),('8','Android'),('q','Quit')]
CONFIG_KEYS=['system','run_task','main_class','build_dir','cmake_preset','build_config','run_artifact','cargo_bin','swift_product','nix_target','scheme','destination','bazel_target','build_script','run_script','test_script','browser_url','phone_session','phone_bundle','avd','android_serial','android_package','android_apk']

def button_rects(width: int):
    """Return wrapped clickable buttons on rows 2 and 3 for narrow terminals."""
    x,y=1,2
    result=[]
    for key,label in BUTTONS:
        text=f'[{key} {label}]'
        if x+len(text)>=width:
            x,y=1,y+1
        if y>3 or x+len(text)>=width:
            break
        result.append((x,x+len(text),y,key,text))
        x+=len(text)+1
    return result

def hit_test(x:int,y:int,width:int):
    return next((key for a,b,row,key,_ in button_rects(width) if row==y and a<=x<b),None)


def safe_add(win,y,x,text,width=None,attr=0):
    h,w=win.getmaxyx()
    if y<0 or y>=h or x>=w or w<=1: return
    maxlen=min(width if width is not None else w-x-1,w-x-1)
    if maxlen<=0: return
    try: win.addnstr(y,x,str(text).replace('\n',' '),maxlen,attr)
    except curses.error: pass


def run_ui(root: Path):
    def inner(stdscr):
        curses.curs_set(0)
        stdscr.keypad(True)
        curses.mousemask(curses.ALL_MOUSE_EVENTS)
        stdscr.timeout(100)
        selected=0
        detected=detect(root)
        cfg=load(root)
        logs=['DLC workbench ready. Only reviewed commands execute. Keys 1..8, Tab selects build system.']
        events=queue.Queue(maxsize=300)
        job_thread=None
        job_running=False
        info=[]
        def refresh_info():
            nonlocal info
            info=inspect(detected[selected]) if detected else {'targets':[],'variants':[],'warnings':['No build system detected']}
        refresh_info()
        def append(s):
            logs.append(str(s))
            del logs[:-200]
        def prompt(title, initial=''):
            h,w=stdscr.getmaxyx()
            safe_add(stdscr,h-2,1,' '*max(w-2,0))
            safe_add(stdscr,h-2,1,title+' ', max(w-2,0),curses.A_BOLD)
            curses.echo(); curses.curs_set(1)
            try:
                stdscr.move(h-2,min(w-2,len(title)+2))
                value=stdscr.getstr(h-2,min(w-2,len(title)+2),max(1,w-len(title)-5))
                return value.decode('utf-8','replace').strip()
            finally:
                curses.noecho(); curses.curs_set(0)
        def configure():
            choice=prompt('Config key or cmake-native:')
            if choice == 'cmake-native':
                from .native_config import prepare_cmake_user_preset, apply_cmake_user_preset
                try:
                    name=prompt('CMake user preset name:')
                    btype=prompt('Build type (Debug/Release/RelWithDebInfo/MinSizeRel):') or 'Debug'
                    preview=prepare_cmake_user_preset(root,name,btype)
                    append('Preview native CMakeUserPresets.json addition: '+name)
                    stdscr.erase()
                    safe_add(stdscr,1,2,'NATIVE CHANGE PREVIEW '+name)
                    safe_add(stdscr,2,2,str(preview['after']['configurePresets'][-1]))
                    safe_add(stdscr,3,2,'Press y to authorize, other to cancel')
                    stdscr.refresh(); stdscr.timeout(-1); answer=stdscr.getch(); stdscr.timeout(100)
                    if answer in (ord('y'),ord('Y')):
                        receipt=apply_cmake_user_preset(root,preview,approved=True)
                        append('CMake preset created: '+receipt['preset'])
                    else: append('Cancelled native preset change')
                except Exception as exc: append('Native config: '+str(exc))
                return
            if choice not in CONFIG_KEYS:
                append(f'Unsupported config field: {choice}')
                return
            value=prompt(f'{choice} (was {cfg.get(choice,"<unset>")}):')
            if not value:
                append('No change')
                return
            cfg[choice]=value
            try: save(root,cfg); append(f'Saved {choice}; native project files unchanged')
            except Exception as exc: append(f'Config rejected: {exc}')
        def confirm(p):
            append('PLAN: '+ ' '.join(p.argv))
            stdscr.erase()
            safe_add(stdscr,1,2,'EXECUTE REVIEW: '+ ' '.join(p.argv),None,curses.A_BOLD)
            safe_add(stdscr,2,2,'This can execute untrusted project code or modify build outputs.')
            safe_add(stdscr,3,2,'Press y to authorize, any other key to cancel.')
            stdscr.refresh()
            stdscr.timeout(-1)
            result=stdscr.getch()
            stdscr.timeout(100)
            return result in (ord('y'),ord('Y'))
        def enqueue(s):
            try: events.put_nowait(s)
            except queue.Full: pass
        def launch_external(p):
            # Upstream viewer consumes terminal graphics/input; do not capture its stream.
            if not confirm(p): append('Cancelled'); return
            curses.endwin()
            try:
                print('\nDLC launching external graphical runtime:', ' '.join(p.argv))
                result=subprocess.run(p.argv,cwd=p.cwd,check=False)
                append(f'External runtime exited with {result.returncode}')
            except Exception as exc: append(f'External runtime error: {exc}')
            finally:
                curses.reset_prog_mode(); stdscr.refresh()
        def begin(action):
            nonlocal job_running,job_thread
            if job_running:
                append('A job is already running.'); return
            try:
                p=plan(detected[selected],action,cfg) if detected else None
                if p is None: raise CapabilityError('No project')
                if not confirm(p): append('Cancelled'); return
                job_running=True
                append(f'Running: {p.argv!r}')
                def worker():
                    try:
                        res=execute(p,timeout=900,allow=True,log=enqueue)
                        enqueue(f'Finished: exit={res["exit_code"]}, elapsed={res["elapsed_seconds"]}s')
                    except Exception as exc: enqueue(f'Execution failure: {exc}')
                    finally: enqueue('__DONE__')
                job_thread=threading.Thread(target=worker,daemon=True)
                job_thread.start()
            except Exception as exc: append(f'{action}: {exc}')
        def runtime(which):
            try:
                p=runtime_plan(which,root,cfg)
                launch_external(p)
            except Exception as exc: append(f'{which}: {exc}')
        while True:
            try:
                while True:
                    msg=events.get_nowait()
                    if msg=='__DONE__': job_running=False
                    else: append(msg)
            except queue.Empty: pass
            h,w=stdscr.getmaxyx()
            stdscr.erase()
            safe_add(stdscr,0,1,'UNIVERSAL PROJECT WORKBENCH  |  '+str(root),None,curses.A_BOLD)
            for a,b,row,key,text in button_rects(w):
                safe_add(stdscr,row,a,text,len(text),curses.A_REVERSE)
            safe_add(stdscr,4,1,'PROJECT SYSTEMS (Tab to switch)')
            for i,p in enumerate(detected[:max(0,min(h-12,12))]):
                safe_add(stdscr,5+i,2,('> ' if i==selected else '  ')+p.system+'  '+', '.join(p.evidence[:2]),w-3,curses.A_BOLD if i==selected else 0)
            baseline=max(8,6+len(detected))
            safe_add(stdscr,baseline,1,'TARGETS: '+(', '.join(info.get('targets',[])) or '(not resolved)'),w-2)
            safe_add(stdscr,baseline+1,1,'VARIANTS: '+(', '.join(info.get('variants',[])) or '(unknown)'),w-2)
            safe_add(stdscr,baseline+2,1,'CONFIG: '+', '.join(f'{k}={v}' for k,v in cfg.items())[:200],w-2)
            safe_add(stdscr,baseline+3,1,'CAPABILITIES: '+('Job active' if job_running else 'Ready')+' | key 6 Browser, 7 Phone, 8 Android',w-2)
            log_at=min(h-7,baseline+5)
            if log_at<h-1:
                safe_add(stdscr,log_at,1,'EVENTS / OUTPUT',None,curses.A_BOLD)
                for y,line in enumerate(logs[-max(h-log_at-2,0):],log_at+1):
                    safe_add(stdscr,y,2,line,w-4)
            stdscr.refresh()
            key=stdscr.getch()
            if key==-1: continue
            if key==curses.KEY_MOUSE:
                try:
                    _,mx,my,_,_ = curses.getmouse()
                    picked=hit_test(mx,my,w)
                    if picked: key=ord(picked)
                    else: continue
                except curses.error: continue
            if key in (ord('q'),27): break
            if key in (9,curses.KEY_RIGHT,curses.KEY_DOWN):
                if detected: selected=(selected+1)%len(detected); refresh_info()
                continue
            if key in (curses.KEY_LEFT,curses.KEY_UP):
                if detected: selected=(selected-1)%len(detected); refresh_info()
                continue
            if key==ord('1'): begin('build')
            elif key==ord('2'): begin('run')
            elif key==ord('3'): configure()
            elif key==ord('4'): begin('test')
            elif key==ord('5'): detected=detect(root); selected=0; refresh_info(); append('Refreshed metadata')
            elif key==ord('6'): runtime('browser')
            elif key==ord('7'): runtime('phone-view')
            elif key==ord('8'): runtime('android-boot')
    return curses.wrapper(inner)
