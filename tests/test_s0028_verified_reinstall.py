#!/usr/bin/env python3
import hashlib
import io
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALL = ROOT / "packaging" / "ouro-install.sh"
BOOTSTRAP = ROOT / "packaging" / "install-bootstrap.sh"
TARGETS = {
    ("Linux", "x86_64"): "x86_64-unknown-linux-musl",
    ("Linux", "aarch64"): "aarch64-unknown-linux-musl",
    ("Darwin", "x86_64"): "x86_64-apple-darwin",
    ("Darwin", "arm64"): "aarch64-apple-darwin",
}


def binary(version, marker="canonical"):
    return f"""#!/bin/sh
case "$1" in
  version) printf '%s\\n' '{{"status":"ok","data":{{"version":"{version}","binary":"ouro-ops"}}}}' ;;
  contract)
    if [ "${{2:-}}" = check ]; then exit 0; fi
    printf '%s\\n' '{{"status":"ok","data":{{"ouro_version":"{version}","cli_contract":1,"runner_platform":"linux/x86_64","runner_sha256":"{'a' * 64}"}}}}'
    ;;
  *) exit 10 ;;
esac
# {marker}
""".encode()


def make_release(directory, version="0.1.1", marker="canonical"):
    payload = binary(version, marker)
    lines = []
    for target in TARGETS.values():
        archive = directory / f"ouro-ops-v{version}-{target}.tar.gz"
        info = tarfile.TarInfo("ouro-ops")
        info.size = len(payload)
        info.mode = 0o755
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.addfile(info, io.BytesIO(payload))
        lines.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n")
    shutil.copy2(INSTALL, directory / "ouro-install.sh")
    installer = directory / "ouro-install.sh"
    lines.append(f"{hashlib.sha256(installer.read_bytes()).hexdigest()}  ouro-install.sh\n")
    (directory / "SHA256SUMS").write_text("".join(lines))


def fake_tools(directory):
    gh = directory / "gh"
    gh.write_text(
        """#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
log = os.environ.get("GH_TEST_LOG")
if log:
    with open(log, "a") as output:
        output.write(" ".join(args) + "\\n")
if args[:2] == ["release", "view"]:
    print(os.environ.get("GH_TEST_TAG", "v0.1.1"))
elif args[:2] == ["release", "download"]:
    fixture = pathlib.Path(os.environ["GH_TEST_FIXTURE"])
    patterns = [
        args[i + 1] for i, value in enumerate(args) if value in ("--pattern", "-p")
    ]
    if os.environ.get("GH_TEST_FAIL_DOWNLOAD"):
        raise SystemExit(1)
    if "-O" in args and args[args.index("-O") + 1] == "-":
        sys.stdout.buffer.write((fixture / patterns[0]).read_bytes())
    else:
        target = pathlib.Path(args[args.index("--dir") + 1])
        for pattern in patterns:
            shutil.copy2(fixture / pattern, target / pattern)
elif args[:2] in (["release", "verify"], ["release", "verify-asset"]):
    if os.environ.get("GH_TEST_FAIL_VERIFY"):
        raise SystemExit(1)
elif args[:2] == ["attestation", "verify"]:
    if os.environ.get("GH_TEST_FAIL_ATTEST"):
        raise SystemExit(1)
else:
    raise SystemExit("unexpected fake gh argv: " + repr(args))
"""
    )
    gh.chmod(0o755)
    uname = directory / "uname"
    uname.write_text(
        """#!/bin/sh
case "$1" in
  -s) printf '%s\\n' "$GH_TEST_OS" ;;
  -m) printf '%s\\n' "$GH_TEST_ARCH" ;;
  *) exit 2 ;;
esac
"""
    )
    uname.chmod(0o755)


def run(home, fixture, fakebin, os_name, arch, entry=INSTALL, **extra):
    env = dict(
        os.environ,
        HOME=str(home),
        PATH=f"{fakebin}:/usr/bin:/bin",
        GH_TEST_FIXTURE=str(fixture),
        GH_TEST_OS=os_name,
        GH_TEST_ARCH=arch,
        GH_TEST_TAG=extra.pop("tag", "v0.1.1"),
        GH_TEST_LOG=str(home / "gh.log"),
        **{key: str(value) for key, value in extra.items()},
    )
    return subprocess.run(
        ["/bin/sh", str(entry)], env=env, text=True, capture_output=True
    )


def write_current(home, version, marker="canonical"):
    destination = home / ".local" / "bin" / "ouro-ops"
    destination.parent.mkdir(parents=True)
    destination.write_bytes(binary(version, marker))
    destination.chmod(0o755)
    return destination


def main():
    subprocess.run(["sh", "-n", str(INSTALL)], check=True)
    subprocess.run(["sh", "-n", str(BOOTSTRAP)], check=True)
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        fixture = root / "release"
        fakebin = root / "fakebin"
        fixture.mkdir()
        fakebin.mkdir()
        make_release(fixture)
        fake_tools(fakebin)

        for platform, target in TARGETS.items():
            home = root / f"fresh-{target}"
            home.mkdir()
            completed = run(home, fixture, fakebin, *platform)
            destination = home / ".local" / "bin" / "ouro-ops"
            assert completed.returncode == 0, completed
            assert destination.is_file() and os.access(destination, os.X_OK)
            assert "fresh_install" in completed.stdout
            log = (home / "gh.log").read_text()
            assert f"ouro-ops-v0.1.1-{target}.tar.gz" in log
            assert "release verify v0.1.1" in log
            assert "release verify-asset" in log
            assert (
                "attestation verify" in log
                and "--signer-workflow cauu/ouro-ops/.github/workflows/release-publish.yml"
                in log
            )

        home = root / "bootstrap"
        home.mkdir()
        completed = run(
            home, fixture, fakebin, "Darwin", "arm64", entry=BOOTSTRAP
        )
        assert completed.returncode == 0, completed
        assert (home / ".local" / "bin" / "ouro-ops").is_file()
        log = (home / "gh.log").read_text()
        assert (
            "release download -R cauu/ouro-ops -p ouro-install.sh -O -" in log
        )
        assert "release download v0.1.1" in log
        assert "release verify-asset v0.1.1" in log
        assert (
            "attestation verify" in log
            and "--signer-workflow cauu/ouro-ops/.github/workflows/release-publish.yml"
            in log
        )

        home = root / "bootstrap-download-refusal"
        home.mkdir()
        refused = run(
            home,
            fixture,
            fakebin,
            "Linux",
            "x86_64",
            entry=BOOTSTRAP,
            GH_TEST_FAIL_DOWNLOAD="1",
        )
        assert refused.returncode != 0
        assert not (home / ".local").exists()

        for platform in (("Darwin", "arm64"), ("Linux", "x86_64")):
            home = root / f"update-{platform[0]}-{platform[1]}"
            home.mkdir()
            destination = write_current(home, "0.1.0")
            completed = run(home, fixture, fakebin, *platform)
            assert completed.returncode == 0 and "forward_update" in completed.stdout
            assert b'"version":"0.1.1"' in destination.read_bytes()

            before = (destination.stat().st_ino, destination.stat().st_mtime_ns, destination.read_bytes())
            repeated = run(home, fixture, fakebin, *platform)
            after = (destination.stat().st_ino, destination.stat().st_mtime_ns, destination.read_bytes())
            assert repeated.returncode == 0 and "no write performed" in repeated.stdout
            assert before == after

        home = root / "downgrade"
        home.mkdir()
        destination = write_current(home, "0.2.0")
        before = destination.read_bytes()
        refused = run(home, fixture, fakebin, "Linux", "x86_64")
        assert refused.returncode != 0 and "refusing downgrade" in refused.stderr
        assert destination.read_bytes() == before

        home = root / "unknown"
        home.mkdir()
        destination = home / ".local" / "bin" / "ouro-ops"
        destination.parent.mkdir(parents=True)
        destination.write_text("#!/bin/sh\nexit 1\n")
        destination.chmod(0o755)
        before = destination.read_bytes()
        refused = run(home, fixture, fakebin, "Darwin", "arm64")
        assert refused.returncode != 0 and "cannot be verified as Ouro Ops" in refused.stderr
        assert destination.read_bytes() == before

        home = root / "same-version-different"
        home.mkdir()
        destination = write_current(home, "0.1.1", marker="different")
        before = destination.read_bytes()
        refused = run(home, fixture, fakebin, "Linux", "x86_64")
        assert refused.returncode != 0 and "same-version" in refused.stderr
        assert destination.read_bytes() == before

        for name, extra in (
            ("prerelease", {"tag": "v0.1.1-rc.1"}),
            ("bad-attestation", {"GH_TEST_FAIL_ATTEST": "1"}),
            ("bad-release", {"GH_TEST_FAIL_VERIFY": "1"}),
        ):
            home = root / name
            home.mkdir()
            refused = run(home, fixture, fakebin, "Linux", "x86_64", **extra)
            assert refused.returncode != 0, (name, refused)
            assert not (home / ".local").exists(), name

        no_gh = root / "no-gh"
        no_gh.mkdir()
        no_gh_path = root / "no-gh-path"
        no_gh_path.mkdir()
        env = dict(os.environ, HOME=str(no_gh), PATH=str(no_gh_path))
        missing = subprocess.run(
            ["/bin/sh", str(INSTALL)], env=env, text=True, capture_output=True
        )
        assert missing.returncode != 0 and "GitHub CLI" in missing.stderr
        assert not (no_gh / ".local").exists()

    cli = (ROOT / "crates" / "ouro" / "src" / "cli.rs").read_text()
    live = "\n".join(
        path.read_text()
        for path in [
            ROOT / "packaging" / "RELEASE.md",
            ROOT / "web" / "onboarding" / "README.md",
        ]
    )
    assert "self-update" not in cli
    for removed in (
        ROOT / "packaging" / "install.sh",
        ROOT / "packaging" / "SIGNING_IDENTITY",
        ROOT / "packaging" / "homebrew" / "ouro-ops.rb",
    ):
        assert not removed.exists(), removed
    for placeholder in ("release@ouro.example", "RWQPLACEHOLDER", "ouro/tap", "@ouro/ops"):
        assert placeholder not in live, placeholder
    assert (
        BOOTSTRAP.read_text()
        == "bash -o pipefail -c 'gh release download -R cauu/ouro-ops "
        "-p ouro-install.sh -O - | sh'\n"
    )
    print("S0028 verified reinstall and legacy distribution removal passed")


if __name__ == "__main__":
    main()
