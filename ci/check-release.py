"""Qualify the binaries placed in the release archive and editor extension."""
import hashlib
import http.client
import io
import json
from pathlib import Path
import platform
import os
import queue
import threading
import shutil
import subprocess
import sys
import tarfile
import tempfile
from urllib.parse import urlsplit
import zipfile


def digest(stream):
    hasher = hashlib.sha256()
    for block in iter(lambda: stream.read(1024 * 1024), b""):
        hasher.update(block)
    return hasher.digest()


def unpack(dist, dest):
    archive_path, = dist.glob("*.tar.gz")
    vsix_path, = dist.glob("*.vsix")
    compiler = dest / "tellur"
    with tarfile.open(archive_path) as tar:
        member = tar.getmember("tellur")
        assert member.isfile() and member.mode & 0o111
        with tar.extractfile(member) as source, compiler.open("wb") as output:
            shutil.copyfileobj(source, output)
    compiler.chmod(0o755)
    with zipfile.ZipFile(vsix_path) as vsix:
        server = vsix.getinfo("extension/server/tellur")
        assert server.external_attr >> 16 & 0o111
        with vsix.open(server) as source, compiler.open("rb") as cli:
            assert digest(source) == digest(cli), "VSIX and archive contain different compilers"
        version = json.loads(vsix.read("extension/package.json"))["version"]
    assert subprocess.check_output([compiler, "--version"], text=True).split()[1] == version
    return compiler, version


def check_lsp(compiler, version):
    messages = [
        {"id": 1, "method": "initialize", "params": {"capabilities": {}, "processId": None}},
        {"method": "initialized", "params": {}},
        {"id": 2, "method": "shutdown", "params": None},
        {"method": "exit", "params": None},
    ]
    frames = bytearray()
    for message in messages:
        payload = json.dumps({"jsonrpc": "2.0", **message}).encode()
        frames.extend(f"Content-Length: {len(payload)}\r\n\r\n".encode() + payload)
    run = subprocess.run([compiler, "lsp"], input=frames, capture_output=True, timeout=15, check=True)
    stream = io.BytesIO(run.stdout)
    replies = {}
    while stream.tell() < len(run.stdout):
        headers = {}
        while (line := stream.readline()) != b"\r\n":
            key, value = line.split(b":", 1)
            headers[key.lower()] = value.strip()
        reply = json.loads(stream.read(int(headers[b"content-length"])))
        replies[reply["id"]] = reply["result"]
    assert replies[1]["serverInfo"]["version"] == version and replies[2] is None
    assert replies[1]["capabilities"]["documentFormattingProvider"]


def check_settings(binary, directory, env):
    with subprocess.Popen([binary, "--entry", "serve", "--", directory / "settings.json", "0", "1", "10000"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env) as server:
        try:
            # Windows pipes cannot be passed to select(). Read the announcement
            # with a deadline on all three hosts, then collect the remaining output.
            lines = queue.Queue()
            reader = threading.Thread(target=lambda: lines.put(server.stdout.readline()), daemon=True)
            reader.start()
            address = urlsplit(lines.get(timeout=15).strip())
            reader.join()
            assert address.scheme == "http" and address.hostname == "127.0.0.1"
            connection = http.client.HTTPConnection(address.hostname, address.port, timeout=10)
            try:
                connection.request("GET", "/settings")
                response = connection.getresponse()
                assert response.status == 200 and json.loads(response.read()) == {}
            finally:
                connection.close()
            stdout, stderr = server.communicate(timeout=15)
            assert server.returncode == 0 and not stderr, (stdout, stderr)
        finally:
            if server.poll() is None:
                server.kill()


def copy_settings_project(project):
    # Model a consumer with an LF checkout, even when the pack producer uses
    # Git's Windows checkout settings. Keep relative module paths intact.
    for pattern in ["applications/settings/*.tellur", "applications/settings/manifest.json",
                    "applications/settings/Cargo.lock", "applications/settings/host.rs",
                    "library/*.tellur", "library/host/*.rs"]:
        for path in Path(".").glob(pattern):
            copy = project / path
            copy.parent.mkdir(parents=True, exist_ok=True)
            copy.write_bytes(path.read_bytes().replace(b"\r\n", b"\n"))
    return project / "applications/settings/main.tellur"


def qualify_adapter_pack(compiler, dest, scratch):
    source = copy_settings_project(dest / "project")
    original = Path("applications/settings/main.tellur")
    pack = dest / "settings.adapter"
    subprocess.run([compiler, "pack-adapter", original, "--out", pack], check=True)
    env = {"HOME": str(scratch), "PATH": "/absent", "TELLUR_NATIVE_TARGET_DIR": str(scratch / "packed-cache")}
    settings = scratch / "settings"
    build = [compiler, "build", source, "--adapter-pack", pack, "--out", settings]
    subprocess.run(build, env=env, check=True)
    check_settings(settings, scratch, env)

    def reject(options, expected):
        output = scratch / "rejected"
        failed = subprocess.run([*options, "--out", output], env=env, capture_output=True, text=True)
        assert failed.returncode and expected in failed.stderr, failed.stderr
        assert not output.exists() and not output.with_suffix(".ll").exists()

    for name in ["host.rs", "Cargo.lock", "manifest.json"]:
        file = source.with_name(name)
        saved = file.read_bytes()
        try:
            if name == "manifest.json":
                manifest = json.loads(saved)
                manifest["primitives"][0]["effects"].append("file.audit")
                file.write_text(json.dumps(manifest))
            else:
                file.write_bytes(saved + b"\n")
            reject(build[:-2], "adapter pack does not match")
        finally:
            file.write_bytes(saved)

    damaged = scratch / "damaged.adapter"
    shutil.copyfile(pack, damaged)
    with damaged.open("r+b") as stream:
        stream.seek(-1, 2)
        byte = stream.read(1)[0]
        stream.seek(-1, 2)
        stream.write(bytes([byte ^ 1]))
    reject([compiler, "build", source, "--adapter-pack", damaged], "damaged adapter archive")
    damaged.write_bytes(b"TellurAdapterPack/v1\n")
    reject([compiler, "build", source, "--adapter-pack", damaged], "truncated adapter pack")
    alternate = "aarch64" if platform.machine() == "x86_64" else "x86_64"
    reject([*build[:-2], "--target", alternate + "-unknown-linux-musl"], "adapter pack does not match")


def main():
    dist, dest = (Path(arg).resolve() for arg in sys.argv[1:])
    dest.mkdir(parents=True, exist_ok=True)
    compiler, version = unpack(dist, dest)
    check_lsp(compiler, version)
    for name in ["hello.tellur", "manifest.json"]:
        shutil.copyfile(Path("playground") / name, dest / name)
    with tempfile.TemporaryDirectory() as directory:
        scratch = Path(directory)
        env = {"HOME": directory, "PATH": "/absent", "TELLUR_NATIVE_TARGET_DIR": str(scratch / "cache")}
        output = subprocess.check_output([compiler, "exec", dest / "hello.tellur",
                                          "--out", scratch / "hello", "--", "--json"], env=env)
        report = json.loads(output)
        assert report["outcome"] == "done" and report["events"][0]["message"] == "Hello from Tellur!"
        qualify_adapter_pack(compiler, dest, scratch)
    print(f"Qualified packaged Tellur {version}, stdio LSP, cold Hello and packed settings HTTP.")



if __name__ == "__main__":
    if sys.argv[1:2] == ["--windows-target"]:
        directory = Path(sys.argv[2]).resolve()
        env = {**os.environ, "PATH": ""}
        hello = subprocess.check_output([directory / "hello-standalone.exe", "--json"], env=env, timeout=15)
        report = json.loads(hello)
        assert report["outcome"] == "done" and report["events"][0]["message"] == "Hello from Tellur!"
        with tempfile.TemporaryDirectory() as scratch:
            check_settings(directory / "settings-standalone.exe", Path(scratch), env)
    else:
        main()
