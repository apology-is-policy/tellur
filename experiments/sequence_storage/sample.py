"""Sample only split/join call stacks, excluding the caller's checked loop."""
import collections
import json
import os
from pathlib import Path
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

with tempfile.TemporaryDirectory(prefix="vn-batch-sample-") as directory:
    temporary = Path(directory)
    manifest = json.loads((here.parents[1] / "examples/batch_fold/manifest.json").read_text())
    manifest["primitives"] = [p for p in manifest["primitives"] if p["name"] == "checked_add"]
    manifest["launch"] = {"main": {"exit_codes": {"ready": 0, "invalid": 1}}}
    (temporary / "manifest.json").write_text(json.dumps(manifest))
    run(vnc, "build", here / "repeat.vn", temporary / "manifest.json", "--release", "--out", temporary / "repeat",
        env={**os.environ, "CARGO_INCREMENTAL": "0", "TELLUR_NATIVE_TARGET_DIR": str(temporary)})
    archive, = (temporary / "tellur-runtime-pack").glob("*.a")
    ir = wrap_calls((temporary / "repeat.ll").read_text(), (b"split_at<u32>", b"join<u32>"), "profile_batch")
    (temporary / "sample.ll").write_text(ir)
    (temporary / "wrapper.c").write_text('''
#include <stdint.h>
#include <stddef.h>
extern uint64_t tellur_rt_v1_call(void *, void *, const char *, size_t, const uint64_t *, size_t);
__attribute__((noinline)) uint64_t profile_batch(void *ctx, void *fn, const char *name,
        size_t length, const uint64_t *args, size_t count) {
    uint64_t result = tellur_rt_v1_call(ctx, fn, name, length, args, count);
    __asm__ volatile("" : "+r"(result)); // Preserve this scope in sampled call stacks.
    return result;
}
''')
    sampler = (here.parent / "ordered_maps/comparison/profile.c").read_text().replace("#define DEPTH 24", "#define DEPTH 64")
    (temporary / "profile.c").write_text(sampler)
    run(llvm / "opt", "-passes=default<O3>", temporary / "sample.ll", "-o", temporary / "sample.bc")
    run(llvm / "llc", "-O3", "-filetype=obj", temporary / "sample.bc", "-o", temporary / "sample.o")
    run("cc", "-O3", "-fno-omit-frame-pointer", temporary / "sample.o", temporary / "wrapper.c", archive, "-o", temporary / "sample")
    run("cc", "-O2", "-dynamiclib", "-DVN_CPU_ONLY", temporary / "profile.c", "-o", temporary / "cpu.dylib")
    summary = {}
    for n, rounds in ((1, 150000), (128, 150000), (4096, 50000)):
        at = n // 3
        values = json.dumps({"kind": "construct", "value": {"name": "vector", "ty": "vector<U32>",
            "fields": [{"kind": "u32", "value": i} for i in range(n)]}})
        result = run(temporary / "sample", values, str(at), str(rounds), capture_output=True,
            env={**os.environ, "VN_SAMPLE": "1", "DYLD_INSERT_LIBRARIES": str(temporary / "cpu.dylib")})
        label, payload = result.stdout.strip().split(' ', 1)
        assert label == "ready"
        shift = at * rounds % n
        assert [v["value"] for v in json.loads(payload)[0]["value"]["fields"]] == list(range(shift, n)) + list(range(shift))
        (results / f"stacks-{n}.txt").write_text(result.stderr)
        counts, leaves = collections.Counter(), collections.Counter()
        for line in result.stderr.splitlines():
            if ":profile_batch " in line:
                frames = line.split(":profile_batch ", 1)[0].split()
                inner = next((f.split(":")[-1] for f in frames if ":tellur_rt_v1_" in f), "generated_or_other")
                counts[inner] += 1
                leaves[frames[0].split(":")[-1]] += 1
        assert counts, "samples must reach the selected batch call scope"
        summary[n] = {"rounds": rounds, "in_batches": counts.total(),
            "innermost_abi": dict(counts.most_common()), "leaves": dict(leaves.most_common(25))}
    (results / "cpu.json").write_text(json.dumps(summary, indent=2) + "\n")
