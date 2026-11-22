use gio::prelude::*;

/// Access to the io.github.adelmonte.EvinceRs schema, falling back to defaults when it is not installed.
#[derive(Clone)]
pub struct Settings {
    main: Option<gio::Settings>,
    default: Option<gio::Settings>,
}

fn schema_exists(id: &str) -> bool {
    gio::SettingsSchemaSource::default().and_then(|s| s.lookup(id, true)).is_some()
}

impl Settings {
    pub fn new() -> Settings {
        if schema_exists("io.github.adelmonte.EvinceRs") && schema_exists("io.github.adelmonte.EvinceRs.Default") {
            let main = gio::Settings::new("io.github.adelmonte.EvinceRs");
            let default = main.child("default");
            Settings { main: Some(main), default: Some(default) }
        } else {
            Settings { main: None, default: None }
        }
    }

    pub fn gio(&self) -> Option<&gio::Settings> {
        self.main.as_ref()
    }

    pub fn override_restrictions(&self) -> bool {
        self.main.as_ref().is_none_or(|s| s.boolean("override-restrictions"))
    }
    pub fn auto_reload(&self) -> bool {
        self.main.as_ref().is_none_or(|s| s.boolean("auto-reload"))
    }
    pub fn page_cache_size(&self) -> u32 {
        self.main.as_ref().map_or(50, |s| s.uint("page-cache-size"))
    }
    pub fn show_caret_navigation_message(&self) -> bool {
        self.main.as_ref().is_none_or(|s| s.boolean("show-caret-navigation-message"))
    }
    pub fn set_show_caret_navigation_message(&self, v: bool) {
        if let Some(s) = &self.main {
            let _ = s.set_boolean("show-caret-navigation-message", v);
        }
    }
    fn maybe_string(&self, key: &str) -> Option<String> {
        let s = self.main.as_ref()?;
        let v = s.value(key);
        v.as_maybe().and_then(|v| v.str().map(|s| s.to_string()))
    }
    fn set_maybe_string(&self, key: &str, val: &str) {
        if let Some(s) = &self.main {
            let v = glib::Variant::from_some(&val.to_variant());
            let _ = s.set_value(key, &v);
        }
    }
    pub fn document_directory(&self) -> Option<String> {
        self.maybe_string("document-directory")
    }
    pub fn set_document_directory(&self, uri: &str) {
        self.set_maybe_string("document-directory", uri);
    }
    pub fn pictures_directory(&self) -> Option<String> {
        self.maybe_string("pictures-directory")
    }
    pub fn set_pictures_directory(&self, uri: &str) {
        self.set_maybe_string("pictures-directory", uri);
    }

    pub fn default_bool(&self, key: &str, fallback: bool) -> bool {
        self.default.as_ref().map_or(fallback, |s| s.boolean(key))
    }
    pub fn default_string(&self, key: &str, fallback: &str) -> String {
        self.default.as_ref().map_or(fallback.to_string(), |s| s.string(key).to_string())
    }
    pub fn default_double(&self, key: &str, fallback: f64) -> f64 {
        self.default.as_ref().map_or(fallback, |s| s.double(key))
    }
    pub fn default_int(&self, key: &str, fallback: i32) -> i32 {
        self.default.as_ref().map_or(fallback, |s| s.int(key))
    }
    pub fn window_ratio(&self) -> (f64, f64) {
        self.default.as_ref().map_or((0.0, 0.0), |s| s.get::<(f64, f64)>("window-ratio"))
    }

    pub fn save_defaults(&self, d: &Defaults) {
        let Some(s) = &self.default else { return };
        s.delay();
        let _ = s.set_boolean("show-sidebar", d.show_sidebar);
        let _ = s.set_string("sidebar-page", &d.sidebar_page);
        let _ = s.set_int("sidebar-size", d.sidebar_size);
        let _ = s.set_boolean("continuous", d.continuous);
        let _ = s.set_boolean("dual-page", d.dual_page);
        let _ = s.set_boolean("dual-page-odd-left", d.dual_page_odd_left);
        let _ = s.set_boolean("fullscreen", d.fullscreen);
        let _ = s.set_boolean("inverted-colors", d.inverted);
        let _ = s.set_string("sizing-mode", &d.sizing_mode);
        let _ = s.set_double("zoom", d.zoom);
        let _ = s.set("window-ratio", d.window_ratio);
        s.apply();
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings::new()
    }
}

pub struct Defaults {
    pub show_sidebar: bool,
    pub sidebar_page: String,
    pub sidebar_size: i32,
    pub continuous: bool,
    pub dual_page: bool,
    pub dual_page_odd_left: bool,
    pub fullscreen: bool,
    pub inverted: bool,
    pub sizing_mode: String,
    pub zoom: f64,
    pub window_ratio: (f64, f64),
}
