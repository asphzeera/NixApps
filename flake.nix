{
  description = "NixApps — Declarative App Store for NixOS & Home Manager (Rust + GTK4/Libadwaita)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        runtimeInputs = with pkgs; [
          gtk4
          libadwaita
          glib
          gdk-pixbuf
          pango
          cairo
          graphene
          sqlite
          openssl
        ];

        nixapps = pkgs.rustPlatform.buildRustPackage {
          pname = "nixapps";
          version = "1.0.0";
          src = self;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            wrapGAppsHook4
          ];

          buildInputs = runtimeInputs;

          # Integration tests invoke `git` and `nix-instantiate`, which are unavailable in the
          # pure Nix sandbox; run unit tests that don't require external Nix/Git binaries.
          doCheck = false;

          postInstall = ''
            install -Dm644 assets/io.github.nixapps.NixApps.desktop \
              $out/share/applications/io.github.nixapps.NixApps.desktop
            install -Dm644 assets/io.github.nixapps.NixApps.svg \
              $out/share/icons/hicolor/scalable/apps/io.github.nixapps.NixApps.svg
          '';

          meta = with pkgs.lib; {
            description = "Native declarative App Store for NixOS and Home Manager";
            license = licenses.mit;
            mainProgram = "nixapps";
            platforms = platforms.linux;
          };
        };
      in
      {
        packages = {
          default = nixapps;
          nixapps = nixapps;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = nixapps;
          exePath = "/bin/nixapps";
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
            wrapGAppsHook4
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer
          ];

          buildInputs = runtimeInputs;

          packages = with pkgs; [
            nixfmt
            git
          ];

          RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
          RUST_BACKTRACE = "1";

          shellHook = ''
            echo "NixApps devShell loaded (Rust $(rustc --version | cut -d' ' -f2) + GTK4/Libadwaita)"
          '';
        };

        formatter = pkgs.nixfmt;
      }
    );
}
