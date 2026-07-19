//! Typed `p4 client` -- read (`-o`) and write (`-i`) client workspace specs.

use crate::client::{Client, UserInterface};
use crate::commands::helpers::{from_str_value, optional_string};
use crate::errors::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

impl Client {
    /// Read a client workspace spec (`p4 client -o [name]`), typed. With
    /// `None`, the connection's current client (P4CLIENT) is used. For a
    /// client that doesn't exist yet, the server returns a defaulted template
    /// -- the create flow is: read the template, modify it, save it.
    pub fn client_spec(&mut self, name: Option<&str>) -> Result<ClientSpec, Error> {
        let mut ui = UserInterface::new();
        let mut args = vec!["-o".to_string()];
        if let Some(name) = name {
            args.push(name.to_string());
        }
        let mut records = self.run_records(&mut ui, "client", args)?;
        let record = if records.is_empty() {
            HashMap::new()
        } else {
            records.swap_remove(0)
        };
        ClientSpec::from_record(record)
    }

    /// Create or update a client workspace spec (`p4 client -i`).
    pub fn save_client_spec(&mut self, spec: &ClientSpec) -> Result<(), Error> {
        let mut ui = UserInterface::new();
        ui.set_input(&spec.to_spec_text());
        self.run_records(&mut ui, "client", vec!["-i".to_string()])?;
        Ok(())
    }
}

/// One line of a client view: a depot path mapped to a client path. The depot
/// side keeps any mapping prefix (`-` exclude, `+` overlay, `&` ditto).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewMapping {
    pub depot: String,
    pub client: String,
}

impl ViewMapping {
    pub fn new(depot: impl Into<String>, client: impl Into<String>) -> Self {
        ViewMapping {
            depot: depot.into(),
            client: client.into(),
        }
    }
}

/// Take one view-line token from `s`: an optional mapping prefix (`-`/`+`/`&`)
/// followed by either a double-quoted string (for paths with spaces) or a run
/// of non-whitespace. Returns (token, rest).
fn take_view_token(s: &str) -> Result<(String, &str), String> {
    let s = s.trim_start();
    if s.is_empty() {
        return Err("expected a path, found end of line".to_string());
    }

    let (prefix, s) = match s.chars().next() {
        Some(c @ ('-' | '+' | '&')) => (Some(c), &s[c.len_utf8()..]),
        _ => (None, s),
    };

    let (body, rest) = if let Some(inner) = s.strip_prefix('"') {
        let end = inner
            .find('"')
            .ok_or_else(|| format!("unterminated quote in view line: {s:?}"))?;
        (&inner[..end], &inner[end + 1..])
    } else {
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        s.split_at(end)
    };

    if body.is_empty() {
        return Err(format!("empty path in view line near {s:?}"));
    }

    let mut token = String::new();
    if let Some(p) = prefix {
        token.push(p);
    }
    token.push_str(body);
    Ok((token, rest))
}

impl FromStr for ViewMapping {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (depot, rest) = take_view_token(s)?;
        let (client, rest) = take_view_token(rest)
            .map_err(|e| format!("view line {s:?} needs a depot and a client path: {e}"))?;
        if !rest.trim().is_empty() {
            return Err(format!("unexpected trailing text in view line: {s:?}"));
        }
        Ok(ViewMapping { depot, client })
    }
}

/// Quote a view path for the spec form if it contains whitespace. The mapping
/// prefix stays inside the quotes, matching p4's own formatting.
fn format_view_side(side: &str) -> String {
    if side.contains(char::is_whitespace) {
        format!("\"{side}\"")
    } else {
        side.to_string()
    }
}

impl Display for ViewMapping {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            format_view_side(&self.depot),
            format_view_side(&self.client)
        )
    }
}

/// The `Options:` flag set of a client spec. Each flag has an on and a `no`
/// form (`allwrite`/`noallwrite`, `locked`/`unlocked`). Tokens this version
/// doesn't know (servers grow new ones, e.g. `noaltsync`) are preserved
/// verbatim so a read-modify-write round trip doesn't drop them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientOptions {
    pub allwrite: bool,
    pub clobber: bool,
    pub compress: bool,
    pub locked: bool,
    pub modtime: bool,
    pub rmdir: bool,
    /// Unrecognized tokens, kept in order for round-tripping.
    pub unknown: Vec<String>,
}

impl FromStr for ClientOptions {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut o = ClientOptions::default();
        for token in s.split_whitespace() {
            match token {
                "allwrite" => o.allwrite = true,
                "noallwrite" => o.allwrite = false,
                "clobber" => o.clobber = true,
                "noclobber" => o.clobber = false,
                "compress" => o.compress = true,
                "nocompress" => o.compress = false,
                "locked" => o.locked = true,
                "unlocked" => o.locked = false,
                "modtime" => o.modtime = true,
                "nomodtime" => o.modtime = false,
                "rmdir" => o.rmdir = true,
                "normdir" => o.rmdir = false,
                other => o.unknown.push(other.to_string()),
            }
        }
        Ok(o)
    }
}

impl Display for ClientOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        fn onoff(on: bool, yes: &'static str, no: &'static str) -> &'static str {
            if on { yes } else { no }
        }
        write!(
            f,
            "{} {} {} {} {} {}",
            onoff(self.allwrite, "allwrite", "noallwrite"),
            onoff(self.clobber, "clobber", "noclobber"),
            onoff(self.compress, "compress", "nocompress"),
            onoff(self.locked, "locked", "unlocked"),
            onoff(self.modtime, "modtime", "nomodtime"),
            onoff(self.rmdir, "rmdir", "normdir"),
        )?;
        for u in &self.unknown {
            write!(f, " {u}")?;
        }
        Ok(())
    }
}

/// `SubmitOptions:` -- what happens to open files on submit. A closed, stable
/// set; an unknown value is a parse error rather than silently passed through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubmitOptions {
    #[default]
    SubmitUnchanged,
    SubmitUnchangedReopen,
    RevertUnchanged,
    RevertUnchangedReopen,
    LeaveUnchanged,
    LeaveUnchangedReopen,
}

impl FromStr for SubmitOptions {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "submitunchanged" => SubmitOptions::SubmitUnchanged,
            "submitunchanged+reopen" => SubmitOptions::SubmitUnchangedReopen,
            "revertunchanged" => SubmitOptions::RevertUnchanged,
            "revertunchanged+reopen" => SubmitOptions::RevertUnchangedReopen,
            "leaveunchanged" => SubmitOptions::LeaveUnchanged,
            "leaveunchanged+reopen" => SubmitOptions::LeaveUnchangedReopen,
            other => return Err(format!("unknown SubmitOptions value: {other:?}")),
        })
    }
}

impl Display for SubmitOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SubmitOptions::SubmitUnchanged => "submitunchanged",
            SubmitOptions::SubmitUnchangedReopen => "submitunchanged+reopen",
            SubmitOptions::RevertUnchanged => "revertunchanged",
            SubmitOptions::RevertUnchangedReopen => "revertunchanged+reopen",
            SubmitOptions::LeaveUnchanged => "leaveunchanged",
            SubmitOptions::LeaveUnchangedReopen => "leaveunchanged+reopen",
        })
    }
}

/// `LineEnd:` -- text-file line-ending handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnd {
    #[default]
    Local,
    Unix,
    Mac,
    Win,
    Share,
}

impl FromStr for LineEnd {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "local" => LineEnd::Local,
            "unix" => LineEnd::Unix,
            "mac" => LineEnd::Mac,
            "win" => LineEnd::Win,
            "share" => LineEnd::Share,
            other => return Err(format!("unknown LineEnd value: {other:?}")),
        })
    }
}

impl Display for LineEnd {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            LineEnd::Local => "local",
            LineEnd::Unix => "unix",
            LineEnd::Mac => "mac",
            LineEnd::Win => "win",
            LineEnd::Share => "share",
        })
    }
}

/// A client workspace spec, as read by `client -o` / written by `client -i`.
///
/// Server-managed fields (`Update`/`Access`) are kept as the raw strings the
/// server reports and are never sent back. `Type`/`Backup` are passed through
/// untyped: they exist only on newer servers and are round-tripped as-is.
#[derive(Debug, Clone, Deserialize)]
pub struct ClientSpec {
    #[serde(rename = "Client")]
    pub client: String,

    #[serde(rename = "Owner", default, deserialize_with = "optional_string")]
    pub owner: Option<String>,

    #[serde(rename = "Host", default, deserialize_with = "optional_string")]
    pub host: Option<String>,

    /// Free-form description; may span multiple lines (newline-separated).
    #[serde(rename = "Description", default)]
    pub description: String,

    #[serde(rename = "Root")]
    pub root: String,

    #[serde(rename = "Options", deserialize_with = "from_str_value")]
    pub options: ClientOptions,

    #[serde(rename = "SubmitOptions", deserialize_with = "from_str_value")]
    pub submit_options: SubmitOptions,

    #[serde(rename = "LineEnd", deserialize_with = "from_str_value")]
    pub line_end: LineEnd,

    /// e.g. "writeable"; newer servers only, round-tripped untyped.
    #[serde(rename = "Type", default, deserialize_with = "optional_string")]
    pub client_type: Option<String>,

    /// e.g. "enable"; newer servers only, round-tripped untyped.
    #[serde(rename = "Backup", default, deserialize_with = "optional_string")]
    pub backup: Option<String>,

    /// Server-managed; never sent back on save.
    #[serde(rename = "Update", default, deserialize_with = "optional_string")]
    pub update: Option<String>,

    /// Server-managed; never sent back on save.
    #[serde(rename = "Access", default, deserialize_with = "optional_string")]
    pub access: Option<String>,

    /// The view, from the `View0..ViewN` record keys.
    #[serde(skip)]
    pub view: Vec<ViewMapping>,
}

impl ClientSpec {
    /// Build a typed spec from a tagged `client -o` record.
    pub fn from_record(mut record: HashMap<String, String>) -> Result<ClientSpec, Error> {
        // Spec-output bookkeeping, not a field.
        record.remove("specFormatted");

        let mut view = Vec::new();
        let mut i = 0;
        while let Some(line) = record.remove(&format!("View{i}")) {
            view.push(
                line.parse::<ViewMapping>()
                    .map_err(|e| Error::SpecError(format!("View{i}: {e}")))?,
            );
            i += 1;
        }

        let mut spec = ClientSpec::deserialize(serde::de::value::MapDeserializer::new(
            record.clone().into_iter(),
        ))
        .map_err(|e| Error::SerializationError(e, record))?;
        spec.view = view;
        Ok(spec)
    }

    /// Render the spec as the text form `client -i` reads. Server-managed
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

        single(&mut out, "Client", &self.client);
        if let Some(owner) = &self.owner {
            single(&mut out, "Owner", owner);
        }
        if let Some(host) = &self.host {
            single(&mut out, "Host", host);
        }
        if !self.description.is_empty() {
            multi(
                &mut out,
                "Description",
                &mut self.description.lines().map(str::to_string),
            );
        }
        single(&mut out, "Root", &self.root);
        single(&mut out, "Options", &self.options);
        single(&mut out, "SubmitOptions", &self.submit_options);
        single(&mut out, "LineEnd", &self.line_end);
        if let Some(client_type) = &self.client_type {
            single(&mut out, "Type", client_type);
        }
        if let Some(backup) = &self.backup {
            single(&mut out, "Backup", backup);
        }
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

    fn captured_record() -> HashMap<String, String> {
        // Verbatim shape from a real tagged `client -o` (p4d 2022.2).
        [
            ("Client", "cap-ws"),
            ("Owner", "danrs"),
            ("Host", "Dan-Desktop"),
            ("Description", "Created by danrs.\n"),
            ("Root", "c:\\work\\p4"),
            (
                "Options",
                "noallwrite noclobber nocompress unlocked nomodtime normdir",
            ),
            ("SubmitOptions", "submitunchanged"),
            ("LineEnd", "local"),
            ("Type", "writeable"),
            ("Backup", "enable"),
            ("View0", "//depot/... //cap-ws/..."),
            ("specFormatted", ""),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn client_spec_from_captured_record() {
        let spec = ClientSpec::from_record(captured_record()).expect("parse captured record");
        assert_eq!(spec.client, "cap-ws");
        assert_eq!(spec.owner.as_deref(), Some("danrs"));
        assert_eq!(spec.description, "Created by danrs.\n");
        assert!(!spec.options.allwrite);
        assert!(!spec.options.locked);
        assert!(spec.options.unknown.is_empty());
        assert_eq!(spec.submit_options, SubmitOptions::SubmitUnchanged);
        assert_eq!(spec.line_end, LineEnd::Local);
        assert_eq!(spec.client_type.as_deref(), Some("writeable"));
        assert_eq!(
            spec.view,
            vec![ViewMapping::new("//depot/...", "//cap-ws/...")]
        );
        // Update/Access absent on an unsaved template.
        assert!(spec.update.is_none());
    }

    #[test]
    fn spec_text_round_trips_the_essentials() {
        let mut spec = ClientSpec::from_record(captured_record()).unwrap();
        spec.description = "Line one.\nLine two.".to_string();
        spec.view.push(ViewMapping::new(
            "-//depot/secret/...",
            "//cap-ws/secret/...",
        ));

        let text = spec.to_spec_text();
        assert!(text.contains("Client:\tcap-ws\n"));
        assert!(text.contains("Description:\n\tLine one.\n\tLine two.\n"));
        assert!(
            text.contains("Options:\tnoallwrite noclobber nocompress unlocked nomodtime normdir\n")
        );
        assert!(text.contains(
            "View:\n\t//depot/... //cap-ws/...\n\t-//depot/secret/... //cap-ws/secret/...\n"
        ));
        // Server-managed fields are never written.
        assert!(!text.contains("Update:"));
        assert!(!text.contains("Access:"));
    }

    #[test]
    fn view_mapping_parses_quotes_and_prefixes() {
        let m: ViewMapping = "//depot/... //ws/...".parse().unwrap();
        assert_eq!(m, ViewMapping::new("//depot/...", "//ws/..."));

        let m: ViewMapping = "\"//depot/has space/...\" \"//ws/has space/...\""
            .parse()
            .unwrap();
        assert_eq!(m.depot, "//depot/has space/...");
        assert_eq!(m.client, "//ws/has space/...");

        let m: ViewMapping = "-//depot/x/... //ws/x/...".parse().unwrap();
        assert_eq!(m.depot, "-//depot/x/...");

        let m: ViewMapping = "+\"//depot/a b/...\" //ws/ab/...".parse().unwrap();
        assert_eq!(m.depot, "+//depot/a b/...");

        assert!("//depot/only-one".parse::<ViewMapping>().is_err());
        assert!("//a //b //c".parse::<ViewMapping>().is_err());
        assert!("\"//unterminated //ws/...".parse::<ViewMapping>().is_err());
    }

    #[test]
    fn view_mapping_display_quotes_spaces() {
        let m = ViewMapping::new("//depot/has space/...", "//ws/x/...");
        assert_eq!(m.to_string(), "\"//depot/has space/...\" //ws/x/...");
        // Round trip.
        assert_eq!(m.to_string().parse::<ViewMapping>().unwrap(), m);
    }

    #[test]
    fn client_options_round_trip_preserves_unknown_tokens() {
        let s = "allwrite noclobber compress locked nomodtime rmdir noaltsync";
        let o: ClientOptions = s.parse().unwrap();
        assert!(o.allwrite && o.compress && o.locked && o.rmdir);
        assert!(!o.clobber && !o.modtime);
        assert_eq!(o.unknown, vec!["noaltsync"]);
        assert_eq!(o.to_string(), s);
    }

    #[test]
    fn submit_options_and_line_end_round_trip() {
        for s in [
            "submitunchanged",
            "submitunchanged+reopen",
            "revertunchanged",
            "revertunchanged+reopen",
            "leaveunchanged",
            "leaveunchanged+reopen",
        ] {
            assert_eq!(s.parse::<SubmitOptions>().unwrap().to_string(), s);
        }
        assert!("frobnicate".parse::<SubmitOptions>().is_err());

        for s in ["local", "unix", "mac", "win", "share"] {
            assert_eq!(s.parse::<LineEnd>().unwrap().to_string(), s);
        }
        assert!("vms".parse::<LineEnd>().is_err());
    }
}
