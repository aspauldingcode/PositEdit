# Illustrative Home Manager integration; not enabled by default.
{ pkgs, lib, config, ... }:
let cfg = config.programs.positEdit;
in {
  options.programs.positEdit.enable = lib.mkEnableOption "Posit IDE";
  options.programs.positEdit.package = lib.mkOption {
    type = lib.types.package;
    default = pkgs.posit-edit or (throw "Supply a Posit Edit flake package");
    description = "Posit IDE package from this flake";
  };
  config = lib.mkIf cfg.enable { home.packages = [ cfg.package ]; };
}
