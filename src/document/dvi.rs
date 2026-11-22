//! DVI backend: a DVI interpreter with PK bitmap fonts, virtual fonts and TFM metrics.

use super::*;
use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;

const BASE_DPI: f64 = 600.0;

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8], p: usize) -> Self {
        Reader { d, p }
    }
    fn eof(&self) -> bool {
        self.p >= self.d.len()
    }
    fn u(&mut self, n: usize) -> Option<u32> {
        if self.p + n > self.d.len() {
            return None;
        }
        let mut v: u32 = 0;
        for i in 0..n {
            v = (v << 8) | self.d[self.p + i] as u32;
        }
        self.p += n;
        Some(v)
    }
    fn s(&mut self, n: usize) -> Option<i32> {
        let v = self.u(n)?;
        let shift = 32 - 8 * n as u32;
        Some(((v << shift) as i32) >> shift)
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.p + n > self.d.len() {
            return None;
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Some(s)
    }
}

#[derive(Clone, Debug)]
struct FontDef {
    _checksum: u32,
    scale: i32,
    design: i32,
    name: String,
}

fn read_fnt_def(r: &mut Reader, op: u8, base: u8) -> Option<(i32, FontDef)> {
    let k = r.s((op - base + 1) as usize)?;
    let checksum = r.u(4)?;
    let scale = r.s(4)?;
    let design = r.s(4)?;
    let a = r.u(1)? as usize;
    let l = r.u(1)? as usize;
    let name = String::from_utf8_lossy(r.bytes(a + l)?).into_owned();
    let name = name.rsplit('/').next().unwrap_or(&name).to_string();
    Some((k, FontDef { _checksum: checksum, scale, design, name }))
}

#[derive(Clone)]
pub struct Glyph {
    tfm: i32,
    w: i32,
    h: i32,
    hoff: i32,
    voff: i32,
    bits: Arc<Vec<u8>>,
}

pub struct PkFont {
    ppp: f64,
    glyphs: HashMap<u32, Glyph>,
}

struct Nybbles<'a> {
    d: &'a [u8],
    pos: usize,
    hi: bool,
}

impl Nybbles<'_> {
    fn get(&mut self) -> u32 {
        let b = *self.d.get(self.pos).unwrap_or(&0) as u32;
        if self.hi {
            self.hi = false;
            b >> 4
        } else {
            self.hi = true;
            self.pos += 1;
            b & 15
        }
    }
    fn packed(&mut self, dyn_f: u32, repeat: &mut u32) -> u32 {
        let i = self.get();
        if i == 0 {
            let mut j = 0;
            let mut i;
            loop {
                i = self.get();
                j += 1;
                if i != 0 || j > 16 {
                    break;
                }
            }
            while j > 0 {
                i = i * 16 + self.get();
                j -= 1;
            }
            i.wrapping_sub(15).wrapping_add((13 - dyn_f) * 16 + dyn_f)
        } else if i <= dyn_f {
            i
        } else if i < 14 {
            (i - dyn_f - 1) * 16 + self.get() + dyn_f + 1
        } else {
            *repeat = if i == 14 { self.packed(dyn_f, repeat) } else { 1 };
            self.packed(dyn_f, repeat)
        }
    }
}

fn decode_pk_raster(data: &[u8], w: usize, h: usize, dyn_f: u32, black_first: bool) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    if w == 0 || h == 0 {
        return out;
    }
    if dyn_f == 14 {
        for i in 0..w * h {
            let byte = *data.get(i / 8).unwrap_or(&0);
            if byte & (0x80 >> (i % 8)) != 0 {
                out[i] = 255;
            }
        }
        return out;
    }
    let mut nyb = Nybbles { d: data, pos: 0, hi: true };
    let mut row = vec![0u8; w];
    let mut rows_done = 0;
    let mut col = 0;
    let mut repeat = 0u32;
    let mut on = black_first;
    let mut guard = 0usize;
    while rows_done < h {
        guard += 1;
        if guard > w * h * 4 + 1000 {
            break;
        }
        let mut count = nyb.packed(dyn_f, &mut repeat) as usize;
        while count > 0 && rows_done < h {
            let take = count.min(w - col);
            if on {
                for c in &mut row[col..col + take] {
                    *c = 255;
                }
            }
            col += take;
            count -= take;
            if col == w {
                for _ in 0..=repeat {
                    if rows_done < h {
                        out[rows_done * w..(rows_done + 1) * w].copy_from_slice(&row);
                        rows_done += 1;
                    }
                }
                repeat = 0;
                col = 0;
                row.iter_mut().for_each(|c| *c = 0);
            }
        }
        on = !on;
    }
    out
}

pub fn parse_pk(d: &[u8]) -> Option<PkFont> {
    let mut r = Reader::new(d, 0);
    if r.u(1)? != 247 || r.u(1)? != 89 {
        return None;
    }
    let k = r.u(1)? as usize;
    r.bytes(k)?;
    let _ds = r.u(4)?;
    let _cs = r.u(4)?;
    let hppp = r.u(4)? as f64 / 65536.0;
    let _vppp = r.u(4)?;
    let mut glyphs = HashMap::new();
    while !r.eof() {
        let flag = r.u(1)? as u8;
        match flag {
            240..=243 => {
                let n = r.u((flag - 239) as usize)? as usize;
                r.bytes(n)?;
            }
            244 => {
                r.u(4)?;
            }
            245 => break,
            246 => {}
            0..=239 => {
                let dyn_f = (flag >> 4) as u32;
                let black = flag & 8 != 0;
                let (pl, cc, tfm, w, h, hoff, voff, start);
                match flag & 7 {
                    0..=3 => {
                        pl = (((flag & 3) as usize) << 8) + r.u(1)? as usize;
                        start = r.p;
                        cc = r.u(1)?;
                        tfm = r.s(3)?;
                        r.u(1)?;
                        w = r.u(1)? as i32;
                        h = r.u(1)? as i32;
                        hoff = r.s(1)?;
                        voff = r.s(1)?;
                    }
                    4..=6 => {
                        pl = (((flag & 3) as usize) << 16) + r.u(2)? as usize;
                        start = r.p;
                        cc = r.u(1)?;
                        tfm = r.s(3)?;
                        r.u(2)?;
                        w = r.u(2)? as i32;
                        h = r.u(2)? as i32;
                        hoff = r.s(2)?;
                        voff = r.s(2)?;
                    }
                    _ => {
                        pl = r.u(4)? as usize;
                        start = r.p;
                        cc = r.u(4)?;
                        tfm = r.s(4)?;
                        r.u(4)?;
                        r.u(4)?;
                        w = r.s(4)?;
                        h = r.s(4)?;
                        hoff = r.s(4)?;
                        voff = r.s(4)?;
                    }
                }
                let end = start + pl;
                if end > d.len() || end < r.p {
                    return None;
                }
                let raster = &d[r.p..end];
                r.p = end;
                let bits = decode_pk_raster(raster, w.max(0) as usize, h.max(0) as usize, dyn_f, black);
                glyphs.insert(cc, Glyph { tfm, w, h, hoff, voff, bits: Arc::new(bits) });
            }
            _ => return None,
        }
    }
    Some(PkFont { ppp: hppp, glyphs })
}

pub fn parse_tfm_widths(d: &[u8]) -> Option<HashMap<u32, i32>> {
    let mut r = Reader::new(d, 0);
    let _lf = r.u(2)?;
    let lh = r.u(2)? as usize;
    let bc = r.u(2)? as usize;
    let ec = r.u(2)? as usize;
    let nw = r.u(2)? as usize;
    let char_base = 24 + lh * 4;
    let width_base = char_base + (ec + 1).saturating_sub(bc) * 4;
    let mut out = HashMap::new();
    if ec < bc {
        return Some(out);
    }
    for c in bc..=ec {
        let wi = *d.get(char_base + (c - bc) * 4)? as usize;
        if wi == 0 || wi >= nw {
            continue;
        }
        let mut wr = Reader::new(d, width_base + wi * 4);
        out.insert(c as u32, wr.s(4)?);
    }
    Some(out)
}

pub struct VfChar {
    tfm: i32,
    dvi: Vec<u8>,
}

pub struct VfFont {
    design: i32,
    fonts: Vec<(i32, FontDef)>,
    chars: HashMap<u32, VfChar>,
}

pub fn parse_vf(d: &[u8]) -> Option<VfFont> {
    let mut r = Reader::new(d, 0);
    if r.u(1)? != 247 || r.u(1)? != 202 {
        return None;
    }
    let k = r.u(1)? as usize;
    r.bytes(k)?;
    let _cs = r.u(4)?;
    let design = r.s(4)?;
    let mut fonts = Vec::new();
    let mut chars = HashMap::new();
    while !r.eof() {
        let op = r.u(1)? as u8;
        match op {
            243..=246 => fonts.push(read_fnt_def(&mut r, op, 243)?),
            242 => {
                let pl = r.u(4)? as usize;
                let cc = r.u(4)?;
                let tfm = r.s(4)?;
                chars.insert(cc, VfChar { tfm, dvi: r.bytes(pl)?.to_vec() });
            }
            0..=241 => {
                let pl = op as usize;
                let cc = r.u(1)?;
                let tfm = r.s(3)?;
                chars.insert(cc, VfChar { tfm, dvi: r.bytes(pl)?.to_vec() });
            }
            248 => break,
            _ => return None,
        }
    }
    Some(VfFont { design, fonts, chars })
}

enum FontData {
    Pk(PkFont),
    Vf(VfFont, HashMap<i32, usize>),
    Metrics(HashMap<u32, i32>),
    Missing,
}

struct LoadedFont {
    _def: FontDef,
    /// Actual scaled size in DVI units.
    size: f64,
    data: FontData,
}

fn kpsewhich(args: &[&str]) -> Option<PathBuf> {
    let out = Command::new("kpsewhich").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn find_font_file(name: &str, ext: &str, dpi: u32) -> Option<PathBuf> {
    let local = [
        PathBuf::from(format!("{name}.{dpi}{ext}")),
        PathBuf::from(format!("{name}.{ext}")),
    ];
    for l in local {
        if l.exists() {
            return Some(l);
        }
    }
    match ext {
        "pk" => {
            let dpi_s = format!("--dpi={dpi}");
            let file = format!("{name}.{dpi}pk");
            kpsewhich(&[&dpi_s, &file]).or_else(|| kpsewhich(&["--mktex=pk", &dpi_s, &file]))
        }
        _ => kpsewhich(&[&format!("{name}.{ext}")]),
    }
}

#[derive(Clone, Copy, Default)]
struct Regs {
    h: f64,
    v: f64,
    w: f64,
    x: f64,
    y: f64,
    z: f64,
}

pub struct DviDocument {
    data: Vec<u8>,
    pages: Vec<usize>,
    num: f64,
    den: f64,
    mag: f64,
    max_w: f64,
    max_h: f64,
    defs: HashMap<i32, FontDef>,
    fonts: Vec<LoadedFont>,
    font_index: HashMap<String, usize>,
    paper: Option<(f64, f64)>,
    source: Source,
}

struct DrawCtx<'a> {
    cr: &'a cairo::Context,
    /// Output pixels per DVI unit.
    k: f64,
    colors: Vec<(f64, f64, f64)>,
    masks: HashMap<(usize, u32), cairo::ImageSurface>,
}

fn parse_color(spec: &str) -> Option<(f64, f64, f64)> {
    let mut it = spec.split_whitespace();
    let model = it.next()?;
    let nums: Vec<f64> = it.clone().filter_map(|n| n.parse().ok()).collect();
    match model.to_ascii_lowercase().as_str() {
        "rgb" if nums.len() >= 3 => Some((nums[0], nums[1], nums[2])),
        "gray" | "grey" if !nums.is_empty() => Some((nums[0], nums[0], nums[0])),
        "cmyk" if nums.len() >= 4 => {
            let k = nums[3];
            Some(((1.0 - nums[0]) * (1.0 - k), (1.0 - nums[1]) * (1.0 - k), (1.0 - nums[2]) * (1.0 - k)))
        }
        "hsb" if nums.len() >= 3 => {
            let rgba = gtk::gdk::RGBA::parse(&format!(
                "hsl({},{}%,{}%)",
                nums[0] * 360.0,
                nums[1] * 100.0,
                nums[2] * 50.0
            ))
            .ok()?;
            Some((rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64))
        }
        name => {
            let named = match name {
                "black" => (0.0, 0.0, 0.0),
                "white" => (1.0, 1.0, 1.0),
                "red" => (1.0, 0.0, 0.0),
                "green" => (0.0, 1.0, 0.0),
                "blue" => (0.0, 0.0, 1.0),
                "cyan" => (0.0, 1.0, 1.0),
                "magenta" => (1.0, 0.0, 1.0),
                "yellow" => (1.0, 1.0, 0.0),
                _ => {
                    let rgba = gtk::gdk::RGBA::parse(name).ok()?;
                    (rgba.red() as f64, rgba.green() as f64, rgba.blue() as f64)
                }
            };
            Some(named)
        }
    }
}

impl DviDocument {
    pub fn load(source: Source) -> Result<Self> {
        let data = std::fs::read(&source.path).map_err(|e| Error::Io(e.to_string()))?;
        let bad = || Error::Invalid("DVI document has incorrect format".into());
        let mut r = Reader::new(&data, 0);
        if r.u(1) != Some(247) || r.u(1) != Some(2) {
            return Err(bad());
        }
        let num = r.u(4).ok_or_else(bad)? as f64;
        let den = r.u(4).ok_or_else(bad)? as f64;
        let mag = r.u(4).ok_or_else(bad)? as f64;
        let mut end = data.len();
        while end > 0 && data[end - 1] == 223 {
            end -= 1;
        }
        if end < 6 {
            return Err(bad());
        }
        let mut r = Reader::new(&data, end - 5);
        let post = r.u(4).ok_or_else(bad)? as usize;
        let mut r = Reader::new(&data, post);
        if r.u(1) != Some(248) {
            return Err(bad());
        }
        let mut last_bop = r.u(4).ok_or_else(bad)? as i64;
        r.u(12);
        let max_h = r.u(4).ok_or_else(bad)? as f64;
        let max_w = r.u(4).ok_or_else(bad)? as f64;
        r.u(2);
        let npages = r.u(2).ok_or_else(bad)? as usize;
        let mut defs = HashMap::new();
        while let Some(op) = r.u(1) {
            match op as u8 {
                243..=246 => {
                    let (k, def) = read_fnt_def(&mut r, op as u8, 243).ok_or_else(bad)?;
                    defs.insert(k, def);
                }
                138 => {}
                _ => break,
            }
        }
        let mut pages = Vec::with_capacity(npages);
        while last_bop >= 0 && (last_bop as usize) < data.len() && pages.len() <= npages {
            pages.push(last_bop as usize);
            let mut r = Reader::new(&data, last_bop as usize + 41);
            last_bop = r.s(4).ok_or_else(bad)? as i64;
        }
        pages.reverse();
        if pages.is_empty() {
            return Err(Error::Invalid("Document has no pages".into()));
        }
        let mut doc = DviDocument {
            data,
            pages,
            num,
            den,
            mag: if mag > 0.0 { mag } else { 1000.0 },
            max_w,
            max_h,
            defs,
            fonts: Vec::new(),
            font_index: HashMap::new(),
            paper: None,
            source,
        };
        doc.paper = doc.find_papersize();
        Ok(doc)
    }

    /// Length in big points of `x` DVI units.
    fn dvi_to_bp(&self, x: f64) -> f64 {
        x * self.num / self.den * self.mag / 1000.0 / 254000.0 * 72.0
    }

    fn find_papersize(&self) -> Option<(f64, f64)> {
        let mut r = Reader::new(&self.data, self.pages[0] + 45);
        for _ in 0..200 {
            let op = r.u(1)? as u8;
            match op {
                239..=242 => {
                    let n = r.u((op - 238) as usize)? as usize;
                    let s = String::from_utf8_lossy(r.bytes(n)?).into_owned();
                    if let Some(spec) = s.strip_prefix("papersize=") {
                        let mut it = spec.split(',');
                        let w = parse_tex_dim(it.next()?)?;
                        let h = parse_tex_dim(it.next()?)?;
                        return Some((w, h));
                    }
                }
                243..=246 => {
                    read_fnt_def(&mut r, op, 243)?;
                }
                138 | 141 | 142 | 171..=234 => {}
                _ => return None,
            }
        }
        None
    }

    fn load_font(&mut self, def: &FontDef, size: f64) -> usize {
        let key = format!("{}@{}", def.name, size.round());
        if let Some(&i) = self.font_index.get(&key) {
            return i;
        }
        let dpi = (BASE_DPI * self.mag / 1000.0 * size / def.design.max(1) as f64).round().max(1.0) as u32;
        let data = if let Some(vf) = find_font_file(&def.name, "vf", dpi)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| parse_vf(&d))
        {
            FontData::Vf(vf, HashMap::new())
        } else if let Some(pk) = find_font_file(&def.name, "pk", dpi)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| parse_pk(&d))
        {
            FontData::Pk(pk)
        } else if let Some(m) = find_font_file(&def.name, "tfm", dpi)
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|d| parse_tfm_widths(&d))
        {
            FontData::Metrics(m)
        } else {
            FontData::Missing
        };
        let idx = self.fonts.len();
        self.fonts.push(LoadedFont { _def: def.clone(), size, data });
        self.font_index.insert(key, idx);
        if let FontData::Vf(vf, _) = &self.fonts[idx].data {
            let locals: Vec<(i32, FontDef)> = vf.fonts.clone();
            let vf_design = vf.design;
            let mut map = HashMap::new();
            for (k, ldef) in locals {
                let lsize = ldef.scale as f64 / 1048576.0 * size;
                let _ = vf_design;
                let li = self.load_font(&ldef, lsize);
                map.insert(k, li);
            }
            if let FontData::Vf(_, m) = &mut self.fonts[idx].data {
                *m = map;
            }
        }
        idx
    }

    fn char_width(&self, font: usize, c: u32) -> f64 {
        let f = &self.fonts[font];
        let tfm = match &f.data {
            FontData::Pk(pk) => pk.glyphs.get(&c).map(|g| g.tfm),
            FontData::Vf(vf, _) => vf.chars.get(&c).map(|v| v.tfm),
            FontData::Metrics(m) => m.get(&c).copied(),
            FontData::Missing => None,
        };
        match tfm {
            Some(t) => t as f64 / 1048576.0 * f.size,
            None => f.size * 0.5,
        }
    }

    fn draw_char(&self, ctx: &mut DrawCtx, font: usize, c: u32, h: f64, v: f64, depth: u32) {
        match &self.fonts[font].data {
            FontData::Pk(pk) => {
                let Some(g) = pk.glyphs.get(&c) else { return };
                if g.w <= 0 || g.h <= 0 {
                    return;
                }
                let mask = ctx.masks.entry((font, c)).or_insert_with(|| {
                    let mut s = cairo::ImageSurface::create(cairo::Format::A8, g.w, g.h).unwrap();
                    let stride = s.stride() as usize;
                    {
                        let mut d = s.data().unwrap();
                        for y in 0..g.h as usize {
                            d[y * stride..y * stride + g.w as usize]
                                .copy_from_slice(&g.bits[y * g.w as usize..(y + 1) * g.w as usize]);
                        }
                    }
                    s
                });
                let px_per_dvi = ctx.k;
                let pk_px_per_bp = pk.ppp * 72.27 / 72.0;
                let bp_per_dvi = self.dvi_to_bp(1.0);
                let f = px_per_dvi / (bp_per_dvi * pk_px_per_bp);
                let cr = ctx.cr;
                let _ = cr.save();
                cr.translate(h * ctx.k, v * ctx.k);
                cr.scale(f, f);
                let col = ctx.colors.last().copied().unwrap_or((0.0, 0.0, 0.0));
                cr.set_source_rgb(col.0, col.1, col.2);
                let _ = cr.mask_surface(&*mask, -g.hoff as f64, -g.voff as f64);
                let _ = cr.restore();
            }
            FontData::Vf(vf, map) => {
                if depth > 8 {
                    return;
                }
                let Some(ch) = vf.chars.get(&c) else { return };
                let scale = self.fonts[font].size / 1048576.0;
                let first = vf.fonts.first().and_then(|(k, _)| map.get(k)).copied();
                let mut regs = Regs { h, v, ..Default::default() };
                self.exec(ctx, &ch.dvi, &mut regs, scale, first, Some(map), depth + 1);
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn exec(
        &self,
        ctx: &mut DrawCtx,
        code: &[u8],
        regs: &mut Regs,
        scale: f64,
        mut font: Option<usize>,
        vf_map: Option<&HashMap<i32, usize>>,
        depth: u32,
    ) {
        let mut r = Reader::new(code, 0);
        let mut stack: Vec<Regs> = Vec::new();
        let font_for = |k: i32| -> Option<usize> {
            match vf_map {
                Some(m) => m.get(&k).copied(),
                None => self.font_index_for_num(k),
            }
        };
        while let Some(op) = r.u(1) {
            let op = op as u8;
            match op {
                0..=127 | 128..=131 | 133..=136 => {
                    let (c, set) = match op {
                        0..=127 => (op as u32, true),
                        128..=131 => match r.u((op - 127) as usize) {
                            Some(c) => (c, true),
                            None => return,
                        },
                        _ => match r.u((op - 132) as usize) {
                            Some(c) => (c, false),
                            None => return,
                        },
                    };
                    if let Some(f) = font {
                        self.draw_char(ctx, f, c, regs.h, regs.v, depth);
                        if set {
                            regs.h += self.char_width(f, c);
                        }
                    }
                }
                132 | 137 => {
                    let (Some(a), Some(b)) = (r.s(4), r.s(4)) else { return };
                    let (a, b) = (a as f64 * scale, b as f64 * scale);
                    if a > 0.0 && b > 0.0 {
                        let col = ctx.colors.last().copied().unwrap_or((0.0, 0.0, 0.0));
                        ctx.cr.set_source_rgb(col.0, col.1, col.2);
                        let x = regs.h * ctx.k;
                        let y = (regs.v - a) * ctx.k;
                        let w = (b * ctx.k).max(1.0);
                        let hh = (a * ctx.k).max(1.0);
                        ctx.cr.rectangle(x, y, w, hh);
                        let _ = ctx.cr.fill();
                    }
                    if op == 132 {
                        regs.h += b;
                    }
                }
                138 => {}
                139 => {
                    r.bytes(44);
                    stack.clear();
                    *regs = Regs::default();
                }
                140 => return,
                141 => stack.push(*regs),
                142 => {
                    if let Some(s) = stack.pop() {
                        *regs = s;
                    }
                }
                143..=146 => regs.h += r.s((op - 142) as usize).unwrap_or(0) as f64 * scale,
                147 => regs.h += regs.w,
                148..=151 => {
                    regs.w = r.s((op - 147) as usize).unwrap_or(0) as f64 * scale;
                    regs.h += regs.w;
                }
                152 => regs.h += regs.x,
                153..=156 => {
                    regs.x = r.s((op - 152) as usize).unwrap_or(0) as f64 * scale;
                    regs.h += regs.x;
                }
                157..=160 => regs.v += r.s((op - 156) as usize).unwrap_or(0) as f64 * scale,
                161 => regs.v += regs.y,
                162..=165 => {
                    regs.y = r.s((op - 161) as usize).unwrap_or(0) as f64 * scale;
                    regs.v += regs.y;
                }
                166 => regs.v += regs.z,
                167..=170 => {
                    regs.z = r.s((op - 166) as usize).unwrap_or(0) as f64 * scale;
                    regs.v += regs.z;
                }
                171..=234 => font = font_for((op - 171) as i32),
                235..=238 => font = font_for(r.u((op - 234) as usize).unwrap_or(0) as i32),
                239..=242 => {
                    let n = r.u((op - 238) as usize).unwrap_or(0) as usize;
                    if let Some(b) = r.bytes(n) {
                        self.special(ctx, &String::from_utf8_lossy(b));
                    }
                }
                243..=246 => {
                    read_fnt_def(&mut r, op, 243);
                }
                _ => return,
            }
        }
    }

    fn font_index_for_num(&self, k: i32) -> Option<usize> {
        let def = self.defs.get(&k)?;
        self.font_index.get(&format!("{}@{}", def.name, (def.scale as f64).round())).copied()
    }

    fn special(&self, ctx: &mut DrawCtx, s: &str) {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix("color") {
            let rest = rest.trim();
            if let Some(spec) = rest.strip_prefix("push") {
                let c = parse_color(spec.trim()).unwrap_or((0.0, 0.0, 0.0));
                ctx.colors.push(c);
            } else if rest.starts_with("pop") {
                if ctx.colors.len() > 1 {
                    ctx.colors.pop();
                }
            } else if let Some(c) = parse_color(rest) {
                if let Some(last) = ctx.colors.last_mut() {
                    *last = c;
                }
            }
        }
    }

    fn page_bp(&self) -> (f64, f64) {
        if let Some(p) = self.paper {
            return p;
        }
        let w = self.dvi_to_bp(self.max_w) + 144.0;
        let h = self.dvi_to_bp(self.max_h) + 144.0;
        if w < 150.0 || h < 150.0 { (612.0, 792.0) } else { (w, h) }
    }
}

fn parse_tex_dim(s: &str) -> Option<f64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let v: f64 = s[..split].trim().parse().ok()?;
    let f = match &s[split..] {
        "pt" => 72.0 / 72.27,
        "bp" => 1.0,
        "in" => 72.0,
        "cm" => 72.0 / 2.54,
        "mm" => 72.0 / 25.4,
        "pc" => 12.0 * 72.0 / 72.27,
        "dd" => 1238.0 / 1157.0 * 72.0 / 72.27,
        "cc" => 12.0 * 1238.0 / 1157.0 * 72.0 / 72.27,
        "sp" => 72.0 / 72.27 / 65536.0,
        _ => return None,
    };
    Some(v * f)
}

impl Document for DviDocument {
    fn n_pages(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, _page: usize) -> (f64, f64) {
        self.page_bp()
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        let defs: Vec<(i32, FontDef)> = self.defs.iter().map(|(k, d)| (*k, d.clone())).collect();
        for (_, d) in &defs {
            self.load_font(d, d.scale as f64);
        }
        let start = *self.pages.get(page)?;
        let end = self.pages.get(page + 1).copied().unwrap_or(self.data.len());
        let (w, h) = self.page_bp();
        let bp_per_dvi = self.dvi_to_bp(1.0);
        let this = &*self;
        vector_surface(w, h, scale, rotation, |cr| {
            let _ = cr.save();
            cr.translate(72.0, 72.0);
            cr.scale(1.0 / scale, 1.0 / scale);
            let mut ctx = DrawCtx { cr, k: scale * bp_per_dvi, colors: vec![(0.0, 0.0, 0.0)], masks: HashMap::new() };
            let mut regs = Regs::default();
            this.exec(&mut ctx, &this.data[start..end], &mut regs, 1.0, None, None, 0);
            let _ = cr.restore();
        })
    }

    fn info(&self) -> DocInfo {
        let (w, h) = self.page_bp();
        let comment = {
            let mut r = Reader::new(&self.data, 14);
            r.u(1).and_then(|k| r.bytes(k as usize)).map(|b| String::from_utf8_lossy(b).trim().to_string())
        };
        DocInfo {
            n_pages: self.pages.len(),
            format: Some("DVI".into()),
            creator: comment,
            paper_size: Some((w / 72.0 * 25.4, h / 72.0 * 25.4)),
            ..Default::default()
        }
    }

    fn mime_type(&self) -> &str {
        &self.source.mime
    }

    fn save(&mut self, path: &Path) -> Result<()> {
        copy_file(&self.source.path, path)
    }

    fn uri(&self) -> &str {
        &self.source.uri
    }

    fn path(&self) -> &Path {
        &self.source.path
    }

    fn supports_synctex(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_u(v: &mut Vec<u8>, x: u32, n: usize) {
        for i in (0..n).rev() {
            v.push((x >> (8 * i)) as u8);
        }
    }

    /// Builds a one-page DVI file with a single rule.
    pub fn sample_dvi() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend([247, 2]);
        push_u(&mut d, 25400000, 4);
        push_u(&mut d, 473628672, 4);
        push_u(&mut d, 1000, 4);
        d.push(0);
        let bop = d.len();
        d.push(139);
        for _ in 0..10 {
            push_u(&mut d, 0, 4);
        }
        push_u(&mut d, 0xffffffff, 4);
        d.push(132);
        push_u(&mut d, 65536 * 10, 4);
        push_u(&mut d, 65536 * 100, 4);
        d.push(140);
        let post = d.len();
        d.push(248);
        push_u(&mut d, bop as u32, 4);
        push_u(&mut d, 25400000, 4);
        push_u(&mut d, 473628672, 4);
        push_u(&mut d, 1000, 4);
        push_u(&mut d, 65536 * 700, 4);
        push_u(&mut d, 65536 * 500, 4);
        push_u(&mut d, 1, 2);
        push_u(&mut d, 1, 2);
        d.push(249);
        push_u(&mut d, post as u32, 4);
        d.push(2);
        d.extend([223; 4]);
        d
    }

    #[test]
    fn parse_and_render_rule() {
        let path = std::env::temp_dir().join(format!("evince-rs-test-{}.dvi", std::process::id()));
        std::fs::write(&path, sample_dvi()).unwrap();
        let src = Source { uri: "file:///x.dvi".into(), path: path.clone(), mime: "application/x-dvi".into() };
        let mut doc = DviDocument::load(src).unwrap();
        assert_eq!(doc.n_pages(), 1);
        let (w, h) = doc.page_size(0);
        assert!(w > 600.0 && h > 800.0, "{w}x{h}");
        let mut surf = doc.render(0, 1.0, 0).unwrap();
        let stride = surf.stride() as usize;
        let data = surf.data().unwrap();
        let px = |x: usize, y: usize| u32::from_ne_bytes(data[y * stride + x * 4..y * stride + x * 4 + 4].try_into().unwrap());
        assert_eq!(px(5, 5), 0xffffffff);
        assert_eq!(px(110, 70), 0xff000000);
        drop(data);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pk_bitmap_decoding() {
        let mut d = vec![247, 89, 0];
        push_u(&mut d, 10 << 20, 4);
        push_u(&mut d, 0, 4);
        push_u(&mut d, (600.0 / 72.27 * 65536.0) as u32, 4);
        push_u(&mut d, (600.0 / 72.27 * 65536.0) as u32, 4);
        let flag = (14u8 << 4) | 0;
        d.push(flag);
        let pl = 9 + 1;
        d.push(pl as u8);
        d.push(b'A');
        push_u(&mut d, 1 << 19, 3);
        d.push(4);
        d.push(4);
        d.push(2);
        d.push(0);
        d.push(2);
        d.push(0b1001_0110);
        d.push(245);
        let pk = parse_pk(&d).unwrap();
        let g = pk.glyphs.get(&(b'A' as u32)).unwrap();
        assert_eq!((g.w, g.h), (4, 2));
        assert_eq!(&g.bits[..], &[255, 0, 0, 255, 0, 255, 255, 0]);
    }

    #[test]
    fn tex_dims() {
        assert!((parse_tex_dim("8.5in").unwrap() - 612.0).abs() < 1e-9);
        assert!((parse_tex_dim("210mm").unwrap() - 595.2756).abs() < 1e-3);
    }
}
