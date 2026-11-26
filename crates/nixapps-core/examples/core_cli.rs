//! Small developer CLI example to exercise `nixapps-core` directly from the terminal.
//!
//! Usage inside `nix develop`:
//!   cargo run -p nixapps-core --example core_cli -- sync nixos-26.05
//!   cargo run -p nixapps-core --example core_cli -- search obsidian

use std::env;
use std::path::PathBuf;
use std::time::Instant;

use nixapps_core::{CatalogDb, SearchQuery, sync_channel};

fn default_db_path() -> PathBuf {
    let base = env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".cache")
        });
    base.join("nixapps").join("catalog.db")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let db_path = default_db_path();
    let mut db = CatalogDb::open(&db_path)?;

    match args.first().map(String::as_str) {
        Some("sync") => {
            let channel = args.get(1).map(String::as_str).unwrap_or("nixos-26.05");
            println!("Syncing channel '{channel}' into {} ...", db_path.display());
            let started = Instant::now();
            let info = sync_channel(&mut db, channel, |count| {
                if count.is_multiple_of(10_000) {
                    eprintln!("  indexed {count} packages...");
                }
            })?;
            println!(
                "Done! Indexed {} packages from '{}' in {:.2?}.",
                info.package_count,
                info.channel,
                started.elapsed()
            );
        }
        Some("search") => {
            let query_text = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            let started = Instant::now();
            let mut q = SearchQuery::new(query_text);
            q.limit = 15;
            let results = db.search(&q)?;
            println!(
                "Found {} results in {:.2?}:",
                results.len(),
                started.elapsed()
            );
            for pkg in results {
                let unfree_tag = if pkg.unfree { " [unfree]" } else { "" };
                println!(
                    "  - {} ({}) [{}]{}: {}",
                    pkg.attr_path,
                    pkg.version,
                    pkg.category.display_name(),
                    unfree_tag,
                    pkg.description.as_deref().unwrap_or("No description")
                );
            }
        }
        Some("stats") => {
            if let Some(info) = db.sync_info()? {
                println!(
                    "Channel: {} | Packages: {} | Synced at epoch: {}",
                    info.channel, info.package_count, info.synced_at_epoch
                );
                for (cat, count) in db.category_counts(false)? {
                    println!("  {:24} {}", cat.display_name(), count);
                }
            } else {
                println!("Catalog is empty. Run `sync <channel>` first.");
            }
        }
        _ => {
            eprintln!("Usage:");
            eprintln!("  core_cli sync [channel]   # e.g. nixos-26.05 or nixos-unstable");
            eprintln!("  core_cli search <query>");
            eprintln!("  core_cli stats");
        }
    }

    Ok(())
}
