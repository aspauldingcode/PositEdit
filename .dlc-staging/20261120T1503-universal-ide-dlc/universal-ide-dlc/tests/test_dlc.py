import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from project_dlc.core import detect, inspect, plan, CapabilityError, Project
from project_dlc.config import load, save
from project_dlc.runtimes import runtime_plan, capabilities
from project_dlc.execution import execute
from project_dlc.tui import hit_test,button_rects

class Fixtures(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.root=Path(self.tmp.name)
    def tearDown(self): self.tmp.cleanup()
    def write(self,file,content):
        dst=self.root/file; dst.parent.mkdir(parents=True,exist_ok=True); dst.write_text(content)
    def p(self,kind): return next(p for p in detect(self.root) if p.system==kind)

    def test_detect_multiple(self):
        self.write('build.gradle.kts','plugins {}'); self.write('pom.xml','<project />')
        self.assertEqual({p.system for p in detect(self.root)}, {'gradle','maven'})
    def test_intellij_not_automatic_build(self):
        self.write('.idea/misc.xml','<project/>')
        with self.assertRaises(CapabilityError): plan(self.p('intellij'),'build')
    def test_gradle_plan_no_execution(self):
        self.write('build.gradle','println(\"unsafe\")')
        self.assertEqual(plan(self.p('gradle'),'build').argv,['gradle','build'])
    def test_gradle_wrapper_prioritized(self):
        self.write('gradlew','#!/bin/sh\n'); self.write('settings.gradle.kts','include(":app")')
        self.assertTrue(plan(self.p('gradle'),'build').argv[0].endswith('gradlew'))
    def test_gradle_run_must_select_task(self):
        self.write('build.gradle.kts','plugins {}')
        with self.assertRaises(CapabilityError): plan(self.p('gradle'),'run')
        self.assertEqual(plan(self.p('gradle'),'run',{'run_task':':app:run'}).argv[-1],':app:run')
    def test_maven_xml(self):
        self.write('pom.xml','<project><modules><module>a</module><module>b</module></modules></project>')
        self.assertEqual(inspect(self.p('maven'))['targets'],['a','b'])
    def test_maven_entity_rejected(self):
        self.write('pom.xml','<!DOCTYPE r [<!ENTITY x SYSTEM "file:///etc/passwd">]><r>&x;</r>')
        with self.assertRaises(CapabilityError): inspect(self.p('maven'))
    def test_maven_requires_main(self):
        self.write('pom.xml','<project/>')
        with self.assertRaises(CapabilityError): plan(self.p('maven'),'run')
    def test_cargo_metadata(self):
        self.write('Cargo.toml','[package]\nname="hello"\nversion="0.1.0"\n')
        self.assertIn('hello',inspect(self.p('cargo'))['targets'])
        self.assertEqual(plan(self.p('cargo'),'build').argv,['cargo','build'])
    def test_cmake_presets(self):
        self.write('CMakeLists.txt','cmake_minimum_required(VERSION 3.20)')
        self.write('CMakePresets.json',json.dumps({'configurePresets':[{'name':'debug'}]}))
        self.assertEqual(inspect(self.p('cmake'))['variants'],['debug'])
        self.assertIn('cmake',plan(self.p('cmake'),'configure').argv)
    def test_cmake_path_escape(self):
        self.write('CMakeLists.txt','project(hello)')
        with self.assertRaises(CapabilityError): plan(self.p('cmake'),'build',{'build_dir':'../../secret'})
        with self.assertRaises(CapabilityError): plan(self.p('cmake'),'run',{'run_artifact':'/bin/sh'})
    def test_meson_introspection(self):
        self.write('meson.build',"project('a','c')")
        self.write('build/meson-info/intro-targets.json',json.dumps([{'name':'hello'}]))
        self.assertEqual(inspect(self.p('meson'))['targets'],['hello'])
        self.assertEqual(plan(self.p('meson'),'build').argv,['meson','compile','-C','build'])
    def test_xcode_json_only(self):
        self.write('Demo.xcodeproj/project.xcproj',json.dumps({'targets':[{'name':'Demo'}]}))
        self.assertEqual(inspect(self.p('xcode-json'))['targets'],['Demo'])
        with self.assertRaises(CapabilityError): plan(self.p('xcode-json'),'build')
        self.assertEqual(plan(self.p('xcode-json'),'build',{'scheme':'Demo'}).argv[-1],'build')
    def test_xcode_legacy_not_supported(self):
        self.write('Demo.xcodeproj/project.pbxproj','{}')
        self.assertNotIn('xcode-json',[p.system for p in detect(self.root)])
    def test_node_script_gated(self):
        self.write('package.json',json.dumps({'name':'app','scripts':{'dev':'vite','test':'vitest'}}))
        with self.assertRaises(CapabilityError): plan(self.p('javascript'),'build')
        self.assertEqual(plan(self.p('javascript'),'run').argv,['npm','run','dev'])
    def test_nix_target_required(self):
        self.write('flake.nix','{}')
        with self.assertRaises(CapabilityError): plan(self.p('nix'),'build')
        self.assertEqual(plan(self.p('nix'),'test').argv,['nix','flake','check'])
    def test_swift_explicit_run(self):
        self.write('Package.swift','import PackageDescription')
        with self.assertRaises(CapabilityError): plan(self.p('swiftpm'),'run')
    def test_zig_run_step(self):
        self.write('build.zig','pub fn build() void {}')
        with self.assertRaises(CapabilityError): plan(self.p('zig'),'run')
    def test_bazel_explicit_target(self):
        self.write('MODULE.bazel','')
        with self.assertRaises(CapabilityError): plan(self.p('bazel'),'build')
    def test_save_config_roundtrip(self):
        save(self.root,{'scheme':'Demo','build_config':'Debug'})
        self.assertEqual(load(self.root)['scheme'],'Demo')
    def test_save_config_unknown_refused(self):
        with self.assertRaises(CapabilityError): save(self.root,{'evil':'yes'})
    def test_save_config_symlink_refused(self):
        (self.root/'.ide-dlc.json').symlink_to('/tmp/invalid-dlc-config')
        with self.assertRaises(CapabilityError): save(self.root,{'scheme':'Demo'})
    def test_exec_requires_approval(self):
        self.write('CMakeLists.txt','project(test)')
        with self.assertRaises(CapabilityError): execute(plan(self.p('cmake'),'build'), allow=False)
    def test_execute_supervision(self):
        from project_dlc.core import Plan
        p=Plan('fixture','build',str(self.root),[sys.executable,'-c','print("fixture-ok")'],'fixture')
        res=execute(p,allow=True)
        self.assertEqual(res['exit_code'],0)
        self.assertIn('fixture-ok',res['captured_output'])
    def test_phone_session_validation(self):
        with self.assertRaises(CapabilityError): runtime_plan('phone-view',self.root,{'phone_session':'../x'})
        self.assertEqual(runtime_plan('phone-view',self.root,{'phone_session':'work'}).argv[:2],['terminal-phone','view'])
    def test_browser_scheme_validation(self):
        with self.assertRaises(CapabilityError): runtime_plan('browser',self.root,{'browser_url':'file:///etc/passwd'})
        self.assertEqual(runtime_plan('browser',self.root,{'browser_url':'http://localhost:3000'}).argv[1],'open')
    def test_android_configuration(self):
        self.assertIn('@Pixel_Arm64',runtime_plan('android-boot',self.root,{'avd':'Pixel_Arm64'}).argv)
        with self.assertRaises(CapabilityError): runtime_plan('android-boot',self.root,{'avd':'bad; rm -rf /'})
    def test_android_apk_must_be_inside_workspace(self):
        with self.assertRaises(CapabilityError): runtime_plan('android-install',self.root,{'android_serial':'emulator-5554','android_apk':'../../x.apk'})
    def test_toolbar_has_real_hit_rectangles(self):
        rects=button_rects(140)
        self.assertEqual(hit_test(rects[0][0],2,140),'1')
        self.assertEqual(hit_test(5,3,140),None)
    def test_doctor_has_transport_and_gpu_declarations(self):
        values=capabilities()
        self.assertTrue(all(x.transport and x.native_gpu for x in values))
    def test_cli_json(self):
        self.write('pom.xml','<project/>')
        path=Path(__file__).parents[1]/'dlc.py'
        p=subprocess.run([sys.executable,str(path),'--root',str(self.root),'scan'],capture_output=True,text=True)
        self.assertEqual(p.returncode,0)
        self.assertEqual(json.loads(p.stdout)[0]['system'],'maven')
    def test_real_cmake_configure_build_run(self):
        if not shutil.which('cmake') or not shutil.which('cc'):
            self.skipTest('CMake and C compiler missing')
        self.write('CMakeLists.txt','cmake_minimum_required(VERSION 3.16)\nproject(dlc_fixture C)\nadd_executable(dlc_fixture main.c)\n')
        self.write('main.c','#include <stdio.h>\nint main(void){puts("cmake-dlc-pass");return 0;}\n')
        proj=self.p('cmake')
        result=execute(plan(proj,'configure'),allow=True,timeout=45)
        self.assertEqual(result['exit_code'],0,result['captured_output'])
        result=execute(plan(proj,'build'),allow=True,timeout=45)
        self.assertEqual(result['exit_code'],0,result['captured_output'])
        result=execute(plan(proj,'run',{'run_artifact':'build/dlc_fixture'}),allow=True,timeout=15)
        self.assertEqual(result['exit_code'],0,result['captured_output'])
        self.assertIn('cmake-dlc-pass',result['captured_output'])

if __name__=='__main__': unittest.main()

class NativeCMakeConfig(unittest.TestCase):
    def setUp(self):
        self.t=tempfile.TemporaryDirectory(); self.root=Path(self.t.name)
        (self.root/'CMakeLists.txt').write_text('cmake_minimum_required(VERSION 3.23)\nproject(test C)\nadd_executable(test main.c)\n')
        (self.root/'main.c').write_text('int main(void){return 0;}\n')
    def tearDown(self):self.t.cleanup()
    def test_native_preview_does_not_write(self):
        from project_dlc.native_config import prepare_cmake_user_preset
        p=prepare_cmake_user_preset(self.root,'dlc-debug')
        self.assertIn('configurePresets',p['after'])
        self.assertFalse((self.root/'CMakeUserPresets.json').exists())
    def test_native_explicit_write_and_semantic_preservation(self):
        from project_dlc.native_config import prepare_cmake_user_preset, apply_cmake_user_preset
        (self.root/'CMakeUserPresets.json').write_text(json.dumps({'version':6,'vendor':{'custom':'keep'},'configurePresets':[]}))
        p=prepare_cmake_user_preset(self.root,'dlc-debug')
        with self.assertRaises(CapabilityError): apply_cmake_user_preset(self.root,p,approved=False)
        r=apply_cmake_user_preset(self.root,p,approved=True)
        self.assertTrue(r['success'])
        data=json.loads((self.root/'CMakeUserPresets.json').read_text())
        self.assertEqual(data['vendor']['custom'],'keep')
        with self.assertRaises(CapabilityError):prepare_cmake_user_preset(self.root,'dlc-debug')
    def test_native_conflict_detection(self):
        from project_dlc.native_config import prepare_cmake_user_preset, apply_cmake_user_preset
        p=prepare_cmake_user_preset(self.root,'dlc-debug')
        (self.root/'CMakeUserPresets.json').write_text(json.dumps({'version':6,'configurePresets':[{'name':'another'}]}))
        with self.assertRaises(CapabilityError):apply_cmake_user_preset(self.root,p,approved=True)
    def test_native_preset_works_in_real_cmake(self):
        if not shutil.which('cmake') or not shutil.which('ninja'):self.skipTest('cmake+ninja needed')
        from project_dlc.native_config import prepare_cmake_user_preset, apply_cmake_user_preset
        apply_cmake_user_preset(self.root,prepare_cmake_user_preset(self.root,'dlc-debug'),approved=True)
        out=subprocess.run(['cmake','--list-presets'],cwd=self.root,capture_output=True,text=True)
        self.assertEqual(out.returncode,0,out.stderr)
        self.assertIn('dlc-debug',out.stdout)
        out=subprocess.run(['cmake','--preset','dlc-debug'],cwd=self.root,capture_output=True,text=True)
        self.assertEqual(out.returncode,0,out.stderr)

class OverlayInstaller(unittest.TestCase):
    def test_dryrun_and_apply_are_non_destructive(self):
        import importlib.util
        source=Path(__file__).parents[1]/'scripts'/'install_overlay.py'
        spec=importlib.util.spec_from_file_location('overlay',source)
        module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as tmp:
            host=Path(tmp)
            (host/'crates'/'ide-tui').mkdir(parents=True)
            (host/'Cargo.toml').write_text('[workspace]\n')
            manifest=host/'crates'/'ide-tui'/'Cargo.toml'
            manifest.write_text('[package]\nname="ide-tui"\nversion="0.1.0"\n[dependencies]\n')
            self.assertEqual(len(module.apply(host,execute=False)),3)
            self.assertFalse((host/'crates'/'ide-dlc-ui').exists())
            module.apply(host,execute=True)
            self.assertTrue((host/'crates'/'ide-dlc-ui'/'src'/'lib.rs').exists())
            self.assertTrue((host/'extensions'/'project-dlc'/'dlc.py').exists())
            self.assertIn('ide-dlc-ui',manifest.read_text())
            self.assertTrue((manifest.parent/'Cargo.toml.before-dlc').exists())
            with self.assertRaises(ValueError):module.apply(host,execute=True)

class ProcessTimeout(unittest.TestCase):
    def test_kills_long_running_process(self):
        from project_dlc.core import Plan
        with tempfile.TemporaryDirectory() as root:
            p=Plan('fixture','test',root,[sys.executable,'-c','import time;time.sleep(5)'],'fixture')
            result=execute(p,allow=True,timeout=0.1)
            self.assertTrue(result['timed_out'])
            self.assertNotEqual(result['exit_code'],0)

class MesonRun(unittest.TestCase):
    def test_requires_explicit_runnable_path(self):
        with tempfile.TemporaryDirectory() as root:
            root=Path(root).resolve()
            (root/'meson.build').write_text("project('x','c')")
            p=next(p for p in detect(root) if p.system=='meson')
            with self.assertRaises(CapabilityError):plan(p,'run')
            runnable=plan(p,'run',{'run_artifact':'build/x'})
            self.assertEqual(runnable.argv[0],str(root/'build'/'x'))

class ResponsiveToolbar(unittest.TestCase):
    def test_buttons_wrap_at_eighty_columns(self):
        buttons=button_rects(80)
        self.assertIn('8',[b[3] for b in buttons])
        self.assertGreater(max(b[2] for b in buttons),2)
        last=next(b for b in buttons if b[3]=='8')
        self.assertEqual(hit_test(last[0],last[2],80),'8')

class TerminalSmoke(unittest.TestCase):
    @unittest.skipUnless(os.name=='posix','PTY smoke needs POSIX')
    def test_full_screen_tui_starts_refreshes_and_exits(self):
        import fcntl, pty, struct, termios, time
        master,slave=pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
        env=os.environ.copy();env['TERM']='xterm-256color'
        binary=Path(__file__).parents[1]/'dlc.py'
        with tempfile.TemporaryDirectory() as root:
            p=subprocess.Popen([sys.executable,str(binary),'--root',root,'ui'],
                                stdin=slave,stdout=slave,stderr=slave,
                                env=env,start_new_session=True)
            os.close(slave)
            os.set_blocking(master, False)
            try:
                time.sleep(0.25);os.write(master,b'5')
                time.sleep(0.1);os.write(master,b'q')
                deadline=time.monotonic()+5
                while p.poll() is None and time.monotonic()<deadline:
                    try: os.read(master,4096)
                    except BlockingIOError: time.sleep(0.05)
                self.assertEqual(p.wait(timeout=1),0)
            finally:
                if p.poll() is None: p.kill();p.wait()
                os.close(master)
