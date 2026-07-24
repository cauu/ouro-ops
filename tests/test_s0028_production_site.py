#!/usr/bin/env python3
import importlib.util
import pathlib
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "packaging" / "verify-production-site.py"


def main():
    subprocess.run([str(ROOT / "web" / "onboarding" / "build.sh")], check=True)
    subprocess.run(
        [
            "python3",
            str(SCRIPT),
            "--file",
            str(ROOT / "web" / "onboarding" / "dist" / "index.html"),
        ],
        check=True,
    )

    spec = importlib.util.spec_from_file_location("production_site", SCRIPT)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader
    spec.loader.exec_module(module)
    html = (ROOT / "web" / "onboarding" / "dist" / "index.html").read_text()
    try:
        module.verify(
            html.replace("connect-src 'none'", "connect-src https:"),
            ROOT / "ouro-skills",
            ROOT / "packaging" / "install-bootstrap.sh",
            ROOT / "packaging" / "ouro-install.sh",
        )
    except ValueError as error:
        assert "CSP" in str(error)
    else:
        raise AssertionError("weakened production CSP must fail")
    print("S0028 production Site smoke verifier passed")


if __name__ == "__main__":
    main()
