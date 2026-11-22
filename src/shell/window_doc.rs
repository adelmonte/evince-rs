use super::application::RunMode;
use super::bookmarks::Bookmarks;
use super::compat::*;
use super::metadata::Metadata;
use super::recent::{recent_manager, RecentView};
use super::title::TitleKind;
use super::window::{MessageKind, Window, PAGE_PASSWORD, PAGE_PRESENTATION, PAGE_RECENT, PAGE_VIEW};
use crate::document::factory::{self, Loaded};
use crate::document::*;
use crate::view::presentation::{Presentation, PresentationEvent};
use crate::view::{DocHandle, SizingMode};
use gtk::prelude::*;
use gtk::{gdk_pixbuf, gio};
use std::rc::Rc;
use std::time::Duration;

fn display_name_for(uri: &str) -> String {
    let f = gio::File::for_uri(uri);
    f.query_info("standard::display-name", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
        .ok()
        .map(|i| i.display_name().to_string())
        .or_else(|| f.basename().map(|b| b.to_string_lossy().to_string()))
        .unwrap_or_else(|| uri.to_string())
}

impl Window {
    // ------------------------------------------------------------ recent view

    pub fn show_recent_view(self: &Rc<Self>) {
        if self.recent.borrow().is_none() {
            let w = self.weak();
            let rv = RecentView::new(move |uri| {
                if let Some(w) = w.upgrade() {
                    w.app.open_uri_at_dest(uri, None, RunMode::Normal, None, Some(&w));
                }
            });
            self.main_stack.add_named(&rv.widget, PAGE_RECENT);
            *self.recent.borrow_mut() = Some(rv);
        }
        self.main_stack.set_visible_child_name(PAGE_RECENT);
        self.title_kind.set(TitleKind::Recent);
        self.update_title();
        self.open_button.set_visible(true);
        self.set_toolbar_doc_widgets(false);
        self.update_actions_sensitivity();
    }

    fn set_toolbar_doc_widgets(&self, doc: bool) {
        self.sidebar_button.set_visible(doc);
        self.page_box.set_visible(doc);
        self.annots_button.set_visible(doc);
        self.find_button.set_visible(doc);
        self.zoom_box.set_visible(doc);
        self.open_button.set_visible(!doc);
    }

    fn destroy_recent_view(&self) {
        if let Some(rv) = self.recent.borrow_mut().take() {
            self.main_stack.remove(&rv.widget);
        }
    }

    // ------------------------------------------------------------ loading

    pub fn open_uri(self: &Rc<Self>, uri: &str, dest: Option<LinkDest>, mode: RunMode, find: Option<String>) {
        if self.uri().as_deref() == Some(uri) && self.document().is_some() {
            if let Some(d) = dest {
                self.view.go_to_dest(&d);
            }
            self.apply_mode(mode);
            if let Some(f) = find {
                self.show_find_bar();
                self.find.entry.set_text(&f);
            }
            return;
        }
        *self.uri.borrow_mut() = Some(uri.to_string());
        *self.display_name.borrow_mut() = Some(display_name_for(uri));
        *self.pending_dest.borrow_mut() = dest;
        self.pending_mode.set(mode);
        *self.pending_find.borrow_mut() = find;
        self.title_kind.set(TitleKind::Document);
        self.update_title();
        let file = gio::File::for_uri(uri);
        let md = Metadata::is_supported(&file).then(|| Rc::new(Metadata::new(&file)));
        *self.metadata.borrow_mut() = md;
        self.setup_size_from_metadata();
        self.load(None);
    }

    fn load(self: &Rc<Self>, password: Option<String>) {
        let Some(uri) = self.uri() else { return };
        let generation = self.loading_gen.get() + 1;
        self.loading_gen.set(generation);
        self.clear_message();
        let w = self.weak();
        let id = glib::timeout_add_local_once(Duration::from_millis(600), move || {
            if let Some(w) = w.upgrade() {
                w.loading_timeout.borrow_mut().take();
                w.loading.set_visible(true);
                if w.document().is_none() {
                    w.main_stack.set_visible_child_name(PAGE_VIEW);
                }
            }
        });
        if let Some(old) = self.loading_timeout.borrow_mut().replace(id) {
            old.remove();
        }
        let (tx, rx) = async_channel::bounded::<std::result::Result<Loaded, Error>>(1);
        let u = uri.clone();
        std::thread::spawn(move || {
            let r = factory::load_document(&u, password.as_deref());
            let _ = tx.send_blocking(r);
        });
        let w = self.weak();
        glib::spawn_future_local(async move {
            let Ok(result) = rx.recv().await else { return };
            let Some(w) = w.upgrade() else { return };
            if w.loading_gen.get() != generation {
                return;
            }
            if let Some(id) = w.loading_timeout.borrow_mut().take() {
                id.remove();
            }
            w.loading.set_visible(false);
            match result {
                Ok(loaded) => w.set_document(loaded),
                Err(Error::Encrypted) => w.show_password_view(),
                Err(e) => {
                    let name = w.display_name.borrow().clone().unwrap_or_default();
                    if w.document().is_none() {
                        w.main_stack.set_visible_child_name(PAGE_VIEW);
                        w.set_toolbar_doc_widgets(true);
                    }
                    w.error_message(&format!("Unable to open document “{name}”."), Some(&e.to_string()));
                    w.app.unregister_failed(&w);
                }
            }
        });
    }

    fn show_password_view(self: &Rc<Self>) {
        let had_attempt = self.main_stack.visible_child_name().as_deref() == Some(PAGE_PASSWORD);
        self.title_kind.set(TitleKind::Password);
        self.update_title();
        let name = self.display_name.borrow().clone().unwrap_or_default();
        self.password_label.set_markup(&format!(
            "The document “{}” is locked and requires a password before it can be opened.",
            glib::markup_escape_text(&name)
        ));
        self.password_error.set_text(if had_attempt { "Wrong password, please try again." } else { "" });
        self.password_error.set_visible(had_attempt);
        self.main_stack.set_visible_child_name(PAGE_PASSWORD);
        self.set_toolbar_doc_widgets(true);
        self.page_box.set_visible(false);
        self.password_entry.set_text("");
        self.password_entry.grab_focus();
    }

    pub(super) fn unlock_document(self: &Rc<Self>) {
        let pw = self.password_entry.text().to_string();
        self.load(Some(pw));
    }

    fn set_document(self: &Rc<Self>, loaded: Loaded) {
        let had_doc = self.document().is_some();
        let doc = DocHandle::new(loaded);
        self.destroy_recent_view();
        self.title_kind.set(TitleKind::Document);
        if let Some(m) = self.metadata() {
            m.set_string("title", doc.info.title.as_deref().filter(|t| !t.is_empty()).unwrap_or(""));
            m.set_string("author", doc.info.author.as_deref().filter(|t| !t.is_empty()).unwrap_or(""));
        }
        if !had_doc {
            self.setup_default();
            self.setup_model_from_metadata();
        }
        *self.doc.borrow_mut() = Some(doc.clone());
        let max = crate::view::max_scale_for_cache(doc.max_size, self.app.settings.page_cache_size() as f64 * 1024.0 * 1024.0);
        self.model.set_max_scale((max * self.view.dpi() / 72.0).max(self.view.dpi() / 72.0).min(64.0 * self.view.dpi() / 72.0));
        self.model.set_document(Some(doc.clone()));
        if !had_doc {
            self.setup_document_from_metadata(&doc);
        }
        if let Some(dest) = self.pending_dest.borrow_mut().take() {
            let d = dest.clone();
            let w = self.weak();
            glib::idle_add_local_once(move || {
                if let Some(w) = w.upgrade() {
                    w.view.go_to_dest(&d);
                }
            });
        }
        *self.bookmarks.borrow_mut() = self.metadata().map(Bookmarks::new);
        self.sidebar().set_document(self, Some(&doc), self.model.rotation(), self.model.inverted());
        self.refresh_bookmarks();
        if doc.n_pages == 0 {
            self.warning_message("The document contains no pages");
        } else if doc.sizes.iter().all(|(w, h)| *w <= 0.0 || *h <= 0.0) {
            self.warning_message("The document contains only empty pages");
        }
        self.main_stack.set_visible_child_name(PAGE_VIEW);
        self.set_toolbar_doc_widgets(true);
        self.update_title();
        self.update_page_entry();
        self.update_zoom_entry();
        self.rebuild_zoom_menu();
        self.update_actions_sensitivity();
        self.setup_monitor();
        if !had_doc {
            self.setup_view_from_metadata();
            let mode = self.pending_mode.replace(RunMode::Normal);
            if mode != RunMode::Normal {
                self.apply_mode(mode);
            } else if doc.info.start_mode == StartModeHint::FullScreen {
                self.run_fullscreen();
            }
            match doc.info.start_mode {
                StartModeHint::UseOutlines => self.sidebar().set_current_page(super::sidebar::LINKS),
                StartModeHint::UseThumbs => self.sidebar().set_current_page(super::sidebar::THUMBNAILS),
                StartModeHint::UseAttachments => self.sidebar().set_current_page(super::sidebar::ATTACHMENTS),
                StartModeHint::UseOc => self.sidebar().set_current_page(super::sidebar::LAYERS),
                _ => {}
            }
            if let Some(uri) = self.uri() {
                let rm = recent_manager();
                let app_name = glib::application_name().map(|s| s.to_string()).unwrap_or_default();
                let exec = format!("{} %u", glib::prgname().map(|s| s.to_string()).unwrap_or_else(|| "evince-rs".into()));
                let data = gtk::RecentData {
                    display_name: None,
                    description: None,
                    mime_type: doc.mime.clone(),
                    app_name,
                    app_exec: exec,
                    groups: Vec::new(),
                    is_private: false,
                };
                rm.add_full(&uri, &data);
            }
        }
        if let Some(f) = self.pending_find.borrow_mut().take() {
            if doc.can_find && !self.is_presentation() {
                self.show_find_bar();
                self.find.entry.set_text(&f);
            }
        }
        if self.is_presentation() {
            if let Some(p) = self.presentation.borrow().as_ref() {
                p.grab_focus();
            }
        } else if !self.find.bar.is_search_mode() {
            self.view.grab_focus();
        }
        if let Some(uri) = self.uri() {
            self.emit_dbus_signal("DocumentLoaded", Some(&(uri,).to_variant()));
        }
        #[cfg(debug_assertions)]
        if !had_doc && std::env::var_os("EVINCE_RS_SELFTEST").is_some() {
            super::selftest::start(self);
        }
        #[cfg(debug_assertions)]
        if !had_doc && std::env::var_os("EVINCE_RS_SCROLLTEST").is_some() {
            super::selftest::scrolltest(self);
        }
        #[cfg(debug_assertions)]
        if !had_doc && std::env::var_os("EVINCE_RS_DIAG").is_some() {
            super::selftest::diag(self);
        }
    }

    fn setup_default(&self) {
        let s = &self.app.settings;
        self.show_sidebar(s.default_bool("show-sidebar", true));
        self.paned.set_position(s.default_int("sidebar-size", 100));
        self.model.set_continuous(s.default_bool("continuous", true));
        self.model.set_dual_page(s.default_bool("dual-page", false));
        self.model.set_dual_odd_left(s.default_bool("dual-page-odd-left", false));
        self.model.set_rtl(gtk::Widget::default_direction() == gtk::TextDirection::Rtl);
        self.model.set_inverted(s.default_bool("inverted-colors", false));
        let mode = SizingMode::from_str(&s.default_string("sizing-mode", "automatic"));
        self.model.set_sizing_mode(mode);
        if mode == SizingMode::Free {
            self.model.set_scale(s.default_double("zoom", 1.0) * self.view.dpi() / 72.0);
        }
        self.sidebar().set_current_page(&s.default_string("sidebar-page", "links"));
        self.set_action_state("sizing-mode", &mode.as_str().to_variant());
        self.set_action_state("continuous", &self.model.continuous().to_variant());
        self.set_action_state("dual-page", &self.model.dual_page().to_variant());
        self.set_action_state("dual-odd-left", &self.model.dual_odd_left().to_variant());
        self.set_action_state("rtl", &self.model.rtl().to_variant());
        self.set_action_state("inverted-colors", &self.model.inverted().to_variant());
    }

    fn setup_size_from_metadata(&self) {
        let Some(m) = self.metadata() else { return };
        if let (Some(w), Some(h)) = (m.get_int("window_width"), m.get_int("window_height")) {
            if w > 0 && h > 0 {
                self.win.set_default_size(w, h);
            }
        }
        if let Some(max) = m.get_bool("window_maximized") {
            if max {
                self.win.maximize();
            } else {
                self.win.unmaximize();
            }
        }
    }

    fn setup_model_from_metadata(&self) {
        let Some(m) = self.metadata() else { return };
        if self.pending_dest.borrow().is_none() {
            if let Some(p) = m.get_int("page") {
                self.model.set_page_unchecked(p.max(0) as usize);
            }
        }
        if let Some(s) = m.get_string("sizing_mode") {
            self.model.set_sizing_mode(SizingMode::from_str(&s));
            self.set_action_state("sizing-mode", &self.model.sizing_mode().as_str().to_variant());
        }
        if self.model.sizing_mode() == SizingMode::Free {
            if let Some(z) = m.get_double("zoom") {
                self.model.set_scale(z * self.view.dpi() / 72.0);
            }
        }
        if let Some(r) = m.get_int("rotation") {
            self.model.set_rotation(if [90, 180, 270].contains(&r) { r } else { 0 });
        }
        if let Some(v) = m.get_bool("inverted-colors") {
            self.model.set_inverted(v);
        }
        if let Some(v) = m.get_bool("continuous") {
            self.model.set_continuous(v);
        }
        if let Some(v) = m.get_bool("dual-page") {
            self.model.set_dual_page(v);
        }
        if let Some(v) = m.get_bool("dual-page-odd-left") {
            self.model.set_dual_odd_left(v);
        }
        if let Some(v) = m.get_bool("rtl") {
            self.model.set_rtl(v);
        }
        if m.get_bool("fullscreen") == Some(true) {
            self.pending_mode.set(if self.pending_mode.get() == RunMode::Normal { RunMode::Fullscreen } else { self.pending_mode.get() });
        }
        if let Some(v) = m.get_bool("sidebar_visibility") {
            self.show_sidebar(v);
        }
    }

    fn setup_document_from_metadata(&self, doc: &Rc<DocHandle>) {
        let m = self.metadata();
        let fresh = m.as_ref().is_none_or(|m| m.get_int("window_width").is_none());
        if let Some(m) = &m {
            if let Some(sz) = m.get_int("sidebar_size") {
                let win_w = m.get_int("window_width").unwrap_or(self.win.default_size().0).max(300);
                self.paned.set_position(sz.clamp(super::window::SIDEBAR_MIN_WIDTH, (win_w / 3).max(100)));
            }
            if let Some(p) = m.get_string("sidebar_page") {
                self.sidebar().set_current_page(&p);
            }
        }
        if m.is_some() && self.model.page() + 1 == doc.n_pages && doc.n_pages > 1 && self.pending_dest.borrow().is_none() {
            self.model.set_page(0);
        }
        if !fresh {
            return;
        }
        if m.as_ref().is_none_or(|m| !m.has_key("dual-page")) {
            match doc.info.layout {
                PageLayoutHint::SinglePage => self.model.set_continuous(false),
                PageLayoutHint::OneColumn => self.model.set_continuous(true),
                PageLayoutHint::TwoColumnLeft => {
                    self.model.set_dual_page(true);
                    self.model.set_dual_odd_left(true);
                }
                PageLayoutHint::TwoColumnRight => self.model.set_dual_page(true),
                PageLayoutHint::TwoPageLeft => {
                    self.model.set_dual_page(true);
                    self.model.set_continuous(false);
                    self.model.set_dual_odd_left(true);
                }
                PageLayoutHint::TwoPageRight => {
                    self.model.set_dual_page(true);
                    self.model.set_continuous(false);
                }
                PageLayoutHint::Unset => {}
            }
        }
        if doc.n_pages == 1 {
            self.model.set_dual_page(false);
        } else if doc.n_pages == 2 {
            self.model.set_dual_odd_left(true);
        }
        let (wr, hr) = self.app.settings.window_ratio();
        if wr > 0.0 && hr > 0.0 {
            let (dw, dh) = doc.max_size;
            let mut rw = (wr * dw + 0.5) as i32;
            let mut rh = (hr * dh + 0.5) as i32;
            if let Some(mon) = WidgetExt::display(&self.win).monitor(0) {
                let g = mon.geometry();
                rw = rw.min(g.width());
                rh = rh.min(g.height());
            }
            if rw > 0 && rh > 0 {
                self.win.set_default_size(rw, rh);
            }
        }
    }

    fn setup_view_from_metadata(&self) {
        let Some(m) = self.metadata() else { return };
        if m.get_bool("presentation") == Some(true) && self.pending_mode.get() == RunMode::Normal {
            self.pending_mode.set(RunMode::Presentation);
        }
        if let Some(pos) = m.get_string("caret-position") {
            if let Ok(v) = glib::Variant::parse(Some(glib::VariantTy::new("(uu)").unwrap()), &pos) {
                if let Some((p, o)) = v.get::<(u32, u32)>() {
                    self.view.set_caret_position(p as usize, o as usize);
                }
            }
        }
        if m.get_bool("caret-navigation") == Some(true) && self.document().is_some_and(|d| d.has_text) {
            self.set_caret_navigation(true);
        }
    }

    pub fn apply_mode(self: &Rc<Self>, mode: RunMode) {
        match mode {
            RunMode::Fullscreen => self.run_fullscreen(),
            RunMode::Presentation => self.run_presentation(),
            RunMode::Normal => {}
        }
    }

    // ------------------------------------------------------------ reload / monitor

    fn setup_monitor(self: &Rc<Self>) {
        let Some(doc) = self.document() else { return };
        if self.monitor.borrow().is_some() {
            return;
        }
        let Some(uri) = self.uri() else { return };
        let file = gio::File::for_uri(&uri);
        let _ = doc;
        let Ok(mon) = file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) else { return };
        let w = self.weak();
        mon.connect_changed(move |_, _, _, ev| {
            let Some(w) = w.upgrade() else { return };
            if !matches!(ev, gio::FileMonitorEvent::Changed | gio::FileMonitorEvent::ChangesDoneHint | gio::FileMonitorEvent::Created) {
                return;
            }
            if let Some(id) = w.reload_timeout.borrow_mut().take() {
                id.remove();
            }
            let ww = w.weak();
            let id = glib::timeout_add_local_once(Duration::from_secs(1), move || {
                if let Some(w) = ww.upgrade() {
                    w.reload_timeout.borrow_mut().take();
                    if w.app.settings.auto_reload() {
                        w.reload(true);
                    }
                }
            });
            *w.reload_timeout.borrow_mut() = Some(id);
        });
        *self.monitor.borrow_mut() = Some(mon);
    }

    pub fn reload(self: &Rc<Self>, from_monitor: bool) {
        let Some(doc) = self.document() else { return };
        if doc.modified.get() || doc.jobs.with(|d| d.is_modified()) {
            if from_monitor {
                return;
            }
            let name = self.display_name.borrow().clone().unwrap_or_default();
            let detail = if doc.jobs.with(|d| !d.form_fields(0).is_empty()) {
                "Document contains form fields that have been filled out. If you reload the document, changes will be permanently lost."
            } else {
                "Document contains new or modified annotations. If you reload the document, changes will be permanently lost."
            };
            let w = self.weak();
            choose(&self.win, &format!("Reload document “{name}”?"), detail, &["_No", "_Reload"], 1, move |r| {
                if r == Some(1) {
                    if let Some(w) = w.upgrade() {
                        w.do_reload();
                    }
                }
            });
            return;
        }
        self.do_reload();
    }

    fn do_reload(self: &Rc<Self>) {
        let Some(uri) = self.uri() else { return };
        let page = self.model.page();
        let generation = self.loading_gen.get() + 1;
        self.loading_gen.set(generation);
        let (tx, rx) = async_channel::bounded::<std::result::Result<Loaded, Error>>(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking(factory::load_document(&uri, None));
        });
        let w = self.weak();
        glib::spawn_future_local(async move {
            let Ok(r) = rx.recv().await else { return };
            let Some(w) = w.upgrade() else { return };
            if w.loading_gen.get() != generation {
                return;
            }
            match r {
                Ok(loaded) => {
                    let doc = DocHandle::new(loaded);
                    *w.doc.borrow_mut() = Some(doc.clone());
                    w.model.set_page_unchecked(page.min(doc.n_pages.saturating_sub(1)));
                    w.model.set_document(Some(doc.clone()));
                    w.sidebar().set_document(&w, Some(&doc), w.model.rotation(), w.model.inverted());
                    w.refresh_bookmarks();
                    w.update_title();
                    w.update_page_entry();
                    w.update_actions_sensitivity();
                    if w.find.bar.is_search_mode() {
                        w.find_restart();
                    }
                }
                Err(Error::Encrypted) => {}
                Err(e) => w.error_message("Failed to reload document.", Some(&e.to_string())),
            }
        });
    }

    // ------------------------------------------------------------ fullscreen / presentation

    pub fn run_fullscreen(self: &Rc<Self>) {
        if self.is_presentation() {
            self.stop_presentation();
        }
        self.win.fullscreen();
        self.set_action_state("fullscreen", &true.to_variant());
        self.view.grab_focus();
    }

    pub fn stop_fullscreen(&self) {
        self.win.unfullscreen();
        self.set_action_state("fullscreen", &false.to_variant());
        self.header_revealer.set_reveal_child(true);
    }

    pub fn run_presentation(self: &Rc<Self>) {
        let Some(doc) = self.document() else { return };
        if self.is_presentation() {
            return;
        }
        if self.find.bar.is_search_mode() {
            self.close_find_bar();
        }
        self.fullscreen_before_presentation.set(self.is_fullscreen());
        let p = Presentation::new(&doc, self.model.page(), self.model.rotation(), self.model.inverted());
        let w = self.weak();
        p.connect_event(move |e| {
            let Some(w) = w.upgrade() else { return };
            match e {
                PresentationEvent::Finished => w.stop_presentation(),
                PresentationEvent::PageChanged(pg) => {
                    if let Some(m) = w.metadata() {
                        m.set_int("page", *pg as i32);
                    }
                }
                PresentationEvent::ExternalLink(a) => w.handle_external_link(a, true),
            }
        });
        p.show();
        self.main_stack.add_named(&p, PAGE_PRESENTATION);
        self.main_stack.set_visible_child_name(PAGE_PRESENTATION);
        *self.presentation.borrow_mut() = Some(p.clone());
        self.header_revealer.set_reveal_child(false);
        self.annots_bar.set_reveal_child(false);
        self.win.fullscreen();
        p.grab_focus();
        self.set_action_state("presentation", &true.to_variant());
        if let Some(m) = self.metadata() {
            m.set_bool("presentation", true);
        }
        let cookie = self.app.gtk_app.inhibit(Some(&self.win), gtk::ApplicationInhibitFlags::IDLE, Some("Running in presentation mode"));
        if cookie != 0 {
            self.inhibit.set(Some(cookie));
        }
        self.update_actions_sensitivity();
    }

    pub fn stop_presentation(self: &Rc<Self>) {
        let Some(p) = self.presentation.borrow_mut().take() else { return };
        let page = p.current_page();
        self.main_stack.set_visible_child_name(PAGE_VIEW);
        self.main_stack.remove(&p);
        self.model.set_page(page);
        if !self.fullscreen_before_presentation.get() {
            self.win.unfullscreen();
            self.header_revealer.set_reveal_child(true);
        }
        if self.annots_button.is_active() {
            self.annots_bar.set_reveal_child(true);
        }
        self.set_action_state("presentation", &false.to_variant());
        if let Some(m) = self.metadata() {
            m.set_bool("presentation", false);
        }
        if let Some(c) = self.inhibit.take() {
            self.app.gtk_app.uninhibit(c);
        }
        self.view.grab_focus();
        self.update_actions_sensitivity();
    }

    // ------------------------------------------------------------ links

    fn resolve_relative(&self, file: &str) -> String {
        if glib::uri_parse_scheme(file).is_some() {
            return file.to_string();
        }
        let p = std::path::Path::new(file);
        if p.is_absolute() {
            return gio::File::for_path(p).uri().to_string();
        }
        let base = self.uri().map(|u| gio::File::for_uri(&u)).and_then(|f| f.parent());
        match base {
            Some(b) => b.resolve_relative_path(file).uri().to_string(),
            None => gio::File::for_path(p).uri().to_string(),
        }
    }

    pub fn handle_external_link(self: &Rc<Self>, action: &LinkAction, from_presentation: bool) {
        match action {
            LinkAction::ExternalUri(uri) => {
                let target = if glib::uri_parse_scheme(uri).is_some() {
                    uri.clone()
                } else if uri.starts_with("www.") {
                    format!("http://{uri}")
                } else {
                    self.resolve_relative(uri)
                };
                if let Err(e) = launch_uri(&self.win, &target) {
                    self.error_message("Unable to open external link", Some(&e.to_string()));
                }
            }
            LinkAction::GotoRemote { file, dest } => {
                let uri = self.resolve_relative(file);
                let mode = if from_presentation { RunMode::Presentation } else { RunMode::Normal };
                self.app.open_uri_at_dest(&uri, dest.clone(), mode, None, Some(self));
            }
            LinkAction::Launch { file, .. } => {
                let uri = self.resolve_relative(file);
                let is_pdf = gio::File::for_uri(&uri)
                    .query_info("standard::content-type", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
                    .ok()
                    .and_then(|i| i.content_type())
                    .is_some_and(|ct| gio::content_type_is_a(&ct, "application/pdf"));
                if !is_pdf {
                    self.warning_message(&format!("Security alert: this document has been prevented from opening the file “{file}”"));
                    return;
                }
                self.app.open_uri_at_dest(&uri, None, RunMode::Normal, None, Some(self));
            }
            LinkAction::Named(n) => match n.as_str() {
                "Find" => self.show_find_bar(),
                "GoToPage" => {
                    self.page_entry.grab_focus();
                }
                "Close" | "Quit" => self.win.close(),
                "Print" => self.cmd_print(),
                "GoBack" => self.history_back(),
                "GoForward" => self.history_forward(),
                "FullScreen" => self.run_fullscreen(),
                _ => {}
            },
            LinkAction::GotoDest(d) => self.view.go_to_dest(d),
            _ => {}
        }
    }

    pub fn open_link_new_window(self: &Rc<Self>, link: &Link) {
        match &link.action {
            Some(LinkAction::GotoDest(d)) => self.cmd_open_copy(Some(d.clone())),
            Some(LinkAction::GotoRemote { file, dest }) => {
                let uri = self.resolve_relative(file);
                self.app.spawn(&uri, dest.clone(), RunMode::Normal, None);
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ commands

    pub(super) fn cmd_open(self: &Rc<Self>) {
        let folder = self
            .uri()
            .and_then(|u| gio::File::for_uri(&u).parent())
            .or_else(|| self.app.settings.document_directory().map(|u| gio::File::for_uri(&u)));
        let w = self.weak();
        choose_files(
            &self.win,
            "Open Document",
            gtk::FileChooserAction::Open,
            "_Open",
            |dlg| {
                dlg.set_select_multiple(true);
                let all_docs = gtk::FileFilter::new();
                all_docs.set_name(Some("All Documents"));
                for m in factory::supported_mime_types() {
                    all_docs.add_mime_type(m);
                }
                dlg.add_filter(all_docs.clone());
                let all = gtk::FileFilter::new();
                all.set_name(Some("All Files"));
                all.add_pattern("*");
                dlg.add_filter(all);
                dlg.set_filter(&all_docs);
                if let Some(f) = &folder {
                    let _ = dlg.set_current_folder_file(f);
                }
            },
            move |files| {
                let Some(w) = w.upgrade() else { return };
                let uris: Vec<String> = files.iter().map(|f| f.uri().to_string()).collect();
                if let Some(first) = uris.first() {
                    if let Some(parent) = gio::File::for_uri(first).parent() {
                        w.app.settings.set_document_directory(&parent.uri());
                    }
                }
                if !uris.is_empty() {
                    w.app.open_uri_list(&uris, Some(&w));
                }
            },
        );
    }

    pub(super) fn cmd_open_copy(self: &Rc<Self>, dest: Option<LinkDest>) {
        let Some(uri) = self.uri() else { return };
        let dest = dest.or(Some(LinkDest::Page(self.model.page())));
        let nw = Window::new(&self.app);
        self.app.add_window(&nw);
        nw.open_uri(&uri, dest, RunMode::Normal, None);
        nw.win.present();
    }

    pub(super) fn cmd_save_as(self: &Rc<Self>) {
        let Some(doc) = self.document() else { return };
        let name = self
            .uri()
            .and_then(|u| gio::File::for_uri(&u).basename())
            .map(|b| b.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".into());
        let folder = self
            .app
            .settings
            .document_directory()
            .map(|u| gio::File::for_uri(&u))
            .or_else(|| glib::user_special_dir(glib::UserDirectory::Documents).map(gio::File::for_path));
        let w = self.weak();
        let setup = move |dlg: &gtk::FileChooserNative| {
            dlg.set_do_overwrite_confirmation(true);
            dlg.set_current_name(&name);
            if let Some(f) = &folder {
                let _ = dlg.set_current_folder_file(f);
            }
        };
        choose_files(&self.win, "Save As…", gtk::FileChooserAction::Save, "_Save", setup, move |files| {
            let (Some(dst), Some(w)) = (files.into_iter().next(), w.upgrade()) else { return };
            if let Some(p) = dst.parent() {
                w.app.settings.set_document_directory(&p.uri());
            }
            let modified = doc.modified.get() || doc.jobs.with(|d| d.is_modified());
            let result: std::result::Result<(), String> = if modified {
                match dst.path() {
                    Some(path) => doc.jobs.with(move |d| d.save(&path)).map_err(|e| e.to_string()),
                    None => {
                        let tmp = factory::temp_path(".pdf");
                        let r = doc.jobs.with({
                            let t = tmp.clone();
                            move |d| d.save(&t)
                        });
                        let r = r.map_err(|e| e.to_string()).and_then(|_| {
                            gio::File::for_path(&tmp)
                                .copy(&dst, gio::FileCopyFlags::OVERWRITE, gio::Cancellable::NONE, None)
                                .map_err(|e| e.to_string())
                        });
                        let _ = std::fs::remove_file(&tmp);
                        r
                    }
                }
            } else {
                let src = gio::File::for_uri(&w.uri().unwrap_or_default());
                src.copy(&dst, gio::FileCopyFlags::OVERWRITE | gio::FileCopyFlags::TARGET_DEFAULT_PERMS, gio::Cancellable::NONE, None)
                    .map_err(|e| e.to_string())
            };
            match result {
                Ok(()) => {
                    if modified {
                        doc.modified.set(false);
                    }
                    recent_manager().add_item(&dst.uri());
                }
                Err(e) => w.error_message(&format!("The file could not be saved as “{}”.", dst.parse_name()), Some(&e)),
            }
        });
    }

    pub(super) fn cmd_send_to(&self) {
        let Some(doc) = self.document() else { return };
        let r = std::process::Command::new("nautilus-sendto").arg(&doc.path).spawn();
        if let Err(e) = r {
            self.error_message("Could not send current document", Some(&e.to_string()));
        }
    }

    pub(super) fn cmd_open_containing_folder(&self) {
        let Some(uri) = self.uri() else { return };
        let file = gio::File::for_uri(&uri);
        open_containing_folder(&self.win, &file);
    }

    pub(super) fn cmd_print(self: &Rc<Self>) {
        let Some(doc) = self.document() else { return };
        self.print_pages(&doc, None);
    }

    pub fn print_range(self: &Rc<Self>, first: usize, last: usize) {
        let Some(doc) = self.document() else { return };
        self.print_pages(&doc, Some((first, last)));
    }

    fn print_pages(self: &Rc<Self>, doc: &Rc<DocHandle>, range: Option<(usize, usize)>) {
        let settings = self.app.load_print_settings();
        let setup = self.app.load_page_setup();
        if let Some(m) = self.metadata() {
            if let Some(o) = m.get_int("page-setup-orientation") {
                setup.set_orientation(match o {
                    1 => gtk::PageOrientation::Landscape,
                    2 => gtk::PageOrientation::ReversePortrait,
                    3 => gtk::PageOrientation::ReverseLandscape,
                    _ => gtk::PageOrientation::Portrait,
                });
            }
            for (k, f) in [
                ("page-setup-margin-top", gtk::PageSetup::set_top_margin as fn(&gtk::PageSetup, f64, gtk::Unit)),
                ("page-setup-margin-bottom", gtk::PageSetup::set_bottom_margin),
                ("page-setup-margin-left", gtk::PageSetup::set_left_margin),
                ("page-setup-margin-right", gtk::PageSetup::set_right_margin),
            ] {
                if let Some(v) = m.get_double(k) {
                    f(&setup, v, gtk::Unit::Mm);
                }
            }
        }
        if let Some((a, b)) = range {
            settings.set_print_pages(gtk::PrintPages::Ranges);
            settings.set_page_ranges(&[gtk::PageRange::new(a as i32, b as i32)]);
        } else if settings.print_pages() == gtk::PrintPages::Ranges {
            settings.set_print_pages(gtk::PrintPages::All);
        }
        if let Some(name) = self.uri().and_then(|u| gio::File::for_uri(&u).basename()) {
            let stem = name.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            settings.set(gtk::PRINT_SETTINGS_OUTPUT_BASENAME, Some(&stem));
        }
        self.printing.set(true);
        let w = self.weak();
        crate::view::print::print_document(Some(self.win.upcast_ref::<gtk::Window>()), doc, Some(&settings), Some(&setup), self.model.page(), false, move |res| {
            let Some(w) = w.upgrade() else { return };
            w.printing.set(false);
            if let Some(e) = res.error {
                w.error_message("Failed to print document", Some(&e));
            }
            if let Some(s) = &res.settings {
                w.app.save_print_settings(s);
            }
            if let Some(ps) = &res.page_setup {
                w.app.save_page_setup(ps);
                if let Some(m) = w.metadata() {
                    let o = match ps.orientation() {
                        gtk::PageOrientation::Landscape => 1,
                        gtk::PageOrientation::ReversePortrait => 2,
                        gtk::PageOrientation::ReverseLandscape => 3,
                        _ => 0,
                    };
                    m.set_int("page-setup-orientation", o);
                    m.set_double("page-setup-margin-top", ps.top_margin(gtk::Unit::Mm));
                    m.set_double("page-setup-margin-bottom", ps.bottom_margin(gtk::Unit::Mm));
                    m.set_double("page-setup-margin-left", ps.left_margin(gtk::Unit::Mm));
                    m.set_double("page-setup-margin-right", ps.right_margin(gtk::Unit::Mm));
                }
            }
            if w.close_confirmed.get() {
                w.win.close();
            }
        });
    }

    pub(super) fn cmd_properties(self: &Rc<Self>) {
        let Some(doc) = self.document() else { return };
        if let Some(p) = self.properties.borrow().as_ref() {
            if p.is_visible() {
                p.present();
                return;
            }
        }
        let p = super::properties::show(self.win.upcast_ref::<gtk::Window>(), &doc);
        *self.properties.borrow_mut() = Some(p);
    }

    pub(super) fn cmd_about(&self) {
        let about = gtk::AboutDialog::new();
        about.set_program_name("Evince RS");
        about.set_version(Some(env!("CARGO_PKG_VERSION")));
        about.set_copyright(Some("© 1996–2023 The Evince document viewer authors"));
        about.set_comments(Some("A simple document viewer, ported to Rust from Evince"));
        about.set_license_type(gtk::License::Gpl20);
        about.set_website(Some("https://gitlab.gnome.org/GNOME/evince-rs"));
        about.set_logo_icon_name(Some("io.github.adelmonte.EvinceRs"));
        about.set_authors(&[
                "Martin Kretzschmar <m_kretzschmar@gmx.net>",
                "Jonathan Blandford <jrb@gnome.org>",
                "Marco Pesenti Gritti <marco@gnome.org>",
                "Nickolay V. Shmyrev <nshmyrev@yandex.ru>",
                "Bryan Clark <clarkbw@gnome.org>",
                "Carlos Garcia Campos <carlosgc@gnome.org>",
                "Wouter Bolsterlee <wbolster@gnome.org>",
                "Christian Persch <chpe\u{0040}src.gnome.org>",
                "Germán Poo-Caamaño <gpoo\u{0040}gnome.org>",
        ]);
        about.set_documenters(&["Nickolay V. Shmyrev <nshmyrev@yandex.ru>", "Phil Bull <philbull@gmail.com>", "Tiffany Antpolski <tiffany.antopolski@gmail.com>"]);
        about.set_transient_for(Some(&self.win));
        about.set_modal(true);
        about.connect_response(|d, _| destroy(d));
        about.present();
    }

    fn image_surface(&self) -> Option<cairo::ImageSurface> {
        let (page, id) = self.view.popup_context().image?;
        let doc = self.document()?;
        doc.jobs.with(move |d| d.image(page, id))
    }

    pub(super) fn cmd_copy_image(&self) {
        let Some(s) = self.image_surface() else { return };
        let Some(bm) = Bitmap::from_surface(s) else { return };
        gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD).set_image(&bm.to_pixbuf());
    }

    pub(super) fn cmd_save_image(self: &Rc<Self>) {
        let Some(s) = self.image_surface() else { return };
        let Some(bm) = Bitmap::from_surface(s) else { return };
        let pixbuf = bm.to_pixbuf();
        let dir = self
            .app
            .settings
            .pictures_directory()
            .map(|u| gio::File::for_uri(&u))
            .or_else(|| glib::user_special_dir(glib::UserDirectory::Pictures).map(gio::File::for_path));
        let setup = move |dlg: &gtk::FileChooserNative| {
            dlg.set_do_overwrite_confirmation(true);
            dlg.set_current_name("image.png");
            if let Some(d) = &dir {
                let _ = dlg.set_current_folder_file(d);
            }
            let f = gtk::FileFilter::new();
            f.set_name(Some("Supported Image Files"));
            for fmt in gdk_pixbuf::Pixbuf::formats() {
                if fmt.is_writable() {
                    for m in fmt.mime_types() {
                        f.add_mime_type(&m);
                    }
                }
            }
            dlg.add_filter(f);
        };
        let w = self.weak();
        choose_files(&self.win, "Save Image", gtk::FileChooserAction::Save, "_Save", setup, move |files| {
            let (Some(file), Some(w)) = (files.into_iter().next(), w.upgrade()) else { return };
            if let Some(p) = file.parent() {
                w.app.settings.set_pictures_directory(&p.uri());
            }
            let name = file.basename().map(|b| b.to_string_lossy().to_lowercase()).unwrap_or_default();
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_else(|| "png".into());
            let format = gdk_pixbuf::Pixbuf::formats().into_iter().find(|f| {
                f.is_writable() && f.extensions().iter().any(|e| e.as_str() == ext)
            });
            let Some(format) = format else {
                w.error_message("Couldn’t find appropriate format to save image", None);
                return;
            };
            let fname = format.name().map(|s| s.to_string()).unwrap_or_else(|| "png".into());
            let res = match file.path() {
                Some(p) => pixbuf.savev(&p, &fname, &[]).map_err(|e| e.to_string()),
                None => pixbuf
                    .save_to_bufferv(&fname, &[])
                    .map_err(|e| e.to_string())
                    .and_then(|buf| file.replace_contents(&buf, None, false, gio::FileCreateFlags::NONE, gio::Cancellable::NONE).map(|_| ()).map_err(|e| e.to_string())),
            };
            if let Err(e) = res {
                w.error_message("The image could not be saved.", Some(&e));
            }
        });
    }

    pub fn open_attachment(&self, name: &str, data: &[u8]) {
        let dir = factory::temp_path("-attachments");
        let _ = std::fs::create_dir_all(&dir);
        let safe = std::path::Path::new(name).file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "attachment".into());
        let path = dir.join(safe);
        if let Err(e) = std::fs::write(&path, data) {
            self.error_message("Unable to open attachment", Some(&e.to_string()));
            return;
        }
        let uri = gio::File::for_path(&path).uri();
        if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE) {
            self.error_message("Unable to open attachment", Some(&e.to_string()));
        }
    }

    pub(super) fn cmd_save_attachment(self: &Rc<Self>) {
        let Some(a) = self.popup_attachment.borrow().clone() else { return };
        let dir = self.app.settings.document_directory().map(|d| gio::File::for_uri(&d));
        let name = a.name.clone();
        let setup = move |dlg: &gtk::FileChooserNative| {
            dlg.set_do_overwrite_confirmation(true);
            dlg.set_current_name(&name);
            if let Some(d) = &dir {
                let _ = dlg.set_current_folder_file(d);
            }
        };
        let w = self.weak();
        choose_files(&self.win, "Save Attachment", gtk::FileChooserAction::Save, "_Save", setup, move |files| {
            let (Some(file), Some(w)) = (files.into_iter().next(), w.upgrade()) else { return };
            if let Err(e) = file.replace_contents(&a.data, None, false, gio::FileCreateFlags::REPLACE_DESTINATION, gio::Cancellable::NONE) {
                w.error_message("The attachment could not be saved.", Some(&e.to_string()));
            }
        });
    }

    pub(super) fn cmd_annot_properties(self: &Rc<Self>) {
        let Some(a) = self.view.popup_context().annot else { return };
        let w = self.weak();
        super::annot_dialog::show(self.win.upcast_ref::<gtk::Window>(), &a, move |na| {
            if let Some(w) = w.upgrade() {
                w.view.update_annotation(&na);
            }
        });
    }

    // ------------------------------------------------------------ synctex

    pub fn sync_source(&self, page: usize, x: f64, y: f64) {
        let Some(doc) = self.document() else { return };
        let Some(st) = doc.synctex() else { return };
        let Some(link) = st.backward(page, x, y) else { return };
        let path = std::path::Path::new(&link.filename);
        let abs = if path.is_absolute() { path.to_path_buf() } else { doc.path.parent().map(|p| p.join(path)).unwrap_or_else(|| path.to_path_buf()) };
        let uri = gio::File::for_path(&abs).uri().to_string();
        let ts = 0u32;
        self.emit_dbus_signal("SyncSource", Some(&(uri, (link.line, link.col), ts).to_variant()));
    }

    pub fn sync_view(&self, source: &str, line: i32, col: i32) {
        let Some(doc) = self.document() else { return };
        let Some(st) = doc.synctex() else { return };
        let filename = if source.contains("://") {
            gio::File::for_uri(source).path().map(|p| p.display().to_string()).unwrap_or_else(|| source.to_string())
        } else {
            source.to_string()
        };
        let link = SourceLink { filename, line, col };
        if let Some((page, rect)) = st.forward(&link) {
            self.view.highlight_forward_search(page, rect);
        }
    }

    // ------------------------------------------------------------ closing

    pub(super) fn on_close_request(self: &Rc<Self>) -> glib::Propagation {
        if !self.close_confirmed.get() {
            if let Some(doc) = self.document() {
                let modified = doc.modified.get() || doc.jobs.with(|d| d.is_modified());
                if modified {
                    self.ask_save_before_close(&doc);
                    return glib::Propagation::Stop;
                }
            }
            if self.printing.get() {
                let name = self.display_name.borrow().clone().unwrap_or_default();
                let w = self.weak();
                choose(
                    &self.win,
                    &format!("Wait until print job “{name}” finishes before closing?"),
                    "If you close the window, pending print jobs will not be printed.",
                    &["Cancel _print and Close", "_Cancel", "Close _after Printing"],
                    2,
                    move |r| {
                    let Some(w) = w.upgrade() else { return };
                    match r {
                        Some(0) => {
                            w.close_confirmed.set(true);
                            w.printing.set(false);
                            w.win.close();
                        }
                        Some(2) => w.close_confirmed.set(true),
                        _ => {}
                    }
                });
                return glib::Propagation::Stop;
            }
        }
        self.save_window_state();
        self.emit_dbus_signal("Closed", None);
        self.unexport_dbus();
        self.app.remove_window(self);
        glib::Propagation::Proceed
    }

    fn ask_save_before_close(self: &Rc<Self>, _doc: &Rc<DocHandle>) {
        let name = self.display_name.borrow().clone().unwrap_or_default();
        let w = self.weak();
        choose(
            &self.win,
            &format!("Save a copy of document “{name}” before closing?"),
            "If you don’t save a copy, changes will be permanently lost.",
            &["Close _without Saving", "C_ancel", "Save a _Copy"],
            2,
            move |r| {
            let Some(w) = w.upgrade() else { return };
            match r {
                Some(0) => {
                    w.close_confirmed.set(true);
                    w.win.close();
                }
                Some(2) => w.cmd_save_as(),
                _ => {}
            }
        });
    }

    fn save_window_state(&self) {
        let Some(m) = self.metadata() else { return };
        if self.document().is_none() {
            return;
        }
        if !self.is_fullscreen() {
            let maximized = self.win.is_maximized();
            m.set_bool("window_maximized", maximized);
            if !maximized {
                let (w, h) = self.win.size();
                m.set_int("window_width", w);
                m.set_int("window_height", h);
            }
        }
        if self.view.caret_navigation() {
            if let Some((p, o)) = self.view.caret_position() {
                m.set_string("caret-position", &(p as u32, o as u32).to_variant().print(false));
            }
        }
        m.flush();
    }

    pub fn present_message(&self, text: &str) {
        self.show_message(MessageKind::Info, text, None, &[]);
    }
}
