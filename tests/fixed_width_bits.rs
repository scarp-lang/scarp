use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    numeric::{self, Builtin as B},
    value::{Value, ValueType},
    ExecutionOptions, RunError,
};

const ONE_ARG: [B; 5] = [
    B::BitsCountOnes,
    B::BitsLeadingZeros,
    B::BitsTrailingZeros,
    B::BitsReverse,
    B::BitsSwapBytes,
];
const ROTATIONS: [B; 2] = [B::BitsRotateLeft, B::BitsRotateRight];

fn call(function: B, values: &[i64], steps: u64) -> Result<Value, Diagnostic> {
    let arguments: Vec<_> = values.iter().copied().map(Value::Integer).collect();
    numeric::evaluate(function, &arguments, steps)
}

fn run(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn fixed_width_intrinsics_register_exact_pure_integer_signatures() {
    for (name, builtin) in [
        ("bits_count_ones", B::BitsCountOnes),
        ("bits_leading_zeros", B::BitsLeadingZeros),
        ("bits_trailing_zeros", B::BitsTrailingZeros),
        ("bits_reverse", B::BitsReverse),
        ("bits_swap_bytes", B::BitsSwapBytes),
        ("bits_rotate_left", B::BitsRotateLeft),
        ("bits_rotate_right", B::BitsRotateRight),
    ] {
        assert_eq!(B::from_name(name), Some(builtin), "{name}");
        assert_eq!(builtin.result_type(), ValueType::Integer);
        assert_eq!(
            builtin.parameter_types(),
            if ROTATIONS.contains(&builtin) {
                &[ValueType::Integer, ValueType::Integer][..]
            } else {
                &[ValueType::Integer][..]
            },
        );
        assert_eq!(
            numeric::evaluate(builtin, &[], 10).unwrap_err().kind,
            Kind::CallArguments
        );
    }
    for source in [
        "print(bits_count_ones(1.0));",
        "print(bits_leading_zeros(true));",
        "print(bits_trailing_zeros(\"a\"));",
        "print(bits_reverse(false));",
        "print(bits_swap_bytes(2.0));",
        "print(bits_rotate_left(1, 2.0));",
        "print(bits_rotate_right(true, 1));",
        "print(bits_rotate_left(1));",
        "print(bits_count_ones(1, 2));",
        "bind wrong: boolean=bits_count_ones(1);",
    ] {
        let kind = scarp::compile(source).unwrap_err().kind;
        assert!(
            matches!(kind, Kind::TypeMismatch | Kind::CallArguments),
            "{source}"
        );
    }
}

#[test]
fn bit_counts_observe_full_signed_i64_two_complement_pattern() {
    for (value, count, leading, trailing) in [
        (0, 0, 64, 64),
        (1, 1, 63, 0),
        (2, 1, 62, 1),
        (3, 2, 62, 0),
        (-1, 64, 0, 0),
        (-2, 63, 0, 1),
        (i64::MIN, 1, 0, 63),
        (i64::MAX, 63, 1, 0),
        (0x1234_5678, 13, 35, 3),
    ] {
        let actual = [
            call(B::BitsCountOnes, &[value], 1)
                .unwrap()
                .as_integer()
                .unwrap(),
            call(B::BitsLeadingZeros, &[value], 1)
                .unwrap()
                .as_integer()
                .unwrap(),
            call(B::BitsTrailingZeros, &[value], 1)
                .unwrap()
                .as_integer()
                .unwrap(),
        ];
        assert_eq!(actual, [count, leading, trailing], "value={value}");
    }
}

#[test]
fn reverse_and_byte_swap_keep_exact_signed_bit_patterns() {
    for value in [
        i64::MIN,
        i64::MAX,
        -1,
        -2,
        0,
        1,
        0x0123_4567_89ab_cdef,
        0x0000_0000_80ff_aa55,
    ] {
        let reverse = call(B::BitsReverse, &[value], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        let swap = call(B::BitsSwapBytes, &[value], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        assert_eq!(reverse, (value as u64).reverse_bits() as i64);
        assert_eq!(swap, (value as u64).swap_bytes() as i64);
        assert_eq!(
            call(B::BitsReverse, &[reverse], 1).unwrap(),
            Value::Integer(value)
        );
        assert_eq!(
            call(B::BitsSwapBytes, &[swap], 1).unwrap(),
            Value::Integer(value)
        );
    }
    assert_eq!(
        call(B::BitsReverse, &[1], 1).unwrap(),
        Value::Integer(i64::MIN)
    );
    assert_eq!(
        call(B::BitsSwapBytes, &[0x0102_0304_0506_0708], 1).unwrap(),
        Value::Integer(0x0807_0605_0403_0201)
    );
}

fn expected_rotate(pattern: u64, count: u32, left: bool) -> i64 {
    // Bit-by-bit oracle, deliberately independent of rotate_* and of the
    // shift-and-OR implementation it could otherwise reproduce.
    let mut rotated = 0_u64;
    for bit in 0..64_u32 {
        if (pattern >> bit) & 1 != 0 {
            let destination = if left {
                (bit + count) % 64
            } else {
                (bit + 64 - count) % 64
            };
            rotated |= 1_u64 << destination;
        }
    }
    rotated as i64
}

#[test]
fn every_rotation_count_matches_independent_shift_oracle() {
    for value in [
        0,
        1,
        -1,
        -2,
        i64::MIN,
        i64::MAX,
        0x0123_4567_89ab_cdef,
        -0x0123_4567_89ab_cdef,
    ] {
        for count in 0..64 {
            let left = call(B::BitsRotateLeft, &[value, count], 1)
                .unwrap()
                .as_integer()
                .unwrap();
            let right = call(B::BitsRotateRight, &[value, count], 1)
                .unwrap()
                .as_integer()
                .unwrap();
            assert_eq!(left, expected_rotate(value as u64, count as u32, true));
            assert_eq!(right, expected_rotate(value as u64, count as u32, false));
            assert_eq!(
                call(B::BitsRotateRight, &[left, count], 1).unwrap(),
                Value::Integer(value)
            );
            assert_eq!(
                call(B::BitsRotateLeft, &[right, count], 1).unwrap(),
                Value::Integer(value)
            );
        }
    }
}

#[test]
fn seeded_pattern_oracle_checks_bit_counts_reverse_and_byte_swap() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..2_048 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let bits = state as i64;
        let ones = call(B::BitsCountOnes, &[bits], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        assert_eq!(ones, i64::from(state.count_ones()));
        let leading = call(B::BitsLeadingZeros, &[bits], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        let trailing = call(B::BitsTrailingZeros, &[bits], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        assert_eq!(leading, i64::from(state.leading_zeros()));
        assert_eq!(trailing, i64::from(state.trailing_zeros()));
        let flipped = call(B::BitsReverse, &[bits], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        assert_eq!(flipped as u64, state.reverse_bits());
        let bytes = call(B::BitsSwapBytes, &[bits], 1)
            .unwrap()
            .as_integer()
            .unwrap();
        assert_eq!(bytes as u64, state.swap_bytes());
    }
}

#[test]
fn source_program_supports_typed_functions_defaults_and_user_shadowing() {
    let source = r#"
        pure funcra rotate(value: integer, count: integer=8)->integer {
            return bits_rotate_left(value,count);
        }
        bind original=0x01020304;
        bind result=rotate(original);
        print(result,bits_rotate_right(result,8),bits_count_ones(0xff));
        print(bits_leading_zeros(0),bits_trailing_zeros(0));
    "#;
    assert_eq!(run(source), "4328719360 16909060 8\n64 64\n");
    assert_eq!(
        run("pure funcra bits_reverse(v:integer)->integer{return v+1;} print(bits_reverse(2));"),
        "3\n"
    );
}

#[test]
fn source_interpreter_matches_independent_reference_over_many_values() {
    let mut source = String::new();
    let mut expected = String::new();
    for value in [i64::MIN, i64::MAX, -1, 0, 1, 0x7fff_aaaa_1234_5678] {
        for count in 0..64_u32 {
            source.push_str(&format!(
                "print(bits_rotate_left({value},{count}),bits_rotate_right({value},{count}));"
            ));
            expected.push_str(&format!(
                "{} {}\n",
                expected_rotate(value as u64, count, true),
                expected_rotate(value as u64, count, false)
            ));
        }
    }
    assert_eq!(run(&source), expected);
}

#[test]
fn invalid_rotation_counts_report_argument_spans_and_preserve_atomic_output() {
    for count in ["-1", "64", "65", "9223372036854775807"] {
        for builtin in ["bits_rotate_left", "bits_rotate_right"] {
            let source =
                format!("print(\"ready\"); eprint(\"pending\"); print({builtin}(1,{count}));");
            let expected = scarp::check(&source).unwrap_err();
            assert_eq!(expected.kind, Kind::InvalidShift, "{source}");
            assert_eq!(&source[expected.span.start..expected.span.end], count);
            assert!(expected.help.unwrap().contains("between 0 and 63"));

            let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
            let RunError::Diagnostic(actual) = scarp::run_with_io_and_options(
                &source,
                &mut stdout,
                &mut stderr,
                ExecutionOptions::default(),
            )
            .unwrap_err() else {
                panic!("expected a diagnostic");
            };
            assert_eq!(actual.kind, Kind::InvalidShift);
            assert_eq!(actual.span, expected.span);
            assert!(stdout.is_empty() && stderr.is_empty());
        }
    }
    for builtin in ROTATIONS {
        for invalid in [-1, 64, i64::MAX, i64::MIN] {
            assert_eq!(
                call(builtin, &[i64::MIN, invalid], 1).unwrap_err().kind,
                Kind::InvalidShift
            );
        }
    }
}

#[test]
fn exact_work_budget_and_mismatched_host_types_are_checked() {
    for builtin in ONE_ARG {
        assert!(call(builtin, &[i64::MIN], 1).is_ok());
        assert_eq!(
            call(builtin, &[i64::MIN], 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            numeric::evaluate(builtin, &[Value::Boolean(true)], 100)
                .unwrap_err()
                .kind,
            Kind::TypeMismatch
        );
    }
    for builtin in ROTATIONS {
        assert!(call(builtin, &[i64::MIN, 63], 1).is_ok());
        assert_eq!(
            call(builtin, &[i64::MIN, 63], 0).unwrap_err().kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            numeric::evaluate(builtin, &[Value::Integer(1), Value::Boolean(true)], 100)
                .unwrap_err()
                .kind,
            Kind::TypeMismatch
        );
    }
}

#[test]
fn no_new_native_lowering_is_implicitly_advertised() {
    for source in [
        "print(bits_reverse(1));",
        "print(bits_count_ones(-1));",
        "print(bits_rotate_left(1,63));",
    ] {
        assert_eq!(
            scarp::c_backend::emit_c(source).unwrap_err().kind,
            Kind::NativeUnsupported
        );
        assert_eq!(
            scarp::native_object::emit_elf64_integer_kernel(source)
                .unwrap_err()
                .kind,
            Kind::NativeUnsupported
        );
    }
}
