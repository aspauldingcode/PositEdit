"""Runtime action registry. Plans in-editor panes only; no host window openers."""
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

ROOT = Path(__file__).resolve().parent

@dataclasses.dataclass
class RuntimeCapability:
    id: str
    available: bool
    reason: str
    transport: str
    native_gpu: str

def _emulator() -> str | None:
    try:
        from .android_sdk import find_emulator
    except ImportError:  # script-style import
        from android_sdk import find_emulator  # type: ignore
    return find_emulator()


def _adb() -> str | None:
    try:
        from .android_sdk import find_adb
    except ImportError:
        from android_sdk import find_adb  # type: ignore
    return find_adb()

def _repo_vendor_bin(name: str) -> Path | None:
    """Prefer forked binaries under <repo>/vendor/bin/."""
    here=Path(__file__).resolve()
    for parent in here.parents:
        candidate=parent/'vendor'/'bin'/name
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
        # Stop at repo root markers.
        if (parent/'extensions'/'project-dlc').is_dir() and (parent/'Cargo.toml').is_file():
            break
    return None

def _find_tool(name: str, env_key: str) -> str | None:
    override=os.environ.get(env_key)
    if override:
        path=Path(override).expanduser()
        if path.is_file() and os.access(path, os.X_OK):
            return str(path)
    vendor=_repo_vendor_bin(name)
    if vendor is not None:
        return str(vendor)
    found=shutil.which(name)
    if found: return found
    local=Path.home()/'.local'/'bin'/name
    if local.is_file() and os.access(local, os.X_OK):
        return str(local)
    return None

def _browser_url(cfg: dict) -> str:
    raw=cfg.get('browser_url')
    # Default to a public page so the in-editor pane works without a local dev server.
    if raw is None or raw=='': return 'https://example.com'
    if not isinstance(raw,str):
        raise CapabilityError('browser_url must be a complete http:// or https:// URL')
    parsed=urlparse(raw)
    if parsed.scheme not in ('https','http') or not parsed.netloc:
        raise CapabilityError('browser_url must be a complete http:// or https:// URL')
    return raw

def _browser_argv(url: str) -> list[str]:
    # Host Canvas paints Kitty graphics from PNG frames. Escape hatch: Ghostty split.
    if os.environ.get('IDE_BROWSER_SPLIT') == '1':
        browser=_find_tool('terminal-browser','IDE_TERMINAL_BROWSER') or 'terminal-browser'
        return [browser,'open',url,'--split','down']
    script=ROOT/'canvas_capture.py'
    python=shutil.which('python3') or sys_executable()
    return [python,str(script),'browser',url]

def _phone_session(cfg: dict) -> str:
    session=cfg.get('phone_session')
    if session is None or session=='': return 'posit'
    if not isinstance(session,str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,64}',session):
        raise CapabilityError('Configure safe phone_session name')
    return session

def _phone_argv(session: str) -> list[str]:
    phone=_find_tool('terminal-phone','IDE_TERMINAL_PHONE') or 'terminal-phone'
    # Host Canvas paints Kitty graphics from phone screenshots — never blocks/halfblocks.
    if os.environ.get('IDE_PHONE_SPLIT') == '1':
        return [phone,'up',session,'--split','down']
    script=ROOT/'canvas_capture.py'
    python=shutil.which('python3') or sys_executable()
    return [python,str(script),'phone',session]

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
    if not pool:
        return 'Android_Emulator'
    return pool[-1]

def _android_pane_argv(avd: str) -> list[str]:
    emulator=_emulator()
    if not emulator: raise CapabilityError('Android emulator is not installed')
    if not _adb():
        raise CapabilityError(
            'Install Android platform-tools (adb) under ANDROID_HOME/platform-tools'
        )
    script=ROOT/'android_pane.py'
    python=shutil.which('python3') or sys_executable()
    return [python,str(script),avd]

def sys_executable() -> str:
    import sys
    return sys.executable

def capabilities() -> list[RuntimeCapability]:
    system=platform.system()
    cpu=platform.machine()
    is_mac_arm = system=='Darwin' and cpu in ('arm64','aarch64')
    emulator=_emulator()
    browser=_find_tool('terminal-browser','IDE_TERMINAL_BROWSER')
    phone=_find_tool('terminal-phone','IDE_TERMINAL_PHONE')
    return [
        RuntimeCapability('terminal-browser',browser is not None,'Executable present' if browser else 'Install terminal-browser','in-editor pane','Chromium GPU when supported'),
        RuntimeCapability('terminal-phone',phone is not None and is_mac_arm,'Requires terminal-phone and Apple Silicon macOS with Xcode','in-editor pane','Simulator rendering / device capture'),
        RuntimeCapability('android-emulator',emulator is not None,'Android emulator found' if emulator else 'Install Android emulator and SDK','headless AVD + in-editor pane','HVF on macOS, KVM on Linux if supported'),
        RuntimeCapability('android-adb',_adb() is not None,'Install Android platform-tools under ANDROID_HOME','ADB','Physical device GPU'),
        RuntimeCapability('xtool',bool(shutil.which('xtool')),'Install and configure xtool with authorized toolchain','Native CLI','Physical Apple device GPU'),
    ]

def runtime_plan(kind: str, root: Path, cfg: dict) -> Plan:
    root = root.resolve(strict=True)
    if kind == 'browser':
        url=_browser_url(cfg)
        return Plan('browser','open',str(root),_browser_argv(url),f'Open {url} in the editor pane')
    if kind == 'phone-view':
        session=_phone_session(cfg)
        return Plan('ios','view',str(root),_phone_argv(session),'Open terminal-phone in the editor pane')
    if kind == 'phone-boot':
        session=_phone_session(cfg)
        phone=_find_tool('terminal-phone','IDE_TERMINAL_PHONE') or 'terminal-phone'
        return Plan('ios','boot',str(root),[phone,'up',session,'--no-view'], 'Upstream iOS sim boot requires macOS and Xcode')
    if kind == 'phone-launch':
        session,bundle=cfg.get('phone_session'),cfg.get('phone_bundle')
        if not isinstance(session,str) or not re.fullmatch(r'[\w-]{1,64}',session):
            raise CapabilityError('Configure phone_session')
        if not isinstance(bundle,str) or not re.fullmatch(r'[A-Za-z0-9_.-]{2,255}',bundle):
            raise CapabilityError('Configure phone_bundle')
        phone=_find_tool('terminal-phone','IDE_TERMINAL_PHONE') or 'terminal-phone'
        return Plan('ios','launch',str(root),[phone,'launch',session,bundle], 'Launch installed bundle on authorized simulator/device')
    if kind == 'android-boot':
        avd=_avd_name(cfg)
        # Prefer the in-editor pane helper. Fall back to a named emulator argv for
        # hosts that only need a plan preview when the helper cannot boot.
        try:
            argv=_android_pane_argv(avd)
        except CapabilityError:
            emulator=_emulator() or 'emulator'
            adb=_adb()
            argv=[emulator,'@'+avd,'-no-window','-gpu','auto']
            if adb:
                argv += ['-adb-path', adb]
        return Plan('android','boot',str(root),argv,
                    'One headless AVD with an in-editor screencap pane')
    if kind == 'android-install':
        serial,apk=cfg.get('android_serial'),cfg.get('android_apk')
        if not isinstance(serial,str) or not re.fullmatch(r'[A-Za-z0-9._:-]+',serial):
            raise CapabilityError('Configure android_serial from adb devices')
        if not isinstance(apk,str) or not (root/apk).resolve().is_relative_to(root) or not (root/apk).is_file():
            raise CapabilityError('android_apk must name an existing workspace file')
        adb=_adb() or 'adb'
        return Plan('android','install',str(root),[adb,'-s',serial,'install','-r',str((root/apk).resolve())],
                    'ADB install to explicitly selected device; requires authorized device')
    if kind == 'android-launch':
        serial,bundle=cfg.get('android_serial'),cfg.get('android_package')
        if not isinstance(serial,str) or not re.fullmatch(r'[A-Za-z0-9._:-]+',serial):
            raise CapabilityError('Configure android_serial')
        if not isinstance(bundle,str) or not re.fullmatch(r'[A-Za-z0-9_]+(\.[A-Za-z0-9_]+)+',bundle):
            raise CapabilityError('Configure android_package')
        adb=_adb() or 'adb'
        return Plan('android','launch',str(root),[adb,'-s',serial,'shell','monkey','-p',bundle,'1'],
                    'ADB launches app; ensure device is authorized')
    if kind == 'xtool-dev':
        return Plan('apple-linux','develop',str(root),['xtool','dev'],
                    'Linux Apple cross-development delegated to xtool; SDK licensing and Apple credentials are external')
    raise CapabilityError(f'Unknown runtime action: {kind}')
