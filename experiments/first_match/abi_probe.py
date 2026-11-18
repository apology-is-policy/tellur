"""Instrument concrete control calls in release LLVM IR on arm64 macOS."""
from pathlib import Path
import re
import subprocess


def wrap_calls(ir, controls, wrapper):
    declarations = set()
    for control in controls:
        escaped = "".join(f"\\{byte:02X}" for byte in control)
        symbol, = [line.split()[0] for line in ir.splitlines() if f'c"{escaped}"' in line]
        call, = [line for line in ir.splitlines()
                 if any(f"call i64 @{abi}(" in line for abi in
                        ("tellur_rt_v1_call", "tellur_rt_v1_join_call"))
                 and f"ptr {symbol}," in line]
        joined = "@tellur_rt_v1_join_call(" in call
        abi = "tellur_rt_v1_join_call" if joined else "tellur_rt_v1_call"
        wrapped = wrapper + ("_join" if joined else "")
        ir = ir.replace(call, call.replace(f"@{abi}(", f"@{wrapped}("))
        params = "ptr, ptr, ptr, i64, ptr, i64" + (", i64, ptr, i64" if joined else "")
        declarations.add(f"declare i64 @{wrapped}({params})")
    return ir + "\n" + "\n".join(sorted(declarations)) + "\n"


def build_allocation_probe(ir, controls, work, llvm_bin, archive):
    """Return a probe executable and its per-operation bucket names."""
    ir = wrap_calls(ir, controls, "profile_outer")
    prefix = "tellur_rt_v1_"
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
    uint64_t profile_outer(void *ctx, void *fn, void *name, uint64_t len, void *args, uint64_t n) {
        probe_begin();
        uint64_t result = tellur_rt_v1_call(ctx, fn, name, len, args, n);
        probe_end();
        return result;
    }
    """)
    if "call i64 @profile_outer_join(" in ir:
        wrappers.append("""
        extern uint64_t tellur_rt_v1_join_call(void *, void *, void *, uint64_t, void *, uint64_t, uint64_t, void *, uint64_t);
        uint64_t profile_outer_join(void *ctx, void *fn, void *name, uint64_t len,
                                   void *args, uint64_t n, uint64_t destination,
                                   void *outcome, uint64_t outcome_len) {
            probe_begin();
            uint64_t result = tellur_rt_v1_join_call(ctx, fn, name, len, args, n,
                                                    destination, outcome, outcome_len);
            probe_end();
            return result;
        }
        """)
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
    run(llvm_bin / "opt", "-passes=default<O3>", work / "profile.ll", "-o", work / "code.bc")
    run(llvm_bin / "llc", "-O3", "-filetype=obj", work / "code.bc", "-o", work / "code.o")
    run("cc", "-O3", "-dynamiclib", "-I", work, Path(__file__).with_name("allocations.c"), "-o", probe)
    run("cc", "-O3", work / "code.o", work / "wrappers.c", probe, archive, "-o", work / "run")
    return work / "run", names
