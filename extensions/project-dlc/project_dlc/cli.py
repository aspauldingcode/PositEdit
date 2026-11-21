"""CLI / JSON adapter to integrate via a Rust child-process bridge."""
from __future__ import annotations
import argparse
import dataclasses
import json
import shutil
import sys
from pathlib import Path
from .core import CapabilityError, detect, inspect, plan
from .config import load, save
from .execution import execute, execute_interactive
from .runtimes import capabilities, runtime_plan

def emit(item, *, pretty=True):
    print(json.dumps(item, indent=2 if pretty else None, default=lambda x: dataclasses.asdict(x)))

def choose(root: Path,system: str|None):
    found=detect(root)
    if not found: raise CapabilityError('No supported project markers found')
    if system:
        matches=[p for p in found if p.system==system]
        if not matches: raise CapabilityError(f'{system} not detected here; detected: {[p.system for p in found]}')
        return matches[0]
    if len(found)!=1:
        raise CapabilityError('Multiple build systems found; choose --system explicitly: '+', '.join(p.system for p in found))
    return found[0]

def main(argv=None):
    parser=argparse.ArgumentParser(description='Universal IDE DLC project workbench')
    parser.add_argument('--root', default='.')
    sub=parser.add_subparsers(dest='cmd',required=True)
    sub.add_parser('scan')
    s=sub.add_parser('inspect'); s.add_argument('--system')
    s=sub.add_parser('plan'); s.add_argument('action',choices=['build','run','test','configure']); s.add_argument('--system')
    s=sub.add_parser('execute'); s.add_argument('action',choices=['build','run','test','configure']); s.add_argument('--system'); s.add_argument('--approve',action='store_true'); s.add_argument('--timeout',type=float,default=600)
    s=sub.add_parser('set'); s.add_argument('key'); s.add_argument('value')
    sub.add_parser('config')
    sub.add_parser('doctor')
    s=sub.add_parser('cmake-user-preset'); s.add_argument('name'); s.add_argument('--build-type',default='Debug'); s.add_argument('--generator',default='Ninja'); s.add_argument('--apply',action='store_true')
    s=sub.add_parser('runtime'); s.add_argument('action',choices=['browser','phone-view','phone-boot','phone-launch','android-boot','android-install','android-launch','xtool-dev']); s.add_argument('--execute',action='store_true'); s.add_argument('--approve',action='store_true'); s.add_argument('--timeout',type=float,default=600)
    sub.add_parser('ui')
    args=parser.parse_args(argv)
    try:
        root=Path(args.root).expanduser().resolve(strict=True)
        cfg=load(root)
        if args.cmd=='scan': emit([dataclasses.asdict(p) for p in detect(root)])
        elif args.cmd=='inspect': emit(inspect(choose(root,args.system)))
        elif args.cmd=='plan': emit(plan(choose(root,args.system),args.action,cfg).asdict())
        elif args.cmd=='execute':
            p=plan(choose(root,args.system),args.action,cfg)
            if not args.approve: raise CapabilityError('Execute requires --approve; review plan first')
            result=execute(p,timeout=args.timeout,allow=True,log=lambda line:print(line,file=sys.stderr))
            emit(result)
            if result['exit_code']!=0: return 1
        elif args.cmd=='config': emit(cfg)
        elif args.cmd=='set':
            if args.key=='run_args': value=args.value.split(',') if args.value else []
            elif args.key=='release': value=args.value.lower() in ('true','1','yes')
            else: value=args.value
            cfg[args.key]=value
            save(root,cfg); emit({'updated':args.key,'value':value,'file':str(root/'.ide-dlc.json')})
        elif args.cmd=='runtime':
            p=runtime_plan(args.action,root,cfg)
            if args.execute:
                if not args.approve: raise CapabilityError('Runtime execution requires --approve')
                result=execute_interactive(p,allow=True) if args.action in ('browser','phone-view','android-boot','xtool-dev') else execute(p,allow=True,timeout=args.timeout,log=lambda line:print(line,file=sys.stderr))
                emit(result)
                if result['exit_code']!=0: return 1
            else: emit(p.asdict())
        elif args.cmd=='cmake-user-preset':
            from .native_config import prepare_cmake_user_preset, apply_cmake_user_preset
            preview=prepare_cmake_user_preset(root,args.name,args.build_type,args.generator)
            emit(apply_cmake_user_preset(root,preview,approved=True) if args.apply else preview)
        elif args.cmd=='doctor':
            emit({'runtimes':[dataclasses.asdict(c) for c in capabilities()],
                  'build_tools':{v:bool(shutil.which(v)) for v in ('gradle','mvn','cmake','meson','cargo','nix','swift','zig','bazel','java','emulator','adb','xcodebuild','xtool')},
                  'notice':'Tool discovery is not a proven build, device, license, or GPU acceleration check'})
        elif args.cmd=='ui':
            from .tui import run_ui
            run_ui(root)
        return 0
    except (CapabilityError,FileNotFoundError,ValueError,OSError) as exc:
        emit({'error':str(exc),'success':False})
        return 2

if __name__=='__main__':
    raise SystemExit(main())
