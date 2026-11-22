use super::{Rect, SourceLink};
use crate::ffi::synctex::*;
use crate::ffi::{cstr_opt, cstring};
use std::path::Path;

pub struct Synctex {
    scanner: synctex_scanner_p,
}

unsafe impl Send for Synctex {}

impl Drop for Synctex {
    fn drop(&mut self) {
        unsafe { synctex_scanner_free(self.scanner) };
    }
}

impl Synctex {
    pub fn new(output: &Path) -> Option<Synctex> {
        let stem = output.with_extension("");
        let exists = ["synctex.gz", "synctex", "synctex(busy)"]
            .iter()
            .any(|e| Path::new(&format!("{}.{e}", stem.display())).exists());
        if !exists {
            return None;
        }
        let c = cstring(&output.to_string_lossy());
        let scanner = unsafe { synctex_scanner_new_with_output_file(c.as_ptr(), std::ptr::null(), 1) };
        if scanner.is_null() { None } else { Some(Synctex { scanner }) }
    }

    pub fn backward(&self, page: usize, x: f64, y: f64) -> Option<SourceLink> {
        unsafe {
            if synctex_edit_query(self.scanner, page as i32 + 1, x as f32, y as f32) > 0 {
                let node = synctex_scanner_next_result(self.scanner);
                if node.is_null() {
                    return None;
                }
                let filename = cstr_opt(synctex_scanner_get_name(self.scanner, synctex_node_tag(node)))?;
                return Some(SourceLink { filename, line: synctex_node_line(node), col: synctex_node_column(node) });
            }
        }
        None
    }

    pub fn forward(&self, link: &SourceLink) -> Option<(usize, Rect)> {
        let name = cstring(&link.filename);
        unsafe {
            if synctex_display_query(self.scanner, name.as_ptr(), link.line, link.col, 0) > 0 {
                let node = synctex_scanner_next_result(self.scanner);
                if node.is_null() {
                    return None;
                }
                let page = (synctex_node_page(node) - 1).max(0) as usize;
                let x1 = synctex_node_box_visible_h(node) as f64;
                let y1 = (synctex_node_box_visible_v(node) - synctex_node_box_visible_height(node)) as f64;
                let x2 = synctex_node_box_visible_width(node) as f64 + x1;
                let y2 = (synctex_node_box_visible_depth(node) + synctex_node_box_visible_height(node)) as f64 + y1;
                return Some((page, Rect::new(x1, y1, x2, y2)));
            }
        }
        None
    }
}
