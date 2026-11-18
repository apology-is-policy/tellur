"""Complete borrowed ordered queries versus Rust slice partition_point, on arm64 macOS."""
import argparse
import bisect
import csv
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "first_match"))
from abi_probe import build_allocation_probe, wrap_calls

parser = argparse.ArgumentParser(description=__doc__)
for name in ("vnc", "llvm", "results"):
    parser.add_argument(name, type=Path)
parser.add_argument("--trials", type=int, default=13)
args = parser.parse_args()
assert args.trials > 0
vnc, llvm, results = (x.resolve() for x in (args.vnc, args.llvm, args.results))
results.mkdir(parents=True, exist_ok=True)
OPERATIONS = ("lower_bound", "upper_bound", "first_equal")
cases = {}
for n in (0, 8, 128, 4096):
    for name, needle in (("early", 0), ("middle", n // 6),
                         ("last", max(0, (n - 1) // 3)), ("missing", n // 3 + 1)):
        cases[f"{n}-{name}"] = (n, needle)


def run(*cmd, **options):
    return subprocess.run(cmd, check=True, text=True, **options)


def rows(path, header, values):
    with path.open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(header)
        writer.writerows(values)


with tempfile.TemporaryDirectory(prefix="vn-bounds-") as directory:
    tmp = Path(directory)
    probes, buckets = {}, {}
    for operation in OPERATIONS:
        build = tmp / operation
        build.mkdir()
        run(vnc, "build", HERE / (operation + ".vn"), "--release", "--out", build / "source",
            env={**os.environ, "CARGO_INCREMENTAL": "0", "TELLUR_NATIVE_TARGET_DIR": str(build)})
        archive, = (build / "tellur-runtime-pack").glob("*.a")
        ir = (build / "source.ll").read_text()
        (results / (operation + ".ll")).write_text(ir)
        controls = ((operation + "<ascending>").encode(),)
        (build / "timed.ll").write_text(wrap_calls(ir, controls, "time_first"))
        run(llvm / "opt", "-passes=default<O3>", build / "timed.ll", "-o", build / "timed.bc")
        run(llvm / "llc", "-O3", "-filetype=obj", build / "timed.bc", "-o", build / "timed.o")
        run("cc", "-O3", "-mmacosx-version-min=13.0", build / "timed.o",
            HERE.parent / "first_match/timer.c", archive, "-o", build / "timed")
        probes[operation], buckets[operation] = build_allocation_probe(ir, controls, build, llvm, archive)
    for suffix, flags in (("", []), ("-count", ["--cfg", "allocation_counts"])):
        run("rustc", "--edition", "2024", "-C", "opt-level=3", *flags,
            HERE / "bounds.rs", "-o", tmp / ("rs" + suffix))
    (results / "environment.json").write_text(json.dumps({
        "compiler": run(vnc, "--version", capture_output=True).stdout.strip(),
        "rust": run("rustc", "-vV", capture_output=True).stdout,
        "llvm": run(llvm / "opt", "--version", capture_output=True).stdout,
        "hardware": "Apple M2 (maintained host evidence; sysctl unavailable in sandbox)",
        "optimization": "LLVM opt default<O3> and llc -O3; rustc opt-level=3",
        "method": f"{args.trials} retained shuffled trials after one discarded trial; single query per process, input/output excluded; short timings include clock/cold-call noise",
        "movement": "Zero source element movement or copies in both queries; generic reference/control metadata transport remains in Villanelle",
        "inputs": "Already sorted u32 vector with values i/3; source and needle constructed before timing",
    }, indent=2) + "\n")

    def invoke(kind, operation, case, allocations=False, peak=False):
        n, needle = cases[case]
        values = [i // 3 for i in range(n)]
        lower, upper = bisect.bisect_left(values, needle), bisect.bisect_right(values, needle)
        index = upper if operation == "upper_bound" else lower
        found = lower < n and values[lower] == needle
        if kind == "rs":
            row = list(map(int, run(tmp / ("rs-count" if allocations else "rs"),
                operation, str(n), str(needle), capture_output=True).stdout.strip().split(",")))
            assert row[4:] == [index, needle if operation == "first_equal" and found else -1], row
            return row
        argument = json.dumps({"kind": "construct", "value": {"name": "vector", "ty": "vector<U32>",
            "fields": [{"kind": "u32", "value": x} for x in values]}}, separators=(",", ":"))
        result = run(probes[operation] if allocations else tmp / operation / "timed", argument, str(needle),
            capture_output=True, env={**os.environ, **({"VN_ALLOC_PEAK": "1"} if peak else {})})
        label, payload = result.stdout.strip().split(" ", 1)
        assert label == ("found" if found else "missing") if operation == "first_equal" else label == "position"
        actual = [x["value"] for x in json.loads(payload)]
        assert actual == ([index, needle] if operation == "first_equal" and found else [index]), actual
        return result.stderr.splitlines()

    rng = random.Random(305)
    schedule = [(kind, operation, case) for kind in ("vn", "rs") for operation in OPERATIONS for case in cases]
    with (results / "timings.csv").open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(("trial", "kind", "operation", "case", "ns"))
        for trial in range(args.trials + 1):
            rng.shuffle(schedule)
            for kind, operation, case in schedule:
                row = invoke(kind, operation, case)
                if trial:
                    writer.writerow((trial, kind, operation, case, int(row[0])))
    totals, detail = [], []
    for kind, operation, case in schedule:
        row = invoke(kind, operation, case, True)
        if kind == "rs":
            totals.append((kind, operation, case, row[1], 0, row[3], row[2]))
            continue
        measured = invoke(kind, operation, case, True, True)
        assert [x for x in measured if not x.startswith("peak ")] == row
        assert row.count("query") == 1
        counts = [list(map(int, x.split())) for x in row if x != "query"]
        peak, = [int(x.split()[1]) for x in measured if x.startswith("peak ")]
        totals.append((kind, operation, case, *[sum(x[i] for x in counts) for i in (2, 3, 4)], peak))
        detail.extend((operation, case, buckets[operation][x[0]], *x[1:]) for x in counts)
    rows(results / "allocations.csv", ("kind", "operation", "case", "allocations", "reallocations", "requested_bytes", "peak_extra_bytes"), totals)
    rows(results / "operations.csv", ("operation", "case", "abi", "calls", "allocations", "reallocations", "requested_bytes"), detail)
