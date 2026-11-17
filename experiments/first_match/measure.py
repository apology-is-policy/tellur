"""Profile four search outcomes in a borrowed vector on arm64 macOS."""
import argparse
import csv
import os
from pathlib import Path
import random
import statistics
import subprocess
import tempfile

parser = argparse.ArgumentParser()
for argument in ("vnc", "llvm_bin", "archive", "results"):
    parser.add_argument(argument, type=Path)
parser.add_argument("--allocator", type=Path, help="macOS allocation probe dylib")
parser.add_argument("--compare-archive", type=Path, help="previous runtime archive (uses current IR unless --compare-ir is given)")
parser.add_argument("--compare-ir", type=Path, help="release IR for the same fixture from the previous compiler")
args = parser.parse_args()
if args.compare_ir and not args.compare_archive:
    parser.error("--compare-ir requires --compare-archive")
here = Path(__file__).resolve().parent
manifest = here.parents[1] / "examples/first_match/manifest.json"
rng = random.Random(288)
rows = []


def timed_object(source, work, stem):
    lines = source.read_text().splitlines()
    # The outer call names its concrete predicate. Leave all inner calls intact.
    for control in (b"first<minimum>", b"try_first<minimum>"):
        name = "".join(f"\\{byte:02X}" for byte in control)
        symbol, = [line.split()[0] for line in lines if f'c"{name}"' in line]
        index, = [i for i, line in enumerate(lines)
                  if "call i64 @tellur_rt_v1_call(" in line and f"ptr {symbol}," in line]
        lines[index] = lines[index].replace("@tellur_rt_v1_call", "@time_first")
    lines.append("declare i64 @time_first(ptr, ptr, ptr, i64, ptr, i64)")
    (work / f"{stem}.ll").write_text("\n".join(lines) + "\n")
    subprocess.run([args.llvm_bin / "opt", "-passes=default<O3>", work / f"{stem}.ll",
                    "-o", work / f"{stem}.bc"], check=True)
    subprocess.run([args.llvm_bin / "llc", "-O3", "-filetype=obj", work / f"{stem}.bc",
                    "-o", work / f"{stem}.o"], check=True)
    return work / f"{stem}.o"


with tempfile.TemporaryDirectory() as temp:
    work = Path(temp)
    subprocess.run([args.vnc.resolve(), "build", here / "main.vn", manifest,
                    "--release", "--out", work / "search"], check=True)
    archives = {"vn": args.archive}
    if args.compare_archive:
        archives["vn_previous"] = args.compare_archive
    current_object = timed_object(work / "search.ll", work, "timed")
    for runtime, archive in archives.items():
        obj = current_object
        if runtime == "vn_previous" and args.compare_ir:
            obj = timed_object(args.compare_ir, work, "previous")
        subprocess.run(["cc", "-O3", "-mmacosx-version-min=13.0", obj,
                        here / "timer.c", archive, "-o", work / runtime], check=True)
    subprocess.run(["rustc", "-C", "opt-level=3", here / "search.rs",
                    "-o", work / "rs"], check=True)
    cases = {"early": ("0", "false", "0"), "last": ("255", "false", "255"),
             "miss": ("256", "false", "missing"), "failure": ("255", "true", "invalid")}
    schedule = [(case, runtime) for case in cases for runtime in ("rs", "rs_batched", *archives)]
    for trial in range(26):
        rng.shuffle(schedule)
        for case, runtime in schedule:
            limit, fallible, expected = cases[case]
            rounds = 9
            command = ([work / "rs", limit, fallible, str(rounds), "1024"] if runtime == "rs_batched"
                       else [work / runtime, limit, fallible, str(rounds)])
            run = subprocess.run(command, capture_output=True, check=True)
            assert run.stdout.decode().splitlines() == [expected] * rounds + ["done"]
            times = list(map(int, run.stderr.splitlines()))
            if runtime == "rs_batched":
                times = [t / 1024 for t in times]
            assert len(times) == rounds
            if trial:
                rows.extend((case, runtime, trial, i + 1, ns) for i, ns in enumerate(times))
    if args.allocator:
        env = {**os.environ, "DYLD_INSERT_LIBRARIES": str(args.allocator.resolve())}
        with args.results.with_suffix(".alloc.csv").open("w") as output:
            writer = csv.writer(output, lineterminator="\n")
            writer.writerow(["case", "runtime", "allocs", "reallocs", "frees", "requested_bytes"])
            for case, runtime in sorted(schedule):
                if runtime == "rs_batched":
                    continue
                limit, fallible, expected = cases[case]
                run = subprocess.run([work / runtime, limit, fallible, "1"], env=env,
                                     capture_output=True, check=True, text=True)
                assert run.stdout == f"{expected}\ndone\n"
                marker, *counts = run.stderr.splitlines()[-1].split()
                assert marker == "heap" and len(counts) == 4
                writer.writerow([case, runtime, *map(int, counts)])
with args.results.open("w") as output:
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(["case", "runtime", "trial", "query", "ns"])
    writer.writerows(rows)
for case, runtime in sorted(schedule):
    times = [(i, ns) for c, r, _, i, ns in rows if (c, r) == (case, runtime)]
    cold = statistics.median(ns for i, ns in times if i == 1)
    warm = statistics.median(ns for i, ns in times if i > 1)
    print(case, runtime, "first", cold, "ns; repeated", warm, "ns")
