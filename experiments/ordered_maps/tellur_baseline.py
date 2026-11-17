"""Conformance and whole-process timings for the current Tellur implementation."""
import argparse
import json
from pathlib import Path
from statistics import median
import subprocess
import tempfile
from time import perf_counter_ns

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "baseline.tellur"


def scenario(entry, keys):
    fields = []
    for key, value in sorted(keys.items()):
        fields.extend([{"kind": "string", "value": key}, {"kind": "u32", "value": value}])
    payload = {"ty": "map<String,U32>", "name": "map", "fields": fields}
    return {"entry": entry, "arguments": [{"kind": "construct", "value": payload}], "calls": []}


def build(compiler, folder, entry, flags=()):
    binary = folder / (entry + ("-expanded" if flags else ""))
    subprocess.run([compiler, "build", SOURCE, "--entry", entry, "--release", *flags, "--out", binary],
                   check=True, stdout=subprocess.DEVNULL)
    return binary


def report(command):
    return json.loads(subprocess.check_output(command))


def run(compiler, folder):
    data = folder / "scenario.json"
    keys = {k: i for i, k in enumerate(["é", "e\u0301", "a\0b", "a", "🍏", "", "aa", "\0", "a\0", "éx", "\0\0"])}
    data.write_text(json.dumps(scenario("observe", keys)))
    expected = {"outcome": "done", "fields": [],
                "events": [{"kind": "emit", "message": key} for key in sorted(keys)]}
    for flags in [[], ["--expanded"]]:
        binary = build(compiler, folder, "observe", flags)
        assert report([compiler, "run", SOURCE, HERE / "manifest.json", data, "--json", *flags]) == expected
        assert report([binary, "--scenario", data, "--json"]) == expected

    binaries = {entry: build(compiler, folder, entry) for entry in ["accept", "drain"]}
    print("size,entry,median_process_ms")
    for size in [8, 64, 512, 4096]:
        keys = {f"settings/tenant/{i % 32:02}/property/{i:05}": i for i in range(size)}
        for entry, binary in binaries.items():
            data.write_text(json.dumps(scenario(entry, keys)))
            command = [binary, "--scenario", data, "--json", "--fuel", "1000000"]
            expected = {"outcome": "done", "fields": [], "events": []}
            assert report(command) == expected
            times = []
            for _ in range(9):
                start = perf_counter_ns()
                output = subprocess.check_output(command)
                times.append((perf_counter_ns() - start) / 1e6)
                assert json.loads(output) == expected
            print(f"{size},{entry},{median(times):.3f}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        run(args.compiler.resolve(), Path(tmp))
