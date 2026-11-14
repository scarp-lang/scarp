//! Checked pure built-ins and finite numerical kernels, independent of user frames.

use crate::{
    diagnostics::{Diagnostic, DiagnosticKind, Span},
    limits::Budget,
    value::{finite_number, FiniteFloat, NumericVector, Value, ValueType, MAX_VECTOR_LENGTH},
};

mod bigints;
pub(crate) mod decimals;
mod correlation;
mod integers;
mod kernels;
mod nucleus;
mod parse_format;
mod samples;
mod scalar_text;
mod similarity;
mod temperature;
mod text;
mod tiles;
mod topk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Builtin {
    Variance,
    StdDev,
    CosineSimilarity,
    PearsonCorrelation,
    SampleArea,
    SampleSlopes,
    SampleCumulativeArea,
    Sigmoid,
    Relu,
    Tanh,
    IntsLength,
    IntsSum,
    IntsWindow,
    IntsJoin,
    IntsSet,
    IntsRange,
    IntsToVector,
    IntsFromVector,
    ToFloat,
    ToInteger,
    TextConcat,
    TextSlice,
    TextRepeat,
    TextLength,
    TextBytes,
    TextCodepoint,
    TextFind,
    TextContains,
    TextStartsWith,
    TextEndsWith,
    Length,
    Sum,
    Mean,
    Dot,
    Mse,
    Add,
    Scale,
    Clamp,
    Fill,
    Softmax,
    SoftmaxTemp,
    LogSoftmaxTemp,
    Norm,
    Rms,
    Normalize,
    LogSumExp,
    LogSoftmax,
    ArgMax,
    ArgMin,
    TopK,
    TopKValues,
    TopP,
    TopPValues,
    MaxValue,
    MinValue,
    LogProb,
    CrossEntropy,
    LossGrad,
    OneHot,
    Window,
    Stitch,
    Tile,
    Height,
    Width,
    Cell,
    Lane,
    Unroll,
    Turn,
    MatVec,
    MatMul,
    TileAdd,
    TileScale,
    IntegerToText,
    FloatToText,
    TextToInteger,
    TextToFloat,
    TextTrim,
    TextReplace,
    BigInt,
    BigIntFromInteger,
    BigIntToInteger,
    BigIntAbs,
    BigIntPow,
    BigIntBitLength,
    BigIntToText,
    BigIntGcd,
    BigIntLcm,
    BigIntSqrt,
    BigIntPowMod,
    BigIntFromRadix,
    BigIntToRadix,
    BigIntAnd,
    BigIntOr,
    BigIntXor,
    BigIntNot,
    BigIntShl,
    BigIntShr,
    Decimal,
    DecimalFromInteger,
    DecimalToInteger,
    DecimalToText,
    DecimalAbs,
    DecimalRound,
    DecimalFormat,
    DecimalDivRounded,
}

impl Builtin {
    pub fn intern_signature(
        self,
        types: &mut crate::types::TypeRegistryBuilder,
    ) -> Result<crate::types::TypeId, crate::types::TypeRegistryError> {
        types.intern(crate::types::TypeKind::Function {
            parameters: self
                .parameter_types()
                .iter()
                .map(|kind| kind.id())
                .collect(),
            result: self.result_type().id(),
            effect: crate::types::FunctionEffect::Pure,
        })
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "decimal" => Self::Decimal,
            "decimal_from_integer" => Self::DecimalFromInteger,
            "decimal_to_integer" => Self::DecimalToInteger,
            "decimal_to_text" => Self::DecimalToText,
            "decimal_abs" => Self::DecimalAbs,
            "decimal_round" => Self::DecimalRound,
            "decimal_format" => Self::DecimalFormat,
            "decimal_div_rounded" => Self::DecimalDivRounded,
            "bigint" => Self::BigInt,
            "bigint_from_integer" => Self::BigIntFromInteger,
            "bigint_to_integer" => Self::BigIntToInteger,
            "bigint_abs" => Self::BigIntAbs,
            "bigint_pow" => Self::BigIntPow,
            "bigint_bit_length" => Self::BigIntBitLength,
            "bigint_to_text" => Self::BigIntToText,
            "bigint_gcd" => Self::BigIntGcd,
            "bigint_lcm" => Self::BigIntLcm,
            "bigint_sqrt" => Self::BigIntSqrt,
            "bigint_pow_mod" => Self::BigIntPowMod,
            "bigint_from_radix" => Self::BigIntFromRadix,
            "bigint_to_radix" => Self::BigIntToRadix,
            "bigint_and" => Self::BigIntAnd,
            "bigint_or" => Self::BigIntOr,
            "bigint_xor" => Self::BigIntXor,
            "bigint_not" => Self::BigIntNot,
            "bigint_shl" => Self::BigIntShl,
            "bigint_shr" => Self::BigIntShr,
            "integer_to_text" => Self::IntegerToText,
            "float_to_text" => Self::FloatToText,
            "text_to_integer" => Self::TextToInteger,
            "text_to_float" => Self::TextToFloat,
            "text_trim" => Self::TextTrim,
            "text_replace" => Self::TextReplace,
            "variance" => Self::Variance,
            "stddev" => Self::StdDev,
            "cosine_similarity" => Self::CosineSimilarity,
            "pearson_correlation" => Self::PearsonCorrelation,
            "sample_area" => Self::SampleArea,
            "sample_slopes" => Self::SampleSlopes,
            "sample_cumulative_area" => Self::SampleCumulativeArea,
            "sigmoid" => Self::Sigmoid,
            "relu" => Self::Relu,
            "tanh" => Self::Tanh,
            "ints_len" => Self::IntsLength,
            "ints_sum" => Self::IntsSum,
            "ints_window" => Self::IntsWindow,
            "ints_join" => Self::IntsJoin,
            "ints_set" => Self::IntsSet,
            "ints_range" => Self::IntsRange,
            "ints_to_vector" => Self::IntsToVector,
            "ints_from_vector" => Self::IntsFromVector,
            "to_float" => Self::ToFloat,
            "to_integer" => Self::ToInteger,
            "text_concat" => Self::TextConcat,
            "text_slice" => Self::TextSlice,
            "text_repeat" => Self::TextRepeat,
            "text_len" => Self::TextLength,
            "text_bytes" => Self::TextBytes,
            "text_codepoint" => Self::TextCodepoint,
            "text_find" => Self::TextFind,
            "text_contains" => Self::TextContains,
            "text_starts_with" => Self::TextStartsWith,
            "text_ends_with" => Self::TextEndsWith,
            "len" => Self::Length,
            "sum" => Self::Sum,
            "mean" => Self::Mean,
            "dot" => Self::Dot,
            "mse" => Self::Mse,
            "add" => Self::Add,
            "scale" => Self::Scale,
            "clamp" => Self::Clamp,
            "fill" => Self::Fill,
            "softmax" => Self::Softmax,
            "softmax_temp" => Self::SoftmaxTemp,
            "logsoftmax_temp" => Self::LogSoftmaxTemp,
            "norm" => Self::Norm,
            "rms" => Self::Rms,
            "normalize" => Self::Normalize,
            "logsumexp" => Self::LogSumExp,
            "logsoftmax" => Self::LogSoftmax,
            "argmax" => Self::ArgMax,
            "argmin" => Self::ArgMin,
            "topk" => Self::TopK,
            "topk_values" => Self::TopKValues,
            "top_p" => Self::TopP,
            "top_p_values" => Self::TopPValues,
            "maxval" => Self::MaxValue,
            "minval" => Self::MinValue,
            "logprob" => Self::LogProb,
            "crossentropy" => Self::CrossEntropy,
            "lossgrad" => Self::LossGrad,
            "onehot" => Self::OneHot,
            "window" => Self::Window,
            "stitch" => Self::Stitch,
            "tile" => Self::Tile,
            "height" => Self::Height,
            "width" => Self::Width,
            "cell" => Self::Cell,
            "lane" => Self::Lane,
            "unroll" => Self::Unroll,
            "turn" => Self::Turn,
            "matvec" => Self::MatVec,
            "matmul" => Self::MatMul,
            "tileadd" => Self::TileAdd,
            "tilescale" => Self::TileScale,
            _ => return None,
        })
    }

    pub fn parameter_types(self) -> &'static [ValueType] {
        use ValueType::{Float, Integer, Integers, String, Tile, Vector};
        match self {
            Self::Decimal => &[String],
            Self::DecimalFromInteger => &[Integer],
            Self::DecimalToInteger | Self::DecimalToText | Self::DecimalAbs => &[ValueType::Decimal],
            Self::DecimalRound | Self::DecimalFormat => &[ValueType::Decimal, Integer, String],
            Self::DecimalDivRounded => &[ValueType::Decimal, ValueType::Decimal, Integer, String],
            Self::BigInt => &[String],
            Self::BigIntFromInteger => &[Integer],
            Self::BigIntToInteger
            | Self::BigIntAbs
            | Self::BigIntBitLength
            | Self::BigIntToText => &[ValueType::BigInt],
            Self::BigIntPow => &[ValueType::BigInt, Integer],
            Self::BigIntPowMod => &[ValueType::BigInt, Integer, ValueType::BigInt],
            Self::BigIntFromRadix => &[String, Integer],
            Self::BigIntToRadix => &[ValueType::BigInt, Integer],
            Self::BigIntAnd | Self::BigIntOr | Self::BigIntXor => {
                &[ValueType::BigInt, ValueType::BigInt]
            }
            Self::BigIntNot => &[ValueType::BigInt],
            Self::BigIntShl | Self::BigIntShr => &[ValueType::BigInt, Integer],
            Self::BigIntGcd | Self::BigIntLcm => &[ValueType::BigInt, ValueType::BigInt],
            Self::BigIntSqrt => &[ValueType::BigInt],
            Self::IntegerToText => &[Integer],
            Self::FloatToText => &[Float],
            Self::TextToInteger | Self::TextToFloat | Self::TextTrim => &[String],
            Self::TextReplace => &[String, String, String],
            Self::Variance | Self::StdDev | Self::Sigmoid | Self::Relu | Self::Tanh => &[Vector],
            Self::SampleArea | Self::SampleSlopes | Self::SampleCumulativeArea => &[Vector, Vector],
            Self::IntsLength | Self::IntsSum | Self::IntsToVector => &[Integers],
            Self::IntsWindow | Self::IntsSet => &[Integers, Integer, Integer],
            Self::IntsJoin => &[Integers, Integers],
            Self::IntsRange => &[Integer, Integer, Integer],
            Self::IntsFromVector => &[Vector],
            Self::ToFloat => &[Integer],
            Self::ToInteger => &[Float],
            Self::TextLength | Self::TextBytes => &[String],
            Self::TextCodepoint | Self::TextRepeat => &[String, Integer],
            Self::TextSlice => &[String, Integer, Integer],
            Self::TextConcat => &[String, String],
            Self::TextFind | Self::TextContains | Self::TextStartsWith | Self::TextEndsWith => {
                &[String, String]
            }
            Self::Length
            | Self::Sum
            | Self::Mean
            | Self::Softmax
            | Self::Norm
            | Self::Rms
            | Self::Normalize
            | Self::LogSumExp
            | Self::LogSoftmax
            | Self::ArgMax
            | Self::ArgMin
            | Self::MaxValue
            | Self::MinValue => &[Vector],
            Self::Dot
            | Self::Mse
            | Self::Add
            | Self::Stitch
            | Self::CosineSimilarity
            | Self::PearsonCorrelation => &[Vector, Vector],
            Self::Window => &[Vector, Integer, Integer],
            Self::Scale => &[Vector, Float],
            Self::Clamp => &[Vector, Float, Float],
            Self::Fill => &[Float, Integer],
            Self::LogProb | Self::CrossEntropy | Self::LossGrad | Self::TopK | Self::TopKValues => {
                &[Vector, Integer]
            }
            Self::SoftmaxTemp | Self::LogSoftmaxTemp | Self::TopP | Self::TopPValues => {
                &[Vector, Float]
            }
            Self::OneHot => &[Integer, Integer],
            Self::Tile => &[Vector, Integer, Integer],
            Self::Height | Self::Width | Self::Unroll | Self::Turn => &[Tile],
            Self::Cell => &[Tile, Integer, Integer],
            Self::Lane => &[Tile, Integer],
            Self::MatVec => &[Tile, Vector],
            Self::MatMul | Self::TileAdd => &[Tile, Tile],
            Self::TileScale => &[Tile, Float],
        }
    }

    pub fn result_type(self) -> ValueType {
        match self {
            Self::Decimal | Self::DecimalFromInteger | Self::DecimalAbs
            | Self::DecimalRound | Self::DecimalDivRounded => ValueType::Decimal,
            Self::DecimalToInteger => ValueType::Integer,
            Self::DecimalToText | Self::DecimalFormat => ValueType::String,
            Self::BigInt
            | Self::BigIntFromInteger
            | Self::BigIntAbs
            | Self::BigIntPow
            | Self::BigIntGcd
            | Self::BigIntLcm
            | Self::BigIntSqrt
            | Self::BigIntPowMod
            | Self::BigIntFromRadix
            | Self::BigIntAnd
            | Self::BigIntOr
            | Self::BigIntXor
            | Self::BigIntNot
            | Self::BigIntShl
            | Self::BigIntShr => ValueType::BigInt,
            Self::BigIntBitLength => ValueType::Integer,
            Self::BigIntToText | Self::BigIntToRadix => ValueType::String,
            Self::BigIntToInteger => ValueType::Integer,
            Self::IntegerToText | Self::FloatToText | Self::TextTrim | Self::TextReplace => {
                ValueType::String
            }
            Self::TextToInteger => ValueType::Integer,
            Self::TextToFloat => ValueType::Float,
            Self::Variance
            | Self::StdDev
            | Self::SampleArea
            | Self::CosineSimilarity
            | Self::PearsonCorrelation => ValueType::Float,
            Self::IntsLength | Self::IntsSum => ValueType::Integer,
            Self::IntsWindow
            | Self::IntsJoin
            | Self::IntsSet
            | Self::IntsRange
            | Self::IntsFromVector
            | Self::TopK
            | Self::TopP => ValueType::Integers,
            Self::ToInteger
            | Self::TextLength
            | Self::TextBytes
            | Self::TextCodepoint
            | Self::TextFind => ValueType::Integer,
            Self::ToFloat => ValueType::Float,
            Self::TextConcat | Self::TextSlice | Self::TextRepeat => ValueType::String,
            Self::TextContains | Self::TextStartsWith | Self::TextEndsWith => ValueType::Boolean,
            Self::Length | Self::ArgMax | Self::ArgMin | Self::Height | Self::Width => {
                ValueType::Integer
            }
            Self::Sum
            | Self::Mean
            | Self::Dot
            | Self::Mse
            | Self::Norm
            | Self::Rms
            | Self::LogSumExp
            | Self::MaxValue
            | Self::MinValue
            | Self::LogProb
            | Self::CrossEntropy
            | Self::Cell => ValueType::Float,
            Self::Tile | Self::Turn | Self::MatMul | Self::TileAdd | Self::TileScale => {
                ValueType::Tile
            }
            _ => ValueType::Vector,
        }
    }

    pub(crate) fn is_tile(self) -> bool {
        matches!(
            self,
            Self::Tile
                | Self::Height
                | Self::Width
                | Self::Cell
                | Self::Lane
                | Self::Unroll
                | Self::Turn
                | Self::MatVec
                | Self::MatMul
                | Self::TileAdd
                | Self::TileScale
        )
    }

    fn is_decimal(self) -> bool {
        matches!(self, Self::Decimal | Self::DecimalFromInteger | Self::DecimalToInteger
            | Self::DecimalToText | Self::DecimalAbs | Self::DecimalRound
            | Self::DecimalFormat | Self::DecimalDivRounded)
    }

    fn is_bigint(self) -> bool {
        matches!(
            self,
            Self::BigInt
                | Self::BigIntFromInteger
                | Self::BigIntToInteger
                | Self::BigIntAbs
                | Self::BigIntPow
                | Self::BigIntBitLength
                | Self::BigIntToText
                | Self::BigIntGcd
                | Self::BigIntLcm
                | Self::BigIntSqrt
                | Self::BigIntPowMod
                | Self::BigIntFromRadix
                | Self::BigIntToRadix
                | Self::BigIntAnd
                | Self::BigIntOr
                | Self::BigIntXor
                | Self::BigIntNot
                | Self::BigIntShl
                | Self::BigIntShr
        )
    }

    fn is_scalar_text(self) -> bool {
        matches!(
            self,
            Self::ToFloat
                | Self::ToInteger
                | Self::TextLength
                | Self::TextBytes
                | Self::TextCodepoint
                | Self::TextFind
                | Self::TextContains
                | Self::TextStartsWith
                | Self::TextEndsWith
        )
    }
}

/// Evaluate a native kernel with checked argument types and an explicit work
/// cap. This host API charges kernel work, excluding language dispatch tasks;
/// host diagnostics use an empty source span. Inputs remain immutable.
pub fn evaluate(
    function: Builtin,
    arguments: &[Value],
    max_steps: u64,
) -> Result<Value, Diagnostic> {
    evaluate_with_budget(
        function,
        arguments,
        &mut Budget::new(max_steps),
        Span::default(),
    )
}

/// Standalone built-in with all configured work and logical allocation budgets.
pub fn evaluate_with_options(
    function: Builtin,
    arguments: &[Value],
    options: crate::ExecutionOptions,
) -> Result<Value, Diagnostic> {
    evaluate_with_budget(
        function,
        arguments,
        &mut Budget::new(options.limits.max_steps)
            .with_vector_bytes(options.max_vector_bytes)
            .with_text_bytes(options.max_text_bytes)
            .with_tuple_bytes(options.max_tuple_bytes)
            .with_bigint_limits(options.max_bigint_digits, options.max_bigint_bytes),
        Span::default(),
    )
}

/// Run a standalone built-in with both the step and cumulative numerical
/// payload-allocation limits. Existing `evaluate` keeps its historical
/// step-only host contract; it does not allocate any user program frames.
pub fn evaluate_with_vector_limit(
    function: Builtin,
    arguments: &[Value],
    max_steps: u64,
    max_vector_bytes: usize,
) -> Result<Value, Diagnostic> {
    evaluate_with_budget(
        function,
        arguments,
        &mut Budget::new(max_steps).with_vector_bytes(max_vector_bytes),
        Span::default(),
    )
}

pub(crate) fn length_error(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::VectorLength,
        span,
        "vector lengths must match and stay within 0..65536 elements",
    )
}

fn window_bounds(span: Span) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::VectorIndex,
        span,
        "window start/count exceed the visible vector bounds",
    )
}

fn domain(span: Span, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::NumericDomain, span, message)
}

pub(crate) fn buffer<T>(
    count: usize,
    budget: &mut Budget,
    span: Span,
) -> Result<Vec<T>, Diagnostic> {
    if count > MAX_VECTOR_LENGTH {
        return Err(length_error(span));
    }
    // Charge the exact logical element payload before any allocation occurs.
    // A failed reservation ends execution; as with work steps, no refund is
    // necessary and no partially prepared output becomes visible.
    let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| {
        Diagnostic::new(
            DiagnosticKind::VectorAllocation,
            span,
            "collection payload size overflow",
        )
    })?;
    budget.charge_vector_allocation(bytes, span)?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| {
        Diagnostic::new(
            DiagnosticKind::VectorAllocation,
            span,
            "cannot reserve the bounded vector buffer",
        )
    })?;
    Ok(values)
}

pub(crate) fn vector(values: Vec<FiniteFloat>, span: Span) -> Result<Value, Diagnostic> {
    NumericVector::from_finite(values)
        .map(Value::Vector)
        .ok_or_else(|| length_error(span))
}

// Neumaier's ordered compensation keeps small cancellation terms. Every stored
// intermediate stays finite; this is not an exact or overflow-proof sum.
fn compensated(
    values: impl Iterator<Item = Result<f64, Diagnostic>>,
    span: Span,
) -> Result<f64, Diagnostic> {
    let mut sum: f64 = 0.0;
    let mut correction = 0.0;
    for value in values {
        let value = value?;
        let next = finite_number(sum + value, span)?.get();
        let delta = if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction = finite_number(correction + delta, span)?.get();
        sum = next;
    }
    Ok(finite_number(sum + correction, span)?.get())
}

fn mean(values: &NumericVector, span: Span) -> Result<f64, Diagnostic> {
    let floats = || values.as_slice().iter().map(|value| value.get());
    if let Ok(sum) = compensated(floats().map(Ok), span) {
        return Ok(sum / values.len() as f64);
    }
    // Average terms before summing to retain tiny cancellation residuals that
    // maximum-magnitude normalization could underflow away.
    if let Ok(average) = compensated(floats().map(|value| Ok(value / values.len() as f64)), span) {
        return Ok(average);
    }
    // Preserve the compensated fast path for cancellation/subnormals, but avoid
    // overflowing a sum when only its finite mean was requested.
    let scale = floats().map(f64::abs).fold(0.0, f64::max);
    let normalized = compensated(floats().map(|value| Ok(value / scale)), span)?;
    let fraction = (normalized / values.len() as f64).clamp(-1.0, 1.0);
    Ok(finite_number(fraction * scale, span)?.get())
}

fn mse(left: &NumericVector, right: &NumericVector, span: Span) -> Result<f64, Diagnostic> {
    let differences = || {
        left.as_slice()
            .iter()
            .zip(right.as_slice())
            .map(|(a, b)| finite_number(a.get() - b.get(), span).map(FiniteFloat::get))
    };
    let mut scale: f64 = 0.0;
    for difference in differences() {
        scale = scale.max(difference?.abs());
    }
    if scale == 0.0 {
        return Ok(0.0);
    }
    let squares = differences().map(|difference| {
        let normalized = difference? / scale;
        Ok(normalized * normalized)
    });
    let average = compensated(squares, span)? / left.len() as f64;
    // Average before rescaling: scale^2 itself need not be representable.
    Ok(finite_number((scale * average) * scale, span)?.get())
}

// Divide by the largest magnitude before squaring so finite input vectors
// can be normalized even if their unscaled squared sum or L2 norm overflows.
// The second component is sqrt(sum((x / scale)^2)); it lies in [1, sqrt(n)]
// for nonzero inputs and remains finite under the vector length bound.
fn scaled_l2(values: &NumericVector, span: Span) -> Result<(f64, f64), Diagnostic> {
    let scale = values
        .as_slice()
        .iter()
        .map(|value| value.get().abs())
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Ok((0.0, 0.0));
    }
    let squares = values.as_slice().iter().map(|value| {
        let normalized = value.get() / scale;
        Ok(normalized * normalized)
    });
    Ok((scale, compensated(squares, span)?.sqrt()))
}

// Maximum-shifted log normalization keeps the exponential sum finite and
// nonzero without materializing a second vector. For a finite input vector,
// shifted terms are <= 0; a negatively overflowing difference represents a
// negligible contribution (exp(-infinity) = 0), not a nonfinite stored value.
// Log outputs are individually checked at the call site.
fn log_partition(values: &NumericVector, span: Span) -> Result<(f64, f64), Diagnostic> {
    let maximum = values
        .as_slice()
        .iter()
        .map(|value| value.get())
        .fold(f64::NEG_INFINITY, f64::max);
    let terms = values.as_slice().iter().map(|value| {
        let shifted = value.get() - maximum;
        Ok(if shifted.is_finite() {
            shifted.exp()
        } else {
            0.0
        })
    });
    let sum = compensated(terms, span)?;
    // The maximum always contributes 1.0, so sum is in [1, vector length].
    Ok((maximum, sum.ln()))
}

// Validate a categorical index before any scan or buffer creation.
fn checked_target(index: i64, count: usize, span: Span) -> Result<usize, Diagnostic> {
    usize::try_from(index)
        .ok()
        .filter(|&i| i < count)
        .ok_or_else(|| {
            Diagnostic::new(
                DiagnosticKind::VectorIndex,
                span,
                format!("target index {index} is outside the vector length {count}"),
            )
            .with_help("provide a zero-based target index within the logits vector")
        })
}

pub fn evaluate_with_text_limit(
    function: Builtin,
    arguments: &[Value],
    max_steps: u64,
    max_text_bytes: usize,
) -> Result<Value, Diagnostic> {
    evaluate_with_budget(
        function,
        arguments,
        &mut Budget::new(max_steps).with_text_bytes(max_text_bytes),
        Span::default(),
    )
}

pub(crate) fn evaluate_with_budget(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
) -> Result<Value, Diagnostic> {
    evaluate_with_spans(function, args, budget, span, |_| span)
}

pub(crate) fn evaluate_with_spans(
    function: Builtin,
    args: &[Value],
    budget: &mut Budget,
    span: Span,
    argument_span: impl Fn(usize) -> Span,
) -> Result<Value, Diagnostic> {
    let types = function.parameter_types();
    if args.len() != types.len() {
        return Err(Diagnostic::new(
            DiagnosticKind::CallArguments,
            span,
            format!("expected {} arguments, found {}", types.len(), args.len()),
        ));
    }
    for (i, (value, &expected)) in args.iter().zip(types).enumerate() {
        if value.kind() != expected {
            return Err(crate::value::type_mismatch(
                argument_span(i),
                expected,
                value.kind(),
            ));
        }
    }
    if function.is_bigint() {
        return bigints::evaluate(function, args, budget, span, argument_span);
    }
    if function.is_decimal() {
        return decimals::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(
        function,
        Builtin::IntsLength
            | Builtin::IntsSum
            | Builtin::IntsWindow
            | Builtin::IntsJoin
            | Builtin::IntsSet
            | Builtin::IntsRange
            | Builtin::IntsToVector
            | Builtin::IntsFromVector
    ) {
        return integers::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(
        function,
        Builtin::TextConcat
            | Builtin::TextSlice
            | Builtin::TextRepeat
            | Builtin::TextTrim
            | Builtin::TextReplace
    ) {
        return text::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(
        function,
        Builtin::IntegerToText
            | Builtin::FloatToText
            | Builtin::TextToInteger
            | Builtin::TextToFloat
    ) {
        return parse_format::evaluate(function, args, budget, span, argument_span);
    }
    if function.is_scalar_text() {
        return scalar_text::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(
        function,
        Builtin::Variance | Builtin::StdDev | Builtin::Sigmoid | Builtin::Relu | Builtin::Tanh
    ) {
        return kernels::evaluate(function, args, budget, span, argument_span);
    }
    if function == Builtin::CosineSimilarity {
        return similarity::evaluate(args, budget, span, argument_span);
    }
    if function == Builtin::PearsonCorrelation {
        return correlation::evaluate(args, budget, span, argument_span);
    }
    if matches!(
        function,
        Builtin::SampleArea | Builtin::SampleSlopes | Builtin::SampleCumulativeArea
    ) {
        return samples::evaluate(function, args, budget, span, argument_span);
    }
    if function.is_tile() {
        return tiles::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(function, Builtin::TopK | Builtin::TopKValues) {
        return topk::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(function, Builtin::TopP | Builtin::TopPValues) {
        return nucleus::evaluate(function, args, budget, span, argument_span);
    }
    if matches!(function, Builtin::SoftmaxTemp | Builtin::LogSoftmaxTemp) {
        return temperature::evaluate(function, args, budget, span, argument_span);
    }
    if function == Builtin::Fill {
        let count = usize::try_from(args[1].require_integer(span)?)
            .ok()
            .filter(|&count| count <= MAX_VECTOR_LENGTH)
            .ok_or_else(|| length_error(span))?;
        budget.charge(count as u64, span)?;
        let value = finite_number(args[0].require_float(span)?, span)?;
        let mut values = buffer(count, budget, span)?;
        values.resize(count, value);
        return vector(values, span);
    }
    if function == Builtin::OneHot {
        let count = usize::try_from(args[0].require_integer(span)?)
            .ok()
            .filter(|&count| count <= MAX_VECTOR_LENGTH)
            .ok_or_else(|| length_error(span))?;
        if count == 0 {
            return Err(domain(span, "onehot requires at least one class"));
        }
        let index = args[1].require_integer(span)?;
        let chosen = checked_target(index, count, span)?;
        budget.charge(count as u64, span)?;
        let mut values = buffer(count, budget, span)?;
        values.resize(count, finite_number(0.0, span)?);
        values[chosen] = finite_number(1.0, span)?;
        return vector(values, span);
    }
    let left = args[0].require_vector(span)?;
    if function == Builtin::Window {
        let start =
            usize::try_from(args[1].require_integer(span)?).map_err(|_| window_bounds(span))?;
        let count = usize::try_from(args[2].require_integer(span)?)
            .ok()
            .filter(|&count| count <= MAX_VECTOR_LENGTH)
            .ok_or_else(|| length_error(span))?;
        // A window only shares an Arc and adjusts a visible range:
        // constant native work, no newly reserved numerical element buffer.
        budget.charge(1, span)?;
        return left
            .window(start, count)
            .map(Value::Vector)
            .ok_or_else(|| window_bounds(span));
    }
    if function == Builtin::Stitch {
        let right = args[1].require_vector(span)?;
        let count = left
            .len()
            .checked_add(right.len())
            .filter(|&count| count <= MAX_VECTOR_LENGTH)
            .ok_or_else(|| length_error(span))?;
        // Copy only the visible slices, not the complete backing buffers.
        // Work and cumulative payload are both charged before allocation.
        budget.charge(count as u64, span)?;
        let mut values = buffer(count, budget, span)?;
        values.extend_from_slice(left.as_slice());
        values.extend_from_slice(right.as_slice());
        return vector(values, span);
    }
    if function == Builtin::Length {
        return Ok(Value::Integer(left.len() as i64));
    }
    let right = if matches!(function, Builtin::Dot | Builtin::Mse | Builtin::Add) {
        let right = args[1].require_vector(span)?;
        if left.len() != right.len() {
            return Err(length_error(span));
        }
        Some(right)
    } else {
        None
    };
    if function == Builtin::Rms && left.is_empty() {
        return Err(domain(argument_span(0), "rms requires a nonempty vector")
            .with_help("provide at least one vector element; an empty window has no RMS"));
    }
    if matches!(
        function,
        Builtin::Mean
            | Builtin::Mse
            | Builtin::Softmax
            | Builtin::Normalize
            | Builtin::LogSumExp
            | Builtin::LogSoftmax
            | Builtin::ArgMax
            | Builtin::ArgMin
            | Builtin::MaxValue
            | Builtin::MinValue
            | Builtin::LogProb
            | Builtin::CrossEntropy
            | Builtin::LossGrad
    ) && left.is_empty()
    {
        return Err(domain(
            span,
            "this numerical kernel requires a nonempty vector",
        ));
    }
    // A label is an index, never a probability or a float. Validate its
    // bounds before the numerical scan so no invalid label reaches the
    // target lookup. A negative index is invalid, not Python-style indexing.
    let target = if matches!(
        function,
        Builtin::LogProb | Builtin::CrossEntropy | Builtin::LossGrad
    ) {
        Some(checked_target(
            args[1].require_integer(span)?,
            left.len(),
            span,
        )?)
    } else {
        None
    };
    let work = match function {
        Builtin::LossGrad => 6,
        Builtin::Mean
        | Builtin::Mse
        | Builtin::Softmax
        | Builtin::Norm
        | Builtin::Rms
        | Builtin::Normalize
        | Builtin::LogSumExp
        | Builtin::LogSoftmax
        | Builtin::LogProb
        | Builtin::CrossEntropy => 4,
        Builtin::Dot => 2,
        _ => 1,
    };
    budget.charge(left.len() as u64 * work, span)?;
    let floats = || left.as_slice().iter().map(|value| value.get());
    match function {
        Builtin::LossGrad => {
            // d crossentropy(logits, target) / d logits_i =
            // softmax(logits)_i - indicator(i == target).
            // The maximum-shifted log partition avoids a second
            // probability vector and tolerates negative infinite *shifts*
            // without storing any nonfinite Scarp values.
            let (maximum, log_sum) = log_partition(left, span)?;
            let mut values = buffer(left.len(), budget, span)?;
            for (index, value) in floats().enumerate() {
                let shifted = value - maximum;
                let probability = if shifted.is_finite() {
                    (shifted - log_sum).exp()
                } else {
                    0.0
                };
                let gradient = if Some(index) == target {
                    probability - 1.0
                } else {
                    probability
                };
                values.push(finite_number(gradient, span)?);
            }
            vector(values, span)
        }
        Builtin::LogProb | Builtin::CrossEntropy => {
            let (maximum, log_sum) = log_partition(left, span)?;
            let target_score = left.as_slice()[target.unwrap()].get();
            // Subtract the target from the maximum *before* adding the log
            // partition. Computing logsumexp(logits) - target directly loses
            // tiny but meaningful losses when scores are very large.
            //
            // Both differences and the final output must be finite. A true
            // real loss larger than f64::MAX is deliberately E0031, not +inf.
            let gap = finite_number(maximum - target_score, span)?.get();
            let loss = finite_number(gap + log_sum, span)?.get();
            finite_number(
                if function == Builtin::LogProb {
                    -loss
                } else {
                    loss
                },
                span,
            )
            .map(Value::Float)
        }
        Builtin::ArgMax | Builtin::ArgMin | Builtin::MaxValue | Builtin::MinValue => {
            // Keep the earliest index on equal values, including the two
            // signed zero encodings. No extra buffer, host call or allocation
            // occurs; selection runs only after the entire work fee is paid.
            let mut chosen = 0;
            for (index, candidate) in left.as_slice().iter().enumerate().skip(1) {
                let current = left.as_slice()[chosen].get();
                let better = if matches!(function, Builtin::ArgMax | Builtin::MaxValue) {
                    candidate.get() > current
                } else {
                    candidate.get() < current
                };
                if better {
                    chosen = index;
                }
            }
            if matches!(function, Builtin::ArgMax | Builtin::ArgMin) {
                Ok(Value::Integer(chosen as i64))
            } else {
                Ok(Value::Float(left.as_slice()[chosen]))
            }
        }
        Builtin::LogSumExp | Builtin::LogSoftmax => {
            let (maximum, log_sum) = log_partition(left, span)?;
            if function == Builtin::LogSumExp {
                return finite_number(maximum + log_sum, span).map(Value::Float);
            }
            let mut values = buffer(left.len(), budget, span)?;
            for value in floats() {
                // The exact log probability can be too negative to fit in
                // binary64; reject it instead of silently storing -infinity.
                values.push(finite_number((value - maximum) - log_sum, span)?);
            }
            vector(values, span)
        }
        Builtin::Norm | Builtin::Normalize | Builtin::Rms => {
            let (scale, magnitude) = scaled_l2(left, span)?;
            if function == Builtin::Rms {
                // Divide the scaled magnitude before restoring the input scale:
                // the RMS stays finite even when the L2 norm would overflow.
                let fraction = (magnitude / (left.len() as f64).sqrt()).clamp(0.0, 1.0);
                return finite_number(scale * fraction, span).map(Value::Float);
            }
            if function == Builtin::Norm {
                return finite_number(scale * magnitude, span).map(Value::Float);
            }
            if scale == 0.0 {
                return Err(domain(span, "cannot normalize a zero vector"));
            }
            let mut values = buffer(left.len(), budget, span)?;
            for value in floats() {
                values.push(finite_number((value / scale) / magnitude, span)?);
            }
            vector(values, span)
        }
        Builtin::Sum => compensated(floats().map(Ok), span)
            .and_then(|v| finite_number(v, span))
            .map(Value::Float),
        Builtin::Mean => mean(left, span)
            .and_then(|v| finite_number(v, span))
            .map(Value::Float),
        Builtin::Mse => mse(left, right.unwrap(), span)
            .and_then(|v| finite_number(v, span))
            .map(Value::Float),
        Builtin::Dot => {
            let products = floats()
                .zip(right.unwrap().as_slice())
                .map(|(a, b)| finite_number(a * b.get(), span).map(FiniteFloat::get));
            compensated(products, span)
                .and_then(|v| finite_number(v, span))
                .map(Value::Float)
        }
        Builtin::Add | Builtin::Scale | Builtin::Clamp => {
            let factor = if function == Builtin::Scale {
                args[1].require_float(span)?
            } else {
                0.0
            };
            let (low, high) = if function == Builtin::Clamp {
                (args[1].require_float(span)?, args[2].require_float(span)?)
            } else {
                (0.0, 0.0)
            };
            if low > high {
                return Err(domain(span, "clamp lower bound exceeds its upper bound"));
            }
            let mut values = buffer(left.len(), budget, span)?;
            for (index, value) in floats().enumerate() {
                let result = match function {
                    Builtin::Add => value + right.unwrap().as_slice()[index].get(),
                    Builtin::Scale => value * factor,
                    _ => {
                        if value < low {
                            low
                        } else if value > high {
                            high
                        } else {
                            value
                        }
                    }
                };
                values.push(finite_number(result, span)?);
            }
            vector(values, span)
        }
        Builtin::Softmax => {
            let maximum = floats().fold(f64::NEG_INFINITY, f64::max);
            let mut values = buffer(left.len(), budget, span)?;
            for value in floats() {
                let shifted = value - maximum;
                // A negative overflowing shift has a negligible exponential.
                values.push(finite_number(
                    if shifted.is_finite() {
                        shifted.exp()
                    } else {
                        0.0
                    },
                    span,
                )?);
            }
            let normalizer = compensated(values.iter().map(|v| Ok(v.get())), span)?;
            for value in &mut values {
                *value = finite_number(value.get() / normalizer, span)?;
            }
            vector(values, span)
        }
        Builtin::Length
        | Builtin::Decimal
        | Builtin::DecimalFromInteger
        | Builtin::DecimalToInteger
        | Builtin::DecimalToText
        | Builtin::DecimalAbs
        | Builtin::DecimalRound
        | Builtin::DecimalFormat
        | Builtin::DecimalDivRounded
        | Builtin::BigInt
        | Builtin::BigIntFromInteger
        | Builtin::BigIntToInteger
        | Builtin::BigIntAbs
        | Builtin::BigIntPow
        | Builtin::BigIntBitLength
        | Builtin::BigIntToText
        | Builtin::BigIntGcd
        | Builtin::BigIntLcm
        | Builtin::BigIntSqrt
        | Builtin::BigIntPowMod
        | Builtin::BigIntFromRadix
        | Builtin::BigIntToRadix
        | Builtin::BigIntAnd
        | Builtin::BigIntOr
        | Builtin::BigIntXor
        | Builtin::BigIntNot
        | Builtin::BigIntShl
        | Builtin::BigIntShr
        | Builtin::IntegerToText
        | Builtin::FloatToText
        | Builtin::TextToInteger
        | Builtin::TextToFloat
        | Builtin::TextTrim
        | Builtin::TextReplace
        | Builtin::Variance
        | Builtin::StdDev
        | Builtin::CosineSimilarity
        | Builtin::PearsonCorrelation
        | Builtin::SampleArea
        | Builtin::SampleSlopes
        | Builtin::SampleCumulativeArea
        | Builtin::Sigmoid
        | Builtin::Relu
        | Builtin::Tanh
        | Builtin::IntsLength
        | Builtin::IntsSum
        | Builtin::IntsWindow
        | Builtin::IntsJoin
        | Builtin::IntsSet
        | Builtin::IntsRange
        | Builtin::IntsToVector
        | Builtin::IntsFromVector
        | Builtin::ToFloat
        | Builtin::ToInteger
        | Builtin::TextConcat
        | Builtin::TextSlice
        | Builtin::TextRepeat
        | Builtin::TextLength
        | Builtin::TextBytes
        | Builtin::TextCodepoint
        | Builtin::TextFind
        | Builtin::TextContains
        | Builtin::TextStartsWith
        | Builtin::TextEndsWith
        | Builtin::Fill
        | Builtin::OneHot
        | Builtin::TopK
        | Builtin::TopKValues
        | Builtin::TopP
        | Builtin::TopPValues
        | Builtin::SoftmaxTemp
        | Builtin::LogSoftmaxTemp
        | Builtin::Window
        | Builtin::Stitch
        | Builtin::Tile
        | Builtin::Height
        | Builtin::Width
        | Builtin::Cell
        | Builtin::Lane
        | Builtin::Unroll
        | Builtin::Turn
        | Builtin::MatVec
        | Builtin::MatMul
        | Builtin::TileAdd
        | Builtin::TileScale => unreachable!("handled before vector kernels"),
    }
}
