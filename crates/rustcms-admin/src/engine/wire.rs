//! Wire format between the admin API and its clients (`docs/admin-protocol.md`).
//!
//! Every column the admin exposes has a [`FieldKind`] derived from its SeaORM
//! [`ColumnType`]. Decoding is strict: a value of the wrong JSON shape is a
//! validation error, never a panic or a silent coercion.
//!
//! | kind             | JSON                                   |
//! |------------------|----------------------------------------|
//! | `I32`, `F64`     | number                                 |
//! | `Bool`           | boolean                                |
//! | `I64`, `Decimal` | string (JS numbers lose precision)     |
//! | `String`, `Uuid` | string (uuid lowercase hyphenated)     |
//! | `Date`           | `"YYYY-MM-DD"`                         |
//! | `LocalDateTime`  | `"YYYY-MM-DDTHH:MM:SS[.fff]"`          |
//! | `Instant`        | RFC 3339, always rendered in UTC (`Z`) |
//! | `Json`           | any JSON                               |

use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use rust_decimal::Decimal;
use sea_orm::sea_query::{ColumnType, Value};
use serde_json::Value as Json;
use uuid::Uuid;

/// Wire-level type of an admin field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// Text of any length.
    String,
    /// 32-bit integer, JSON number.
    I32,
    /// 64-bit integer, JSON string.
    I64,
    /// Floating point, JSON number.
    F64,
    /// Boolean.
    Bool,
    /// Exact decimal, JSON string keeping its scale.
    Decimal,
    /// UUID.
    Uuid,
    /// Calendar date without time.
    Date,
    /// Date and time without timezone.
    LocalDateTime,
    /// Point in time (UTC).
    Instant,
    /// Arbitrary JSON document.
    Json,
}

/// Why a JSON value could not be decoded. The message is safe to show to users.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DecodeError(pub String);

fn err<T>(msg: &str) -> Result<T, DecodeError> {
    Err(DecodeError(msg.to_owned()))
}

const LOCAL_DATETIME_FORMATS: [&str; 2] = ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"];

impl FieldKind {
    /// Maps a SeaORM column type. `None` means the type is not supported by the admin in 0.1.
    pub fn from_column_type(ty: &ColumnType) -> Option<Self> {
        Some(match ty {
            ColumnType::Char(_) | ColumnType::String(_) | ColumnType::Text => Self::String,
            ColumnType::Integer => Self::I32,
            ColumnType::BigInteger => Self::I64,
            ColumnType::Float | ColumnType::Double => Self::F64,
            ColumnType::Boolean => Self::Bool,
            ColumnType::Decimal(_) | ColumnType::Money(_) => Self::Decimal,
            ColumnType::Uuid => Self::Uuid,
            ColumnType::Date => Self::Date,
            ColumnType::DateTime | ColumnType::Timestamp => Self::LocalDateTime,
            ColumnType::TimestampWithTimeZone => Self::Instant,
            ColumnType::Json | ColumnType::JsonBinary => Self::Json,
            _ => return None,
        })
    }

    /// Name used as `valueType` in schemas.
    pub fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F64 => "f64",
            Self::Bool => "bool",
            Self::Decimal => "decimal",
            Self::Uuid => "uuid",
            Self::Date => "date",
            Self::LocalDateTime => "local_datetime",
            Self::Instant => "instant",
            Self::Json => "json",
        }
    }

    /// Interprets a value that arrived as a query-string string: numbers and
    /// booleans are parsed for kinds whose wire format is not a string.
    pub fn from_query(self, value: &Json) -> Json {
        match (self, value) {
            (Self::I32 | Self::F64 | Self::Bool | Self::Json, Json::String(s)) => {
                serde_json::from_str(s).unwrap_or_else(|_| value.clone())
            }
            _ => value.clone(),
        }
    }

    /// Typed SQL `NULL` values for this kind.
    ///
    /// Several are returned when the Rust field type is ambiguous (an `Instant`
    /// column may be `DateTime<Utc>` or `DateTime<FixedOffset>`); callers try
    /// them in order with `ActiveModelTrait::try_set`.
    pub fn nulls(self) -> Vec<Value> {
        match self {
            Self::String => vec![Value::String(None)],
            Self::I32 => vec![Value::Int(None)],
            Self::I64 => vec![Value::BigInt(None)],
            Self::F64 => vec![Value::Double(None), Value::Float(None)],
            Self::Bool => vec![Value::Bool(None)],
            Self::Decimal => vec![Value::Decimal(None)],
            Self::Uuid => vec![Value::Uuid(None)],
            Self::Date => vec![Value::ChronoDate(None)],
            Self::LocalDateTime => vec![Value::ChronoDateTime(None)],
            Self::Instant => vec![Value::ChronoDateTimeUtc(None), Value::ChronoDateTimeWithTimeZone(None)],
            Self::Json => vec![Value::Json(None)],
        }
    }

    /// Decodes a JSON value into candidate SeaORM values (see [`FieldKind::nulls`]).
    pub fn decode(self, json: &Json) -> Result<Vec<Value>, DecodeError> {
        if json.is_null() {
            return Ok(self.nulls());
        }
        Ok(match self {
            Self::String => vec![Value::String(Some(expect_str(json)?.to_owned()))],
            Self::I32 => {
                let n = json.as_i64().ok_or(DecodeError("must be an integer".into()))?;
                let n = i32::try_from(n).map_err(|_| DecodeError("is out of range".into()))?;
                vec![Value::Int(Some(n))]
            }
            Self::I64 => vec![Value::BigInt(Some(parse_i64(expect_str(json)?)?))],
            Self::F64 => {
                let n = json.as_f64().ok_or(DecodeError("must be a number".into()))?;
                // An `f32` column only takes values that stay finite as `f32`;
                // without that candidate, `apply` reports the value as out of range.
                let narrow = n as f32;
                let mut candidates = vec![Value::Double(Some(n))];
                if narrow.is_finite() {
                    candidates.push(Value::Float(Some(narrow)));
                }
                candidates
            }
            Self::Bool => vec![Value::Bool(Some(json.as_bool().ok_or(DecodeError("must be true or false".into()))?))],
            Self::Decimal => {
                let d: Decimal =
                    expect_str(json)?.parse().map_err(|_| DecodeError("must be a decimal string".into()))?;
                vec![Value::Decimal(Some(d))]
            }
            Self::Uuid => vec![Value::Uuid(Some(parse_uuid(expect_str(json)?)?))],
            Self::Date => {
                let d = NaiveDate::parse_from_str(expect_str(json)?, "%Y-%m-%d")
                    .map_err(|_| DecodeError("must be a date (YYYY-MM-DD)".into()))?;
                vec![Value::ChronoDate(Some(d))]
            }
            Self::LocalDateTime => vec![Value::ChronoDateTime(Some(parse_local_datetime(expect_str(json)?)?))],
            Self::Instant => {
                let dt = DateTime::<FixedOffset>::parse_from_rfc3339(expect_str(json)?)
                    .map_err(|_| DecodeError("must be an RFC 3339 timestamp".into()))?;
                vec![
                    Value::ChronoDateTimeUtc(Some(dt.with_timezone(&Utc))),
                    Value::ChronoDateTimeWithTimeZone(Some(dt)),
                ]
            }
            Self::Json => vec![Value::Json(Some(Box::new(json.clone())))],
        })
    }

    /// Parses a record key taken from a URL path segment.
    pub fn decode_key(self, key: &str) -> Result<Value, DecodeError> {
        Ok(match self {
            Self::String => Value::String(Some(key.to_owned())),
            Self::I32 => Value::Int(Some(key.parse().map_err(|_| DecodeError("invalid key".into()))?)),
            Self::I64 => Value::BigInt(Some(parse_i64(key)?)),
            Self::Uuid => Value::Uuid(Some(parse_uuid(key)?)),
            _ => return err("unsupported key type"),
        })
    }
}

/// Encodes a SeaORM value following the wire format.
pub fn encode(value: &Value) -> Json {
    match value {
        Value::Bool(v) => v.map(Json::Bool).unwrap_or(Json::Null),
        Value::TinyInt(v) => v.map(Json::from).unwrap_or(Json::Null),
        Value::SmallInt(v) => v.map(Json::from).unwrap_or(Json::Null),
        Value::Int(v) => v.map(Json::from).unwrap_or(Json::Null),
        Value::BigInt(v) => v.map(|n| Json::String(n.to_string())).unwrap_or(Json::Null),
        Value::BigUnsigned(v) => v.map(|n| Json::String(n.to_string())).unwrap_or(Json::Null),
        Value::Float(v) => v.map(Json::from).unwrap_or(Json::Null),
        Value::Double(v) => v.map(Json::from).unwrap_or(Json::Null),
        Value::String(v) => v.clone().map(Json::String).unwrap_or(Json::Null),
        Value::Decimal(v) => v.map(|d| Json::String(d.to_string())).unwrap_or(Json::Null),
        Value::Uuid(v) => v.map(|u| Json::String(u.hyphenated().to_string())).unwrap_or(Json::Null),
        Value::ChronoDate(v) => v.map(|d| Json::String(d.format("%Y-%m-%d").to_string())).unwrap_or(Json::Null),
        Value::ChronoDateTime(v) => v.map(|d| Json::String(format_local(&d))).unwrap_or(Json::Null),
        Value::ChronoDateTimeUtc(v) => v.map(|d| Json::String(format_instant(&d))).unwrap_or(Json::Null),
        Value::ChronoDateTimeWithTimeZone(v) => {
            v.map(|d| Json::String(format_instant(&d.with_timezone(&Utc)))).unwrap_or(Json::Null)
        }
        Value::Json(v) => v.as_deref().cloned().unwrap_or(Json::Null),
        other => {
            tracing::warn!(?other, "value type not supported by the admin wire format");
            Json::Null
        }
    }
}

/// Encodes a column value, normalising decimals to the column's declared scale.
///
/// SQLite stores decimals as REAL and loses trailing zeros (`10.50` → `10.5`);
/// rescaling keeps the wire format identical across databases.
pub fn encode_column(column_type: &ColumnType, value: &Value) -> Json {
    match (column_type, value) {
        (ColumnType::Decimal(Some((_, scale))) | ColumnType::Money(Some((_, scale))), Value::Decimal(Some(d))) => {
            let mut d = d.round_dp(*scale);
            d.rescale(*scale);
            Json::String(d.to_string())
        }
        _ => encode(value),
    }
}

/// Encodes a primary key value as the string used in URLs and envelopes.
pub fn encode_key(value: &Value) -> String {
    match encode(value) {
        Json::String(s) => s,
        other => other.to_string(),
    }
}

fn expect_str(json: &Json) -> Result<&str, DecodeError> {
    json.as_str().ok_or(DecodeError("must be a string".into()))
}

fn parse_i64(s: &str) -> Result<i64, DecodeError> {
    s.parse().map_err(|_| DecodeError("must be an integer string".into()))
}

fn parse_uuid(s: &str) -> Result<Uuid, DecodeError> {
    Uuid::parse_str(s).map_err(|_| DecodeError("must be a UUID".into()))
}

fn parse_local_datetime(s: &str) -> Result<NaiveDateTime, DecodeError> {
    LOCAL_DATETIME_FORMATS
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
        .ok_or(DecodeError("must be a local date-time (YYYY-MM-DDTHH:MM:SS)".into()))
}

fn format_local(d: &NaiveDateTime) -> String {
    d.format("%Y-%m-%dT%H:%M:%S%.f").to_string()
}

fn format_instant(d: &DateTime<Utc>) -> String {
    d.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn i64_is_a_string_on_the_wire() {
        assert_eq!(encode(&Value::BigInt(Some(9_007_199_254_740_993))), json!("9007199254740993"));
        assert_eq!(
            FieldKind::I64.decode(&json!("9007199254740993")).unwrap()[0],
            Value::BigInt(Some(9_007_199_254_740_993))
        );
        assert!(FieldKind::I64.decode(&json!(42)).is_err());
    }

    #[test]
    fn decimal_keeps_scale() {
        let v = FieldKind::Decimal.decode(&json!("10.50")).unwrap();
        assert_eq!(encode(&v[0]), json!("10.50"));
        assert!(FieldKind::Decimal.decode(&json!(10.5)).is_err());
    }

    #[test]
    fn i32_rejects_strings_and_overflow() {
        assert!(FieldKind::I32.decode(&json!("1")).is_err());
        assert!(FieldKind::I32.decode(&json!(1_i64 << 40)).is_err());
        assert!(FieldKind::I32.decode(&json!(1.5)).is_err());
    }

    #[test]
    fn instant_is_normalised_to_utc() {
        let v = FieldKind::Instant.decode(&json!("2026-10-11T21:30:00+07:00")).unwrap();
        assert_eq!(encode(&v[0]), json!("2026-10-11T14:30:00Z"));
        assert_eq!(encode(&v[1]), json!("2026-10-11T14:30:00Z"));
        assert!(FieldKind::Instant.decode(&json!("2026-10-11T21:30:00")).is_err());
    }

    #[test]
    fn local_datetime_and_date() {
        let v = FieldKind::LocalDateTime.decode(&json!("2026-10-11T08:00:00")).unwrap();
        assert_eq!(encode(&v[0]), json!("2026-10-11T08:00:00"));
        let v = FieldKind::LocalDateTime.decode(&json!("2026-10-11T08:00:00.250")).unwrap();
        assert_eq!(encode(&v[0]), json!("2026-10-11T08:00:00.250"));
        let v = FieldKind::Date.decode(&json!("2026-02-28")).unwrap();
        assert_eq!(encode(&v[0]), json!("2026-02-28"));
        assert!(FieldKind::Date.decode(&json!("2026-02-30")).is_err());
    }

    #[test]
    fn decimal_is_rescaled_to_column_scale() {
        let ty = ColumnType::Decimal(Some((10, 2)));
        assert_eq!(encode_column(&ty, &Value::Decimal(Some("10.5".parse().unwrap()))), json!("10.50"));
        assert_eq!(encode_column(&ty, &Value::Decimal(Some("10.555".parse().unwrap()))), json!("10.56"));
        assert_eq!(
            encode_column(&ColumnType::Decimal(None), &Value::Decimal(Some("10.5".parse().unwrap()))),
            json!("10.5")
        );
    }

    #[test]
    fn uuid_is_lowercase() {
        let v = FieldKind::Uuid.decode(&json!("67E55044-10B1-426F-9247-BB680E5FE0C8")).unwrap();
        assert_eq!(encode(&v[0]), json!("67e55044-10b1-426f-9247-bb680e5fe0c8"));
    }

    #[test]
    fn null_yields_typed_nulls() {
        assert_eq!(FieldKind::I64.decode(&Json::Null).unwrap(), vec![Value::BigInt(None)]);
        assert_eq!(encode(&Value::BigInt(None)), Json::Null);
    }

    #[test]
    fn keys_round_trip() {
        assert_eq!(encode_key(&FieldKind::I32.decode_key("42").unwrap()), "42");
        assert_eq!(encode_key(&FieldKind::I64.decode_key("42").unwrap()), "42");
        assert!(FieldKind::I32.decode_key("4x").is_err());
        assert!(FieldKind::Bool.decode_key("true").is_err());
    }

    proptest::proptest! {
        #[test]
        fn i64_round_trips(n: i64) {
            let wire = encode(&Value::BigInt(Some(n)));
            proptest::prop_assert_eq!(FieldKind::I64.decode(&wire).unwrap()[0].clone(), Value::BigInt(Some(n)));
        }

        #[test]
        fn decoding_never_panics(s in ".*", n: f64, b: bool) {
            for kind in [FieldKind::String, FieldKind::I32, FieldKind::I64, FieldKind::F64, FieldKind::Bool,
                         FieldKind::Decimal, FieldKind::Uuid, FieldKind::Date, FieldKind::LocalDateTime,
                         FieldKind::Instant, FieldKind::Json] {
                let _ = kind.decode(&json!(s));
                let _ = kind.decode(&json!(n));
                let _ = kind.decode(&json!(b));
                let _ = kind.decode_key(&s);
            }
        }
    }
}
