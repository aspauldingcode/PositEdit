# Autonomous preflight blocker playbook

The harness may require one-time external prerequisites that software cannot safely self-grant: initial provider sign-in, Apple CLI Tools license/installation, Nix installation if not already present, macOS TCC Accessibility/Screen Recording, signing credentials and paid API access. The default workflow does NOT attempt them silently. It records precise blockers in `engineering/state/preflight.json`, avoids incompatible tasks, and continues independent tasks where possible. It does not download an unverified Bend installer and execute it as a shell script.

Mac: verify `xcode-select -p`, `xcrun --find clang`; if unavailable, see Apple URL in REFERENCES. Nix: verify `nix --version`, `nix develop`, flake locked inputs. Bend: this flake pins the upstream `bendlang/bend` input at `v2.0.40`. Use `nix develop` or `nix develop -c bend PROOF.bend --verdict`; without Nix, prepare a source-verified/pinned install proposal and mark proofs blocked. Agent: `agent -p`, `codex exec` or `claude -p` must already be signed in; do not attempt to retrieve credentials from keychain.

Do not fake successful provisioning. Complete the remaining graph tasks that do not depend on missing prerequisites.
