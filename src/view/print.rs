use super::model::DocHandle;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PageScale {
    None,
    #[default]
    ShrinkToPrintable,
    FitToPrintable,
}

impl PageScale {
    fn from_id(s: &str) -> PageScale {
        match s {
            "none" => PageScale::None,
            "fit" => PageScale::FitToPrintable,
            _ => PageScale::ShrinkToPrintable,
        }
    }
    fn id(&self) -> &'static str {
        match self {
            PageScale::None => "none",
            PageScale::ShrinkToPrintable => "shrink",
            PageScale::FitToPrintable => "fit",
        }
    }
}

pub struct PrintResult {
    pub settings: Option<gtk::PrintSettings>,
    pub page_setup: Option<gtk::PageSetup>,
    pub error: Option<String>,
}

struct Opts {
    scale: Cell<PageScale>,
    autorotate: Cell<bool>,
    use_pdf_size: Cell<bool>,
}

/// Prints the document with a GtkPrintOperation (EvPrintOperation).
pub fn print_document(
    parent: Option<&gtk::Window>,
    doc: &Rc<DocHandle>,
    settings: Option<&gtk::PrintSettings>,
    page_setup: Option<&gtk::PageSetup>,
    current_page: usize,
    direct: bool,
    done: impl Fn(PrintResult) + 'static,
) {
    let op = gtk::PrintOperation::new();
    op.set_allow_async(true);
    op.set_embed_page_setup(true);
    op.set_has_selection(false);
    op.set_support_selection(false);
    op.set_use_full_page(false);
    op.set_unit(gtk::Unit::Points);
    op.set_current_page(current_page as i32);
    op.set_show_progress(true);
    let name = doc
        .info
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| doc.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    op.set_job_name(&name);
    if let Some(s) = settings {
        op.set_print_settings(Some(s));
    }
    if let Some(p) = page_setup {
        op.set_default_page_setup(Some(p));
    }
    let opts = Rc::new(Opts {
        scale: Cell::new(
            settings.and_then(|s| s.get("evince-rs-page-scale")).map(|v| PageScale::from_id(&v)).unwrap_or_default(),
        ),
        autorotate: Cell::new(settings.and_then(|s| s.get("evince-rs-autorotate")).is_none_or(|v| v != "false")),
        use_pdf_size: Cell::new(settings.and_then(|s| s.get("evince-rs-use-pdf-size")).is_some_and(|v| v == "true")),
    });

    let n = doc.n_pages as i32;
    op.connect_begin_print(move |op, _| op.set_n_pages(n));

    let d = doc.clone();
    let o = opts.clone();
    op.connect_request_page_setup(move |_, _, page, setup| {
        let (w, h) = d.page_size(page as usize);
        if o.use_pdf_size.get() {
            let size = gtk::PaperSize::new_custom("evince-rs-custom", "Document page size", w, h, gtk::Unit::Points);
            setup.set_paper_size(&size);
        }
        if o.autorotate.get() {
            let landscape = w > h;
            setup.set_orientation(if landscape { gtk::PageOrientation::Landscape } else { gtk::PageOrientation::Portrait });
        }
    });

    let d = doc.clone();
    let o = opts.clone();
    op.connect_draw_page(move |_, ctx, page| {
        let page = page as usize;
        let Some(cr) = ctx.cairo_context() else { return };
        let (pw, ph) = d.page_size(page);
        let (cw, ch) = (ctx.width(), ctx.height());
        let scale = match o.scale.get() {
            PageScale::None => 1.0,
            PageScale::ShrinkToPrintable => (cw / pw).min(ch / ph).min(1.0),
            PageScale::FitToPrintable => (cw / pw).min(ch / ph),
        };
        let _ = cr.save();
        if o.autorotate.get() {
            let x = ((cw - pw * scale) / 2.0).max(0.0);
            let y = ((ch - ph * scale) / 2.0).max(0.0);
            cr.translate(x, y);
        }
        cr.scale(scale, scale);
        cr.rectangle(0.0, 0.0, pw, ph);
        cr.clip();
        d.jobs.with(|doc| doc.print_page(page, &cr));
        let _ = cr.restore();
    });

    let o = opts.clone();
    let widgets: Rc<RefCell<Option<(gtk::ComboBoxText, gtk::CheckButton, gtk::CheckButton)>>> = Rc::new(RefCell::new(None));
    let w2 = widgets.clone();
    op.set_custom_tab_label(Some("Page Handling"));
    op.connect_create_custom_widget(move |_| {
        let grid = gtk::Grid::new();
        grid.set_row_spacing(6);
        grid.set_column_spacing(12);
        grid.set_border_width(12);
        let label = gtk::Label::with_mnemonic("Page _Scaling:");
        label.set_xalign(0.0);
        let combo = gtk::ComboBoxText::new();
        for t in ["None", "Shrink to Printable Area", "Fit to Printable Area"] {
            combo.append_text(t);
        }
        combo.set_active(Some(match o.scale.get() {
            PageScale::None => 0,
            PageScale::ShrinkToPrintable => 1,
            PageScale::FitToPrintable => 2,
        }));
        combo.set_tooltip_text(Some(
            "Scale document pages to fit the selected printer page. Select from one of the following:\n\n\
             • “None”: No page scaling is performed.\n\n\
             • “Shrink to Printable Area”: Document pages larger than the printable area are reduced to fit the printable area of the printer page.\n\n\
             • “Fit to Printable Area”: Document pages are enlarged or reduced as required to fit the printable area of the printer page.\n",
        ));
        label.set_mnemonic_widget(Some(&combo));
        grid.attach(&label, 0, 0, 1, 1);
        grid.attach(&combo, 1, 0, 1, 1);
        let auto = gtk::CheckButton::with_mnemonic("Auto _Rotate and Center");
        auto.set_active(o.autorotate.get());
        auto.set_tooltip_text(Some(
            "Rotate printer page orientation of each page to match orientation of each document page. Document pages will be centered within the printer page.",
        ));
        grid.attach(&auto, 0, 1, 2, 1);
        let pdf = gtk::CheckButton::with_mnemonic("Select page size using document page size");
        pdf.set_active(o.use_pdf_size.get());
        pdf.set_tooltip_text(Some("When enabled, each page will be printed on the same size paper as the document page."));
        grid.attach(&pdf, 0, 2, 2, 1);
        *w2.borrow_mut() = Some((combo, auto, pdf));
        grid.show_all();
        grid.upcast()
    });
    let o = opts.clone();
    let w3 = widgets.clone();
    op.connect_custom_widget_apply(move |op, _| {
        if let Some((combo, auto, pdf)) = w3.borrow().as_ref() {
            o.scale.set(match combo.active() {
                Some(0) => PageScale::None,
                Some(2) => PageScale::FitToPrintable,
                _ => PageScale::ShrinkToPrintable,
            });
            o.autorotate.set(auto.is_active());
            o.use_pdf_size.set(pdf.is_active());
        }
        if let Some(s) = op.print_settings() {
            s.set("evince-rs-page-scale", Some(o.scale.get().id()));
            s.set("evince-rs-autorotate", Some(if o.autorotate.get() { "true" } else { "false" }));
            s.set("evince-rs-use-pdf-size", Some(if o.use_pdf_size.get() { "true" } else { "false" }));
        }
    });

    let o = opts.clone();
    op.connect_done(move |op, res| {
        let settings = op.print_settings();
        if let Some(s) = &settings {
            s.set("evince-rs-page-scale", Some(o.scale.get().id()));
            s.set("evince-rs-autorotate", Some(if o.autorotate.get() { "true" } else { "false" }));
            s.set("evince-rs-use-pdf-size", Some(if o.use_pdf_size.get() { "true" } else { "false" }));
        }
        let error = if res == gtk::PrintOperationResult::Error { Some("Failed to print document".to_string()) } else { None };
        let applied = matches!(res, gtk::PrintOperationResult::Apply);
        done(PrintResult {
            settings: if applied || error.is_none() { settings } else { None },
            page_setup: op.default_page_setup(),
            error,
        });
    });

    let action = if direct { gtk::PrintOperationAction::Print } else { gtk::PrintOperationAction::PrintDialog };
    if let Err(e) = op.run(action, parent) {
        eprintln!("evince-rs: print failed: {e}");
    }
}

/// Exports pages to a PDF/PS file using the backend's print path.
pub fn export(doc: &Rc<DocHandle>, path: &std::path::Path, pages: &[usize], ps: bool) -> Result<(), String> {
    let first = pages.first().copied().unwrap_or(0);
    let (w, h) = doc.page_size(first);
    let pages = pages.to_vec();
    let p = path.to_path_buf();
    doc.jobs.with(move |d| -> Result<(), String> {
        let cr = if ps {
            let s = cairo::PsSurface::new(w, h, &p).map_err(|e| e.to_string())?;
            let cr = cairo::Context::new(&s).map_err(|e| e.to_string())?;
            for &pg in &pages {
                let (pw, ph) = d.page_size(pg);
                s.set_size(pw, ph);
                d.print_page(pg, &cr);
                cr.show_page().map_err(|e| e.to_string())?;
            }
            s.finish();
            cr
        } else {
            let s = cairo::PdfSurface::new(w, h, &p).map_err(|e| e.to_string())?;
            let cr = cairo::Context::new(&s).map_err(|e| e.to_string())?;
            for &pg in &pages {
                let (pw, ph) = d.page_size(pg);
                s.set_size(pw, ph).map_err(|e| e.to_string())?;
                d.print_page(pg, &cr);
                cr.show_page().map_err(|e| e.to_string())?;
            }
            s.finish();
            cr
        };
        drop(cr);
        Ok(())
    })
}
