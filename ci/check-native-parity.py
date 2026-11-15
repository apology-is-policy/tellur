"""Qualify the native compiler and its host link pack on Linux and Windows."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


CARGO = ["cargo", "test", "--locked", "--release"]
if os.name == "nt":
    # The compiler links MSVC LLVM; the programs it emits use the GNU pack.
    HOST_TARGET = "x86_64-pc-windows-msvc"
    CARGO += ["--target", HOST_TARGET, "--features", "windows-gnu-pack"]
else:
    HOST_TARGET = ""
    CARGO += ["--features", "linux-musl-packs"]


def run_integration(source, env):
    # Cargo supplies the paths to the CLI binaries when compiling each harness.
    # Run that same harness under both backends when it uses the suite selector.
    deps = Path(env["CARGO_TARGET_DIR"]) / HOST_TARGET / "release" / "deps"
    existing = set(deps.glob(f"{source.stem}-*"))
    build = subprocess.run(
        [*CARGO, "--no-run", "--message-format=json-render-diagnostics", "--test", source.stem],
        check=True, stdout=subprocess.PIPE, encoding="utf-8", env=env,
    )
    artifacts = (json.loads(line) for line in build.stdout.splitlines())
    executable, = (
        Path(record["executable"]) for record in artifacts
        if record.get("reason") == "compiler-artifact" and record.get("executable")
        and record["target"]["kind"] == ["test"] and record["target"]["name"] == source.stem
    )
    contents = source.read_text(encoding="utf-8")
    backends = ["rust", "llvm"] if "build_for_test(" in contents or "test_backend(" in contents else ["rust"]
    # Keep the generated program and Rust adapter cache across both passes of
    # this target. Its executables and archives are disposable after the target.
    failures = []
    with tempfile.TemporaryDirectory(prefix="tellur-native-") as cache:
        for backend in backends:
            print(f"::group::{source.stem} ({backend})", flush=True)
            try:
                result = subprocess.run(
                    [executable, "--test-threads=2"],
                    env=dict(env, TELLUR_TEST_BACKEND=backend, TELLUR_NATIVE_TARGET_DIR=cache),
                )
                if result.returncode:
                    failures.append(f"{source.stem} ({backend}): {result.returncode}")
            finally:
                print("::endgroup::", flush=True)
    # Each harness can contain LLVM and both runtime packs. Drop new test
    # executables after use while preserving any supplied build cache.
    if executable.parent == deps and executable not in existing:
        executable.unlink()
        pdb = executable.with_suffix(".pdb")
        if os.name == "nt" and pdb not in existing:
            pdb.unlink(missing_ok=True)
    return failures


def main():
    with tempfile.TemporaryDirectory(prefix="tellur-parity-") as directory:
        target = Path(os.environ.get("CARGO_TARGET_DIR", directory)).resolve()
        env = dict(os.environ, CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0", CARGO_TARGET_DIR=str(target))
        failures = []
        with tempfile.TemporaryDirectory(prefix="tellur-core-") as cache:
            for options in [["--lib", "--bins"], ["--doc"]]:
                result = subprocess.run(
                    [*CARGO, *options, "--", "--test-threads=2"],
                    env=dict(env, TELLUR_NATIVE_TARGET_DIR=cache),
                )
                if result.returncode:
                    failures.append(f"{' '.join(options)}: {result.returncode}")
        sources = sorted(Path("tests").glob("*.rs"))
        for source in sources:
            failures.extend(run_integration(source, env))
        if failures:
            raise SystemExit("Native parity failures:\n" + "\n".join(failures))
        print(f"Completed {len(sources)} integration targets, library, binary and documentation tests.")


if __name__ == "__main__":
    main()
