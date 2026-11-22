#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_int, c_void};

pub type archive = c_void;
pub type archive_entry = c_void;

pub const ARCHIVE_EOF: c_int = 1;
pub const ARCHIVE_OK: c_int = 0;
pub const ARCHIVE_WARN: c_int = -20;
pub const AE_IFREG: u32 = 0o100000;

unsafe extern "C" {
    pub fn archive_read_new() -> *mut archive;
    pub fn archive_read_free(a: *mut archive) -> c_int;
    pub fn archive_read_support_filter_all(a: *mut archive) -> c_int;
    pub fn archive_read_support_format_all(a: *mut archive) -> c_int;
    pub fn archive_read_support_format_zip(a: *mut archive) -> c_int;
    pub fn archive_read_support_format_rar(a: *mut archive) -> c_int;
    pub fn archive_read_support_format_rar5(a: *mut archive) -> c_int;
    pub fn archive_read_support_format_7zip(a: *mut archive) -> c_int;
    pub fn archive_read_support_format_tar(a: *mut archive) -> c_int;
    pub fn archive_read_open_filename(a: *mut archive, name: *const c_char, block: usize) -> c_int;
    pub fn archive_read_next_header(a: *mut archive, entry: *mut *mut archive_entry) -> c_int;
    pub fn archive_read_data(a: *mut archive, buf: *mut c_void, size: usize) -> isize;
    pub fn archive_read_data_skip(a: *mut archive) -> c_int;
    pub fn archive_error_string(a: *mut archive) -> *const c_char;
    pub fn archive_entry_pathname_utf8(e: *mut archive_entry) -> *const c_char;
    pub fn archive_entry_pathname(e: *mut archive_entry) -> *const c_char;
    pub fn archive_entry_size(e: *mut archive_entry) -> i64;
    pub fn archive_entry_size_is_set(e: *mut archive_entry) -> c_int;
    pub fn archive_entry_filetype(e: *mut archive_entry) -> u32;
    pub fn archive_entry_is_encrypted(e: *mut archive_entry) -> c_int;
}
