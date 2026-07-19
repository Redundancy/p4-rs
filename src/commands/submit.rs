//! Committing open files: `submit`.
//!
//! `p4 submit` emits several tagged records: per-file records for each file
//! being committed, and a final record carrying `submittedChange` -- the number
//! the changelist ended up with (a renumber is common, so it is not necessarily
//! the pending number). [`SubmitResult`] pulls those apart.

use crate::client::{Client, UserInterface};
use crate::commands::files::OpenAction;
use crate::commands::helpers::opt_from_str;
use crate::errors::Error;
use serde::Deserialize;

/// One file recorded as part of a submit.
#[derive(Debug, Clone, Deserialize)]
pub struct SubmittedFile {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    #[serde(rename = "action", default, deserialize_with = "opt_from_str")]
    pub action: Option<OpenAction>,

    /// The revision the submit created.
    #[serde(rename = "rev", default, deserialize_with = "opt_from_str")]
    pub rev: Option<u64>,
}

/// The outcome of a `submit`.
#[derive(Debug, Clone)]
pub struct SubmitResult {
    /// The changelist number the submit produced (post-renumber).
    pub change: u64,
    /// The files committed.
    pub files: Vec<SubmittedFile>,
}

impl Client {
    /// Submit the default changelist's open files with a description
    /// (`p4 submit -d <desc>`). Errors (e.g. nothing open, needs resolve)
    /// surface through the returned `Result`.
    pub fn submit(&mut self, description: &str) -> Result<SubmitResult, Error> {
        self.submit_args(vec!["-d".to_string(), description.to_string()])
    }

    /// Submit a specific pending changelist (`p4 submit -c <n>`). The change's
    /// own description is used.
    pub fn submit_change(&mut self, change: u64) -> Result<SubmitResult, Error> {
        self.submit_args(vec!["-c".to_string(), change.to_string()])
    }

    fn submit_args(&mut self, args: Vec<String>) -> Result<SubmitResult, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "submit", args)?;

        // The final record carries submittedChange; per-file records carry a
        // depotFile. A single response contains both kinds.
        let mut change: Option<u64> = None;
        let mut files = Vec::new();
        for m in &records {
            if let Some(sc) = m.get("submittedChange") {
                change = sc.parse::<u64>().ok();
            }
            if m.contains_key("depotFile") {
                files.push(
                    SubmittedFile::deserialize(serde::de::value::MapDeserializer::new(
                        m.clone().into_iter(),
                    ))
                    .map_err(|e| Error::SerializationError(e, m.clone()))?,
                );
            }
        }

        let change = change.ok_or_else(|| {
            Error::SpecError("submit produced no submittedChange record".to_string())
        })?;
        Ok(SubmitResult { change, files })
    }
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

    // Captured verbatim from `p4 submit -d` (per-file record shape).
    #[test]
    fn submitted_file_from_record() {
        let sf: SubmittedFile =
            SubmittedFile::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
                record(&[
                    ("action", "add"),
                    ("depotFile", "//depot/hello.txt"),
                    ("rev", "1"),
                ])
                .into_iter(),
            ))
            .expect("deserialize submitted file");
        assert_eq!(sf.depot_file, "//depot/hello.txt");
        assert_eq!(sf.action, Some(OpenAction::Add));
        assert_eq!(sf.rev, Some(1));
    }
}
