# Releasing

How packages are released, and how an older minor version gets a security fix. ADR 0031 has the reasons; this page has the commands.

## From main

Each package releases from main on its own (`.github/workflows/release-please.yml`). The workflow runs only when started by hand:

```sh
gh workflow run release-please.yml
```

The first run opens a release PR for each package with releasable commits. Merge the ones you want with a merge commit, then run the workflow again: that run tags them (`kotodama-clipboard-v1.0.4`), publishes them on GitHub, and sends `kotodama-protocol` and `kotodama-extension` to crates.io if they were among them.

## Which versions get fixes

Security fixes go to the latest minor of a package's current major and the latest minor of its previous major. Every other fix lands on main only. A release line exists only for a minor that has needed a patch.

## Cutting a release line

A line is `release/<package>-<major>.<minor>`, cut from that minor's latest tag. The core's two crates share one line, `release/kotodama-<major>.<minor>`. The example cuts `kotodama-clipboard` 1.0; the commands for the core follow.

```sh
git fetch origin --tags
line=kotodama-clipboard-1.0
tag=$(git tag --list 'kotodama-clipboard-v1.0.*' --sort=-v:refname | head -1)
git switch -c "release/$line" "$tag"
```

Trim the release-please config and manifest to the one package, and drop the `linked-versions` plugin, which names the core:

```sh
package=crates/kotodama-clipboard
jq --arg p "$package" '.packages |= {($p): .[$p]} | del(.plugins)' release-please-config.json >config.tmp
mv config.tmp release-please-config.json
jq --arg p "$package" '{($p): .[$p]}' .release-please-manifest.json >manifest.tmp
mv manifest.tmp .release-please-manifest.json
echo "$line" >.release-line
```

For the core, `line=kotodama-1.0`, the tag pattern is `kotodama-v1.0.*`, and both crates stay, with the plugin:

```sh
core='.key == "crates/kotodama" or .key == "crates/kotodamad"'
jq ".packages |= with_entries(select($core))" release-please-config.json >config.tmp
mv config.tmp release-please-config.json
jq "with_entries(select($core))" .release-please-manifest.json >manifest.tmp
mv manifest.tmp .release-please-manifest.json
echo "$line" >.release-line
```

Check the trim, commit it, and push the branch:

```sh
scripts/tests/release-please-crates.sh
git add release-please-config.json .release-please-manifest.json .release-line
git commit -m "chore(release): cut the $line release line"
git push origin "release/$line"
```

The scripts test now checks only the line's packages: that each is a workspace member and in both files, and that the config has no other.

## Patching a line

The fix lands on main first, through the usual pull request. Then, for each supported line, take it onto a branch of its own and open a pull request into the line:

```sh
git fetch origin
git switch -c hotfix/kotodama-clipboard-1-0-<what> "origin/release/$line"
git cherry-pick -x <commit on main>
git push -u origin HEAD
gh pr create --base "release/$line" --fill
```

A conflict is resolved on this branch, and the pull request says what changed from main's version. It lands like one into main: `ci` green, rebased onto the line (`git rebase "origin/release/$line"`, `git push --force-with-lease`) rather than merged with it, and merged with a merge commit:

```sh
gh pr merge <number> --merge
```

Then release the line. The workflow file comes from main; `target-branch` names the line:

```sh
gh workflow run release-please.yml -f target-branch="release/$line"
```

That run opens the line's release PR against its branch. Merge it with a merge commit, and run the same command again to tag the release (`kotodama-clipboard-v1.0.5`) and publish it. The trimmed config means the run can release nothing but the line's package.

## A fix to the SDK or the protocol

The Bundled extensions, the core and the Front end build `kotodama-protocol`, and most of them `kotodama-extension`, from their own tree by `path`. A fix to either has to reach every package that builds on it (`scripts/path-deps <crate>` lists what a crate builds on).

1. **main.** Land the fix and release the SDK crate it touched. Then, in one pull request, raise each dependent package's requirement to the new version, one commit per package so each gets a changelog entry:

   ```toml
   kotodama-extension = { path = "../kotodama-extension", version = "1.2.4", features = ["wayland"] }
   ```

   ```sh
   git commit -m "fix(clipboard): build on kotodama-extension 1.2.4, which fixes <the issue>"
   ```

   Run the workflow, merge the release PRs, run it again.
2. **The SDK's lines.** Patch each supported line of the crate the fix touched, as above (`release/kotodama-extension-1.0`). Its release run publishes it to crates.io; a fix to `kotodama-protocol` releases it before `kotodama-extension`.
3. **The dependent packages' lines.** For each supported line of each dependent package, take the SDK as its release published it, raise the requirement, and release:

   ```sh
   git switch -c hotfix/kotodama-clipboard-1-0-sdk-1-0-5 "origin/release/$line"
   git checkout kotodama-extension-v1.0.5 -- crates/kotodama-extension
   cargo update --workspace
   # raise the requirement in crates/kotodama-clipboard/Cargo.toml to "1.0.5"
   git commit -am "fix(clipboard): build on kotodama-extension 1.0.5, which fixes <the issue>"
   ```

   Take the SDK line with the same `major.minor` as the SDK in the line's tree; if that one no longer gets fixes, the newest supported line of the same major. The commit in the package's own path is what makes release-please release it. Open the pull request into the line and release it as above.

## Protocol majors

A new protocol major turns away the Extensions built for older ones, unless the Daemon keeps them. The Daemon accepts the previous protocol major for at least as long as that major's SDK line gets security fixes (ADR 0031, on top of ADR 0026).

## Repository setup

Give `release/**` the rules main has: a pull request with `ci` green for every change, landed with a merge commit. Pushing a new branch with its trimming commit is a creation, not an update, so the rules allow it.
