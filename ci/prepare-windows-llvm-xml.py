"""Provide the static libxml2 used by the official LLVM 22.1.8 MSVC build."""
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
from urllib.request import urlopen

COMMIT = "b48e77cf4f6fa0792c5f4b639707a2b0675e461b"  # libxml2 v2.9.12
SHA256 = "103f519711a4bdc7683e7368c8a56786f8151f3fbbbb04615337a951b52beff7"
# Match llvm/utils/release/build_llvm_release.bat at llvmorg-22.1.8.
DISABLED = (
    "C14N CATALOG DEBUG DOCB FTP HTML HTTP ICONV ICU ISO8859X LEGACY LZMA MEM_DEBUG "
    "MODULES PATTERN PROGRAMS PUSH PYTHON READER REGEXPS RUN_DEBUG SCHEMAS SCHEMATRON "
    "TESTS THREAD_ALLOC VALID WRITER XINCLUDE XPATH XPTR ZLIB"
).split()


def prepare(prefix):
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        archive = root / "libxml2.tar.gz"
        with urlopen(f"https://codeload.github.com/GNOME/libxml2/tar.gz/{COMMIT}", timeout=60) as src:
            with archive.open("wb") as dst:
                shutil.copyfileobj(src, dst)
        if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
            sys.exit("Unexpected digest for the pinned LLVM libxml2 source")
        with tarfile.open(archive) as tar:
            tar.extractall(root, filter="data")
        source = root / f"libxml2-{COMMIT}"
        build = root / "build"
        subprocess.run(["cmake", "-S", source, "-B", build, "-A", "x64", "-DBUILD_SHARED_LIBS=OFF",
                        "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded",
                        *(f"-DLIBXML2_WITH_{item}=OFF" for item in DISABLED),
                        *(f"-DLIBXML2_WITH_{item}=ON" for item in ("OUTPUT", "SAX1", "THREADS", "TREE"))],
                       check=True)
        subprocess.run(["cmake", "--build", build, "--config", "Release", "--target", "LibXml2"], check=True)
        # llvm-config reports xml2s.lib. CMake places the same static library under libxml2s.lib.
        shutil.copy2(build / "Release/libxml2s.lib", prefix / "lib/xml2s.lib")
        shutil.copy2(source / "Copyright", prefix / "libxml2-Copyright.txt")


if __name__ == "__main__":
    prepare(Path(sys.argv[1]))
