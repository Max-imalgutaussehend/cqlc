//! The CQL type model.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CqlType {
    Ascii,
    BigInt,
    Blob,
    Boolean,
    Counter,
    Date,
    Decimal,
    Double,
    Duration,
    Float,
    Inet,
    Int,
    SmallInt,
    Text,
    Time,
    Timestamp,
    TimeUuid,
    TinyInt,
    Uuid,
    VarInt,
    List(Box<CqlType>),
    Set(Box<CqlType>),
    Map(Box<CqlType>, Box<CqlType>),
    Tuple(Vec<CqlType>),
    Frozen(Box<CqlType>),
    Udt(String),
}

impl CqlType {
    pub fn from_primitive(name: &str) -> Option<CqlType> {
        Some(match name {
            "ascii" => CqlType::Ascii,
            "bigint" => CqlType::BigInt,
            "blob" => CqlType::Blob,
            "boolean" => CqlType::Boolean,
            "counter" => CqlType::Counter,
            "date" => CqlType::Date,
            "decimal" => CqlType::Decimal,
            "double" => CqlType::Double,
            "duration" => CqlType::Duration,
            "float" => CqlType::Float,
            "inet" => CqlType::Inet,
            "int" => CqlType::Int,
            "smallint" => CqlType::SmallInt,
            "text" | "varchar" => CqlType::Text,
            "time" => CqlType::Time,
            "timestamp" => CqlType::Timestamp,
            "timeuuid" => CqlType::TimeUuid,
            "tinyint" => CqlType::TinyInt,
            "uuid" => CqlType::Uuid,
            "varint" => CqlType::VarInt,
            _ => return None,
        })
    }

    /// Strips any number of `frozen<>` wrappers.
    pub fn unfrozen(&self) -> &CqlType {
        match self {
            CqlType::Frozen(inner) => inner.unfrozen(),
            other => other,
        }
    }

    pub fn is_numeric(&self) -> bool {
        use CqlType::*;
        matches!(
            self.unfrozen(),
            TinyInt | SmallInt | Int | BigInt | VarInt | Float | Double | Decimal | Counter
        )
    }

    pub fn is_collection(&self) -> bool {
        matches!(
            self.unfrozen(),
            CqlType::List(_) | CqlType::Set(_) | CqlType::Map(..)
        )
    }

    pub fn is_frozen(&self) -> bool {
        matches!(self, CqlType::Frozen(_))
    }

    pub fn contains_udt(&self) -> bool {
        match self {
            CqlType::Udt(_) => true,
            CqlType::List(t) | CqlType::Set(t) | CqlType::Frozen(t) => t.contains_udt(),
            CqlType::Map(k, v) => k.contains_udt() || v.contains_udt(),
            CqlType::Tuple(ts) => ts.iter().any(CqlType::contains_udt),
            _ => false,
        }
    }
}

impl fmt::Display for CqlType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CqlType::Ascii => "ascii",
            CqlType::BigInt => "bigint",
            CqlType::Blob => "blob",
            CqlType::Boolean => "boolean",
            CqlType::Counter => "counter",
            CqlType::Date => "date",
            CqlType::Decimal => "decimal",
            CqlType::Double => "double",
            CqlType::Duration => "duration",
            CqlType::Float => "float",
            CqlType::Inet => "inet",
            CqlType::Int => "int",
            CqlType::SmallInt => "smallint",
            CqlType::Text => "text",
            CqlType::Time => "time",
            CqlType::Timestamp => "timestamp",
            CqlType::TimeUuid => "timeuuid",
            CqlType::TinyInt => "tinyint",
            CqlType::Uuid => "uuid",
            CqlType::VarInt => "varint",
            CqlType::List(t) => return write!(f, "list<{t}>"),
            CqlType::Set(t) => return write!(f, "set<{t}>"),
            CqlType::Frozen(t) => return write!(f, "frozen<{t}>"),
            CqlType::Map(k, v) => return write!(f, "map<{k}, {v}>"),
            CqlType::Tuple(ts) => {
                f.write_str("tuple<")?;
                for (i, t) in ts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{t}")?;
                }
                return f.write_str(">");
            }
            CqlType::Udt(n) => n,
        };
        f.write_str(s)
    }
}
