//! Passes main-thread closures to gio APIs that demand `Send + Sync` callbacks.
//!
//! The closure stays in a thread-local table; the callback handed to gio only carries its key,
//! so a call from any other thread finds nothing and does nothing.

use gio::prelude::*;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    static SLOTS: RefCell<HashMap<u64, Rc<dyn Any>>> = RefCell::new(HashMap::new());
}
static NEXT: AtomicU64 = AtomicU64::new(1);

pub struct Local<F> {
    id: u64,
    _f: PhantomData<fn() -> F>,
}

impl<F> Clone for Local<F> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F> Copy for Local<F> {}

impl<F: 'static> Local<F> {
    pub fn new(f: F) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        SLOTS.with(|s| s.borrow_mut().insert(id, Rc::new(f)));
        Local { id, _f: PhantomData }
    }

    pub fn get(&self) -> Option<Rc<F>> {
        SLOTS.with(|s| s.borrow().get(&self.id).cloned()).and_then(|a| a.downcast::<F>().ok())
    }

    pub fn free(&self) {
        SLOTS.with(|s| s.borrow_mut().remove(&self.id));
    }
}

/// An exported D-Bus object; call `unregister` to remove it.
pub struct Registration {
    id: Option<gio::RegistrationId>,
    slot: u64,
}

impl Registration {
    pub fn unregister(mut self, conn: &gio::DBusConnection) {
        if let Some(id) = self.id.take() {
            let _ = conn.unregister_object(id);
        }
        SLOTS.with(|s| s.borrow_mut().remove(&self.slot));
    }
}

/// Exports `iface` at `path`; the handler receives (connection, sender, method, parameters, invocation).
pub fn register_object<F>(
    conn: &gio::DBusConnection,
    path: &str,
    iface: &gio::DBusInterfaceInfo,
    handler: F,
) -> Result<Registration, glib::Error>
where
    F: Fn(&gio::DBusConnection, &str, &str, glib::Variant, gio::DBusMethodInvocation) + 'static,
{
    let local = Local::new(handler);
    let id = conn.register_object(
        path,
        iface,
        move |conn, sender, _path, _iface, method, params, inv| {
            if let Some(f) = local.get() {
                f(&conn, sender, method, params, inv);
            }
        },
        |_, _, _, _, _| ().to_variant(),
        |_, _, _, _, _, _| false,
    );
    match id {
        Ok(id) => Ok(Registration { id: Some(id), slot: local.id }),
        Err(e) => {
            local.free();
            Err(e)
        }
    }
}

/// Watches a bus name and calls `vanished` when its owner goes away; returns an unwatch function.
pub fn watch_name_vanished<F>(conn: &gio::DBusConnection, name: &str, vanished: F) -> Box<dyn FnOnce()>
where
    F: Fn(&gio::DBusConnection, &str) + 'static,
{
    let local = Local::new(vanished);
    let id = gio::bus_watch_name_on_connection(
        conn,
        name,
        gio::BusNameWatcherFlags::NONE,
        |_, _, _| {},
        move |conn, name| {
            if let Some(f) = local.get() {
                f(&conn, name);
            }
        },
    );
    Box::new(move || {
        gio::bus_unwatch_name(id);
        local.free();
    })
}
