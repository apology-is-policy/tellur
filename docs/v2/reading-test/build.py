#!/usr/bin/env python3
"""Assemble the four subject files of the reading and repair test.

Run from the repository root:  python3 -B docs/v2/reading-test/build.py [OUTPUT_DIRECTORY]

  subject-v1-a.md, subject-v2-a.md   primer, program, tasks 1 and 2
  subject-v1-b.md, subject-v2-b.md   primer, program with two seeded omissions, task 3

The files go to `build/` beside this script unless a directory is given. The
program text is taken from the repository as it stands, so a rebuild after a
change to the corpus gives the current text; `results.md` has the digests of
the files that were run. Each seeded omission is one asserted exact
replacement.
"""
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
OUT = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else HERE / "build"
FILES = ["settings", "http", "server", "main"]

SOURCES = {
    "v1": {name: ROOT / "applications/settings" / f"{name}.vn" for name in FILES},
    "v2": {name: ROOT / "docs/v2/corpus/settings" / f"{name}.txt" for name in FILES},
}
HOST = {"v1": HERE / "host-v1.txt", "v2": HERE / "host-v2.txt"}
FENCE = {"v1": "villanelle", "v2": "text"}
LOCKED = {"v1": "locked { string holder }", "v2": "locked {holder: string}"}

# Seeded omissions: (file, exact text, replacement).
SEEDS = {
    "v1": [
        (
            "http",
            "            invalid_file { string message } => text { 500, message },\n"
            "            storage_failed { string message } => text { 500, message },\n"
            '            too_large => text { 500, "Settings file exceeds 1 MiB" }\n',
            "            invalid_file { string message } => text { 500, message },\n"
            '            too_large => text { 500, "Settings file exceeds 1 MiB" }\n',
        ),
        (
            "server",
            "        );\n        transport.release(listener);\n        exit delta result {\n",
            "        );\n        exit delta result {\n",
        ),
    ],
    "v2": [
        (
            "http",
            "│ ├ found {value}\n"
            '│ ├ missing                  → ■ text {404, "Setting not found"}\n'
            '│ ├ invalid_key              → ■ text {400, "Invalid setting key"}\n'
            "│ ├ invalid_file {message}   → ■ text {500, message}\n"
            "│ ├ storage_failed {message} → ■ text {500, message}\n"
            '│ └ too_large                → ■ text {500, "Settings file exceeds 1 MiB"}\n',
            "│ ├ found {value}\n"
            '│ ├ missing                → ■ text {404, "Setting not found"}\n'
            '│ ├ invalid_key            → ■ text {400, "Invalid setting key"}\n'
            "│ ├ invalid_file {message} → ■ text {500, message}\n"
            '│ └ too_large              → ■ text {500, "Settings file exceeds 1 MiB"}\n',
        ),
        (
            "server",
            "│ transport.release(▾listener)\n│ ■ ▾result\n",
            "│ ■ ▾result\n",
        ),
    ],
}


def sections():
    parts = {}
    name = None
    for line in (HERE / "tasks.md").read_text().splitlines(keepends=True):
        if line.startswith("=== ") and line.rstrip().endswith(" ==="):
            name = line.strip()[4:-4]
            parts[name] = ""
        elif name:
            parts[name] += line
    return {key: value.strip("\n") + "\n" for key, value in parts.items()}


def program(version, seeded):
    texts = {name: path.read_text() for name, path in SOURCES[version].items()}
    if seeded:
        for name, old, new in SEEDS[version]:
            assert texts[name].count(old) == 1, (version, name, texts[name].count(old))
            texts[name] = texts[name].replace(old, new)
    out = ["## Part 2. The program\n",
           "A small service that keeps settings (text keys and values) in a JSON file. It has\n"
           "command-line entries (`main`, `get`, `set`, `remove`, `serve`) and an HTTP interface.\n"
           "Four files make up the application. A fifth lists the signatures of what it calls\n"
           "and does not define.\n"]
    for name in FILES:
        out.append(f"### File `{name}.vn`\n")
        out.append(f"```{FENCE[version]}\n{texts[name].rstrip()}\n```\n")
    out.append("### Signatures of the host and the two libraries (not part of the application)\n")
    out.append(f"```{FENCE[version]}\n{HOST[version].read_text().rstrip()}\n```\n")
    return "\n".join(out)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    parts = sections()
    for version in ("v1", "v2"):
        primer = (HERE / f"primer-{version}.md").read_text().strip("\n") + "\n"
        tasks_a = parts["tasks-a"].replace("{LOCKED}", LOCKED[version]).replace(
            "{LOAD_RESULT}", "The outcome `load_result`")
        for letter, seeded, tasks in (("a", False, tasks_a), ("b", True, parts["tasks-b"])):
            text = "\n".join([parts["intro"], primer, program(version, seeded), tasks])
            target = OUT / f"subject-{version}-{letter}.md"
            target.write_text(text)
            print(f"{target.name}: {len(text.splitlines())} lines, {len(text.split())} words, "
                  f"{len(text)} characters")


if __name__ == "__main__":
    main()
