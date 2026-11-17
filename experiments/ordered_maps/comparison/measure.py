"""Compare AOT Villanelle and Rust BTreeMap on the same decoded outcome and trace."""
import argparse
import csv
import json
import random
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path
from time import perf_counter_ns

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tellur_baseline import scenario

parser = argparse.ArgumentParser()
parser.add_argument("--drain-only", action="store_true")
parser.add_argument("executables", type=Path, help="output directory from build_o3")
parser.add_argument("rust_map", type=Path)
parser.add_argument("results", type=Path, help="destination CSV")
args = parser.parse_args()
rng = random.Random(287)
rows = []
with tempfile.TemporaryDirectory() as temp:
    for size in [8, 4096, 16384]:
        keys = {f"settings/tenant/{i % 32:02}/property/{i:05}": i for i in range(size)}
        events = [
            {"kind": "emit", "message": message}
            for key in sorted(keys) for message in (key, str(keys[key]))
        ]
        cases = []
        entries = ["accept", "scan", "observe_scan"]
        if args.drain_only:
            entries = ["accept", "drain", "observe_drain"]
        else:
            entries.extend(["drain", "observe_drain"])
        for entry in entries:
            path = Path(temp) / f"{entry}.json"
            path.write_text(json.dumps(scenario(entry, keys)))
            cases.append(("Rust", entry, [args.rust_map.resolve(), path]))
            if entry in ("observe_scan", "observe_drain"):
                cases.append(("Rust-pooled", entry, [args.rust_map.resolve(), path, "--pooled"]))
            if entry == "observe_drain":
                cases.append(("Rust-into-iter-pooled", entry, [args.rust_map.resolve(), path, "--into-iter"]))
            for suffix in ["", "-expanded"]:
                binary = args.executables.resolve() / (entry + suffix)
                cases.append(("VN-O3" + suffix, entry, [binary, "--scenario", path, "--fuel", "10000000", "--json"]))
        for trial in range(14):
            rng.shuffle(cases)
            for backend, entry, command in cases:
                start = perf_counter_ns()
                data = subprocess.check_output(command)
                elapsed = perf_counter_ns() - start
                expected = {"events": events if entry in ("observe_scan", "observe_drain") else [], "fields": [], "outcome": "done"}
                assert json.loads(data) == expected, (size, backend, entry)
                if trial:
                    rows.append((size, trial, backend, entry, elapsed))
with args.results.open("w") as output:
    writer = csv.writer(output)
    writer.writerow(["size", "trial", "backend", "entry", "ns"])
    writer.writerows(rows)
for size, backend, entry in sorted({(s, b, e) for s, _, b, e, _ in rows}):
    times = [ns / 1e6 for s, _, b, e, ns in rows if (s, b, e) == (size, backend, entry)]
    print(size, backend, entry, round(statistics.median(times), 3))
