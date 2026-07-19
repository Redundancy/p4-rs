//! Typed `p4 describe -s` -- a changelist's header plus its affected files
//! (the `-s` "short" form: no diffs).

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, optional_string};
use crate::errors::Error;
use serde::Deserialize;
use std::collections::HashMap;

impl Client {
    /// Describe a changelist (`p4 describe -s <change>`), typed.
    ///
    /// The `-s` form reports the change header and the list of affected files
    /// but no content diffs. A pending change with no open files describes
    /// successfully with an empty file list.
    pub fn describe(&mut self, change: &str) -> Result<Describe, Error> {
        let mut ui = UserInterface::new();
        let args = vec!["-s".to_string(), change.to_string()];
        let mut records = self.run_records(&mut ui, "describe", args)?;
        let record = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        Describe::from_record(record)
    }
}

/// One affected file from `describe -s`, assembled from the parallel indexed
/// record keys (`depotFile0`, `action0`, ...). `file_size`/`digest` are only
/// present for some file types/actions, hence optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribedFile {
    pub depot_file: String,
    pub rev: String,
    pub action: String,
    pub file_type: String,
    pub file_size: Option<String>,
    pub digest: Option<String>,
}

/// A changelist as reported by `describe -s`: the header fields plus the
/// affected files.
///
/// Tagged keys are lowercase here (unlike the `change` spec's Capitalized
/// form). `time` is epoch seconds. `path`/`old_change`/`stream` are conditional
/// on the change and server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Describe {
    #[serde(rename = "change", deserialize_with = "from_str_value")]
    pub change: u64,

    #[serde(rename = "user")]
    pub user: String,

    #[serde(rename = "client")]
    pub client: String,

    /// Submit (or last-modified) time, seconds since the Unix epoch.
    #[serde(rename = "time", deserialize_with = "from_str_value")]
    pub time: u64,

    /// `pending` or `submitted`.
    #[serde(rename = "status")]
    pub status: String,

    /// `public` / `restricted`; not always present.
    #[serde(rename = "changeType", default, deserialize_with = "optional_string")]
    pub change_type: Option<String>,

    /// Full description text (may span multiple lines).
    #[serde(rename = "desc", default)]
    pub desc: String,

    /// Common path prefix of the affected files; present when the server
    /// computes one.
    #[serde(rename = "path", default, deserialize_with = "optional_string")]
    pub path: Option<String>,

    /// For a renumbered submitted change, the original pending number.
    #[serde(rename = "oldChange", default, deserialize_with = "optional_string")]
    pub old_change: Option<String>,

    /// The stream the change was submitted to, when applicable.
    #[serde(rename = "stream", default, deserialize_with = "optional_string")]
    pub stream: Option<String>,

    /// Affected files, from the parallel `depotFile0..`/`action0..`/... keys.
    #[serde(skip)]
    pub files: Vec<DescribedFile>,
}

impl Describe {
    /// Build a typed `Describe` from a tagged `describe -s` record. The
    /// per-file data arrives as parallel indexed arrays keyed by position; they
    /// are pulled out (and removed) before the header is deserialized.
    pub fn from_record(mut record: HashMap<String, String>) -> Result<Describe, Error> {
        record.remove("specFormatted");

        let mut files = Vec::new();
        let mut i = 0;
        // `depotFile{i}` anchors a file entry; the sibling arrays share its
        // index. fileSize/digest are optional per entry.
        while let Some(depot_file) = record.remove(&format!("depotFile{i}")) {
            let rev = record.remove(&format!("rev{i}")).ok_or_else(|| {
                Error::SpecError(format!("describe: missing rev{i} for {depot_file}"))
            })?;
            let action = record.remove(&format!("action{i}")).ok_or_else(|| {
                Error::SpecError(format!("describe: missing action{i} for {depot_file}"))
            })?;
            let file_type = record.remove(&format!("type{i}")).ok_or_else(|| {
                Error::SpecError(format!("describe: missing type{i} for {depot_file}"))
            })?;
            files.push(DescribedFile {
                depot_file,
                rev,
                action,
                file_type,
                file_size: record.remove(&format!("fileSize{i}")),
                digest: record.remove(&format!("digest{i}")),
            });
            i += 1;
        }

        let mut describe = Describe::deserialize(serde::de::value::MapDeserializer::new(
            record.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, record))?;
        describe.files = files;
        Ok(describe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim shape of a pending change with no files (`describe -s <n>`,
    /// p4d 2022.2): lowercase header keys, epoch `time`, no `depotFile*`.
    fn captured_pending_no_files() -> HashMap<String, String> {
        [
            ("change", "5"),
            ("user", "danrs"),
            ("client", "chspec-ws"),
            ("time", "1784461296"),
            ("status", "pending"),
            ("changeType", "public"),
            ("desc", "typed change roundtrip\n"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// Verbatim shape of a submitted change with two files: the header plus
    /// parallel indexed file arrays (fileSize/digest present here).
    fn captured_submitted_with_files() -> HashMap<String, String> {
        [
            ("change", "7"),
            ("user", "danrs"),
            ("client", "chspec-ws"),
            ("time", "1784470000"),
            ("status", "submitted"),
            ("changeType", "public"),
            ("desc", "add two files\n"),
            ("path", "//depot/main/..."),
            ("depotFile0", "//depot/main/a.txt"),
            ("action0", "add"),
            ("type0", "text"),
            ("rev0", "1"),
            ("fileSize0", "12"),
            ("digest0", "5F4DCC3B5AA765D61D8327DEB882CF99"),
            ("depotFile1", "//depot/main/b.txt"),
            ("action1", "add"),
            ("type1", "text"),
            ("rev1", "1"),
            ("fileSize1", "34"),
            ("digest1", "AABBCCDDEEFF00112233445566778899"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn describe_pending_no_files() {
        let d = Describe::from_record(captured_pending_no_files()).expect("parse pending");
        assert_eq!(d.change, 5);
        assert_eq!(d.user, "danrs");
        assert_eq!(d.client, "chspec-ws");
        assert_eq!(d.time, 1_784_461_296);
        assert_eq!(d.status, "pending");
        assert_eq!(d.change_type.as_deref(), Some("public"));
        assert_eq!(d.desc, "typed change roundtrip\n");
        assert!(d.path.is_none());
        assert!(d.files.is_empty());
    }

    #[test]
    fn describe_submitted_parses_indexed_files() {
        let d = Describe::from_record(captured_submitted_with_files()).expect("parse submitted");
        assert_eq!(d.change, 7);
        assert_eq!(d.status, "submitted");
        assert_eq!(d.path.as_deref(), Some("//depot/main/..."));
        assert_eq!(d.files.len(), 2);
        assert_eq!(
            d.files[0],
            DescribedFile {
                depot_file: "//depot/main/a.txt".to_string(),
                rev: "1".to_string(),
                action: "add".to_string(),
                file_type: "text".to_string(),
                file_size: Some("12".to_string()),
                digest: Some("5F4DCC3B5AA765D61D8327DEB882CF99".to_string()),
            }
        );
        assert_eq!(d.files[1].depot_file, "//depot/main/b.txt");
        assert_eq!(d.files[1].file_size.as_deref(), Some("34"));
    }

    #[test]
    fn describe_missing_sibling_key_is_spec_error() {
        // A depotFile without its parallel rev/action/type is malformed input.
        let mut rec = captured_pending_no_files();
        rec.insert("depotFile0".to_string(), "//depot/main/x.txt".to_string());
        let err = Describe::from_record(rec).unwrap_err();
        assert!(matches!(err, Error::SpecError(_)));
    }
}
