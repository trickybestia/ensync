{
  description = "Ensync, an encrypted file synchroniser";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      manifest = (fromTOML (builtins.readFile ./Cargo.toml)).package;
    in
    {
      packages = nixpkgs.lib.genAttrs systems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          ensync = pkgs.rustPlatform.buildRustPackage {
            pname = manifest.name;
            inherit (manifest) version;

            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./src
                ./COPYING
              ];
            };
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.sqlite ];

            meta = {
              description = "Encrypted file synchroniser";
              homepage = manifest.repository;
              license = pkgs.lib.licenses.gpl3Plus;
              mainProgram = "ensync";
            };
          };
        in
        {
          inherit ensync;
          default = ensync;
        }
      );
    };
}
