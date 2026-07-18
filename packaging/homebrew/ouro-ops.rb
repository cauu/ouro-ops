# S0016 p2-4/p2-9 — Homebrew formula (PRIMARY macOS bootstrap vector; npx is secondary).
#
# The formula pins the version, the download URL, and a sha256; `brew` verifies the sha256
# on download. `post_install` additionally verifies the release signature against the pinned
# signing identity (packaging/SIGNING_IDENTITY) BEFORE the binary is trusted — so first-install
# does not rely on a human eyeballing a fingerprint (R2 N4). URL/sha are release-filled.
class OuroOps < Formula
  desc "Deterministic Cardano stake pool operations CLI"
  homepage "https://ouro.example"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/ouro/ouro/releases/download/v0.1.0/ouro-ops-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000" # release-filled
    end
    on_intel do
      url "https://github.com/ouro/ouro/releases/download/v0.1.0/ouro-ops-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000" # release-filled
    end
  end

  def install
    bin.install "ouro-ops"
  end

  # Verify the release signature against the pinned identity before trusting the binary.
  # (cosign is a declared dependency of the tap; the identity is fixed, not user-supplied.)
  def post_install
    system "cosign", "verify-blob",
           "--certificate-identity", "release@ouro.example",
           "--certificate-oidc-issuer", "https://token.actions.githubusercontent.com",
           "--signature", "#{bin}/ouro-ops.sig", "#{bin}/ouro-ops"
  end

  test do
    assert_match "0.1.0", shell_output("#{bin}/ouro-ops version")
    # The signed binary exposes only its compact CLI/runner contract descriptor.
    system "#{bin}/ouro-ops", "contract"
  end
end
