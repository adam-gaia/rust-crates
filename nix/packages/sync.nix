
{ inputs, system, perSystem, ... }:
  inputs.nonstdlib.outputs.lib.writeNonstdlibShellApplication {
    inherit system;
    name = "sync";
    text = (builtins.readFile ../../scripts/sync);
    runtimeInputs = [perSystem.toml-path.default];
  }
