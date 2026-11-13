//! Host-side exact decimal API. Source-language decimal is NOT available yet.
use scarp::decimal::{Decimal, DecimalError, RoundingMode};

fn d(text: &str) -> Decimal {
    text.parse().unwrap()
}

#[test]
fn exact_decimal_addition_does_not_change_existing_float_results() {
    let exact = d("0.1").checked_add(d("0.2")).unwrap();
    assert_eq!(exact.to_string(), "0.3");
    let mut output = Vec::new();
    scarp::run("print(0.1 + 0.2);", &mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "0.30000000000000004\n");
}

#[test]
fn exact_accounting_example_and_explicit_rounding() {
    let subtotal = d("199.99");
    let tax = subtotal.checked_mul(d("0.18")).unwrap();
    assert_eq!(tax.to_string(), "35.9982");
    let total = subtotal.checked_add(tax).unwrap();
    assert_eq!(total.to_string(), "235.9882");
    assert_eq!(
        total
            .round_to(2, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "235.99"
    );
    assert_eq!(
        d("1")
            .div_rounded(d("3"), 2, RoundingMode::HalfEven)
            .unwrap()
            .to_string(),
        "0.33"
    );
    assert_eq!(
        d("1").div_rounded(d("0"), 2, RoundingMode::HalfEven),
        Err(DecimalError::DivisionByZero)
    );
}

#[test]
fn source_level_type_does_not_exist_until_separate_integration() {
    assert!(scarp::compile("bind amount: decimal = 10;").is_err());
}
