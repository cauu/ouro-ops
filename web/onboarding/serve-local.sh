#!/usr/bin/env bash
set -euo pipefail

SITE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PORT="${1:-4173}"

if [[ ! "$PORT" =~ ^[0-9]+$ ]] || (( PORT < 1 || PORT > 65535 )); then
  echo "usage: $0 [port]" >&2
  exit 2
fi

"$SITE_DIR/build.sh"
exec python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$SITE_DIR/dist"
