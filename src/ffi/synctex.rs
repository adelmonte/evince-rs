#![allow(non_camel_case_types, dead_code)]

use std::os::raw::{c_char, c_float, c_int, c_void};

pub type synctex_scanner_p = *mut c_void;
pub type synctex_node_p = *mut c_void;

unsafe extern "C" {
    pub fn synctex_scanner_new_with_output_file(
        output: *const c_char,
        build_directory: *const c_char,
        parse: c_int,
    ) -> synctex_scanner_p;
    pub fn synctex_scanner_free(scanner: synctex_scanner_p) -> c_int;
    pub fn synctex_display_query(
        scanner: synctex_scanner_p,
        name: *const c_char,
        line: c_int,
        column: c_int,
        page_hint: c_int,
    ) -> c_int;
    pub fn synctex_edit_query(scanner: synctex_scanner_p, page: c_int, h: c_float, v: c_float) -> c_int;
    pub fn synctex_scanner_next_result(scanner: synctex_scanner_p) -> synctex_node_p;
    pub fn synctex_scanner_get_name(scanner: synctex_scanner_p, tag: c_int) -> *const c_char;
    pub fn synctex_node_tag(node: synctex_node_p) -> c_int;
    pub fn synctex_node_line(node: synctex_node_p) -> c_int;
    pub fn synctex_node_column(node: synctex_node_p) -> c_int;
    pub fn synctex_node_page(node: synctex_node_p) -> c_int;
    pub fn synctex_node_box_visible_h(node: synctex_node_p) -> c_float;
    pub fn synctex_node_box_visible_v(node: synctex_node_p) -> c_float;
    pub fn synctex_node_box_visible_width(node: synctex_node_p) -> c_float;
    pub fn synctex_node_box_visible_height(node: synctex_node_p) -> c_float;
    pub fn synctex_node_box_visible_depth(node: synctex_node_p) -> c_float;
}
