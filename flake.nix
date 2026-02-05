{
  description = "Bevy Avian3D Level Editor";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rust-analyzer"];
          targets = ["wasm32-unknown-unknown"];
        };
      in {
        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
            clang
            lld
            mold
          ];

          buildInputs = with pkgs; [
            # Audio
            alsa-lib
            libjack2

            # udev
            udev

            # Vulkan
            vulkan-loader
            vulkan-headers
            vulkan-tools
            vulkan-validation-layers

            # OpenGL / Mesa
            libGL
            libGLU

            # X11
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
            xorg.libXrender
            xorg.libXext
            xorg.libXxf86vm
            xorg.libxcb

            # Wayland
            libxkbcommon
            wayland
            wayland-protocols

            # Font rendering
            fontconfig
            freetype

            # Other
            libffi
            openssl
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;

          # Vulkan ICD (use system drivers via LD_LIBRARY_PATH instead of hardcoded paths)

          RUST_BACKTRACE = 1;

          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH"
          '';
        };
      }
    );
}
