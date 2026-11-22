use crate::document::{misc, DocInfo, FontInfo};
use crate::view::DocHandle;
use super::compat::*;
use gtk::prelude::*;
use std::rc::Rc;

fn tolerance(size: f64) -> f64 {
    if size < 150.0 {
        1.5
    } else if size <= 600.0 {
        2.0
    } else {
        3.0
    }
}

fn uses_inches() -> bool {
    let lc = std::env::var("LC_MEASUREMENT")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    ["en_US", "en_LR", "my_MM"].iter().any(|l| lc.starts_with(l))
}

pub fn regular_paper_size(w: f64, h: f64) -> String {
    let exact = if uses_inches() {
        format!("{:.2} × {:.2} inch", w / 25.4, h / 25.4)
    } else {
        format!("{w:.0} × {h:.0} mm")
    };
    let mut found = None;
    for size in gtk::PaperSize::paper_sizes(false) {
        let pw = size.width(gtk::Unit::Mm);
        let ph = size.height(gtk::Unit::Mm);
        let (wt, ht) = (tolerance(pw), tolerance(ph));
        if (h - ph).abs() <= ht && (w - pw).abs() <= wt {
            found = Some(format!("{}, Portrait ({exact})", size.display_name().unwrap_or_default()));
        } else if (w - ph).abs() <= ht && (h - pw).abs() <= wt {
            found = Some(format!("{}, Landscape ({exact})", size.display_name().unwrap_or_default()));
        }
    }
    found.unwrap_or(exact)
}

fn yes_no(v: bool) -> &'static str {
    if v { "Yes" } else { "No" }
}

fn add_row(grid: &gtk::Grid, row: &mut i32, label: &str, value: Option<&str>) {
    let l = gtk::Label::new(None);
    l.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(label)));
    l.set_xalign(1.0);
    l.set_yalign(0.0);
    let v = gtk::Label::new(None);
    match value.filter(|s| !s.is_empty()) {
        Some(t) => v.set_text(t),
        None => v.set_markup("<i>None</i>"),
    }
    v.set_xalign(0.0);
    v.set_yalign(0.0);
    v.set_selectable(true);
    v.set_line_wrap(true);
    v.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    v.set_max_width_chars(60);
    v.set_hexpand(true);
    grid.attach(&l, 0, *row, 1, 1);
    grid.attach(&v, 1, *row, 1, 1);
    *row += 1;
}

fn general_page(doc: &DocHandle, info: &DocInfo) -> gtk::Widget {
    let grid = gtk::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(6);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let mut row = 0;
    add_row(&grid, &mut row, "Title:", info.title.as_deref());
    let location = gio::File::for_uri(&doc.uri)
        .parse_name()
        .to_string();
    add_row(&grid, &mut row, "Location:", Some(&location));
    add_row(&grid, &mut row, "Subject:", info.subject.as_deref());
    add_row(&grid, &mut row, "Author:", info.author.as_deref());
    add_row(&grid, &mut row, "Keywords:", info.keywords.as_deref());
    add_row(&grid, &mut row, "Producer:", info.producer.as_deref());
    add_row(&grid, &mut row, "Creator:", info.creator.as_deref());
    add_row(&grid, &mut row, "Created:", info.creation_date.map(misc::format_date).as_deref());
    add_row(&grid, &mut row, "Modified:", info.modified_date.map(misc::format_date).as_deref());
    if let Some(f) = &info.format {
        add_row(&grid, &mut row, "Format:", Some(f));
    }
    add_row(&grid, &mut row, "Number of Pages:", Some(&doc.n_pages.to_string()));
    if let Some(l) = info.linearized {
        add_row(&grid, &mut row, "Optimized:", Some(yes_no(l)));
    }
    if let Some(s) = info.security {
        add_row(&grid, &mut row, "Security:", Some(yes_no(s)));
    }
    if let Some((w, h)) = info.paper_size {
        add_row(&grid, &mut row, "Paper Size:", Some(&regular_paper_size(w, h)));
    }
    if info.format.as_deref().is_some_and(|f| f.starts_with("PDF")) {
        add_row(&grid, &mut row, "Contains Javascript:", Some(yes_no(info.contains_js)));
    }
    if let Ok(md) = std::fs::metadata(&doc.path) {
        add_row(&grid, &mut row, "Size:", Some(&glib::format_size(md.len())));
    }
    let sw = scrolled_window();
    sw.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    sw.set_propagate_natural_height(true);
    sw.set_content(Some(&grid));
    sw.upcast()
}

fn fonts_page(doc: &Rc<DocHandle>) -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    let sw = scrolled_window();
    sw.set_vexpand(true);
    sw.set_content(Some(&list));
    let progress = gtk::Label::new(Some("Gathering font information…   0%"));
    progress.set_xalign(0.0);
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 6);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    vbox.append(&sw);
    vbox.append(&progress);
    doc.jobs.run(
        crate::view::Priority::Low,
        |d| d.fonts(),
        move |fonts: Vec<FontInfo>| {
            for f in fonts {
                let l = gtk::Label::new(None);
                l.set_xalign(0.0);
                l.set_margin_top(4);
                l.set_margin_bottom(4);
                l.set_margin_start(6);
                l.set_line_wrap(true);
                l.set_markup(&format!(
                    "<b><big>{}</big></b>\n<small>{}</small>",
                    glib::markup_escape_text(&f.name),
                    glib::markup_escape_text(&f.details)
                ));
                l.show();
                list.add(&l);
            }
            progress.set_text("Gathering font information… 100%");
            progress.set_visible(false);
        },
    );
    vbox.upcast()
}

fn license_page(info: &DocInfo) -> Option<gtk::Widget> {
    let lic = info.license.as_ref()?;
    if lic.text.is_none() && lic.uri.is_none() && lic.web_statement.is_none() {
        return None;
    }
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);
    let section = |title: &str, w: &gtk::Widget| {
        let l = gtk::Label::new(None);
        l.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(title)));
        l.set_xalign(0.0);
        vbox.append(&l);
        w.set_margin_start(12);
        vbox.append(w);
    };
    if let Some(t) = &lic.text {
        let tv = gtk::TextView::new();
        tv.set_editable(false);
        tv.set_wrap_mode(gtk::WrapMode::Word);
        if let Some(b) = tv.buffer() {
            b.set_text(t);
        }
        let sw = scrolled_window();
        sw.set_min_content_height(150);
        sw.set_content(Some(&tv));
        sw.set_vexpand(true);
        section("Text License", sw.upcast_ref());
    }
    let link = |u: &str| -> gtk::Widget {
        if glib::uri_parse_scheme(u).is_some() {
            let l = gtk::Label::new(None);
            l.set_markup(&format!("<a href=\"{0}\">{0}</a>", glib::markup_escape_text(u)));
            l.set_xalign(0.0);
            l.set_selectable(true);
            l.upcast()
        } else {
            let l = gtk::Label::new(Some(u));
            l.set_xalign(0.0);
            l.set_selectable(true);
            l.upcast()
        }
    };
    if let Some(u) = &lic.uri {
        section("Usage terms", &link(u));
    }
    if let Some(u) = &lic.web_statement {
        section("Further Information", &link(u));
    }
    Some(vbox.upcast())
}

pub fn show(parent: &gtk::Window, doc: &Rc<DocHandle>) -> gtk::Window {
    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.set_title("Properties");
    win.set_transient_for(Some(parent));
    win.set_destroy_with_parent(true);
    win.set_default_size(-1, 400);
    let nb = gtk::Notebook::new();
    nb.append_page(&general_page(doc, &doc.info), Some(&gtk::Label::new(Some("General"))));
    if doc.mime.contains("pdf") || doc.mime.contains("postscript") || doc.mime.contains("dvi") {
        nb.append_page(&fonts_page(doc), Some(&gtk::Label::new(Some("Fonts"))));
    }
    if let Some(p) = license_page(&doc.info) {
        nb.append_page(&p, Some(&gtk::Label::new(Some("Document License"))));
    }
    win.set_content(Some(&nb));
    win.connect_key_press_event(|w, e| {
        if e.keyval() == gtk::gdk::keys::constants::Escape {
            w.close();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    win.show_all();
    win.present();
    win
}
