#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    Int { signed: bool, bits: u8 },
    Float { bits: u8 },
    Bool,
    Void,
    Pointer(Box<Type>),
    Text,
    NumberList,
    List(Box<Type>),
}
impl Type {
    pub fn is_managed(&self) -> bool {
        matches!(self, Self::Text | Self::List(_))
    }
    pub fn is_list_element(&self) -> bool {
        matches!(
            self,
            Self::Int {
                bits: 8 | 16 | 32 | 64,
                ..
            } | Self::Float { bits: 32 | 64 }
                | Self::Bool
                | Self::Text
        )
    }
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int { .. } | Self::Float { .. })
    }
    pub fn int_bounds(&self) -> Option<(i128, i128)> {
        match self {
            Self::Int { signed: true, bits } if [8, 16, 32, 64].contains(bits) => {
                Some((-(1i128 << (*bits - 1)), (1i128 << (*bits - 1)) - 1))
            }
            Self::Int {
                signed: false,
                bits,
            } if [8, 16, 32, 64].contains(bits) => Some((0, (1i128 << *bits) - 1)),
            _ => None,
        }
    }
}
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int { signed, bits } => write!(f, "{}{bits}", if *signed { "ص" } else { "ط" }),
            Self::Float { bits } => write!(f, "ع{bits}"),
            Self::Bool => write!(f, "منطقي"),
            Self::Void => write!(f, "فراغ"),
            Self::Pointer(t) => write!(f, "مؤشر<{t}>"),
            Self::Text => write!(f, "نص"),
            Self::List(t) => write!(f, "قائمة<{t}>"),
            Self::NumberList => write!(f, "قائمة"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Abi {
    Lisan,
    C,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSig {
    pub params: Vec<Type>,
    pub result: Type,
    pub abi: Abi,
}
