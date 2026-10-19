#!/bin/sh
# Map one pristine corpus copy in an isolated environment and keep the graph.
set -eu
if [ "$#" -ne 5 ]; then
  echo "usage: bench/ts-resolution.sh <binary> <corpus-copy> <pin> <scratch> <output>" >&2
  exit 2
fi
binary="$1"
corpus="$2"
pin="$3"
scratch="$4"
output="$5"
case "$binary:$corpus:$scratch:$output" in
  /*:/*:/*:/*) ;;
  *) echo "absolute paths required" >&2; exit 2 ;;
esac
actual="$(git -C "$corpus" rev-parse HEAD)"
if [ "$actual" != "$pin" ] || [ -n "$(git -C "$corpus" status --porcelain --untracked-files=all)" ]; then
  echo "corpus copy is not pristine at its pin" >&2
  exit 1
fi
mkdir -p "$scratch/home" "$output"
env -i PATH=/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin LANG=en_US.UTF-8 \
  HOME="$scratch/home" SCOUT_REGISTRY="$scratch/registry.json" \
  SCOUT_CONTENT_DB="$scratch/content.db" \
  sh -c 'cd "$1"; "$2" init --no-hooks --no-map; "$2" map .' -- "$corpus" "$binary" \
  > "$output/stdout.txt" 2> "$output/stderr.txt"
graph="$corpus/.git/scout/graph/graph.json"
if [ ! -f "$graph" ]; then
  graph="$corpus/.scout/graph/graph.json"
fi
if [ ! -f "$graph" ]; then
  echo "graph.json not found after map" >&2
  exit 1
fi
cp "$graph" "$output/graph.json"
