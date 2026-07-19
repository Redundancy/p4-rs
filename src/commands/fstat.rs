//! File metadata: `fstat` -- the structured status of files in the depot and
//! workspace. This is the richest per-file record; the fields modeled here are
//! the commonly-used ones, and any others the server sends are ignored.

use crate::client::{Client, UserInterface};
use crate::commands::files::{ChangelistId, OpenAction, parse_records};
use crate::commands::helpers::{opt_from_str, optional_string};
use crate::errors::Error;
use serde::{Deserialize, Deserializer};

#[derive(Debug, Default)]
pub struct Options {
    max: Option<u32>,
}

impl Options {
    pub fn new() -> Options {
        Options::default()
    }

    /// `-m <n>`: report at most `n` files.
    pub fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    pub fn get_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(max) = self.max {
            args.push("-m".to_string());
            args.push(max.to_string());
        }
        args
    }
}

/// A file's status from `p4 fstat`.
///
/// `head*` fields describe the depot head revision; `have_rev` and the `action`
/// group describe the workspace. Fields absent from a given record deserialize
/// as `None`/`false`.
#[derive(Debug, Clone, Deserialize)]
pub struct FileStat {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    /// Local filesystem path; absent when the file is not mapped into the
    /// current client.
    #[serde(rename = "clientFile", default, deserialize_with = "optional_string")]
    pub client_file: Option<String>,

    /// Whether the file is mapped into the current client's view.
    #[serde(rename = "isMapped", default, deserialize_with = "flag")]
    pub is_mapped: bool,

    /// The action of the head revision (`add`/`edit`/`delete`/...).
    #[serde(rename = "headAction", default, deserialize_with = "opt_from_str")]
    pub head_action: Option<OpenAction>,

    /// The changelist that submitted the head revision.
    #[serde(rename = "headChange", default, deserialize_with = "opt_from_str")]
    pub head_change: Option<u64>,

    /// The head revision number.
    #[serde(rename = "headRev", default, deserialize_with = "opt_from_str")]
    pub head_rev: Option<u64>,

    /// The head revision's Perforce file type.
    #[serde(rename = "headType", default, deserialize_with = "optional_string")]
    pub head_type: Option<String>,

    /// Submit time of the head revision, epoch seconds.
    #[serde(rename = "headTime", default, deserialize_with = "opt_from_str")]
    pub head_time: Option<u64>,

    /// Modification time of the head revision, epoch seconds.
    #[serde(rename = "headModTime", default, deserialize_with = "opt_from_str")]
    pub head_mod_time: Option<u64>,

    /// The revision the workspace holds (`None` if not synced).
    #[serde(rename = "haveRev", default, deserialize_with = "opt_rev")]
    pub have_rev: Option<u64>,

    /// If the file is open in this workspace, what it is open for.
    #[serde(rename = "action", default, deserialize_with = "opt_from_str")]
    pub action: Option<OpenAction>,

    /// If open, the changelist it is open in.
    #[serde(rename = "change", default, deserialize_with = "opt_changelist")]
    pub change: Option<ChangelistId>,

    /// The file's size in bytes, when reported.
    #[serde(rename = "fileSize", default, deserialize_with = "opt_from_str")]
    pub file_size: Option<u64>,

    /// The head revision's content digest (MD5), when reported.
    #[serde(rename = "digest", default, deserialize_with = "optional_string")]
    pub digest: Option<String>,
}

/// Empty-value presence flag: the key exists (any value) => true; `#[serde(default)]`
/// supplies false when the key is absent.
fn flag<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let _ = String::deserialize(d)?;
    Ok(true)
}

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

fn opt_changelist<'de, D>(d: D) -> Result<Option<ChangelistId>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    s.parse().map(Some).map_err(serde::de::Error::custom)
}

impl Client {
    /// Status of files matching `paths` (`p4 fstat <paths>`).
    pub fn fstat(&mut self, paths: &[&str], options: &Options) -> Result<Vec<FileStat>, Error> {
        let mut ui = UserInterface::new();
        let mut args = options.get_args();
        args.extend(paths.iter().map(|p| p.to_string()));
        let records = self.run_records(&mut ui, "fstat", args)?;
        parse_records(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    fn de(pairs: &[(&str, &str)]) -> FileStat {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        FileStat::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize fstat record")
    }

    // Captured verbatim from `p4 fstat //depot/...` on a submitted file.
    #[test]
    fn fstat_from_submitted_record() {
        let s = de(&[
            ("clientFile", "C:\\ws\\hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("haveRev", "1"),
            ("headAction", "add"),
            ("headChange", "1"),
            ("headModTime", "1784475848"),
            ("headRev", "1"),
            ("headTime", "1784475848"),
            ("headType", "text"),
            ("isMapped", ""),
        ]);
        assert_eq!(s.depot_file, "//depot/hello.txt");
        assert!(s.is_mapped, "isMapped present => mapped");
        assert_eq!(s.head_action, Some(OpenAction::Add));
        assert_eq!(s.head_change, Some(1));
        assert_eq!(s.head_rev, Some(1));
        assert_eq!(s.head_type.as_deref(), Some("text"));
        assert_eq!(s.head_time, Some(1_784_475_848));
        assert_eq!(s.have_rev, Some(1));
        // Not open, so the open-file group is empty.
        assert!(s.action.is_none());
        assert!(s.change.is_none());
    }

    #[test]
    fn fstat_of_open_file() {
        let s = de(&[
            ("depotFile", "//depot/a.txt"),
            ("headRev", "2"),
            ("haveRev", "2"),
            ("action", "edit"),
            ("change", "default"),
            ("type", "text"),
        ]);
        assert_eq!(s.action, Some(OpenAction::Edit));
        assert_eq!(s.change, Some(ChangelistId::Default));
    }

    #[test]
    fn fstat_of_unmapped_unsynced_file() {
        // A depot file not in this client's view and never synced: no
        // clientFile, isMapped absent (=> false), no haveRev.
        let s = de(&[
            ("depotFile", "//other/x.txt"),
            ("headAction", "add"),
            ("headRev", "1"),
        ]);
        assert!(!s.is_mapped);
        assert!(s.client_file.is_none());
        assert!(s.have_rev.is_none());
    }
}
