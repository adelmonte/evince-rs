use super::*;
use crate::ffi::archive::*;
use crate::ffi::{cstr_opt, cstring};
use gdk_pixbuf::prelude::*;
use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;

struct Archive(*mut archive);

impl Drop for Archive {
    fn drop(&mut self) {
        unsafe { archive_read_free(self.0) };
    }
}

impl Archive {
    fn open(path: &Path) -> Result<Archive> {
        let a = unsafe { archive_read_new() };
        unsafe {
            archive_read_support_filter_all(a);
            archive_read_support_format_zip(a);
            archive_read_support_format_rar(a);
            archive_read_support_format_rar5(a);
            archive_read_support_format_7zip(a);
            archive_read_support_format_tar(a);
        }
        let arc = Archive(a);
        let name = cstring(&path.to_string_lossy());
        if unsafe { archive_read_open_filename(a, name.as_ptr(), 10240) } != ARCHIVE_OK {
            return Err(Error::Invalid(arc.error()));
        }
        Ok(arc)
    }

    fn error(&self) -> String {
        unsafe { cstr_opt(archive_error_string(self.0)) }.unwrap_or_else(|| "Failed to open archive".into())
    }

    /// Advances to the next regular file entry; returns its name.
    fn next(&mut self) -> Option<Result<(String, bool)>> {
        let mut entry = std::ptr::null_mut();
        loop {
            let r = unsafe { archive_read_next_header(self.0, &mut entry) };
            if r == ARCHIVE_EOF {
                return None;
            }
            if r != ARCHIVE_OK && r != ARCHIVE_WARN {
                return Some(Err(Error::Invalid(self.error())));
            }
            let ft = unsafe { archive_entry_filetype(entry) };
            if ft != AE_IFREG {
                continue;
            }
            let name = unsafe { cstr_opt(archive_entry_pathname_utf8(entry)) }
                .or_else(|| unsafe { cstr_opt(archive_entry_pathname(entry)) })
                .unwrap_or_default();
            let encrypted = unsafe { archive_entry_is_encrypted(entry) } != 0;
            return Some(Ok((name, encrypted)));
        }
    }

    fn read_chunk(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = unsafe { archive_read_data(self.0, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n < 0 {
            return Err(Error::Io(self.error()));
        }
        Ok(n as usize)
    }

    fn read_all(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = self.read_chunk(&mut buf)?;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        Ok(out)
    }
}

fn image_extensions() -> HashSet<String> {
    let mut set = HashSet::new();
    for f in gdk_pixbuf::Pixbuf::formats() {
        for e in f.extensions() {
            set.insert(e.to_lowercase());
        }
    }
    set
}

struct Page {
    name: String,
    width: i32,
    height: i32,
}

pub struct ComicsDocument {
    pages: Vec<Page>,
    cache: VecDeque<(usize, Vec<u8>)>,
    source: Source,
}

fn header_size(arc: &mut Archive) -> Option<(i32, i32)> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    let size: Rc<Cell<Option<(i32, i32)>>> = Rc::new(Cell::new(None));
    let s = size.clone();
    loader.connect_size_prepared(move |_, w, h| s.set(Some((w, h))));
    let mut buf = vec![0u8; 16 * 1024];
    while size.get().is_none() {
        let n = arc.read_chunk(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        if loader.write(&buf[..n]).is_err() {
            break;
        }
    }
    let _ = loader.close();
    let (w, h) = size.get()?;
    Some((w, h))
}

impl ComicsDocument {
    pub fn load(source: Source) -> Result<Self> {
        let exts = image_extensions();
        let mut arc = Archive::open(&source.path)?;
        let mut pages = Vec::new();
        let mut any_encrypted = false;
        while let Some(entry) = arc.next() {
            let (name, encrypted) = entry?;
            let base = name.rsplit('/').next().unwrap_or(&name);
            if base.starts_with("._") {
                continue;
            }
            let ext = base.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
            if !exts.contains(&ext) {
                continue;
            }
            if encrypted {
                any_encrypted = true;
                continue;
            }
            if let Some((width, height)) = header_size(&mut arc) {
                pages.push(Page { name, width, height });
            }
        }
        if pages.is_empty() {
            if any_encrypted {
                return Err(Error::Invalid("Encrypted comic book archives are not supported".into()));
            }
            return Err(Error::Invalid("No files in archive".into()));
        }
        pages.sort_by(|a, b| {
            glib::FilenameCollationKey::from(&a.name).cmp(&glib::FilenameCollationKey::from(&b.name))
        });
        Ok(ComicsDocument { pages, cache: VecDeque::new(), source })
    }

    fn page_bytes(&mut self, page: usize) -> Option<Vec<u8>> {
        if let Some((_, d)) = self.cache.iter().find(|(p, _)| *p == page) {
            return Some(d.clone());
        }
        let want = &self.pages.get(page)?.name;
        let mut arc = Archive::open(&self.source.path).ok()?;
        while let Some(entry) = arc.next() {
            let (name, _) = entry.ok()?;
            if &name == want {
                let data = arc.read_all().ok()?;
                self.cache.push_back((page, data.clone()));
                while self.cache.len() > 3 {
                    self.cache.pop_front();
                }
                return Some(data);
            }
        }
        None
    }

    fn decode(&mut self, page: usize, size: Option<(i32, i32)>) -> Option<gdk_pixbuf::Pixbuf> {
        let data = self.page_bytes(page)?;
        let loader = gdk_pixbuf::PixbufLoader::new();
        if let Some((w, h)) = size {
            loader.connect_size_prepared(move |l, _, _| l.set_size(w.max(1), h.max(1)));
        }
        loader.write(&data).ok()?;
        loader.close().ok()?;
        let pb = loader.pixbuf()?;
        Some(pb.apply_embedded_orientation().unwrap_or(pb))
    }
}

impl Document for ComicsDocument {
    fn n_pages(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, page: usize) -> (f64, f64) {
        self.pages.get(page).map_or((1.0, 1.0), |p| (p.width as f64, p.height as f64))
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        let (w, h) = self.page_size(page);
        let sw = (w * scale + 0.5).max(1.0) as i32;
        let sh = (h * scale + 0.5).max(1.0) as i32;
        let pb = self.decode(page, Some((sw, sh)))?;
        let rot = match rotation {
            90 => gdk_pixbuf::PixbufRotation::Clockwise,
            180 => gdk_pixbuf::PixbufRotation::Upsidedown,
            270 => gdk_pixbuf::PixbufRotation::Counterclockwise,
            _ => gdk_pixbuf::PixbufRotation::None,
        };
        let pb = pb.rotate_simple(rot).unwrap_or(pb);
        Some(misc::pixbuf_to_surface(&pb))
    }

    fn info(&self) -> DocInfo {
        DocInfo { n_pages: self.pages.len(), format: Some("Comic book".into()), ..Default::default() }
    }

    fn mime_type(&self) -> &str {
        &self.source.mime
    }

    fn images(&mut self, page: usize) -> Vec<Mapping<i32>> {
        let (w, h) = self.page_size(page);
        vec![Mapping { area: Rect::new(0.0, 0.0, w, h), data: 0 }]
    }

    fn image(&mut self, page: usize, _id: i32) -> Option<cairo::ImageSurface> {
        self.decode(page, None).map(|pb| misc::pixbuf_to_surface(&pb))
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
