pub mod archive;
#[cfg(feature = "djvu")]
pub mod djvu;
#[cfg(feature = "xps")]
pub mod gxps;
pub mod poppler;
#[cfg(feature = "ps")]
pub mod spectre;
pub mod synctex;
pub mod tiff;

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub unsafe fn cstr_opt(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    if s.is_empty() { None } else { Some(s) }
}

pub unsafe fn gstr_take(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { cstr_opt(p) };
    unsafe { glib::ffi::g_free(p as *mut _) };
    s
}

pub fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap()
}

pub unsafe fn take_gerror(err: *mut glib::ffi::GError) -> String {
    if err.is_null() {
        return "Unknown error".into();
    }
    let msg = unsafe { cstr_opt((*err).message) }.unwrap_or_else(|| "Unknown error".into());
    unsafe { glib::ffi::g_error_free(err) };
    msg
}

pub unsafe fn glist_iter<T>(list: *mut glib::ffi::GList) -> Vec<*mut T> {
    let mut out = Vec::new();
    let mut l = list;
    while !l.is_null() {
        out.push(unsafe { (*l).data } as *mut T);
        l = unsafe { (*l).next };
    }
    out
}
