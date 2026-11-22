use super::*;
use gio::prelude::*;
use std::io::{Read, Write};

pub const COMICS_MIME: &[&str] = &[
    "application/vnd.comicbook-rar",
    "application/vnd.comicbook+zip",
    "application/x-cb7",
    "application/x-cbr",
    "application/x-cbt",
    "application/x-cbz",
    "application/x-ext-cb7",
    "application/x-ext-cbr",
    "application/x-ext-cbt",
    "application/x-ext-cbz",
];
pub const DJVU_MIME: &[&str] = &["application/x-ext-djv", "application/x-ext-djvu", "image/vnd.djvu", "image/vnd.djvu+multipage"];
pub const DVI_MIME: &[&str] = &["application/x-bzdvi", "application/x-dvi", "application/x-ext-dvi", "application/x-gzdvi"];
pub const PDF_MIME: &[&str] = &[
    "application/pdf",
    "application/x-bzpdf",
    "application/x-ext-pdf",
    "application/x-gzpdf",
    "application/x-xzpdf",
    "application/illustrator",
];
pub const PS_MIME: &[&str] = &[
    "application/postscript",
    "application/x-bzpostscript",
    "application/x-gzpostscript",
    "application/x-ext-eps",
    "application/x-ext-ps",
    "image/x-bzeps",
    "image/x-eps",
    "image/x-gzeps",
];
pub const TIFF_MIME: &[&str] = &["image/tiff"];
pub const XPS_MIME: &[&str] = &["application/oxps", "application/vnd.ms-xpsdocument"];

pub fn supported_mime_types() -> Vec<&'static str> {
    [COMICS_MIME, DJVU_MIME, DVI_MIME, PDF_MIME, PS_MIME, TIFF_MIME, XPS_MIME].concat()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Pdf,
    Ps,
    Djvu,
    Tiff,
    Xps,
    Dvi,
    Comics,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Compression {
    None,
    Gzip,
    Bzip2,
    Xz,
}

fn backend_for_mime(mime: &str) -> Option<Backend> {
    let m = mime;
    if PDF_MIME.contains(&m) {
        Some(Backend::Pdf)
    } else if PS_MIME.contains(&m) {
        Some(Backend::Ps)
    } else if DJVU_MIME.contains(&m) {
        Some(Backend::Djvu)
    } else if TIFF_MIME.contains(&m) {
        Some(Backend::Tiff)
    } else if XPS_MIME.contains(&m) {
        Some(Backend::Xps)
    } else if DVI_MIME.contains(&m) {
        Some(Backend::Dvi)
    } else if COMICS_MIME.contains(&m) || m == "application/zip" || m == "application/vnd.rar" || m == "application/x-rar" {
        Some(Backend::Comics)
    } else {
        None
    }
}

fn compression_for_mime(mime: &str) -> Compression {
    match mime {
        "application/x-gzpdf" | "application/x-gzpostscript" | "image/x-gzeps" | "application/x-gzdvi" | "application/gzip" => {
            Compression::Gzip
        }
        "application/x-bzpdf" | "application/x-bzpostscript" | "image/x-bzeps" | "application/x-bzdvi" | "application/x-bzip" => {
            Compression::Bzip2
        }
        "application/x-xzpdf" | "application/x-xz" => Compression::Xz,
        _ => Compression::None,
    }
}

fn backend_for_extension(path: &Path) -> Option<(Backend, &'static str)> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let strip = name.trim_end_matches(".gz").trim_end_matches(".bz2").trim_end_matches(".xz");
    let ext = strip.rsplit_once('.').map(|(_, e)| e)?;
    Some(match ext {
        "pdf" | "ai" => (Backend::Pdf, "application/pdf"),
        "ps" | "eps" | "epsi" | "epsf" => (Backend::Ps, "application/postscript"),
        "djvu" | "djv" => (Backend::Djvu, "image/vnd.djvu"),
        "tif" | "tiff" => (Backend::Tiff, "image/tiff"),
        "xps" | "oxps" => (Backend::Xps, "application/oxps"),
        "dvi" => (Backend::Dvi, "application/x-dvi"),
        "cbz" => (Backend::Comics, "application/x-cbz"),
        "cbr" => (Backend::Comics, "application/x-cbr"),
        "cb7" => (Backend::Comics, "application/x-cb7"),
        "cbt" => (Backend::Comics, "application/x-cbt"),
        _ => return None,
    })
}

fn sniff_magic(path: &Path) -> Option<(Backend, &'static str)> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = [0u8; 16];
    let n = f.read(&mut buf).ok()?;
    let b = &buf[..n];
    if b.starts_with(b"%PDF") {
        Some((Backend::Pdf, "application/pdf"))
    } else if b.starts_with(b"%!PS") || b.starts_with(&[0xc5, 0xd0, 0xd3, 0xc6]) {
        Some((Backend::Ps, "application/postscript"))
    } else if b.starts_with(b"AT&TFORM") {
        Some((Backend::Djvu, "image/vnd.djvu"))
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        Some((Backend::Tiff, "image/tiff"))
    } else if b.starts_with(&[247, 2]) {
        Some((Backend::Dvi, "application/x-dvi"))
    } else {
        None
    }
}

/// A temporary file removed when dropped.
#[derive(Debug)]
pub struct TempFile(pub PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn temp_path(suffix: &str) -> PathBuf {
    let dir = glib::user_cache_dir().join("evince-rs");
    let _ = std::fs::create_dir_all(&dir);
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    dir.join(format!("document-{}-{n}{suffix}", std::process::id()))
}

fn decompress(src: &Path, c: Compression) -> Result<TempFile> {
    let ext = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .and_then(|n| {
            let n = n.trim_end_matches(".gz").trim_end_matches(".bz2").trim_end_matches(".xz").to_string();
            n.rsplit_once('.').map(|(_, e)| format!(".{e}"))
        })
        .unwrap_or_default();
    let out = TempFile(temp_path(&ext));
    match c {
        Compression::Gzip => {
            let file = gio::File::for_path(src);
            let input = file.read(gio::Cancellable::NONE).map_err(|e| Error::Io(e.to_string()))?;
            let conv = gio::ZlibDecompressor::new(gio::ZlibCompressorFormat::Gzip);
            let stream = gio::ConverterInputStream::new(&input, &conv);
            let mut reader = stream.into_read();
            let mut f = std::fs::File::create(&out.0).map_err(|e| Error::Io(e.to_string()))?;
            std::io::copy(&mut reader, &mut f).map_err(|e| Error::Io(e.to_string()))?;
            f.flush().ok();
        }
        Compression::Bzip2 | Compression::Xz => {
            let tool = if c == Compression::Bzip2 { "bzip2" } else { "xz" };
            let output = std::process::Command::new(tool)
                .arg("-dc")
                .arg(src)
                .output()
                .map_err(|e| Error::Io(format!("Failed to run {tool}: {e}")))?;
            if !output.status.success() {
                return Err(Error::Io(format!("Failed to uncompress document with {tool}")));
            }
            std::fs::write(&out.0, output.stdout).map_err(|e| Error::Io(e.to_string()))?;
        }
        Compression::None => unreachable!(),
    }
    Ok(out)
}

/// Copies a non-local document (e.g. on a gvfs mount) to a local temporary file.
fn make_local(file: &gio::File) -> Result<(PathBuf, Option<TempFile>)> {
    if let Some(p) = file.path() {
        return Ok((p, None));
    }
    let name = file.basename().map(|b| b.to_string_lossy().to_string()).unwrap_or_default();
    let ext = name.rsplit_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
    let tmp = TempFile(temp_path(&ext));
    let dst = gio::File::for_path(&tmp.0);
    file.copy(&dst, gio::FileCopyFlags::OVERWRITE, gio::Cancellable::NONE, None)
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok((tmp.0.clone(), Some(tmp)))
}

pub fn content_type(file: &gio::File) -> Option<String> {
    let info = file
        .query_info(gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
        .ok()?;
    let ct = info.content_type()?;
    gio::content_type_get_mime_type(&ct).map(|m| m.to_string())
}

pub struct Loaded {
    pub doc: Box<dyn Document>,
    pub temps: Vec<TempFile>,
}

/// Loads a document from a URI, choosing the backend from the MIME type.
pub fn load_document(uri: &str, password: Option<&str>) -> Result<Loaded> {
    let file = gio::File::for_uri(uri);
    let mime = content_type(&file);
    let (local, remote_tmp) = make_local(&file)?;
    let mut temps: Vec<TempFile> = remote_tmp.into_iter().collect();
    let mut mime = mime.unwrap_or_default();
    let mut backend = backend_for_mime(&mime);
    if backend.is_none() || mime == "application/octet-stream" || mime == "application/zip" {
        if let Some((b, m)) = sniff_magic(&local).or_else(|| backend_for_extension(&local)) {
            if backend.is_none() || b != Backend::Comics {
                backend = Some(b);
                if !mime.contains("gz") && !mime.contains("bz") && !mime.contains("xz") {
                    mime = m.to_string();
                }
            }
        }
    }
    let Some(backend) = backend else {
        let desc = gio::content_type_get_description(&mime);
        return Err(Error::UnsupportedType(if desc.is_empty() { mime } else { desc.to_string() }));
    };
    let compression = compression_for_mime(&mime);
    let path = if compression != Compression::None {
        let t = decompress(&local, compression)?;
        let p = t.0.clone();
        temps.push(t);
        p
    } else {
        local
    };
    let source = Source { uri: uri.to_string(), path, mime };
    let doc: Box<dyn Document> = match backend {
        Backend::Pdf => Box::new(pdf::PdfDocument::load(source, password)?),
        #[cfg(feature = "ps")]
        Backend::Ps => Box::new(ps::PsDocument::load(source)?),
        #[cfg(feature = "djvu")]
        Backend::Djvu => Box::new(djvu::DjvuDocument::load(source)?),
        Backend::Tiff => Box::new(tiff::TiffDocument::load(source)?),
        #[cfg(feature = "xps")]
        Backend::Xps => Box::new(xps::XpsDocument::load(source)?),
        Backend::Dvi => Box::new(dvi::DviDocument::load(source)?),
        Backend::Comics => Box::new(comics::ComicsDocument::load(source)?),
        #[allow(unreachable_patterns)]
        _ => return Err(Error::UnsupportedType(source.mime)),
    };
    Ok(Loaded { doc, temps })
}

#[allow(dead_code)]
fn _assert_send() {
    fn is_send<T: Send>() {}
    is_send::<Box<dyn Document>>();
}
