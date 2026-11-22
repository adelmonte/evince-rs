use crate::document::{AnnotKind, Annotation, Color, MarkupType, TextIcon};
use super::compat::*;
use gtk::prelude::*;

const ICONS: &[(&str, TextIcon)] = &[
    ("Note", TextIcon::Note),
    ("Comment", TextIcon::Comment),
    ("Key", TextIcon::Key),
    ("Help", TextIcon::Help),
    ("New Paragraph", TextIcon::NewParagraph),
    ("Paragraph", TextIcon::Paragraph),
    ("Insert", TextIcon::Insert),
    ("Cross", TextIcon::Cross),
    ("Circle", TextIcon::Circle),
    ("Unknown", TextIcon::Unknown),
];

const MARKUPS: &[(&str, MarkupType)] = &[
    ("Highlight", MarkupType::Highlight),
    ("Strike out", MarkupType::StrikeOut),
    ("Underline", MarkupType::Underline),
    ("Squiggly", MarkupType::Squiggly),
];

/// Annotation Properties dialog; `apply` receives the edited annotation.
pub fn show(parent: &gtk::Window, annot: &Annotation, apply: impl Fn(Annotation) + 'static) {
    let win = gtk::Window::new(gtk::WindowType::Toplevel);
    win.set_title("Annotation Properties");
    win.set_transient_for(Some(parent));
    win.set_modal(true);
    win.set_resizable(false);
    let grid = gtk::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(12);
    grid.set_margin_top(12);
    grid.set_margin_bottom(12);
    grid.set_margin_start(12);
    grid.set_margin_end(12);
    let label = |t: &str, row: i32| {
        let l = gtk::Label::new(Some(t));
        l.set_xalign(0.0);
        grid.attach(&l, 0, row, 1, 1);
    };
    label("Author:", 0);
    let author = gtk::Entry::new();
    author.set_text(annot.label.as_deref().unwrap_or(""));
    author.set_hexpand(true);
    grid.attach(&author, 1, 0, 1, 1);
    label("Color:", 1);
    let c = annot.color;
    let color = gtk::ColorButton::with_rgba(&gtk::gdk::RGBA::new(c.r, c.g, c.b, 1.0));
    grid.attach(&color, 1, 1, 1, 1);
    label("Opacity:", 2);
    let opacity = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 5.0);
    opacity.set_value(annot.opacity * 100.0);
    opacity.set_draw_value(true);
    grid.attach(&opacity, 1, 2, 1, 1);
    label("Initial window state:", 3);
    let state = combo(&["Open", "Close"], if annot.popup_is_open { 0 } else { 1 });
    grid.attach(&state, 1, 3, 1, 1);
    let mut icon_dd = None;
    let mut markup_dd = None;
    match &annot.kind {
        AnnotKind::Text { icon, .. } => {
            label("Icon:", 4);
            let names: Vec<&str> = ICONS.iter().map(|i| i.0).collect();
            let dd = combo(&names, ICONS.iter().position(|i| i.1 == *icon).unwrap_or(0) as u32);
            grid.attach(&dd, 1, 4, 1, 1);
            icon_dd = Some(dd);
        }
        AnnotKind::TextMarkup(t) => {
            label("Markup type:", 4);
            let names: Vec<&str> = MARKUPS.iter().map(|i| i.0).collect();
            let dd = combo(&names, MARKUPS.iter().position(|i| i.1 == *t).unwrap_or(0) as u32);
            grid.attach(&dd, 1, 4, 1, 1);
            markup_dd = Some(dd);
        }
        _ => {}
    }
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_halign(gtk::Align::End);
    buttons.set_margin_top(12);
    let close = gtk::Button::with_mnemonic("_Close");
    let applyb = gtk::Button::with_mnemonic("_Apply");
    applyb.add_css_class("suggested-action");
    buttons.append(&close);
    buttons.append(&applyb);
    grid.attach(&buttons, 0, 5, 2, 1);
    win.set_content(Some(&grid));
    let w = win.clone();
    close.connect_clicked(move |_| w.close());
    let w = win.clone();
    let base = annot.clone();
    applyb.connect_clicked(move |_| {
        let mut a = base.clone();
        a.label = Some(author.text().to_string());
        let rgba = color.rgba();
        a.color = Color::rgb(rgba.red(), rgba.green(), rgba.blue());
        a.opacity = opacity.value() / 100.0;
        a.popup_is_open = state.active() == Some(0);
        if let (Some(dd), AnnotKind::Text { is_open, .. }) = (&icon_dd, &a.kind) {
            a.kind = AnnotKind::Text { icon: ICONS[dd.active().unwrap_or(0) as usize].1, is_open: *is_open };
        }
        if let Some(dd) = &markup_dd {
            a.kind = AnnotKind::TextMarkup(MARKUPS[dd.active().unwrap_or(0) as usize].1);
        }
        apply(a);
        w.close();
    });
    win.show_all();
    win.present();
}

fn combo(items: &[&str], active: u32) -> gtk::ComboBoxText {
    let c = gtk::ComboBoxText::new();
    for i in items {
        c.append_text(i);
    }
    c.set_active(Some(active));
    c
}
