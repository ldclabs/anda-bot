#!/bin/sh
# Generate and publish the Homebrew formula for anda.
#
# Required in CI:
#   HOMEBREW_TAP_TOKEN  GitHub token with write access to HOMEBREW_TAP_REPO
#
# Optional:
#   HOMEBREW_TAP_REPO   Defaults to ldclabs/homebrew-tap
#   HOMEBREW_TAP_BRANCH Defaults to main
#   HOMEBREW_TAP_DIR    Existing local tap checkout to update instead of cloning
#   DRY_RUN=1           Print the generated formula and skip git operations

set -eu

REPO="${REPO:-ldclabs/anda-bot}"
FORMULA_NAME="${FORMULA_NAME:-anda}"
FORMULA_CLASS="${FORMULA_CLASS:-Anda}"
TAP_REPO="${HOMEBREW_TAP_REPO:-ldclabs/homebrew-tap}"
TAP_BRANCH="${HOMEBREW_TAP_BRANCH:-main}"
FORMULA_PATH="${HOMEBREW_FORMULA_PATH:-Formula/${FORMULA_NAME}.rb}"
TAG="${1:-${GITHUB_REF_NAME:-}}"

info() { printf '%s\n' "$1"; }
error() { printf 'Error: %s\n' "$1" >&2; exit 1; }

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || error "Missing required command: $1"
}

normalize_tag() {
    RAW_TAG="$1"
    RAW_TAG="${RAW_TAG#refs/tags/}"

    if [ -z "$RAW_TAG" ]; then
        error "Usage: $0 <tag>. Example: $0 v0.4.0"
    fi

    case "$RAW_TAG" in
        v*) printf '%s\n' "$RAW_TAG" ;;
        *) printf 'v%s\n' "$RAW_TAG" ;;
    esac
}

fetch_checksum() {
    ASSET_NAME="$1"
    CHECKSUM_URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET_NAME}.sha256"
    HASH=$(curl -fsSL "$CHECKSUM_URL" | awk '{print $1}' | tr -d '\r\n')

    if [ -z "$HASH" ]; then
        error "Could not read checksum from ${CHECKSUM_URL}"
    fi

    printf '%s\n' "$HASH"
}

write_formula() {
    cat <<EOF
class ${FORMULA_CLASS} < Formula
  desc "Local AI agent with a long-term memory brain"
  homepage "https://github.com/${REPO}"
  version "${VERSION}"
  license "Apache-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "${BASE_URL}/anda-macos-arm64", using: :nounzip
      sha256 "${MACOS_ARM64_SHA}"
    else
      url "${BASE_URL}/anda-macos-x86_64", using: :nounzip
      sha256 "${MACOS_X86_64_SHA}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "${BASE_URL}/anda-linux-arm64", using: :nounzip
      sha256 "${LINUX_ARM64_SHA}"
    else
      url "${BASE_URL}/anda-linux-x86_64", using: :nounzip
      sha256 "${LINUX_X86_64_SHA}"
    end
  end

  def install
    binary = Dir["anda-*"].first
    chmod 0755, binary
    bin.install binary => "anda"
  end

  def caveats
    <<~EOS
      Homebrew does not write runtime files into ~/.anda during install.
      To install or refresh curated skills, run:
        anda update --skills
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/anda --version")
  end
end
EOF
}

need_cmd curl
need_cmd awk
need_cmd git

TAG=$(normalize_tag "$TAG")
VERSION="${TAG#v}"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

info "Generating Homebrew formula for ${REPO} ${TAG}..."

MACOS_ARM64_SHA=$(fetch_checksum "anda-macos-arm64")
MACOS_X86_64_SHA=$(fetch_checksum "anda-macos-x86_64")
LINUX_ARM64_SHA=$(fetch_checksum "anda-linux-arm64")
LINUX_X86_64_SHA=$(fetch_checksum "anda-linux-x86_64")

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

FORMULA_TMP="${TMPDIR}/${FORMULA_NAME}.rb"
write_formula > "$FORMULA_TMP"

if [ "${DRY_RUN:-}" = "1" ]; then
    cat "$FORMULA_TMP"
    exit 0
fi

if [ -n "${HOMEBREW_TAP_DIR:-}" ]; then
    TAP_DIR="$HOMEBREW_TAP_DIR"
    [ -d "${TAP_DIR}/.git" ] || error "HOMEBREW_TAP_DIR is not a git checkout: ${TAP_DIR}"
else
    [ -n "${HOMEBREW_TAP_TOKEN:-}" ] || error "Set HOMEBREW_TAP_TOKEN or HOMEBREW_TAP_DIR to publish the formula"
    TAP_DIR="${TMPDIR}/tap"
    git clone --branch "$TAP_BRANCH" "https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/${TAP_REPO}.git" "$TAP_DIR"
fi

mkdir -p "${TAP_DIR}/$(dirname "$FORMULA_PATH")"
cp "$FORMULA_TMP" "${TAP_DIR}/${FORMULA_PATH}"

git -C "$TAP_DIR" config user.name "${GIT_COMMITTER_NAME:-github-actions[bot]}"
git -C "$TAP_DIR" config user.email "${GIT_COMMITTER_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}"

if [ -z "$(git -C "$TAP_DIR" status --porcelain -- "$FORMULA_PATH")" ]; then
    info "Homebrew formula is already up to date."
    exit 0
fi

git -C "$TAP_DIR" add "$FORMULA_PATH"
git -C "$TAP_DIR" commit -m "Update ${FORMULA_NAME} formula to ${TAG}"

if [ "${PUSH:-1}" = "1" ]; then
    git -C "$TAP_DIR" push origin "HEAD:${TAP_BRANCH}"
    info "Published ${FORMULA_PATH} to ${TAP_REPO} ${TAP_BRANCH}."
else
    info "Updated ${FORMULA_PATH} in ${TAP_DIR}; PUSH=0 so changes were not pushed."
fi