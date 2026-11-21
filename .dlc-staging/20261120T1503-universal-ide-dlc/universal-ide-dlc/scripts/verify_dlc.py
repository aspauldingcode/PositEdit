#!/usr/bin/env python3
"""Produce a truthful, local verification report for DLC targets.
This script never claims hardware or native-vendor parity from missing tools.
"""
from __future__ import annotations
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]

def command(cmd,timeout=90):
    try:
        p=subprocess.run(cmd,cwd=ROOT,capture_output=True,text=True,timeout=timeout)
        return {'status':'pass' if p.returncode==0 else 'fail','exit':p.returncode,
                'stdout_tail':p.stdout[-6000:],'stderr_tail':p.stderr[-3000:]}
    except (OSError,subprocess.TimeoutExpired) as e:
        return {'status':'error','message':str(e)}

def main():
    report={'schema':'dlc-verification-v1','when_epoch':time.time(),
            'host':{'os':platform.system(),'arch':platform.machine(),'python':sys.version.split()[0]},
            'tests':{},'capabilities':{},'release_ready':False,
            'reason':'Device, GPU, vendor native-model roundtrip, Rust host integration and platform matrices are not automatically proven'}
    report['tests']['python_unit']=command([sys.executable,'-m','unittest','discover','-s','tests','-q'])
    report['tests']['python_compile']=command([sys.executable,'-m','compileall','-q','project_dlc'])
    report['tests']['graph_dag']=command([sys.executable,'engineering/graph/validate.py'])
    for item,exe in [('rust_cargo','cargo'),('nix','nix'),('bend','bend'),('cmake','cmake'),('meson','meson'),('gradle','gradle'),('maven','mvn'),('swift','swift'),('xcodebuild','xcodebuild'),('xtool','xtool'),('terminal_phone','terminal-phone'),('terminal_browser','terminal-browser'),('android_emulator','emulator'),('adb','adb')]:
        report['capabilities'][item]={'installed':bool(shutil.which(exe)),'status':'presence_only_not_verified'}
    out=ROOT/'verification_report.json'
    out.write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
    print(json.dumps({'report':str(out),'passive_tests':{k:v['status'] for k,v in report['tests'].items()},
                      'release_ready':False},indent=2))
    return 0 if all(x['status']=='pass' for x in report['tests'].values()) else 1

if __name__=='__main__':raise SystemExit(main())
