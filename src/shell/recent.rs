use crate::document::{factory, Bitmap};
use super::compat::*;
use gtk::gdk::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use gtk::{gdk, gdk_pixbuf, gio};
use std::path::PathBuf;
use std::rc::Rc;

const ICON_VIEW_SIZE: i32 = 128;
const MAX_RECENT_VIEW_ITEMS: usize = 64;

#[derive(Clone)]
struct Item {
    uri: String,
    name: String,
}

fn thumbnail_cache_path(uri: &str) -> Vec<PathBuf> {
    let sum = glib::compute_checksum_for_data(glib::ChecksumType::Md5, uri.as_bytes()).map(|s| s.to_string()).unwrap_or_default();
    let base = glib::user_cache_dir().join("thumbnails");
    ["large", "normal", "x-large"].iter().map(|d| base.join(d).join(format!("{sum}.png"))).collect()
}

pub fn recent_manager() -> gtk::RecentManager {
    gtk::RecentManager::default().expect("recent manager")
}

/// Grid of recently opened documents shown in an empty window (EvRecentView).
pub struct RecentView {
    pub widget: gtk::ScrolledWindow,
    flow: gtk::FlowBox,
    items: Rc<RefCell<Vec<Item>>>,
    cells: RefCell<Vec<(gtk::Image, gtk::Label)>>,
    generation: std::cell::Cell<u64>,
}

impl RecentView {
    pub fn new(open: impl Fn(&str) + 'static) -> Rc<RecentView> {
        let flow = gtk::FlowBox::new();
        flow.set_selection_mode(gtk::SelectionMode::None);
        flow.set_activate_on_single_click(true);
        flow.set_max_children_per_line(12);
        flow.set_valign(gtk::Align::Start);
        flow.set_homogeneous(true);
        let items: Rc<RefCell<Vec<Item>>> = Rc::default();
        let it = items.clone();
        flow.connect_child_activated(move |_, child| {
            let uri = it.borrow().get(child.index() as usize).map(|i| i.uri.clone());
            if let Some(uri) = uri {
                open(&uri);
            }
        });
        let widget = scrolled_window();
        widget.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        widget.set_vexpand(true);
        widget.set_hexpand(true);
        widget.add_css_class("content-view");
        widget.set_content(Some(&flow));
        let rv = Rc::new(RecentView { widget, flow, items, cells: RefCell::default(), generation: Default::default() });
        let w = Rc::downgrade(&rv);
        recent_manager().connect_changed(move |_| {
            if let Some(rv) = w.upgrade() {
                rv.refresh();
            }
        });
        rv.refresh();
        rv.widget.show_all();
        rv
    }

    fn cell(&self, it: &Item) -> (gtk::Widget, gtk::Image, gtk::Label) {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 4);
        vbox.set_margin_top(12);
        vbox.set_margin_bottom(12);
        vbox.set_margin_start(12);
        vbox.set_margin_end(12);
        let pic = gtk::Image::from_icon_name(Some("x-office-document"), gtk::IconSize::Dialog);
        pic.set_pixel_size(ICON_VIEW_SIZE);
        pic.set_size_request(ICON_VIEW_SIZE, ICON_VIEW_SIZE);
        let name = gtk::Label::new(Some(&it.name));
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        name.set_max_width_chars(18);
        let author = gtk::Label::new(None);
        author.add_css_class("dim-label");
        author.set_ellipsize(gtk::pango::EllipsizeMode::End);
        author.set_max_width_chars(18);
        vbox.append(&pic);
        vbox.append(&name);
        vbox.append(&author);
        vbox.set_tooltip_text(Some(&gio::File::for_uri(&it.uri).parse_name()));
        vbox.show_all();
        author.set_visible(false);
        (vbox.upcast(), pic, author)
    }

    pub fn refresh(self: &Rc<Self>) {
        for c in self.flow.children() {
            self.flow.remove(&c);
        }
        self.cells.borrow_mut().clear();
        let sf = self.widget.scale_factor().max(1);
        let app = glib::application_name().map(|s| s.to_string()).unwrap_or_default();
        let mut infos: Vec<gtk::RecentInfo> = recent_manager()
            .items()
            .into_iter()
            .filter(|i| i.has_application(&app))
            .filter(|i| !i.is_local() || i.exists())
            .collect();
        infos.sort_by_key(|i| std::cmp::Reverse(i.modified()));
        let mut jobs = Vec::new();
        let mut items = Vec::new();
        for (idx, info) in infos.into_iter().take(MAX_RECENT_VIEW_ITEMS).enumerate() {
            let Some(uri) = info.uri().map(|u| u.to_string()) else { continue };
            let item = Item { uri: uri.clone(), name: info.display_name().map(|s| s.to_string()).unwrap_or_default() };
            let (w, pic, author) = self.cell(&item);
            let mut have_thumb = false;
            for p in thumbnail_cache_path(&uri) {
                let size = ICON_VIEW_SIZE * sf;
                if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(&p, size, size, true) {
                    if let Some(s) = pb.create_surface(sf, None::<&gdk::Window>) {
                        pic.set_from_surface(Some(&s));
                        pic.add_css_class("document-page");
                        have_thumb = true;
                        break;
                    }
                }
            }
            jobs.push((idx, uri, !have_thumb));
            self.flow.insert(&w, -1);
            self.cells.borrow_mut().push((pic, author));
            items.push(item);
        }
        *self.items.borrow_mut() = items;
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let (tx, rx) = async_channel::unbounded::<(usize, Option<String>, Option<Bitmap>)>();
        std::thread::spawn(move || {
            for (idx, uri, need_thumb) in jobs {
                let r = factory::load_document(&uri, None).ok().map(|loaded| {
                    let mut doc = loaded.doc;
                    let author = doc.info().author.filter(|a| !a.trim().is_empty());
                    let bm = if need_thumb && doc.n_pages() > 0 {
                        let (w, h) = doc.page_size(0);
                        let scale = (ICON_VIEW_SIZE * sf) as f64 / w.max(h).max(1.0);
                        doc.render(0, scale, 0).and_then(Bitmap::from_surface)
                    } else {
                        None
                    };
                    (author, bm)
                });
                let (a, b) = r.unwrap_or((None, None));
                if tx.send_blocking((idx, a, b)).is_err() {
                    return;
                }
            }
        });
        let w = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok((idx, author, bm)) = rx.recv().await {
                let Some(rv) = w.upgrade() else { return };
                if rv.generation.get() != generation {
                    return;
                }
                let cells = rv.cells.borrow();
                let Some((pic, alabel)) = cells.get(idx) else { continue };
                if let Some(a) = author {
                    alabel.set_text(&a);
                    alabel.set_visible(true);
                }
                if let Some(s) = bm.and_then(|bm| bm.to_surface()) {
                    s.set_device_scale(sf as f64, sf as f64);
                    pic.set_from_surface(Some(&s));
                    pic.add_css_class("document-page");
                }
            }
        });
    }
}
