mod app_data;
mod bootc;
mod catalog;
mod flatpak;
mod store_model;

use slint::ComponentHandle;
use store_model::StoreModel;

slint::include_modules!();

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let window = MainWindow::new()?;
    let model  = StoreModel::new(window.as_weak());
    model.init();

    // ── Sync callbacks ────────────────────────────────────────────────────────

    {
        let m = model.clone();
        window.global::<StoreLogic>().on_search_changed(move |q| {
            m.filter_search(q.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_category_changed(move |c| {
            m.filter_category(c.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_sort_changed(move |s| {
            m.set_sort(s.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_suggestion_selected(move |id| {
            m.suggestion_selected(id.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_open_app_detail(move |id| {
            m.open_detail(id.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_load_more(move || {
            m.load_more();
        });
    }
    {
        window.global::<StoreLogic>().on_clear_search_history(move || {
            // v0.3: persist & clear history
        });
    }

    // ── Async callbacks ───────────────────────────────────────────────────────

    {
        let m = model.clone();
        window.global::<StoreLogic>().on_install_app(move |id| {
            let m = m.clone();
            let id = id.to_string();
            tokio::spawn(async move { m.install(&id).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_remove_app(move |id| {
            let m = m.clone();
            let id = id.to_string();
            tokio::spawn(async move { m.remove(&id).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_update_app(move |id| {
            let m = m.clone();
            let id = id.to_string();
            tokio::spawn(async move { m.update_app(&id).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_update_all(move || {
            let m = m.clone();
            tokio::spawn(async move { m.update_all().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_refresh_installed(move || {
            let m = m.clone();
            tokio::spawn(async move { m.refresh_installed().await; });
        });
    }

    window.run()?;
    Ok(())
}
