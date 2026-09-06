# Releasing (maintainer notes)

This is maintainer-only reference for cutting a release and for the one-time crates.io
setup that lets the release workflow publish without a stored registry token. Nothing here
is required reading for a contribution — see [CONTRIBUTING.md](CONTRIBUTING.md) instead.

## Cutting a release

1. Update `CHANGELOG.md` (move `[Unreleased]` into a dated `[x.y.z]` section) and bump the
   `version` in `Cargo.toml` in the same commit.
2. Merge that to `main`, then push a tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. The `Release` workflow builds Linux/macOS/Windows binaries, signs each with cosign
   (keyless), attests build provenance for each, generates a CycloneDX SBOM, and attaches
   all of it — plus the pre-existing `.sha256` sidecars — to the GitHub Release created for
   that tag. It never publishes to crates.io on its own.
4. To publish the same version to crates.io, run the `Release` workflow manually
   (`workflow_dispatch`) against the `vX.Y.Z` tag with the `publish` input set to `true`.
   This requires the one-time Trusted Publishing setup below to already be in place.

## One-time crates.io Trusted Publishing setup

The `crates-io` job in `.github/workflows/release.yml` authenticates to crates.io via
[Trusted Publishing](https://crates.io) (OIDC) using `rust-lang/crates-io-auth-action`,
instead of a long-lived `CARGO_REGISTRY_TOKEN` secret. This has to be configured once, by a
crate owner, before the first Trusted Publishing run:

1. Sign in to crates.io with an account that owns (or will own) the `devscout-rs` crate.
2. From the crate's settings — or, before the crate's first publish, from the account-level
   "add a trusted publisher" flow — add a GitHub Actions trusted publisher with:
   - Repository owner: `H3xas`
   - Repository name: `devscout-rs`
   - Workflow filename: `release.yml`
   - Environment: leave blank (this workflow does not use a GitHub Environment)
3. Do not create or store a `CARGO_REGISTRY_TOKEN` repository secret. If one exists from
   before this setup, remove it — Trusted Publishing replaces it entirely, and an unused
   long-lived token left in place is exactly the kind of standing credential this setup is
   meant to avoid.
4. Verify by dispatching the `Release` workflow against an existing tag with
   `publish: true` and confirming the `cargo publish` step succeeds.

If the trusted publisher is ever misconfigured or removed, the `Authenticate to crates.io`
step fails with a clear error from crates.io rather than silently skipping the publish.
