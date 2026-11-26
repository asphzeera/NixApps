use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nixapps_core::{
    CatalogDb, Category, ConfigTarget, ElevationMethod, EnrichedPackageInfo, FEATURED_ATTRS,
    InstallMode, ManagedApp, ManagedState, PackageDependencies, PackageRecord, RebuildOutcome,
    SearchQuery, SyncInfo, WriteOptions, cached_icon_path, diff_strings, enrich_package,
    fetch_icon_cached, inspect_dependencies, onboarding_import_snippet, render_nix_module,
    run_rebuild_streaming, suggest_rebuild_command, sync_channel,
};
use relm4::adw;
use relm4::adw::prelude::*;
use relm4::gtk;
use relm4::prelude::*;

use crate::settings::{AppSettings, catalog_db_path, icons_cache_dir};
use crate::ui_helpers::{
    apply_app_icon, category_icon_name, format_count, format_stars, icon_button,
    install_mode_label, nixapps_logo_texture, nixos_snowflake_texture, set_icon_button,
};

const ICON_WORKER_THREADS: usize = 6;

/// Top-level Relm4 application state.
pub struct NixAppsModel {
    settings: AppSettings,
    managed_state: ManagedState,
    sync_info: Option<SyncInfo>,
    is_syncing: bool,
    sync_progress: usize,

    // Explore & Browse state
    search_text: String,
    selected_category: Option<Category>,
    search_results: Vec<PackageRecord>,
    featured_apps: Vec<PackageRecord>,
    category_counts: Vec<(Category, usize)>,
    icon_cache: HashMap<String, PathBuf>,
    icon_fetch_gen: Arc<AtomicU64>,

    // Detail page state
    selected_package: Option<PackageRecord>,
    detail_enriched: Option<EnrichedPackageInfo>,
    detail_loading_meta: bool,
    detail_deps: Option<PackageDependencies>,
    detail_loading_deps: bool,
    detail_deps_error: Option<String>,
    detail_error: Option<String>,

    // Diff & Rebuild workflow state
    baseline_nix: String,
    pending_changes: usize,
    last_git_warning: Option<String>,
    is_rebuilding: bool,
    rebuild_status: String,
    toast_generation: u64,
}

#[derive(Debug)]
pub enum AppMsg {
    // Navigation & Search
    SearchChanged(String),
    CategorySelected(Option<Category>),
    ToggleUnfree(bool),
    OpenDetail(Box<PackageRecord>),
    OpenDetailByAttr(String),
    CloseDetail,
    SwitchTab(&'static str),
    BannerActionClicked,
    OpenAboutDialog,
    DismissTopToast(u64),

    // Background Metadata & Icons
    IconResolved {
        attr_path: String,
        icon_path: PathBuf,
    },
    DetailMetadataLoaded {
        attr_path: String,
        info: EnrichedPackageInfo,
    },
    InspectDependencies(String),
    DependenciesLoaded {
        attr_path: String,
        result: Result<PackageDependencies, String>,
    },

    // Package Actions
    QuickInstall(String),
    QuickUninstall(String),
    ToggleInstall(String),
    SaveDetailConfig {
        attr_path: String,
        mode: InstallMode,
        custom_nix: String,
    },
    InsertNixTemplate(&'static str),
    OpenHomepage(String),

    // Diff Preview & System Rebuild
    OpenDiffDialog,
    OpenRebuildDialog,
    RequestRebuild,
    StartRebuildWithPassword(Option<String>),
    RebuildLogLine(String),
    RebuildFinished(Result<RebuildOutcome, String>),

    // Onboarding & Settings
    PickConfigFolder,
    ConfigFolderPicked(PathBuf),
    SetTarget(ConfigTarget),
    SetChannel(String),
    SetGithubToken(String),
    SetRebuildCommand(String),
    SetRebuildElevation(ElevationMethod),
    AutoDetectRebuildCommand,
    InitializeManagedConfig,
    CopyImportSnippet,

    // Catalog Sync
    StartSync,
    SyncProgress(usize),
    SyncFinished(Result<SyncInfo, String>),
}

pub struct AppWidgets {
    window: adw::ApplicationWindow,
    back_button: gtk::Button,
    sync_button: gtk::Button,
    status_banner: adw::Banner,
    top_toast_revealer: gtk::Revealer,
    top_toast_label: gtk::Label,
    pending_pill_revealer: gtk::Revealer,
    pending_pill_label: gtk::Label,
    root_stack: gtk::Stack,
    view_stack: adw::ViewStack,

    // Explore tab widgets
    search_entry: gtk::SearchEntry,
    unfree_switch: gtk::Switch,
    category_dropdown: gtk::DropDown,
    results_summary_label: gtk::Label,
    home_scroll: gtk::ScrolledWindow,
    categories_flow: gtk::FlowBox,
    featured_flow: gtk::FlowBox,
    results_list: gtk::ListBox,
    explore_empty_status: adw::StatusPage,
    explore_results_scroll: gtk::ScrolledWindow,

    // Detail view widgets
    detail_icon: gtk::Image,
    detail_title: gtk::Label,
    detail_subtitle: gtk::Label,
    detail_badges_box: gtk::Box,
    detail_primary_btn: gtk::Button,
    detail_homepage_btn: gtk::Button,
    detail_briefing_status: gtk::Label,
    detail_description: gtk::Label,
    detail_meta_group: adw::PreferencesGroup,
    detail_deps_btn: gtk::Button,
    detail_deps_status: gtk::Label,
    detail_deps_box: gtk::Box,
    detail_mode_dropdown: gtk::DropDown,
    detail_nix_buffer: gtk::TextBuffer,
    detail_error_label: gtk::Label,

    // Installed tab widgets
    installed_summary_label: gtk::Label,
    installed_list: gtk::ListBox,
    installed_empty_status: adw::StatusPage,
    installed_content_scroll: gtk::ScrolledWindow,
    installed_preview_buffer: gtk::TextBuffer,

    // Setup / Onboarding tab widgets
    setup_folder_row: adw::ActionRow,
    setup_target_row: adw::ComboRow,
    setup_channel_entry: adw::EntryRow,
    setup_github_token_entry: adw::PasswordEntryRow,
    setup_rebuild_cmd_entry: adw::EntryRow,
    setup_rebuild_elevation_row: adw::ComboRow,
    setup_import_label: gtk::Label,
    setup_sync_status_row: adw::ActionRow,

    // Rebuild modal widgets
    rebuild_dialog: adw::Dialog,
    rebuild_cmd_label: gtk::Label,
    rebuild_status_label: gtk::Label,
    rebuild_run_btn: gtk::Button,
    rebuild_log_buffer: gtk::TextBuffer,
    rebuild_log_view: gtk::TextView,
}

fn elevation_to_index(method: ElevationMethod) -> u32 {
    match method {
        ElevationMethod::BuiltInSudo => 0,
        ElevationMethod::Polkit => 1,
        ElevationMethod::None => 2,
    }
}

fn index_to_elevation(idx: u32) -> ElevationMethod {
    match idx {
        1 => ElevationMethod::Polkit,
        2 => ElevationMethod::None,
        _ => ElevationMethod::BuiltInSudo,
    }
}

impl NixAppsModel {
    fn open_db(&self) -> Option<CatalogDb> {
        CatalogDb::open(&catalog_db_path()).ok()
    }

    fn reload_managed_state(&mut self) {
        if let Some(cfg) = self.settings.managed_config() {
            if let Ok(state) = cfg.load_or_init(self.settings.target, &self.settings.channel) {
                self.managed_state = state;
                self.managed_state.target = self.settings.target;
                self.managed_state.channel = self.settings.channel.clone();
            }
            self.baseline_nix = fs::read_to_string(&cfg.module_path)
                .unwrap_or_else(|_| render_nix_module(&self.managed_state).unwrap_or_default());
        } else {
            self.managed_state = ManagedState::new(self.settings.target, &self.settings.channel);
            self.baseline_nix.clear();
        }
    }

    fn reload_home_data(&mut self) {
        let Some(db) = self.open_db() else {
            self.featured_apps.clear();
            self.category_counts.clear();
            return;
        };

        self.featured_apps = db.get_packages_by_attrs(FEATURED_ATTRS).unwrap_or_default();
        self.category_counts = db.category_counts(false).unwrap_or_default();

        let icons_dir = icons_cache_dir();
        for pkg in &self.featured_apps {
            if let Some(path) = cached_icon_path(&icons_dir, &pkg.attr_path) {
                self.icon_cache.insert(pkg.attr_path.clone(), path);
            }
        }
    }

    fn is_home_mode(&self) -> bool {
        self.sync_info.is_some()
            && self.search_text.trim().is_empty()
            && self.selected_category.is_none()
    }

    fn run_search(&mut self) {
        let Some(db) = self.open_db() else {
            self.search_results.clear();
            return;
        };

        let mut query = SearchQuery::new(self.search_text.clone());
        query.category = self.selected_category;
        query.include_unfree = self.settings.include_unfree;
        query.limit = 60;

        self.search_results = db.search(&query).unwrap_or_default();

        let icons_dir = icons_cache_dir();
        for pkg in &self.search_results {
            if !self.icon_cache.contains_key(&pkg.attr_path)
                && let Some(path) = cached_icon_path(&icons_dir, &pkg.attr_path)
            {
                self.icon_cache.insert(pkg.attr_path.clone(), path);
            }
        }
    }

    fn spawn_icon_batch(&self, packages: Vec<PackageRecord>, sender: &ComponentSender<Self>) {
        let pending: VecDeque<PackageRecord> = packages
            .into_iter()
            .filter(|p| !self.icon_cache.contains_key(&p.attr_path))
            .take(22)
            .collect();

        if pending.is_empty() {
            return;
        }

        let worker_count = pending.len().min(ICON_WORKER_THREADS);
        let queue = Arc::new(Mutex::new(pending));
        let next_gen = self.icon_fetch_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let db_path = catalog_db_path();
        let icons_dir = icons_cache_dir();
        let token = self.settings.github_token.clone();

        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let gen_ref = Arc::clone(&self.icon_fetch_gen);
            let input_sender = sender.input_sender().clone();
            let db_path = db_path.clone();
            let icons_dir = icons_dir.clone();
            let token = token.clone();

            thread::spawn(move || {
                let Ok(db) = CatalogDb::open(&db_path) else {
                    return;
                };
                loop {
                    if gen_ref.load(Ordering::SeqCst) != next_gen {
                        break;
                    }
                    let next_pkg = {
                        let Ok(mut guard) = queue.lock() else {
                            break;
                        };
                        guard.pop_front()
                    };
                    let Some(pkg) = next_pkg else {
                        break;
                    };
                    if let Some(icon_path) =
                        fetch_icon_cached(&db, &icons_dir, &pkg, token.as_deref())
                    {
                        let _ = input_sender.send(AppMsg::IconResolved {
                            attr_path: pkg.attr_path,
                            icon_path,
                        });
                    }
                }
            });
        }
    }

    fn spawn_detail_enrichment(&self, pkg: PackageRecord, sender: &ComponentSender<Self>) {
        let input_sender = sender.input_sender().clone();
        let db_path = catalog_db_path();
        let icons_dir = icons_cache_dir();
        let token = self.settings.github_token.clone();
        let attr_path = pkg.attr_path.clone();

        thread::spawn(move || {
            let Ok(db) = CatalogDb::open(&db_path) else {
                return;
            };
            if let Ok(info) = enrich_package(&db, &icons_dir, &pkg, token.as_deref()) {
                let _ = input_sender.send(AppMsg::DetailMetadataLoaded { attr_path, info });
            }
        });
    }

    fn current_session_diff(&self) -> String {
        let current = render_nix_module(&self.managed_state).unwrap_or_default();
        let label = self
            .settings
            .managed_config()
            .and_then(|c| {
                c.module_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "nixapps-managed.nix".to_string());

        let diff = diff_strings(&self.baseline_nix, &current, &label);
        if diff.trim().is_empty() {
            "# No pending changes since the last rebuild.\n".to_string()
        } else {
            diff
        }
    }

    fn apply_state_to_disk(&mut self) -> Result<String, String> {
        let Some(cfg) = self.settings.managed_config() else {
            return Err("Configure your Nix dotfiles folder in Setup first.".to_string());
        };

        self.managed_state.target = self.settings.target;
        self.managed_state.channel = self.settings.channel.clone();

        let report = cfg
            .apply(&self.managed_state, &WriteOptions::default())
            .map_err(|e| e.to_string())?;

        self.pending_changes += 1;
        self.last_git_warning = report.git_warning.clone();

        if report.git_warning.is_some() {
            Ok(format!(
                "Saved {} (check `.git` permissions for Flakes)",
                cfg.relative_import_path()
            ))
        } else if report.git_staged_new_files {
            Ok(format!(
                "Updated {} (staged with `git add -N` for Flakes)",
                cfg.relative_import_path()
            ))
        } else {
            Ok(format!("Updated {}", cfg.relative_import_path()))
        }
    }
}

impl Component for NixAppsModel {
    type Init = ();
    type Input = AppMsg;
    type Output = ();
    type CommandOutput = ();
    type Root = adw::ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        adw::ApplicationWindow::builder()
            .title("NixApps")
            .default_width(1040)
            .default_height(740)
            .build()
    }

    fn init(
        _init: Self::Init,
        window: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let settings = AppSettings::load();
        let sync_info = CatalogDb::open(&catalog_db_path())
            .ok()
            .and_then(|db| db.sync_info().ok().flatten());

        let mut model = NixAppsModel {
            managed_state: ManagedState::new(settings.target, &settings.channel),
            settings,
            sync_info,
            is_syncing: false,
            sync_progress: 0,
            search_text: String::new(),
            selected_category: None,
            search_results: Vec::new(),
            featured_apps: Vec::new(),
            category_counts: Vec::new(),
            icon_cache: HashMap::new(),
            icon_fetch_gen: Arc::new(AtomicU64::new(0)),
            selected_package: None,
            detail_enriched: None,
            detail_loading_meta: false,
            detail_deps: None,
            detail_loading_deps: false,
            detail_deps_error: None,
            detail_error: None,
            baseline_nix: String::new(),
            pending_changes: 0,
            last_git_warning: None,
            is_rebuilding: false,
            rebuild_status: "Ready to run system rebuild.".to_string(),
            toast_generation: 0,
        };

        model.reload_managed_state();
        model.reload_home_data();
        model.run_search();

        if !model.featured_apps.is_empty() {
            model.spawn_icon_batch(model.featured_apps.clone(), &sender);
        }

        // --- HeaderBar & ViewSwitcher ---
        let view_stack = adw::ViewStack::new();
        let view_switcher = adw::ViewSwitcher::builder()
            .stack(&view_stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build();

        let back_button = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Back to Store")
            .visible(false)
            .build();
        {
            let s = sender.clone();
            back_button.connect_clicked(move |_| s.input(AppMsg::CloseDetail));
        }

        let about_button = gtk::Button::builder()
            .icon_name("help-about-symbolic")
            .tooltip_text("About NixApps")
            .build();
        {
            let s = sender.clone();
            about_button.connect_clicked(move |_| s.input(AppMsg::OpenAboutDialog));
        }

        let sync_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text("Sync Nixpkgs catalog")
            .build();
        {
            let s = sender.clone();
            sync_button.connect_clicked(move |_| s.input(AppMsg::StartSync));
        }

        let header = adw::HeaderBar::new();
        header.pack_start(&back_button);
        header.set_title_widget(Some(&view_switcher));
        header.pack_end(&about_button);
        header.pack_end(&sync_button);

        // --- Status Banner ---
        let status_banner = adw::Banner::new("");
        {
            let s = sender.clone();
            status_banner.connect_button_clicked(move |_| {
                s.input(AppMsg::BannerActionClicked);
            });
        }

        // --- TAB 1: EXPLORE / STORE HOME + SEARCH ---
        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Search 140k+ Nix packages (e.g. obsidian, spotify, helix, lutris)…")
            .hexpand(true)
            .build();
        {
            let s = sender.clone();
            search_entry.connect_search_changed(move |entry| {
                s.input(AppMsg::SearchChanged(entry.text().to_string()));
            });
        }

        let mut category_labels: Vec<&str> = vec!["All Categories"];
        category_labels.extend(Category::ALL.iter().map(|c| c.display_name()));
        let category_model = gtk::StringList::new(&category_labels);
        let category_dropdown = gtk::DropDown::builder()
            .model(&category_model)
            .selected(0)
            .tooltip_text("Filter by category")
            .build();
        {
            let s = sender.clone();
            category_dropdown.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                let cat = if idx == 0 {
                    None
                } else {
                    Category::ALL.get(idx - 1).copied()
                };
                s.input(AppMsg::CategorySelected(cat));
            });
        }

        let unfree_label = gtk::Label::builder()
            .label("Unfree")
            .css_classes(["dim-label"])
            .build();
        let unfree_switch = gtk::Switch::builder()
            .active(model.settings.include_unfree)
            .valign(gtk::Align::Center)
            .tooltip_text("Show unfree packages (Spotify, Obsidian, Steam, etc.)")
            .build();
        {
            let s = sender.clone();
            unfree_switch.connect_active_notify(move |sw| {
                s.input(AppMsg::ToggleUnfree(sw.is_active()));
            });
        }

        let controls_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .margin_top(14)
            .margin_bottom(6)
            .build();
        controls_row.append(&search_entry);
        controls_row.append(&category_dropdown);
        controls_row.append(&unfree_label);
        controls_row.append(&unfree_switch);

        let results_summary_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "caption"])
            .margin_bottom(8)
            .build();

        let cat_heading = gtk::Label::builder()
            .label("Browse Categories")
            .css_classes(["title-3"])
            .halign(gtk::Align::Start)
            .margin_top(6)
            .margin_bottom(8)
            .build();

        let categories_flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .row_spacing(10)
            .column_spacing(10)
            .min_children_per_line(2)
            .max_children_per_line(4)
            .build();

        let featured_heading = gtk::Label::builder()
            .label("Featured Desktop Apps")
            .css_classes(["title-3"])
            .halign(gtk::Align::Start)
            .margin_top(22)
            .margin_bottom(8)
            .build();

        let featured_flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .row_spacing(12)
            .column_spacing(12)
            .min_children_per_line(2)
            .max_children_per_line(3)
            .build();

        let home_vbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_bottom(28)
            .build();
        home_vbox.append(&cat_heading);
        home_vbox.append(&categories_flow);
        home_vbox.append(&featured_heading);
        home_vbox.append(&featured_flow);

        let home_clamp = adw::Clamp::builder()
            .maximum_size(940)
            .margin_start(16)
            .margin_end(16)
            .child(&home_vbox)
            .build();

        let home_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&home_clamp)
            .build();

        let results_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();

        let results_clamp = adw::Clamp::builder()
            .maximum_size(940)
            .margin_start(16)
            .margin_end(16)
            .margin_bottom(24)
            .child(&results_list)
            .build();

        let explore_results_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&results_clamp)
            .build();

        let explore_empty_btn = icon_button(
            "view-refresh-symbolic",
            "Sync Nixpkgs Catalog",
            &["suggested-action", "pill"],
        );
        explore_empty_btn.set_halign(gtk::Align::Center);
        {
            let s = sender.clone();
            explore_empty_btn.connect_clicked(move |_| s.input(AppMsg::StartSync));
        }

        let explore_empty_status = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("No Packages Found")
            .description("Sync a NixOS channel or adjust your search query.")
            .child(&explore_empty_btn)
            .vexpand(true)
            .build();

        let controls_clamp = adw::Clamp::builder()
            .maximum_size(940)
            .margin_start(16)
            .margin_end(16)
            .build();
        let controls_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        controls_vbox.append(&controls_row);
        controls_vbox.append(&results_summary_label);
        controls_clamp.set_child(Some(&controls_vbox));

        let explore_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        explore_page.append(&controls_clamp);
        explore_page.append(&home_scroll);
        explore_page.append(&explore_results_scroll);
        explore_page.append(&explore_empty_status);

        view_stack.add_titled_with_icon(
            &explore_page,
            Some("explore"),
            "Explore",
            "system-search-symbolic",
        );

        // --- TAB 2: INSTALLED APPS + DIFF & REBUILD ---
        let installed_summary_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["title-4"])
            .hexpand(true)
            .build();

        let review_diff_btn = icon_button("document-edit-symbolic", "Review Diff", &[]);
        {
            let s = sender.clone();
            review_diff_btn.connect_clicked(move |_| s.input(AppMsg::OpenDiffDialog));
        }

        let apply_rebuild_btn = icon_button(
            "emblem-synchronizing-symbolic",
            "Apply & Rebuild",
            &["suggested-action"],
        );
        {
            let s = sender.clone();
            apply_rebuild_btn.connect_clicked(move |_| s.input(AppMsg::OpenRebuildDialog));
        }

        let installed_header_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(16)
            .margin_bottom(8)
            .build();
        installed_header_row.append(&installed_summary_label);
        installed_header_row.append(&review_diff_btn);
        installed_header_row.append(&apply_rebuild_btn);

        let installed_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();

        let installed_preview_buffer = gtk::TextBuffer::new(None);
        let installed_preview_view = gtk::TextView::builder()
            .buffer(&installed_preview_buffer)
            .editable(false)
            .monospace(true)
            .css_classes(["nix-code-view"])
            .top_margin(10)
            .bottom_margin(10)
            .left_margin(12)
            .right_margin(12)
            .build();

        let preview_frame = gtk::Frame::builder()
            .child(&installed_preview_view)
            .margin_top(8)
            .build();

        let preview_header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(20)
            .build();
        let preview_title = gtk::Label::builder()
            .label("Generated Nix Module Preview")
            .css_classes(["title-4"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        let copy_import_btn_2 = icon_button("edit-copy-symbolic", "Copy Import Line", &[]);
        {
            let s = sender.clone();
            copy_import_btn_2.connect_clicked(move |_| s.input(AppMsg::CopyImportSnippet));
        }
        preview_header.append(&preview_title);
        preview_header.append(&copy_import_btn_2);

        let installed_vbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_bottom(24)
            .build();
        installed_vbox.append(&installed_header_row);
        installed_vbox.append(&installed_list);
        installed_vbox.append(&preview_header);
        installed_vbox.append(&preview_frame);

        let installed_clamp = adw::Clamp::builder()
            .maximum_size(900)
            .margin_start(16)
            .margin_end(16)
            .child(&installed_vbox)
            .build();

        let installed_content_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&installed_clamp)
            .build();

        let browse_btn = icon_button(
            "system-search-symbolic",
            "Browse Packages",
            &["suggested-action", "pill"],
        );
        browse_btn.set_halign(gtk::Align::Center);
        {
            let s = sender.clone();
            browse_btn.connect_clicked(move |_| s.input(AppMsg::SwitchTab("explore")));
        }

        let installed_empty_status = adw::StatusPage::builder()
            .icon_name("package-x-generic-symbolic")
            .title("No Managed Apps Yet")
            .description(
                "Applications installed through NixApps appear here and are written cleanly to your managed .nix module.",
            )
            .child(&browse_btn)
            .vexpand(true)
            .build();

        let installed_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        installed_page.append(&installed_content_scroll);
        installed_page.append(&installed_empty_status);

        view_stack.add_titled_with_icon(
            &installed_page,
            Some("installed"),
            "Installed",
            "folder-download-symbolic",
        );

        // --- TAB 3: ONBOARDING & SETUP (clean responsive header with large NixOS snowflake) ---
        let setup_hero = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .halign(gtk::Align::Center)
            .margin_top(20)
            .margin_bottom(18)
            .build();

        if let Some(tex) = nixos_snowflake_texture() {
            let setup_logo = gtk::Picture::builder()
                .paintable(&tex)
                .content_fit(gtk::ContentFit::Contain)
                .width_request(192)
                .height_request(192)
                .halign(gtk::Align::Center)
                .build();
            setup_hero.append(&setup_logo);
        } else {
            let setup_logo = gtk::Image::builder()
                .icon_name("preferences-system-symbolic")
                .pixel_size(160)
                .halign(gtk::Align::Center)
                .build();
            setup_hero.append(&setup_logo);
        }

        let setup_title = gtk::Label::builder()
            .label("Declarative NixOS Setup")
            .css_classes(["title-1"])
            .halign(gtk::Align::Center)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .build();

        let setup_desc = gtk::Label::builder()
            .label(
                "NixApps never edits your hand-written dotfiles. Choose your configuration folder, and NixApps will manage a single dedicated .nix module that auto-adapts to Home Manager or NixOS.",
            )
            .css_classes(["dim-label"])
            .halign(gtk::Align::Center)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .max_width_chars(64)
            .build();

        setup_hero.append(&setup_title);
        setup_hero.append(&setup_desc);

        let config_group = adw::PreferencesGroup::builder()
            .title("1. Configuration Binding")
            .description("Point NixApps to your NixOS / Home Manager configuration directory.")
            .build();

        let pick_folder_btn = icon_button("folder-open-symbolic", "Choose Folder…", &[]);
        pick_folder_btn.set_valign(gtk::Align::Center);
        {
            let s = sender.clone();
            pick_folder_btn.connect_clicked(move |_| s.input(AppMsg::PickConfigFolder));
        }

        let setup_folder_row = adw::ActionRow::builder()
            .title("Nix Configuration Directory")
            .subtitle("Not selected yet (e.g. ~/av-dotfiles or /etc/nixos)")
            .build();
        setup_folder_row.add_suffix(&pick_folder_btn);
        config_group.add(&setup_folder_row);

        let target_model = gtk::StringList::new(&[
            "Home Manager (home.packages)",
            "NixOS System (environment.systemPackages)",
        ]);
        let setup_target_row = adw::ComboRow::builder()
            .title("Target Configuration Scope")
            .subtitle("Choose whether packages are written for Home Manager or NixOS System")
            .model(&target_model)
            .selected(match model.settings.target {
                ConfigTarget::HomeManager => 0,
                ConfigTarget::NixosSystem => 1,
            })
            .build();
        {
            let s = sender.clone();
            setup_target_row.connect_selected_notify(move |row| {
                let target = if row.selected() == 0 {
                    ConfigTarget::HomeManager
                } else {
                    ConfigTarget::NixosSystem
                };
                s.input(AppMsg::SetTarget(target));
            });
        }
        config_group.add(&setup_target_row);

        let setup_channel_entry = adw::EntryRow::builder()
            .title("NixOS Channel (e.g. nixos-26.05, nixos-unstable)")
            .text(&model.settings.channel)
            .build();
        {
            let s = sender.clone();
            setup_channel_entry.connect_changed(move |entry| {
                s.input(AppMsg::SetChannel(entry.text().to_string()));
            });
        }
        config_group.add(&setup_channel_entry);

        let init_module_btn = icon_button(
            "document-save-symbolic",
            "Save & Initialize Managed Module",
            &["suggested-action", "pill"],
        );
        init_module_btn.set_halign(gtk::Align::Start);
        init_module_btn.set_margin_top(10);
        {
            let s = sender.clone();
            init_module_btn.connect_clicked(move |_| s.input(AppMsg::InitializeManagedConfig));
        }

        // Import instructions card
        let import_group = adw::PreferencesGroup::builder()
            .title("2. One-Line Import Snippet")
            .description(
                "Add this single line to your imports list once. NixApps handles everything inside the managed file.",
            )
            .margin_top(16)
            .build();

        let setup_import_label = gtk::Label::builder()
            .xalign(0.0)
            .selectable(true)
            .wrap(true)
            .css_classes(["nix-code-view"])
            .build();
        let copy_snippet_btn = icon_button("edit-copy-symbolic", "Copy", &[]);
        copy_snippet_btn.set_valign(gtk::Align::Center);
        {
            let s = sender.clone();
            copy_snippet_btn.connect_clicked(move |_| s.input(AppMsg::CopyImportSnippet));
        }
        let snippet_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .css_classes(["snippet-card"])
            .build();
        setup_import_label.set_hexpand(true);
        snippet_box.append(&setup_import_label);
        snippet_box.append(&copy_snippet_btn);
        import_group.add(&snippet_box);

        // Catalog sync & API enrichment group
        let sync_group = adw::PreferencesGroup::builder()
            .title("3. Package Catalog &amp; Metadata Enrichment")
            .description("Sync official channels.nixos.org packages.json.br into SQLite FTS5. Optionally add a GitHub token to raise API limits for briefings.")
            .margin_top(16)
            .build();

        let sync_now_btn = icon_button("view-refresh-symbolic", "Sync Now", &[]);
        sync_now_btn.set_valign(gtk::Align::Center);
        {
            let s = sender.clone();
            sync_now_btn.connect_clicked(move |_| s.input(AppMsg::StartSync));
        }
        let setup_sync_status_row = adw::ActionRow::builder()
            .title("Local Catalog Status")
            .subtitle("Not synced yet")
            .build();
        setup_sync_status_row.add_suffix(&sync_now_btn);
        sync_group.add(&setup_sync_status_row);

        let setup_github_token_entry = adw::PasswordEntryRow::builder()
            .title("GitHub Personal Access Token (Optional — 5,000 req/h)")
            .text(model.settings.github_token.as_deref().unwrap_or(""))
            .build();
        {
            let s = sender.clone();
            setup_github_token_entry.connect_changed(move |entry| {
                s.input(AppMsg::SetGithubToken(entry.text().to_string()));
            });
        }
        sync_group.add(&setup_github_token_entry);

        // System Rebuild Runner Configuration
        let rebuild_group = adw::PreferencesGroup::builder()
            .title("4. System Rebuild Command")
            .description("Queue multiple app installs/removals, then click Apply &amp; Rebuild to activate them on your NixOS system.")
            .margin_top(16)
            .build();

        let effective_rebuild = model.settings.effective_rebuild_command();
        let setup_rebuild_cmd_entry = adw::EntryRow::builder()
            .title("Rebuild Command")
            .text(&effective_rebuild.command)
            .build();
        {
            let s = sender.clone();
            setup_rebuild_cmd_entry.connect_changed(move |entry| {
                s.input(AppMsg::SetRebuildCommand(entry.text().to_string()));
            });
        }
        rebuild_group.add(&setup_rebuild_cmd_entry);

        let elevation_model = gtk::StringList::new(&[
            "Built-in Password Dialog (sudo) — Recommended",
            "Desktop Polkit Agent (pkexec)",
            "None (no root — standalone home-manager)",
        ]);
        let setup_rebuild_elevation_row = adw::ComboRow::builder()
            .title("Privilege Elevation")
            .subtitle(
                "Built-in dialog works on every Wayland compositor even without a Polkit agent",
            )
            .model(&elevation_model)
            .selected(elevation_to_index(effective_rebuild.elevation))
            .build();
        {
            let s = sender.clone();
            setup_rebuild_elevation_row.connect_selected_notify(move |row| {
                s.input(AppMsg::SetRebuildElevation(index_to_elevation(
                    row.selected(),
                )));
            });
        }
        rebuild_group.add(&setup_rebuild_elevation_row);

        let auto_detect_btn = icon_button("find-location-symbolic", "Auto-Detect Command", &[]);
        auto_detect_btn.set_halign(gtk::Align::Start);
        auto_detect_btn.set_margin_top(8);
        {
            let s = sender.clone();
            auto_detect_btn.connect_clicked(move |_| s.input(AppMsg::AutoDetectRebuildCommand));
        }

        let setup_vbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_bottom(32)
            .build();
        setup_vbox.append(&setup_hero);
        setup_vbox.append(&config_group);
        setup_vbox.append(&init_module_btn);
        setup_vbox.append(&import_group);
        setup_vbox.append(&sync_group);
        setup_vbox.append(&rebuild_group);
        setup_vbox.append(&auto_detect_btn);

        let setup_clamp = adw::Clamp::builder()
            .maximum_size(760)
            .margin_start(16)
            .margin_end(16)
            .child(&setup_vbox)
            .build();

        let setup_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&setup_clamp)
            .build();

        view_stack.add_titled_with_icon(
            &setup_scroll,
            Some("setup"),
            "Setup",
            "emblem-system-symbolic",
        );

        if !model.settings.is_configured() {
            view_stack.set_visible_child_name("setup");
        } else {
            view_stack.set_visible_child_name("explore");
        }

        // --- DETAIL VIEW ---
        let detail_icon = gtk::Image::builder()
            .pixel_size(64)
            .css_classes(["app-hero-icon"])
            .build();

        let detail_title = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["title-1"])
            .wrap(true)
            .build();

        let detail_subtitle = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["dim-label"])
            .selectable(true)
            .build();

        let detail_badges_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_top(6)
            .build();

        let detail_title_col = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        detail_title_col.append(&detail_title);
        detail_title_col.append(&detail_subtitle);
        detail_title_col.append(&detail_badges_box);

        let detail_primary_btn = icon_button(
            "folder-download-symbolic",
            "Install",
            &["suggested-action", "pill"],
        );
        detail_primary_btn.set_valign(gtk::Align::Center);

        let detail_homepage_btn = icon_button("web-browser-symbolic", "Homepage", &[]);
        detail_homepage_btn.set_valign(gtk::Align::Center);

        let detail_actions_col = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .valign(gtk::Align::Center)
            .build();
        detail_actions_col.append(&detail_homepage_btn);
        detail_actions_col.append(&detail_primary_btn);

        let detail_header_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(16)
            .margin_top(20)
            .margin_bottom(12)
            .build();
        detail_header_row.append(&detail_icon);
        detail_header_row.append(&detail_title_col);
        detail_header_row.append(&detail_actions_col);

        let detail_briefing_status = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "caption"])
            .margin_top(6)
            .build();

        let detail_description = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .margin_top(4)
            .margin_bottom(16)
            .build();

        let detail_meta_group = adw::PreferencesGroup::builder()
            .title("Package Details")
            .build();

        // On-demand Dependencies Viewer
        let deps_group = adw::PreferencesGroup::builder()
            .title("Dependencies (On-Demand via Nix)")
            .description(
                "Evaluates real build &amp; runtime inputs directly from your local Nixpkgs.",
            )
            .margin_top(18)
            .build();

        let detail_deps_btn = icon_button("system-search-symbolic", "Inspect Dependencies", &[]);
        detail_deps_btn.set_halign(gtk::Align::Start);

        let detail_deps_status = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "caption"])
            .wrap(true)
            .margin_top(4)
            .build();

        let detail_deps_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_top(6)
            .build();

        let deps_outer = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        deps_outer.append(&detail_deps_btn);
        deps_outer.append(&detail_deps_status);
        deps_outer.append(&detail_deps_box);
        deps_group.add(&deps_outer);

        // Custom per-app Nix configuration section
        let custom_cfg_group = adw::PreferencesGroup::builder()
            .title("Declarative Configuration (Optional)")
            .description(
                "Add custom Nix options, `programs.&lt;name&gt;` settings, or overrides for this application. Validated with `nix-instantiate --parse` before saving.",
            )
            .margin_top(20)
            .build();

        let mode_model = gtk::StringList::new(&[
            "Standard Package (packages list only)",
            "Package + Custom Nix Config",
            "Custom Nix Config Only (e.g. programs.<name>.enable = true;)",
        ]);
        let detail_mode_dropdown = gtk::DropDown::builder()
            .model(&mode_model)
            .selected(0)
            .margin_bottom(6)
            .build();

        // Opaque left-aligned hover help card shown directly under the template chips.
        let template_help_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(68)
            .css_classes(["caption"])
            .build();
        let template_help_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Start)
            .css_classes(["template-help-box"])
            .margin_bottom(8)
            .build();
        let help_info_icon = gtk::Image::builder()
            .icon_name("dialog-information-symbolic")
            .pixel_size(16)
            .valign(gtk::Align::Start)
            .css_classes(["pending-pill-dot"])
            .build();
        template_help_box.append(&help_info_icon);
        template_help_box.append(&template_help_label);

        let template_help_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(150)
            .reveal_child(false)
            .halign(gtk::Align::Start)
            .child(&template_help_box)
            .build();

        let tmpl_programs_btn = icon_button(
            "list-add-symbolic",
            "Insert programs.<name>.enable",
            &["template-chip"],
        );
        {
            let s = sender.clone();
            tmpl_programs_btn.connect_clicked(move |_| {
                s.input(AppMsg::InsertNixTemplate("programs"));
            });
            let motion = gtk::EventControllerMotion::new();
            let rev_in = template_help_revealer.clone();
            let lbl_in = template_help_label.clone();
            motion.connect_enter(move |_, _, _| {
                lbl_in.set_label(
                    "Inserts `programs.<name>.enable = true;` — use this when the app offers built-in NixOS / Home Manager configuration options (e.g. git, helix, firefox, steam) instead of only adding the binary.",
                );
                rev_in.set_reveal_child(true);
            });
            let rev_out = template_help_revealer.clone();
            motion.connect_leave(move |_| {
                rev_out.set_reveal_child(false);
            });
            tmpl_programs_btn.add_controller(motion);
        }

        let tmpl_override_btn = icon_button(
            "list-add-symbolic",
            "Insert override template",
            &["template-chip"],
        );
        {
            let s = sender.clone();
            tmpl_override_btn.connect_clicked(move |_| {
                s.input(AppMsg::InsertNixTemplate("override"));
            });
            let motion = gtk::EventControllerMotion::new();
            let rev_in = template_help_revealer.clone();
            let lbl_in = template_help_label.clone();
            motion.connect_enter(move |_, _, _| {
                lbl_in.set_label(
                    "Inserts a `pkgs.<name>.override { ... }` block — use this when you need to pass custom build options, extra libraries, or plugins to a package (e.g. lutris, ffmpeg, vim).",
                );
                rev_in.set_reveal_child(true);
            });
            let rev_out = template_help_revealer.clone();
            motion.connect_leave(move |_| {
                rev_out.set_reveal_child(false);
            });
            tmpl_override_btn.add_controller(motion);
        }

        let tmpl_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Start)
            .margin_top(2)
            .margin_bottom(6)
            .build();
        tmpl_row.append(&tmpl_programs_btn);
        tmpl_row.append(&tmpl_override_btn);

        let detail_nix_buffer = gtk::TextBuffer::new(None);
        let detail_nix_view = gtk::TextView::builder()
            .buffer(&detail_nix_buffer)
            .monospace(true)
            .css_classes(["nix-code-view"])
            .height_request(120)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(10)
            .right_margin(10)
            .build();

        let detail_nix_frame = gtk::Frame::builder().child(&detail_nix_view).build();

        let detail_error_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["error"])
            .margin_top(6)
            .build();

        let save_custom_btn = icon_button(
            "document-save-symbolic",
            "Save & Validate Custom Config",
            &["suggested-action"],
        );
        save_custom_btn.set_halign(gtk::Align::Start);
        save_custom_btn.set_margin_top(10);

        let custom_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .build();
        custom_box.append(&detail_mode_dropdown);
        custom_box.append(&tmpl_row);
        custom_box.append(&template_help_revealer);
        custom_box.append(&detail_nix_frame);
        custom_box.append(&detail_error_label);
        custom_box.append(&save_custom_btn);
        custom_cfg_group.add(&custom_box);

        let detail_vbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_bottom(32)
            .build();
        detail_vbox.append(&detail_header_row);
        detail_vbox.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        detail_vbox.append(&detail_briefing_status);
        detail_vbox.append(&detail_description);
        detail_vbox.append(&detail_meta_group);
        detail_vbox.append(&deps_group);
        detail_vbox.append(&custom_cfg_group);

        let detail_clamp = adw::Clamp::builder()
            .maximum_size(840)
            .margin_start(16)
            .margin_end(16)
            .child(&detail_vbox)
            .build();

        let detail_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&detail_clamp)
            .build();

        // Connect detail buttons
        {
            let s = sender.clone();
            let title_lbl = detail_subtitle.clone();
            detail_primary_btn.connect_clicked(move |_| {
                let attr = title_lbl.widget_name().to_string();
                if !attr.is_empty() {
                    s.input(AppMsg::ToggleInstall(attr));
                }
            });
        }
        {
            let s = sender.clone();
            let hp_btn = detail_homepage_btn.clone();
            detail_homepage_btn.connect_clicked(move |_| {
                let url = hp_btn.widget_name().to_string();
                if !url.is_empty() {
                    s.input(AppMsg::OpenHomepage(url));
                }
            });
        }
        {
            let s = sender.clone();
            let title_lbl = detail_subtitle.clone();
            detail_deps_btn.connect_clicked(move |_| {
                let attr = title_lbl.widget_name().to_string();
                if !attr.is_empty() {
                    s.input(AppMsg::InspectDependencies(attr));
                }
            });
        }
        {
            let s = sender.clone();
            let title_lbl = detail_subtitle.clone();
            let dd = detail_mode_dropdown.clone();
            let buf = detail_nix_buffer.clone();
            save_custom_btn.connect_clicked(move |_| {
                let attr_path = title_lbl.widget_name().to_string();
                if attr_path.is_empty() {
                    return;
                }
                let mode = match dd.selected() {
                    1 => InstallMode::PackageWithConfig,
                    2 => InstallMode::CustomOnly,
                    _ => InstallMode::Package,
                };
                let (start, end) = buf.bounds();
                let custom_nix = buf.text(&start, &end, false).to_string();
                s.input(AppMsg::SaveDetailConfig {
                    attr_path,
                    mode,
                    custom_nix,
                });
            });
        }

        // --- Root Stack ("tabs" vs "detail") ---
        let root_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .vexpand(true)
            .build();
        root_stack.add_named(&view_stack, Some("tabs"));
        root_stack.add_named(&detail_scroll, Some("detail"));
        root_stack.set_visible_child_name("tabs");

        {
            let s = sender.clone();
            let rs = root_stack.clone();
            view_stack.connect_visible_child_name_notify(move |_| {
                if rs.visible_child_name().as_deref() == Some("detail") {
                    s.input(AppMsg::CloseDetail);
                }
            });
        }

        // --- Rebuild Dialog ---
        let rebuild_dialog = adw::Dialog::builder()
            .title("Apply & Rebuild System")
            .content_width(720)
            .content_height(500)
            .build();

        let rebuild_cmd_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .css_classes(["nix-code-view", "snippet-card"])
            .build();

        let rebuild_status_label = gtk::Label::builder()
            .halign(gtk::Align::Start)
            .css_classes(["heading"])
            .label("Ready to run system rebuild.")
            .build();

        let rebuild_log_buffer = gtk::TextBuffer::new(None);
        let rebuild_log_view = gtk::TextView::builder()
            .buffer(&rebuild_log_buffer)
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .css_classes(["nix-code-view"])
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(10)
            .right_margin(10)
            .build();

        let rebuild_log_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&rebuild_log_view)
            .build();
        let rebuild_log_frame = gtk::Frame::builder()
            .vexpand(true)
            .child(&rebuild_log_scroll)
            .build();

        let rebuild_run_btn = icon_button(
            "emblem-synchronizing-symbolic",
            "Run Rebuild Now",
            &["suggested-action", "pill"],
        );
        {
            let s = sender.clone();
            rebuild_run_btn.connect_clicked(move |_| s.input(AppMsg::RequestRebuild));
        }

        let rebuild_diff_btn = icon_button("document-edit-symbolic", "Review Diff", &[]);
        {
            let s = sender.clone();
            rebuild_diff_btn.connect_clicked(move |_| s.input(AppMsg::OpenDiffDialog));
        }

        let rebuild_btn_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .halign(gtk::Align::End)
            .margin_top(6)
            .build();
        rebuild_btn_row.append(&rebuild_diff_btn);
        rebuild_btn_row.append(&rebuild_run_btn);

        let rebuild_body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_top(12)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        rebuild_body.append(&rebuild_cmd_label);
        rebuild_body.append(&rebuild_status_label);
        rebuild_body.append(&rebuild_log_frame);
        rebuild_body.append(&rebuild_btn_row);

        let rebuild_tv = adw::ToolbarView::new();
        rebuild_tv.add_top_bar(&adw::HeaderBar::new());
        rebuild_tv.set_content(Some(&rebuild_body));
        rebuild_dialog.set_child(Some(&rebuild_tv));

        // --- Floating Bottom-Right Pending Changes Pill (nested pill design) ---
        let pending_pill_icon = gtk::Image::builder()
            .icon_name("software-update-available-symbolic")
            .pixel_size(16)
            .css_classes(["pending-pill-dot"])
            .build();

        let pending_pill_label = gtk::Label::builder()
            .label("Pending changes")
            .css_classes(["heading"])
            .build();

        let pending_pill_btn = icon_button(
            "emblem-synchronizing-symbolic",
            "Apply & Rebuild",
            &["suggested-action", "pill"],
        );
        {
            let s = sender.clone();
            pending_pill_btn.connect_clicked(move |_| s.input(AppMsg::OpenRebuildDialog));
        }

        let pending_pill_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .valign(gtk::Align::Center)
            .css_classes(["pending-pill"])
            .build();
        pending_pill_box.append(&pending_pill_icon);
        pending_pill_box.append(&pending_pill_label);
        pending_pill_box.append(&pending_pill_btn);

        let pending_pill_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(200)
            .reveal_child(false)
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .margin_end(20)
            .margin_bottom(20)
            .child(&pending_pill_box)
            .build();

        // --- Floating Top-Right Notification Popup (opaque, left-aligned text, auto-dismiss) ---
        let top_toast_icon = gtk::Image::builder()
            .icon_name("emblem-ok-symbolic")
            .pixel_size(16)
            .css_classes(["pending-pill-dot"])
            .build();

        let top_toast_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(44)
            .build();

        let top_toast_close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .css_classes(["flat", "circular"])
            .tooltip_text("Dismiss")
            .valign(gtk::Align::Center)
            .build();
        {
            let s = sender.clone();
            top_toast_close.connect_clicked(move |_| s.input(AppMsg::DismissTopToast(0)));
        }

        let top_toast_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .valign(gtk::Align::Center)
            .css_classes(["top-toast-pill"])
            .build();
        top_toast_box.append(&top_toast_icon);
        top_toast_box.append(&top_toast_label);
        top_toast_box.append(&top_toast_close);

        let top_toast_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(200)
            .reveal_child(false)
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_end(20)
            .margin_top(12)
            .child(&top_toast_box)
            .build();

        // --- Window assembly ---
        let main_vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_vbox.append(&status_banner);
        main_vbox.append(&root_stack);

        let content_overlay = gtk::Overlay::new();
        content_overlay.set_child(Some(&main_vbox));
        content_overlay.add_overlay(&top_toast_revealer);
        content_overlay.add_overlay(&pending_pill_revealer);

        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.set_content(Some(&content_overlay));
        window.set_content(Some(&toolbar_view));

        let widgets = AppWidgets {
            window,
            back_button,
            sync_button,
            status_banner,
            top_toast_revealer,
            top_toast_label,
            pending_pill_revealer,
            pending_pill_label,
            root_stack,
            view_stack,
            search_entry,
            unfree_switch,
            category_dropdown,
            results_summary_label,
            home_scroll,
            categories_flow,
            featured_flow,
            results_list,
            explore_empty_status,
            explore_results_scroll,
            detail_icon,
            detail_title,
            detail_subtitle,
            detail_badges_box,
            detail_primary_btn,
            detail_homepage_btn,
            detail_briefing_status,
            detail_description,
            detail_meta_group,
            detail_deps_btn,
            detail_deps_status,
            detail_deps_box,
            detail_mode_dropdown,
            detail_nix_buffer,
            detail_error_label,
            installed_summary_label,
            installed_list,
            installed_empty_status,
            installed_content_scroll,
            installed_preview_buffer,
            setup_folder_row,
            setup_target_row,
            setup_channel_entry,
            setup_github_token_entry,
            setup_rebuild_cmd_entry,
            setup_rebuild_elevation_row,
            setup_import_label,
            setup_sync_status_row,
            rebuild_dialog,
            rebuild_cmd_label,
            rebuild_status_label,
            rebuild_run_btn,
            rebuild_log_buffer,
            rebuild_log_view,
        };

        refresh_all_views(&model, &widgets, &sender);

        ComponentParts { model, widgets }
    }

    fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        msg: Self::Input,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        let mut toast_msg: Option<String> = None;

        match msg {
            AppMsg::SearchChanged(text) => {
                self.search_text = text;
                self.run_search();
                if !self.is_home_mode() {
                    self.spawn_icon_batch(
                        self.search_results.iter().take(16).cloned().collect(),
                        &sender,
                    );
                }
            }
            AppMsg::CategorySelected(category) => {
                self.selected_category = category;
                let target_idx = match category {
                    None => 0u32,
                    Some(cat) => Category::ALL
                        .iter()
                        .position(|&c| c == cat)
                        .map(|i| (i + 1) as u32)
                        .unwrap_or(0),
                };
                if widgets.category_dropdown.selected() != target_idx {
                    widgets.category_dropdown.set_selected(target_idx);
                }
                self.run_search();
                if !self.is_home_mode() {
                    self.spawn_icon_batch(
                        self.search_results.iter().take(16).cloned().collect(),
                        &sender,
                    );
                }
            }
            AppMsg::ToggleUnfree(include_unfree) => {
                self.settings.include_unfree = include_unfree;
                let _ = self.settings.save();
                self.run_search();
            }
            AppMsg::OpenDetail(pkg) => {
                let pkg_val = *pkg;
                self.detail_error = None;
                self.detail_enriched = None;
                self.detail_loading_meta = true;
                self.detail_deps = None;
                self.detail_loading_deps = false;
                self.detail_deps_error = None;
                self.selected_package = Some(pkg_val.clone());

                populate_detail_view(self, widgets);
                widgets.root_stack.set_visible_child_name("detail");
                widgets.back_button.set_visible(true);

                self.spawn_detail_enrichment(pkg_val, &sender);
            }
            AppMsg::OpenDetailByAttr(attr) => {
                let pkg = self
                    .open_db()
                    .and_then(|db| db.get_package(&attr).ok().flatten())
                    .unwrap_or_else(|| PackageRecord {
                        attr_path: attr.clone(),
                        pname: attr.clone(),
                        version: String::new(),
                        description: Some(
                            "Managed application (not in current local index)".into(),
                        ),
                        long_description: None,
                        homepage: None,
                        license: None,
                        main_program: None,
                        position: None,
                        category: Category::Other,
                        unfree: false,
                        broken: false,
                        insecure: false,
                    });
                sender.input(AppMsg::OpenDetail(Box::new(pkg)));
                return;
            }
            AppMsg::CloseDetail => {
                self.selected_package = None;
                self.detail_enriched = None;
                self.detail_deps = None;
                self.detail_error = None;
                widgets.root_stack.set_visible_child_name("tabs");
                widgets.back_button.set_visible(false);
            }
            AppMsg::SwitchTab(name) => {
                self.selected_package = None;
                widgets.root_stack.set_visible_child_name("tabs");
                widgets.back_button.set_visible(false);
                widgets.view_stack.set_visible_child_name(name);
            }
            AppMsg::BannerActionClicked => {
                if !self.settings.is_configured() {
                    sender.input(AppMsg::SwitchTab("setup"));
                } else {
                    sender.input(AppMsg::OpenRebuildDialog);
                }
                return;
            }
            AppMsg::OpenAboutDialog => {
                show_about_dialog(&widgets.window);
                return;
            }
            AppMsg::DismissTopToast(generation) => {
                if generation == 0 || generation == self.toast_generation {
                    widgets.top_toast_revealer.set_reveal_child(false);
                }
                return;
            }
            AppMsg::IconResolved {
                attr_path,
                icon_path,
            } => {
                self.icon_cache.insert(attr_path, icon_path);
            }
            AppMsg::DetailMetadataLoaded { attr_path, info } => {
                if let Some(path) = &info.icon_path {
                    self.icon_cache.insert(attr_path.clone(), path.clone());
                }
                if self
                    .selected_package
                    .as_ref()
                    .is_some_and(|p| p.attr_path == attr_path)
                {
                    self.detail_loading_meta = false;
                    self.detail_enriched = Some(info);
                }
            }
            AppMsg::InspectDependencies(attr_path) => {
                if self.detail_loading_deps {
                    return;
                }
                self.detail_loading_deps = true;
                self.detail_deps = None;
                self.detail_deps_error = None;

                let input_sender = sender.input_sender().clone();
                thread::spawn(move || {
                    let result = inspect_dependencies(&attr_path).map_err(|e| e.to_string());
                    let _ = input_sender.send(AppMsg::DependenciesLoaded { attr_path, result });
                });
            }
            AppMsg::DependenciesLoaded { attr_path, result } => {
                if self
                    .selected_package
                    .as_ref()
                    .is_some_and(|p| p.attr_path == attr_path)
                {
                    self.detail_loading_deps = false;
                    match result {
                        Ok(deps) => {
                            self.detail_deps = Some(deps);
                            self.detail_deps_error = None;
                        }
                        Err(err) => {
                            self.detail_deps_error = Some(err);
                        }
                    }
                }
            }
            AppMsg::InsertNixTemplate(kind) => {
                if let Some(pkg) = &self.selected_package {
                    let snippet = match kind {
                        "programs" => {
                            widgets.detail_mode_dropdown.set_selected(2);
                            format!("programs.{} = {{\n  enable = true;\n}};", pkg.pname)
                        }
                        _ => {
                            widgets.detail_mode_dropdown.set_selected(2);
                            let list_attr = self.settings.target.packages_attr();
                            format!(
                                "{list_attr} = [\n  (pkgs.{}.override {{\n    # Add override options here\n  }})\n];",
                                pkg.attr_path
                            )
                        }
                    };
                    widgets.detail_nix_buffer.set_text(&snippet);
                }
                return;
            }
            AppMsg::ToggleInstall(attr) => {
                if self.managed_state.is_installed(&attr) {
                    sender.input(AppMsg::QuickUninstall(attr));
                } else {
                    sender.input(AppMsg::QuickInstall(attr));
                }
                return;
            }
            AppMsg::QuickInstall(attr) => {
                if !self.settings.is_configured() {
                    toast_msg = Some("Select your Nix dotfiles folder in Setup first.".into());
                    widgets.view_stack.set_visible_child_name("setup");
                } else {
                    match self.managed_state.install_package(&attr) {
                        Ok(()) => match self.apply_state_to_disk() {
                            Ok(info) => {
                                toast_msg = Some(format!("Queued `{attr}` · {info}"));
                                self.detail_error = None;
                            }
                            Err(err) => {
                                self.managed_state.remove_app(&attr);
                                toast_msg = Some(format!("Failed: {err}"));
                            }
                        },
                        Err(err) => toast_msg = Some(format!("Invalid package: {err}")),
                    }
                }
            }
            AppMsg::QuickUninstall(attr) => {
                let prev = self.managed_state.get_app(&attr).cloned();
                if self.managed_state.remove_app(&attr) {
                    match self.apply_state_to_disk() {
                        Ok(info) => {
                            toast_msg = Some(format!("Removed `{attr}` · {info}"));
                            self.detail_error = None;
                        }
                        Err(err) => {
                            if let Some(app) = prev {
                                let _ = self.managed_state.upsert_app(app);
                            }
                            toast_msg = Some(format!("Failed: {err}"));
                        }
                    }
                }
            }
            AppMsg::SaveDetailConfig {
                attr_path,
                mode,
                custom_nix,
            } => {
                if !self.settings.is_configured() {
                    toast_msg = Some("Select your Nix dotfiles folder in Setup first.".into());
                } else {
                    let prev = self.managed_state.get_app(&attr_path).cloned();
                    let trimmed = custom_nix.trim().to_string();
                    let app = ManagedApp {
                        attr_path: attr_path.clone(),
                        mode,
                        custom_nix: (!trimmed.is_empty()).then_some(trimmed),
                        notes: None,
                    };
                    match self.managed_state.upsert_app(app) {
                        Ok(()) => match self.apply_state_to_disk() {
                            Ok(info) => {
                                self.detail_error = None;
                                toast_msg =
                                    Some(format!("Saved config for `{attr_path}` · {info}"));
                            }
                            Err(err) => {
                                if let Some(old) = prev {
                                    let _ = self.managed_state.upsert_app(old);
                                } else {
                                    self.managed_state.remove_app(&attr_path);
                                }
                                self.detail_error = Some(err);
                                toast_msg = Some(
                                    "Nix syntax validation failed — file left untouched.".into(),
                                );
                            }
                        },
                        Err(err) => {
                            self.detail_error = Some(err.to_string());
                        }
                    }
                }
            }
            AppMsg::OpenHomepage(url) => {
                let launcher = gtk::UriLauncher::new(&url);
                launcher.launch(Some(&widgets.window), gtk::gio::Cancellable::NONE, |_| {});
                return;
            }
            AppMsg::OpenDiffDialog => {
                let diff_text = self.current_session_diff();
                show_diff_dialog(&widgets.window, &diff_text);
                return;
            }
            AppMsg::OpenRebuildDialog => {
                let cmd = self.settings.effective_rebuild_command();
                let preview = match cmd.elevation {
                    ElevationMethod::BuiltInSudo => format!("sudo {}", cmd.command),
                    ElevationMethod::Polkit => format!("pkexec {}", cmd.command),
                    ElevationMethod::None => cmd.command.clone(),
                };
                widgets.rebuild_cmd_label.set_label(&preview);
                widgets.rebuild_status_label.set_label(&self.rebuild_status);
                widgets.rebuild_run_btn.set_sensitive(!self.is_rebuilding);
                widgets.rebuild_dialog.present(Some(&widgets.window));
                return;
            }
            AppMsg::RequestRebuild => {
                if self.is_rebuilding {
                    return;
                }
                let cmd = self.settings.effective_rebuild_command();
                match cmd.elevation {
                    ElevationMethod::BuiltInSudo => {
                        show_password_prompt_dialog(&widgets.window, &cmd.command, &sender);
                    }
                    ElevationMethod::Polkit | ElevationMethod::None => {
                        sender.input(AppMsg::StartRebuildWithPassword(None));
                    }
                }
                return;
            }
            AppMsg::StartRebuildWithPassword(password) => {
                if self.is_rebuilding {
                    return;
                }
                let cmd = self.settings.effective_rebuild_command();
                self.is_rebuilding = true;
                self.rebuild_status = "Running rebuild… streaming live Nix output".into();
                widgets.rebuild_status_label.set_label(&self.rebuild_status);
                widgets.rebuild_run_btn.set_sensitive(false);

                let prefix = match cmd.elevation {
                    ElevationMethod::BuiltInSudo => "sudo ",
                    ElevationMethod::Polkit => "pkexec ",
                    ElevationMethod::None => "",
                };
                widgets
                    .rebuild_log_buffer
                    .set_text(&format!("$ {prefix}{}\n\n", cmd.command));

                let input_sender = sender.input_sender().clone();
                thread::spawn(move || {
                    let line_sender = input_sender.clone();
                    let res = run_rebuild_streaming(&cmd, password.as_deref(), |line| {
                        let _ = line_sender.send(AppMsg::RebuildLogLine(line));
                    })
                    .map_err(|e| e.to_string());
                    // `password` is dropped right here once the rebuild finishes.
                    let _ = input_sender.send(AppMsg::RebuildFinished(res));
                });
                return;
            }
            AppMsg::RebuildLogLine(line) => {
                let mut end = widgets.rebuild_log_buffer.end_iter();
                widgets
                    .rebuild_log_buffer
                    .insert(&mut end, &format!("{line}\n"));
                let mut scroll_end = widgets.rebuild_log_buffer.end_iter();
                widgets
                    .rebuild_log_view
                    .scroll_to_iter(&mut scroll_end, 0.0, false, 0.0, 1.0);
                return;
            }
            AppMsg::RebuildFinished(result) => {
                self.is_rebuilding = false;
                widgets.rebuild_run_btn.set_sensitive(true);
                match result {
                    Ok(outcome) if outcome.success => {
                        let applied_count = self.pending_changes;
                        self.pending_changes = 0;
                        self.last_git_warning = None;
                        self.baseline_nix =
                            render_nix_module(&self.managed_state).unwrap_or_default();
                        self.rebuild_status =
                            "Rebuild succeeded! Your applications are now active.".into();
                        widgets.rebuild_status_label.remove_css_class("error");
                        widgets.rebuild_status_label.add_css_class("success");
                        widgets.rebuild_dialog.close();
                        show_rebuild_success_dialog(
                            &widgets.window,
                            applied_count,
                            self.managed_state.apps.len(),
                            &sender,
                        );
                    }
                    Ok(outcome) => {
                        let code = outcome
                            .exit_code
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "unknown".into());
                        self.rebuild_status =
                            format!("Rebuild failed (exit code {code}). Check log above.");
                        widgets.rebuild_status_label.remove_css_class("success");
                        widgets.rebuild_status_label.add_css_class("error");
                        toast_msg = Some(format!("Rebuild failed (exit code {code})"));
                    }
                    Err(err) => {
                        self.rebuild_status = format!("Failed to launch rebuild: {err}");
                        widgets.rebuild_status_label.remove_css_class("success");
                        widgets.rebuild_status_label.add_css_class("error");
                        toast_msg = Some(format!("Rebuild error: {err}"));
                    }
                }
                widgets.rebuild_status_label.set_label(&self.rebuild_status);
            }
            AppMsg::PickConfigFolder => {
                let dialog = gtk::FileDialog::builder()
                    .title("Select Your NixOS / Home Manager Configuration Folder")
                    .modal(true)
                    .build();
                let s = sender.clone();
                dialog.select_folder(
                    Some(&widgets.window),
                    gtk::gio::Cancellable::NONE,
                    move |res| {
                        if let Ok(file) = res
                            && let Some(path) = file.path()
                        {
                            s.input(AppMsg::ConfigFolderPicked(path));
                        }
                    },
                );
                return;
            }
            AppMsg::ConfigFolderPicked(path) => {
                self.settings.config_dir = Some(path.clone());
                if self.settings.rebuild_command.is_none() {
                    let suggested = suggest_rebuild_command(&path, self.settings.target);
                    self.settings.rebuild_command = Some(suggested.command.clone());
                    self.settings.rebuild_elevation = Some(suggested.elevation);
                    widgets.setup_rebuild_cmd_entry.set_text(&suggested.command);
                    widgets
                        .setup_rebuild_elevation_row
                        .set_selected(elevation_to_index(suggested.elevation));
                }
                let _ = self.settings.save();
                self.reload_managed_state();
                toast_msg = Some("Configuration folder selected.".into());
            }
            AppMsg::SetTarget(target) => {
                self.settings.target = target;
                self.managed_state.target = target;
                let _ = self.settings.save();
            }
            AppMsg::SetChannel(channel) => {
                self.settings.channel = channel.trim().to_string();
                self.managed_state.channel = self.settings.channel.clone();
                let _ = self.settings.save();
            }
            AppMsg::SetGithubToken(token) => {
                let trimmed = token.trim().to_string();
                self.settings.github_token = (!trimmed.is_empty()).then_some(trimmed);
                let _ = self.settings.save();
                return;
            }
            AppMsg::SetRebuildCommand(cmd) => {
                let trimmed = cmd.trim().to_string();
                self.settings.rebuild_command = (!trimmed.is_empty()).then_some(trimmed);
                let _ = self.settings.save();
                return;
            }
            AppMsg::SetRebuildElevation(elevation) => {
                self.settings.rebuild_elevation = Some(elevation);
                let _ = self.settings.save();
                return;
            }
            AppMsg::AutoDetectRebuildCommand => {
                self.settings.rebuild_command = None;
                self.settings.rebuild_elevation = None;
                let suggested = self.settings.effective_rebuild_command();
                self.settings.rebuild_command = Some(suggested.command.clone());
                self.settings.rebuild_elevation = Some(suggested.elevation);
                let _ = self.settings.save();
                widgets.setup_rebuild_cmd_entry.set_text(&suggested.command);
                widgets
                    .setup_rebuild_elevation_row
                    .set_selected(elevation_to_index(suggested.elevation));
                toast_msg = Some(format!("Detected: {}", suggested.command));
            }
            AppMsg::InitializeManagedConfig => {
                if !self.settings.is_configured() {
                    toast_msg =
                        Some("Please select a valid Nix configuration folder first.".into());
                } else {
                    let _ = self.settings.save();
                    match self.apply_state_to_disk() {
                        Ok(info) => {
                            toast_msg = Some(format!("Ready! {info}"));
                        }
                        Err(err) => {
                            toast_msg = Some(format!("Setup failed: {err}"));
                        }
                    }
                }
            }
            AppMsg::CopyImportSnippet => {
                let rel = self
                    .settings
                    .managed_config()
                    .map(|c| c.relative_import_path())
                    .unwrap_or_else(|| "./modules/nixapps-managed.nix".to_string());
                widgets.window.clipboard().set_text(&rel);
                show_top_toast(
                    self,
                    widgets,
                    &sender,
                    &format!("Copied `{rel}` to clipboard"),
                );
                return;
            }
            AppMsg::StartSync => {
                if self.is_syncing {
                    return;
                }
                let channel = if self.settings.channel.trim().is_empty() {
                    "nixos-26.05".to_string()
                } else {
                    self.settings.channel.trim().to_string()
                };
                self.is_syncing = true;
                self.sync_progress = 0;

                let input_sender = sender.input_sender().clone();
                let db_path = catalog_db_path();
                thread::spawn(move || {
                    let result = CatalogDb::open(&db_path)
                        .and_then(|mut db| {
                            sync_channel(&mut db, &channel, |count| {
                                let _ = input_sender.send(AppMsg::SyncProgress(count));
                            })
                        })
                        .map_err(|e| e.to_string());
                    let _ = input_sender.send(AppMsg::SyncFinished(result));
                });
            }
            AppMsg::SyncProgress(count) => {
                self.sync_progress = count;
            }
            AppMsg::SyncFinished(result) => {
                self.is_syncing = false;
                match result {
                    Ok(info) => {
                        toast_msg = Some(format!(
                            "Synced {} packages from `{}`",
                            format_count(info.package_count),
                            info.channel
                        ));
                        self.sync_info = Some(info);
                        self.reload_home_data();
                        self.run_search();
                        self.spawn_icon_batch(self.featured_apps.clone(), &sender);
                    }
                    Err(err) => {
                        toast_msg = Some(format!("Sync failed: {err}"));
                    }
                }
            }
        }

        if let Some(text) = toast_msg {
            show_top_toast(self, widgets, &sender, &text);
        }

        refresh_all_views(self, widgets, &sender);
    }
}

/// Displays an opaque top-right notification that auto-dismisses after 3.5 seconds.
fn show_top_toast(
    model: &mut NixAppsModel,
    widgets: &AppWidgets,
    sender: &ComponentSender<NixAppsModel>,
    text: &str,
) {
    model.toast_generation = model.toast_generation.wrapping_add(1).max(1);
    let generation = model.toast_generation;

    widgets.top_toast_label.set_label(text);
    widgets.top_toast_revealer.set_reveal_child(true);

    let s = sender.clone();
    gtk::glib::timeout_add_local_once(Duration::from_millis(3500), move || {
        s.input(AppMsg::DismissTopToast(generation));
    });
}

/// Shows a native Libadwaita password popup dialog for `sudo -S` authentication.
fn show_password_prompt_dialog(
    parent: &adw::ApplicationWindow,
    command_preview: &str,
    sender: &ComponentSender<NixAppsModel>,
) {
    let dialog = adw::Dialog::builder()
        .title("Authentication Required")
        .content_width(440)
        .build();

    let lock_icon = gtk::Image::builder()
        .icon_name("dialog-password-symbolic")
        .pixel_size(48)
        .margin_top(8)
        .build();

    let heading = gtk::Label::builder()
        .label("Administrator Password Required")
        .css_classes(["title-3"])
        .build();

    let desc = gtk::Label::builder()
        .label("Enter your user password to activate your declarative NixOS configuration:")
        .css_classes(["dim-label"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();

    let cmd_box = gtk::Label::builder()
        .label(command_preview)
        .css_classes(["nix-code-view", "snippet-card"])
        .wrap(true)
        .selectable(true)
        .halign(gtk::Align::Fill)
        .xalign(0.0)
        .build();

    let pw_entry = gtk::PasswordEntry::builder()
        .placeholder_text("Enter sudo password…")
        .show_peek_icon(true)
        .hexpand(true)
        .margin_top(6)
        .build();

    let cancel_btn = gtk::Button::builder().label("Cancel").build();
    let confirm_btn = icon_button(
        "emblem-synchronizing-symbolic",
        "Authenticate & Rebuild",
        &["suggested-action"],
    );

    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .margin_top(10)
        .build();
    btn_row.append(&cancel_btn);
    btn_row.append(&confirm_btn);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    body.append(&lock_icon);
    body.append(&heading);
    body.append(&desc);
    body.append(&cmd_box);
    body.append(&pw_entry);
    body.append(&btn_row);

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&body));
    dialog.set_child(Some(&tv));

    {
        let dlg = dialog.clone();
        cancel_btn.connect_clicked(move |_| {
            dlg.close();
        });
    }
    {
        let dlg = dialog.clone();
        let entry = pw_entry.clone();
        let s = sender.clone();
        let submit = move || {
            let pw = entry.text().to_string();
            entry.set_text("");
            dlg.close();
            s.input(AppMsg::StartRebuildWithPassword(Some(pw)));
        };

        let submit_clone = submit.clone();
        confirm_btn.connect_clicked(move |_| submit_clone());
        pw_entry.connect_activate(move |_| submit());
    }

    dialog.present(Some(parent));
    pw_entry.grab_focus();
}

/// Shows a branded Libadwaita About dialog with NixApps logo and AI transparency credits.
fn show_about_dialog(parent: &adw::ApplicationWindow) {
    let dialog = adw::Dialog::builder()
        .title("About NixApps")
        .content_width(420)
        .build();

    let logo_img = gtk::Image::builder().pixel_size(96).margin_top(8).build();
    if let Some(tex) = nixapps_logo_texture() {
        logo_img.set_paintable(Some(&tex));
    } else if let Some(tex) = nixos_snowflake_texture() {
        logo_img.set_paintable(Some(&tex));
    } else {
        logo_img.set_icon_name(Some("application-x-executable-symbolic"));
    }

    let name_lbl = gtk::Label::builder()
        .label("NixApps")
        .css_classes(["title-1"])
        .build();

    let ver_badge = gtk::Label::builder()
        .label(format!("v{}", nixapps_core::version()))
        .css_classes(["badge", "badge-stars"])
        .halign(gtk::Align::Center)
        .build();

    let tagline = gtk::Label::builder()
        .label("Native Declarative App Store for NixOS & Home Manager")
        .css_classes(["dim-label"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();

    let info_group = adw::PreferencesGroup::builder().margin_top(10).build();
    info_group.add(
        &adw::ActionRow::builder()
            .title("Repository")
            .subtitle("https://github.com/asphzeera/NixApps")
            .build(),
    );
    info_group.add(
        &adw::ActionRow::builder()
            .title("Development Model")
            .subtitle("Human-in-the-Loop + AI-Assisted Engineering (MIT License)")
            .build(),
    );
    info_group.add(
        &adw::ActionRow::builder()
            .title("Stack")
            .subtitle("Rust + GTK4 + Libadwaita (Relm4) + SQLite FTS5")
            .build(),
    );

    let github_btn = icon_button("web-browser-symbolic", "Open on GitHub", &["pill"]);
    github_btn.set_halign(gtk::Align::Center);
    github_btn.set_margin_top(8);
    {
        let win = parent.clone();
        github_btn.connect_clicked(move |_| {
            let launcher = gtk::UriLauncher::new("https://github.com/asphzeera/NixApps");
            launcher.launch(Some(&win), gtk::gio::Cancellable::NONE, |_| {});
        });
    }

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(22)
        .margin_start(20)
        .margin_end(20)
        .build();
    body.append(&logo_img);
    body.append(&name_lbl);
    body.append(&ver_badge);
    body.append(&tagline);
    body.append(&info_group);
    body.append(&github_btn);

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&body));
    dialog.set_child(Some(&tv));
    dialog.present(Some(parent));
}

fn show_diff_dialog(parent: &adw::ApplicationWindow, diff_text: &str) {
    let dialog = adw::Dialog::builder()
        .title("Managed Module Diff")
        .content_width(700)
        .content_height(460)
        .build();

    let buf = gtk::TextBuffer::new(None);
    buf.set_text(diff_text);

    let view = gtk::TextView::builder()
        .buffer(&buf)
        .editable(false)
        .monospace(true)
        .css_classes(["nix-code-view"])
        .top_margin(12)
        .bottom_margin(12)
        .left_margin(14)
        .right_margin(14)
        .build();

    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&view)
        .build();

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&scroll));
    dialog.set_child(Some(&tv));
    dialog.present(Some(parent));
}

/// Shows a celebratory confirmation popup after a NixOS / Home Manager rebuild finishes cleanly.
fn show_rebuild_success_dialog(
    parent: &adw::ApplicationWindow,
    applied_changes: usize,
    total_managed: usize,
    sender: &ComponentSender<NixAppsModel>,
) {
    let dialog = adw::Dialog::builder()
        .title("System Updated")
        .content_width(420)
        .build();

    let ok_icon = gtk::Image::builder()
        .icon_name("emblem-ok-symbolic")
        .pixel_size(64)
        .css_classes(["success"])
        .margin_top(10)
        .build();

    let heading = gtk::Label::builder()
        .label("System Updated Successfully!")
        .css_classes(["title-2"])
        .justify(gtk::Justification::Center)
        .wrap(true)
        .build();

    let detail_text = if applied_changes > 0 {
        let plural = if applied_changes == 1 {
            "change"
        } else {
            "changes"
        };
        format!(
            "{applied_changes} declarative {plural} activated. Your NixApps module now manages {total_managed} application(s) and everything is ready to use."
        )
    } else {
        format!(
            "Your NixOS configuration rebuilt cleanly ({total_managed} managed application(s) active)."
        )
    };

    let desc = gtk::Label::builder()
        .label(&detail_text)
        .css_classes(["dim-label"])
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();

    let view_log_btn = icon_button("utilities-terminal-symbolic", "View Log", &[]);
    let done_btn = icon_button("emblem-ok-symbolic", "Done", &["suggested-action", "pill"]);

    let btn_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .halign(gtk::Align::Center)
        .margin_top(12)
        .build();
    btn_row.append(&view_log_btn);
    btn_row.append(&done_btn);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(10)
        .margin_bottom(22)
        .margin_start(22)
        .margin_end(22)
        .build();
    body.append(&ok_icon);
    body.append(&heading);
    body.append(&desc);
    body.append(&btn_row);

    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    tv.set_content(Some(&body));
    dialog.set_child(Some(&tv));

    {
        let dlg = dialog.clone();
        done_btn.connect_clicked(move |_| {
            dlg.close();
        });
    }
    {
        let dlg = dialog.clone();
        let s = sender.clone();
        view_log_btn.connect_clicked(move |_| {
            dlg.close();
            s.input(AppMsg::OpenRebuildDialog);
        });
    }

    dialog.present(Some(parent));
    done_btn.grab_focus();
}

/// Synchronizes all visible widgets with the latest `NixAppsModel`.
fn refresh_all_views(
    model: &NixAppsModel,
    widgets: &AppWidgets,
    sender: &ComponentSender<NixAppsModel>,
) {
    widgets.sync_button.set_sensitive(!model.is_syncing);
    if model.is_syncing {
        widgets.status_banner.set_title(&format!(
            "Syncing channel `{}`… {} packages indexed",
            model.settings.channel,
            format_count(model.sync_progress)
        ));
        widgets.status_banner.set_button_label(None);
        widgets.status_banner.set_revealed(true);
    } else if !model.settings.is_configured() {
        widgets
            .status_banner
            .set_title("Choose your Nix dotfiles folder in Setup to enable declarative installs.");
        widgets.status_banner.set_button_label(Some("Open Setup"));
        widgets.status_banner.set_revealed(true);
    } else if let Some(warn) = &model.last_git_warning {
        widgets.status_banner.set_title(warn);
        widgets.status_banner.set_button_label(None);
        widgets.status_banner.set_revealed(true);
    } else {
        widgets.status_banner.set_revealed(false);
    }

    // Floating nested pill in bottom-right corner for queued changes ready to rebuild.
    if model.pending_changes > 0 && model.settings.is_configured() {
        let label = if model.pending_changes == 1 {
            "1 pending change".to_string()
        } else {
            format!("{} pending changes", model.pending_changes)
        };
        widgets.pending_pill_label.set_label(&label);
        widgets.pending_pill_revealer.set_reveal_child(true);
    } else {
        widgets.pending_pill_revealer.set_reveal_child(false);
    }

    if widgets.unfree_switch.is_active() != model.settings.include_unfree {
        widgets
            .unfree_switch
            .set_active(model.settings.include_unfree);
    }
    let _ = &widgets.search_entry;

    refresh_explore_view(model, widgets, sender);
    rebuild_installed_list(model, widgets, sender);
    refresh_setup_tab(model, widgets);

    if model.selected_package.is_some() {
        refresh_detail_dynamic_state(model, widgets);
    }
}

fn refresh_explore_view(
    model: &NixAppsModel,
    widgets: &AppWidgets,
    sender: &ComponentSender<NixAppsModel>,
) {
    let has_catalog = model.sync_info.is_some();
    let show_home = model.is_home_mode() && !model.featured_apps.is_empty();

    if let Some(info) = &model.sync_info {
        let summary = if show_home {
            format!(
                "Catalog ready · {} packages indexed from `{}`",
                format_count(info.package_count),
                info.channel
            )
        } else if model.search_text.trim().is_empty() {
            let cat_name = model
                .selected_category
                .map(|c| c.display_name())
                .unwrap_or("All");
            format!(
                "Browsing {} · showing {} packages from `{}`",
                cat_name,
                model.search_results.len(),
                info.channel
            )
        } else {
            format!(
                "{} results for \"{}\" in `{}`",
                model.search_results.len(),
                model.search_text.trim(),
                info.channel
            )
        };
        widgets.results_summary_label.set_label(&summary);
    } else {
        widgets
            .results_summary_label
            .set_label("Local catalog not synced yet — click Sync to download nixpkgs index.");
    }

    if show_home {
        widgets.home_scroll.set_visible(true);
        widgets.explore_results_scroll.set_visible(false);
        widgets.explore_empty_status.set_visible(false);
        rebuild_home_cards(model, widgets, sender);
        return;
    }

    widgets.home_scroll.set_visible(false);
    let has_results = !model.search_results.is_empty();
    widgets.explore_results_scroll.set_visible(has_results);
    widgets.explore_empty_status.set_visible(!has_results);

    if !has_catalog {
        widgets.explore_empty_status.set_title("Catalog Not Synced");
        widgets.explore_empty_status.set_description(Some(
            "Download the official NixOS packages.json.br index for instant offline search.",
        ));
    } else {
        widgets
            .explore_empty_status
            .set_title("No Matching Packages");
        widgets.explore_empty_status.set_description(Some(
            "Try another keyword, switch categories, or enable Unfree packages.",
        ));
    }

    while let Some(child) = widgets.results_list.first_child() {
        widgets.results_list.remove(&child);
    }

    for pkg in &model.search_results {
        let installed = model.managed_state.is_installed(&pkg.attr_path);
        let subtitle = pkg
            .description
            .as_deref()
            .unwrap_or("No description provided in Nixpkgs");

        let row = adw::ActionRow::builder()
            .title(gtk::glib::markup_escape_text(&pkg.attr_path))
            .subtitle(gtk::glib::markup_escape_text(subtitle))
            .activatable(true)
            .build();

        let icon = gtk::Image::new();
        apply_app_icon(
            &icon,
            model.icon_cache.get(&pkg.attr_path).map(PathBuf::as_path),
            pkg.category,
            32,
        );
        row.add_prefix(&icon);

        let right_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();

        if !pkg.version.is_empty() {
            let ver = gtk::Label::builder()
                .label(&pkg.version)
                .css_classes(["dim-label", "caption"])
                .build();
            right_box.append(&ver);
        }

        if pkg.unfree {
            let badge = gtk::Label::builder()
                .label("Unfree")
                .css_classes(["badge", "badge-unfree"])
                .build();
            right_box.append(&badge);
        }

        if installed {
            let badge = gtk::Label::builder()
                .label("Installed")
                .css_classes(["badge", "badge-installed"])
                .build();
            right_box.append(&badge);
        }

        let action_btn = if installed {
            icon_button("user-trash-symbolic", "Remove", &["flat"])
        } else {
            icon_button("folder-download-symbolic", "Install", &["suggested-action"])
        };

        {
            let s = sender.clone();
            let attr = pkg.attr_path.clone();
            action_btn.connect_clicked(move |_| {
                if installed {
                    s.input(AppMsg::QuickUninstall(attr.clone()));
                } else {
                    s.input(AppMsg::QuickInstall(attr.clone()));
                }
            });
        }

        right_box.append(&action_btn);
        row.add_suffix(&right_box);

        {
            let s = sender.clone();
            let pkg_clone = pkg.clone();
            row.connect_activated(move |_| {
                s.input(AppMsg::OpenDetail(Box::new(pkg_clone.clone())));
            });
        }

        widgets.results_list.append(&row);
    }
}

fn rebuild_home_cards(
    model: &NixAppsModel,
    widgets: &AppWidgets,
    sender: &ComponentSender<NixAppsModel>,
) {
    while let Some(child) = widgets.categories_flow.first_child() {
        widgets.categories_flow.remove(&child);
    }

    let browse_categories = [
        Category::Games,
        Category::AudioVideo,
        Category::Development,
        Category::Graphics,
        Category::Office,
        Category::Network,
        Category::Utilities,
        Category::System,
    ];

    for cat in browse_categories {
        let count = model
            .category_counts
            .iter()
            .find(|(c, _)| *c == cat)
            .map(|(_, n)| *n)
            .unwrap_or(0);

        let icon = gtk::Image::builder()
            .icon_name(category_icon_name(cat))
            .pixel_size(24)
            .build();
        let name_lbl = gtk::Label::builder()
            .label(cat.display_name())
            .halign(gtk::Align::Start)
            .css_classes(["heading"])
            .build();
        let count_lbl = gtk::Label::builder()
            .label(format!("{} packages", format_count(count)))
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "caption"])
            .build();

        let text_col = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text_col.append(&name_lbl);
        text_col.append(&count_lbl);

        let inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .build();
        inner.append(&icon);
        inner.append(&text_col);

        let btn = gtk::Button::builder()
            .child(&inner)
            .css_classes(["card", "category-tile"])
            .build();
        {
            let s = sender.clone();
            btn.connect_clicked(move |_| {
                s.input(AppMsg::CategorySelected(Some(cat)));
            });
        }
        widgets.categories_flow.insert(&btn, -1);
    }

    while let Some(child) = widgets.featured_flow.first_child() {
        widgets.featured_flow.remove(&child);
    }

    for pkg in &model.featured_apps {
        let installed = model.managed_state.is_installed(&pkg.attr_path);
        let icon = gtk::Image::new();
        apply_app_icon(
            &icon,
            model.icon_cache.get(&pkg.attr_path).map(PathBuf::as_path),
            pkg.category,
            48,
        );

        let title = gtk::Label::builder()
            .label(&pkg.pname)
            .halign(gtk::Align::Start)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();

        let desc_text = pkg
            .description
            .as_deref()
            .unwrap_or(pkg.category.display_name());
        let desc = gtk::Label::builder()
            .label(desc_text)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .lines(2)
            .wrap(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label", "caption"])
            .build();

        let meta_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_top(4)
            .build();
        let cat_lbl = gtk::Label::builder()
            .label(pkg.category.display_name())
            .css_classes(["badge", "badge-category"])
            .build();
        meta_row.append(&cat_lbl);

        if installed {
            let inst_lbl = gtk::Label::builder()
                .label("Installed")
                .css_classes(["badge", "badge-installed"])
                .build();
            meta_row.append(&inst_lbl);
        }

        let text_col = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .hexpand(true)
            .build();
        text_col.append(&title);
        text_col.append(&desc);
        text_col.append(&meta_row);

        let card_inner = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .valign(gtk::Align::Center)
            .build();
        card_inner.append(&icon);
        card_inner.append(&text_col);

        let card_btn = gtk::Button::builder()
            .child(&card_inner)
            .css_classes(["card", "store-card"])
            .build();
        {
            let s = sender.clone();
            let pkg_clone = pkg.clone();
            card_btn.connect_clicked(move |_| {
                s.input(AppMsg::OpenDetail(Box::new(pkg_clone.clone())));
            });
        }

        widgets.featured_flow.insert(&card_btn, -1);
    }
}

fn rebuild_installed_list(
    model: &NixAppsModel,
    widgets: &AppWidgets,
    sender: &ComponentSender<NixAppsModel>,
) {
    while let Some(child) = widgets.installed_list.first_child() {
        widgets.installed_list.remove(&child);
    }

    let count = model.managed_state.apps.len();
    widgets.installed_content_scroll.set_visible(count > 0);
    widgets.installed_empty_status.set_visible(count == 0);

    widgets.installed_summary_label.set_label(&format!(
        "{} Managed Applications ({})",
        count,
        model.settings.target.display_name()
    ));

    for app in &model.managed_state.apps {
        let subtitle = format!(
            "Mode: {}{}",
            install_mode_label(&app.mode),
            if app.custom_nix.is_some() {
                " · Custom Nix attached"
            } else {
                ""
            }
        );

        let row = adw::ActionRow::builder()
            .title(gtk::glib::markup_escape_text(&app.attr_path))
            .subtitle(gtk::glib::markup_escape_text(&subtitle))
            .activatable(true)
            .build();

        let icon = gtk::Image::new();
        apply_app_icon(
            &icon,
            model.icon_cache.get(&app.attr_path).map(PathBuf::as_path),
            Category::Other,
            28,
        );
        row.add_prefix(&icon);

        let remove_btn = icon_button(
            "user-trash-symbolic",
            "Uninstall",
            &["destructive-action", "flat"],
        );
        remove_btn.set_valign(gtk::Align::Center);
        {
            let s = sender.clone();
            let attr = app.attr_path.clone();
            remove_btn.connect_clicked(move |_| {
                s.input(AppMsg::QuickUninstall(attr.clone()));
            });
        }
        row.add_suffix(&remove_btn);

        {
            let s = sender.clone();
            let attr = app.attr_path.clone();
            row.connect_activated(move |_| {
                s.input(AppMsg::OpenDetailByAttr(attr.clone()));
            });
        }

        widgets.installed_list.append(&row);
    }

    let preview = render_nix_module(&model.managed_state)
        .unwrap_or_else(|e| format!("# Error rendering module: {e}\n"));
    widgets.installed_preview_buffer.set_text(&preview);
}

fn refresh_setup_tab(model: &NixAppsModel, widgets: &AppWidgets) {
    if let Some(dir) = &model.settings.config_dir {
        widgets
            .setup_folder_row
            .set_subtitle(&dir.display().to_string());
    } else {
        widgets
            .setup_folder_row
            .set_subtitle("Not selected yet (e.g. ~/av-dotfiles or /etc/nixos)");
    }

    let rel = model
        .settings
        .managed_config()
        .map(|c| c.relative_import_path())
        .unwrap_or_else(|| "./modules/nixapps-managed.nix".to_string());
    widgets
        .setup_import_label
        .set_label(&onboarding_import_snippet(&rel, model.settings.target));

    if let Some(info) = &model.sync_info {
        widgets.setup_sync_status_row.set_subtitle(&format!(
            "{} packages indexed from `{}`",
            format_count(info.package_count),
            info.channel
        ));
    } else {
        widgets
            .setup_sync_status_row
            .set_subtitle("No local index yet — click Sync Now to download");
    }

    let _ = (
        &widgets.setup_target_row,
        &widgets.setup_channel_entry,
        &widgets.setup_github_token_entry,
        &widgets.setup_rebuild_cmd_entry,
        &widgets.setup_rebuild_elevation_row,
    );
}

fn populate_detail_view(model: &NixAppsModel, widgets: &AppWidgets) {
    let Some(pkg) = &model.selected_package else {
        return;
    };

    widgets.detail_title.set_label(&pkg.pname);

    let sub = if pkg.version.is_empty() {
        pkg.attr_path.clone()
    } else {
        format!("pkgs.{} · v{}", pkg.attr_path, pkg.version)
    };
    widgets.detail_subtitle.set_label(&sub);
    widgets.detail_subtitle.set_widget_name(&pkg.attr_path);

    if let Some(hp) = &pkg.homepage {
        widgets.detail_homepage_btn.set_visible(true);
        widgets.detail_homepage_btn.set_tooltip_text(Some(hp));
        widgets.detail_homepage_btn.set_widget_name(hp);
    } else {
        widgets.detail_homepage_btn.set_visible(false);
        widgets.detail_homepage_btn.set_widget_name("");
    }

    let mut meta_lines = vec![format!("Attribute: pkgs.{}", pkg.attr_path)];
    if let Some(bin) = &pkg.main_program {
        meta_lines.push(format!("Main binary: {bin}"));
    }
    if let Some(hp) = &pkg.homepage {
        meta_lines.push(format!("Homepage: {hp}"));
    }
    if let Some(pos) = &pkg.position {
        meta_lines.push(format!("Nixpkgs source: {pos}"));
    }
    widgets
        .detail_meta_group
        .set_description(Some(&gtk::glib::markup_escape_text(&meta_lines.join("\n"))));

    if let Some(managed) = model.managed_state.get_app(&pkg.attr_path) {
        let mode_idx = match managed.mode {
            InstallMode::Package => 0,
            InstallMode::PackageWithConfig => 1,
            InstallMode::CustomOnly => 2,
        };
        widgets.detail_mode_dropdown.set_selected(mode_idx);
        widgets
            .detail_nix_buffer
            .set_text(managed.custom_nix.as_deref().unwrap_or(""));
    } else {
        widgets.detail_mode_dropdown.set_selected(0);
        widgets.detail_nix_buffer.set_text("");
    }

    refresh_detail_dynamic_state(model, widgets);
}

fn refresh_detail_dynamic_state(model: &NixAppsModel, widgets: &AppWidgets) {
    let Some(pkg) = &model.selected_package else {
        return;
    };

    let icon_path = model
        .detail_enriched
        .as_ref()
        .and_then(|e| e.icon_path.as_deref())
        .or_else(|| model.icon_cache.get(&pkg.attr_path).map(PathBuf::as_path));
    apply_app_icon(&widgets.detail_icon, icon_path, pkg.category, 64);

    while let Some(child) = widgets.detail_badges_box.first_child() {
        widgets.detail_badges_box.remove(&child);
    }
    let cat_badge = gtk::Label::builder()
        .label(pkg.category.display_name())
        .css_classes(["badge", "badge-category"])
        .build();
    widgets.detail_badges_box.append(&cat_badge);

    if let Some(lic) = &pkg.license {
        let lic_badge = gtk::Label::builder()
            .label(lic)
            .css_classes(["badge", "badge-category"])
            .build();
        widgets.detail_badges_box.append(&lic_badge);
    }
    if let Some(stars) = model.detail_enriched.as_ref().and_then(|e| e.stars) {
        let stars_badge = gtk::Label::builder()
            .label(format_stars(stars))
            .css_classes(["badge", "badge-stars"])
            .build();
        widgets.detail_badges_box.append(&stars_badge);
    }
    if pkg.unfree {
        let unfree_badge = gtk::Label::builder()
            .label("Unfree")
            .css_classes(["badge", "badge-unfree"])
            .build();
        widgets.detail_badges_box.append(&unfree_badge);
    }
    if pkg.broken {
        let broken_badge = gtk::Label::builder()
            .label("Broken")
            .css_classes(["badge", "badge-broken"])
            .build();
        widgets.detail_badges_box.append(&broken_badge);
    }

    let enriched_briefing = model
        .detail_enriched
        .as_ref()
        .and_then(|e| e.briefing.as_deref());
    let fallback_briefing = pkg
        .long_description
        .as_deref()
        .or(pkg.description.as_deref())
        .unwrap_or("No extended description is available for this package in Nixpkgs.");

    if let Some(rich) = enriched_briefing {
        let src = model
            .detail_enriched
            .as_ref()
            .map(|e| e.source.as_str())
            .unwrap_or("cache");
        widgets
            .detail_briefing_status
            .set_label(&format!("Briefing · enriched via {src}"));
        widgets.detail_description.set_label(rich);
    } else if model.detail_loading_meta {
        widgets
            .detail_briefing_status
            .set_label("Briefing · checking Flathub / GitHub for icon & summary…");
        widgets.detail_description.set_label(fallback_briefing);
    } else {
        widgets
            .detail_briefing_status
            .set_label("Briefing · Nixpkgs metadata");
        widgets.detail_description.set_label(fallback_briefing);
    }

    let installed = model.managed_state.is_installed(&pkg.attr_path);
    if installed {
        set_icon_button(
            &widgets.detail_primary_btn,
            "user-trash-symbolic",
            "Uninstall",
        );
        widgets
            .detail_primary_btn
            .remove_css_class("suggested-action");
        widgets
            .detail_primary_btn
            .add_css_class("destructive-action");
    } else {
        set_icon_button(
            &widgets.detail_primary_btn,
            "folder-download-symbolic",
            "Install",
        );
        widgets
            .detail_primary_btn
            .remove_css_class("destructive-action");
        widgets.detail_primary_btn.add_css_class("suggested-action");
    }

    widgets
        .detail_deps_btn
        .set_sensitive(!model.detail_loading_deps);
    while let Some(child) = widgets.detail_deps_box.first_child() {
        widgets.detail_deps_box.remove(&child);
    }

    if model.detail_loading_deps {
        widgets
            .detail_deps_status
            .set_label("Evaluating dependencies via `nix eval`…");
    } else if let Some(err) = &model.detail_deps_error {
        widgets
            .detail_deps_status
            .set_label(&format!("Could not inspect dependencies: {err}"));
    } else if let Some(deps) = &model.detail_deps {
        if deps.is_empty() {
            widgets.detail_deps_status.set_label(
                "No explicit buildInputs / nativeBuildInputs declared on top-level derivation.",
            );
        } else {
            widgets.detail_deps_status.set_label(&format!(
                "{} dependencies evaluated from local Nixpkgs",
                deps.total_count()
            ));
            append_dep_group(&widgets.detail_deps_box, "Build Inputs", &deps.build_inputs);
            append_dep_group(
                &widgets.detail_deps_box,
                "Native Build Inputs",
                &deps.native_build_inputs,
            );
            append_dep_group(
                &widgets.detail_deps_box,
                "Propagated Build Inputs",
                &deps.propagated_build_inputs,
            );
        }
    } else {
        widgets
            .detail_deps_status
            .set_label("Click to inspect real Nix dependencies on demand.");
    }

    if let Some(err) = &model.detail_error {
        widgets.detail_error_label.set_label(err);
        widgets.detail_error_label.set_visible(true);
    } else {
        widgets.detail_error_label.set_visible(false);
    }
}

fn append_dep_group(container: &gtk::Box, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }

    let lbl = gtk::Label::builder()
        .label(format!("{heading} ({})", items.len()))
        .halign(gtk::Align::Start)
        .css_classes(["caption", "heading"])
        .build();
    container.append(&lbl);

    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .row_spacing(6)
        .column_spacing(6)
        .max_children_per_line(8)
        .build();

    for dep in items {
        let chip = gtk::Label::builder()
            .label(dep)
            .css_classes(["dep-chip"])
            .build();
        flow.insert(&chip, -1);
    }
    container.append(&flow);
}
