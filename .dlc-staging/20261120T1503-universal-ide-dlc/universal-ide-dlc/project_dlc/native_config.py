"""Narrow, schema-aware native CMake USER preset mutation.

This is an additive implementation, not a generic Gradle/Maven/Xcode writer.
Always preview, require owner opt-in, preserve unknown JSON keys semantically,
verify expected version and avoid silently replacing existing presets.
"""
from __future__ import annotations
import json
import os
import re
from pathlib import Path
from .core import CapabilityError, safe_json

FILENAME='CMakeUserPresets.json'

def prepare_cmake_user_preset(root:Path, name:str, build_type:str='Debug', generator:str='Ninja') -> dict:
    if not (root/'CMakeLists.txt').is_file(): raise CapabilityError('No CMakeLists.txt present')
    if not isinstance(name,str) or not re.fullmatch(r'[A-Za-z0-9_.-]{1,64}',name):
        raise CapabilityError('Invalid CMake preset name')
    if build_type not in ('Debug','Release','RelWithDebInfo','MinSizeRel'):
        raise CapabilityError('Unsupported build type')
    if generator not in ('Ninja','Unix Makefiles'):
        raise CapabilityError('Unsupported generator for this narrow adapter')
    path=root/FILENAME
    if path.is_symlink(): raise CapabilityError('Reject symlinked native configuration')
    before=safe_json(path) if path.exists() else {'version':6,'configurePresets':[]}
    if not isinstance(before,dict) or not isinstance(before.get('configurePresets',[]),list):
        raise CapabilityError('Invalid existing native CMake User Presets')
    if type(before.get('version')) is not int or before['version']<3:
        raise CapabilityError('CMake presets version must be an integer >= 3')
    if any(p.get('name')==name for p in before['configurePresets'] if isinstance(p,dict)):
        raise CapabilityError('Native preset with the same name exists; refuse destructive overwrite')
    preset={'name':name,'displayName':'IDE DLC '+name,'generator':generator,
            'binaryDir':'${sourceDir}/build/'+name,
            'cacheVariables':{'CMAKE_BUILD_TYPE':build_type}}
    after=json.loads(json.dumps(before))
    after.setdefault('configurePresets',[]).append(preset)
    return {'path':str(path),'before':before,'after':after,'operation':'append-user-configure-preset',
            'native_owner':'CMakeUserPresets.json','requires_native_validation':True,
            'format_note':'JSON properties preserved semantically; whitespace formatting may change'}

def apply_cmake_user_preset(root:Path, preview:dict, *, approved=False) -> dict:
    if not approved: raise CapabilityError('Native project modification requires explicit --apply')
    path=(root/FILENAME)
    if path.is_symlink() or str(path)!=preview.get('path'):
        raise CapabilityError('Native preset destination changed')
    current=safe_json(path) if path.exists() else {'version':6,'configurePresets':[]}
    if current!=preview['before']:
        raise CapabilityError('Native CMake preset file changed since preview; aborting transaction')
    tmp=root/(FILENAME+'.tmp.'+str(os.getpid()))
    try:
        fd=os.open(tmp,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
        with os.fdopen(fd,'w',encoding='utf-8') as f:
            json.dump(preview['after'],f,sort_keys=True,indent=2)
            f.write('\n'); f.flush(); os.fsync(f.fileno())
        os.replace(tmp,path)
        saved=safe_json(path)
        if saved!=preview['after']:
            raise CapabilityError('Native config read-after-write mismatch')
        return {'success':True,'path':str(path),'preset':preview['after']['configurePresets'][-1]['name']}
    finally:
        tmp.unlink(missing_ok=True)
