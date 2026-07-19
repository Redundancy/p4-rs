//! Path resolution: `where` (map a path through the client view) and `have`
//! (what revision the workspace holds).
//!
//! Both report all three path forms -- `depotFile` (`//depot/...`),
//! `clientFile` (`//client/...`), and `path` (the local filesystem path).

use crate::client::{Client, UserInterface};
use crate::commands::files::{parse_records, str_args};
use crate::commands::helpers::{from_str_value, opt_from_str};
use crate::errors::Error;
use serde::Deserialize;

/// One line of `p4 where`: the three names for a mapped path.
#[derive(Debug, Clone, Deserialize)]
pub struct WhereMapping {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    /// `//client/...` syntax.
    #[serde(rename = "clientFile")]
    pub client_file: String,

    /// Local filesystem path.
    #[serde(rename = "path")]
    pub path: String,
}

/// One line of `p4 have`: a synced file and the revision the workspace holds.
#[derive(Debug, Clone, Deserialize)]
pub struct HaveFile {
    #[serde(rename = "depotFile")]
    pub depot_file: String,

    /// `//client/...` syntax.
    #[serde(rename = "clientFile")]
    pub client_file: String,

    /// Local filesystem path.
    #[serde(rename = "path")]
    pub path: String,

    /// The revision the workspace holds.
    #[serde(rename = "haveRev", deserialize_with = "from_str_value")]
    pub have_rev: u64,

    /// When the file was synced, epoch seconds.
    #[serde(rename = "syncTime", default, deserialize_with = "opt_from_str")]
    pub sync_time: Option<u64>,
}

impl Client {
    /// Map paths through the client view (`p4 where`). With no paths, maps the
    /// whole client view. Note: `where` is a Rust keyword, hence the method
    /// name `where_files`.
    pub fn where_files(&mut self, paths: &[&str]) -> Result<Vec<WhereMapping>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "where", str_args(paths))?;
        parse_records(records)
    }

    /// List the revisions the workspace holds (`p4 have`). With no paths,
    /// reports the whole workspace.
    pub fn have(&mut self, paths: &[&str]) -> Result<Vec<HaveFile>, Error> {
        let mut ui = UserInterface::new();
        let records = self.run_records(&mut ui, "have", str_args(paths))?;
        parse_records(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::value::MapDeserializer;
    use std::collections::HashMap;

    fn de<T: for<'de> Deserialize<'de>>(pairs: &[(&str, &str)]) -> T {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        T::deserialize(MapDeserializer::<_, serde::de::value::Error>::new(
            m.into_iter(),
        ))
        .expect("deserialize record")
    }

    // Captured verbatim from `p4 where`.
    #[test]
    fn where_mapping_from_record() {
        let w: WhereMapping = de(&[
            ("clientFile", "//fc-ws/hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("path", "C:\\ws\\hello.txt"),
        ]);
        assert_eq!(w.depot_file, "//depot/hello.txt");
        assert_eq!(w.client_file, "//fc-ws/hello.txt");
        assert_eq!(w.path, "C:\\ws\\hello.txt");
    }

    // Captured verbatim from `p4 have`.
    #[test]
    fn have_file_from_record() {
        let h: HaveFile = de(&[
            ("clientFile", "//fc-ws/hello.txt"),
            ("depotFile", "//depot/hello.txt"),
            ("haveRev", "1"),
            ("path", "C:\\ws\\hello.txt"),
            ("syncTime", "1784475848"),
        ]);
        assert_eq!(h.have_rev, 1);
        assert_eq!(h.sync_time, Some(1_784_475_848));
        assert_eq!(h.depot_file, "//depot/hello.txt");
    }
}
