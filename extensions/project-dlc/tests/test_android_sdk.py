"""ADB / SDK resolution for emulator launches."""
from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "project_dlc"
sys.path.insert(0, str(ROOT))

import android_sdk  # noqa: E402


class AndroidSdkTests(unittest.TestCase):
    def test_find_adb_prefers_sdk_platform_tools(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            sdk = Path(tmp) / "sdk"
            adb = sdk / "platform-tools" / "adb"
            adb.parent.mkdir(parents=True)
            adb.write_text("#!/bin/sh\nexit 0\n")
            adb.chmod(0o755)
            prev_home = os.environ.get("ANDROID_HOME")
            prev_root = os.environ.get("ANDROID_SDK_ROOT")
            prev_ide = os.environ.get("IDE_ADB")
            prev_android_adb = os.environ.get("ANDROID_ADB")
            try:
                os.environ["ANDROID_HOME"] = str(sdk)
                os.environ["ANDROID_SDK_ROOT"] = str(sdk)
                os.environ.pop("IDE_ADB", None)
                os.environ.pop("ANDROID_ADB", None)
                found = android_sdk.find_adb()
                self.assertEqual(Path(found).resolve(), adb.resolve())
                env = android_sdk.emulator_env(found)
                self.assertEqual(env.get("ANDROID_HOME"), str(sdk))
                self.assertEqual(env.get("ANDROID_ADB"), found)
                self.assertTrue(env["PATH"].startswith(str(sdk / "platform-tools")))
            finally:
                self._restore("ANDROID_HOME", prev_home)
                self._restore("ANDROID_SDK_ROOT", prev_root)
                self._restore("IDE_ADB", prev_ide)
                self._restore("ANDROID_ADB", prev_android_adb)

    def test_ide_adb_override_wins(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            custom = Path(tmp) / "custom-adb"
            custom.write_text("#!/bin/sh\nexit 0\n")
            custom.chmod(0o755)
            prev = os.environ.get("IDE_ADB")
            try:
                os.environ["IDE_ADB"] = str(custom)
                self.assertEqual(Path(android_sdk.find_adb()).resolve(), custom.resolve())
            finally:
                self._restore("IDE_ADB", prev)

    @staticmethod
    def _restore(key: str, value: str | None) -> None:
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value


if __name__ == "__main__":
    unittest.main()
