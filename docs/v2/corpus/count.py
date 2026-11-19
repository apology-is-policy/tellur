#!/usr/bin/env python3
"""Counts for the paper corpus: the v1 sources and their v2 rewrites.

Run from the repository root:  python3 docs/v2/corpus/count.py

Everything here is counted from the text by the rules in docs/v2/corpus.md,
section 2. Filler arms are judged by reading and are listed there by line.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
CORPUS = Path(__file__).resolve().parent

PAIRS = [
    ("library", "library/membership.vn", "library/membership.txt"),
    ("library", "library/search.vn", "library/search.txt"),
    ("library", "library/reduction.vn", "library/reduction.txt"),
    ("library", "library/partition.vn", "library/partition.txt"),
    ("library", "library/sequence_batches.vn", "library/sequence_batches.txt"),
    ("library", "library/traversal.vn", "library/traversal.txt"),
    ("library", "library/ordering.vn", "library/ordering.txt"),
    ("library", "library/ordered_search.vn", "library/ordered_search.txt"),
    ("settings", "applications/settings/settings.vn", "settings/settings.txt"),
    ("settings", "applications/settings/http.vn", "settings/http.txt"),
    ("settings", "applications/settings/server.vn", "settings/server.txt"),
    ("settings", "applications/settings/main.vn", "settings/main.txt"),
    ("programs", None, "programs/retry_client.txt"),
    ("programs", None, "programs/parser.txt"),
    ("programs", None, "programs/interpreter.txt"),
]

V1_WORDS = {"exit", "again", "forward", "delta", "fault", "impossible", "terminate"}
RAILS = "│║┆├└╞╠═╵ "


def v1_code(text):
    """Lines of v1 that are neither blank nor only a comment, without trailing comments."""
    lines = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("//"):
            continue
        lines.append(re.sub(r"\s//.*$", "", line))
    return lines


def v1_binding_types(code):
    """Types or `auto` written where v1 binds a name: payloads, locals, loop parameters."""
    count = 0
    for pattern in re.findall(r"\{([^{}]*)\}\s*=>", code):
        count += len([entry for entry in pattern.split(",") if entry.strip()])
    binding = re.compile(
        r"(?:^|[(,;:{])\s*(?:mutable\s+)?(?:auto|[a-z_][a-z0-9_.]*(?:<[^=;()]*>)?)"
        r"\s+[a-z_][a-z0-9_]*\s*=(?![=>])",
        flags=re.M,
    )
    return count + len(binding.findall(code))


def count_v1(text):
    lines = v1_code(text)
    code = "\n".join(lines)
    strings = re.sub(r'"(?:\\.|[^"\\])*"', '""', code)
    routes = 0
    for name in re.findall(r"=>\s*([a-z_][a-z0-9_]*)\s*(?:,|$|\})", strings, flags=re.M):
        if name not in V1_WORDS:
            routes += 1
    declaration = re.compile(r"^\s*(pub\s+)?(async\s+)?(control|trait|struct|variant|boundary|implement)\b")
    type_arguments = sum(
        len(re.findall(r"[a-z_][a-z0-9_.]*<[^()=]*>\(", line))
        for line in strings.splitlines()
        if not declaration.match(line)
    )
    return {
        "lines": len(lines),
        "decisions": len(re.findall(r"\bdelta\b", strings)),
        "labels": len(re.findall(r"\[[a-z_][a-z0-9_ |&!().]*\]\s*:", strings)),
        "routes": routes,
        "origin marks": len(re.findall(r"\bfrom\b", strings))
        + len(re.findall(r"\['", strings))
        + len(re.findall(r"<-\s*'", strings))
        + len(re.findall(r">\[[a-z_]+\]", strings)),
        "borrow and move marks": len(re.findall(r"\bborrow\b", strings))
        + len(re.findall(r"\bderef\b", strings))
        + len(re.findall(r"\bmove\b", strings)),
        "clone": len(re.findall(r"\bclone\(", strings)),
        "mutable": len(re.findall(r"\bmutable\b", strings))
        - len(re.findall(r"\bborrow mutable\b", strings)),
        "types at bindings": v1_binding_types(strings),
        "type arguments": type_arguments,
        "proof lines": 0,
        "impossible": 0,
        "uses": 0,
    }


def v2_code(text):
    """Lines of v2 that hold something other than rails and are not only a comment."""
    lines = []
    for line in text.splitlines():
        content = line.lstrip(RAILS)
        if line.strip() == "╵":
            lines.append(line)
            continue
        if not content.strip() or content.startswith("┄"):
            continue
        lines.append(line)
    return lines


def count_v2(text):
    lines = v2_code(text)
    strings = [re.sub(r'"(?:\\.|[^"\\])*"', '""', line) for line in lines]
    code = "\n".join(strings)
    routes = 0
    in_adapter = False
    for line in strings:
        content = line.lstrip("│║┆ ")
        is_arm = content.startswith(("├", "└"))
        if not is_arm:
            in_adapter = False
        if content.startswith("■ Δ"):
            in_adapter = True
            continue
        if in_adapter:
            continue
        if "Δ" not in content and not is_arm:
            continue
        for name in re.findall(r"→\s*([a-z_][a-z0-9_]*)\s*(?=│|$)", content):
            routes += 1
    binder = len(re.findall(r"(?:^|[│║┆({,]\s*)~[a-z_][a-z0-9_]*(?=\s*:|\s*[},])", code, flags=re.M))
    tilde = len(re.findall(r"(?<!&)~", code))
    written = 0
    for line in strings:
        if not line.startswith(("│", "╞")):
            continue
        written += len(re.findall(r"[a-z_][a-z0-9_]*:\s*[^:=,Δ]+?\s:=", line))
    proof_lines = sum(1 for line in strings if "⊢" in line or "⇐" in line)
    return {
        "lines": len(lines),
        "decisions": len(re.findall(r"Δ", code)),
        "labels": len(re.findall(r"[╞╠]═", code)),
        "routes": routes,
        "origin marks": len(re.findall(r"∈", code)) - len(re.findall(r"∀\s*[a-z_][a-z0-9_]*\s*∈", code)),
        "borrow and move marks": (tilde - binder) + len(re.findall(r"▾", code)),
        "clone": len(re.findall(r"\bclone\(", code)),
        "mutable": binder,
        "types at bindings": written,
        "type arguments": 0,
        "proof lines": proof_lines,
        "impossible": len(re.findall(r"⊥", code)),
        "uses": len(re.findall(r"\buses\b", code)),
    }


COLUMNS = [
    "lines", "decisions", "labels", "routes", "origin marks", "borrow and move marks",
    "clone", "mutable", "types at bindings", "type arguments", "proof lines",
    "impossible", "uses",
]


def add(total, row):
    for key in COLUMNS:
        total[key] = total.get(key, 0) + row[key]


def main():
    print("| File | Version | " + " | ".join(COLUMNS) + " |")
    print("| --- | --- | " + " | ".join("---:" for _ in COLUMNS) + " |")
    totals = {}
    for group, v1_path, v2_path in PAIRS:
        name = Path(v2_path).stem
        if v1_path:
            row = count_v1((ROOT / v1_path).read_text())
            add(totals.setdefault((group, "v1"), {}), row)
            print(f"| {name} | v1 | " + " | ".join(str(row[k]) for k in COLUMNS) + " |")
        row = count_v2((CORPUS / v2_path).read_text())
        add(totals.setdefault((group, "v2"), {}), row)
        print(f"| {name} | v2 | " + " | ".join(str(row[k]) for k in COLUMNS) + " |")
    print()
    print("| Group | Version | " + " | ".join(COLUMNS) + " |")
    print("| --- | --- | " + " | ".join("---:" for _ in COLUMNS) + " |")
    for (group, version), row in totals.items():
        print(f"| {group} | {version} | " + " | ".join(str(row[k]) for k in COLUMNS) + " |")
    over = []
    for _, _, v2_path in PAIRS:
        for number, line in enumerate((CORPUS / v2_path).read_text().splitlines(), 1):
            if len(line) > 100:
                over.append(f"{v2_path}:{number}: {len(line)} columns")
    if over:
        print("\nLines over 100 columns:", *over, sep="\n", file=sys.stderr)


if __name__ == "__main__":
    main()
