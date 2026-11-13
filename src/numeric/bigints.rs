//! Explicit BigInt constructors/conversions; no implicit numeric coercion.
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
        _ => {
            let value = args[0].as_bigint().expect("typed bigint");
            budget.check_bigint_digits(value.decimal_digits(), argument_span(0))?;
            budget.charge(1, span)?;
            match function {
                Builtin::BigIntAbs => Ok(Value::BigInt(value.absolute())),
                Builtin::BigIntToInteger => value.to_integer().map(Value::Integer).ok_or_else(|| Diagnostic::new(DiagnosticKind::NumericConversion, argument_span(0), "bigint is outside the signed 64-bit integer range").with_help("keep the exact bigint value or select a value from -9223372036854775808 through 9223372036854775807")),
                _ => unreachable!("checked bigint dispatch"),
            }
        }
    }
}
