//! Explicit, bounded BigInt operations; never coerce to float or i64 implicitly.
use super::*;
use crate::bigint::BigInt;

pub(super) fn evaluate(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    match function {
        Builtin::BigInt => BigInt::parse_decimal(
            args[0].as_str().expect("typed string"),
            budget,
            argument_span(0),
        )
        .map(Value::BigInt),
        Builtin::BigIntFromInteger => BigInt::from_integer(
            args[0].as_integer().expect("typed integer"),
            budget,
            argument_span(0),
        )
        .map(Value::BigInt),
        Builtin::BigIntFibonacci => BigInt::fibonacci(
            args[0].as_integer().expect("typed integer"),
            budget,
            argument_span(0),
        )
        .map(Value::BigInt),
        Builtin::BigIntBinomial | Builtin::BigIntPermutation => {
            let n = args[0].as_integer().expect("typed integer");
            let k = args[1].as_integer().expect("typed integer");
            if function == Builtin::BigIntBinomial {
                BigInt::binomial(n, k, budget, span)
            } else {
                BigInt::permutation(n, k, budget, span)
            }
            .map(Value::BigInt)
        }
        Builtin::BigIntMin | Builtin::BigIntMax => {
            let a = args[0].as_bigint().expect("typed bigint");
            let b = args[1].as_bigint().expect("typed bigint");
            budget.check_bigint_digits(a.decimal_digits().max(b.decimal_digits()), span)?;
            budget.charge(a.decimal_digits().max(b.decimal_digits()) as u64, span)?;
            let selected = if function == Builtin::BigIntMin {
                a.min(b)
            } else {
                a.max(b)
            };
            Ok(Value::BigInt(selected.clone()))
        }
        Builtin::BigIntModInverse => args[0]
            .as_bigint()
            .expect("typed bigint")
            .mod_inverse(args[1].as_bigint().expect("typed bigint"), budget, span)
            .map(Value::BigInt),
        Builtin::BigIntFactorial => BigInt::factorial(
            args[0].as_integer().expect("typed integer"),
            budget,
            argument_span(0),
        )
        .map(Value::BigInt),
        Builtin::BigIntRoot => args[0]
            .as_bigint()
            .expect("typed bigint")
            .integer_root(args[1].as_integer().expect("typed integer"), budget, span)
            .map(Value::BigInt),
        Builtin::BigIntPow => args[0]
            .as_bigint()
            .expect("typed bigint")
            .pow(
                args[1].as_integer().expect("typed integer"),
                budget,
                argument_span(1),
            )
            .map(Value::BigInt),
        Builtin::BigIntGcd => args[0]
            .as_bigint()
            .expect("typed bigint")
            .gcd(args[1].as_bigint().expect("typed bigint"), budget, span)
            .map(Value::BigInt),
        Builtin::BigIntLcm => args[0]
            .as_bigint()
            .expect("typed bigint")
            .lcm(args[1].as_bigint().expect("typed bigint"), budget, span)
            .map(Value::BigInt),
        Builtin::BigIntSqrt => args[0]
            .as_bigint()
            .expect("typed bigint")
            .integer_sqrt(budget, argument_span(0))
            .map(Value::BigInt),
        Builtin::BigIntPowMod => args[0]
            .as_bigint()
            .expect("typed bigint")
            .pow_mod(
                args[1].as_integer().expect("typed integer"),
                args[2].as_bigint().expect("typed bigint"),
                budget,
                argument_span(1),
                argument_span(2),
            )
            .map(Value::BigInt),
        Builtin::BigIntFromRadix => BigInt::from_radix(
            args[0].as_str().expect("typed string"),
            args[1].as_integer().expect("typed integer"),
            budget,
            argument_span(0),
            argument_span(1),
        )
        .map(Value::BigInt),
        Builtin::BigIntToRadix => args[0]
            .as_bigint()
            .expect("typed bigint")
            .to_radix(
                args[1].as_integer().expect("typed integer"),
                budget,
                argument_span(0),
                argument_span(1),
            )
            .map(Value::String),
        Builtin::BigIntAnd | Builtin::BigIntOr | Builtin::BigIntXor => {
            use crate::bigint::bitwise::BitwiseOperation;
            let operation = match function {
                Builtin::BigIntAnd => BitwiseOperation::And,
                Builtin::BigIntOr => BitwiseOperation::Or,
                _ => BitwiseOperation::Xor,
            };
            args[0]
                .as_bigint()
                .expect("typed bigint")
                .bitwise(
                    args[1].as_bigint().expect("typed bigint"),
                    operation,
                    budget,
                    span,
                )
                .map(Value::BigInt)
        }
        Builtin::BigIntNot => args[0]
            .as_bigint()
            .expect("typed bigint")
            .bitwise_not(budget, argument_span(0))
            .map(Value::BigInt),
        Builtin::BigIntShl | Builtin::BigIntShr => {
            let value = args[0].as_bigint().expect("typed bigint");
            let count = args[1].as_integer().expect("typed integer");
            if function == Builtin::BigIntShl {
                value.shift_left(count, budget, argument_span(1))
            } else {
                value.shift_right(count, budget, argument_span(1))
            }
            .map(Value::BigInt)
        }
        _ => {
            let value = args[0].as_bigint().expect("typed bigint");
            budget.check_bigint_digits(value.decimal_digits(), argument_span(0))?;
            match function {
                Builtin::BigIntAbs => {
                    budget.charge(1, span)?;
                    Ok(Value::BigInt(value.absolute()))
                }
                Builtin::BigIntToInteger => {
                    budget.charge(1, span)?;
                    value.to_integer().map(Value::Integer).ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticKind::NumericConversion,
                            argument_span(0),
                            "bigint is outside the signed 64-bit integer range",
                        )
                        .with_help("keep the exact bigint value or select a value from -9223372036854775808 through 9223372036854775807")
                    })
                }
                Builtin::BigIntNextPrime => value.next_prime(budget, span).map(Value::BigInt),
                Builtin::BigIntIsPerfectSquare => {
                    value.is_perfect_square(budget, span).map(Value::Boolean)
                }
                Builtin::BigIntIsPowerOfTwo => {
                    value.is_power_of_two(budget, span).map(Value::Boolean)
                }
                Builtin::BigIntReverse => value.reverse_digits(budget, span).map(Value::BigInt),
                Builtin::BigIntIsPalindrome => {
                    value.is_palindrome(budget, span).map(Value::Boolean)
                }
                Builtin::BigIntDigits => {
                    budget.charge(1, span)?;
                    Ok(Value::Integer(value.decimal_digits() as i64))
                }
                Builtin::BigIntDigitSum => value.digit_sum(budget, span).map(Value::Integer),
                Builtin::BigIntTrailingZeros => {
                    value.trailing_zeros(budget, span).map(Value::Integer)
                }
                Builtin::BigIntLog2 => value.log2(budget, span).map(Value::Integer),
                Builtin::BigIntIsPrime => value.is_prime(budget, span).map(Value::Boolean),
                Builtin::BigIntIsProbablePrime => {
                    value.is_probable_prime(budget, span).map(Value::Boolean)
                }
                Builtin::BigIntSign => {
                    budget.charge(1, span)?;
                    Ok(Value::Integer(if value.is_zero() {
                        0
                    } else if value.is_negative() {
                        -1
                    } else {
                        1
                    }))
                }
                Builtin::BigIntIsEven
                | Builtin::BigIntIsOdd
                | Builtin::BigIntIsZero
                | Builtin::BigIntIsPositive
                | Builtin::BigIntIsNegative => {
                    budget.charge(1, span)?;
                    Ok(Value::Boolean(match function {
                        Builtin::BigIntIsEven => value.is_even(),
                        Builtin::BigIntIsOdd => !value.is_even(),
                        Builtin::BigIntIsZero => value.is_zero(),
                        Builtin::BigIntIsPositive => !value.is_zero() && !value.is_negative(),
                        _ => value.is_negative(),
                    }))
                }
                Builtin::BigIntBitLength => value
                    .bit_length(budget, argument_span(0))
                    .map(Value::Integer),
                Builtin::BigIntToText => value.to_text(budget, argument_span(0)).map(Value::String),
                _ => unreachable!("checked bigint dispatch"),
            }
        }
    }
}
