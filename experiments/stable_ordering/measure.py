"""Measure consuming stable ordering, excluding input construction and output checking."""
import argparse, csv, json, os, random, subprocess, sys, tempfile
from pathlib import Path
here = Path(__file__).resolve().parent
sys.path.insert(0, str(here.parent / 'first_match'))
from abi_probe import build_allocation_probe, wrap_calls
p = argparse.ArgumentParser(description=__doc__)
for name in ('vnc', 'llvm', 'results'): p.add_argument(name, type=Path)
p.add_argument('--alloc-only', action='store_true', help='reuse existing timings and collect scoped allocation evidence')

a = p.parse_args(); vnc, llvm, results = a.vnc.resolve(), a.llvm.resolve(), a.results.resolve()
results.mkdir(exist_ok=True, parents=True)
def run(*args, **kw): return subprocess.run(args, check=True, text=True, **kw)
cases = {'empty':(0,'mixed'), 'single':(1,'mixed'), 'small':(128,'mixed'),
         'mixed':(1024,'mixed'), 'sorted':(1024,'sorted'), 'reverse':(1024,'reverse'), 'equal':(1024,'equal')}
with tempfile.TemporaryDirectory(prefix='vn-ordering-') as directory:
    tmp = Path(directory)
    compilers = {'vn': vnc}
    probes, buckets = {}, {}
    for kind, compiler in compilers.items():
        build = tmp / (kind+'-build'); build.mkdir()
        run(compiler, 'build', here.parents[1] / 'examples/stable_ordering/main.vn', '--release', '--out', build / 'source',
            env={**os.environ, 'CARGO_INCREMENTAL': '0', 'TELLUR_NATIVE_TARGET_DIR': str(build)})
        archive, = (build / 'tellur-runtime-pack').glob('*.a')
        ir = (build / 'source.ll').read_text(); controls = (b'sort<key_order>',)
        (results / (kind+'-sort.ll')).write_text(ir)
        (build / 'timed.ll').write_text(wrap_calls(ir, controls, 'time_first'))
        run(llvm / 'opt', '-passes=default<O3>', build / 'timed.ll', '-o', build / 'timed.bc')
        run(llvm / 'llc', '-O3', '-filetype=obj', build / 'timed.bc', '-o', build / 'timed.o')
        run('cc', '-O3', '-mmacosx-version-min=13.0', build / 'timed.o', here.parent / 'first_match/timer.c', archive, '-o', tmp / kind)
        probes[kind], buckets[kind] = build_allocation_probe(ir, controls, build, llvm, archive)
    for suffix, flags in [('', []), ('-count', ['--cfg', 'allocation_counts'])]:
        run('rustc', '--edition', '2024', '-C', 'opt-level=3', *flags, here / 'sort.rs', '-o', tmp / ('rs'+suffix))
    (results / 'environment.json').write_text(json.dumps({
        'compiler':run(vnc,'--version',capture_output=True).stdout.strip(),
        'comparison':'each compiler uses its own IR and matching runtime',
        'rust':run('rustc','-vV',capture_output=True).stdout,
        'llvm':run(llvm/'opt','--version',capture_output=True).stdout,
        'optimization':'LLVM opt default O3 and llc O3; Rust opt-level=3',
        'method':'13 retained shuffled trials after one discarded trial; one consuming operation per process. Timing excludes construction and checking; short cases include timer noise.'},indent=2)+'\n')
    def query(kind, case, allocations=False, peak=False):
        n, pattern = cases[case]
        if kind == 'rs':
            return list(map(int,run(tmp/('rs-count' if allocations else 'rs'),str(n),pattern,capture_output=True).stdout.strip().split(',')))
        keys=[i//3 if pattern=='sorted' else (n-1-i)//3 if pattern=='reverse' else 1 if pattern=='equal' else (i*37+11)%31 for i in range(n)]
        values=list(zip(keys,range(n)))
        records=[{'kind':'construct','value':{'ty':'record','name':'record','fields':[{'kind':'u32','value':k},{'kind':'u32','value':i}]}} for k,i in values]
        value=json.dumps({'kind':'construct','value':{'ty':'vector<record>','name':'vector','fields':records}},separators=(',',':'))
        process = subprocess.run([str(probes[kind] if allocations else tmp/kind),value],text=True,capture_output=True,
            env={**os.environ, **({'VN_ALLOC_PEAK':'1'} if peak else {})})
        assert process.returncode==0,process.stderr
        label,payload=process.stdout.strip().split(' ',1)
        assert label=='sorted',label
        fields=json.loads(payload)
        actual=[tuple(x['value'] for x in r['value']['fields']) for r in fields[0]['value']['fields']]
        assert actual==sorted(values),(case,actual)
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
