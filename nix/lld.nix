# The linker for the QML module, in the dev shell and the package alike.
# cxx-qt-build links it with lld when `lld --help` runs, else with the
# deprecated gold; gcc's -fuse-ld=lld then runs `ld.lld`. Both names point at
# Nix's wrapped lld, so the module keeps its runpath to Qt (bare pkgs.lld
# drops it). Only these two names: `ld`, `ar` and the rest stay GNU.
#
# The package needs it as much as the shell: under Rosetta (an x86_64 build
# on Apple Silicon) running a missing `lld` exits 127 instead of failing to
# start, so cxx-qt-build takes lld for present and the link can't find it.
{ runCommand, llvmPackages }:

runCommand "kotodama-lld" { } ''
  mkdir -p $out/bin
  ln -s ${llvmPackages.bintools}/bin/ld.lld $out/bin/ld.lld
  ln -s ld.lld $out/bin/lld
''
