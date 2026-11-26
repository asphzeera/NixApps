use std::fmt;
use std::io::{BufReader, Read};
use std::time::Duration;

use brotli::Decompressor;
use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};

use crate::db::{CatalogDb, CatalogWriter};
use crate::error::{NixAppsError, Result};
use crate::models::{Category, PackageRecord, SyncInfo};

/// Base URL for official NixOS channel artifacts.
pub const CHANNELS_BASE_URL: &str = "https://channels.nixos.org";

/// Validates a NixOS channel name (e.g. `nixos-26.05`, `nixos-unstable`).
pub fn validate_channel_name(channel: &str) -> Result<()> {
    if channel.is_empty()
        || !channel
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err(NixAppsError::InvalidChannel(channel.to_string()));
    }
    Ok(())
}

/// Builds the official `packages.json.br` URL for a given NixOS channel.
pub fn channel_packages_url(channel: &str) -> Result<String> {
    validate_channel_name(channel)?;
    Ok(format!("{CHANNELS_BASE_URL}/{channel}/packages.json.br"))
}

/// Downloads `packages.json.br` for `channel`, decompresses Brotli on the fly, and
/// streams all packages directly into `db` with near-constant RAM usage.
///
/// The optional `on_progress` callback is invoked every 2,000 indexed packages with
/// the running count so UI code can update a progress indicator.
pub fn sync_channel<F>(db: &mut CatalogDb, channel: &str, on_progress: F) -> Result<SyncInfo>
where
    F: FnMut(usize),
{
    let url = channel_packages_url(channel)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("NixApps/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .build()?;

    let response = client.get(&url).send()?.error_for_status()?;
    let reader = BufReader::new(response);
    let decompressor = Decompressor::new(reader, 64 * 1024);

    sync_from_json_reader(db, channel, BufReader::new(decompressor), on_progress)
}

/// Streams a plain (uncompressed) `packages.json` reader directly into `db`.
///
/// Exposed publicly so tests and offline imports can stream from local fixtures
/// without network access.
pub fn sync_from_json_reader<R, F>(
    db: &mut CatalogDb,
    channel: &str,
    reader: R,
    on_progress: F,
) -> Result<SyncInfo>
where
    R: Read,
    F: FnMut(usize),
{
    validate_channel_name(channel)?;

    let mut writer = db.begin_catalog_replace(channel)?;
    let mut deserializer = serde_json::Deserializer::from_reader(reader);

    let seed = RootStreamSeed {
        writer: &mut writer,
        on_progress,
    };

    de::DeserializeSeed::deserialize(seed, &mut deserializer)
        .map_err(|e| NixAppsError::Sync(e.to_string()))?;

    writer.commit()
}

/// Streaming serde seed for the top-level `{ "packages": { ... } }` object.
struct RootStreamSeed<'w, 'db, F> {
    writer: &'w mut CatalogWriter<'db>,
    on_progress: F,
}

impl<'de, 'w, 'db, F> de::DeserializeSeed<'de> for RootStreamSeed<'w, 'db, F>
where
    F: FnMut(usize),
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RootVisitor<'w, 'db, F> {
            writer: &'w mut CatalogWriter<'db>,
            on_progress: F,
        }

        impl<'de, 'w, 'db, F> Visitor<'de> for RootVisitor<'w, 'db, F>
        where
            F: FnMut(usize),
        {
            type Value = ();

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a NixOS packages.json root object containing 'packages'")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<(), M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut saw_packages = false;
                let mut on_progress = self.on_progress;

                while let Some(key) = map.next_key::<String>()? {
                    if key == "packages" {
                        saw_packages = true;
                        map.next_value_seed(PackagesMapSeed {
                            writer: self.writer,
                            on_progress: &mut on_progress,
                        })?;
                    } else {
                        let _: IgnoredAny = map.next_value()?;
                    }
                }

                if !saw_packages {
                    return Err(de::Error::missing_field("packages"));
                }
                Ok(())
            }
        }

        deserializer.deserialize_map(RootVisitor {
            writer: self.writer,
            on_progress: self.on_progress,
        })
    }
}

/// Streaming serde seed for the `"packages": { "<attr>": { ... }, ... }` map.
struct PackagesMapSeed<'w, 'db, 'p, F> {
    writer: &'w mut CatalogWriter<'db>,
    on_progress: &'p mut F,
}

impl<'de, 'w, 'db, 'p, F> de::DeserializeSeed<'de> for PackagesMapSeed<'w, 'db, 'p, F>
where
    F: FnMut(usize),
{
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PackagesVisitor<'w, 'db, 'p, F> {
            writer: &'w mut CatalogWriter<'db>,
            on_progress: &'p mut F,
        }

        impl<'de, 'w, 'db, 'p, F> Visitor<'de> for PackagesVisitor<'w, 'db, 'p, F>
        where
            F: FnMut(usize),
        {
            type Value = ();

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map of attribute paths to package objects")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<(), M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut count: usize = 0;

                while let Some((attr_path, raw)) = map.next_entry::<String, RawPackage>()? {
                    let record = raw.into_record(attr_path);
                    self.writer
                        .insert(&record)
                        .map_err(|e| de::Error::custom(format!("db insert failed: {e}")))?;

                    count += 1;
                    if count.is_multiple_of(2_000) {
                        (self.on_progress)(count);
                    }
                }

                (self.on_progress)(count);
                Ok(())
            }
        }

        deserializer.deserialize_map(PackagesVisitor {
            writer: self.writer,
            on_progress: self.on_progress,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    #[serde(default)]
    pname: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    meta: RawMeta,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMeta {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    long_description: Option<String>,
    #[serde(default)]
    homepage: Option<HomepageField>,
    #[serde(default)]
    license: Option<LicenseField>,
    #[serde(default)]
    main_program: Option<String>,
    #[serde(default)]
    position: Option<String>,
    #[serde(default)]
    unfree: bool,
    #[serde(default)]
    broken: bool,
    #[serde(default)]
    insecure: bool,
}

/// Nixpkgs `meta.homepage` can be a single string or an array of strings.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HomepageField {
    Single(String),
    Multiple(Vec<String>),
}

impl HomepageField {
    fn primary(self) -> Option<String> {
        match self {
            HomepageField::Single(s) => (!s.trim().is_empty()).then_some(s),
            HomepageField::Multiple(list) => list.into_iter().find(|s| !s.trim().is_empty()),
        }
    }
}

/// Nixpkgs `meta.license` can be a string, an object, or an array of either.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LicenseField {
    Text(String),
    Obj(LicenseObj),
    List(Vec<LicenseItem>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LicenseItem {
    Text(String),
    Obj(LicenseObj),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LicenseObj {
    #[serde(default)]
    spdx_id: Option<String>,
    #[serde(default)]
    short_name: Option<String>,
    #[serde(default)]
    full_name: Option<String>,
}

impl LicenseObj {
    fn label(self) -> Option<String> {
        self.spdx_id.or(self.short_name).or(self.full_name)
    }
}

impl LicenseField {
    fn summary(self) -> Option<String> {
        match self {
            LicenseField::Text(s) => (!s.trim().is_empty()).then_some(s),
            LicenseField::Obj(o) => o.label(),
            LicenseField::List(items) => {
                let labels: Vec<String> = items
                    .into_iter()
                    .filter_map(|item| match item {
                        LicenseItem::Text(s) => (!s.trim().is_empty()).then_some(s),
                        LicenseItem::Obj(o) => o.label(),
                    })
                    .collect();
                if labels.is_empty() {
                    None
                } else {
                    Some(labels.join(", "))
                }
            }
        }
    }
}

impl RawPackage {
    fn into_record(self, attr_path: String) -> PackageRecord {
        let pname = self
            .pname
            .filter(|s| !s.is_empty())
            .or(self.name)
            .unwrap_or_else(|| attr_path.clone());
        let version = self.version.unwrap_or_default();
        let category = infer_category(
            &attr_path,
            self.meta.position.as_deref(),
            self.meta.description.as_deref(),
        );

        let description = self.meta.description.filter(|s| !s.trim().is_empty());
        let long_description = self.meta.long_description.filter(|s| !s.trim().is_empty());

        PackageRecord {
            attr_path,
            pname,
            version,
            description,
            long_description,
            homepage: self.meta.homepage.and_then(HomepageField::primary),
            license: self.meta.license.and_then(LicenseField::summary),
            main_program: self.meta.main_program,
            position: self.meta.position,
            category,
            unfree: self.meta.unfree,
            broken: self.meta.broken,
            insecure: self.meta.insecure,
        }
    }
}

/// Infers an initial store category from Nixpkgs `meta.position`, attribute prefix, and description.
///
/// In Phase 3, AppStream / Flathub metadata overrides this heuristic whenever available.
pub fn infer_category(
    attr_path: &str,
    position: Option<&str>,
    description: Option<&str>,
) -> Category {
    if let Some(pos) = position {
        let pos_l = pos.to_lowercase();
        if pos_l.contains("/games/") {
            return Category::Games;
        }
        if pos_l.contains("/audio/") || pos_l.contains("/video/") {
            return Category::AudioVideo;
        }
        if pos_l.contains("/graphics/") {
            return Category::Graphics;
        }
        if pos_l.contains("/office/") || pos_l.contains("/editors/") {
            return Category::Office;
        }
        if pos_l.contains("/networking/") || pos_l.contains("/browsers/") {
            return Category::Network;
        }
        if pos_l.contains("/development/") || pos_l.contains("/compilers/") {
            return Category::Development;
        }
        if pos_l.contains("/science/") {
            return Category::Science;
        }
        if pos_l.contains("/education/") {
            return Category::Education;
        }
        if pos_l.contains("/os-specific/")
            || pos_l.contains("/system/")
            || pos_l.contains("/admin/")
        {
            return Category::System;
        }
        if pos_l.contains("/tools/") {
            return Category::Utilities;
        }
    }

    let attr_l = attr_path.to_lowercase();
    if attr_l.starts_with("python3packages.")
        || attr_l.starts_with("nodepackages.")
        || attr_l.starts_with("perlpackages.")
        || attr_l.starts_with("haskellpackages.")
        || attr_l.starts_with("ocamlpackages.")
    {
        return Category::Development;
    }

    if let Some(desc) = description {
        let d = desc.to_lowercase();
        if d.contains("game") || d.contains("emulator") {
            return Category::Games;
        }
        if d.contains("audio")
            || d.contains("music")
            || d.contains("video")
            || d.contains("media player")
        {
            return Category::AudioVideo;
        }
        if d.contains("image editor") || d.contains("vector graphics") || d.contains("3d") {
            return Category::Graphics;
        }
        if d.contains("browser") || d.contains("torrent") || d.contains("chat") || d.contains("vpn")
        {
            return Category::Network;
        }
        if d.contains("compiler")
            || d.contains("ide")
            || d.contains("debugger")
            || d.contains("programming language")
        {
            return Category::Development;
        }
        if d.contains("markdown")
            || d.contains("note")
            || d.contains("spreadsheet")
            || d.contains("office")
        {
            return Category::Office;
        }
    }

    Category::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SearchQuery;
    use std::io::Cursor;

    const SAMPLE_JSON: &str = r#"{
      "version": 2,
      "packages": {
        "obsidian": {
          "pname": "obsidian",
          "version": "1.7.7",
          "meta": {
            "description": "Powerful knowledge base that works on top of a local folder of plain text Markdown files",
            "homepage": ["https://obsidian.md", "https://github.com/obsidianmd"],
            "license": { "spdxId": "Unfree", "shortName": "unfree" },
            "mainProgram": "obsidian",
            "position": "pkgs/by-name/ob/obsidian/package.nix:42",
            "unfree": true,
            "broken": false
          }
        },
        "lutris": {
          "pname": "lutris",
          "version": "0.5.18",
          "meta": {
            "description": "Open Source gaming platform for GNU/Linux",
            "homepage": "https://lutris.net",
            "license": [
              { "spdxId": "GPL-3.0-or-later" },
              { "shortName": "mit" }
            ],
            "position": "pkgs/games/lutris/default.nix:10",
            "unfree": false
          }
        },
        "ripgrep": {
          "pname": "ripgrep",
          "version": "14.1.1",
          "meta": {
            "description": "Utility that combines the usability of The Silver Searcher with the raw speed of grep",
            "homepage": "https://github.com/BurntSushi/ripgrep",
            "license": "MIT",
            "position": "pkgs/tools/text/ripgrep/default.nix:15"
          }
        }
      }
    }"#;

    #[test]
    fn streaming_sync_parses_polymorphic_nixpkgs_fields() {
        let mut db = CatalogDb::open_in_memory().unwrap();
        let mut last_count = 0;

        let info = sync_from_json_reader(
            &mut db,
            "nixos-26.05",
            Cursor::new(SAMPLE_JSON.as_bytes()),
            |c| last_count = c,
        )
        .unwrap();

        assert_eq!(info.channel, "nixos-26.05");
        assert_eq!(info.package_count, 3);
        assert_eq!(last_count, 3);

        let obsidian = db.get_package("obsidian").unwrap().unwrap();
        assert_eq!(obsidian.homepage.as_deref(), Some("https://obsidian.md"));
        assert_eq!(obsidian.category, Category::Office);
        assert!(obsidian.unfree);

        let lutris = db.get_package("lutris").unwrap().unwrap();
        assert_eq!(lutris.license.as_deref(), Some("GPL-3.0-or-later, mit"));
        assert_eq!(lutris.category, Category::Games);

        let rg = db.get_package("ripgrep").unwrap().unwrap();
        assert_eq!(rg.license.as_deref(), Some("MIT"));
        assert_eq!(rg.category, Category::Utilities);

        let found = db.search(&SearchQuery::new("markdown")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].attr_path, "obsidian");
    }

    #[test]
    fn channel_validation_blocks_path_traversal() {
        assert!(channel_packages_url("nixos-26.05").is_ok());
        assert!(channel_packages_url("nixos-unstable").is_ok());
        assert!(channel_packages_url("../evil").is_err());
        assert!(channel_packages_url("nixos/26.05").is_err());
    }
}
