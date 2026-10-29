# Kotodama

A fast, extensible command menu for Wayland. Rust everywhere; the default front end is a Quickshell QML module. Kotodama knows nothing about any particular distro: OS-specific features arrive as Extensions. Architecture: `docs/adr/0001-architecture.md`.

Read the root `CODEMAP.md` first, then the `CODEMAP.md` of the crate you work in. In the same PR, update the map of each crate you change when you add, move or rename a module, test suite or bench, and the root map when you add a crate, top-level dir or recipe.

## Agent skills

- **Issues**: GitHub Issues via `gh`. See `docs/agents/issue-tracker.md`.
- **Triage labels**: defaults (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.
- **Domain docs**: single-context (`GLOSSARY.md` + `docs/adr/` at root). See `docs/agents/domain.md`.

## Building

Run cargo and just through `nix develop -c` (e.g. `nix develop -c just check`). The shell's toolchain and linker override a global `~/.cargo/config.toml`, so kotodama-qml's tests find the shell's Qt and a warm worktree's build matches main's.

## Writing code

Before writing or reviewing Rust in this repo, read `docs/agents/code-quality.md`: the coding and comment rules, each with its source. New code follows them; older code is brought in line by #108. Code comments carry the why, never an issue or ADR number; those belong in the issue and the PR.

## Priorities

Speed and memory use come first. Every design choice is measured against keystroke-to-pixel latency and resident memory.

## Git

- **Commits**: [Conventional Commits](https://www.conventionalcommits.org/). `<type>(<scope>)?: <description>`, types `feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert`; `!` or a `BREAKING CHANGE:` footer for breaking changes.
- **Branches**: [Conventional Branch](https://conventional-branch.github.io/). `<type>/<description>`, types `feature|bugfix|hotfix|release|chore`; lowercase, hyphens only (dots allowed in `release/` versions), e.g. `feature/issue-12-clipboard-history`.
- **Linear branches**: a branch never merges main into itself. Bring it up to date with `git rebase origin/main` and push with `git push --force-with-lease`; CI's `linear` job fails a pull request whose branch has a merge commit. The pull request itself lands with a merge commit, never squashed.
