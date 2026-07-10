#!/usr/bin/env sh
# S0016 p2-4 — official install script (curl fallback to brew/npx).
#
# The trust model (R2 N4): the expected signing identity is PINNED here (from
# packaging/SIGNING_IDENTITY), not fetched from wherever the user landed. The binary is
# verified against that fixed identity BEFORE it is trusted. A spoofed site cannot make this
# accept an attacker binary, because it cannot produce a signature under the pinned identity.
#
# URLs/version below are release-filled placeholders; the VERIFICATION LOGIC is real.
set -eu

VERSION="0.1.0"
# Pinned trust anchors (must match packaging/SIGNING_IDENTITY and the official site).
COSIGN_IDENTITY="release@ouro.example"
COSIGN_ISSUER="https://token.actions.githubusercontent.com"
BASE="https://github.com/ouro/ouro/releases/download/v${VERSION}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
  Darwin-x86_64) TARGET="x86_64-apple-darwin" ;;
  Linux-x86_64)  TARGET="x86_64-unknown-linux-musl" ;;
  Linux-aarch64) TARGET="aarch64-unknown-linux-musl" ;;
  *) echo "unsupported platform: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
echo "Downloading ouro ${VERSION} (${TARGET})..."
curl -fsSL "${BASE}/ouro-ops-${TARGET}"      -o "$TMP/ouro-ops"
curl -fsSL "${BASE}/ouro-ops-${TARGET}.sig"  -o "$TMP/ouro-ops.sig"
curl -fsSL "${BASE}/ouro-ops-${TARGET}.pem"  -o "$TMP/ouro-ops.pem"

# Verify against the PINNED identity (fail closed if cosign is absent — do not trust unverified).
if ! command -v cosign >/dev/null 2>&1; then
  echo "cosign is required to verify the release signature. Install cosign and retry, or use 'brew install ouro/tap/ouro-ops'." >&2
  exit 1
fi
echo "Verifying signature against pinned identity ${COSIGN_IDENTITY}..."
cosign verify-blob \
  --certificate "$TMP/ouro-ops.pem" \
  --signature "$TMP/ouro-ops.sig" \
  --certificate-identity "$COSIGN_IDENTITY" \
  --certificate-oidc-issuer "$COSIGN_ISSUER" \
  "$TMP/ouro-ops"

install -m 0755 "$TMP/ouro-ops" "${OURO_BIN_DIR:-/usr/local/bin}/ouro-ops"
echo "Installed. Cross-check now:  ouro-ops version && ouro-ops contract"
echo "(compare the output against the values shown on the official site.)"
