"""Exercise the Windows-hosted compiler, embedded pack and language server."""
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile


def unpack(dist, directory, version, digest):
    basename = f"tellur-{version}-win32-x64"
    compiler = directory / "tellur.exe"
    with zipfile.ZipFile(dist / f"{basename}.zip") as archive:
        with archive.open("tellur.exe") as source, compiler.open("wb") as output:
            shutil.copyfileobj(source, output)
        assert archive.read("LICENSE")
    with zipfile.ZipFile(dist / f"{basename}.vsix") as vsix:
        manifest = ET.fromstring(vsix.read("extension.vsixmanifest"))
        identity = manifest.find("{*}Metadata/{*}Identity")
        assert identity is not None and identity.get("TargetPlatform") == "win32-x64"
        assert identity.get("Version") == version
        assert json.loads(vsix.read("extension/package.json"))["version"] == version
        with vsix.open("extension/server/tellur.exe") as server, compiler.open("rb") as cli:
            assert digest(server) == digest(cli), "VSIX and CLI ship different executables"
    return compiler


def qualify(dist, directory):
    checks = runpy.run_path(str(Path(__file__).with_name("check-release.py")))
    version = json.loads(Path("editors/vscode/package.json").read_text())["version"]
    compiler = unpack(dist, directory, version, checks["digest"])
    target = "x86_64-pc-windows-gnu"
    settings = "applications/settings/main.tellur"
    adapter = directory / "settings.adapter"
    build_env = {**os.environ, "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS":
                 "-Cdlltool=" + os.environ["TELLUR_WINDOWS_DLLTOOL"]}
    subprocess.run([compiler, "pack-adapter", settings, "--target", target, "--out", adapter],
                   check=True, env=build_env)
    consumer = checks["copy_settings_project"](directory / "project")
    os.environ["PATH"] = ""
    os.environ["TELLUR_NATIVE_TARGET_DIR"] = str(directory / "clean-cache")
    assert subprocess.check_output([compiler, "--version"], text=True).split()[1] == version
    checks["check_lsp"](compiler, version)
    hello = directory / "hello.exe"
    subprocess.run([compiler, "build", "playground/hello.tellur", "--backend", "llvm", "--out", hello], check=True)
    report = json.loads(subprocess.check_output([hello, "--json"], timeout=15))
    assert report["outcome"] == "done" and report["events"][0]["message"] == "Hello from Tellur!"
    service = directory / "settings.exe"
    subprocess.run([compiler, "build", consumer, "--backend", "llvm", "--adapter-pack", adapter, "--out", service],
                   check=True)
    checks["check_settings"](service, directory, os.environ)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory() as scratch:
        qualify(Path(sys.argv[1]), Path(scratch))
