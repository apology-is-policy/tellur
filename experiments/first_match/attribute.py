"""Count allocations within each search ABI call on single-threaded arm64 macOS."""
import argparse
import csv
from pathlib import Path
from abi_probe import build_allocation_probe
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
for arg in ("llvm_ir", "llvm_bin", "archive", "results"):
    parser.add_argument(arg, type=Path)
args = parser.parse_args()
with tempfile.TemporaryDirectory() as temp:
    exe, names = build_allocation_probe(args.llvm_ir.read_text(), (b"first<minimum>", b"try_first<minimum>"),
                                        Path(temp), args.llvm_bin, args.archive)
    with args.results.open("w") as out:
        writer = csv.writer(out, lineterminator="\n")
        writer.writerow(("case", "query", "operation", "calls", "allocations", "reallocations", "bytes"))
        for case, limit, flag, expected in (("early", "0", "false", "0"), ("last", "255", "false", "255"),
                                            ("miss", "256", "false", "missing"), ("failure", "255", "true", "invalid")):
            result = subprocess.run([exe, limit, flag, "2"], check=True, capture_output=True, text=True)
            assert result.stdout.splitlines() == [expected, expected, "done"]
            query = 0
            for line in result.stderr.splitlines():
                if line == "query":
                    query += 1
                else:
                    bucket, *counts = map(int, line.split())
                    writer.writerow([case, query, names[bucket], *counts])
            assert query == 2
