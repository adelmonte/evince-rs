use super::*;
use crate::ffi::gxps::*;
use crate::ffi::{cstr_opt, glist_iter, take_gerror};
use glib::translate::ToGlibPtr;
use std::ptr;

pub struct XpsDocument {
    file: *mut GXPSFile,
    docs: Vec<*mut GXPSDocument>,
    pages: Vec<(usize, *mut GXPSPage, f64, f64)>,
    source: Source,
}

unsafe impl Send for XpsDocument {}

impl Drop for XpsDocument {
    fn drop(&mut self) {
        unsafe {
            for (_, p, _, _) in self.pages.drain(..) {
                glib::gobject_ffi::g_object_unref(p);
            }
            for d in self.docs.drain(..) {
                glib::gobject_ffi::g_object_unref(d);
            }
            glib::gobject_ffi::g_object_unref(self.file);
        }
    }
}

impl XpsDocument {
    pub fn load(source: Source) -> Result<Self> {
        let gf = gio::File::for_path(&source.path);
        let mut err = ptr::null_mut();
        let file = unsafe { gxps_file_new(gf.to_glib_none().0, &mut err) };
        if file.is_null() {
            return Err(Error::Invalid(unsafe { take_gerror(err) }));
        }
        let mut docs = Vec::new();
        let mut pages = Vec::new();
        let nd = unsafe { gxps_file_get_n_documents(file) };
        for di in 0..nd {
            let mut err = ptr::null_mut();
            let d = unsafe { gxps_file_get_document(file, di, &mut err) };
            if d.is_null() {
                unsafe { take_gerror(err) };
                continue;
            }
            let np = unsafe { gxps_document_get_n_pages(d) };
            for pi in 0..np {
                let mut err = ptr::null_mut();
                let p = unsafe { gxps_document_get_page(d, pi, &mut err) };
                if p.is_null() {
                    unsafe { take_gerror(err) };
                    continue;
                }
                let (mut w, mut h) = (0.0, 0.0);
                unsafe { gxps_page_get_size(p, &mut w, &mut h) };
                pages.push((docs.len(), p, w, h));
            }
            docs.push(d);
        }
        if pages.is_empty() {
            unsafe { glib::gobject_ffi::g_object_unref(file) };
            return Err(Error::Invalid("Document has no pages".into()));
        }
        Ok(XpsDocument { file, docs, pages, source })
    }

    fn doc_page_offset(&self, doc: usize) -> usize {
        self.pages.iter().position(|(d, ..)| *d == doc).unwrap_or(0)
    }

    fn target_action(&self, target: *mut GXPSLinkTarget) -> Option<LinkAction> {
        unsafe {
            if gxps_link_target_is_internal(target) != 0 {
                let di = gxps_file_get_document_for_link_target(self.file, target);
                if di < 0 {
                    return None;
                }
                let doc = *self.docs.get(di as usize)?;
                let anchor = gxps_link_target_get_anchor(target);
                let page = if anchor.is_null() { 0 } else { gxps_document_get_page_for_anchor(doc, anchor).max(0) as usize };
                Some(LinkAction::GotoDest(LinkDest::Page(self.doc_page_offset(di as usize) + page)))
            } else {
                cstr_opt(gxps_link_target_get_uri(target)).map(LinkAction::ExternalUri)
            }
        }
    }
}

impl Document for XpsDocument {
    fn n_pages(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, page: usize) -> (f64, f64) {
        self.pages.get(page).map_or((612.0, 792.0), |p| (p.2, p.3))
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        let (_, p, w, h) = *self.pages.get(page)?;
        vector_surface(w, h, scale, rotation, |cr| unsafe {
            let mut err = ptr::null_mut();
            if gxps_page_render(p, cr.to_raw_none(), &mut err) == 0 {
                take_gerror(err);
            }
        })
    }

    fn print_page(&mut self, page: usize, cr: &cairo::Context) {
        if let Some(&(_, p, _, _)) = self.pages.get(page) {
            unsafe {
                let mut err = ptr::null_mut();
                if gxps_page_render(p, cr.to_raw_none(), &mut err) == 0 {
                    take_gerror(err);
                }
            }
        }
    }

    fn info(&self) -> DocInfo {
        let (w, h) = self.page_size(0);
        let mut info = DocInfo {
            format: Some("XPS".into()),
            n_pages: self.pages.len(),
            paper_size: Some((w / 96.0 * 25.4, h / 96.0 * 25.4)),
            ..Default::default()
        };
        unsafe {
            let mut err = ptr::null_mut();
            let cp = gxps_file_get_core_properties(self.file, &mut err);
            if cp.is_null() {
                if !err.is_null() {
                    take_gerror(err);
                }
                return info;
            }
            info.title = cstr_opt(gxps_core_properties_get_title(cp));
            info.author = cstr_opt(gxps_core_properties_get_creator(cp));
            info.subject = cstr_opt(gxps_core_properties_get_subject(cp));
            info.keywords = cstr_opt(gxps_core_properties_get_keywords(cp));
            let c = gxps_core_properties_get_created(cp);
            let m = gxps_core_properties_get_modified(cp);
            info.creation_date = (c > 0).then_some(c as i64);
            info.modified_date = (m > 0).then_some(m as i64);
            glib::gobject_ffi::g_object_unref(cp);
        }
        info
    }

    fn mime_type(&self) -> &str {
        &self.source.mime
    }

    fn links(&mut self, page: usize) -> Vec<Mapping<Link>> {
        let Some(&(_, p, _, _)) = self.pages.get(page) else { return Vec::new() };
        let mut out = Vec::new();
        unsafe {
            let mut err = ptr::null_mut();
            let list = gxps_page_get_links(p, &mut err);
            if list.is_null() {
                if !err.is_null() {
                    take_gerror(err);
                }
                return out;
            }
            for l in glist_iter::<GXPSLink>(list) {
                let mut area = cairo::ffi::cairo_rectangle_t { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
                gxps_link_get_area(l, &mut area);
                if let Some(action) = self.target_action(gxps_link_get_target(l)) {
                    out.push(Mapping {
                        area: Rect::new(area.x, area.y, area.x + area.width, area.y + area.height),
                        data: Link { title: None, action: Some(action) },
                    });
                }
                gxps_link_free(l);
            }
            glib::ffi::g_list_free(list);
        }
        out
    }

    fn outline(&mut self) -> Vec<OutlineItem> {
        fn walk(this: &XpsDocument, iter: *mut GXPSOutlineIter, out: &mut Vec<OutlineItem>) {
            unsafe {
                loop {
                    let title = cstr_opt(gxps_outline_iter_get_description(iter)).unwrap_or_default();
                    let target = gxps_outline_iter_get_target(iter);
                    let link = this.target_action(target).map(|a| Link { title: Some(title.clone()), action: Some(a) });
                    let mut item = OutlineItem { title, link, expand: false, children: Vec::new() };
                    let mut child = GXPSOutlineIter { dummy1: ptr::null_mut(), dummy2: ptr::null_mut() };
                    if gxps_outline_iter_children(&mut child, iter) != 0 {
                        walk(this, &mut child, &mut item.children);
                    }
                    out.push(item);
                    if gxps_outline_iter_next(iter) == 0 {
                        break;
                    }
                }
            }
        }
        let mut out = Vec::new();
        for &d in &self.docs {
            unsafe {
                let s = gxps_document_get_structure(d);
                if s.is_null() {
                    continue;
                }
                if gxps_document_structure_has_outline(s) != 0 {
                    let mut iter = GXPSOutlineIter { dummy1: ptr::null_mut(), dummy2: ptr::null_mut() };
                    if gxps_document_structure_outline_iter_init(&mut iter, s) != 0 {
                        walk(self, &mut iter, &mut out);
                    }
                }
                glib::gobject_ffi::g_object_unref(s);
            }
        }
        out
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

