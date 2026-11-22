use gio::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

const NS: &str = "metadata::evince-rs::";

/// Per-document state stored as GVfs metadata attributes (EvMetadata).
pub struct Metadata {
    file: gio::File,
    items: RefCell<HashMap<String, String>>,
    pending: std::rc::Rc<RefCell<HashMap<String, String>>>,
}

impl Metadata {
    pub fn is_supported(file: &gio::File) -> bool {
        file.query_settable_attributes(gio::Cancellable::NONE)
            .ok()
            .is_some_and(|l| l.attributes().iter().any(|a| a.name().starts_with("metadata")))
            || file.query_writable_namespaces(gio::Cancellable::NONE)
                .ok()
                .is_some_and(|l| l.attributes().iter().any(|a| a.name() == "metadata"))
    }

    pub fn new(file: &gio::File) -> Metadata {
        let mut items = HashMap::new();
        if let Ok(info) = file.query_info("metadata::*", gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
            for attr in info.list_attributes(Some("metadata")) {
                let Some(key) = attr.strip_prefix(NS) else { continue };
                if let Some(v) = info.attribute_as_string(&attr) {
                    items.insert(key.to_string(), v.to_string());
                }
            }
        }
        Metadata { file: file.clone(), items: RefCell::new(items), pending: Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.items.borrow().is_empty()
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.items.borrow().contains_key(key)
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.items.borrow().get(key).cloned()
    }

    pub fn set_string(&self, key: &str, value: &str) {
        if self.items.borrow().get(key).map(|s| s.as_str()) == Some(value) {
            return;
        }
        self.items.borrow_mut().insert(key.to_string(), value.to_string());
        let first = self.pending.borrow().is_empty();
        self.pending.borrow_mut().insert(key.to_string(), value.to_string());
        if first {
            let file = self.file.clone();
            let pending = std::rc::Rc::downgrade(&self.pending);
            glib::idle_add_local_once(move || {
                if let Some(p) = pending.upgrade() {
                    write_pending(&file, &p);
                }
            });
        }
    }

    /// Writes queued changes now; called before the window goes away.
    pub fn flush(&self) {
        write_pending(&self.file, &self.pending);
    }

    pub fn get_int(&self, key: &str) -> Option<i32> {
        self.get_string(key)?.trim().parse::<i64>().ok().map(|v| v as i32)
    }

    pub fn set_int(&self, key: &str, v: i32) {
        self.set_string(key, &v.to_string());
    }

    pub fn get_double(&self, key: &str) -> Option<f64> {
        self.get_string(key)?.trim().parse::<f64>().ok()
    }

    pub fn set_double(&self, key: &str, v: f64) {
        self.set_string(key, &format!("{v}"));
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get_int(key).map(|v| v != 0)
    }

    pub fn set_bool(&self, key: &str, v: bool) {
        self.set_int(key, v as i32);
    }
}

impl Drop for Metadata {
    fn drop(&mut self) {
        self.flush();
    }
}

fn write_pending(file: &gio::File, pending: &RefCell<HashMap<String, String>>) {
    let items = std::mem::take(&mut *pending.borrow_mut());
    if items.is_empty() {
        return;
    }
    let info = gio::FileInfo::new();
    for (k, v) in &items {
        info.set_attribute_string(&format!("{NS}{k}"), v);
    }
    if let Err(e) = file.set_attributes_from_info(&info, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) {
        if !e.matches(gio::IOErrorEnum::NotSupported) {
            eprintln!("evince-rs: failed to save metadata: {e}");
        }
    }
}
