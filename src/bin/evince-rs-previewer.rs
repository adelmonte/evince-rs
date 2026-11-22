use evince_rs::document::factory;
use evince_rs::view::{DocHandle, Model, SizingMode, View};
use evince_rs::shell::compat::*;
use gtk::prelude::*;
use gtk::{gdk, gio};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

const USAGE: &str = "Usage:\n  evince-rs-previewer [OPTION…] FILE\n\nEvince RS Print Preview\n\nHelp Options:\n  -h, --help                   Show help options\n\nApplication Options:\n  -u, --unlink-tempfile        Delete the temporary file\n  -p, --print-settings FILE    Print settings file\n  --fd=FD                      Read input from file descriptor\n  --mime-type=TYPE             The MIME type of input file\n  --print-settings-fd=FD       Read print settings from file descriptor\n";

#[derive(Default)]
struct Opts {
    unlink: bool,
    print_settings: Option<String>,
    fd: Option<i32>,
    mime: Option<String>,
    settings_fd: Option<i32>,
    file: Option<String>,
}

fn parse() -> Result<Opts, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts::default();
    let mut i = 0;
    let mut files = Vec::new();
    while i < args.len() {
        let a = args[i].clone();
        i += 1;
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut val = || -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            let v = args.get(i).cloned().ok_or(format!("Missing argument for {name}"))?;
            i += 1;
            Ok(v)
        };
        match name.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-u" | "--unlink-tempfile" => o.unlink = true,
            "-p" | "--print-settings" => o.print_settings = Some(val()?),
            "--fd" => o.fd = Some(val()?.parse().map_err(|_| "Invalid --fd".to_string())?),
            "--mime-type" => o.mime = Some(val()?),
            "--print-settings-fd" => o.settings_fd = Some(val()?.parse().map_err(|_| "Invalid --print-settings-fd".to_string())?),
            _ if a.starts_with('-') && a.len() > 1 => return Err(format!("Unknown option {a}")),
            _ => files.push(a),
        }
    }
    if o.print_settings.is_some() && o.settings_fd.is_some() {
        return Err("Must use only one of --print-settings or --print-settings-fd".into());
    }
    if o.fd.is_some() {
        if !files.is_empty() {
            return Err("Too many arguments".into());
        }
        if o.unlink {
            return Err("Must not specify --unlink-tempfile".into());
        }
    } else {
        if files.len() != 1 {
            return Err("Need exactly one argument".into());
        }
        if o.mime.is_some() {
            return Err("Must not specify --mime-type".into());
        }
        let f = &files[0];
        let path = glib::filename_from_uri(f).map(|(p, _)| p).unwrap_or_else(|_| PathBuf::from(f));
        if !path.is_file() && !std::path::Path::new(f).is_file() {
            return Err(format!("File \"{f}\" does not exist or is not a regular file\n"));
        }
        o.file = Some(f.clone());
    }
    Ok(o)
}

fn read_fd(fd: i32) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    use std::os::fd::FromRawFd;
    let mut f = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    Ok(v)
}

fn unlink_tempfile(path: &str) {
    let p = glib::filename_from_uri(path).map(|(p, _)| p).unwrap_or_else(|_| PathBuf::from(path));
    let tmp = glib::tmp_dir();
    if p.starts_with(&tmp) || p.starts_with(glib::user_cache_dir()) {
        let _ = std::fs::remove_file(&p);
    }
}

struct Previewer {
    win: gtk::ApplicationWindow,
    view: View,
    model: Rc<Model>,
    doc: RefCell<Option<Rc<DocHandle>>>,
    settings: RefCell<gtk::PrintSettings>,
    setup: RefCell<gtk::PageSetup>,
    title: RefCell<String>,
    page_entry: gtk::Entry,
    page_total: gtk::Label,
}

impl Previewer {
    fn load_settings(&self, data: &[u8]) {
        let kf = glib::KeyFile::new();
        if kf.load_from_bytes(&glib::Bytes::from(data), glib::KeyFileFlags::NONE).is_err() {
            *self.title.borrow_mut() = "Evince RS".into();
            return;
        }
        if let Ok(s) = gtk::PrintSettings::from_key_file(&kf, Some("Print Settings")) {
            *self.settings.borrow_mut() = s;
        }
        if let Ok(p) = gtk::PageSetup::from_key_file(&kf, Some("Page Setup")) {
            *self.setup.borrow_mut() = p;
        }
        match kf.string("Print Job", "title") {
            Ok(t) if !t.is_empty() => {
                self.win.set_title(&t);
                *self.title.borrow_mut() = t.to_string();
            }
            _ => *self.title.borrow_mut() = "Evince RS".into(),
        }
    }

    fn update_page(&self) {
        let Some(doc) = self.doc.borrow().clone() else { return };
        let p = self.model.page();
        self.page_entry.set_text(&doc.label(p));
        self.page_total.set_text(&format!("of {}", doc.n_pages));
        let en = |n: &str, v: bool| {
            if let Some(a) = self.win.lookup_action(n).and_downcast::<gio::SimpleAction>() {
                a.set_enabled(v);
            }
        };
        en("go-previous-page", p > 0);
        en("go-next-page", p + 1 < doc.n_pages);
    }

    fn print(self: &Rc<Self>) {
        let Some(doc) = self.doc.borrow().clone() else { return };
        let settings = self.settings.borrow().clone();
        let setup = self.setup.borrow().clone();
        let me = self.clone();
        self.win.set_visible(false);
        evince_rs::view::print::print_document(Some(self.win.upcast_ref::<gtk::Window>()), &doc, Some(&settings), Some(&setup), 0, true, move |r| {
            if let Some(e) = r.error {
                let d = gtk::MessageDialog::new(Some(&me.win), gtk::DialogFlags::MODAL, gtk::MessageType::Error, gtk::ButtonsType::Close, "Failed to print document");
                d.set_secondary_text(Some(&e));
                d.run();
                destroy(&d);
            }
            me.win.close();
        });
    }
}

fn main() -> glib::ExitCode {
    let opts = match parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Error parsing command line arguments: {e}");
            return glib::ExitCode::FAILURE;
        }
    };
    glib::set_application_name("Evince RS Print Preview");
    glib::set_prgname(Some("io.github.adelmonte.EvinceRs.Previewer"));
    let app = gtk::Application::new(Some("io.github.adelmonte.EvinceRs.Previewer"), gio::ApplicationFlags::NON_UNIQUE);
    let accels: &[(&str, &[&str])] = &[
        ("win.select-page", &["<Ctrl>L"]),
        ("win.go-previous-page", &["p", "<Ctrl>Page_Up"]),
        ("win.go-next-page", &["n", "<Ctrl>Page_Down"]),
        ("win.print", &["<Ctrl>P"]),
        ("win.zoom-in", &["plus", "<Ctrl>plus", "KP_Add", "<Ctrl>KP_Add", "equal", "<Ctrl>equal"]),
        ("win.zoom-out", &["minus", "<Ctrl>minus", "KP_Subtract", "<Ctrl>KP_Subtract"]),
        ("win.zoom-default", &["a"]),
    ];
    let opts = Rc::new(opts);
    let o = opts.clone();
    app.connect_activate(move |app| {
        for (a, k) in accels {
            app.set_accels_for_action(a, k);
        }
        gtk::Window::set_default_icon_name("io.github.adelmonte.EvinceRs");
        let win = gtk::ApplicationWindow::new(app);
        win.set_default_size(600, 600);
        win.set_title("Evince RS Print Preview");
        let model = Model::new();
        let view = View::new();
        view.set_model(&model);
        let sw = scrolled_window();
        sw.add(&view);
        let header = gtk::HeaderBar::new();
        header.set_show_close_button(true);
        let print = gtk::Button::with_mnemonic("_Print");
        print.set_action_name(Some("win.print"));
        print.add_css_class("suggested-action");
        header.pack_end(&print);
        let prev = icon_button("go-up-symbolic");
        prev.set_action_name(Some("win.go-previous-page"));
        prev.set_tooltip_text(Some("Previous Page"));
        let next = icon_button("go-down-symbolic");
        next.set_action_name(Some("win.go-next-page"));
        next.set_tooltip_text(Some("Next Page"));
        let nav = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        nav.add_css_class("linked");
        nav.append(&prev);
        nav.append(&next);
        header.pack_start(&nav);
        let page_entry = gtk::Entry::new();
        page_entry.set_width_chars(5);
        page_entry.set_tooltip_text(Some("Select page or search in the outline"));
        let page_total = gtk::Label::new(None);
        page_total.add_css_class("dim-label");
        header.pack_start(&page_entry);
        header.pack_start(&page_total);
        let zin = icon_button("zoom-in-symbolic");
        zin.set_action_name(Some("win.zoom-in"));
        zin.set_tooltip_text(Some("Enlarge the document"));
        let zout = icon_button("zoom-out-symbolic");
        zout.set_action_name(Some("win.zoom-out"));
        zout.set_tooltip_text(Some("Shrink the document"));
        let zdef = icon_button("zoom-fit-best-symbolic");
        zdef.set_action_name(Some("win.zoom-default"));
        zdef.set_tooltip_text(Some("Fit Width"));
        let zbox = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        zbox.add_css_class("linked");
        zbox.append(&zout);
        zbox.append(&zdef);
        zbox.append(&zin);
        header.pack_end(&zbox);
        win.set_titlebar(Some(&header));
        win.set_content(Some(&sw));
        let p = Rc::new(Previewer {
            win: win.clone(),
            view: view.clone(),
            model: model.clone(),
            doc: RefCell::new(None),
            settings: RefCell::new(gtk::PrintSettings::new()),
            setup: RefCell::new(gtk::PageSetup::new()),
            title: RefCell::new("Evince RS".into()),
            page_entry: page_entry.clone(),
            page_total,
        });
        let add = |name: &str, f: Box<dyn Fn(&Rc<Previewer>)>| {
            let a = gio::SimpleAction::new(name, None);
            let p = p.clone();
            a.connect_activate(move |_, _| f(&p));
            win.add_action(&a);
        };
        add("print", Box::new(|p| p.print()));
        add("go-previous-page", Box::new(|p| {
            p.view.previous_page();
        }));
        add("go-next-page", Box::new(|p| {
            p.view.next_page();
        }));
        add("select-page", Box::new(|p| {
            p.page_entry.grab_focus();
        }));
        add("zoom-in", Box::new(|p| {
            p.model.set_sizing_mode(SizingMode::Free);
            p.view.zoom_in();
        }));
        add("zoom-out", Box::new(|p| {
            p.model.set_sizing_mode(SizingMode::Free);
            p.view.zoom_out();
        }));
        add("zoom-default", Box::new(|p| p.model.set_sizing_mode(SizingMode::FitWidth)));
        let pp = p.clone();
        page_entry.connect_activate(move |e| {
            if let Some(d) = pp.doc.borrow().clone() {
                if let Some(pg) = d.find_page_by_label(&e.text()) {
                    pp.model.set_page(pg);
                }
            }
            pp.update_page();
        });
        let pp = Rc::downgrade(&p);
        model.connect(move |c| {
            if let (Some(p), evince_rs::view::Change::Page { .. }) = (pp.upgrade(), c) {
                p.update_page();
            }
        });
        if let Some(fd) = o.settings_fd {
            match read_fd(fd) {
                Ok(d) => p.load_settings(&d),
                Err(e) => eprintln!("Error reading print settings: {e}"),
            }
        } else if let Some(f) = &o.print_settings {
            match std::fs::read(f) {
                Ok(d) => p.load_settings(&d),
                Err(e) => eprintln!("Error reading print settings: {e}"),
            }
        }
        let (uri, _tmp) = if let Some(fd) = o.fd {
            let data = match read_fd(fd) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error reading input: {e}");
                    return;
                }
            };
            let ext = o.mime.as_deref().map(|m| match m {
                "application/pdf" => ".pdf",
                "application/postscript" => ".ps",
                _ => "",
            });
            let path = factory::temp_path(ext.unwrap_or(""));
            if std::fs::write(&path, data).is_err() {
                return;
            }
            (gio::File::for_path(&path).uri().to_string(), Some(factory::TempFile(path)))
        } else {
            (gio::File::for_commandline_arg(o.file.as_deref().unwrap()).uri().to_string(), None)
        };
        match factory::load_document(&uri, None) {
            Ok(mut loaded) => {
                if let Some(t) = _tmp {
                    loaded.temps.push(t);
                }
                let d = DocHandle::new(loaded);
                *p.doc.borrow_mut() = Some(d.clone());
                model.set_sizing_mode(SizingMode::FitWidth);
                model.set_continuous(true);
                model.set_document(Some(d));
                p.update_page();
            }
            Err(e) => {
                let dlg = gtk::MessageDialog::new(Some(&win), gtk::DialogFlags::MODAL, gtk::MessageType::Error, gtk::ButtonsType::Close, "Failed to load document");
                dlg.set_secondary_text(Some(&e.to_string()));
                dlg.connect_response(|d, _| destroy(d));
                dlg.show_all();
            }
        }
        win.connect_key_press_event(|w, e| {
            if e.keyval() == gdk::keys::constants::w && e.state().contains(gdk::ModifierType::CONTROL_MASK) {
                w.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        win.show_all();
        win.present();
        view.grab_focus();
    });
    let status = app.run_with_args::<&str>(&[]);
    if opts.unlink {
        if let Some(f) = &opts.file {
            unlink_tempfile(f);
        }
    }
    if let Some(f) = &opts.print_settings {
        unlink_tempfile(f);
    }
    status
}
