use super::{FindMatch, FindOptions, License, Rect, SelectionStyle, TextLayout};

pub fn transformed_size(w: f64, h: f64, scale: f64, rotation: i32) -> (i32, i32) {
    let pw = (w * scale + 0.5) as i32;
    let ph = (h * scale + 0.5) as i32;
    if rotation == 90 || rotation == 270 { (ph, pw) } else { (pw, ph) }
}

/// Sets up `cr` so that drawing in unrotated page space ends up rotated in a surface
/// of size `sw` x `sh` (already rotated dimensions).
pub fn apply_rotation(cr: &cairo::Context, sw: f64, sh: f64, rotation: i32) {
    match rotation {
        90 => {
            cr.translate(sw, 0.0);
        }
        180 => {
            cr.translate(sw, sh);
        }
        270 => {
            cr.translate(0.0, sh);
        }
        _ => {}
    }
    cr.rotate((rotation as f64).to_radians());
}

/// Scales and rotates a raster image surface into a new surface for the given page size.
pub fn scale_and_rotate(src: &cairo::ImageSurface, scale_w: i32, scale_h: i32, rotation: i32) -> Option<cairo::ImageSurface> {
    let (dw, dh) = if rotation == 90 || rotation == 270 { (scale_h, scale_w) } else { (scale_w, scale_h) };
    let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, dw.max(1), dh.max(1)).ok()?;
    {
        let cr = cairo::Context::new(&surf).ok()?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().ok()?;
        apply_rotation(&cr, dw as f64, dh as f64, rotation);
        let sx = scale_w as f64 / src.width().max(1) as f64;
        let sy = scale_h as f64 / src.height().max(1) as f64;
        cr.scale(sx, sy);
        cr.set_source_surface(src, 0.0, 0.0).ok()?;
        let pat = cr.source();
        let filter = if sx < 1.0 || sy < 1.0 { cairo::Filter::Good } else { cairo::Filter::Bilinear };
        pat.set_filter(filter);
        cr.paint().ok()?;
    }
    Some(surf)
}

pub fn rotate_rect(r: Rect, pw: f64, ph: f64, rotation: i32) -> Rect {
    match rotation {
        90 => Rect::new(ph - r.y2, r.x1, ph - r.y1, r.x2),
        180 => Rect::new(pw - r.x2, ph - r.y2, pw - r.x1, ph - r.y1),
        270 => Rect::new(r.y1, pw - r.x2, r.y2, pw - r.x1),
        _ => r,
    }
}

/// Maps a point in rotated page space (already divided by scale) back to unrotated page points.
pub fn unrotate_point(x: f64, y: f64, pw: f64, ph: f64, rotation: i32) -> (f64, f64) {
    match rotation {
        90 => (y, ph - x),
        180 => (pw - x, ph - y),
        270 => (pw - y, x),
        _ => (x, y),
    }
}

fn line_ranges(layout: &TextLayout) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, c) in layout.chars.iter().enumerate() {
        if *c == '\n' {
            lines.push((start, i));
            start = i + 1;
        }
    }
    if start < layout.chars.len() {
        lines.push((start, layout.chars.len()));
    }
    lines
}

/// Returns the character offset closest to the given point, in reading order.
pub fn offset_at_point(layout: &TextLayout, x: f64, y: f64) -> Option<usize> {
    if layout.rects.is_empty() {
        return None;
    }
    let mut best_line: Option<(usize, usize)> = None;
    let mut best_dist = f64::MAX;
    for (s, e) in line_ranges(layout) {
        if s >= e {
            continue;
        }
        let bbox = layout.rects[s..e].iter().fold(layout.rects[s], |a, r| a.union(r));
        let dy = if y < bbox.y1 {
            bbox.y1 - y
        } else if y > bbox.y2 {
            y - bbox.y2
        } else {
            0.0
        };
        let dx = if x < bbox.x1 {
            bbox.x1 - x
        } else if x > bbox.x2 {
            x - bbox.x2
        } else {
            0.0
        };
        let d = dy * 4.0 + dx;
        if d < best_dist {
            best_dist = d;
            best_line = Some((s, e));
        }
    }
    let (s, e) = best_line?;
    for i in s..e {
        let r = &layout.rects[i];
        if x < (r.x1 + r.x2) / 2.0 {
            return Some(i);
        }
    }
    Some(e)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

fn expand_range(layout: &TextLayout, style: SelectionStyle, mut a: usize, mut b: usize) -> (usize, usize) {
    let n = layout.chars.len();
    match style {
        SelectionStyle::Glyph => {}
        SelectionStyle::Word => {
            while a > 0 && a <= n && is_word_char(layout.chars[a - 1]) {
                a -= 1;
            }
            while b < n && is_word_char(layout.chars[b]) {
                b += 1;
            }
        }
        SelectionStyle::Line => {
            while a > 0 && layout.chars[a - 1] != '\n' {
                a -= 1;
            }
            while b < n && layout.chars[b] != '\n' {
                b += 1;
            }
        }
    }
    (a, b)
}

pub fn selection_range(layout: &TextLayout, style: SelectionStyle, sel: Rect) -> Option<(usize, usize)> {
    let a = offset_at_point(layout, sel.x1, sel.y1)?;
    let b = offset_at_point(layout, sel.x2, sel.y2)?;
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    let (a, b) = expand_range(layout, style, a, b);
    if a >= b { None } else { Some((a, b)) }
}

pub fn rects_for_range(layout: &TextLayout, a: usize, b: usize) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    let mut cur: Option<Rect> = None;
    for i in a..b.min(layout.rects.len()) {
        let c = layout.chars[i];
        if c == '\n' {
            if let Some(r) = cur.take() {
                out.push(r);
            }
            continue;
        }
        let r = layout.rects[i];
        cur = Some(match cur {
            Some(cr) if (r.y1 - cr.y1).abs() < (cr.height() * 0.5).max(1.0) && r.x1 >= cr.x1 - 1.0 => cr.union(&r),
            Some(cr) => {
                out.push(cr);
                r
            }
            None => r,
        });
    }
    if let Some(r) = cur {
        out.push(r);
    }
    out
}

pub fn layout_selection_region(layout: &TextLayout, style: SelectionStyle, sel: Rect) -> Vec<Rect> {
    match selection_range(layout, style, sel) {
        Some((a, b)) => rects_for_range(layout, a, b),
        None => Vec::new(),
    }
}

pub fn layout_selected_text(layout: &TextLayout, style: SelectionStyle, sel: Rect) -> String {
    match selection_range(layout, style, sel) {
        Some((a, b)) => layout.chars[a..b].iter().collect(),
        None => String::new(),
    }
}

pub fn fold_char(c: char, case_sensitive: bool) -> char {
    let c = if c == '\n' { ' ' } else { c };
    if case_sensitive { c } else { c.to_lowercase().next().unwrap_or(c) }
}

pub fn layout_find(layout: &TextLayout, text: &str, opts: FindOptions) -> Vec<FindMatch> {
    let needle: Vec<char> = text.chars().map(|c| fold_char(c, opts.case_sensitive)).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let hay: Vec<char> = layout.chars.iter().map(|c| fold_char(*c, opts.case_sensitive)).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            let end = i + needle.len();
            let ok = !opts.whole_words
                || ((i == 0 || !is_word_char(hay[i - 1])) && (end == hay.len() || !is_word_char(hay[end])));
            if ok {
                out.push(FindMatch { rects: rects_for_range(layout, i, end) });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Compares two file names the way a human would (numbers compared by value).
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().filter(|c| c.is_ascii_digit()) {
                    na.push(*c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().filter(|c| c.is_ascii_digit()) {
                    nb.push(*c);
                    bi.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let o = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(ca), Some(cb)) => {
                let la = ca.to_lowercase().next().unwrap_or(*ca);
                let lb = cb.to_lowercase().next().unwrap_or(*cb);
                if la != lb {
                    return la.cmp(&lb);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn xmp_find(xmp: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xmp.find(&open)?;
    let rest = &xmp[start + open.len()..];
    if let Some(attr_end) = rest.find('>') {
        let head = &rest[..attr_end];
        if head.ends_with('/') {
            let key = "rdf:resource=\"";
            if let Some(p) = head.find(key) {
                let v = &head[p + key.len()..];
                return v.find('"').map(|e| xml_unescape(&v[..e]));
            }
            return None;
        }
        let body = &rest[attr_end + 1..];
        let close = format!("</{tag}>");
        let end = body.find(&close)?;
        let inner = &body[..end];
        if let Some(li) = inner.find("<rdf:li") {
            let after = &inner[li..];
            let gt = after.find('>')?;
            let txt = &after[gt + 1..];
            let e = txt.find("</rdf:li>").unwrap_or(txt.len());
            return Some(xml_unescape(txt[..e].trim()));
        }
        let t = inner.trim();
        if t.is_empty() { None } else { Some(xml_unescape(t)) }
    } else {
        None
    }
}

/// Extracts license information from an XMP packet the way Evince does.
pub fn xmp_license(xmp: &str) -> Option<License> {
    let text = xmp_find(xmp, "xmpRights:UsageTerms").or_else(|| xmp_find(xmp, "dc:rights"));
    let uri = xmp_find(xmp, "cc:license").or_else(|| xmp_find(xmp, "xmpRights:Certificate"));
    let web = xmp_find(xmp, "xmpRights:WebStatement");
    if text.is_none() && uri.is_none() && web.is_none() {
        return None;
    }
    Some(License { text, uri, web_statement: web })
}

pub fn xmp_field(xmp: &str, tag: &str) -> Option<String> {
    xmp_find(xmp, tag)
}

/// Parses an XMP/ISO date ("2020-05-01T10:00:00+02:00") into a unix timestamp.
pub fn parse_iso_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let dt = glib::DateTime::from_iso8601(s, Some(&glib::TimeZone::utc()))
        .or_else(|_| glib::DateTime::from_iso8601(&format!("{s}T00:00:00Z"), None))
        .ok()?;
    Some(dt.to_unix())
}

/// Formats a unix time like Evince's properties dialog.
pub fn format_date(t: i64) -> String {
    glib::DateTime::from_unix_local(t)
        .and_then(|d| d.format("%c"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

pub fn premultiply_rgba_to_argb(rgba: &[u8], w: usize, h: usize, src_stride: usize, alpha: bool) -> cairo::ImageSurface {
    let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).unwrap();
    let stride = surf.stride() as usize;
    let nc = if alpha { 4 } else { 3 };
    {
        let mut d = surf.data().unwrap();
        for y in 0..h {
            for x in 0..w {
                let s = &rgba[y * src_stride + x * nc..];
                let a = if alpha { s[3] as u32 } else { 255 };
                let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
                let px = u32::from_be_bytes([a as u8, pm(s[0]), pm(s[1]), pm(s[2])]);
                d[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&px.to_ne_bytes());
            }
        }
    }
    surf
}

pub fn pixbuf_to_surface(pb: &gdk_pixbuf::Pixbuf) -> cairo::ImageSurface {
    let w = pb.width() as usize;
    let h = pb.height() as usize;
    let bytes = pb.read_pixel_bytes();
    premultiply_rgba_to_argb(&bytes, w, h, pb.rowstride() as usize, pb.has_alpha())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(text: &str) -> TextLayout {
        let mut chars = Vec::new();
        let mut rects = Vec::new();
        let (mut x, mut y) = (0.0, 0.0);
        for c in text.chars() {
            chars.push(c);
            rects.push(Rect::new(x, y, x + 10.0, y + 10.0));
            if c == '\n' {
                x = 0.0;
                y += 12.0;
            } else {
                x += 10.0;
            }
        }
        TextLayout { chars, rects }
    }

    #[test]
    fn find_case_and_words() {
        let l = layout("Hello world\nhello again");
        assert_eq!(layout_find(&l, "hello", FindOptions::default()).len(), 2);
        assert_eq!(layout_find(&l, "hello", FindOptions { case_sensitive: true, whole_words: false }).len(), 1);
        assert_eq!(layout_find(&l, "hell", FindOptions { case_sensitive: false, whole_words: true }).len(), 0);
        let m = layout_find(&l, "world hello", FindOptions::default());
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].rects.len(), 2);
    }

    #[test]
    fn selection_word_and_line() {
        let l = layout("Hello world\nhello again");
        let sel = Rect::new(62.0, 5.0, 62.0, 5.0);
        assert_eq!(layout_selected_text(&l, SelectionStyle::Word, sel), "world");
        assert_eq!(layout_selected_text(&l, SelectionStyle::Line, sel), "Hello world");
        let sel = Rect::new(0.0, 5.0, 40.0, 17.0);
        assert_eq!(layout_selected_text(&l, SelectionStyle::Glyph, sel), "Hello world\nhell");
    }

    #[test]
    fn natural_sorting() {
        let mut v = vec!["p10.jpg", "p2.jpg", "P1.jpg", "p02b.jpg"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["P1.jpg", "p2.jpg", "p02b.jpg", "p10.jpg"]);
    }

    #[test]
    fn rotation_roundtrip() {
        for rot in [0, 90, 180, 270] {
            let (pw, ph) = (100.0, 200.0);
            let r = rotate_rect(Rect::new(10.0, 20.0, 30.0, 40.0), pw, ph, rot);
            let (x, y) = unrotate_point(r.x1, r.y1, pw, ph, rot);
            let (x2, y2) = unrotate_point(r.x2, r.y2, pw, ph, rot);
            let back = Rect::new(x, y, x2, y2).normalized();
            assert_eq!(back, Rect::new(10.0, 20.0, 30.0, 40.0), "rotation {rot}");
        }
    }

    #[test]
    fn xmp_license_parse() {
        let x = r#"<x:xmpmeta><rdf:RDF><rdf:Description><xmpRights:UsageTerms><rdf:Alt><rdf:li xml:lang="x-default">CC BY &amp; more</rdf:li></rdf:Alt></xmpRights:UsageTerms><cc:license rdf:resource="http://creativecommons.org/licenses/by/4.0/"/></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let l = xmp_license(x).unwrap();
        assert_eq!(l.text.as_deref(), Some("CC BY & more"));
        assert_eq!(l.uri.as_deref(), Some("http://creativecommons.org/licenses/by/4.0/"));
    }
}
