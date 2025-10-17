{
  description = "A very basic rust flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable"; # We want to use packages from the binary cache
    flake-utils.url = "github:numtide/flake-utils";
    # Rust overlay
    crane = {
      url = "github:ipetkov/crane";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
    self,
    nixpkgs,
    crane,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachSystem [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ] (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain (p:
        p.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rust-std" "clippy" "rustfmt" "rust-analyzer"];
        });
      # Define the source files root for rust project
      unfilteredRoot = ./.;

      # Define fileset source for rust project
      src = pkgs.lib.fileset.toSource {
        root = unfilteredRoot;
        fileset = pkgs.lib.fileset.unions [
          (craneLib.fileset.commonCargoSources unfilteredRoot)
          # If u are using sqlx/diesel/etc, uncomment the following line and adapt it to where your migrations are
          # ./migrations
        ];
      };

      # Common arguments for both dependency build and application build
      commonArgs = {
        inherit src;
        strictDeps = true;

        nativeBuildInputs = [
          pkgs.pkg-config
        ];

        buildInputs =
          [
            pkgs.openssl
            pkgs.libclang
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.darwin.apple_sdk.frameworks.Security
          ];
      };

      # Build the dependencies only first to leverage caching
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      # Now build the actual application
      application = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;

          nativeBuildInputs =
            (commonArgs.nativeBuildInputs or [])
            ++ [
              # Add additional native build inputs here
            ];
        }
      );
    in {
      # For `nix build` & `nix run`:
      packages = {
        default = application;
      };

      devShell = craneLib.devShell {
        buildInputs = [];
        packages = with pkgs; [];
      };
    });
}
