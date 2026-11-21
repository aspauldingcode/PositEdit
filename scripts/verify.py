#!/usr/bin/env python3
"""Actual verification commands. Missing prerequisites count as failures."""
import argparse, pathlib, shutil, subprocess, sys, time, json, datetime
ROOT=pathlib.Path(__file__).resolve().parents[1]
_TOOLCHAIN=None
def toolchain():
    global _TOOLCHAIN
    if _TOOLCHAIN is None:
        import importlib.util
        path=ROOT/'scripts'/'toolchain.py'
        spec=importlib.util.spec_from_file_location('axiom_toolchain', path)
        mod=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        _TOOLCHAIN=mod
    return _TOOLCHAIN

def run(cmd,timeout=600):
    original=list(cmd)
    wrapped=toolchain().wrap(original)
    name=wrapped[0] if wrapped else ''
    if not wrapped or shutil.which(name) is None:
        return {'command':original,'status':'blocked','exit_code':127,'detail':'missing executable '+(original[0] if original else '<empty>')}
    limit=1800 if wrapped!=original else timeout
    try:
        p=subprocess.run(wrapped,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=limit)
        body=p.stdout[-30000:]
        good=p.returncode==0
        if original[:3]==['bend','PROOF.bend','--verdict'] and 'ALL PROOFS CHECK' not in body:good=False
        if wrapped!=original:
            body='via: '+' '.join(wrapped)+'\n'+body
        return {'command':original,'status':'pass' if good else 'fail','exit_code':p.returncode,'detail':body}
    except subprocess.TimeoutExpired:
        return {'command':original,'status':'fail','exit_code':124,'detail':'timeout'}

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--level',choices=['scaffold','full','release'],default='scaffold')
    args=parser.parse_args()
    jobs=[['python3','-m','unittest','discover','-s','harness/tests','-v'],
          ['python3','scripts/check_governance.py'],['cargo','test','--workspace']]
    if args.level in ('full','release'):
        jobs += [['cargo','fmt','--all','--','--check'],['cargo','clippy','--workspace','--all-targets','--','-D','warnings'],
                 ['bend','PROOF.bend','--verdict'],['nix','flake','check','--no-write-lock-file'],['python3','scripts/tui_smoke.py'],['python3','scripts/verify_language_matrix.py']]
    if args.level=='release':
        jobs += [['python3','scripts/check_release.py']]
    results=[]
    for cmd in jobs:
        result=run(cmd);results.append(result)
        print(result['status'].upper(),' '.join(cmd))
        if result['status']!='pass':print(result['detail'][-1000:])
    p=ROOT/'engineering/state';p.mkdir(parents=True,exist_ok=True)
    dest=p/('verification-'+args.level+'.json')
    dest.write_text(json.dumps({'timestamp':datetime.datetime.now(datetime.timezone.utc).isoformat(),
      'level':args.level,'results':results},indent=2)+'\n')
    return 0 if all(x['status']=='pass' for x in results) else 1
if __name__=='__main__':sys.exit(main())
