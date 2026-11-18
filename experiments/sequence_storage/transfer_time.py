"""Attribute time in the two outer batch controls to their bulk transfer calls."""
import csv
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile

here = Path(__file__).resolve().parent
sys.path.insert(0, str(here.parent / "first_match"))
from abi_probe import wrap_calls

vnc, llvm, results = map(lambda x: Path(x).resolve(), sys.argv[1:])
results.mkdir(parents=True, exist_ok=True)

def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)

wrapper = r'''
#include <stdint.h>
#include <stdio.h>
#include <time.h>
extern uint64_t tellur_rt_v1_call(void *, void *, const char *, size_t, const uint64_t *, size_t);
extern void tellur_rt_v1_vector_transfer(void *, uint64_t, uint64_t, uint64_t);
static uint64_t transfer_ns, transfer_calls;
void timed_transfer(void *ctx, uint64_t src, uint64_t dst, uint64_t limit) {
    uint64_t start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    tellur_rt_v1_vector_transfer(ctx, src, dst, limit);
    transfer_ns += clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start;
    transfer_calls++;
}
uint64_t timed_outer(void *ctx, void *control, const char *name, size_t length,
                     const uint64_t *args, size_t count) {
    transfer_ns = transfer_calls = 0;
    uint64_t start = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    uint64_t result = tellur_rt_v1_call(ctx, control, name, length, args, count);
    uint64_t elapsed = clock_gettime_nsec_np(CLOCK_UPTIME_RAW) - start;
    fprintf(stderr, "%llu,%llu,%llu\n", (unsigned long long)elapsed,
            (unsigned long long)transfer_ns, (unsigned long long)transfer_calls);
    return result;
}
'''

with tempfile.TemporaryDirectory(prefix="vn-transfer-time-") as directory:
    temporary = Path(directory)
    run(vnc, "build", here.parents[1] / "examples/rotate_batch/main.vn", "--release",
        "--out", temporary / "rotate", env={**os.environ, "CARGO_INCREMENTAL": "0",
            "TELLUR_NATIVE_TARGET_DIR": str(temporary)})
    archive, = (temporary / "tellur-runtime-pack").glob("*.a")
    ir = wrap_calls((temporary / "rotate.ll").read_text(), (b"split_at<u32>", b"join<u32>"), "timed_outer")
    ir = ir.replace("call void @tellur_rt_v1_vector_transfer(", "call void @timed_transfer(")
    ir += "\ndeclare void @timed_transfer(ptr, i64, i64, i64)\n"
    (temporary / "timed.ll").write_text(ir)
    (temporary / "timer.c").write_text(wrapper)
    run(llvm / "opt", "-passes=default<O3>", temporary / "timed.ll", "-o", temporary / "timed.bc")
    run(llvm / "llc", "-O3", "-filetype=obj", temporary / "timed.bc", "-o", temporary / "timed.o")
    run("cc", "-O3", temporary / "timed.o", temporary / "timer.c", archive, "-o", temporary / "timed")
    schedule = [0, 1, 128, 4096]
    randomizer = random.Random(293)
    with (results / "transfer-time.csv").open("w") as output:
        writer = csv.writer(output, lineterminator="\n")
        writer.writerow(("trial", "n", "control", "total_ns", "transfer_ns", "transfer_calls"))
        for trial in range(14):
            randomizer.shuffle(schedule)
            for n in schedule:
                at = n // 3
                values = json.dumps({"kind": "construct", "value": {"name": "vector", "ty": "vector<U32>",
                    "fields": [{"kind": "u32", "value": i} for i in range(n)]}})
                result = run(temporary / "timed", values, str(at), capture_output=True)
                label, payload = result.stdout.strip().split(' ', 1)
                assert label == "ready"
                assert [v["value"] for v in json.loads(payload)[0]["value"]["fields"]] == list(range(at, n)) + list(range(at))
                rows = result.stderr.splitlines()
                assert len(rows) == 2
                for control, row in zip(("split_at", "join"), rows):
                    if trial:
                        writer.writerow((trial, n, control, *map(int, row.split(','))))
