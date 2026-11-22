use super::jobs::Jobs;
use crate::document::factory::{Loaded, TempFile};
use crate::document::synctex::Synctex;
use crate::document::{DocInfo, LinkDest};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SizingMode {
    FitPage,
    #[default]
    FitWidth,
    Free,
    Automatic,
}

impl SizingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SizingMode::FitPage => "fit-page",
            SizingMode::FitWidth => "fit-width",
            SizingMode::Free => "free",
            SizingMode::Automatic => "automatic",
        }
    }
    pub fn from_str(s: &str) -> SizingMode {
        match s {
            "fit-page" | "best-fit" => SizingMode::FitPage,
            "fit-width" => SizingMode::FitWidth,
            "automatic" => SizingMode::Automatic,
            _ => SizingMode::Free,
        }
    }
}

/// A loaded document plus geometry cached for the UI thread.
pub struct DocHandle {
    pub jobs: Jobs,
    pub n_pages: usize,
    pub sizes: Vec<(f64, f64)>,
    pub labels: Vec<Option<String>>,
    pub has_labels: bool,
    pub uniform: bool,
    pub max_size: (f64, f64),
    pub info: DocInfo,
    pub mime: String,
    pub uri: String,
    pub path: PathBuf,
    pub has_text: bool,
    pub can_find: bool,
    pub can_annotate: bool,
    pub supports_synctex: bool,
    synctex: RefCell<Option<Option<Rc<Synctex>>>>,
    pub modified: Cell<bool>,
    _temps: Vec<TempFile>,
}

impl DocHandle {
    pub fn new(loaded: Loaded) -> Rc<DocHandle> {
        let Loaded { doc, temps } = loaded;
        let n_pages = doc.n_pages();
        let sizes: Vec<_> = (0..n_pages).map(|i| doc.page_size(i)).collect();
        let labels: Vec<_> = (0..n_pages).map(|i| doc.page_label(i)).collect();
        let has_labels = doc.has_page_labels();
        let uniform = sizes.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.5 && (w[0].1 - w[1].1).abs() < 0.5);
        let max_size = sizes.iter().fold((0.0f64, 0.0f64), |a, s| (a.0.max(s.0), a.1.max(s.1)));
        let info = doc.info();
        let mime = doc.mime_type().to_string();
        let uri = doc.uri().to_string();
        let path = doc.path().to_path_buf();
        let has_text = doc.has_text();
        let can_find = doc.can_find();
        let can_annotate = doc.can_add_annotations();
        let supports_synctex = doc.supports_synctex();
        Rc::new(DocHandle {
            jobs: Jobs::new(doc),
            n_pages,
            sizes,
            labels,
            has_labels,
            uniform,
            max_size,
            info,
            mime,
            uri,
            path,
            has_text,
            can_find,
            can_annotate,
            supports_synctex,
            synctex: RefCell::new(None),
            modified: Cell::new(false),
            _temps: temps,
        })
    }

    pub fn page_size(&self, page: usize) -> (f64, f64) {
        self.sizes.get(page).copied().unwrap_or((612.0, 792.0))
    }

    pub fn rotated_size(&self, page: usize, rotation: i32) -> (f64, f64) {
        let (w, h) = self.page_size(page);
        if rotation == 90 || rotation == 270 { (h, w) } else { (w, h) }
    }

    /// Label to show for a page, falling back to the 1-based page number.
    pub fn label(&self, page: usize) -> String {
        self.labels.get(page).cloned().flatten().unwrap_or_else(|| (page + 1).to_string())
    }

    pub fn find_page_by_label(&self, label: &str) -> Option<usize> {
        let l = label.trim();
        if let Some(i) = self.labels.iter().position(|x| x.as_deref() == Some(l)) {
            return Some(i);
        }
        if let Some(i) = self.labels.iter().position(|x| x.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(l))) {
            return Some(i);
        }
        l.parse::<usize>().ok().filter(|&n| n >= 1 && n <= self.n_pages).map(|n| n - 1)
    }

    pub fn synctex(&self) -> Option<Rc<Synctex>> {
        if !self.supports_synctex {
            return None;
        }
        self.synctex.borrow_mut().get_or_insert_with(|| Synctex::new(&self.path).map(Rc::new)).clone()
    }

    pub fn reset_synctex(&self) {
        *self.synctex.borrow_mut() = None;
    }

    /// Resolves a destination to a concrete page, resolving named and label destinations.
    pub fn resolve_dest(&self, dest: &LinkDest) -> Option<(usize, LinkDest)> {
        match dest {
            LinkDest::Named(name) => {
                let name = name.clone();
                let d = self.jobs.with(move |d| d.resolve_named_dest(&name))?;
                let p = d.page()?;
                Some((p, d))
            }
            LinkDest::PageLabel(l) => self.find_page_by_label(l).map(|p| (p, LinkDest::Page(p))),
            other => other.page().filter(|&p| p < self.n_pages).map(|p| (p, other.clone())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Document,
    Page { old: usize, new: usize },
    Scale,
    Rotation,
    Sizing,
    Continuous,
    DualPage,
    DualOddLeft,
    Rtl,
    Inverted,
    Fullscreen,
    MaxScale,
}

type Listener = Rc<dyn Fn(Change)>;

/// Shared view state, mirroring EvDocumentModel.
#[derive(Default)]
pub struct Model {
    doc: RefCell<Option<Rc<DocHandle>>>,
    page: Cell<usize>,
    scale: Cell<f64>,
    rotation: Cell<i32>,
    sizing: Cell<SizingMode>,
    continuous: Cell<bool>,
    dual: Cell<bool>,
    dual_odd_left: Cell<bool>,
    rtl: Cell<bool>,
    inverted: Cell<bool>,
    fullscreen: Cell<bool>,
    min_scale: Cell<f64>,
    max_scale: Cell<f64>,
    listeners: RefCell<Vec<(usize, Listener)>>,
    next_id: Cell<usize>,
}

impl Model {
    pub fn new() -> Rc<Model> {
        let m = Model::default();
        m.scale.set(1.0);
        m.continuous.set(true);
        m.min_scale.set(0.05409);
        m.max_scale.set(5.0);
        Rc::new(m)
    }

    pub fn connect(&self, f: impl Fn(Change) + 'static) -> usize {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        self.listeners.borrow_mut().push((id, Rc::new(f)));
        id
    }

    pub fn disconnect(&self, id: usize) {
        self.listeners.borrow_mut().retain(|(i, _)| *i != id);
    }

    fn emit(&self, c: Change) {
        let ls: Vec<Listener> = self.listeners.borrow().iter().map(|(_, l)| l.clone()).collect();
        for l in ls {
            l(c);
        }
    }

    pub fn document(&self) -> Option<Rc<DocHandle>> {
        self.doc.borrow().clone()
    }

    pub fn set_document(&self, doc: Option<Rc<DocHandle>>) {
        let n = doc.as_ref().map_or(0, |d| d.n_pages);
        *self.doc.borrow_mut() = doc;
        if n > 0 && self.page.get() >= n {
            self.page.set(n - 1);
        } else if self.page.get() >= n {
            self.page.set(n.saturating_sub(1));
        }
        self.emit(Change::Document);
    }

    pub fn page(&self) -> usize {
        self.page.get()
    }

    pub fn set_page(&self, page: usize) {
        let n = self.document().map_or(0, |d| d.n_pages);
        if n == 0 {
            return;
        }
        let page = page.min(n - 1);
        let old = self.page.get();
        if old == page {
            return;
        }
        self.page.set(page);
        self.emit(Change::Page { old, new: page });
    }

    /// Sets the page without clamping or notifying, used before a document is attached.
    pub fn set_page_unchecked(&self, page: usize) {
        self.page.set(page);
    }

    pub fn scale(&self) -> f64 {
        self.scale.get()
    }

    pub fn set_scale(&self, s: f64) {
        let s = s.clamp(self.min_scale.get(), self.max_scale.get().max(self.min_scale.get()));
        if (s - self.scale.get()).abs() < 1e-9 {
            return;
        }
        self.scale.set(s);
        self.emit(Change::Scale);
    }

    pub fn min_scale(&self) -> f64 {
        self.min_scale.get()
    }

    pub fn max_scale(&self) -> f64 {
        self.max_scale.get()
    }

    pub fn set_max_scale(&self, m: f64) {
        if (m - self.max_scale.get()).abs() < 1e-9 {
            return;
        }
        self.max_scale.set(m);
        self.emit(Change::MaxScale);
        if self.scale.get() > m {
            self.set_scale(m);
        }
    }

    pub fn rotation(&self) -> i32 {
        self.rotation.get()
    }

    pub fn set_rotation(&self, r: i32) {
        let r = r.rem_euclid(360);
        let r = r - r % 90;
        if r == self.rotation.get() {
            return;
        }
        self.rotation.set(r);
        self.emit(Change::Rotation);
    }

    pub fn sizing_mode(&self) -> SizingMode {
        self.sizing.get()
    }

    pub fn set_sizing_mode(&self, m: SizingMode) {
        if m == self.sizing.get() {
            return;
        }
        self.sizing.set(m);
        self.emit(Change::Sizing);
    }

    pub fn continuous(&self) -> bool {
        self.continuous.get()
    }

    pub fn set_continuous(&self, v: bool) {
        if v != self.continuous.get() {
            self.continuous.set(v);
            self.emit(Change::Continuous);
        }
    }

    pub fn dual_page(&self) -> bool {
        self.dual.get()
    }

    pub fn set_dual_page(&self, v: bool) {
        if v != self.dual.get() {
            self.dual.set(v);
            self.emit(Change::DualPage);
        }
    }

    pub fn dual_odd_left(&self) -> bool {
        self.dual_odd_left.get()
    }

    pub fn set_dual_odd_left(&self, v: bool) {
        if v != self.dual_odd_left.get() {
            self.dual_odd_left.set(v);
            self.emit(Change::DualOddLeft);
        }
    }

    pub fn rtl(&self) -> bool {
        self.rtl.get()
    }

    pub fn set_rtl(&self, v: bool) {
        if v != self.rtl.get() {
            self.rtl.set(v);
            self.emit(Change::Rtl);
        }
    }

    pub fn inverted(&self) -> bool {
        self.inverted.get()
    }

    pub fn set_inverted(&self, v: bool) {
        if v != self.inverted.get() {
            self.inverted.set(v);
            self.emit(Change::Inverted);
        }
    }

    pub fn fullscreen(&self) -> bool {
        self.fullscreen.get()
    }

    pub fn set_fullscreen(&self, v: bool) {
        if v != self.fullscreen.get() {
            self.fullscreen.set(v);
            self.emit(Change::Fullscreen);
        }
    }
}
