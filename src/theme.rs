//! Theme, sync-mode and zoom state (pure logic).
//!
//! `Theme` is the user's choice (light/dark/system); `resolve` maps it to an
//! effective light/dark using OS detection for `System`. `SyncMode` is the
//! Zed-linkage policy (state only for now; full wiring lands in a later chunk).
//! Zoom helpers clamp the body scale to a sane range.

use serde::{Deserialize, Serialize};

/// The user's theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

/// An effective, resolved appearance (no `System`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
}

impl Theme {
    /// Resolve to a concrete appearance. `System` queries the OS; on any
    /// detection failure it falls back to light.
    pub fn resolve(self) -> Appearance {
        match self {
            Theme::Light => Appearance::Light,
            Theme::Dark => Appearance::Dark,
            Theme::System => match dark_light::detect() {
                Ok(dark_light::Mode::Dark) => Appearance::Dark,
                _ => Appearance::Light,
            },
        }
    }
}

/// Zed-linkage policy. `Auto` follows Zed's focused project, `Self` pins the
/// current project, `Off` ignores Zed entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMode {
    #[default]
    Auto,
    #[serde(rename = "self")]
    SelfPinned,
    Off,
}

/// What the app should do with a Zed project-switch notification, given the
/// active [`SyncMode`]. Decided purely so it can be unit-tested apart from the
/// async watch loop in `main.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncDecision {
    /// Replace the sidebar root(s) with the switched-to project.
    pub update_root: bool,
    /// Open the project's representative markdown in a tab.
    pub open_markdown: bool,
}

impl SyncMode {
    /// Decide how to react to a Zed project switch.
    ///
    /// - `Auto`: follow fully — swap root and open a representative md.
    /// - `SelfPinned`: update the sidebar root but do not steal the active tab.
    /// - `Off`: ignore the switch entirely.
    pub fn decide(self) -> SyncDecision {
        match self {
            SyncMode::Auto => SyncDecision {
                update_root: true,
                open_markdown: true,
            },
            SyncMode::SelfPinned => SyncDecision {
                update_root: true,
                open_markdown: false,
            },
            SyncMode::Off => SyncDecision {
                update_root: false,
                open_markdown: false,
            },
        }
    }
}

/// Zoom clamp range for the markdown body.
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 2.0;
pub const ZOOM_STEP: f32 = 0.1;
pub const ZOOM_DEFAULT: f32 = 1.0;

/// Increase zoom by one step, clamped.
pub fn zoom_in(z: f32) -> f32 {
    (z + ZOOM_STEP).min(ZOOM_MAX)
}

/// Decrease zoom by one step, clamped.
pub fn zoom_out(z: f32) -> f32 {
    (z - ZOOM_STEP).max(ZOOM_MIN)
}

#[cfg(test)]
#[allow(non_snake_case)] // Japanese test names may embed ASCII.
mod tests {
    use super::*;

    #[test]
    fn light_darkは固定で解決する() {
        assert_eq!(Theme::Light.resolve(), Appearance::Light);
        assert_eq!(Theme::Dark.resolve(), Appearance::Dark);
    }

    #[test]
    fn zoom_inは上限で頭打ちになる() {
        let z = zoom_in(ZOOM_MAX);
        assert!((z - ZOOM_MAX).abs() < f32::EPSILON);
    }

    #[test]
    fn zoom_outは下限で頭打ちになる() {
        let z = zoom_out(ZOOM_MIN);
        assert!((z - ZOOM_MIN).abs() < f32::EPSILON);
    }

    #[test]
    fn autoはrootもmdも更新する() {
        let d = SyncMode::Auto.decide();
        assert!(d.update_root);
        assert!(d.open_markdown);
    }

    #[test]
    fn selfはrootだけ更新しmdは開かない() {
        let d = SyncMode::SelfPinned.decide();
        assert!(d.update_root);
        assert!(!d.open_markdown);
    }

    #[test]
    fn offは何も更新しない() {
        let d = SyncMode::Off.decide();
        assert!(!d.update_root);
        assert!(!d.open_markdown);
    }

    #[test]
    fn zoomは1ステップ増減する() {
        let z = zoom_in(1.0);
        assert!((z - 1.1).abs() < 1e-6);
        let z2 = zoom_out(1.0);
        assert!((z2 - 0.9).abs() < 1e-6);
    }
}
