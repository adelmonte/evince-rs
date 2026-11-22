pub mod jobs;
pub mod layout;
pub mod model;
pub mod presentation;
pub mod print;
mod view;

pub use jobs::{JobHandle, Jobs, Priority};
pub use model::{Change, DocHandle, Model, SizingMode};
pub use view::{AddMode, PageData, PopupContext, View, ViewEvent};

pub const ZOOM_IN_FACTOR: f64 = 1.2;
pub const ZOOM_OUT_FACTOR: f64 = 1.0 / 1.2;
pub const MIN_SCALE: f64 = 0.05409;

pub const ZOOM_LEVELS: &[(&str, f64)] = &[
    ("50%", 0.5),
    ("70%", 0.7071067811),
    ("85%", 0.8408964152),
    ("100%", 1.0),
    ("125%", 1.1892071149),
    ("150%", 1.4142135623),
    ("175%", 1.6817928304),
    ("200%", 2.0),
    ("300%", 2.8284271247),
    ("400%", 4.0),
    ("800%", 8.0),
    ("1600%", 16.0),
    ("3200%", 32.0),
    ("6400%", 64.0),
];

use gtk::prelude::*;

/// Screen resolution used to map 100% zoom to the physical size of the page.
pub fn widget_dpi(widget: &impl IsA<gtk::Widget>) -> f64 {
    let w = widget.as_ref();
    let display = w.display();
    let monitor = w.window().and_then(|win| display.monitor_at_window(&win)).or_else(|| display.monitor(0));
    if let Some(m) = monitor {
        let geom = m.geometry();
        let wmm = m.width_mm();
        let hmm = m.height_mm();
        if wmm > 0 && hmm > 0 {
            let dx = geom.width() as f64 / (wmm as f64 / 25.4);
            let dy = geom.height() as f64 / (hmm as f64 / 25.4);
            let dpi = (dx + dy) / 2.0;
            if (50.0..=500.0).contains(&dpi) {
                return dpi;
            }
        }
    }
    96.0
}

/// Maximum zoom so that a rendered page still fits in the page cache (bytes).
pub fn max_scale_for_cache(max_page: (f64, f64), cache_bytes: f64) -> f64 {
    let area = (max_page.0 * max_page.1).max(1.0);
    (cache_bytes / (4.0 * area)).sqrt().max(1.0)
}
