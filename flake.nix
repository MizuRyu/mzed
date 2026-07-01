{
  description = "mzed - Zed-linked Markdown viewer";

  inputs = {
    # NOTE: bare github:NixOS/nixpkgs/nixpkgs-unstable hits the GitHub API and
    # gets rate-limited (HTTP 403) in this environment. Pin to the FlakeHub
    # nixpkgs-weekly mirror so lock resolution does not call the GitHub API.
    nixpkgs.url = "https://flakehub.com/f/DeterminateSystems/nixpkgs-weekly/0.1.tar.gz";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "aarch64-linux" "x86_64-linux" ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = eachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.rust-analyzer
              pkgs.cargo-nextest
              pkgs.dioxus-cli
              pkgs.nodejs-slim
              pkgs.pnpm_9
              pkgs.just
              pkgs.sqlite
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.libiconv
            ];

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

            shellHook = ''
              unset DEVELOPER_DIR
              echo "🦀 mzed dev shell"
              echo "  cargo: $(cargo --version)"
              echo "  dx:    $(dx --version 2>/dev/null || echo 'n/a')"
            '';
          };
        }
      );
    };
}
