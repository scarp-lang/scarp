use scarp::{
    diagnostics::DiagnosticKind as Kind, native_object::plan_x86_64_value_layout,
    types::PrimitiveType, value::FixedInteger,
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
