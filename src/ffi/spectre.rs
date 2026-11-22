#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_double, c_int, c_uchar, c_uint, c_void};

pub type SpectreDocument = c_void;
pub type SpectrePage = c_void;
pub type SpectreRenderContext = c_void;
pub type SpectreExporter = c_void;

pub const ORIENTATION_PORTRAIT: c_int = 0;
pub const ORIENTATION_LANDSCAPE: c_int = 1;
pub const ORIENTATION_REVERSE_PORTRAIT: c_int = 2;
pub const ORIENTATION_REVERSE_LANDSCAPE: c_int = 3;

pub const EXPORTER_FORMAT_PS: c_int = 0;
pub const EXPORTER_FORMAT_PDF: c_int = 1;

unsafe extern "C" {
    pub fn spectre_document_new() -> *mut SpectreDocument;
    pub fn spectre_document_load(doc: *mut SpectreDocument, filename: *const c_char);
    pub fn spectre_document_status(doc: *mut SpectreDocument) -> c_int;
    pub fn spectre_document_free(doc: *mut SpectreDocument);
    pub fn spectre_document_get_n_pages(doc: *mut SpectreDocument) -> c_uint;
    pub fn spectre_document_get_title(doc: *mut SpectreDocument) -> *const c_char;
    pub fn spectre_document_get_creator(doc: *mut SpectreDocument) -> *const c_char;
    pub fn spectre_document_get_for(doc: *mut SpectreDocument) -> *const c_char;
    pub fn spectre_document_get_creation_date(doc: *mut SpectreDocument) -> *const c_char;
    pub fn spectre_document_get_format(doc: *mut SpectreDocument) -> *const c_char;
    pub fn spectre_document_is_eps(doc: *mut SpectreDocument) -> c_int;
    pub fn spectre_document_get_page(doc: *mut SpectreDocument, index: c_uint) -> *mut SpectrePage;
    pub fn spectre_document_save(doc: *mut SpectreDocument, filename: *const c_char);
    pub fn spectre_document_save_to_pdf(doc: *mut SpectreDocument, filename: *const c_char);
    pub fn spectre_page_free(page: *mut SpectrePage);
    pub fn spectre_page_status(page: *mut SpectrePage) -> c_int;
    pub fn spectre_page_get_label(page: *mut SpectrePage) -> *const c_char;
    pub fn spectre_page_get_orientation(page: *mut SpectrePage) -> c_int;
    pub fn spectre_page_get_size(page: *mut SpectrePage, w: *mut c_int, h: *mut c_int);
    pub fn spectre_page_render(
        page: *mut SpectrePage,
        rc: *mut SpectreRenderContext,
        data: *mut *mut c_uchar,
        row_length: *mut c_int,
    );
    pub fn spectre_render_context_new() -> *mut SpectreRenderContext;
    pub fn spectre_render_context_free(rc: *mut SpectreRenderContext);
    pub fn spectre_render_context_set_scale(rc: *mut SpectreRenderContext, x: c_double, y: c_double);
    pub fn spectre_render_context_set_rotation(rc: *mut SpectreRenderContext, rot: c_uint);
    pub fn spectre_render_context_set_antialias_bits(rc: *mut SpectreRenderContext, g: c_int, t: c_int);
    pub fn spectre_status_to_string(status: c_int) -> *const c_char;
}
