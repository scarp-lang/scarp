use scarp::native_object::{
    emit_elf64_bounded_integer_function, emit_elf64_checked_integer_function,
    emit_freestanding_function, FreestandingFunctionMode, FreestandingTarget,
    FREESTANDING_X86_64_MAX_FRAME_BYTES,
};

const SCALAR: &str = "pure funcra f(n: integer) -> integer { return n + 1; }";
const LOOP: &str = "pure funcra f(n: integer) -> integer {
    var total = 0;
    var i = n;
    while i > 0 { total += i; i -= 1; }
    return total;
}";

#[test]
fn target_identifier_is_exact_and_never_falls_back_to_host() {
    let target: FreestandingTarget = "x86_64-none".parse().unwrap();
    assert_eq!(target, FreestandingTarget::X86_64None);
    assert_eq!(target.name(), "x86_64-none");
    assert_eq!(target.call_stack_alignment(), 16);
    assert_eq!(
        target.max_frame_bytes(),
        FREESTANDING_X86_64_MAX_FRAME_BYTES
    );
    for invalid in [
        "",
        "X86_64-none",
        "x86_64-none ",
        "x86_64-unknown-linux-gnu",
        "aarch64-none",
        "wasm32",
    ] {
        assert!(invalid.parse::<FreestandingTarget>().is_err(), "{invalid}");
    }
}

#[test]
fn explicit_checked_target_preserves_native_object_and_failure_spans() {
    let target = FreestandingTarget::X86_64None;
    let mode = FreestandingFunctionMode::Checked;
    let artifact = emit_freestanding_function(SCALAR, target, mode).unwrap();
    assert_eq!(artifact.target, target);
    assert_eq!(artifact.mode, mode);
    assert_eq!(artifact.mode.symbol(), "scarp_checked_integer_function");
    assert_eq!(
        artifact.kernel,
        emit_elf64_checked_integer_function(SCALAR).unwrap()
    );
    assert!(emit_freestanding_function(LOOP, target, mode).is_err());
}

#[test]
fn explicit_bounded_target_preserves_allowance_and_abi() {
    let target = FreestandingTarget::X86_64None;
    for max_work in [0, 1, 1000, u32::MAX] {
        let mode = FreestandingFunctionMode::Bounded { max_work };
        let artifact = emit_freestanding_function(LOOP, target, mode).unwrap();
        assert_eq!(artifact.mode, mode);
        assert_eq!(artifact.mode.symbol(), "scarp_bounded_integer_function");
        assert_eq!(
            artifact.kernel,
            emit_elf64_bounded_integer_function(LOOP, max_work).unwrap()
        );
    }
}

#[test]
fn explicit_target_does_not_enable_host_effects() {
    let target = FreestandingTarget::X86_64None;
    for mode in [
        FreestandingFunctionMode::Checked,
        FreestandingFunctionMode::Bounded { max_work: 1000 },
    ] {
        for source in [
            "print(1);",
            "funcra f() -> integer { return 1; }",
            "pure funcra f(n: integer) -> integer { return f(n); }",
        ] {
            assert!(emit_freestanding_function(source, target, mode).is_err());
        }
    }
}

#[test]
fn reported_stack_layout_matches_emitted_small_frame_and_argument_count() {
    let target = FreestandingTarget::X86_64None;
    for mode in [
        FreestandingFunctionMode::Checked,
        FreestandingFunctionMode::Bounded { max_work: 1000 },
    ] {
        for count in [0, 1, 6, 7, 16] {
            let parameters = (0..count)
                .map(|n| format!("a{n}: integer"))
                .collect::<Vec<_>>()
                .join(", ");
            let source = format!("pure funcra f({parameters}) -> integer {{ return 42; }}");
            let artifact = emit_freestanding_function(&source, target, mode).unwrap();
            let layout = artifact.layout;
            assert_eq!(layout.integer_arguments, count);
            assert_eq!(layout.stack_argument_bytes(), count.saturating_sub(6) * 8);
            assert_eq!(layout.callee_stack_bytes() % 16, 0);
            assert!(layout.frame_bytes as usize <= target.max_frame_bytes());
            // Read .text offset from the ELF section header, then independently
            // inspect the actual sub rsp, imm32 instruction for these small frames.
            let bytes = &artifact.kernel.object;
            let shoff = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
            let offset = shoff + 64 + 24;
            let text =
                u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
            assert_eq!(&bytes[text..text + 3], &[0x48, 0x81, 0xec]);
            let emitted =
                u32::from_le_bytes(bytes[text + 3..text + 7].try_into().unwrap());
            assert_eq!(layout.frame_bytes, emitted);
        }
    }
}
