//! Listing open files: `opened`.
//!
//! Unlike `add`/`edit`, `opened` reports `clientFile` in `//client/...` syntax
//! (not a local path), and includes the changelist each file is open in.

use crate::client::{Client, UserInterface};
use crate::commands::files::{ChangelistId, OpenAction, parse_records};
use crate::commands::helpers::{from_str_value, opt_from_str, optional_string};
use crate::errors::Error;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Default)]
pub struct Options {
    all_clients: bool,
    change: Option<u64>,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-a`: list open files across all clients, not just the current one.
    pub fn all_clients(mut self) -> Self {
        self.all_clients = true;
        self
    }

    /// `-c <n>`: only files open in the given pending changelist.
    pub fn change(mut self, change: u64) -> Self {
        self.change = Some(change);
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.all_clients {
            args.push("-a".to_string());
        }
        if let Some(change) = self.change {
            args.push("-c".to_string());
            args.push(change.to_string());
        }
        args
    }
}

/// `default` or a number in tagged output.
fn changelist<'de, D>(d: D) -> Result<ChangelistId, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

/// One open file from `p4 opened`.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenedFile {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    /// `//client/...` syntax (not a local path, unlike `add`/`edit`).
    #[serde(rename = "clientFile")]
    pub client_file: String,

    #[serde(rename = "action", deserialize_with = "from_str_value")]
    pub action: OpenAction,

    /// The changelist the file is open in (`default` or numbered).
    #[serde(rename = "change", deserialize_with = "changelist")]
    pub change: ChangelistId,

    /// The revision open in the workspace.
    #[serde(rename = "rev", default, deserialize_with = "opt_from_str")]
    pub rev: Option<u64>,

    /// The revision synced (`None` when never synced -- reported as `none`).
    #[serde(rename = "haveRev", default, deserialize_with = "have_rev")]
    pub have_rev: Option<u64>,

    #[serde(rename = "type", default, deserialize_with = "optional_string")]
    pub file_type: Option<String>,

    #[serde(rename = "user", default, deserialize_with = "optional_string")]
    pub user: Option<String>,

    #[serde(rename = "client", default, deserialize_with = "optional_string")]
    pub client: Option<String>,
}

fn have_rev<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    if s == "none" || s.is_empty() {
        return Ok(None);
    }
    s.parse::<u64>().map(Some).map_err(serde::de::Error::custom)
}

impl Client {
    /// List open files (`p4 opened`).
    pub fn opened(&mut self, options: &Options) -> Result<Vec<OpenedFile>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "opened", options.get_args())?;
        parse_records(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    // Captured verbatim from `p4 opened` after `add`.
    #[test]
    fn opened_file_from_record() {
        let m: HashMap<String, String> = [
            ("action", "add"),
            ("change", "default"),
            ("client", "fc-ws"),
            ("clientFile", "//fc-ws/hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("haveRev", "none"),
            ("rev", "1"),
            ("type", "text"),
            ("user", "danrs"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let f = OpenedFile::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize opened record");
        assert_eq!(f.depot_file, "//depot/hello.txt");
        assert_eq!(f.action, OpenAction::Add);
        assert_eq!(f.change, ChangelistId::Default);
        assert_eq!(f.rev, Some(1));
        assert_eq!(f.have_rev, None); // "none" for a not-yet-synced add
        assert_eq!(f.user.as_deref(), Some("danrs"));
    }

    #[test]
    fn opened_in_numbered_change() {
        let m: HashMap<String, String> = [
            ("action", "edit"),
            ("change", "7"),
            ("clientFile", "//ws/a.txt"),
            ("depotFile", "//depot/a.txt"),
            ("haveRev", "3"),
            ("rev", "3"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let f = OpenedFile::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .unwrap();
        assert_eq!(f.change, ChangelistId::Number(7));
        assert_eq!(f.change.number(), Some(7));
        assert_eq!(f.have_rev, Some(3));
    }

    #[test]
    fn options_build_args() {
        assert!(Options::new().get_args().is_empty());
        assert_eq!(
            Options::new().all_clients().change(5).get_args(),
            vec!["-a", "-c", "5"]
        );
    }
}
