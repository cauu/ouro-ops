#!/usr/bin/env bash
set -euo pipefail

SITE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE="$SITE_DIR/index.html"
DIST="$SITE_DIR/dist"

if [[ ! -f "$SOURCE" ]]; then
  echo "missing onboarding entry point: $SOURCE" >&2
  exit 1
fi

rm -rf "$DIST"
install -d "$DIST"
install -m 0644 "$SOURCE" "$DIST/index.html"

if [[ "$(find "$DIST" -type f | wc -l | tr -d ' ')" != "1" ]]; then
  echo "site build must contain exactly one file" >&2
  exit 1
fi

if ! cmp -s "$SOURCE" "$DIST/index.html"; then
  echo "staged onboarding page differs from its source" >&2
  exit 1
fi

echo "built $DIST/index.html"
