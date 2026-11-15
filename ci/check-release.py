"""Qualify the binaries placed in the release archive and editor extension."""
import hashlib
import http.client
import io
import json
from pathlib import Path
import select
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
            assert select.select([server.stdout], [], [], 15)[0], "settings did not announce its port"
            address = urlsplit(server.stdout.readline().strip())
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
        output = subprocess.check_output([compiler, "exec", dest / "hello.tellur", "--backend", "llvm",
                                          "--out", scratch / "hello", "--", "--json"], env=env)
        report = json.loads(output)
        assert report["outcome"] == "done" and report["events"][0]["message"] == "Hello from Tellur!"
        settings = dest / "settings"
        subprocess.run([compiler, "build", "applications/settings/main.tellur", "--backend", "llvm",
                        "--out", settings], check=True)
        check_settings(settings, scratch, env)
    print(f"Qualified packaged Tellur {version}, stdio LSP, cold Hello build and settings HTTP.")


if __name__ == "__main__":
    main()
