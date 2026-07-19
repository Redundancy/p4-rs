use crate::client::{Client, UserInterface};
use crate::errors::Error;
use serde::de::value::MapDeserializer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One Perforce counter, as reported by `p4 counters`.
///
/// Counter values are stored by the server as opaque strings -- most are
/// numeric (`change`, `journal`, custom monotonic counters) but there is no
/// guarantee, so `value` stays a `String`. Use [`Counter::as_u64`] when a
/// numeric interpretation is wanted.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Counter {
    /// The counter's name (the server's tagged `counter` key).
    #[serde(rename = "counter")]
    pub name: String,
    /// The counter's raw value.
    pub value: String,
}

impl Counter {
    /// Parse the value as an unsigned integer, or `None` if it is not one.
    pub fn as_u64(&self) -> Option<u64> {
        self.value.parse().ok()
    }
}

/// The `value` of a single counter (`p4 counter <name>`). The get form of the
/// command reports only the value; the name echo is not guaranteed, so this
/// narrower record deserializes just what is always present.
#[derive(Deserialize)]
struct CounterValue {
    value: String,
}

fn deserialize<'a, T: Deserialize<'a>>(m: HashMap<String, String>) -> Result<T, Error> {
    T::deserialize(MapDeserializer::new(m.clone().into_iter()))
        .map_err(|e| Error::SerializationError(e, m))
}

impl Client {
    /// List all counters (`p4 counters`), one typed [`Counter`] per record.
    pub fn counters(&mut self) -> Result<Vec<Counter>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "counters", Vec::new())?;
        records.into_iter().map(deserialize::<Counter>).collect()
    }

    /// Read a single counter's value (`p4 counter <name>`). A counter that does
    /// not exist reports `"0"` -- the server's convention for absent counters --
    /// rather than an error.
    pub fn counter(&mut self, name: &str) -> Result<String, Error> {
        let mut ui = UserInterface::new();
        let mut records = self.run_records(&mut ui, "counter", vec![name.to_string()])?;
        // `counter <name>` produces exactly one record; an empty result
        // deserializes as a missing `value` field via SerializationError.
        let m = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        Ok(deserialize::<CounterValue>(m)?.value)
    }

    /// Set a counter (`p4 counter <name> <value>`), creating it if needed.
    pub fn set_counter(&mut self, name: &str, value: &str) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        self.run_records(
            &mut ui,
            "counter",
            vec![name.to_string(), value.to_string()],
        )?;
        Ok(())
    }

    /// Delete a counter (`p4 counter -d <name>`).
    pub fn delete_counter(&mut self, name: &str) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        self.run_records(&mut ui, "counter", vec!["-d".to_string(), name.to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn counter_deserializes_from_tagged_record() {
        // Verbatim shape of one `p4 counters` record.
        let m = map(&[("counter", "p4rs-test-counter"), ("value", "42")]);
        let c = Counter::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize tagged counter record");
        assert_eq!(c.name, "p4rs-test-counter");
        assert_eq!(c.value, "42");
        assert_eq!(c.as_u64(), Some(42));
    }

    #[test]
    fn as_u64_is_none_for_non_numeric() {
        let c = Counter {
            name: "security".to_string(),
            value: "high".to_string(),
        };
        assert_eq!(c.as_u64(), None);
    }

    #[test]
    fn counter_value_deserializes_value_field() {
        // Verbatim shape of a `p4 counter <name>` get record.
        let m = map(&[("counter", "p4rs-test-counter"), ("value", "43")]);
        let cv = CounterValue::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize counter value record");
        assert_eq!(cv.value, "43");
    }
}
