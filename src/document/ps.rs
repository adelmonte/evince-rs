use super::*;
use crate::ffi::spectre::*;
use crate::ffi::{cstr_opt, cstring};

pub struct PsDocument {
    doc: *mut SpectreDocument,
    pages: Vec<*mut SpectrePage>,
    source: Source,
}

unsafe impl Send for PsDocument {}

impl Drop for PsDocument {
    fn drop(&mut self) {
        unsafe {
            for p in self.pages.drain(..) {
                spectre_page_free(p);
            }
            spectre_document_free(self.doc);
        }
    }
}

fn page_rotation(p: *mut SpectrePage) -> i32 {
    match unsafe { spectre_page_get_orientation(p) } {
        ORIENTATION_LANDSCAPE => 90,
        ORIENTATION_REVERSE_PORTRAIT => 180,
        ORIENTATION_REVERSE_LANDSCAPE => 270,
        _ => 0,
    }
}

impl PsDocument {
    pub fn load(source: Source) -> Result<Self> {
        let doc = unsafe { spectre_document_new() };
        let name = cstring(&source.path.to_string_lossy());
        unsafe { spectre_document_load(doc, name.as_ptr()) };
        let st = unsafe { spectre_document_status(doc) };
        if st != 0 {
            let msg = unsafe { cstr_opt(spectre_status_to_string(st)) }.unwrap_or_else(|| "Failed to load document".into());
            unsafe { spectre_document_free(doc) };
            return Err(Error::Invalid(format!("Failed to load document “{}”: {msg}", source.uri)));
        }
        let n = unsafe { spectre_document_get_n_pages(doc) };
        let pages: Vec<_> = (0..n).map(|i| unsafe { spectre_document_get_page(doc, i) }).filter(|p| !p.is_null()).collect();
        if pages.is_empty() {
            unsafe { spectre_document_free(doc) };
            return Err(Error::Invalid("Document has no pages".into()));
        }
        Ok(PsDocument { doc, pages, source })
    }
}

impl Document for PsDocument {
    fn n_pages(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, page: usize) -> (f64, f64) {
        let Some(&p) = self.pages.get(page) else { return (612.0, 792.0) };
        let (mut w, mut h) = (0, 0);
        unsafe { spectre_page_get_size(p, &mut w, &mut h) };
        if matches!(page_rotation(p), 90 | 270) { (h as f64, w as f64) } else { (w as f64, h as f64) }
    }

    fn page_label(&self, page: usize) -> Option<String> {
        self.pages.get(page).and_then(|&p| unsafe { cstr_opt(spectre_page_get_label(p)) })
    }

    fn has_page_labels(&self) -> bool {
        (0..self.pages.len()).any(|i| self.page_label(i).is_some_and(|l| l != (i + 1).to_string()))
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        let p = *self.pages.get(page)?;
        let (mut wp, mut hp) = (0, 0);
        unsafe { spectre_page_get_size(p, &mut wp, &mut hp) };
        let (pw, ph) = self.page_size(page);
        let (w, h) = misc::transformed_size(pw, ph, scale, rotation);
        let rot = (rotation + page_rotation(p)) % 360;
        let (sw, sh) = if rot == 90 || rot == 270 { (h, w) } else { (w, h) };
        let mut data: *mut u8 = std::ptr::null_mut();
        let mut stride = 0;
        unsafe {
            let rc = spectre_render_context_new();
            spectre_render_context_set_scale(rc, sw as f64 / wp.max(1) as f64, sh as f64 / hp.max(1) as f64);
            spectre_render_context_set_rotation(rc, rot as u32);
            spectre_page_render(p, rc, &mut data, &mut stride);
            spectre_render_context_free(rc);
        }
        if data.is_null() {
            return None;
        }
        if unsafe { spectre_page_status(p) } != 0 {
            unsafe { libc::free(data as *mut _) };
            return None;
        }
        let mut surf = cairo::ImageSurface::create(cairo::Format::ARgb32, w.max(1), h.max(1)).ok()?;
        let dstride = surf.stride() as usize;
        {
            let mut d = surf.data().ok()?;
            let src = unsafe { std::slice::from_raw_parts(data, stride as usize * h as usize) };
            let row = (w as usize * 4).min(stride as usize).min(dstride);
            for y in 0..h as usize {
                d[y * dstride..y * dstride + row].copy_from_slice(&src[y * stride as usize..y * stride as usize + row]);
                for x in 0..row / 4 {
                    d[y * dstride + x * 4 + if cfg!(target_endian = "little") { 3 } else { 0 }] = 255;
                }
            }
        }
        unsafe { libc::free(data as *mut _) };
        Some(surf)
    }

    fn info(&self) -> DocInfo {
        let (w, h) = self.page_size(0);
        unsafe {
            DocInfo {
                title: cstr_opt(spectre_document_get_title(self.doc)),
                format: cstr_opt(spectre_document_get_format(self.doc)),
                creator: cstr_opt(spectre_document_get_creator(self.doc))
                    .or_else(|| cstr_opt(spectre_document_get_for(self.doc))),
                n_pages: self.pages.len(),
                paper_size: Some((w / 72.0 * 25.4, h / 72.0 * 25.4)),
                ..Default::default()
            }
        }
    }

    fn mime_type(&self) -> &str {
        &self.source.mime
    }

    fn save(&mut self, path: &Path) -> Result<()> {
        let name = cstring(&path.to_string_lossy());
        unsafe { spectre_document_save(self.doc, name.as_ptr()) };
        if unsafe { spectre_document_status(self.doc) } != 0 {
            return Err(Error::Io(format!("Failed to save document “{}”", path.display())));
        }
        Ok(())
    }

    fn uri(&self) -> &str {
        &self.source.uri
    }

    fn path(&self) -> &Path {
        &self.source.path
    }
}
