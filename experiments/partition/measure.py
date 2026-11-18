"""Measure consuming stable partition, excluding input construction and output checking."""
import argparse, csv, json, os, random, subprocess, sys, tempfile
from pathlib import Path
here = Path(__file__).resolve().parent
sys.path.insert(0, str(here.parent / 'first_match'))
from abi_probe import build_allocation_probe, wrap_calls
p = argparse.ArgumentParser(description=__doc__)
for name in ('vnc', 'llvm', 'results'): p.add_argument(name, type=Path)
p.add_argument('--alloc-only', action='store_true', help='reuse existing timings and collect scoped allocation evidence')
p.add_argument('--previous-vnc', type=Path, help='interleave a previous compiler using its own IR and runtime')
a = p.parse_args(); vnc, llvm, results = a.vnc.resolve(), a.llvm.resolve(), a.results.resolve()
results.mkdir(exist_ok=True, parents=True)
def run(*args, **kw): return subprocess.run(args, check=True, text=True, **kw)
cases = {'empty': (0, 0), 'small': (128, 128), 'full': (4096, 4096),
         'first': (4096, 0), 'middle': (4096, 2048), 'last': (4096, 4095)}
with tempfile.TemporaryDirectory(prefix='vn-partition-') as directory:
    tmp = Path(directory)
    compilers = {'vn': vnc}
    if a.previous_vnc: compilers['previous'] = a.previous_vnc.resolve()
    probes, buckets = {}, {}
    for kind, compiler in compilers.items():
        build = tmp / (kind+'-build'); build.mkdir()
        run(compiler, 'build', here.parents[1] / 'examples/partition_batch/main.vn', '--release', '--out', build / 'source',
            env={**os.environ, 'CARGO_INCREMENTAL': '0', 'TELLUR_NATIVE_TARGET_DIR': str(build)})
        archive, = (build / 'tellur-runtime-pack').glob('*.a')
        ir = (build / 'source.ll').read_text(); controls = (b'partition<pivot>',)
        (results / (kind+'-partition.ll')).write_text(ir)
        (build / 'timed.ll').write_text(wrap_calls(ir, controls, 'time_first'))
        run(llvm / 'opt', '-passes=default<O3>', build / 'timed.ll', '-o', build / 'timed.bc')
        run(llvm / 'llc', '-O3', '-filetype=obj', build / 'timed.bc', '-o', build / 'timed.o')
        run('cc', '-O3', '-mmacosx-version-min=13.0', build / 'timed.o', here.parent / 'first_match/timer.c', archive, '-o', tmp / kind)
        probes[kind], buckets[kind] = build_allocation_probe(ir, controls, build, llvm, archive)
    for suffix, flags in [('', []), ('-count', ['--cfg', 'allocation_counts'])]:
        run('rustc', '--edition', '2024', '-C', 'opt-level=3', *flags, here / 'partition.rs', '-o', tmp / ('rs'+suffix))
    (results / 'environment.json').write_text(json.dumps({
        'compiler':run(vnc,'--version',capture_output=True).stdout.strip(),
        'previous_compiler':run(a.previous_vnc.resolve(),'--version',capture_output=True).stdout.strip() if a.previous_vnc else None,
        'comparison':'each compiler uses its own IR and matching runtime',
        'rust':run('rustc','-vV',capture_output=True).stdout,
        'llvm':run(llvm/'opt','--version',capture_output=True).stdout,
        'optimization':'LLVM opt default O3 and llc O3; Rust opt-level=3',
        'method':'13 retained shuffled trials after one discarded trial; one consuming operation per process. Timing excludes construction and checking; short cases include timer noise.'},indent=2)+'\n')
    def query(kind, case, allocations=False, peak=False):
        n, stop = cases[case]
        if kind == 'rs':
            return list(map(int,run(tmp/('rs-count' if allocations else 'rs'),str(n),str(stop),capture_output=True).stdout.strip().split(',')))
        values = [0 if i == stop else i+1 for i in range(n)]; limit=n//2+1
        value = json.dumps({'kind':'construct','value':{'ty':'vector<U32>','name':'vector','fields':[{'kind':'u32','value':x} for x in values]}})
        process = subprocess.run([str(probes[kind] if allocations else tmp/kind),value,str(limit)],text=True,capture_output=True,
            env={**os.environ, **({'VN_ALLOC_PEAK':'1'} if peak else {})})
        assert process.returncode == (7 if stop<n else 0),process.stderr
        label,payload=process.stdout.strip().split(' ',1); fields=json.loads(payload)
        vector=lambda x:[f['value'] for f in x['value']['fields']]
        assert label == ('stopped' if stop<n else 'split')
        end=min(stop,n)
        assert vector(fields[0]) == [x for x in values[:end] if x<limit]
        assert vector(fields[1]) == [x for x in values[:end] if x>=limit]
        if stop<n:
            assert fields[2]['value']==0 and vector(fields[3])==values[stop+1:] and fields[4]['value']=='unclassified'
        return process.stderr.splitlines()
    schedule=[(kind,case) for kind in (*compilers,'rs') for case in cases]; rng=random.Random(297)
    if not a.alloc_only:
      with (results/'timings.csv').open('w') as f:
          writer=csv.writer(f);writer.writerow(('trial','kind','case','ns'))
          for trial in range(14):
              rng.shuffle(schedule)
              for kind,case in schedule:
                  row=query(kind,case)
                  if trial: writer.writerow((trial,kind,case,int(row[0])))
    detail=[]
    with (results/'allocations.csv').open('w') as f:
        writer=csv.writer(f);writer.writerow(('kind','case','allocations','reallocations','requested_bytes','peak_extra_bytes'))
        for kind,case in schedule:
            row=query(kind,case,True)
            if kind=='rs':writer.writerow((kind,case,row[1],'',row[3],row[2]));continue
            measured=query(kind,case,True,True)
            assert [x for x in measured if not x.startswith('peak ')]==row
            assert row.count('query')==1
            counts=[list(map(int,x.split())) for x in row if x!='query']
            detail.extend((kind,case,buckets[kind][x[0]],*x[1:]) for x in counts)
            peak,=[int(x.split()[1]) for x in measured if x.startswith('peak ')]
            writer.writerow((kind,case,*[sum(x[i] for x in counts) for i in (2,3,4)],peak))
    with (results/'operations.csv').open('w') as f:
        writer=csv.writer(f);writer.writerow(('kind','case','operation','calls','allocations','reallocations','requested_bytes'));writer.writerows(detail)
