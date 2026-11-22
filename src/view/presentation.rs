use super::model::DocHandle;
use super::Priority;
use crate::document::*;
use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gdk::keys::constants as key;
use gtk::gdk;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Normal,
    Black,
    White,
}

#[derive(Clone)]
pub enum PresentationEvent {
    Finished,
    PageChanged(usize),
    ExternalLink(LinkAction),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct R {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl R {
    fn new(x: f64, y: f64, w: f64, h: f64) -> R {
        R { x, y, w, h }
    }
    fn path(&self, cr: &cairo::Context) {
        cr.rectangle(self.x, self.y, self.w, self.h);
    }
}

pub struct Anim {
    from: Option<cairo::ImageSurface>,
    transition: Transition,
    start: Instant,
    backwards: bool,
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Presentation {
        pub doc: RefCell<Option<Rc<DocHandle>>>,
        pub page: Cell<usize>,
        pub rotation: Cell<i32>,
        pub inverted: Cell<bool>,
        pub screen: Cell<Option<Screen>>,
        pub textures: RefCell<HashMap<usize, (cairo::ImageSurface, f64, i32)>>,
        pub pending: RefCell<HashMap<usize, super::super::JobHandle>>,
        pub links: RefCell<HashMap<usize, Vec<Mapping<Link>>>>,
        pub anim: RefCell<Option<Anim>>,
        pub tick: RefCell<Option<gtk::TickCallbackId>>,
        pub auto_advance: RefCell<Option<glib::SourceId>>,
        pub hide_cursor: RefCell<Option<glib::SourceId>>,
        pub goto_buffer: RefCell<String>,
        pub listeners: RefCell<Vec<Rc<dyn Fn(&PresentationEvent)>>>,
        pub last_motion: Cell<(f64, f64)>,
        pub scroll_acc: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Presentation {
        const NAME: &'static str = "EvViewPresentation";
        type Type = super::Presentation;
        type ParentType = gtk::DrawingArea;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("evpresentationview");
        }
    }

    impl ObjectImpl for Presentation {
        fn constructed(&self) {
            self.parent_constructed();
            self.screen.set(Some(Screen::Normal));
            let obj = self.obj();
            obj.set_can_focus(true);
            obj.add_events(
                gdk::EventMask::BUTTON_PRESS_MASK
                    | gdk::EventMask::BUTTON_RELEASE_MASK
                    | gdk::EventMask::POINTER_MOTION_MASK
                    | gdk::EventMask::SCROLL_MASK
                    | gdk::EventMask::KEY_PRESS_MASK,
            );
            obj.set_hexpand(true);
            obj.set_vexpand(true);
            obj.setup_controllers();
        }

        fn dispose(&self) {
            if let Some(id) = self.auto_advance.borrow_mut().take() {
                id.remove();
            }
            if let Some(id) = self.hide_cursor.borrow_mut().take() {
                id.remove();
            }
            if let Some(t) = self.tick.borrow_mut().take() {
                t.remove();
            }
            for (_, j) in self.pending.borrow_mut().drain() {
                j.cancel();
            }
        }
    }

    impl WidgetImpl for Presentation {
        fn size_allocate(&self, allocation: &gtk::Allocation) {
            self.parent_size_allocate(allocation);
            self.obj().request_pages();
        }

        fn draw(&self, cr: &cairo::Context) -> glib::Propagation {
            self.obj().draw_presentation(cr);
            glib::Propagation::Stop
        }

        fn button_release_event(&self, e: &gdk::EventButton) -> glib::Propagation {
            let p = self.obj();
            let (x, y) = e.position();
            p.grab_focus();
            if self.screen.get().is_none() {
                p.emit(PresentationEvent::Finished);
                return glib::Propagation::Stop;
            }
            match e.button() {
                1 => match p.link_at(x, y) {
                    Some(l) => p.handle_link(&l),
                    None => p.next(),
                },
                3 => p.previous(),
                _ => {}
            }
            glib::Propagation::Stop
        }

        fn scroll_event(&self, e: &gdk::EventScroll) -> glib::Propagation {
            let p = self.obj();
            let dy = match e.direction() {
                gdk::ScrollDirection::Up => -1.0,
                gdk::ScrollDirection::Down => 1.0,
                gdk::ScrollDirection::Smooth => e.delta().1,
                _ => 0.0,
            };
            if e.direction() == gdk::ScrollDirection::Smooth {
                let acc = self.scroll_acc.get() + dy;
                if acc.abs() < 1.0 {
                    self.scroll_acc.set(acc);
                    return glib::Propagation::Stop;
                }
                self.scroll_acc.set(0.0);
                if acc > 0.0 { p.next() } else { p.previous() }
            } else if dy > 0.0 {
                p.next();
            } else if dy < 0.0 {
                p.previous();
            }
            glib::Propagation::Stop
        }

        fn motion_notify_event(&self, e: &gdk::EventMotion) -> glib::Propagation {
            let p = self.obj();
            let (x, y) = e.position();
            let (ox, oy) = self.last_motion.get();
            if (ox - x).abs() > 0.5 || (oy - y).abs() > 0.5 {
                self.last_motion.set((x, y));
                p.poke_cursor();
            }
            glib::Propagation::Proceed
        }
    }

    impl DrawingAreaImpl for Presentation {}
}

glib::wrapper! {
    pub struct Presentation(ObjectSubclass<imp::Presentation>)
        @extends gtk::DrawingArea, gtk::Widget,
        @implements gtk::Buildable;
}

impl Presentation {
    pub fn new(doc: &Rc<DocHandle>, page: usize, rotation: i32, inverted: bool) -> Presentation {
        let p: Presentation = glib::Object::new();
        let imp = p.imp();
        *imp.doc.borrow_mut() = Some(doc.clone());
        imp.page.set(page.min(doc.n_pages.saturating_sub(1)));
        imp.rotation.set(rotation);
        imp.inverted.set(inverted);
        p.schedule_auto_advance();
        p
    }

    pub fn connect_event(&self, f: impl Fn(&PresentationEvent) + 'static) {
        self.imp().listeners.borrow_mut().push(Rc::new(f));
    }

    fn emit(&self, e: PresentationEvent) {
        let ls: Vec<_> = self.imp().listeners.borrow().clone();
        for l in ls {
            l(&e);
        }
    }

    pub fn current_page(&self) -> usize {
        self.imp().page.get()
    }

    pub fn set_rotation(&self, r: i32) {
        self.imp().rotation.set(r);
        self.imp().textures.borrow_mut().clear();
        self.request_pages();
    }

    pub fn set_inverted(&self, v: bool) {
        self.imp().inverted.set(v);
        self.queue_draw();
    }

    fn doc(&self) -> Option<Rc<DocHandle>> {
        self.imp().doc.borrow().clone()
    }

    fn page_scale(&self, page: usize) -> f64 {
        let Some(doc) = self.doc() else { return 1.0 };
        let (w, h) = doc.rotated_size(page, self.imp().rotation.get());
        let (vw, vh) = (self.allocated_width() as f64, self.allocated_height() as f64);
        (vw / w).min(vh / h).max(0.01)
    }

    fn page_rect(&self, page: usize) -> R {
        let Some(doc) = self.doc() else { return R::default() };
        let s = self.page_scale(page);
        let (w, h) = doc.rotated_size(page, self.imp().rotation.get());
        let (pw, ph) = ((w * s).round(), (h * s).round());
        let x = ((self.allocated_width() as f64 - pw) / 2.0).floor();
        let y = ((self.allocated_height() as f64 - ph) / 2.0).floor();
        R::new(x, y, pw, ph)
    }

    fn request_pages(&self) {
        let Some(doc) = self.doc() else { return };
        if self.allocated_width() <= 1 {
            return;
        }
        let imp = self.imp();
        let cur = imp.page.get();
        let rot = imp.rotation.get();
        let mut wanted = vec![(cur, Priority::Urgent)];
        if cur + 1 < doc.n_pages {
            wanted.push((cur + 1, Priority::High));
        }
        if cur > 0 {
            wanted.push((cur - 1, Priority::Low));
        }
        imp.textures.borrow_mut().retain(|p, _| p.abs_diff(cur) <= 2);
        for (page, prio) in wanted {
            let scale = self.page_scale(page) * self.scale_factor().max(1) as f64;
            let ok = imp.textures.borrow().get(&page).is_some_and(|t| (t.1 - scale).abs() < 1e-6 && t.2 == rot);
            if ok || imp.pending.borrow().contains_key(&page) {
                continue;
            }
            let weak = self.downgrade();
            let job = doc.jobs.run(
                prio,
                move |d| (d.render(page, scale, rot).and_then(Bitmap::from_surface), d.links(page)),
                move |(bm, links)| {
                    let Some(p) = weak.upgrade() else { return };
                    p.imp().pending.borrow_mut().remove(&page);
                    p.imp().links.borrow_mut().insert(page, links);
                    if let Some(tex) = bm.and_then(|bm| bm.to_surface()) {
                        p.imp().textures.borrow_mut().insert(page, (tex, scale, rot));
                    }
                    p.queue_draw();
                },
            );
            imp.pending.borrow_mut().insert(page, job);
        }
    }

    pub fn go_to(&self, page: usize) {
        let Some(doc) = self.doc() else { return };
        let imp = self.imp();
        let page = page.min(doc.n_pages.saturating_sub(1));
        let old = imp.page.get();
        if page == old {
            return;
        }
        let from = imp.textures.borrow().get(&old).map(|t| t.0.clone());
        let trans = doc.jobs.with(move |d| d.transition(page));
        imp.page.set(page);
        if let Some(t) = trans.filter(|t| t.kind != TransitionKind::Replace && t.duration > 0.0) {
            *imp.anim.borrow_mut() = Some(Anim { from, transition: t, start: Instant::now(), backwards: page < old });
            if imp.tick.borrow().is_none() {
                let id = self.add_tick_callback(|w, _| {
                    let p = w.downcast_ref::<Presentation>().unwrap();
                    let done = p
                        .imp()
                        .anim
                        .borrow()
                        .as_ref()
                        .is_none_or(|a| a.start.elapsed().as_secs_f64() >= a.transition.duration);
                    p.queue_draw();
                    if done {
                        p.imp().anim.borrow_mut().take();
                        p.imp().tick.borrow_mut().take();
                        return glib::ControlFlow::Break;
                    }
                    glib::ControlFlow::Continue
                });
                *imp.tick.borrow_mut() = Some(id);
            }
        }
        self.request_pages();
        self.schedule_auto_advance();
        self.queue_draw();
        self.emit(PresentationEvent::PageChanged(page));
    }

    pub fn next(&self) {
        let imp = self.imp();
        if imp.screen.get() != Some(Screen::Normal) {
            imp.screen.set(Some(Screen::Normal));
            self.queue_draw();
            return;
        }
        let n = self.doc().map_or(0, |d| d.n_pages);
        let cur = imp.page.get();
        if cur + 1 < n {
            self.go_to(cur + 1);
        } else {
            imp.screen.set(None);
            self.queue_draw();
        }
    }

    pub fn previous(&self) {
        let imp = self.imp();
        if imp.screen.get() != Some(Screen::Normal) {
            imp.screen.set(Some(Screen::Normal));
            self.queue_draw();
            return;
        }
        let cur = imp.page.get();
        if cur > 0 {
            self.go_to(cur - 1);
        }
    }

    fn toggle_screen(&self, s: Screen) {
        let imp = self.imp();
        imp.screen.set(Some(if imp.screen.get() == Some(s) { Screen::Normal } else { s }));
        self.queue_draw();
    }

    fn schedule_auto_advance(&self) {
        let imp = self.imp();
        if let Some(id) = imp.auto_advance.borrow_mut().take() {
            id.remove();
        }
        let Some(doc) = self.doc() else { return };
        let page = imp.page.get();
        let Some(secs) = doc.jobs.with(move |d| d.page_duration(page)).filter(|s| *s > 0.0) else { return };
        let weak = self.downgrade();
        let id = glib::timeout_add_local_once(Duration::from_secs_f64(secs), move || {
            if let Some(p) = weak.upgrade() {
                p.imp().auto_advance.borrow_mut().take();
                p.next();
            }
        });
        *imp.auto_advance.borrow_mut() = Some(id);
    }

    fn draw_presentation(&self, cr: &cairo::Context) {
        let imp = self.imp();
        let (w, h) = (self.allocated_width() as f64, self.allocated_height() as f64);
        let full = R::new(0.0, 0.0, w, h);
        let fill = |r: &R, c: f64| {
            cr.set_source_rgb(c, c, c);
            r.path(cr);
            let _ = cr.fill();
        };
        match imp.screen.get() {
            Some(Screen::Black) => return fill(&full, 0.0),
            Some(Screen::White) => return fill(&full, 1.0),
            None => {
                fill(&full, 0.0);
                let layout = self.create_pango_layout(Some("End of presentation. Click to exit."));
                let (lw, _) = layout.pixel_size();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.move_to((w - lw as f64) / 2.0, 20.0);
                pangocairo::functions::show_layout(cr, &layout);
                return;
            }
            Some(Screen::Normal) => {}
        }
        fill(&full, 0.0);
        let page = imp.page.get();
        let rect = self.page_rect(page);
        let cur = imp.textures.borrow().get(&page).map(|t| t.0.clone());
        let anim = imp.anim.borrow();
        let draw_tex = |cr: &cairo::Context, t: &Option<cairo::ImageSurface>, r: &R| match t {
            Some(t) if t.width() > 0 && t.height() > 0 => {
                cr.save().ok();
                cr.translate(r.x, r.y);
                cr.scale(r.w / t.width() as f64, r.h / t.height() as f64);
                let _ = cr.set_source_surface(t, 0.0, 0.0);
                let _ = cr.paint();
                cr.restore().ok();
            }
            _ => {
                cr.set_source_rgb(1.0, 1.0, 1.0);
                r.path(cr);
                let _ = cr.fill();
            }
        };
        match anim.as_ref() {
            Some(a) => {
                let t = (a.start.elapsed().as_secs_f64() / a.transition.duration).clamp(0.0, 1.0);
                self.draw_transition(cr, a, t, &full, &rect, &cur, draw_tex);
            }
            None => draw_tex(cr, &cur, &rect),
        }
        if imp.inverted.get() {
            cr.save().ok();
            cr.set_operator(cairo::Operator::Difference);
            cr.set_source_rgb(1.0, 1.0, 1.0);
            full.path(cr);
            let _ = cr.fill();
            cr.restore().ok();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_transition(
        &self,
        cr: &cairo::Context,
        a: &Anim,
        t: f64,
        full: &R,
        rect: &R,
        cur: &Option<cairo::ImageSurface>,
        draw_tex: impl Fn(&cairo::Context, &Option<cairo::ImageSurface>, &R),
    ) {
        let (w, h) = (full.w, full.h);
        let tr = &a.transition;
        let from_rect = *rect;
        let old = |cr: &cairo::Context| {
            if a.from.is_some() {
                draw_tex(cr, &a.from, &from_rect);
            }
        };
        let black = |cr: &cairo::Context, r: &R| {
            cr.set_source_rgb(0.0, 0.0, 0.0);
            r.path(cr);
            let _ = cr.fill();
        };
        let clipped = |cr: &cairo::Context, clip: &R, f: &dyn Fn(&cairo::Context)| {
            cr.save().ok();
            clip.path(cr);
            cr.clip();
            black(cr, clip);
            f(cr);
            cr.restore().ok();
        };
        let dir = if a.backwards { -1.0 } else { 1.0 };
        let (dx, dy) = match tr.angle {
            90 => (0.0, -1.0),
            180 => (-1.0, 0.0),
            270 => (0.0, 1.0),
            _ => (1.0, 0.0),
        };
        let clip_new = |cr: &cairo::Context, clip: R| {
            old(cr);
            clipped(cr, &clip, &|cr| draw_tex(cr, cur, rect));
        };
        match tr.kind {
            TransitionKind::Fade | TransitionKind::Dissolve | TransitionKind::Glitter => {
                old(cr);
                cr.push_group();
                black(cr, full);
                draw_tex(cr, cur, rect);
                let _ = cr.pop_group_to_source();
                let _ = cr.paint_with_alpha(t);
            }
            TransitionKind::Push => {
                cr.save().ok();
                cr.translate(-dx * dir * t * w, dy * dir * t * h);
                old(cr);
                cr.restore().ok();
                cr.save().ok();
                cr.translate(dx * dir * (1.0 - t) * w, -dy * dir * (1.0 - t) * h);
                draw_tex(cr, cur, rect);
                cr.restore().ok();
            }
            TransitionKind::Cover | TransitionKind::Fly => {
                old(cr);
                cr.save().ok();
                cr.translate(dx * dir * (1.0 - t) * w, -dy * dir * (1.0 - t) * h);
                black(cr, full);
                draw_tex(cr, cur, rect);
                cr.restore().ok();
            }
            TransitionKind::Uncover => {
                draw_tex(cr, cur, rect);
                cr.save().ok();
                cr.translate(-dx * dir * t * w, dy * dir * t * h);
                black(cr, full);
                old(cr);
                cr.restore().ok();
            }
            TransitionKind::Wipe => {
                let clip = if dx != 0.0 {
                    if dx * dir > 0.0 {
                        R::new(0.0, 0.0, w * t, h)
                    } else {
                        R::new(w * (1.0 - t), 0.0, w * t, h)
                    }
                } else if dy * dir < 0.0 {
                    R::new(0.0, 0.0, w, h * t)
                } else {
                    R::new(0.0, h * (1.0 - t), w, h * t)
                };
                clip_new(cr, clip);
            }
            TransitionKind::Split => {
                let p = if tr.outward { t } else { 1.0 - t };
                let clip = if tr.horizontal {
                    R::new(0.0, h * (1.0 - p) / 2.0, w, h * p)
                } else {
                    R::new(w * (1.0 - p) / 2.0, 0.0, w * p, h)
                };
                if tr.outward {
                    clip_new(cr, clip);
                } else {
                    draw_tex(cr, cur, rect);
                    clipped(cr, &clip, &old);
                }
            }
            TransitionKind::Box => {
                let p = if tr.outward { t } else { 1.0 - t };
                let clip = R::new(w * (1.0 - p) / 2.0, h * (1.0 - p) / 2.0, w * p, h * p);
                if tr.outward {
                    clip_new(cr, clip);
                } else {
                    draw_tex(cr, cur, rect);
                    clipped(cr, &clip, &old);
                }
            }
            TransitionKind::Blinds => {
                old(cr);
                let n = 6;
                for i in 0..n {
                    let clip = if tr.horizontal {
                        let bh = h / n as f64;
                        R::new(0.0, i as f64 * bh, w, bh * t)
                    } else {
                        let bw = w / n as f64;
                        R::new(i as f64 * bw, 0.0, bw * t, h)
                    };
                    clipped(cr, &clip, &|cr| draw_tex(cr, cur, rect));
                }
            }
            TransitionKind::Replace => draw_tex(cr, cur, rect),
        }
    }

    fn link_at(&self, x: f64, y: f64) -> Option<Link> {
        let doc = self.doc()?;
        let page = self.imp().page.get();
        let r = self.page_rect(page);
        let s = self.page_scale(page);
        let (rw, rh) = (r.w, r.h);
        let (lx, ly) = (x - r.x, y - r.y);
        if lx < 0.0 || ly < 0.0 || lx > rw || ly > rh {
            return None;
        }
        let (pw, ph) = doc.page_size(page);
        let (ux, uy) = misc::unrotate_point(lx, ly, pw * s, ph * s, self.imp().rotation.get());
        let (px, py) = (ux / s, uy / s);
        self.imp().links.borrow().get(&page)?.iter().rev().find(|m| m.area.contains(px, py)).map(|m| m.data.clone())
    }

    fn handle_link(&self, link: &Link) {
        let Some(action) = &link.action else { return };
        let Some(doc) = self.doc() else { return };
        match action {
            LinkAction::GotoDest(d) => {
                if let Some((p, _)) = doc.resolve_dest(d) {
                    self.go_to(p);
                }
            }
            LinkAction::Named(n) => match n.as_str() {
                "NextPage" => self.next(),
                "PrevPage" => self.previous(),
                "FirstPage" => self.go_to(0),
                "LastPage" => self.go_to(doc.n_pages.saturating_sub(1)),
                "Close" | "Quit" => self.emit(PresentationEvent::Finished),
                _ => {}
            },
            a => self.emit(PresentationEvent::ExternalLink(a.clone())),
        }
    }

    fn set_cursor_name(&self, name: Option<&str>) {
        let Some(win) = self.window() else { return };
        let cursor = name.and_then(|n| gdk::Cursor::from_name(&self.display(), n));
        win.set_cursor(cursor.as_ref());
    }

    fn poke_cursor(&self) {
        let imp = self.imp();
        if let Some(id) = imp.hide_cursor.borrow_mut().take() {
            id.remove();
        }
        let (x, y) = imp.last_motion.get();
        if self.link_at(x, y).is_some() {
            self.set_cursor_name(Some("pointer"));
        } else {
            self.set_cursor_name(None);
        }
        let weak = self.downgrade();
        let id = glib::timeout_add_local_once(Duration::from_secs(5), move || {
            if let Some(p) = weak.upgrade() {
                p.imp().hide_cursor.borrow_mut().take();
                p.set_cursor_name(Some("none"));
            }
        });
        *imp.hide_cursor.borrow_mut() = Some(id);
    }

    fn setup_controllers(&self) {
        let weak = self.downgrade();
        self.connect_key_press_event(move |_, e| match weak.upgrade() {
            Some(p) => p.on_key(e.keyval(), e.state()),
            None => glib::Propagation::Proceed,
        });
    }

    fn on_key(&self, k: gdk::keys::Key, mods: gdk::ModifierType) -> glib::Propagation {
        if mods.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::MOD1_MASK) {
            return glib::Propagation::Proceed;
        }
        let imp = self.imp();
        if let Some(d) = k.to_unicode().and_then(|c| c.to_digit(10)) {
            imp.goto_buffer.borrow_mut().push(char::from_digit(d, 10).unwrap());
            return glib::Propagation::Stop;
        }
        match k {
            key::Return | key::KP_Enter if !imp.goto_buffer.borrow().is_empty() => {
                let s = imp.goto_buffer.take();
                if let (Some(doc), Ok(_)) = (self.doc(), s.parse::<usize>()) {
                    if let Some(p) = doc.find_page_by_label(&s) {
                        imp.screen.set(Some(Screen::Normal));
                        self.go_to(p);
                    }
                }
            }
            key::Right | key::Down | key::Page_Down | key::space | key::Return | key::KP_Enter | key::n | key::N
            | key::KP_Right | key::KP_Down | key::KP_Page_Down => self.next(),
            key::Left | key::Up | key::Page_Up | key::BackSpace | key::p | key::P | key::KP_Left | key::KP_Up
            | key::KP_Page_Up => self.previous(),
            key::Home | key::KP_Home => {
                imp.screen.set(Some(Screen::Normal));
                self.go_to(0);
            }
            key::End | key::KP_End => {
                imp.screen.set(Some(Screen::Normal));
                let n = self.doc().map_or(0, |d| d.n_pages);
                self.go_to(n.saturating_sub(1));
            }
            key::b | key::B | key::period | key::KP_Decimal => self.toggle_screen(Screen::Black),
            key::w | key::W | key::comma | key::KP_Separator => self.toggle_screen(Screen::White),
            key::Escape => self.emit(PresentationEvent::Finished),
            _ => {
                imp.goto_buffer.borrow_mut().clear();
                return glib::Propagation::Proceed;
            }
        }
        if !matches!(k, key::Return | key::KP_Enter) {
            imp.goto_buffer.borrow_mut().clear();
        }
        glib::Propagation::Stop
    }
}
