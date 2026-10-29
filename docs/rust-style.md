# Rust style

Our Rust follows [Google's Rust style guide](https://google.github.io/styleguide/rustguide.html), which defers to the
official Rust style and API guidelines. This page is a short list of its rules, written in our own words, plus the places
where brisk differs. `rustfmt` and `clippy -D warnings` (see `scripts/check.sh`) settle formatting and the mechanical rules;
this page is for what they cannot see. Agents auditing code use it as the checklist.

## Organization

- Name a module after its main item. Keep non-leaf modules free of implementations: they declare and re-export.
- No glob imports, except `use super::*` in a test module and enum variants in a match that names many of them.
- Import types by name. Call free functions through their module (`fsutil::write_atomic`, not a bare `write_atomic`).
- Prefer small files with one job. A file that needs section banners to be read is two files.

## Ownership and borrowing

- Prefer unique ownership and borrows to `Rc` and `Arc`. Shared ownership needs a reason a reader can name.
- Do not `.clone()` to quiet the borrow checker. Restructure, or take a reference.
- Take the widest borrow a function needs: `&str` over `&String`, `&[T]` over `&Vec<T>`, `&Path` over `&PathBuf`.
- Return `impl Iterator` or take `impl IntoIterator` in library code instead of building a `Vec` the caller discards.
  Hot paths (RTP, FEC, ENet, the A/V pump) copy as little as they can.

## Function inputs and outputs

- No bare `bool` parameters whose meaning is unclear at the call site. Use a two-variant enum.
- Pick the failure type by who can act on it:
  - `Option` when absence is normal and needs no explanation.
  - `Result` when the caller can recover or must report why. Use a typed error, not `String`.
  - `panic!` only for a bug in our own code, never for bad input from a file, the network or a user.
- Prefer `expect("why this cannot fail")` to a bare `unwrap()` outside tests.
- Match or `let ... else` instead of `is_some()` followed by `unwrap()`.

## Types and traits

- A newtype for each value that must not be mixed up (a game id, a pid, a port, a slug).
- Named fields over tuples once a type has more than two members.
- Derive what you can (`Debug`, `Clone`, `PartialEq`, `Default`). Do not hand-write what a derive gives you.
- A trait is for generic code, `dyn` use or an extension trait (suffix `Ext`). A trait with one implementor and no test
  double is an indirection to delete.
- Public enums and structs that may grow get `#[non_exhaustive]`. Functions and types where ignoring the result is a bug
  get `#[must_use]`.

## Naming

- Getters are `foo()` and `foo_mut()`, setters `set_foo()`, builders `with_foo()`. Constructors are `new()`, or
  `try_new()` when they can fail. Conversions follow `as_`, `to_`, `into_`.
- No abbreviations a new reader would have to look up. Protocol names (`rtp`, `fec`, `enet`) are fine.

## Documentation and comments

- Every public item has a `///` comment that says what it is for, not how it works. Every crate and module root has `//!`.
- Document panics, error cases and `unsafe` invariants.
- Ordinary `//` comments say why, never what. A comment that restates the next line goes.
- `unsafe` needs a `// SAFETY:` comment directly above it (clippy enforces this) and should be rare. Product code is pure
  Rust, so there is no FFI in the bridge.
- A known shortcut is marked `shortcut: <the limit>, <when to upgrade>`.

## Macros

Reach for a plain function first, then `macro_rules!`. A procedural macro is the last resort and needs a dependency decision.

## Errors, logging, tests

- Library crates return typed errors with `Display` and `source()`. `hostd` and the binaries turn them into messages.
- Log through `tracing` (`tracing::warn!`, never `eprintln!`) in `brisk-hostd`, `brisk-bridge` and `brisk-client`. Only
  one-shot CLI output (usage text, `wrap`, `add`) uses `eprintln!`. `BRISK_LOG` sets the level.
- Tests assert behavior, not implementation. No fixed sleeps: poll, or use a paused tokio clock. No `set_var` in tests.
  Keep regression tests; delete tests that only repeat another test.

## Where brisk differs from the Google guide

The guide is written for Chromium and Fuchsia, and some of it does not apply here.

- **`async` is fine.** hostd and the bridge run on tokio. The guide's ban on `async`/`.await` and on spawning threads is for
  Chromium. Keep blocking work (file scans, the Steam HTTPS calls) off the runtime threads.
- **Mutexes** are `std::sync::Mutex`. Do not hold a guard across an `.await`. A poisoned lock means a bug: do not read past it.
- **Crates are not tiny.** The workspace has four crates by design (`brisk-proto` is sans-io and has no runtime).
- **Formatting** is `cargo fmt`, not `git cl format`.
- **Dependencies** follow `deny.toml` and the rules in `CLAUDE.md`: exact pins, pure Rust, one version of each crate.
