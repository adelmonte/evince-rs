use super::settings::Settings;
use super::window::Window;
use crate::document::LinkDest;
use gtk::prelude::*;
use gtk::{gdk, gio};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub const APP_ID: &str = "io.github.adelmonte.EvinceRs";
const DAEMON_SERVICE: &str = "io.github.adelmonte.EvinceRs.Daemon";
const DAEMON_PATH: &str = "/io/github/adelmonte/EvinceRs/Daemon";
const DAEMON_IFACE: &str = "io.github.adelmonte.EvinceRs.Daemon";
pub const APP_PATH: &str = "/io/github/adelmonte/EvinceRs";
pub const APP_IFACE: &str = "io.github.adelmonte.EvinceRs.Application";
pub const WINDOW_IFACE: &str = "io.github.adelmonte.EvinceRs.Window";

pub const DBUS_XML: &str = r#"
<node>
  <interface name='io.github.adelmonte.EvinceRs.Application'>
    <method name='Reload'>
      <arg type='a{sv}' name='args' direction='in'/>
      <arg type='u' name='timestamp' direction='in'/>
    </method>
    <method name='GetWindowList'>
      <arg type='ao' name='window_list' direction='out'/>
    </method>
  </interface>
  <interface name='io.github.adelmonte.EvinceRs.Window'>
    <method name='SyncView'>
      <arg type='s' name='source_file' direction='in'/>
      <arg type='(ii)' name='source_point' direction='in'/>
      <arg type='u' name='timestamp' direction='in'/>
    </method>
    <signal name='SyncSource'>
      <arg type='s' name='source_file' direction='out'/>
      <arg type='(ii)' name='source_point' direction='out'/>
      <arg type='u' name='timestamp' direction='out'/>
    </signal>
    <signal name='Closed'/>
    <signal name='DocumentLoaded'>
      <arg type='s' name='uri' direction='out'/>
    </signal>
  </interface>
</node>"#;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RunMode {
    #[default]
    Normal,
    Fullscreen,
    Presentation,
}

impl RunMode {
    fn to_u32(self) -> u32 {
        match self {
            RunMode::Normal => 0,
            RunMode::Fullscreen => 1,
            RunMode::Presentation => 2,
        }
    }
    fn from_u32(v: u32) -> RunMode {
        match v {
            1 => RunMode::Fullscreen,
            2 => RunMode::Presentation,
            _ => RunMode::Normal,
        }
    }
}

#[derive(Default, Debug)]
pub struct Options {
    pub new_window: bool,
    pub page_label: Option<String>,
    pub page_index: i32,
    pub named_dest: Option<String>,
    pub fullscreen: bool,
    pub presentation: bool,
    pub preview: bool,
    pub find: Option<String>,
    pub unlink_tempfile: bool,
    pub print_settings: Option<String>,
    pub files: Vec<String>,
}

const HELP: &str = "Usage:
  evince-rs [OPTION…] [FILE…]

Help Options:
  -h, --help                     Show help options

Application Options:
  -o, --new-window               Open a new window.
  -p, --page-label=PAGE          The page label of the document to display.
  -i, --page-index=NUMBER        The page number of the document to display.
  -n, --named-dest=DEST          Named destination to display.
  -f, --fullscreen               Run evince-rs in fullscreen mode
  -s, --presentation             Run evince-rs in presentation mode
  -w, --preview                  Run evince-rs as a previewer
  -l, --find=STRING              The word or phrase to find in the document
  --display=DISPLAY              X display to use
";

/// Parses the command line like GOptionContext would for evince's entries.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0;
    let mut rest_files = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if rest_files || !a.starts_with('-') || a == "-" {
            o.files.push(a.clone());
            continue;
        }
        if a == "--" {
            rest_files = true;
            continue;
        }
        let (name, inline): (String, Option<String>) = if let Some(long) = a.strip_prefix("--") {
            match long.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (long.to_string(), None),
            }
        } else {
            let s = &a[1..];
            let mut chars = s.chars();
            let c = chars.next().unwrap_or(' ');
            let rest: String = chars.collect();
            let long = match c {
                'o' => "new-window",
                'p' => "page-label",
                'i' => "page-index",
                'n' => "named-dest",
                'f' => "fullscreen",
                's' => "presentation",
                'w' => "preview",
                'l' => "find",
                'u' => "unlink-tempfile",
                't' => "print-settings",
                'h' | '?' => "help",
                _ => return Err(format!("Unknown option {a}")),
            };
            let flag = matches!(c, 'o' | 'f' | 's' | 'w' | 'u' | 'h' | '?');
            if flag && !rest.is_empty() {
                let mut expanded: Vec<String> = rest.chars().map(|c| format!("-{c}")).collect();
                expanded.reverse();
                for e in expanded {
                    args_insert(&mut o, &e)?;
                }
            }
            (long.to_string(), if rest.is_empty() || flag { None } else { Some(rest) })
        };
        let mut value = |inline: Option<String>| -> Result<String, String> {
            match inline {
                Some(v) => Ok(v),
                None if i < args.len() => {
                    i += 1;
                    Ok(args[i - 1].clone())
                }
                None => Err(format!("Missing argument for --{name}")),
            }
        };
        match name.as_str() {
            "new-window" => o.new_window = true,
            "fullscreen" => o.fullscreen = true,
            "presentation" => o.presentation = true,
            "preview" => o.preview = true,
            "unlink-tempfile" => o.unlink_tempfile = true,
            "page-label" => o.page_label = Some(value(inline)?),
            "page-index" => {
                let v = value(inline)?;
                o.page_index = v.trim().parse().map_err(|_| format!("Cannot parse integer value “{v}” for --page-index"))?;
            }
            "named-dest" => o.named_dest = Some(value(inline)?),
            "find" => o.find = Some(value(inline)?),
            "print-settings" => o.print_settings = Some(value(inline)?),
            "display" => {
                let d = value(inline)?;
                unsafe { std::env::set_var("DISPLAY", d) };
            }
            "version" => {
                println!("evince-rs {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "help" | "help-all" | "help-gtk" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            _ => return Err(format!("Unknown option {a}")),
        }
    }
    Ok(o)
}

fn args_insert(o: &mut Options, flag: &str) -> Result<(), String> {
    match flag {
        "-o" => o.new_window = true,
        "-f" => o.fullscreen = true,
        "-s" => o.presentation = true,
        "-w" => o.preview = true,
        "-u" => o.unlink_tempfile = true,
        "-h" | "-?" => {
            print!("{HELP}");
            std::process::exit(0);
        }
        _ => return Err(format!("Unknown option {flag}")),
    }
    Ok(())
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn fragment_key(fragment: &str, key: &str) -> Option<String> {
    if key == "page" && is_number(fragment) {
        return Some(fragment.to_string());
    }
    for part in fragment.split('&') {
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        if k.eq_ignore_ascii_case(key) {
            return Some(glib::Uri::unescape_string(v, None::<&str>).map(|s| s.to_string()).unwrap_or_else(|| v.to_string()));
        }
    }
    None
}

/// Splits "file.pdf#page=3" style arguments into the file and destination.
pub fn split_fragment(arg: &str) -> (String, Option<LinkDest>) {
    let mut file_part = arg.to_string();
    let mut fragment = None;
    if let Ok(u) = glib::Uri::parse(arg, glib::UriFlags::ENCODED | glib::UriFlags::NON_DNS) {
        if let Some(f) = u.fragment() {
            fragment = Some(f.to_string());
            file_part = arg[..arg.len() - f.len() - 1].to_string();
        }
    } else if let Some(pos) = arg.rfind('#') {
        if !gio::File::for_commandline_arg(arg).query_exists(gio::Cancellable::NONE) {
            fragment = Some(arg[pos + 1..].to_string());
            file_part = arg[..pos].to_string();
        }
    }
    let dest = fragment.and_then(|f| {
        if let Some(v) = fragment_key(&f, "nameddest") {
            Some(LinkDest::Named(v))
        } else {
            fragment_key(&f, "page").map(LinkDest::PageLabel)
        }
    });
    (file_part, dest)
}

pub struct App {
    pub gtk_app: gtk::Application,
    pub settings: Settings,
    windows: RefCell<Vec<Rc<Window>>>,
    uri: RefCell<Option<String>>,
    doc_registered: Cell<bool>,
    window_counter: Cell<u32>,
    app_reg: RefCell<Option<crate::mainthread::Registration>>,
    pub node: gio::DBusNodeInfo,
}

impl App {
    pub fn new(gtk_app: &gtk::Application) -> Rc<App> {
        Rc::new(App {
            gtk_app: gtk_app.clone(),
            settings: Settings::new(),
            windows: RefCell::new(Vec::new()),
            uri: RefCell::new(None),
            doc_registered: Cell::new(false),
            window_counter: Cell::new(0),
            app_reg: RefCell::new(None),
            node: gio::DBusNodeInfo::for_xml(DBUS_XML).expect("dbus xml"),
        })
    }

    pub fn connection(&self) -> Option<gio::DBusConnection> {
        self.gtk_app.dbus_connection()
    }

    pub fn next_window_id(&self) -> u32 {
        let n = self.window_counter.get() + 1;
        self.window_counter.set(n);
        n
    }

    pub fn add_window(&self, w: &Rc<Window>) {
        self.windows.borrow_mut().push(w.clone());
    }

    pub fn remove_window(&self, w: &Window) {
        self.windows.borrow_mut().retain(|x| x.id != w.id);
        let any_doc = self.windows.borrow().iter().any(|x| x.uri().is_some());
        if !any_doc && self.doc_registered.get() {
            if let (Some(c), Some(uri)) = (self.connection(), self.uri.borrow().clone()) {
                let _ = c.call_sync(
                    Some(DAEMON_SERVICE),
                    DAEMON_PATH,
                    DAEMON_IFACE,
                    "UnregisterDocument",
                    Some(&(uri,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    2000,
                    gio::Cancellable::NONE,
                );
            }
            self.doc_registered.set(false);
        }
    }

    pub fn windows(&self) -> Vec<Rc<Window>> {
        self.windows.borrow().clone()
    }

    pub fn unregister_failed(&self, _w: &Window) {}

    pub fn startup(self: &Rc<Self>) {
        gtk::Window::set_default_icon_name("io.github.adelmonte.EvinceRs");
        if let Some(theme) = gtk::IconTheme::default() {
            for dir in data_dirs("icons") {
                theme.append_search_path(&dir);
            }
        }
        let css = gtk::CssProvider::new();
        if let Err(e) = css.load_from_data(include_str!("style.css").as_bytes()) {
            eprintln!("evince-rs: style.css: {e}");
        }
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(&screen, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        self.setup_accels();
        self.export_app_dbus();
    }

    fn setup_accels(&self) {
        let accels: &[(&str, &[&str])] = &[
            ("win.open", &["<Ctrl>O"]),
            ("win.open-copy", &["<Ctrl>N"]),
            ("win.save-as", &["<Ctrl>S"]),
            ("win.print", &["<Ctrl>P"]),
            ("win.show-properties", &["<alt>Return"]),
            ("win.copy", &["<Ctrl>C", "<Ctrl>Insert"]),
            ("win.select-all", &["<Ctrl>A"]),
            ("win.add-bookmark", &["<Ctrl>D"]),
            ("win.delete-bookmark", &["<Ctrl><Shift>D"]),
            ("win.close", &["<Ctrl>W"]),
            ("win.escape", &["Escape"]),
            ("win.find", &["<Ctrl>F", "slash", "KP_Divide"]),
            ("win.find-next", &["<Ctrl>G", "F3"]),
            ("win.find-previous", &["<Ctrl><Shift>G", "<Shift>F3"]),
            ("win.select-page", &["<Ctrl>L"]),
            ("win.go-backwards", &["<Shift>Page_Up"]),
            ("win.go-forward", &["<Shift>Page_Down"]),
            ("win.go-next-page", &["n", "<Ctrl>Page_Down"]),
            ("win.go-previous-page", &["p", "<Ctrl>Page_Up"]),
            ("win.go-back-history", &["<alt>P", "Back"]),
            ("win.go-forward-history", &["<alt>N", "Forward"]),
            ("win.sizing-mode::fit-page", &["f"]),
            ("win.sizing-mode::fit-width", &["w"]),
            ("win.sizing-mode::automatic", &["a"]),
            ("win.default-zoom", &["<Ctrl>0", "<Ctrl>KP_0"]),
            ("win.toggle-menu", &["F10"]),
            ("win.caret-navigation", &["F7"]),
            ("win.zoom-in", &["plus", "<Ctrl>plus", "KP_Add", "<Ctrl>KP_Add", "equal", "<Ctrl>equal"]),
            ("win.zoom-out", &["minus", "<Ctrl>minus", "KP_Subtract", "<Ctrl>KP_Subtract"]),
            ("win.show-side-pane", &["F9"]),
            ("win.fullscreen", &["F11"]),
            ("win.presentation", &["F5", "<Shift>F5"]),
            ("win.continuous", &["c"]),
            ("win.dual-page", &["d"]),
            ("win.dual-odd-left", &["o"]),
            ("win.rotate-left", &["<Ctrl>Left"]),
            ("win.rotate-right", &["<Ctrl>Right"]),
            ("win.inverted-colors", &["<Ctrl>I"]),
            ("win.reload", &["<Ctrl>R"]),
            ("win.add-annotation", &["s"]),
            ("win.highlight-annotation", &["<Ctrl>H"]),
            ("win.help", &["F1"]),
            ("win.show-help-overlay", &["<Ctrl>question", "<Ctrl>F1"]),
        ];
        for (action, keys) in accels {
            self.gtk_app.set_accels_for_action(action, keys);
        }
    }

    fn export_app_dbus(self: &Rc<Self>) {
        let Some(conn) = self.connection() else { return };
        let Some(iface) = self.node.lookup_interface(APP_IFACE) else { return };
        let weak = Rc::downgrade(self);
        let reg = crate::mainthread::register_object(&conn, APP_PATH, &iface, move |_, _, method, params, inv| {
                let Some(app) = weak.upgrade() else { return };
                match method {
                    "Reload" => {
                        let mut dest = None;
                        let mut mode = RunMode::Normal;
                        let mut find = None;
                        if let Some(dict) = params.try_child_value(0) {
                            for entry in dict.iter() {
                                let Some(k) = entry.try_child_value(0).and_then(|k| k.str().map(str::to_string)) else { continue };
                                let Some(v) = entry.try_child_value(1).and_then(|v| v.as_variant()) else { continue };
                                match k.as_str() {
                                    "mode" => mode = v.get::<u32>().map(RunMode::from_u32).unwrap_or(mode),
                                    "page-label" => dest = v.str().map(|s| LinkDest::PageLabel(s.into())),
                                    "named-dest" => dest = v.str().map(|s| LinkDest::Named(s.into())),
                                    "page-index" => dest = v.get::<u32>().map(|p| LinkDest::Page(p as usize)),
                                    "find-string" => find = v.str().map(str::to_string),
                                    _ => {}
                                }
                            }
                        }
                        for w in app.windows() {
                            let uri = w.uri().or_else(|| app.uri.borrow().clone());
                            if let Some(uri) = uri {
                                w.open_uri(&uri, dest.clone(), mode, find.clone());
                            }
                            w.win.present();
                        }
                        inv.return_value(None);
                    }
                    "GetWindowList" => {
                        let paths: Vec<glib::variant::ObjectPath> = app
                            .windows()
                            .iter()
                            .filter_map(|w| glib::variant::ObjectPath::try_from(format!("/io/github/adelmonte/EvinceRs/Window/{}", w.id - 1)).ok())
                            .collect();
                        inv.return_value(Some(&(paths,).to_variant()));
                    }
                    _ => inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "Unknown method"),
                }
            });
        if let Ok(r) = reg {
            *self.app_reg.borrow_mut() = Some(r);
        }
    }

    pub fn has_window(&self) -> bool {
        !self.windows.borrow().is_empty()
    }

    fn empty_window(&self) -> Option<Rc<Window>> {
        self.windows.borrow().iter().find(|w| w.is_empty()).cloned()
    }

    pub fn open_recent_view(self: &Rc<Self>) {
        let w = Window::new(self);
        self.add_window(&w);
        w.show_recent_view();
        w.win.present();
    }

    /// Opens files given on the command line or dropped on a window.
    pub fn open_uri_list(self: &Rc<Self>, uris: &[String], from: Option<&Rc<Window>>) {
        for u in uris {
            let (file, dest) = split_fragment(u);
            let uri = gio::File::for_commandline_arg(&file).uri().to_string();
            self.open_uri_at_dest(&uri, dest, RunMode::Normal, None, from);
        }
    }

    pub fn open_uri_at_dest(self: &Rc<Self>, uri: &str, dest: Option<LinkDest>, mode: RunMode, find: Option<String>, from: Option<&Rc<Window>>) {
        let current = self.uri.borrow().clone();
        if let Some(cur) = &current {
            if cur != uri {
                self.spawn(uri, dest, mode, find.as_deref());
                return;
            }
        } else {
            *self.uri.borrow_mut() = Some(uri.to_string());
        }
        let _ = from;
        self.register_uri(uri, dest, mode, find);
    }

    fn open_in_window(self: &Rc<Self>, uri: &str, dest: Option<LinkDest>, mode: RunMode, find: Option<String>) {
        let w = match self.empty_window().or_else(|| self.windows.borrow().iter().find(|w| w.uri().is_none()).cloned()) {
            Some(w) => w,
            None => {
                let w = Window::new(self);
                self.add_window(&w);
                w
            }
        };
        w.open_uri(uri, dest, mode, find);
        w.win.present();
    }

    fn register_uri(self: &Rc<Self>, uri: &str, dest: Option<LinkDest>, mode: RunMode, find: Option<String>) {
        let Some(conn) = self.connection() else {
            self.open_in_window(uri, dest, mode, find);
            return;
        };
        if self.doc_registered.get() {
            for w in self.windows() {
                w.open_uri(uri, dest.clone(), mode, find.clone());
                w.win.present();
            }
            if self.windows.borrow().is_empty() {
                self.open_in_window(uri, dest, mode, find);
            }
            return;
        }
        let hold = self.gtk_app.hold();
        let app = self.clone();
        let uri_s = uri.to_string();
        conn.call(
            Some(DAEMON_SERVICE),
            DAEMON_PATH,
            DAEMON_IFACE,
            "RegisterDocument",
            Some(&(uri,).to_variant()),
            Some(glib::VariantTy::new("(s)").unwrap()),
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |res| {
                let _hold = hold;
                let owner = match res {
                    Ok(v) => v.get::<(String,)>().map(|t| t.0).unwrap_or_default(),
                    Err(e) => {
                        eprintln!("Error registering document: {e}");
                        app.open_in_window(&uri_s, dest, mode, find);
                        return;
                    }
                };
                if owner.is_empty() {
                    app.doc_registered.set(true);
                    app.open_in_window(&uri_s, dest, mode, find);
                    return;
                }
                app.reload_remote(&owner, dest, mode, find);
            },
        );
    }

    fn reload_remote(self: &Rc<Self>, owner: &str, dest: Option<LinkDest>, mode: RunMode, find: Option<String>) {
        let Some(conn) = self.connection() else { return };
        let dict = glib::VariantDict::new(None);
        let display = gdk::Display::default().map(|d| d.name().to_string()).unwrap_or_default();
        dict.insert_value("display", &display.to_variant());
        match dest {
            Some(LinkDest::PageLabel(l)) => dict.insert_value("page-label", &l.to_variant()),
            Some(LinkDest::Named(n)) => dict.insert_value("named-dest", &n.to_variant()),
            Some(d) => {
                if let Some(p) = d.page() {
                    dict.insert_value("page-index", &(p as u32).to_variant());
                }
            }
            None => {}
        }
        if let Some(f) = find {
            dict.insert_value("find-string", &f.to_variant());
        }
        if mode != RunMode::Normal {
            dict.insert_value("mode", &mode.to_u32().to_variant());
        }
        let args = glib::Variant::tuple_from_iter([dict.end(), 0u32.to_variant()]);
        let hold = self.gtk_app.hold();
        let app = self.clone();
        conn.call(
            Some(owner),
            APP_PATH,
            APP_IFACE,
            "Reload",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |r| {
                if let Err(e) = r {
                    eprintln!("Failed to Reload: {e}");
                }
                drop(hold);
                if app.windows.borrow().is_empty() {
                    app.gtk_app.quit();
                }
            },
        );
    }

    fn viewer_exe() -> std::path::PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| {
                let name = p.file_name()?.to_string_lossy().to_string();
                if name.starts_with("evince-rs") && !name.contains("previewer") && !name.contains("thumbnailer") && !name.contains("daemon") {
                    Some(p)
                } else {
                    p.parent().map(|d| d.join("evince-rs"))
                }
            })
            .filter(|p| p.exists())
            .unwrap_or_else(|| "evince-rs".into())
    }

    /// Starts a separate viewer process for another document (ev_spawn).
    pub fn spawn(&self, uri: &str, dest: Option<LinkDest>, mode: RunMode, find: Option<&str>) {
        let mut cmd = std::process::Command::new(Self::viewer_exe());
        match dest {
            Some(LinkDest::PageLabel(l)) => {
                cmd.arg(format!("--page-label={l}"));
            }
            Some(LinkDest::Named(n)) => {
                cmd.arg(format!("--named-dest={n}"));
            }
            Some(d) => {
                if let Some(p) = d.page() {
                    cmd.arg(format!("--page-index={}", p + 1));
                }
            }
            None => {}
        }
        match mode {
            RunMode::Fullscreen => {
                cmd.arg("-f");
            }
            RunMode::Presentation => {
                cmd.arg("-s");
            }
            RunMode::Normal => {}
        }
        if let Some(f) = find {
            cmd.arg(format!("--find={f}"));
        }
        cmd.arg(uri);
        if let Err(e) = cmd.spawn() {
            eprintln!("Error launching evince-rs {uri}: {e}");
        }
    }

    pub fn spawn_new_window(&self) {
        let _ = std::process::Command::new(Self::viewer_exe()).spawn();
    }

    // ------------------------------------------------------------ print settings

    fn print_settings_file() -> std::path::PathBuf {
        glib::user_config_dir().join("evince-rs").join("print-settings")
    }

    fn key_file() -> glib::KeyFile {
        let kf = glib::KeyFile::new();
        let _ = kf.load_from_file(Self::print_settings_file(), glib::KeyFileFlags::KEEP_COMMENTS | glib::KeyFileFlags::KEEP_TRANSLATIONS);
        kf
    }

    pub fn load_print_settings(&self) -> gtk::PrintSettings {
        let kf = Self::key_file();
        gtk::PrintSettings::from_key_file(&kf, Some("Print Settings")).unwrap_or_else(|_| gtk::PrintSettings::new())
    }

    pub fn load_page_setup(&self) -> gtk::PageSetup {
        let kf = Self::key_file();
        gtk::PageSetup::from_key_file(&kf, Some("Page Setup")).unwrap_or_else(|_| gtk::PageSetup::new())
    }

    fn save_key_file(kf: &glib::KeyFile) {
        let path = Self::print_settings_file();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = kf.save_to_file(&path);
    }

    pub fn save_print_settings(&self, s: &gtk::PrintSettings) {
        let kf = Self::key_file();
        let Some(copy) = s.copy() else { return };
        for k in [
            gtk::PRINT_SETTINGS_PAGE_RANGES,
            gtk::PRINT_SETTINGS_PRINT_PAGES,
            gtk::PRINT_SETTINGS_OUTPUT_BASENAME,
            gtk::PRINT_SETTINGS_COLLATE,
            gtk::PRINT_SETTINGS_REVERSE,
            gtk::PRINT_SETTINGS_N_COPIES,
            gtk::PRINT_SETTINGS_SCALE,
        ] {
            copy.unset(k);
        }
        copy.to_key_file(&kf, Some("Print Settings"));
        Self::save_key_file(&kf);
    }

    pub fn save_page_setup(&self, p: &gtk::PageSetup) {
        let kf = Self::key_file();
        p.to_key_file(&kf, Some("Page Setup"));
        Self::save_key_file(&kf);
    }
}

/// Directories holding installed data (icons, ...) plus the source tree when run from it.
pub fn data_dirs(sub: &str) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    #[cfg(debug_assertions)]
    if let Some(dev) = option_env!("CARGO_MANIFEST_DIR") {
        let p = std::path::Path::new(dev).join("data").join(sub);
        if p.exists() {
            v.push(p);
        }
    }
    for d in glib::system_data_dirs() {
        let p = d.join("evince-rs").join(sub);
        if p.exists() {
            v.push(p);
        }
    }
    v
}

/// Entry point used by the `evince-rs` binary.
pub fn run() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let opts = match parse_args(&args[1..]) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}\nRun 'evince-rs --help' to see a full list of available command line options.");
            return glib::ExitCode::FAILURE;
        }
    };
    glib::set_application_name("Evince RS");
    glib::set_prgname(Some("evince-rs"));
    if opts.preview {
        let mut cmd = std::process::Command::new(
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("evince-rs-previewer")))
                .filter(|p| p.exists())
                .unwrap_or_else(|| "evince-rs-previewer".into()),
        );
        if let Some(ps) = &opts.print_settings {
            cmd.arg("--print-settings").arg(ps);
        }
        if opts.unlink_tempfile {
            cmd.arg("--unlink-tempfile");
        }
        if let Some(f) = opts.files.first() {
            cmd.arg(f);
        }
        return match cmd.spawn() {
            Ok(_) => glib::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error launching previewer: {e}");
                glib::ExitCode::FAILURE
            }
        };
    }
    let gtk_app = gtk::Application::new(Some(APP_ID), gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN);
    let state: Rc<RefCell<Option<Rc<App>>>> = Rc::default();
    let opts = Rc::new(RefCell::new(Some(opts)));
    let st = state.clone();
    gtk_app.connect_startup(move |a| {
        let app = App::new(a);
        app.startup();
        *st.borrow_mut() = Some(app);
    });
    let st = state.clone();
    let o = opts.clone();
    gtk_app.connect_activate(move |_| {
        let Some(app) = st.borrow().clone() else { return };
        let Some(opts) = o.borrow_mut().take() else {
            if !app.has_window() {
                app.open_recent_view();
            }
            return;
        };
        load_files(&app, opts);
    });
    gtk_app.connect_open(|a, _, _| a.activate());
    gtk_app.run_with_args::<&str>(&[])
}

fn load_files(app: &Rc<App>, opts: Options) {
    if opts.files.is_empty() || opts.new_window {
        if !app.has_window() || opts.new_window {
            app.open_recent_view();
        }
        return;
    }
    let global = if let Some(l) = &opts.page_label {
        Some(LinkDest::PageLabel(l.clone()))
    } else if opts.page_index != 0 {
        Some(LinkDest::Page((opts.page_index - 1).max(0) as usize))
    } else {
        opts.named_dest.clone().map(LinkDest::Named)
    };
    let mode = if opts.fullscreen {
        RunMode::Fullscreen
    } else if opts.presentation {
        RunMode::Presentation
    } else {
        RunMode::Normal
    };
    for f in &opts.files {
        let (file, frag) = split_fragment(f);
        let dest = frag.or_else(|| global.clone());
        let uri = gio::File::for_commandline_arg(&file).uri().to_string();
        if app.uri.borrow().as_deref() == Some(uri.as_str()) {
            continue;
        }
        app.open_uri_at_dest(&uri, dest, mode, opts.find.clone(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn options() {
        let o = parse_args(&s(&["-p", "iv", "--find=foo", "-fs", "a.pdf", "--page-index", "3"])).unwrap();
        assert_eq!(o.page_label.as_deref(), Some("iv"));
        assert_eq!(o.find.as_deref(), Some("foo"));
        assert!(o.fullscreen && o.presentation);
        assert_eq!(o.page_index, 3);
        assert_eq!(o.files, vec!["a.pdf"]);
        let o = parse_args(&s(&["-ifoo"]));
        assert!(o.is_err());
        let o = parse_args(&s(&["-i5", "--", "-x.pdf"])).unwrap();
        assert_eq!(o.page_index, 5);
        assert_eq!(o.files, vec!["-x.pdf"]);
    }

    #[test]
    fn fragments() {
        let (f, d) = split_fragment("file:///tmp/a.pdf#page=4");
        assert_eq!(f, "file:///tmp/a.pdf");
        assert_eq!(d, Some(LinkDest::PageLabel("4".into())));
        let (_, d) = split_fragment("file:///tmp/a.pdf#nameddest=intro");
        assert_eq!(d, Some(LinkDest::Named("intro".into())));
        let (f, d) = split_fragment("/nonexistent/b.pdf#12");
        assert_eq!(f, "/nonexistent/b.pdf");
        assert_eq!(d, Some(LinkDest::PageLabel("12".into())));
    }
}
