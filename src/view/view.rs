use super::layout::{self, BORDER, Layout};
use super::model::{Change, DocHandle, Model, SizingMode};
use super::{JobHandle, Priority, ZOOM_IN_FACTOR, ZOOM_OUT_FACTOR};
use crate::document::*;
use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::gdk;
use gdk::keys::constants as key;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// GDK 3 reports Wayland touchpad motion in tenths of a pixel; this restores the 1:1 finger tracking of GTK 4.
const TOUCHPAD_PIXELS: f64 = 10.0;
const KINETIC_FRICTION: f64 = 4.0;
/// How far a touchpad swipe has to push past the end of a page before single-page mode flips, once per swipe.
const TOUCHPAD_FLIP_DISTANCE: f64 = 80.0;
const KINETIC_MIN_VELOCITY: f64 = 30.0;

fn is_touchpad(e: &gdk::EventScroll) -> bool {
    e.direction() == gdk::ScrollDirection::Smooth
        && (e.source_device().is_some_and(|d| matches!(d.source(), gdk::InputSource::Touchpad | gdk::InputSource::Trackpoint))
            || cfg!(debug_assertions) && std::env::var_os("EVINCE_RS_FAKE_TOUCHPAD").is_some())
}

/// Per-page interactive data fetched in the background (EvPageCache).
#[derive(Clone, Default)]
pub struct PageData {
    pub links: Vec<Mapping<Link>>,
    pub images: Vec<Mapping<i32>>,
    pub annots: Vec<Annotation>,
    pub forms: Vec<FormField>,
    pub media: Vec<Mapping<Media>>,
    pub text: Option<TextLayout>,
}

#[derive(Clone, Default)]
pub struct PopupContext {
    pub page: Option<usize>,
    pub link: Option<Link>,
    pub image: Option<(usize, i32)>,
    pub annot: Option<Annotation>,
    pub attachment: Option<std::sync::Arc<AttachmentRef>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AddMode {
    Text,
    Highlight,
}

#[derive(Clone)]
pub enum ViewEvent {
    ExternalLink(LinkAction),
    Popup { x: f64, y: f64 },
    SelectionChanged,
    SyncSource { page: usize, x: f64, y: f64 },
    AnnotsChanged(Option<usize>),
    AnnotAdded(Annotation),
    LayersChanged,
    FormChanged,
    FindUpdated,
    HistoryJump { from_page: usize },
    AddModeFinished,
    CaretToggleRequested,
}

pub struct Tex {
    tex: cairo::ImageSurface,
    scale: f64,
    rotation: i32,
    version: u64,
}

pub struct Pending {
    scale: f64,
    rotation: i32,
    version: u64,
    job: JobHandle,
}

#[derive(Clone)]
pub struct SelPage {
    page: usize,
    rect: Rect,
    style: SelectionStyle,
    region: Vec<Rect>,
}

#[derive(Default)]
pub struct FindState {
    text: String,
    opts: FindOptions,
    results: HashMap<usize, Vec<FindMatch>>,
    current: Option<(usize, usize)>,
    pages_done: usize,
    start_page: usize,
    running: Option<JobHandle>,
    highlight: bool,
    generation: u64,
}

pub struct Child {
    widget: gtk::Widget,
    page: usize,
    rect: Rect,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Press {
    None,
    Link,
    Form,
    Annot,
    Media,
}

mod imp {
    use super::*;

    #[derive(glib::Properties)]
    #[properties(wrapper_type = super::View)]
    pub struct View {
        #[property(override_interface = gtk::Scrollable, get, set = Self::set_hadjustment, nullable)]
        pub hadjustment: RefCell<Option<gtk::Adjustment>>,
        #[property(override_interface = gtk::Scrollable, get, set = Self::set_vadjustment, nullable)]
        pub vadjustment: RefCell<Option<gtk::Adjustment>>,
        #[property(override_interface = gtk::Scrollable, name = "hscroll-policy", get, set, builder(gtk::ScrollablePolicy::Minimum))]
        pub hscroll_policy: Cell<gtk::ScrollablePolicy>,
        #[property(override_interface = gtk::Scrollable, name = "vscroll-policy", get, set, builder(gtk::ScrollablePolicy::Minimum))]
        pub vscroll_policy: Cell<gtk::ScrollablePolicy>,

        pub model: RefCell<Option<Rc<Model>>>,
        pub model_handler: Cell<usize>,
        pub doc: RefCell<Option<Rc<DocHandle>>>,
        pub layout: RefCell<Layout>,
        pub textures: RefCell<HashMap<usize, Tex>>,
        pub pending: RefCell<HashMap<usize, Pending>>,
        pub versions: RefCell<HashMap<usize, u64>>,
        pub page_data: RefCell<HashMap<usize, PageData>>,
        pub data_pending: RefCell<HashMap<usize, JobHandle>>,
        pub selection: RefCell<Vec<SelPage>>,
        pub sel_start: Cell<Option<(f64, f64)>>,
        pub sel_style: Cell<SelectionStyle>,
        pub selecting: Cell<bool>,
        pub press: Cell<Press>,
        pub press_point: Cell<(f64, f64)>,
        pub find: RefCell<FindState>,
        pub caret_enabled: Cell<bool>,
        pub caret: Cell<Option<(usize, usize)>>,
        pub caret_anchor: Cell<Option<(usize, usize)>>,
        pub caret_visible: Cell<bool>,
        pub caret_blink: RefCell<Option<glib::SourceId>>,
        pub add_mode: Cell<Option<AddMode>>,
        pub synctex_hl: RefCell<Option<(usize, Rect, Instant)>>,
        pub listeners: RefCell<Vec<Rc<dyn Fn(&ViewEvent)>>>,
        pub popup_ctx: RefCell<PopupContext>,
        pub children: RefCell<Vec<Child>>,
        pub pending_scroll: Cell<Option<(usize, f64, f64)>>,
        pub touch_samples: RefCell<Vec<(u32, f64, f64)>>,
        pub touch_flip: Cell<(f64, bool)>,
        pub kinetic: RefCell<Option<gtk::TickCallbackId>>,
        pub zoom_anchor: Cell<Option<(usize, f64, f64, f64, f64)>>,
        pub internal_page_change: Cell<bool>,
        pub autoscroll: RefCell<Option<(f64, f64, glib::SourceId)>>,
        pub pointer: Cell<(f64, f64)>,
        pub dpi: Cell<f64>,
        pub link_preview: RefCell<Option<(gtk::Popover, glib::SourceId)>>,
        pub zoom_gesture: RefCell<Option<gtk::GestureZoom>>,
        pub drag_start: Cell<Option<(u32, f64, f64)>>,
        pub hover_link: RefCell<Option<Link>>,
        pub pan_origin: Cell<(f64, f64)>,
        pub zoom_gesture_scale: Cell<f64>,
        pub last_alloc: Cell<(i32, i32)>,
        pub annot_popup: RefCell<Option<gtk::Popover>>,
        pub cache_bytes: Cell<f64>,
        pub find_text_cache: RefCell<HashMap<usize, TextLayout>>,
    }

    impl Default for View {
        fn default() -> Self {
            View {
                hadjustment: RefCell::new(None),
                vadjustment: RefCell::new(None),
                hscroll_policy: Cell::new(gtk::ScrollablePolicy::Minimum),
                vscroll_policy: Cell::new(gtk::ScrollablePolicy::Minimum),
                model: RefCell::new(None),
                model_handler: Cell::new(0),
                doc: RefCell::new(None),
                layout: RefCell::new(Layout::default()),
                textures: RefCell::new(HashMap::new()),
                pending: RefCell::new(HashMap::new()),
                versions: RefCell::new(HashMap::new()),
                page_data: RefCell::new(HashMap::new()),
                data_pending: RefCell::new(HashMap::new()),
                selection: RefCell::new(Vec::new()),
                sel_start: Cell::new(None),
                sel_style: Cell::new(SelectionStyle::Glyph),
                selecting: Cell::new(false),
                press: Cell::new(Press::None),
                press_point: Cell::new((0.0, 0.0)),
                find: RefCell::new(FindState { highlight: true, ..Default::default() }),
                caret_enabled: Cell::new(false),
                caret: Cell::new(None),
                caret_anchor: Cell::new(None),
                caret_visible: Cell::new(true),
                caret_blink: RefCell::new(None),
                add_mode: Cell::new(None),
                synctex_hl: RefCell::new(None),
                listeners: RefCell::new(Vec::new()),
                popup_ctx: RefCell::new(PopupContext::default()),
                children: RefCell::new(Vec::new()),
                pending_scroll: Cell::new(None),
                touch_samples: RefCell::new(Vec::new()),
                touch_flip: Cell::new((0.0, false)),
                kinetic: RefCell::new(None),
                zoom_anchor: Cell::new(None),
                internal_page_change: Cell::new(false),
                autoscroll: RefCell::new(None),
                pointer: Cell::new((0.0, 0.0)),
                dpi: Cell::new(96.0),
                link_preview: RefCell::new(None),
                zoom_gesture: RefCell::new(None),
                drag_start: Cell::new(None),
                hover_link: RefCell::new(None),
                pan_origin: Cell::new((0.0, 0.0)),
                zoom_gesture_scale: Cell::new(1.0),
                last_alloc: Cell::new((0, 0)),
                annot_popup: RefCell::new(None),
                cache_bytes: Cell::new(50.0 * 1024.0 * 1024.0),
                find_text_cache: RefCell::new(HashMap::new()),
            }
        }
    }

    impl View {
        fn set_hadjustment(&self, adj: Option<gtk::Adjustment>) {
            self.set_adj(adj, true);
        }
        fn set_vadjustment(&self, adj: Option<gtk::Adjustment>) {
            self.set_adj(adj, false);
        }
        fn set_adj(&self, adj: Option<gtk::Adjustment>, horizontal: bool) {
            if let Some(a) = &adj {
                let obj = self.obj().downgrade();
                a.connect_value_changed(move |_| {
                    if let Some(v) = obj.upgrade() {
                        v.on_scrolled();
                    }
                });
            }
            if horizontal {
                *self.hadjustment.borrow_mut() = adj;
            } else {
                *self.vadjustment.borrow_mut() = adj;
            }
            self.obj().queue_resize();
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for View {
        const NAME: &'static str = "EvView";
        type Type = super::View;
        type ParentType = gtk::Container;
        type Interfaces = (gtk::Scrollable,);

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("evview");
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for View {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_has_window(true);
            obj.set_can_focus(true);
            obj.set_redraw_on_allocate(false);
            self.dpi.set(crate::view::widget_dpi(&*obj));
            obj.setup_controllers();
        }

        fn dispose(&self) {
            for c in self.children.borrow_mut().drain(..) {
                c.widget.unparent();
            }
            if let Some(p) = self.annot_popup.borrow_mut().take() {
                super::destroy_widget(&p);
            }
            if let Some((p, id)) = self.link_preview.borrow_mut().take() {
                id.remove();
                super::destroy_widget(&p);
            }
            self.zoom_gesture.borrow_mut().take();
            if let Some(m) = self.model.borrow().as_ref() {
                m.disconnect(self.model_handler.get());
            }
        }
    }

    impl WidgetImpl for View {
        fn preferred_width(&self) -> (i32, i32) {
            (0, 0)
        }

        fn preferred_height(&self) -> (i32, i32) {
            (0, 0)
        }

        fn size_allocate(&self, allocation: &gtk::Allocation) {
            let obj = self.obj();
            obj.set_allocation(allocation);
            if obj.is_realized() {
                if let Some(w) = obj.window() {
                    w.move_resize(allocation.x(), allocation.y(), allocation.width(), allocation.height());
                }
            }
            let (width, height) = (allocation.width(), allocation.height());
            let changed = self.last_alloc.get() != (width, height);
            self.last_alloc.set((width, height));
            if changed || self.layout.borrow().boxes.is_empty() {
                obj.update_dpi();
                obj.apply_sizing_mode();
            }
            obj.relayout();
            obj.allocate_children();
        }

        fn draw(&self, cr: &cairo::Context) -> glib::Propagation {
            let obj = self.obj();
            obj.draw_view(cr);
            for c in self.children.borrow().iter() {
                obj.propagate_draw(&c.widget, cr);
            }
            glib::Propagation::Proceed
        }

        fn realize(&self) {
            let obj = self.obj();
            let a = obj.allocation();
            let attrs = gdk::WindowAttr {
                x: Some(a.x()),
                y: Some(a.y()),
                width: a.width(),
                height: a.height(),
                wclass: gdk::WindowWindowClass::InputOutput,
                window_type: gdk::WindowType::Child,
                visual: obj.visual(),
                event_mask: obj.events()
                    | gdk::EventMask::EXPOSURE_MASK
                    | gdk::EventMask::BUTTON_PRESS_MASK
                    | gdk::EventMask::BUTTON_RELEASE_MASK
                    | gdk::EventMask::POINTER_MOTION_MASK
                    | gdk::EventMask::SCROLL_MASK
                    | gdk::EventMask::SMOOTH_SCROLL_MASK
                    | gdk::EventMask::KEY_PRESS_MASK
                    | gdk::EventMask::LEAVE_NOTIFY_MASK
                    | gdk::EventMask::ENTER_NOTIFY_MASK
                    | gdk::EventMask::TOUCHPAD_GESTURE_MASK
                    | gdk::EventMask::TOUCH_MASK,
                ..Default::default()
            };
            let parent = obj.parent_window();
            let win = gdk::Window::new(parent.as_ref(), &attrs);
            obj.register_window(&win);
            obj.set_window(win.clone());
            obj.set_realized(true);
            for c in self.children.borrow().iter() {
                c.widget.set_parent_window(&win);
            }
            obj.update_dpi();
            let weak = obj.downgrade();
            obj.connect_scale_factor_notify(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.on_scrolled();
                }
            });
        }

        fn button_press_event(&self, event: &gdk::EventButton) -> glib::Propagation {
            self.obj().on_button_press(event)
        }

        fn button_release_event(&self, event: &gdk::EventButton) -> glib::Propagation {
            self.obj().on_button_release(event)
        }

        fn motion_notify_event(&self, event: &gdk::EventMotion) -> glib::Propagation {
            self.obj().on_motion(event)
        }

        fn scroll_event(&self, event: &gdk::EventScroll) -> glib::Propagation {
            self.obj().on_scroll(event)
        }

        fn leave_notify_event(&self, _event: &gdk::EventCrossing) -> glib::Propagation {
            self.obj().set_hover_link(None, 0.0, 0.0);
            glib::Propagation::Proceed
        }
    }

    impl ContainerImpl for View {
        fn add(&self, _widget: &gtk::Widget) {}

        fn remove(&self, widget: &gtk::Widget) {
            let mut ch = self.children.borrow_mut();
            if let Some(i) = ch.iter().position(|c| &c.widget == widget) {
                let c = ch.remove(i);
                drop(ch);
                c.widget.unparent();
                self.obj().queue_resize();
            }
        }

        fn forall(&self, _include_internals: bool, callback: &gtk::subclass::container::Callback) {
            let kids: Vec<gtk::Widget> = self.children.borrow().iter().map(|c| c.widget.clone()).collect();
            for k in kids {
                callback.call(&k);
            }
        }
    }

    // gtk-rs 0.18 has no ScrollableImpl; GtkScrollable has no required vfuncs, only properties.
    unsafe impl IsImplementable<View> for gtk::Scrollable {}
}

glib::wrapper! {
    pub struct View(ObjectSubclass<imp::View>)
        @extends gtk::Container, gtk::Widget,
        @implements gtk::Buildable, gtk::Scrollable;
}

/// Destroys a toplevel-like widget (popover, window) that GTK keeps alive on its own.
pub(crate) fn destroy_widget(w: &impl IsA<gtk::Widget>) {
    unsafe { w.as_ref().destroy() };
}

fn set_source(cr: &cairo::Context, c: (f64, f64, f64, f64)) {
    cr.set_source_rgba(c.0, c.1, c.2, c.3);
}

fn fill_rect(cr: &cairo::Context, c: (f64, f64, f64, f64), x: f64, y: f64, w: f64, h: f64) {
    set_source(cr, c);
    cr.rectangle(x, y, w, h);
    let _ = cr.fill();
}

fn fill_r(cr: &cairo::Context, c: (f64, f64, f64, f64), r: &Rect) {
    fill_rect(cr, c, r.x1, r.y1, r.x2 - r.x1, r.y2 - r.y1);
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}


impl View {
    pub fn new() -> View {
        glib::Object::new()
    }

    fn imp_(&self) -> &imp::View {
        self.imp()
    }

    pub fn connect_event(&self, f: impl Fn(&ViewEvent) + 'static) {
        self.imp_().listeners.borrow_mut().push(Rc::new(f));
    }

    fn emit(&self, ev: ViewEvent) {
        let ls: Vec<_> = self.imp_().listeners.borrow().clone();
        for l in ls {
            l(&ev);
        }
    }

    pub fn set_page_cache_size(&self, bytes: f64) {
        self.imp_().cache_bytes.set(bytes);
    }

    pub fn model(&self) -> Option<Rc<Model>> {
        self.imp_().model.borrow().clone()
    }

    pub fn document(&self) -> Option<Rc<DocHandle>> {
        self.imp_().doc.borrow().clone()
    }

    pub fn set_model(&self, model: &Rc<Model>) {
        let imp = self.imp_();
        if let Some(old) = imp.model.borrow().as_ref() {
            old.disconnect(imp.model_handler.get());
        }
        let weak = self.downgrade();
        let id = model.connect(move |c| {
            if let Some(v) = weak.upgrade() {
                v.on_model_changed(c);
            }
        });
        imp.model_handler.set(id);
        *imp.model.borrow_mut() = Some(model.clone());
        self.on_model_changed(Change::Document);
    }

    fn on_model_changed(&self, c: Change) {
        let imp = self.imp_();
        let Some(model) = self.model() else { return };
        match c {
            Change::Document => {
                *imp.doc.borrow_mut() = model.document();
                self.clear_caches();
                imp.selection.borrow_mut().clear();
                self.find_cancel();
                imp.caret.set(None);
                imp.last_alloc.set((0, 0));
                self.queue_resize();
                self.queue_draw();
                if model.document().is_some() {
                    imp.pending_scroll.set(Some((model.page(), 0.0, 0.0)));
                }
            }
            Change::Page { new, .. } => {
                if imp.internal_page_change.get() {
                    return;
                }
                if model.continuous() {
                    self.scroll_to_page_point(new, None, None);
                } else {
                    imp.pending_scroll.set(Some((new, 0.0, 0.0)));
                    self.apply_sizing_mode();
                    self.relayout();
                }
            }
            Change::Scale => {
                self.capture_zoom_anchor();
                self.relayout();
            }
            Change::Rotation => {
                let page = model.page();
                imp.pending_scroll.set(Some((page, 0.0, 0.0)));
                self.apply_sizing_mode();
                self.relayout();
                self.close_children();
            }
            Change::Sizing => {
                self.apply_sizing_mode();
                self.relayout();
            }
            Change::Continuous | Change::DualPage | Change::DualOddLeft | Change::Rtl => {
                imp.pending_scroll.set(Some((model.page(), 0.0, 0.0)));
                self.apply_sizing_mode();
                self.relayout();
            }
            Change::Inverted => self.queue_draw(),
            Change::Fullscreen | Change::MaxScale => {}
        }
    }

    fn clear_caches(&self) {
        let imp = self.imp_();
        for (_, p) in imp.pending.borrow_mut().drain() {
            p.job.cancel();
        }
        for (_, j) in imp.data_pending.borrow_mut().drain() {
            j.cancel();
        }
        imp.textures.borrow_mut().clear();
        imp.page_data.borrow_mut().clear();
        imp.versions.borrow_mut().clear();
        imp.find_text_cache.borrow_mut().clear();
        self.close_children();
    }

    fn update_dpi(&self) {
        let dpi = super::widget_dpi(self);
        let old = self.imp_().dpi.replace(dpi);
        if old != dpi {
            if let Some(m) = self.model().filter(|m| m.sizing_mode() == SizingMode::Free) {
                m.set_scale(m.scale() * dpi / old);
            }
        }
    }

    pub fn dpi(&self) -> f64 {
        self.imp_().dpi.get()
    }

    fn hadj(&self) -> Option<gtk::Adjustment> {
        self.imp_().hadjustment.borrow().clone()
    }

    fn vadj(&self) -> Option<gtk::Adjustment> {
        self.imp_().vadjustment.borrow().clone()
    }

    fn scroll_offset(&self) -> (f64, f64) {
        (self.hadj().map_or(0.0, |a| a.value()), self.vadj().map_or(0.0, |a| a.value()))
    }

    fn viewport(&self) -> (f64, f64) {
        (self.allocated_width() as f64, self.allocated_height() as f64)
    }

    // ---------------------------------------------------------------- sizing

    fn apply_sizing_mode(&self) {
        let Some(model) = self.model() else { return };
        let Some(doc) = self.document() else { return };
        let (w, h) = self.viewport();
        if w <= 1.0 || h <= 1.0 {
            return;
        }
        let mode = model.sizing_mode();
        if mode == SizingMode::Free {
            return;
        }
        let rot = model.rotation();
        let b = BORDER;
        let sp = layout::SPACING;
        let (mut dw, mut dh) = if model.continuous() {
            doc.max_size
        } else if model.dual_page() {
            let (l, r) = layout::dual_row(model.page(), doc.n_pages, model.dual_odd_left());
            [l, r].iter().flatten().map(|&p| doc.page_size(p)).fold((0.0f64, 0.0f64), |a, s| (a.0.max(s.0), a.1.max(s.1)))
        } else {
            doc.page_size(model.page())
        };
        if rot == 90 || rot == 270 {
            std::mem::swap(&mut dw, &mut dh);
        }
        let (mut tw, th);
        if model.dual_page() {
            dw *= 2.0;
            tw = w - (2.0 * b.h() + 3.0 * sp);
            th = h - (b.v() + 2.0 * sp);
        } else {
            tw = w - (b.h() + 2.0 * sp);
            th = h - (b.v() + 2.0 * sp);
        }
        let sb = if mode != SizingMode::FitPage || model.continuous() { 0.0 } else { 0.0 };
        tw -= sb;
        let fit_w = tw / dw;
        let fit_h = th / dh;
        let scale = match mode {
            SizingMode::FitWidth => fit_w,
            SizingMode::FitPage => fit_w.min(fit_h),
            SizingMode::Automatic => {
                if dh < dw {
                    fit_w.min(fit_h)
                } else {
                    fit_w.min(self.dpi() / 72.0)
                }
            }
            SizingMode::Free => return,
        };
        if scale > 0.0 && scale.is_finite() {
            model.set_scale(scale);
        }
    }

    // ---------------------------------------------------------------- layout

    fn compute_layout(&self) -> Layout {
        let (Some(model), Some(doc)) = (self.model(), self.document()) else {
            return Layout::default();
        };
        layout::compute(&layout::Params {
            sizes: &doc.sizes,
            scale: model.scale(),
            rotation: model.rotation(),
            continuous: model.continuous(),
            dual: model.dual_page(),
            odd_left: model.dual_odd_left(),
            rtl: model.rtl(),
            current: model.page(),
            viewport: self.viewport(),
        })
    }

    fn capture_zoom_anchor(&self) {
        let imp = self.imp_();
        if imp.zoom_anchor.get().is_some() {
            return;
        }
        let (vw, vh) = self.viewport();
        let (px, py) = (vw / 2.0, vh / 2.0);
        if let Some((page, x, y)) = self.point_to_page(px, py, true) {
            imp.zoom_anchor.set(Some((page, x, y, px, py)));
        }
    }

    fn relayout(&self) {
        let imp = self.imp_();
        let new = self.compute_layout();
        let (vw, vh) = self.viewport();
        *imp.layout.borrow_mut() = new.clone();
        let hadj = self.hadj();
        let vadj = self.vadj();
        if let Some(h) = &hadj {
            let upper = new.width.max(vw);
            let v = h.value().min((upper - vw).max(0.0));
            h.configure(v, 0.0, upper, vw * 0.1, vw * 0.9, vw);
        }
        if let Some(v) = &vadj {
            let upper = new.height.max(vh);
            let val = v.value().min((upper - vh).max(0.0));
            v.configure(val, 0.0, upper, vh * 0.1, vh * 0.9, vh);
        }
        if let Some((page, x, y)) = imp.pending_scroll.take() {
            if self.document().is_some() && vw > 1.0 {
                if x == 0.0 && y == 0.0 {
                    self.scroll_to_page_point(page, None, None);
                } else {
                    self.scroll_to_page_point(page, Some(x), Some(y));
                }
            } else {
                imp.pending_scroll.set(Some((page, x, y)));
            }
        } else if let Some((page, x, y, px, py)) = imp.zoom_anchor.take() {
            if let (Some(bx), Some(doc)) = (new.page_box(page).copied(), self.document()) {
                let r = new.page_to_canvas(&bx, doc.page_size(page), Rect::new(x, y, x, y));
                if let Some(h) = &hadj {
                    h.set_value((r.x1 - px).clamp(h.lower(), (h.upper() - h.page_size()).max(0.0)));
                }
                if let Some(v) = &vadj {
                    v.set_value((r.y1 - py).clamp(v.lower(), (v.upper() - v.page_size()).max(0.0)));
                }
            }
        }
        self.on_scrolled();
    }

    /// Scrolls so that the given page point is at the top-left of the viewport.
    pub fn scroll_to_page_point(&self, page: usize, x: Option<f64>, y: Option<f64>) {
        let l = self.imp_().layout.borrow().clone();
        let Some(doc) = self.document() else { return };
        let Some(bx) = l.page_box(page).copied() else {
            self.imp_().pending_scroll.set(Some((page, x.unwrap_or(0.0), y.unwrap_or(0.0))));
            return;
        };
        let r = l.page_to_canvas(&bx, doc.page_size(page), Rect::new(x.unwrap_or(0.0), y.unwrap_or(0.0), 0.0, 0.0));
        if let Some(v) = self.vadj() {
            let target = if y.is_some() { r.y1 } else { bx.y - BORDER.top - layout::SPACING };
            v.set_value(target.clamp(0.0, (v.upper() - v.page_size()).max(0.0)));
        }
        if let Some(h) = self.hadj() {
            if x.is_some() {
                h.set_value(r.x1.clamp(0.0, (h.upper() - h.page_size()).max(0.0)));
            } else if bx.x + bx.w > h.value() + h.page_size() || bx.x < h.value() {
                let c = bx.x + bx.w / 2.0 - h.page_size() / 2.0;
                h.set_value(c.clamp(0.0, (h.upper() - h.page_size()).max(0.0)));
            }
        }
        self.queue_draw();
    }

    fn scroll_to_rect(&self, page: usize, r: Rect) {
        let l = self.imp_().layout.borrow().clone();
        let Some(doc) = self.document() else { return };
        let Some(bx) = l.page_box(page).copied() else { return };
        let c = l.page_to_canvas(&bx, doc.page_size(page), r).normalized();
        if let Some(v) = self.vadj() {
            let (val, ps) = (v.value(), v.page_size());
            if c.y1 < val || c.y2 > val + ps {
                v.set_value((c.y1 - ps / 3.0).clamp(0.0, (v.upper() - ps).max(0.0)));
            }
        }
        if let Some(h) = self.hadj() {
            let (val, ps) = (h.value(), h.page_size());
            if c.x1 < val || c.x2 > val + ps {
                h.set_value((c.x1 - ps / 3.0).clamp(0.0, (h.upper() - ps).max(0.0)));
            }
        }
    }

    fn on_scrolled(&self) {
        let imp = self.imp_();
        let (Some(model), Some(doc)) = (self.model(), self.document()) else {
            self.queue_draw();
            return;
        };
        let (ox, oy) = self.scroll_offset();
        let (vw, vh) = self.viewport();
        let l = imp.layout.borrow().clone();
        let mut visible = Vec::new();
        let mut best: Option<(usize, f64)> = None;
        for b in &l.boxes {
            let x1 = b.x.max(ox);
            let x2 = (b.x + b.w).min(ox + vw);
            let y1 = b.y.max(oy);
            let y2 = (b.y + b.h).min(oy + vh);
            if x2 > x1 && y2 > y1 {
                visible.push(b.page);
                let area = (x2 - x1) * (y2 - y1);
                if best.is_none_or(|(_, a)| area > a + 1.0) {
                    best = Some((b.page, area));
                }
            }
        }
        if model.continuous() && self.is_mapped() && vw > 1.0 && vh > 1.0 {
            if let Some((p, _)) = best {
                if p != model.page() {
                    imp.internal_page_change.set(true);
                    model.set_page(p);
                    imp.internal_page_change.set(false);
                }
            }
        }
        visible.sort_unstable();
        let mut wanted: Vec<(usize, Priority)> = visible.iter().map(|&p| (p, Priority::Urgent)).collect();
        if let (Some(&first), Some(&last)) = (visible.first(), visible.last()) {
            let extra = if model.dual_page() { 2 } else { 1 };
            for i in 1..=extra {
                if first >= i {
                    wanted.push((first - i, Priority::High));
                }
                if last + i < doc.n_pages {
                    wanted.push((last + i, Priority::High));
                }
            }
        }
        self.request_renders(&doc, &wanted);
        for (p, _) in &wanted {
            self.request_page_data(&doc, *p);
        }
        self.allocate_children();
        self.queue_draw();
    }

    /// Physical pixels per logical pixel (GTK 3 only supports integer scales).
    pub fn device_scale(&self) -> f64 {
        self.scale_factor().max(1) as f64
    }

    fn render_scale(&self) -> f64 {
        let l = self.imp_().layout.borrow();
        l.scale * self.device_scale()
    }

    fn request_renders(&self, doc: &Rc<DocHandle>, wanted: &[(usize, Priority)]) {
        let imp = self.imp_();
        let scale = self.render_scale();
        let rot = imp.layout.borrow().rotation;
        let keep: HashSet<usize> = wanted.iter().map(|w| w.0).collect();
        {
            let mut pend = imp.pending.borrow_mut();
            pend.retain(|p, j| {
                let k = keep.contains(p);
                if !k {
                    j.job.cancel();
                }
                k
            });
        }
        {
            let mut texs = imp.textures.borrow_mut();
            let (lo, hi) = (
                keep.iter().min().copied().unwrap_or(0).saturating_sub(2),
                keep.iter().max().copied().unwrap_or(0) + 2,
            );
            texs.retain(|p, _| *p >= lo && *p <= hi);
        }
        for &(page, prio) in wanted {
            let version = imp.versions.borrow().get(&page).copied().unwrap_or(0);
            let up_to_date = imp
                .textures
                .borrow()
                .get(&page)
                .is_some_and(|t| (t.scale - scale).abs() < 1e-6 && t.rotation == rot && t.version == version);
            if up_to_date {
                continue;
            }
            let already = imp
                .pending
                .borrow()
                .get(&page)
                .is_some_and(|p| (p.scale - scale).abs() < 1e-6 && p.rotation == rot && p.version == version);
            if already {
                continue;
            }
            if let Some(old) = imp.pending.borrow_mut().remove(&page) {
                old.job.cancel();
            }
            let (pw, ph) = doc.page_size(page);
            let max_px = 8192.0 * 8192.0;
            let eff = if pw * ph * scale * scale > max_px { (max_px / (pw * ph)).sqrt() } else { scale };
            let weak = self.downgrade();
            let job = doc.jobs.run(
                prio,
                move |d| d.render(page, eff, rot).and_then(Bitmap::from_surface),
                move |bm| {
                    let Some(v) = weak.upgrade() else { return };
                    v.imp_().pending.borrow_mut().remove(&page);
                    if let Some(tex) = bm.and_then(|bm| bm.to_surface()) {
                        v.imp_().textures.borrow_mut().insert(page, Tex { tex, scale, rotation: rot, version });
                        v.queue_draw();
                    }
                },
            );
            imp.pending.borrow_mut().insert(page, Pending { scale, rotation: rot, version, job });
        }
    }

    fn request_page_data(&self, doc: &Rc<DocHandle>, page: usize) {
        let imp = self.imp_();
        if imp.page_data.borrow().contains_key(&page) || imp.data_pending.borrow().contains_key(&page) {
            return;
        }
        let has_text = doc.has_text;
        let weak = self.downgrade();
        let job = doc.jobs.run(
            Priority::High,
            move |d| PageData {
                links: d.links(page),
                images: d.images(page),
                annots: d.annotations(page),
                forms: d.form_fields(page),
                media: d.media(page),
                text: if has_text { d.text_layout(page) } else { None },
            },
            move |data| {
                let Some(v) = weak.upgrade() else { return };
                v.imp_().data_pending.borrow_mut().remove(&page);
                v.imp_().page_data.borrow_mut().insert(page, data);
                v.queue_draw();
            },
        );
        imp.data_pending.borrow_mut().insert(page, job);
    }

    /// Device-pixel origin (widget coordinates) and image of a drawn page, for diagnostics.
    pub fn debug_page_device(&self, page: usize) -> Option<(f64, f64, cairo::ImageSurface)> {
        let imp = self.imp_();
        let l = imp.layout.borrow();
        let bx = l.page_box(page)?;
        let tex = imp.textures.borrow().get(&page)?.tex.clone();
        let ds = self.device_scale();
        let (ox, oy) = self.scroll_offset();
        Some((((bx.x - ox) * ds).round(), ((bx.y - oy) * ds).round(), tex))
    }

    /// Scale and texture geometry, used by the debug diagnostics.
    pub fn debug_render(&self) -> String {
        let imp = self.imp_();
        let l = imp.layout.borrow();
        let frac = Some(self.device_scale());
        let mut out = format!(
            "scale_factor={} device_scale={} layout_scale={:.4}",
            self.scale_factor(),
            self.device_scale(),
            l.scale,
        );
        for (p, t) in imp.textures.borrow().iter() {
            if let Some(bx) = l.page_box(*p) {
                out += &format!(
                    "\n  page {p}: tex={}x{} tex_scale={:.4} box={:.2}x{:.2} box_px@frac={:.1}x{:.1}",
                    t.tex.width(),
                    t.tex.height(),
                    t.scale,
                    bx.w,
                    bx.h,
                    bx.w * frac.unwrap_or(1.0),
                    bx.h * frac.unwrap_or(1.0)
                );
            }
        }
        out
    }

    /// Textual summary of the view state, used by the debug self-test.
    pub fn debug_state(&self) -> String {
        let imp = self.imp_();
        let mut tex: Vec<usize> = imp.textures.borrow().keys().copied().collect();
        tex.sort_unstable();
        let l = imp.layout.borrow();
        let (ox, oy) = self.scroll_offset();
        format!(
            "alloc={}x{} canvas={:.0}x{:.0} scale={:.3} rot={} boxes={} textures={:?} scroll=({:.0},{:.0}) links_p0={} sel={}",
            self.allocated_width(),
            self.allocated_height(),
            l.width,
            l.height,
            l.scale,
            l.rotation,
            l.boxes.len(),
            tex,
            ox,
            oy,
            imp.page_data.borrow().get(&0).map_or(0, |d| d.links.len()),
            imp.selection.borrow().len()
        )
    }

    pub fn page_data(&self, page: usize) -> Option<PageData> {
        self.imp_().page_data.borrow().get(&page).cloned()
    }

    /// Marks a page as changed: it is re-rendered and its interactive data reloaded.
    pub fn invalidate_page(&self, page: usize) {
        let imp = self.imp_();
        *imp.versions.borrow_mut().entry(page).or_insert(0) += 1;
        imp.page_data.borrow_mut().remove(&page);
        if let Some(j) = imp.data_pending.borrow_mut().remove(&page) {
            j.cancel();
        }
        self.on_scrolled();
    }

    pub fn invalidate_all(&self) {
        let Some(doc) = self.document() else { return };
        let imp = self.imp_();
        {
            let mut v = imp.versions.borrow_mut();
            for p in 0..doc.n_pages {
                *v.entry(p).or_insert(0) += 1;
            }
        }
        imp.page_data.borrow_mut().clear();
        for (_, j) in imp.data_pending.borrow_mut().drain() {
            j.cancel();
        }
        self.on_scrolled();
    }

    // ---------------------------------------------------------------- geometry

    /// Maps a widget point to (page, x, y) in unrotated page points.
    pub fn point_to_page(&self, wx: f64, wy: f64, nearest: bool) -> Option<(usize, f64, f64)> {
        let doc = self.document()?;
        let (ox, oy) = self.scroll_offset();
        let (cx, cy) = (wx + ox, wy + oy);
        let l = self.imp_().layout.borrow();
        let bx = if nearest { l.box_at(cx, cy).or_else(|| l.nearest_box(cx, cy)) } else { l.box_at(cx, cy) }?;
        let (px, py) = l.canvas_to_page(bx, doc.page_size(bx.page), cx, cy);
        Some((bx.page, px, py))
    }

    /// Maps a rectangle in page points to widget coordinates.
    pub fn page_rect_to_widget(&self, page: usize, r: Rect) -> Option<Rect> {
        let doc = self.document()?;
        let l = self.imp_().layout.borrow();
        let bx = l.page_box(page)?;
        let c = l.page_to_canvas(bx, doc.page_size(page), r).normalized();
        let (ox, oy) = self.scroll_offset();
        Some(Rect::new(c.x1 - ox, c.y1 - oy, c.x2 - ox, c.y2 - oy))
    }

    // ---------------------------------------------------------------- drawing

    fn draw_view(&self, cr: &cairo::Context) {
        let imp = self.imp_();
        let Some(doc) = self.document() else { return };
        let Some(model) = self.model() else { return };
        let (ox, oy) = self.scroll_offset();
        let (vw, vh) = self.viewport();
        let l = imp.layout.borrow().clone();
        let inverted = model.inverted();
        let rot = l.rotation;
        let sel = imp.selection.borrow().clone();
        let find = imp.find.borrow();
        let accent = (0.21, 0.52, 0.89, 0.35);
        let ds = self.device_scale();
        let want_scale = l.scale * ds;
        let snap = |v: f64| (v * ds).round() / ds;
        for bx in &l.boxes {
            let x = snap(bx.x - ox);
            let y = snap(bx.y - oy);
            if x + bx.w + BORDER.right < 0.0 || y + bx.h + BORDER.bottom < 0.0 || x - BORDER.left > vw || y - BORDER.top > vh {
                continue;
            }
            fill_rect(cr, (0.0, 0.0, 0.0, 0.18), x - BORDER.left + 1.0, y - BORDER.top + 1.0, bx.w + BORDER.h() - 1.0, bx.h + BORDER.v() - 1.0);
            fill_rect(cr, (0.0, 0.0, 0.0, 0.35), x - 1.0, y - 1.0, bx.w + 2.0, bx.h + 2.0);
            fill_rect(cr, (1.0, 1.0, 1.0, 1.0), x, y, bx.w, bx.h);
            if let Some(t) = imp.textures.borrow().get(&bx.page) {
                if t.rotation == rot && t.tex.width() > 0 && t.tex.height() > 0 {
                    cr.save().ok();
                    cr.rectangle(x, y, bx.w, bx.h);
                    cr.clip();
                    cr.translate(x, y);
                    if (t.scale - want_scale).abs() < 1e-6 {
                        cr.scale(1.0 / ds, 1.0 / ds);
                    } else {
                        cr.scale(bx.w / t.tex.width() as f64, bx.h / t.tex.height() as f64);
                    }
                    let _ = cr.set_source_surface(&t.tex, 0.0, 0.0);
                    let _ = cr.paint();
                    cr.restore().ok();
                }
            }
            if inverted {
                cr.save().ok();
                cr.set_operator(cairo::Operator::Difference);
                fill_rect(cr, (1.0, 1.0, 1.0, 1.0), x, y, bx.w, bx.h);
                cr.restore().ok();
            }
            let size = doc.page_size(bx.page);
            let to_w = |r: &Rect| {
                let c = l.page_to_canvas(bx, size, *r).normalized();
                Rect::new(c.x1 - ox, c.y1 - oy, c.x2 - ox, c.y2 - oy)
            };
            if find.highlight {
                if let Some(results) = find.results.get(&bx.page) {
                    for (i, m) in results.iter().enumerate() {
                        let current = find.current == Some((bx.page, i));
                        let color = if current { (0.98, 0.55, 0.1, 0.55) } else { (0.95, 0.85, 0.15, 0.4) };
                        for r in &m.rects {
                            fill_r(cr, color, &to_w(r));
                        }
                    }
                }
            }
            for s in sel.iter().filter(|s| s.page == bx.page) {
                for r in &s.region {
                    fill_r(cr, accent, &to_w(r));
                }
            }
            if let Some((p, r, t)) = imp.synctex_hl.borrow().as_ref() {
                if *p == bx.page && t.elapsed() < Duration::from_secs(6) {
                    let wr = to_w(r);
                    let gr = Rect::new(wr.x1 - 2.0, wr.y1 - 2.0, wr.x2 + 2.0, wr.y2 + 2.0);
                    fill_r(cr, (1.0, 0.1, 0.1, 0.18), &gr);
                    set_source(cr, (0.9, 0.1, 0.1, 0.8));
                    cr.set_line_width(1.5);
                    cr.rectangle(gr.x1 + 0.75, gr.y1 + 0.75, gr.x2 - gr.x1 - 1.5, gr.y2 - gr.y1 - 1.5);
                    let _ = cr.stroke();
                }
            }
            if imp.caret_enabled.get() && imp.caret_visible.get() && self.has_focus() {
                if let Some((cp, off)) = imp.caret.get() {
                    if cp == bx.page {
                        if let Some(r) = self.caret_rect(cp, off) {
                            let wr = to_w(&r);
                            #[allow(deprecated)]
                            let c = self.style_context().color(gtk::StateFlags::NORMAL);
                            fill_rect(cr, (c.red(), c.green(), c.blue(), c.alpha()), wr.x1, wr.y1, 1.0, wr.y2 - wr.y1);
                        }
                    }
                }
            }
        }
    }

    fn caret_rect(&self, page: usize, offset: usize) -> Option<Rect> {
        let data = self.imp_().page_data.borrow();
        let text = data.get(&page)?.text.as_ref()?;
        if text.rects.is_empty() {
            return None;
        }
        if offset < text.rects.len() {
            let r = text.rects[offset];
            Some(Rect::new(r.x1, r.y1, r.x1, r.y2))
        } else {
            let r = *text.rects.last()?;
            Some(Rect::new(r.x2, r.y1, r.x2, r.y2))
        }
    }

    // ---------------------------------------------------------------- children

    fn add_child(&self, widget: &gtk::Widget, page: usize, rect: Rect) {
        if let Some(w) = self.window() {
            widget.set_parent_window(&w);
        }
        widget.set_parent(self);
        self.imp_().children.borrow_mut().push(Child { widget: widget.clone(), page, rect });
        widget.show_all();
        self.allocate_children();
        self.queue_resize();
    }

    fn remove_child(&self, widget: &gtk::Widget) {
        self.remove(widget);
    }

    fn set_cursor_name(&self, name: Option<&str>) {
        let Some(win) = self.window() else { return };
        let cursor = name.and_then(|n| gdk::Cursor::from_name(&self.display(), n));
        win.set_cursor(cursor.as_ref());
    }

    fn close_children(&self) {
        let kids: Vec<gtk::Widget> = self.imp_().children.borrow().iter().map(|c| c.widget.clone()).collect();
        for k in kids {
            self.remove_child(&k);
        }
        if let Some(p) = self.imp_().annot_popup.borrow().as_ref() {
            p.popdown();
        }
    }

    fn allocate_children(&self) {
        let kids: Vec<(gtk::Widget, usize, Rect)> =
            self.imp_().children.borrow().iter().map(|c| (c.widget.clone(), c.page, c.rect)).collect();
        for (w, page, rect) in kids {
            match self.page_rect_to_widget(page, rect) {
                Some(r) => {
                    w.set_child_visible(true);
                    let (_, nat) = w.preferred_size();
                    let width = (r.width().round() as i32).max(nat.width.min(40));
                    let height = (r.height().round() as i32).max(nat.height.min(24));
                    w.size_allocate(&gtk::Allocation::new(r.x1.round() as i32, r.y1.round() as i32, width, height));
                }
                None => w.set_child_visible(false),
            }
        }
    }

    // ---------------------------------------------------------------- zoom

    pub fn can_zoom_in(&self) -> bool {
        self.model().is_some_and(|m| m.scale() * ZOOM_IN_FACTOR <= m.max_scale() + 1e-6)
    }

    pub fn can_zoom_out(&self) -> bool {
        self.model().is_some_and(|m| m.scale() * ZOOM_OUT_FACTOR >= m.min_scale() - 1e-6)
    }

    pub fn zoom(&self, factor: f64, anchor: Option<(f64, f64)>) {
        let Some(model) = self.model() else { return };
        let (vw, vh) = self.viewport();
        let (px, py) = anchor.unwrap_or((vw / 2.0, vh / 2.0));
        if let Some((page, x, y)) = self.point_to_page(px, py, true) {
            self.imp_().zoom_anchor.set(Some((page, x, y, px, py)));
        }
        model.set_sizing_mode(SizingMode::Free);
        model.set_scale(model.scale() * factor);
        self.imp_().zoom_anchor.set(None);
    }

    pub fn zoom_in(&self) {
        self.zoom(ZOOM_IN_FACTOR, None);
    }

    pub fn zoom_out(&self) {
        self.zoom(ZOOM_OUT_FACTOR, None);
    }

    // ---------------------------------------------------------------- navigation

    pub fn next_page(&self) -> bool {
        let (Some(m), Some(d)) = (self.model(), self.document()) else { return false };
        let step = if m.dual_page() { 2 } else { 1 };
        let cur = m.page();
        let target = if m.dual_page() {
            let (l, r) = layout::dual_row(cur, d.n_pages, m.dual_odd_left());
            r.or(l).unwrap_or(cur) + 1
        } else {
            cur + step
        };
        if target >= d.n_pages {
            return false;
        }
        m.set_page(target);
        true
    }

    pub fn previous_page(&self) -> bool {
        let (Some(m), Some(d)) = (self.model(), self.document()) else { return false };
        let cur = m.page();
        if m.dual_page() {
            let (l, r) = layout::dual_row(cur, d.n_pages, m.dual_odd_left());
            let first = l.or(r).unwrap_or(cur);
            if first == 0 {
                return false;
            }
            let (pl, pr) = layout::dual_row(first - 1, d.n_pages, m.dual_odd_left());
            m.set_page(pl.or(pr).unwrap_or(first - 1));
            return true;
        }
        if cur == 0 {
            return false;
        }
        m.set_page(cur - 1);
        true
    }

    /// Scrolls one screen forward (`forward`) or backward, flipping pages at the ends.
    pub fn scroll_page(&self, forward: bool) {
        let Some(m) = self.model() else { return };
        let Some(v) = self.vadj() else { return };
        let ps = v.page_size();
        let at_end = v.value() + ps >= v.upper() - 1.0;
        let at_start = v.value() <= v.lower() + 1.0;
        if !m.continuous() && forward && at_end {
            if self.next_page() {
                self.imp_().pending_scroll.set(Some((m.page(), 0.0, 0.0)));
            }
            return;
        }
        if !m.continuous() && !forward && at_start {
            if self.previous_page() {
                let p = m.page();
                let weak = self.downgrade();
                glib::idle_add_local_once(move || {
                    let Some(v) = weak.upgrade() else { return };
                    if let Some(a) = v.vadj() {
                        a.set_value(a.upper() - a.page_size());
                    }
                    let _ = p;
                });
            }
            return;
        }
        let delta = ps * 0.9;
        let nv = if forward { v.value() + delta } else { v.value() - delta };
        v.set_value(nv.clamp(v.lower(), (v.upper() - ps).max(0.0)));
    }

    fn scroll_step(&self, dx: f64, dy: f64) {
        if let Some(h) = self.hadj() {
            if dx != 0.0 {
                h.set_value((h.value() + dx * h.step_increment()).clamp(h.lower(), (h.upper() - h.page_size()).max(0.0)));
            }
        }
        if let Some(v) = self.vadj() {
            if dy != 0.0 {
                let m = self.model();
                let at_end = v.value() + v.page_size() >= v.upper() - 1.0;
                let at_start = v.value() <= v.lower() + 1.0;
                if let Some(m) = m.filter(|m| !m.continuous()) {
                    if dy > 0.0 && at_end {
                        if self.next_page() {
                            self.imp_().pending_scroll.set(Some((m.page(), 0.0, 0.0)));
                        }
                        return;
                    }
                    if dy < 0.0 && at_start {
                        self.previous_page();
                        return;
                    }
                }
                v.set_value((v.value() + dy * v.step_increment()).clamp(v.lower(), (v.upper() - v.page_size()).max(0.0)));
            }
        }
    }

    pub fn go_to_dest(&self, dest: &LinkDest) {
        let (Some(m), Some(doc)) = (self.model(), self.document()) else { return };
        let Some((page, dest)) = doc.resolve_dest(dest) else { return };
        self.emit(ViewEvent::HistoryJump { from_page: m.page() });
        match dest {
            LinkDest::Xyz { left, top, zoom, .. } => {
                if let Some(z) = zoom.filter(|z| *z > 1.0) {
                    m.set_sizing_mode(SizingMode::Free);
                    m.set_scale(z * self.dpi() / 72.0);
                }
                self.show_page_point(page, left, top);
            }
            LinkDest::Fit { .. } => {
                m.set_sizing_mode(SizingMode::FitPage);
                self.show_page_point(page, None, None);
            }
            LinkDest::FitH { top, .. } => {
                m.set_sizing_mode(SizingMode::FitWidth);
                self.show_page_point(page, None, top);
            }
            LinkDest::FitV { left, .. } => {
                m.set_sizing_mode(SizingMode::FitPage);
                self.show_page_point(page, left, None);
            }
            LinkDest::FitR { rect, .. } => {
                let (vw, vh) = self.viewport();
                let s = ((vw - 20.0) / rect.width().max(1.0)).min((vh - 20.0) / rect.height().max(1.0));
                m.set_sizing_mode(SizingMode::Free);
                m.set_scale(s);
                self.show_page_point(page, Some(rect.x1), Some(rect.y1));
            }
            _ => self.show_page_point(page, None, None),
        }
    }

    fn show_page_point(&self, page: usize, x: Option<f64>, y: Option<f64>) {
        let Some(m) = self.model() else { return };
        let imp = self.imp_();
        if m.continuous() {
            imp.internal_page_change.set(true);
            m.set_page(page);
            imp.internal_page_change.set(false);
            self.relayout();
            self.scroll_to_page_point(page, x, y);
        } else {
            imp.pending_scroll.set(Some((page, x.unwrap_or(0.0), y.unwrap_or(0.0))));
            if m.page() == page {
                self.relayout();
            } else {
                m.set_page(page);
            }
        }
    }

    /// Follows a link. Internal destinations are handled here, the rest is forwarded.
    pub fn handle_link(&self, link: &Link) {
        let Some(action) = &link.action else { return };
        match action {
            LinkAction::GotoDest(d) => self.go_to_dest(d),
            LinkAction::Layers(states) => {
                if let Some(doc) = self.document() {
                    let states = states.clone();
                    doc.jobs.with(move |d| {
                        let layers = d.layers();
                        fn find(ls: &[Layer], id: usize) -> Option<bool> {
                            for l in ls {
                                if l.id == id {
                                    return Some(l.visible);
                                }
                                if let Some(v) = find(&l.children, id) {
                                    return Some(v);
                                }
                            }
                            None
                        }
                        for (act, ids) in states {
                            for id in ids {
                                let vis = match act {
                                    LayerAction::On => true,
                                    LayerAction::Off => false,
                                    LayerAction::Toggle => !find(&layers, id).unwrap_or(false),
                                };
                                d.set_layer_visible(id, vis);
                            }
                        }
                    });
                    self.invalidate_all();
                    self.emit(ViewEvent::LayersChanged);
                }
            }
            LinkAction::ResetForm { fields, exclude } => {
                if let Some(doc) = self.document() {
                    let (f, e) = (fields.clone(), *exclude);
                    doc.jobs.with(move |d| d.reset_form(&f, e));
                    doc.modified.set(true);
                    self.invalidate_all();
                    self.emit(ViewEvent::FormChanged);
                }
            }
            LinkAction::Named(n) => match n.as_str() {
                "NextPage" => {
                    self.next_page();
                }
                "PrevPage" => {
                    self.previous_page();
                }
                "FirstPage" => {
                    if let Some(m) = self.model() {
                        m.set_page(0)
                    }
                }
                "LastPage" => {
                    if let (Some(m), Some(d)) = (self.model(), self.document()) {
                        m.set_page(d.n_pages.saturating_sub(1))
                    }
                }
                _ => self.emit(ViewEvent::ExternalLink(action.clone())),
            },
            LinkAction::Media { media, .. } => self.play_media(None, media.clone()),
            other => self.emit(ViewEvent::ExternalLink(other.clone())),
        }
    }

    pub fn link_tooltip(link: &Link, doc: &DocHandle) -> Option<String> {
        match link.action.as_ref()? {
            LinkAction::GotoDest(d) => {
                let page = doc.resolve_dest(d).map(|(p, _)| p);
                match page {
                    Some(p) => Some(format!("Go to page {}", doc.label(p))),
                    None => link.title.clone(),
                }
            }
            LinkAction::GotoRemote { file, dest } => Some(match dest.as_ref().and_then(|d| d.page()) {
                Some(p) => format!("Go to page {} on file “{}”", p + 1, file),
                None => format!("Go to file “{file}”"),
            }),
            LinkAction::ExternalUri(u) => Some(u.clone()),
            LinkAction::Launch { file, .. } => Some(format!("Launch {file}")),
            LinkAction::Named(n) => Some(match n.as_str() {
                "PrevPage" => "Go to previous page".into(),
                "NextPage" => "Go to next page".into(),
                "FirstPage" => "Go to first page".into(),
                "LastPage" => "Go to last page".into(),
                "GoToPage" => "Go to page".into(),
                "Find" => "Find".into(),
                "Close" | "Quit" => "Close".into(),
                "Print" => "Print".into(),
                other => format!("Unknown action {other}"),
            }),
            _ => link.title.clone(),
        }
    }

    // ---------------------------------------------------------------- hit testing

    fn link_at(&self, page: usize, x: f64, y: f64) -> Option<Link> {
        let data = self.imp_().page_data.borrow();
        let d = data.get(&page)?;
        d.links.iter().rev().find(|m| m.area.contains(x, y)).map(|m| m.data.clone())
    }

    fn form_at(&self, page: usize, x: f64, y: f64) -> Option<FormField> {
        let data = self.imp_().page_data.borrow();
        let d = data.get(&page)?;
        d.forms.iter().find(|f| f.area.contains(x, y)).cloned()
    }

    fn annot_at(&self, page: usize, x: f64, y: f64) -> Option<Annotation> {
        let data = self.imp_().page_data.borrow();
        let d = data.get(&page)?;
        d.annots.iter().rev().find(|a| !a.hidden && a.area.contains(x, y)).cloned()
    }

    fn image_at(&self, page: usize, x: f64, y: f64) -> Option<i32> {
        let data = self.imp_().page_data.borrow();
        let d = data.get(&page)?;
        d.images.iter().rev().find(|m| m.area.contains(x, y)).map(|m| m.data)
    }

    fn media_at(&self, page: usize, x: f64, y: f64) -> Option<(Rect, Media)> {
        let data = self.imp_().page_data.borrow();
        let d = data.get(&page)?;
        d.media.iter().find(|m| m.area.contains(x, y)).map(|m| (m.area, m.data.clone()))
    }

    fn over_text(&self, page: usize, x: f64, y: f64) -> bool {
        let data = self.imp_().page_data.borrow();
        data.get(&page)
            .and_then(|d| d.text.as_ref())
            .is_some_and(|t| t.rects.iter().any(|r| r.contains(x, y)))
    }

    fn update_cursor(&self, wx: f64, wy: f64) {
        let imp = self.imp_();
        if imp.autoscroll.borrow().is_some() {
            self.set_cursor_name(Some("move"));
            return;
        }
        if imp.add_mode.get() == Some(AddMode::Text) {
            self.set_cursor_name(Some("crosshair"));
            return;
        }
        let Some((page, x, y)) = self.point_to_page(wx, wy, false) else {
            self.set_cursor_name(None);
            self.set_hover_link(None, wx, wy);
            return;
        };
        if let Some(link) = self.link_at(page, x, y) {
            self.set_cursor_name(Some("pointer"));
            self.set_hover_link(Some(link), wx, wy);
            return;
        }
        self.set_hover_link(None, wx, wy);
        if let Some(f) = self.form_at(page, x, y) {
            if !f.read_only {
                let name = if matches!(f.kind, FormFieldKind::Text { .. }) { "text" } else { "pointer" };
                self.set_cursor_name(Some(name));
                return;
            }
        }
        if self.annot_at(page, x, y).is_some() || self.media_at(page, x, y).is_some() {
            self.set_cursor_name(Some("pointer"));
            return;
        }
        if imp.caret_enabled.get() || self.over_text(page, x, y) {
            self.set_cursor_name(Some("text"));
            return;
        }
        self.set_cursor_name(None);
    }

    fn set_hover_link(&self, link: Option<Link>, wx: f64, wy: f64) {
        let imp = self.imp_();
        let same = *imp.hover_link.borrow() == link;
        if same {
            return;
        }
        if let Some((p, id)) = imp.link_preview.borrow_mut().take() {
            id.remove();
            destroy_widget(&p);
        }
        let doc = self.document();
        let tip = link.as_ref().zip(doc.as_ref()).and_then(|(l, d)| View::link_tooltip(l, d));
        self.set_tooltip_text(tip.as_deref());
        if let (Some(l), Some(doc)) = (&link, &doc) {
            if let Some(LinkAction::GotoDest(dest)) = &l.action {
                if let Some((page, dest)) = doc.resolve_dest(dest) {
                    self.schedule_link_preview(page, dest, wx, wy);
                }
            }
        }
        *imp.hover_link.borrow_mut() = link;
    }

    fn schedule_link_preview(&self, page: usize, dest: LinkDest, wx: f64, wy: f64) {
        let pop = gtk::Popover::new(Some(self));
        pop.set_modal(false);
        pop.set_pointing_to(&gdk::Rectangle::new(wx as i32, wy as i32, 1, 1));
        let weak = self.downgrade();
        let pop2 = pop.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(300), move || {
            let Some(v) = weak.upgrade() else { return };
            let Some(doc) = v.document() else { return };
            let Some(m) = v.model() else { return };
            let (pw, ph) = doc.page_size(page);
            let scale = m.scale();
            let (width, height) = ((pw * scale / 3.0).max(120.0), (ph * scale / 3.0).max(90.0));
            let (dx, dy) = match &dest {
                LinkDest::Xyz { left, top, .. } => (left.unwrap_or(0.0), top.unwrap_or(0.0)),
                LinkDest::FitH { top, .. } => (0.0, top.unwrap_or(0.0)),
                LinkDest::FitR { rect, .. } => (rect.x1, rect.y1),
                _ => (0.0, 0.0),
            };
            let area = gtk::DrawingArea::new();
            area.set_size_request(width as i32, height as i32);
            pop2.add(&area);
            area.show();
            let sf = v.scale_factor().max(1) as f64;
            let area_w = area.downgrade();
            doc.jobs.run(
                Priority::Urgent,
                move |d| d.render(page, scale * sf, 0).and_then(Bitmap::from_surface),
                move |bm| {
                    let (Some(bm), Some(area)) = (bm, area_w.upgrade()) else { return };
                    let Some(s) = bm.to_surface() else { return };
                    area.connect_draw(move |a, cr| {
                        let (w, h) = (a.allocated_width() as f64, a.allocated_height() as f64);
                        let ox = (dx * scale - w * 0.5).max(0.0);
                        let oy = (dy * scale - h * 0.3).max(0.0);
                        cr.scale(1.0 / sf, 1.0 / sf);
                        let _ = cr.set_source_surface(&s, (-ox * sf).round(), (-oy * sf).round());
                        let _ = cr.paint();
                        glib::Propagation::Proceed
                    });
                    area.queue_draw();
                },
            );
            pop2.popup();
        });
        *self.imp_().link_preview.borrow_mut() = Some((pop, id));
    }

    // ---------------------------------------------------------------- selection

    fn compute_selection(&self, start: (f64, f64), end: (f64, f64), style: SelectionStyle) {
        let imp = self.imp_();
        let Some(doc) = self.document() else { return };
        if !doc.has_text {
            return;
        }
        let (ox, oy) = self.scroll_offset();
        let l = imp.layout.borrow().clone();
        let (sx, sy) = (start.0, start.1);
        let (ex, ey) = (end.0 + ox, end.1 + oy);
        let Some(sb) = l.box_at(sx, sy).or_else(|| l.nearest_box(sx, sy)).copied() else { return };
        let Some(eb) = l.box_at(ex, ey).or_else(|| l.nearest_box(ex, ey)).copied() else { return };
        let (first, last, fp, lp) = if (sb.page, sy) <= (eb.page, ey) {
            (sb, eb, (sx, sy), (ex, ey))
        } else {
            (eb, sb, (ex, ey), (sx, sy))
        };
        let mut pages = Vec::new();
        for bx in l.boxes.iter().filter(|b| b.page >= first.page && b.page <= last.page) {
            let size = doc.page_size(bx.page);
            let (a, b) = if bx.page == first.page && bx.page == last.page {
                (l.canvas_to_page(bx, size, fp.0, fp.1), l.canvas_to_page(bx, size, lp.0, lp.1))
            } else if bx.page == first.page {
                (l.canvas_to_page(bx, size, fp.0, fp.1), (size.0, size.1))
            } else if bx.page == last.page {
                ((0.0, 0.0), l.canvas_to_page(bx, size, lp.0, lp.1))
            } else {
                ((0.0, 0.0), (size.0, size.1))
            };
            pages.push((bx.page, Rect::new(a.0, a.1, b.0, b.1)));
        }
        self.set_selection(pages, style);
    }

    fn set_selection(&self, pages: Vec<(usize, Rect)>, style: SelectionStyle) {
        let imp = self.imp_();
        let Some(doc) = self.document() else { return };
        let old: HashMap<usize, Vec<Rect>> =
            imp.selection.borrow().iter().map(|s| (s.page, s.region.clone())).collect();
        let sel: Vec<SelPage> = pages
            .iter()
            .map(|(p, r)| SelPage { page: *p, rect: *r, style, region: old.get(p).cloned().unwrap_or_default() })
            .collect();
        *imp.selection.borrow_mut() = sel;
        for (page, rect) in pages {
            let weak = self.downgrade();
            doc.jobs.run(
                Priority::Urgent,
                move |d| d.selection_region(page, style, rect),
                move |region| {
                    let Some(v) = weak.upgrade() else { return };
                    let mut sel = v.imp_().selection.borrow_mut();
                    if let Some(s) = sel.iter_mut().find(|s| s.page == page && s.rect == rect) {
                        s.region = region;
                    }
                    drop(sel);
                    v.queue_draw();
                },
            );
        }
        self.queue_draw();
    }

    pub fn has_selection(&self) -> bool {
        !self.imp_().selection.borrow().is_empty()
    }

    pub fn clear_selection(&self) {
        if self.has_selection() {
            self.imp_().selection.borrow_mut().clear();
            self.queue_draw();
            self.emit(ViewEvent::SelectionChanged);
        }
    }

    pub fn select_all(&self) {
        let Some(doc) = self.document() else { return };
        let pages = (0..doc.n_pages)
            .map(|p| {
                let (w, h) = doc.page_size(p);
                (p, Rect::new(0.0, 0.0, w, h))
            })
            .collect();
        self.set_selection(pages, SelectionStyle::Glyph);
        self.emit(ViewEvent::SelectionChanged);
    }

    pub fn selected_text(&self) -> String {
        let Some(doc) = self.document() else { return String::new() };
        let sel = self.imp_().selection.borrow().clone();
        doc.jobs.with(move |d| {
            sel.iter()
                .map(|s| d.selected_text(s.page, s.style, s.rect))
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    pub fn copy(&self) {
        if !self.has_selection() {
            return;
        }
        let perms = self.document().map(|d| d.info.permissions.copy).unwrap_or(true);
        if !perms {
            return;
        }
        let text = self.selected_text();
        if !text.is_empty() {
            gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_text(&text);
        }
    }

    fn update_primary(&self) {
        if !self.has_selection() {
            return;
        }
        let text = self.selected_text();
        if !text.is_empty() {
            gtk::Clipboard::get(&gdk::SELECTION_PRIMARY).set_text(&text);
        }
    }

    /// Selection rectangles per page in page coordinates (for highlight annotations).
    pub fn selection_pages(&self) -> Vec<(usize, Rect)> {
        self.imp_().selection.borrow().iter().map(|s| (s.page, s.rect)).collect()
    }

    // ---------------------------------------------------------------- find

    pub fn find_start(&self, text: &str, opts: FindOptions) {
        self.find_cancel();
        let Some(doc) = self.document() else { return };
        let Some(m) = self.model() else { return };
        if text.is_empty() || !doc.can_find {
            self.emit(ViewEvent::FindUpdated);
            self.queue_draw();
            return;
        }
        {
            let mut f = self.imp_().find.borrow_mut();
            f.text = text.to_string();
            f.opts = opts;
            f.start_page = m.page();
            f.pages_done = 0;
            f.current = None;
            f.results.clear();
            f.generation += 1;
        }
        self.find_next_page();
    }

    fn find_next_page(&self) {
        let Some(doc) = self.document() else { return };
        let (text, opts, page, generation) = {
            let f = self.imp_().find.borrow();
            if f.pages_done >= doc.n_pages {
                return;
            }
            (f.text.clone(), f.opts, (f.start_page + f.pages_done) % doc.n_pages, f.generation)
        };
        let weak = self.downgrade();
        let job = doc.jobs.run(
            Priority::High,
            move |d| d.find(page, &text, opts),
            move |matches| {
                let Some(v) = weak.upgrade() else { return };
                let first = {
                    let mut f = v.imp_().find.borrow_mut();
                    if f.generation != generation {
                        return;
                    }
                    f.pages_done += 1;
                    f.running = None;
                    let first = f.current.is_none() && !matches.is_empty();
                    if !matches.is_empty() {
                        f.results.insert(page, matches);
                    }
                    if first {
                        f.current = Some((page, 0));
                    }
                    first
                };
                if first {
                    v.scroll_to_current_result();
                }
                v.queue_draw();
                v.emit(ViewEvent::FindUpdated);
                v.find_next_page();
            },
        );
        self.imp_().find.borrow_mut().running = Some(job);
    }

    pub fn find_cancel(&self) {
        let mut f = self.imp_().find.borrow_mut();
        if let Some(j) = f.running.take() {
            j.cancel();
        }
        f.generation += 1;
        f.results.clear();
        f.current = None;
        f.text.clear();
        f.pages_done = 0;
        drop(f);
        self.queue_draw();
    }

    pub fn find_set_highlight(&self, on: bool) {
        self.imp_().find.borrow_mut().highlight = on;
        self.queue_draw();
    }

    pub fn find_progress(&self) -> (usize, usize) {
        let n = self.document().map_or(0, |d| d.n_pages);
        (self.imp_().find.borrow().pages_done, n)
    }

    pub fn find_text(&self) -> String {
        self.imp_().find.borrow().text.clone()
    }

    pub fn find_results(&self) -> Vec<(usize, Vec<FindMatch>)> {
        let f = self.imp_().find.borrow();
        let mut v: Vec<_> = f.results.iter().map(|(p, m)| (*p, m.clone())).collect();
        v.sort_by_key(|(p, _)| *p);
        v
    }

    pub fn find_n_results(&self) -> usize {
        self.imp_().find.borrow().results.values().map(|v| v.len()).sum()
    }

    pub fn find_current(&self) -> Option<(usize, usize)> {
        self.imp_().find.borrow().current
    }

    pub fn find_select(&self, page: usize, idx: usize) {
        self.imp_().find.borrow_mut().current = Some((page, idx));
        self.scroll_to_current_result();
        self.emit(ViewEvent::FindUpdated);
        self.queue_draw();
    }

    fn scroll_to_current_result(&self) {
        let cur = self.imp_().find.borrow().current;
        let Some((page, idx)) = cur else { return };
        let rect = self.imp_().find.borrow().results.get(&page).and_then(|m| m.get(idx)).and_then(|m| m.rects.first().copied());
        let Some(rect) = rect else { return };
        let Some(m) = self.model() else { return };
        if m.page() != page || !m.continuous() {
            let imp = self.imp_();
            if m.continuous() {
                imp.internal_page_change.set(true);
                m.set_page(page);
                imp.internal_page_change.set(false);
            } else {
                m.set_page(page);
            }
        }
        self.relayout();
        self.scroll_to_rect(page, rect);
    }

    fn find_step(&self, forward: bool) {
        let (cur, ordered) = {
            let f = self.imp_().find.borrow();
            let mut ordered: Vec<(usize, usize)> = Vec::new();
            let mut pages: Vec<_> = f.results.keys().copied().collect();
            pages.sort_unstable();
            for p in pages {
                for i in 0..f.results[&p].len() {
                    ordered.push((p, i));
                }
            }
            (f.current, ordered)
        };
        if ordered.is_empty() {
            return;
        }
        let page_now = self.model().map_or(0, |m| m.page());
        let pos = cur.and_then(|c| ordered.iter().position(|x| *x == c));
        let next = match pos {
            Some(i) if forward => ordered[(i + 1) % ordered.len()],
            Some(i) => ordered[(i + ordered.len() - 1) % ordered.len()],
            None => *ordered.iter().find(|(p, _)| *p >= page_now).unwrap_or(&ordered[0]),
        };
        self.find_select(next.0, next.1);
    }

    pub fn find_next(&self) {
        self.find_step(true);
    }

    pub fn find_previous(&self) {
        self.find_step(false);
    }

    /// Text around a find match, for the find sidebar.
    pub fn find_context(&self, page: usize, m: &FindMatch) -> Option<String> {
        let cached = self.imp_().page_data.borrow().get(&page).and_then(|d| d.text.clone());
        let text = match cached {
            Some(t) => t,
            None => {
                let doc = self.document()?;
                let mut cache = self.imp_().find_text_cache.borrow_mut();
                if let Some(t) = cache.get(&page) {
                    t.clone()
                } else {
                    let t = doc.jobs.with(move |d| d.text_layout(page))?;
                    cache.insert(page, t.clone());
                    t
                }
            }
        };
        let text = &text;
        let r = m.rects.first()?;
        let idx: Vec<usize> = text
            .rects
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let (cx, cy) = c.center();
                r.contains(cx, cy)
            })
            .map(|(i, _)| i)
            .collect();
        let (a, b) = (*idx.first()?, *idx.last()? + 1);
        let start = a.saturating_sub(30);
        let end = (b + 40).min(text.chars.len());
        let pre: String = text.chars[start..a].iter().collect::<String>().replace('\n', " ");
        let mid: String = text.chars[a..b].iter().collect::<String>().replace('\n', " ");
        let post: String = text.chars[b..end].iter().collect::<String>().replace('\n', " ");
        let pre = pre.rsplit_once(' ').map(|(_, t)| t.to_string()).filter(|_| start > 0).unwrap_or(pre);
        Some(format!(
            "{}<b>{}</b>{}",
            glib::markup_escape_text(&pre),
            glib::markup_escape_text(&mid),
            glib::markup_escape_text(&post)
        ))
    }

    // ---------------------------------------------------------------- caret

    pub fn set_caret_navigation(&self, on: bool) {
        let imp = self.imp_();
        imp.caret_enabled.set(on);
        if on {
            if imp.caret.get().is_none() {
                if let Some(m) = self.model() {
                    imp.caret.set(Some((m.page(), 0)));
                }
            }
            let weak = self.downgrade();
            let id = glib::timeout_add_local(Duration::from_millis(600), move || {
                let Some(v) = weak.upgrade() else { return glib::ControlFlow::Break };
                let i = v.imp_();
                i.caret_visible.set(!i.caret_visible.get());
                v.queue_draw();
                glib::ControlFlow::Continue
            });
            if let Some(old) = imp.caret_blink.borrow_mut().replace(id) {
                old.remove();
            }
            self.grab_focus();
        } else if let Some(old) = imp.caret_blink.borrow_mut().take() {
            old.remove();
        }
        self.queue_draw();
    }

    pub fn caret_position(&self) -> Option<(usize, usize)> {
        self.imp_().caret.get()
    }

    pub fn set_caret_position(&self, page: usize, offset: usize) {
        self.imp_().caret.set(Some((page, offset)));
        self.queue_draw();
    }

    pub fn caret_navigation(&self) -> bool {
        self.imp_().caret_enabled.get()
    }

    fn move_caret(&self, kind: CaretMove, extend: bool) -> bool {
        let imp = self.imp_();
        let Some(doc) = self.document() else { return false };
        let Some((page, off)) = imp.caret.get() else { return false };
        let Some(data) = self.page_data(page) else {
            self.request_page_data(&doc, page);
            return true;
        };
        let Some(text) = data.text else { return false };
        let n = text.chars.len();
        let word = |c: char| c.is_alphanumeric();
        let line_start = |o: usize| {
            let mut i = o.min(n);
            while i > 0 && text.chars[i - 1] != '\n' {
                i -= 1;
            }
            i
        };
        let line_end = |o: usize| {
            let mut i = o.min(n);
            while i < n && text.chars[i] != '\n' {
                i += 1;
            }
            i
        };
        let mut target = (page, off.min(n));
        match kind {
            CaretMove::Left => {
                if off > 0 {
                    target.1 = off - 1;
                } else if page > 0 {
                    target = (page - 1, usize::MAX);
                }
            }
            CaretMove::Right => {
                if off < n {
                    target.1 = off + 1;
                } else if page + 1 < doc.n_pages {
                    target = (page + 1, 0);
                }
            }
            CaretMove::WordLeft => {
                let mut i = off.min(n);
                while i > 0 && !word(text.chars[i - 1]) {
                    i -= 1;
                }
                while i > 0 && word(text.chars[i - 1]) {
                    i -= 1;
                }
                target.1 = i;
            }
            CaretMove::WordRight => {
                let mut i = off.min(n);
                while i < n && !word(text.chars[i]) {
                    i += 1;
                }
                while i < n && word(text.chars[i]) {
                    i += 1;
                }
                target.1 = i;
            }
            CaretMove::LineStart => target.1 = line_start(off),
            CaretMove::LineEnd => target.1 = line_end(off),
            CaretMove::Up | CaretMove::Down => {
                let x = text.rects.get(off.min(n.saturating_sub(1))).map_or(0.0, |r| r.x1);
                let ls = line_start(off);
                let le = line_end(off);
                let (s, e) = if kind == CaretMove::Up {
                    if ls == 0 {
                        if page > 0 {
                            target = (page - 1, usize::MAX);
                        }
                        (usize::MAX, 0)
                    } else {
                        let pe = ls - 1;
                        (line_start(pe), pe)
                    }
                } else if le >= n {
                    if page + 1 < doc.n_pages {
                        target = (page + 1, 0);
                    }
                    (usize::MAX, 0)
                } else {
                    (le + 1, line_end(le + 1))
                };
                if s != usize::MAX {
                    let mut best = s;
                    let mut bd = f64::MAX;
                    for i in s..e {
                        let d = (text.rects[i].x1 - x).abs();
                        if d < bd {
                            bd = d;
                            best = i;
                        }
                    }
                    target.1 = best;
                }
            }
            CaretMove::DocStart => target = (0, 0),
            CaretMove::DocEnd => target = (doc.n_pages - 1, usize::MAX),
        }
        if target.1 == usize::MAX {
            match self.page_data(target.0).and_then(|d| d.text) {
                Some(t) => target.1 = t.chars.len(),
                None => {
                    self.request_page_data(&doc, target.0);
                    target.1 = 0;
                }
            }
        }
        if extend {
            if imp.caret_anchor.get().is_none() {
                imp.caret_anchor.set(Some((page, off)));
            }
        } else {
            imp.caret_anchor.set(None);
            if self.has_selection() {
                self.clear_selection();
            }
        }
        imp.caret.set(Some(target));
        imp.caret_visible.set(true);
        if let Some(r) = self.caret_rect(target.0, target.1) {
            if let Some(m) = self.model() {
                if m.page() != target.0 {
                    imp.internal_page_change.set(m.continuous());
                    m.set_page(target.0);
                    imp.internal_page_change.set(false);
                }
            }
            self.scroll_to_rect(target.0, r);
        } else {
            self.request_page_data(&doc, target.0);
        }
        if let Some(anchor) = imp.caret_anchor.get() {
            self.select_between_offsets(anchor, target);
        }
        self.queue_draw();
        true
    }

    fn offset_point(&self, page: usize, off: usize) -> Option<(f64, f64)> {
        let r = self.caret_rect(page, off)?;
        Some((r.x1, (r.y1 + r.y2) / 2.0))
    }

    fn select_between_offsets(&self, a: (usize, usize), b: (usize, usize)) {
        let (first, last) = if a <= b { (a, b) } else { (b, a) };
        let Some(doc) = self.document() else { return };
        let mut pages = Vec::new();
        for p in first.0..=last.0 {
            let (w, h) = doc.page_size(p);
            let start = if p == first.0 { self.offset_point(p, first.1).unwrap_or((0.0, 0.0)) } else { (0.0, 0.0) };
            let end = if p == last.0 { self.offset_point(p, last.1).unwrap_or((w, h)) } else { (w, h) };
            pages.push((p, Rect::new(start.0, start.1, end.0, end.1)));
        }
        self.set_selection(pages, SelectionStyle::Glyph);
        self.emit(ViewEvent::SelectionChanged);
    }

    fn place_caret_at(&self, page: usize, x: f64, y: f64) {
        let imp = self.imp_();
        if !imp.caret_enabled.get() {
            return;
        }
        let off = self
            .page_data(page)
            .and_then(|d| d.text)
            .and_then(|t| misc::offset_at_point(&t, x, y))
            .unwrap_or(0);
        imp.caret.set(Some((page, off)));
        imp.caret_anchor.set(None);
        imp.caret_visible.set(true);
        self.queue_draw();
    }

    // ---------------------------------------------------------------- annotations

    pub fn set_add_mode(&self, mode: Option<AddMode>) {
        self.imp_().add_mode.set(mode);
        let (x, y) = self.imp_().pointer.get();
        self.update_cursor(x, y);
    }

    pub fn add_mode(&self) -> Option<AddMode> {
        self.imp_().add_mode.get()
    }

    fn author_name() -> String {
        let n = glib::real_name().to_string_lossy().to_string();
        if n.is_empty() || n == "Unknown" { glib::user_name().to_string_lossy().to_string() } else { n }
    }

    pub fn add_text_annotation_at(&self, page: usize, x: f64, y: f64) {
        let Some(doc) = self.document() else { return };
        if !doc.can_annotate {
            return;
        }
        let (pw, ph) = doc.page_size(page);
        let size = 24.0;
        let x1 = (x - size / 2.0).clamp(0.0, (pw - size).max(0.0));
        let y1 = (y - size / 2.0).clamp(0.0, (ph - size).max(0.0));
        let rect = Rect::new(x1, y1, x1 + size, y1 + size);
        let author = View::author_name();
        let created = doc.jobs.with(move |d| {
            d.add_annotation(page, &NewAnnot::Text { icon: TextIcon::Note }, rect, Color::rgb(1.0, 1.0, 0.0), &author)
        });
        if let Some(a) = created {
            doc.modified.set(true);
            self.invalidate_page(page);
            self.emit(ViewEvent::AnnotAdded(a.clone()));
            self.open_annotation_popup(&a);
        }
    }

    /// Creates highlight annotations from the current text selection.
    pub fn add_highlight_from_selection(&self) -> bool {
        let Some(doc) = self.document() else { return false };
        if !doc.can_annotate {
            return false;
        }
        let sel = self.selection_pages();
        if sel.is_empty() {
            return false;
        }
        let author = View::author_name();
        let created: Vec<Annotation> = doc.jobs.with(move |d| {
            sel.iter()
                .filter_map(|(p, r)| {
                    d.add_annotation(*p, &NewAnnot::TextMarkup(MarkupType::Highlight), r.normalized(), Color::rgb(1.0, 1.0, 0.0), &author)
                })
                .collect()
        });
        if created.is_empty() {
            return false;
        }
        doc.modified.set(true);
        self.clear_selection();
        for a in created {
            self.invalidate_page(a.page);
            self.emit(ViewEvent::AnnotAdded(a));
        }
        true
    }

    pub fn update_annotation(&self, annot: &Annotation) {
        let Some(doc) = self.document() else { return };
        let a = annot.clone();
        doc.jobs.with(move |d| d.update_annotation(&a));
        doc.modified.set(true);
        self.invalidate_page(annot.page);
        self.emit(ViewEvent::AnnotsChanged(Some(annot.page)));
    }

    pub fn remove_annotation(&self, annot: &Annotation) {
        let Some(doc) = self.document() else { return };
        let (p, id) = (annot.page, annot.id);
        doc.jobs.with(move |d| d.remove_annotation(p, id));
        doc.modified.set(true);
        self.invalidate_page(annot.page);
        self.emit(ViewEvent::AnnotsChanged(Some(annot.page)));
    }

    pub fn open_annotation_popup(&self, annot: &Annotation) {
        let imp = self.imp_();
        if let Some(old) = imp.annot_popup.borrow_mut().take() {
            destroy_widget(&old);
        }
        let Some(r) = self.page_rect_to_widget(annot.page, annot.area) else { return };
        let pop = gtk::Popover::new(Some(self));
        pop.set_pointing_to(&gdk::Rectangle::new(r.x1 as i32, r.y1 as i32, r.width().max(1.0) as i32, r.height().max(1.0) as i32));
        pop.set_position(gtk::PositionType::Right);
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
        vbox.set_border_width(6);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let swatch = gtk::DrawingArea::new();
        swatch.set_size_request(14, 14);
        swatch.set_valign(gtk::Align::Center);
        let c = annot.color;
        swatch.connect_draw(move |a, cr| {
            cr.set_source_rgb(c.r, c.g, c.b);
            cr.rectangle(0.0, 0.0, a.allocated_width() as f64, a.allocated_height() as f64);
            let _ = cr.fill();
            glib::Propagation::Proceed
        });
        header.pack_start(&swatch, false, false, 0);
        let title = gtk::Label::new(None);
        title.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(annot.label.as_deref().unwrap_or(annot.kind_name()))));
        title.set_hexpand(true);
        title.set_xalign(0.0);
        header.pack_start(&title, true, true, 0);
        vbox.pack_start(&header, false, false, 0);
        let tv = gtk::TextView::new();
        tv.set_wrap_mode(gtk::WrapMode::WordChar);
        if let Some(b) = tv.buffer() {
            b.set_text(&annot.contents);
        }
        let sw = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        sw.set_shadow_type(gtk::ShadowType::In);
        sw.add(&tv);
        sw.set_size_request(ANNOT_POPUP_W, ANNOT_POPUP_H);
        vbox.pack_start(&sw, true, true, 0);
        pop.add(&vbox);
        vbox.show_all();
        let weak = self.downgrade();
        let a = RefCell::new(annot.clone());
        let tvc = tv.clone();
        pop.connect_closed(move |p| {
            let Some(v) = weak.upgrade() else { return };
            let text = text_view_text(&tvc);
            let mut an = a.borrow_mut();
            if text != an.contents {
                an.contents = text;
                v.update_annotation(&an);
            }
            let p = p.clone();
            glib::idle_add_local_once(move || destroy_widget(&p));
            v.imp_().annot_popup.borrow_mut().take();
        });
        *imp.annot_popup.borrow_mut() = Some(pop.clone());
        pop.popup();
        tv.grab_focus();
    }

    // ---------------------------------------------------------------- forms

    fn activate_form(&self, field: &FormField) {
        let Some(doc) = self.document() else { return };
        if field.read_only {
            return;
        }
        let (page, id) = (field.page, field.id);
        match &field.kind {
            FormFieldKind::Button { kind: ButtonKind::Push, .. } => {
                if let Some(link) = &field.activation {
                    self.handle_link(link);
                }
            }
            FormFieldKind::Button { kind, state } => {
                let new = if *kind == ButtonKind::Radio { true } else { !state };
                doc.jobs.with(move |d| d.set_form_button(page, id, new));
                doc.modified.set(true);
                self.invalidate_page(page);
                self.emit(ViewEvent::FormChanged);
            }
            FormFieldKind::Text { text, multiline, password, max_len, .. } => {
                let (widget, getter): (gtk::Widget, Rc<dyn Fn() -> String>) = if *multiline {
                    let tv = gtk::TextView::new();
                    if let Some(b) = tv.buffer() {
                        b.set_text(text);
                    }
                    tv.set_wrap_mode(gtk::WrapMode::WordChar);
                    let tvc = tv.clone();
                    (tv.upcast(), Rc::new(move || text_view_text(&tvc)))
                } else {
                    let e = gtk::Entry::new();
                    e.set_text(text);
                    e.set_visibility(!password);
                    e.set_has_frame(false);
                    if *max_len > 0 {
                        e.set_max_length(*max_len);
                    }
                    let ec = e.clone();
                    (e.upcast(), Rc::new(move || ec.text().to_string()))
                };
                self.show_form_editor(field.clone(), widget, getter);
            }
            FormFieldKind::Choice { combo, editable, multi, items, selected, text } => {
                let pop = gtk::Popover::new(Some(self));
                let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
                vbox.set_border_width(4);
                let entry = editable.then(|| {
                    let e = gtk::Entry::new();
                    e.set_text(text);
                    vbox.pack_start(&e, false, false, 0);
                    e
                });
                let list = gtk::ListBox::new();
                list.set_selection_mode(if *multi && !*combo { gtk::SelectionMode::Multiple } else { gtk::SelectionMode::Single });
                for it in items {
                    let l = gtk::Label::new(Some(it));
                    l.set_xalign(0.0);
                    l.set_margin_start(6);
                    l.set_margin_end(6);
                    list.add(&l);
                }
                for &s in selected {
                    if let Some(row) = list.row_at_index(s as i32) {
                        list.select_row(Some(&row));
                    }
                }
                let sw = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
                sw.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
                sw.set_propagate_natural_height(true);
                sw.set_max_content_height(300);
                sw.add(&list);
                vbox.pack_start(&sw, true, true, 0);
                pop.add(&vbox);
                vbox.show_all();
                if let Some(r) = self.page_rect_to_widget(page, field.area) {
                    pop.set_pointing_to(&gdk::Rectangle::new(r.x1 as i32, r.y1 as i32, r.width() as i32, r.height() as i32));
                }
                let weak = self.downgrade();
                let committed = Rc::new(Cell::new(false));
                let commit = {
                    let list = list.clone();
                    let entry = entry.clone();
                    let committed = committed.clone();
                    let orig_text = text.clone();
                    move |v: &View| {
                        if committed.replace(true) {
                            return;
                        }
                        let Some(doc) = v.document() else { return };
                        let sel: Vec<usize> = list.selected_rows().iter().map(|r| r.index() as usize).collect();
                        let txt = entry.as_ref().map(|e| e.text().to_string()).filter(|t| *t != orig_text && sel.is_empty());
                        doc.jobs.with(move |d| d.set_form_choice(page, id, &sel, txt.as_deref()));
                        doc.modified.set(true);
                        v.invalidate_page(page);
                        v.emit(ViewEvent::FormChanged);
                    }
                };
                let commit = Rc::new(commit);
                if !*multi || *combo {
                    let c = commit.clone();
                    let w = weak.clone();
                    let p = pop.clone();
                    list.connect_row_activated(move |_, _| {
                        if let Some(v) = w.upgrade() {
                            c(&v);
                        }
                        p.popdown();
                    });
                }
                if let Some(e) = &entry {
                    let c = commit.clone();
                    let w = weak.clone();
                    let p = pop.clone();
                    e.connect_activate(move |_| {
                        if let Some(v) = w.upgrade() {
                            c(&v);
                        }
                        p.popdown();
                    });
                }
                let c = commit.clone();
                pop.connect_closed(move |p| {
                    if let Some(v) = weak.upgrade() {
                        c(&v);
                    }
                    let p = p.clone();
                    glib::idle_add_local_once(move || destroy_widget(&p));
                });
                pop.popup();
            }
            FormFieldKind::Signature | FormFieldKind::Unknown => {}
        }
    }

    fn show_form_editor(&self, field: FormField, widget: gtk::Widget, get: Rc<dyn Fn() -> String>) {
        self.close_children();
        widget.style_context().add_class("ev-form-field");
        self.add_child(&widget, field.page, field.area);
        widget.grab_focus();
        let weak = self.downgrade();
        let done = Rc::new(Cell::new(false));
        let commit = {
            let widget = widget.clone();
            let done = done.clone();
            let field = field.clone();
            Rc::new(move |save: bool| {
                if done.replace(true) {
                    return;
                }
                let Some(v) = weak.upgrade() else { return };
                if save {
                    let text = get();
                    let orig = match &field.kind {
                        FormFieldKind::Text { text, .. } => text.clone(),
                        _ => String::new(),
                    };
                    if text != orig {
                        if let Some(doc) = v.document() {
                            let (p, id) = (field.page, field.id);
                            doc.jobs.with(move |d| d.set_form_text(p, id, &text));
                            doc.modified.set(true);
                            v.invalidate_page(field.page);
                            v.emit(ViewEvent::FormChanged);
                        }
                    }
                }
                let w = widget.clone();
                glib::idle_add_local_once(move || {
                    if let Some(v) = w.parent().and_downcast::<View>() {
                        v.remove_child(&w);
                        v.grab_focus();
                    }
                });
            })
        };
        if let Some(e) = widget.downcast_ref::<gtk::Entry>() {
            let c = commit.clone();
            e.connect_activate(move |_| c(true));
        }
        let c = commit.clone();
        widget.connect_focus_out_event(move |_, _| {
            c(true);
            glib::Propagation::Proceed
        });
        let c = commit.clone();
        widget.connect_key_press_event(move |_, e| {
            if e.keyval() == key::Escape {
                c(false);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }

    // ---------------------------------------------------------------- media

    fn play_media(&self, _area: Option<(usize, Rect)>, media: Media) {
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&media.uri, gio::AppLaunchContext::NONE) {
            eprintln!("Failed to play {}: {e}", media.uri);
        }
    }

    // ---------------------------------------------------------------- autoscroll

    pub fn autoscroll_start(&self) {
        let imp = self.imp_();
        if imp.autoscroll.borrow().is_some() {
            return;
        }
        let (_, py) = imp.pointer.get();
        let weak = self.downgrade();
        let id = glib::timeout_add_local(Duration::from_millis(20), move || {
            let Some(v) = weak.upgrade() else { return glib::ControlFlow::Break };
            let (start, _) = match v.imp_().autoscroll.borrow().as_ref() {
                Some(a) => (a.0, a.1),
                None => return glib::ControlFlow::Break,
            };
            let (_, y) = v.imp_().pointer.get();
            let speed = (y - start) / 8.0;
            if let Some(a) = v.vadj() {
                a.set_value((a.value() + speed).clamp(a.lower(), (a.upper() - a.page_size()).max(0.0)));
            }
            glib::ControlFlow::Continue
        });
        *imp.autoscroll.borrow_mut() = Some((py, 0.0, id));
        self.set_cursor_name(Some("move"));
    }

    pub fn autoscroll_stop(&self) {
        if let Some((_, _, id)) = self.imp_().autoscroll.borrow_mut().take() {
            id.remove();
        }
        self.set_cursor_name(None);
    }

    // ---------------------------------------------------------------- synctex

    pub fn highlight_forward_search(&self, page: usize, rect: Rect) {
        *self.imp_().synctex_hl.borrow_mut() = Some((page, rect, Instant::now()));
        if let Some(m) = self.model() {
            if m.page() != page {
                let imp = self.imp_();
                imp.internal_page_change.set(m.continuous());
                m.set_page(page);
                imp.internal_page_change.set(false);
            }
        }
        self.relayout();
        self.scroll_to_rect(page, rect);
        let weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_secs(6), move || {
            if let Some(v) = weak.upgrade() {
                v.queue_draw();
            }
        });
        self.queue_draw();
    }

    pub fn popup_context(&self) -> PopupContext {
        self.imp_().popup_ctx.borrow().clone()
    }

    // ---------------------------------------------------------------- input

    fn setup_controllers(&self) {
        let weak = self.downgrade();
        self.connect_key_press_event(move |_, e| match weak.upgrade() {
            Some(v) => v.on_key(e.keyval(), e.state()),
            None => glib::Propagation::Proceed,
        });
        self.connect_focus_in_event(|v, _| {
            v.queue_draw();
            glib::Propagation::Proceed
        });
        self.connect_focus_out_event(|v, _| {
            v.queue_draw();
            glib::Propagation::Proceed
        });

        let zoom = gtk::GestureZoom::new(self);
        zoom.set_propagation_phase(gtk::PropagationPhase::Bubble);
        let weak = self.downgrade();
        zoom.connect_begin(move |_, _| {
            let Some(v) = weak.upgrade() else { return };
            if let Some(m) = v.model() {
                v.imp_().zoom_gesture_scale.set(m.scale());
            }
        });
        let weak = self.downgrade();
        zoom.connect_scale_changed(move |g, s| {
            let Some(v) = weak.upgrade() else { return };
            let Some(m) = v.model() else { return };
            let base = v.imp_().zoom_gesture_scale.get();
            if let Some((x, y)) = g.bounding_box_center() {
                if let Some((page, px, py)) = v.point_to_page(x, y, true) {
                    v.imp_().zoom_anchor.set(Some((page, px, py, x, y)));
                }
            }
            m.set_sizing_mode(SizingMode::Free);
            m.set_scale(base * s);
            v.imp_().zoom_anchor.set(None);
        });
        *self.imp_().zoom_gesture.borrow_mut() = Some(zoom);
    }

    fn on_button_press(&self, e: &gdk::EventButton) -> glib::Propagation {
        self.kinetic_stop();
        let (x, y) = e.position();
        let n = match e.event_type() {
            gdk::EventType::DoubleButtonPress => 2,
            gdk::EventType::TripleButtonPress => 3,
            _ => 1,
        };
        let button = e.button();
        if n == 1 {
            self.imp_().drag_start.set(Some((button, x, y)));
        }
        if button == 2 && n == 1 {
            self.imp_().pan_origin.set(self.scroll_offset());
            self.set_cursor_name(Some("grabbing"));
            return glib::Propagation::Stop;
        }
        self.on_pressed(button, e.state(), n, x, y);
        glib::Propagation::Stop
    }

    fn on_button_release(&self, e: &gdk::EventButton) -> glib::Propagation {
        let (x, y) = e.position();
        let button = e.button();
        let started = self.imp_().drag_start.take();
        if button == 2 {
            self.set_cursor_name(None);
            return glib::Propagation::Stop;
        }
        self.on_released(button, 1, x, y);
        if button == 1 && started.is_some() {
            self.on_drag_end();
        }
        glib::Propagation::Stop
    }

    fn on_motion(&self, e: &gdk::EventMotion) -> glib::Propagation {
        let (x, y) = e.position();
        let imp = self.imp_();
        imp.pointer.set((x, y));
        if let Some((button, sx, sy)) = imp.drag_start.get() {
            let state = e.state();
            if button == 1 && state.contains(gdk::ModifierType::BUTTON1_MASK) {
                self.on_drag_update((sx, sy), x - sx, y - sy);
            } else if button == 2 && state.contains(gdk::ModifierType::BUTTON2_MASK) {
                let (ox, oy) = imp.pan_origin.get();
                let (dx, dy) = (x - sx, y - sy);
                if let Some(h) = self.hadj() {
                    h.set_value((ox - dx).clamp(0.0, (h.upper() - h.page_size()).max(0.0)));
                }
                if let Some(a) = self.vadj() {
                    a.set_value((oy - dy).clamp(0.0, (a.upper() - a.page_size()).max(0.0)));
                }
                return glib::Propagation::Stop;
            }
        }
        if !imp.selecting.get() {
            self.update_cursor(x, y);
        }
        glib::Propagation::Proceed
    }

    fn on_pressed(&self, button: u32, mods: gdk::ModifierType, n: i32, x: f64, y: f64) {
        let imp = self.imp_();
        if !self.has_focus() {
            self.grab_focus();
        }
        imp.press.set(Press::None);
        imp.press_point.set((x, y));
        if imp.autoscroll.borrow().is_some() {
            self.autoscroll_stop();
            return;
        }
        let hit = self.point_to_page(x, y, false);
        if button == 3 {
            let mut ctx = PopupContext::default();
            if let Some((page, px, py)) = hit {
                ctx.page = Some(page);
                ctx.link = self.link_at(page, px, py);
                ctx.image = self.image_at(page, px, py).map(|id| (page, id));
                ctx.annot = self.annot_at(page, px, py);
                if let Some(AnnotKind::Attachment(Some(a))) = ctx.annot.as_ref().map(|a| &a.kind) {
                    ctx.attachment = Some(a.clone());
                }
            }
            *imp.popup_ctx.borrow_mut() = ctx;
            self.emit(ViewEvent::Popup { x, y });
            return;
        }
        if button != 1 {
            return;
        }
        let Some((page, px, py)) = hit else {
            self.clear_selection();
            return;
        };
        if mods.contains(gdk::ModifierType::CONTROL_MASK) {
            if let Some(doc) = self.document() {
                if doc.supports_synctex {
                    self.emit(ViewEvent::SyncSource { page, x: px, y: py });
                            return;
                }
            }
        }
        if imp.add_mode.get() == Some(AddMode::Text) {
            self.add_text_annotation_at(page, px, py);
            imp.add_mode.set(None);
            self.emit(ViewEvent::AddModeFinished);
            return;
        }
        if n == 2 || n == 3 {
            let style = if n == 2 { SelectionStyle::Word } else { SelectionStyle::Line };
            self.set_selection(vec![(page, Rect::new(px, py, px, py))], style);
            imp.sel_style.set(style);
            let (ox, oy) = self.scroll_offset();
            imp.sel_start.set(Some((x + ox, y + oy)));
            self.emit(ViewEvent::SelectionChanged);
            let weak = self.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(v) = weak.upgrade() {
                    v.update_primary();
                }
            });
            return;
        }
        if imp.add_mode.get().is_none() {
            if self.link_at(page, px, py).is_some() {
                imp.press.set(Press::Link);
            } else if self.form_at(page, px, py).is_some_and(|f| !f.read_only) {
                imp.press.set(Press::Form);
            } else if self.media_at(page, px, py).is_some() {
                imp.press.set(Press::Media);
            } else if self.annot_at(page, px, py).is_some_and(|a| a.is_markup && !matches!(a.kind, AnnotKind::TextMarkup(_))) {
                imp.press.set(Press::Annot);
            }
        }
        self.place_caret_at(page, px, py);
        let (ox, oy) = self.scroll_offset();
        imp.sel_start.set(Some((x + ox, y + oy)));
        imp.sel_style.set(SelectionStyle::Glyph);
    }

    fn on_released(&self, button: u32, n: i32, x: f64, y: f64) {
        let imp = self.imp_();
        if button != 1 || n != 1 {
            return;
        }
        if imp.selecting.get() {
            return;
        }
        let (sx, sy) = imp.press_point.get();
        if (sx - x).abs() > 4.0 || (sy - y).abs() > 4.0 {
            return;
        }
        let press = imp.press.replace(Press::None);
        let Some((page, px, py)) = self.point_to_page(x, y, false) else { return };
        match press {
            Press::Link => {
                if let Some(link) = self.link_at(page, px, py) {
                    self.set_hover_link(None, x, y);
                    self.handle_link(&link);
                }
            }
            Press::Form => {
                if let Some(f) = self.form_at(page, px, py) {
                    self.activate_form(&f);
                }
            }
            Press::Media => {
                if let Some((rect, media)) = self.media_at(page, px, py) {
                    self.play_media(Some((page, rect)), media);
                }
            }
            Press::Annot => {
                if let Some(a) = self.annot_at(page, px, py) {
                    self.open_annotation_popup(&a);
                }
            }
            Press::None => {
                if !imp.caret_enabled.get() || imp.caret_anchor.get().is_none() {
                    self.clear_selection();
                }
            }
        }
    }

    fn on_drag_update(&self, origin: (f64, f64), dx: f64, dy: f64) {
        let imp = self.imp_();
        if imp.press.get() != Press::None || imp.add_mode.get() == Some(AddMode::Text) {
            return;
        }
        let Some(start) = imp.sel_start.get() else { return };
        if !imp.selecting.get() && dx.abs() < 3.0 && dy.abs() < 3.0 {
            return;
        }
        let (bx, by) = origin;
        let (x, y) = (bx + dx, by + dy);
        if !imp.selecting.get() {
            imp.selecting.set(true);
        }
        self.compute_selection(start, (x, y), imp.sel_style.get());
        let (vw, vh) = self.viewport();
        if y < 0.0 || y > vh || x < 0.0 || x > vw {
            if let Some(v) = self.vadj() {
                let d = if y < 0.0 { y } else if y > vh { y - vh } else { 0.0 };
                v.set_value((v.value() + d / 4.0).clamp(0.0, (v.upper() - v.page_size()).max(0.0)));
            }
        }
    }

    fn on_drag_end(&self) {
        let imp = self.imp_();
        let was = imp.selecting.replace(false);
        imp.sel_start.set(None);
        if !was {
            return;
        }
        if imp.add_mode.get() == Some(AddMode::Highlight) {
            if self.add_highlight_from_selection() {
                imp.add_mode.set(None);
                self.emit(ViewEvent::AddModeFinished);
            }
            return;
        }
        self.emit(ViewEvent::SelectionChanged);
        let weak = self.downgrade();
        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            if let Some(v) = weak.upgrade() {
                v.update_primary();
            }
        });
    }

    fn on_scroll(&self, e: &gdk::EventScroll) -> glib::Propagation {
        let mods = e.state();
        let (dx, dy) = match e.direction() {
            gdk::ScrollDirection::Up => (0.0, -1.0),
            gdk::ScrollDirection::Down => (0.0, 1.0),
            gdk::ScrollDirection::Left => (-1.0, 0.0),
            gdk::ScrollDirection::Right => (1.0, 0.0),
            _ => e.delta(),
        };
        if mods.contains(gdk::ModifierType::CONTROL_MASK) {
            if dy == 0.0 {
                return glib::Propagation::Stop;
            }
            let smooth_pixels = is_touchpad(e);
            let factor = if smooth_pixels { ZOOM_IN_FACTOR.powf(-dy / 2.0) } else { ZOOM_IN_FACTOR.powf(-dy) };
            self.zoom(factor, Some(e.position()));
            return glib::Propagation::Stop;
        }
        let Some(m) = self.model() else { return glib::Propagation::Proceed };
        let touchpad = is_touchpad(e);
        if touchpad {
            self.kinetic_stop();
            if e.is_stop() {
                self.imp_().touch_flip.set((0.0, false));
                self.kinetic_start();
                return glib::Propagation::Stop;
            }
        }
        if !m.continuous() && dx.abs() < dy.abs() {
            if let Some(v) = self.vadj() {
                let at_end = v.value() + v.page_size() >= v.upper() - 1.0;
                let at_start = v.value() <= v.lower() + 1.0;
                if touchpad && (dy > 0.0 && at_end || dy < 0.0 && at_start) {
                    let (acc, flipped) = self.imp_().touch_flip.get();
                    let acc = if acc * dy < 0.0 { 0.0 } else { acc + dy * TOUCHPAD_PIXELS };
                    self.imp_().touch_flip.set((acc, flipped));
                    if flipped || acc.abs() < TOUCHPAD_FLIP_DISTANCE {
                        return glib::Propagation::Stop;
                    }
                    self.imp_().touch_flip.set((0.0, true));
                    self.imp_().touch_samples.borrow_mut().clear();
                }
                if dy > 0.0 && at_end {
                    if self.next_page() {
                        self.imp_().pending_scroll.set(Some((m.page(), 0.0, 0.0)));
                    }
                    return glib::Propagation::Stop;
                }
                if dy < 0.0 && at_start {
                    self.previous_page();
                    return glib::Propagation::Stop;
                }
            }
        }
        if touchpad {
            let (px, py) = (dx * TOUCHPAD_PIXELS, dy * TOUCHPAD_PIXELS);
            let t = e.time();
            let mut samples = self.imp_().touch_samples.borrow_mut();
            samples.retain(|s| t.wrapping_sub(s.0) < 100);
            samples.push((t, px, py));
            drop(samples);
            self.scroll_by(px, py);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    }

    fn scroll_by(&self, dx: f64, dy: f64) -> bool {
        let mut moved = false;
        for (a, d) in [(self.hadj(), dx), (self.vadj(), dy)] {
            let Some(a) = a else { continue };
            if d == 0.0 {
                continue;
            }
            let old = a.value();
            a.set_value((old + d).clamp(a.lower(), (a.upper() - a.page_size()).max(a.lower())));
            moved |= a.value() != old;
        }
        moved
    }

    fn kinetic_stop(&self) {
        if let Some(id) = self.imp_().kinetic.borrow_mut().take() {
            id.remove();
        }
    }

    /// Keeps scrolling after the fingers lift, decelerating like GTK 4's kinetic scrolling.
    fn kinetic_start(&self) {
        let samples = std::mem::take(&mut *self.imp_().touch_samples.borrow_mut());
        let (Some(first), Some(last)) = (samples.first(), samples.last()) else { return };
        let span = last.0.wrapping_sub(first.0).max(8) as f64 / 1000.0;
        let (sx, sy) = samples.iter().skip(1).fold((0.0, 0.0), |(x, y), s| (x + s.1, y + s.2));
        let (vx, vy) = (sx / span, sy / span);
        if samples.len() < 2 || vx.hypot(vy) < KINETIC_MIN_VELOCITY {
            return;
        }
        let start = Cell::new(None::<i64>);
        let done = Cell::new((0.0f64, 0.0f64));
        let id = self.add_tick_callback(move |v, clock| {
            let now = clock.frame_time();
            let t0 = *start.get().get_or_insert(now);
            start.set(Some(t0));
            let t = (now - t0) as f64 / 1_000_000.0;
            let k = (1.0 - (-KINETIC_FRICTION * t).exp()) / KINETIC_FRICTION;
            let (px, py) = (vx * k, vy * k);
            let (ox, oy) = done.get();
            done.set((px, py));
            let moved = v.scroll_by(px - ox, py - oy);
            let speed = vx.hypot(vy) * (-KINETIC_FRICTION * t).exp();
            if !moved && t > 0.0 || speed < KINETIC_MIN_VELOCITY {
                v.imp_().kinetic.borrow_mut().take();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
        *self.imp_().kinetic.borrow_mut() = Some(id);
    }

    fn on_key(&self, k: gdk::keys::Key, mods: gdk::ModifierType) -> glib::Propagation {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let alt = mods.contains(gdk::ModifierType::MOD1_MASK);
        if self.imp_().caret_enabled.get() && !alt {
            let mv = match k {
                key::Left | key::KP_Left => Some(if ctrl { CaretMove::WordLeft } else { CaretMove::Left }),
                key::Right | key::KP_Right => Some(if ctrl { CaretMove::WordRight } else { CaretMove::Right }),
                key::Up | key::KP_Up => Some(CaretMove::Up),
                key::Down | key::KP_Down => Some(CaretMove::Down),
                key::Home | key::KP_Home => Some(if ctrl { CaretMove::DocStart } else { CaretMove::LineStart }),
                key::End | key::KP_End => Some(if ctrl { CaretMove::DocEnd } else { CaretMove::LineEnd }),
                _ => None,
            };
            if let Some(mv) = mv {
                if self.move_caret(mv, shift) {
                    return glib::Propagation::Stop;
                }
            }
        }
        if ctrl && !matches!(k, key::Home | key::End | key::KP_Home | key::KP_End) {
            return glib::Propagation::Proceed;
        }
        match k {
            key::Left | key::KP_Left => self.scroll_step(if alt { 0.0 } else { -1.0 }, if alt { 1.0 } else { 0.0 }),
            key::Right | key::KP_Right => self.scroll_step(if alt { 0.0 } else { 1.0 }, if alt { -1.0 } else { 0.0 }),
            key::Up | key::KP_Up => self.scroll_step(0.0, if alt { 1.0 } else { -1.0 }),
            key::Down | key::KP_Down => self.scroll_step(0.0, if alt { -1.0 } else { 1.0 }),
            key::H => self.scroll_step(-1.0, 0.0),
            key::L => self.scroll_step(1.0, 0.0),
            key::J => self.scroll_step(0.0, 1.0),
            key::K => self.scroll_step(0.0, -1.0),
            key::Page_Up | key::KP_Page_Up => {
                if shift {
                    return glib::Propagation::Proceed;
                }
                self.scroll_page(false)
            }
            key::Page_Down | key::KP_Page_Down => {
                if shift {
                    return glib::Propagation::Proceed;
                }
                self.scroll_page(true)
            }
            key::space | key::KP_Space | key::Return | key::KP_Enter | key::ISO_Enter => self.scroll_page(!shift),
            key::BackSpace => self.scroll_page(shift),
            key::Home | key::KP_Home if ctrl => {
                if let Some(m) = self.model() {
                    m.set_page(0);
                    self.scroll_to_page_point(0, None, None);
                }
            }
            key::End | key::KP_End if ctrl => {
                if let (Some(m), Some(d)) = (self.model(), self.document()) {
                    let last = d.n_pages.saturating_sub(1);
                    m.set_page(last);
                    if let Some(v) = self.vadj() {
                        v.set_value(v.upper() - v.page_size());
                    }
                }
            }
            key::Home | key::KP_Home => {
                if let Some(m) = self.model() {
                    self.scroll_to_page_point(m.page(), None, None);
                }
            }
            key::End | key::KP_End => {
                if let Some(m) = self.model() {
                    let p = m.page();
                    if let Some((_, h)) = self.document().map(|d| d.page_size(p)) {
                        self.scroll_to_page_point(p, None, Some(h));
                    }
                }
            }
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaretMove {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

pub const ANNOT_POPUP_W: i32 = 200;
pub const ANNOT_POPUP_H: i32 = 150;

fn text_view_text(tv: &gtk::TextView) -> String {
    tv.buffer()
        .and_then(|b| b.text(&b.start_iter(), &b.end_iter(), false))
        .map(|t| t.to_string())
        .unwrap_or_default()
}
