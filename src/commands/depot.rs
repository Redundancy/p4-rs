//! Typed `p4 depots` (list) and `p4 depot -o`/`-i` (spec) support.
//!
//! Two distinct record shapes back the same object:
//!
//! * the `depots` listing uses lowercase tagged keys (`name`, `time`, `type`,
//!   `map`, `desc`, ...), with `time` an epoch-seconds string; and
//! * the `depot -o` spec form uses capitalized field keys (`Depot`, `Owner`,
//!   `Date`, `Type`, `Map`, ...) plus indexed `SpecMap0..N` lines.
//!
//! They are therefore modelled as separate structs -- [`DepotSummary`] and
//! [`DepotSpec`] -- rather than one type with a union of keys.

use crate::client::{Client, UserInterface};
use crate::errors::Error;
use serde::de::value::MapDeserializer;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{self, Display};
use std::str::FromStr;

/// Deserialize a field whose tagged value is always a plain string but which we
/// want parsed into a richer type via its [`FromStr`] (e.g. epoch seconds into
/// `u64`, or a depot type keyword into [`DepotType`]). `MapDeserializer` only
/// ever offers string values, so serde's native integer/enum handling can't be
/// used directly.
fn from_str_value<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    <T as FromStr>::Err: Display,
{
    let s = String::deserialize(d)?;
    T::from_str(&s).map_err(serde::de::Error::custom)
}

/// Deserialize an optional-by-absence string field: a present key wraps its
/// plain-string value in `Some`; an absent key is supplied as `None` by
/// `#[serde(default)]`. `MapDeserializer`'s string values don't support serde's
/// native `Option` handling (a present value would fail with "invalid type:
/// string, expected option"), hence this shim.
fn optional_string<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(d).map(Some)
}

/// Depot type. This is an open enum: the set of built-in types has grown across
/// server releases, so an unrecognized keyword is preserved verbatim as
/// [`DepotType::Other`] rather than failing a listing or spec read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DepotType {
    #[default]
    Local,
    Stream,
    Remote,
    Spec,
    Archive,
    Unload,
    Tangent,
    Graph,
    Other(String),
}

impl FromStr for DepotType {
    // Every input maps to a variant (unknown -> Other), so parsing never fails.
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "local" => DepotType::Local,
            "stream" => DepotType::Stream,
            "remote" => DepotType::Remote,
            "spec" => DepotType::Spec,
            "archive" => DepotType::Archive,
            "unload" => DepotType::Unload,
            "tangent" => DepotType::Tangent,
            "graph" => DepotType::Graph,
            other => DepotType::Other(other.to_string()),
        })
    }
}

impl Display for DepotType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Round-trips with FromStr: Other holds the original keyword.
        let s = match self {
            DepotType::Local => "local",
            DepotType::Stream => "stream",
            DepotType::Remote => "remote",
            DepotType::Spec => "spec",
            DepotType::Archive => "archive",
            DepotType::Unload => "unload",
            DepotType::Tangent => "tangent",
            DepotType::Graph => "graph",
            DepotType::Other(s) => s.as_str(),
        };
        f.write_str(s)
    }
}

impl Serialize for DepotType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// One entry from a `p4 depots` listing (tagged record).
///
/// Keys are the server's lowercase tagged names -- note these differ from the
/// capitalized field names of the `depot -o` spec form (see [`DepotSpec`]).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DepotSummary {
    /// Depot name (the `depot -o` spec form calls this `Depot`).
    pub name: String,

    /// Creation time, epoch seconds.
    #[serde(deserialize_with = "from_str_value")]
    pub time: u64,

    #[serde(rename = "type", deserialize_with = "from_str_value")]
    pub depot_type: DepotType,

    /// Depot map, e.g. `"depotname/..."`.
    pub map: String,

    /// Description, kept verbatim (a live 2025.2 server sends it without a
    /// trailing newline in listings, unlike the spec form's `Description`).
    /// Defaults to empty: the tagged protocol omits empty fields, and one
    /// description-less depot must not fail the whole listing.
    #[serde(default)]
    pub desc: String,

    /// Present only for stream depots.
    #[serde(rename = "streamDepth", default, deserialize_with = "optional_string")]
    pub stream_depth: Option<String>,

    /// Present only for depots that carry an `extra` attribute.
    #[serde(default, deserialize_with = "optional_string")]
    pub extra: Option<String>,
}

impl DepotSummary {
    /// Build from one tagged listing record, carrying the raw map into the
    /// error on failure so a malformed record can be diagnosed.
    pub(crate) fn from_record(m: HashMap<String, String>) -> Result<Self, Error> {
        DepotSummary::deserialize(MapDeserializer::new(m.clone().into_iter()))
            .map_err(|e| Error::SerializationError(e, m))
    }
}

/// A depot spec, as read by `p4 depot -o` and written by `p4 depot -i`.
///
/// Field keys are the capitalized spec-form names. `Date` is server-managed and
/// is never sent back on a save. Conditional fields (`Address` for remote,
/// `Suffix` for spec, `StreamDepth` for stream depots) are `None` when absent.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DepotSpec {
    #[serde(rename = "Depot")]
    pub depot: String,

    #[serde(rename = "Owner", default, deserialize_with = "optional_string")]
    pub owner: Option<String>,

    /// Server-formatted creation date. Read-only: informational on read and
    /// omitted from [`DepotSpec::to_spec_text`] so a save never tries to set it.
    #[serde(rename = "Date", default, deserialize_with = "optional_string")]
    pub date: Option<String>,

    #[serde(rename = "Description", default, deserialize_with = "optional_string")]
    pub description: Option<String>,

    #[serde(rename = "Type", deserialize_with = "from_str_value")]
    pub depot_type: DepotType,

    #[serde(rename = "Map", default, deserialize_with = "optional_string")]
    pub map: Option<String>,

    /// Remote depots only.
    #[serde(rename = "Address", default, deserialize_with = "optional_string")]
    pub address: Option<String>,

    /// Spec depots only.
    #[serde(rename = "Suffix", default, deserialize_with = "optional_string")]
    pub suffix: Option<String>,

    /// Stream depots only.
    #[serde(rename = "StreamDepth", default, deserialize_with = "optional_string")]
    pub stream_depth: Option<String>,

    /// Spec-depot map lines, arriving as indexed `SpecMap0..N` keys and parsed
    /// manually in [`DepotSpec::from_record`]. Empty for non-spec depots.
    #[serde(skip)]
    pub spec_map: Vec<String>,
}

impl DepotSpec {
    /// Build from a `depot -o` record. Bookkeeping keys (`specFormatted`,
    /// `func`, `specdef`) are dropped, the indexed `SpecMap0..N` lines are
    /// collected in order, and the remaining flat fields go through serde.
    pub(crate) fn from_record(mut m: HashMap<String, String>) -> Result<Self, Error> {
        // Not spec fields: protocol/bookkeeping the server tacks on.
        m.remove("specFormatted");
        m.remove("func");
        m.remove("specdef");

        // SpecMap0, SpecMap1, ... are contiguous from zero; stop at the first gap.
        let mut spec_map = Vec::new();
        while let Some(v) = m.remove(&format!("SpecMap{}", spec_map.len())) {
            spec_map.push(v);
        }

        let mut spec = DepotSpec::deserialize(MapDeserializer::new(m.clone().into_iter()))
            .map_err(|e| Error::SerializationError(e, m))?;
        spec.spec_map = spec_map;
        Ok(spec)
    }

    /// Render the spec as `depot` form text for `p4 depot -i`.
    ///
    /// Single-line fields are `Name:\tvalue`, multi-line fields (Description,
    /// SpecMap) put each line on its own tab-indented row under a bare `Name:`,
    /// and every field block is followed by a blank separator line. `Date` is
    /// deliberately omitted (server-managed).
    pub fn to_spec_text(&self) -> String {
        let mut out = String::new();
        push_single(&mut out, "Depot", &self.depot);
        if let Some(owner) = &self.owner {
            push_single(&mut out, "Owner", owner);
        }
        if let Some(description) = &self.description {
            push_multi(&mut out, "Description", description.lines());
        }
        push_single(&mut out, "Type", &self.depot_type.to_string());
        if let Some(address) = &self.address {
            push_single(&mut out, "Address", address);
        }
        if let Some(suffix) = &self.suffix {
            push_single(&mut out, "Suffix", suffix);
        }
        if let Some(map) = &self.map {
            push_single(&mut out, "Map", map);
        }
        if let Some(stream_depth) = &self.stream_depth {
            push_single(&mut out, "StreamDepth", stream_depth);
        }
        if !self.spec_map.is_empty() {
            push_multi(
                &mut out,
                "SpecMap",
                self.spec_map.iter().map(String::as_str),
            );
        }
        out
    }
}

/// Append a single-line `Name:\tvalue` field followed by a blank separator line.
fn push_single(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push_str(":\t");
    out.push_str(value);
    out.push_str("\n\n");
}

/// Append a multi-line field: a bare `Name:` header, each value line on its own
/// tab-indented row, then a blank separator line.
fn push_multi<'a>(out: &mut String, name: &str, lines: impl Iterator<Item = &'a str>) {
    out.push_str(name);
    out.push_str(":\n");
    for line in lines {
        out.push('\t');
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
}

impl Client {
    /// List depots (`p4 depots`), typed. A fresh server auto-provisions a
    /// default depot named `depot`, so this is non-empty from day one.
    pub fn depots(&mut self) -> Result<Vec<DepotSummary>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "depots", Vec::new())?;
        records.into_iter().map(DepotSummary::from_record).collect()
    }

    /// Read a depot spec (`p4 depot -o <name>`), typed. For a name that does not
    /// yet exist the server returns a template spec, which is the basis for
    /// creating a new depot.
    pub fn depot_spec(&mut self, name: &str) -> Result<DepotSpec, Error> {
        let mut ui = UserInterface::new();
        let records =
            self.run_records(&mut ui, "depot", vec!["-o".to_string(), name.to_string()])?;
        // depot -o yields exactly one record; an empty result deserializes an
        // empty map, surfacing the missing required fields via SerializationError.
        DepotSpec::from_record(records.into_iter().next().unwrap_or_default())
    }

    /// Save a depot spec (`p4 depot -i`), creating or updating it.
    ///
    /// The form text is produced by [`DepotSpec::to_spec_text`] and must reach
    /// the server as the command's input. Delivering it requires the C++ bridge
    /// to override `ClientUser::InputData`; the current bridge does not, and
    /// the SDK's default `InputData` blocks reading the process's stdin --
    /// verified against a live 2025.2 p4d, where `depot -i` hangs indefinitely.
    /// Rather than hang (or race stdin), this returns an error immediately
    /// without contacting the server. The error carries the rendered form under
    /// the `spec` key so a caller can still submit it out-of-band (e.g. piping
    /// to `p4 depot -i`). Once the bridge grows form-input support, replace the
    /// error below with the run call.
    pub fn save_depot_spec(&mut self, spec: &DepotSpec) -> Result<(), Error> {
        let text = spec.to_spec_text();
        Err(Error::SerializationError(
            serde::de::Error::custom(
                "save_depot_spec: the bridge has no ClientUser::InputData override, so \
                 `depot -i` cannot be fed the spec form (the SDK default would block on stdin)",
            ),
            [("spec".to_string(), text)].into_iter().collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures below are captured verbatim from a live 2025.2 p4d via the
    // P4RS_CAPTURE dump test in tests/depot.rs.

    fn record(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn depot_type_round_trips_known_and_unknown() {
        for kw in [
            "local", "stream", "remote", "spec", "archive", "unload", "tangent", "graph",
        ] {
            let t: DepotType = kw.parse().unwrap();
            assert_eq!(t.to_string(), kw, "known type should round-trip");
        }
        // Unknown keyword is preserved and round-trips.
        let t: DepotType = "quantum".parse().unwrap();
        assert_eq!(t, DepotType::Other("quantum".to_string()));
        assert_eq!(t.to_string(), "quantum");
    }

    #[test]
    fn summary_deserializes_default_depot_record() {
        // Captured verbatim from `depots` on a fresh 2025.2 p4d.
        let m = record(&[
            ("name", "depot"),
            ("time", "1784465784"),
            ("type", "local"),
            ("map", "depot/..."),
            ("desc", "Default depot"),
        ]);
        let s = DepotSummary::from_record(m).expect("deserialize depots record");
        assert_eq!(s.name, "depot");
        assert_eq!(s.time, 1784465784);
        assert_eq!(s.depot_type, DepotType::Local);
        assert_eq!(s.map, "depot/...");
        assert_eq!(s.desc, "Default depot");
        assert!(s.stream_depth.is_none());
        assert!(s.extra.is_none());
    }

    #[test]
    fn summary_carries_conditional_stream_depth() {
        let m = record(&[
            ("name", "streams"),
            ("time", "1721300500"),
            ("type", "stream"),
            ("map", "streams/..."),
            ("desc", "Stream depot\n"),
            ("streamDepth", "//streams/1"),
        ]);
        let s = DepotSummary::from_record(m).expect("deserialize stream depot record");
        assert_eq!(s.depot_type, DepotType::Stream);
        assert_eq!(s.stream_depth.as_deref(), Some("//streams/1"));
    }

    #[test]
    fn summary_tolerates_absent_desc() {
        // The tagged protocol omits empty fields: a depot with an empty
        // description sends no `desc` key, which must not fail the listing.
        let m = record(&[
            ("name", "nodesc"),
            ("time", "1784465784"),
            ("type", "local"),
            ("map", "nodesc/..."),
        ]);
        let s = DepotSummary::from_record(m).expect("record without desc");
        assert_eq!(s.desc, "");
    }

    #[test]
    fn summary_missing_required_field_is_serialization_error() {
        // No `time`: must surface as SerializationError carrying the raw map,
        // not panic.
        let m = record(&[("name", "depot"), ("type", "local"), ("map", "depot/...")]);
        let err = DepotSummary::from_record(m).unwrap_err();
        assert!(matches!(err, Error::SerializationError(_, _)));
    }

    #[test]
    fn spec_deserializes_default_depot_form() {
        // Captured verbatim from `depot -o depot` on a fresh 2025.2 p4d. Note:
        // no Owner on the auto-provisioned default depot, and the spec form's
        // Description keeps its trailing newline (the listing's `desc` doesn't).
        let m = record(&[
            ("Date", "2026/07/19 08:56:24"),
            ("Depot", "depot"),
            ("Description", "Default depot\n"),
            ("Map", "depot/..."),
            ("Type", "local"),
            ("specFormatted", ""),
        ]);
        let spec = DepotSpec::from_record(m).expect("deserialize depot -o form");
        assert_eq!(spec.depot, "depot");
        assert!(spec.owner.is_none(), "default depot has no Owner");
        // Date is captured on read (informational)...
        assert_eq!(spec.date.as_deref(), Some("2026/07/19 08:56:24"));
        assert_eq!(spec.description.as_deref(), Some("Default depot\n"));
        assert_eq!(spec.depot_type, DepotType::Local);
        assert!(spec.spec_map.is_empty());
        assert!(spec.address.is_none());
    }

    #[test]
    fn spec_deserializes_new_depot_template_form() {
        // Captured verbatim from `depot -o newstream` (a not-yet-existing
        // depot) on a fresh 2025.2 p4d: the template pre-fills every
        // conditional field (Address, StreamDepth, Suffix) even for Type local.
        let m = record(&[
            ("Address", "local"),
            ("Date", "2026/07/19 08:56:25"),
            ("Depot", "newstream"),
            ("Description", "Created by danrs.\n"),
            ("Map", "newstream/..."),
            ("Owner", "danrs"),
            ("StreamDepth", "//newstream/1"),
            ("Suffix", ".p4s"),
            ("Type", "local"),
            ("specFormatted", ""),
        ]);
        let spec = DepotSpec::from_record(m).expect("deserialize template form");
        assert_eq!(spec.depot, "newstream");
        assert_eq!(spec.owner.as_deref(), Some("danrs"));
        assert_eq!(spec.depot_type, DepotType::Local);
        assert_eq!(spec.address.as_deref(), Some("local"));
        assert_eq!(spec.stream_depth.as_deref(), Some("//newstream/1"));
        assert_eq!(spec.suffix.as_deref(), Some(".p4s"));
        assert_eq!(spec.map.as_deref(), Some("newstream/..."));
    }

    #[test]
    fn spec_collects_indexed_spec_map_lines() {
        let m = record(&[
            ("Depot", "specs"),
            ("Owner", "bruno"),
            ("Type", "spec"),
            ("Map", "specs/..."),
            ("Suffix", ".p4s"),
            ("SpecMap0", "//specs/..."),
            ("SpecMap1", "-//specs/client/..."),
        ]);
        let spec = DepotSpec::from_record(m).expect("deserialize spec depot form");
        assert_eq!(spec.depot_type, DepotType::Spec);
        assert_eq!(spec.suffix.as_deref(), Some(".p4s"));
        assert_eq!(spec.spec_map, vec!["//specs/...", "-//specs/client/..."]);
    }

    #[test]
    fn to_spec_text_omits_date_and_formats_fields() {
        let spec = DepotSpec {
            depot: "projects".to_string(),
            owner: Some("bruno".to_string()),
            date: Some("2026/07/19 12:00:00".to_string()),
            description: Some("A test depot.".to_string()),
            depot_type: DepotType::Local,
            map: Some("projects/...".to_string()),
            ..Default::default()
        };
        let text = spec.to_spec_text();
        assert!(text.contains("Depot:\tprojects\n\n"));
        assert!(text.contains("Owner:\tbruno\n\n"));
        assert!(text.contains("Description:\n\tA test depot.\n\n"));
        assert!(text.contains("Type:\tlocal\n\n"));
        assert!(text.contains("Map:\tprojects/...\n\n"));
        // Date is server-managed: never emitted.
        assert!(
            !text.contains("Date:"),
            "spec text must not send back the server-managed Date"
        );
    }

    #[test]
    fn to_spec_text_renders_multiline_spec_map() {
        let spec = DepotSpec {
            depot: "specs".to_string(),
            depot_type: DepotType::Spec,
            map: Some("specs/...".to_string()),
            suffix: Some(".p4s".to_string()),
            spec_map: vec!["//specs/...".to_string(), "-//specs/client/...".to_string()],
            ..Default::default()
        };
        let text = spec.to_spec_text();
        assert!(text.contains("Suffix:\t.p4s\n\n"));
        assert!(text.contains("SpecMap:\n\t//specs/...\n\t-//specs/client/...\n\n"));
    }
}
