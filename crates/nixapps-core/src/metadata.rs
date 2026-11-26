use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::db::CatalogDb;
use crate::error::Result;
use crate::models::{CachedMetadata, Category, PackageRecord};

/// Default cache TTL for external metadata lookups (7 days).
pub const DEFAULT_CACHE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

const FLATHUB_SEARCH_URL: &str = "https://flathub.org/api/v2/search";
const FLATHUB_APPSTREAM_URL: &str = "https://flathub.org/api/v2/appstream";
const GITHUB_API_BASE: &str = "https://api.github.com";

fn shared_http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(concat!("NixApps/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(8)
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

/// Rich metadata returned to the UI for a package (icon file on disk, briefing, stars, category hint).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnrichedPackageInfo {
    pub attr_path: String,
    pub source: String,
    pub icon_path: Option<PathBuf>,
    pub icon_url: Option<String>,
    pub briefing: Option<String>,
    pub stars: Option<i64>,
    pub category_hint: Option<Category>,
}

/// Curated desktop applications featured on the Explore home screen when present in the catalog.
pub const FEATURED_ATTRS: &[&str] = &[
    "firefox",
    "obsidian",
    "spotify",
    "vscode",
    "blender",
    "gimp",
    "obs-studio",
    "vlc",
    "lutris",
    "steam",
    "heroic",
    "prismlauncher",
    "qbittorrent",
    "inkscape",
    "krita",
    "audacity",
    "libreoffice",
    "telegram-desktop",
    "discord",
    "helix",
    "ghostty",
    "btop",
];

/// Well-known Nixpkgs `attr_path` -> Flathub `app_id` mappings for popular desktop apps
/// whose package name differs from the Flathub display name.
pub fn known_flathub_id(attr_path: &str) -> Option<&'static str> {
    match attr_path {
        "obsidian" => Some("md.obsidian.Obsidian"),
        "spotify" => Some("com.spotify.Client"),
        "firefox" | "firefox-bin" => Some("org.mozilla.firefox"),
        "thunderbird" => Some("org.mozilla.Thunderbird"),
        "vscode" | "vscodium" => Some("com.visualstudio.code"),
        "discord" | "vesktop" => Some("com.discordapp.Discord"),
        "telegram-desktop" => Some("org.telegram.desktop"),
        "steam" => Some("com.valvesoftware.Steam"),
        "lutris" => Some("net.lutris.Lutris"),
        "heroic" => Some("com.heroicgameslauncher.hgl"),
        "prismlauncher" => Some("org.prismlauncher.PrismLauncher"),
        "obs-studio" => Some("com.obsproject.Studio"),
        "blender" => Some("org.blender.Blender"),
        "gimp" | "gimp-with-plugins" => Some("org.gimp.GIMP"),
        "inkscape" => Some("org.inkscape.Inkscape"),
        "krita" | "kdePackages.krita" => Some("org.kde.krita"),
        "kdenlive" | "kdePackages.kdenlive" => Some("org.kde.kdenlive"),
        "vlc" => Some("org.videolan.VLC"),
        "audacity" => Some("org.audacityteam.Audacity"),
        "qbittorrent" | "qbittorrent-enhanced" => Some("org.qbittorrent.qBittorrent"),
        "libreoffice" | "libreoffice-qt" | "libreoffice-fresh" => {
            Some("org.libreoffice.LibreOffice")
        }
        "bitwarden-desktop" => Some("com.bitwarden.desktop"),
        "protonup-qt" => Some("net.davidotek.pupgui2"),
        "signal-desktop" => Some("org.signal.Signal"),
        "brave" => Some("com.brave.Browser"),
        "chromium" => Some("org.chromium.Chromium"),
        "bottles" => Some("com.usebottles.bottles"),
        "easyeffects" => Some("com.github.wwmm.easyeffects"),
        "mpv" => Some("io.mpv.Mpv"),
        "zed-editor" => Some("dev.zed.Zed"),
        _ => None,
    }
}

/// Extracts `(owner, repo)` from a GitHub URL if present.
pub fn parse_github_repo(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))?;
    let path = without_scheme
        .strip_prefix("github.com/")
        .or_else(|| without_scheme.strip_prefix("www.github.com/"))?;

    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let raw_repo = parts.next()?.trim();
    let repo = raw_repo.strip_suffix(".git").unwrap_or(raw_repo);

    if owner.is_empty()
        || repo.is_empty()
        || matches!(
            owner,
            "orgs" | "topics" | "search" | "marketplace" | "features" | "explore"
        )
    {
        return None;
    }

    Some((owner.to_string(), repo.to_string()))
}

/// Returns the deterministic local icon path inside `icons_dir` for `attr_path`, if it already exists on disk.
pub fn cached_icon_path(icons_dir: &Path, attr_path: &str) -> Option<PathBuf> {
    let safe = sanitize_filename(attr_path);
    for ext in ["png", "svg", "jpg", "webp"] {
        let candidate = icons_dir.join(format!("{safe}.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Enriches a package with Flathub AppStream (icon + briefing + category) and/or GitHub API
/// (stars + cleaned README briefing + avatar fallback), caching results in SQLite and on disk.
pub fn enrich_package(
    db: &CatalogDb,
    icons_dir: &Path,
    pkg: &PackageRecord,
    github_token: Option<&str>,
) -> Result<EnrichedPackageInfo> {
    let now = unix_now();

    // 1. Check fresh SQLite cache first.
    if let Some(cached) = db.get_cached_metadata(&pkg.attr_path, "enriched")?
        && now.saturating_sub(cached.fetched_at_epoch) < DEFAULT_CACHE_TTL_SECS
    {
        let mut icon_path = cached_icon_path(icons_dir, &pkg.attr_path);
        if icon_path.is_none()
            && let Some(url) = cached.icon_url.as_deref()
        {
            icon_path = download_icon_to_cache(icons_dir, &pkg.attr_path, url);
        }

        return Ok(EnrichedPackageInfo {
            attr_path: pkg.attr_path.clone(),
            source: if cached.icon_url.is_some() || cached.briefing.is_some() {
                "cache".to_string()
            } else {
                "none".to_string()
            },
            icon_path,
            icon_url: cached.icon_url,
            briefing: cached.briefing,
            stars: cached.stars,
            category_hint: None,
        });
    }

    let client = shared_http_client();
    let mut icon_url: Option<String> = None;
    let mut briefing: Option<String> = None;
    let mut stars: Option<i64> = None;
    let mut category_hint: Option<Category> = None;
    let mut source = "none".to_string();

    // 2. Try Flathub AppStream / Search API for high-res desktop app icons & summaries.
    if let Some(fh) = fetch_flathub_metadata(client, pkg) {
        icon_url = fh.icon_url;
        briefing = fh.briefing;
        category_hint = fh.category;
        source = "flathub".to_string();
    }

    // 3. Try GitHub REST API when homepage points to GitHub (stars, README briefing, avatar fallback).
    if let Some(hp) = pkg.homepage.as_deref()
        && let Some((owner, repo)) = parse_github_repo(hp)
        && let Some(gh) = fetch_github_metadata(client, &owner, &repo, github_token, true)
    {
        stars = gh.stars;
        if briefing.is_none() {
            briefing = gh.briefing;
        }
        if icon_url.is_none() {
            icon_url = gh.avatar_url;
        }
        source = if source == "flathub" {
            "flathub+github".to_string()
        } else {
            "github".to_string()
        };
    }

    // 4. Download icon into ~/.cache/nixapps/icons/ if available.
    let icon_path = icon_url
        .as_deref()
        .and_then(|url| download_icon_to_cache(icons_dir, &pkg.attr_path, url));

    // 5. Persist result (including negative cache) in SQLite.
    let cache_entry = CachedMetadata {
        attr_path: pkg.attr_path.clone(),
        source: "enriched".to_string(),
        icon_url: icon_url.clone(),
        briefing: briefing.clone(),
        stars,
        fetched_at_epoch: now,
    };
    let _ = db.upsert_cached_metadata(&cache_entry);

    Ok(EnrichedPackageInfo {
        attr_path: pkg.attr_path.clone(),
        source,
        icon_path,
        icon_url,
        briefing,
        stars,
        category_hint,
    })
}

/// Fast icon-only fetch for Explore cards and search rows.
///
/// Reuses the pooled HTTP client and skips heavy GitHub README fetches so card grids
/// populate quickly when combined with concurrent worker threads.
pub fn fetch_icon_cached(
    db: &CatalogDb,
    icons_dir: &Path,
    pkg: &PackageRecord,
    github_token: Option<&str>,
) -> Option<PathBuf> {
    if let Some(existing) = cached_icon_path(icons_dir, &pkg.attr_path) {
        return Some(existing);
    }

    let now = unix_now();
    // Check either full enriched cache or lightweight icon cache to avoid redundant HTTP calls.
    for cache_src in ["enriched", "icon"] {
        if let Ok(Some(cached)) = db.get_cached_metadata(&pkg.attr_path, cache_src)
            && now.saturating_sub(cached.fetched_at_epoch) < DEFAULT_CACHE_TTL_SECS
        {
            return cached
                .icon_url
                .as_deref()
                .and_then(|url| download_icon_to_cache(icons_dir, &pkg.attr_path, url));
        }
    }

    let client = shared_http_client();
    let mut icon_url = fetch_flathub_metadata(client, pkg).and_then(|fh| fh.icon_url);

    // Lightweight GitHub owner avatar fallback (no README download!).
    if icon_url.is_none()
        && let Some(hp) = pkg.homepage.as_deref()
        && let Some((owner, _repo)) = parse_github_repo(hp)
    {
        // Direct GitHub avatar URL avoids even consuming a GitHub REST API quota call!
        let _ = github_token;
        icon_url = Some(format!("https://github.com/{owner}.png?size=128"));
    }

    let icon_path = icon_url
        .as_deref()
        .and_then(|url| download_icon_to_cache(icons_dir, &pkg.attr_path, url));

    let _ = db.upsert_cached_metadata(&CachedMetadata {
        attr_path: pkg.attr_path.clone(),
        source: "icon".to_string(),
        icon_url: if icon_path.is_some() { icon_url } else { None },
        briefing: None,
        stars: None,
        fetched_at_epoch: now,
    });

    icon_path
}

struct FlathubResult {
    icon_url: Option<String>,
    briefing: Option<String>,
    category: Option<Category>,
}

#[derive(Debug, Deserialize)]
struct FlathubAppstreamResp {
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    categories: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct FlathubSearchResp {
    #[serde(default)]
    hits: Vec<FlathubSearchHit>,
}

#[derive(Debug, Deserialize)]
struct FlathubSearchHit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    app_id: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    main_categories: Option<serde_json::Value>,
}

fn fetch_flathub_metadata(
    client: &reqwest::blocking::Client,
    pkg: &PackageRecord,
) -> Option<FlathubResult> {
    // Fast exact lookup via curated Flathub app_id mapping first.
    if let Some(app_id) = known_flathub_id(&pkg.attr_path) {
        let url = format!("{FLATHUB_APPSTREAM_URL}/{app_id}");
        if let Ok(resp) = client.get(&url).send()
            && resp.status().is_success()
            && let Ok(data) = resp.json::<FlathubAppstreamResp>()
        {
            let icon_url = data.icon.filter(|u| !u.trim().is_empty());
            let briefing = data
                .description
                .as_deref()
                .map(clean_flathub_description)
                .filter(|s| !s.is_empty())
                .or(data.summary);
            let category = data
                .categories
                .as_deref()
                .and_then(|cats| cats.iter().find_map(|c| map_freedesktop_category(c)));

            if icon_url.is_some() || briefing.is_some() {
                return Some(FlathubResult {
                    icon_url,
                    briefing,
                    category,
                });
            }
        }
    }

    // Avoid wasting Flathub searches on language-specific libraries (python313Packages.*, etc.).
    if pkg.attr_path.contains("Packages.") || pkg.attr_path.starts_with("lib") {
        return None;
    }

    let body = serde_json::json!({
        "query": pkg.pname,
        "hits_per_page": 5
    });

    let resp = client.post(FLATHUB_SEARCH_URL).json(&body).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let parsed: FlathubSearchResp = resp.json().ok()?;

    let target_norm = normalize_name(&pkg.pname);
    let best = parsed.hits.into_iter().find(|hit| {
        let name_norm = normalize_name(&hit.name);
        let id_tail = hit
            .app_id
            .rsplit('.')
            .next()
            .map(normalize_name)
            .unwrap_or_default();
        name_norm == target_norm || id_tail == target_norm
    })?;

    let category = best.main_categories.as_ref().and_then(|val| match val {
        serde_json::Value::String(s) => map_freedesktop_category(s),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .find_map(map_freedesktop_category),
        _ => None,
    });

    let briefing = best
        .description
        .as_deref()
        .map(clean_flathub_description)
        .filter(|s| !s.is_empty())
        .or(best.summary);

    Some(FlathubResult {
        icon_url: best.icon.filter(|u| !u.trim().is_empty()),
        briefing,
        category,
    })
}

struct GitHubResult {
    stars: Option<i64>,
    avatar_url: Option<String>,
    briefing: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRepoResp {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    stargazers_count: Option<i64>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    owner: Option<GitHubOwner>,
}

#[derive(Debug, Deserialize)]
struct GitHubOwner {
    #[serde(default)]
    avatar_url: Option<String>,
}

fn fetch_github_metadata(
    client: &reqwest::blocking::Client,
    owner: &str,
    repo: &str,
    github_token: Option<&str>,
    fetch_readme: bool,
) -> Option<GitHubResult> {
    let repo_url = format!("{GITHUB_API_BASE}/repos/{owner}/{repo}");
    let mut req = client
        .get(&repo_url)
        .header("Accept", "application/vnd.github+json");
    if let Some(tok) = github_token.filter(|t| !t.trim().is_empty()) {
        req = req.header("Authorization", format!("Bearer {}", tok.trim()));
    }

    let resp = req.send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let repo_data: GitHubRepoResp = resp.json().ok()?;

    let avatar_url = repo_data.owner.and_then(|o| o.avatar_url).map(|u| {
        if u.contains('?') {
            format!("{u}&s=128")
        } else {
            format!("{u}?s=128")
        }
    });

    // Fetch raw README for a richer briefing (best-effort, only when requested).
    let readme_excerpt = if fetch_readme {
        let readme_url = format!("{GITHUB_API_BASE}/repos/{owner}/{repo}/readme");
        let mut readme_req = client
            .get(&readme_url)
            .header("Accept", "application/vnd.github.raw+json");
        if let Some(tok) = github_token.filter(|t| !t.trim().is_empty()) {
            readme_req = readme_req.header("Authorization", format!("Bearer {}", tok.trim()));
        }

        readme_req
            .send()
            .ok()
            .filter(|r| r.status().is_success())
            .and_then(|r| r.text().ok())
            .map(|raw| extract_clean_readme_briefing(&raw))
            .filter(|s| !s.is_empty())
    } else {
        None
    };

    let mut briefing_parts = Vec::new();
    if let Some(desc) = repo_data.description.filter(|d| !d.trim().is_empty()) {
        briefing_parts.push(desc);
    }
    if let Some(lang) = repo_data.language.filter(|l| !l.trim().is_empty()) {
        briefing_parts.push(format!("Primary language: {lang}"));
    }
    if let Some(excerpt) = readme_excerpt {
        briefing_parts.push(excerpt);
    }

    let briefing = (!briefing_parts.is_empty()).then(|| briefing_parts.join("\n\n"));

    Some(GitHubResult {
        stars: repo_data.stargazers_count,
        avatar_url,
        briefing,
    })
}

/// Downloads an icon URL to `~/.cache/nixapps/icons/<safe_attr>.<ext>` (capped at 2 MB).
fn download_icon_to_cache(icons_dir: &Path, attr_path: &str, url: &str) -> Option<PathBuf> {
    fs::create_dir_all(icons_dir).ok()?;

    let client = shared_http_client();
    let resp = client.get(url).send().ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let bytes = resp.bytes().ok()?;
    if bytes.is_empty() || bytes.len() > 2 * 1024 * 1024 {
        return None;
    }

    let ext = if content_type.contains("svg") || url.ends_with(".svg") {
        "svg"
    } else if content_type.contains("jpeg") || url.ends_with(".jpg") || url.ends_with(".jpeg") {
        "jpg"
    } else if content_type.contains("webp") || url.ends_with(".webp") {
        "webp"
    } else {
        "png"
    };

    let target = icons_dir.join(format!("{}.{ext}", sanitize_filename(attr_path)));
    fs::write(&target, &bytes).ok()?;
    Some(target)
}

/// Strips HTML tags and Flatpak-specific sandbox disclaimers from Flathub AppStream text.
pub fn clean_flathub_description(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut in_tag = false;
    let mut tag_buf = String::new();

    for ch in raw.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag_buf.trim().to_lowercase();
                if t == "/p" || t == "br" || t == "br/" || t == "/li" {
                    text.push('\n');
                } else if t == "li" {
                    text.push_str("• ");
                }
            }
            _ if in_tag => tag_buf.push(ch),
            _ => text.push(ch),
        }
    }

    let decoded = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    let paragraphs: Vec<String> = decoded
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let low = l.to_lowercase();
            // Skip Flatpak-specific sandbox notes that don't apply on NixOS.
            !low.contains("flatseal")
                && !low.contains("this flatpak")
                && !low.contains("handled by flatpak")
                && !low.contains("org.freedesktop.sdk.extension")
        })
        .take(4)
        .collect();

    paragraphs.join("\n\n")
}

/// Extracts a concise, readable 2–3 paragraph briefing from a raw Markdown README.
pub fn extract_clean_readme_briefing(raw_md: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut in_code_fence = false;

    for line in raw_md.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }

        if trimmed.is_empty() {
            if !current.is_empty() {
                let para = clean_markdown_inline(&current.join(" "));
                if para.len() >= 30 {
                    paragraphs.push(para);
                    if paragraphs.len() >= 3 {
                        break;
                    }
                }
                current.clear();
            }
            continue;
        }

        // Skip headings, badges, pure HTML blocks, images, horizontal rules, and tables.
        if trimmed.starts_with('#')
            || trimmed.starts_with("![")
            || trimmed.starts_with("[![")
            || trimmed.starts_with('<')
            || trimmed.starts_with("---")
            || trimmed.starts_with("===")
            || trimmed.starts_with('|')
        {
            continue;
        }

        current.push(trimmed.to_string());
    }

    if paragraphs.len() < 3 && !current.is_empty() {
        let para = clean_markdown_inline(&current.join(" "));
        if para.len() >= 30 {
            paragraphs.push(para);
        }
    }

    let joined = paragraphs.join("\n\n");
    if joined.len() > 900 {
        let truncated: String = joined.chars().take(900).collect();
        format!("{truncated}…")
    } else {
        joined
    }
}

/// Converts simple Markdown links `[label](url)` -> `label` and strips emphasis markers.
fn clean_markdown_inline(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '['
            && let Some(close_bracket) = chars[i + 1..].iter().position(|&c| c == ']')
        {
            let close_b = i + 1 + close_bracket;
            if close_b + 1 < chars.len()
                && chars[close_b + 1] == '('
                && let Some(close_paren) = chars[close_b + 2..].iter().position(|&c| c == ')')
            {
                let label: String = chars[i + 1..close_b].iter().collect();
                out.push_str(&label);
                i = close_b + 2 + close_paren + 1;
                continue;
            }
        }
        if !matches!(chars[i], '*' | '`') {
            out.push(chars[i]);
        }
        i += 1;
    }

    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn map_freedesktop_category(cat: &str) -> Option<Category> {
    match cat.to_lowercase().as_str() {
        "audiovideo" | "audio" | "video" => Some(Category::AudioVideo),
        "development" => Some(Category::Development),
        "education" => Some(Category::Education),
        "game" | "games" => Some(Category::Games),
        "graphics" => Some(Category::Graphics),
        "network" => Some(Category::Network),
        "office" => Some(Category::Office),
        "science" => Some(Category::Science),
        "system" => Some(Category::System),
        "utility" | "utilities" => Some(Category::Utilities),
        _ => None,
    }
}

fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn sanitize_filename(attr_path: &str) -> String {
    attr_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_urls_cleanly() {
        assert_eq!(
            parse_github_repo("https://github.com/BurntSushi/ripgrep"),
            Some(("BurntSushi".into(), "ripgrep".into()))
        );
        assert_eq!(
            parse_github_repo("https://github.com/helix-editor/helix.git/"),
            Some(("helix-editor".into(), "helix".into()))
        );
        assert_eq!(parse_github_repo("https://obsidian.md"), None);
        assert_eq!(parse_github_repo("https://github.com/topics/rust"), None);
    }

    #[test]
    fn cleans_flathub_html_and_skips_flatpak_sandbox_notes() {
        let html = "<p>Obsidian is a <b>Markdown</b> knowledge base.</p><p>GPU acceleration can be disabled via Flatseal.</p><p>Supports plugins and local vaults.</p>";
        let cleaned = clean_flathub_description(html);
        assert!(cleaned.contains("Obsidian is a Markdown knowledge base."));
        assert!(cleaned.contains("Supports plugins and local vaults."));
        assert!(!cleaned.contains("Flatseal"));
    }

    #[test]
    fn extracts_readable_briefing_from_markdown_readme() {
        let md = r#"
# MyTool

[![CI](https://img.shields.io/badge/ci-passing-green)](https://example.com)

<p align="center"><img src="logo.png" /></p>

**MyTool** is a fast declarative [NixOS](https://nixos.org) utility built in `Rust` for managing desktop software safely.

```bash
cargo run
```

It indexes official Nix channels locally into SQLite for instant search.
"#;
        let briefing = extract_clean_readme_briefing(md);
        assert!(briefing.contains("MyTool is a fast declarative NixOS utility built in Rust"));
        assert!(briefing.contains("indexes official Nix channels locally"));
        assert!(!briefing.contains("cargo run"));
        assert!(!briefing.contains("shields.io"));
    }
}
