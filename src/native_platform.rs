//! Source-visible capabilities of each native execution environment.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeProfile {
    Scalar,
    Firmware,
    Kernel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeStatement {
    Pause,
    Fence,
    Breakpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapField {
    Type,
    Physical,
    Virtual,
    Pages,
    Attributes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeValueOp {
    Load64,
    Exchange64,
    CompareExchange64,
    MapCount,
    Map(MapField),
    DebugByte,
    CpuId,
    Timestamp,
    ReadCr3,
}

impl NativeValueOp {
    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Load64 | Self::Map(_) | Self::DebugByte => 1,
            Self::Exchange64 => 2,
            Self::CompareExchange64 | Self::CpuId => 3,
            Self::MapCount | Self::Timestamp | Self::ReadCr3 => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeIntrinsic {
    Statement(NativeStatement),
    Value(NativeValueOp),
}

impl NativeIntrinsic {
    pub(crate) fn named(name: &str) -> Option<Self> {
        use NativeIntrinsic::{Statement, Value};
        Some(match name {
            "x86_pause" => Statement(NativeStatement::Pause),
            "kernel_fence" => Statement(NativeStatement::Fence),
            "x86_breakpoint" => Statement(NativeStatement::Breakpoint),
            "qemu_debug_byte" => Value(NativeValueOp::DebugByte),
            "kernel_load64" => Value(NativeValueOp::Load64),
            "kernel_exchange64" => Value(NativeValueOp::Exchange64),
            "kernel_compare_exchange64" => Value(NativeValueOp::CompareExchange64),
            "kernel_map_count" => Value(NativeValueOp::MapCount),
            "kernel_map_type" => Value(NativeValueOp::Map(MapField::Type)),
            "kernel_map_physical" => Value(NativeValueOp::Map(MapField::Physical)),
            "kernel_map_virtual" => Value(NativeValueOp::Map(MapField::Virtual)),
            "kernel_map_pages" => Value(NativeValueOp::Map(MapField::Pages)),
            "kernel_map_attributes" => Value(NativeValueOp::Map(MapField::Attributes)),
            "x86_cpuid" => Value(NativeValueOp::CpuId),
            "x86_read_tsc" => Value(NativeValueOp::Timestamp),
            "x86_read_cr3" => Value(NativeValueOp::ReadCr3),
            _ => return None,
        })
    }

    pub(crate) const fn available(self, profile: NativeProfile) -> bool {
        matches!(profile, NativeProfile::Kernel)
            || matches!(
                (self, profile),
                (
                    Self::Statement(NativeStatement::Pause),
                    NativeProfile::Firmware
                )
            )
    }

    pub(crate) const fn arity(self) -> usize {
        match self {
            Self::Statement(_) => 0,
            Self::Value(operation) => operation.arity(),
        }
    }
}
