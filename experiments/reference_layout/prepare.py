"""Prepare an isolated runtime candidate from the recorded 0.305 source layout."""
import argparse
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('output', type=Path, help='new directory for the isolated runtime sources')
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
output = args.output.resolve()
# Fail on an existing directory so experiments cannot overwrite another job.
output.mkdir(parents=True)
(output / 'runtime').mkdir()
(output / 'src/native').mkdir(parents=True)
paths = [Path('runtime') / name for name in ('lib.rs', 'Cargo.toml', 'Cargo.lock')]
paths += [p.relative_to(repo) for p in (repo / 'src/native').glob('*.rs')]
paths += [Path('src') / name for name in ('collection_support.rs', 'fault.rs', 'task_support.rs')]
for path in paths:
    shutil.copy2(repo / path, output / path)
subprocess.run(['patch', '-p1', '-i', str(Path(__file__).with_name('candidate.patch'))],
               cwd=output, check=True)
print(output / 'runtime/Cargo.toml')
