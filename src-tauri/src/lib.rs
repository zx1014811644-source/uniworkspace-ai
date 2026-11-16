mod detector;
mod partition;
#[cfg(target_os = "macos")]
mod partition_mac;
mod shortcuts;
mod snap;
#[cfg(target_os = "macos")]
mod snap_mac;
mod window_mac;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _placeholders = (
        detector::status(),
        window_mac::status(),
        shortcuts::status(),
        snap::status(),
    );
    let _ = _placeholders;

    let builder = tauri::Builder::default()
        .setup(|app| {
            snap::recover_orphans(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            partition::load_workspace,
            partition::save_workspace,
            partition::sync_panes,
            partition::focus_partition,
            partition::hide_partitions,
            partition::probe_partitions,
            detector::desktop_scan,
            detector::a11y_status,
            detector::a11y_request,
            detector::a11y_open_settings,
            detector::a11y_choose,
            snap::snap_attach,
            snap::snap_update,
            snap::snap_detach,
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    snap::restore_for_exit();
                    window.app_handle().exit(0);
                }
            }
        });

    // Tauri 2.12 exposes this per webview on the app builder. The handler
    // receives the webview whose content process died, so one pane can be
    // rebuilt without reloading the others.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        partition_mac::handle_content_process_terminated(webview);
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running UniWorkspace AI");
}
