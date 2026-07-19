//! Typed `p4 branches` (list) and `p4 branch` spec (`-o` read / `-i` write).

use crate::client::{Client, UserInterface};
use crate::commands::client::ViewMapping;
use crate::commands::helpers::{from_str_value, optional_string};
use crate::errors::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

impl Client {
    /// List the server's branch mappings (`p4 branches`), typed. A fresh server
    /// has none, so the list is empty until a branch spec is saved.
    pub fn branches(&mut self) -> Result<Vec<BranchSummary>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "branches", Vec::new())?;
        records
            .into_iter()
            .map(|m| {
                BranchSummary::deserialize(serde::de::value::MapDeserializer::new(
                    m.clone().into_iter(),
                ))
                .map_err(|e| Error::SerializationError(e, m))
            })
            .collect()
    }

    /// Read a branch spec (`p4 branch -o name`), typed. For a branch that
    /// doesn't exist yet the server returns a defaulted template -- the create
    /// flow is: read the template, modify it, save it.
    pub fn branch_spec(&mut self, name: &str) -> Result<BranchSpec, Error> {
        let mut ui = UserInterface::new();
        let mut records =
            self.run_records(&mut ui, "branch", vec!["-o".to_string(), name.to_string()])?;
        let record = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        BranchSpec::from_record(record)
    }

    /// Create or update a branch spec (`p4 branch -i`).
    pub fn save_branch_spec(&mut self, spec: &BranchSpec) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        ui.set_input(&spec.to_spec_text());
        self.run_records(&mut ui, "branch", vec!["-i".to_string()])?;
        Ok(())
    }
}

/// The `Options:` flag set of a branch spec. The only defined flag is
/// `locked`/`unlocked` (a locked branch can only be changed by its owner).
/// Tokens this version doesn't know are preserved verbatim so a
/// read-modify-write round trip doesn't drop them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchOptions {
    pub locked: bool,
    /// Unrecognized tokens, kept in order for round-tripping.
    pub unknown: Vec<String>,
}

impl FromStr for BranchOptions {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut o = BranchOptions::default();
        for token in s.split_whitespace() {
            match token {
                "locked" => o.locked = true,
                "unlocked" => o.locked = false,
                other => o.unknown.push(other.to_string()),
            }
        }
        Ok(o)
    }
}

impl Display for BranchOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(if self.locked { "locked" } else { "unlocked" })?;
        for u in &self.unknown {
            write!(f, " {u}")?;
        }
        Ok(())
    }
}

/// One branch from `p4 branches`, deserialized from the tagged record. In
/// tagged output `Update`/`Access` are epoch seconds (unlike the formatted
/// dates the spec form shows).
#[derive(Debug, Clone, Deserialize)]
pub struct BranchSummary {
    #[serde(rename = "branch")]
    pub branch: String,

    #[serde(rename = "Owner", default, deserialize_with = "optional_string")]
    pub owner: Option<String>,

    #[serde(rename = "Description", default)]
    pub description: String,

    #[serde(rename = "Options", default, deserialize_with = "optional_string")]
    pub options: Option<String>,

    /// Last spec update, seconds since the Unix epoch.
    #[serde(rename = "Update", deserialize_with = "from_str_value")]
    pub update: u64,

    /// Last access, seconds since the Unix epoch.
    #[serde(rename = "Access", deserialize_with = "from_str_value")]
    pub access: u64,
}

/// A branch spec, as read by `branch -o` / written by `branch -i`.
///
/// Server-managed fields (`Update`/`Access`) are kept as the raw formatted
/// strings the server reports and are never sent back. The view is two-sided
/// (source depot path -> target depot path).
#[derive(Debug, Clone, Deserialize)]
pub struct BranchSpec {
    #[serde(rename = "Branch")]
    pub branch: String,

    #[serde(rename = "Owner", default, deserialize_with = "optional_string")]
    pub owner: Option<String>,

    /// Free-form description; may span multiple lines (newline-separated).
    #[serde(rename = "Description", default)]
    pub description: String,

    #[serde(rename = "Options", deserialize_with = "from_str_value")]
    pub options: BranchOptions,

    /// Server-managed; never sent back on save.
    #[serde(rename = "Update", default, deserialize_with = "optional_string")]
    pub update: Option<String>,

    /// Server-managed; never sent back on save.
    #[serde(rename = "Access", default, deserialize_with = "optional_string")]
    pub access: Option<String>,

    /// The view, from the `View0..ViewN` record keys. Two-sided
    /// (source -> target depot paths).
    #[serde(skip)]
    pub view: Vec<ViewMapping>,
}

impl BranchSpec {
    /// Build a typed spec from a tagged `branch -o` record.
    pub fn from_record(mut record: HashMap<String, String>) -> Result<BranchSpec, Error> {
        // Spec-output bookkeeping, not fields.
        record.remove("specFormatted");
        record.remove("func");
        record.remove("specdef");

        let mut view = Vec::new();
        let mut i = 0;
        while let Some(line) = record.remove(&format!("View{i}")) {
            view.push(
                line.parse::<ViewMapping>()
                    .map_err(|e| Error::SpecError(format!("View{i}: {e}")))?,
            );
            i += 1;
        }

        let mut spec = BranchSpec::deserialize(serde::de::value::MapDeserializer::new(
            record.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, record))?;
        spec.view = view;
        Ok(spec)
    }

    /// Render the spec as the text form `branch -i` reads. Server-managed
    /// fields (Update/Access) are omitted.
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

        single(&mut out, "Branch", &self.branch);
        if let Some(owner) = &self.owner {
            single(&mut out, "Owner", owner);
        }
        if !self.description.is_empty() {
            multi(
                &mut out,
                "Description",
                &mut self.description.lines().map(str::to_string),
            );
        }
        single(&mut out, "Options", &self.options);
        multi(
            &mut out,
            "View",
            &mut self.view.iter().map(ToString::to_string),
        );

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_spec_record() -> HashMap<String, String> {
        // Verbatim shape from a real tagged `branch -o` (p4d 2025.2).
        [
            ("Branch", "main-to-rel"),
            ("Owner", "danrs"),
            ("Description", "Created by danrs.\n"),
            ("Options", "unlocked"),
            ("View0", "//depot/main/... //depot/rel/..."),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn branch_spec_from_captured_record() {
        let spec = BranchSpec::from_record(captured_spec_record()).expect("parse captured record");
        assert_eq!(spec.branch, "main-to-rel");
        assert_eq!(spec.owner.as_deref(), Some("danrs"));
        assert_eq!(spec.description, "Created by danrs.\n");
        assert!(!spec.options.locked);
        assert!(spec.options.unknown.is_empty());
        assert_eq!(
            spec.view,
            vec![ViewMapping::new("//depot/main/...", "//depot/rel/...")]
        );
        // Update/Access absent on an unsaved template.
        assert!(spec.update.is_none());
    }

    #[test]
    fn spec_text_round_trips_the_essentials() {
        let mut spec = BranchSpec::from_record(captured_spec_record()).unwrap();
        spec.description = "Line one.\nLine two.".to_string();
        spec.view.push(ViewMapping::new(
            "-//depot/main/secret/...",
            "//depot/rel/secret/...",
        ));

        let text = spec.to_spec_text();
        assert!(text.contains("Branch:\tmain-to-rel\n"));
        assert!(text.contains("Description:\n\tLine one.\n\tLine two.\n"));
        assert!(text.contains("Options:\tunlocked\n"));
        assert!(text.contains(
            "View:\n\t//depot/main/... //depot/rel/...\n\t-//depot/main/secret/... //depot/rel/secret/...\n"
        ));
        // Server-managed fields are never written.
        assert!(!text.contains("Update:"));
        assert!(!text.contains("Access:"));
    }

    #[test]
    fn branch_options_round_trip_preserves_unknown_tokens() {
        let o: BranchOptions = "locked somethingnew".parse().unwrap();
        assert!(o.locked);
        assert_eq!(o.unknown, vec!["somethingnew"]);
        assert_eq!(o.to_string(), "locked somethingnew");

        let o: BranchOptions = "unlocked".parse().unwrap();
        assert!(!o.locked);
        assert!(o.unknown.is_empty());
        assert_eq!(o.to_string(), "unlocked");
    }

    #[test]
    fn summary_deserializes_from_captured_record() {
        use serde::de::value::MapDeserializer;

        // Field shapes captured from a real `p4 branches` tagged record.
        let m: HashMap<String, String> = [
            ("branch", "main-to-rel"),
            ("Owner", "danrs"),
            ("Description", "Integration test branch.\n"),
            ("Options", "unlocked"),
            ("Update", "1784461296"),
            ("Access", "1784461296"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let s = BranchSummary::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize captured branches record");
        assert_eq!(s.branch, "main-to-rel");
        assert_eq!(s.owner.as_deref(), Some("danrs"));
        assert_eq!(s.options.as_deref(), Some("unlocked"));
        assert_eq!(s.update, 1_784_461_296);
        assert_eq!(s.access, 1_784_461_296);
    }
}
