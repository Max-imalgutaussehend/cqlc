//! The CQL type model.

use std::fmt;

/// A CQL data type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CqlType {
    /// `ascii`
    Ascii,
    /// `bigint`
    BigInt,
    /// `blob`
    Blob,
    /// `boolean`
    Boolean,
    /// `counter`
    Counter,
    /// `date`
    Date,
    /// `decimal`
    Decimal,
    /// `double`
    Double,
    /// `duration`
    Duration,
    /// `float`
    Float,
    /// `inet`
    Inet,
    /// `int`
    Int,
    /// `smallint`
    SmallInt,
    /// `text` / `varchar`
    Text,
    /// `time`
    Time,
    /// `timestamp`
    Timestamp,
    /// `timeuuid`
    TimeUuid,
    /// `tinyint`
    TinyInt,
    /// `uuid`
    Uuid,
    /// `varint`
    VarInt,
    /// `list<T>`
    List(Box<CqlType>),
    /// `set<T>`
    Set(Box<CqlType>),
    /// `map<K, V>`
    Map(Box<CqlType>, Box<CqlType>),
    /// `tuple<A, B, ...>`
    Tuple(Vec<CqlType>),
    /// `frozen<T>`
    Frozen(Box<CqlType>),
    /// `vector<T, N>` (Cassandra 5.0)
    Vector(Box<CqlType>, u32),
    /// A user-defined type, by name.
    Udt(String),
}

impl CqlType {
    /// Maps a lower-case primitive type name (`int`, `text`, ...) to its type.
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

    /// Whether the type is a number (including `counter`).
    pub fn is_numeric(&self) -> bool {
        use CqlType::*;
        matches!(
            self.unfrozen(),
            TinyInt | SmallInt | Int | BigInt | VarInt | Float | Double | Decimal | Counter
        )
    }

    /// Whether the type is a list, set or map.
    pub fn is_collection(&self) -> bool {
        matches!(
            self.unfrozen(),
            CqlType::List(_) | CqlType::Set(_) | CqlType::Map(..)
        )
    }

    /// Whether the type is wrapped in `frozen<>`.
    pub fn is_frozen(&self) -> bool {
        matches!(self, CqlType::Frozen(_))
    }

    /// Whether a user-defined type occurs anywhere inside the type.
    pub fn contains_udt(&self) -> bool {
        match self {
            CqlType::Udt(_) => true,
            CqlType::List(t) | CqlType::Set(t) | CqlType::Frozen(t) | CqlType::Vector(t, _) => {
                t.contains_udt()
            }
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
            CqlType::Vector(t, n) => return write!(f, "vector<{t}, {n}>"),
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

/// Sort direction of a clustering column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Order {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

/// Comparison operator of a relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RelOp {
    /// `=`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `IN`
    In,
    /// `CONTAINS`
    Contains,
    /// `CONTAINS KEY`
    ContainsKey,
    /// `LIKE` (SASI indexes)
    Like,
}

impl RelOp {
    /// `<`, `<=`, `>` or `>=`.
    pub fn is_range(self) -> bool {
        matches!(self, RelOp::Lt | RelOp::Le | RelOp::Gt | RelOp::Ge)
    }

    /// `=` or `IN`.
    pub fn is_eq_or_in(self) -> bool {
        matches!(self, RelOp::Eq | RelOp::In)
    }

    /// Short name used to disambiguate parameter names (`gte`, `in`, ...).
    pub fn suffix(self) -> &'static str {
        match self {
            RelOp::Eq => "eq",
            RelOp::Ne => "ne",
            RelOp::Lt => "lt",
            RelOp::Le => "lte",
            RelOp::Gt => "gt",
            RelOp::Ge => "gte",
            RelOp::In => "in",
            RelOp::Contains => "contains",
            RelOp::ContainsKey => "contains_key",
            RelOp::Like => "like",
        }
    }
}

/// How a query's result is exposed in the generated API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum QueryKind {
    /// Zero or one row (`:one`).
    One,
    /// All rows (`:many`).
    Many,
    /// No result rows (`:exec`).
    Exec,
    /// Conditional write with an `applied` result (`:lwt`).
    Lwt,
}
