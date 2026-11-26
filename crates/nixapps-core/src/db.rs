use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{NixAppsError, Result};
use crate::models::{CachedMetadata, Category, PackageRecord, SearchQuery, SyncInfo};

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS sync_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    channel TEXT NOT NULL,
    package_count INTEGER NOT NULL,
    synced_at_epoch INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS packages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attr_path TEXT NOT NULL UNIQUE,
    pname TEXT NOT NULL,
    version TEXT NOT NULL,
    description TEXT,
    long_description TEXT,
    homepage TEXT,
    license TEXT,
    main_program TEXT,
    position TEXT,
    category TEXT NOT NULL,
    unfree INTEGER NOT NULL DEFAULT 0,
    broken INTEGER NOT NULL DEFAULT 0,
    insecure INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_packages_category ON packages(category);
CREATE INDEX IF NOT EXISTS idx_packages_pname ON packages(pname);

CREATE VIRTUAL TABLE IF NOT EXISTS packages_fts USING fts5(
    attr_path,
    pname,
    description,
    content='packages',
    content_rowid='id',
    tokenize='unicode61'
);

CREATE TABLE IF NOT EXISTS metadata_cache (
    attr_path TEXT NOT NULL,
    source TEXT NOT NULL,
    icon_url TEXT,
    briefing TEXT,
    stars INTEGER,
    fetched_at_epoch INTEGER NOT NULL,
    PRIMARY KEY (attr_path, source)
);
"#;

/// Local SQLite + FTS5 package catalog and metadata cache.
pub struct CatalogDb {
    conn: Connection,
}

impl CatalogDb {
    /// Opens (or creates) a persistent catalog database on disk.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| NixAppsError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    /// Opens an in-memory SQLite database (useful for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = MEMORY;",
        )?;
        self.conn.execute_batch(SCHEMA_SQL)?;
        Ok(())
    }

    /// Starts an atomic catalog replacement session that streams packages directly
    /// into SQLite inside a single transaction and rebuilds the FTS5 index on commit.
    pub fn begin_catalog_replace<'a>(&'a mut self, channel: &str) -> Result<CatalogWriter<'a>> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM packages", [])?;
        Ok(CatalogWriter {
            tx,
            channel: channel.to_string(),
            inserted: 0,
        })
    }

    /// Returns metadata about the last completed channel sync, if any.
    pub fn sync_info(&self) -> Result<Option<SyncInfo>> {
        self.conn
            .query_row(
                "SELECT channel, package_count, synced_at_epoch FROM sync_meta WHERE id = 1",
                [],
                |row| {
                    Ok(SyncInfo {
                        channel: row.get(0)?,
                        package_count: row.get::<_, i64>(1)? as usize,
                        synced_at_epoch: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Fetches a single package by exact `attr_path`.
    pub fn get_package(&self, attr_path: &str) -> Result<Option<PackageRecord>> {
        self.conn
            .query_row(
                "SELECT attr_path, pname, version, description, long_description,
                        homepage, license, main_program, position, category,
                        unfree, broken, insecure
                 FROM packages
                 WHERE attr_path = ?1",
                params![attr_path],
                row_to_package,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Fetches multiple packages preserving the requested attribute order, skipping missing ones.
    pub fn get_packages_by_attrs(&self, attrs: &[&str]) -> Result<Vec<PackageRecord>> {
        let mut out = Vec::with_capacity(attrs.len());
        for attr in attrs {
            if let Some(pkg) = self.get_package(attr)? {
                out.push(pkg);
            }
        }
        Ok(out)
    }

    /// Returns package counts per category (excluding broken packages by default).
    pub fn category_counts(&self, include_broken: bool) -> Result<Vec<(Category, usize)>> {
        let sql = if include_broken {
            "SELECT category, COUNT(*) FROM packages GROUP BY category ORDER BY COUNT(*) DESC"
        } else {
            "SELECT category, COUNT(*) FROM packages WHERE broken = 0 GROUP BY category ORDER BY COUNT(*) DESC"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |row| {
            let cat: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((Category::parse(&cat), count as usize))
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Searches the local catalog using FTS5 full-text ranking + exact-match boosting.
    pub fn search(&self, query: &SearchQuery) -> Result<Vec<PackageRecord>> {
        let limit = query.limit.clamp(1, 500) as i64;
        let offset = query.offset as i64;
        let trimmed = query.text.trim();
        let cat_filter = query.category.map(|c| c.as_str().to_string());
        let allow_unfree: i64 = if query.include_unfree { 1 } else { 0 };
        let allow_broken: i64 = if query.include_broken { 1 } else { 0 };

        if trimmed.is_empty() {
            let mut stmt = self.conn.prepare(
                "SELECT attr_path, pname, version, description, long_description,
                        homepage, license, main_program, position, category,
                        unfree, broken, insecure
                 FROM packages
                 WHERE (?1 IS NULL OR category = ?1)
                   AND (?2 = 1 OR unfree = 0)
                   AND (?3 = 1 OR broken = 0)
                 ORDER BY pname ASC, attr_path ASC
                 LIMIT ?4 OFFSET ?5",
            )?;
            let rows = stmt.query_map(
                params![cat_filter, allow_unfree, allow_broken, limit, offset],
                row_to_package,
            )?;
            return rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into);
        }

        let Some(fts_query) = build_fts_query(trimmed) else {
            return Ok(Vec::new());
        };

        let exact_lower = trimmed.to_lowercase();
        let prefix_pattern = format!("{exact_lower}%");

        let mut stmt = self.conn.prepare(
            "SELECT p.attr_path, p.pname, p.version, p.description, p.long_description,
                    p.homepage, p.license, p.main_program, p.position, p.category,
                    p.unfree, p.broken, p.insecure
             FROM packages_fts f
             JOIN packages p ON p.id = f.rowid
             WHERE packages_fts MATCH ?1
               AND (?2 IS NULL OR p.category = ?2)
               AND (?3 = 1 OR p.unfree = 0)
               AND (?4 = 1 OR p.broken = 0)
             ORDER BY
               CASE
                 WHEN lower(p.attr_path) = ?5 THEN 0
                 WHEN lower(p.pname) = ?5 THEN 1
                 WHEN lower(p.attr_path) LIKE ?6 THEN 2
                 WHEN lower(p.pname) LIKE ?6 THEN 3
                 ELSE 4
               END ASC,
               bm25(packages_fts, 4.0, 3.0, 1.0) ASC,
               length(p.attr_path) ASC
             LIMIT ?7 OFFSET ?8",
        )?;

        let rows = stmt.query_map(
            params![
                fts_query,
                cat_filter,
                allow_unfree,
                allow_broken,
                exact_lower,
                prefix_pattern,
                limit,
                offset
            ],
            row_to_package,
        )?;

        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Inserts or updates a cached external metadata record (AppStream/Flathub/GitHub).
    pub fn upsert_cached_metadata(&self, entry: &CachedMetadata) -> Result<()> {
        self.conn.execute(
            "INSERT INTO metadata_cache (attr_path, source, icon_url, briefing, stars, fetched_at_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(attr_path, source) DO UPDATE SET
               icon_url = excluded.icon_url,
               briefing = excluded.briefing,
               stars = excluded.stars,
               fetched_at_epoch = excluded.fetched_at_epoch",
            params![
                entry.attr_path,
                entry.source,
                entry.icon_url,
                entry.briefing,
                entry.stars,
                entry.fetched_at_epoch
            ],
        )?;
        Ok(())
    }

    /// Fetches cached external metadata for a given package and source.
    pub fn get_cached_metadata(
        &self,
        attr_path: &str,
        source: &str,
    ) -> Result<Option<CachedMetadata>> {
        self.conn
            .query_row(
                "SELECT attr_path, source, icon_url, briefing, stars, fetched_at_epoch
                 FROM metadata_cache
                 WHERE attr_path = ?1 AND source = ?2",
                params![attr_path, source],
                |row| {
                    Ok(CachedMetadata {
                        attr_path: row.get(0)?,
                        source: row.get(1)?,
                        icon_url: row.get(2)?,
                        briefing: row.get(3)?,
                        stars: row.get(4)?,
                        fetched_at_epoch: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
}

/// Streaming transactional writer returned by [`CatalogDb::begin_catalog_replace`].
pub struct CatalogWriter<'a> {
    tx: rusqlite::Transaction<'a>,
    channel: String,
    inserted: usize,
}

impl<'a> CatalogWriter<'a> {
    /// Inserts a single package into the active sync transaction.
    pub fn insert(&mut self, pkg: &PackageRecord) -> Result<()> {
        let mut stmt = self.tx.prepare_cached(
            "INSERT OR REPLACE INTO packages (
                attr_path, pname, version, description, long_description,
                homepage, license, main_program, position, category,
                unfree, broken, insecure
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        )?;

        stmt.execute(params![
            pkg.attr_path,
            pkg.pname,
            pkg.version,
            pkg.description,
            pkg.long_description,
            pkg.homepage,
            pkg.license,
            pkg.main_program,
            pkg.position,
            pkg.category.as_str(),
            i64::from(pkg.unfree),
            i64::from(pkg.broken),
            i64::from(pkg.insecure),
        ])?;

        self.inserted += 1;
        Ok(())
    }

    /// Rebuilds the FTS5 index, records `sync_meta`, and commits the transaction.
    pub fn commit(self) -> Result<SyncInfo> {
        self.tx.execute(
            "INSERT INTO packages_fts(packages_fts) VALUES('rebuild')",
            [],
        )?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.tx.execute(
            "INSERT INTO sync_meta (id, channel, package_count, synced_at_epoch)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               channel = excluded.channel,
               package_count = excluded.package_count,
               synced_at_epoch = excluded.synced_at_epoch",
            params![self.channel, self.inserted as i64, now],
        )?;

        let info = SyncInfo {
            channel: self.channel,
            package_count: self.inserted,
            synced_at_epoch: now,
        };

        self.tx.commit()?;
        Ok(info)
    }
}

fn row_to_package(row: &rusqlite::Row<'_>) -> rusqlite::Result<PackageRecord> {
    let category_str: String = row.get(9)?;
    Ok(PackageRecord {
        attr_path: row.get(0)?,
        pname: row.get(1)?,
        version: row.get(2)?,
        description: row.get(3)?,
        long_description: row.get(4)?,
        homepage: row.get(5)?,
        license: row.get(6)?,
        main_program: row.get(7)?,
        position: row.get(8)?,
        category: Category::parse(&category_str),
        unfree: row.get::<_, i64>(10)? != 0,
        broken: row.get::<_, i64>(11)? != 0,
        insecure: row.get::<_, i64>(12)? != 0,
    })
}

/// Converts raw user input into a safe FTS5 prefix query (`"tok1"* "tok2"*`).
fn build_fts_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|s| !s.is_empty())
        .map(|tok| {
            let escaped = tok.replace('"', "");
            format!("\"{escaped}\"*")
        })
        .collect();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pkg(
        attr: &str,
        pname: &str,
        desc: &str,
        cat: Category,
        unfree: bool,
    ) -> PackageRecord {
        PackageRecord {
            attr_path: attr.into(),
            pname: pname.into(),
            version: "1.0.0".into(),
            description: Some(desc.into()),
            long_description: None,
            homepage: Some(format!("https://example.org/{pname}")),
            license: Some("MIT".into()),
            main_program: Some(pname.into()),
            position: None,
            category: cat,
            unfree,
            broken: false,
            insecure: false,
        }
    }

    #[test]
    fn fts5_search_ranks_exact_match_first_and_respects_filters() {
        let mut db = CatalogDb::open_in_memory().unwrap();
        let mut writer = db.begin_catalog_replace("nixos-26.05").unwrap();
        writer
            .insert(&sample_pkg(
                "obsidian",
                "obsidian",
                "Powerful knowledge base on top of local Markdown files",
                Category::Office,
                true,
            ))
            .unwrap();
        writer
            .insert(&sample_pkg(
                "obsidian-export",
                "obsidian-export",
                "Export an Obsidian vault to regular Markdown",
                Category::Utilities,
                false,
            ))
            .unwrap();
        writer
            .insert(&sample_pkg(
                "ripgrep",
                "ripgrep",
                "Fast line-oriented search tool",
                Category::Development,
                false,
            ))
            .unwrap();
        let info = writer.commit().unwrap();
        assert_eq!(info.package_count, 3);

        // Exact match should come first.
        let results = db.search(&SearchQuery::new("obsidian")).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].attr_path, "obsidian");
        assert_eq!(results[1].attr_path, "obsidian-export");

        // Excluding unfree hides obsidian but keeps obsidian-export.
        let mut free_only = SearchQuery::new("obsidian");
        free_only.include_unfree = false;
        let free_results = db.search(&free_only).unwrap();
        assert_eq!(free_results.len(), 1);
        assert_eq!(free_results[0].attr_path, "obsidian-export");

        // Category filter works.
        let mut dev_only = SearchQuery::new("");
        dev_only.category = Some(Category::Development);
        let dev_results = db.search(&dev_only).unwrap();
        assert_eq!(dev_results.len(), 1);
        assert_eq!(dev_results[0].attr_path, "ripgrep");
    }

    #[test]
    fn fts5_handles_special_characters_without_syntax_errors() {
        let mut db = CatalogDb::open_in_memory().unwrap();
        let mut writer = db.begin_catalog_replace("nixos-26.05").unwrap();
        writer
            .insert(&sample_pkg(
                "nix-search-cli",
                "nix-search-cli",
                "CLI for search.nixos.org",
                Category::Utilities,
                false,
            ))
            .unwrap();
        writer.commit().unwrap();

        let res = db.search(&SearchQuery::new("nix-search")).unwrap();
        assert_eq!(res.len(), 1);

        // Weird punctuation must not crash FTS5.
        let _ = db
            .search(&SearchQuery::new("c++ \"broken\" (test) : *"))
            .unwrap();
    }
}
