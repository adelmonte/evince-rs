use super::application::{App, RunMode};
use super::bookmarks::Bookmarks;
use super::compat::*;
use super::find::{FindBar, FindSidebar};
use super::history::History;
use super::metadata::Metadata;
use super::recent::RecentView;
use super::sidebar::Sidebar;
use super::title::{self, TitleKind};
use crate::document::*;
use crate::view::presentation::Presentation;
use crate::view::{Change, DocHandle, Model, SizingMode, View, ViewEvent, ZOOM_LEVELS};
use gtk::prelude::*;
use gdk::keys::constants as key;
use gtk::{gdk, gio};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

pub const SIDEBAR_MIN_WIDTH: i32 = 56;
pub const PAGE_VIEW: &str = "view";
pub const PAGE_PASSWORD: &str = "password";
pub const PAGE_RECENT: &str = "recent";
pub const PAGE_PRESENTATION: &str = "presentation";
pub const PAGE_EMPTY: &str = "empty";

/// A document window (EvWindow).
pub struct Window {
    pub app: Rc<App>,
    pub win: gtk::ApplicationWindow,
    pub id: u32,
    pub model: Rc<Model>,
    pub view: View,
    pub scrolled: gtk::ScrolledWindow,
    pub header: gtk::HeaderBar,
    pub header_revealer: gtk::Revealer,
    pub title_label: gtk::Label,
    pub subtitle_label: gtk::Label,
    pub open_button: gtk::Button,
    pub sidebar_button: gtk::ToggleButton,
    pub page_entry: gtk::Entry,
    pub page_total: gtk::Label,
    pub page_box: gtk::Box,
    pub annots_button: gtk::ToggleButton,
    pub find_button: gtk::ToggleButton,
    pub zoom_entry: gtk::Entry,
    pub zoom_menu: gio::Menu,
    pub zoom_box: gtk::Box,
    pub action_menu: gtk::MenuButton,
    pub annots_bar: gtk::Revealer,
    pub annot_text_button: gtk::ToggleButton,
    pub annot_highlight_button: gtk::ToggleButton,
    pub find: FindBar,
    pub find_sidebar: FindSidebar,
    pub sidebar: RefCell<Option<Rc<Sidebar>>>,
    pub side_stack: gtk::Stack,
    pub paned: gtk::Paned,
    pub main_stack: gtk::Stack,
    pub message_box: gtk::Box,
    pub loading: gtk::Box,
    pub password_entry: gtk::Entry,
    pub password_label: gtk::Label,
    pub password_error: gtk::Label,
    pub recent: RefCell<Option<Rc<RecentView>>>,
    pub presentation: RefCell<Option<Presentation>>,
    pub doc: RefCell<Option<Rc<DocHandle>>>,
    pub uri: RefCell<Option<String>>,
    pub display_name: RefCell<Option<String>>,
    pub metadata: RefCell<Option<Rc<Metadata>>>,
    pub bookmarks: RefCell<Option<Bookmarks>>,
    pub history: RefCell<History>,
    pub history_pending: Cell<bool>,
    pub pending_dest: RefCell<Option<LinkDest>>,
    pub pending_mode: Cell<RunMode>,
    pub pending_find: RefCell<Option<String>>,
    pub title_kind: Cell<TitleKind>,
    pub loading_gen: Cell<u64>,
    pub loading_timeout: RefCell<Option<glib::SourceId>>,
    pub monitor: RefCell<Option<gio::FileMonitor>>,
    pub reload_timeout: RefCell<Option<glib::SourceId>>,
    pub popup_attachment: RefCell<Option<std::sync::Arc<AttachmentRef>>>,
    pub dbus_reg: RefCell<Option<crate::mainthread::Registration>>,
    pub inhibit: Cell<Option<u32>>,
    pub fullscreen_before_presentation: Cell<bool>,
    pub sidebar_before_fullscreen: Cell<bool>,
    pub header_hide: RefCell<Option<glib::SourceId>>,
    pub printing: Cell<bool>,
    pub close_confirmed: Cell<bool>,
    pub properties: RefCell<Option<gtk::Window>>,
    pub self_weak: RefCell<Weak<Window>>,
    pub fullscreen: Cell<bool>,
}

fn icon_toggle(icon: &str, action: &str, tooltip: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::new();
    b.set_icon_name(icon);
    b.set_action_name(Some(action));
    b.set_tooltip_text(Some(tooltip));
    b
}

impl Window {
    pub fn new(app: &Rc<App>) -> Rc<Window> {
        let win = gtk::ApplicationWindow::new(&app.gtk_app);
        win.set_default_size(600, 600);
        win.set_icon_name(Some("io.github.adelmonte.EvinceRs"));
        win.add_css_class("evince-rs");
        win.add_events(gdk::EventMask::POINTER_MOTION_MASK);
        let id = app.next_window_id();
        let model = Model::new();
        let view = View::new();
        view.set_model(&model);
        view.set_page_cache_size(app.settings.page_cache_size() as f64 * 1024.0 * 1024.0);
        view.add_css_class("content-view");
        let scrolled = scrolled_window();
        scrolled.set_content(Some(&view));
        scrolled.set_hexpand(true);
        scrolled.set_vexpand(true);

        let header = gtk::HeaderBar::new();
        header.set_show_close_button(true);
        let title_label = gtk::Label::new(Some("Evince RS"));
        title_label.add_css_class("title");
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let subtitle_label = gtk::Label::new(None);
        subtitle_label.add_css_class("subtitle");
        subtitle_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let tbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tbox.set_valign(gtk::Align::Center);
        tbox.append(&title_label);
        tbox.append(&subtitle_label);
        tbox.show_all();
        hide_initially(&subtitle_label);
        header.set_custom_title(Some(&tbox));

        let open_button = gtk::Button::with_label("Open…");
        open_button.set_action_name(Some("win.open"));
        open_button.set_tooltip_text(Some("Open an existing document"));
        header.pack_start(&open_button);
        let sidebar_button = icon_toggle("view-sidebar-symbolic", "win.show-side-pane", "Side pane");
        header.pack_start(&sidebar_button);
        let page_entry = gtk::Entry::new();
        page_entry.set_width_chars(5);
        page_entry.set_max_width_chars(8);
        EntryExt::set_alignment(&page_entry, 1.0);
        page_entry.add_css_class("tnum");
        page_entry.set_tooltip_text(Some("Select page or search in the outline"));
        set_accessible_name(&page_entry, "Select page");
        let page_total = gtk::Label::new(None);
        page_total.add_css_class("dim-label");
        page_total.add_css_class("tnum");
        let page_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        page_box.append(&page_entry);
        page_box.append(&page_total);
        header.pack_start(&page_box);
        let annots_button = icon_toggle("document-edit-symbolic", "win.toggle-edit-annots", "Annotate the document");
        set_accessible_name(&annots_button, "Annotate document");
        header.pack_start(&annots_button);

        let action_menu = gtk::MenuButton::new();
        action_menu.set_icon_name("open-menu-symbolic");
        action_menu.set_tooltip_text(Some("File options"));
        action_menu.set_use_popover(true);
        action_menu.set_menu_model(Some(&Self::build_action_menu()));
        header.pack_end(&action_menu);
        let find_button = icon_toggle("edit-find-symbolic", "win.toggle-find", "Find a word or phrase in the document");
        header.pack_end(&find_button);
        let zoom_entry = gtk::Entry::new();
        zoom_entry.set_width_chars(6);
        zoom_entry.set_max_width_chars(7);
        EntryExt::set_alignment(&zoom_entry, 1.0);
        zoom_entry.add_css_class("tnum");
        zoom_entry.set_icon_from_icon_name(gtk::EntryIconPosition::Secondary, Some("pan-down-symbolic"));
        zoom_entry.set_tooltip_text(Some("Select or set the zoom level of the document"));
        set_accessible_name(&zoom_entry, "Set zoom level");
        let zoom_menu = gio::Menu::new();
        let zoom_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        zoom_box.append(&zoom_entry);
        header.pack_end(&zoom_box);

        let annot_text_button = gtk::ToggleButton::new();
        annot_text_button.set_icon_name("annotations-text-symbolic");
        annot_text_button.set_tooltip_text(Some("Add text annotation"));
        let annot_highlight_button = gtk::ToggleButton::new();
        annot_highlight_button.set_icon_name("format-text-highlight-symbolic");
        annot_highlight_button.set_tooltip_text(Some("Add highlight annotation"));
        let abox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        abox.set_halign(gtk::Align::Center);
        abox.set_margin_top(6);
        abox.set_margin_bottom(6);
        abox.append(&annot_text_button);
        abox.append(&annot_highlight_button);
        abox.add_css_class("toolbar");
        let annots_bar = gtk::Revealer::new();
        annots_bar.set_content(Some(&abox));
        annots_bar.add_css_class("inline-toolbar");

        let side_stack = gtk::Stack::new();
        side_stack.set_size_request(SIDEBAR_MIN_WIDTH, -1);
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.pack1(&side_stack, false, true);
        paned.set_position(100);

        let overlay = gtk::Overlay::new();
        overlay.set_content(Some(&scrolled));
        let loading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        loading.set_widget_name("ev-loading-message");
        loading.add_css_class("osd");
        loading.add_css_class("app-notification");
        loading.set_halign(gtk::Align::End);
        loading.set_valign(gtk::Align::Start);
        loading.set_margin_top(12);
        loading.set_margin_end(12);
        let spinner = gtk::Spinner::new();
        spinner.start();
        loading.append(&spinner);
        loading.append(&gtk::Label::new(Some("Loading…")));
        hide_initially(&loading);
        overlay.add_overlay(&loading);
        let message_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let view_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        view_page.append(&message_box);
        view_page.append(&overlay);
        paned.pack2(&view_page, true, false);

        let password_entry = gtk::Entry::new();
        password_entry.set_visibility(false);
        password_entry.set_input_purpose(gtk::InputPurpose::Password);
        password_entry.set_icon_from_icon_name(gtk::EntryIconPosition::Secondary, Some("view-reveal-symbolic"));
        password_entry.connect_icon_press(|e, pos, _| {
            if pos == gtk::EntryIconPosition::Secondary {
                let show = !EntryExt::is_visible(e);
                e.set_visibility(show);
                e.set_icon_from_icon_name(
                    gtk::EntryIconPosition::Secondary,
                    Some(if show { "view-conceal-symbolic" } else { "view-reveal-symbolic" }),
                );
            }
        });
        password_entry.set_width_chars(30);
        let password_label = gtk::Label::new(Some("This document is locked and can only be read by entering the correct password."));
        password_label.set_line_wrap(true);
        password_label.set_justify(gtk::Justification::Center);
        password_label.set_max_width_chars(50);
        let password_error = gtk::Label::new(None);
        password_error.add_css_class("error");
        let unlock = gtk::Button::with_mnemonic("_Unlock Document");
        unlock.add_css_class("suggested-action");
        unlock.set_halign(gtk::Align::Center);
        let pbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
        pbox.set_valign(gtk::Align::Center);
        pbox.set_halign(gtk::Align::Center);
        let lock = icon_image("dialog-password-symbolic");
        lock.set_pixel_size(64);
        lock.add_css_class("dim-label");
        pbox.append(&lock);
        pbox.append(&password_label);
        pbox.append(&password_entry);
        pbox.append(&password_error);
        pbox.append(&unlock);

        let main_stack = gtk::Stack::new();
        main_stack.add_named(&paned, PAGE_VIEW);
        main_stack.add_named(&pbox, PAGE_PASSWORD);
        main_stack.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), PAGE_EMPTY);
        main_stack.show_all();
        main_stack.set_visible_child_name(PAGE_EMPTY);

        let header_revealer = gtk::Revealer::new();
        header_revealer.set_content(Some(&header));
        header_revealer.set_reveal_child(true);
        header_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);

        let w = Rc::new_cyclic(|weak: &Weak<Window>| Window {
            app: app.clone(),
            win: win.clone(),
            id,
            model,
            view,
            scrolled,
            header,
            header_revealer: header_revealer.clone(),
            title_label,
            subtitle_label,
            open_button,
            sidebar_button,
            page_entry,
            page_total,
            page_box,
            annots_button,
            find_button,
            zoom_entry,
            zoom_menu,
            zoom_box,
            action_menu,
            annots_bar: annots_bar.clone(),
            annot_text_button,
            annot_highlight_button,
            find: FindBar::new(weak.clone()),
            find_sidebar: FindSidebar::new(weak.clone()),
            sidebar: RefCell::new(None),
            side_stack,
            paned,
            main_stack: main_stack.clone(),
            message_box,
            loading,
            password_entry: password_entry.clone(),
            password_label,
            password_error,
            recent: RefCell::new(None),
            presentation: RefCell::new(None),
            doc: RefCell::new(None),
            uri: RefCell::new(None),
            display_name: RefCell::new(None),
            metadata: RefCell::new(None),
            bookmarks: RefCell::new(None),
            history: RefCell::new(History::default()),
            history_pending: Cell::new(false),
            pending_dest: RefCell::new(None),
            pending_mode: Cell::new(RunMode::Normal),
            pending_find: RefCell::new(None),
            title_kind: Cell::new(TitleKind::Document),
            loading_gen: Cell::new(0),
            loading_timeout: RefCell::new(None),
            monitor: RefCell::new(None),
            reload_timeout: RefCell::new(None),
            popup_attachment: RefCell::new(None),
            dbus_reg: RefCell::new(None),
            inhibit: Cell::new(None),
            fullscreen_before_presentation: Cell::new(false),
            sidebar_before_fullscreen: Cell::new(true),
            header_hide: RefCell::new(None),
            printing: Cell::new(false),
            close_confirmed: Cell::new(false),
            properties: RefCell::new(None),
            self_weak: RefCell::new(weak.clone()),
            fullscreen: Cell::new(false),
        });
        let sidebar = Sidebar::new(Rc::downgrade(&w));
        w.side_stack.add_named(&sidebar.widget, "sidebar");
        w.side_stack.add_named(&w.find_sidebar.widget, "find");
        w.side_stack.set_visible_child_name("sidebar");
        *w.sidebar.borrow_mut() = Some(sidebar);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header_revealer);
        content.append(&w.find.bar);
        content.append(&annots_bar);
        content.append(&main_stack);
        win.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        win.set_content(Some(&content));
        content.show_all();

        w.setup_actions();
        w.setup_zoom_widget();
        w.setup_page_entry();
        w.setup_view_events();
        w.setup_model_events();
        w.setup_annots_toolbar();
        w.setup_window_events();
        w.setup_help_overlay();
        let ww = Rc::downgrade(&w);
        let unlock_cb = move || {
            if let Some(w) = ww.upgrade() {
                w.unlock_document();
            }
        };
        let u2 = unlock_cb.clone();
        unlock.connect_clicked(move |_| u2());
        password_entry.connect_activate(move |_| unlock_cb());
        w.update_title();
        w.update_actions_sensitivity();
        w.export_dbus();
        w
    }

    pub fn weak(&self) -> Weak<Window> {
        self.self_weak.borrow().clone()
    }

    pub fn rc(&self) -> Option<Rc<Window>> {
        self.weak().upgrade()
    }

    pub fn document(&self) -> Option<Rc<DocHandle>> {
        self.doc.borrow().clone()
    }

    pub fn metadata(&self) -> Option<Rc<Metadata>> {
        self.metadata.borrow().clone()
    }

    pub fn sidebar(&self) -> Rc<Sidebar> {
        self.sidebar.borrow().clone().expect("sidebar")
    }

    pub fn is_empty(&self) -> bool {
        self.doc.borrow().is_none() && self.uri.borrow().is_none()
    }

    pub fn uri(&self) -> Option<String> {
        self.uri.borrow().clone()
    }

    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen.get()
    }

    pub fn is_presentation(&self) -> bool {
        self.presentation.borrow().is_some()
    }

    fn build_action_menu() -> gio::Menu {
        let menu = gio::Menu::new();
        let section = |items: &[(&str, &str)]| {
            let s = gio::Menu::new();
            for (label, action) in items {
                s.append(Some(label), Some(action));
            }
            menu.append_section(None, &s);
        };
        let top = gio::Menu::new();
        top.append(Some("Print…"), Some("win.print"));
        top.append(Some("Fullscreen"), Some("win.fullscreen"));
        top.append(Some("Send To…"), Some("win.send-to"));
        menu.append_section(None, &top);
        section(&[("New _Window", "win.new")]);
        section(&[("Open a C_opy", "win.open-copy"), ("Open Containing _Folder", "win.open-containing-folder")]);
        section(&[("_Save As…", "win.save-as")]);
        section(&[("Present as _Slideshow", "win.presentation")]);
        section(&[
            ("_Continuous", "win.continuous"),
            ("_Dual", "win.dual-page"),
            ("_Odd Pages Left", "win.dual-odd-left"),
            ("Right to Left Document", "win.rtl"),
        ]);
        section(&[("Ro_tate ⤵", "win.rotate-right")]);
        section(&[("Ni_ght Mode", "win.inverted-colors")]);
        section(&[("Prop_erties", "win.show-properties")]);
        section(&[
            ("_Keyboard Shortcuts", "win.show-help-overlay"),
            ("_Help", "win.help"),
            ("_About Evince RS", "win.about"),
        ]);
        menu
    }

    fn setup_help_overlay(&self) {
        let b = gtk::Builder::new();
        if b.add_from_string(include_str!("help-overlay.ui")).is_ok() {
            if let Some(sw) = b.object::<gtk::ShortcutsWindow>("help_overlay") {
                #[allow(deprecated)]
                self.win.set_help_overlay(Some(&sw));
            }
        }
    }

    // ------------------------------------------------------------ zoom widget

    fn setup_zoom_widget(self: &Rc<Self>) {
        self.rebuild_zoom_menu();
        let pop = gtk::Popover::from_model(Some(&self.zoom_entry), &self.zoom_menu);
        pop.set_position(gtk::PositionType::Bottom);
        let p = pop.clone();
        self.zoom_entry.connect_icon_press(move |e, pos, _| {
            if pos == gtk::EntryIconPosition::Secondary {
                p.set_pointing_to(&e.icon_area(pos));
                p.popup();
            }
        });
        let w = self.weak();
        self.zoom_entry.connect_activate(move |e| {
            let Some(w) = w.upgrade() else { return };
            let text = e.text().to_string();
            let num: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
            if let Ok(z) = num.replace(',', ".").parse::<f64>() {
                let z = (z / 100.0).clamp(w.model.min_scale(), w.model.max_scale() * 72.0 / w.view.dpi());
                w.model.set_sizing_mode(SizingMode::Free);
                w.model.set_scale(z * w.view.dpi() / 72.0);
            }
            w.update_zoom_entry();
            w.view.grab_focus();
        });
        let w = self.weak();
        self.zoom_entry.connect_focus_out_event(move |_, _| {
            if let Some(w) = w.upgrade() {
                w.update_zoom_entry();
            }
            glib::Propagation::Proceed
        });
    }

    pub fn rebuild_zoom_menu(&self) {
        self.zoom_menu.remove_all();
        let s1 = gio::Menu::new();
        s1.append(Some("Fit Pa_ge"), Some("win.sizing-mode::fit-page"));
        s1.append(Some("Fit _Width"), Some("win.sizing-mode::fit-width"));
        s1.append(Some("_Automatic"), Some("win.sizing-mode::automatic"));
        self.zoom_menu.append_section(None, &s1);
        let s2 = gio::Menu::new();
        let max = self.model.max_scale() * 72.0 / self.view.dpi().max(1.0);
        for (label, z) in ZOOM_LEVELS {
            if *z > max + 1e-6 {
                break;
            }
            let item = gio::MenuItem::new(Some(label), None);
            item.set_action_and_target_value(Some("win.zoom"), Some(&z.to_variant()));
            s2.append_item(&item);
        }
        self.zoom_menu.append_section(None, &s2);
    }

    pub fn update_zoom_entry(&self) {
        let z = self.model.scale() * 72.0 / self.view.dpi().max(1.0);
        if !self.zoom_entry.has_focus() {
            self.zoom_entry.set_text(&format!("{}%", (z * 100.0).round() as i64));
        }
    }

    // ------------------------------------------------------------ page entry

    fn setup_page_entry(self: &Rc<Self>) {
        let w = self.weak();
        self.page_entry.connect_activate(move |e| {
            let Some(w) = w.upgrade() else { return };
            let Some(doc) = w.document() else { return };
            let text = e.text().to_string();
            if let Some(p) = doc.find_page_by_label(&text) {
                w.go_to_page_with_history(p);
                w.view.grab_focus();
            } else if !text.trim().is_empty() && w.sidebar().outline.has_items.get() {
                w.show_sidebar(true);
                w.sidebar().set_current_page(super::sidebar::LINKS);
                w.sidebar().outline.show_search();
            } else {
                w.update_page_entry();
            }
        });
        let w = self.weak();
        self.page_entry.connect_focus_out_event(move |_, _| {
            if let Some(w) = w.upgrade() {
                w.update_page_entry();
            }
            glib::Propagation::Proceed
        });
    }

    pub fn update_page_entry(&self) {
        let Some(doc) = self.document() else {
            self.page_entry.set_text("");
            self.page_total.set_text("");
            return;
        };
        let page = self.model.page();
        self.page_entry.set_text(&doc.label(page));
        if doc.has_labels && doc.label(page) != (page + 1).to_string() {
            self.page_total.set_text(&format!("({} of {})", page + 1, doc.n_pages));
        } else {
            self.page_total.set_text(&format!("of {}", doc.n_pages));
        }
        let chars = doc.label(doc.n_pages.saturating_sub(1)).chars().count().max(3) as i32;
        self.page_entry.set_width_chars(chars + 1);
    }

    // ------------------------------------------------------------ annotations toolbar

    fn setup_annots_toolbar(self: &Rc<Self>) {
        let w = self.weak();
        self.annot_text_button.connect_toggled(move |b| {
            let Some(w) = w.upgrade() else { return };
            if b.is_active() {
                w.annot_highlight_button.set_active(false);
                w.view.set_add_mode(Some(crate::view::AddMode::Text));
            } else if w.view.add_mode() == Some(crate::view::AddMode::Text) {
                w.view.set_add_mode(None);
            }
            w.sync_add_annotation_state();
        });
        let w = self.weak();
        self.annot_highlight_button.connect_toggled(move |b| {
            let Some(w) = w.upgrade() else { return };
            if b.is_active() {
                w.annot_text_button.set_active(false);
                if w.view.has_selection() && w.view.add_highlight_from_selection() {
                    b.set_active(false);
                    return;
                }
                w.view.set_add_mode(Some(crate::view::AddMode::Highlight));
            } else if w.view.add_mode() == Some(crate::view::AddMode::Highlight) {
                w.view.set_add_mode(None);
            }
        });
    }

    fn sync_add_annotation_state(&self) {
        if let Some(a) = self.win.lookup_action("add-annotation").and_downcast::<gio::SimpleAction>() {
            let on = self.view.add_mode() == Some(crate::view::AddMode::Text);
            a.set_state(&on.to_variant());
        }
    }

    // ------------------------------------------------------------ events

    fn setup_view_events(self: &Rc<Self>) {
        let w = self.weak();
        self.view.connect_event(move |ev| {
            let Some(w) = w.upgrade() else { return };
            match ev {
                ViewEvent::ExternalLink(a) => w.handle_external_link(a, false),
                ViewEvent::Popup { x, y } => w.show_view_popup(*x, *y),
                ViewEvent::SelectionChanged => w.update_actions_sensitivity(),
                ViewEvent::SyncSource { page, x, y } => w.sync_source(*page, *x, *y),
                ViewEvent::AnnotsChanged(_) | ViewEvent::AnnotAdded(_) => {
                    let doc = w.document();
                    w.sidebar().reload_annotations(doc.as_ref());
                    w.update_actions_sensitivity();
                }
                ViewEvent::LayersChanged => {
                    if let Some(d) = w.document() {
                        w.sidebar().layers.reload(&d);
                    }
                }
                ViewEvent::FormChanged => w.update_actions_sensitivity(),
                ViewEvent::FindUpdated => w.find_updated(),
                ViewEvent::HistoryJump { from_page } => {
                    w.history.borrow_mut().add_page(*from_page);
                    w.history_pending.set(true);
                    w.update_history_actions();
                }
                ViewEvent::AddModeFinished => {
                    w.annot_text_button.set_active(false);
                    w.annot_highlight_button.set_active(false);
                    w.sync_add_annotation_state();
                }
                ViewEvent::CaretToggleRequested => w.toggle_caret_navigation(),
            }
        });
    }

    fn setup_model_events(self: &Rc<Self>) {
        let w = self.weak();
        self.model.connect(move |c| {
            let Some(w) = w.upgrade() else { return };
            w.on_model_change(c);
        });
    }

    fn on_model_change(&self, c: Change) {
        let md = self.document().and(self.metadata());
        match c {
            Change::Page { new, .. } => {
                self.update_page_entry();
                self.sidebar().page_changed(new);
                if self.history_pending.replace(false) {
                    self.history.borrow_mut().add_page(new);
                }
                if let Some(m) = &md {
                    if !self.is_presentation() {
                        m.set_int("page", new as i32);
                    }
                }
                self.update_actions_sensitivity();
            }
            Change::Scale => {
                self.update_zoom_entry();
                if let Some(m) = &md {
                    if self.model.sizing_mode() == SizingMode::Free {
                        m.set_double("zoom", self.model.scale() * 72.0 / self.view.dpi());
                    }
                }
                self.update_zoom_actions();
            }
            Change::Sizing => {
                self.set_action_state("sizing-mode", &self.model.sizing_mode().as_str().to_variant());
                if let Some(m) = &md {
                    m.set_string("sizing_mode", self.model.sizing_mode().as_str());
                }
            }
            Change::Rotation => {
                if let Some(m) = &md {
                    m.set_int("rotation", self.model.rotation());
                }
                self.sidebar().thumbnails.refresh(self.model.rotation(), self.model.inverted());
                if let Some(p) = self.presentation.borrow().as_ref() {
                    p.set_rotation(self.model.rotation());
                }
            }
            Change::Continuous => {
                self.set_action_state("continuous", &self.model.continuous().to_variant());
                if let Some(m) = &md {
                    m.set_bool("continuous", self.model.continuous());
                }
            }
            Change::DualPage => {
                self.set_action_state("dual-page", &self.model.dual_page().to_variant());
                if let Some(m) = &md {
                    m.set_bool("dual-page", self.model.dual_page());
                }
                self.update_actions_sensitivity();
            }
            Change::DualOddLeft => {
                self.set_action_state("dual-odd-left", &self.model.dual_odd_left().to_variant());
                if let Some(m) = &md {
                    m.set_bool("dual-page-odd-left", self.model.dual_odd_left());
                }
            }
            Change::Rtl => {
                self.set_action_state("rtl", &self.model.rtl().to_variant());
                if let Some(m) = &md {
                    m.set_bool("rtl", self.model.rtl());
                }
            }
            Change::Inverted => {
                self.set_action_state("inverted-colors", &self.model.inverted().to_variant());
                if let Some(m) = &md {
                    m.set_bool("inverted-colors", self.model.inverted());
                }
                self.sidebar().thumbnails.refresh(self.model.rotation(), self.model.inverted());
                if let Some(p) = self.presentation.borrow().as_ref() {
                    p.set_inverted(self.model.inverted());
                }
            }
            Change::MaxScale => {
                self.rebuild_zoom_menu();
                self.update_zoom_actions();
            }
            Change::Document | Change::Fullscreen => {}
        }
    }

    fn setup_window_events(self: &Rc<Self>) {
        let w = self.weak();
        self.win.connect_delete_event(move |_, _| {
            let Some(w) = w.upgrade() else { return glib::Propagation::Proceed };
            w.on_close_request()
        });
        let w = self.weak();
        self.win.connect_window_state_event(move |_, e| {
            if let Some(w) = w.upgrade() {
                let fs = e.new_window_state().contains(gdk::WindowState::FULLSCREEN);
                if fs == w.fullscreen.get() {
                    return glib::Propagation::Proceed;
                }
                w.fullscreen.set(fs);
                if !w.is_presentation() {
                    w.set_action_state("fullscreen", &fs.to_variant());
                    if let Some(m) = w.metadata() {
                        m.set_bool("fullscreen", fs);
                    }
                }
                w.header_revealer.set_reveal_child(!fs);
                w.model.set_fullscreen(fs);
            }
            glib::Propagation::Proceed
        });
        let w = self.weak();
        self.win.connect_motion_notify_event(move |win, e| {
            let Some(w) = w.upgrade() else { return glib::Propagation::Proceed };
            if !w.is_fullscreen() || w.is_presentation() {
                return glib::Propagation::Proceed;
            }
            let y = match (win.window(), e.device()) {
                (Some(gw), Some(dev)) => gw.device_position(&dev).2 as f64,
                _ => e.position().1,
            };
            if y < 8.0 {
                if let Some(id) = w.header_hide.borrow_mut().take() {
                    id.remove();
                }
                w.header_revealer.set_reveal_child(true);
            } else if w.header_revealer.reveals_child() && y > w.header_revealer.allocated_height() as f64 + 20.0 && w.header_hide.borrow().is_none() {
                let ww = w.weak();
                let id = glib::timeout_add_local_once(Duration::from_millis(1000), move || {
                    if let Some(w) = ww.upgrade() {
                        w.header_hide.borrow_mut().take();
                        if w.is_fullscreen() && !w.action_menu.is_active() {
                            w.header_revealer.set_reveal_child(false);
                        }
                    }
                });
                *w.header_hide.borrow_mut() = Some(id);
            }
            glib::Propagation::Proceed
        });

        let w = self.weak();
        self.win.connect_key_press_event(move |_, e| {
            let Some(w) = w.upgrade() else { return glib::Propagation::Proceed };
            w.capture_key(e.keyval(), e.state())
        });

        self.win.drag_dest_set(
            gtk::DestDefaults::ALL,
            &[gtk::TargetEntry::new("text/uri-list", gtk::TargetFlags::OTHER_APP, 0)],
            gdk::DragAction::COPY,
        );
        let w = self.weak();
        self.win.connect_drag_data_received(move |_, _, _, _, data, _, _| {
            let Some(w) = w.upgrade() else { return };
            let uris: Vec<String> = data.uris().iter().map(|u| u.to_string()).collect();
            if !uris.is_empty() {
                w.app.open_uri_list(&uris, Some(&w));
            }
        });

        let w = self.weak();
        self.paned.connect_position_notify(move |p| {
            if p.position() < SIDEBAR_MIN_WIDTH {
                p.set_position(SIDEBAR_MIN_WIDTH);
                return;
            }
            if let Some(w) = w.upgrade() {
                if let Some(m) = w.metadata() {
                    if p.child1().is_some_and(|c| c.is_visible()) && w.side_stack.visible_child_name().as_deref() == Some("sidebar") {
                        m.set_int("sidebar_size", p.position());
                    }
                }
            }
        });
        let w = self.weak();
        self.sidebar().stack.connect_visible_child_name_notify(move |s| {
            if let (Some(w), Some(name)) = (w.upgrade(), s.visible_child_name()) {
                if let Some(m) = w.metadata() {
                    if w.document().is_some() {
                        m.set_string("sidebar_page", &name);
                    }
                }
            }
        });
    }

    /// Handles keys that a scrolled window or entry would otherwise swallow.
    fn capture_key(&self, k: gdk::keys::Key, mods: gdk::ModifierType) -> glib::Propagation {
        let focus = self.win.focused_widget();
        let editable = focus.as_ref().is_some_and(|f| f.is::<gtk::Entry>() || f.is::<gtk::TextView>());
        if editable || self.is_presentation() {
            return glib::Propagation::Proceed;
        }
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let caret = self.view.caret_navigation() && focus.as_ref().is_some_and(|f| f == self.view.upcast_ref::<gtk::Widget>());
        let action = match k {
            key::Left if ctrl && !caret => Some("rotate-left"),
            key::Right if ctrl && !caret => Some("rotate-right"),
            key::Page_Up | key::KP_Page_Up if ctrl => Some("go-previous-page"),
            key::Page_Down | key::KP_Page_Down if ctrl => Some("go-next-page"),
            key::Page_Up | key::KP_Page_Up if shift => Some("go-backwards"),
            key::Page_Down | key::KP_Page_Down if shift => Some("go-forward"),
            _ => None,
        };
        if let Some(a) = action {
            gio::prelude::ActionGroupExt::activate_action(&self.win, a, None);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    }

    pub fn set_action_state(&self, name: &str, v: &glib::Variant) {
        if let Some(a) = self.win.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            if a.state().as_ref() != Some(v) {
                a.set_state(v);
            }
        }
    }

    pub fn set_action_enabled(&self, name: &str, on: bool) {
        if let Some(a) = self.win.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            a.set_enabled(on);
        }
    }

    // ------------------------------------------------------------ title

    pub fn update_title(&self) {
        let ltr = self.win.direction() != gtk::TextDirection::Rtl;
        let doc = self.document();
        let doc_title = doc
            .as_ref()
            .and_then(|d| d.info.title.clone())
            .map(|t| title::sanitize_title(t.trim(), doc.as_ref().map_or("", |d| d.mime.as_str())));
        let name = self.display_name.borrow().clone();
        let t = title::compute(self.title_kind.get(), name.as_deref(), doc_title.as_deref(), ltr);
        self.win.set_title(&t.window);
        self.title_label.set_text(&t.header);
        match &t.subtitle {
            Some(s) => {
                self.subtitle_label.set_text(s);
                self.subtitle_label.set_visible(true);
            }
            None => self.subtitle_label.set_visible(false),
        }
        let dir = doc.as_ref().and_then(|d| d.path.parent().map(|p| p.display().to_string()));
        self.header.set_tooltip_text(dir.as_deref());
    }

    // ------------------------------------------------------------ message area

    pub fn clear_message(&self) {
        while let Some(c) = self.message_box.first_child() {
            self.message_box.remove(&c);
        }
    }

    pub fn show_message(&self, kind: MessageKind, primary: &str, secondary: Option<&str>, buttons: &[(&str, Rc<dyn Fn()>)]) {
        self.clear_message();
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        bar.add_css_class("ev-message-area");
        bar.add_css_class(match kind {
            MessageKind::Error => "error",
            MessageKind::Warning => "warning",
            MessageKind::Question | MessageKind::Info => "info",
        });
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_margin_start(6);
        bar.set_margin_end(6);
        let icon = icon_image(match kind {
            MessageKind::Error => "dialog-error-symbolic",
            MessageKind::Warning => "dialog-warning-symbolic",
            MessageKind::Question => "dialog-question-symbolic",
            MessageKind::Info => "dialog-information-symbolic",
        });
        icon.set_valign(gtk::Align::Start);
        bar.append(&icon);
        let texts = gtk::Box::new(gtk::Orientation::Vertical, 6);
        texts.set_hexpand(true);
        let p = gtk::Label::new(None);
        p.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(primary)));
        p.set_xalign(0.0);
        p.set_line_wrap(true);
        p.set_selectable(true);
        texts.append(&p);
        if let Some(s) = secondary {
            let l = gtk::Label::new(None);
            l.set_markup(&format!("<small>{}</small>", glib::markup_escape_text(s)));
            l.set_xalign(0.0);
            l.set_line_wrap(true);
            l.set_selectable(true);
            texts.append(&l);
        }
        bar.append(&texts);
        for (label, cb) in buttons {
            let b = gtk::Button::with_mnemonic(label);
            b.set_valign(gtk::Align::Center);
            let cb = cb.clone();
            let w = self.weak();
            b.connect_clicked(move |_| {
                cb();
                if let Some(w) = w.upgrade() {
                    w.clear_message();
                }
            });
            bar.append(&b);
        }
        let close = icon_button("window-close-symbolic");
        close.set_relief(gtk::ReliefStyle::None);
        close.set_valign(gtk::Align::Start);
        let w = self.weak();
        close.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.clear_message();
            }
        });
        bar.append(&close);
        let frame = gtk::Frame::new(None);
        frame.set_content(Some(&bar));
        frame.show_all();
        self.message_box.append(&frame);
    }

    pub fn error_message(&self, primary: &str, err: Option<&str>) {
        self.show_message(MessageKind::Error, primary, err, &[]);
    }

    pub fn warning_message(&self, primary: &str) {
        self.show_message(MessageKind::Warning, primary, None, &[]);
    }

    // ------------------------------------------------------------ sidebar

    pub fn show_sidebar(&self, on: bool) {
        self.side_stack.set_visible(on);
        self.set_action_state("show-side-pane", &on.to_variant());
        if let Some(m) = self.metadata() {
            if self.document().is_some() && !self.is_fullscreen() {
                m.set_bool("sidebar_visibility", on);
            }
        }
    }

    pub fn go_to_page(&self, page: usize) {
        self.model.set_page(page);
    }

    pub fn go_to_page_with_history(&self, page: usize) {
        let cur = self.model.page();
        if cur != page {
            self.history.borrow_mut().add_page(cur);
            self.history_pending.set(true);
        }
        self.view.go_to_dest(&LinkDest::Page(page));
        self.update_history_actions();
    }

    pub fn show_annotation(&self, a: &Annotation) {
        self.view.go_to_dest(&LinkDest::Xyz { page: a.page, left: Some(a.area.x1), top: Some(a.area.y1), zoom: None });
        if a.is_markup {
            let a = a.clone();
            let w = self.weak();
            glib::idle_add_local_once(move || {
                if let Some(w) = w.upgrade() {
                    w.view.open_annotation_popup(&a);
                }
            });
        }
    }

    pub fn set_layer_visible(&self, id: usize, visible: bool) {
        let Some(doc) = self.document() else { return };
        doc.jobs.with(move |d| d.set_layer_visible(id, visible));
        self.view.invalidate_all();
        for p in 0..doc.n_pages {
            self.sidebar().thumbnails.invalidate(p);
        }
    }

    pub fn set_popup_attachment(&self, a: Option<std::sync::Arc<AttachmentRef>>) {
        let on = a.is_some();
        *self.popup_attachment.borrow_mut() = a;
        self.set_action_enabled("open-attachment", on);
        self.set_action_enabled("save-attachment", on);
    }

    // ------------------------------------------------------------ bookmarks

    pub fn refresh_bookmarks(&self) {
        let items = self.bookmarks.borrow().as_ref().map(|b| b.items().to_vec()).unwrap_or_default();
        self.sidebar().bookmarks.set_items(&items);
        self.update_actions_sensitivity();
    }

    pub fn add_bookmark(&self) {
        let Some(doc) = self.document() else { return };
        let page = self.model.page();
        if let Some(b) = self.bookmarks.borrow_mut().as_mut() {
            b.add(super::bookmarks::Bookmark { page: page as u32, title: format!("Page {}", doc.label(page)) });
        }
        self.refresh_bookmarks();
    }

    pub fn delete_bookmark(&self, page: u32) {
        if let Some(b) = self.bookmarks.borrow_mut().as_mut() {
            b.delete(page);
        }
        self.refresh_bookmarks();
    }

    pub fn rename_bookmark(&self, page: u32, title: &str) {
        if let Some(b) = self.bookmarks.borrow_mut().as_mut() {
            b.rename(page, title);
        }
        self.refresh_bookmarks();
    }

    // ------------------------------------------------------------ find

    pub fn show_find_bar(&self) {
        if self.document().is_none_or(|d| !d.can_find) || self.is_presentation() {
            return;
        }
        self.find.bar.set_search_mode(true);
        self.set_action_state("toggle-find", &true.to_variant());
        self.find.entry.grab_focus();
        self.side_stack.set_visible_child_name("find");
        if !self.side_stack.is_visible() {
            self.side_stack.set_visible(true);
            self.side_stack.set_widget_name("find-only");
        }
        self.view.find_set_highlight(true);
    }

    pub fn close_find_bar(&self) {
        if self.find.bar.is_search_mode() {
            self.find.bar.set_search_mode(false);
        }
        self.set_action_state("toggle-find", &false.to_variant());
        self.view.find_set_highlight(false);
        self.view.find_cancel();
        self.find_sidebar.clear();
        self.find.status.set_text("");
        self.side_stack.set_visible_child_name("sidebar");
        if self.side_stack.widget_name() == "find-only" {
            self.side_stack.set_widget_name("");
            self.side_stack.set_visible(false);
        }
        self.view.grab_focus();
    }

    pub fn find_restart(&self) {
        if !self.document().is_some_and(|d| d.can_find) {
            return;
        }
        let text = self.find.entry.text().to_string();
        self.find_sidebar.clear();
        self.view.find_start(&text, self.find.options());
        self.find_updated();
    }

    fn find_updated(&self) {
        let (done, total) = self.view.find_progress();
        let n = self.view.find_n_results();
        let text = self.find.entry.text();
        let status = if text.is_empty() {
            String::new()
        } else if done < total {
            format!("{}% remaining to search", ((total - done) * 100) / total.max(1))
        } else if n == 0 {
            "No results found".into()
        } else if n == 1 {
            "1 result".into()
        } else {
            format!("{n} results")
        };
        self.find.status.set_text(&status);
        if n == 0 && done >= total && !text.is_empty() {
            self.find.entry.add_css_class("error");
        } else {
            self.find.entry.remove_css_class("error");
        }
        self.find_sidebar.update(self);
        self.set_action_enabled("find-next", n > 0);
        self.set_action_enabled("find-previous", n > 0);
    }

    // ------------------------------------------------------------ history

    pub fn update_history_actions(&self) {
        let h = self.history.borrow();
        let cur = self.model.page();
        let back = h.can_go_back() || (!h.can_go_forward() && h.back_list().first().is_some_and(|p| *p != cur));
        drop(h);
        self.set_action_enabled("go-back-history", back || self.history.borrow().can_go_back());
        self.set_action_enabled("go-forward-history", self.history.borrow().can_go_forward());
    }

    pub fn history_back(&self) {
        let cur = self.model.page();
        {
            let mut h = self.history.borrow_mut();
            if !h.can_go_forward() {
                h.add_page(cur);
            }
        }
        let p = self.history.borrow_mut().go_back();
        if let Some(p) = p {
            self.history.borrow_mut().freeze();
            self.view.go_to_dest(&LinkDest::Page(p));
            self.history.borrow_mut().thaw();
        }
        self.history_pending.set(false);
        self.update_history_actions();
    }

    pub fn history_forward(&self) {
        let p = self.history.borrow_mut().go_forward();
        if let Some(p) = p {
            self.history.borrow_mut().freeze();
            self.view.go_to_dest(&LinkDest::Page(p));
            self.history.borrow_mut().thaw();
        }
        self.history_pending.set(false);
        self.update_history_actions();
    }

    // ------------------------------------------------------------ caret

    pub fn toggle_caret_navigation(&self) {
        let enabled = self.view.caret_navigation();
        if enabled || !self.app.settings.show_caret_navigation_message() {
            self.set_caret_navigation(!enabled);
            return;
        }
        let check = gtk::CheckButton::with_label("Don’t show this message again");
        let w = self.weak();
        let c = check.clone();
        let enable: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(w) = w.upgrade() {
                w.set_caret_navigation(true);
                if c.is_active() {
                    w.app.settings.set_show_caret_navigation_message(false);
                }
            }
        });
        self.show_message(
            MessageKind::Question,
            "Enable caret navigation?",
            Some(
                "Pressing F7 turns the caret navigation on or off. This feature places a moveable cursor in text pages, \
                 allowing you to move around and select text with your keyboard. Do you want to enable the caret navigation?",
            ),
            &[("_Enable", enable)],
        );
        if let Some(frame) = self.message_box.first_child() {
            if let Some(bar) = frame.first_child().and_downcast::<gtk::Box>() {
                if let Some(texts) = bar.first_child().and_then(|i| i.next_sibling()).and_downcast::<gtk::Box>() {
                    texts.append(&check);
                    check.show();
                }
            }
        }
    }

    pub fn set_caret_navigation(&self, on: bool) {
        self.view.set_caret_navigation(on);
        self.set_action_state("caret-navigation", &on.to_variant());
        if let Some(m) = self.metadata() {
            m.set_bool("caret-navigation", on);
        }
    }
}

#[derive(Clone, Copy)]
pub enum MessageKind {
    Error,
    Warning,
    Question,
    Info,
}

pub fn set_accessible_name(w: &impl IsA<gtk::Widget>, name: &str) {
    use gtk::atk::prelude::*;
    if let Some(a) = w.as_ref().accessible() {
        a.set_name(name);
    }
}
