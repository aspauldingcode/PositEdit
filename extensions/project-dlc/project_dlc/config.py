"""Explicit IDE-owned run configuration; never silently rewrite native project files."""
from __future__ import annotations
import json
import os
from pathlib import Path
from .core import CapabilityError

NAME = '.ide-dlc.json'
KNOWN = {'system','run_task','config_task','main_class','build_dir','cmake_preset','build_config','run_artifact','run_args','run_target','cargo_bin','release','swift_product','zig_step','nix_target','scheme','destination','bazel_target','build_script','test_script','run_script','configure_script','browser_url','phone_session','phone_bundle','avd','android_serial','android_apk','android_package','xtool_command'}

def load(root: Path) -> dict:
    path=root/NAME
    if not path.exists(): return {}
    if path.stat().st_size > 262144: raise CapabilityError('IDE configuration too large')
    result=json.loads(path.read_text('utf-8'))
    if not isinstance(result,dict): raise CapabilityError('IDE configuration must be a JSON object')
    return result

def save(root: Path, values: dict) -> None:
    if set(values)-KNOWN: raise CapabilityError(f'Unknown config keys: {sorted(set(values)-KNOWN)}')
    root=root.resolve(strict=True)
    path=root/NAME
    if path.is_symlink(): raise CapabilityError('Refuse to overwrite symlink configuration')
    temp=root/(NAME+'.tmp.'+str(os.getpid()))
    try:
        fd=os.open(temp,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
        with os.fdopen(fd,'w',encoding='utf-8') as f:
            json.dump(values,f,indent=2,sort_keys=True)
            f.write('\n'); f.flush(); os.fsync(f.fileno())
        os.replace(temp,path)
    finally:
        try: temp.unlink(missing_ok=True)
        except OSError: pass
