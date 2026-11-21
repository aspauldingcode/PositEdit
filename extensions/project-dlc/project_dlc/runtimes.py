"""Runtime action registry. Does not assert unavailable embedded GPU interfaces."""
from __future__ import annotations
import dataclasses
import os
import platform
import re
import shutil
import subprocess
from pathlib import Path
from urllib.parse import urlparse
from .core import CapabilityError, Plan

@dataclasses.dataclass
class RuntimeCapability:
    id: str
    available: bool
    reason: str
    transport: str
    native_gpu: str

def _emulator() -> str | None:
    found=shutil.which('emulator')
    if found: return found
    roots=[]
    for key in ('ANDROID_HOME','ANDROID_SDK_ROOT'):
        value=os.environ.get(key)
        if value: roots.append(Path(value))
    roots += [Path.home()/'Library'/'Android'/'sdk', Path.home()/'Android'/'Sdk']
    for root in roots:
        candidate=root/'emulator'/'emulator'
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return str(candidate)
    return None

def _simctl() -> str | None:
    found=shutil.which('simctl')
    candidates=[Path(found)] if found else []
    candidates.append(Path('/Applications/Xcode.app/Contents/Developer/usr/bin/simctl'))
    for path in candidates:
        if path.is_file() and os.access(path, os.X_OK):
            return str(path)
    xcrun=shutil.which('xcrun')
    if not xcrun: return None
    try:
        out=subprocess.check_output([xcrun,'--find','simctl'], text=True, stderr=subprocess.DEVNULL, timeout=5).strip()
    except (subprocess.SubprocessError, OSError):
        return None
    return out if out and Path(out).is_file() else None

def _iphone(simctl: str) -> str | None:
    try:
        out=subprocess.check_output([simctl,'list','devices','available'], text=True, stderr=subprocess.DEVNULL, timeout=20)
    except (subprocess.SubprocessError, OSError):
        return None
    sections: list[list[tuple[str,str]]] = []
    current: list[tuple[str,str]] = []
    for line in out.splitlines():
        if line.startswith('--'):
            if current:
                sections.append(current)
                current=[]
            continue
        if 'iPhone' not in line or 'Duo' in line: continue
        match=re.search(r'\(([0-9A-Fa-f-]{36})\)', line)
        if match: current.append((match.group(1), line))
    if current: sections.append(current)
    for section in reversed(sections):
        booted=[udid for udid, line in section if '(Booted)' in line]
        if booted: return booted[0]
        if section: return section[0][0]
    return None

def _browser_url(cfg: dict) -> str:
    raw=cfg.get('browser_url')
    if raw is None or raw=='': return 'http://localhost:3000'
    if not isinstance(raw,str):
        raise CapabilityError('browser_url must be a complete http:// or https:// URL')
    parsed=urlparse(raw)
    if parsed.scheme not in ('https','http') or not parsed.netloc:
        raise CapabilityError('browser_url must be a complete http:// or https:// URL')
    return raw

def _browser_argv(url: str) -> list[str]:
    browser=shutil.which('terminal-browser')
    if browser: return [browser,'open',url]
    if platform.system()=='Darwin': return ['open',url]
    opener=shutil.which('xdg-open')
    if not opener: raise CapabilityError('No browser opener is installed')
    return [opener,url]

def _phone_session(cfg: dict) -> str | None:
    session=cfg.get('phone_session')
    if session is None or session=='': return None
    if not isinstance(session,str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,64}',session):
        raise CapabilityError('Configure safe phone_session name')
    return session

def _phone_argv(session: str | None) -> list[str]:
    phone=shutil.which('terminal-phone')
    if session and phone:
        return [phone,'view',session,'--split','right']
    simctl=_simctl()
    if not simctl: raise CapabilityError('No iPhone simulator is installed')
    udid=_iphone(simctl)
    if not udid: raise CapabilityError('No iPhone simulator is installed')
    return [simctl,'boot',udid]

def _avd_name(cfg: dict) -> str:
    avd=cfg.get('avd')
    if isinstance(avd,str) and avd!='':
        if not re.fullmatch(r'[A-Za-z0-9_.-]{1,128}',avd):
            raise CapabilityError('Configure AVD name')
        return avd
    if avd is not None and not isinstance(avd,str):
        raise CapabilityError('Configure AVD name')
    home=Path.home()/'.android'/'avd'
    names=sorted(path.stem for path in home.glob('*.ini')) if home.is_dir() else []
    phones=[name for name in names if 'wear' not in name.lower() and 'tv' not in name.lower()]
    preferred=[name for name in phones if 'phone' in name.lower()]
    pool=preferred or phones or names
    if not pool: raise CapabilityError('No Android virtual device is installed')
    return pool[-1]

def capabilities() -> list[RuntimeCapability]:
    system=platform.system()
    cpu=platform.machine()
    is_mac_arm = system=='Darwin' and cpu in ('arm64','aarch64')
    emulator=_emulator()
    return [
        RuntimeCapability('terminal-browser',bool(shutil.which('terminal-browser')),'Executable present' if shutil.which('terminal-browser') else 'Install terminal-browser','external terminal pane','Chromium GPU when supported'),
        RuntimeCapability('terminal-phone',bool(shutil.which('terminal-phone')) and is_mac_arm,'Requires terminal-phone and Apple Silicon macOS with Xcode for documented simulator mode','external terminal pane','Simulator rendering / device capture'),
        RuntimeCapability('android-emulator',emulator is not None,'Android emulator found' if emulator else 'Install Android emulator and SDK','Emulator window now; embedded gRPC viewer is a future adapter','HVF on macOS, KVM on Linux if supported'),
        RuntimeCapability('android-adb',bool(shutil.which('adb')),'Install Android platform-tools','ADB','Physical device GPU'),
        RuntimeCapability('xtool',bool(shutil.which('xtool')),'Install and configure xtool with authorized toolchain','Native CLI','Physical Apple device GPU'),
    ]

def runtime_plan(kind: str, root: Path, cfg: dict) -> Plan:
    root = root.resolve(strict=True)
    if kind == 'browser':
        url=_browser_url(cfg)
        return Plan('browser','open',str(root),_browser_argv(url),f'Open {url}')
    if kind == 'phone-view':
        return Plan('ios','view',str(root),_phone_argv(_phone_session(cfg)),'Boot the iPhone simulator')
    if kind == 'phone-boot':
        session=_phone_session(cfg)
        phone=shutil.which('terminal-phone')
        if session and phone:
            return Plan('ios','boot',str(root),[phone,'up',session,'--no-view'], 'Upstream iOS sim boot requires macOS and Xcode')
        return Plan('ios','boot',str(root),_phone_argv(None),'Boot the iPhone simulator')
    if kind == 'phone-launch':
        session,bundle=cfg.get('phone_session'),cfg.get('phone_bundle')
        if not isinstance(session,str) or not re.fullmatch(r'[\w-]{1,64}',session):
            raise CapabilityError('Configure phone_session')
        if not isinstance(bundle,str) or not re.fullmatch(r'[A-Za-z0-9_.-]{2,255}',bundle):
            raise CapabilityError('Configure phone_bundle')
        return Plan('ios','launch',str(root),['terminal-phone','launch',session,bundle], 'Launch installed bundle on authorized simulator/device')
    if kind == 'android-boot':
        avd=_avd_name(cfg)
        emulator=_emulator()
        if not emulator: raise CapabilityError('Android emulator is not installed')
        return Plan('android','boot',str(root),[emulator,'@'+avd,'-gpu','auto'],
                    'Hardware acceleration requested; verify with emulator -accel-check. Embedded gRPC viewport not yet implemented')
    if kind == 'android-install':
        serial,apk=cfg.get('android_serial'),cfg.get('android_apk')
        if not isinstance(serial,str) or not re.fullmatch(r'[A-Za-z0-9._:-]+',serial):
            raise CapabilityError('Configure android_serial from adb devices')
        if not isinstance(apk,str) or not (root/apk).resolve().is_relative_to(root) or not (root/apk).is_file():
            raise CapabilityError('android_apk must name an existing workspace file')
        return Plan('android','install',str(root),['adb','-s',serial,'install','-r',str((root/apk).resolve())],
                    'ADB install to explicitly selected device; requires authorized device')
    if kind == 'android-launch':
        serial,bundle=cfg.get('android_serial'),cfg.get('android_package')
        if not isinstance(serial,str) or not re.fullmatch(r'[A-Za-z0-9._:-]+',serial):
            raise CapabilityError('Configure android_serial')
        if not isinstance(bundle,str) or not re.fullmatch(r'[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)+',bundle):
            raise CapabilityError('Configure android_package')
        return Plan('android','launch',str(root),['adb','-s',serial,'shell','monkey','-p',bundle,'1'],
                    'ADB launches app; ensure device is authorized')
    if kind == 'xtool-dev':
        # Upstream CLI xtool dev is documented. Requires explicit SDK and project setup.
        return Plan('apple-linux','develop',str(root),['xtool','dev'],
                    'Linux Apple cross-development delegated to xtool; SDK licensing and Apple credentials are external')
    raise CapabilityError(f'Unknown runtime action: {kind}')
