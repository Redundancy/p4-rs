//! Typed `p4 changes` -- list changelists (pending, submitted, or shelved).

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, opt_from_str, optional_string};
use crate::errors::Error;
use serde::{Deserialize, Deserializer};
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

impl Client {
    /// List changelists (`p4 changes`), typed. With no options this lists all
    /// changelists the connection can see; narrow it with [`Options`].
    pub fn changes(&mut self, options: &Options) -> Result<Vec<Change>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "changes", options.get_args())?;
        records
            .into_iter()
            .map(|m| {
                Change::deserialize(serde::de::value::MapDeserializer::new(
                    m.clone().into_iter(),
                ))
                .map_err(|e| Error::SerializationError(e, m))
            })
            .collect()
    }
}

/// The state of a changelist, as filtered by `changes -s`. A closed, stable set
/// -- an unknown value is a parse error rather than silently accepted, so a
/// server protocol change is surfaced instead of mis-typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Pending,
    Submitted,
    Shelved,
}

impl FromStr for ChangeStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "pending" => ChangeStatus::Pending,
            "submitted" => ChangeStatus::Submitted,
            "shelved" => ChangeStatus::Shelved,
            other => return Err(format!("unknown change status: {other:?}")),
        })
    }
}

impl Display for ChangeStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChangeStatus::Pending => "pending",
            ChangeStatus::Submitted => "submitted",
            ChangeStatus::Shelved => "shelved",
        })
    }
}

/// Whether a changelist's description is publicly visible or restricted to
/// users with access. A closed, stable set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Public,
    Restricted,
}

impl FromStr for ChangeType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "public" => ChangeType::Public,
            "restricted" => ChangeType::Restricted,
            other => return Err(format!("unknown change type: {other:?}")),
        })
    }
}

impl Display for ChangeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ChangeType::Public => "public",
            ChangeType::Restricted => "restricted",
        })
    }
}

/// Deserialize an empty-value flag key: the server emits the key (e.g.
/// `shelved`) with an empty value to mark a boolean condition. Presence means
/// true; absence (via `#[serde(default)]`) means false.
fn flag_present<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(d).map(|_| true)
}

#[derive(Debug, Default)]
pub struct Options {
    max: Option<u32>,
    status: Option<ChangeStatus>,
    user: Option<String>,
    client: Option<String>,
    long_output: bool,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-m max`: list only the most recent `max` changelists.
    pub fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    /// `-s status`: restrict to changelists in the given state
    /// (`pending`, `submitted`, or `shelved`).
    pub fn status(mut self, status: ChangeStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// `-u user`: restrict to changelists owned by `user`.
    pub fn user(mut self, user: &str) -> Self {
        self.user = Some(user.to_string());
        self
    }

    /// `-c client`: restrict to changelists on workspace `client`.
    pub fn client(mut self, client: &str) -> Self {
        self.client = Some(client.to_string());
        self
    }

    /// `-l`: return full changelist descriptions rather than truncated ones.
    pub fn long_output(mut self) -> Self {
        self.long_output = true;
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(max) = self.max {
            args.push("-m".to_string());
            args.push(max.to_string());
        }
        if let Some(status) = self.status {
            args.push("-s".to_string());
            args.push(status.to_string());
        }
        if let Some(user) = &self.user {
            args.push("-u".to_string());
            args.push(user.clone());
        }
        if let Some(client) = &self.client {
            args.push("-c".to_string());
            args.push(client.clone());
        }
        if self.long_output {
            args.push("-l".to_string());
        }
        args
    }
}

/// One changelist from `p4 changes`, deserialized from the tagged record. In
/// tagged output `time` is epoch seconds (unlike the formatted date the human
/// output shows) and `desc` is truncated unless [`Options::long_output`] was
/// set (it carries a trailing newline either way).
#[derive(Debug, Clone, Deserialize)]
pub struct Change {
    /// The changelist number.
    #[serde(rename = "change", deserialize_with = "from_str_value")]
    pub change: u64,

    /// Creation (pending) or submission time, seconds since the Unix epoch.
    #[serde(rename = "time", deserialize_with = "from_str_value")]
    pub time: u64,

    #[serde(rename = "user")]
    pub user: String,

    #[serde(rename = "client")]
    pub client: String,

    #[serde(rename = "status", deserialize_with = "from_str_value")]
    pub status: ChangeStatus,

    /// Description visibility. Absent on older servers, hence optional.
    #[serde(rename = "changeType", default, deserialize_with = "opt_from_str")]
    pub change_type: Option<ChangeType>,

    /// The changelist description; truncated unless `-l` was requested. Carries
    /// a trailing newline as the server sends it.
    #[serde(rename = "desc", default)]
    pub desc: String,

    /// Common path prefix of the files in the change; present only for
    /// submitted changes and only in some listings.
    #[serde(rename = "path", default, deserialize_with = "optional_string")]
    pub path: Option<String>,

    /// For a renumbered submitted change, its original pending number.
    #[serde(rename = "oldChange", default, deserialize_with = "opt_from_str")]
    pub old_change: Option<u64>,

    /// True when the change has shelved files (server emits the key with an
    /// empty value as a flag).
    #[serde(rename = "shelved", default, deserialize_with = "flag_present")]
    pub shelved: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    fn record(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn deserialize(m: HashMap<String, String>) -> Change {
        Change::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize captured changes record")
    }

    /// Field shapes captured from a real `p4 changes` tagged record for a
    /// pending change (p4d 2025.2).
    #[test]
    fn change_deserializes_from_captured_pending_record() {
        let ch = deserialize(record(&[
            ("change", "1"),
            ("time", "1784461296"),
            ("user", "danrs"),
            ("client", "chg-ws"),
            ("status", "pending"),
            ("changeType", "public"),
            ("desc", "test change one\n"),
        ]));

        assert_eq!(ch.change, 1);
        assert_eq!(ch.time, 1_784_461_296);
        assert_eq!(ch.user, "danrs");
        assert_eq!(ch.client, "chg-ws");
        assert_eq!(ch.status, ChangeStatus::Pending);
        assert_eq!(ch.change_type, Some(ChangeType::Public));
        assert_eq!(ch.desc, "test change one\n");
        assert!(ch.path.is_none());
        assert!(ch.old_change.is_none());
        assert!(!ch.shelved);
    }

    /// A submitted change may carry `path`, `oldChange`, and a `shelved` flag
    /// key (empty value).
    #[test]
    fn change_deserializes_optional_and_flag_fields() {
        let ch = deserialize(record(&[
            ("change", "42"),
            ("time", "1784461300"),
            ("user", "danrs"),
            ("client", "chg-ws"),
            ("status", "submitted"),
            ("changeType", "restricted"),
            ("desc", "did a thing\n"),
            ("path", "//depot/main/..."),
            ("oldChange", "40"),
            ("shelved", ""),
        ]));

        assert_eq!(ch.status, ChangeStatus::Submitted);
        assert_eq!(ch.change_type, Some(ChangeType::Restricted));
        assert_eq!(ch.path.as_deref(), Some("//depot/main/..."));
        assert_eq!(ch.old_change, Some(40));
        assert!(ch.shelved);
    }

    /// changeType is absent on older servers; that must not be fatal.
    #[test]
    fn change_type_absent_is_none() {
        let ch = deserialize(record(&[
            ("change", "7"),
            ("time", "1784461296"),
            ("user", "danrs"),
            ("client", "chg-ws"),
            ("status", "pending"),
            ("desc", "no changeType key\n"),
        ]));
        assert!(ch.change_type.is_none());
    }

    #[test]
    fn change_status_round_trips_and_rejects_unknown() {
        for s in ["pending", "submitted", "shelved"] {
            assert_eq!(s.parse::<ChangeStatus>().unwrap().to_string(), s);
        }
        assert!("abandoned".parse::<ChangeStatus>().is_err());
    }

    #[test]
    fn change_type_round_trips_and_rejects_unknown() {
        for s in ["public", "restricted"] {
            assert_eq!(s.parse::<ChangeType>().unwrap().to_string(), s);
        }
        assert!("secret".parse::<ChangeType>().is_err());
    }

    #[test]
    fn options_build_expected_args() {
        assert!(Options::new().get_args().is_empty());
        assert_eq!(
            Options::new()
                .max(10)
                .status(ChangeStatus::Pending)
                .user("danrs")
                .client("chg-ws")
                .long_output()
                .get_args(),
            vec![
                "-m", "10", "-s", "pending", "-u", "danrs", "-c", "chg-ws", "-l"
            ]
        );
    }
}
