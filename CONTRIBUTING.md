# Contributing to devscout

Thanks for considering a contribution. This document covers how to build and test the
project, the quality gates a pull request needs to pass, how to sign off your commits, and
what the release gate actually checks.

For anything not covered here, open a discussion or an issue first — especially before a
large change, so the design can be agreed on before code is written.

## Code of conduct

Participation in this project is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).

## Building and testing

```sh
cargo build --all-targets
cargo test
```

`cargo test` runs the unit tests embedded in `src/` plus every integration test under
`tests/`, including the public conformance suite described below. There are no test-only
dependencies beyond a stable Rust toolchain; nothing in the test suite reaches the network.

Before opening a pull request, run:

```sh
cargo fmt --all -- --check
cargo clippy --lib --bins
cargo clippy --all-targets
sh tools/check-architecture.sh
sh tools/check-size-and-complexity.sh
```

[ARCHITECTURE.md](ARCHITECTURE.md) maps every module under `src/` to its responsibility and
invariants and says where a change goes; read it before editing code, and keep its table
current when you add, move, or remove a module. The check script fails when a module has no
row or a row names a module that no longer exists, and CI runs it on every push.

`cargo fmt --all -- --check` must be clean — CI enforces this on every pull request.

Two clippy lints are denied and CI gates on them: `too_many_lines` at 100 lines and
`cognitive_complexity` at 25, with the thresholds in `clippy.toml`. A function that trips
either is split; an existing one that cannot be is carried by a single `#[allow(...)]`
attribute whose `reason` says why, and `tools/size-ratchet.toml` caps how many of those
attributes the tree may hold. That same ratchet caps every file under `src/` at 800 lines,
except the large files it names individually at their current length. Those numbers only
shrink: a new file over the limit is split, never added to the list. CI runs
`cargo clippy --lib --bins --locked`, the ratchet check, and a self-test proving the check
rejects an oversized file and an extra exemption.

`cargo clippy` is not otherwise a zero-warnings baseline project-wide (a pre-existing set of
`too_long_first_doc_paragraph` / missing-backtick rustdoc lints predates this contributing
guide and CI does not gate on it — see [ROADMAP.md](ROADMAP.md)). What CI does expect,
and what review will ask for, is that a pull request does not make clippy's opinion of the
files it touches any worse than it found them, and that any new module you add is clean
under `cargo clippy --all-targets -- -D warnings` on its own. When in doubt, run the
deny-warnings form locally and treat every new warning your diff introduces as a defect to
fix before requesting review.

If you add or change a dependency, also run the license/advisory check CI runs:

```sh
cargo install cargo-deny --locked
cargo deny check
```

Its policy lives in `deny.toml` at the repo root.

## Developer Certificate of Origin

Every commit must carry a sign-off certifying you wrote it or otherwise have the right to
submit it under the project's license, per the [Developer Certificate of
Origin](https://developercertificate.org/). Add the sign-off with:

```sh
git commit -s
```

This appends a `Signed-off-by: Your Name <you@example.com>` trailer using the name and
email from your git config. Pull requests with unsigned commits will be asked to amend and
force-push before merge; CODEOWNERS review does not substitute for the DCO trailer.

## Pull requests

- Keep changes focused; unrelated cleanup belongs in its own PR.
- Add or update tests for behavior you add or change.
- Update `CHANGELOG.md` under `[Unreleased]` for anything user-visible.
- Fill in the pull request template, including the DCO checkbox.

## Reaching release-gate confidence locally

Releases are additionally checked, before a tag is pushed, against a private behavioral
parity corpus that pins this project's graph contract against a second, non-public
implementation of the same contract. That corpus is not published, so a contributor outside
the maintainer team cannot re-run it directly — but nothing you need to trust a change
depends on it being public.

The public equivalent lives in this repository and runs in CI on every pull request:

- **`tests/conformance.rs`**, backed by the invented fixtures under
  `fixtures/conformance/`, exercises the same command surface the private corpus pins —
  `map`, `find`, `refs`, `impact`, and `tests` — across both a C# and a TypeScript file in
  one pass. Run it on its own with `cargo test --test conformance`.
- The rest of the suite under `tests/` and the unit tests in `src/` cover the extraction and
  resolution behavior in depth (generic arity, nested-type binding, preprocessor handling,
  freshness, and more).

A green `cargo test` locally is the same signal CI produces, and CI's `build-and-test` job
is what a maintainer checks before cutting a release — the private corpus is an additional,
maintainer-side check on top of that, not a replacement for it. If you can make `cargo test`
pass, you have reached the same public confidence bar the release process starts from.

## License

By contributing, you agree that your contributions are licensed under the same terms as the
project: MIT OR Apache-2.0 (see [README.md](README.md#license)). The project will not be
relicensed away from those terms — see [GOVERNANCE.md](GOVERNANCE.md).
