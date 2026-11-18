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
mode = parser.add_mutually_exclusive_group()
mode.add_argument("--timing-only", action="store_true", help="collect timings separately from allocator probes")
mode.add_argument("--alloc-only", action="store_true", help="reuse timings and run allocation probes only")
baseline = parser.add_mutually_exclusive_group()
baseline.add_argument("--previous-archive", type=Path, help="baseline runtime for the same fold object")
baseline.add_argument("--previous-vnc", type=Path, help="baseline compiler using its own IR and runtime")
parser.add_argument("--borrow-only-archive", type=Path, help="intermediate runtime with declaration borrowing alone")
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
    archives = {"vn": archive}
    objects = {"vn": work / "timed.o"}
    sources = {"vn": ir}
    if args.previous_vnc:
        old = work / "previous-build"
        old.mkdir()
        old_env = {**env, "TELLUR_NATIVE_TARGET_DIR": str(old)}
        run(args.previous_vnc.resolve(), "build", here / "main.vn", here.parents[1] / "examples/batch_fold/manifest.json",
            "--release", "--backend", "llvm", "--out", old / "fold", env=old_env)
        archives["previous"], = (old / "tellur-runtime-pack").glob("*.a")
        sources["previous"] = (old / "fold.ll").read_text()
        (old / "timed.ll").write_text(wrap_calls(sources["previous"], controls, "time_first"))
        run(llvm / "opt", "-passes=default<O3>", old / "timed.ll", "-o", old / "timed.bc")
        run(llvm / "llc", "-O3", "-filetype=obj", old / "timed.bc", "-o", old / "timed.o")
        objects["previous"] = old / "timed.o"
    args.results.with_suffix(".environment.json").write_text(json.dumps({
        "compiler": run(args.vnc.resolve(), "--version", capture_output=True).stdout.strip(),
        "previous_compiler": run(args.previous_vnc.resolve(), "--version", capture_output=True).stdout.strip() if args.previous_vnc else None,
        "comparison": "each compiler's own IR and runtime" if args.previous_vnc else "same IR and object",
        "llvm": run(llvm / "opt", "--version", capture_output=True).stdout,
        "rust": run("rustc", "--version", capture_output=True).stdout.strip(),
        "optimization": "LLVM default O3 and llc O3; Rust opt-level=3"
    }, indent=2) + "\n")
    if args.previous_archive:
        archives["previous"] = args.previous_archive.resolve()
    if args.borrow_only_archive:
        archives["borrow_only"] = args.borrow_only_archive.resolve()
    for name, runtime in archives.items():
        run("cc", "-O3", "-mmacosx-version-min=13.0", objects.get(name, work / "timed.o"), search / "timer.c", runtime,
            "-o", work / name)
    run("rustc", "-C", "opt-level=3", here / "fold.rs", "-o", work / "rs")

    if not args.alloc_only:
        rng = random.Random(291)
        schedule = [(case, impl) for case in cases for impl in (*archives, "rs")]
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
            for impl in (*archives, "rs"):
                warm = [ns for c, r, _, i, ns in rows if (c, r) == (case, impl) and i > 1]
                print(case, impl, statistics.median(warm), "ns")

    if args.timing_only:
        sys.exit(0)

    probes = {}
    for name, runtime in archives.items():
        directory = work / (name + "-profile")
        directory.mkdir()
        probes[name] = build_allocation_probe(sources.get(name, ir), controls, directory, llvm, runtime)
    run("rustc", "-C", "opt-level=3", "--cfg", "alloc_probe", here / "fold.rs",
        "-C", f"link-arg={work / 'vn-profile/alloc.dylib'}", "-o", work / "rs-probe")
    probes["rs"] = (work / "rs-probe", ["fold"])
    # Change only the unused suffix length to detect whole-input copying on the first visit.
    cases["early_one"] = ([1], 1, "target reached", 1)
    with args.results.with_suffix(".alloc.csv").open("w") as file:
        writer = csv.writer(file, lineterminator="\n")
        writer.writerow(("case", "runtime", "query", "operation", "calls", "allocations", "reallocations", "bytes"))
        for case in cases:
            for name, (exe, names) in probes.items():
                n = 0
                for line in query(exe, case, 2, rust=name == "rs"):
                    if line == "query":
                        n += 1
                    else:
                        bucket, *counts = map(int, line.split())
                        writer.writerow((case, name, n, names[bucket], *counts))
                assert n == 2

    # Measure peaks separately; require the same request counts and checked output.
    with args.results.with_suffix(".peaks.csv").open("w") as file:
        writer = csv.writer(file, lineterminator="\n")
        writer.writerow(("case", "runtime", "query", "peak_extra_usable_bytes"))
        for case in cases:
            for name, (exe, _) in probes.items():
                if name == "rs":
                    continue  # Rust's standalone allocator reports zero requests above.
                plain = query(exe, case, 2)
                measured = query(exe, case, 2, env={**os.environ, "VN_ALLOC_PEAK": "1"})
                assert [line for line in measured if not line.startswith("peak ")] == plain
                peaks = [int(line.split()[1]) for line in measured if line.startswith("peak ")]
                assert len(peaks) == 2
                for i, peak in enumerate(peaks, 1):
                    writer.writerow((case, name, i, peak))

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
