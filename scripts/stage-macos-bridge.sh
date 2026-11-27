#!/usr/bin/env bash
# Pre-0.15 clients read latest-mac.yml in GitHub Latest and require an LDC signature.
set -euo pipefail
output=$(cd "${1:?expected the release directory}" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

zip=Anda-mac-arm64.zip
bridge=Anda-bridge-0.15.0-mac-arm64.zip
base=https://github.com/ldclabs/anda-bot/releases/download/v0.15.0
for asset in "$zip" "$zip.blockmap"; do
  curl -fsSL --retry 3 --retry-all-errors "$base/$asset" -o "$asset"
done
shasum -a 256 --check <<'SUMS'
4f54966a2cb289e16b0e8b77567bc8716087e7aaa5150cb7b7b31851a274100a  Anda-mac-arm64.zip
69a8daf3eee548fc86702b374eac955a5fde264e297434215af125e4985cae13  Anda-mac-arm64.zip.blockmap
SUMS
sha512=$(openssl dgst -sha512 -binary "$zip" | openssl base64 -A)
size=$(wc -c < "$zip" | tr -d ' ')
cat > latest-mac.yml <<EOF
version: 0.15.0
files:
  - url: $bridge
    sha512: $sha512
    size: $size
path: $bridge
sha512: $sha512
releaseDate: '2026-11-20T07:33:27.825Z'
EOF
mv "$zip" "$output/$bridge"
mv "$zip.blockmap" "$output/$bridge.blockmap"
mv latest-mac.yml "$output/latest-mac.yml"
