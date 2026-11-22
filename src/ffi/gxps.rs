#![allow(non_camel_case_types, dead_code)]

use glib::ffi::{GError, GList, gboolean, gpointer};
use std::os::raw::{c_char, c_double, c_int, c_uint};

pub type GXPSFile = glib::gobject_ffi::GObject;
pub type GXPSDocument = glib::gobject_ffi::GObject;
pub type GXPSPage = glib::gobject_ffi::GObject;
pub type GXPSDocumentStructure = glib::gobject_ffi::GObject;
pub type GXPSLink = std::os::raw::c_void;
pub type GXPSLinkTarget = std::os::raw::c_void;
pub type GXPSCoreProperties = glib::gobject_ffi::GObject;

#[repr(C)]
pub struct GXPSOutlineIter {
    pub dummy1: gpointer,
    pub dummy2: gpointer,
}

unsafe extern "C" {
    pub fn gxps_file_new(file: *mut gio::ffi::GFile, error: *mut *mut GError) -> *mut GXPSFile;
    pub fn gxps_file_get_n_documents(xps: *mut GXPSFile) -> c_uint;
    pub fn gxps_file_get_document(xps: *mut GXPSFile, n: c_uint, error: *mut *mut GError) -> *mut GXPSDocument;
    pub fn gxps_file_get_document_for_link_target(xps: *mut GXPSFile, target: *mut GXPSLinkTarget) -> c_int;
    pub fn gxps_file_get_core_properties(xps: *mut GXPSFile, error: *mut *mut GError) -> *mut GXPSCoreProperties;
    pub fn gxps_document_get_n_pages(doc: *mut GXPSDocument) -> c_uint;
    pub fn gxps_document_get_page(doc: *mut GXPSDocument, n: c_uint, error: *mut *mut GError) -> *mut GXPSPage;
    pub fn gxps_document_get_page_for_anchor(doc: *mut GXPSDocument, anchor: *const c_char) -> c_int;
    pub fn gxps_document_get_structure(doc: *mut GXPSDocument) -> *mut GXPSDocumentStructure;
    pub fn gxps_page_get_size(page: *mut GXPSPage, w: *mut c_double, h: *mut c_double);
    pub fn gxps_page_render(page: *mut GXPSPage, cr: *mut cairo::ffi::cairo_t, error: *mut *mut GError) -> gboolean;
    pub fn gxps_page_get_links(page: *mut GXPSPage, error: *mut *mut GError) -> *mut GList;
    pub fn gxps_page_get_anchor_destination(
        page: *mut GXPSPage,
        anchor: *const c_char,
        area: *mut cairo::ffi::cairo_rectangle_t,
        error: *mut *mut GError,
    ) -> gboolean;
    pub fn gxps_link_free(link: *mut GXPSLink);
    pub fn gxps_link_get_target(link: *mut GXPSLink) -> *mut GXPSLinkTarget;
    pub fn gxps_link_get_area(link: *mut GXPSLink, area: *mut cairo::ffi::cairo_rectangle_t);
    pub fn gxps_link_target_is_internal(t: *mut GXPSLinkTarget) -> gboolean;
    pub fn gxps_link_target_get_anchor(t: *mut GXPSLinkTarget) -> *const c_char;
    pub fn gxps_link_target_get_uri(t: *mut GXPSLinkTarget) -> *const c_char;
    pub fn gxps_document_structure_has_outline(s: *mut GXPSDocumentStructure) -> gboolean;
    pub fn gxps_document_structure_outline_iter_init(iter: *mut GXPSOutlineIter, s: *mut GXPSDocumentStructure)
    -> gboolean;
    pub fn gxps_outline_iter_next(iter: *mut GXPSOutlineIter) -> gboolean;
    pub fn gxps_outline_iter_children(iter: *mut GXPSOutlineIter, parent: *mut GXPSOutlineIter) -> gboolean;
    pub fn gxps_outline_iter_get_description(iter: *mut GXPSOutlineIter) -> *const c_char;
    pub fn gxps_outline_iter_get_target(iter: *mut GXPSOutlineIter) -> *mut GXPSLinkTarget;
    pub fn gxps_core_properties_get_title(p: *mut GXPSCoreProperties) -> *const c_char;
    pub fn gxps_core_properties_get_creator(p: *mut GXPSCoreProperties) -> *const c_char;
    pub fn gxps_core_properties_get_subject(p: *mut GXPSCoreProperties) -> *const c_char;
    pub fn gxps_core_properties_get_keywords(p: *mut GXPSCoreProperties) -> *const c_char;
    pub fn gxps_core_properties_get_created(p: *mut GXPSCoreProperties) -> libc::time_t;
    pub fn gxps_core_properties_get_modified(p: *mut GXPSCoreProperties) -> libc::time_t;
}
