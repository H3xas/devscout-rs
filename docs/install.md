# Install

The crate is named `devscout-rs`; the binary it installs is `devscout`.

## From crates.io

```sh
cargo install devscout-rs --locked
```

## Release binaries

Pre-built binaries for Linux, macOS, and Windows are attached to each tagged release on the
[releases page](https://github.com/H3xas/devscout-rs/releases). Releases are cut by pushing a `v*`
tag, which builds and attaches them. Each release carries these files:

| File | What it is |
| --- | --- |
| `devscout-linux-x86_64` | Linux binary (x86-64) |
| `devscout-macos-aarch64` | macOS binary (Apple silicon) |
| `devscout-windows-x86_64.exe` | Windows binary (x86-64) |
| `<asset>.sha256` | The checksum of each binary, one per binary |
| `<asset>.sigstore.json` | The keyless Sigstore signature bundle of each binary, one per binary |
| `devscout-sbom.cdx.json` | A CycloneDX SBOM covering the full dependency graph |

Each release binary is keylessly signed (Sigstore/cosign) and carries a GitHub build-provenance
attestation, alongside its `.sha256` checksum. Download the binary for your platform, verify it as
below, make it executable on Linux and macOS (`chmod +x`), and put it on your `PATH`.

## From source

Clone this repository and build it:

```sh
git clone https://github.com/H3xas/devscout-rs.git
cd devscout-rs
cargo build --release --locked
# binary at ./target/release/devscout
```

## Verify a download

Each step below uses the binary's own name in place of `<asset>`, for example
`devscout-linux-x86_64`.

1. **Checksum.** Compare the binary against its `.sha256` file:

   ```sh
   sha256sum -c <asset>.sha256        # Linux
   shasum -a 256 -c <asset>.sha256    # macOS
   ```

   On Windows, compare the output of `Get-FileHash <asset> -Algorithm SHA256` with the hash in
   `<asset>.sha256`.

2. **Signature.** Verify the keyless signature with [cosign](https://docs.sigstore.dev/):

   ```sh
   cosign verify-blob --bundle <asset>.sigstore.json \
     --certificate-identity-regexp 'https://github.com/H3xas/devscout-rs/.*' \
     --certificate-oidc-issuer https://token.actions.githubusercontent.com \
     <asset>
   ```

3. **Build provenance.** Verify that the binary was built by this repository's release workflow
   with the [GitHub CLI](https://cli.github.com/):

   ```sh
   gh attestation verify <asset> --repo H3xas/devscout-rs
   ```

The SBOM, `devscout-sbom.cdx.json`, lists every dependency the release was built from.

[RELEASING.md](../RELEASING.md) describes the maintainer side of a release.
