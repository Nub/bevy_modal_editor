{
  description = "bevy_modal_editor v2 — modal level editor + game framework for Bevy";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {inherit system overlays;};
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rust-analyzer" "clippy" "rustfmt"];
          targets = ["wasm32-unknown-unknown"];
        };
      in {
        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs;
            [
              rustToolchain
              pkg-config
              clang
            ]
            ++ lib.optionals stdenv.isLinux [
              lld
              mold
            ];

          buildInputs = with pkgs;
            [
              libffi
              openssl
            ]
            ++ lib.optionals stdenv.isLinux [
              alsa-lib
              udev
              vulkan-loader
              vulkan-headers
              vulkan-validation-layers
              libGL
              xorg.libX11
              xorg.libXcursor
              xorg.libXi
              xorg.libXrandr
              xorg.libxcb
              libxkbcommon
              wayland
              fontconfig
              freetype
            ];

          # Linux links against the nix-provided libs at runtime; macOS uses Metal via
          # system frameworks and needs no library path.
          RUST_BACKTRACE = 1;

          shellHook = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH"
          '';
        };
      }
    );
}
