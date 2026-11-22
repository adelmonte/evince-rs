#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_int, c_uint, c_ulong, c_void};

pub type ddjvu_context_t = c_void;
pub type ddjvu_document_t = c_void;
pub type ddjvu_page_t = c_void;
pub type ddjvu_job_t = c_void;
pub type ddjvu_format_t = c_void;
pub type miniexp_t = *mut c_void;

pub const JOB_OK: c_int = 2;
pub const JOB_FAILED: c_int = 3;

pub const MSG_ERROR: c_int = 0;

pub const FORMAT_RGB24: c_int = 1;
pub const FORMAT_RGBMASK32: c_int = 3;
pub const RENDER_COLOR: c_int = 0;

pub const DOCTYPE_INDIRECT: c_int = 3;

#[repr(C)]
pub struct ddjvu_message_any_t {
    pub tag: c_int,
    pub context: *mut ddjvu_context_t,
    pub document: *mut ddjvu_document_t,
    pub page: *mut ddjvu_page_t,
    pub job: *mut ddjvu_job_t,
}

#[repr(C)]
pub struct ddjvu_message_error_t {
    pub any: ddjvu_message_any_t,
    pub message: *const c_char,
    pub function: *const c_char,
    pub filename: *const c_char,
    pub lineno: c_int,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ddjvu_pageinfo_t {
    pub width: c_int,
    pub height: c_int,
    pub dpi: c_int,
    pub rotation: c_int,
    pub version: c_int,
}

#[repr(C)]
pub struct ddjvu_fileinfo_t {
    pub type_: c_char,
    pub pageno: c_int,
    pub size: c_int,
    pub id: *const c_char,
    pub name: *const c_char,
    pub title: *const c_char,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct ddjvu_rect_t {
    pub x: c_int,
    pub y: c_int,
    pub w: c_uint,
    pub h: c_uint,
}

unsafe extern "C" {
    pub fn ddjvu_context_create(name: *const c_char) -> *mut ddjvu_context_t;
    pub fn ddjvu_context_release(ctx: *mut ddjvu_context_t);
    pub fn ddjvu_message_wait(ctx: *mut ddjvu_context_t) -> *const ddjvu_message_any_t;
    pub fn ddjvu_message_peek(ctx: *mut ddjvu_context_t) -> *const ddjvu_message_any_t;
    pub fn ddjvu_message_pop(ctx: *mut ddjvu_context_t);
    pub fn ddjvu_job_status(job: *mut ddjvu_job_t) -> c_int;
    pub fn ddjvu_job_release(job: *mut ddjvu_job_t);
    pub fn ddjvu_job_done(job: *mut ddjvu_job_t) -> c_int;
    pub fn ddjvu_document_job(doc: *mut ddjvu_document_t) -> *mut ddjvu_job_t;
    pub fn ddjvu_page_job(page: *mut ddjvu_page_t) -> *mut ddjvu_job_t;
    pub fn ddjvu_document_create_by_filename_utf8(
        ctx: *mut ddjvu_context_t,
        filename: *const c_char,
        cache: c_int,
    ) -> *mut ddjvu_document_t;
    pub fn ddjvu_document_get_type(doc: *mut ddjvu_document_t) -> c_int;
    pub fn ddjvu_document_get_pagenum(doc: *mut ddjvu_document_t) -> c_int;
    pub fn ddjvu_document_get_filenum(doc: *mut ddjvu_document_t) -> c_int;
    pub fn ddjvu_document_get_fileinfo_imp(
        doc: *mut ddjvu_document_t,
        fileno: c_int,
        info: *mut ddjvu_fileinfo_t,
        infosz: c_uint,
    ) -> c_int;
    pub fn ddjvu_document_get_pageinfo_imp(
        doc: *mut ddjvu_document_t,
        pageno: c_int,
        info: *mut ddjvu_pageinfo_t,
        infosz: c_uint,
    ) -> c_int;
    pub fn ddjvu_page_create_by_pageno(doc: *mut ddjvu_document_t, pageno: c_int) -> *mut ddjvu_page_t;
    pub fn ddjvu_page_get_initial_rotation(page: *mut ddjvu_page_t) -> c_int;
    pub fn ddjvu_page_set_rotation(page: *mut ddjvu_page_t, rot: c_int);
    pub fn ddjvu_page_render(
        page: *mut ddjvu_page_t,
        mode: c_int,
        pagerect: *const ddjvu_rect_t,
        renderrect: *const ddjvu_rect_t,
        format: *const ddjvu_format_t,
        rowsize: c_ulong,
        buffer: *mut c_char,
    ) -> c_int;
    pub fn ddjvu_format_create(style: c_int, nargs: c_int, args: *const c_uint) -> *mut ddjvu_format_t;
    pub fn ddjvu_format_set_row_order(fmt: *mut ddjvu_format_t, top_to_bottom: c_int);
    pub fn ddjvu_format_release(fmt: *mut ddjvu_format_t);
    pub fn ddjvu_thumbnail_status(doc: *mut ddjvu_document_t, pagenum: c_int, start: c_int) -> c_int;
    pub fn ddjvu_thumbnail_render(
        doc: *mut ddjvu_document_t,
        pagenum: c_int,
        wptr: *mut c_int,
        hptr: *mut c_int,
        format: *const ddjvu_format_t,
        rowsize: c_ulong,
        buffer: *mut c_char,
    ) -> c_int;
    pub fn ddjvu_document_get_pagetext(doc: *mut ddjvu_document_t, pageno: c_int, maxdetail: *const c_char)
    -> miniexp_t;
    pub fn ddjvu_document_get_outline(doc: *mut ddjvu_document_t) -> miniexp_t;
    pub fn ddjvu_document_get_pageanno(doc: *mut ddjvu_document_t, pageno: c_int) -> miniexp_t;
    pub fn ddjvu_document_get_anno(doc: *mut ddjvu_document_t, compat: c_int) -> miniexp_t;
    pub fn ddjvu_miniexp_release(doc: *mut ddjvu_document_t, expr: miniexp_t);
    pub fn ddjvu_anno_get_xmp(anno: miniexp_t) -> *const c_char;
    pub fn ddjvu_document_print(
        doc: *mut ddjvu_document_t,
        output: *mut libc::FILE,
        optc: c_int,
        optv: *const *const c_char,
    ) -> *mut ddjvu_job_t;

    pub fn miniexp_to_name(p: miniexp_t) -> *const c_char;
    pub fn miniexp_stringp(p: miniexp_t) -> c_int;
    pub fn miniexp_to_str(p: miniexp_t) -> *const c_char;
    pub fn miniexp_doublep(p: miniexp_t) -> c_int;
    pub fn miniexp_to_double(p: miniexp_t) -> f64;
}

pub const MINIEXP_DUMMY: miniexp_t = 2 as miniexp_t;
