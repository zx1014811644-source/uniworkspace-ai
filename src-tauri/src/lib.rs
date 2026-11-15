mod detector;
mod partition;
#[cfg(target_os = "macos")]
mod partition_mac;
mod shortcuts;
mod window_mac;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _placeholders = (
        detector::status(),
        window_mac::status(),
        shortcuts::status(),
    );
    let _ = _placeholders;

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            partition::show_partition,
            partition::set_partition_bounds,
            partition::hide_partitions,
            partition::probe_partitions,
        ])
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    window.app_handle().exit(0);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running UniWorkspace AI");
}
