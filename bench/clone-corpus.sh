#!/bin/sh
# Clone one corpus from bench/corpus.lock at its pinned SHA, with no remote.
#
# Usage: bench/clone-corpus.sh <corpus-id> <destination>
#   e.g. bench/clone-corpus.sh csharp bench/clones/csharp
#
# Shallow-fetches exactly the pinned commit and removes origin, so nothing after
# the pin is reachable and no run can fetch an upstream answer.

set -eu

id="${1:-}"
dest="${2:-}"
lock="$(dirname "$0")/corpus.lock"

if [ -z "$id" ] || [ -z "$dest" ]; then
	echo "usage: $0 <corpus-id> <destination>" >&2
	exit 2
fi

field() {
	sed -n "/^\[$id\]/,/^\[/p" "$lock" |
		sed -n "s/^$1 = \"\(.*\)\"$/\1/p" |
		head -n 1
}

url="$(field url)"
sha="$(field sha)"
status="$(field status)"

if [ -z "$url" ]; then
	echo "no corpus '$id' in $lock" >&2
	exit 1
fi

if [ -z "$sha" ]; then
	echo "corpus '$id' is $status and carries no pin; refusing to clone an unpinned corpus" >&2
	exit 1
fi

if [ -e "$dest" ]; then
	echo "$dest already exists; remove it first (one clone per arm, never reused)" >&2
	exit 1
fi

mkdir -p "$dest"
git -C "$dest" init -q
git -C "$dest" remote add origin "$url"
git -C "$dest" fetch -q --depth 1 origin "$sha"
git -C "$dest" checkout -q FETCH_HEAD
git -C "$dest" remote remove origin

echo "$id cloned at $sha into $dest (origin removed)"
