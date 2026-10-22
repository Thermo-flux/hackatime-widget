//! Keeps the widget on a screen that exists: a remembered position can belong to a monitor that
//! has since been unplugged. Pure geometry (physical pixels), so it is unit-tested without a window.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    fn right(&self) -> i64 {
        self.x as i64 + self.w as i64
    }
    fn bottom(&self) -> i64 {
        self.y as i64 + self.h as i64
    }

    fn overlap(&self, other: &Rect) -> i64 {
        let w = self.right().min(other.right()) - (self.x as i64).max(other.x as i64);
        let h = self.bottom().min(other.bottom()) - (self.y as i64).max(other.y as i64);
        if w > 0 && h > 0 { w * h } else { 0 }
    }
}

/// Index of the area `window` overlaps most; if it overlaps none, the first (the primary monitor).
pub fn area_for(window: &Rect, areas: &[Rect]) -> Option<usize> {
    let best = areas.iter().enumerate().filter(|(_, a)| window.overlap(a) > 0).max_by_key(|(_, a)| window.overlap(a));
    best.map(|(i, _)| i).or(if areas.is_empty() { None } else { Some(0) })
}

/// Moves a window of `size` at `pos` the least it takes to be fully inside `area`.
/// A window larger than the area is pinned to the area's top-left (the title and close button stay reachable).
pub fn fit_into(pos: (i32, i32), size: (u32, u32), area: &Rect) -> (i32, i32) {
    let fit = |v: i32, start: i32, room: u32, len: u32| {
        let max = start as i64 + room as i64 - len as i64;
        (v as i64).min(max).max(start as i64) as i32
    };
    (fit(pos.0, area.x, area.w, size.0), fit(pos.1, area.y, area.h, size.1))
}

/// Where a window of `size` at `pos` should sit so it is fully inside one of the `areas` (work areas,
/// the first being the primary monitor). The area it overlaps most is used; if it overlaps none, the
/// first. A window larger than the area is pinned to the area's top-left.
pub fn clamp_into(pos: (i32, i32), size: (u32, u32), areas: &[Rect]) -> (i32, i32) {
    let window = Rect { x: pos.0, y: pos.1, w: size.0, h: size.1 };
    match area_for(&window, areas) {
        Some(i) => fit_into(pos, size, &areas[i]),
        None => pos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rect = Rect { x: 0, y: 0, w: 1920, h: 1040 };
    const B: Rect = Rect { x: 1920, y: 0, w: 1280, h: 1024 };

    #[test]
    fn a_position_already_on_screen_is_kept() {
        assert_eq!(clamp_into((100, 80), (320, 436), &[A, B]), (100, 80));
    }

    #[test]
    fn a_window_on_an_unplugged_monitor_comes_back_to_the_first() {
        assert_eq!(clamp_into((2400, 100), (320, 436), &[A]), (1600, 100));
        assert_eq!(clamp_into((-3000, -500), (320, 436), &[A]), (0, 0));
    }

    #[test]
    fn a_window_hanging_off_an_edge_is_pulled_back_in() {
        assert_eq!(clamp_into((1800, 900), (320, 436), &[A]), (1600, 604));
    }

    #[test]
    fn the_monitor_it_mostly_overlaps_wins() {
        // Mostly on B, partly on A: stays on B.
        assert_eq!(clamp_into((1900, 100), (320, 436), &[A, B]), (1920, 100));
        // Mostly on A.
        assert_eq!(clamp_into((1700, 100), (320, 436), &[A, B]), (1600 + 0, 100));
    }

    #[test]
    fn area_for_prefers_most_overlap_then_the_first() {
        assert_eq!(area_for(&Rect { x: 1900, y: 0, w: 100, h: 10 }, &[A, B]), Some(1));
        assert_eq!(area_for(&Rect { x: 9000, y: 0, w: 100, h: 10 }, &[A, B]), Some(0));
        assert_eq!(area_for(&Rect { x: 0, y: 0, w: 1, h: 1 }, &[]), None);
    }

    #[test]
    fn fit_into_one_area_ignores_the_others() {
        // Hanging off B's left edge into A: pulled into B, not A.
        assert_eq!(fit_into((1800, 10), (320, 436), &B), (1920, 10));
    }

    #[test]
    fn no_monitors_or_a_huge_window_do_not_panic() {
        assert_eq!(clamp_into((5, 5), (320, 436), &[]), (5, 5));
        assert_eq!(clamp_into((50, 50), (4000, 3000), &[A]), (0, 0));
    }
}
