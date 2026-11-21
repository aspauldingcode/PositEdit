"""Project discovery, native metadata inspection, executable action plans.

No native build files are executed by discovery or passive inspection.
Commands are argv arrays, never shell strings. Native build tools remain authoritative.
"""
from __future__ import annotations
import dataclasses
import json
import os
import re
import shutil
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

MAX_INPUT = 8 * 1024 * 1024

class CapabilityError(RuntimeError):
    pass

@dataclasses.dataclass
class Project:
    system: str
    root: str
    evidence: list[str]
    confidence: str = 'marker'

@dataclasses.dataclass
class Plan:
    system: str
    action: str
    cwd: str
    argv: list[str]
    explanation: str
    trust: str = 'executes_project_code'
    requires: list[str] = dataclasses.field(default_factory=list)

    def asdict(self):
        return dataclasses.asdict(self)

DETECTORS = [
    ('xcode-json', ['*.xcodeproj/project.xcproj']),
    ('intellij', ['.idea/misc.xml', '.idea/modules.xml', '*.iml']),
    ('gradle', ['settings.gradle.kts', 'settings.gradle', 'build.gradle.kts', 'build.gradle']),
    ('maven', ['pom.xml']),
    ('cmake', ['CMakeLists.txt']),
    ('meson', ['meson.build']),
    ('nix', ['flake.nix', 'default.nix', 'shell.nix']),
    ('cargo', ['Cargo.toml']),
    ('swiftpm', ['Package.swift']),
    ('zig', ['build.zig']),
    ('javascript', ['package.json']),
    ('bazel', ['MODULE.bazel', 'WORKSPACE.bazel', 'WORKSPACE', 'BUILD.bazel', 'BUILD']),
    ('make', ['Makefile', 'makefile']),
    ('ninja', ['build.ninja']),
]

def safe_read(path: Path) -> str:
    if not path.is_file() or path.stat().st_size > MAX_INPUT:
        raise CapabilityError(f'File missing or over {MAX_INPUT} byte limit: {path}')
    return path.read_text('utf-8')

def safe_json(path: Path) -> Any:
    return json.loads(safe_read(path))

def toml_loads(text: str) -> dict[str, Any]:
    """Cargo metadata. Uses tomllib on Python 3.11+, and a table reader before that."""
    try:
        import tomllib
    except ModuleNotFoundError:
        return _toml_tables(text)
    return tomllib.loads(text)

def _toml_tables(text: str) -> dict[str, Any]:
    root: dict[str, Any] = {}
    current: Any = root
    for raw in text.splitlines():
        line = raw.split('#', 1)[0].strip()
        if not line:
            continue
        if line.startswith('[[') and line.endswith(']]'):
            key = line[2:-2].strip()
            bucket = root.setdefault(key, [])
            if not isinstance(bucket, list):
                bucket = []
                root[key] = bucket
            item: dict[str, Any] = {}
            bucket.append(item)
            current = item
            continue
        if line.startswith('[') and line.endswith(']'):
            key = line[1:-1].strip()
            table = root.setdefault(key, {})
            if not isinstance(table, dict):
                table = {}
                root[key] = table
            current = table
            continue
        if '=' not in line or not isinstance(current, dict):
            continue
        key, value = line.split('=', 1)
        current[key.strip()] = _toml_value(value.strip())
    return root

def _toml_value(value: str) -> Any:
    if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
        return value[1:-1]
    if value == 'true':
        return True
    if value == 'false':
        return False
    return value

def detect(root: Path) -> list[Project]:
    root = root.expanduser().resolve(strict=True)
    if not root.is_dir():
        raise CapabilityError('Project root must be a directory')
    found = []
    for system, markers in DETECTORS:
        paths = []
        for marker in markers:
            paths += [p for p in root.glob(marker) if p.is_file()]
        if paths:
            found.append(Project(system, str(root), sorted({str(p.relative_to(root)) for p in paths})))
    return found

def _xml(root: Path):
    raw = safe_read(root)
    if '<!DOCTYPE' in raw.upper() or '<!ENTITY' in raw.upper():
        raise CapabilityError('DTD/entity declarations are not supported in passive XML inspection')
    return ET.fromstring(raw)

def _children(node, name):
    return [x for x in node.iter() if x.tag.rsplit('}', 1)[-1] == name]

def inspect(project: Project) -> dict[str, Any]:
    root = Path(project.root)
    out: dict[str, Any] = {'system': project.system, 'root': project.root,
                           'evidence': project.evidence, 'targets': [], 'variants': [],
                           'metadata_source': 'passive', 'warnings': [], 'native_owned': True}
    try:
        s = project.system
        if s == 'maven':
            doc = _xml(root/'pom.xml')
            out['targets'] = [x.text for x in _children(doc, 'module') if x.text]
            out['variants'] = [x.text for x in _children(doc, 'id') if x.text]
            out['warnings'].append('Effective POM, plugins and inherited settings require Maven evaluation in a trusted workspace')
        elif s == 'cargo':
            data = toml_loads(safe_read(root/'Cargo.toml'))
            out['targets'] = [x.get('name','') for x in data.get('bin', []) if isinstance(x,dict)]
            out['targets'] += [data['package'].get('name','')] if isinstance(data.get('package'),dict) else []
            out['variants'] = ['debug','release']
        elif s == 'javascript':
            data = safe_json(root/'package.json')
            scripts = data.get('scripts', {})
            out['targets'] = list(scripts) if isinstance(scripts, dict) else []
            out['package_manager'] = 'pnpm' if (root/'pnpm-lock.yaml').exists() else 'bun' if (root/'bun.lock').exists() or (root/'bun.lockb').exists() else 'yarn' if (root/'yarn.lock').exists() else 'npm'
        elif s == 'cmake':
            preset = root/'CMakePresets.json'
            if preset.exists():
                cfg = safe_json(preset)
                out['variants'] = [x.get('name') for x in cfg.get('configurePresets', []) if isinstance(x,dict) and x.get('name')]
            reply = root/'build'/'.cmake'/'api'/'v1'/'reply'
            indexes = sorted(reply.glob('index-*.json')) if reply.is_dir() else []
            if indexes:
                idx = safe_json(indexes[-1])
                obj = next((x for x in idx.get('objects',[]) if x.get('kind') == 'codemodel' and x.get('jsonFile')),None)
                if obj:
                    model = safe_json(reply/obj['jsonFile'])
                    out['targets'] = sorted({t['name'] for cfg in model.get('configurations',[]) for t in cfg.get('targets',[]) if 'name' in t})
                    out['metadata_source'] = 'cmake-file-api'
            else:
                out['warnings'].append('Configure with CMake File API query to discover authoritative targets')
        elif s == 'meson':
            targets = root/'build'/'meson-info'/'intro-targets.json'
            if targets.exists():
                arr = safe_json(targets)
                out['targets'] = [x.get('name') for x in arr if isinstance(x,dict) and x.get('name')]
                out['metadata_source'] = 'meson-introspection'
            else:
                out['warnings'].append('Run meson setup to generate introspection; do not statically execute meson.build')
        elif s == 'xcode-json':
            for filename in project.evidence:
                if filename.endswith('project.xcproj'):
                    data = safe_json(root/filename)
                    targets = data.get('targets', []) if isinstance(data,dict) else []
                    if isinstance(targets, list):
                        out['targets'] = [v['name'] for v in targets if isinstance(v,dict) and 'name' in v]
                    elif isinstance(targets,dict):
                        out['targets'] = list(targets)
                    break
            out['warnings'].append('Xcode scheme discovery and correct round-trip editing require Apple xcode-project-format / xcodebuild')
        elif s == 'gradle':
            for file in ['settings.gradle.kts','settings.gradle']:
                if (root/file).exists():
                    raw = safe_read(root/file)
                    # Only a hint; executable DSL cannot be statically resolved.
                    out['targets'] += re.findall(r'''include\s*\(\s*['"](:[^'\"]+)['"]''',raw)
            out['warnings'].append('Static Gradle detection is non-authoritative; full model requires Gradle Tooling API and trusted evaluation')
        elif s == 'intellij':
            out['warnings'].append('IntelliJ .idea metadata is not itself a build system; Gradle, Maven, JPS and plugin facets require separate models')
        elif s == 'nix':
            out['warnings'].append('Outputs are system-sensitive; evaluate explicitly with trusted nix flake show/eval')
        elif s == 'swiftpm':
            out['warnings'].append('Package.swift is executable Swift; query SwiftPM in a trusted environment for actual targets')
        elif s == 'bazel':
            out['warnings'].append('Use bazel query/cquery/aquery under explicit trust for authoritative targets')
        elif s == 'zig':
            out['warnings'].append('Zig build steps are defined by executable build.zig; explicit step discovery required')
    except (ValueError, OSError, ET.ParseError, KeyError, TypeError, UnicodeDecodeError) as exc:
        out['warnings'].append(f'Incomplete native metadata: {exc}')
    out['targets'] = sorted({x for x in out['targets'] if isinstance(x,str) and x})
    out['variants'] = sorted({x for x in out['variants'] if isinstance(x,str) and x})
    return out

def _need(cfg: dict, key: str, purpose: str) -> str:
    val = cfg.get(key)
    if not isinstance(val,str) or not val.strip():
        raise CapabilityError(f'{purpose} requires configured {key}; refusing to guess')
    return val

def _tool(root: Path, wrapper: str, fallback: str):
    candidate = root/wrapper
    # Only accept project wrappers by explicit opt-in later at execution time.
    return str(candidate) if candidate.is_file() else fallback

def plan(project: Project, action: str, cfg: dict[str,Any] | None = None) -> Plan:
    cfg = cfg or {}
    root = Path(project.root)
    s = project.system
    args: list[str] | None = None
    message = 'Native build system is authoritative; command is a proposed operation.'
    if s == 'intellij':
        raise CapabilityError('IntelliJ metadata alone does not define a portable build command; choose Gradle, Maven or explicit JPS bridge')
    if s == 'gradle':
        exe = _tool(root, 'gradlew', 'gradle')
        task = {'build':'build','test':'test','run':cfg.get('run_task'),'configure':cfg.get('config_task')}.get(action)
        if not task or not re.fullmatch(r'[\w:.\-]+',task):
            raise CapabilityError(f'Gradle {action} requires an explicit valid task' if action!='build' and action!='test' else 'Invalid Gradle task')
        args = [exe,task]
    elif s == 'maven':
        exe = _tool(root,'mvnw','mvn')
        if action in ('build','test'):
            args = [exe, 'package' if action=='build' else 'test']
        elif action=='run':
            _need(cfg,'main_class','Maven run')
            args = [exe, 'exec:java', f'-Dexec.mainClass={cfg["main_class"]}']
        else:
            raise CapabilityError('Maven configuration editing requires native POM/profile writer; use Configure to save launch settings')
    elif s == 'cmake':
        builddir = cfg.get('build_dir','build')
        if not isinstance(builddir,str) or not builddir or Path(builddir).is_absolute() or '..' in Path(builddir).parts:
            raise CapabilityError('Build directory must be a relative path inside the workspace')
        if action=='configure':
            args=['cmake','-S',str(root),'-B',str(root/builddir)]
            if cfg.get('cmake_preset'):
                args=['cmake','--preset',_need(cfg,'cmake_preset','CMake configure')]
        elif action in ('build','test'):
            args=['cmake','--build',str(root/builddir)] if action=='build' else ['ctest','--test-dir',str(root/builddir),'--output-on-failure']
            if cfg.get('build_config') and action=='build': args += ['--config', cfg['build_config']]
        elif action=='run':
            artifact=Path(_need(cfg,'run_artifact','CMake run'))
            if not artifact.is_absolute(): artifact=root/artifact
            if not artifact.resolve().is_relative_to(root):
                raise CapabilityError('Refusing to run artifact outside selected workspace')
            args=[str(artifact)]+list(cfg.get('run_args',[]))
    elif s == 'meson':
        builddir=cfg.get('build_dir','build')
        if not isinstance(builddir,str) or not builddir or Path(builddir).is_absolute() or '..' in Path(builddir).parts:
            raise CapabilityError('Invalid Meson build_dir')
        if action=='configure':
            args=['meson','setup',builddir] if not (root/builddir/'build.ninja').exists() else ['meson','configure',builddir]
        elif action=='build': args=['meson','compile','-C',builddir]
        elif action=='test': args=['meson','test','-C',builddir,'--print-errorlogs']
        elif action=='run':
            artifact=Path(_need(cfg,'run_artifact','Meson run'))
            if not artifact.is_absolute(): artifact=root/artifact
            if not artifact.resolve().is_relative_to(root):
                raise CapabilityError('Refusing to run artifact outside selected workspace')
            args=[str(artifact)]+list(cfg.get('run_args',[]))
    elif s == 'cargo':
        if action in ('build','test','run'):
            args=['cargo',action]
            if cfg.get('cargo_bin') and action=='run': args+=['--bin',cfg['cargo_bin']]
            if cfg.get('release') is True: args+=['--release']
        else: raise CapabilityError('Cargo has no general configure command; configure run settings separately')
    elif s == 'swiftpm':
        if action in ('build','test','run'):
            args=['swift',action]
            if action=='run': args += [_need(cfg,'swift_product','SwiftPM run')]
        else: raise CapabilityError('SwiftPM manifest must be edited with a native-aware writer')
    elif s == 'zig':
        if action in ('build','test'): args=['zig','build'] + (['test'] if action=='test' else [])
        elif action=='run': args=['zig','build',_need(cfg,'zig_step','Zig run')]
        else: raise CapabilityError('Zig configure requires editing build.zig or explicit -D options')
    elif s == 'javascript':
        meta=inspect(project)
        pm=meta.get('package_manager','npm')
        script=cfg.get(f'{action}_script', {'build':'build','test':'test','run':'dev'}.get(action))
        if not script or script not in meta['targets']:
            raise CapabilityError(f'{pm} script "{script}" not declared in package.json')
        args=[pm,'run',script]
    elif s == 'nix':
        if action == 'build': args=['nix','build',_need(cfg,'nix_target','Nix build'), '--print-build-logs']
        elif action == 'test': args=['nix','flake','check']
        elif action == 'run': args=['nix','run',_need(cfg,'nix_target','Nix run')]
        else: raise CapabilityError('Nix configuration requires editing flake inputs/outputs with a native-aware writer')
    elif s == 'xcode-json':
        scheme=_need(cfg,'scheme','Xcode build/run')
        name=next((v for v in project.evidence if v.endswith('project.xcproj')),None)
        if not name: raise CapabilityError('JSON xcproj metadata not found')
        project_bundle=str(root/Path(name).parent)
        if action in ('build','test'):
            args=['xcodebuild','-project',project_bundle,'-scheme',scheme]
            if cfg.get('build_config'): args+=['-configuration',cfg['build_config']]
            if cfg.get('destination'): args+=['-destination',cfg['destination']]
            args+=[action]
        elif action=='run':
            raise CapabilityError('Xcode Run requires build output, a verified bundle ID, and matching simulator/device destination; use runtime adapter after build')
        else:
            raise CapabilityError('Xcode JSON editing requires Apple XcodeProjectFormat native writer; do not change unknown schema fields')
    elif s=='bazel':
        target=_need(cfg,'bazel_target','Bazel action')
        args=['bazel',action,target] if action in ('build','test','run') else None
    elif s in ('make','ninja'):
        if action in ('build','test'):
            args=['make',action] if s=='make' else ['ninja',action] if action=='test' else ['ninja']
        else: raise CapabilityError(f'{s} run/configure requires explicit target contract')
    if not args:
        raise CapabilityError(f'No safe authoritative {action} plan for {s} project')
    return Plan(s,action,str(root),list(map(str,args)),message,requires=[args[0]])
