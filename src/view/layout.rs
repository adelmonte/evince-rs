use crate::document::Rect;
use crate::document::misc::{rotate_rect, unrotate_point};

pub const SPACING: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

pub const BORDER: Border = Border { left: 4.0, top: 3.0, right: 3.0, bottom: 6.0 };

impl Border {
    pub fn h(&self) -> f64 {
        self.left + self.right
    }
    pub fn v(&self) -> f64 {
        self.top + self.bottom
    }
}

/// Position of a page's content area (inside its border) on the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageBox {
    pub page: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl PageBox {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub boxes: Vec<PageBox>,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub rotation: i32,
}

pub struct Params<'a> {
    /// Unrotated page sizes in points.
    pub sizes: &'a [(f64, f64)],
    pub scale: f64,
    pub rotation: i32,
    pub continuous: bool,
    pub dual: bool,
    pub odd_left: bool,
    pub rtl: bool,
    pub current: usize,
    pub viewport: (f64, f64),
}

pub fn page_pixel_size(size: (f64, f64), scale: f64, rotation: i32) -> (f64, f64) {
    let w = (size.0 * scale + 0.5).floor();
    let h = (size.1 * scale + 0.5).floor();
    if rotation == 90 || rotation == 270 { (h, w) } else { (w, h) }
}

/// Pages shown side by side in dual mode: returns (left, right) for the row containing `page`.
pub fn dual_row(page: usize, n: usize, odd_left: bool) -> (Option<usize>, Option<usize>) {
    if odd_left {
        let l = page - page % 2;
        (Some(l), (l + 1 < n).then_some(l + 1))
    } else if page == 0 {
        (None, Some(0))
    } else {
        let l = page - (page + 1) % 2;
        (Some(l), (l + 1 < n).then_some(l + 1))
    }
}

fn row_start_pages(n: usize, odd_left: bool) -> Vec<usize> {
    let mut v = Vec::new();
    let mut p = 0;
    if !odd_left && n > 0 {
        v.push(0);
        p = 1;
    }
    while p < n {
        v.push(p);
        p += 2;
    }
    v
}

pub fn compute(p: &Params) -> Layout {
    let n = p.sizes.len();
    let b = BORDER;
    let mut layout = Layout { scale: p.scale, rotation: p.rotation, ..Default::default() };
    if n == 0 {
        return layout;
    }
    let px: Vec<(f64, f64)> = p.sizes.iter().map(|s| page_pixel_size(*s, p.scale, p.rotation)).collect();
    let (vw, vh) = p.viewport;
    let current = p.current.min(n - 1);

    if !p.dual {
        let pages: Vec<usize> = if p.continuous { (0..n).collect() } else { vec![current] };
        let max_w = pages.iter().map(|&i| px[i].0).fold(0.0, f64::max);
        let width = max_w + b.h() + 2.0 * SPACING;
        let canvas_w = width.max(vw);
        let mut y = SPACING;
        for &i in &pages {
            let (w, h) = px[i];
            layout.boxes.push(PageBox { page: i, x: ((canvas_w - (w + b.h())) / 2.0).floor() + b.left, y: y + b.top, w, h });
            y += h + b.v() + SPACING;
        }
        layout.width = width;
        layout.height = y;
        if !p.continuous {
            let ch = layout.height.max(vh);
            if let Some(bx) = layout.boxes.first_mut() {
                bx.y = ((ch - (bx.h + b.v())) / 2.0).floor() + b.top;
            }
        }
        return layout;
    }

    let max_w = if p.continuous {
        px.iter().map(|s| s.0).fold(0.0, f64::max)
    } else {
        let (l, r) = dual_row(current, n, p.odd_left);
        [l, r].iter().flatten().map(|&i| px[i].0).fold(0.0, f64::max)
    };
    let col = max_w + b.h();
    let width = 2.0 * col + 3.0 * SPACING;
    let ox = ((vw - width).max(0.0) / 2.0).floor();
    let rows: Vec<usize> = if p.continuous {
        row_start_pages(n, p.odd_left)
    } else {
        let (l, r) = dual_row(current, n, p.odd_left);
        vec![l.or(r).unwrap()]
    };
    let mut y = SPACING;
    for start in rows {
        let (l, r) = dual_row(start, n, p.odd_left);
        let row_h = [l, r].iter().flatten().map(|&i| px[i].1).fold(0.0, f64::max) + b.v();
        let place = |page: usize, left_slot: bool, layout: &mut Layout| {
            let (w, h) = px[page];
            let left_slot = if p.rtl { !left_slot } else { left_slot };
            let x = if left_slot { ox + SPACING + (col - (w + b.h())) } else { ox + 2.0 * SPACING + col };
            let yy = y + ((row_h - (h + b.v())) / 2.0).floor();
            layout.boxes.push(PageBox { page, x: x + b.left, y: yy + b.top, w, h });
        };
        if let Some(l) = l {
            place(l, true, &mut layout);
        }
        if let Some(r) = r {
            place(r, false, &mut layout);
        }
        y += row_h + SPACING;
    }
    layout.width = width;
    layout.height = y;
    if !p.continuous {
        let ch = layout.height.max(vh);
        let off = ((ch - layout.height) / 2.0).floor();
        for bx in &mut layout.boxes {
            bx.y += off;
        }
    }
    layout
}

impl Layout {
    pub fn page_box(&self, page: usize) -> Option<&PageBox> {
        self.boxes.iter().find(|b| b.page == page)
    }

    pub fn box_at(&self, x: f64, y: f64) -> Option<&PageBox> {
        self.boxes.iter().find(|b| b.contains(x, y))
    }

    /// Nearest page box to a canvas point (used while dragging a selection outside pages).
    pub fn nearest_box(&self, x: f64, y: f64) -> Option<&PageBox> {
        self.boxes.iter().min_by(|a, b| {
            let d = |bx: &PageBox| {
                let dx = if x < bx.x { bx.x - x } else if x > bx.x + bx.w { x - bx.x - bx.w } else { 0.0 };
                let dy = if y < bx.y { bx.y - y } else if y > bx.y + bx.h { y - bx.y - bx.h } else { 0.0 };
                dx * dx + dy * dy
            };
            d(a).partial_cmp(&d(b)).unwrap()
        })
    }

    /// Converts a rectangle in unrotated page points to canvas coordinates.
    pub fn page_to_canvas(&self, bx: &PageBox, size: (f64, f64), r: Rect) -> Rect {
        let s = self.scale;
        let scaled = Rect::new(r.x1 * s, r.y1 * s, r.x2 * s, r.y2 * s);
        let rr = rotate_rect(scaled, size.0 * s, size.1 * s, self.rotation);
        Rect::new(bx.x + rr.x1, bx.y + rr.y1, bx.x + rr.x2, bx.y + rr.y2)
    }

    /// Converts a canvas point to unrotated page points of the page `bx`.
    pub fn canvas_to_page(&self, bx: &PageBox, size: (f64, f64), x: f64, y: f64) -> (f64, f64) {
        let s = self.scale;
        let (px, py) = unrotate_point(x - bx.x, y - bx.y, size.0 * s, size.1 * s, self.rotation);
        (px / s, py / s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params<'a>(sizes: &'a [(f64, f64)], continuous: bool, dual: bool, odd_left: bool) -> Params<'a> {
        Params {
            sizes,
            scale: 1.0,
            rotation: 0,
            continuous,
            dual,
            odd_left,
            rtl: false,
            current: 0,
            viewport: (800.0, 600.0),
        }
    }

    #[test]
    fn continuous_single() {
        let sizes = vec![(100.0, 200.0); 3];
        let l = compute(&params(&sizes, true, false, false));
        assert_eq!(l.boxes.len(), 3);
        assert_eq!(l.boxes[0].y, SPACING + BORDER.top);
        assert_eq!(l.boxes[1].y, l.boxes[0].y + 200.0 + BORDER.v() + SPACING);
        assert!(l.boxes.iter().all(|b| b.x == l.boxes[0].x));
        assert_eq!(l.height, SPACING + 3.0 * (200.0 + BORDER.v() + SPACING));
    }

    #[test]
    fn dual_rows() {
        assert_eq!(dual_row(0, 5, false), (None, Some(0)));
        assert_eq!(dual_row(1, 5, false), (Some(1), Some(2)));
        assert_eq!(dual_row(2, 5, false), (Some(1), Some(2)));
        assert_eq!(dual_row(4, 5, false), (Some(3), Some(4)));
        assert_eq!(dual_row(0, 5, true), (Some(0), Some(1)));
        assert_eq!(dual_row(4, 5, true), (Some(4), None));
        let sizes = vec![(100.0, 200.0); 4];
        let l = compute(&params(&sizes, true, true, false));
        assert_eq!(l.boxes.len(), 4);
        let b0 = l.page_box(0).unwrap();
        let b1 = l.page_box(1).unwrap();
        let b2 = l.page_box(2).unwrap();
        assert!(b0.x > b1.x, "first page alone on the right");
        assert_eq!(b1.y, b2.y);
        assert!(b2.x > b1.x);
    }

    #[test]
    fn rtl_swaps_sides() {
        let sizes = vec![(100.0, 200.0); 2];
        let mut p = params(&sizes, true, true, true);
        p.rtl = true;
        let l = compute(&p);
        assert!(l.page_box(0).unwrap().x > l.page_box(1).unwrap().x);
    }

    #[test]
    fn single_page_centered() {
        let sizes = vec![(100.0, 200.0); 3];
        let mut p = params(&sizes, false, false, false);
        p.current = 2;
        let l = compute(&p);
        assert_eq!(l.boxes.len(), 1);
        assert_eq!(l.boxes[0].page, 2);
        assert!(l.boxes[0].y > 100.0, "vertically centered in tall viewport");
    }

    #[test]
    fn coordinate_roundtrip_rotated() {
        let sizes = vec![(100.0, 200.0)];
        let mut p = params(&sizes, true, false, false);
        p.rotation = 90;
        p.scale = 2.0;
        let l = compute(&p);
        let b = l.boxes[0];
        assert_eq!((b.w, b.h), (400.0, 200.0));
        let r = l.page_to_canvas(&b, sizes[0], Rect::new(10.0, 20.0, 10.0, 20.0));
        let (x, y) = l.canvas_to_page(&b, sizes[0], r.x1, r.y1);
        assert!((x - 10.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9);
    }
}
