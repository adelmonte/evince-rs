#![allow(non_camel_case_types, dead_code, non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

pub type TIFF = c_void;

pub const TIFFTAG_IMAGEWIDTH: u32 = 256;
pub const TIFFTAG_IMAGELENGTH: u32 = 257;
pub const TIFFTAG_XRESOLUTION: u32 = 282;
pub const TIFFTAG_YRESOLUTION: u32 = 283;
pub const TIFFTAG_RESOLUTIONUNIT: u32 = 296;
pub const TIFFTAG_DOCUMENTNAME: u32 = 269;
pub const TIFFTAG_IMAGEDESCRIPTION: u32 = 270;
pub const TIFFTAG_ARTIST: u32 = 315;
pub const TIFFTAG_SOFTWARE: u32 = 305;
pub const TIFFTAG_DATETIME: u32 = 306;
pub const TIFFTAG_ORIENTATION: u32 = 274;
pub const RESUNIT_INCH: u16 = 2;
pub const RESUNIT_CENTIMETER: u16 = 3;
pub const ORIENTATION_TOPLEFT: c_int = 1;

pub type TIFFErrorHandler = Option<unsafe extern "C" fn(module: *const c_char, fmt: *const c_char, ap: *mut c_void)>;

unsafe extern "C" {
    pub fn TIFFOpen(name: *const c_char, mode: *const c_char) -> *mut TIFF;
    pub fn TIFFClose(tif: *mut TIFF);
    pub fn TIFFNumberOfDirectories(tif: *mut TIFF) -> u32;
    pub fn TIFFSetDirectory(tif: *mut TIFF, dir: u32) -> c_int;
    pub fn TIFFGetField(tif: *mut TIFF, tag: u32, ...) -> c_int;
    pub fn TIFFGetFieldDefaulted(tif: *mut TIFF, tag: u32, ...) -> c_int;
    pub fn TIFFReadRGBAImageOriented(
        tif: *mut TIFF,
        w: u32,
        h: u32,
        raster: *mut u32,
        orientation: c_int,
        stop_on_error: c_int,
    ) -> c_int;
    pub fn TIFFSetErrorHandler(h: TIFFErrorHandler) -> TIFFErrorHandler;
    pub fn TIFFSetWarningHandler(h: TIFFErrorHandler) -> TIFFErrorHandler;
}
