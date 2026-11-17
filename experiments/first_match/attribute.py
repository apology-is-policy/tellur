"""Count allocations within each search ABI call on single-threaded arm64 macOS."""
import argparse
import csv
from pathlib import Path
import re
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
for arg in ("llvm_ir", "llvm_bin", "archive", "results"):
    parser.add_argument(arg, type=Path)
args = parser.parse_args()
here = Path(__file__).resolve().parent
ir = args.llvm_ir.read_text()
prefix = "tellur_rt_v1_"
outer = "i64 @profile_first(ptr, ptr, ptr, i64, ptr, i64)"
for control in (b"first<minimum>", b"try_first<minimum>"):
    escaped = "".join(f"\\{byte:02X}" for byte in control)
    symbol, = [line.split()[0] for line in ir.splitlines() if f'c"{escaped}"' in line]
    call, = [line for line in ir.splitlines()
             if f"call i64 @{prefix}call(" in line and f"ptr {symbol}," in line]
    ir = ir.replace(call, call.replace(f"@{prefix}call(", "@profile_first("))
ir += f"\ndeclare {outer}\n"
ctype = {"void": "void", "ptr": "void *", "i64": "uint64_t", "i32": "uint32_t", "i8": "uint8_t"}
wrappers = ['#include <stdint.h>', '#include "probe.h"']
names = ["outer_call"]
for ret, name, params in re.findall(r"declare (\w+) @tellur_rt_v1_(\w+)\(([^)]*)\)", ir):
    call = f"call {ret} @{prefix}{name}("
    if call not in ir:
        continue
    ir = ir.replace(call, f"call {ret} @profile_{name}(")
    ir += f"declare {ret} @profile_{name}({params})\n"
    types = params.split(", ")
    signature = ", ".join(f"{ctype[t]} a{i}" for i, t in enumerate(types))
    argv = ", ".join(f"a{i}" for i in range(len(types)))
    wrappers.extend([f"extern {ctype[ret]} {prefix}{name}({signature});",
                     f"{ctype[ret]} profile_{name}({signature}) {{",
                     f"int previous = probe_enter({len(names)});"])
    assign = "" if ret == "void" else f"{ctype[ret]} result = "
    wrappers.append(f"{assign}{prefix}{name}({argv});")
    wrappers.append("probe_leave(previous);")
    if ret != "void":
        wrappers.append("return result;")
    wrappers.append("}")
    names.append(name)
wrappers.append("""
uint64_t profile_first(void *ctx, void *fn, void *name, uint64_t len, void *args, uint64_t n) {
    probe_begin();
    uint64_t result = tellur_rt_v1_call(ctx, fn, name, len, args, n);
    probe_end();
    return result;
}
""")
with tempfile.TemporaryDirectory() as temp:
    work = Path(temp)
    header = f"#define BUCKETS {len(names)}\n" + """
int probe_enter(int bucket);
void probe_leave(int previous);
void probe_begin(void);
void probe_end(void);
"""
    (work / "probe.h").write_text(header)
    (work / "wrappers.c").write_text("\n".join(wrappers))
    (work / "profile.ll").write_text(ir)
    probe = work / "alloc.dylib"
    def run(*cmd):
        subprocess.run(cmd, check=True)
    run(args.llvm_bin / "opt", "-passes=default<O3>", work / "profile.ll", "-o", work / "code.bc")
    run(args.llvm_bin / "llc", "-O3", "-filetype=obj", work / "code.bc", "-o", work / "code.o")
    run("cc", "-O3", "-dynamiclib", "-I", work, here / "allocations.c", "-o", probe)
    run("cc", "-O3", work / "code.o", work / "wrappers.c", probe, args.archive, "-o", work / "run")
    with args.results.open("w") as out:
        writer = csv.writer(out, lineterminator="\n")
        writer.writerow(("case", "query", "operation", "calls", "allocations", "reallocations", "bytes"))
        for case, limit, flag, expected in (("early", "0", "false", "0"), ("last", "255", "false", "255"),
                                            ("miss", "256", "false", "missing"), ("failure", "255", "true", "invalid")):
            result = subprocess.run([work / "run", limit, flag, "2"], check=True, capture_output=True, text=True)
            assert result.stdout.splitlines() == [expected, expected, "done"]
            query = 0
            for line in result.stderr.splitlines():
                if line == "query":
                    query += 1
                else:
                    bucket, *counts = map(int, line.split())
                    writer.writerow([case, query, names[bucket], *counts])
            assert query == 2
