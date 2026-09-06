# Security Policy

## Supported Versions

`devscout` is currently pre-1.0 (`0.x`). Security fixes are made against the latest `0.x`
minor release; older minor releases are not separately patched.

| Version | Supported |
| ------- | --------- |
| 0.3.x   | Yes       |
| < 0.3   | No        |

This table is updated as new minor versions ship.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for a suspected security vulnerability.

Instead, use GitHub's private reporting flow:

1. Go to the repository's **Security** tab.
2. Select **Report a vulnerability** to open a private security advisory.
3. Include, as far as you know them: affected version(s), the code path or command that
   triggers the issue, a proof of concept or reproduction steps, and the impact you expect
   (e.g., what an attacker gains — arbitrary artifact-directory writes, denial of service on
   `map`, a parser crash on crafted input, etc.).

If GitHub private reporting is ever unavailable to you, open a regular issue asking a
maintainer to contact you privately, without describing the vulnerability itself.

## What to Expect

- Acknowledgement of a new report within a reasonable time.
- An initial assessment of severity and affected versions.
- Coordinated disclosure: we ask for **90 days** from initial report before any public
  disclosure (blog post, advisory publication, or CVE request), to give a fix time to ship
  and users time to upgrade. This window can be shortened by mutual agreement (for example,
  if a fix ships quickly) or extended for unusually complex issues.
- Credit in the published advisory and the changelog, unless you prefer to remain anonymous.

## Scope

In scope: the `devscout` binary itself — the CLI, its parsers (C#, TypeScript, TSX,
JavaScript via tree-sitter grammars), its on-disk artifact and cache formats, and its build
and release pipeline (including the supply-chain controls described in `RELEASING.md` and
the release workflow).

Out of scope: vulnerabilities in a project that `devscout` is pointed at (i.e., in the
source code being indexed) — `devscout` is a read-only analysis tool over that code and does
not execute it.
