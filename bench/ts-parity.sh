#!/bin/sh
# Run one arm of the TypeScript reference parity bench against its own copy of the corpus.
#
# Usage: bench/ts-parity.sh <devscout-base|devscout-head|crg> <arm binary> <corpus copy> \
#          <scratch root> <output dir>
#
# The runner starts from an empty environment, so no credential, package index or state
# directory of the calling shell reaches an arm. It refuses to run when the templates or the
# seeds differ from the pre-registered manifest, a template uses a verb outside the arm's
# allowlist, the peer binary is not the entry script of a virtual environment whose installed
# package passes the upstream identity check (checked again here, not only at install), the
# corpus is not at the pin, or the corpus or a state directory (HOME, CRG_DATA_DIR,
# SCOUT_REGISTRY, SCOUT_CONTENT_DB) is outside the scratch root, inside the real home
# directory or inside a checkout of this repository. Every path is resolved to its canonical
# absolute form before it is checked, because the arm runs with the corpus as its working
# directory.

set -eu

if [ "$#" -ne 5 ]; then
	echo "usage: $0 <devscout-base|devscout-head|crg> <arm binary> <corpus copy> <scratch root> <output dir>" >&2
	exit 2
fi

here="$(cd "$(dirname "$0")" && pwd)"

exec env -i PATH=/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin LANG=en_US.UTF-8 HOME="$4/runner-home" \
	python3 -B "$here/ts_parity.py" run "$1" "$2" "$3" "$4" "$5" "$here/truth/typescript"
