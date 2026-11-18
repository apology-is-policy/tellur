"""Compare one checked Villanelle rotation with the owned Rust storage operations."""

import csv
import argparse
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
sys.path.insert(0, str(here.parent / "first_match"))
from abi_probe import build_allocation_probe, wrap_calls

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("vnc", type=Path)
parser.add_argument("llvm", type=Path)
parser.add_argument("results", type=Path)
parser.add_argument("previous", type=Path, nargs="?")
parser.add_argument("--previous-ir", action="store_true",
                    help="compare compiler emission as well as runtime, using each compiler's own IR")
arguments = parser.parse_args()
vnc, llvm, results = (path.resolve() for path in (arguments.vnc, arguments.llvm, arguments.results))
previous = arguments.previous.resolve() if arguments.previous else None
if arguments.previous_ir and previous is None:
    parser.error("--previous-ir requires a previous compiler")
results.mkdir(exist_ok=True, parents=True)


def run(*arguments, **options):
    return subprocess.run(arguments, check=True, text=True, **options)


with tempfile.TemporaryDirectory() as directory:
    temporary = Path(directory)
    run(
        vnc, "build", here.parents[1] / "examples/rotate_batch/main.vn",
        "--release", "--out", temporary / "source",
        env={
            **os.environ,
            "TELLUR_NATIVE_TARGET_DIR": str(temporary),
            "CARGO_INCREMENTAL": "0",
        },
    )
    archive, = (temporary / "tellur-runtime-pack").glob("*.a")
    ir = (temporary / "source.ll").read_text()
    controls = (b"split_at<u32>", b"join<u32>")
    (temporary / "timed.ll").write_text(wrap_calls(ir, controls, "time_first"))
    run(llvm / "opt", "-passes=default<O3>", temporary / "timed.ll", "-o", temporary / "timed.bc")
    run(llvm / "llc", "-O3", "-filetype=obj", temporary / "timed.bc", "-o", temporary / "timed.o")
    run(
        "cc", "-O3", "-mmacosx-version-min=13.0", temporary / "timed.o",
        here.parent / "first_match/timer.c", archive, "-o", temporary / "vn",
    )
    archives = {"vn": archive}
    if previous:
        previous_dir = temporary / "previous"
        previous_dir.mkdir()
        run(previous, "build", here.parents[1] / "examples/rotate_batch/main.vn",
            "--release", "--out", previous_dir / "source",
            env={**os.environ, "TELLUR_NATIVE_TARGET_DIR": str(previous_dir), "CARGO_INCREMENTAL": "0"})
        old_ir = (previous_dir / "source.ll").read_text()
        previous_object = temporary / "timed.o"
        if arguments.previous_ir:
            (previous_dir / "timed.ll").write_text(wrap_calls(old_ir, controls, "time_first"))
            run(llvm / "opt", "-passes=default<O3>", previous_dir / "timed.ll", "-o", previous_dir / "timed.bc")
            previous_object = previous_dir / "timed.o"
            run(llvm / "llc", "-O3", "-filetype=obj", previous_dir / "timed.bc", "-o", previous_object)
        else:
            assert ir == old_ir, "paired runtimes require identical generated IR; use --previous-ir for compiler comparisons"
        old_archive, = (previous_dir / "tellur-runtime-pack").glob("*.a")
        archives["vn-previous"] = old_archive
        run("cc", "-O3", "-mmacosx-version-min=13.0", previous_object,
            here.parent / "first_match/timer.c", old_archive, "-o", temporary / "vn-previous")
    (results / "native-environment.json").write_text(json.dumps({
        "compiler": run(vnc, "--version", capture_output=True).stdout.strip(),
        "previous_compiler": run(previous, "--version", capture_output=True).stdout.strip() if previous else None,
        "comparison": "each compiler's own IR" if arguments.previous_ir else "same IR and object" if previous else "single compiler",
        "rustc": run("rustc", "-vV", capture_output=True).stdout,
    }, indent=2) + "\n")
    run("rustc", "--edition", "2024", "-C", "opt-level=3", here / "rotate.rs", "-o", temporary / "rs")
    run(
        "rustc", "--edition", "2024", "-C", "opt-level=3", "--cfg", "allocation_counts",
        here / "rotate.rs", "-o", temporary / "rs-count",
    )
    probes = {}
    for name, runtime in archives.items():
        directory = temporary / (name + "-alloc")
        directory.mkdir()
        measured_ir = old_ir if name == "vn-previous" and arguments.previous_ir else ir
        probes[name], _ = build_allocation_probe(measured_ir, controls, directory, llvm, runtime)

    def invoke(kind, count, allocations=False):
        if kind not in archives:
            binary = temporary / ("rs-count" if allocations else "rs")
            result = run(binary, kind, str(count), capture_output=True)
            return list(map(int, result.stdout.split(',')))
        fields = [{"kind": "u32", "value": index} for index in range(count)]
        argument = json.dumps({
            "kind": "construct",
            "value": {"name": "vector", "ty": "vector<U32>", "fields": fields},
        })
        result = run(
            probes[kind] if allocations else temporary / kind,
            argument, str(count // 3), capture_output=True,
        )
        label, values = result.stdout.strip().split(' ', 1)
        assert label == "ready"
        fields = json.loads(values)[0]["value"]["fields"]
        expected = list(range(count // 3, count)) + list(range(count // 3))
        assert [field["value"] for field in fields] == expected
        return result.stderr.splitlines()

    randomizer = random.Random(292)
    schedule = [
        (kind, count)
        for kind in (*archives, "flat", "relaxed")
        for count in (128, 4096)
    ]
    with (results / "native.csv").open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(("trial", "kind", "n", "ns"))
        for trial in range(14):
            randomizer.shuffle(schedule)
            for kind, count in schedule:
                values = invoke(kind, count)
                elapsed = sum(map(int, values)) if kind in archives else values[0]
                if trial:
                    writer.writerow((trial, kind, count, elapsed))
    with (results / "native-alloc.csv").open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(("kind", "n", "allocations", "reallocations", "requested_bytes"))
        for kind, count in schedule:
            rows = invoke(kind, count, True)
            if kind in archives:
                data = [list(map(int, row.split())) for row in rows if row != "query"]
                assert rows.count("query") == 2
                statistics = [sum(row[index] for row in data) for index in (2, 3, 4)]
            else:
                statistics = [rows[1], "", rows[3]]
            writer.writerow((kind, count, *statistics))
