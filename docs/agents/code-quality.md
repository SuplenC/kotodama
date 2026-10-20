# Code quality

How Kotodama's Rust is written. Each rule names its source: Microsoft's Pragmatic Rust Guidelines (`M-…`), the Rust API Guidelines (`C-…`), the Style Guide, the std docs, or **kotodama** where the choice is this project's own. The rules are adapted from castello's guide; castello's framework-specific rules (async, plugins, preludes) are left out.

New code follows every rule here. Code written before this guide is being brought in line crate by crate (#108); until a crate's turn, edit it in the new style where you touch it, without restyling the rest of the file in the same PR. The mechanical rules are not yet enforced by `rustfmt.toml`, `clippy.toml` or `[workspace.lints]`: they arrive with that rewrite, because turning them on reformats every file. Until then, follow them by hand.

## Performance comes before style

Speed and memory come first (`CLAUDE.md`). On a hot path (anything between a keystroke and the pixels, the index's memory layout, and anything that runs per Item), performance wins over every rule in this guide, readability included. Write the fast version: a hand-rolled loop instead of an iterator chain, `unsafe`-free index tricks, a struct of arrays, a duplicated branch, a long function the optimiser handles better than three small ones. Three conditions keep it honest:

- **It is measured.** The PR shows the instruction count or the benchmark the faster version wins on; a guess at speed does not override a rule.
- **It says why, once.** A comment above the code names what it is optimising and the number, so the next reader does not "clean it up": `// HAZARD: one pass over the keys, no iterator adapters. This loop is 40% of first_keystroke's instructions.`
- **It stays on the hot path.** Everything around it (setup, config, errors, tests) still follows the guide.

## Formatting and layout

- **Imports: three blocks, one item per `use`.** `std`, then external crates, then `crate`/`super`/`self`, a blank line between; one item per line. Source: kotodama (from castello F1, F2).
- **`std::` paths everywhere.** No crate is `no_std`, so `core::`/`alloc::` paths buy nothing. Source: kotodama.
- **Line width 120.** Source: kotodama.
- **A module with children is `foo.rs` beside `foo/`**, never `foo/mod.rs`. Clippy `mod_module_files`. Source: kotodama.
- **`unsafe` only where a boundary needs it**: FFI (cxx-qt, `libc`), and nothing else. A crate with no such boundary opens with `#![forbid(unsafe_code)]`. Each `unsafe` block carries a `// SAFETY:` comment saying why every requirement of the call holds. Source: M-UNSAFE.

## Lints

- **Silence a lint with `#[expect(lint, reason = "…")]`**, at the narrowest item. `#[allow]` is for generated code and macro output only. An `expect` that stops firing warns, so suppressions do not outlive their cause. Source: M-LINT-OVERRIDE-EXPECT.
- **Clippy is clean under `-D warnings`**, locally (`just check`) and in CI. Source: M-STATIC-VERIFICATION.

## Errors

- **A fallible public operation returns a public enum, one variant per cause.** Callers match on the cause; a stringly error cannot be matched. Source: kotodama.
- **`Display` is this layer's message only; the cause is `source()`.** Write `#[error("reading the History")] Read(#[source] io::Error)`, never `#[error("reading the History: {0}")]` on a variant whose `source()` returns the same error. A reporter that walks the chain prints each cause once. Source: `std::error::Error` docs.
- **`thiserror` writes `Display` and `Error`.** Source: kotodama.
- **Convert at the call site with an explicit, named constructor** (`.map_err(Error::read)?`), not a blanket `From`: the call site is where the meaning of the failure is known. Source: kotodama.
- **`#[non_exhaustive]` case by case.** A public enum or public-field struct is `#[non_exhaustive]` when it will grow and no caller must react to a new case; it stays exhaustive when a new case must break the caller's `match`, so the compiler finds every site. Protocol types follow ADR 0012's version rule first. Source: C-STRUCT-PRIVATE.

## Panics

- **Return an error or restructure so the case cannot arise.** That is always the first choice. A Daemon that panics takes every open menu with it. Source: M-PANIC-IS-STOP.
- **A panic is acceptable at startup**, while the process is being assembled, where the developer sees it at once.
- **At runtime, a panic only for a broken process**: a poisoned mutex is the example. Write `.expect("history mutex poisoned")`, naming what broke. Avoid `unwrap()` outside tests. Source: M-PANIC-MESSAGE.

## Control flow

- **`let … else` for early exit.** Pull conditions out of an `if` with `let … else` first, so what remains in the `if` is short. Source: castello C1; clippy `manual_let_else`.
- **Let-chains for the conditions that stay together**: `if let Some(item) = item && item.hidden`. Where a chain still reads long, a small named helper function is fine. Do not extract one for every condition. Source: castello C2.
- **Iterator chains for pure transforms; a `for` loop once there is an early return, a side effect, or more than two steps.** On the keystroke path, write whichever the benchmark prefers. Source: castello C3.

## Threads and shared state

- **Shared mutable state is a `std::sync::Mutex` behind a struct with methods**, so no caller holds the lock longer than one method. A thread owning the state behind channels when the state has its own lifecycle (the indexer, a watcher). Source: kotodama.
- **Lock order lives on the field's `///`**, with a one-line pointer at each site that takes more than one lock. Source: castello.

## Signatures and types

- **Generic spelling by shape**: `impl Trait` for a simple, single-use bound; a named parameter when a caller may turbofish it or it repeats; a `where` clause once bounds no longer fit on the line. Source: castello G1.
- **String arguments: `impl AsRef<str>` when the function only reads**, so the caller controls allocation; `impl Into<String>` only when the function keeps the string and will allocate anyway. Source: castello G2.
- **An on/off tunable is a pair of setters, `x()` and `no_x()`**, never a `bool` argument. Source: castello G3.
- **A builder method returns `Self` and is `#[must_use]`**, and so is every constructor. Clippy `return_self_not_must_use`. Source: C-BUILDER.
- **Every public type implements `Debug`.** A type holding a secret (an API key, a clipboard entry's contents in logs) writes `Debug` by hand and prints `<redacted>`. Source: M-PUBLIC-DEBUG, C-DEBUG.
- **Clone an `Arc` as `Arc::clone(&x)`**, so the reader sees a pointer copy, not a deep clone. Clippy `clone_on_ref_ptr`. Source: castello L2.
- **A default value is a documented `const`**, named `DEFAULT_…`, whose doc says why that number. The `Default` impl names the const. Source: M-DOCUMENTED-MAGIC.

## Docs

- **Rustdoc: a summary sentence, then `# Errors` and `# Panics` on every public item that can.** `# Examples` on entry points an Extension author uses (`kotodama-extension`, `kotodama-protocol`). Source: M-CANONICAL-DOCS.
- **Rustdoc describes the end state.** It never cites ADRs, issue numbers or decision numbers; those live in the issue and the PR. Source: M-NO-META-DESIGN-DOCUMENTATION.
- **Domain terms are the glossary's** (`GLOSSARY.md`): a Result, an Item, an Extension, never a synonym.

## Comments

Comments and rustdoc are written for a human: the developer reading the code to understand it, and the reviewer of the change. Developers read code first, so the code explains itself. When something is long or strange, the first fix is to split it into well-named functions; a comment is the fallback for logic that stays non-obvious. Obvious code carries no comment at all. Source: castello's comment-style prototype.

### What gets a comment

- **Nothing that restates the code.** A comment repeating the next line is deleted, and a choice the method name already explains needs none.
- **Guard clauses and early returns speak for themselves**; they carry no comment.
- **A non-obvious reason**, in plain prose: the constraint from outside the file, or what the simpler version would break. Performance-driven code says why in a sentence, with the number if one was measured.
- **Invariants** the code relies on and the types do not show, as a plain sentence where they are relied on.
- **Hazards** (code a careful developer would plausibly "fix" and break) start with `HAZARD:` and say what breaks: `// HAZARD: swap the index before answering the queued query. Answering first serves Results from the old version.`
- **Concurrency rules live on the field**: lock order on the mutex field's `///`, with a one-line pointer at each site that takes more than one lock.
- **A comment describes the code as it is.** It never narrates a change: no "before this", "now uses", "previously", "no longer", "changed to", "added". What changed and why belongs in the commit message.
- **Facts, not hedges.** "Probably", "should work", "I think" become a fact that was checked, or a TODO.

### Pointing elsewhere

- **Other code by backticked path**: `` // `Index::publish` holds the write lock. ``
- **External sources named and linked**: `// ext-data-control replaces wlr-data-control (wayland-protocols, staging).` followed by the URL on its own line.
- **Workarounds for a dependency**: a `TODO(#n):` saying what it works around and when it can go, plus the upstream link.
- **Wording reads naturally**; common abbreviations (msg, ctx, fd) are fine in comments.

### Form

- **Full sentences, capitalised, ending with a period**, declarative and about the code: "The watcher re-arms on the new target", never "we re-arm".
- **Above the code, never trailing.** No comment after code on the same line, and no labels on closing braces.
- **`//` only**, never `/* */`. Wrap at 100 columns.
- **Markdown is allowed** inside comments: backticks, emphasis, short lists.
- **A comment needing paragraphs means the block wants to be a function**: extract it and put the prose in its `///`.
- **Numbered steps (`// 1. …`) only where a sequence is genuinely non-obvious** and splitting it into functions would not make it clearer. The first choice is still named functions.
- **Small text diagrams** (a state machine, a timeline, under about ten lines) are allowed where a picture beats prose.
- **Commented-out code is deleted**; git keeps it, and an intended future list lives in an issue, not in the code.
- **Section banners** use the dashed block, and only in long flat lists (`Cargo.toml`, a long table of consts); a long file elsewhere splits into modules:

  ```rust
  // ---------------------------------------------------------------------------
  // Bundled extensions.
  // ---------------------------------------------------------------------------
  ```

### Items and fields

- **Private fields that need explaining get `///`**, so the explanation shows on hover; so do **non-trivial private functions**. Trivial ones get nothing.
- **Type-level tricks** (`PhantomData`, marker or sealed traits) are explained once, on the item's doc.
- **A protocol table** (wire tags, key codes, status mappings) is named consts matched by name, with no comments.
- **A module's `//!`** gives its summary, then `# Owns` and `# Not here` (with where those things live instead).

### Next to tools

- **`#[expect]`** carries its whole explanation in `reason`.
- **`.expect("…")`** keeps a short message that tells at a glance what failed and where; when the reason it cannot fail is not obvious, a comment above explains it for the reader.
- **Copied code** opens with one line naming its origin and licence.
- **Config files** (`Cargo.toml`, the `justfile`, workflows, Nix files, QML) may explain at more length, for the developer setting things up. They follow the same rule on history: none.

### Tests

- **The test's name is a sentence saying what it checks**: `a_hidden_item_never_shows_in_the_search`, no comment.
- **Blank lines separate setup, action and assertion**; a comment only for setup whose purpose is not visible.
- **A regression test** has one line linking the bug: `// Regression: #102.`

### TODOs

- **`// TODO(#77): <the action>; <why it waits>.`** Every TODO has an open issue, so the work is visible and a developer can take it; whoever writes the TODO opens the issue first. The sentence still stands alone.
- **The PR that closes an issue removes its TODOs.**
- **`FIXME` and `XXX` are not used.**

## Tests

- **Unit tests in a sibling file**: `#[cfg(test)] mod tests;` at the bottom of `index.rs`, the tests in `index/tests.rs`. Behaviour across processes is tested in `crates/*/tests/` and `e2e/`. Source: castello T1.
- **A fallible test step is `.expect("the step")`**, naming what was being done: `.expect("start the Daemon")`. Source: castello T2.
- **A ranking test never changes to make a PR pass.** If a ranking test must change, the PR says which expectation changed and why.

## Logging

- **Log with the `log` macros**, one event per fact, with the values in the message: `log::warn!("extension {name} exited: {status}")`. Source: kotodama.
- **No `println!`, `eprintln!` or `dbg!`** outside a binary's user-facing output (the `kotodama` command). Source: M-LOG-NOT-PRINT.
- **Nothing on the keystroke path logs above `debug`**, and nothing formats a message that the level filter would drop. Source: kotodama.

## Benchmarks

- **Hot paths are measured by instruction counts** (`gungraun`, `crates/kotodamad/benches/instructions.rs`), which CI compares against the base commit. Wall-clock benches (`criterion`) are for local exploration and the CI timing gate. Source: castello; kotodama.
