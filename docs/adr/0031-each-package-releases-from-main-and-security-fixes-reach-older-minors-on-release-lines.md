# 31. Each package releases from main, and security fixes reach older minors on release lines

Status: accepted (2026-10-22)

Kotodama is fourteen crates released as separate packages: the core (`kotodama` and `kotodamad`, released together), `kotodama-protocol`, `kotodama-extension`, the Front end and each Bundled extension. release-please releases each one from main when a Conventional Commit touches its path. That covers the newest version of everything and nothing else. Once main has moved to `kotodama-clipboard` 1.1, a user or a distro on 1.0 has no way to get a security fix short of taking 1.1 with it. This ADR decides how older versions get patched.

## What it does

- **main is the development channel.** Every package releases from it on its own, tagged `<package>-v<version>` (`kotodama-clipboard-v1.0.4`). Building from a commit of main is the way to run the newest code: the Nix flake, `cargo install --git`, and later the AUR's `-git` packages.
- **A release line is a branch for one minor version of one package**, `release/<package>-<major>.<minor>` (`release/kotodama-clipboard-1.0`). The core is one line for both its crates, `release/kotodama-<major>.<minor>`, since they share a version. A line is cut only when a patch for it is needed, from that minor's latest tag. Most minors never get one.
- **A line releases only its own package.** The branch's first commit trims `release-please-config.json` and `.release-please-manifest.json` to that package, or to the core's two crates with their `linked-versions` plugin, and adds a `.release-line` file naming the line. A cherry-pick that touches shared code, such as `kotodama-extension`, then can't release a package the line isn't for. Without the trim, release-please would tag that package on the branch with a version main may already have used. `scripts/tests/release-please-crates.sh` checks the trimmed lists on a branch with `.release-line`, and every crate everywhere else.
- **A fix lands on main first.** It is then brought to each supported line with `git cherry-pick -x`, in a pull request into the line's branch. The pull request follows main's rules: a linear branch, rebased onto the line, landed with a merge commit, `ci` green. CI runs on pushes to `release/**` as it does on main, and `linear` checks a pull request against the branch it targets.
- **A line is released by hand.** The Release workflow takes a `target-branch` input (main by default). release-please reads the config and the manifest from that branch, opens the release PR against it, and tags it once that PR is merged. The `crates-io` job publishes from the same branch, `kotodama-protocol` before `kotodama-extension`, and only the ones that run released. `docs/releasing.md` has the commands.

## Which versions get fixes

- **Security fixes** go to the latest minor of a package's current major and the latest minor of its previous major. With `kotodama-clipboard` at 2.3 on main, that is 2.3 (main) and the 1.x line with the highest minor.
- **Every other fix** lands on main only and ships in the next release.
- Each package counts its own majors and minors. The core at 2.0 says nothing about which `kotodama-clipboard` lines are supported.

Two lines per package is the most one maintainer can keep building and testing. It still covers the user who stays on a major because the next one broke something they rely on.

## Shared code: the SDK and the protocol

Every package builds `kotodama-protocol` from the same tree, through a `path` dependency, and so do the core and the Bundled extensions with `kotodama-extension` (`scripts/path-deps <crate>` lists what a crate builds on). A release of a Bundled extension contains the SDK as it was on the commit it was tagged from. A vulnerability in the SDK or the protocol therefore takes two steps.

1. **Patch each supported SDK line.** `kotodama-protocol` and `kotodama-extension` have release lines like any package. Their `major.minor` is the protocol version, so their lines are protocol versions too. Each line releases its patch and publishes it to crates.io from its own branch.
2. **Release a patch of every supported line of every package that builds on it.** The line takes the patched SDK as that SDK release published it, and raises its requirement on it, `kotodama-extension = { path = "../kotodama-extension", version = "1.0.5" }`. That change is the commit in the package's own path that release-please needs before it will release it, and its message is the changelog entry, which says the release rebuilds on the fixed SDK. A line whose SDK minor no longer gets fixes moves to the newest supported SDK line of the same major: a minor version of the protocol only adds (ADR 0026).

main does the same: the SDK fix releases the SDK, then one commit per dependent package raises its requirement and releases it.

## Mixed versions

A user may run a Daemon from one line and Extensions from others. That works because of protocol negotiation (ADR 0026): the Daemon accepts every protocol version from 1.0 up to its own and sends a peer nothing its version lacks. A new major version is the one change that would turn older Extensions away. ADR 0026 already keeps every older major, so the policy here is a floor under it. The Daemon accepts the previous protocol major for at least as long as that major's SDK line gets security fixes. Dropping a major sooner needs an ADR of its own.

## Considered options

- **One maintenance branch per Kotodama version for every package (`release/1.0`).** Rejected: the packages have versions of their own, and a release from that branch would tag every package it touched.
- **Cut a line at every minor release.** Rejected: most minors never get a patch, and every open line is a branch to keep green.
- **Leave the config whole on a line.** Rejected: a cherry-picked fix to shared code would release every package that path covers, from the line, with versions main may already have tagged.
- **Fix on the line, then forward-port to main.** Rejected: a fix that only exists on an old line can be lost from the next minor, and main is where the fix gets the most testing.
- **Publish the SDK lines' fixes without releasing the packages that link them.** Rejected: the Bundled extensions build the SDK by path, so they keep the vulnerable code until they are rebuilt and released.
