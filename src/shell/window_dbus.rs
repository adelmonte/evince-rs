use super::application::WINDOW_IFACE;
use super::window::Window;
use gtk::prelude::*;

impl Window {
    fn dbus_path(&self) -> String {
        format!("/io/github/adelmonte/EvinceRs/Window/{}", self.id - 1)
    }

    pub(super) fn export_dbus(&self) {
        let Some(conn) = self.app.connection() else { return };
        let Some(iface) = self.app.node.lookup_interface(WINDOW_IFACE) else { return };
        let w = self.weak();
        let reg = crate::mainthread::register_object(&conn, &self.dbus_path(), &iface, move |_, _, method, params, inv| {
                let Some(w) = w.upgrade() else { return };
                if method == "SyncView" {
                    if let Some((file, (line, col), _ts)) = params.get::<(String, (i32, i32), u32)>() {
                        w.sync_view(&file, line, col);
                        w.win.present();
                    }
                    inv.return_value(None);
                } else {
                    inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "Unknown method");
                }
            });
        if let Ok(r) = reg {
            *self.dbus_reg.borrow_mut() = Some(r);
        }
    }

    pub(super) fn unexport_dbus(&self) {
        if let (Some(conn), Some(r)) = (self.app.connection(), self.dbus_reg.borrow_mut().take()) {
            r.unregister(&conn);
        }
    }

    pub(super) fn emit_dbus_signal(&self, name: &str, args: Option<&glib::Variant>) {
        let Some(conn) = self.app.connection() else { return };
        if self.dbus_reg.borrow().is_none() {
            return;
        }
        let _ = conn.emit_signal(None, &self.dbus_path(), WINDOW_IFACE, name, args);
    }
}
