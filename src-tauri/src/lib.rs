mod detector;
mod shortcuts;
mod window_mac;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _placeholders = (
        detector::status(),
        window_mac::status(),
        shortcuts::status(),
    );
    let _ = _placeholders;

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running UniWorkspace AI");
}
