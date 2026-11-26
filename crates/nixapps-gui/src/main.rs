//! NixApps — Native GTK4 + Libadwaita (Relm4) Desktop Entrypoint.

mod app;
mod settings;
mod ui_helpers;

use relm4::RelmApp;

use crate::app::NixAppsModel;
use crate::ui_helpers::APP_CSS;

fn main() {
    let app = RelmApp::new("io.github.nixapps.NixApps");
    relm4::set_global_css(APP_CSS);
    app.run::<NixAppsModel>(());
}
