pub mod comics;
#[cfg(feature = "djvu")]
pub mod djvu;
pub mod dvi;
pub mod factory;
pub mod misc;
pub mod pdf;
#[cfg(feature = "ps")]
pub mod ps;
pub mod synctex;
pub mod tiff;
#[cfg(feature = "xps")]
pub mod xps;

use std::fmt;
use std::path::{Path, PathBuf};

pub use factory::{load_document, supported_mime_types};

#[derive(Debug, Clone)]
pub enum Error {
    Encrypted,
    UnsupportedType(String),
    Io(String),
    Invalid(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Encrypted => write!(f, "The document is encrypted"),
            Error::UnsupportedType(m) => write!(f, "File type {m} is not supported"),
            Error::Io(m) | Error::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// A rectangle in page coordinates (points, origin at top-left of the unrotated page).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Rect {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Rect { x1, y1, x2, y2 }
    }
    pub fn normalized(self) -> Self {
        Rect {
            x1: self.x1.min(self.x2),
            y1: self.y1.min(self.y2),
            x2: self.x1.max(self.x2),
            y2: self.y1.max(self.y2),
        }
    }
    pub fn width(&self) -> f64 {
        self.x2 - self.x1
    }
    pub fn height(&self) -> f64 {
        self.y2 - self.y1
    }
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x1 && x <= self.x2 && y >= self.y1 && y <= self.y2
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
            x2: self.x2.max(o.x2),
            y2: self.y2.max(o.y2),
        }
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x1 < o.x2 && o.x1 < self.x2 && self.y1 < o.y2 && o.y1 < self.y2
    }
    pub fn center(&self) -> (f64, f64) {
        ((self.x1 + self.x2) / 2.0, (self.y1 + self.y2) / 2.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Color { r, g, b, a: 1.0 }
    }
    pub fn to_hex(&self) -> String {
        format!(
            "#{:02x}{:02x}{:02x}",
            (self.r * 255.0).round() as u8,
            (self.g * 255.0).round() as u8,
            (self.b * 255.0).round() as u8
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkDest {
    Page(usize),
    PageLabel(String),
    Named(String),
    Xyz { page: usize, left: Option<f64>, top: Option<f64>, zoom: Option<f64> },
    Fit { page: usize },
    FitH { page: usize, top: Option<f64> },
    FitV { page: usize, left: Option<f64> },
    FitR { page: usize, rect: Rect },
}

impl LinkDest {
    pub fn page(&self) -> Option<usize> {
        match self {
            LinkDest::Page(p)
            | LinkDest::Xyz { page: p, .. }
            | LinkDest::Fit { page: p }
            | LinkDest::FitH { page: p, .. }
            | LinkDest::FitV { page: p, .. }
            | LinkDest::FitR { page: p, .. } => Some(*p),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayerAction {
    On,
    Off,
    Toggle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkAction {
    GotoDest(LinkDest),
    GotoRemote { file: String, dest: Option<LinkDest> },
    ExternalUri(String),
    Launch { file: String, params: Option<String> },
    Named(String),
    Layers(Vec<(LayerAction, Vec<usize>)>),
    ResetForm { fields: Vec<String>, exclude: bool },
    Media { page: usize, media: Media },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub title: Option<String>,
    pub action: Option<LinkAction>,
}

#[derive(Debug, Clone)]
pub struct Mapping<T> {
    pub area: Rect,
    pub data: T,
}

#[derive(Debug, Clone)]
pub struct OutlineItem {
    pub title: String,
    pub link: Option<Link>,
    pub expand: bool,
    pub children: Vec<OutlineItem>,
}

#[derive(Debug, Clone, Default)]
pub struct DocInfo {
    pub title: Option<String>,
    pub format: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<i64>,
    pub modified_date: Option<i64>,
    pub linearized: Option<bool>,
    pub security: Option<bool>,
    pub n_pages: usize,
    pub paper_size: Option<(f64, f64)>,
    pub layout: PageLayoutHint,
    pub start_mode: StartModeHint,
    pub permissions: Permissions,
    pub contains_js: bool,
    pub license: Option<License>,
}

#[derive(Debug, Clone, Default)]
pub struct License {
    pub text: Option<String>,
    pub uri: Option<String>,
    pub web_statement: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum PageLayoutHint {
    #[default]
    Unset,
    SinglePage,
    OneColumn,
    TwoColumnLeft,
    TwoColumnRight,
    TwoPageLeft,
    TwoPageRight,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum StartModeHint {
    #[default]
    None,
    UseOutlines,
    UseThumbs,
    FullScreen,
    UseOc,
    UseAttachments,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Permissions {
    pub print: bool,
    pub modify: bool,
    pub copy: bool,
    pub add_notes: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Permissions { print: true, modify: true, copy: true, add_notes: true }
    }
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    pub description: Option<String>,
    pub mtime: Option<i64>,
    pub ctime: Option<i64>,
    pub data: std::sync::Arc<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub id: usize,
    pub title: String,
    pub visible: bool,
    pub title_only: bool,
    pub radio_group: i32,
    pub children: Vec<Layer>,
}

#[derive(Debug, Clone)]
pub struct FontInfo {
    pub name: String,
    pub details: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupType {
    Highlight,
    StrikeOut,
    Underline,
    Squiggly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextIcon {
    Note,
    Comment,
    Key,
    Help,
    NewParagraph,
    Paragraph,
    Insert,
    Cross,
    Circle,
    Unknown,
}

impl TextIcon {
    pub const ALL: [TextIcon; 9] = [
        TextIcon::Note,
        TextIcon::Comment,
        TextIcon::Key,
        TextIcon::Help,
        TextIcon::NewParagraph,
        TextIcon::Paragraph,
        TextIcon::Insert,
        TextIcon::Cross,
        TextIcon::Circle,
    ];
    pub fn name(&self) -> &'static str {
        match self {
            TextIcon::Note => "Note",
            TextIcon::Comment => "Comment",
            TextIcon::Key => "Key",
            TextIcon::Help => "Help",
            TextIcon::NewParagraph => "NewParagraph",
            TextIcon::Paragraph => "Paragraph",
            TextIcon::Insert => "Insert",
            TextIcon::Cross => "Cross",
            TextIcon::Circle => "Circle",
            TextIcon::Unknown => "Note",
        }
    }
    pub fn from_name(s: &str) -> TextIcon {
        TextIcon::ALL.iter().copied().find(|i| i.name() == s).unwrap_or(TextIcon::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotKind {
    Text { icon: TextIcon, is_open: bool },
    TextMarkup(MarkupType),
    FreeText,
    Attachment(Option<std::sync::Arc<AttachmentRef>>),
    Other(&'static str),
}

#[derive(Debug, PartialEq)]
pub struct AttachmentRef {
    pub name: String,
    pub description: Option<String>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// Backend specific stable identifier inside the page.
    pub id: usize,
    pub page: usize,
    pub kind: AnnotKind,
    pub area: Rect,
    pub contents: String,
    pub name: Option<String>,
    pub modified: Option<String>,
    pub color: Color,
    pub is_markup: bool,
    pub label: Option<String>,
    pub opacity: f64,
    pub has_popup: bool,
    pub popup_rect: Option<Rect>,
    pub popup_is_open: bool,
    pub hidden: bool,
}

impl Annotation {
    pub fn kind_name(&self) -> &'static str {
        match &self.kind {
            AnnotKind::Text { .. } => "Note",
            AnnotKind::TextMarkup(MarkupType::Highlight) => "Highlight",
            AnnotKind::TextMarkup(MarkupType::StrikeOut) => "Strike Out",
            AnnotKind::TextMarkup(MarkupType::Underline) => "Underline",
            AnnotKind::TextMarkup(MarkupType::Squiggly) => "Squiggly",
            AnnotKind::FreeText => "Free Text",
            AnnotKind::Attachment(_) => "Attachment",
            AnnotKind::Other(n) => n,
        }
    }
}

/// What a newly created annotation should look like.
#[derive(Debug, Clone)]
pub enum NewAnnot {
    Text { icon: TextIcon },
    TextMarkup(MarkupType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FormFieldKind {
    Text { text: String, multiline: bool, password: bool, max_len: i32, do_scroll: bool, file_select: bool },
    Button { kind: ButtonKind, state: bool },
    Choice { combo: bool, editable: bool, multi: bool, items: Vec<String>, selected: Vec<usize>, text: String },
    Signature,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Push,
    Check,
    Radio,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    pub id: i32,
    pub page: usize,
    pub area: Rect,
    pub read_only: bool,
    pub font_size: f64,
    pub name: Option<String>,
    pub kind: FormFieldKind,
    pub activation: Option<Link>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransitionKind {
    #[default]
    Replace,
    Split,
    Blinds,
    Box,
    Wipe,
    Dissolve,
    Glitter,
    Fly,
    Push,
    Cover,
    Uncover,
    Fade,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transition {
    pub kind: TransitionKind,
    pub horizontal: bool,
    pub outward: bool,
    pub duration: f64,
    pub angle: i32,
    pub scale: f64,
    pub rectangular: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub uri: String,
    pub show_controls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SelectionStyle {
    Glyph,
    Word,
    Line,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_words: bool,
}

/// One search hit. A match can span several lines and therefore several rectangles.
#[derive(Debug, Clone)]
pub struct FindMatch {
    pub rects: Vec<Rect>,
}

/// Plain text of a page together with one bounding rectangle per character.
#[derive(Debug, Clone, Default)]
pub struct TextLayout {
    pub chars: Vec<char>,
    pub rects: Vec<Rect>,
}

impl TextLayout {
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }
}

/// Rendered page pixels in cairo ARGB32 (premultiplied, native endian) layout.
#[derive(Clone)]
pub struct Bitmap {
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub data: Vec<u8>,
}

impl Bitmap {
    pub fn from_surface(mut surf: cairo::ImageSurface) -> Option<Bitmap> {
        surf.flush();
        let width = surf.width();
        let height = surf.height();
        let stride = surf.stride();
        let format = surf.format();
        let data = surf.data().ok()?.to_vec();
        let mut bm = Bitmap { width, height, stride, data };
        if format == cairo::Format::Rgb24 {
            for px in bm.data.chunks_exact_mut(4) {
                if cfg!(target_endian = "little") {
                    px[3] = 255;
                } else {
                    px[0] = 255;
                }
            }
        }
        Some(bm)
    }

    pub fn to_surface(&self) -> Option<cairo::ImageSurface> {
        cairo::ImageSurface::create_for_data(
            self.data.clone(),
            cairo::Format::ARgb32,
            self.width,
            self.height,
            self.stride,
        )
        .ok()
    }

    pub fn to_pixbuf(&self) -> gdk_pixbuf::Pixbuf {
        let pb = gdk_pixbuf::Pixbuf::new(gdk_pixbuf::Colorspace::Rgb, true, 8, self.width, self.height).unwrap();
        let rs = pb.rowstride() as usize;
        let le = cfg!(target_endian = "little");
        unsafe {
            let dst = pb.pixels();
            for y in 0..self.height as usize {
                for x in 0..self.width as usize {
                    let s = &self.data[y * self.stride as usize + x * 4..][..4];
                    let (b, g, r, a) = if le { (s[0], s[1], s[2], s[3]) } else { (s[3], s[2], s[1], s[0]) };
                    let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
                    let d = &mut dst[y * rs + x * 4..][..4];
                    d[0] = un(r);
                    d[1] = un(g);
                    d[2] = un(b);
                    d[3] = a;
                }
            }
        }
        pb
    }
}


/// Source location for SyncTeX backward search.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLink {
    pub filename: String,
    pub line: i32,
    pub col: i32,
}

/// Common interface implemented by every document backend.
///
/// All coordinates are page points with origin at the top-left corner of the unrotated page.
pub trait Document: Send {
    fn n_pages(&self) -> usize;
    fn page_size(&self, page: usize) -> (f64, f64);
    fn page_label(&self, _page: usize) -> Option<String> {
        None
    }
    fn has_page_labels(&self) -> bool {
        false
    }
    /// Renders the page with the given scale and rotation (0/90/180/270).
    fn render(&mut self, page: usize, scale: f64, rotation: i32) -> Option<cairo::ImageSurface>;
    /// Draws the page at 1pt = 1 unit on a cairo context (used for printing).
    fn print_page(&mut self, page: usize, cr: &cairo::Context) {
        let scale = 300.0 / 72.0;
        if let Some(surf) = self.render(page, scale, 0) {
            let _ = cr.save();
            cr.scale(1.0 / scale, 1.0 / scale);
            let _ = cr.set_source_surface(&surf, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }
    }
    fn info(&self) -> DocInfo;
    fn mime_type(&self) -> &str;

    fn text_layout(&mut self, _page: usize) -> Option<TextLayout> {
        None
    }
    fn page_text(&mut self, page: usize) -> Option<String> {
        self.text_layout(page).map(|l| l.text())
    }
    fn has_text(&self) -> bool {
        false
    }
    /// Selection region for the selection from `start` to `end` (page points), as rectangles.
    fn selection_region(&mut self, page: usize, style: SelectionStyle, sel: Rect) -> Vec<Rect> {
        match self.text_layout(page) {
            Some(l) => misc::layout_selection_region(&l, style, sel),
            None => Vec::new(),
        }
    }
    fn selected_text(&mut self, page: usize, style: SelectionStyle, sel: Rect) -> String {
        match self.text_layout(page) {
            Some(l) => misc::layout_selected_text(&l, style, sel),
            None => String::new(),
        }
    }
    fn find(&mut self, page: usize, text: &str, opts: FindOptions) -> Vec<FindMatch> {
        match self.text_layout(page) {
            Some(l) => misc::layout_find(&l, text, opts),
            None => Vec::new(),
        }
    }
    fn can_find(&self) -> bool {
        self.has_text()
    }

    fn links(&mut self, _page: usize) -> Vec<Mapping<Link>> {
        Vec::new()
    }
    fn outline(&mut self) -> Vec<OutlineItem> {
        Vec::new()
    }
    fn resolve_named_dest(&mut self, _name: &str) -> Option<LinkDest> {
        None
    }
    fn page_by_label(&self, label: &str) -> Option<usize> {
        (0..self.n_pages()).find(|&p| self.page_label(p).as_deref() == Some(label))
    }

    fn images(&mut self, _page: usize) -> Vec<Mapping<i32>> {
        Vec::new()
    }
    fn image(&mut self, _page: usize, _id: i32) -> Option<cairo::ImageSurface> {
        None
    }

    fn attachments(&mut self) -> Vec<Attachment> {
        Vec::new()
    }
    fn fonts(&mut self) -> Vec<FontInfo> {
        Vec::new()
    }
    fn layers(&mut self) -> Vec<Layer> {
        Vec::new()
    }
    fn set_layer_visible(&mut self, _id: usize, _visible: bool) {}

    fn annotations(&mut self, _page: usize) -> Vec<Annotation> {
        Vec::new()
    }
    fn can_add_annotations(&self) -> bool {
        false
    }
    fn add_annotation(
        &mut self,
        _page: usize,
        _kind: &NewAnnot,
        _area: Rect,
        _color: Color,
        _author: &str,
    ) -> Option<Annotation> {
        None
    }
    fn update_annotation(&mut self, _annot: &Annotation) {}
    fn remove_annotation(&mut self, _page: usize, _id: usize) {}

    fn form_fields(&mut self, _page: usize) -> Vec<FormField> {
        Vec::new()
    }
    fn set_form_text(&mut self, _page: usize, _id: i32, _text: &str) {}
    fn set_form_button(&mut self, _page: usize, _id: i32, _state: bool) {}
    fn set_form_choice(&mut self, _page: usize, _id: i32, _selected: &[usize], _text: Option<&str>) {}
    fn reset_form(&mut self, _fields: &[String], _exclude: bool) {}

    fn transition(&mut self, _page: usize) -> Option<Transition> {
        None
    }
    fn page_duration(&mut self, _page: usize) -> Option<f64> {
        None
    }
    fn media(&mut self, _page: usize) -> Vec<Mapping<Media>> {
        Vec::new()
    }

    fn is_modified(&self) -> bool {
        false
    }
    /// Saves the document (including modifications when supported) to `path`.
    fn save(&mut self, path: &Path) -> Result<()>;

    fn uri(&self) -> &str;
    fn path(&self) -> &Path;
    fn supports_synctex(&self) -> bool {
        false
    }
}

pub fn copy_file(src: &Path, dst: &Path) -> Result<()> {
    if src == dst {
        return Ok(());
    }
    std::fs::copy(src, dst).map(|_| ()).map_err(|e| Error::Io(e.to_string()))
}

/// Uri and path of a document: the path may be a temporary uncompressed copy.
#[derive(Debug, Clone)]
pub struct Source {
    pub uri: String,
    pub path: PathBuf,
    pub mime: String,
}

/// Helper used by vector backends: creates a white surface for a page and sets up
/// the cairo transformation for the requested scale and rotation.
pub fn vector_surface(
    w: f64,
    h: f64,
    scale: f64,
    rotation: i32,
    draw: impl FnOnce(&cairo::Context),
) -> Option<cairo::ImageSurface> {
    let (pw, ph) = misc::transformed_size(w, h, scale, rotation);
    let surf = cairo::ImageSurface::create(cairo::Format::ARgb32, pw.max(1), ph.max(1)).ok()?;
    {
        let cr = cairo::Context::new(&surf).ok()?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().ok()?;
        misc::apply_rotation(&cr, pw as f64, ph as f64, rotation);
        cr.scale(scale, scale);
        draw(&cr);
    }
    Some(surf)
}
