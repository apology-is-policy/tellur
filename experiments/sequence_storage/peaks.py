"""Measure extra live allocator-usable bytes inside each split/join control."""
import argparse
import csv
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
sys.path.insert(0, str(here.parent / "first_match"))
from abi_probe import build_allocation_probe

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("llvm", type=Path)
parser.add_argument("results", type=Path)
parser.add_argument("compilers", type=Path, nargs="+")
args = parser.parse_args()

def run(*arguments, **options):
    return subprocess.run(arguments, check=True, text=True, **options)

with args.results.open("w") as output:
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(("compiler", "n", "control", "peak_extra_usable_bytes", "allocations", "reallocations", "requested_bytes"))
    for compiler in args.compilers:
        compiler = compiler.resolve()
        version = run(compiler, "--version", capture_output=True).stdout.split()[1]
        with tempfile.TemporaryDirectory(prefix="vn-batch-peak-") as directory:
            temporary = Path(directory)
            run(compiler, "build", here.parents[1] / "examples/rotate_batch/main.vn", "--release", "--out", temporary / "rotate",
                env={**os.environ, "CARGO_INCREMENTAL": "0", "TELLUR_NATIVE_TARGET_DIR": str(temporary)})
            archive, = (temporary / "tellur-runtime-pack").glob("*.a")
            probe, _ = build_allocation_probe((temporary / "rotate.ll").read_text(),
                (b"split_at<u32>", b"join<u32>"), temporary, args.llvm.resolve(), archive)
            for n in (128, 4096):
                at = n // 3
                values = json.dumps({"kind": "construct", "value": {"name": "vector", "ty": "vector<U32>",
                    "fields": [{"kind": "u32", "value": i} for i in range(n)]}})
                plain = run(probe, values, str(at), capture_output=True)
                measured = run(probe, values, str(at), capture_output=True,
                    env={**os.environ, "VN_ALLOC_PEAK": "1"})
                assert measured.stdout == plain.stdout
                label, payload = measured.stdout.strip().split(' ', 1)
                assert label == "ready"
                assert [v["value"] for v in json.loads(payload)[0]["value"]["fields"]] == list(range(at, n)) + list(range(at))
                # Adding frees/usable-size accounting must not change request counts.
                lines = measured.stderr.splitlines()
                assert [line for line in lines if not line.startswith("peak ")] == plain.stderr.splitlines()
                groups = []
                for line in lines:
                    if line == "query":
                        groups.append({"counts": [0, 0, 0]})
                    elif line.startswith("peak "):
                        groups[-1]["peak"] = int(line.split()[1])
                    else:
                        _, _, allocations, reallocations, requested = map(int, line.split())
                        groups[-1]["counts"] = [a + b for a, b in zip(groups[-1]["counts"], (allocations, reallocations, requested))]
                assert len(groups) == 2
                for name, group in zip(("split_at", "join"), groups):
                    writer.writerow((version, n, name, group["peak"], *group["counts"]))
