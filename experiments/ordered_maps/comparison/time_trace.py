"""Time the compiled traversal and trace construction on arm64 macOS."""
import argparse
import csv
import json
import os
from pathlib import Path
import random
import statistics
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
sys.path.insert(0, str(here.parent))
from tellur_baseline import scenario

parser = argparse.ArgumentParser()
for argument in ("program", "rust_map", "archive", "llvm_bin", "results"):
    parser.add_argument(argument, type=Path)
args = parser.parse_args()
rows = []
rng = random.Random(287)
with tempfile.TemporaryDirectory() as temp:
    scratch = Path(temp)
    native = args.program.resolve()
    build = native.with_name(native.name + ".tellur-build")
    original = build / "program.o"
    objcopy = args.llvm_bin / "llvm-objcopy"
    # Confirm the selected archive reproduces the original executable's machine code.
    relinked = scratch / "relinked"
    subprocess.run([args.llvm_bin / "ld64.lld", "-arch", "arm64", "-platform_version",
                    "macos", "13.0", "13.0", "-o", relinked, original, args.archive,
                    "-Z", build / "libSystem.tbd"], check=True)
    for binary, text in [(native, "original"), (relinked, "rebuilt")]:
        subprocess.run([objcopy, "--dump-section", f"__TEXT,__text={scratch/text}",
                        binary, scratch / "discard"], check=True)
    if (scratch / "original").read_bytes() != (scratch / "rebuilt").read_bytes():
        raise ValueError("Runtime archive differs from the optimized executable")
    timed_object = scratch / "timed.o"
    subprocess.run([objcopy, "--redefine-sym", "_tellur_rt_v1_map_scan=_bench_map_scan",
                    original, timed_object], check=True)
    timed = scratch / "timed"
    subprocess.run(["cc", "-arch", "arm64", "-O3", "-mmacosx-version-min=13.0",
                    timed_object, here / "time_scan.c", args.archive, "-o", timed], check=True)
    for size in [4096, 16384]:
        keys = {f"settings/tenant/{i % 32:02}/property/{i:05}": i for i in range(size)}
        path = scratch / "scenario.json"
        path.write_text(json.dumps(scenario("observe_scan", keys)))
        expected = {"outcome": "done", "fields": [], "events": [
            {"kind": "emit", "message": message}
            for key in sorted(keys) for message in (key, str(keys[key]))
        ]}
        cases = [("Rust", [args.rust_map.resolve(), path]),
                 ("Rust-pooled", [args.rust_map.resolve(), path, "--pooled"]),
                 ("Villanelle", [timed, "--scenario", path, "--json", "--fuel", "10000000"])]
        for trial in range(26):
            rng.shuffle(cases)
            for language, command in cases:
                run = subprocess.run(command, env={**os.environ, "VN_MAP_TIMER": "1"},
                                     capture_output=True, check=True)
                assert json.loads(run.stdout) == expected
                elapsed = int(run.stderr.strip())
                if trial:
                    rows.append((size, language, trial, elapsed))
with args.results.open("w") as output:
    writer = csv.writer(output)
    writer.writerow(["entries", "runtime", "trial", "ns"])
    writer.writerows(rows)
for size, language in sorted({(s, lang) for s, lang, _, _ in rows}):
    times = [elapsed / 1000 for s, lang, _, elapsed in rows if (s, lang) == (size, language)]
    print(size, language, round(statistics.median(times), 2), "us")
