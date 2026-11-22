use super::*;
use crate::ffi::poppler::*;
use crate::ffi::{cstr_opt, cstring, glist_iter, gstr_take, take_gerror};
use glib::translate::ToGlibPtr;
use std::collections::HashMap;
use std::ptr;

pub struct PdfDocument {
    doc: *mut PopplerDocument,
    pages: Vec<*mut PopplerPage>,
    sizes: Vec<(f64, f64)>,
    labels: Vec<Option<String>>,
    source: Source,
    modified: bool,
    annots: HashMap<usize, Vec<(usize, *mut PopplerAnnot)>>,
    next_annot_id: usize,
    fields: HashMap<usize, Vec<*mut PopplerFormField>>,
    layers: Vec<*mut PopplerLayer>,
    temp_media: Vec<PathBuf>,
}

unsafe impl Send for PdfDocument {}

impl Drop for PdfDocument {
    fn drop(&mut self) {
        unsafe {
            for (_, v) in self.annots.drain() {
                for (_, a) in v {
                    glib::gobject_ffi::g_object_unref(a as *mut _);
                }
            }
            for (_, v) in self.fields.drain() {
                for f in v {
                    glib::gobject_ffi::g_object_unref(f as *mut _);
                }
            }
            for p in self.pages.drain(..) {
                glib::gobject_ffi::g_object_unref(p as *mut _);
            }
            glib::gobject_ffi::g_object_unref(self.doc as *mut _);
        }
        for p in &self.temp_media {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn is_encrypted_error(err: *mut glib::ffi::GError) -> bool {
    if err.is_null() {
        return false;
    }
    unsafe {
        let domain = cstr_opt(glib::ffi::g_quark_to_string((*err).domain)).unwrap_or_default();
        domain == "poppler-quark" && (*err).code == ERROR_ENCRYPTED
    }
}

fn pcolor_to_color(c: *mut PopplerColor) -> Color {
    if c.is_null() {
        return Color::rgb(1.0, 1.0, 0.0);
    }
    let col = unsafe { *c };
    unsafe { glib::ffi::g_free(c as *mut _) };
    Color::rgb(col.red as f64 / 65535.0, col.green as f64 / 65535.0, col.blue as f64 / 65535.0)
}

fn color_to_pcolor(c: Color) -> PopplerColor {
    PopplerColor {
        red: (c.r * 65535.0).round() as u16,
        green: (c.g * 65535.0).round() as u16,
        blue: (c.b * 65535.0).round() as u16,
    }
}

impl PdfDocument {
    pub fn load(source: Source, password: Option<&str>) -> Result<Self> {
        let file = gio::File::for_path(&source.path);
        let pw = password.map(cstring);
        let mut err = ptr::null_mut();
        let doc = unsafe {
            poppler_document_new_from_gfile(
                file.to_glib_none().0,
                pw.as_ref().map_or(ptr::null(), |p| p.as_ptr()),
                ptr::null_mut(),
                &mut err,
            )
        };
        if doc.is_null() {
            if is_encrypted_error(err) {
                unsafe { glib::ffi::g_error_free(err) };
                return Err(Error::Encrypted);
            }
            return Err(Error::Invalid(unsafe { take_gerror(err) }));
        }
        let n = unsafe { poppler_document_get_n_pages(doc) }.max(0) as usize;
        let mut pages = Vec::with_capacity(n);
        let mut sizes = Vec::with_capacity(n);
        let mut labels = Vec::with_capacity(n);
        for i in 0..n {
            let p = unsafe { poppler_document_get_page(doc, i as i32) };
            let (mut w, mut h) = (0.0, 0.0);
            if !p.is_null() {
                unsafe { poppler_page_get_size(p, &mut w, &mut h) };
                labels.push(unsafe { gstr_take(poppler_page_get_label(p)) });
            } else {
                labels.push(None);
            }
            pages.push(p);
            sizes.push((w, h));
        }
        if n == 0 {
            unsafe { glib::gobject_ffi::g_object_unref(doc as *mut _) };
            return Err(Error::Invalid("Document has no pages".into()));
        }
        Ok(PdfDocument {
            doc,
            pages,
            sizes,
            labels,
            source,
            modified: false,
            annots: HashMap::new(),
            next_annot_id: 1,
            fields: HashMap::new(),
            layers: Vec::new(),
            temp_media: Vec::new(),
        })
    }

    fn page(&self, i: usize) -> *mut PopplerPage {
        self.pages.get(i).copied().unwrap_or(ptr::null_mut())
    }

    fn height(&self, i: usize) -> f64 {
        self.sizes.get(i).map_or(0.0, |s| s.1)
    }

    fn convert_dest(&self, d: *mut PopplerDest) -> Option<LinkDest> {
        if d.is_null() {
            return None;
        }
        let d = unsafe { &*d };
        if d.type_ == DEST_NAMED {
            return unsafe { cstr_opt(d.named_dest) }.map(LinkDest::Named);
        }
        let page = (d.page_num.max(1) - 1) as usize;
        let h = self.height(page);
        let flags = d.change_flags;
        let top = || Some(h - d.top.min(h));
        Some(match d.type_ {
            DEST_XYZ => LinkDest::Xyz {
                page,
                left: (flags & DEST_CHANGE_LEFT != 0).then_some(d.left),
                top: if flags & DEST_CHANGE_TOP != 0 { top() } else { None },
                zoom: (flags & DEST_CHANGE_ZOOM != 0 && d.zoom > 0.0).then_some(d.zoom),
            },
            DEST_FIT | DEST_FITB => LinkDest::Fit { page },
            DEST_FITH | DEST_FITBH => LinkDest::FitH { page, top: if flags & DEST_CHANGE_TOP != 0 { top() } else { None } },
            DEST_FITV | DEST_FITBV => {
                LinkDest::FitV { page, left: (flags & DEST_CHANGE_LEFT != 0).then_some(d.left) }
            }
            DEST_FITR => LinkDest::FitR {
                page,
                rect: Rect::new(
                    d.left,
                    h - d.top.max(d.bottom).min(h),
                    d.right,
                    h - d.top.min(d.bottom).min(h),
                )
                .normalized(),
            },
            _ => LinkDest::Page(page),
        })
    }

    fn convert_action(&mut self, a: *mut PopplerAction) -> Option<Link> {
        if a.is_null() {
            return None;
        }
        unsafe {
            let ty = (*a).type_;
            let title = cstr_opt((*a).any.title);
            let action = match ty {
                ACTION_GOTO_DEST => self.convert_dest((*a).goto_dest.dest).map(LinkAction::GotoDest),
                ACTION_GOTO_REMOTE => cstr_opt((*a).goto_remote.file_name).map(|file| LinkAction::GotoRemote {
                    file,
                    dest: self.convert_dest((*a).goto_remote.dest),
                }),
                ACTION_LAUNCH => cstr_opt((*a).launch.file_name)
                    .map(|file| LinkAction::Launch { file, params: cstr_opt((*a).launch.params) }),
                ACTION_URI => cstr_opt((*a).uri.uri).map(LinkAction::ExternalUri),
                ACTION_NAMED => cstr_opt((*a).named.named_dest).map(LinkAction::Named),
                ACTION_OCG_STATE => {
                    self.ensure_layers();
                    let mut states = Vec::new();
                    for st in glist_iter::<PopplerActionLayer>((*a).ocg_state.state_list) {
                        let act = match (*st).action {
                            0 => LayerAction::On,
                            1 => LayerAction::Off,
                            _ => LayerAction::Toggle,
                        };
                        let ids = glist_iter::<PopplerLayer>((*st).layers)
                            .into_iter()
                            .filter_map(|l| self.layer_id(l))
                            .collect();
                        states.push((act, ids));
                    }
                    Some(LinkAction::Layers(states))
                }
                ACTION_RESET_FORM => {
                    let fields = glist_iter::<std::os::raw::c_char>((*a).reset_form.fields)
                        .into_iter()
                        .filter_map(|p| cstr_opt(p))
                        .collect();
                    Some(LinkAction::ResetForm { fields, exclude: (*a).reset_form.exclude != 0 })
                }
                ACTION_RENDITION => {
                    let m = (*a).rendition.media;
                    self.media_from_poppler(m).map(|media| LinkAction::Media { page: 0, media })
                }
                _ => None,
            };
            if action.is_none() && title.is_none() {
                return None;
            }
            Some(Link { title, action })
        }
    }

    fn media_from_poppler(&mut self, m: *mut PopplerMedia) -> Option<Media> {
        if m.is_null() {
            return None;
        }
        unsafe {
            let show_controls = poppler_media_get_show_controls(m) != 0;
            if poppler_media_is_embedded(m) != 0 {
                let path = glib::tmp_dir().join(format!("evince-rs-media-{}-{}", std::process::id(), self.temp_media.len()));
                let cpath = cstring(&path.to_string_lossy());
                let mut err = ptr::null_mut();
                if poppler_media_save(m, cpath.as_ptr(), &mut err) == 0 {
                    take_gerror(err);
                    return None;
                }
                self.temp_media.push(path.clone());
                Some(Media { uri: glib::filename_to_uri(&path, None).ok()?.to_string(), show_controls })
            } else {
                let f = cstr_opt(poppler_media_get_filename(m))?;
                Some(Media { uri: self.resolve_uri(&f), show_controls })
            }
        }
    }

    fn resolve_uri(&self, f: &str) -> String {
        if f.contains("://") {
            return f.to_string();
        }
        let p = Path::new(f);
        let p = if p.is_absolute() { p.to_path_buf() } else { self.source.path.parent().unwrap_or(Path::new("/")).join(p) };
        glib::filename_to_uri(&p, None).map(|s| s.to_string()).unwrap_or_default()
    }

    fn ensure_layers(&mut self) {
        if !self.layers.is_empty() {
            return;
        }
        unsafe {
            let iter = poppler_layers_iter_new(self.doc);
            if iter.is_null() {
                return;
            }
            fn walk(iter: *mut PopplerLayersIter, out: &mut Vec<*mut PopplerLayer>) {
                unsafe {
                    loop {
                        let l = poppler_layers_iter_get_layer(iter);
                        if !l.is_null() {
                            out.push(l);
                        }
                        let child = poppler_layers_iter_get_child(iter);
                        if !child.is_null() {
                            walk(child, out);
                            poppler_layers_iter_free(child);
                        }
                        if poppler_layers_iter_next(iter) == 0 {
                            break;
                        }
                    }
                }
            }
            walk(iter, &mut self.layers);
            poppler_layers_iter_free(iter);
        }
    }

    fn layer_id(&self, l: *mut PopplerLayer) -> Option<usize> {
        let title = unsafe { cstr_opt(poppler_layer_get_title(l)) };
        self.layers
            .iter()
            .position(|&x| x == l)
            .or_else(|| self.layers.iter().position(|&x| unsafe { cstr_opt(poppler_layer_get_title(x)) } == title))
    }

    fn ensure_annots(&mut self, page: usize) {
        if self.annots.contains_key(&page) {
            return;
        }
        let p = self.page(page);
        let mut v = Vec::new();
        if !p.is_null() {
            unsafe {
                let list = poppler_page_get_annot_mapping(p);
                for m in glist_iter::<PopplerAnnotMapping>(list) {
                    let a = (*m).annot;
                    let ty = poppler_annot_get_annot_type(a);
                    if ty == ANNOT_LINK || ty == ANNOT_POPUP || ty == ANNOT_WIDGET {
                        continue;
                    }
                    glib::gobject_ffi::g_object_ref(a as *mut _);
                    v.push((self.next_annot_id, a));
                    self.next_annot_id += 1;
                }
                poppler_page_free_annot_mapping(list);
            }
        }
        self.annots.insert(page, v);
    }

    fn annot_ptr(&mut self, page: usize, id: usize) -> Option<*mut PopplerAnnot> {
        self.ensure_annots(page);
        self.annots.get(&page)?.iter().find(|(i, _)| *i == id).map(|(_, a)| *a)
    }

    fn convert_annot(&self, page: usize, id: usize, a: *mut PopplerAnnot) -> Option<Annotation> {
        let h = self.height(page);
        unsafe {
            let ty = poppler_annot_get_annot_type(a);
            let mut r = PopplerRectangle::default();
            poppler_annot_get_rectangle(a, &mut r);
            let area = Rect::new(r.x1, h - r.y2, r.x2, h - r.y1).normalized();
            let is_markup = matches!(
                ty,
                ANNOT_TEXT
                    | ANNOT_FREE_TEXT
                    | ANNOT_LINE
                    | ANNOT_SQUARE
                    | ANNOT_CIRCLE
                    | ANNOT_POLYGON
                    | ANNOT_POLY_LINE
                    | ANNOT_HIGHLIGHT
                    | ANNOT_UNDERLINE
                    | ANNOT_SQUIGGLY
                    | ANNOT_STRIKE_OUT
                    | ANNOT_STAMP
                    | ANNOT_CARET
                    | ANNOT_INK
                    | ANNOT_FILE_ATTACHMENT
                    | ANNOT_SOUND
            );
            let kind = match ty {
                ANNOT_TEXT => AnnotKind::Text {
                    icon: TextIcon::from_name(&gstr_take(poppler_annot_text_get_icon(a)).unwrap_or_default()),
                    is_open: poppler_annot_text_get_is_open(a) != 0,
                },
                ANNOT_HIGHLIGHT => AnnotKind::TextMarkup(MarkupType::Highlight),
                ANNOT_UNDERLINE => AnnotKind::TextMarkup(MarkupType::Underline),
                ANNOT_SQUIGGLY => AnnotKind::TextMarkup(MarkupType::Squiggly),
                ANNOT_STRIKE_OUT => AnnotKind::TextMarkup(MarkupType::StrikeOut),
                ANNOT_FREE_TEXT => AnnotKind::FreeText,
                ANNOT_FILE_ATTACHMENT => {
                    let att = poppler_annot_file_attachment_get_attachment(a);
                    let data = if att.is_null() {
                        None
                    } else {
                        let r = attachment_data(att).map(|data| {
                            std::sync::Arc::new(AttachmentRef {
                                name: cstr_opt((*att).name).unwrap_or_else(|| "attachment".into()),
                                description: cstr_opt((*att).description),
                                data,
                            })
                        });
                        glib::gobject_ffi::g_object_unref(att as *mut _);
                        r
                    };
                    AnnotKind::Attachment(data)
                }
                ANNOT_LINE => AnnotKind::Other("Line"),
                ANNOT_SQUARE => AnnotKind::Other("Square"),
                ANNOT_CIRCLE => AnnotKind::Other("Circle"),
                ANNOT_POLYGON => AnnotKind::Other("Polygon"),
                ANNOT_POLY_LINE => AnnotKind::Other("Polyline"),
                ANNOT_STAMP => AnnotKind::Other("Stamp"),
                ANNOT_CARET => AnnotKind::Other("Caret"),
                ANNOT_INK => AnnotKind::Other("Ink"),
                ANNOT_SOUND => AnnotKind::Other("Sound"),
                ANNOT_MOVIE => AnnotKind::Other("Movie"),
                ANNOT_SCREEN => AnnotKind::Other("Screen"),
                _ => AnnotKind::Other("Annotation"),
            };
            let flags = poppler_annot_get_flags(a);
            let (label, opacity, has_popup, popup_rect, popup_is_open) = if is_markup {
                let mut pr = PopplerRectangle::default();
                let has_rect = poppler_annot_markup_get_popup_rectangle(a, &mut pr) != 0;
                (
                    gstr_take(poppler_annot_markup_get_label(a)),
                    poppler_annot_markup_get_opacity(a),
                    poppler_annot_markup_has_popup(a) != 0,
                    has_rect.then(|| Rect::new(pr.x1, h - pr.y2, pr.x2, h - pr.y1).normalized()),
                    poppler_annot_markup_get_popup_is_open(a) != 0,
                )
            } else {
                (None, 1.0, false, None, false)
            };
            Some(Annotation {
                id,
                page,
                kind,
                area,
                contents: gstr_take(poppler_annot_get_contents(a)).unwrap_or_default(),
                name: gstr_take(poppler_annot_get_name(a)),
                modified: gstr_take(poppler_annot_get_modified(a)),
                color: pcolor_to_color(poppler_annot_get_color(a)),
                is_markup,
                label,
                opacity,
                has_popup,
                popup_rect,
                popup_is_open,
                hidden: flags & ANNOT_FLAG_HIDDEN != 0,
            })
        }
    }

    fn ensure_fields(&mut self, page: usize) {
        if self.fields.contains_key(&page) {
            return;
        }
        let p = self.page(page);
        let mut v = Vec::new();
        if !p.is_null() {
            unsafe {
                let list = poppler_page_get_form_field_mapping(p);
                for m in glist_iter::<PopplerFormFieldMapping>(list) {
                    glib::gobject_ffi::g_object_ref((*m).field as *mut _);
                    v.push((*m).field);
                }
                poppler_page_free_form_field_mapping(list);
            }
        }
        self.fields.insert(page, v);
    }

    fn field_ptr(&mut self, page: usize, id: i32) -> Option<*mut PopplerFormField> {
        self.ensure_fields(page);
        self.fields
            .get(&page)?
            .iter()
            .copied()
            .find(|&f| unsafe { poppler_form_field_get_id(f) } == id)
    }

    fn quads_for_area(&self, page: usize, area: Rect) -> (Vec<PopplerQuadrilateral>, Option<Rect>) {
        let h = self.height(page);
        let mut quads = Vec::new();
        let mut bbox: Option<Rect> = None;
        let rects = selected_region_rects(self.page(page), SelectionStyle::Glyph, area);
        for r in rects {
            quads.push(PopplerQuadrilateral {
                p1: PopplerPoint { x: r.x1, y: h - r.y1 },
                p2: PopplerPoint { x: r.x2, y: h - r.y1 },
                p3: PopplerPoint { x: r.x1, y: h - r.y2 },
                p4: PopplerPoint { x: r.x2, y: h - r.y2 },
            });
            bbox = Some(bbox.map_or(r, |b| b.union(&r)));
        }
        (quads, bbox)
    }
}

fn selected_region_rects(page: *mut PopplerPage, style: SelectionStyle, sel: Rect) -> Vec<Rect> {
    if page.is_null() {
        return Vec::new();
    }
    const SCALE: f64 = 4.0;
    let mut r = PopplerRectangle { x1: sel.x1, y1: sel.y1, x2: sel.x2, y2: sel.y2 };
    let st = match style {
        SelectionStyle::Glyph => SELECTION_GLYPH,
        SelectionStyle::Word => SELECTION_WORD,
        SelectionStyle::Line => SELECTION_LINE,
    };
    let reg = unsafe { poppler_page_get_selected_region(page, SCALE, st, &mut r) };
    if reg.is_null() {
        return Vec::new();
    }
    let region: cairo::Region = unsafe { glib::translate::from_glib_full(reg) };
    (0..region.num_rectangles())
        .map(|i| {
            let r = region.rectangle(i);
            Rect::new(
                r.x() as f64 / SCALE,
                r.y() as f64 / SCALE,
                (r.x() + r.width()) as f64 / SCALE,
                (r.y() + r.height()) as f64 / SCALE,
            )
        })
        .collect()
}

unsafe extern "C" fn collect_bytes(
    buf: *const std::os::raw::c_char,
    count: usize,
    data: glib::ffi::gpointer,
    _err: *mut *mut glib::ffi::GError,
) -> glib::ffi::gboolean {
    let v = unsafe { &mut *(data as *mut Vec<u8>) };
    v.extend_from_slice(unsafe { std::slice::from_raw_parts(buf as *const u8, count) });
    1
}

fn attachment_data(att: *mut PopplerAttachment) -> Option<Vec<u8>> {
    let mut v: Vec<u8> = Vec::new();
    let mut err = ptr::null_mut();
    let ok = unsafe {
        poppler_attachment_save_to_callback(att, Some(collect_bytes), &mut v as *mut Vec<u8> as *mut _, &mut err)
    };
    if ok == 0 {
        unsafe { take_gerror(err) };
        return None;
    }
    Some(v)
}

fn gdatetime_unix(dt: *mut glib::ffi::GDateTime) -> Option<i64> {
    if dt.is_null() {
        return None;
    }
    let t = unsafe { glib::ffi::g_date_time_to_unix(dt) };
    unsafe { glib::ffi::g_date_time_unref(dt) };
    Some(t)
}

fn font_type_name(t: i32) -> &'static str {
    match t {
        1 => "Type 1",
        2 => "Type 1C",
        3 => "Type 1C (OpenType)",
        4 => "Type 3",
        5 => "TrueType",
        6 => "TrueType (OpenType)",
        7 => "Type 1 (CID)",
        8 => "Type 1C (CID)",
        9 => "Type 1C (CID, OpenType)",
        10 => "TrueType (CID)",
        11 => "TrueType (CID, OpenType)",
        _ => "Unknown font type",
    }
}

impl Document for PdfDocument {
    fn n_pages(&self) -> usize {
        self.pages.len()
    }

    fn page_size(&self, page: usize) -> (f64, f64) {
        self.sizes.get(page).copied().unwrap_or((612.0, 792.0))
    }

    fn page_label(&self, page: usize) -> Option<String> {
        self.labels.get(page).cloned().flatten()
    }

    fn has_page_labels(&self) -> bool {
        self.labels
            .iter()
            .enumerate()
            .any(|(i, l)| l.as_deref().is_some_and(|l| l != (i + 1).to_string()))
    }

    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        let (w, h) = self.page_size(page);
        vector_surface(w, h, scale, rotation, |cr| unsafe {
            poppler_page_render_full(p, cr.to_raw_none(), 0, RENDER_ANNOTS_ALL);
        })
    }

    fn print_page(&mut self, page: usize, cr: &cairo::Context) {
        let p = self.page(page);
        if !p.is_null() {
            unsafe { poppler_page_render_full(p, cr.to_raw_none(), 1, RENDER_ANNOTS_PRINT_MARKUP) };
        }
    }

    fn info(&self) -> DocInfo {
        unsafe {
            let d = self.doc;
            let perms = poppler_document_get_permissions(d);
            let xmp = gstr_take(poppler_document_get_metadata(d));
            let layout = match poppler_document_get_page_layout(d) {
                1 => PageLayoutHint::SinglePage,
                2 => PageLayoutHint::OneColumn,
                3 => PageLayoutHint::TwoColumnLeft,
                4 => PageLayoutHint::TwoColumnRight,
                5 => PageLayoutHint::TwoPageLeft,
                6 => PageLayoutHint::TwoPageRight,
                _ => PageLayoutHint::Unset,
            };
            let start_mode = match poppler_document_get_page_mode(d) {
                2 => StartModeHint::UseOutlines,
                3 => StartModeHint::UseThumbs,
                4 => StartModeHint::FullScreen,
                5 => StartModeHint::UseOc,
                6 => StartModeHint::UseAttachments,
                _ => StartModeHint::None,
            };
            let (w, h) = self.page_size(0);
            DocInfo {
                title: gstr_take(poppler_document_get_title(d)),
                format: gstr_take(poppler_document_get_pdf_version_string(d)),
                author: gstr_take(poppler_document_get_author(d)),
                subject: gstr_take(poppler_document_get_subject(d)),
                keywords: gstr_take(poppler_document_get_keywords(d)),
                creator: gstr_take(poppler_document_get_creator(d)),
                producer: gstr_take(poppler_document_get_producer(d)),
                creation_date: gdatetime_unix(poppler_document_get_creation_date_time(d)),
                modified_date: gdatetime_unix(poppler_document_get_modification_date_time(d)),
                linearized: Some(poppler_document_is_linearized(d) != 0),
                security: None,
                n_pages: self.pages.len(),
                paper_size: Some((w / 72.0 * 25.4, h / 72.0 * 25.4)),
                layout,
                start_mode,
                permissions: Permissions {
                    print: perms & PERMISSIONS_OK_TO_PRINT != 0,
                    modify: perms & PERMISSIONS_OK_TO_MODIFY != 0,
                    copy: perms & PERMISSIONS_OK_TO_COPY != 0,
                    add_notes: perms & PERMISSIONS_OK_TO_ADD_NOTES != 0,
                },
                contains_js: false,
                license: xmp.as_deref().and_then(misc::xmp_license),
            }
        }
    }

    fn mime_type(&self) -> &str {
        &self.source.mime
    }

    fn has_text(&self) -> bool {
        true
    }

    fn text_layout(&mut self, page: usize) -> Option<TextLayout> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        unsafe {
            let text = gstr_take(poppler_page_get_text(p)).unwrap_or_default();
            let mut rects: *mut PopplerRectangle = ptr::null_mut();
            let mut n: u32 = 0;
            if poppler_page_get_text_layout(p, &mut rects, &mut n) == 0 {
                return Some(TextLayout::default());
            }
            let slice = std::slice::from_raw_parts(rects, n as usize);
            let chars: Vec<char> = text.chars().collect();
            let count = chars.len().min(slice.len());
            let layout = TextLayout {
                chars: chars[..count].to_vec(),
                rects: slice[..count].iter().map(|r| Rect::new(r.x1, r.y1, r.x2, r.y2)).collect(),
            };
            glib::ffi::g_free(rects as *mut _);
            Some(layout)
        }
    }

    fn page_text(&mut self, page: usize) -> Option<String> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        unsafe { gstr_take(poppler_page_get_text(p)) }
    }

    fn selection_region(&mut self, page: usize, style: SelectionStyle, sel: Rect) -> Vec<Rect> {
        selected_region_rects(self.page(page), style, sel)
    }

    fn selected_text(&mut self, page: usize, style: SelectionStyle, sel: Rect) -> String {
        let p = self.page(page);
        if p.is_null() {
            return String::new();
        }
        let st = match style {
            SelectionStyle::Glyph => SELECTION_GLYPH,
            SelectionStyle::Word => SELECTION_WORD,
            SelectionStyle::Line => SELECTION_LINE,
        };
        let mut r = PopplerRectangle { x1: sel.x1, y1: sel.y1, x2: sel.x2, y2: sel.y2 };
        unsafe { gstr_take(poppler_page_get_selected_text(p, st, &mut r)) }.unwrap_or_default()
    }

    fn find(&mut self, page: usize, text: &str, opts: FindOptions) -> Vec<FindMatch> {
        let p = self.page(page);
        if p.is_null() || text.is_empty() {
            return Vec::new();
        }
        let h = self.height(page);
        let mut flags = FIND_MULTILINE;
        if opts.case_sensitive {
            flags |= FIND_CASE_SENSITIVE;
        }
        if opts.whole_words {
            flags |= FIND_WHOLE_WORDS_ONLY;
        }
        let ctext = cstring(text);
        let mut out: Vec<FindMatch> = Vec::new();
        unsafe {
            let list = poppler_page_find_text_with_options(p, ctext.as_ptr(), flags);
            let mut continuing = false;
            for r in glist_iter::<PopplerRectangle>(list) {
                let rect = Rect::new((*r).x1, h - (*r).y2, (*r).x2, h - (*r).y1);
                if continuing {
                    if let Some(last) = out.last_mut() {
                        last.rects.push(rect);
                    }
                } else {
                    out.push(FindMatch { rects: vec![rect] });
                }
                continuing = poppler_rectangle_find_get_match_continued(r) != 0;
                poppler_rectangle_free(r);
            }
            glib::ffi::g_list_free(list);
        }
        out
    }

    fn links(&mut self, page: usize) -> Vec<Mapping<Link>> {
        let p = self.page(page);
        if p.is_null() {
            return Vec::new();
        }
        let h = self.height(page);
        let mut out = Vec::new();
        unsafe {
            let list = poppler_page_get_link_mapping(p);
            for m in glist_iter::<PopplerLinkMapping>(list) {
                if let Some(link) = self.convert_action((*m).action) {
                    if link.action.is_none() {
                        continue;
                    }
                    let a = (*m).area;
                    out.push(Mapping { area: Rect::new(a.x1, h - a.y2, a.x2, h - a.y1).normalized(), data: link });
                }
            }
            poppler_page_free_link_mapping(list);
        }
        out
    }

    fn outline(&mut self) -> Vec<OutlineItem> {
        fn walk(this: &mut PdfDocument, iter: *mut PopplerIndexIter, out: &mut Vec<OutlineItem>) {
            unsafe {
                loop {
                    let action = poppler_index_iter_get_action(iter);
                    let link = this.convert_action(action);
                    if !action.is_null() {
                        poppler_action_free(action);
                    }
                    let title = link.as_ref().and_then(|l| l.title.clone()).unwrap_or_default();
                    let mut item = OutlineItem {
                        title,
                        link,
                        expand: poppler_index_iter_is_open(iter) != 0,
                        children: Vec::new(),
                    };
                    let child = poppler_index_iter_get_child(iter);
                    if !child.is_null() {
                        walk(this, child, &mut item.children);
                        poppler_index_iter_free(child);
                    }
                    out.push(item);
                    if poppler_index_iter_next(iter) == 0 {
                        break;
                    }
                }
            }
        }
        let mut out = Vec::new();
        unsafe {
            let iter = poppler_index_iter_new(self.doc);
            if !iter.is_null() {
                walk(self, iter, &mut out);
                poppler_index_iter_free(iter);
            }
        }
        out
    }

    fn resolve_named_dest(&mut self, name: &str) -> Option<LinkDest> {
        let c = cstring(name);
        unsafe {
            let d = poppler_document_find_dest(self.doc, c.as_ptr());
            if d.is_null() {
                return None;
            }
            let r = self.convert_dest(d);
            poppler_dest_free(d);
            r
        }
    }

    fn images(&mut self, page: usize) -> Vec<Mapping<i32>> {
        let p = self.page(page);
        if p.is_null() {
            return Vec::new();
        }
        let mut out = Vec::new();
        unsafe {
            let list = poppler_page_get_image_mapping(p);
            for m in glist_iter::<PopplerImageMapping>(list) {
                let a = (*m).area;
                out.push(Mapping { area: Rect::new(a.x1, a.y1, a.x2, a.y2).normalized(), data: (*m).image_id });
            }
            poppler_page_free_image_mapping(list);
        }
        out
    }

    fn image(&mut self, page: usize, id: i32) -> Option<cairo::ImageSurface> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        unsafe {
            let s = poppler_page_get_image(p, id);
            if s.is_null() {
                return None;
            }
            let surf = cairo::Surface::from_raw_full(s).ok()?;
            cairo::ImageSurface::try_from(surf).ok()
        }
    }

    fn attachments(&mut self) -> Vec<Attachment> {
        let mut out = Vec::new();
        unsafe {
            if poppler_document_has_attachments(self.doc) == 0 {
                return out;
            }
            let list = poppler_document_get_attachments(self.doc);
            for a in glist_iter::<PopplerAttachment>(list) {
                let data = attachment_data(a).unwrap_or_default();
                out.push(Attachment {
                    name: cstr_opt((*a).name).unwrap_or_else(|| "attachment".into()),
                    description: cstr_opt((*a).description),
                    mtime: ((*a).mtime > 0).then_some((*a).mtime as i64),
                    ctime: ((*a).ctime > 0).then_some((*a).ctime as i64),
                    data: std::sync::Arc::new(data),
                });
                glib::gobject_ffi::g_object_unref(a as *mut _);
            }
            glib::ffi::g_list_free(list);
        }
        out
    }

    fn fonts(&mut self) -> Vec<FontInfo> {
        let mut out = Vec::new();
        unsafe {
            let fi = poppler_font_info_new(self.doc);
            let mut iter = ptr::null_mut();
            if poppler_font_info_scan(fi, self.pages.len() as i32, &mut iter) != 0 && !iter.is_null() {
                loop {
                    let name = cstr_opt(poppler_fonts_iter_get_full_name(iter))
                        .or_else(|| cstr_opt(poppler_fonts_iter_get_name(iter)))
                        .unwrap_or_else(|| "No name".into());
                    let embedded = if poppler_fonts_iter_is_embedded(iter) != 0 {
                        if poppler_fonts_iter_is_subset(iter) != 0 { "Embedded subset" } else { "Embedded" }
                    } else {
                        "Not embedded"
                    };
                    let mut details = format!("{}\n{}", font_type_name(poppler_fonts_iter_get_font_type(iter)), embedded);
                    if let Some(enc) = cstr_opt(poppler_fonts_iter_get_encoding(iter)) {
                        details.push_str(&format!("\nEncoding: {enc}"));
                    }
                    if poppler_fonts_iter_is_embedded(iter) == 0 {
                        if let Some(sub) = cstr_opt(poppler_fonts_iter_get_substitute_name(iter)) {
                            details.push_str(&format!("\nSubstituting with {sub}"));
                            if let Some(f) = cstr_opt(poppler_fonts_iter_get_file_name(iter)) {
                                details.push_str(&format!("\n({f})"));
                            }
                        }
                    }
                    out.push(FontInfo { name, details });
                    if poppler_fonts_iter_next(iter) == 0 {
                        break;
                    }
                }
                poppler_fonts_iter_free(iter);
            }
            poppler_font_info_free(fi);
        }
        out
    }

    fn layers(&mut self) -> Vec<Layer> {
        self.ensure_layers();
        fn walk(this: &PdfDocument, iter: *mut PopplerLayersIter, out: &mut Vec<Layer>) {
            unsafe {
                loop {
                    let l = poppler_layers_iter_get_layer(iter);
                    let mut layer = if l.is_null() {
                        Layer {
                            id: usize::MAX,
                            title: gstr_take(poppler_layers_iter_get_title(iter)).unwrap_or_default(),
                            visible: false,
                            title_only: true,
                            radio_group: 0,
                            children: Vec::new(),
                        }
                    } else {
                        let layer = Layer {
                            id: this.layer_id(l).unwrap_or(usize::MAX),
                            title: cstr_opt(poppler_layer_get_title(l)).unwrap_or_default(),
                            visible: poppler_layer_is_visible(l) != 0,
                            title_only: false,
                            radio_group: poppler_layer_get_radio_button_group_id(l),
                            children: Vec::new(),
                        };
                        glib::gobject_ffi::g_object_unref(l as *mut _);
                        layer
                    };
                    let child = poppler_layers_iter_get_child(iter);
                    if !child.is_null() {
                        walk(this, child, &mut layer.children);
                        poppler_layers_iter_free(child);
                    }
                    out.push(layer);
                    if poppler_layers_iter_next(iter) == 0 {
                        break;
                    }
                }
            }
        }
        let mut out = Vec::new();
        unsafe {
            let iter = poppler_layers_iter_new(self.doc);
            if !iter.is_null() {
                walk(self, iter, &mut out);
                poppler_layers_iter_free(iter);
            }
        }
        out
    }

    fn set_layer_visible(&mut self, id: usize, visible: bool) {
        self.ensure_layers();
        if let Some(&l) = self.layers.get(id) {
            unsafe {
                if visible {
                    poppler_layer_show(l)
                } else {
                    poppler_layer_hide(l)
                }
            }
        }
    }

    fn annotations(&mut self, page: usize) -> Vec<Annotation> {
        self.ensure_annots(page);
        let list = self.annots.get(&page).cloned().unwrap_or_default();
        list.into_iter().filter_map(|(id, a)| self.convert_annot(page, id, a)).collect()
    }

    fn can_add_annotations(&self) -> bool {
        true
    }

    fn add_annotation(&mut self, page: usize, kind: &NewAnnot, area: Rect, color: Color, author: &str) -> Option<Annotation> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        self.ensure_annots(page);
        let h = self.height(page);
        let mut prect = PopplerRectangle { x1: area.x1, y1: h - area.y2, x2: area.x2, y2: h - area.y1 };
        let annot = unsafe {
            match kind {
                NewAnnot::Text { icon } => {
                    let a = poppler_annot_text_new(self.doc, &mut prect);
                    let ic = cstring(icon.name());
                    poppler_annot_text_set_icon(a, ic.as_ptr());
                    a
                }
                NewAnnot::TextMarkup(mt) => {
                    let (quads, bbox) = self.quads_for_area(page, area);
                    if quads.is_empty() {
                        return None;
                    }
                    if let Some(b) = bbox {
                        prect = PopplerRectangle { x1: b.x1, y1: h - b.y2, x2: b.x2, y2: h - b.y1 };
                    }
                    let arr = glib::ffi::g_array_sized_new(
                        0,
                        1,
                        std::mem::size_of::<PopplerQuadrilateral>() as u32,
                        quads.len() as u32,
                    );
                    glib::ffi::g_array_append_vals(arr, quads.as_ptr() as *const _, quads.len() as u32);
                    let a = match mt {
                        MarkupType::Highlight => poppler_annot_text_markup_new_highlight(self.doc, &mut prect, arr),
                        MarkupType::StrikeOut => poppler_annot_text_markup_new_strikeout(self.doc, &mut prect, arr),
                        MarkupType::Underline => poppler_annot_text_markup_new_underline(self.doc, &mut prect, arr),
                        MarkupType::Squiggly => poppler_annot_text_markup_new_squiggly(self.doc, &mut prect, arr),
                    };
                    glib::ffi::g_array_unref(arr);
                    a
                }
            }
        };
        if annot.is_null() {
            return None;
        }
        unsafe {
            let mut c = color_to_pcolor(color);
            poppler_annot_set_color(annot, &mut c);
            let label = cstring(author);
            poppler_annot_markup_set_label(annot, label.as_ptr());
            poppler_annot_markup_set_opacity(annot, 1.0);
            poppler_annot_set_flags(annot, ANNOT_FLAG_PRINT);
            if let NewAnnot::Text { .. } = kind {
                let (pw, _) = self.page_size(page);
                let x1 = (area.x2 + 10.0).min(pw - 150.0).max(0.0);
                let mut pr = PopplerRectangle { x1, y1: (h - area.y1 - 150.0).max(0.0), x2: x1 + 150.0, y2: h - area.y1 };
                poppler_annot_markup_set_popup(annot, &mut pr);
                poppler_annot_markup_set_popup_is_open(annot, 0);
            }
            poppler_page_add_annot(p, annot);
        }
        let id = self.next_annot_id;
        self.next_annot_id += 1;
        self.annots.get_mut(&page).unwrap().push((id, annot));
        self.modified = true;
        self.convert_annot(page, id, annot)
    }

    fn update_annotation(&mut self, an: &Annotation) {
        let Some(a) = self.annot_ptr(an.page, an.id) else { return };
        let h = self.height(an.page);
        unsafe {
            let contents = cstring(&an.contents);
            poppler_annot_set_contents(a, contents.as_ptr());
            let mut c = color_to_pcolor(an.color);
            poppler_annot_set_color(a, &mut c);
            if an.is_markup {
                if let Some(l) = &an.label {
                    let l = cstring(l);
                    poppler_annot_markup_set_label(a, l.as_ptr());
                }
                poppler_annot_markup_set_opacity(a, an.opacity);
                if an.has_popup {
                    poppler_annot_markup_set_popup_is_open(a, an.popup_is_open as i32);
                    if let Some(pr) = an.popup_rect {
                        let mut r = PopplerRectangle { x1: pr.x1, y1: h - pr.y2, x2: pr.x2, y2: h - pr.y1 };
                        poppler_annot_markup_set_popup_rectangle(a, &mut r);
                    }
                }
            }
            if let AnnotKind::Text { icon, is_open } = &an.kind {
                let ic = cstring(icon.name());
                poppler_annot_text_set_icon(a, ic.as_ptr());
                poppler_annot_text_set_is_open(a, *is_open as i32);
            }
            let mut r = PopplerRectangle { x1: an.area.x1, y1: h - an.area.y2, x2: an.area.x2, y2: h - an.area.y1 };
            poppler_annot_set_rectangle(a, &mut r);
        }
        self.modified = true;
    }

    fn remove_annotation(&mut self, page: usize, id: usize) {
        let Some(a) = self.annot_ptr(page, id) else { return };
        unsafe {
            poppler_page_remove_annot(self.page(page), a);
            glib::gobject_ffi::g_object_unref(a as *mut _);
        }
        if let Some(v) = self.annots.get_mut(&page) {
            v.retain(|(i, _)| *i != id);
        }
        self.modified = true;
    }

    fn form_fields(&mut self, page: usize) -> Vec<FormField> {
        self.ensure_fields(page);
        let p = self.page(page);
        if p.is_null() {
            return Vec::new();
        }
        let h = self.height(page);
        let mut out = Vec::new();
        unsafe {
            let list = poppler_page_get_form_field_mapping(p);
            for m in glist_iter::<PopplerFormFieldMapping>(list) {
                let f = (*m).field;
                let a = (*m).area;
                let kind = match poppler_form_field_get_field_type(f) {
                    FORM_FIELD_TEXT => {
                        let tt = poppler_form_field_text_get_text_type(f);
                        FormFieldKind::Text {
                            text: gstr_take(poppler_form_field_text_get_text(f)).unwrap_or_default(),
                            multiline: tt == FORM_TEXT_MULTILINE,
                            file_select: tt == FORM_TEXT_FILE_SELECT,
                            password: poppler_form_field_text_is_password(f) != 0,
                            max_len: poppler_form_field_text_get_max_len(f),
                            do_scroll: poppler_form_field_text_do_scroll(f) != 0,
                        }
                    }
                    FORM_FIELD_BUTTON => FormFieldKind::Button {
                        kind: match poppler_form_field_button_get_button_type(f) {
                            FORM_BUTTON_CHECK => ButtonKind::Check,
                            FORM_BUTTON_RADIO => ButtonKind::Radio,
                            _ => ButtonKind::Push,
                        },
                        state: poppler_form_field_button_get_state(f) != 0,
                    },
                    FORM_FIELD_CHOICE => {
                        let n = poppler_form_field_choice_get_n_items(f).max(0);
                        let items =
                            (0..n).map(|i| gstr_take(poppler_form_field_choice_get_item(f, i)).unwrap_or_default()).collect();
                        let selected = (0..n)
                            .filter(|&i| poppler_form_field_choice_is_item_selected(f, i) != 0)
                            .map(|i| i as usize)
                            .collect();
                        FormFieldKind::Choice {
                            combo: poppler_form_field_choice_get_choice_type(f) == FORM_CHOICE_COMBO,
                            editable: poppler_form_field_choice_is_editable(f) != 0,
                            multi: poppler_form_field_choice_can_select_multiple(f) != 0,
                            items,
                            selected,
                            text: gstr_take(poppler_form_field_choice_get_text(f)).unwrap_or_default(),
                        }
                    }
                    FORM_FIELD_SIGNATURE => FormFieldKind::Signature,
                    _ => FormFieldKind::Unknown,
                };
                let action = poppler_form_field_get_action(f);
                let activation = self.convert_action(action);
                out.push(FormField {
                    id: poppler_form_field_get_id(f),
                    page,
                    area: Rect::new(a.x1, h - a.y2, a.x2, h - a.y1).normalized(),
                    read_only: poppler_form_field_is_read_only(f) != 0,
                    font_size: poppler_form_field_get_font_size(f),
                    name: gstr_take(poppler_form_field_get_name(f)),
                    kind,
                    activation,
                });
            }
            poppler_page_free_form_field_mapping(list);
        }
        out
    }

    fn set_form_text(&mut self, page: usize, id: i32, text: &str) {
        if let Some(f) = self.field_ptr(page, id) {
            let t = cstring(text);
            unsafe { poppler_form_field_text_set_text(f, t.as_ptr()) };
            self.modified = true;
        }
    }

    fn set_form_button(&mut self, page: usize, id: i32, state: bool) {
        if let Some(f) = self.field_ptr(page, id) {
            unsafe { poppler_form_field_button_set_state(f, state as i32) };
            self.modified = true;
        }
    }

    fn set_form_choice(&mut self, page: usize, id: i32, selected: &[usize], text: Option<&str>) {
        if let Some(f) = self.field_ptr(page, id) {
            unsafe {
                if let Some(t) = text {
                    let t = cstring(t);
                    poppler_form_field_choice_set_text(f, t.as_ptr());
                } else {
                    poppler_form_field_choice_unselect_all(f);
                    for &i in selected {
                        poppler_form_field_choice_select_item(f, i as i32);
                    }
                }
            }
            self.modified = true;
        }
    }

    fn reset_form(&mut self, fields: &[String], exclude: bool) {
        unsafe {
            let mut list: *mut glib::ffi::GList = ptr::null_mut();
            let cs: Vec<_> = fields.iter().map(|f| cstring(f)).collect();
            for c in cs.iter().rev() {
                list = glib::ffi::g_list_prepend(list, c.as_ptr() as *mut _);
            }
            poppler_document_reset_form(self.doc, list, exclude as i32);
            glib::ffi::g_list_free(list);
        }
        self.modified = true;
    }

    fn transition(&mut self, page: usize) -> Option<Transition> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        unsafe {
            let t = poppler_page_get_transition(p);
            if t.is_null() {
                return None;
            }
            let tr = &*t;
            let kind = match tr.type_ {
                1 => TransitionKind::Split,
                2 => TransitionKind::Blinds,
                3 => TransitionKind::Box,
                4 => TransitionKind::Wipe,
                5 => TransitionKind::Dissolve,
                6 => TransitionKind::Glitter,
                7 => TransitionKind::Fly,
                8 => TransitionKind::Push,
                9 => TransitionKind::Cover,
                10 => TransitionKind::Uncover,
                11 => TransitionKind::Fade,
                _ => TransitionKind::Replace,
            };
            let res = Transition {
                kind,
                horizontal: tr.alignment == 0,
                outward: tr.direction == 1,
                duration: if tr.duration_real > 0.0 { tr.duration_real } else { tr.duration as f64 },
                angle: tr.angle,
                scale: tr.scale,
                rectangular: tr.rectangular != 0,
            };
            poppler_page_transition_free(t);
            Some(res)
        }
    }

    fn page_duration(&mut self, page: usize) -> Option<f64> {
        let p = self.page(page);
        if p.is_null() {
            return None;
        }
        let d = unsafe { poppler_page_get_duration(p) };
        (d > 0.0).then_some(d)
    }

    fn media(&mut self, page: usize) -> Vec<Mapping<Media>> {
        let p = self.page(page);
        if p.is_null() {
            return Vec::new();
        }
        let h = self.height(page);
        let mut out = Vec::new();
        unsafe {
            let list = poppler_page_get_annot_mapping(p);
            for m in glist_iter::<PopplerAnnotMapping>(list) {
                let a = (*m).annot;
                let ty = poppler_annot_get_annot_type(a);
                let media = if ty == ANNOT_MOVIE {
                    let mv = poppler_annot_movie_get_movie(a);
                    if mv.is_null() {
                        None
                    } else {
                        cstr_opt(poppler_movie_get_filename(mv))
                            .map(|f| Media { uri: self.resolve_uri(&f), show_controls: poppler_movie_show_controls(mv) != 0 })
                    }
                } else if ty == ANNOT_SCREEN {
                    let act = poppler_annot_screen_get_action(a);
                    if !act.is_null() && (*act).type_ == ACTION_RENDITION {
                        self.media_from_poppler((*act).rendition.media)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(media) = media {
                    let r = (*m).area;
                    out.push(Mapping { area: Rect::new(r.x1, h - r.y2, r.x2, h - r.y1).normalized(), data: media });
                }
            }
            poppler_page_free_annot_mapping(list);
        }
        out
    }

    fn is_modified(&self) -> bool {
        self.modified
    }

    fn save(&mut self, path: &Path) -> Result<()> {
        if !self.modified {
            return copy_file(&self.source.path, path);
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        let tmp = dir.join(format!(".evince-rs-save-{}.pdf", std::process::id()));
        let uri = glib::filename_to_uri(&tmp, None).map_err(|e| Error::Io(e.to_string()))?;
        let curi = cstring(&uri);
        let mut err = ptr::null_mut();
        let ok = unsafe { poppler_document_save(self.doc, curi.as_ptr(), &mut err) };
        if ok == 0 {
            let _ = std::fs::remove_file(&tmp);
            return Err(Error::Io(unsafe { take_gerror(err) }));
        }
        std::fs::rename(&tmp, path)
            .or_else(|_| std::fs::copy(&tmp, path).map(|_| ()).and_then(|_| std::fs::remove_file(&tmp)))
            .map_err(|e| Error::Io(e.to_string()))?;
        Ok(())
    }

    fn uri(&self) -> &str {
        &self.source.uri
    }

    fn path(&self) -> &Path {
        &self.source.path
    }

    fn supports_synctex(&self) -> bool {
        true
    }
}

unsafe extern "C" {
    fn poppler_action_free(a: *mut PopplerAction);
    fn poppler_dest_free(d: *mut PopplerDest);
}
