//! Opening files for change: `add`, `edit`, `delete`, and `revert`.
//!
//! `add`/`edit`/`delete` all report the same per-file record shape
//! ([`FileAction`]); `revert` reports a distinct shape ([`RevertedFile`]) since
//! it undoes an open rather than creating one.
//!
//! Note on `clientFile`: for these commands the server reports it as the
//! *local filesystem path*, not the `//client/...` syntax that `opened`/`have`
//! use -- so it is kept as a plain `String` here.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, opt_from_str, optional_string};
use crate::errors::Error;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

/// Deserialize a revision field that the server reports as `"none"` when the
/// workspace holds no revision (e.g. after reverting an `add`): `"none"` and
/// the empty string map to `None`, a number to `Some`.
fn opt_rev<'de, D>(d: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    if s == "none" || s.is_empty() {
        return Ok(None);
    }
    s.parse::<u64>().map(Some).map_err(serde::de::Error::custom)
}

/// The `action` recorded when a file is opened. Open-ended (`Other`) so a new
/// server action never makes a whole listing fail to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAction {
    Add,
    Edit,
    Delete,
    Branch,
    Integrate,
    MoveAdd,
    MoveDelete,
    Other(String),
}

impl FromStr for OpenAction {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "add" => OpenAction::Add,
            "edit" => OpenAction::Edit,
            "delete" => OpenAction::Delete,
            "branch" => OpenAction::Branch,
            "integrate" => OpenAction::Integrate,
            "move/add" => OpenAction::MoveAdd,
            "move/delete" => OpenAction::MoveDelete,
            other => OpenAction::Other(other.to_string()),
        })
    }
}

impl Display for OpenAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OpenAction::Add => "add",
            OpenAction::Edit => "edit",
            OpenAction::Delete => "delete",
            OpenAction::Branch => "branch",
            OpenAction::Integrate => "integrate",
            OpenAction::MoveAdd => "move/add",
            OpenAction::MoveDelete => "move/delete",
            OpenAction::Other(s) => s,
        })
    }
}

/// One file opened by `add`/`edit`/`delete`.
#[derive(Debug, Clone, Deserialize)]
pub struct FileAction {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    /// Local filesystem path of the file in the workspace.
    #[serde(rename = "clientFile")]
    pub client_file: String,

    #[serde(rename = "action", deserialize_with = "from_str_value")]
    pub action: OpenAction,

    /// The Perforce file type (e.g. `text`, `binary`, `symlink`).
    #[serde(rename = "type", default, deserialize_with = "optional_string")]
    pub file_type: Option<String>,

    /// The revision now open in the workspace.
    #[serde(rename = "workRev", default, deserialize_with = "opt_from_str")]
    pub work_rev: Option<u64>,
}

/// One file restored by `revert`.
#[derive(Debug, Clone, Deserialize)]
pub struct RevertedFile {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    #[serde(rename = "clientFile")]
    pub client_file: String,

    /// The revision the workspace is left at (`None` if the file was never
    /// synced, e.g. reverting an `add`).
    #[serde(rename = "haveRev", default, deserialize_with = "opt_rev")]
    pub have_rev: Option<u64>,

    /// What the file was open for before the revert (`edit`, `add`, ...).
    #[serde(rename = "oldAction", default, deserialize_with = "opt_from_str")]
    pub old_action: Option<OpenAction>,
}

/// Deserialize a `Vec<T>` of per-file records, carrying the raw map on error.
pub(crate) fn parse_records<T>(records: Vec<HashMap<String, String>>) -> Result<Vec<T>, Error>
where
    T: for<'de> Deserialize<'de>,
{
    records
        .into_iter()
        .map(|m| {
            T::deserialize(serde::de::value::MapDeserializer::new(
                m.clone().into_iter(),
            ))
            .map_err(|e| Error::SerializationError(e, m))
        })
        .collect()
}

impl Client {
    /// Open new files for add (`p4 add`). Paths are local filesystem paths.
    pub fn add(&mut self, paths: &[&str]) -> Result<Vec<FileAction>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "add", str_args(paths))?;
        parse_records(records)
    }

    /// Open existing files for edit (`p4 edit`).
    pub fn edit(&mut self, paths: &[&str]) -> Result<Vec<FileAction>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "edit", str_args(paths))?;
        parse_records(records)
    }

    /// Open files for delete (`p4 delete`).
    pub fn delete(&mut self, paths: &[&str]) -> Result<Vec<FileAction>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "delete", str_args(paths))?;
        parse_records(records)
    }

    /// Discard open files, restoring them (`p4 revert`).
    pub fn revert(&mut self, paths: &[&str]) -> Result<Vec<RevertedFile>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "revert", str_args(paths))?;
        parse_records(records)
    }
}

/// Turn borrowed path arguments into the owned `Vec<String>` run_records wants.
pub(crate) fn str_args(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
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

    fn de<T: for<'de> Deserialize<'de>>(m: HashMap<String, String>) -> T {
        T::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize record")
    }

    // Captured verbatim from `p4 add` against a live p4d.
    #[test]
    fn file_action_from_add_record() {
        let fa: FileAction = de(record(&[
            ("action", "add"),
            ("clientFile", "C:\\ws\\hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("type", "text"),
            ("workRev", "1"),
        ]));
        assert_eq!(fa.depot_file, "//depot/hello.txt");
        assert_eq!(fa.action, OpenAction::Add);
        assert_eq!(fa.file_type.as_deref(), Some("text"));
        assert_eq!(fa.work_rev, Some(1));
    }

    // Captured verbatim from `p4 revert`.
    #[test]
    fn reverted_file_from_record() {
        let rf: RevertedFile = de(record(&[
            ("action", "reverted"),
            ("clientFile", "C:\\ws\\hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("haveRev", "1"),
            ("oldAction", "edit"),
        ]));
        assert_eq!(rf.depot_file, "//depot/hello.txt");
        assert_eq!(rf.have_rev, Some(1));
        assert_eq!(rf.old_action, Some(OpenAction::Edit));
    }

    #[test]
    fn open_action_is_open_ended_and_round_trips() {
        assert_eq!(
            "move/add".parse::<OpenAction>().unwrap(),
            OpenAction::MoveAdd
        );
        assert_eq!(OpenAction::MoveDelete.to_string(), "move/delete");
        assert_eq!(
            "purge".parse::<OpenAction>().unwrap(),
            OpenAction::Other("purge".to_string())
        );
    }

    #[test]
    fn revert_of_added_file_has_no_have_rev() {
        // Reverting an `add` leaves nothing in the workspace: haveRev "none".
        let rf: RevertedFile = de(record(&[
            ("clientFile", "C:\\ws\\new.txt"),
            ("depotFile", "//depot/new.txt"),
            ("haveRev", "none"),
            ("oldAction", "add"),
        ]));
        assert_eq!(rf.have_rev, None);
        assert_eq!(rf.old_action, Some(OpenAction::Add));
    }
}
