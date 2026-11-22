use gio::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const DAEMON_XML: &str = r#"
<node>
  <interface name="io.github.adelmonte.EvinceRs.Daemon">
    <method name="RegisterDocument">
      <arg type="s" name="uri" direction="in"/>
      <arg type="s" name="owner" direction="out"/>
    </method>
    <method name="UnregisterDocument">
      <arg type="s" name="uri" direction="in"/>
    </method>
    <method name="FindDocument">
      <arg type="s" name="uri" direction="in"/>
      <arg type="b" name="spawn" direction="in"/>
      <arg type="s" name="owner" direction="out"/>
    </method>
  </interface>
</node>"#;

struct Doc {
    dbus_name: String,
    uri: String,
    unwatch: Option<Box<dyn FnOnce()>>,
    loaded: Option<gio::SignalSubscriptionId>,
}

#[derive(Default)]
struct State {
    docs: Vec<Doc>,
    pending: HashMap<String, Vec<gio::DBusMethodInvocation>>,
}

fn spawn_viewer(uri: &str) -> bool {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("evince-rs")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| "evince-rs".into());
    match std::process::Command::new(exe).arg(uri).spawn() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("Error spawning evince-rs for uri {uri}: {e}");
            false
        }
    }
}

fn process_pending(state: &Rc<RefCell<State>>, uri: &str, name: &str) {
    let invs = state.borrow_mut().pending.remove(uri).unwrap_or_default();
    for inv in invs {
        inv.return_value(Some(&(name,).to_variant()));
    }
}

fn remove_doc(state: &Rc<RefCell<State>>, conn: &gio::DBusConnection, idx: usize) {
    let mut doc = state.borrow_mut().docs.remove(idx);
    if let Some(w) = doc.unwatch.take() {
        w();
    }
    if let Some(l) = doc.loaded {
        #[allow(deprecated)]
        conn.signal_unsubscribe(l);
    }
}

fn schedule_idle_quit(state: &Rc<RefCell<State>>, main: &glib::MainLoop) {
    let st = state.clone();
    let ml = main.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        let s = st.borrow();
        if s.docs.is_empty() && s.pending.is_empty() {
            ml.quit();
        }
    });
}

fn main() -> glib::ExitCode {
    glib::set_prgname(Some("evince-rs-daemon"));
    let main_loop = glib::MainLoop::new(None, false);
    let state: Rc<RefCell<State>> = Rc::default();
    let node = gio::DBusNodeInfo::for_xml(DAEMON_XML).expect("daemon introspection");
    let iface = node.lookup_interface("io.github.adelmonte.EvinceRs.Daemon").unwrap();
    let conn = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to register: {e}");
            return glib::ExitCode::FAILURE;
        }
    };

    let st = state.clone();
    let ml = main_loop.clone();
    let reg = evince_rs::mainthread::register_object(&conn, "/io/github/adelmonte/EvinceRs/Daemon", &iface, move |conn, sender, method, params, inv| {
            let conn = conn.clone();
            let sender = sender.to_string();
            match method {
                "RegisterDocument" => {
                    let (uri,) = params.get::<(String,)>().unwrap_or_default();
                    if let Some(d) = st.borrow().docs.iter().find(|d| d.uri == uri) {
                        inv.return_value(Some(&(d.dbus_name.as_str(),).to_variant()));
                        return;
                    }
                    let st_l = st.clone();
                    let uri_l = uri.clone();
                    #[allow(deprecated)]
                    let loaded = conn.signal_subscribe(
                        Some(&sender),
                        Some("io.github.adelmonte.EvinceRs.Window"),
                        Some("DocumentLoaded"),
                        None,
                        None,
                        gio::DBusSignalFlags::NONE,
                        move |conn, sender, _, _, _, params| {
                            let (u,) = params.get::<(String,)>().unwrap_or_default();
                            if u == uri_l {
                                process_pending(&st_l, &u, sender);
                            }
                            let id = {
                                let mut s = st_l.borrow_mut();
                                s.docs.iter_mut().find(|d| d.uri == uri_l).and_then(|d| d.loaded.take())
                            };
                            if let Some(id) = id {
                                #[allow(deprecated)]
                                conn.signal_unsubscribe(id);
                            }
                        },
                    );
                    let st_w = st.clone();
                    let ml_w = ml.clone();
                    let unwatch = evince_rs::mainthread::watch_name_vanished(&conn, &sender, move |conn, name| {
                        let idx = st_w.borrow().docs.iter().position(|d| d.dbus_name == name);
                        if let Some(i) = idx {
                            remove_doc(&st_w, conn, i);
                            schedule_idle_quit(&st_w, &ml_w);
                        }
                    });
                    st.borrow_mut().docs.push(Doc {
                        dbus_name: sender,
                        uri,
                        unwatch: Some(unwatch),
                        loaded: Some(loaded),
                    });
                    inv.return_value(Some(&("",).to_variant()));
                }
                "UnregisterDocument" => {
                    let (uri,) = params.get::<(String,)>().unwrap_or_default();
                    let idx = st.borrow().docs.iter().position(|d| d.uri == uri);
                    let Some(i) = idx else {
                        inv.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", "URI not registered");
                        return;
                    };
                    if st.borrow().docs[i].dbus_name != sender {
                        inv.return_dbus_error("org.freedesktop.DBus.Error.BadAddress", "Only owner can call this method");
                        return;
                    }
                    remove_doc(&st, &conn, i);
                    schedule_idle_quit(&st, &ml);
                    inv.return_value(None);
                }
                "FindDocument" => {
                    let (uri, spawn) = params.get::<(String, bool)>().unwrap_or_default();
                    if let Some(d) = st.borrow().docs.iter().find(|d| d.uri == uri) {
                        inv.return_value(Some(&(d.dbus_name.as_str(),).to_variant()));
                        return;
                    }
                    if spawn {
                        let first = !st.borrow().pending.contains_key(&uri);
                        let ok = !first || spawn_viewer(&uri);
                        if ok {
                            st.borrow_mut().pending.entry(uri).or_default().push(inv);
                            return;
                        }
                    }
                    inv.return_value(Some(&("",).to_variant()));
                }
                _ => inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "Unknown method"),
            }
        });
    if let Err(e) = reg {
        eprintln!("Failed to register: {e}");
        return glib::ExitCode::FAILURE;
    }
    conn.set_exit_on_close(false);
    let ml = main_loop.clone();
    conn.connect_closed(move |_, _, _| ml.quit());
    let reply = conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&("io.github.adelmonte.EvinceRs.Daemon", 4u32).to_variant()),
        Some(glib::VariantTy::new("(u)").unwrap()),
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
    );
    match reply.ok().and_then(|r| r.get::<(u32,)>()) {
        Some((1,)) | Some((4,)) => {}
        _ => {
            eprintln!("Failed to register: name already owned");
            return glib::ExitCode::FAILURE;
        }
    }
    schedule_idle_quit(&state, &main_loop);
    main_loop.run();
    glib::ExitCode::SUCCESS
}
