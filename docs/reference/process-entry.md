# Executable outcomes and process status

Since compiler 0.48.0, native executables require an explicit process status for
every alternative in their entry contract. Status metadata belongs to the host
boundary; it does not classify an outcome as a language-level error or change its
payload, assignability, ownership, or handling obligation.

## Declaration defaults

<!-- tellur:check -->
```tellur
outcome command_result {
    @exit 0 success,
    @exit 2 invalid_arguments { string message },
    @exit 1 failed { string message }
}

control main() exits command_result {
    exit success;
}
```

`@exit N` immediately precedes an alternative in a named `outcome` declaration.
`N` is an unsuffixed decimal integer from 0 through 255. An alternative can have
at most one annotation. For example, this out-of-range declaration is rejected:

<!-- tellur:reject E_EXIT_CODE -->
```tellur
outcome invalid_status { @exit 256 done }
```

Annotations on ordinary `variant` declarations, inline
control contracts, or a whole outcome declaration are not supported. A standalone
`outcome success {}` is not a singleton alternative: it is an empty contract and
remains invalid.

Annotations are optional in libraries. A nested call returning `failed` simply
returns to its caller, even when `failed` has a process-status annotation. It never
terminates the process. Different contracts and modules can use the same outcome
labels and status numbers. Numbers need not be unique, and the compiler never
derives a status from a name or hash.

Named contract instances retain their defaults through generic specialization and
module imports. `exits auto` infers outcome shapes, not a producer's process policy.
Use a named final contract or an explicit entry mapping for such an executable.

## Entry-specific mappings

A manifest can supply or override statuses for a control used as an executable
entry. This example maps an inline `get` contract with `done`, `missing`, and
`failed` alternatives:

```json
{
  "launch": {
    "get": {
      "exit_codes": {
        "done": 0,
        "missing": 3,
        "failed": 1
      }
    }
  }
}
```

This is a fragment of the existing manifest, alongside `version`, `types`, and
`primitives`. Keys identify entry controls in the compiled root source, such as `main` or
`get`. To expose an imported operation, declare a root entry that calls or forwards
it. Per-entry entries override declaration defaults for the same labels;
unmentioned labels keep their declaration defaults. After merging, every declared
alternative must have a status, including alternatives currently unreachable in
the body. Several alternatives can intentionally map to the same status.

Native generation validates the selected entry and every explicitly configured
entry. Unknown controls, borrowed entry parameters, unknown alternatives, and
missing mappings are `E_EXIT_CODE` errors. Invalid JSON fields, duplicate policy
keys, and out-of-range manifest numbers are manifest errors. Ordinary library
checking does not require complete process mappings; `check` and the LSP validate
annotation syntax but executable-entry validation happens during native generation.

The Rust embedding API exposes `CheckedProgram::entry_exit_codes` for resolving
and validating a mapping, and `with_entry_exit_codes` for supplying one explicitly.

## Final-contract semantics

Only the final entry contract determines process status. If an adapter converts
`missing` to `success`, `success`'s mapping applies. Even identity forwarding into
a differently annotated compatible contract uses the destination contract's
defaults. Capturing or moving a result carries no hidden process-status metadata.
Certified expansion preserves the original entry's mapping.

A generated executable exposes controls with complete mappings and supported
entry parameters through `--entry`; `--help` lists them. Other controls remain
callable from Tellur but cannot be selected as process entries. Status mappings do
not relax the launcher's existing argument-type validation.

Normal text output and `--json` still report the complete final outcome and payload.
`--scenario` uses the same status policy. `tellurc exec` propagates the generated
program's numeric status, while `tellurc run` remains an interpreter/scenario tool:
its command status reports whether interpretation completed successfully.

Launcher/runtime faults still report a diagnostic on stderr and status 1; they do
not become declared outcomes. Application status 1 is also permitted, so status
alone does not distinguish a domain outcome from a runtime fault. Foreign panics
retain their existing fatal behavior. `--help` returns 0. Returning a mapped status
occurs after execution and result reporting, allowing normal runtime cleanup.

## Migration and future execution

Before 0.48, all normal completion outcomes returned process status 0. Existing
executables must now declare their policy explicitly. The maintained playgrounds
and [settings application](../../applications/settings/README.md) have been migrated.

Future asynchronous launchers can apply this same mapping after receiving the
entry's final outcome. These annotations define neither an executor nor a coroutine
ABI, and do not yet define cancellation or shutdown behavior.
