"""Compare real runtime values, equal-width records and scalars; remove all builds."""
import csv
import json
import os
from pathlib import Path
import random
import re
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
results = Path(sys.argv[1]).resolve()
results.mkdir(parents=True, exist_ok=True)

def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)

with tempfile.TemporaryDirectory(prefix="vn-layout-") as directory:
    temporary = Path(directory)
    manifest = f'''[package]
name = "vn-layout-probe"
version = "0.0.0"
edition = "2024"
[workspace]
[[bin]]
name = "layout"
path = {json.dumps(str(here / "layout.rs"))}
[features]
default = ["runtime-abi"]
runtime-abi = []
[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
[profile.release]
opt-level = 2
debug-assertions = true
overflow-checks = true
[lints.rust]
unexpected_cfgs = {{ level = "allow", check-cfg = ['cfg(allocation_counts)', 'cfg(target_os, values("thylacine"))'] }}
'''
    (temporary / "Cargo.toml").write_text(manifest)
    # Pin dependencies to the runtime's existing offline lockfile.
    (temporary / "Cargo.lock").write_text((here.parents[1] / "runtime/Cargo.lock").read_text())
    env = {**os.environ, "CARGO_INCREMENTAL": "0", "CARGO_NET_OFFLINE": "true",
           "CARGO_TARGET_DIR": str(temporary / "target")}
    for name, level, counting in (("o2", 2, False), ("o3", 3, False), ("counts", 2, True)):
        flags = "--cfg allocation_counts" if counting else ""
        run("cargo", "rustc", "--offline", "--release", "--manifest-path", temporary / "Cargo.toml",
            "--", "--emit=asm,link",
            env={**env, "RUSTFLAGS": flags, "CARGO_PROFILE_RELEASE_OPT_LEVEL": str(level)},
            stdout=subprocess.DEVNULL)
        if not counting:
            assembly = max((temporary / "target/release/deps").glob("layout-*.s"), key=lambda p: p.stat().st_mtime).read_text()
            functions = re.findall(r"^_[^\n:]*6rotate[^\n:]*:\n.*?\.cfi_endproc", assembly, re.M | re.S)
            assert functions, "retain payload kernel assembly (including Rust v0 mangling)"
            (results / f"layout-{name}.s").write_text("\n\n".join(functions) + "\n")
        (temporary / "target/release/layout").rename(temporary / name)
    metadata = {"rustc": run("rustc", "-vV", capture_output=True).stdout,
                "machine": run("uname", "-m", capture_output=True).stdout.strip(),
                "note": "O2 matches runtime opt level and checks; O3 differs only in opt level. Builds removed."}
    (results / "layout-environment.json").write_text(json.dumps(metadata, indent=2) + "\n")
    schedule = [(opt, item, kernel, n, repeats)
                for opt in ("o2", "o3") for item in ("u32", "wide", "value")
                for kernel in ("prefix", "bulk_prefix", "suffix", "in_place")
                for n in (128, 4096) for repeats in (1, 256)]
    randomizer = random.Random(293)
    with (results / "layout.csv").open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(("trial", "opt", "item", "kernel", "n", "bytes", "total_ns", "repeats"))
        for trial in range(14):
            randomizer.shuffle(schedule)
            for opt, item, kernel, n, repeats in schedule:
                row = run(temporary / opt, item, kernel, str(n), str(repeats), capture_output=True)
                width, ns, count, *_ = map(int, row.stdout.split(','))
                if trial:
                    writer.writerow((trial, opt, item, kernel, n, width, ns, count))
    with (results / "layout-alloc.csv").open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(("item", "kernel", "n", "bytes", "allocations", "peak_extra", "requested_bytes"))
        for item in ("u32", "wide", "value"):
            for kernel in ("prefix", "bulk_prefix", "suffix", "in_place"):
                for n in (128, 4096):
                    row = run(temporary / "counts", item, kernel, str(n), "1", capture_output=True)
                    width, _, _, calls, peak, requested = map(int, row.stdout.split(','))
                    writer.writerow((item, kernel, n, width, calls, peak, requested))
