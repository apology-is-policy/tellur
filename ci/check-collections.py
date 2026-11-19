#!/usr/bin/env python3
"""Qualify vendored collections outside the checkout and package their sources.

Requires a qualified vnc and Rust toolchain for the caller-owned adapter example.
No compiler rebuild is performed. Temporary projects/binaries are removed on exit.
"""
import argparse
import bisect
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EXAMPLE_FILES = ("main.vn", "manifest.json", "host.rs", "limits.rs", "Cargo.lock", "README.md")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def scalar(value):
    return {"kind": "string" if isinstance(value, str) else "u32", "value": value}


def vector(values):
    return {"kind": "construct", "value": {
        "name": "vector", "ty": "vector<U32>", "fields": [scalar(v) for v in values]}}


def unpack(value):
    if value["kind"] == "construct":
        return [unpack(field) for field in value["value"]["fields"]]
    return value["value"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vnc", type=Path, default=ROOT / "bin/vnc")
    parser.add_argument("--out", type=Path, default=ROOT / "build/collections")
    args = parser.parse_args()
    compiler, output = args.vnc.resolve(), args.out.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "CARGO_INCREMENTAL": "0"}
    reports = []
    with tempfile.TemporaryDirectory(prefix="Villanelle collection adoption ") as temporary:
        scratch = Path(temporary)
        cwd = scratch / "unrelated working directory"
        cwd.mkdir()

        def run(command, expected=0):
            result = subprocess.run([str(v) for v in command], cwd=cwd, env=env,
                                    capture_output=True, text=True, timeout=300)
            if result.returncode != expected:
                raise RuntimeError(f"{command}: exit {result.returncode}, expected {expected}\n"
                                   f"{result.stdout}\n{result.stderr}")
            return result

        version = run([compiler, "--version"]).stdout.strip()
        project = scratch / "new project"
        library = project / "vendor/villanelle"
        run([sys.executable, "-B", ROOT / "library/vendor_collections.py", library])
        receipt = json.loads((library / "collections.lock.json").read_text())
        inventory = json.loads((library / "collections.json").read_text())
        assert set(p.name for p in library.iterdir()) == set(receipt["sha256"]) | {"collections.lock.json"}
        assert set(receipt["sha256"]) == set(inventory["modules"]) | {
            "arithmetic.manifest.json", "COLLECTIONS.md", "collections.json", "LICENSE"}
        assert all(digest(library / name) == sha for name, sha in receipt["sha256"].items())
        assert any(v in version for v in receipt["tested_compilers"]), version
        # Refusal must preserve every file, including unrelated user content.
        (library / "user marker").write_text("preserve me\n")
        before = {p.name: digest(p) for p in library.iterdir()}
        run([sys.executable, "-B", ROOT / "library/vendor_collections.py", library], 1)
        assert before == {p.name: digest(p) for p in library.iterdir()}
        (library / "user marker").unlink()
        link = project / "linked destination"
        link.symlink_to(library, target_is_directory=True)
        run([sys.executable, "-B", ROOT / "library/vendor_collections.py", link], 1)
        assert all(digest(library / name) == sha for name, sha in receipt["sha256"].items())
        reports.append({"case": "fixed inventory, hashes, existing-directory and symlink refusal"})

        def copy_example(name, files, destination=None):
            dest = destination or project / "apps" / name
            dest.mkdir(parents=True)
            for name_in_example in files:
                data = (ROOT / "examples" / name / name_in_example).read_text()
                if name_in_example.endswith(".vn"):
                    data = data.replace("../../library/", "../../vendor/villanelle/")
                (dest / name_in_example).write_text(data)
            return dest

        def build(source, backend="llvm"):
            binary = source.parent / (source.stem + "-" + backend)
            run([compiler, "check", source])
            run([compiler, "build", source, "--release", "--backend", backend, "--out", binary])
            return binary

        def case(binary, arguments, outcome, fields, emitted=(), code=0, adapter_calls=None):
            arguments = [json.dumps(a) if isinstance(a, dict) else str(a) for a in arguments]
            # Exercise normal optimized execution as well as its observed JSON path.
            normal = run([binary, *arguments], code).stdout.splitlines()
            actual_outcome, _, payload = normal[-1].partition(" ")
            assert actual_outcome == outcome, normal
            assert [unpack(v) for v in json.loads(payload or "[]")] == fields, normal
            assert normal[:-1] == list(emitted), normal
            observed = json.loads(run([binary, *arguments, "--json"], code).stdout)
            assert observed["outcome"] == outcome, observed
            assert [unpack(v) for v in observed["fields"]] == fields, observed
            assert [e["message"] for e in observed["events"] if e["kind"] == "emit"] == list(emitted)
            if adapter_calls is not None:
                calls = [e["arguments"][0]["value"] for e in observed["events"]
                         if e.get("operation") == "accept_reading"]
                assert calls == adapter_calls, calls
            reports.append({"case": binary.name, "arguments": arguments, "outcome": outcome,
                            "fields": fields, "observed_and_unobserved": True})

        pipeline = copy_example("collection_pipeline", EXAMPLE_FILES)
        for backend in ("llvm", "rust"):
            binary = build(pipeline / "main.vn", backend)
            for limit, last in ((100, 3), (5, 3), (100, 0), (2**32 - 1, 2**32 - 1)):
                values = sorted([4, 1, 2, 2, last])
                first, end = bisect.bisect_left(values, 2), bisect.bisect_right(values, 2)
                total, calls, outcome, reason = 0, [], "finished", None
                for value in values:
                    calls.append(value)
                    if value == 0:
                        outcome, reason = "failed", "sensor offline"
                        break
                    if total + value > 2**32 - 1:
                        outcome, reason = "failed", "total overflow"
                        break
                    total += value
                    if total >= limit:
                        outcome = "stopped"
                        break
                fields = [total] + ([reason] if reason else []) + [first, end, values]
                case(binary, [limit, last], outcome, fields, code=int(reason is not None), adapter_calls=calls)

        search = copy_example("first_match", ("main.vn", "manifest.json"))
        case(build(search / "main.vn"), [], "done", [], ["17", "29"])
        failure = search / "failure.vn"
        failure.write_text((search / "main.vn").read_text().replace("limit: 25", "limit: 50"))
        case(build(failure), [], "unexpected", [], ["17", "sensor offline"], code=1)
        traversal = copy_example("borrowed_traversal", ("main.vn", "headers.vn", "manifest.json"))
        case(build(traversal / "main.vn"), [], "done", [5], ["green", "blue"])
        case(build(traversal / "headers.vn"), [], "done", [1], ["accept", "text/plain", "origin", "example.org"])
        # Test both map membership outcomes before the existing loan-ending mutation.
        membership = traversal / "membership.vn"
        membership.write_text('import "../../vendor/villanelle/membership.vn" as membership;\n' +
            (traversal / "headers.vn").read_text().replace("print_headers(borrow headers);", '''
        delta membership.contains_key<string>(borrow headers, borrow "origin") {
            present => found,
            absent => exit unexpected
        }
        [found]: delta membership.contains_key<string>(borrow headers, borrow "missing") {
            present => exit unexpected,
            absent => scan
        }
        [scan]: print_headers(borrow headers);'''))
        run([compiler, "format", membership, "--write"])
        case(build(membership), [], "done", [1], ["accept", "text/plain", "origin", "example.org"])
        rotation = copy_example("rotate_batch", ("main.vn", "manifest.json"))
        binary = build(rotation / "main.vn")
        for values, boundary in (([], 0), ([1, 2, 3], 0), ([1, 2, 3], 1), ([1, 2, 3], 3), ([1, 2], 3)):
            valid = boundary <= len(values)
            case(binary, [vector(values), boundary], "ready" if valid else "invalid",
                 [values[boundary:] + values[:boundary] if valid else values], code=int(not valid))
        partition = copy_example("partition_batch", ("main.vn", "manifest.json"))
        binary = build(partition / "main.vn")
        for values in ([], [3, 1, 5, 2, 4], [3, 1, 0, 2, 4]):
            stop = values.index(0) if 0 in values else len(values)
            left = [v for v in values[:stop] if v < 3]
            right = [v for v in values[:stop] if v >= 3]
            fields = [left, right]
            if stop < len(values):
                fields += [values[stop], values[stop + 1:], "unclassified"]
            case(binary, [vector(values), 3], "stopped" if stop < len(values) else "split", fields, code=7 if stop < len(values) else 0)
        ordered = copy_example("ordered_search", ("main.vn", "demo.vn", "manifest.json"))
        binary = build(ordered / "demo.vn")
        records = sorted([[2, 0], [1, 1], [2, 2], [1, 3], [3, 4]], key=lambda r: r[0])
        keys = [r[0] for r in records]
        for key in (0, 1, 2, 3, 4):
            first, end = bisect.bisect_left(keys, key), bisect.bisect_right(keys, key)
            found = first != end
            case(binary, [key], "present" if found else "absent",
                 [first, end, records[first][1], records] if found else [first, records])

        # Bundle only allowlisted source payloads; generated outputs and local configs
        # are never traversed. Extract our own bundle and run it independently.
        bundle = output / "villanelle-collections-0.1.0.tar.gz"
        bundle_root = scratch / "bundle sources"
        shutil.copytree(library, bundle_root / "library")
        example = bundle_root / "example"
        example.mkdir()
        for name in EXAMPLE_FILES:
            text = (ROOT / "examples/collection_pipeline" / name).read_text()
            if name.endswith(".vn"):
                text = text.replace("../../library/", "../library/")
            (example / name).write_text(text)
        (bundle_root / "README.md").write_text(
            "# Villanelle collections 0.1.0\n\nSee library/COLLECTIONS.md for contracts.\n"
            "With vnc 0.306.0 and a Rust toolchain, run:\n\n"
            "```sh\nvnc exec example/main.vn -- 100 3\n```\n\n"
            "The example owns its explicitly wired Rust adapter. Library modules need no adapter.\n")
        with tarfile.open(bundle, "w:gz") as archive:
            for name in ("README.md", *["library/" + n for n in receipt["sha256"]],
                         "library/collections.lock.json", *["example/" + n for n in EXAMPLE_FILES]):
                archive.add(bundle_root / name, arcname="villanelle-collections-0.1.0/" + name)
        extracted = scratch / "extracted source bundle"
        with tarfile.open(bundle) as archive:
            archive.extractall(extracted, filter="data")
        extracted_root = extracted / "villanelle-collections-0.1.0"
        assert all(digest(extracted_root / "library" / name) == sha for name, sha in receipt["sha256"].items())
        case(build(extracted_root / "example/main.vn"), [100, 3], "finished", [12, 1, 3, [1, 2, 2, 3, 4]])
        evidence = {"compiler": version, "library": inventory, "cases": reports,
                    "source_hashes": receipt["sha256"], "bundle": str(bundle),
                    "bundle_sha256": digest(bundle), "fresh_project_outside_checkout": True,
                    "different_working_directory": True, "paths_with_spaces": True,
                    "incremental": False, "temporary_projects_removed_on_exit": True}
        (output / "verified.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(f"Qualified {len(reports)} adoption cases, all eight modules, LLVM/Rust caller adapter and extracted source bundle.")


if __name__ == "__main__":
    main()
