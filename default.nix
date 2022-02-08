{ release ? false }:

with import ./common.nix;

naersk.buildPackage {
    pname = "ics-anon";
    version = "dev";

    src = pkgs.lib.sourceFilesBySuffices ./. [".rs" ".toml" ".lock"];

    buildInputs = with pkgs; [
        openssl
        pkg-config
    ];

    inherit release;
}
