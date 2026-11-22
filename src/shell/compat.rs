//! Small GTK 3 helpers that keep widget-building code close to the GTK 4 idioms it was written with.

use gtk::prelude::*;
use std::cell::RefCell;

pub trait CssClassExt {
    fn add_css_class(&self, name: &str);
    fn remove_css_class(&self, name: &str);
}

impl<W: IsA<gtk::Widget>> CssClassExt for W {
    fn add_css_class(&self, name: &str) {
        self.as_ref().style_context().add_class(name);
    }
    fn remove_css_class(&self, name: &str) {
        self.as_ref().style_context().remove_class(name);
    }
}

pub trait AppendExt {
    fn append(&self, child: &impl IsA<gtk::Widget>);
    fn prepend(&self, child: &impl IsA<gtk::Widget>);
}

impl<B: IsA<gtk::Box>> AppendExt for B {
    fn append(&self, child: &impl IsA<gtk::Widget>) {
        self.as_ref().pack_start(child, false, true, 0);
    }
    fn prepend(&self, child: &impl IsA<gtk::Widget>) {
        self.as_ref().pack_start(child, false, true, 0);
        self.as_ref().reorder_child(child, 0);
    }
}

pub trait SetContentExt {
    fn set_content(&self, child: Option<&impl IsA<gtk::Widget>>);
}

impl<B: IsA<gtk::Bin>> SetContentExt for B {
    fn set_content(&self, child: Option<&impl IsA<gtk::Widget>>) {
        let bin = self.as_ref();
        if let Some(old) = bin.child() {
            bin.remove(&old);
        }
        if let Some(c) = child {
            bin.add(c);
        }
    }
}

pub trait ChildrenExt {
    fn first_child(&self) -> Option<gtk::Widget>;
    fn last_child(&self) -> Option<gtk::Widget>;
    fn next_sibling(&self) -> Option<gtk::Widget>;
}

impl<W: IsA<gtk::Widget>> ChildrenExt for W {
    fn first_child(&self) -> Option<gtk::Widget> {
        self.as_ref().downcast_ref::<gtk::Container>()?.children().into_iter().next()
    }
    fn last_child(&self) -> Option<gtk::Widget> {
        self.as_ref().downcast_ref::<gtk::Container>()?.children().into_iter().last()
    }
    fn next_sibling(&self) -> Option<gtk::Widget> {
        let me = self.as_ref();
        let parent = me.parent().and_downcast::<gtk::Container>()?;
        let kids = parent.children();
        let i = kids.iter().position(|k| k == me)?;
        kids.get(i + 1).cloned()
    }
}

pub trait IconButtonExt {
    fn set_icon_name(&self, name: &str);
}

impl<B: IsA<gtk::Button>> IconButtonExt for B {
    fn set_icon_name(&self, name: &str) {
        let img = gtk::Image::from_icon_name(Some(name), gtk::IconSize::Button);
        self.as_ref().set_image(Some(&img));
        self.as_ref().set_always_show_image(true);
    }
}

pub fn icon_button(name: &str) -> gtk::Button {
    gtk::Button::from_icon_name(Some(name), gtk::IconSize::Button)
}

pub fn icon_image(name: &str) -> gtk::Image {
    gtk::Image::from_icon_name(Some(name), gtk::IconSize::Button)
}

pub fn scrolled_window() -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE)
}

/// Hides a widget that is built visible, and keeps a later `show_all` from revealing it.
pub fn hide_initially(w: &impl IsA<gtk::Widget>) {
    let w = w.as_ref();
    w.show_all();
    w.set_visible(false);
    w.set_no_show_all(true);
}

/// Destroys a toplevel-like widget (popover, window) that GTK keeps alive on its own.
pub fn destroy(w: &impl IsA<gtk::Widget>) {
    unsafe { w.as_ref().destroy() };
}

/// A popover menu at (x, y) of `parent`, destroyed once it closes.
pub fn popup_menu_at(parent: &impl IsA<gtk::Widget>, model: &gio::MenuModel, x: f64, y: f64) {
    let pop = gtk::Popover::from_model(Some(parent), model);
    pop.set_position(gtk::PositionType::Bottom);
    pop.set_pointing_to(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1));
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || destroy(&p));
    });
    pop.popup();
}

/// A native file chooser; `done` receives the chosen files (empty on cancel).
pub fn choose_files(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    action: gtk::FileChooserAction,
    accept: &str,
    setup: impl FnOnce(&gtk::FileChooserNative),
    done: impl Fn(Vec<gio::File>) + 'static,
) {
    let dlg = gtk::FileChooserNative::new(Some(title), Some(parent), action, Some(accept), Some("_Cancel"));
    dlg.set_modal(true);
    setup(&dlg);
    let keep = RefCell::new(Some(dlg.clone()));
    dlg.connect_response(move |d, r| {
        let files = if r == gtk::ResponseType::Accept { d.files() } else { Vec::new() };
        keep.borrow_mut().take();
        done(files);
    });
    dlg.show();
}

/// A modal question; `done` receives the index of the chosen button, or None if dismissed.
pub fn choose(
    parent: &impl IsA<gtk::Window>,
    message: &str,
    detail: &str,
    buttons: &[&str],
    default: usize,
    done: impl Fn(Option<usize>) + 'static,
) {
    let dlg = gtk::MessageDialog::new(
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        gtk::MessageType::Question,
        gtk::ButtonsType::None,
        message,
    );
    dlg.set_secondary_text(Some(detail));
    for (i, b) in buttons.iter().enumerate() {
        dlg.add_button(b, gtk::ResponseType::Other(i as u16));
    }
    dlg.set_default_response(gtk::ResponseType::Other(default as u16));
    dlg.connect_response(move |d, r| {
        let choice = match r {
            gtk::ResponseType::Other(i) => Some(i as usize),
            _ => None,
        };
        d.close();
        destroy(d);
        done(choice);
    });
    dlg.show_all();
}

/// Opens a URI with the default handler for its scheme.
pub fn launch_uri(parent: &impl IsA<gtk::Window>, uri: &str) -> Result<(), glib::Error> {
    gtk::show_uri_on_window(Some(parent), uri, gtk::current_event_time())
}

/// Shows a file in the file manager, selecting it when the file manager supports that.
pub fn open_containing_folder(parent: &impl IsA<gtk::Window>, file: &gio::File) {
    let uri = file.uri().to_string();
    let parent_uri = file.parent().map(|p| p.uri().to_string());
    let win = parent.as_ref().clone();
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, move |conn| {
        let fallback = move || {
            if let Some(p) = &parent_uri {
                let _ = launch_uri(&win, p);
            }
        };
        let Ok(conn) = conn else { return fallback() };
        conn.call(
            Some("org.freedesktop.FileManager1"),
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1",
            "ShowItems",
            Some(&(vec![uri], "").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            5000,
            gio::Cancellable::NONE,
            move |r| {
                if r.is_err() {
                    fallback();
                }
            },
        );
    });
}
