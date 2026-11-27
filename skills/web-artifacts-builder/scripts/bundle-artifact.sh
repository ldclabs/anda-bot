#!/usr/bin/env bash
set -euo pipefail

if [[ ! -f package.json || ! -f index.html ]]; then
  echo "Run this script from the generated project directory." >&2
  exit 1
fi

# Build separately so a failed rebuild leaves the last usable artifact in place.
build_dir=$(mktemp -d "./.artifact-build.XXXXXXXX")
trap 'rm -rf -- "$build_dir"' EXIT
pnpm exec vite build --outDir "$build_dir"
if [[ ! -s "$build_dir/index.html" ]]; then
  echo "Build did not produce an HTML file." >&2
  exit 1
fi
extra_files=$(find "$build_dir" -type f ! -path "$build_dir/index.html" -print)
if [[ -n "$extra_files" ]]; then
  echo "The build contains separate assets. Import or inline them before bundling:" >&2
  echo "$extra_files" >&2
  exit 1
fi
mv -- "$build_dir/index.html" bundle.html
echo "Wrote $PWD/bundle.html"
