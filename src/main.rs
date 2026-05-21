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
    // renderer-software: native Wayland via softbuffer, no glutin/OpenGL
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
        let m = model.clone();
        window.global::<StoreLogic>().on_toggle_select(move |id| {
            m.toggle_select(id.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_select_all_visible(move || {
            m.select_all_visible();
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_clear_selection(move || {
            m.clear_selection();
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_cancel_queue_item(move |id| {
            m.cancel_queue_item(id.as_str());
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_clear_done_queue(move || {
            m.clear_done_queue();
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_dismiss_notification(move || {
            m.dismiss_notification();
        });
    }
    {
        window.global::<StoreLogic>().on_clear_search_history(move || {});
    }

    // ── Async callbacks ───────────────────────────────────────────────────────
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_install_app(move |id| {
            let m = m.clone(); let id = id.to_string();
            tokio::spawn(async move { m.install(&id).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_remove_app(move |id| {
            let m = m.clone(); let id = id.to_string();
            tokio::spawn(async move { m.remove(&id).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_update_app(move |id| {
            let m = m.clone(); let id = id.to_string();
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
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_batch_install(move || {
            let m = m.clone();
            tokio::spawn(async move { m.batch_install().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_batch_remove(move || {
            let m = m.clone();
            tokio::spawn(async move { m.batch_remove().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_batch_update(move || {
            let m = m.clone();
            tokio::spawn(async move { m.batch_update().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_refresh_disk_usage(move || {
            let m = m.clone();
            tokio::spawn(async move { m.refresh_disk_usage().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_run_cleanup(move || {
            let m = m.clone();
            tokio::spawn(async move { m.run_cleanup().await; });
        });
    }
    // v0.5 bootc
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_bootc_upgrade(move || {
            let m = m.clone();
            tokio::spawn(async move { m.bootc_upgrade().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_bootc_rollback(move || {
            let m = m.clone();
            tokio::spawn(async move { m.bootc_rollback().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_bootc_refresh_status(move || {
            let m = m.clone();
            tokio::spawn(async move { m.refresh_bootc_status().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_rpmostree_upgrade(move || {
            let m = m.clone();
            tokio::spawn(async move { m.rpmostree_upgrade().await; });
        });
    }
    // v0.5 remotes
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_refresh_remotes(move || {
            let m = m.clone();
            tokio::spawn(async move { m.refresh_remotes().await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_toggle_remote(move |name| {
            let m = m.clone(); let name = name.to_string();
            tokio::spawn(async move { m.toggle_remote(&name).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_add_remote(move |name, url| {
            let m = m.clone(); let name = name.to_string(); let url = url.to_string();
            tokio::spawn(async move { m.add_remote(&name, &url).await; });
        });
    }
    {
        let m = model.clone();
        window.global::<StoreLogic>().on_remove_remote(move |name| {
            let m = m.clone(); let name = name.to_string();
            tokio::spawn(async move { m.remove_remote(&name).await; });
        });
    }

    window.run()?;
    Ok(())
}
