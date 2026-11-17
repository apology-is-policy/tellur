"""Measure owned-state fold execution and attribute allocation requests on Apple silicon."""
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
search = here.parent / "first_match"
sys.path.insert(0, str(search))
from abi_probe import build_allocation_probe, wrap_calls

parser = argparse.ArgumentParser(description=__doc__)
for name in ("vnc", "llvm_bin", "results"):
    parser.add_argument(name, type=Path)
parser.add_argument("--alloc-only", action="store_true", help="reuse timings and run allocation probes only")
args = parser.parse_args()
llvm = args.llvm_bin.resolve()
controls = (b"fold<batch_limit>",)
cases = {
    "empty": ([], 1, "batch exhausted", 0),
    "early": ([1] * 256, 1, "target reached", 1),
    "last": ([1] * 256, 256, "target reached", 256),
    "exhausted": ([1] * 256, 257, "batch exhausted", 256),
    "offline": ([1] * 127 + [0] + [1] * 128, 257, "scale offline", 127),
    "overflow": ([4294967294, 3], 4294967295, "weight overflow", 4294967294),
}


def run(*argv, **kwargs):
    return subprocess.run(argv, check=True, text=True, **kwargs)


def query(exe, case, rounds, rust=False, batch=1, env=None):
    weights, target, label, state = cases[case]
    if rust:
        values = ",".join(map(str, weights))
        extra = [str(batch)]
    else:
        fields = [{"kind": "u32", "value": value} for value in weights]
        values = json.dumps({"kind": "construct", "value": {
            "ty": "vector<U32>", "name": "vector", "fields": fields}})
        extra = []
    result = run(exe, values, str(target), str(rounds), *extra, capture_output=True, env=env)
    assert result.stdout.splitlines() == [label, str(state)] * rounds + ["done"]
    return result.stderr.splitlines()


with tempfile.TemporaryDirectory() as temp:
    work = Path(temp)
    # Extract this compiler's exact embedded archive into an empty temporary cache.
    env = {**os.environ, "TELLUR_NATIVE_TARGET_DIR": str(work), "CARGO_INCREMENTAL": "0"}
    run(args.vnc.resolve(), "build", here / "main.vn", here.parents[1] / "examples/batch_fold/manifest.json",
        "--release", "--backend", "llvm", "--out", work / "fold", env=env)
    archive, = (work / "tellur-runtime-pack").glob("*.a")
    ir = (work / "fold.ll").read_text()
    (work / "timed.ll").write_text(wrap_calls(ir, controls, "time_first"))
    run(llvm / "opt", "-passes=default<O3>", work / "timed.ll", "-o", work / "timed.bc")
    run(llvm / "llc", "-O3", "-filetype=obj", work / "timed.bc", "-o", work / "timed.o")
    run("cc", "-O3", "-mmacosx-version-min=13.0", work / "timed.o", search / "timer.c", archive,
        "-o", work / "vn")
    run("rustc", "-C", "opt-level=3", here / "fold.rs", "-o", work / "rs")

    if not args.alloc_only:
        rng = random.Random(291)
        schedule = [(case, impl) for case in cases for impl in ("vn", "rs")]
        rounds = 7
        rows = []
        for trial in range(13):
            rng.shuffle(schedule)
            for case, impl in schedule:
                times = query(work / impl, case, rounds, rust=impl == "rs", batch=1024)
                assert len(times) == rounds
                if trial:
                    for i, ns in enumerate(times, 1):
                        rows.append((case, impl, trial, i, int(ns) / (1024 if impl == "rs" else 1)))
        with args.results.open("w") as file:
            writer = csv.writer(file, lineterminator="\n")
            writer.writerow(("case", "runtime", "trial", "query", "ns"))
            writer.writerows(rows)
        for case in cases:
            for impl in ("vn", "rs"):
                warm = [ns for c, r, _, i, ns in rows if (c, r) == (case, impl) and i > 1]
                print(case, impl, statistics.median(warm), "ns")

    probe, names = build_allocation_probe(ir, controls, work, llvm, archive)
    run("rustc", "-C", "opt-level=3", "--cfg", "alloc_probe", here / "fold.rs",
        "-C", f"link-arg={work / 'alloc.dylib'}", "-o", work / "rs-probe")
    # Change only the unused suffix length to detect whole-input copying on the first visit.
    cases["early_one"] = ([1], 1, "target reached", 1)
    with args.results.with_suffix(".alloc.csv").open("w") as file:
        writer = csv.writer(file, lineterminator="\n")
        writer.writerow(("case", "runtime", "query", "operation", "calls", "allocations", "reallocations", "bytes"))
        for case in cases:
            for name, exe in (("vn", probe), ("rs", work / "rs-probe")):
                n = 0
                for line in query(exe, case, 2, rust=name == "rs"):
                    if line == "query":
                        n += 1
                    else:
                        bucket, *counts = map(int, line.split())
                        writer.writerow((case, name, n, names[bucket] if name == "vn" else "fold", *counts))
                assert n == 2

    process_probe = work / "process.dylib"
    run("cc", "-O2", "-dynamiclib", here.parent / "ordered_maps/comparison/profile.c", "-o", process_probe)
    child_env = {**os.environ, "DYLD_INSERT_LIBRARIES": str(process_probe)}
    with args.results.with_suffix(".process.csv").open("w") as file:
        writer = csv.writer(file, lineterminator="\n")
        writer.writerow(("case", "runtime", "allocations", "reallocations", "frees", "bytes"))
        for case in cases:
            for name, exe in (("vn", work / "fold"), ("rs", work / "rs")):
                lines = query(exe, case, 1, rust=name == "rs", env=child_env)
                marker, *counts = lines[-1].split()
                assert marker == "heap" and len(counts) == 4
                writer.writerow((case, name, *map(int, counts)))
