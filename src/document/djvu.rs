use super::*;
use crate::ffi::djvu::*;
use crate::ffi::{cstr_opt, cstring};
use std::collections::HashMap;
use std::os::raw::c_uint;
use std::ptr;

#[derive(Debug, Clone, PartialEq)]
pub enum Sexp {
    Nil,
    Int(i32),
    Real(f64),
    Sym(String),
    Str(String),
    List(Vec<Sexp>),
}

impl Sexp {
    fn list(&self) -> &[Sexp] {
        match self {
            Sexp::List(v) => v,
            _ => &[],
        }
    }
    fn sym(&self) -> Option<&str> {
        match self {
            Sexp::Sym(s) => Some(s),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Sexp::Str(s) => Some(s),
            _ => None,
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Sexp::Int(i) => Some(*i as f64),
            Sexp::Real(r) => Some(*r),
            _ => None,
        }
    }
}

/// Converts a minilisp expression into an owned tree, reimplementing the inline accessors.
unsafe fn convert(p: miniexp_t) -> Sexp {
    let bits = p as usize;
    if p.is_null() {
        return Sexp::Nil;
    }
    match bits & 3 {
        3 => Sexp::Int(((bits as isize) >> 2) as i32),
        2 => Sexp::Sym(unsafe { cstr_opt(miniexp_to_name(p)) }.unwrap_or_default()),
        0 => {
            let mut v = Vec::new();
            let mut cur = p;
            while !cur.is_null() && (cur as usize) & 3 == 0 {
                let cell = cur as *const miniexp_t;
                v.push(unsafe { convert(*cell) });
                cur = unsafe { *cell.add(1) };
            }
            Sexp::List(v)
        }
        _ => unsafe {
            if miniexp_stringp(p) != 0 {
                Sexp::Str(cstr_opt(miniexp_to_str(p)).unwrap_or_default())
            } else if miniexp_doublep(p) != 0 {
                Sexp::Real(miniexp_to_double(p))
            } else {
                Sexp::Nil
            }
        },
    }
}

pub struct DjvuDocument {
    ctx: *mut ddjvu_context_t,
    doc: *mut ddjvu_document_t,
    format: *mut ddjvu_format_t,
    info: Vec<ddjvu_pageinfo_t>,
    titles: Vec<Option<String>>,
    file_ids: HashMap<String, usize>,
    text_cache: HashMap<usize, Option<TextLayout>>,
    source: Source,
}

unsafe impl Send for DjvuDocument {}

impl Drop for DjvuDocument {
    fn drop(&mut self) {
        unsafe {
            ddjvu_job_release(ddjvu_document_job(self.doc));
            ddjvu_format_release(self.format);
            ddjvu_context_release(self.ctx);
        }
    }
}

impl DjvuDocument {
    fn handle_events(&self, wait: bool) -> Option<String> {
        let mut error = None;
        unsafe {
            if wait {
                ddjvu_message_wait(self.ctx);
            }
            loop {
                let msg = ddjvu_message_peek(self.ctx);
                if msg.is_null() {
                    break;
                }
                if (*msg).tag == MSG_ERROR {
                    let e = msg as *const ddjvu_message_error_t;
                    error = cstr_opt((*e).message);
                }
                ddjvu_message_pop(self.ctx);
            }
        }
        error
    }

    pub fn load(source: Source) -> Result<Self> {
        let ctx = unsafe { ddjvu_context_create(c"evince-rs".as_ptr()) };
        let masks: [c_uint; 4] = [0x00ff0000, 0x0000ff00, 0x000000ff, 0xff000000];
        let format = unsafe { ddjvu_format_create(FORMAT_RGBMASK32, 4, masks.as_ptr()) };
        unsafe { ddjvu_format_set_row_order(format, 1) };
        let fname = cstring(&source.path.to_string_lossy());
        let doc = unsafe { ddjvu_document_create_by_filename_utf8(ctx, fname.as_ptr(), 1) };
        if doc.is_null() {
            unsafe {
                ddjvu_format_release(format);
                ddjvu_context_release(ctx);
            }
            return Err(Error::Invalid("DjVu document has incorrect format".into()));
        }
        let mut this = DjvuDocument {
            ctx,
            doc,
            format,
            info: Vec::new(),
            titles: Vec::new(),
            file_ids: HashMap::new(),
            text_cache: HashMap::new(),
            source,
        };
        let mut last_err = None;
        while unsafe { ddjvu_job_status(ddjvu_document_job(doc)) } < JOB_OK {
            if let Some(e) = this.handle_events(true) {
                last_err = Some(e);
            }
        }
        if unsafe { ddjvu_job_status(ddjvu_document_job(doc)) } >= JOB_FAILED {
            return Err(Error::Invalid(last_err.unwrap_or_else(|| "DjVu document has incorrect format".into())));
        }
        let n = unsafe { ddjvu_document_get_pagenum(doc) }.max(0) as usize;
        if n == 0 {
            return Err(Error::Invalid("Document has no pages".into()));
        }
        this.titles = vec![None; n];
        let nfiles = unsafe { ddjvu_document_get_filenum(doc) };
        let indirect = unsafe { ddjvu_document_get_type(doc) } == DOCTYPE_INDIRECT;
        let base = this.source.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        for i in 0..nfiles {
            let mut fi = ddjvu_fileinfo_t {
                type_: 0,
                pageno: -1,
                size: 0,
                id: ptr::null(),
                name: ptr::null(),
                title: ptr::null(),
            };
            unsafe { ddjvu_document_get_fileinfo_imp(doc, i, &mut fi, std::mem::size_of::<ddjvu_fileinfo_t>() as u32) };
            if fi.type_ as u8 != b'P' || fi.pageno < 0 || fi.pageno as usize >= n {
                continue;
            }
            let id = unsafe { cstr_opt(fi.id) }.unwrap_or_default();
            if indirect && !base.join(&id).exists() {
                return Err(Error::Invalid(
                    "The document is composed of several files. One or more of these files cannot be accessed.".into(),
                ));
            }
            this.file_ids.insert(id, fi.pageno as usize);
            this.titles[fi.pageno as usize] = unsafe { cstr_opt(fi.title) }.filter(|t| !t.ends_with(".djvu"));
        }
        for p in 0..n {
            let mut info = ddjvu_pageinfo_t::default();
            loop {
                let r = unsafe {
                    ddjvu_document_get_pageinfo_imp(doc, p as i32, &mut info, std::mem::size_of::<ddjvu_pageinfo_t>() as u32)
                };
                if r >= JOB_OK {
                    if r >= JOB_FAILED {
                        this.handle_events(false);
                    }
                    break;
                }
                this.handle_events(true);
            }
            if info.dpi <= 0 {
                info.dpi = 300;
            }
            this.info.push(info);
        }
        Ok(this)
    }

    fn dpi(&self, page: usize) -> f64 {
        self.info.get(page).map_or(300.0, |i| i.dpi as f64)
    }

    fn get_sexp(&self, f: impl Fn() -> miniexp_t) -> Sexp {
        let mut p = f();
        while p == MINIEXP_DUMMY {
            self.handle_events(true);
            p = f();
        }
        let s = unsafe { convert(p) };
        unsafe { ddjvu_miniexp_release(self.doc, p) };
        s
    }

    fn link_dest(&self, name: &str, base_page: Option<usize>) -> Option<LinkDest> {
        let rest = name.strip_prefix('#')?;
        if rest.ends_with(".djvu") {
            return Some(LinkDest::Page(*self.file_ids.get(rest).unwrap_or(&0)));
        }
        if let Some(bp) = base_page {
            if let Some(n) = rest.strip_prefix('+').and_then(|n| n.parse::<usize>().ok()) {
                return Some(LinkDest::Page(bp + n));
            }
            if let Some(n) = rest.strip_prefix('-').and_then(|n| n.parse::<usize>().ok()) {
                return Some(LinkDest::Page(bp.saturating_sub(n)));
            }
        }
        Some(LinkDest::PageLabel(rest.to_string()))
    }

    fn link_action(&self, name: &str, base_page: Option<usize>) -> Option<LinkAction> {
        if let Some(d) = self.link_dest(name, base_page) {
            Some(LinkAction::GotoDest(d))
        } else if name.contains("://") {
            Some(LinkAction::ExternalUri(name.to_string()))
        } else {
            None
        }
    }

    fn build_layout(&self, page: usize) -> Option<TextLayout> {
        let sexp = self.get_sexp(|| unsafe { ddjvu_document_get_pagetext(self.doc, page as i32, c"char".as_ptr()) });
        if sexp == Sexp::Nil {
            return None;
        }
        let info = self.info.get(page)?;
        let h = info.height as f64;
        let k = 72.0 / self.dpi(page);
        let mut layout = TextLayout::default();
        fn walk(s: &Sexp, h: f64, k: f64, out: &mut TextLayout) {
            let items = s.list();
            if items.len() < 5 {
                return;
            }
            let kind = items[0].sym().unwrap_or("");
            let nums: Vec<f64> = items[1..5].iter().filter_map(|n| n.num()).collect();
            if nums.len() != 4 {
                return;
            }
            let rect = Rect::new(nums[0] * k, (h - nums[3]) * k, nums[2] * k, (h - nums[1]) * k);
            if let Some(text) = items.get(5).and_then(|t| t.str()) {
                let chars: Vec<char> = text.chars().collect();
                let n = chars.len().max(1) as f64;
                for (i, c) in chars.iter().enumerate() {
                    let x1 = rect.x1 + rect.width() * i as f64 / n;
                    let x2 = rect.x1 + rect.width() * (i + 1) as f64 / n;
                    out.chars.push(*c);
                    out.rects.push(Rect::new(x1, rect.y1, x2, rect.y2));
                }
                if kind == "word" {
                    out.chars.push(' ');
                    out.rects.push(Rect::new(rect.x2, rect.y1, rect.x2, rect.y2));
                }
            } else {
                for child in &items[5..] {
                    walk(child, h, k, out);
                }
                if kind == "word" {
                    out.chars.push(' ');
                    out.rects.push(Rect::new(rect.x2, rect.y1, rect.x2, rect.y2));
                } else if matches!(kind, "line" | "para" | "region" | "column") {
                    if out.chars.last() == Some(&' ') {
                        out.chars.pop();
                        out.rects.pop();
                    }
                    if out.chars.last() != Some(&'\n') && !out.chars.is_empty() {
                        out.chars.push('\n');
                        out.rects.push(Rect::new(rect.x2, rect.y1, rect.x2, rect.y2));
                    }
                }
            }
        }
        walk(&sexp, h, k, &mut layout);
        Some(layout)
    }

    fn cached_layout(&mut self, page: usize) -> Option<TextLayout> {
        if !self.text_cache.contains_key(&page) {
            let l = self.build_layout(page);
            if self.text_cache.len() > 64 {
                self.text_cache.clear();
            }
            self.text_cache.insert(page, l);
        }
        self.text_cache.get(&page).cloned().flatten()
    }
}

impl Document for DjvuDocument {
    fn n_pages(&self) -> usize {
        self.info.len()
    }

    fn page_size(&self, page: usize) -> (f64, f64) {
        let i = self.info.get(page).copied().unwrap_or_default();
        let dpi = self.dpi(page);
        let (w, h) = (i.width as f64 * 72.0 / dpi, i.height as f64 * 72.0 / dpi);
        if i.rotation % 2 == 1 { (h, w) } else { (w, h) }
    }

    fn page_label(&self, page: usize) -> Option<String> {
        self.titles.get(page).cloned().flatten()
    }

    fn has_page_labels(&self) -> bool {
        self.titles.iter().any(|t| t.is_some())
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        let d_page = unsafe { ddjvu_page_create_by_pageno(self.doc, page as i32) };
        if d_page.is_null() {
            return None;
        }
        while unsafe { ddjvu_job_status(ddjvu_page_job(d_page)) } < JOB_OK {
            self.handle_events(true);
        }
        let (w, h) = self.page_size(page);
        let (tw, th) = misc::transformed_size(w, h, scale, rotation);
        let mut rot = unsafe { ddjvu_page_get_initial_rotation(d_page) };
        rot += match rotation {
            90 => 3,
            180 => 2,
            270 => 1,
            _ => 0,
        };
        rot %= 4;
        let mut surf = cairo::ImageSurface::create(cairo::Format::Rgb24, tw.max(1), th.max(1)).ok()?;
        let stride = surf.stride();
        let rect = ddjvu_rect_t { x: 0, y: 0, w: tw.max(1) as u32, h: th.max(1) as u32 };
        unsafe { ddjvu_page_set_rotation(d_page, rot) };
        let modified = {
            let mut data = surf.data().ok()?;
            unsafe {
                ddjvu_page_render(
                    d_page,
                    RENDER_COLOR,
                    &rect,
                    &rect,
                    self.format,
                    stride as std::os::raw::c_ulong,
                    data.as_mut_ptr() as *mut _,
                )
            }
        };
        unsafe { ddjvu_job_release(ddjvu_page_job(d_page)) };
        if modified == 0 {
            let cr = cairo::Context::new(&surf).ok()?;
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().ok()?;
        }
        Some(surf)
    }

    fn info(&self) -> DocInfo {
        let anno = self.get_sexp(|| unsafe { ddjvu_document_get_anno(self.doc, 1) });
        let mut info = DocInfo { n_pages: self.n_pages(), format: Some("DjVu".into()), ..Default::default() };
        let (w, h) = self.page_size(0);
        info.paper_size = Some((w / 72.0 * 25.4, h / 72.0 * 25.4));
        for item in anno.list() {
            let l = item.list();
            if l.first().and_then(|s| s.sym()) == Some("metadata") {
                for kv in &l[1..] {
                    let kv = kv.list();
                    let (Some(k), Some(v)) = (kv.first().and_then(|s| s.sym()), kv.get(1).and_then(|s| s.str())) else {
                        continue;
                    };
                    let v = Some(v.to_string());
                    match k {
                        "title" => info.title = v,
                        "author" => info.author = v,
                        "subject" => info.subject = v,
                        "keywords" => info.keywords = v,
                        "creator" => info.creator = v,
                        "producer" => info.producer = v,
                        "CreationDate" => info.creation_date = v.as_deref().and_then(misc::parse_iso_date),
                        "ModDate" => info.modified_date = v.as_deref().and_then(misc::parse_iso_date),
                        _ => {}
                    }
                }
            } else if l.first().and_then(|s| s.sym()) == Some("xmp") {
                if let Some(x) = l.get(1).and_then(|s| s.str()) {
                    info.license = misc::xmp_license(x);
                }
            }
        }
        info
    }

    fn mime_type(&self) -> &str {
        &self.source.mime
    }

    fn has_text(&self) -> bool {
        true
    }

    fn text_layout(&mut self, page: usize) -> Option<TextLayout> {
        self.cached_layout(page)
    }

    fn links(&mut self, page: usize) -> Vec<Mapping<Link>> {
        let anno = self.get_sexp(|| unsafe { ddjvu_document_get_pageanno(self.doc, page as i32) });
        let Some(info) = self.info.get(page).copied() else { return Vec::new() };
        let k = 72.0 / self.dpi(page);
        let h = info.height as f64;
        let mut out = Vec::new();
        for item in anno.list() {
            let l = item.list();
            if l.first().and_then(|s| s.sym()) != Some("maparea") || l.len() < 4 {
                continue;
            }
            let url = match &l[1] {
                Sexp::Str(s) => s.clone(),
                Sexp::List(v) if v.first().and_then(|s| s.sym()) == Some("url") => {
                    v.get(1).and_then(|s| s.str()).unwrap_or_default().to_string()
                }
                _ => continue,
            };
            let comment = l[2].str().filter(|s| !s.is_empty()).map(String::from);
            let shape = l[3].list();
            let kind = shape.first().and_then(|s| s.sym()).unwrap_or("");
            let nums: Vec<f64> = shape.iter().skip(1).filter_map(|n| n.num()).collect();
            let area = match kind {
                "rect" | "oval" if nums.len() >= 4 => {
                    let (x, y, w, hh) = (nums[0], nums[1], nums[2], nums[3]);
                    Rect::new(x * k, (h - (y + hh)) * k, (x + w) * k, (h - y) * k)
                }
                "poly" if nums.len() >= 4 => {
                    let xs: Vec<f64> = nums.iter().step_by(2).copied().collect();
                    let ys: Vec<f64> = nums.iter().skip(1).step_by(2).copied().collect();
                    let (minx, maxx) = (xs.iter().cloned().fold(f64::MAX, f64::min), xs.iter().cloned().fold(f64::MIN, f64::max));
                    let (miny, maxy) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
                    Rect::new(minx * k, (h - maxy) * k, maxx * k, (h - miny) * k)
                }
                _ => continue,
            };
            if let Some(action) = self.link_action(&url, Some(page)) {
                out.push(Mapping { area, data: Link { title: comment, action: Some(action) } });
            }
        }
        out
    }

    fn outline(&mut self) -> Vec<OutlineItem> {
        let sexp = self.get_sexp(|| unsafe { ddjvu_document_get_outline(self.doc) });
        let l = sexp.list();
        if l.first().and_then(|s| s.sym()) != Some("bookmarks") {
            return Vec::new();
        }
        fn build(this: &DjvuDocument, items: &[Sexp]) -> Vec<OutlineItem> {
            let mut out = Vec::new();
            for it in items {
                let v = it.list();
                let (Some(title), Some(dest)) = (v.first().and_then(|s| s.str()), v.get(1).and_then(|s| s.str())) else {
                    continue;
                };
                out.push(OutlineItem {
                    title: title.to_string(),
                    link: this
                        .link_action(dest, None)
                        .map(|a| Link { title: Some(title.to_string()), action: Some(a) }),
                    expand: false,
                    children: build(this, &v[2..]),
                });
            }
            out
        }
        build(self, &l[1..])
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
}
