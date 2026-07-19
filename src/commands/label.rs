//! Typed wrappers for Perforce labels: the `labels` list and the `label`
//! spec (`label -o` / `label -i`).
//!
//! Two distinct shapes, matching the two commands:
//! * [`LabelSummary`] -- one row of `p4 labels`. `Update`/`Access` arrive as
//!   epoch seconds (u64) and `label` is lowercase in this record.
//! * [`LabelSpec`] -- the editable form from `p4 label -o`. `Update`/`Access`
//!   are server-managed *formatted* strings (never sent back), and the view is
//!   an indexed `View0..N` block of **one-sided** depot paths (a label view is
//!   a single path per line, unlike a client view's two-sided mapping).

use crate::client::UserInterface;
use crate::errors::Error;
use serde::de::value::MapDeserializer;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

/// Deserialize a field that is optional-by-absence: a present key is a plain
/// string (wrapped in `Some`); an absent key falls back to `#[serde(default)]`
/// = `None`. MapDeserializer's string values don't support serde's native
/// `Option` handling, so the explicit adapter is required (see `commands::info`).
fn optional_string<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(d).map(Some)
}

/// Deserialize epoch seconds delivered as a decimal string into `u64`.
/// MapDeserializer hands integers over as strings, so `u64`'s native path
/// ("invalid type: string, expected u64") does not apply.
fn epoch_secs<'de, D>(d: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    s.trim()
        .parse::<u64>()
        .map_err(|e| serde::de::Error::custom(format!("invalid epoch seconds {s:?}: {e}")))
}

/// The `Options:` field of a label, as a flag struct.
///
/// A label's options string is a set of space-separated tokens; the two the
/// server defines are `locked`/`unlocked` and `autoreload`/`noautoreload`.
/// Any token we do not recognize is preserved verbatim (and in order) so a
/// round-trip through [`Display`] never silently drops a future flag -- the
/// same forgiving policy used for client options.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LabelOptions {
    /// `locked` prevents the label from being modified once tagged.
    pub locked: bool,
    /// `autoreload` stores the label in the unload depot.
    pub autoreload: bool,
    /// Tokens not recognized as one of the known flags, kept in encounter order.
    unknown: Vec<String>,
}

impl FromStr for LabelOptions {
    // Parsing never fails: unknown tokens are preserved rather than rejected.
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut opts = LabelOptions::default();
        for token in s.split_whitespace() {
            match token {
                "locked" => opts.locked = true,
                "unlocked" => opts.locked = false,
                "autoreload" => opts.autoreload = true,
                "noautoreload" => opts.autoreload = false,
                other => opts.unknown.push(other.to_string()),
            }
        }
        Ok(opts)
    }
}

impl Display for LabelOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            if self.locked { "locked" } else { "unlocked" },
            if self.autoreload {
                "autoreload"
            } else {
                "noautoreload"
            },
        )?;
        for token in &self.unknown {
            write!(f, " {token}")?;
        }
        Ok(())
    }
}

impl Serialize for LabelOptions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LabelOptions {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        // FromStr is Infallible.
        Ok(LabelOptions::from_str(&s).unwrap())
    }
}

/// One row of `p4 labels`.
///
/// Note the lowercase `label` key (the server's own naming in this record) and
/// the epoch-seconds timestamps -- both differ from the [`LabelSpec`] form.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LabelSummary {
    /// The label's name (`label` -- lowercase -- in this record).
    #[serde(rename = "label")]
    pub name: String,

    /// Last-modified time, epoch seconds.
    #[serde(rename = "Update", deserialize_with = "epoch_secs")]
    pub update: u64,

    /// Last-access time, epoch seconds.
    #[serde(rename = "Access", deserialize_with = "epoch_secs")]
    pub access: u64,

    #[serde(rename = "Owner")]
    pub owner: String,

    #[serde(rename = "Options", default)]
    pub options: LabelOptions,

    #[serde(rename = "Description", default, deserialize_with = "optional_string")]
    pub description: Option<String>,

    /// Present only when the label pins a changelist/revision.
    #[serde(rename = "Revision", default, deserialize_with = "optional_string")]
    pub revision: Option<String>,
}

/// The editable label form from `p4 label -o` (and the input to `label -i`).
///
/// `update`/`access` are server-managed formatted timestamps and are `None` on
/// an unsaved template; [`to_spec_text`](LabelSpec::to_spec_text) never emits
/// them. `view` holds one-sided depot paths.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct LabelSpec {
    pub label: String,
    pub owner: Option<String>,
    pub description: Option<String>,
    pub options: LabelOptions,
    /// A pinned revision/changelist specifier (e.g. `@1234`), if set.
    pub revision: Option<String>,
    /// The label's home server in a multi-server setup, if set.
    pub server_id: Option<String>,
    /// Server-managed; `None` on an unsaved template. Never sent back.
    pub update: Option<String>,
    /// Server-managed; `None` on an unsaved template. Never sent back.
    pub access: Option<String>,
    /// One-sided depot paths (`View0..N`), one path per line.
    pub view: Vec<String>,
}

/// The scalar (non-`View`) fields of a [`LabelSpec`], deserialized via serde
/// once the indexed `View` lines have been split off by hand.
#[derive(Deserialize)]
struct LabelSpecFields {
    #[serde(rename = "Label")]
    label: String,
    #[serde(rename = "Owner", default, deserialize_with = "optional_string")]
    owner: Option<String>,
    #[serde(rename = "Description", default, deserialize_with = "optional_string")]
    description: Option<String>,
    #[serde(rename = "Options", default)]
    options: LabelOptions,
    #[serde(rename = "Revision", default, deserialize_with = "optional_string")]
    revision: Option<String>,
    #[serde(rename = "ServerID", default, deserialize_with = "optional_string")]
    server_id: Option<String>,
    #[serde(rename = "Update", default, deserialize_with = "optional_string")]
    update: Option<String>,
    #[serde(rename = "Access", default, deserialize_with = "optional_string")]
    access: Option<String>,
}

/// Append a single-line spec field: `Name:\tvalue\n\n`.
fn push_field(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push_str(":\t");
    out.push_str(value);
    out.push_str("\n\n");
}

impl LabelSpec {
    /// Build a [`LabelSpec`] from a single tagged `label -o` record.
    ///
    /// The indexed `View0..N` keys are pulled out and ordered manually before
    /// serde touches the map (serde has no notion of the numbered-key idiom),
    /// and protocol-only keys (`specFormatted`, `specdef`, `func`) are dropped.
    pub fn from_record(mut m: HashMap<String, String>) -> Result<LabelSpec, Error> {
        // Collect the one-sided view lines, ordered by their numeric suffix.
        let mut views: Vec<(usize, String)> = Vec::new();
        for (k, v) in m.iter() {
            if let Some(idx) = k.strip_prefix("View")
                && let Ok(n) = idx.parse::<usize>()
            {
                views.push((n, v.clone()));
            }
        }
        views.sort_by_key(|(n, _)| *n);
        let view: Vec<String> = views.into_iter().map(|(_, v)| v).collect();

        // Everything serde should not see: the numbered view keys plus the
        // form-protocol bookkeeping keys.
        m.retain(|k, _| {
            !(k.starts_with("View") || k == "specFormatted" || k == "specdef" || k == "func")
        });

        let fields = LabelSpecFields::deserialize(MapDeserializer::new(m.clone().into_iter()))
            .map_err(|e| Error::SerializationError(e, m))?;

        Ok(LabelSpec {
            label: fields.label,
            owner: fields.owner,
            description: fields.description,
            options: fields.options,
            revision: fields.revision,
            server_id: fields.server_id,
            update: fields.update,
            access: fields.access,
            view,
        })
    }

    /// Render the spec as the form text `label -i` expects on stdin.
    ///
    /// Scalar fields are single lines (`Name:\tvalue`); the view is a
    /// tab-indented multi-line block. Server-managed `Update`/`Access` are
    /// deliberately omitted -- the server sets them and rejects client values.
    pub fn to_spec_text(&self) -> String {
        let mut out = String::new();
        push_field(&mut out, "Label", &self.label);
        if let Some(owner) = &self.owner {
            push_field(&mut out, "Owner", owner);
        }
        if let Some(desc) = &self.description {
            let desc = desc.trim_end_matches('\n');
            if !desc.is_empty() {
                push_field(&mut out, "Description", desc);
            }
        }
        push_field(&mut out, "Options", &self.options.to_string());
        if let Some(revision) = &self.revision {
            push_field(&mut out, "Revision", revision);
        }
        if let Some(server_id) = &self.server_id {
            push_field(&mut out, "ServerID", server_id);
        }
        if !self.view.is_empty() {
            out.push_str("View:\n");
            for path in &self.view {
                out.push('\t');
                out.push_str(path);
                out.push('\n');
            }
            out.push('\n');
        }
        out
    }
}

impl crate::client::Client {
    /// List all labels (`p4 labels`). A fresh server has none, so this can
    /// legitimately return an empty vector.
    pub fn labels(&mut self) -> Result<Vec<LabelSummary>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "labels", Vec::new())?;
        records
            .into_iter()
            .map(|m| {
                LabelSummary::deserialize(MapDeserializer::new(m.clone().into_iter()))
                    .map_err(|e| Error::SerializationError(e, m))
            })
            .collect()
    }

    /// Fetch a label's editable form (`p4 label -o <name>`). For a name that
    /// does not exist yet the server returns a default template (with `update`
    /// = `None`), matching `p4`'s own behavior.
    pub fn label_spec(&mut self, name: &str) -> Result<LabelSpec, Error> {
        let mut ui = UserInterface::new();
        let mut records =
            self.run_records(&mut ui, "label", vec!["-o".to_string(), name.to_string()])?;
        let m = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        LabelSpec::from_record(m)
    }

    /// Create or update a label spec (`p4 label -i`), feeding the form to the
    /// server through the bridge's input channel.
    pub fn save_label_spec(&mut self, spec: &LabelSpec) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        ui.set_input(&spec.to_spec_text());
        self.run_records(&mut ui, "label", vec!["-i".to_string()])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn options_parse_and_render_known_flags() {
        let o: LabelOptions = "locked noautoreload".parse().unwrap();
        assert!(o.locked);
        assert!(!o.autoreload);
        assert_eq!(o.to_string(), "locked noautoreload");

        let d: LabelOptions = "unlocked noautoreload".parse().unwrap();
        assert_eq!(d, LabelOptions::default());
        assert_eq!(d.to_string(), "unlocked noautoreload");

        let a: LabelOptions = "unlocked autoreload".parse().unwrap();
        assert!(!a.locked);
        assert!(a.autoreload);
        assert_eq!(a.to_string(), "unlocked autoreload");
    }

    #[test]
    fn options_preserve_unknown_tokens_in_order() {
        let o: LabelOptions = "locked mystery autoreload extra".parse().unwrap();
        assert!(o.locked);
        assert!(o.autoreload);
        // Unknown tokens survive a round-trip, emitted after the known flags.
        assert_eq!(o.to_string(), "locked autoreload mystery extra");
    }

    // Captured verbatim from `labels` against a live 2025.2 p4d.
    #[test]
    fn summary_deserializes_from_list_record() {
        let m = map(&[
            ("label", "rel-1.0"),
            ("Update", "1784465382"),
            ("Access", "1784465382"),
            ("Owner", "probeuser"),
            ("Options", "locked noautoreload"),
            ("Description", "Release 1.0 label\n"),
        ]);
        let s = LabelSummary::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize labels record");

        assert_eq!(s.name, "rel-1.0");
        assert_eq!(s.update, 1784465382);
        assert_eq!(s.access, 1784465382);
        assert_eq!(s.owner, "probeuser");
        assert!(s.options.locked);
        assert!(!s.options.autoreload);
        assert_eq!(s.description.as_deref(), Some("Release 1.0 label\n"));
        assert!(s.revision.is_none());
    }

    // Captured verbatim from `label -o rel-1.0` after a save (note the
    // formatted, server-managed Update/Access and the empty specFormatted key
    // that must be stripped before serde).
    #[test]
    fn spec_from_saved_record() {
        let m = map(&[
            ("Label", "rel-1.0"),
            ("Update", "2026/07/19 08:49:42"),
            ("Access", "2026/07/19 08:49:42"),
            ("Owner", "probeuser"),
            ("Description", "Release 1.0 label\n"),
            ("Options", "locked noautoreload"),
            ("View0", "//depot/rel/..."),
            ("specFormatted", ""),
        ]);
        let spec = LabelSpec::from_record(m).expect("from_record saved");

        assert_eq!(spec.label, "rel-1.0");
        assert_eq!(spec.owner.as_deref(), Some("probeuser"));
        assert_eq!(spec.update.as_deref(), Some("2026/07/19 08:49:42"));
        assert_eq!(spec.access.as_deref(), Some("2026/07/19 08:49:42"));
        assert!(spec.options.locked);
        assert_eq!(spec.view, vec!["//depot/rel/...".to_string()]);
        assert!(spec.revision.is_none());
        assert!(spec.server_id.is_none());
    }

    // Captured verbatim from `label -o brandnew` on a fresh server: an unsaved
    // template has no Update/Access and default (unlocked) options.
    #[test]
    fn spec_from_template_record() {
        let m = map(&[
            ("Label", "brandnew"),
            ("Owner", "probeuser"),
            ("Description", "Created by probeuser.\n"),
            ("Options", "unlocked noautoreload"),
            ("View0", "//depot/..."),
            ("specFormatted", ""),
        ]);
        let spec = LabelSpec::from_record(m).expect("from_record template");

        assert_eq!(spec.label, "brandnew");
        assert!(spec.update.is_none());
        assert!(spec.access.is_none());
        assert_eq!(spec.options, LabelOptions::default());
        assert_eq!(spec.view, vec!["//depot/...".to_string()]);
    }

    #[test]
    fn view_lines_ordered_by_numeric_suffix() {
        // Deliberately out of order in the map; the parse must sort View0..N
        // numerically (not lexically -- View10 must follow View9).
        let m = map(&[
            ("Label", "multi"),
            ("Options", "unlocked noautoreload"),
            ("View10", "//depot/j/..."),
            ("View2", "//depot/c/..."),
            ("View0", "//depot/a/..."),
            ("View1", "//depot/b/..."),
            ("View9", "//depot/i/..."),
        ]);
        let spec = LabelSpec::from_record(m).expect("from_record multi-view");
        assert_eq!(
            spec.view,
            vec![
                "//depot/a/...".to_string(),
                "//depot/b/...".to_string(),
                "//depot/c/...".to_string(),
                "//depot/i/...".to_string(),
                "//depot/j/...".to_string(),
            ]
        );
    }

    #[test]
    fn to_spec_text_round_trips_fields_and_omits_server_managed() {
        let spec = LabelSpec {
            label: "rel-1.0".to_string(),
            owner: Some("probeuser".to_string()),
            description: Some("Release 1.0 label\n".to_string()),
            options: "locked noautoreload".parse().unwrap(),
            revision: None,
            server_id: None,
            update: Some("2026/07/19 08:49:42".to_string()),
            access: Some("2026/07/19 08:49:42".to_string()),
            view: vec!["//depot/rel/...".to_string()],
        };
        let text = spec.to_spec_text();

        assert!(text.contains("Label:\trel-1.0\n"));
        assert!(text.contains("Owner:\tprobeuser\n"));
        // Trailing newline in the captured description is trimmed to a clean line.
        assert!(text.contains("Description:\tRelease 1.0 label\n"));
        assert!(text.contains("Options:\tlocked noautoreload\n"));
        assert!(text.contains("View:\n\t//depot/rel/...\n"));
        // Server-managed timestamps must never be sent back.
        assert!(!text.contains("Update"));
        assert!(!text.contains("Access"));
    }
}
