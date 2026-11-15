"""Prepare the Thylacine runtime archives and startup files for an LLVM-enabled Tellur build."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

TRIPLE = "aarch64-unknown-thylacine"
REPO = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-spec", type=Path, required=True)
    parser.add_argument("--rust-sysroot", type=Path, required=True, help="Patched target std for the active nightly")
    parser.add_argument("--pouch-sysroot", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True, help="New pack directory")
    args = parser.parse_args()
    spec = args.target_spec.resolve(strict=True)
    if spec.stem != TRIPLE:
        parser.error(f"expected {TRIPLE}.json")
    std = args.rust_sysroot.resolve(strict=True)
    lib = args.pouch_sysroot.resolve(strict=True) / "lib"
    out = args.out.resolve()
    if out.exists():
        parser.error(f"output already exists: {out}")
    out.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_INCREMENTAL="0", CARGO_ENCODED_RUSTFLAGS="--sysroot\x1f" + str(std))
    env.pop("RUSTFLAGS", None)
    with tempfile.TemporaryDirectory(dir=out.parent, prefix=".tellur-pouch-") as staging:
        pack = Path(staging) / "pack"
        pack.mkdir()
        for name in ["crt1.o", "crti.o", "crtn.o", "libunwind.a", "libc.a", "libclang_rt.builtins.a", "libdl.a", "libpthread.a"]:
            shutil.copyfile(lib / name, pack / name)
        subprocess.run(["rustc", "-Zunstable-options", "--edition=2024", "--crate-name", "tellur_pouch_start",
                        "--emit=obj", "-Copt-level=2", "--target", spec, "--sysroot", std,
                        "runtime/linux_entry.rs", "-o", pack / "entry.o"], check=True, cwd=REPO, env=env)
        target = Path(env.get("CARGO_TARGET_DIR", Path(staging) / "target")).resolve()
        for profile in ["plain", "polled"]:
            artifacts = Path(staging) / profile
            cmd = ["cargo", "build", "-Zjson-target-spec", "-Zunstable-options", "--artifact-dir", artifacts,
                   "--locked", "--release", "--manifest-path",
                   "runtime/Cargo.toml", "--target", spec, "--target-dir", target]
            if profile == "polled":
                cmd.extend(["--features", "polled"])
            subprocess.run(cmd, cwd=REPO, env=env, check=True)
            shutil.copyfile(artifacts / "libtellur_runtime_abi.a", pack / f"runtime-{profile}.a")
        pack.rename(out)


if __name__ == "__main__":
    main()
