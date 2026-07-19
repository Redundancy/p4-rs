//! Typed `p4 change` -- read (`-o`) and write (`-i`) changelist specs.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::optional_string;
use crate::errors::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Display;

impl Client {
    /// Read a changelist spec (`p4 change -o [change]`), typed.
    ///
    /// With `None` the server returns a new-change template (its `Change` field
    /// is the literal `"new"`); the create flow is read the template, set a
    /// description, save it. With `Some(number)` the existing pending or
    /// submitted change is read.
    ///
    /// `change -o` runs in workspace context, so the connection must have a
    /// client set (`Options::set_client`).
    pub fn change_spec(&mut self, change: Option<&str>) -> Result<ChangeSpec, Error> {
        let mut ui = UserInterface::new();
        let mut args = vec!["-o".to_string()];
        if let Some(change) = change {
            args.push(change.to_string());
        }
        let mut records = self.run_records(&mut ui, "change", args)?;
        let record = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        ChangeSpec::from_record(record)
    }

    /// Create or update a changelist spec (`p4 change -i`).
    ///
    /// For a new change, set `change` to `"new"` (the value the template
    /// carries); the server assigns and reports the real number. Server-managed
    /// fields (`Date`) are not sent.
    pub fn save_change_spec(&mut self, spec: &ChangeSpec) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        ui.set_input(&spec.to_spec_text());
        self.run_records(&mut ui, "change", vec!["-i".to_string()])?;
        Ok(())
    }
}

/// A changelist spec, as read by `change -o` / written by `change -i`.
///
/// The `Date` field is server-managed: it is kept as the raw formatted string
/// the server reports and is never sent back on save. `Change` is `"new"` for
/// an unsaved template and the changelist number otherwise; it is sent as-is so
/// the server either assigns a number or updates the existing change.
#[derive(Debug, Clone, Deserialize)]
pub struct ChangeSpec {
    /// `"new"` for a new-change template, otherwise the changelist number.
    #[serde(rename = "Change")]
    pub change: String,

    /// Server-managed formatted date; never sent back on save.
    #[serde(rename = "Date", default, deserialize_with = "optional_string")]
    pub date: Option<String>,

    #[serde(rename = "Client")]
    pub client: String,

    #[serde(rename = "User")]
    pub user: String,

    /// `new` / `pending` / `submitted`. Present in normal output; optional so an
    /// unusual record doesn't fail the whole parse.
    #[serde(rename = "Status", default, deserialize_with = "optional_string")]
    pub status: Option<String>,

    /// `public` or `restricted`; only present on servers/changes that set it.
    #[serde(rename = "Type", default, deserialize_with = "optional_string")]
    pub change_type: Option<String>,

    /// Free-form description; may span multiple lines (newline-separated).
    #[serde(rename = "Description", default)]
    pub description: String,

    /// Jobs attached to the change, from the `Jobs0..JobsN` record keys.
    #[serde(skip)]
    pub jobs: Vec<String>,

    /// Files in the change, from the `Files0..FilesN` record keys.
    #[serde(skip)]
    pub files: Vec<String>,
}

/// Read the `Prefix0`, `Prefix1`, ... indexed keys out of `record` (removing
/// them) and return their values in order.
fn take_indexed(record: &mut HashMap<String, String>, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(v) = record.remove(&format!("{prefix}{i}")) {
        out.push(v);
        i += 1;
    }
    out
}

impl ChangeSpec {
    /// Build a typed spec from a tagged `change -o` record.
    pub fn from_record(mut record: HashMap<String, String>) -> Result<ChangeSpec, Error> {
        // Spec-output bookkeeping, not fields.
        record.remove("specFormatted");
        record.remove("func");
        record.remove("specdef");

        let jobs = take_indexed(&mut record, "Jobs");
        let files = take_indexed(&mut record, "Files");

        let mut spec = ChangeSpec::deserialize(serde::de::value::MapDeserializer::new(
            record.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, record))?;
        spec.jobs = jobs;
        spec.files = files;
        Ok(spec)
    }

    /// Render the spec as the text form `change -i` reads. The server-managed
    /// `Date` field is omitted.
    pub fn to_spec_text(&self) -> String {
        let mut out = String::new();

        let single = |out: &mut String, name: &str, value: &dyn Display| {
            out.push_str(&format!("{name}:\t{value}\n\n"));
        };
        let multi = |out: &mut String, name: &str, lines: &mut dyn Iterator<Item = String>| {
            out.push_str(name);
            out.push_str(":\n");
            for line in lines {
                out.push('\t');
                out.push_str(&line);
                out.push('\n');
            }
            out.push('\n');
        };

        single(&mut out, "Change", &self.change);
        single(&mut out, "Client", &self.client);
        single(&mut out, "User", &self.user);
        if let Some(status) = &self.status {
            single(&mut out, "Status", status);
        }
        if let Some(change_type) = &self.change_type {
            single(&mut out, "Type", change_type);
        }
        if !self.description.is_empty() {
            multi(
                &mut out,
                "Description",
                &mut self.description.lines().map(str::to_string),
            );
        }
        if !self.jobs.is_empty() {
            multi(&mut out, "Jobs", &mut self.jobs.iter().cloned());
        }
        if !self.files.is_empty() {
            multi(&mut out, "Files", &mut self.files.iter().cloned());
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim shape of a real tagged `change -o` new-change template
    /// (p4d 2022.2): Capitalized keys, `Change` = "new", and -- notably -- no
    /// `Date`/`Type` on an unsaved template.
    fn captured_new_template() -> HashMap<String, String> {
        [
            ("Change", "new"),
            ("Client", "chspec-ws"),
            ("User", "danrs"),
            ("Status", "new"),
            ("Description", "<enter description here>\n"),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// An existing pending change (`change -o <n>`). The scalar keys are
    /// verbatim from a real capture (Capitalized, with `Date`/`Type` that the
    /// new template lacks); `Jobs0`/`Files0..` are the indexed keys the server
    /// adds once jobs/files are attached, exercising the indexed-array path.
    fn captured_existing_with_job_and_file() -> HashMap<String, String> {
        [
            ("Change", "42"),
            ("Date", "2026/07/19 08:46:59"),
            ("Client", "chspec-ws"),
            ("User", "danrs"),
            ("Status", "pending"),
            ("Type", "restricted"),
            ("Description", "Line one.\nLine two.\n"),
            ("Jobs0", "job000123"),
            ("Files0", "//depot/main/a.txt"),
            ("Files1", "//depot/main/b.txt"),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn change_spec_from_new_template() {
        let spec = ChangeSpec::from_record(captured_new_template()).expect("parse template");
        assert_eq!(spec.change, "new");
        assert_eq!(spec.client, "chspec-ws");
        assert_eq!(spec.user, "danrs");
        assert_eq!(spec.status.as_deref(), Some("new"));
        // An unsaved template carries no Date/Type.
        assert!(spec.date.is_none());
        assert!(spec.change_type.is_none());
        assert!(spec.jobs.is_empty());
        assert!(spec.files.is_empty());
    }

    #[test]
    fn change_spec_from_existing_parses_indexed_jobs_and_files() {
        let spec =
            ChangeSpec::from_record(captured_existing_with_job_and_file()).expect("parse existing");
        assert_eq!(spec.change, "42");
        assert_eq!(spec.status.as_deref(), Some("pending"));
        assert_eq!(spec.change_type.as_deref(), Some("restricted"));
        assert_eq!(spec.description, "Line one.\nLine two.\n");
        assert_eq!(spec.jobs, vec!["job000123"]);
        assert_eq!(spec.files, vec!["//depot/main/a.txt", "//depot/main/b.txt"]);
    }

    #[test]
    fn to_spec_text_omits_date_and_renders_sections() {
        let mut spec =
            ChangeSpec::from_record(captured_existing_with_job_and_file()).expect("parse existing");
        spec.description = "New description.".to_string();
        let text = spec.to_spec_text();

        assert!(text.contains("Change:\t42\n"));
        assert!(text.contains("Client:\tchspec-ws\n"));
        assert!(text.contains("User:\tdanrs\n"));
        assert!(text.contains("Status:\tpending\n"));
        assert!(text.contains("Type:\trestricted\n"));
        assert!(text.contains("Description:\n\tNew description.\n"));
        assert!(text.contains("Jobs:\n\tjob000123\n"));
        assert!(text.contains("Files:\n\t//depot/main/a.txt\n\t//depot/main/b.txt\n"));
        // Server-managed Date is never written.
        assert!(!text.contains("Date:"));
    }

    #[test]
    fn to_spec_text_new_template_has_change_new() {
        let spec = ChangeSpec::from_record(captured_new_template()).unwrap();
        let text = spec.to_spec_text();
        assert!(text.starts_with("Change:\tnew\n"));
        assert!(!text.contains("Date:"));
        // No jobs/files sections on an empty template.
        assert!(!text.contains("Jobs:"));
        assert!(!text.contains("Files:"));
    }
}
