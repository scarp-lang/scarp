use scarp::{
    diagnostics::DiagnosticKind,
    native_object::{
        emit_elf64_bounded_integer_entry, emit_elf64_bounded_integer_entry_with_layout,
        emit_elf64_bounded_integer_function, BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES,
    },
};

const EXAMPLE: &str = include_str!("../examples/native_direct_calls.srp");

fn definitions() -> &'static str {
    EXAMPLE.split_once("\nprint(").unwrap().0
}

fn wide_definitions(arity: usize) -> String {
    let list =
        |part: &dyn Fn(usize) -> String| (1..=arity).map(part).collect::<Vec<_>>().join(", ");
    let params = list(&|index| format!("p{index}: integer"));
    let args = list(&|index| format!("p{index}"));
    let named = list(&|index| {
        let parameter = arity + 1 - index;
        format!("p{parameter}: p{parameter}")
    });
    let weighted = (1..=arity)
        .map(|index| format!("p{index} * {index}"))
        .collect::<Vec<_>>()
        .join(" + ");
    format!(
        r#"pure funcra dispatch({params}) -> integer {{
            var answer = p1 + 7;
            repeat 2 as pass {{
                answer += if pass == 0 {{ relay({args}) }} else {{ wide({args}) }};
            }}
            return answer + p{arity};
        }}
        pure funcra relay({params}) -> integer {{ return p1 + wide({named}); }}
        pure funcra wide({params}) -> integer {{ return {weighted}; }}"#
    )
}

#[test]
fn direct_object_is_deterministic_and_exports_only_the_selected_entry() {
    let source = definitions();
    let compiled = emit_elf64_bounded_integer_entry(source, "evaluate", 1000).unwrap();
    assert_eq!(
        compiled,
        emit_elf64_bounded_integer_entry(source, "evaluate", 1000).unwrap()
    );
    assert_ne!(
        compiled,
        emit_elf64_bounded_integer_entry(source, "quotient", 1000).unwrap()
    );
    let bytes = &compiled.object;
    let read = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
    let sections = read(40);
    let text_bytes = read(sections + 64 + 32);
    let symbol_offset = read(sections + 2 * 64 + 24);
    let symbol_bytes = read(sections + 2 * 64 + 32);
    let names_offset = read(sections + 3 * 64 + 24);
    let names_bytes = read(sections + 3 * 64 + 32);
    let entry_bytes = read(symbol_offset + 24 + 16);
    assert_eq!(symbol_bytes, 48);
    assert!((1..text_bytes).contains(&entry_bytes));
    assert_eq!(
        &bytes[names_offset..names_offset + names_bytes],
        b"\0scarp_bounded_integer_entry\0"
    );
    assert!(emit_elf64_bounded_integer_function(source, 1000).is_err());

    let mut output = Vec::new();
    scarp::run(EXAMPLE, &mut output).unwrap();
    assert_eq!(
        output,
        include_bytes!("../examples/native_direct_calls.stdout")
    );
    let wide = include_str!("../examples/native_wide_calls.srp");
    output.clear();
    scarp::run(wide, &mut output).unwrap();
    assert_eq!(
        output,
        include_bytes!("../examples/native_wide_calls.stdout")
    );
    let wide = wide.split_once("\nprint(").unwrap().0;
    assert!(emit_elf64_bounded_integer_entry(wide, "entry", 1000).is_ok());
}

#[test]
fn direct_object_omits_unreachable_bodies_after_validating_them() {
    let source = "pure funcra entry(n: integer) -> integer { return middle(n); }
                  pure funcra leaf(n: integer) -> integer { return n + 3; }
                  pure funcra middle(n: integer) -> integer { return leaf(n); }";
    let artifact = emit_elf64_bounded_integer_entry(source, "entry", 100).unwrap();
    let extra = format!("{source}\npure funcra unused() -> integer {{ return entry(0); }}");
    assert_eq!(
        artifact,
        emit_elf64_bounded_integer_entry(&extra, "entry", 100).unwrap()
    );
    let cycle = format!("{source}\npure funcra unused() -> integer {{ return unused(); }}");
    let error = emit_elf64_bounded_integer_entry(&cycle, "entry", 100).unwrap_err();
    assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
    assert!(error.message.contains("nonrecursive"), "{error:?}");
}

#[test]
fn entry_stack_budget_covers_the_deepest_active_chain() {
    let source = "pure funcra entry(n: integer) -> integer { return middle(n) + leaf(n); }
                  pure funcra leaf(n: integer) -> integer { return n + 3; }
                  pure funcra middle(n: integer) -> integer { return leaf(n); }
                  pure funcra unused() -> integer { return entry(0); }";
    let compile =
        |name, budget| emit_elf64_bounded_integer_entry_with_layout(source, name, 100, budget);
    let limit = BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES;
    let entry = compile("entry", limit).unwrap();
    let unused = compile("unused", limit).unwrap();
    let expected_peak: u64 = ["entry", "middle", "leaf"]
        .map(|name| {
            compile(name, limit)
                .unwrap()
                .entry_layout
                .callee_stack_bytes()
        })
        .into_iter()
        .sum();
    assert_eq!(u64::from(entry.peak_stack_bytes), expected_peak);
    assert_eq!(entry.max_active_frames, 3);
    assert_eq!(unused.max_active_frames, 4);
    assert!(unused.peak_stack_bytes > entry.peak_stack_bytes);
    assert_eq!(entry.entry_layout.integer_arguments, 1);
    assert_eq!(entry.entry_layout.stack_argument_bytes(), 0);
    assert_eq!(compile("entry", entry.peak_stack_bytes).unwrap(), entry);
    assert_eq!(
        emit_elf64_bounded_integer_entry(source, "entry", 100).unwrap(),
        entry.kernel,
    );
    for budget in [0, entry.peak_stack_bytes - 1] {
        let error = compile("entry", budget).unwrap_err();
        assert_eq!(error.kind, DiagnosticKind::NativeUnsupported);
        assert!(
            error.message.contains(&format!(
                "requires {} stack bytes, exceeding the caller budget of {budget}",
                entry.peak_stack_bytes
            )),
            "{error:?}"
        );
    }
}

#[test]
fn entry_stack_layout_separates_external_arguments_from_internal_outgoing_areas() {
    let source = wide_definitions(16);
    let limit = BOUNDED_INTEGER_ENTRY_MAX_STACK_BYTES;
    let entry =
        emit_elf64_bounded_integer_entry_with_layout(&source, "dispatch", 2000, limit).unwrap();
    let relay =
        emit_elf64_bounded_integer_entry_with_layout(&source, "relay", 2000, limit).unwrap();
    let leaf = emit_elf64_bounded_integer_entry_with_layout(&source, "wide", 2000, limit).unwrap();
    assert_eq!(entry.entry_layout.integer_arguments, 16);
    assert_eq!(entry.entry_layout.stack_argument_bytes(), 80);
    assert_eq!(entry.max_active_frames, 3);
    assert!(entry.entry_layout.frame_bytes >= 80);
    assert!(relay.entry_layout.frame_bytes >= 80);
    assert_eq!(
        u64::from(entry.peak_stack_bytes),
        entry.entry_layout.callee_stack_bytes()
            + relay.entry_layout.callee_stack_bytes()
            + leaf.entry_layout.callee_stack_bytes(),
    );
}

#[test]
fn straight_line_entry_uses_r11_and_a_smaller_frame() {
    let source = "pure funcra f(x: integer) -> integer { return ~x; }";
    let artifact = emit_elf64_bounded_integer_entry_with_layout(source, "f", 100, 32).unwrap();
    assert_eq!(artifact.entry_layout.frame_bytes, 24);
    assert_eq!(artifact.peak_stack_bytes, 32);
    let bytes = &artifact.kernel.object;
    let read = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
    let header = read(40) + 64;
    let start = read(header + 24);
    let code = &bytes[start..start + read(header + 32)];
    assert_eq!(&code[..6], &[0x41, 0xba, 100, 0, 0, 0]); // mov r10d, 100
    assert_eq!(&code[6..13], &[0x48, 0x81, 0xec, 24, 0, 0, 0]); // sub rsp, 24
    assert!(code.windows(3).any(|bytes| bytes == [0x49, 0x89, 0xc3])); // mov r11, rax
    assert!(code.windows(3).any(|bytes| bytes == [0x4c, 0x89, 0xd8])); // mov rax, r11
}

#[test]
fn constant_entry_fits_the_exact_reduced_stack_budget() {
    let source = "pure funcra f() -> integer { return (2 * 3) + 1; }";
    let artifact = emit_elf64_bounded_integer_entry_with_layout(source, "f", 100, 16).unwrap();
    assert_eq!(artifact.entry_layout.frame_bytes, 8);
    assert_eq!(artifact.peak_stack_bytes, 16);
    let error = emit_elf64_bounded_integer_entry_with_layout(source, "f", 100, 15).unwrap_err();
    assert!(error.message.contains("requires 16 stack bytes"));
}

#[test]
fn direct_objects_reject_cycles_unknown_entries_and_unsupported_signatures() {
    for (source, entry, message) in [
        (
            "pure funcra f() -> integer { return f(); }",
            "f",
            "nonrecursive",
        ),
        (
            "pure funcra f() -> integer { return g(); } pure funcra g() -> integer { return f(); }",
            "f",
            "nonrecursive",
        ),
        (
            "pure funcra f() -> integer { return 1; }",
            "missing",
            "was not declared",
        ),
        (
            "pure funcra f(n: integer = 3) -> integer { return n; }",
            "f",
            "private scalar IR",
        ),
        (
            "funcra f() -> integer { return 1; }",
            "f",
            "private scalar IR",
        ),
        (
            "pure funcra f(n: integer) -> integer { return integer_abs(n); }",
            "f",
            "private scalar IR",
        ),
        (
            "pure funcra f() -> integer { return 1; } print(f());",
            "f",
            "function declarations only",
        ),
    ] {
        let error = emit_elf64_bounded_integer_entry(source, entry, 1000).unwrap_err();
        assert_eq!(
            error.kind,
            DiagnosticKind::NativeUnsupported,
            "{source}: {error:?}"
        );
        assert!(error.message.contains(message), "{source}: {error:?}");
    }
    let too_wide =
        emit_elf64_bounded_integer_entry(&wide_definitions(17), "dispatch", 1000).unwrap_err();
    assert_eq!(too_wide.kind, DiagnosticKind::NativeUnsupported);
    assert!(too_wide.message.contains("sixteen integer"), "{too_wide:?}");
}

#[test]
fn direct_objects_enforce_a_closed_finite_call_graph() {
    let mut declarations = "pure funcra f0() -> integer { return 0; }\n".to_string();
    for id in 1..32 {
        declarations.push_str(&format!(
            "pure funcra f{id}() -> integer {{ return f{}(); }}\n",
            id - 1
        ));
        if id == 15 {
            assert!(emit_elf64_bounded_integer_entry(&declarations, "f15", 1000).is_ok());
        }
        if id == 16 {
            let error = emit_elf64_bounded_integer_entry(&declarations, "f16", 1000).unwrap_err();
            assert!(error.message.contains("16-frame"), "{error:?}");
        }
    }
    declarations.push_str("pure funcra f32() -> integer { return 32; }");
    let error = emit_elf64_bounded_integer_entry(&declarations, "f0", 1000).unwrap_err();
    assert!(error.message.contains("32 functions"), "{error:?}");
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_direct_calls_share_meter_and_propagate_the_first_failure() {
    use scarp::{
        diagnostics::Span,
        native_object::{
            CheckedElf64Kernel, CHECKED_KERNEL_DIVISION_BY_ZERO, CHECKED_KERNEL_INTEGER_OVERFLOW,
            CHECKED_KERNEL_INVALID_SHIFT, CHECKED_KERNEL_WORK_LIMIT,
        },
        RunError,
    };
    use std::{fs, path::PathBuf, process::Command};

    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let cc = std::env::var_os("SCARP_CC").unwrap_or_else(|| "cc".into());
    if Command::new(&cc).arg("--version").output().is_err() {
        assert!(
            std::env::var_os("SCARP_REQUIRE_C").is_none(),
            "C compiler required"
        );
        return;
    }
    let directory =
        Temp(std::env::temp_dir().join(format!("scarp-native-calls-{}", std::process::id())));
    fs::create_dir(&directory.0).unwrap();
    let object = directory.0.join("entry.o");
    let harness = directory.0.join("harness.c");
    let binary = directory.0.join("harness");
    let invoke = |artifact: &CheckedElf64Kernel, arity, arguments: &[i64]| {
        let types = if arity == 0 {
            "void".to_string()
        } else {
            vec!["int64_t"; arity].join(", ")
        };
        let arguments_list = (0..arity)
            .map(|i| format!("strtoll(argv[i + {}], NULL, 10)", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(&object, &artifact.object).unwrap();
        fs::write(
            &harness,
            format!(
                r#"#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
struct result {{ int64_t value; uint32_t status; uint32_t span_id; }};
extern struct result scarp_bounded_integer_entry({types});
int main(int argc, char **argv) {{
    alarm(5);
    (void)argv;
    for (int i = 0; i < argc - 1 || ({arity} == 0 && i < 2); i += {stride}) {{
        struct result r = scarp_bounded_integer_entry({arguments_list});
        printf("%" PRId64 " %" PRIu32 " %" PRIu32 "\n", r.value, r.status, r.span_id);
    }}
    return 0;
}}
"#,
                stride = arity.max(1)
            ),
        )
        .unwrap();
        let link = Command::new(&cc)
            .args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror"])
            .arg(&harness)
            .arg(&object)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let output = Command::new(&binary)
            .args(arguments.iter().map(i64::to_string))
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.status);
        let rows: Vec<_> = std::str::from_utf8(&output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                let mut fields = line.split_whitespace();
                let value = fields.next().unwrap().parse::<i64>().unwrap();
                let status = fields.next().unwrap().parse::<u32>().unwrap();
                let span = fields.next().unwrap().parse::<usize>().unwrap();
                assert!(fields.next().is_none());
                (value, status, span)
            })
            .collect();
        assert_eq!(rows.len(), arguments.len().checked_div(arity).unwrap_or(2));
        rows
    };

    let source = definitions();
    let artifact = emit_elf64_bounded_integer_entry(source, "evaluate", 1000).unwrap();
    let inputs = [0, 3, -3, i64::MAX, i64::MIN, 7, -1];
    let values = invoke(&artifact, 1, &inputs);
    for (input, (value, status, span)) in inputs.into_iter().zip(values) {
        let mut expected = Vec::new();
        scarp::run(
            &format!("{source}\nprint(evaluate({input}));"),
            &mut expected,
        )
        .unwrap();
        assert_eq!(format!("{value}\n").as_bytes(), expected);
        assert_eq!((status, span), (0, 0));
    }
    let external_six = emit_elf64_bounded_integer_entry(source, "weighted", 1000).unwrap();
    assert_eq!(invoke(&external_six, 6, &[1, 2, 3, 4, 5, 6]), [(91, 0, 0)]);
    let sample = include_str!("../examples/native_wide_calls.srp");
    let sample = sample.split_once("\nprint(").unwrap().0;
    let artifact = emit_elf64_bounded_integer_entry(sample, "entry", 1000).unwrap();
    assert_eq!(invoke(&artifact, 1, &[2, 4]), [(61, 0, 0), (75, 0, 0)]);

    // Verify every mixed register/stack arity through three frames. The
    // external caller and both Scarp callers independently lay out arguments.
    for arity in 7..=16 {
        let source = wide_definitions(arity);
        let artifact = emit_elf64_bounded_integer_entry(&source, "dispatch", 2000).unwrap();
        let mut cases = Vec::new();
        for sign in [1, -1] {
            cases.extend((1..=arity as i64).map(|index| sign * index));
        }
        cases.resize(3 * arity - 1, 0);
        cases.push(i64::MAX);
        let results = invoke(&artifact, arity, &cases);
        for (inputs, (value, status, span)) in cases.chunks_exact(arity).zip(results) {
            let args = inputs
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let mut expected = Vec::new();
            match scarp::run(
                &format!("{source}\nprint(dispatch({args}));"),
                &mut expected,
            ) {
                Ok(()) => {
                    assert_eq!(format!("{value}\n").as_bytes(), expected, "arity {arity}");
                    assert_eq!((status, span), (0, 0));
                }
                Err(RunError::Diagnostic(error)) => {
                    assert_eq!(error.kind, DiagnosticKind::ArithmeticOverflow);
                    assert_eq!((value, status), (0, CHECKED_KERNEL_INTEGER_OVERFLOW));
                    assert_eq!(artifact.failure_spans[span], error.span, "arity {arity}");
                }
                Err(error) => panic!("{error:?}"),
            }
        }
    }

    let failures = "// गणना\npure funcra failure(n: integer) -> integer {
        return pair(second: 25 / n, first: 1 << -1) + 5;
    }
    pure funcra pair(first: integer, second: integer) -> integer { return first + second; }
    pure funcra nested(n: integer) -> integer { return level(n) + 17; }
    pure funcra level(n: integer) -> integer { return pair(n, 1); }";
    for (entry, input, expected_status) in [
        ("failure", 0, CHECKED_KERNEL_DIVISION_BY_ZERO),
        ("nested", i64::MAX, CHECKED_KERNEL_INTEGER_OVERFLOW),
    ] {
        let artifact = emit_elf64_bounded_integer_entry(failures, entry, 1000).unwrap();
        let (value, status, id) = invoke(&artifact, 1, &[input])[0];
        let Err(RunError::Diagnostic(error)) = scarp::run(
            &format!("{failures}\nprint({entry}({input}));"),
            &mut Vec::new(),
        ) else {
            panic!("interpreter should reject {entry}");
        };
        assert_eq!(
            (value, status, artifact.failure_spans[id]),
            (0, expected_status, error.span)
        );
    }

    let scalar = "// गणना\npure funcra f(n: integer, shift: integer) -> integer {
        return ((n + 1) * 3 / 2) << shift;
    }";
    let artifact = emit_elf64_bounded_integer_entry(scalar, "f", 100).unwrap();
    let rows = invoke(&artifact, 2, &[5, 1, i64::MAX, -1, 5, -1]);
    assert_eq!(rows[0], (18, 0, 0));
    for (args, row, expected_status) in [
        (
            format!("{}, -1", i64::MAX),
            rows[1],
            CHECKED_KERNEL_INTEGER_OVERFLOW,
        ),
        ("5, -1".to_string(), rows[2], CHECKED_KERNEL_INVALID_SHIFT),
    ] {
        let Err(RunError::Diagnostic(error)) =
            scarp::run(&format!("{scalar}\nprint(f({args}));"), &mut Vec::new())
        else {
            panic!("interpreter should reject f({args})");
        };
        assert_eq!(
            (row.0, row.1, artifact.failure_spans[row.2]),
            (0, expected_status, error.span)
        );
    }

    let metered = "pure funcra twice() -> integer { return leaf() + leaf(); }
                   pure funcra leaf() -> integer { return 2; }";
    let at = |text: &str, start: usize| {
        let begin = start + metered[start..].find(text).unwrap();
        Span::new(begin, begin + text.len())
    };
    let first = at("leaf()", 0);
    let second = at("leaf()", first.end);
    let leaf_statement = at("return 2;", second.end);
    let trace = [
        at("return leaf() + leaf();", 0),
        at("leaf() + leaf()", 0),
        first,
        first,
        leaf_statement,
        at("2", leaf_statement.start),
        second,
        second,
        leaf_statement,
        at("2", leaf_statement.start),
        at("+", 0),
    ];
    for (budget, expected_span) in trace.into_iter().enumerate() {
        let artifact = emit_elf64_bounded_integer_entry(metered, "twice", budget as u32).unwrap();
        let rows = invoke(&artifact, 0, &[]);
        assert_eq!(rows[0], rows[1], "work resets on each external invocation");
        let (value, status, id) = rows[0];
        assert_eq!(
            (value, status, artifact.failure_spans[id]),
            (0, CHECKED_KERNEL_WORK_LIMIT, expected_span)
        );
    }
    let artifact = emit_elf64_bounded_integer_entry(metered, "twice", trace.len() as u32).unwrap();
    assert_eq!(invoke(&artifact, 0, &[]), [(4, 0, 0), (4, 0, 0)]);

    let folded = "pure funcra f(n: integer) -> integer {
        return add(second: (2 * 3) + 1, first: n) + (6 / 2);
    }
    pure funcra add(first: integer, second: integer) -> integer {
        return first + second;
    }";
    let artifact = emit_elf64_bounded_integer_entry(folded, "f", 100).unwrap();
    let inputs = [-5, 0, 3, i64::MIN];
    for (row, input) in invoke(&artifact, 1, &inputs).into_iter().zip(inputs) {
        let mut expected = Vec::new();
        scarp::run(&format!("{folded}\nprint(f({input}));"), &mut expected).unwrap();
        assert_eq!(format!("{}\n", row.0).as_bytes(), expected);
        assert_eq!((row.1, row.2), (0, 0));
    }
}
