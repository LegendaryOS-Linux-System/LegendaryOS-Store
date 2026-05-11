// src/main.rs — LegendaryOS Store
// Rust backend: data model, Flatpak integration, Slint bridge

mod app_data;
mod flatpak;
mod store_model;

use slint::ComponentHandle;
use store_model::StoreModel;

slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build the Slint window
    let window = MainWindow::new()?;

    // Initialise model
    let model = StoreModel::new(window.as_weak());
    model.init();

    // Wire up callbacks from Slint → Rust
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_install_app(move |flatpak_id| {
            let m = m.clone();
            let id = flatpak_id.to_string();
            tokio::spawn(async move {
                m.install(&id).await;
            });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_remove_app(move |flatpak_id| {
            let m = m.clone();
            let id = flatpak_id.to_string();
            tokio::spawn(async move {
                m.remove(&id).await;
            });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_search_changed(move |query| {
            m.filter_search(query.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_category_changed(move |cat| {
            m.filter_category(cat.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_refresh_installed(move || {
            let m = m.clone();
            tokio::spawn(async move {
                m.refresh_installed().await;
            });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_open_app_detail(move |id| {
            m.open_detail(id.as_str());
        });
    }

    window.run()?;
    Ok(())
}
