use scarp::{
    diagnostics::DiagnosticKind as Kind,
    native_object::plan_x86_64_value_layout,
    types::PrimitiveType,
    value::{FixedInteger, WideFixedInteger},
};

fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn fixed_integers_support_all_widths_and_full_unsigned_range() {
    assert_eq!(
        output(
            "
            print(to_i8(-128), to_u8(255), to_i16(-32768), to_u16(65535));
            print(to_i32(-2147483648), to_u32(4294967295), to_i64(-9223372036854775808));
            bind v = to_u64(bigint(\"18446744073709551615\"));
            print(v, v - to_u64(1), v % to_u64(5), to_integer(to_i64(-10)));
            print(to_integer(42.0));
        "
        ),
        "-128 255 -32768 65535\n-2147483648 4294967295 -9223372036854775808\n\
         18446744073709551615 18446744073709551614 0 -10\n42\n",
    );
}

#[test]
fn bitwise_shift_assignment_and_ordering_obey_type_width() {
    assert_eq!(
        output(
            "
            bind negative = to_i8(-2);
            var mask = to_u8(129);
            mask <<= 1;
            print(negative >> 1, negative >>> 1, ~negative, mask);
            mask |= to_u8(128);
            print(mask & to_u8(3), mask ^ to_u8(128));
            print((negative, to_u8(1)) < (to_i8(0), to_u8(0)));
        "
        ),
        "-1 127 1 2\n2 2\ntrue\n",
    );
}

#[test]
fn fixed_integers_reject_checked_overflow_and_mixing() {
    let cases = [
        ("to_i8(128)", Kind::NumericConversion),
        ("to_u8(-1)", Kind::NumericConversion),
        (
            "to_u64(bigint(\"18446744073709551616\"))",
            Kind::NumericConversion,
        ),
        (
            "to_integer(to_u64(bigint(\"18446744073709551615\")))",
            Kind::NumericConversion,
        ),
        ("to_i8(127) + to_i8(1)", Kind::FixedIntegerOverflow),
        ("to_u8(0) - to_u8(1)", Kind::FixedIntegerOverflow),
        (
            "to_u64(bigint(\"18446744073709551615\")) * to_u64(2)",
            Kind::FixedIntegerOverflow,
        ),
        ("to_i8(-128) / to_i8(-1)", Kind::FixedIntegerOverflow),
        ("to_i8(-128) % to_i8(-1)", Kind::FixedIntegerOverflow),
        ("-to_i8(-128)", Kind::FixedIntegerOverflow),
        ("to_u16(1) / to_u16(0)", Kind::DivisionByZero),
        ("to_u8(1) << 8", Kind::InvalidShift),
        ("to_i8(1) << -1", Kind::InvalidShift),
        ("to_i8(5) + 2", Kind::TypeMismatch),
        ("to_i8(5) == to_u8(5)", Kind::TypeMismatch),
        ("to_i8(1.0)", Kind::TypeMismatch),
    ];
    for (expression, kind) in cases {
        let source = format!("print({expression});");
        assert_eq!(
            scarp::check(&source).unwrap_err().kind,
            kind,
            "{expression}"
        );
    }
}

#[test]
fn value_layouts_use_width_and_natural_alignment() {
    let compiled = scarp::compile("bind x = (to_u8(3), to_i32(4), to_u16(5));").unwrap();
    let registry = compiled.type_registry();
    for kind in PrimitiveType::ALL.iter().copied() {
        let Some((width, signed)) = kind.fixed_integer_width() else {
            continue;
        };
        let layout = plan_x86_64_value_layout(registry, kind.id()).unwrap();
        assert_eq!(layout.size, u64::from(width / 8));
        assert_eq!(layout.alignment, layout.size);
        if width == 128 {
            assert!(FixedInteger::new(kind, 0).is_none());
            continue;
        }
        let min = if signed { -(1_i128 << (width - 1)) } else { 0 };
        let max = if signed {
            (1_i128 << (width - 1)) - 1
        } else {
            (1_i128 << width) - 1
        };
        assert_eq!(FixedInteger::new(kind, min).unwrap().value(), min);
        assert_eq!(FixedInteger::new(kind, max).unwrap().value(), max);
        assert!(FixedInteger::new(kind, min - 1).is_none());
        assert!(FixedInteger::new(kind, max + 1).is_none());
    }
}

#[test]
fn generic_numeric_functions_specialize_without_legacy_coercion() {
    let source = "
        pure funcra add<T: Numeric>(a: T, b: T) -> T { return a + b; }
        var x: u16 = to_u16(4);
        x += add(to_u16(4), to_u16(5));
        print(x, add(1, 2), add(1.0, 2.0));
    ";
    assert_eq!(output(source), "13 3 3.0\n");
}

#[test]
fn wide_and_pointer_sized_values_keep_exact_extremes() {
    let result = output(
        r#"
        bind signed = to_i128(bigint("-170141183460469231731687303715884105728"));
        bind unsigned = to_u128(bigint("340282366920938463463374607431768211455"));
        print(signed, unsigned);
        print(signed >> 127, signed >>> 127, ~signed);
        print(unsigned / to_u128(3), unsigned % to_u128(3));
        print((to_u128(1), unsigned) > (to_u128(1), unsigned - to_u128(1)));
        print(to_isize(-9223372036854775808), to_usize(bigint("18446744073709551615")));
        print(to_integer(to_i128(-9223372036854775808)), to_i128(to_u64(3)));
    "#,
    );
    assert_eq!(
        result,
        concat!(
            "-170141183460469231731687303715884105728 340282366920938463463374607431768211455\n",
            "-1 1 170141183460469231731687303715884105727\n",
            "113427455640312821154458202477256070485 0\n",
            "true\n-9223372036854775808 18446744073709551615\n-9223372036854775808 3\n",
        )
    );
    let signed = WideFixedInteger::from_i128(i128::MIN);
    let unsigned = WideFixedInteger::from_u128(u128::MAX);
    assert_eq!(signed.to_i128(), Some(i128::MIN));
    assert_eq!(signed.to_u128(), None);
    assert_eq!(unsigned.to_i128(), None);
    assert_eq!(unsigned.to_u128(), Some(u128::MAX));
}

#[test]
fn signed_and_unsigned_128_bit_overflow_is_checked_before_result() {
    let minimum = "to_i128(bigint(\"-170141183460469231731687303715884105728\"))";
    let maximum = "to_u128(bigint(\"340282366920938463463374607431768211455\"))";
    for (expression, expected) in [
        (
            format!("{minimum} / to_i128(-1)"),
            Kind::FixedIntegerOverflow,
        ),
        (
            format!("{minimum} % to_i128(-1)"),
            Kind::FixedIntegerOverflow,
        ),
        (format!("-{minimum}"), Kind::FixedIntegerOverflow),
        (
            format!("{maximum} + to_u128(1)"),
            Kind::FixedIntegerOverflow,
        ),
        (
            format!("{maximum} * to_u128(2)"),
            Kind::FixedIntegerOverflow,
        ),
        (format!("{maximum} / to_u128(0)"), Kind::DivisionByZero),
        (format!("to_i128({maximum})"), Kind::NumericConversion),
        (format!("to_u128({minimum})"), Kind::NumericConversion),
        (format!("to_usize({maximum})"), Kind::NumericConversion),
        (
            "to_u128(bigint(\"340282366920938463463374607431768211456\"))".to_owned(),
            Kind::NumericConversion,
        ),
        ("to_i128(1) << 128".to_owned(), Kind::InvalidShift),
        ("to_usize(1) == to_u64(1)".to_owned(), Kind::TypeMismatch),
    ] {
        let source = format!("print({expression});");
        assert_eq!(
            scarp::check(&source).unwrap_err().kind,
            expected,
            "{expression}"
        );
    }
}

#[test]
fn explicit_arithmetic_modes_preserve_width_and_report_overflow() {
    assert_eq!(
        output(
            r#"
            print(wrapping_add(to_i8(127), to_i8(1)), wrapping_sub(to_u8(0), to_u8(1)));
            print(wrapping_mul(to_i8(64), to_i8(2)), saturating_mul(to_i8(-128), to_i8(-1)));
            print(saturating_add(to_i8(-120), to_i8(-20)), saturating_sub(to_u16(2), to_u16(3)));
            print(overflowing_add(to_i8(127), to_i8(1)), overflowing_sub(to_u8(1), to_u8(0)));
            bind maximum = to_u128(bigint("340282366920938463463374607431768211455"));
            print(overflowing_mul(maximum, to_u128(2)), saturating_add(maximum, to_u128(1)));
            print(wrapping_add(to_usize(bigint("18446744073709551615")), to_usize(1)));
        "#
        ),
        concat!(
            "-128 255\n-128 127\n-128 0\n(-128, true) (1, false)\n",
            "(340282366920938463463374607431768211454, true) ",
            "340282366920938463463374607431768211455\n0\n",
        ),
    );
    let options = scarp::ExecutionOptions {
        max_tuple_bytes: 0,
        ..Default::default()
    };
    assert_eq!(
        scarp::check_with_options("bind x = overflowing_add(to_u8(1), to_u8(2));", options)
            .unwrap_err()
            .kind,
        Kind::TupleAllocation,
    );
}

#[test]
fn bit_queries_rotations_and_endianness_use_the_declared_width() {
    assert_eq!(
        output(
            r#"
            print(bits_count_ones(to_i128(-1)), bits_leading_zeros(to_u128(0)),
                bits_trailing_zeros(to_i8(0)), bits_count_ones(7));
            print(bits_rotate_left(to_u8(129), 1), bits_rotate_right(to_u8(129), 1));
            print(bits_rotate_left(to_u128(1), 127), bits_rotate_right(to_u128(1), 1));
            print(bits_swap_bytes(to_u16(4660)), bits_reverse(to_i8(8)));
            print(bits_swap_bytes(to_u128(1)), bits_leading_zeros(to_usize(1)));
        "#
        ),
        concat!(
            "128 128 8 3\n3 192\n",
            "170141183460469231731687303715884105728 170141183460469231731687303715884105728\n",
            "13330 16\n1329227995784915872903807060280344576 63\n",
        ),
    );
    assert_eq!(
        scarp::check("print(bits_rotate_left(to_u8(1), 8));")
            .unwrap_err()
            .kind,
        Kind::InvalidShift,
    );
}

#[test]
fn wide_specializations_keep_payloads_inline_and_support_generic_calls() {
    assert_eq!(
        output(
            r#"
            pure funcra add<T: Numeric>(a: T, b: T) -> T { return a + b; }
            var value = to_u128(bigint("340282366920938463463374607431768211453"));
            value += add(to_u128(1), to_u128(1));
            print(value, add(to_isize(11), to_isize(31)));
        "#
        ),
        "340282366920938463463374607431768211455 42\n",
    );
    assert!(std::mem::align_of::<WideFixedInteger>() <= std::mem::align_of::<u64>());
}
