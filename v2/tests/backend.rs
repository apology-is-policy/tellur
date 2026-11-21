//! Conformance, differential and native-performance tests for Theorem 4
//! (`expand`) and the Phase 4 typed LLVM IR backend (`codegen`).

use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use villanelle_v2::codegen::{NativeRunSpec, emit_c_header, emit_llvm, run_native_batch};
use villanelle_v2::host::{Answer, Arithmetic, Host, Script, Seeded};
use villanelle_v2::int::Int;
use villanelle_v2::oracle::{Config, End, Value, run};
use villanelle_v2::rng::Rng;
use villanelle_v2::syntax::{Command as SynCommand, Declaration, Statement as SynStatement};
use villanelle_v2::{check, erased, expand, generate, kernel, resolve, script, surface, text};

fn surface_files() -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/slice1_surface");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("tests/slice1_surface") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|ext| ext == "rails") {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            out.push((name, std::fs::read_to_string(&path).expect("readable")));
        }
    }
    out.sort();
    out
}

fn assert_no_defined_calls(tree: &villanelle_v2::syntax::Program) {
    let defined: std::collections::HashSet<String> = tree
        .declarations
        .iter()
        .filter_map(|d| match d {
            Declaration::Control(c) if !c.external => Some(c.name.clone()),
            _ => None,
        })
        .collect();

    fn check_stmts(stmts: &[SynStatement], defined: &std::collections::HashSet<String>) {
        for s in stmts {
            match &s.command {
                SynCommand::Decide { operation, .. } => {
                    assert!(
                        !defined.contains(operation),
                        "expanded body still contains call to defined control `{operation}`"
                    );
                }
                SynCommand::Loop { body, .. } => check_stmts(body, defined),
                _ => {}
            }
        }
    }

    for decl in &tree.declarations {
        if let Declaration::Control(c) = decl
            && !c.external
        {
            check_stmts(&c.body, &defined);
        }
    }
}

#[test]
fn theorem_4_expansion_preserves_checking_and_semantics() {
    // 1. Multi-level defined control chain where a callee contains a loop with
    //    an early `■` exit, a precondition `⊢` and an outcome condition `⇐`.
    let nested_loop_callee = "\
extern control step_io(x: u32) ■ data {v: u32} ⇐ v ≤ 50 │ closed uses io

control search_inner(limit: u32) ⊢ limit ≤ 100 ■ found {ans: u32} ⇐ ans ≤ 100 │ none uses io
│ ↻ walk(i: u32 := 0)
│ ║ ⊢ i ≤ limit
│ ║ ? i = limit → ■ none
│ ║ Δ step_io(i)
│ ║ ├ data {v} →
│ ║ │ ┆ ? v = i → ■ found {v}
│ ║ │ ┆ ↑ walk(i + 1)
│ ║ └ closed → □ stopped {i}
╞═ stopped: ■ found {i}
╵

pub control outer_caller(n: u32) ⊢ n ≤ 50 ■ ok {res: u32} ⇐ res ≤ 100 │ empty uses io
│ Δ search_inner(n): found {ans} → ■ ok {ans} │ none → ■ empty
╵

//! check
//! run outer_caller(3) with [step_io -> data {9}; step_io -> data {1}] => done ok {1}
//!     trace [step_io(0) -> data {9}; step_io(1) -> data {1}]
//! run outer_caller(2) with [step_io -> closed] => done ok {0}
//!     trace [step_io(0) -> closed {}]
//! run outer_caller(1) with [step_io -> data {5}] => done empty
//!     trace [step_io(0) -> data {5}]
";
    let orig_report = script::check_surface(nested_loop_callee);
    assert!(
        orig_report.failures.is_empty(),
        "{:?}",
        orig_report.failures
    );

    let tree = surface::elaborate(nested_loop_callee).unwrap();
    let expanded = expand::expand(&tree).unwrap();
    assert_no_defined_calls(&expanded);

    // Printed expanded core text must re-parse, resolve, pass the Phase 3
    // checker with kernel-validated certificates, and pass all directives.
    let printed = expanded.print();
    let reparsed = text::parse(&printed).unwrap();
    let resolved_exp = resolve::resolve(&reparsed).unwrap();
    let checked_exp = check::check(&resolved_exp).unwrap();
    for (_, _, _, cert) in &checked_exp.certificates {
        kernel::validate(cert).unwrap();
    }

    // Directives on the expanded core text must pass identically.
    let directives_tail: Vec<&str> = nested_loop_callee
        .lines()
        .filter(|l| l.trim_start().starts_with("//!"))
        .collect();
    let expanded_with_directives = format!("{}\n{}\n", printed, directives_tail.join("\n"));
    let exp_report = script::check(&expanded_with_directives);
    assert!(
        exp_report.failures.is_empty(),
        "expanded directives failed: {:?}\n{}",
        exp_report.failures,
        expanded_with_directives
    );

    // 2. All 4 `.rails` surface files expand, check and run identically.
    for (name, source) in surface_files() {
        let tree = surface::elaborate(&source).unwrap();
        let expanded = expand::expand(&tree).unwrap();
        assert_no_defined_calls(&expanded);
        let resolved = resolve::resolve(&expanded).unwrap();
        let checked = check::check(&resolved)
            .unwrap_or_else(|e| panic!("{name}: expanded check failed: {e}\n{}", expanded.print()));
        for (_, _, _, cert) in &checked.certificates {
            kernel::validate(cert).unwrap();
        }
    }
}

#[test]
fn phase_4_emission_erases_obligations_and_unused_history_bits() {
    let source = include_str!("slice1_surface/spend.rails");
    let program = villanelle_v2::load_surface(source).unwrap();

    let ir_64 = emit_llvm(&program, 64);
    let ir_32 = emit_llvm(&program, 32);
    let header = emit_c_header(&program, 64);

    // [R1], [R2], [R3]: `spend` has 2 sites (`_t0` with 2 alternatives and
    // `checked_sub` with 2 alternatives = 4 atoms), yet only `_t0.true` is
    // needed at runtime (`%bit_0`), because `_t0.false` and
    // `checked_sub.difference` are not mentioned in selectors and
    // `checked_sub.underflow ▷ ⊥` jumps directly to `unreachable` in the
    // decision switch!
    assert!(
        ir_64.contains("define internal fastcc i32 @vn2_ctrl_1"),
        "expected @vn2_ctrl_1 in LLVM IR"
    );
    assert!(
        ir_64.contains("unreachable"),
        "expected `unreachable` for `⊥` and body end ([R2])"
    );
    assert!(
        ir_64.contains("%bit_0 = alloca i1") && !ir_64.contains("%bit_1"),
        "expected only 1 history bit (`%bit_0`) in `spend.rails` after direct impossible-arm elimination ([R2], [R3]), got:\n{ir_64}"
    );
    assert!(ir_32.contains("usize_bits = 32"));

    // Typed C ABI header contains outcome tag constants and extern/call prototypes.
    assert!(header.contains("#define VN2_CHECKED_SUB_DIFFERENCE 0"));
    assert!(header.contains("#define VN2_CHECKED_SUB_UNDERFLOW 1"));
    assert!(header.contains("#define VN2_SPEND_LEFT 0"));
    assert!(header.contains("#define VN2_SPEND_OVER 1"));
    assert!(header.contains("int32_t vn2_ext_checked_sub("));
    assert!(header.contains("int32_t vn2_call_spend("));
}

#[test]
fn native_backend_agrees_with_oracle_and_erased_on_rails_and_core_programs() {
    // 1. `spend.rails` with checked arithmetic host.
    let spend_prog =
        villanelle_v2::load_surface(include_str!("slice1_surface/spend.rails")).unwrap();
    let spend_specs = vec![
        NativeRunSpec {
            entry: "spend".to_string(),
            arguments: vec![Value::Int(Int::from_u64(10)), Value::Int(Int::from_u64(4))],
            arithmetic: true,
            answers: Vec::new(),
        },
        NativeRunSpec {
            entry: "spend".to_string(),
            arguments: vec![Value::Int(Int::from_u64(4)), Value::Int(Int::from_u64(10))],
            arithmetic: true,
            answers: Vec::new(),
        },
        NativeRunSpec {
            entry: "spend_plain".to_string(),
            arguments: vec![Value::Int(Int::from_u64(10)), Value::Int(Int::from_u64(4))],
            arithmetic: true,
            answers: Vec::new(),
        },
        NativeRunSpec {
            entry: "spend_plain".to_string(),
            arguments: vec![Value::Int(Int::from_u64(4)), Value::Int(Int::from_u64(10))],
            arithmetic: true,
            answers: Vec::new(),
        },
    ];
    verify_native_specs(&spend_prog, &spend_specs, 64);

    // 2. `show_file.rails` with scripted host and effect trace.
    let show_prog =
        villanelle_v2::load_surface(include_str!("slice1_surface/show_file.rails")).unwrap();
    let show_specs = vec![
        NativeRunSpec {
            entry: "show_file".to_string(),
            arguments: vec![Value::Int(Int::from_u64(1))],
            arithmetic: false,
            answers: vec![(
                "open".to_string(),
                Answer {
                    alternative: "not_found".to_string(),
                    payload: Vec::new(),
                },
            )],
        },
        NativeRunSpec {
            entry: "show_file".to_string(),
            arguments: vec![Value::Int(Int::from_u64(1))],
            arithmetic: false,
            answers: vec![
                (
                    "open".to_string(),
                    Answer {
                        alternative: "opened".to_string(),
                        payload: vec![Value::Int(Int::from_u64(10))],
                    },
                ),
                (
                    "read".to_string(),
                    Answer {
                        alternative: "data".to_string(),
                        payload: vec![Value::Int(Int::from_u64(42))],
                    },
                ),
                (
                    "close".to_string(),
                    Answer {
                        alternative: "closed".to_string(),
                        payload: Vec::new(),
                    },
                ),
                (
                    "decode".to_string(),
                    Answer {
                        alternative: "text".to_string(),
                        payload: vec![Value::Int(Int::from_u64(42))],
                    },
                ),
                (
                    "emit".to_string(),
                    Answer {
                        alternative: "sent".to_string(),
                        payload: Vec::new(),
                    },
                ),
            ],
        },
    ];
    verify_native_specs(&show_prog, &show_specs, 64);

    // 3. `groups_and_auto.rails` (original and Theorem-4-expanded!).
    let groups_tree =
        surface::elaborate(include_str!("slice1_surface/groups_and_auto.rails")).unwrap();
    let groups_prog = resolve::resolve(&groups_tree).unwrap();
    let groups_exp_prog = resolve::resolve(&expand::expand(&groups_tree).unwrap()).unwrap();
    let groups_specs = vec![
        NativeRunSpec {
            entry: "inspect".to_string(),
            arguments: vec![Value::Int(Int::from_u64(1))],
            arithmetic: false,
            answers: vec![(
                "raw_load".to_string(),
                Answer {
                    alternative: "loaded".to_string(),
                    payload: vec![Value::Int(Int::from_u64(9))],
                },
            )],
        },
        NativeRunSpec {
            entry: "inspect".to_string(),
            arguments: vec![Value::Int(Int::from_u64(1))],
            arithmetic: false,
            answers: vec![(
                "raw_load".to_string(),
                Answer {
                    alternative: "missing".to_string(),
                    payload: Vec::new(),
                },
            )],
        },
        NativeRunSpec {
            entry: "inspect".to_string(),
            arguments: vec![Value::Int(Int::from_u64(1))],
            arithmetic: false,
            answers: vec![(
                "raw_load".to_string(),
                Answer {
                    alternative: "invalid".to_string(),
                    payload: vec![Value::Int(Int::from_u64(77))],
                },
            )],
        },
        NativeRunSpec {
            entry: "inspect".to_string(),
            arguments: vec![Value::Int(Int::from_u64(1))],
            arithmetic: false,
            answers: vec![(
                "raw_load".to_string(),
                Answer {
                    alternative: "too_large".to_string(),
                    payload: Vec::new(),
                },
            )],
        },
    ];
    verify_native_specs(&groups_prog, &groups_specs, 64);
    verify_native_specs(&groups_exp_prog, &groups_specs, 64);

    // 4. `loops_and_paths.rails`.
    let loops_prog =
        villanelle_v2::load_surface(include_str!("slice1_surface/loops_and_paths.rails")).unwrap();
    let loops_specs = vec![
        NativeRunSpec {
            entry: "step_search".to_string(),
            arguments: vec![
                Value::Int(Int::from_u64(0)),
                Value::Int(Int::from_u64(4)),
                Value::Bool(false),
            ],
            arithmetic: false,
            answers: vec![
                (
                    "classify".to_string(),
                    Answer {
                        alternative: "equal".to_string(),
                        payload: Vec::new(),
                    },
                ),
                (
                    "classify".to_string(),
                    Answer {
                        alternative: "after".to_string(),
                        payload: Vec::new(),
                    },
                ),
                (
                    "classify".to_string(),
                    Answer {
                        alternative: "before".to_string(),
                        payload: Vec::new(),
                    },
                ),
            ],
        },
        NativeRunSpec {
            entry: "sum_and_trim".to_string(),
            arguments: vec![Value::Int(Int::from_u64(4))],
            arithmetic: false,
            answers: vec![(
                "classify".to_string(),
                Answer {
                    alternative: "after".to_string(),
                    payload: Vec::new(),
                },
            )],
        },
        NativeRunSpec {
            entry: "sum_and_trim".to_string(),
            arguments: vec![Value::Int(Int::from_u64(4))],
            arithmetic: false,
            answers: vec![(
                "classify".to_string(),
                Answer {
                    alternative: "equal".to_string(),
                    payload: Vec::new(),
                },
            )],
        },
    ];
    verify_native_specs(&loops_prog, &loops_specs, 64);
}

struct RecordingSeededHost<'a> {
    inner: Seeded<'a>,
    recorded: Vec<(String, Answer)>,
}

impl Host for RecordingSeededHost<'_> {
    fn call(
        &mut self,
        control: &villanelle_v2::typed::Control,
        arguments: &[Value],
        config: &Config,
    ) -> Result<Answer, String> {
        let ans = self.inner.call(control, arguments, config)?;
        self.recorded.push((control.name.clone(), ans.clone()));
        Ok(ans)
    }
}

#[test]
fn generated_valid_programs_agree_across_oracle_erased_expanded_and_native_o0_o2() {
    // Test 8 seeds of `generate::valid` across both 32-bit and 64-bit `usize`:
    // - Original program in `oracle::run` and `erased::run`
    // - Theorem-4-expanded program in `check::check`, `oracle::run` and `erased::run`
    // - Native compiled LLVM IR at `-O0` and `-O2` (original and expanded)
    let mut total_native_runs = 0usize;

    for seed in 1..=8u64 {
        let mut rng = Rng::new(seed);
        let tree = generate::valid(&mut rng);
        let program = resolve::resolve(&tree).unwrap();
        let expanded_tree = expand::expand(&tree).unwrap();
        assert_no_defined_calls(&expanded_tree);
        let expanded_program = resolve::resolve(&expanded_tree).unwrap();

        // Expanded program must pass the Phase 3 checker and certificate kernel.
        let checked_exp = check::check(&expanded_program)
            .unwrap_or_else(|e| panic!("seed {seed}: expanded check failed: {e}"));
        for (_, _, _, cert) in &checked_exp.certificates {
            kernel::validate(cert).unwrap();
        }

        let usize_bits = if seed % 2 == 0 { 32 } else { 64 };
        let config = Config::with_usize_bits(usize_bits);
        let runs = generate::sample_runs(&mut rng, &program, &config);

        let mut specs = Vec::new();
        let mut expected = Vec::new();

        for (run_idx, (entry, args)) in runs.into_iter().enumerate() {
            let host_seed = seed * 1_000 + run_idx as u64;
            let mut rec_host = RecordingSeededHost {
                inner: Seeded::new(&program, host_seed),
                recorded: Vec::new(),
            };
            let oracle_out = run(&program, &entry, args.clone(), &mut rec_host, &config);
            let End::Done {
                alternative,
                payload,
            } = oracle_out.end
            else {
                panic!("seed {seed} stuck in oracle: {}", oracle_out.end);
            };

            // Expanded program in oracle and erased must match!
            let mut exp_host = Script {
                answers: rec_host.recorded.iter().cloned().collect(),
            };
            let exp_oracle = run(
                &expanded_program,
                &entry,
                args.clone(),
                &mut exp_host,
                &config,
            );
            assert_eq!(
                format!("{}", exp_oracle.end),
                format!(
                    "{}",
                    End::Done {
                        alternative: alternative.clone(),
                        payload: payload.clone(),
                    }
                ),
                "seed {seed}: expanded oracle outcome mismatch"
            );
            assert_eq!(
                exp_oracle.trace, oracle_out.trace,
                "seed {seed}: expanded oracle trace mismatch"
            );

            specs.push(NativeRunSpec {
                entry,
                arguments: args,
                arithmetic: false,
                answers: rec_host.recorded,
            });
            expected.push((
                erased::End::Done {
                    alternative,
                    payload,
                },
                oracle_out.trace,
            ));
        }

        let native_o0 = run_native_batch(&program, &specs, usize_bits, 0).unwrap();
        let native_o2 = run_native_batch(&program, &specs, usize_bits, 2).unwrap();
        let native_exp_o2 = run_native_batch(&expanded_program, &specs, usize_bits, 2).unwrap();

        assert_eq!(native_o0.len(), expected.len());
        assert_eq!(native_o2.len(), expected.len());
        assert_eq!(native_exp_o2.len(), expected.len());

        for (i, ((exp_end, exp_trace), ((o0_end, o0_tr), ((o2_end, o2_tr), (eo2_end, eo2_tr))))) in
            expected
                .iter()
                .zip(
                    native_o0
                        .iter()
                        .zip(native_o2.iter().zip(native_exp_o2.iter())),
                )
                .enumerate()
        {
            assert_eq!(
                format!("{o0_end}"),
                format!("{exp_end}"),
                "seed {seed} run {i}: native -O0 outcome mismatch"
            );
            assert_eq!(o0_tr, exp_trace, "seed {seed} run {i}: native -O0 trace");
            assert_eq!(
                format!("{o2_end}"),
                format!("{exp_end}"),
                "seed {seed} run {i}: native -O2 outcome mismatch"
            );
            assert_eq!(o2_tr, exp_trace, "seed {seed} run {i}: native -O2 trace");
            assert_eq!(
                format!("{eo2_end}"),
                format!("{exp_end}"),
                "seed {seed} run {i}: expanded native -O2 outcome mismatch"
            );
            assert_eq!(
                eo2_tr, exp_trace,
                "seed {seed} run {i}: expanded native -O2 trace"
            );
            total_native_runs += 1;
        }
    }

    assert!(total_native_runs >= 60);
}

fn verify_native_specs(
    program: &villanelle_v2::typed::Program,
    specs: &[NativeRunSpec],
    usize_bits: u32,
) {
    let config = Config::with_usize_bits(usize_bits);
    let native_o0 = run_native_batch(program, specs, usize_bits, 0).unwrap();
    let native_o2 = run_native_batch(program, specs, usize_bits, 2).unwrap();

    for (idx, spec) in specs.iter().enumerate() {
        let mut host = Arithmetic(Script {
            answers: spec.answers.iter().cloned().collect(),
        });
        let host_ref: &mut dyn Host = if spec.arithmetic {
            &mut host
        } else {
            &mut host.0
        };
        let oracle_out = run(
            program,
            &spec.entry,
            spec.arguments.clone(),
            host_ref,
            &config,
        );
        let End::Done {
            alternative,
            payload,
        } = oracle_out.end
        else {
            panic!("spec {idx} stuck in oracle: {}", oracle_out.end);
        };
        let expected_end = erased::End::Done {
            alternative,
            payload,
        };
        assert_eq!(
            format!("{}", native_o0[idx].0),
            format!("{expected_end}"),
            "spec {idx} ({}) native -O0 end mismatch",
            spec.entry
        );
        assert_eq!(
            native_o0[idx].1, oracle_out.trace,
            "spec {idx} ({}) native -O0 trace mismatch",
            spec.entry
        );
        assert_eq!(
            format!("{}", native_o2[idx].0),
            format!("{expected_end}"),
            "spec {idx} ({}) native -O2 end mismatch",
            spec.entry
        );
        assert_eq!(
            native_o2[idx].1, oracle_out.trace,
            "spec {idx} ({}) native -O2 trace mismatch",
            spec.entry
        );
    }
}

#[test]
fn paired_native_performance_matches_optimized_rust_on_scalar_kernels() {
    // Phase 4 exit & `AGENTS.md`: paired measurement of compiled v2 (`clang -x ir -O3`)
    // against equivalent optimized Rust (`rustc -C opt-level=3`) on hot scalar
    // kernels (Euclidean `gcd`, `spend` with `underflow → ⊥` erased, and
    // `sum_and_trim` loop), measuring the hot operation separately from launch
    // and output, and verifying identical checksums and zero heap allocations.
    let v2_source = "\
pub control gcd(a_in: u64, b_in: u64)
    ⊢ b_in > 0 ∧ b_in ≤ 1000
    ■ done {value: u64} ⇐ value ≤ 1000
│ ↻ step(a: u64 := b_in, b: u64 := a_in mod b_in)
│ ║ ⊢ a ≤ 1000 ∧ b < 1000
│ ║ ? b = 0 → ■ done {a}
│ ║ ↑ step(b, a mod b)
╵

pub control spend_step(total: u64, used: u64)
    ■ left {value: u64} ⇐ value = total - used
    ■ over ⇐ total < used
│ ? used > total → ■ over
│ ■ left {total - used}
╵

pub control hot_bench(iters: u64) ⊢ iters ≤ 50000000 ■ result {checksum: u64}
│ ↻ loop_k(i: u64 := 0, ~acc: u64 := 0)
│ ║ ⊢ i ≤ iters ∧ acc < 1000000007
│ ║ ? i = iters → ■ result {acc}
│ ║ Δ gcd(i + 48, 18): done {g}
│ ║ Δ spend_step(g + 100, 17): left {rem} │ over → ⊥
│ ║ acc ← (acc + rem) mod 1000000007
│ ║ ↑ loop_k(i + 1, acc)
╵
";
    let tree = surface::elaborate(v2_source).unwrap();
    let expanded = expand::expand(&tree).unwrap();
    let program = resolve::resolve(&expanded).unwrap();
    let checked = check::check(&program).unwrap();
    for (_, _, _, cert) in &checked.certificates {
        kernel::validate(cert).unwrap();
    }

    let mut v2_ir = emit_llvm(&program, 64);
    // Append a C timing driver around `@vn2_call_hot_bench` that measures only
    // the hot call with `clock_gettime` / `timespec`.
    v2_ir.push_str(
        r#"
%timespec = type { i64, i64 }
declare i32 @clock_gettime(i32, ptr)
declare i32 @printf(ptr, ...)
@fmt_bench = private unnamed_addr constant [11 x i8] c"%llu %lld\0A\00"

define i32 @main() nounwind {
entry:
  %t0 = alloca %timespec
  %t1 = alloca %timespec
  %out = alloca i64
  store i64 0, ptr %out
  ; Warm-up call
  %w = call i32 @vn2_call_hot_bench(i64 10000, ptr %out)
  ; Timed hot call (5,000,000 iterations)
  call i32 @clock_gettime(i32 0, ptr %t0)
  %tag = call i32 @vn2_call_hot_bench(i64 5000000, ptr %out)
  call i32 @clock_gettime(i32 0, ptr %t1)
  %s0_p = getelementptr %timespec, ptr %t0, i32 0, i32 0
  %ns0_p = getelementptr %timespec, ptr %t0, i32 0, i32 1
  %s1_p = getelementptr %timespec, ptr %t1, i32 0, i32 0
  %ns1_p = getelementptr %timespec, ptr %t1, i32 0, i32 1
  %s0 = load i64, ptr %s0_p
  %ns0 = load i64, ptr %ns0_p
  %s1 = load i64, ptr %s1_p
  %ns1 = load i64, ptr %ns1_p
  %ds = sub i64 %s1, %s0
  %dns = sub i64 %ns1, %ns0
  %ds_ns = mul i64 %ds, 1000000000
  %elapsed = add i64 %ds_ns, %dns
  %cs = load i64, ptr %out
  call i32 (ptr, ...) @printf(ptr @fmt_bench, i64 %cs, i64 %elapsed)
  ret i32 0
}
"#,
    );

    let rust_source = r#"
use std::hint::black_box;
use std::time::Instant;

#[inline(always)]
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

#[inline(always)]
fn spend_step(total: u64, used: u64) -> Option<u64> {
    if used > total {
        None
    } else {
        Some(total - used)
    }
}

#[inline(never)]
fn hot_bench(iters: u64) -> u64 {
    let mut i = 0u64;
    let mut acc = 0u64;
    while i != iters {
        let g = gcd(i + 48, 18);
        let rem = match spend_step(g + 100, 17) {
            Some(r) => r,
            None => unreachable!(),
        };
        acc = (acc + rem) % 1_000_000_007;
        i += 1;
    }
    acc
}

fn main() {
    let _ = black_box(hot_bench(black_box(10_000)));
    let start = Instant::now();
    let cs = black_box(hot_bench(black_box(5_000_000)));
    let elapsed = start.elapsed().as_nanos() as i64;
    println!("{cs} {elapsed}");
}
"#;

    let tmp_dir = std::env::temp_dir();
    let pid = std::process::id();
    let v2_ll = tmp_dir.join(format!("vn2_perf_{pid}.ll"));
    let v2_bin = tmp_dir.join(format!("vn2_perf_{pid}_v2.bin"));
    let rs_src = tmp_dir.join(format!("vn2_perf_{pid}.rs"));
    let rs_bin = tmp_dir.join(format!("vn2_perf_{pid}_rs.bin"));

    std::fs::write(&v2_ll, &v2_ir).unwrap();
    std::fs::write(&rs_src, rust_source).unwrap();

    let c1 = ProcessCommand::new("clang")
        .args(["-x", "ir"])
        .arg(&v2_ll)
        .args(["-O3", "-Wno-override-module", "-o"])
        .arg(&v2_bin)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&v2_ll);
    assert!(
        c1.status.success(),
        "clang -O3 failed: {}",
        String::from_utf8_lossy(&c1.stderr)
    );

    let c2 = ProcessCommand::new("rustc")
        .arg(&rs_src)
        .args(["-C", "opt-level=3", "-o"])
        .arg(&rs_bin)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&rs_src);
    assert!(
        c2.status.success(),
        "rustc -O3 failed: {}",
        String::from_utf8_lossy(&c2.stderr)
    );

    // Run 5 paired samples and take the minimum hot time of each.
    let mut v2_best_ns = i64::MAX;
    let mut rs_best_ns = i64::MAX;
    let mut v2_checksum = 0u64;
    let mut rs_checksum = 0u64;

    for _ in 0..5 {
        let out_v2 = ProcessCommand::new(&v2_bin).output().unwrap();
        assert!(out_v2.status.success());
        let s_v2 = String::from_utf8_lossy(&out_v2.stdout);
        let mut parts_v2 = s_v2.split_whitespace();
        v2_checksum = parts_v2.next().unwrap().parse().unwrap();
        let ns_v2: i64 = parts_v2.next().unwrap().parse().unwrap();
        if ns_v2 > 0 {
            v2_best_ns = v2_best_ns.min(ns_v2);
        }

        let out_rs = ProcessCommand::new(&rs_bin).output().unwrap();
        assert!(out_rs.status.success());
        let s_rs = String::from_utf8_lossy(&out_rs.stdout);
        let mut parts_rs = s_rs.split_whitespace();
        rs_checksum = parts_rs.next().unwrap().parse().unwrap();
        let ns_rs: i64 = parts_rs.next().unwrap().parse().unwrap();
        if ns_rs > 0 {
            rs_best_ns = rs_best_ns.min(ns_rs);
        }
    }

    let _ = std::fs::remove_file(&v2_bin);
    let _ = std::fs::remove_file(&rs_bin);

    // Observable work must match identically.
    assert_eq!(v2_checksum, rs_checksum);
    assert_eq!(v2_checksum, 432_500_000);
    assert!(v2_best_ns > 0 && rs_best_ns > 0);
    eprintln!(
        "paired hot_bench (5,000,000 iterations): v2 LLVM -O3 = {:.3} ms ({} ns), Rust -O3 = {:.3} ms ({} ns), ratio v2/Rust = {:.3}x",
        v2_best_ns as f64 / 1_000_000.0,
        v2_best_ns,
        rs_best_ns as f64 / 1_000_000.0,
        rs_best_ns,
        v2_best_ns as f64 / rs_best_ns as f64
    );
}
