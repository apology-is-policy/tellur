"""Compare exclusively owned sequences from this directory or the repository root."""

import csv
import itertools
import random
from pathlib import Path
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
results = Path(sys.argv[1]).resolve()
results.mkdir(parents=True, exist_ok=True)
cases = [
    (kind, operation, count, size)
    for count, size in itertools.product((128, 4096, 65536), (8, 64))
    for operation in ("append", "scan", "index", "split_join", "join_batches", "front_drain")
    for kind in (
        ("flat", "relaxed", "cursor")
        if operation == "front_drain"
        else ("flat", "relaxed")
    )
]
with tempfile.TemporaryDirectory() as directory:
    temporary = Path(directory)
    builds = (
        ("timing", []),
        ("counts", ["--cfg", "allocation_counts", "--cfg", "movement"]),
        ("tests", ["--test"]),
    )
    for mode, flags in builds:
        subprocess.run(
            [
                "rustc", "--edition", "2024", "-C", "opt-level=3",
                here / "main.rs", *flags, "-o", temporary / mode,
            ],
            check=True,
        )
    subprocess.run([temporary / "tests"], check=True)
    randomizer = random.Random(292)
    for mode in ("timing", "counts"):
        with (results / (mode + ".csv")).open("w") as output:
            writer = csv.writer(output, lineterminator="\n")
            writer.writerow((
                "trial", "kind", "operation", "n", "item_bytes", "ns",
                "allocations", "peak_extra_bytes", "requested_bytes",
                "element_transfers", "growth_exposure",
            ))
            for trial in range(14 if mode == "timing" else 1):
                randomizer.shuffle(cases)
                for kind, operation, count, size in cases:
                    result = subprocess.run(
                        [temporary / mode, kind, operation, str(count), str(size)],
                        check=True, capture_output=True, text=True,
                    )
                    row = next(csv.reader([result.stdout.strip()]))
                    assert row[:4] == [kind, operation, str(count), str(size)]
                    if trial or mode == "counts":
                        writer.writerow((trial, *row))
                output.flush()
                print(mode, trial, flush=True)
