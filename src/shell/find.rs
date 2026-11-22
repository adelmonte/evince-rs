use super::compat::*;
use super::window::Window;
use crate::document::{FindMatch, FindOptions};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// Search bar shown under the header bar (part of EvToolbar/EvSearchBox in GTK3 evince).
pub struct FindBar {
    pub bar: gtk::SearchBar,
    pub entry: gtk::SearchEntry,
    pub case_sensitive: Rc<Cell<bool>>,
    pub whole_words: Rc<Cell<bool>>,
    pub prev: gtk::Button,
    pub next: gtk::Button,
    pub status: gtk::Label,
}

impl FindBar {
    pub fn new(win: Weak<Window>) -> FindBar {
        let entry = gtk::SearchEntry::new();
        entry.set_width_chars(32);
        entry.set_placeholder_text(Some("Find in document…"));
        let prev = icon_button("go-up-symbolic");
        prev.set_tooltip_text(Some("Find previous occurrence of the search string"));
        prev.set_action_name(Some("win.find-previous"));
        let next = icon_button("go-down-symbolic");
        next.set_tooltip_text(Some("Find next occurrence of the search string"));
        next.set_action_name(Some("win.find-next"));
        let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        nav.add_css_class("linked");
        nav.append(&prev);
        nav.append(&next);
        let case_sensitive = Rc::new(Cell::new(false));
        let whole_words = Rc::new(Cell::new(false));
        let opts = gtk::Popover::new(gtk::Widget::NONE);
        let ob = gtk::Box::new(gtk::Orientation::Vertical, 6);
        ob.set_border_width(6);
        let cs = gtk::CheckButton::with_mnemonic("_Case Sensitive");
        let ww = gtk::CheckButton::with_mnemonic("_Whole Words Only");
        ob.append(&cs);
        ob.append(&ww);
        opts.set_content(Some(&ob));
        ob.show_all();
        let opt_button = gtk::MenuButton::new();
        opt_button.set_icon_name("view-more-symbolic");
        opt_button.set_tooltip_text(Some("Search options"));
        opt_button.set_popover(Some(&opts));
        let status = gtk::Label::new(None);
        status.add_css_class("dim-label");
        status.add_css_class("tnum");
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        hbox.append(&entry);
        hbox.append(&nav);
        hbox.append(&opt_button);
        hbox.append(&status);
        let bar = gtk::SearchBar::new();
        bar.set_content(Some(&hbox));
        hbox.show_all();
        bar.connect_entry(&entry);
        bar.set_show_close_button(true);
        let restart = {
            let win = win.clone();
            Rc::new(move || {
                if let Some(w) = win.upgrade() {
                    w.find_restart();
                }
            })
        };
        let r = restart.clone();
        let c = case_sensitive.clone();
        cs.connect_toggled(move |b| {
            c.set(b.is_active());
            r();
        });
        let r = restart.clone();
        let c = whole_words.clone();
        ww.connect_toggled(move |b| {
            c.set(b.is_active());
            r();
        });
        let r = restart.clone();
        entry.connect_search_changed(move |_| r());
        let w = win.clone();
        entry.connect_activate(move |_| {
            if let Some(w) = w.upgrade() {
                w.view.find_next();
            }
        });
        let w = win.clone();
        entry.connect_next_match(move |_| {
            if let Some(w) = w.upgrade() {
                w.view.find_next();
            }
        });
        let w = win.clone();
        entry.connect_previous_match(move |_| {
            if let Some(w) = w.upgrade() {
                w.view.find_previous();
            }
        });
        let w = win.clone();
        entry.connect_stop_search(move |_| {
            if let Some(w) = w.upgrade() {
                w.close_find_bar();
            }
        });
        let w = win.clone();
        entry.connect_key_press_event(move |_, e| {
            let k = e.keyval();
            let enter = k == gtk::gdk::keys::constants::Return || k == gtk::gdk::keys::constants::KP_Enter;
            if enter && e.state().contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                if let Some(w) = w.upgrade() {
                    w.view.find_previous();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        let w = win.clone();
        bar.connect_search_mode_enabled_notify(move |b| {
            if !b.is_search_mode() {
                if let Some(w) = w.upgrade() {
                    w.close_find_bar();
                }
            }
        });
        FindBar { bar, entry, case_sensitive, whole_words, prev, next, status }
    }

    pub fn options(&self) -> FindOptions {
        FindOptions { case_sensitive: self.case_sensitive.get(), whole_words: self.whole_words.get() }
    }
}

#[derive(Clone)]
struct ResultRow {
    page: usize,
    index: usize,
    label: String,
    markup: String,
}

/// Results list shown in place of the sidebar during a search (EvFindSidebar).
pub struct FindSidebar {
    pub widget: gtk::ScrolledWindow,
    list: gtk::ListBox,
    rows: Rc<RefCell<Vec<ResultRow>>>,
    syncing: Rc<Cell<bool>>,
}

impl FindSidebar {
    pub fn new(win: Weak<Window>) -> FindSidebar {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        let rows: Rc<RefCell<Vec<ResultRow>>> = Rc::default();
        let syncing = Rc::new(Cell::new(false));
        let w = win.clone();
        let sy = syncing.clone();
        let rs = rows.clone();
        list.connect_row_activated(move |_, row| {
            if sy.get() {
                return;
            }
            let r = rs.borrow().get(row.index() as usize).cloned();
            if let (Some(r), Some(w)) = (r, w.upgrade()) {
                w.view.find_select(r.page, r.index);
            }
        });
        let sw = scrolled_window();
        sw.set_policy(gtk::PolicyType::External, gtk::PolicyType::Automatic);
        sw.set_vexpand(true);
        sw.set_content(Some(&list));
        list.set_adjustment(Some(&sw.vadjustment()));
        sw.show_all();
        FindSidebar { widget: sw, list, rows, syncing }
    }

    pub fn clear(&self) {
        for c in self.list.children() {
            self.list.remove(&c);
        }
        self.rows.borrow_mut().clear();
    }

    fn row_widget(r: &ResultRow) -> gtk::Widget {
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.set_margin_start(6);
        hbox.set_margin_end(6);
        let text = gtk::Label::new(None);
        text.set_xalign(0.0);
        text.set_hexpand(true);
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text.set_markup(&r.markup);
        let page = gtk::Label::new(Some(&r.label));
        page.add_css_class("dim-label");
        page.add_css_class("tnum");
        hbox.append(&text);
        hbox.append(&page);
        hbox.show_all();
        hbox.upcast()
    }

    pub fn update(&self, win: &Window) {
        let Some(doc) = win.document() else { return };
        let results: Vec<(usize, Vec<FindMatch>)> = win.view.find_results();
        let n: usize = results.iter().map(|(_, m)| m.len()).sum();
        if n != self.rows.borrow().len() {
            let mut rows = Vec::new();
            for (page, matches) in &results {
                for (i, m) in matches.iter().enumerate() {
                    let markup = win
                        .view
                        .find_context(*page, m)
                        .unwrap_or_else(|| format!("<b>{}</b>", glib::markup_escape_text(&win.view.find_text())));
                    rows.push(ResultRow { page: *page, index: i, label: doc.label(*page), markup });
                }
            }
            self.clear();
            for r in &rows {
                self.list.add(&Self::row_widget(r));
            }
            *self.rows.borrow_mut() = rows;
        }
        if let Some((p, i)) = win.view.find_current() {
            let k = self.rows.borrow().iter().position(|r| r.page == p && r.index == i);
            if let Some(row) = k.and_then(|k| self.list.row_at_index(k as i32)) {
                self.syncing.set(true);
                self.list.select_row(Some(&row));
                self.syncing.set(false);
                if self.list.is_mapped() {
                    let a = row.allocation();
                    self.widget.vadjustment().clamp_page(a.y() as f64, (a.y() + a.height()) as f64);
                }
            }
        }
    }
}
