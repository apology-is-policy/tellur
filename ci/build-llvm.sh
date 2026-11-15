#!/usr/bin/env bash
# Install only Tellur's static LLVM libraries, linker drivers and headers.
set -euo pipefail
prefix=${1:?expected LLVM installation directory}
version=22.1.8
digest=922f1817a0df7b1489272d18134ee0087a8b068828f87ac63b9861b1a9965888
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
cd "$scratch"
archive="llvm-project-$version.src.tar.xz"
curl --fail --location --retry 4 --output "$archive" \
    "https://github.com/llvm/llvm-project/releases/download/llvmorg-$version/$archive"
printf '%s  %s\n' "$digest" "$archive" | shasum -a 256 -c -
tar -xf "$archive"
cmake -G Ninja -S "llvm-project-$version.src/llvm" -B build \
    -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_BUILD_TYPE=Release \
    -DLLVM_ENABLE_PROJECTS=lld '-DLLVM_TARGETS_TO_BUILD=AArch64;X86' \
    -DLLVM_BUILD_LLVM_DYLIB=OFF -DBUILD_SHARED_LIBS=OFF \
    -DLLVM_ENABLE_LIBEDIT=OFF -DLLVM_ENABLE_LIBXML2=OFF \
    -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_TERMINFO=OFF \
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF \
    -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_DOCS=OFF \
    -DLLVM_PARALLEL_LINK_JOBS=1
cmake --build build --target install-llvm-headers install-llvm-libraries install-llvm-config \
    install-lld-headers install-lldMachO install-lldELF install-lldCOFF install-lldCommon
test "$("$prefix/bin/llvm-config" --version)" = "$version"
