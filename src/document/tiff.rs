use super::*;
use crate::ffi::cstring;
use crate::ffi::tiff::*;
use std::os::raw::{c_char, c_void};

pub struct TiffDocument {
    tif: *mut TIFF,
    pages: Vec<(u32, u32, f32, f32)>,
    source: Source,
}

unsafe impl Send for TiffDocument {}

impl Drop for TiffDocument {
    fn drop(&mut self) {
        unsafe { TIFFClose(self.tif) };
    }
}

unsafe extern "C" fn quiet(_m: *const c_char, _f: *const c_char, _a: *mut c_void) {}

fn silence() {
    unsafe {
        TIFFSetErrorHandler(Some(quiet));
        TIFFSetWarningHandler(Some(quiet));
    }
}

impl TiffDocument {
    pub fn load(source: Source) -> Result<Self> {
        silence();
        let name = cstring(&source.path.to_string_lossy());
        let tif = unsafe { TIFFOpen(name.as_ptr(), c"r".as_ptr()) };
        if tif.is_null() {
            return Err(Error::Invalid("Invalid document".into()));
        }
        let n = unsafe { TIFFNumberOfDirectories(tif) };
        let mut pages = Vec::new();
        for i in 0..n {
            if unsafe { TIFFSetDirectory(tif, i) } != 1 {
                break;
            }
            let (mut w, mut h): (u32, u32) = (0, 0);
            let (mut xr, mut yr): (f32, f32) = (0.0, 0.0);
            let mut unit: u16 = 0;
            unsafe {
                TIFFGetField(tif, TIFFTAG_IMAGEWIDTH, &mut w as *mut u32);
                TIFFGetField(tif, TIFFTAG_IMAGELENGTH, &mut h as *mut u32);
                if TIFFGetField(tif, TIFFTAG_XRESOLUTION, &mut xr as *mut f32) != 0
                    && TIFFGetField(tif, TIFFTAG_YRESOLUTION, &mut yr as *mut f32) != 0
                    && TIFFGetFieldDefaulted(tif, TIFFTAG_RESOLUTIONUNIT, &mut unit as *mut u16) != 0
                    && unit == RESUNIT_CENTIMETER
                {
                    xr *= 2.54;
                    yr *= 2.54;
                }
            }
            if !(xr > 0.0) {
                xr = 72.0;
            }
            if !(yr > 0.0) {
                yr = 72.0;
            }
            pages.push((w, h, xr, yr));
        }
        if pages.is_empty() {
            unsafe { TIFFClose(tif) };
            return Err(Error::Invalid("Invalid document".into()));
        }
        Ok(TiffDocument { tif, pages, source })
    }

    fn get_string(&self, tag: u32) -> Option<String> {
        if unsafe { TIFFSetDirectory(self.tif, 0) } != 1 {
            return None;
        }
        let mut p: *const c_char = std::ptr::null();
        if unsafe { TIFFGetField(self.tif, tag, &mut p as *mut *const c_char) } != 0 {
            unsafe { crate::ffi::cstr_opt(p) }
        } else {
            None
        }
    }
}

impl Document for TiffDocument {
    fn n_pages(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, page: usize) -> (f64, f64) {
        let (w, h, xr, yr) = self.pages.get(page).copied().unwrap_or((1, 1, 72.0, 72.0));
        (w as f64, (h as f64 * (xr / yr) as f64).floor())
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        silence();
        if unsafe { TIFFSetDirectory(self.tif, page as u32) } != 1 {
            return None;
        }
        let (w, h, _, _) = *self.pages.get(page)?;
        if w == 0 || h == 0 || (w as u64) * (h as u64) > (1u64 << 30) {
            return None;
        }
        let mut raster = vec![0u32; (w * h) as usize];
        if unsafe { TIFFReadRGBAImageOriented(self.tif, w, h, raster.as_mut_ptr(), ORIENTATION_TOPLEFT, 0) } == 0 {
            return None;
        }
        let mut src = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).ok()?;
        let stride = src.stride() as usize;
        {
            let mut d = src.data().ok()?;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let p = raster[y * w as usize + x];
                    let (r, g, b, a) = (p & 0xff, (p >> 8) & 0xff, (p >> 16) & 0xff, (p >> 24) & 0xff);
                    let px = (a << 24) | (r << 16) | (g << 8) | b;
                    d[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&px.to_ne_bytes());
                }
            }
        }
        let (pw, ph) = self.page_size(page);
        let sw = (pw * scale + 0.5).max(1.0) as i32;
        let sh = (ph * scale + 0.5).max(1.0) as i32;
        misc::scale_and_rotate(&src, sw, sh, rotation)
    }

    fn info(&self) -> DocInfo {
        let (w, h) = self.page_size(0);
        let (_, _, xr, yr) = self.pages[0];
        DocInfo {
            n_pages: self.pages.len(),
            format: Some("TIFF".into()),
            title: self.get_string(TIFFTAG_DOCUMENTNAME),
            subject: self.get_string(TIFFTAG_IMAGEDESCRIPTION),
            author: self.get_string(TIFFTAG_ARTIST),
            creator: self.get_string(TIFFTAG_SOFTWARE),
            paper_size: Some((w / xr as f64 * 25.4, h / yr as f64 * 25.4)),
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
}
