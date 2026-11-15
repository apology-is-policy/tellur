"""Exercise the Windows-hosted compiler, embedded pack and language server."""
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile


def qualify(executable, directory):
    compiler = directory / "tellur.exe"
    shutil.copy2(executable, compiler)
    checks = runpy.run_path(str(Path(__file__).with_name("check-release.py")))
    version = json.loads(Path("editors/vscode/package.json").read_text())["version"]
    target = "x86_64-pc-windows-gnu"
    settings = "applications/settings/main.tellur"
    adapter = directory / "settings.adapter"
    build_env = {**os.environ, "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS":
                 "-Cdlltool=" + os.environ["TELLUR_WINDOWS_DLLTOOL"]}
    subprocess.run([compiler, "pack-adapter", settings, "--target", target, "--out", adapter],
                   check=True, env=build_env)
    os.environ["PATH"] = ""
    os.environ["TELLUR_NATIVE_TARGET_DIR"] = str(directory / "clean-cache")
    assert subprocess.check_output([compiler, "--version"], text=True).split()[1] == version
    checks["check_lsp"](compiler, version)
    hello = directory / "hello.exe"
    subprocess.run([compiler, "build", "playground/hello.tellur", "--backend", "llvm", "--out", hello], check=True)
    report = json.loads(subprocess.check_output([hello, "--json"], timeout=15))
    assert report["outcome"] == "done" and report["events"][0]["message"] == "Hello from Tellur!"
    service = directory / "settings.exe"
    subprocess.run([compiler, "build", settings, "--backend", "llvm", "--adapter-pack", adapter, "--out", service],
                   check=True)
    checks["check_settings"](service, directory, os.environ)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as scratch:
        qualify(Path(sys.argv[1]), Path(scratch))
