#!/usr/bin/env python3
"""Measure the two hand lowerings with the same embedded LLVM/LLD compiler.

Usage: python3 experiments/coroutines/measure.py --out /tmp/tellur-frames
All generated objects, binaries, scenario scripts and raw evidence stay in --out.
"""
import argparse
import hashlib
import json
import os
import platform
import re
import statistics
import subprocess
from pathlib import Path


def run(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, capture_output=True,
                          text=True, **kwargs).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--compiler', type=Path, default=Path('bin/tellurc'))
    parser.add_argument('--clang', type=Path, default=Path('/opt/homebrew/opt/llvm@22/bin/clang'))
    parser.add_argument('--iterations', type=int, default=100000)
    parser.add_argument('--samples', type=int, default=5)
    args = parser.parse_args()
    if not 1 <= args.iterations <= 1000000 or not 1 <= args.samples <= 20:
        parser.error('iterations must be 1..1000000, samples 1..20')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).resolve().parent
    compiler = args.compiler.resolve()
    sysroot = run(['xcrun', '--show-sdk-path'])
    for name in ['runtime', 'benchmark']:
        run([args.clang, '-isysroot', sysroot, '-S', '-emit-llvm', '-O0', '-Xclang',
             '-disable-O0-optnone', source / f'{name}.c', '-o', out / f'{name}.ll'])
    scenarios = {
        'cancellation': 'open opened 0\nopen opened 0\nreceive data 0 hello\n'
                        'receive data 9 late\nclose closed 0\nclose closed 0\ndecode_utf8 text 0 hello\n',
        'sets': 'tick ticked 1 a\ntick ticked 2 b\ntick ticked 4 c\n',
        'fault': 'tick ticked 4 one\n',
        'nested-fault': 'tick ticked 4 one\n',
    }
    records = []
    for specimen, scenario in scenarios.items():
        script = out / f'{specimen}.txt'
        script.write_text(scenario)
        for opt in [0, 2]:
            pair = []
            for model in ['coroutine', 'frontend']:
                stem = specimen if model == 'coroutine' else f'frontend-{specimen}'
                ir = source / f'{stem}.ll'
                bridge = source / ('intrinsics.ll' if model == 'coroutine' else 'frontend.ll')
                normal = out / f'{stem}-O{opt}'
                flags = ['--release'] if opt == 2 else []
                run([compiler, 'llvm-smoke', *flags, normal, out / 'runtime.ll', bridge, ir])
                # Preserve fault statuses; metrics are a separate stderr record.
                trace = subprocess.run([str(normal), str(script)], capture_output=True, text=True,
                                       env={**os.environ, 'TELLUR_FRAME_METRICS': '1'})
                expected = 1 if 'fault' in specimen else 0
                if trace.returncode != expected:
                    raise RuntimeError(f'{stem}: {trace.stderr}')
                metrics = json.loads(next(line[8:] for line in trace.stderr.splitlines()
                                          if line.startswith('frames: ')))
                size = run([args.clang.parent / 'llvm-size', '-A', normal])
                text_bytes = int(re.search(r'^__text\s+(\d+)', size, re.M)[1])
                benchmark_ir = out / f'{stem}-benchmark.ll'
                text = ir.read_text()
                assert text.count('define i32 @main(') == 1
                benchmark_ir.write_text(text.replace('define i32 @main(', 'define i32 @specimen_main('))
                binary = out / f'{stem}-O{opt}-benchmark'
                run([compiler, 'llvm-smoke', *flags, binary, out / 'runtime.ll', bridge,
                     benchmark_ir, out / 'benchmark.ll'])
                record = dict(specimen=specimen, model=model, opt=opt, metrics=metrics,
                              binary_bytes=normal.stat().st_size, text_bytes=text_bytes,
                              status=trace.returncode, events=trace.stdout.splitlines(),
                              diagnostics=[line for line in trace.stderr.splitlines()
                                           if not line.startswith('frames: ')], samples=[])
                records.append(record)
                pair.append((binary, record))
            assert pair[0][1]['events'] == pair[1][1]['events']
            assert pair[0][1]['diagnostics'] == pair[1][1]['diagnostics']
            # Alternate model order to reduce systematic drift. Each subprocess
            # times the loop internally; launch and 1001 warmups are excluded.
            for sample in range(args.samples):
                for binary, record in (pair if sample % 2 == 0 else pair[::-1]):
                    record['samples'].append(json.loads(run([binary, script, args.iterations])))
            assert len({r['checksum'] for _, record in pair for r in record['samples']}) == 1
            for _, record in pair:
                record['median_ns_per_run'] = statistics.median(
                    s['elapsed_ns'] / s['iterations'] for s in record['samples'])
            print(f'{specimen} O{opt}: ' + ', '.join(
                f"{r['model']} {r['median_ns_per_run']:.1f} ns/run" for _, r in pair), flush=True)
    inputs = sorted(source.glob('*.ll')) + [source / name for name in ['runtime.c', 'benchmark.c', 'measure.py']]
    evidence = dict(platform=platform.platform(), machine=platform.machine(),
                    clang=run([args.clang, '--version']), compiler=run([compiler, '--version']),
                    compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                    sysroot=sysroot, iterations=args.iterations, samples=args.samples,
                    inputs={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs},
                    records=records)
    (out / 'measurements.json').write_text(json.dumps(evidence, indent=2) + '\n')


if __name__ == '__main__':
    main()
