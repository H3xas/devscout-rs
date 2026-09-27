#!/bin/sh
# Install code-review-graph into a fresh virtual environment from PyPI alone, then refuse
# the result unless it is the upstream release.
#
# Usage: UV_CACHE_DIR=<empty dir> HOME=<scratch dir> \
#          bench/peers/code-review-graph/install.sh <new venv dir> [python]
#
# A shared or reused package cache can hold a different build published under the same
# name and version, so the cache must start empty, every requirement is hash-pinned, and
# the only index is PyPI. No extras are installed: the embedding providers and the
# visualizer are the parts of the package that reach the network.

set -eu

venv="${1:-}"
python="${2:-python3.11}"
here="$(cd "$(dirname "$0")" && pwd)"
bench="$(cd "$here/../.." && pwd)"
requirements="$here/requirements.txt"

if [ -z "$venv" ]; then
	echo "usage: $0 <new venv dir> [python]" >&2
	exit 2
fi
if [ -e "$venv" ]; then
	echo "$venv already exists; install into a fresh directory" >&2
	exit 1
fi

python3 -B "$bench/ts_parity.py" peer-preinstall "$requirements"

export UV_NO_CONFIG=1
export UV_PYTHON_DOWNLOADS=never

uv venv --python "$python" "$venv"
uv pip install --python "$venv/bin/python" --require-hashes --no-build \
	--index-url https://pypi.org/simple -r "$requirements"

python3 -B "$bench/ts_parity.py" peer-identity "$venv"
