"""Exercise the native Linux compiler before changing the default backend."""
import json
import os
from pathlib import Path
import subprocess
import tempfile


CARGO = ["cargo", "test", "--locked", "--release", "--features", "linux-musl-packs"]


def run_integration(source, env):
    # Cargo supplies the paths to the CLI binaries when compiling each harness.
    # Run that same harness under both backends when it uses the suite selector.
    deps = Path(env["CARGO_TARGET_DIR"]) / "release" / "deps"
    existing = set(deps.glob(f"{source.stem}-*"))
    build = subprocess.run(
        [*CARGO, "--no-run", "--message-format=json-render-diagnostics", "--test", source.stem],
        check=True, stdout=subprocess.PIPE, text=True, env=env,
    )
    artifacts = (json.loads(line) for line in build.stdout.splitlines())
    executable, = (
        Path(record["executable"]) for record in artifacts
        if record.get("reason") == "compiler-artifact" and record.get("executable")
        and record["target"]["kind"] == ["test"] and record["target"]["name"] == source.stem
    )
    contents = source.read_text()
    backends = ["rust", "llvm"] if "build_for_test(" in contents or "test_backend(" in contents else ["rust"]
    # Keep the generated program and Rust adapter cache across both passes of
    # this target. Its executables and archives are disposable after the target.
    with tempfile.TemporaryDirectory(prefix="tellur-native-") as cache:
        for backend in backends:
            print(f"::group::{source.stem} ({backend})", flush=True)
            try:
                subprocess.run([executable, "--test-threads=2"], check=True,
                               env=dict(env, TELLUR_TEST_BACKEND=backend, TELLUR_NATIVE_TARGET_DIR=cache))
            finally:
                print("::endgroup::", flush=True)
    # Each harness can contain LLVM and both runtime packs. Drop new test
    # executables after use while preserving any supplied build cache.
    if executable.parent == deps and executable not in existing:
        executable.unlink()


def main():
    with tempfile.TemporaryDirectory(prefix="tellur-parity-") as directory:
        target = Path(os.environ.get("CARGO_TARGET_DIR", directory)).resolve()
        env = dict(os.environ, CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0", CARGO_TARGET_DIR=str(target))
        with tempfile.TemporaryDirectory(prefix="tellur-core-") as cache:
            for options in [["--lib", "--bins"], ["--doc"]]:
                subprocess.run([*CARGO, *options, "--", "--test-threads=2"], check=True,
                               env=dict(env, TELLUR_NATIVE_TARGET_DIR=cache))
        sources = sorted(Path("tests").glob("*.rs"))
        for source in sources:
            run_integration(source, env)
        print(f"Completed {len(sources)} integration targets, library, binary and documentation tests.")


if __name__ == "__main__":
    main()
