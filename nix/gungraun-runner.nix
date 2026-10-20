# The runner the gungraun benchmarks call: it runs them under Valgrind and
# counts their instructions. Its version has to be the one of the `gungraun`
# crate the benchmarks use (crates/kotodamad/Cargo.toml pins it), and nixpkgs
# has no package of it.
{
  lib,
  rustPlatform,
  fetchCrate,
}:

rustPlatform.buildRustPackage rec {
  pname = "gungraun-runner";
  version = "0.20.0";
  src = fetchCrate {
    inherit pname version;
    hash = "sha256-XZHnq0t5XI/lSAdtP0UYxcaaV/YVZOmT/E7YQWX7jOU=";
  };
  cargoHash = "sha256-nYOFsVAuLw13XObUO4mS4edlv5N/BSHRYPPS1N7y0Os=";
  # Its tests need Valgrind and the gungraun benchmarks of its own repository.
  doCheck = false;

  meta = {
    description = "The runner of gungraun, a Valgrind-based benchmark harness";
    homepage = "https://github.com/gungraun/gungraun";
    license = with lib.licenses; [
      asl20
      mit
    ];
    mainProgram = "gungraun-runner";
  };
}
