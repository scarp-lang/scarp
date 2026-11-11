//! Byte spans and source diagnostics. Columns count Unicode scalar values.

use std::fmt;

mod json;
pub use json::write_json_message;

/// A half-open byte range in the original UTF-8 source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
}

fn boundary(source: &str, offset: usize) -> usize {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// Resolve a byte offset to a one-based line and column, including at EOF.
/// Out-of-range and non-boundary offsets are clamped for diagnostic rendering.
pub fn location(source: &str, offset: usize) -> Location {
    let prefix = &source[..boundary(source, offset)];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    Location { line, column }
}

/// Incremental locations for nondecreasing byte offsets, such as token spans.
/// Backward requests reset the scan; all offsets use the same clamping and
/// Unicode scalar column rules as `location`.
pub struct LocationTracker<'a> {
    source: &'a str,
    offset: usize,
    position: Location,
}

impl<'a> LocationTracker<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            position: Location { line: 1, column: 1 },
        }
    }

    pub fn at(&mut self, offset: usize) -> Location {
        let offset = boundary(self.source, offset);
        if offset < self.offset {
            self.offset = 0;
            self.position = Location { line: 1, column: 1 };
        }
        for character in self.source[self.offset..offset].chars() {
            if character == '\n' {
                self.position.line += 1;
                self.position.column = 1;
            } else {
                self.position.column += 1;
            }
        }
        self.offset = offset;
        self.position
    }
}

// One catalogue owns variants, stable codes and developer-facing explanations.
macro_rules! diagnostic_catalogue {
    ($($variant:ident => ($code:literal, $explanation:literal)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum DiagnosticKind { $($variant),+ }
        impl DiagnosticKind {
            /// Every supported diagnostic kind in stable code order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }
            /// Standalone explanation and actionable repair guidance.
            pub const fn explanation(self) -> &'static str {
                match self { $(Self::$variant => $explanation),+ }
            }
            /// Look up an exact canonical code such as E0007.
            pub fn from_code(code: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|kind| kind.code() == code)
            }
        }
    };
}
diagnostic_catalogue! {
    InvalidCharacter => ("E0001", "Invalid source character. Use the documented punctuation and ASCII identifiers; write Unicode text inside strings or comments."),
    UnterminatedString => ("E0002", "Unterminated string. Close the double quote before a physical newline or end of input; use escaped newline text inside a string."),
    InvalidEscape => ("E0003", "Invalid string escape. Use backslash, double quote, n, t or r after a backslash, or \\u{HEX} with one to six ASCII hexadecimal digits for a Unicode scalar value."),
    UnexpectedToken => ("E0004", "Unexpected syntax or invalid binding type. Check punctuation, required initializers, braces and primitive type names at the highlighted token."),
    UnknownStatement => ("E0005", "Unknown statement form. Use bind/var, supported output forms and documented control flow; migrate old let declarations to bind or var."),
    InvalidInteger => ("E0006", "Invalid integer literal or magnitude outside signed 64-bit range. Use decimal, 0x hexadecimal or 0b binary digits from -9223372036854775808 through 9223372036854775807; the minimum magnitude requires direct unary minus. Radix literals do not support separators or fractional parts."),
    ArithmeticOverflow => ("E0007", "Integer arithmetic overflow. Every evaluated intermediate result must fit signed 64-bit, including negation and minimum-integer division by -1. Reduce the operands; a later operation cannot undo an overflow."),
    DivisionByZero => ("E0008", "Division by zero. Supply a nonzero divisor; either sign of floating zero is invalid. Only selected runtime or constant branches are evaluated."),
    ExpressionNesting => ("E0009", "Expression nesting limit exceeded. Reduce nested groups, unary forms, calls or conditional expressions; long flat operator sequences are handled iteratively."),
    UnknownName => ("E0010", "Unknown or inaccessible binding. Declare the name in the current lexical scope before use; functions cannot capture entry-script bindings, and constant initializers cannot read runtime values."),
    DuplicateBinding => ("E0011", "Duplicate binding or module alias collision. Choose a distinct name or a nested lexical scope, and follow the related declaration site."),
    ReservedName => ("E0012", "Reserved binding name. Choose a name outside the language keywords and output forms; identifiers are case-sensitive ASCII names."),
    ImmutableAssignment => ("E0013", "Assignment to an immutable binding. Declare a var when mutation is intended; module constants and repeat indices remain immutable."),
    BlockNesting => ("E0014", "Statement-block nesting limit exceeded. Simplify nested blocks or extract a function without changing scope and control-flow semantics."),
    TypeMismatch => ("E0015", "Incompatible primitive types. Operators, annotations, arguments and assignments require their documented kinds. Use to_float/to_integer only when an explicit exact conversion is intended, and follow related type sites."),
    LoopControl => ("E0016", "Loop control outside its valid context. Use break/continue inside the loop they control; function calls do not inherit a caller's loop."),
    ExecutionLimit => ("E0017", "Execution work budget exceeded. Check loop progress, recursive work and text scan costs; reduce work or explicitly raise --max-steps. The budget counts deterministic tasks, not elapsed time."),
    OutputLimit => ("E0018", "Selected output-statement limit exceeded. Reduce print/write statements or raise --max-prints; check mode enforces the same selected-output budget."),
    RepeatCount => ("E0019", "Invalid counted-repeat value. Supply a nonnegative integer count; the repeat index is immutable and local to the loop."),
    FunctionDefinition => ("E0020", "Invalid or duplicate function declaration. Check names, parameter types/order and defaults; the declared result may be unit, while parameters must be value types."),
    InvalidReturn => ("E0021", "Invalid return context or result type. Return only inside funcra; use a bare return for unit and an expression matching the declared result otherwise."),
    MissingReturn => ("E0022", "A value-returning function has an uncovered path. Return a matching value on every branch or after control flow; a loop alone does not establish a guaranteed return."),
    UnknownFunction => ("E0023", "Unknown or non-callable name. Declare a funcra or use an available built-in with its exact spelling; ordinary bindings are not callable values."),
    CallArguments => ("E0024", "Arguments do not fit the call signature. Check arity, required/default parameters, labels and duplicates. Positional arguments precede named ones; built-ins use positional arguments only."),
    ImpureFunction => ("E0025", "An effect violates a pure context. Pure functions/defaults/contracts cannot call impure user functions; move output to an appropriate runtime caller."),
    ContractViolation => ("E0026", "A needs/ensures condition evaluated to false. Repair the supplied arguments or function result to satisfy the predicate; related call locations identify the invocation."),
    CallLimit => ("E0027", "Function-call count, depth or live-slot allowance exceeded. Check recursion and frame size, then explicitly adjust the named ExecutionOptions or CLI limit when appropriate."),
    FunctionUnsupported => ("E0028", "Unsupported function form or value use. Nested/first-class functions and unsupported captures require a future callable-value design; use a file-level funcra and explicit parameters."),
    CaptureLimit => ("E0029", "Buffered output capture allowance exceeded. Reduce selected output payload or raise --max-capture-bytes; output is prepared before either stream is written."),
    InvalidFloat => ("E0030", "Invalid or nonfinite floating literal. Use a finite binary64 decimal/scientific value; NaN and infinities are not valid Scarp float values."),
    NonFiniteNumber => ("E0031", "A numerical intermediate became nonfinite. Reduce magnitudes or use stable numerical kernels; every stored numerical result must stay finite."),
    VectorLength => ("E0032", "Invalid numerical length or shape. Check matching vector lengths, tile dimensions and kernel-specific nonempty requirements against the related argument locations."),
    VectorIndex => ("E0033", "Numerical vector or tile index outside bounds. Use a nonnegative index within the visible vector window or tile dimensions; negative indexing is not supported."),
    NumericDomain => ("E0034", "Numerical kernel domain requirement failed. Check nonempty inputs, clamp bounds, class indices and the operation's documented mathematical conditions."),
    VectorAllocation => ("E0035", "A bounded numerical buffer could not be reserved. Reduce the requested materialization or host memory pressure; this differs from the configured cumulative payload quota."),
    VectorBudget => ("E0036", "Cumulative numerical payload quota exceeded. Reuse shared immutable windows or reduce materialization, or explicitly raise --max-vector-bytes. This is not a live-heap/process-memory measurement."),
    ModuleSyntax => ("E0037", "Invalid module declaration or constant initializer. Libraries contain imports, functions and immutable scalar constants; calls/vectors/indexing remain forbidden in constant initializers. Put runtime work in a function or entry script."),
    ModuleLoad => ("E0038", "A module source could not be loaded. Check the path, trusted source root, permissions and UTF-8 encoding; explicit loaders own filesystem access."),
    ModuleCycle => ("E0039", "Import or constant dependency cycle. Follow the actual related edges and remove a cycle; local constant initializers may reference only earlier constants."),
    ModuleLimit => ("E0040", "Module-loading, expansion or constant-compilation allowance exceeded. Reduce the graph/expressions or adjust ModuleLimits/ConstantLimits in an embedding host. Runtime --max-steps does not raise compiler limits."),
    ModuleVisibility => ("E0041", "Imported member is private. Use a public wrapper or mark the intended declaration pub in its defining module; aliases do not grant private access."),
    ModuleSymbol => ("E0042", "Unknown alias/member, wrong symbol kind or later constant reference. Check spelling, public declarations and declaration order; functions must be called and constants read as values."),
    ModulePath => ("E0043", "Invalid portable module path. Use a relative slash-separated .srp path inside the trusted root; absolute/platform-specific or escaping paths are rejected."),
    NumericConversion => ("E0044", "Numeric conversion failed or would lose precision. to_float requires exact representability; to_integer requires an integral binary64 value in [-2^63,2^63). The positive i64 boundary rounds upward and is rejected. text_to_integer requires in-range ASCII decimal text; text_to_float requires complete finite decimal syntax and uses binary64 literal rounding. Whitespace, radix prefixes and separators are rejected; trim explicitly."),
    TextIndex => ("E0045", "Text scalar index or slice outside bounds. Use nonnegative indices/counts; single indices must be below text_len(text), while slice start + count may equal it. Indices count Unicode scalar values, not UTF-8 bytes or grapheme clusters."),
    InvalidShift => ("E0046", "Invalid integer shift count. Supply an integer from 0 through 63. Left shift operates on a fixed 64-bit bit pattern, >> extends the sign and >>> fills with zero. Counts are checked rather than masked."),
    TypeComplexity => ("E0047", "Semantic type registry limit exceeded or invalid type limits. Reduce signature/type complexity or adjust TypeLimits in an embedding host. These compiler limits are independent of runtime execution and allocation budgets."),
    TextConstruction => ("E0048", "Invalid or excessive text construction. Repetition counts must be nonnegative, each materialized string is limited to 1048576 UTF-8 bytes, and cumulative new payload is limited by --max-text-bytes / ExecutionOptions.max_text_bytes. Sharing strings does not recharge; retirement does not refund allocation. This is a logical payload cap, not a physical process-memory limit."),
    NativeUnsupported => ("E0049", "Experimental scalar native emission cannot represent this program or would exceed its explicit resource caps. Use interpreter execution, or restrict the source to supported integer/boolean operations, bounded control flow and single-argument stdout lines."),
    SymbolIndexLimit => ("E0050", "Symbol navigation index limit exceeded. Reduce symbol/reference metadata or adjust IndexLimits in Rust embedding. These opt-in limits are separate from compiler module, constant, type, and runtime limits."),
    InputUnavailable => ("E0054", "Program input is unavailable in this execution mode. Supply an explicit buffered reader through the input embedding APIs or redirect stdin to scarp exec. Legacy check never reads input; terminal prompts need a future interactive mode."),
    InputEof => ("E0055", "End of program input before a new line. An empty LF/CRLF-terminated line is valid empty text; a final nonempty unterminated line is valid. Supply another line for each selected read_line call."),
    InputEncoding => ("E0056", "Program input contains invalid UTF-8. Encode input as UTF-8; no replacement, normalization or implicit numeric conversion is performed."),
    InputIo => ("E0057", "The explicit program input reader failed. Check the host reader or redirected input source. Consumed input cannot be rolled back; source and runtime failures still prevent program-output replay."),
    InputLimit => ("E0058", "Program input exceeded a raw per-line byte, cumulative-byte, or selected-read quota, or bounded storage reservation failed. InputLimits and --max-input-* configure these bounds; CR/LF terminators count. Text and work budgets also apply; raising input limits cannot exceed the existing string ceiling."),
    TupleIndex => ("E0059", "Tuple projection requires a nonnegative integer literal within the fixed tuple arity. Use t[0], not a runtime/computed/negative index. Numerical collection indexing retains its existing rules."),
    TupleLimit => ("E0060", "Tuple shape exceeds the supported bounds. Use one through 32 members, tuple depth at most 32 and structural type size at most 4096 nodes. Empty tuples and unit members are unsupported."),
    TupleAllocation => ("E0061", "Tuple payload allocation exceeded its cumulative quota or bounded reservation failed. Reuse immutable tuples or reduce construction; adjust --max-tuple-bytes / ExecutionOptions.max_tuple_bytes explicitly. This is a logical payload quota, not process RAM."),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Span,
    pub help: Option<String>,
    pub related: Vec<RelatedDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedDiagnostic {
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind, span: Span, message: impl Into<String>) -> Self {
        Self {
            kind,
            span,
            message: message.into(),
            help: None,
            related: Vec::new(),
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_related(mut self, span: Span, message: impl Into<String>) -> Self {
        self.related.push(RelatedDiagnostic {
            span,
            message: message.into(),
        });
        self
    }

    /// Render the first affected source line with a caret and optional help.
    pub fn render(&self, filename: &str, source: &str) -> String {
        let mut rendered = format!("{self}\n{}", excerpt(self.span, filename, source));
        for related in &self.related {
            rendered.push_str(&format!(
                "\n = note: {}\n{}",
                related.message,
                excerpt(related.span, filename, source)
            ));
        }
        if let Some(help) = &self.help {
            rendered.push_str(&format!("\n = help: {help}"));
        }
        rendered
    }
}

pub(crate) fn excerpt(span: Span, filename: &str, source: &str) -> String {
    let start = boundary(source, span.start);
    let end = boundary(source, span.end).max(start);
    let position = location(source, start);
    let line_start = source[..start].rfind('\n').map_or(0, |index| index + 1);
    let line_end = source[start..]
        .find('\n')
        .map_or(source.len(), |index| start + index);
    let line = source[line_start..line_end].trim_end_matches('\r');
    let visible_start = start.min(line_start + line.len());
    let visible_end = end.min(line_start + line.len()).max(visible_start);
    let indent = display_width(&source[line_start..visible_start]);
    let width = display_width(&source[visible_start..visible_end]).max(1);
    let gutter = position.line.to_string().len();
    format!(
        " --> {filename}:{}:{}\n{:gutter$} |\n{} | {}\n{:gutter$} | {}{}",
        position.line,
        position.column,
        "",
        position.line,
        display_text(line),
        "",
        " ".repeat(indent),
        "^".repeat(width),
    )
}

fn display_text(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\t' => "    ".to_string(),
            ch if ch.is_control() => ch.escape_default().to_string(),
            ch => ch.to_string(),
        })
        .collect()
}

fn display_width(text: &str) -> usize {
    display_text(text).chars().count()
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.kind.code(), self.message)
    }
}

impl std::error::Error for Diagnostic {}
