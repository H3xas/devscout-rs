# Governance

## Today

`devscout` has a single maintainer today (see [MAINTAINERS.md](MAINTAINERS.md)). The
maintainer has final say on all decisions, but this document describes the ladder and
decision process the project intends to grow into as it gains contributors, and that
process is already in force for anyone who reaches "reviewer" or "maintainer" status below.

## Roles

### Contributor

Anyone who has opened a pull request, filed a useful issue, or otherwise participated. No
special access is required or granted.

### Reviewer

A contributor who has:

- had at least 3 non-trivial pull requests merged, and
- demonstrated familiarity with the codebase through substantive review comments on others'
  pull requests (not just approvals).

Reviewers are added by a maintainer, are listed in MAINTAINERS.md, and may be asked for
review on incoming pull requests and may approve them. Reviewer approval alone does not
merge a pull request unless a maintainer has also signed off, until a reviewer is promoted
to maintainer.

### Maintainer

A reviewer who has:

- sustained review and triage activity over multiple months, and
- demonstrated sound judgment on design and backward-compatibility questions, and
- the trust of the existing maintainer(s), granted explicitly (not by default tenure).

Maintainers can merge pull requests, cut releases, and manage repository settings. Adding a
new maintainer requires agreement from all existing maintainers.

## Decision Making

Routine decisions (bug fixes, most features, documentation, dependency updates) use
**lazy consensus**: a pull request may be merged by a maintainer once open review comments
are addressed and no maintainer has objected within a reasonable review window. Silence is
treated as consent for routine changes.

Decisions with broader or harder-to-reverse impact require explicit maintainer agreement
(not just the absence of objection) — this includes:

- Breaking changes to the CLI surface or artifact formats.
- Changes to this governance document or to `MAINTAINERS.md` roles.
- Any change to the project's license.
- Adding or removing a maintainer.

Disagreements that cannot be resolved through discussion are decided by maintainer vote,
with the existing maintainer(s) as tie-breakers.

## License Commitment

`devscout` is licensed under `MIT OR Apache-2.0`. **The project will not be relicensed away
from `MIT OR Apache-2.0`.** Any contribution accepted into this repository is accepted under
that dual license (see [CONTRIBUTING.md](CONTRIBUTING.md#license)), and no future governance
decision can change the license terms applied to past contributions without the affected
contributors' consent.

## Changing This Document

Changes to this document follow the "broader impact" decision rule above: they require
explicit agreement from all current maintainers, proposed as a pull request so the
reasoning is visible.
