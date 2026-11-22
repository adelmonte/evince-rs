#![allow(non_camel_case_types, dead_code)]

use cairo::ffi::{cairo_region_t, cairo_surface_t, cairo_t};
use glib::ffi::{GArray, GBytes, GDateTime, GError, GList, gboolean, gpointer};
use std::os::raw::{c_char, c_double, c_int, c_uint};

#[repr(C)]
pub struct PopplerDocument {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerPage {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerIndexIter {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerLayersIter {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerLayer {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerFontInfo {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerFontsIter {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerAnnot {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerFormField {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerMovie {
    _p: [u8; 0],
}
#[repr(C)]
pub struct PopplerMedia {
    _p: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PopplerRectangle {
    pub x1: c_double,
    pub y1: c_double,
    pub x2: c_double,
    pub y2: c_double,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PopplerPoint {
    pub x: c_double,
    pub y: c_double,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PopplerQuadrilateral {
    pub p1: PopplerPoint,
    pub p2: PopplerPoint,
    pub p3: PopplerPoint,
    pub p4: PopplerPoint,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PopplerColor {
    pub red: u16,
    pub green: u16,
    pub blue: u16,
}

#[repr(C)]
pub struct PopplerDest {
    pub type_: c_int,
    pub page_num: c_int,
    pub left: c_double,
    pub bottom: c_double,
    pub right: c_double,
    pub top: c_double,
    pub zoom: c_double,
    pub named_dest: *mut c_char,
    pub change_flags: c_uint,
}

pub const DEST_UNKNOWN: c_int = 0;
pub const DEST_XYZ: c_int = 1;
pub const DEST_FIT: c_int = 2;
pub const DEST_FITH: c_int = 3;
pub const DEST_FITV: c_int = 4;
pub const DEST_FITR: c_int = 5;
pub const DEST_FITB: c_int = 6;
pub const DEST_FITBH: c_int = 7;
pub const DEST_FITBV: c_int = 8;
pub const DEST_NAMED: c_int = 9;

pub const DEST_CHANGE_LEFT: c_uint = 1;
pub const DEST_CHANGE_TOP: c_uint = 2;
pub const DEST_CHANGE_ZOOM: c_uint = 4;

pub const ACTION_UNKNOWN: c_int = 0;
pub const ACTION_NONE: c_int = 1;
pub const ACTION_GOTO_DEST: c_int = 2;
pub const ACTION_GOTO_REMOTE: c_int = 3;
pub const ACTION_LAUNCH: c_int = 4;
pub const ACTION_URI: c_int = 5;
pub const ACTION_NAMED: c_int = 6;
pub const ACTION_MOVIE: c_int = 7;
pub const ACTION_RENDITION: c_int = 8;
pub const ACTION_OCG_STATE: c_int = 9;
pub const ACTION_JAVASCRIPT: c_int = 10;
pub const ACTION_RESET_FORM: c_int = 11;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionAny {
    pub type_: c_int,
    pub title: *mut c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionGotoDest {
    pub type_: c_int,
    pub title: *mut c_char,
    pub dest: *mut PopplerDest,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionGotoRemote {
    pub type_: c_int,
    pub title: *mut c_char,
    pub file_name: *mut c_char,
    pub dest: *mut PopplerDest,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionLaunch {
    pub type_: c_int,
    pub title: *mut c_char,
    pub file_name: *mut c_char,
    pub params: *mut c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionUri {
    pub type_: c_int,
    pub title: *mut c_char,
    pub uri: *mut c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionNamed {
    pub type_: c_int,
    pub title: *mut c_char,
    pub named_dest: *mut c_char,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionMovie {
    pub type_: c_int,
    pub title: *mut c_char,
    pub operation: c_int,
    pub movie: *mut PopplerMovie,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionRendition {
    pub type_: c_int,
    pub title: *mut c_char,
    pub op: c_int,
    pub media: *mut PopplerMedia,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionOCGState {
    pub type_: c_int,
    pub title: *mut c_char,
    pub state_list: *mut GList,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PopplerActionResetForm {
    pub type_: c_int,
    pub title: *mut c_char,
    pub fields: *mut GList,
    pub exclude: gboolean,
}
#[repr(C)]
pub struct PopplerActionLayer {
    pub action: c_int,
    pub layers: *mut GList,
}

#[repr(C)]
pub union PopplerAction {
    pub type_: c_int,
    pub any: PopplerActionAny,
    pub goto_dest: PopplerActionGotoDest,
    pub goto_remote: PopplerActionGotoRemote,
    pub launch: PopplerActionLaunch,
    pub uri: PopplerActionUri,
    pub named: PopplerActionNamed,
    pub movie: PopplerActionMovie,
    pub rendition: PopplerActionRendition,
    pub ocg_state: PopplerActionOCGState,
    pub reset_form: PopplerActionResetForm,
}

#[repr(C)]
pub struct PopplerLinkMapping {
    pub area: PopplerRectangle,
    pub action: *mut PopplerAction,
}
#[repr(C)]
pub struct PopplerImageMapping {
    pub area: PopplerRectangle,
    pub image_id: c_int,
}
#[repr(C)]
pub struct PopplerFormFieldMapping {
    pub area: PopplerRectangle,
    pub field: *mut PopplerFormField,
}
#[repr(C)]
pub struct PopplerAnnotMapping {
    pub area: PopplerRectangle,
    pub annot: *mut PopplerAnnot,
}

#[repr(C)]
pub struct PopplerPageTransition {
    pub type_: c_int,
    pub alignment: c_int,
    pub direction: c_int,
    pub duration: c_int,
    pub angle: c_int,
    pub scale: c_double,
    pub rectangular: gboolean,
    pub duration_real: c_double,
}

#[repr(C)]
pub struct PopplerAttachment {
    pub parent: glib::gobject_ffi::GObject,
    pub name: *mut c_char,
    pub description: *mut c_char,
    pub size: usize,
    pub mtime: i32,
    pub ctime: i32,
    pub checksum: *mut glib::ffi::GString,
}

#[repr(C)]
pub struct PopplerFontDescription {
    pub font_name: *mut c_char,
    pub size_pt: c_double,
    pub stretch: c_int,
    pub weight: c_int,
    pub style: c_int,
}

pub const ERROR_ENCRYPTED: c_int = 1;

pub const FIND_CASE_SENSITIVE: c_uint = 1;
pub const FIND_WHOLE_WORDS_ONLY: c_uint = 4;
pub const FIND_MULTILINE: c_uint = 16;

pub const ANNOT_UNKNOWN: c_int = 0;
pub const ANNOT_TEXT: c_int = 1;
pub const ANNOT_LINK: c_int = 2;
pub const ANNOT_FREE_TEXT: c_int = 3;
pub const ANNOT_LINE: c_int = 4;
pub const ANNOT_SQUARE: c_int = 5;
pub const ANNOT_CIRCLE: c_int = 6;
pub const ANNOT_POLYGON: c_int = 7;
pub const ANNOT_POLY_LINE: c_int = 8;
pub const ANNOT_HIGHLIGHT: c_int = 9;
pub const ANNOT_UNDERLINE: c_int = 10;
pub const ANNOT_SQUIGGLY: c_int = 11;
pub const ANNOT_STRIKE_OUT: c_int = 12;
pub const ANNOT_STAMP: c_int = 13;
pub const ANNOT_CARET: c_int = 14;
pub const ANNOT_INK: c_int = 15;
pub const ANNOT_POPUP: c_int = 16;
pub const ANNOT_FILE_ATTACHMENT: c_int = 17;
pub const ANNOT_SOUND: c_int = 18;
pub const ANNOT_MOVIE: c_int = 19;
pub const ANNOT_WIDGET: c_int = 20;
pub const ANNOT_SCREEN: c_int = 21;

pub const ANNOT_FLAG_HIDDEN: c_uint = 1 << 1;
pub const ANNOT_FLAG_PRINT: c_uint = 1 << 2;

pub const FORM_FIELD_UNKNOWN: c_int = 0;
pub const FORM_FIELD_BUTTON: c_int = 1;
pub const FORM_FIELD_TEXT: c_int = 2;
pub const FORM_FIELD_CHOICE: c_int = 3;
pub const FORM_FIELD_SIGNATURE: c_int = 4;

pub const FORM_BUTTON_PUSH: c_int = 0;
pub const FORM_BUTTON_CHECK: c_int = 1;
pub const FORM_BUTTON_RADIO: c_int = 2;

pub const FORM_TEXT_NORMAL: c_int = 0;
pub const FORM_TEXT_MULTILINE: c_int = 1;
pub const FORM_TEXT_FILE_SELECT: c_int = 2;

pub const FORM_CHOICE_COMBO: c_int = 0;
pub const FORM_CHOICE_LIST: c_int = 1;

pub const SELECTION_GLYPH: c_int = 0;
pub const SELECTION_WORD: c_int = 1;
pub const SELECTION_LINE: c_int = 2;

pub const RENDER_ANNOTS_ALL: c_uint = (1 << 26) - 1;
pub const RENDER_ANNOTS_PRINT_DOCUMENT: c_uint = 1 << 19;
pub const RENDER_ANNOTS_PRINT_MARKUP: c_uint =
    !((1 << 1) | (1 << 15) | (1 << 18) | (1 << 20) | (1 << 21) | (1 << 22) | (1 << 23) | (1 << 24));
pub const RENDER_ANNOTS_PRINT_STAMP: c_uint = (1 << 19) | (1 << 12);

pub const PERMISSIONS_OK_TO_PRINT: c_uint = 1;
pub const PERMISSIONS_OK_TO_MODIFY: c_uint = 2;
pub const PERMISSIONS_OK_TO_COPY: c_uint = 4;
pub const PERMISSIONS_OK_TO_ADD_NOTES: c_uint = 8;

pub type PopplerAttachmentSaveFunc = Option<
    unsafe extern "C" fn(buf: *const c_char, count: usize, data: gpointer, error: *mut *mut GError) -> gboolean,
>;

unsafe extern "C" {
    pub fn poppler_get_version() -> *const c_char;

    pub fn poppler_document_new_from_gfile(
        file: *mut gio::ffi::GFile,
        password: *const c_char,
        cancellable: *mut gio::ffi::GCancellable,
        error: *mut *mut GError,
    ) -> *mut PopplerDocument;
    pub fn poppler_document_new_from_bytes(
        bytes: *mut GBytes,
        password: *const c_char,
        error: *mut *mut GError,
    ) -> *mut PopplerDocument;
    pub fn poppler_document_save(doc: *mut PopplerDocument, uri: *const c_char, error: *mut *mut GError) -> gboolean;
    pub fn poppler_document_save_a_copy(
        doc: *mut PopplerDocument,
        uri: *const c_char,
        error: *mut *mut GError,
    ) -> gboolean;
    pub fn poppler_document_get_n_pages(doc: *mut PopplerDocument) -> c_int;
    pub fn poppler_document_get_page(doc: *mut PopplerDocument, index: c_int) -> *mut PopplerPage;
    pub fn poppler_document_get_pdf_version_string(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_title(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_author(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_subject(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_keywords(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_creator(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_producer(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_creation_date_time(doc: *mut PopplerDocument) -> *mut GDateTime;
    pub fn poppler_document_get_modification_date_time(doc: *mut PopplerDocument) -> *mut GDateTime;
    pub fn poppler_document_is_linearized(doc: *mut PopplerDocument) -> gboolean;
    pub fn poppler_document_get_page_layout(doc: *mut PopplerDocument) -> c_int;
    pub fn poppler_document_get_page_mode(doc: *mut PopplerDocument) -> c_int;
    pub fn poppler_document_get_permissions(doc: *mut PopplerDocument) -> c_uint;
    pub fn poppler_document_get_metadata(doc: *mut PopplerDocument) -> *mut c_char;
    pub fn poppler_document_get_n_attachments(doc: *mut PopplerDocument) -> c_uint;
    pub fn poppler_document_has_attachments(doc: *mut PopplerDocument) -> gboolean;
    pub fn poppler_document_get_attachments(doc: *mut PopplerDocument) -> *mut GList;
    pub fn poppler_document_find_dest(doc: *mut PopplerDocument, name: *const c_char) -> *mut PopplerDest;
    pub fn poppler_document_reset_form(doc: *mut PopplerDocument, fields: *mut GList, exclude: gboolean);

    pub fn poppler_index_iter_new(doc: *mut PopplerDocument) -> *mut PopplerIndexIter;
    pub fn poppler_index_iter_free(iter: *mut PopplerIndexIter);
    pub fn poppler_index_iter_get_child(iter: *mut PopplerIndexIter) -> *mut PopplerIndexIter;
    pub fn poppler_index_iter_is_open(iter: *mut PopplerIndexIter) -> gboolean;
    pub fn poppler_index_iter_get_action(iter: *mut PopplerIndexIter) -> *mut PopplerAction;
    pub fn poppler_index_iter_next(iter: *mut PopplerIndexIter) -> gboolean;

    pub fn poppler_font_info_new(doc: *mut PopplerDocument) -> *mut PopplerFontInfo;
    pub fn poppler_font_info_scan(fi: *mut PopplerFontInfo, n_pages: c_int, iter: *mut *mut PopplerFontsIter) -> gboolean;
    pub fn poppler_font_info_free(fi: *mut PopplerFontInfo);
    pub fn poppler_fonts_iter_free(iter: *mut PopplerFontsIter);
    pub fn poppler_fonts_iter_get_name(iter: *mut PopplerFontsIter) -> *const c_char;
    pub fn poppler_fonts_iter_get_full_name(iter: *mut PopplerFontsIter) -> *const c_char;
    pub fn poppler_fonts_iter_get_substitute_name(iter: *mut PopplerFontsIter) -> *const c_char;
    pub fn poppler_fonts_iter_get_file_name(iter: *mut PopplerFontsIter) -> *const c_char;
    pub fn poppler_fonts_iter_get_font_type(iter: *mut PopplerFontsIter) -> c_int;
    pub fn poppler_fonts_iter_get_encoding(iter: *mut PopplerFontsIter) -> *const c_char;
    pub fn poppler_fonts_iter_is_embedded(iter: *mut PopplerFontsIter) -> gboolean;
    pub fn poppler_fonts_iter_is_subset(iter: *mut PopplerFontsIter) -> gboolean;
    pub fn poppler_fonts_iter_next(iter: *mut PopplerFontsIter) -> gboolean;

    pub fn poppler_layers_iter_new(doc: *mut PopplerDocument) -> *mut PopplerLayersIter;
    pub fn poppler_layers_iter_free(iter: *mut PopplerLayersIter);
    pub fn poppler_layers_iter_get_child(iter: *mut PopplerLayersIter) -> *mut PopplerLayersIter;
    pub fn poppler_layers_iter_get_title(iter: *mut PopplerLayersIter) -> *mut c_char;
    pub fn poppler_layers_iter_get_layer(iter: *mut PopplerLayersIter) -> *mut PopplerLayer;
    pub fn poppler_layers_iter_next(iter: *mut PopplerLayersIter) -> gboolean;
    pub fn poppler_layer_get_title(layer: *mut PopplerLayer) -> *const c_char;
    pub fn poppler_layer_is_visible(layer: *mut PopplerLayer) -> gboolean;
    pub fn poppler_layer_show(layer: *mut PopplerLayer);
    pub fn poppler_layer_hide(layer: *mut PopplerLayer);
    pub fn poppler_layer_get_radio_button_group_id(layer: *mut PopplerLayer) -> c_int;

    pub fn poppler_page_get_size(page: *mut PopplerPage, w: *mut c_double, h: *mut c_double);
    pub fn poppler_page_get_label(page: *mut PopplerPage) -> *mut c_char;
    pub fn poppler_page_get_duration(page: *mut PopplerPage) -> c_double;
    pub fn poppler_page_get_transition(page: *mut PopplerPage) -> *mut PopplerPageTransition;
    pub fn poppler_page_transition_free(t: *mut PopplerPageTransition);
    pub fn poppler_page_render_full(page: *mut PopplerPage, cr: *mut cairo_t, printing: gboolean, flags: c_uint);
    pub fn poppler_page_get_text(page: *mut PopplerPage) -> *mut c_char;
    pub fn poppler_page_get_text_layout(
        page: *mut PopplerPage,
        rects: *mut *mut PopplerRectangle,
        n: *mut c_uint,
    ) -> gboolean;
    pub fn poppler_page_get_selected_text(
        page: *mut PopplerPage,
        style: c_int,
        sel: *mut PopplerRectangle,
    ) -> *mut c_char;
    pub fn poppler_page_get_selected_region(
        page: *mut PopplerPage,
        scale: c_double,
        style: c_int,
        sel: *mut PopplerRectangle,
    ) -> *mut cairo_region_t;
    pub fn poppler_page_find_text_with_options(page: *mut PopplerPage, text: *const c_char, opts: c_uint) -> *mut GList;
    pub fn poppler_rectangle_find_get_match_continued(r: *const PopplerRectangle) -> gboolean;
    pub fn poppler_rectangle_free(r: *mut PopplerRectangle);
    pub fn poppler_page_get_link_mapping(page: *mut PopplerPage) -> *mut GList;
    pub fn poppler_page_free_link_mapping(list: *mut GList);
    pub fn poppler_page_get_image_mapping(page: *mut PopplerPage) -> *mut GList;
    pub fn poppler_page_free_image_mapping(list: *mut GList);
    pub fn poppler_page_get_image(page: *mut PopplerPage, id: c_int) -> *mut cairo_surface_t;
    pub fn poppler_page_get_annot_mapping(page: *mut PopplerPage) -> *mut GList;
    pub fn poppler_page_free_annot_mapping(list: *mut GList);
    pub fn poppler_page_add_annot(page: *mut PopplerPage, annot: *mut PopplerAnnot);
    pub fn poppler_page_remove_annot(page: *mut PopplerPage, annot: *mut PopplerAnnot);
    pub fn poppler_page_get_form_field_mapping(page: *mut PopplerPage) -> *mut GList;
    pub fn poppler_page_free_form_field_mapping(list: *mut GList);
    pub fn poppler_page_get_crop_box(page: *mut PopplerPage, rect: *mut PopplerRectangle);

    pub fn poppler_attachment_save_to_callback(
        a: *mut PopplerAttachment,
        func: PopplerAttachmentSaveFunc,
        data: gpointer,
        error: *mut *mut GError,
    ) -> gboolean;

    pub fn poppler_annot_get_annot_type(a: *mut PopplerAnnot) -> c_int;
    pub fn poppler_annot_get_contents(a: *mut PopplerAnnot) -> *mut c_char;
    pub fn poppler_annot_set_contents(a: *mut PopplerAnnot, c: *const c_char);
    pub fn poppler_annot_get_name(a: *mut PopplerAnnot) -> *mut c_char;
    pub fn poppler_annot_get_modified(a: *mut PopplerAnnot) -> *mut c_char;
    pub fn poppler_annot_get_flags(a: *mut PopplerAnnot) -> c_uint;
    pub fn poppler_annot_set_flags(a: *mut PopplerAnnot, f: c_uint);
    pub fn poppler_annot_get_color(a: *mut PopplerAnnot) -> *mut PopplerColor;
    pub fn poppler_annot_set_color(a: *mut PopplerAnnot, c: *mut PopplerColor);
    pub fn poppler_annot_get_rectangle(a: *mut PopplerAnnot, r: *mut PopplerRectangle);
    pub fn poppler_annot_set_rectangle(a: *mut PopplerAnnot, r: *mut PopplerRectangle);
    pub fn poppler_annot_markup_get_label(a: *mut PopplerAnnot) -> *mut c_char;
    pub fn poppler_annot_markup_set_label(a: *mut PopplerAnnot, l: *const c_char);
    pub fn poppler_annot_markup_has_popup(a: *mut PopplerAnnot) -> gboolean;
    pub fn poppler_annot_markup_set_popup(a: *mut PopplerAnnot, r: *mut PopplerRectangle);
    pub fn poppler_annot_markup_get_popup_is_open(a: *mut PopplerAnnot) -> gboolean;
    pub fn poppler_annot_markup_set_popup_is_open(a: *mut PopplerAnnot, open: gboolean);
    pub fn poppler_annot_markup_get_popup_rectangle(a: *mut PopplerAnnot, r: *mut PopplerRectangle) -> gboolean;
    pub fn poppler_annot_markup_set_popup_rectangle(a: *mut PopplerAnnot, r: *mut PopplerRectangle);
    pub fn poppler_annot_markup_get_opacity(a: *mut PopplerAnnot) -> c_double;
    pub fn poppler_annot_markup_set_opacity(a: *mut PopplerAnnot, o: c_double);
    pub fn poppler_annot_markup_get_subject(a: *mut PopplerAnnot) -> *mut c_char;
    pub fn poppler_annot_text_new(doc: *mut PopplerDocument, r: *mut PopplerRectangle) -> *mut PopplerAnnot;
    pub fn poppler_annot_text_get_icon(a: *mut PopplerAnnot) -> *mut c_char;
    pub fn poppler_annot_text_set_icon(a: *mut PopplerAnnot, icon: *const c_char);
    pub fn poppler_annot_text_get_is_open(a: *mut PopplerAnnot) -> gboolean;
    pub fn poppler_annot_text_set_is_open(a: *mut PopplerAnnot, open: gboolean);
    pub fn poppler_annot_text_markup_new_highlight(
        doc: *mut PopplerDocument,
        r: *mut PopplerRectangle,
        quads: *mut GArray,
    ) -> *mut PopplerAnnot;
    pub fn poppler_annot_text_markup_new_squiggly(
        doc: *mut PopplerDocument,
        r: *mut PopplerRectangle,
        quads: *mut GArray,
    ) -> *mut PopplerAnnot;
    pub fn poppler_annot_text_markup_new_strikeout(
        doc: *mut PopplerDocument,
        r: *mut PopplerRectangle,
        quads: *mut GArray,
    ) -> *mut PopplerAnnot;
    pub fn poppler_annot_text_markup_new_underline(
        doc: *mut PopplerDocument,
        r: *mut PopplerRectangle,
        quads: *mut GArray,
    ) -> *mut PopplerAnnot;
    pub fn poppler_annot_text_markup_get_quadrilaterals(a: *mut PopplerAnnot) -> *mut GArray;
    pub fn poppler_annot_file_attachment_get_attachment(a: *mut PopplerAnnot) -> *mut PopplerAttachment;
    pub fn poppler_annot_movie_get_movie(a: *mut PopplerAnnot) -> *mut PopplerMovie;
    pub fn poppler_annot_screen_get_action(a: *mut PopplerAnnot) -> *mut PopplerAction;
    pub fn poppler_annot_free_text_get_font_desc(a: *mut PopplerAnnot) -> *mut PopplerFontDescription;

    pub fn poppler_movie_get_filename(m: *mut PopplerMovie) -> *const c_char;
    pub fn poppler_movie_show_controls(m: *mut PopplerMovie) -> gboolean;
    pub fn poppler_media_is_embedded(m: *mut PopplerMedia) -> gboolean;
    pub fn poppler_media_get_filename(m: *mut PopplerMedia) -> *const c_char;
    pub fn poppler_media_get_show_controls(m: *mut PopplerMedia) -> gboolean;
    pub fn poppler_media_save(m: *mut PopplerMedia, filename: *const c_char, error: *mut *mut GError) -> gboolean;

    pub fn poppler_form_field_get_field_type(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_get_id(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_get_font_size(f: *mut PopplerFormField) -> c_double;
    pub fn poppler_form_field_is_read_only(f: *mut PopplerFormField) -> gboolean;
    pub fn poppler_form_field_get_name(f: *mut PopplerFormField) -> *mut c_char;
    pub fn poppler_form_field_get_alternate_ui_name(f: *mut PopplerFormField) -> *mut c_char;
    pub fn poppler_form_field_get_action(f: *mut PopplerFormField) -> *mut PopplerAction;
    pub fn poppler_form_field_button_get_button_type(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_button_get_state(f: *mut PopplerFormField) -> gboolean;
    pub fn poppler_form_field_button_set_state(f: *mut PopplerFormField, state: gboolean);
    pub fn poppler_form_field_text_get_text_type(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_text_get_text(f: *mut PopplerFormField) -> *mut c_char;
    pub fn poppler_form_field_text_set_text(f: *mut PopplerFormField, t: *const c_char);
    pub fn poppler_form_field_text_get_max_len(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_text_do_scroll(f: *mut PopplerFormField) -> gboolean;
    pub fn poppler_form_field_text_is_password(f: *mut PopplerFormField) -> gboolean;
    pub fn poppler_form_field_choice_get_choice_type(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_choice_is_editable(f: *mut PopplerFormField) -> gboolean;
    pub fn poppler_form_field_choice_can_select_multiple(f: *mut PopplerFormField) -> gboolean;
    pub fn poppler_form_field_choice_get_n_items(f: *mut PopplerFormField) -> c_int;
    pub fn poppler_form_field_choice_get_item(f: *mut PopplerFormField, i: c_int) -> *mut c_char;
    pub fn poppler_form_field_choice_is_item_selected(f: *mut PopplerFormField, i: c_int) -> gboolean;
    pub fn poppler_form_field_choice_select_item(f: *mut PopplerFormField, i: c_int);
    pub fn poppler_form_field_choice_unselect_all(f: *mut PopplerFormField);
    pub fn poppler_form_field_choice_toggle_item(f: *mut PopplerFormField, i: c_int);
    pub fn poppler_form_field_choice_set_text(f: *mut PopplerFormField, t: *const c_char);
    pub fn poppler_form_field_choice_get_text(f: *mut PopplerFormField) -> *mut c_char;
}
