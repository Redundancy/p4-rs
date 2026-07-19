//! Bringing the workspace to a revision: `sync`.

use crate::client::{Client, UserInterface};
use crate::commands::files::parse_records;
use crate::commands::helpers::{from_str_value, opt_from_str, optional_string};
use crate::errors::Error;
use serde::Deserialize;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Default)]
pub struct Options {
    force: bool,
    preview: bool,
    max: Option<u32>,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-f`: force resync, ignoring the have-table.
    pub fn force(mut self) -> Self {
        self.force = true;
        self
    }

    /// `-n`: preview only -- report what would sync without touching the disk.
    pub fn preview(mut self) -> Self {
        self.preview = true;
        self
    }

    /// `-m <n>`: sync at most `n` files.
    pub fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.force {
            args.push("-f".to_string());
        }
        if self.preview {
            args.push("-n".to_string());
        }
        if let Some(max) = self.max {
            args.push("-m".to_string());
            args.push(max.to_string());
        }
        args
    }
}

/// What `sync` did to a file. Open-ended: server wording has grown over time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncAction {
    Added,
    Updated,
    Refreshed,
    Replaced,
    Deleted,
    Other(String),
}

impl FromStr for SyncAction {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "added" => SyncAction::Added,
            "updated" => SyncAction::Updated,
            "refreshed" => SyncAction::Refreshed,
            "replaced" => SyncAction::Replaced,
            "deleted" => SyncAction::Deleted,
            other => SyncAction::Other(other.to_string()),
        })
    }
}

impl Display for SyncAction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SyncAction::Added => "added",
            SyncAction::Updated => "updated",
            SyncAction::Refreshed => "refreshed",
            SyncAction::Replaced => "replaced",
            SyncAction::Deleted => "deleted",
            SyncAction::Other(s) => s,
        })
    }
}

/// One file touched by `sync`.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncedFile {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    /// Local filesystem path.
    #[serde(rename = "clientFile")]
    pub client_file: String,

    #[serde(rename = "action", deserialize_with = "from_str_value")]
    pub action: SyncAction,

    /// The revision synced to.
    #[serde(rename = "rev", default, deserialize_with = "opt_from_str")]
    pub rev: Option<u64>,

    #[serde(rename = "fileSize", default, deserialize_with = "opt_from_str")]
    pub file_size: Option<u64>,

    /// The submitting changelist of the synced revision.
    #[serde(rename = "change", default, deserialize_with = "optional_string")]
    pub change: Option<String>,
}

impl Client {
    /// Sync the whole workspace to the head revision (`p4 sync`).
    pub fn sync(&mut self, options: &Options) -> Result<Vec<SyncedFile>, Error> {
        self.sync_paths(&[], options)
    }

    /// Sync specific paths (`p4 sync <paths>`), which may include revision
    /// specifiers (e.g. `//depot/f#3`, `//depot/...@2024`).
    pub fn sync_paths(
        &mut self,
        paths: &[&str],
        options: &Options,
    ) -> Result<Vec<SyncedFile>, Error> {
        let mut ui = UserInterface::new();
        let mut args = options.get_args();
        args.extend(paths.iter().map(|p| p.to_string()));
        let records = self.run_records(&mut ui, "sync", args)?;
        parse_records(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    // Captured verbatim from `p4 sync -f`.
    #[test]
    fn synced_file_from_record() {
        let m: HashMap<String, String> = [
            ("action", "refreshed"),
            ("change", "1"),
            ("clientFile", "C:\\ws\\hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("fileSize", "12"),
            ("rev", "1"),
            ("totalFileCount", "1"),
            ("totalFileSize", "12"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let s = SyncedFile::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize sync record");
        assert_eq!(s.depot_file, "//depot/hello.txt");
        assert_eq!(s.action, SyncAction::Refreshed);
        assert_eq!(s.rev, Some(1));
        assert_eq!(s.file_size, Some(12));
    }

    #[test]
    fn sync_action_open_ended() {
        assert_eq!("added".parse::<SyncAction>().unwrap(), SyncAction::Added);
        assert_eq!(
            "purged".parse::<SyncAction>().unwrap(),
            SyncAction::Other("purged".to_string())
        );
    }

    #[test]
    fn options_build_args() {
        assert_eq!(
            Options::new().force().preview().max(3).get_args(),
            vec!["-f", "-n", "-m", "3"]
        );
    }
}
