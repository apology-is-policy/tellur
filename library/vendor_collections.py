#!/usr/bin/env python3
"""Copy the supported collection source slice into a new project directory."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def vendor(destination):
    inventory = json.loads((ROOT / "collections.json").read_text())
    names = [*inventory["modules"], inventory["arithmetic_manifest"], inventory["contracts"], "collections.json"]
    # Read the fixed published inventory before creating a destination. No checkout
    # walk, compiler binaries, host adapters or user configuration are copied.
    payloads = {name: (ROOT / name).read_bytes() for name in names}
    payloads["LICENSE"] = (ROOT.parent / "LICENSE").read_bytes()
    receipt = {
        "format": "villanelle.vendored-source", "format_version": 1,
        "name": inventory["name"], "version": inventory["version"],
        "tested_compilers": inventory["tested_compilers"],
        "sha256": {name: hashlib.sha256(data).hexdigest() for name, data in sorted(payloads.items())},
    }
    payloads["collections.lock.json"] = (json.dumps(receipt, indent=2) + "\n").encode()
    # Existing files/directories/symlinks are never replaced. A failed write leaves
    # a visibly incomplete destination; retry with a new path after inspecting it.
    destination.mkdir(parents=True, exist_ok=False)
    for name, data in payloads.items():
        with (destination / name).open("xb") as output:
            output.write(data)
    return receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new directory, e.g. my-project/vendor/villanelle")
    args = parser.parse_args()
    try:
        receipt = vendor(args.destination)
    except (OSError, ValueError) as error:
        parser.exit(1, f"Cannot vendor collections: {error}\n")
    print(f"Vendored {receipt['name']} {receipt['version']} into {args.destination}")
