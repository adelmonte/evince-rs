use super::bookmarks::Bookmark;
use super::compat::*;
use super::window::Window;
use crate::document::*;
use crate::view::{DocHandle, Priority};
use gtk::prelude::*;
use gtk::{gdk, gio};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

pub const THUMBNAILS: &str = "thumbnails";
pub const LINKS: &str = "links";
pub const ANNOTATIONS: &str = "annotations";
pub const BOOKMARKS: &str = "bookmarks";
pub const ATTACHMENTS: &str = "attachments";
pub const LAYERS: &str = "layers";

const THUMB_WIDTH: f64 = 80.0;

fn on_right_click(widget: &impl IsA<gtk::Widget>, f: impl Fn(f64, f64) + 'static) {
    widget.as_ref().connect_button_press_event(move |_, e| {
        if e.button() == 3 && e.event_type() == gdk::EventType::ButtonPress {
            let (x, y) = e.position();
            f(x, y);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
}

fn menu_from_ui(id: &str) -> Option<gio::MenuModel> {
    gtk::Builder::from_string(include_str!("menus.ui")).object::<gio::MenuModel>(id)
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let sw = scrolled_window();
    sw.set_policy(gtk::PolicyType::External, gtk::PolicyType::Automatic);
    sw.set_vexpand(true);
    sw.set_content(Some(child));
    sw
}

/// Lets a row of buttons be clipped instead of forcing the sidebar wider.
fn clip(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    let sw = scrolled_window();
    sw.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
    sw.set_propagate_natural_width(true);
    sw.set_propagate_natural_height(true);
    sw.set_content(Some(child));
    sw
}

// ------------------------------------------------------------------ thumbnails

const TH_SURFACE: u32 = 0;
const TH_LABEL: u32 = 1;

pub struct Thumbnails {
    pub widget: gtk::ScrolledWindow,
    icons: gtk::IconView,
    store: gtk::ListStore,
    cache: RefCell<HashMap<usize, cairo::ImageSurface>>,
    placeholders: RefCell<HashMap<(i32, i32, i32), cairo::ImageSurface>>,
    pending: RefCell<HashMap<usize, crate::view::JobHandle>>,
    doc: RefCell<Option<Rc<DocHandle>>>,
    rotation: Cell<i32>,
    inverted: Cell<bool>,
    syncing: Cell<bool>,
    me: RefCell<Weak<Thumbnails>>,
}

fn invert_surface(s: &cairo::ImageSurface) {
    if let Ok(cr) = cairo::Context::new(s) {
        cr.set_operator(cairo::Operator::Difference);
        cr.set_source_rgb(1.0, 1.0, 1.0);
        let _ = cr.paint();
    }
}

fn frame_surface(s: &cairo::ImageSurface, sf: f64) {
    if let Ok(cr) = cairo::Context::new(s) {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
        cr.set_line_width(sf);
        cr.rectangle(sf / 2.0, sf / 2.0, s.width() as f64 - sf, s.height() as f64 - sf);
        let _ = cr.stroke();
    }
}

impl Thumbnails {
    fn new(win: Weak<Window>) -> Rc<Thumbnails> {
        let store = gtk::ListStore::new(&[cairo::Surface::static_type(), String::static_type()]);
        let icons = gtk::IconView::with_model(&store);
        icons.set_selection_mode(gtk::SelectionMode::Single);
        icons.set_item_orientation(gtk::Orientation::Vertical);
        icons.set_item_padding(4);
        icons.set_row_spacing(2);
        icons.set_column_spacing(2);
        IconViewExt::set_margin(&icons, 4);
        icons.set_columns(-1);
        icons.set_item_width(THUMB_WIDTH as i32);
        let pix = gtk::CellRendererPixbuf::new();
        icons.pack_start(&pix, false);
        icons.add_attribute(&pix, "surface", TH_SURFACE as i32);
        let text = gtk::CellRendererText::new();
        text.set_xalign(0.5);
        icons.pack_start(&text, false);
        icons.add_attribute(&text, "markup", TH_LABEL as i32);
        let th = Rc::new(Thumbnails {
            widget: scrolled(&icons),
            icons: icons.clone(),
            store,
            cache: RefCell::default(),
            placeholders: RefCell::default(),
            pending: RefCell::default(),
            doc: RefCell::default(),
            rotation: Cell::new(0),
            inverted: Cell::new(false),
            syncing: Cell::new(false),
            me: RefCell::default(),
        });
        *th.me.borrow_mut() = Rc::downgrade(&th);
        let tw = Rc::downgrade(&th);
        icons.connect_selection_changed(move |iv| {
            let Some(th) = tw.upgrade() else { return };
            if th.syncing.get() {
                return;
            }
            if let Some(p) = iv.selected_items().first().and_then(|p| p.indices().first().copied()) {
                if let Some(win) = win.upgrade() {
                    win.go_to_page(p as usize);
                }
            }
        });
        let tw = Rc::downgrade(&th);
        th.widget.vadjustment().connect_value_changed(move |_| {
            if let Some(th) = tw.upgrade() {
                th.update_visible();
            }
        });
        let tw = Rc::downgrade(&th);
        icons.connect_size_allocate(move |_, _| {
            if let Some(th) = tw.upgrade() {
                let tw = Rc::downgrade(&th);
                glib::idle_add_local_once(move || {
                    if let Some(th) = tw.upgrade() {
                        th.update_visible();
                    }
                });
            }
        });
        th
    }

    fn scale_factor(&self) -> f64 {
        self.icons.scale_factor().max(1) as f64
    }

    fn placeholder(&self, w: i32, h: i32) -> cairo::Surface {
        let sf = self.scale_factor() as i32;
        let key = (w, h, sf);
        if let Some(s) = self.placeholders.borrow().get(&key) {
            return (**s).clone();
        }
        let s = cairo::ImageSurface::create(cairo::Format::ARgb32, (w * sf).max(1), (h * sf).max(1)).expect("surface");
        if let Ok(cr) = cairo::Context::new(&s) {
            cr.set_source_rgb(1.0, 1.0, 1.0);
            let _ = cr.paint();
        }
        frame_surface(&s, sf as f64);
        s.set_device_scale(sf as f64, sf as f64);
        self.placeholders.borrow_mut().insert(key, s.clone());
        (*s).clone()
    }

    fn thumb_size(&self, d: &DocHandle, page: usize) -> (i32, i32) {
        let (pw, ph) = d.rotated_size(page, self.rotation.get());
        let scale = THUMB_WIDTH / pw.max(1.0);
        (THUMB_WIDTH as i32, (ph * scale).round().max(1.0) as i32)
    }

    fn set_document(&self, doc: Option<&Rc<DocHandle>>, rotation: i32, inverted: bool) {
        for (_, j) in self.pending.borrow_mut().drain() {
            j.cancel();
        }
        self.cache.borrow_mut().clear();
        *self.doc.borrow_mut() = doc.cloned();
        self.rotation.set(rotation);
        self.inverted.set(inverted);
        self.syncing.set(true);
        self.store.clear();
        if let Some(d) = doc {
            for p in 0..d.n_pages {
                let (w, h) = self.thumb_size(d, p);
                let label = glib::markup_escape_text(&d.label(p)).to_string();
                self.store.insert_with_values(None, &[(TH_SURFACE, &self.placeholder(w, h)), (TH_LABEL, &label)]);
            }
        }
        self.syncing.set(false);
        let me = self.me.borrow().clone();
        glib::idle_add_local_once(move || {
            if let Some(th) = me.upgrade() {
                th.update_visible();
            }
        });
    }

    fn update_visible(&self) {
        let Some(d) = self.doc.borrow().clone() else { return };
        let Some((a, b)) = self.icons.visible_range() else { return };
        let (Some(&a), Some(&b)) = (a.indices().first(), b.indices().first()) else { return };
        let lo = (a as usize).saturating_sub(2);
        let hi = (b as usize + 2).min(d.n_pages.saturating_sub(1));
        for page in lo..=hi {
            self.request(&d, page);
        }
    }

    fn request(&self, d: &Rc<DocHandle>, page: usize) {
        if self.cache.borrow().contains_key(&page) || self.pending.borrow().contains_key(&page) {
            return;
        }
        let rot = self.rotation.get();
        let sf = self.scale_factor();
        let (uw, uh) = d.page_size(page);
        let rscale = THUMB_WIDTH / if rot == 90 || rot == 270 { uh } else { uw };
        let me = self.me.borrow().clone();
        let inverted = self.inverted.get();
        let doc = Rc::downgrade(d);
        let job = d.jobs.run(
            Priority::Low,
            move |doc| doc.render(page, rscale * sf, rot).and_then(Bitmap::from_surface),
            move |bm| {
                let Some(th) = me.upgrade() else { return };
                th.pending.borrow_mut().remove(&page);
                let same_doc = th.doc.borrow().as_ref().zip(doc.upgrade()).is_some_and(|(a, b)| Rc::ptr_eq(a, &b));
                if !same_doc {
                    return;
                }
                let Some(s) = bm.and_then(|bm| bm.to_surface()) else { return };
                if inverted {
                    invert_surface(&s);
                }
                frame_surface(&s, sf);
                s.set_device_scale(sf, sf);
                th.cache.borrow_mut().insert(page, s.clone());
                if let Some(it) = th.store.iter_nth_child(None, page as i32) {
                    let surf: cairo::Surface = (*s).clone();
                    th.store.set_value(&it, TH_SURFACE, &surf.to_value());
                }
            },
        );
        self.pending.borrow_mut().insert(page, job);
    }

    pub fn refresh(&self, rotation: i32, inverted: bool) {
        let d = self.doc.borrow().clone();
        let page = self.icons.selected_items().first().and_then(|p| p.indices().first().copied());
        self.set_document(d.as_ref(), rotation, inverted);
        if let Some(p) = page {
            self.select_page(p as usize);
        }
    }

    pub fn invalidate(&self, page: usize) {
        self.cache.borrow_mut().remove(&page);
        if let Some(j) = self.pending.borrow_mut().remove(&page) {
            j.cancel();
        }
        if let Some(d) = self.doc.borrow().clone() {
            if page < d.n_pages {
                self.request(&d, page);
            }
        }
    }

    pub fn select_page(&self, page: usize) {
        if self.store.iter_nth_child(None, page as i32).is_none() {
            return;
        }
        let path = gtk::TreePath::from_indicesv(&[page as i32]);
        self.syncing.set(true);
        self.icons.unselect_all();
        self.icons.select_path(&path);
        self.syncing.set(false);
        if self.widget.is_mapped() {
            self.icons.scroll_to_path(&path, false, 0.0, 0.0);
        }
    }
}

// ------------------------------------------------------------------ outline

const OL_TITLE: u32 = 0;
const OL_LABEL: u32 = 1;
const OL_INDEX: u32 = 2;

struct OutlineNode {
    item: OutlineItem,
    path: String,
    page: Option<usize>,
}

pub struct Outline {
    pub widget: gtk::Box,
    tree: gtk::TreeView,
    store: gtk::TreeStore,
    nodes: RefCell<Vec<OutlineNode>>,
    search: gtk::SearchEntry,
    pub has_items: Cell<bool>,
    syncing: Cell<bool>,
    loading: Cell<bool>,
    menu_row: RefCell<Option<gtk::TreePath>>,
}

fn path_in(index: &str, path: &str) -> bool {
    index.contains(&format!("|{path}|"))
}

fn add_path(index: &str, path: &str) -> String {
    if path_in(index, path) {
        return index.to_string();
    }
    if index.is_empty() { format!("|{path}|") } else { format!("{index}{path}|") }
}

fn remove_path(index: &str, path: &str) -> String {
    let r = index.replace(&format!("|{path}|"), "|");
    if r == "|" { String::new() } else { r }
}

fn remove_descendants(index: &str, path: &str) -> String {
    let parts: Vec<&str> = index.split('|').filter(|s| !s.is_empty()).collect();
    let prefix = format!("{path}:");
    let kept: Vec<&str> = parts.into_iter().filter(|p| !p.starts_with(&prefix)).collect();
    if kept.is_empty() { String::new() } else { format!("|{}|", kept.join("|")) }
}

impl Outline {
    fn new(win: Weak<Window>) -> Rc<Outline> {
        let store = gtk::TreeStore::new(&[String::static_type(), String::static_type(), u32::static_type()]);
        let tree = gtk::TreeView::with_model(&store);
        tree.set_headers_visible(false);
        tree.set_activate_on_single_click(true);
        tree.set_enable_search(false);
        let col = gtk::TreeViewColumn::new();
        col.set_expand(true);
        let title = gtk::CellRendererText::new();
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        TreeViewColumnExt::pack_start(&col, &title, true);
        TreeViewColumnExt::add_attribute(&col, &title, "text", OL_TITLE as i32);
        let page = gtk::CellRendererText::new();
        page.set_xalign(1.0);
        page.set_padding(6, 0);
        page.set_property("foreground-rgba", gdk::RGBA::new(0.5, 0.5, 0.5, 1.0));
        TreeViewColumnExt::pack_end(&col, &page, false);
        TreeViewColumnExt::add_attribute(&col, &page, "text", OL_LABEL as i32);
        tree.append_column(&col);
        tree.set_tooltip_column(OL_TITLE as i32);
        let search = gtk::SearchEntry::new();
        search.set_margin_start(6);
        search.set_margin_end(6);
        search.set_margin_top(6);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&search);
        widget.append(&scrolled(&tree));
        widget.show_all();
        hide_initially(&search);
        let o = Rc::new(Outline {
            widget,
            tree: tree.clone(),
            store,
            nodes: RefCell::default(),
            search: search.clone(),
            has_items: Cell::new(false),
            syncing: Cell::new(false),
            loading: Cell::new(false),
            menu_row: RefCell::default(),
        });
        let w = win.clone();
        let ow = Rc::downgrade(&o);
        tree.connect_row_activated(move |_, path, _| {
            let Some(o) = ow.upgrade() else { return };
            let link = o.node_at(path).and_then(|i| o.nodes.borrow()[i].item.link.clone());
            if let (Some(win), Some(link)) = (w.upgrade(), link) {
                win.view.handle_link(&link);
            }
        });
        for expanded in [true, false] {
            let ow = Rc::downgrade(&o);
            let w = win.clone();
            let f = move |path: &gtk::TreePath| {
                let (Some(o), Some(win)) = (ow.upgrade(), w.upgrade()) else { return };
                if o.loading.get() {
                    return;
                }
                let Some(md) = win.metadata() else { return };
                let Some(i) = o.node_at(path) else { return };
                let (expand, path) = {
                    let n = &o.nodes.borrow()[i];
                    (n.item.expand, n.path.clone())
                };
                let mut ex = md.get_string("index-expand").unwrap_or_default();
                let mut co = md.get_string("index-collapse").unwrap_or_default();
                if expanded {
                    co = remove_path(&co, &path);
                    if !expand {
                        ex = add_path(&ex, &path);
                    }
                } else {
                    ex = remove_path(&ex, &path);
                    ex = remove_descendants(&ex, &path);
                    co = remove_descendants(&co, &path);
                    if expand {
                        co = add_path(&co, &path);
                    }
                }
                md.set_string("index-expand", &ex);
                md.set_string("index-collapse", &co);
            };
            if expanded {
                tree.connect_row_expanded(move |_, _, p| f(p));
            } else {
                tree.connect_row_collapsed(move |_, _, p| f(p));
            }
        }
        let ow = Rc::downgrade(&o);
        search.connect_search_changed(move |e| {
            if let Some(o) = ow.upgrade() {
                o.search_for(&e.text());
            }
        });
        let ow = Rc::downgrade(&o);
        search.connect_stop_search(move |e| {
            e.set_visible(false);
            if let Some(o) = ow.upgrade() {
                o.tree.grab_focus();
            }
        });
        let actions = gio::SimpleActionGroup::new();
        let ow = Rc::downgrade(&o);
        let add = |name: &str, f: Rc<dyn Fn(&Outline)>| {
            let a = gio::SimpleAction::new(name, None);
            let ow = ow.clone();
            a.connect_activate(move |_, _| {
                if let Some(o) = ow.upgrade() {
                    f(&o);
                }
            });
            actions.add_action(&a);
        };
        add("search-outline", Rc::new(|o| o.show_search()));
        add("collapse-all", Rc::new(|o| o.tree.collapse_all()));
        add("expand-all", Rc::new(|o| o.tree.expand_all()));
        add("expand-element", Rc::new(|o| {
            let row = o.menu_row.borrow().clone();
            if let Some(r) = row {
                o.tree.expand_row(&r, true);
            }
        }));
        let w = win.clone();
        add("print-section", Rc::new(move |o| {
            let row = o.menu_row.borrow().clone();
            if let (Some(r), Some(win)) = (row, w.upgrade()) {
                if let Some((a, b)) = o.section_range(&r, &win) {
                    win.print_range(a, b);
                }
            }
        }));
        o.widget.insert_action_group("links", Some(&actions));
        let ow = Rc::downgrade(&o);
        on_right_click(&tree, move |x, y| {
            let Some(o) = ow.upgrade() else { return };
            let row = o.tree.path_at_pos(x as i32, y as i32).and_then(|(p, ..)| p);
            *o.menu_row.borrow_mut() = row;
            let (wx, wy) = o.tree.convert_bin_window_to_widget_coords(x as i32, y as i32);
            if let Some(m) = menu_from_ui("links-popup") {
                popup_menu_at(&o.tree, &m, wx as f64, wy as f64);
            }
        });
        o
    }

    fn node_at(&self, path: &gtk::TreePath) -> Option<usize> {
        let it = self.store.iter(path)?;
        self.store.value(&it, OL_INDEX as i32).get::<u32>().ok().map(|i| i as usize)
    }

    fn fill(&self, parent: Option<&gtk::TreeIter>, items: &[OutlineItem], prefix: &str, doc: &DocHandle) {
        for (i, it) in items.iter().enumerate() {
            let path = if prefix.is_empty() { i.to_string() } else { format!("{prefix}:{i}") };
            let page = match it.link.as_ref().and_then(|l| l.action.as_ref()) {
                Some(LinkAction::GotoDest(d)) => match d {
                    LinkDest::Named(_) | LinkDest::PageLabel(_) => doc.resolve_dest(d).map(|(p, _)| p),
                    other => other.page(),
                },
                _ => None,
            };
            let label = page.map(|p| doc.label(p)).unwrap_or_default();
            let idx = self.nodes.borrow().len() as u32;
            let iter = self
                .store
                .insert_with_values(parent, None, &[(OL_TITLE, &it.title), (OL_LABEL, &label), (OL_INDEX, &idx)]);
            self.nodes.borrow_mut().push(OutlineNode { item: it.clone(), path: path.clone(), page });
            self.fill(Some(&iter), &it.children, &path, doc);
        }
    }

    /// Calls `f` for every row in display order, descending only where `descend` returns true.
    fn walk(&self, f: &mut dyn FnMut(&gtk::TreeIter, &gtk::TreePath) -> bool, descend: &dyn Fn(&gtk::TreePath) -> bool) {
        fn rec(
            o: &Outline,
            parent: Option<&gtk::TreeIter>,
            f: &mut dyn FnMut(&gtk::TreeIter, &gtk::TreePath) -> bool,
            descend: &dyn Fn(&gtk::TreePath) -> bool,
        ) -> bool {
            let Some(it) = o.store.iter_children(parent) else { return true };
            loop {
                let Some(path) = o.store.path(&it) else { return true };
                if !f(&it, &path) {
                    return false;
                }
                if o.store.iter_has_child(&it) && descend(&path) && !rec(o, Some(&it), f, descend) {
                    return false;
                }
                if !o.store.iter_next(&it) {
                    return true;
                }
            }
        }
        rec(self, None, f, descend);
    }

    fn set_document(self: &Rc<Self>, doc: Option<&Rc<DocHandle>>, items: Vec<OutlineItem>, win: &Rc<Window>) {
        self.has_items.set(!items.is_empty());
        self.loading.set(true);
        self.store.clear();
        self.nodes.borrow_mut().clear();
        let Some(doc) = doc else {
            self.loading.set(false);
            return;
        };
        self.fill(None, &items, "", doc);
        let (expand_idx, collapse_idx) = win
            .metadata()
            .map(|m| (m.get_string("index-expand").unwrap_or_default(), m.get_string("index-collapse").unwrap_or_default()))
            .unwrap_or_default();
        let tree = self.tree.clone();
        let mut expand = |it: &gtk::TreeIter, path: &gtk::TreePath| {
            let Ok(i) = self.store.value(it, OL_INDEX as i32).get::<u32>() else { return true };
            let (want, has_children) = {
                let nodes = self.nodes.borrow();
                let n = &nodes[i as usize];
                ((n.item.expand && !path_in(&collapse_idx, &n.path)) || path_in(&expand_idx, &n.path), !n.item.children.is_empty())
            };
            if want && has_children {
                tree.expand_row(path, false);
            }
            true
        };
        let t2 = self.tree.clone();
        self.walk(&mut expand, &|p| t2.row_expanded(p));
        self.loading.set(false);
    }

    fn section_range(&self, row: &gtk::TreePath, win: &Rc<Window>) -> Option<(usize, usize)> {
        let start_idx = self.node_at(row)?;
        let start = self.nodes.borrow()[start_idx].page?;
        let n = win.document()?.n_pages;
        let depth = row.depth();
        let mut seen = false;
        let mut found = None;
        let nodes = self.nodes.borrow();
        self.walk(
            &mut |it, path| {
                if !seen {
                    seen = path == row;
                    return true;
                }
                if path.depth() <= depth && !path.is_descendant(row) {
                    let Ok(i) = self.store.value(it, OL_INDEX as i32).get::<u32>() else { return true };
                    if let Some(p) = nodes[i as usize].page {
                        found = Some(p);
                        return false;
                    }
                }
                true
            },
            &|_| true,
        );
        Some(match found {
            Some(p) => (start, p.saturating_sub(1).max(start)),
            None => (start, n - 1),
        })
    }

    fn search_for(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        let needle = text.to_lowercase();
        let mut hit = None;
        let tree = self.tree.clone();
        let nodes = self.nodes.borrow();
        self.walk(
            &mut |it, path| {
                let Ok(i) = self.store.value(it, OL_INDEX as i32).get::<u32>() else { return true };
                if nodes[i as usize].item.title.to_lowercase().contains(&needle) {
                    hit = Some(path.clone());
                    return false;
                }
                true
            },
            &|_| true,
        );
        drop(nodes);
        if let Some(p) = hit {
            tree.expand_to_path(&p);
            self.syncing.set(true);
            tree.selection().select_path(&p);
            self.syncing.set(false);
            tree.scroll_to_cell(Some(&p), None::<&gtk::TreeViewColumn>, false, 0.0, 0.0);
        }
    }

    pub fn show_search(&self) {
        self.search.set_visible(true);
        self.search.grab_focus();
    }

    pub fn select_page(&self, page: usize) {
        let mut best = None;
        let nodes = self.nodes.borrow();
        let tree = self.tree.clone();
        self.walk(
            &mut |it, path| {
                let Ok(i) = self.store.value(it, OL_INDEX as i32).get::<u32>() else { return true };
                if nodes[i as usize].page == Some(page) {
                    best = Some(path.clone());
                    return false;
                }
                true
            },
            &|p| tree.row_expanded(p),
        );
        drop(nodes);
        self.syncing.set(true);
        match best {
            Some(p) => self.tree.selection().select_path(&p),
            None => self.tree.selection().unselect_all(),
        }
        self.syncing.set(false);
    }
}

// ------------------------------------------------------------------ annotations

#[derive(Clone)]
enum AnnotRow {
    Header(usize),
    Item(Annotation),
}

pub struct Annotations {
    pub widget: gtk::ScrolledWindow,
    list: gtk::ListBox,
    rows: Rc<RefCell<Vec<AnnotRow>>>,
    pub has_items: Cell<bool>,
}

fn annot_icon(a: &Annotation) -> &'static str {
    match &a.kind {
        AnnotKind::Text { .. } => "annotations-text-symbolic",
        AnnotKind::TextMarkup(MarkupType::Highlight) => "format-text-highlight-symbolic",
        AnnotKind::TextMarkup(MarkupType::StrikeOut) => "format-text-strikethrough-symbolic",
        AnnotKind::TextMarkup(MarkupType::Underline) => "format-text-underline-symbolic",
        AnnotKind::TextMarkup(MarkupType::Squiggly) => "annotations-squiggly-symbolic",
        AnnotKind::Attachment(_) => "mail-attachment-symbolic",
        _ => "annotations-text-symbolic",
    }
}

fn clear_container(c: &impl IsA<gtk::Container>) {
    for ch in c.children() {
        c.remove(&ch);
    }
}

impl Annotations {
    fn new(win: Weak<Window>) -> Rc<Annotations> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        let rows: Rc<RefCell<Vec<AnnotRow>>> = Rc::default();
        let r = rows.clone();
        list.connect_row_activated(move |_, row| {
            let item = r.borrow().get(row.index() as usize).cloned();
            if let (Some(AnnotRow::Item(a)), Some(win)) = (item, win.upgrade()) {
                win.show_annotation(&a);
            }
        });
        Rc::new(Annotations { widget: scrolled(&list), list, rows, has_items: Cell::new(false) })
    }

    fn row_widget(row: &AnnotRow) -> gtk::ListBoxRow {
        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.set_margin_start(6);
        hbox.set_margin_end(6);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_line_wrap(true);
        label.set_line_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_hexpand(true);
        let r = gtk::ListBoxRow::new();
        match row {
            AnnotRow::Header(p) => {
                label.set_markup(&format!("<b>Page {}</b>", p + 1));
                r.set_activatable(false);
                r.set_selectable(false);
            }
            AnnotRow::Item(a) => {
                let img = icon_image(annot_icon(a));
                img.set_valign(gtk::Align::Start);
                hbox.append(&img);
                let author = a.label.clone().unwrap_or_default();
                let text = if a.contents.is_empty() { a.kind_name().to_string() } else { a.contents.clone() };
                let snippet: String = text.chars().take(120).collect();
                label.set_markup(&format!(
                    "<span weight=\"bold\">{}</span>\n{}",
                    glib::markup_escape_text(&author),
                    glib::markup_escape_text(&snippet)
                ));
            }
        }
        hbox.append(&label);
        r.add(&hbox);
        r.show_all();
        r
    }

    pub fn load(self: &Rc<Self>, doc: Option<&Rc<DocHandle>>, on_done: impl Fn(bool) + 'static) {
        clear_container(&self.list);
        self.rows.borrow_mut().clear();
        let Some(doc) = doc else { return };
        let n = doc.n_pages;
        let me = Rc::downgrade(self);
        doc.jobs.run(
            Priority::Low,
            move |d| (0..n).map(|p| (p, d.annotations(p))).filter(|(_, a)| !a.is_empty()).collect::<Vec<_>>(),
            move |pages| {
                let Some(me) = me.upgrade() else { return };
                clear_container(&me.list);
                let mut rows = Vec::new();
                for (p, annots) in pages {
                    let shown: Vec<_> = annots.into_iter().filter(|a| a.is_markup || matches!(a.kind, AnnotKind::Attachment(_))).collect();
                    if shown.is_empty() {
                        continue;
                    }
                    rows.push(AnnotRow::Header(p));
                    rows.extend(shown.into_iter().map(AnnotRow::Item));
                }
                for r in &rows {
                    me.list.add(&Self::row_widget(r));
                }
                let any = !rows.is_empty();
                *me.rows.borrow_mut() = rows;
                me.has_items.set(any);
                on_done(any);
            },
        );
    }
}

// ------------------------------------------------------------------ bookmarks

pub struct BookmarksPane {
    pub widget: gtk::Box,
    list: gtk::ListBox,
    items: Rc<RefCell<Vec<Bookmark>>>,
    del_button: gtk::Button,
    menu_page: Rc<Cell<Option<u32>>>,
    win: Weak<Window>,
}

impl BookmarksPane {
    fn new(win: Weak<Window>) -> Rc<BookmarksPane> {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        let items: Rc<RefCell<Vec<Bookmark>>> = Rc::default();
        let w = win.clone();
        let it = items.clone();
        list.connect_row_activated(move |_, row| {
            let page = it.borrow().get(row.index() as usize).map(|b| b.page);
            if let (Some(page), Some(win)) = (page, w.upgrade()) {
                win.go_to_page(page as usize);
            }
        });
        let add = icon_button("list-add-symbolic");
        add.set_action_name(Some("win.add-bookmark"));
        add.set_tooltip_text(Some("Add bookmark"));
        let del = icon_button("list-remove-symbolic");
        del.set_tooltip_text(Some("Remove bookmark"));
        del.set_sensitive(false);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        bar.set_margin_top(4);
        bar.set_margin_bottom(4);
        bar.set_margin_start(4);
        bar.set_margin_end(4);
        bar.set_halign(gtk::Align::Center);
        for b in [&add, &del] {
            b.set_relief(gtk::ReliefStyle::None);
            bar.append(b);
        }
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&scrolled(&list));
        widget.append(&clip(&bar));
        let pane = Rc::new(BookmarksPane {
            widget,
            list: list.clone(),
            items: items.clone(),
            del_button: del.clone(),
            menu_page: Rc::default(),
            win: win.clone(),
        });
        let d2 = del.clone();
        list.connect_row_selected(move |_, r| d2.set_sensitive(r.is_some()));
        let w = win.clone();
        let pw = Rc::downgrade(&pane);
        del.connect_clicked(move |_| {
            let (Some(p), Some(win)) = (pw.upgrade(), w.upgrade()) else { return };
            let page = p.list.selected_row().and_then(|r| p.items.borrow().get(r.index() as usize).map(|b| b.page));
            if let Some(page) = page {
                win.delete_bookmark(page);
            }
        });
        let actions = gio::SimpleActionGroup::new();
        let mk = |name: &str, f: Rc<dyn Fn(&Rc<Window>, u32, &Rc<BookmarksPane>)>| {
            let a = gio::SimpleAction::new(name, None);
            let w = win.clone();
            let pw = Rc::downgrade(&pane);
            a.connect_activate(move |_, _| {
                let (Some(win), Some(p)) = (w.upgrade(), pw.upgrade()) else { return };
                if let Some(page) = p.menu_page.get() {
                    f(&win, page, &p);
                }
            });
            actions.add_action(&a);
        };
        mk("open-bookmark", Rc::new(|win, page, _| win.go_to_page(page as usize)));
        mk("delete-bookmark", Rc::new(|win, page, _| win.delete_bookmark(page)));
        mk("rename-bookmark", Rc::new(|win, page, p| p.rename_popover(win, page)));
        pane.widget.insert_action_group("bookmarks", Some(&actions));
        let pw = Rc::downgrade(&pane);
        on_right_click(&list, move |x, y| {
            let Some(p) = pw.upgrade() else { return };
            let Some(row) = p.list.row_at_y(y as i32) else { return };
            let Some(pg) = p.items.borrow().get(row.index() as usize).map(|b| b.page) else { return };
            p.list.select_row(Some(&row));
            p.menu_page.set(Some(pg));
            if let Some(m) = menu_from_ui("bookmarks-popup") {
                popup_menu_at(&p.list, &m, x, y);
            }
        });
        pane
    }

    fn rename_popover(&self, win: &Rc<Window>, page: u32) {
        let Some(idx) = self.items.borrow().iter().position(|b| b.page == page) else { return };
        let title = self.items.borrow()[idx].title.clone();
        let Some(row) = self.list.row_at_index(idx as i32) else { return };
        let pop = gtk::Popover::new(Some(&row));
        let entry = gtk::Entry::new();
        entry.set_text(&title);
        entry.set_margin_top(6);
        entry.set_margin_bottom(6);
        entry.set_margin_start(6);
        entry.set_margin_end(6);
        pop.set_content(Some(&entry));
        entry.show();
        let w = Rc::downgrade(win);
        let p = pop.clone();
        entry.connect_activate(move |e| {
            if let Some(win) = w.upgrade() {
                win.rename_bookmark(page, &e.text());
            }
            p.popdown();
        });
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || destroy(&p));
        });
        pop.popup();
        entry.grab_focus();
    }

    pub fn set_items(&self, items: &[Bookmark]) {
        clear_container(&self.list);
        let doc = self.win.upgrade().and_then(|w| w.document());
        for b in items {
            let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            hbox.set_margin_top(4);
            hbox.set_margin_bottom(4);
            hbox.set_margin_start(6);
            hbox.set_margin_end(6);
            let t = gtk::Label::new(Some(&b.title));
            t.set_xalign(0.0);
            t.set_hexpand(true);
            t.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let p = gtk::Label::new(Some(&doc.as_ref().map(|d| d.label(b.page as usize)).unwrap_or_default()));
            p.add_css_class("dim-label");
            p.add_css_class("tnum");
            hbox.append(&t);
            hbox.append(&p);
            hbox.show_all();
            self.list.add(&hbox);
        }
        *self.items.borrow_mut() = items.to_vec();
        self.del_button.set_sensitive(false);
    }
}

// ------------------------------------------------------------------ attachments

pub struct Attachments {
    pub widget: gtk::ScrolledWindow,
    flow: gtk::FlowBox,
    items: Rc<RefCell<Vec<Attachment>>>,
    pub has_items: Cell<bool>,
}

impl Attachments {
    fn new(win: Weak<Window>) -> Rc<Attachments> {
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::Single);
        flow.set_activate_on_single_click(false);
        flow.set_max_children_per_line(4);
        flow.set_valign(gtk::Align::Start);
        let items: Rc<RefCell<Vec<Attachment>>> = Rc::default();
        let w = win.clone();
        let it = items.clone();
        flow.connect_child_activated(move |_, child| {
            let a = it.borrow().get(child.index() as usize).cloned();
            if let (Some(a), Some(win)) = (a, w.upgrade()) {
                win.open_attachment(&a.name, &a.data);
            }
        });
        let pressed: Rc<Cell<Option<usize>>> = Rc::default();
        let w = win.clone();
        let it = items.clone();
        let pr = pressed.clone();
        flow.connect_button_press_event(move |f, e| {
            let (x, y) = e.position();
            let child = f.child_at_pos(x as i32, y as i32);
            pr.set(child.as_ref().map(|c| c.index() as usize));
            if e.button() != 3 {
                return glib::Propagation::Proceed;
            }
            let Some(child) = child else { return glib::Propagation::Stop };
            f.select_child(&child);
            let Some(a) = it.borrow().get(child.index() as usize).cloned() else { return glib::Propagation::Stop };
            let Some(win) = w.upgrade() else { return glib::Propagation::Stop };
            win.set_popup_attachment(Some(std::sync::Arc::new(AttachmentRef {
                name: a.name.clone(),
                description: a.description.clone(),
                data: a.data.to_vec(),
            })));
            if let Some(m) = menu_from_ui("attachments-popup") {
                popup_menu_at(f, &m, x, y);
            }
            glib::Propagation::Stop
        });
        flow.drag_source_set(
            gdk::ModifierType::BUTTON1_MASK,
            &[gtk::TargetEntry::new("text/uri-list", gtk::TargetFlags::OTHER_APP, 0)],
            gdk::DragAction::COPY,
        );
        let it = items.clone();
        flow.connect_drag_data_get(move |_, _, data, _, _| {
            let Some(i) = pressed.get() else { return };
            let Some(a) = it.borrow().get(i).cloned() else { return };
            let dir = crate::document::factory::temp_path("-attachments");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(&a.name);
            if std::fs::write(&path, &a.data[..]).is_ok() {
                let uri = gio::File::for_path(&path).uri();
                data.set_uris(&[uri.as_str()]);
            }
        });
        Rc::new(Attachments { widget: scrolled(&flow), flow, items, has_items: Cell::new(false) })
    }

    fn load(self: &Rc<Self>, doc: Option<&Rc<DocHandle>>, on_done: impl Fn(bool) + 'static) {
        clear_container(&self.flow);
        self.items.borrow_mut().clear();
        self.has_items.set(false);
        let Some(doc) = doc else { return };
        let me = Rc::downgrade(self);
        doc.jobs.run(Priority::Low, |d| d.attachments(), move |list| {
            let Some(me) = me.upgrade() else { return };
            for a in &list {
                let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
                vbox.set_margin_top(6);
                vbox.set_margin_bottom(6);
                let (ct, _) = gio::content_type_guess(Some(&a.name), &a.data[..a.data.len().min(4096)]);
                let img = gtk::Image::from_gicon(&gio::content_type_get_icon(&ct), gtk::IconSize::Dialog);
                img.set_pixel_size(48);
                let l = gtk::Label::new(Some(&a.name));
                l.set_line_wrap(true);
                l.set_line_wrap_mode(gtk::pango::WrapMode::WordChar);
                l.set_max_width_chars(14);
                l.set_justify(gtk::Justification::Center);
                vbox.append(&img);
                vbox.append(&l);
                vbox.set_tooltip_text(a.description.as_deref().or(Some(&a.name)));
                vbox.show_all();
                me.flow.insert(&vbox, -1);
            }
            let any = !list.is_empty();
            *me.items.borrow_mut() = list;
            me.has_items.set(any);
            on_done(any);
        });
    }
}

// ------------------------------------------------------------------ layers

const LY_TITLE: u32 = 0;
const LY_VISIBLE: u32 = 1;
const LY_HAS_CHECK: u32 = 2;
const LY_ID: u32 = 3;

pub struct Layers {
    pub widget: gtk::ScrolledWindow,
    tree: gtk::TreeView,
    store: gtk::TreeStore,
    pub has_items: Cell<bool>,
}

impl Layers {
    fn new(win: Weak<Window>) -> Rc<Layers> {
        let store = gtk::TreeStore::new(&[String::static_type(), bool::static_type(), bool::static_type(), u32::static_type()]);
        let tree = gtk::TreeView::with_model(&store);
        tree.set_headers_visible(false);
        tree.set_enable_search(false);
        let col = gtk::TreeViewColumn::new();
        let toggle = gtk::CellRendererToggle::new();
        TreeViewColumnExt::pack_start(&col, &toggle, false);
        TreeViewColumnExt::add_attribute(&col, &toggle, "active", LY_VISIBLE as i32);
        TreeViewColumnExt::add_attribute(&col, &toggle, "visible", LY_HAS_CHECK as i32);
        let text = gtk::CellRendererText::new();
        text.set_ellipsize(gtk::pango::EllipsizeMode::End);
        TreeViewColumnExt::pack_start(&col, &text, true);
        TreeViewColumnExt::add_attribute(&col, &text, "text", LY_TITLE as i32);
        col.set_expand(true);
        tree.append_column(&col);
        let s = store.clone();
        toggle.connect_toggled(move |_, path| {
            let Some(it) = s.iter(&path) else { return };
            let vis = !s.value(&it, LY_VISIBLE as i32).get::<bool>().unwrap_or(false);
            let id = s.value(&it, LY_ID as i32).get::<u32>().unwrap_or(0);
            s.set_value(&it, LY_VISIBLE, &vis.to_value());
            if let Some(win) = win.upgrade() {
                win.set_layer_visible(id as usize, vis);
            }
        });
        Rc::new(Layers { widget: scrolled(&tree), tree, store, has_items: Cell::new(false) })
    }

    fn set_layers(&self, layers: Vec<Layer>) {
        self.has_items.set(!layers.is_empty());
        self.store.clear();
        fn fill(store: &gtk::TreeStore, parent: Option<&gtk::TreeIter>, layers: &[Layer]) {
            for l in layers {
                let it = store.insert_with_values(
                    parent,
                    None,
                    &[(LY_TITLE, &l.title), (LY_VISIBLE, &l.visible), (LY_HAS_CHECK, &!l.title_only), (LY_ID, &(l.id as u32))],
                );
                fill(store, Some(&it), &l.children);
            }
        }
        fill(&self.store, None, &layers);
        self.tree.expand_all();
    }

    pub fn reload(&self, doc: &Rc<DocHandle>) {
        let layers = doc.jobs.with(|d| d.layers());
        self.set_layers(layers);
    }
}

// ------------------------------------------------------------------ container

pub struct Sidebar {
    pub widget: gtk::Box,
    pub stack: gtk::Stack,
    pub thumbnails: Rc<Thumbnails>,
    pub outline: Rc<Outline>,
    pub annotations: Rc<Annotations>,
    pub bookmarks: Rc<BookmarksPane>,
    pub attachments: Rc<Attachments>,
    pub layers: Rc<Layers>,
    preferred: RefCell<String>,
}

impl Sidebar {
    pub fn new(win: Weak<Window>) -> Rc<Sidebar> {
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hhomogeneous(false);
        let thumbnails = Thumbnails::new(win.clone());
        let outline = Outline::new(win.clone());
        let annotations = Annotations::new(win.clone());
        let bookmarks = BookmarksPane::new(win.clone());
        let attachments = Attachments::new(win.clone());
        let layers = Layers::new(win.clone());
        let add = |w: &gtk::Widget, name: &str, title: &str, icon: &str| {
            w.show_all();
            stack.add_titled(w, name, title);
            stack.set_child_icon_name(w, Some(icon));
        };
        add(thumbnails.widget.upcast_ref(), THUMBNAILS, "Thumbnails", "view-grid-symbolic");
        add(outline.widget.upcast_ref(), LINKS, "Outline", "outline-symbolic");
        add(annotations.widget.upcast_ref(), ANNOTATIONS, "Annotations", "annotations-text-symbolic");
        add(bookmarks.widget.upcast_ref(), BOOKMARKS, "Bookmarks", "user-bookmarks-symbolic");
        add(attachments.widget.upcast_ref(), ATTACHMENTS, "Attachments", "mail-attachment-symbolic");
        add(layers.widget.upcast_ref(), LAYERS, "Layers", "view-paged-symbolic");
        let switcher = gtk::StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_halign(gtk::Align::Center);
        switcher.set_margin_top(4);
        switcher.set_margin_bottom(4);
        switcher.set_margin_start(4);
        switcher.set_margin_end(4);
        let switcher_clip = clip(&switcher);
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.add_css_class("sidebar");
        widget.append(&stack);
        widget.append(&switcher_clip);
        widget.show_all();
        let sb = Rc::new(Sidebar {
            widget,
            stack,
            thumbnails,
            outline,
            annotations,
            bookmarks,
            attachments,
            layers,
            preferred: RefCell::new(LINKS.into()),
        });
        for name in [ANNOTATIONS, ATTACHMENTS, LAYERS] {
            sb.set_page_visible(name, false);
        }
        sb
    }

    fn set_page_visible(&self, name: &str, visible: bool) {
        if let Some(c) = self.stack.child_by_name(name) {
            c.set_visible(visible);
        }
    }

    pub fn current_page(&self) -> String {
        self.stack.visible_child_name().map(|s| s.to_string()).unwrap_or_else(|| THUMBNAILS.into())
    }

    pub fn set_current_page(&self, name: &str) {
        *self.preferred.borrow_mut() = name.to_string();
        self.apply_preferred();
    }

    fn apply_preferred(&self) {
        let name = self.preferred.borrow().clone();
        if let Some(c) = self.stack.child_by_name(&name) {
            if c.is_visible() {
                self.stack.set_visible_child(&c);
                return;
            }
        }
        if let Some(c) = self.stack.child_by_name(THUMBNAILS) {
            self.stack.set_visible_child(&c);
        }
    }

    pub fn set_document(self: &Rc<Self>, win: &Rc<Window>, doc: Option<&Rc<DocHandle>>, rotation: i32, inverted: bool) {
        self.thumbnails.set_document(doc, rotation, inverted);
        let outline = doc.map(|d| d.jobs.with(|x| x.outline())).unwrap_or_default();
        let has_outline = !outline.is_empty();
        self.outline.set_document(doc, outline, win);
        self.set_page_visible(LINKS, has_outline);
        self.set_page_visible(BOOKMARKS, doc.is_some());
        let layers = doc.map(|d| d.jobs.with(|x| x.layers())).unwrap_or_default();
        let has_layers = !layers.is_empty();
        self.layers.set_layers(layers);
        self.set_page_visible(LAYERS, has_layers);
        self.set_page_visible(ATTACHMENTS, false);
        self.set_page_visible(ANNOTATIONS, false);
        let me = Rc::downgrade(self);
        self.attachments.load(doc, move |any| {
            if let Some(s) = me.upgrade() {
                s.set_page_visible(ATTACHMENTS, any);
                if any {
                    s.apply_preferred();
                }
            }
        });
        self.reload_annotations(doc);
        self.apply_preferred();
    }

    pub fn reload_annotations(self: &Rc<Self>, doc: Option<&Rc<DocHandle>>) {
        let me = Rc::downgrade(self);
        let current = self.current_page();
        self.annotations.load(doc, move |any| {
            if let Some(s) = me.upgrade() {
                s.set_page_visible(ANNOTATIONS, any);
                if any && *s.preferred.borrow() == ANNOTATIONS || current == ANNOTATIONS {
                    s.apply_preferred();
                }
            }
        });
    }

    pub fn page_changed(&self, page: usize) {
        self.thumbnails.select_page(page);
        self.outline.select_page(page);
    }
}
