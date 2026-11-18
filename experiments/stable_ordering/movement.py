"""Model suffix movement and run-header reversal for the mixed stable-sort workload.

Counts describe logical value slots, not measured memory-bus traffic. Input/output
transfers, buffer growth, record fields and control/ABI transport are excluded.
"""
import argparse
import json

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--items', type=int, default=1024)
p.add_argument('--value-bytes', type=int, default=136, help='measured generic V size on this target')
a = p.parse_args()
if a.items < 0 or a.value_bytes < 0:
    p.error('counts must be nonnegative')
values = [((i * 37 + 11) % 31, i) for i in range(a.items)]
runs = [[item] for item in values]
comparisons = remaining = swaps = previous_headers = 0
while len(runs) > 1:
    swaps += len(runs) // 2
    previous_headers += len(runs) * (len(runs) - 1) // 2
    output = []
    for k in range(0, len(runs), 2):
        if k + 1 == len(runs):
            output.append(runs[k])
            continue
        left, right = runs[k:k + 2]
        i = j = 0
        merged = []
        while i < len(left) and j < len(right):
            comparisons += 1
            if left[i][0] <= right[j][0]:
                merged.append(left[i]); i += 1
            else:
                merged.append(right[j]); j += 1
        remaining += (len(left) - i if i else 0) + (len(right) - j if j else 0)
        output.append(merged + left[i:] + right[j:])
    runs = output
assert (runs[0] if runs else []) == sorted(values)
print(json.dumps({
    'items': a.items, 'comparisons': comparisons,
    'merge_suffix_slots': remaining, 'singleton_suffix_slots': 0,
    'run_header_suffix_slots': 0, 'previous_run_header_suffix_slots': previous_headers,
    'run_header_swaps': swaps, 'run_header_positions_rewritten': 2 * swaps,
    'generic_value_bytes': a.value_bytes, 'suffix_bytes': remaining * a.value_bytes,
    'previous_suffix_bytes': (remaining + previous_headers) * a.value_bytes,
    'scope': __doc__.split('\n\n')[1].strip(),
}, indent=2))
