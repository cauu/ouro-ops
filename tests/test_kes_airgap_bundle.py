#!/usr/bin/env python3
"""S0025 p6-1-fix5 — platform bundle provenance, atomicity and offline execution."""

import hashlib
import io
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path

from test_s0020_kes_airgap_preflight import KES_VKEY
from test_s0020_stateless_plan import BIN, ROOT


VERSION = "10.14.0.0"
PLATFORMS = {
    "mac-apple-silicon": "aarch64-darwin",
    "mac-intel": "x86_64-darwin",
    "linux-intel-amd": "x86_64-linux",
    "linux-arm": "aarch64-linux",
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def host_choice() -> str:
    rustc = subprocess.run(["rustc", "-vV"], text=True, capture_output=True, check=True).stdout
    host = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    return {
        "aarch64-apple-darwin": "mac-apple-silicon",
        "x86_64-apple-darwin": "mac-intel",
        "x86_64-unknown-linux-gnu": "linux-intel-amd",
        "aarch64-unknown-linux-gnu": "linux-arm",
    }[host]


def mock_cli(version: str = VERSION) -> bytes:
    return f"""#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$*" >>"${{MOCK_CARDANO_CLI_LOG:-/dev/null}}"
if [ "${{1:-}}" = "--version" ]; then
  echo 'cardano-cli {version} - test-platform - ghc-test'
  exit 0
fi
[ "${{1:-}} ${{2:-}}" = 'node issue-op-cert' ] || exit 90
shift 2
while [ "$#" -gt 0 ]; do
  case "$1" in
    --operational-certificate-issue-counter-file) counter="$2"; shift 2 ;;
    --out-file) out="$2"; shift 2 ;;
    *) shift 2 ;;
  esac
done
before="$(cat "$counter")"
printf '%s\\n' "$((before + 1))" >"$counter"
printf '%s\\n' '{{"type":"NodeOperationalCertificate","description":"public mock","cborHex":"8200"}}' >"$out"
""".encode()


def write_archive(path: Path, entries: list[tuple[str, bytes]]) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, content in entries:
            info = tarfile.TarInfo(name)
            info.size = len(content)
            info.mode = 0o755
            archive.addfile(info, io.BytesIO(content))


def rewrite_sums(release: Path) -> None:
    lines = []
    for archive in sorted(release.glob("*.tar.gz")):
        lines.append(f"{digest(archive)}  {archive.name}\n")
    (release / f"cardano-cli-{VERSION}-sha256sums.txt").write_text("".join(lines))


def invoke(release: Path, vkey: Path, choice: str, out: Path, version: str = VERSION):
    env = os.environ.copy()
    env["OURO_CARDANO_CLI_RELEASE_DIR"] = str(release)
    return subprocess.run(
        [
            str(BIN),
            "kes",
            "airgap-bundle",
            "--kes-vkey",
            str(vkey),
            "--kes-period",
            "100",
            "--cardano-cli-version",
            version,
            "--platform",
            choice,
            "--out",
            str(out),
        ],
        text=True,
        capture_output=True,
        env=env,
    )


def invoke_canonical(release: Path, vkey: Path, choice: str, spec: Path, version: str = VERSION):
    env = os.environ.copy()
    env["OURO_CARDANO_CLI_RELEASE_DIR"] = str(release)
    return subprocess.run(
        [
            str(BIN), "kes", "airgap-bundle",
            "--kes-vkey", str(vkey),
            "--kes-period", "100",
            "--cardano-cli-version", version,
            "--platform", choice,
            "--spec", str(spec),
            "--node", "bp1",
        ],
        text=True, capture_output=True, env=env,
    )


def cleanup(spec: Path, expected: str):
    return subprocess.run(
        [
            str(BIN), "kes", "airgap-cleanup",
            "--spec", str(spec),
            "--node", "bp1",
            "--expected-vkey-sha256", expected,
        ],
        text=True, capture_output=True,
    )


def assert_no_partial(parent: Path, name: str) -> None:
    assert not any(parent.glob(f".{name}.ouro-partial-*"))


def main() -> None:
    subprocess.run(["cargo", "build", "-p", "ouro"], cwd=ROOT, check=True)
    home = Path(tempfile.mkdtemp(prefix="ouro-kes-airgap-bundle-"))
    release = home / "release"
    release.mkdir()
    cli = mock_cli()
    for target in PLATFORMS.values():
        asset = release / f"cardano-cli-{VERSION}-{target}.tar.gz"
        write_archive(asset, [(f"bin/cardano-cli-{target}", cli)])
    rewrite_sums(release)
    vkey = home / "source-kes.vkey"
    vkey.write_text(json.dumps(KES_VKEY, separators=(",", ":")))

    spec = home / "control" / "pool-spec.yaml"
    spec.parent.mkdir()
    spec.write_text("""spec_version: 1
pool:
  network: mainnet
  network_magic: 764824073
  genesis_hashes:
    shelley: "1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81"
topology_mode: p2p
machines:
  - id: bp1
    role: bp
    ssh: { host: bp1, port: 22, user: cardano, key_ref: "creds://bp1" }
  - id: relay1
    role: relay
    public_endpoint: { host: relay1, port: 3001 }
    ssh: { host: relay1, port: 22, user: cardano, key_ref: "creds://relay1" }
upgrade:
  min_online_relays: 0
""")

    # Every friendly device choice selects exactly one official platform asset.
    bundles = {}
    for choice, target in PLATFORMS.items():
        out = home / f"bundle-{choice}"
        completed = invoke(release, vkey, choice, out)
        assert completed.returncode == 0, completed.stderr
        value = json.loads(completed.stdout)
        assert value["tool"] == "ouro.kes.airgap-bundle" and value["changed"] is True
        manifest = json.loads((out / "manifest.json").read_text())
        assert manifest["platform"] == target
        assert manifest["cardano_cli"]["asset"] == f"cardano-cli-{VERSION}-{target}.tar.gz"
        assert set(path.name for path in out.iterdir()) == {
            "kes.vkey",
            "cold-sign.sh",
            "cardano-cli",
            "manifest.json",
            "SHA256SUMS",
        }
        script = (out / "cold-sign.sh").read_text()
        assert 'CARDANO_CLI="$SCRIPT_DIR/cardano-cli"' in script
        assert "curl" not in script and "wget" not in script
        for line in (out / "SHA256SUMS").read_text().splitlines():
            expected, name = line.split()
            assert digest(out / name) == expected
        bundles[choice] = out

    # The canonical control-machine handoff has one derived path and is safely resumable.
    canonical = invoke_canonical(release, vkey, host_choice(), spec)
    assert canonical.returncode == 0, canonical.stderr
    canonical_value = json.loads(canonical.stdout)
    pending = spec.parent / "ouro-kes-rotation" / "bp1" / "pending"
    assert canonical_value["changed"] is True
    assert canonical_value["data"]["bundle_dir"] == str(pending.resolve())
    assert canonical_value["data"]["node_cert_path"] == str((pending / "node.cert").resolve())
    assert canonical_value["data"]["node_cert_present"] is False

    resumed = invoke_canonical(release, vkey, host_choice(), spec)
    assert resumed.returncode == 0, resumed.stderr
    resumed_value = json.loads(resumed.stdout)
    assert resumed_value["changed"] is False
    assert resumed_value["data"]["reused"] is True

    # Every structural or content deviation refuses reuse without changing the directory.
    for anomaly in ("missing", "unknown", "nested", "symlink", "content"):
        bad = home / f"resume-{anomaly}"
        shutil.copytree(pending, bad)
        if anomaly == "missing":
            (bad / "SHA256SUMS").unlink()
        elif anomaly == "unknown":
            (bad / "notes.txt").write_text("unexpected")
        elif anomaly == "nested":
            (bad / "extra").mkdir()
        elif anomaly == "symlink":
            (bad / "SHA256SUMS").unlink()
            (bad / "SHA256SUMS").symlink_to(bad / "manifest.json")
        else:
            with (bad / "cold-sign.sh").open("a") as changed:
                changed.write("# modified\n")
        refused = invoke(release, vkey, host_choice(), bad)
        assert refused.returncode != 0
        assert bad.exists()

    # Cleanup is bound to the public staged key and permits only the fixed returned node.cert.
    (pending / "node.cert").write_text("public mock returned certificate")
    (pending / "notes.txt").write_text("must not be deleted")
    unsafe_cleanup = cleanup(spec, digest(vkey))
    assert unsafe_cleanup.returncode != 0 and pending.exists()
    (pending / "notes.txt").unlink()
    wrong_cleanup = cleanup(spec, "0" * 64)
    assert wrong_cleanup.returncode != 0 and pending.exists()
    completed_cleanup = cleanup(spec, digest(vkey))
    assert completed_cleanup.returncode == 0, completed_cleanup.stderr
    cleanup_value = json.loads(completed_cleanup.stdout)
    assert cleanup_value["changed"] is True and cleanup_value["data"]["absent"] is True
    assert not pending.exists()
    assert not (spec.parent / "ouro-kes-rotation").exists()
    repeated_cleanup = cleanup(spec, digest(vkey))
    assert repeated_cleanup.returncode == 0
    assert json.loads(repeated_cleanup.stdout)["changed"] is False

    # With no PATH cardano-cli and no network, the adjacent binary signs once and advances counter.
    bundle = bundles[host_choice()]
    cold = home / "cold"
    cold.mkdir()
    (cold / "cold.skey").write_text("private-key-stays-here")
    (cold / "opcert.counter").write_text("7\n")
    cli_log = cold / "cli.log"
    env = os.environ.copy()
    env.update({
        "COLD_SKEY": str(cold / "cold.skey"),
        "COUNTER": str(cold / "opcert.counter"),
        "OUT": str(cold / "node.cert"),
        "MOCK_CARDANO_CLI_LOG": str(cli_log),
    })
    signed = subprocess.run([str(bundle / "cold-sign.sh")], cwd=cold, env=env, text=True, capture_output=True)
    assert signed.returncode == 0, signed.stderr
    assert (cold / "opcert.counter").read_text().strip() == "8"
    assert (cold / "node.cert").is_file()
    calls = cli_log.read_text().splitlines()
    assert sum("node issue-op-cert" in call for call in calls) == 1
    assert sum(call == "--version" for call in calls) == 1

    # Public manifest or executable tampering stops before backup/counter/signing.
    for target in ("manifest.json", "cardano-cli"):
        tampered = home / f"tampered-{target}"
        shutil.copytree(bundle, tampered)
        with (tampered / target).open("ab") as changed:
            changed.write(b"tampered")
        counter = home / f"counter-{target}"
        counter.write_text("9\n")
        tamper_log = home / f"log-{target}"
        tamper_env = env | {
            "COUNTER": str(counter),
            "OUT": str(home / f"node-{target}.cert"),
            "MOCK_CARDANO_CLI_LOG": str(tamper_log),
        }
        refused = subprocess.run([str(tampered / "cold-sign.sh")], env=tamper_env, text=True, capture_output=True)
        assert refused.returncode != 0
        assert counter.read_text().strip() == "9"
        assert not Path(f"{counter}.ouro-bak").exists()
        assert not tamper_log.exists() or "issue-op-cert" not in tamper_log.read_text()

    # Closed platform/version inputs and release integrity failures never promote an output.
    refused_out = home / "refused-platform"
    refused = invoke(release, vkey, "windows", refused_out)
    assert refused.returncode != 0 and not refused_out.exists()
    assert_no_partial(home, refused_out.name)

    missing_out = home / "missing-version"
    missing = invoke(release, vkey, host_choice(), missing_out, version="9.9.9.9")
    assert missing.returncode != 0 and not missing_out.exists()
    assert_no_partial(home, missing_out.name)

    asset_name = f"cardano-cli-{VERSION}-{PLATFORMS[host_choice()]}.tar.gz"
    sums = release / f"cardano-cli-{VERSION}-sha256sums.txt"
    original_sums = sums.read_text()
    sums.write_text(original_sums.replace(digest(release / asset_name), "0" * 64))
    checksum_out = home / "bad-checksum"
    checksum = invoke(release, vkey, host_choice(), checksum_out)
    assert checksum.returncode != 0 and not checksum_out.exists()
    assert_no_partial(home, checksum_out.name)
    sums.write_text(original_sums)

    sums.write_text("not-a-checksum\n")
    malformed_out = home / "malformed-checksum"
    malformed = invoke(release, vkey, host_choice(), malformed_out)
    assert malformed.returncode != 0 and not malformed_out.exists()
    assert_no_partial(home, malformed_out.name)
    sums.write_text(original_sums)

    # Unsafe archive paths are rejected even when their archive checksum is authentic.
    archive = release / asset_name
    original_archive = archive.read_bytes()
    write_archive(archive, [(f"../cardano-cli-{PLATFORMS[host_choice()]}", cli)])
    rewrite_sums(release)
    unsafe_out = home / "unsafe-archive"
    unsafe = invoke(release, vkey, host_choice(), unsafe_out)
    assert unsafe.returncode != 0 and not unsafe_out.exists()
    assert_no_partial(home, unsafe_out.name)
    archive.write_bytes(original_archive)
    rewrite_sums(release)

    expected_binary = f"cardano-cli-{PLATFORMS[host_choice()]}"
    write_archive(archive, [("README", b"no executable here")])
    rewrite_sums(release)
    missing_binary_out = home / "missing-binary"
    missing_binary = invoke(release, vkey, host_choice(), missing_binary_out)
    assert missing_binary.returncode != 0 and not missing_binary_out.exists()
    assert_no_partial(home, missing_binary_out.name)

    write_archive(
        archive,
        [(f"one/{expected_binary}", cli), (f"two/{expected_binary}", cli)],
    )
    rewrite_sums(release)
    multiple_out = home / "multiple-binaries"
    multiple = invoke(release, vkey, host_choice(), multiple_out)
    assert multiple.returncode != 0 and not multiple_out.exists()
    assert_no_partial(home, multiple_out.name)

    # A same-platform executable reporting another version is rejected before promotion.
    write_archive(
        archive,
        [(f"bin/cardano-cli-{PLATFORMS[host_choice()]}", mock_cli("99.0.0.0"))],
    )
    rewrite_sums(release)
    wrong_version_out = home / "wrong-reported-version"
    wrong_version = invoke(release, vkey, host_choice(), wrong_version_out)
    assert wrong_version.returncode != 0 and not wrong_version_out.exists(), wrong_version
    assert_no_partial(home, wrong_version_out.name)

    print("S0025 platform-specific KES air-gap bundle passed")


if __name__ == "__main__":
    main()
