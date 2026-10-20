# Kotodama

A fast, extensible command menu for Wayland. Rust everywhere; the default front end is a Quickshell QML module. Kotodama knows nothing about any particular distro: OS-specific features arrive as Extensions. Architecture: `docs/adr/0001-architecture.md`.

Read `CODEMAP.md` first to find what is where. Update it in the same PR when you add, move or rename a module, crate, test suite or recipe.

## Agent skills

- **Issues**: GitHub Issues via `gh`. See `docs/agents/issue-tracker.md`.
- **Triage labels**: defaults (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`). See `docs/agents/triage-labels.md`.
- **Domain docs**: single-context (`GLOSSARY.md` + `docs/adr/` at root). See `docs/agents/domain.md`.

## Priorities

Speed and memory use come first. Every design choice is measured against keystroke-to-pixel latency and resident memory.

## Git

- **Commits**: [Conventional Commits](https://www.conventionalcommits.org/). `<type>(<scope>)?: <description>`, types `feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert`; `!` or a `BREAKING CHANGE:` footer for breaking changes.
- **Branches**: [Conventional Branch](https://conventional-branch.github.io/). `<type>/<description>`, types `feature|bugfix|hotfix|release|chore`; lowercase, hyphens only (dots allowed in `release/` versions), e.g. `feature/issue-12-clipboard-history`.
