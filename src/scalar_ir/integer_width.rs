use crate::value::ValueType;

#[derive(Clone, Copy)]
pub(super) struct IntegerWidth {
    pub bits: u8,
    pub signed: bool,
}

impl IntegerWidth {
    pub(super) fn fixed(kind: ValueType) -> Option<Self> {
        let (bits, signed) = kind.fixed_integer_width()?;
        (bits <= 64).then_some(Self {
            bits: bits as u8,
            signed,
        })
    }

    pub(super) fn of(kind: ValueType) -> Option<Self> {
        if kind == ValueType::Integer {
            Some(Self {
                bits: 64,
                signed: true,
            })
        } else {
            Self::fixed(kind)
        }
    }
}

pub(super) fn scalar(kind: ValueType) -> bool {
    kind == ValueType::Boolean || IntegerWidth::of(kind).is_some()
}
