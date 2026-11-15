//! Week 1 placeholder for a future macOS window bridge.
//! This module does not attach, parent, or resize other processes.

pub fn status() -> &'static str {
    if cfg!(target_os = "macos") {
        "window_mac: placeholder"
    } else {
        "window_mac: placeholder, inactive off macOS"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_placeholder() {
        assert!(super::status().contains("placeholder"));
    }
}
