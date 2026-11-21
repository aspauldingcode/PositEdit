"""Runtime action registry. Does not assert unavailable embedded GPU interfaces."""
from __future__ import annotations
import dataclasses
import platform
import re
import shutil
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

def capabilities() -> list[RuntimeCapability]:
    system=platform.system()
    cpu=platform.machine()
    is_mac_arm = system=='Darwin' and cpu in ('arm64','aarch64')
    return [
        RuntimeCapability('terminal-browser',bool(shutil.which('terminal-browser')),'Executable present' if shutil.which('terminal-browser') else 'Install terminal-browser','external terminal pane','Chromium GPU when supported'),
        RuntimeCapability('terminal-phone',bool(shutil.which('terminal-phone')) and is_mac_arm,'Requires terminal-phone and Apple Silicon macOS with Xcode for documented simulator mode','external terminal pane','Simulator rendering / device capture'),
        RuntimeCapability('android-emulator',bool(shutil.which('emulator')),'Install Android emulator and SDK','Emulator window now; embedded gRPC viewer is a future adapter','HVF on macOS, KVM on Linux if supported'),
        RuntimeCapability('android-adb',bool(shutil.which('adb')),'Install Android platform-tools','ADB','Physical device GPU'),
        RuntimeCapability('xtool',bool(shutil.which('xtool')),'Install and configure xtool with authorized toolchain','Native CLI','Physical Apple device GPU'),
    ]

def runtime_plan(kind: str, root: Path, cfg: dict) -> Plan:
    root = root.resolve(strict=True)
    if kind == 'browser':
        url=cfg.get('browser_url')
        if not isinstance(url,str): raise CapabilityError('Configure browser_url')
        parsed=urlparse(url)
        if parsed.scheme not in ('https','http') or not parsed.netloc:
            raise CapabilityError('browser_url must be a complete http:// or https:// URL')
        argv=['terminal-browser','open',url]
        return Plan('browser','open',str(root),argv,'External GPU browser terminal surface, not yet embedded in IDE compositor')
    if kind == 'phone-view':
        session=cfg.get('phone_session')
        if not isinstance(session,str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,64}',session):
            raise CapabilityError('Configure safe phone_session name')
        return Plan('ios','view',str(root),['terminal-phone','view',session,'--split','right'],
                    'Uses upstream terminal-phone managed pane, not custom graphics compositor')
    if kind == 'phone-boot':
        session=cfg.get('phone_session')
        if not isinstance(session,str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,64}',session):
            raise CapabilityError('Configure safe phone_session name')
        return Plan('ios','boot',str(root),['terminal-phone','up',session,'--no-view'], 'Upstream iOS sim boot requires macOS and Xcode')
    if kind == 'phone-launch':
        session,bundle=cfg.get('phone_session'),cfg.get('phone_bundle')
        if not isinstance(session,str) or not re.fullmatch(r'[\w-]{1,64}',session):
            raise CapabilityError('Configure phone_session')
        if not isinstance(bundle,str) or not re.fullmatch(r'[A-Za-z0-9_.-]{2,255}',bundle):
            raise CapabilityError('Configure phone_bundle')
        return Plan('ios','launch',str(root),['terminal-phone','launch',session,bundle], 'Launch installed bundle on authorized simulator/device')
    if kind == 'android-boot':
        avd=cfg.get('avd')
        if not isinstance(avd,str) or not re.fullmatch(r'[A-Za-z0-9_.-]{1,128}',avd):
            raise CapabilityError('Configure AVD name')
        return Plan('android','boot',str(root),['emulator','@'+avd,'-gpu','auto'],
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
