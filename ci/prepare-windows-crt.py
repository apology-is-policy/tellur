"""Add the pinned x64 MinGW CRT libraries to a local copy of Rust's Windows target."""
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
from urllib.request import urlopen

TARGET = "x86_64-pc-windows-gnu"
COMPONENT = f"rust-mingw-1.97.1-{TARGET}"
URL = f"https://static.rust-lang.org/dist/2026-07-16/{COMPONENT}.tar.xz"
SHA256 = "aa257e5f8354bd1bc1cacbcbcdede3ebb89a100bef504d72da56db8527807ad2"
CRT = ("libmingw32.a", "libmingwex.a", "libmoldname.a", "libmsvcrt.a", "libgcc.a", "libgcc_eh.a")


def prepare(destination):
    sysroot = Path(subprocess.check_output(["rustc", "--print", "sysroot"], text=True).strip())
    relative = Path("lib/rustlib") / TARGET / "lib"
    if not (sysroot / relative / "self-contained/crt2.o").is_file():
        sys.exit(f"Install the {TARGET} Rust standard library first")
    shutil.copytree(sysroot / relative, destination / relative, dirs_exist_ok=True)
    with tempfile.TemporaryDirectory() as scratch:
        archive = Path(scratch) / f"{COMPONENT}.tar.xz"
        with urlopen(URL, timeout=60) as source, archive.open("wb") as output:
            shutil.copyfileobj(source, output)
        if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
            sys.exit("The Windows CRT component did not match the pinned Rust distribution")
        with tarfile.open(archive) as tar:
            for library in CRT:
                path = relative / "self-contained" / library
                with tar.extractfile(f"{COMPONENT}/rust-mingw/{path.as_posix()}") as source:
                    with (destination / path).open("wb") as output:
                        shutil.copyfileobj(source, output)


if __name__ == "__main__":
    prepare(Path(sys.argv[1]))
