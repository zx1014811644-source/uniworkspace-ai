//! macOS window bridge.
//!
//! Week 2 does not snap, follow, or reparent other apps. The notes below are the
//! research record for that later work. Calling Accessibility from here would
//! raise the system permission prompt, so this module still does not touch AX.

pub fn status() -> &'static str {
    if cfg!(target_os = "macos") {
        "window_mac: placeholder; a11y and snap research only"
    } else {
        "window_mac: placeholder, inactive off macOS"
    }
}

// Week 4+ geometric snap, not implemented:
//
// The content/split placeholder is a rectangle in this window, the same kind of
// bounds the partition webviews already receive from the shell. It should be an
// NSView frame inside the existing opaque window, not a transparent window and
// not a hole that shows another process behind us. Transparency of the whole
// shell would fight the custom title bar and the WKWebViews.
//
// Reading another app's frame can use CGWindowListCopyWindowInfo without the
// Accessibility prompt. Moving or resizing that app cannot. That needs
// AXIsProcessTrustedWithOptions (kAXTrustedCheckOptionPrompt) and then
// AXUIElementSetAttributeValue for kAXPositionAttribute / kAXSizeAttribute.
// AX coordinates are top-left; AppKit screen coordinates are bottom-left.
//
// Follow the placeholder only while a snap is active, from CVDisplayLink (or
// the newer CADisplayLink) onto the main queue. Stop the link when nothing is
// snapped. Stay in this file with objc2 unless a private SkyLight call forces
// a Swift helper. Do not prompt for Accessibility in Week 2.
//
// See docs/week2-a11y-placeholder-research.md.

#[cfg(test)]
mod tests {
    #[test]
    fn reports_placeholder() {
        assert!(super::status().contains("placeholder"));
    }
}
