{
  description = "metaarch — a distributed system compiler built on quilt";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        # `bin/main` (on PATH via direnv) is the CLI entrypoint; the shell
        # only provides the toolchain.
        devShells.default = pkgs.mkShell {
          packages = [
            pkgs.rustup
            pkgs.rust-script
            pkgs.cargo-nextest
            pkgs.lolcat
            # bin/grammar: regenerate tree-sitter-arch (the CLI drives node).
            pkgs.tree-sitter
            pkgs.nodejs
          ];

          RUST_BACKTRACE = "1";
        };
      });
}
