//! Week 1 placeholder. No global shortcuts are registered.

pub fn status() -> &'static str {
    "shortcuts: placeholder"
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_placeholder() {
        assert!(super::status().contains("placeholder"));
    }
}
