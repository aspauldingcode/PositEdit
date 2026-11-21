import importlib.util, pathlib, unittest
ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("axiom_toolchain", ROOT / "scripts" / "toolchain.py")
tc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tc)


def which_host_gap(name):
    return {"nix": "/nix/bin/nix", "cargo": "/nix-profile/bin/cargo", "python3": "/usr/bin/python3"}.get(name)


def which_complete(name):
    return "/usr/bin/" + name


def which_nix_only(name):
    return "/nix/bin/nix" if name == "nix" else None


class ToolchainTests(unittest.TestCase):
    def test_complete_host_is_unchanged(self):
        self.assertEqual(tc.wrap(["cargo", "test", "--workspace"], which_complete), ["cargo", "test", "--workspace"])

    def test_cargo_without_rustc_uses_devshell(self):
        argv = tc.wrap(["cargo", "test", "--workspace"], which_host_gap)
        self.assertEqual(argv[0], "nix")
        self.assertEqual(argv[-3:], ["cargo", "test", "--workspace"])
        self.assertIn("develop", argv)

    def test_missing_bend_uses_devshell(self):
        argv = tc.wrap(["bend", "PROOF.bend", "--verdict"], which_nix_only)
        self.assertEqual(argv[: len(tc.NIX_PREFIX)], tc.NIX_PREFIX)
        self.assertEqual(argv[len(tc.NIX_PREFIX) :], ["bend", "PROOF.bend", "--verdict"])

    def test_unknown_missing_tool_is_not_wrapped(self):
        self.assertEqual(tc.wrap(["not-a-real-tool", "--version"], which_nix_only), ["not-a-real-tool", "--version"])

    def test_python_stays_on_path(self):
        self.assertEqual(tc.wrap(["python3", "scripts/preflight.py"], which_host_gap), ["python3", "scripts/preflight.py"])


if __name__ == "__main__":
    unittest.main()
