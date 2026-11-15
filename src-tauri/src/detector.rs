//! Week 1 placeholder. Installed-app detection is not implemented.

pub fn status() -> &'static str {
    "detector: placeholder"
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_placeholder() {
        assert!(super::status().contains("placeholder"));
    }
}
