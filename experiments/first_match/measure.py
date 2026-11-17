"""Profile the last match in a borrowed vector on arm64 macOS."""
import argparse
import csv
from pathlib import Path
import random
import statistics
import subprocess
import tempfile

parser = argparse.ArgumentParser()
for argument in ("vnc", "llvm_bin", "archive", "results"):
    parser.add_argument(argument, type=Path)
args = parser.parse_args()
here = Path(__file__).resolve().parent
manifest = here.parents[1] / "examples/first_match/manifest.json"
rng = random.Random(288)
rows = []
with tempfile.TemporaryDirectory() as temp:
    work = Path(temp)
    subprocess.run([args.vnc.resolve(), "build", here / "main.vn", manifest,
                    "--release", "--out", work / "search"], check=True)
    lines = (work / "search.ll").read_text().splitlines()
    # The outer call names its concrete predicate. Leave all inner calls intact.
    name = "".join(f"\\{byte:02X}" for byte in b"first<minimum>")
    symbol, = [line.split()[0] for line in lines if f'c"{name}"' in line]
    index, = [i for i, line in enumerate(lines)
              if "call i64 @tellur_rt_v1_call(" in line and f"ptr {symbol}," in line]
    lines[index] = lines[index].replace("@tellur_rt_v1_call", "@time_first")
    lines.append("declare i64 @time_first(ptr, ptr, ptr, i64, ptr, i64)")
    (work / "timed.ll").write_text("\n".join(lines) + "\n")
    subprocess.run([args.llvm_bin / "opt", "-passes=default<O3>", work / "timed.ll",
                    "-o", work / "timed.bc"], check=True)
    subprocess.run([args.llvm_bin / "llc", "-O3", "-filetype=obj", work / "timed.bc",
                    "-o", work / "timed.o"], check=True)
    subprocess.run(["cc", "-O3", "-mmacosx-version-min=13.0", work / "timed.o",
                    here / "timer.c", args.archive, "-o", work / "vn"], check=True)
    subprocess.run(["rustc", "-C", "opt-level=3", here / "search.rs",
                    "-o", work / "rs"], check=True)
    for trial in range(26):
        cases = ["rs", "vn"]
        rng.shuffle(cases)
        for case in cases:
            run = subprocess.run([work / case], capture_output=True, check=True)
            assert run.stdout == b"255\ndone\n"
            if trial:
                rows.append((case, trial, int(run.stderr)))
with args.results.open("w") as output:
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(["runtime", "trial", "ns"])
    writer.writerows(rows)
for case in ("vn", "rs"):
    print(case, statistics.median(t for c, _, t in rows if c == case), "ns")
