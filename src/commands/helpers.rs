//! Shared serde helpers for deserializing tagged records, whose values are all
//! strings (from `MapDeserializer<String, String>`).

use serde::{Deserialize, Deserializer};
use std::fmt::Display;
use std::str::FromStr;

/// Deserialize a field that is optional-by-absence: when the key is present its
/// value is a plain string (wrap in Some); when absent, `#[serde(default)]`
/// supplies None. Needed because MapDeserializer's string values don't support
/// serde's native Option handling (a present value fails with
/// "invalid type: string, expected option").
pub(crate) fn optional_string<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(d).map(Some)
}

/// Deserialize a typed value from its string form via FromStr -- used for the
/// enums/flag-sets in specs and for numeric fields like epoch timestamps.
pub(crate) fn from_str_value<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    let s = String::deserialize(d)?;
    s.parse::<T>().map_err(serde::de::Error::custom)
}

/// Optional-by-absence variant of [`from_str_value`]; pair with
/// `#[serde(default)]`.
pub(crate) fn opt_from_str<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    from_str_value(d).map(Some)
}
