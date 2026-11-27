#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 1 || -z "$1" ]]; then
  echo "Usage: bash init-artifact.sh <new-project-directory>" >&2
  exit 1
fi
command -v pnpm >/dev/null || { echo "pnpm is required on PATH" >&2; exit 1; }
if ! node -e 'const [major, minor] = process.versions.node.split(".").map(Number); process.exit(Number(!(major === 20 && minor >= 19 || major === 22 && minor >= 12 || major > 22)))'; then
  echo "Use Node.js 20.19+, 22.12+, or a later supported major." >&2
  exit 1
fi

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
mkdir -- "$1"  # a new directory prevents replacing an existing project
cd -- "$1"
cp -R "$script_dir/../assets/template/." .
tar -xzf "$script_dir/shadcn-components.tar.gz" -C src
pnpm install
echo "Project ready at $PWD"
