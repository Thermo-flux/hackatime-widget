//! Full widget vs compact pill (Phase 7): the window geometry of switching between them. Pure, so the
//! anchor and clamp rules are unit-tested without a window; `app.rs` only feeds in the monitors.
//!
//! Units: sizes are defined in logical pixels and turned into physical pixels with the scale factor of the
//! monitor the window is on. Positions are worked out (and persisted) in physical pixels, the OS's own
//! desktop coordinates: logical positions are ambiguous when monitors have different scale factors (a
//! 2x monitor right of a 1x one overlaps it in logical space), physical ones never are.

use crate::placement::{self, Rect};
use serde::{Deserialize, Serialize};

/// The full widget's logical size. Must match the window in `tauri.conf.json` (a test checks it).
pub const FULL_SIZE: (f64, f64) = (320.0, 436.0);
/// The compact window: the 220x40 pill plus a 10px margin on every side that holds its shadow.
pub const COMPACT_SIZE: (f64, f64) = (240.0, 60.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Full,
    Compact,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "full" => Some(Mode::Full),
            "compact" => Some(Mode::Compact),
            _ => None,
        }
    }

    pub fn logical_size(self) -> (f64, f64) {
        match self {
            Mode::Full => FULL_SIZE,
            Mode::Compact => COMPACT_SIZE,
        }
    }
}

/// A monitor as far as placement cares: its work area (physical, without the taskbar) and scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub work_area: Rect,
    pub scale: f64,
}

/// A usable scale factor: anything non-finite or non-positive reads as 1.
fn sane(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }
}

/// A logical size in physical pixels at `scale`, rounded, never zero.
pub fn physical_size(logical: (f64, f64), scale: f64) -> (u32, u32) {
    let s = sane(scale);
    let px = |v: f64| ((v * s).round() as u32).max(1);
    (px(logical.0), px(logical.1))
}

/// Physical pixels as logical pixels at `scale`.
pub fn to_logical(px: i64, scale: f64) -> f64 {
    px as f64 / sane(scale)
}

/// The position of a window of `to` size whose top-right corner is where the top-right corner of the
/// window at `pos` of `from` size is now.
pub fn anchor_top_right(pos: (i32, i32), from: (u32, u32), to: (u32, u32)) -> (i32, i32) {
    let x = pos.0 as i64 + from.0 as i64 - to.0 as i64;
    (x.clamp(i32::MIN as i64, i32::MAX as i64) as i32, pos.1)
}

/// Where the window goes, and how big it is, after a switch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub pos: (i32, i32),
    pub size: (u32, u32),
    /// Scale factor of the monitor the window is on.
    pub scale: f64,
    /// How far clamping moved the window from the pure top-right anchor (physical px). The UI uses it to
    /// start the expand animation where the pill really was.
    pub shift: (i32, i32),
}

/// The monitor a window rect is on: the one it overlaps most, else the first (primary).
fn monitor_for(window: &Rect, monitors: &[Monitor]) -> Option<Monitor> {
    let areas: Vec<Rect> = monitors.iter().map(|m| m.work_area).collect();
    placement::area_for(window, &areas).map(|i| monitors[i])
}

/// Switching to `to` from a window currently at `current` (physical outer rect): the new size uses the
/// current monitor's scale factor, the top-right corner stays put, and the result is clamped into that
/// monitor's work area (so expanding near a screen edge never pushes the widget off-screen, and never
/// onto a neighbouring monitor either).
pub fn switch(to: Mode, current: Rect, monitors: &[Monitor]) -> Placement {
    let monitor = monitor_for(&current, monitors);
    let scale = monitor.map_or(1.0, |m| sane(m.scale));
    let size = physical_size(to.logical_size(), scale);
    let anchored = anchor_top_right((current.x, current.y), (current.w, current.h), size);
    let pos = monitor.map_or(anchored, |m| placement::fit_into(anchored, size, &m.work_area));
    Placement { pos, size, scale, shift: (pos.0 - anchored.0, pos.1 - anchored.1) }
}

/// Where a window of mode `mode` goes at launch, given the position remembered for that mode. `None`
/// when nothing is remembered (the OS, or a legacy fallback, decides). A position on a monitor that is
/// gone comes back onto the primary one.
pub fn at_launch(mode: Mode, saved: Option<(i32, i32)>, monitors: &[Monitor]) -> Option<Placement> {
    let pos = saved?;
    // The monitor is found with a 1x-sized window first; its scale then gives the real size.
    let probe = physical_size(mode.logical_size(), 1.0);
    let monitor = monitor_for(&Rect { x: pos.0, y: pos.1, w: probe.0, h: probe.1 }, monitors);
    let scale = monitor.map_or(1.0, |m| sane(m.scale));
    let size = physical_size(mode.logical_size(), scale);
    let fitted = monitor.map_or(pos, |m| placement::fit_into(pos, size, &m.work_area));
    Some(Placement { pos: fitted, size, scale, shift: (fitted.0 - pos.0, fitted.1 - pos.1) })
}

/// Script run before the page on window creation when the app starts compact, so the very first paint is
/// already the pill (the module scripts that set the mode run a little later). It only sets the mode
/// attribute; `mode.js` takes over from there.
pub fn init_script(mode: Mode) -> Option<String> {
    (mode == Mode::Compact).then(|| {
        "(function(){var s=function(){document.documentElement.dataset.mode='compact';};\
         if(document.documentElement)s();else new MutationObserver(function(_,o){if(document.documentElement){s();o.disconnect();}})\
         .observe(document,{childList:true});})();"
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 }; // 1080p minus a 40px taskbar
    fn mon(work_area: Rect, scale: f64) -> Monitor {
        Monitor { work_area, scale }
    }
    fn rect(x: i32, y: i32, (w, h): (u32, u32)) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn modes_parse_and_print() {
        assert_eq!(Mode::parse("full"), Some(Mode::Full));
        assert_eq!(Mode::parse("compact"), Some(Mode::Compact));
        assert_eq!(Mode::parse("Compact"), None);
        assert_eq!(Mode::parse(""), None);
        assert_eq!(Mode::default(), Mode::Full);
        assert_eq!(serde_json::to_string(&Mode::Compact).unwrap(), "\"compact\"");
    }

    #[test]
    fn the_full_size_matches_the_window_in_tauri_conf() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let w = &conf["app"]["windows"][0];
        assert_eq!((w["width"].as_f64().unwrap(), w["height"].as_f64().unwrap()), FULL_SIZE);
        assert_eq!(w["resizable"], false);
    }

    #[test]
    fn sizes_follow_the_scale_factor() {
        assert_eq!(physical_size(COMPACT_SIZE, 1.0), (240, 60));
        assert_eq!(physical_size(COMPACT_SIZE, 1.25), (300, 75));
        assert_eq!(physical_size(COMPACT_SIZE, 1.5), (360, 90));
        assert_eq!(physical_size(COMPACT_SIZE, 2.0), (480, 120));
        assert_eq!(physical_size(FULL_SIZE, 1.25), (400, 545));
        assert_eq!(physical_size(FULL_SIZE, 1.75), (560, 763));
        // Nonsense scale factors read as 1.
        assert_eq!(physical_size(COMPACT_SIZE, 0.0), (240, 60));
        assert_eq!(physical_size(COMPACT_SIZE, f64::NAN), (240, 60));
        assert_eq!(to_logical(360, 1.5), 240.0);
        assert_eq!(to_logical(-300, 1.5), -200.0);
        assert_eq!(to_logical(7, -2.0), 7.0);
    }

    #[test]
    fn the_anchor_keeps_the_top_right_corner() {
        assert_eq!(anchor_top_right((1000, 200), (320, 436), (240, 60)), (1080, 200));
        assert_eq!(anchor_top_right((1080, 200), (240, 60), (320, 436)), (1000, 200));
        assert_eq!(anchor_top_right((-2000, -300), (480, 120), (640, 872)), (-2160, -300));
    }

    #[test]
    fn collapse_then_expand_comes_back_to_the_same_place() {
        let monitors = [mon(PRIMARY, 1.0)];
        let full = rect(1200, 300, (320, 436));
        let c = switch(Mode::Compact, full, &monitors);
        assert_eq!(c, Placement { pos: (1280, 300), size: (240, 60), scale: 1.0, shift: (0, 0) });
        let f = switch(Mode::Full, rect(c.pos.0, c.pos.1, c.size), &monitors);
        assert_eq!(f, Placement { pos: (1200, 300), size: (320, 436), scale: 1.0, shift: (0, 0) });
    }

    #[test]
    fn scale_1_5_and_2_monitors_use_their_own_scale() {
        // 4K at 150%: work area 2560x1400 logical = 3840x2100 physical.
        let m = [mon(Rect { x: 0, y: 0, w: 3840, h: 2100 }, 1.5)];
        let full = rect(2000, 400, physical_size(FULL_SIZE, 1.5)); // 480x654
        let c = switch(Mode::Compact, full, &m);
        assert_eq!((c.pos, c.size, c.scale), ((2120, 400), (360, 90), 1.5)); // 2000 + 480 - 360
        let f = switch(Mode::Full, rect(c.pos.0, c.pos.1, c.size), &m);
        assert_eq!((f.pos, f.size), ((2000, 400), (480, 654)));

        let m2 = [mon(Rect { x: 0, y: 0, w: 2880, h: 1720 }, 2.0)];
        let c = switch(Mode::Compact, rect(100, 100, (640, 872)), &m2);
        assert_eq!((c.pos, c.size), ((260, 100), (480, 120)));
    }

    #[test]
    fn expanding_near_each_edge_stays_on_screen() {
        let monitors = [mon(PRIMARY, 1.0)];
        let pill = (240, 60);
        // Left edge: the full window would start 80px off-screen.
        let f = switch(Mode::Full, rect(0, 300, pill), &monitors);
        assert_eq!((f.pos, f.shift), ((0, 300), (80, 0)));
        // Right edge: the anchor keeps the right edge, which is already inside.
        let f = switch(Mode::Full, rect(1680, 300, pill), &monitors);
        assert_eq!((f.pos, f.shift), ((1600, 300), (0, 0)));
        // Bottom edge (just above the taskbar): pushed up so the 436px window fits.
        let f = switch(Mode::Full, rect(800, 980, pill), &monitors);
        assert_eq!((f.pos, f.shift), ((720, 604), (0, -376)));
        // Top edge, and a pill that was dragged partly above the screen.
        let f = switch(Mode::Full, rect(800, -20, pill), &monitors);
        assert_eq!((f.pos, f.shift), ((720, 0), (0, 20)));
        // Bottom-left corner: both axes move.
        let f = switch(Mode::Full, rect(10, 1000, pill), &monitors);
        assert_eq!((f.pos, f.shift), ((0, 604), (70, -396)));
    }

    #[test]
    fn collapsing_a_window_partly_off_screen_brings_the_pill_back() {
        let monitors = [mon(PRIMARY, 1.0)];
        // Full window dragged so its right part is past the right edge: the pill (its top-right) is pulled in.
        let c = switch(Mode::Compact, rect(1800, 100, (320, 436)), &monitors);
        assert_eq!((c.pos, c.shift), ((1680, 100), (-200, 0)));
    }

    #[test]
    fn a_secondary_monitor_left_of_the_primary_has_negative_coordinates() {
        // Secondary at 125% to the left of the primary: x from -2400 to 0.
        let left = Rect { x: -2400, y: 0, w: 2400, h: 1300 };
        let monitors = [mon(PRIMARY, 1.0), mon(left, 1.25)];
        let full = rect(-1000, 200, physical_size(FULL_SIZE, 1.25)); // 400x545
        let c = switch(Mode::Compact, full, &monitors);
        assert_eq!((c.pos, c.size, c.scale), ((-900, 200), (300, 75), 1.25));
        // Expanding at the secondary's left edge clamps into the secondary, not the primary.
        let f = switch(Mode::Full, rect(-2400, 50, (300, 75)), &monitors);
        assert_eq!((f.pos, f.size, f.shift), ((-2400, 50), (400, 545), (100, 0)));
        // Expanding at the secondary's right edge stays on the secondary (the anchor keeps the right edge).
        let f = switch(Mode::Full, rect(-300, 50, (300, 75)), &monitors);
        assert_eq!((f.pos, f.shift), ((-400, 50), (0, 0)));
    }

    #[test]
    fn a_pill_straddling_two_monitors_expands_on_the_one_it_mostly_covers() {
        let right = Rect { x: 1920, y: 0, w: 2560, h: 1400 };
        let monitors = [mon(PRIMARY, 1.0), mon(right, 2.0)];
        // Mostly on the 2x monitor: full size at 2x, clamped into that monitor.
        let f = switch(Mode::Full, rect(1900, 100, (240, 60)), &monitors);
        assert_eq!((f.size, f.scale, f.pos), ((640, 872), 2.0, (1920, 100)));
        // Mostly on the 1x primary.
        let f = switch(Mode::Full, rect(1700, 100, (240, 60)), &monitors);
        assert_eq!((f.size, f.scale, f.pos), ((320, 436), 1.0, (1600, 100)));
    }

    #[test]
    fn a_work_area_smaller_than_the_window_pins_it_top_left() {
        let tiny = Rect { x: 100, y: 50, w: 300, h: 400 };
        let f = switch(Mode::Full, rect(200, 100, (240, 60)), &[mon(tiny, 1.0)]);
        assert_eq!((f.pos, f.size), ((100, 50), (320, 436)));
    }

    #[test]
    fn no_monitors_means_pure_anchoring_at_1x() {
        let c = switch(Mode::Compact, rect(10, 10, (320, 436)), &[]);
        assert_eq!(c, Placement { pos: (90, 10), size: (240, 60), scale: 1.0, shift: (0, 0) });
    }

    #[test]
    fn switching_to_the_same_mode_changes_nothing() {
        let monitors = [mon(PRIMARY, 1.0)];
        let c = switch(Mode::Compact, rect(500, 500, (240, 60)), &monitors);
        assert_eq!((c.pos, c.size), ((500, 500), (240, 60)));
    }

    #[test]
    fn launch_uses_the_saved_position_and_that_monitors_scale() {
        let left = Rect { x: -2880, y: 0, w: 2880, h: 1700 };
        let monitors = [mon(PRIMARY, 1.0), mon(left, 1.5)];
        assert_eq!(at_launch(Mode::Compact, None, &monitors), None);
        let p = at_launch(Mode::Compact, Some((-500, 300)), &monitors).unwrap();
        assert_eq!((p.pos, p.size, p.scale), ((-500, 300), (360, 90), 1.5));
        let p = at_launch(Mode::Full, Some((1700, 900)), &monitors).unwrap();
        assert_eq!((p.pos, p.size), ((1600, 604), (320, 436)));
        // The saved monitor is gone: back onto the primary.
        let p = at_launch(Mode::Compact, Some((5000, 300)), &[mon(PRIMARY, 1.0)]).unwrap();
        assert_eq!((p.pos, p.size), ((1680, 300), (240, 60)));
    }

    #[test]
    fn only_compact_launches_get_an_init_script() {
        assert!(init_script(Mode::Full).is_none());
        assert!(init_script(Mode::Compact).unwrap().contains("dataset.mode='compact'"));
    }
}
