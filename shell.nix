with import ./common.nix;

pkgs.stdenv.mkDerivation {
  name = "caldav-anon";
  buildInputs = (
    (with pkgs; [
      mdbook
      openssl
      pkg-config
      rust-analyzer
    ]) ++
    (with rustNightlyChannel; [
      cargo
      rust
    ])
  );
}
