use super::window::Window;
use crate::document::*;
use crate::view::{AddMode, SizingMode};
use gtk::prelude::*;
use gtk::{gdk, gio};
use std::rc::Rc;

type Handler = fn(&Rc<Window>, Option<&glib::Variant>);

impl Window {
    pub(super) fn setup_actions(self: &Rc<Self>) {
        let plain: &[(&str, Handler)] = &[
            ("new", |w, _| w.app.spawn_new_window()),
            ("open", |w, _| w.cmd_open()),
            ("open-copy", |w, _| w.cmd_open_copy(None)),
            ("save-as", |w, _| w.cmd_save_as()),
            ("send-to", |w, _| w.cmd_send_to()),
            ("open-containing-folder", |w, _| w.cmd_open_containing_folder()),
            ("print", |w, _| w.cmd_print()),
            ("show-properties", |w, _| w.cmd_properties()),
            ("copy", |w, _| w.view.copy()),
            ("select-all", |w, _| {
                if w.find.entry.has_focus() || w.page_entry.has_focus() || w.zoom_entry.has_focus() {
                    return;
                }
                w.view.select_all();
            }),
            ("go-previous-page", |w, _| {
                w.view.previous_page();
            }),
            ("go-next-page", |w, _| {
                w.view.next_page();
            }),
            ("go-first-page", |w, _| w.go_to_page_with_history(0)),
            ("go-last-page", |w, _| {
                if let Some(d) = w.document() {
                    w.go_to_page_with_history(d.n_pages.saturating_sub(1));
                }
            }),
            ("go-forward", |w, _| {
                if let Some(d) = w.document() {
                    let p = (w.model.page() + 10).min(d.n_pages.saturating_sub(1));
                    w.model.set_page(p);
                }
            }),
            ("go-backwards", |w, _| {
                let p = w.model.page().saturating_sub(10);
                w.model.set_page(p);
            }),
            ("go-back-history", |w, _| w.history_back()),
            ("go-forward-history", |w, _| w.history_forward()),
            ("find", |w, _| w.show_find_bar()),
            ("find-next", |w, _| {
                if !w.find.bar.is_search_mode() {
                    w.show_find_bar();
                }
                w.view.find_next();
            }),
            ("find-previous", |w, _| {
                if !w.find.bar.is_search_mode() {
                    w.show_find_bar();
                }
                w.view.find_previous();
            }),
            ("select-page", |w, _| {
                if w.document().is_some() {
                    w.page_entry.grab_focus();
                }
            }),
            ("rotate-left", |w, _| w.model.set_rotation(w.model.rotation() - 90)),
            ("rotate-right", |w, _| w.model.set_rotation(w.model.rotation() + 90)),
            ("zoom-in", |w, _| {
                w.model.set_sizing_mode(SizingMode::Free);
                w.view.zoom_in();
            }),
            ("zoom-out", |w, _| {
                w.model.set_sizing_mode(SizingMode::Free);
                w.view.zoom_out();
            }),
            ("default-zoom", |w, _| {
                w.model.set_sizing_mode(SizingMode::Free);
                w.model.set_scale(w.view.dpi() / 72.0);
            }),
            ("reload", |w, _| w.reload(false)),
            ("auto-scroll", |w, _| w.view.autoscroll_start()),
            ("add-bookmark", |w, _| w.add_bookmark()),
            ("delete-bookmark", |w, _| {
                let p = w.model.page() as u32;
                w.delete_bookmark(p);
            }),
            ("close", |w, _| w.win.close()),
            ("scroll-forward", |w, _| w.view.scroll_page(true)),
            ("scroll-backwards", |w, _| w.view.scroll_page(false)),
            ("escape", |w, _| w.cmd_escape()),
            ("toggle-menu", |w, _| w.action_menu.set_active(!w.action_menu.is_active())),
            ("highlight-annotation", |w, _| {
                if !w.view.add_highlight_from_selection() {
                    w.annot_highlight_button.set_active(true);
                }
            }),
            ("about", |w, _| w.cmd_about()),
            ("help", |w, _| {
                let _ = super::compat::launch_uri(&w.win, "help:evince-rs");
            }),
            ("open-link", |w, _| {
                if let Some(l) = w.view.popup_context().link {
                    w.view.handle_link(&l);
                }
            }),
            ("open-link-new-window", |w, _| {
                if let Some(l) = w.view.popup_context().link {
                    w.open_link_new_window(&l);
                }
            }),
            ("go-to-link", |w, _| {
                if let Some(l) = w.view.popup_context().link {
                    w.view.handle_link(&l);
                }
            }),
            ("copy-link-address", |w, _| {
                if let Some(LinkAction::ExternalUri(u)) = w.view.popup_context().link.and_then(|l| l.action) {
                    gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_text(&u);
                    gtk::Clipboard::get(&gdk::SELECTION_PRIMARY).set_text(&u);
                }
            }),
            ("save-image", |w, _| w.cmd_save_image()),
            ("copy-image", |w, _| w.cmd_copy_image()),
            ("open-attachment", |w, _| {
                let a = w.popup_attachment.borrow().clone();
                if let Some(a) = a {
                    w.open_attachment(&a.name, &a.data);
                }
            }),
            ("save-attachment", |w, _| w.cmd_save_attachment()),
            ("annot-properties", |w, _| w.cmd_annot_properties()),
            ("remove-annot", |w, _| {
                if let Some(a) = w.view.popup_context().annot {
                    w.view.remove_annotation(&a);
                }
            }),
        ];
        for (name, f) in plain {
            let a = gio::SimpleAction::new(name, None);
            let w = self.weak();
            let f = *f;
            a.connect_activate(move |_, p| {
                if let Some(w) = w.upgrade() {
                    f(&w, p);
                }
            });
            self.win.add_action(&a);
        }

        let toggles: &[(&str, bool, fn(&Rc<Window>, bool))] = &[
            ("continuous", true, |w, v| w.model.set_continuous(v)),
            ("dual-page", false, |w, v| w.model.set_dual_page(v)),
            ("dual-odd-left", false, |w, v| w.model.set_dual_odd_left(v)),
            ("rtl", false, |w, v| w.model.set_rtl(v)),
            ("show-side-pane", true, |w, v| {
                if w.side_stack.visible_child_name().as_deref() == Some("find") && !v {
                    w.close_find_bar();
                }
                w.show_sidebar(v)
            }),
            ("inverted-colors", false, |w, v| w.model.set_inverted(v)),
            ("enable-spellchecking", false, |_, _| {}),
            ("fullscreen", false, |w, v| {
                if v {
                    w.run_fullscreen();
                } else {
                    w.stop_fullscreen();
                }
            }),
            ("presentation", false, |w, v| {
                if v {
                    w.run_presentation();
                } else {
                    w.stop_presentation();
                }
            }),
            ("caret-navigation", false, |w, v| {
                if v != w.view.caret_navigation() {
                    w.toggle_caret_navigation();
                }
                w.set_action_state("caret-navigation", &w.view.caret_navigation().to_variant());
            }),
            ("toggle-find", false, |w, v| {
                if v {
                    w.show_find_bar();
                } else {
                    w.close_find_bar();
                }
            }),
            ("toggle-edit-annots", false, |w, v| {
                w.annots_bar.set_reveal_child(v);
                if !v {
                    w.annot_text_button.set_active(false);
                    w.annot_highlight_button.set_active(false);
                    w.view.set_add_mode(None);
                }
            }),
            ("add-annotation", false, |w, v| {
                if v {
                    w.view.set_add_mode(Some(AddMode::Text));
                } else if w.view.add_mode() == Some(AddMode::Text) {
                    w.view.set_add_mode(None);
                }
                w.annot_text_button.set_active(v);
            }),
        ];
        for (name, init, f) in toggles {
            let a = gio::SimpleAction::new_stateful(name, None, &init.to_variant());
            let w = self.weak();
            let f = *f;
            a.connect_change_state(move |a, v| {
                let Some(v) = v.and_then(|v| v.get::<bool>()) else { return };
                a.set_state(&v.to_variant());
                if let Some(w) = w.upgrade() {
                    f(&w, v);
                }
            });
            self.win.add_action(&a);
        }

        let sizing = gio::SimpleAction::new_stateful("sizing-mode", Some(glib::VariantTy::STRING), &"free".to_variant());
        let w = self.weak();
        sizing.connect_change_state(move |a, v| {
            let Some(s) = v.and_then(|v| v.str().map(str::to_string)) else { return };
            a.set_state(&s.to_variant());
            if let Some(w) = w.upgrade() {
                w.model.set_sizing_mode(SizingMode::from_str(&s));
            }
        });
        self.win.add_action(&sizing);
        let zoom = gio::SimpleAction::new("zoom", Some(glib::VariantTy::DOUBLE));
        let w = self.weak();
        zoom.connect_activate(move |_, v| {
            let (Some(w), Some(z)) = (w.upgrade(), v.and_then(|v| v.get::<f64>())) else { return };
            w.model.set_sizing_mode(SizingMode::Free);
            w.model.set_scale(z * w.view.dpi() / 72.0);
        });
        self.win.add_action(&zoom);
    }

    pub fn update_zoom_actions(&self) {
        let has_doc = self.document().is_some() && !self.is_presentation();
        self.set_action_enabled("zoom-in", has_doc && self.view.can_zoom_in());
        self.set_action_enabled("zoom-out", has_doc && self.view.can_zoom_out());
    }

    pub fn update_actions_sensitivity(&self) {
        let doc = self.document();
        let has_doc = doc.is_some();
        let presentation = self.is_presentation();
        let n = doc.as_ref().map_or(0, |d| d.n_pages);
        let page = self.model.page();
        let has_pages = n > 0;
        let perms = doc.as_ref().map(|d| d.info.permissions).unwrap_or_default();
        let override_r = self.app.settings.override_restrictions();
        let can_find = doc.as_ref().is_some_and(|d| d.can_find);
        let has_text = doc.as_ref().is_some_and(|d| d.has_text);
        let can_annot = doc.as_ref().is_some_and(|d| d.can_annotate) && (override_r || perms.add_notes);
        let ok = has_doc && has_pages && !presentation;
        let en = |n: &str, v: bool| self.set_action_enabled(n, v);
        en("open-copy", has_doc);
        en("save-as", has_doc && (override_r || perms.copy || true));
        en("send-to", has_doc && glib::find_program_in_path("nautilus-sendto").is_some());
        en("open-containing-folder", has_doc && doc.as_ref().is_some_and(|d| d.uri.starts_with("file:")));
        en("print", has_pages && (override_r || perms.print));
        en("show-properties", has_doc);
        en("copy", has_doc && has_text && (override_r || perms.copy));
        en("select-all", has_doc && has_text);
        en("go-previous-page", ok && page > 0);
        en("go-next-page", ok && page + 1 < n);
        en("go-first-page", ok && page > 0);
        en("go-last-page", ok && page + 1 < n);
        en("go-forward", ok && page + 1 < n);
        en("go-backwards", ok && page > 0);
        en("find", has_doc && can_find && !presentation);
        en("toggle-find", has_doc && can_find && !presentation);
        en("find-next", has_doc && can_find && self.view.find_n_results() > 0);
        en("find-previous", has_doc && can_find && self.view.find_n_results() > 0);
        en("select-page", ok);
        en("continuous", ok);
        en("dual-page", ok);
        en("dual-odd-left", ok && self.model.dual_page());
        en("rtl", ok);
        en("show-side-pane", has_doc && !presentation && !self.is_fullscreen() || has_doc && self.is_fullscreen());
        en("inverted-colors", has_pages);
        en("enable-spellchecking", false);
        en("fullscreen", has_doc);
        en("presentation", has_pages);
        en("rotate-left", has_pages);
        en("rotate-right", has_pages);
        en("default-zoom", ok);
        en("sizing-mode", ok);
        en("zoom", ok);
        en("reload", has_doc && !presentation);
        en("auto-scroll", ok);
        en("add-bookmark", has_pages);
        en("delete-bookmark", has_pages && self.bookmarks.borrow().as_ref().is_some_and(|b| b.has_page(page as u32)));
        en("scroll-forward", ok);
        en("scroll-backwards", ok);
        en("caret-navigation", has_doc && has_text);
        en("toggle-edit-annots", can_annot && !presentation);
        en("add-annotation", can_annot && !presentation);
        en("highlight-annotation", can_annot && !presentation);
        let fit = has_doc && !presentation;
        self.page_box.set_sensitive(ok);
        self.zoom_box.set_sensitive(fit);
        self.update_zoom_actions();
        self.update_history_actions();
        if !can_annot {
            self.annots_bar.set_reveal_child(false);
        }
    }

    pub(super) fn show_view_popup(self: &Rc<Self>, x: f64, y: f64) {
        let ctx = self.view.popup_context();
        let link = ctx.link.as_ref().and_then(|l| l.action.as_ref());
        let is_goto = matches!(link, Some(LinkAction::GotoDest(_)) | Some(LinkAction::GotoRemote { .. }));
        let is_uri = matches!(link, Some(LinkAction::ExternalUri(_)));
        let can_annot = self.document().is_some_and(|d| d.can_annotate);
        let en = |n: &str, v: bool| self.set_action_enabled(n, v);
        en("open-link", ctx.link.is_some() && !is_goto);
        en("go-to-link", is_goto);
        en("open-link-new-window", is_goto);
        en("copy-link-address", is_uri);
        en("save-image", ctx.image.is_some());
        en("copy-image", ctx.image.is_some());
        let att = ctx.attachment.clone();
        self.set_popup_attachment(att);
        en("annot-properties", ctx.annot.as_ref().is_some_and(|a| a.is_markup) && can_annot);
        en("remove-annot", ctx.annot.is_some() && can_annot);
        en("highlight-annotation", self.view.has_selection() && can_annot);
        let b = gtk::Builder::from_string(include_str!("menus.ui"));
        let Some(model) = b.object::<gio::MenuModel>("view-popup-menu") else { return };
        let pop = gtk::Popover::from_model(Some(&self.view), &model);
        pop.set_position(gtk::PositionType::Bottom);
        pop.set_pointing_to(&gdk::Rectangle::new(x as i32, y as i32, 1, 1));
        let w = self.weak();
        pop.connect_closed(move |p| {
            let p = p.clone();
            let w = w.clone();
            glib::idle_add_local_once(move || {
                super::compat::destroy(&p);
                if let Some(w) = w.upgrade() {
                    w.update_actions_sensitivity();
                }
            });
        });
        pop.popup();
    }

    fn cmd_escape(self: &Rc<Self>) {
        if self.view.add_mode().is_some() {
            self.view.set_add_mode(None);
            self.annot_text_button.set_active(false);
            self.annot_highlight_button.set_active(false);
            return;
        }
        if self.find.bar.is_search_mode() {
            self.close_find_bar();
            return;
        }
        if self.is_presentation() {
            self.stop_presentation();
            return;
        }
        if self.is_fullscreen() {
            self.stop_fullscreen();
            return;
        }
        self.view.autoscroll_stop();
        self.view.clear_selection();
        self.view.grab_focus();
    }
}
