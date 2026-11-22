{
  description = "Posit Edit: Nix-first terminal IDE and engineering harness";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    bend.url = "github:bendlang/bend/v2.0.40";
  };
  outputs = { self, nixpkgs, flake-utils, bend }:
    flake-utils.lib.eachSystem [
      "aarch64-darwin" "x86_64-darwin"
      "aarch64-linux" "x86_64-linux"
    ] (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;
        ide = pkgs.rustPlatform.buildRustPackage {
          pname = "posit-edit";
          version = "0.1.0";
          src = lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = [ "-p" "ide" ];
          cargoTestFlags = [ "-p" "ide" ];
          nativeBuildInputs = [ pkgs.pkg-config ];
          postInstall = ''
            ln -s posit-edit $out/bin/pst
          '';
          meta = { description = "Minimal terminal IDE starter (incomplete)"; platforms = lib.platforms.unix; };
        };
        bendTool = bend.packages.${system}.default;
        # Bend 2.0.39 --verdict builds its kernel with Lean 4.34.0 exactly.
        lean434 = if system == "aarch64-darwin" then pkgs.stdenv.mkDerivation {
          pname = "lean";
          version = "4.34.0";
          src = pkgs.fetchurl {
            url = "https://github.com/leanprover/lean4/releases/download/v4.34.0/lean-4.34.0-darwin_aarch64.tar.zst";
            sha256 = "0ci9jajrkq28wmys10y82hkywy94v7yazcdzdd3c5fr1dvx67wk9";
          };
          nativeBuildInputs = [ pkgs.zstd ];
          dontConfigure = true;
          dontBuild = true;
          dontFixup = true;
          installPhase = ''
            mkdir -p $out
            cp -a . $out/
          '';
        } else null;
        languageTools = [ pkgs.rust-analyzer pkgs.clang-tools ]
           ++ lib.optionals (builtins.hasAttr "zls" pkgs) [ pkgs.zls ]
           ++ lib.optionals (builtins.hasAttr "nixd" pkgs) [ pkgs.nixd ];
        full = pkgs.buildEnv {
          name = "posit-edit-with-language-tools";
          paths = [ ide bendTool pkgs.git pkgs.ripgrep pkgs.fd ] ++ languageTools;
        };
      in {
        packages = { default = ide; core = ide; full = full; bend = bendTool; };
        apps.default = { type = "app"; program = "${ide}/bin/posit-edit"; };
        devShells.default = pkgs.mkShell {
          packages = (with pkgs; [ rustc cargo rustfmt clippy git python3 pkg-config tmux ripgrep fd nodejs uv ])
            ++ [ bendTool ] ++ languageTools;
          RUST_BACKTRACE = "1";
        };
        checks = { 
          core = ide;
          bend-laws = pkgs.runCommand "posit-bend-laws" {
            nativeBuildInputs = [ bendTool ] ++ lib.optionals (lean434 != null) [ lean434 ];
          } ''
            export HOME=$TMPDIR
            export XDG_CACHE_HOME=$TMPDIR/cache
            mkdir -p "$out" "$XDG_CACHE_HOME"
            : > "$out/verdict.txt"
            verdict() {
              local f="$1"
              local name="$2"
              local tmp
              tmp="$(mktemp)"
              echo "== $name ==" | tee -a "$out/verdict.txt"
              bend "$f" --verdict | tee -a "$out/verdict.txt" | tee "$tmp"
              grep -q "ALL PROOFS CHECK" "$tmp"
            }
            # Root accepted laws + additive modules (doc/minimap/scroll/palette/dlc).
            verdict ${./.}/PROOF.bend root
            verdict ${./.}/bend/palette/PROOF.bend palette
            verdict ${./.}/bend/dlc/PROOF.bend dlc
            verdict ${./.}/bend/doc/PROOF.bend doc
            verdict ${./.}/bend/minimap/PROOF.bend minimap
            verdict ${./.}/bend/scroll/PROOF.bend scroll
          '';
        };
      });
}
