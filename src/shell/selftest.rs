use super::compat::*;
use super::window::Window;
use gtk::cairo;
use gtk::prelude::*;
use std::rc::Rc;
use std::time::Duration;

type Step = (u64, Box<dyn Fn(&Rc<Window>)>);

fn act(w: &Rc<Window>, name: &str) {
    gio::prelude::ActionGroupExt::activate_action(&w.win, name, None);
}

fn report(w: &Rc<Window>, tag: &str) {
    let sb = w.sidebar();
    let mut pages = Vec::new();
    let st = &sb.stack;
    let mut c = st.first_child();
    while let Some(ch) = c {
        if ch.is_visible() {
            pages.push(st.child_name(&ch).map(|s| s.to_string()).unwrap_or_default());
        }
        c = ch.next_sibling();
    }
    eprintln!(
        "SELFTEST {tag}: title={:?} header={:?} page={} entry={:?} total={:?} zoom={:?} sizing={} cont={} dual={} sidebar={:?} cur={} find={:?} results={} pres={} | {}",
        w.win.title().map(|s| s.to_string()),
        w.title_label.text().to_string(),
        w.model.page(),
        w.page_entry.text().to_string(),
        w.page_total.text().to_string(),
        w.zoom_entry.text().to_string(),
        w.model.sizing_mode().as_str(),
        w.model.continuous(),
        w.model.dual_page(),
        pages,
        sb.current_page(),
        w.find.status.text().to_string(),
        w.view.find_n_results(),
        w.is_presentation(),
        w.view.debug_state()
    );
}

fn diag_widths(w: &Rc<Window>, tag: &str) {
    let m = |x: &gtk::Widget| x.preferred_width().0;
    let sb = w.sidebar();
    let start = w.paned.child1().unwrap();
    eprintln!("DIAG {tag}: paned_pos={} start_alloc={} start_min={} sidebar_min={}", w.paned.position(), start.allocated_width(), m(&start), m(sb.widget.upcast_ref()));
    let mut c = start.first_child();
    while let Some(ch) = c {
        eprintln!("DIAG   side_stack child {} min={}", ch.type_().name(), m(&ch));
        c = ch.next_sibling();
    }
    let mut c = sb.widget.first_child();
    while let Some(ch) = c {
        eprintln!("DIAG   sidebar child {} min={}", ch.type_().name(), m(&ch));
        c = ch.next_sibling();
    }
    let mut c = sb.stack.first_child();
    while let Some(ch) = c {
        eprintln!("DIAG   page {:?} visible={} min={}", sb.stack.child_name(&ch), ch.is_child_visible(), m(&ch));
        c = ch.next_sibling();
    }
}

fn dump_tree(w: &gtk::Widget, depth: usize) {
    if depth > 9 {
        return;
    }
    let (min, nat) = w.preferred_width();
    let classes: Vec<String> = w.style_context().list_classes().iter().map(|c| c.to_string()).collect();
    eprintln!("DIAG {}{} vis={} css={:?} min={} nat={}", "  ".repeat(depth), w.type_().name(), w.is_visible(), classes, min, nat);
    if let Some(c) = w.downcast_ref::<gtk::Container>() {
        for ch in c.children() {
            dump_tree(&ch, depth + 1);
        }
    }
}

fn download(s: &cairo::ImageSurface) -> (Vec<u8>, usize) {
    s.flush();
    let stride = s.stride() as usize;
    let mut buf = vec![0u8; stride * s.height() as usize];
    let n = buf.len();
    let _ = s.with_data(|d| buf.copy_from_slice(&d[..n]));
    (buf, stride)
}

fn save_crop(path: &std::path::Path, buf: &[u8], stride: usize, x: usize, y: usize, w: usize, h: usize) {
    let Ok(mut surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32) else { return };
    let ss = surf.stride() as usize;
    {
        let mut d = surf.data().unwrap();
        for r in 0..h {
            d[r * ss..r * ss + w * 4].copy_from_slice(&buf[(y + r) * stride + x * 4..][..w * 4]);
        }
    }
    if let Ok(mut f) = std::fs::File::create(path) {
        let _ = surf.write_to_png(&mut f);
    }
}

/// Renders the window offscreen at the device scale and compares page pixels with the page texture.
fn offscreen_check(w: &Rc<Window>) {
    let Some(out) = std::env::var_os("EVINCE_RS_DIAG_OUT").map(std::path::PathBuf::from) else { return };
    let ds = w.view.device_scale();
    let view = &w.view;
    let (vw, vh) = (view.allocated_width(), view.allocated_height());
    let Ok(full) = cairo::ImageSurface::create(cairo::Format::ARgb32, (vw as f64 * ds) as i32, (vh as f64 * ds) as i32) else { return };
    full.set_device_scale(ds, ds);
    if let Ok(cr) = cairo::Context::new(&full) {
        view.draw(&cr);
    }
    let page = w.model.page();
    let Some((px, py, tex)) = w.view.debug_page_device(page) else {
        eprintln!("DIAG offscreen: page {page} not drawn");
        return;
    };
    let (fb, fs) = download(&full);
    let (tb, ts) = download(&tex);
    let (x0, y0) = (px.max(0.0) as usize, py.max(0.0) as usize);
    let (sx, sy) = ((x0 as f64 - px) as usize, (y0 as f64 - py) as usize);
    let cw = (full.width() as usize).saturating_sub(x0).min((tex.width() as usize).saturating_sub(sx)).min(500);
    let ch = (full.height() as usize).saturating_sub(y0).min((tex.height() as usize).saturating_sub(sy)).min(400);
    if cw < 40 || ch < 40 {
        eprintln!("DIAG offscreen: page {page} not visible enough to compare");
        return;
    }
    let mut best = (f64::MAX, 0i32, 0i32);
    for dy in -12i32..=12 {
        for dx in -12i32..=12 {
            let (mut sum, mut n) = (0f64, 0f64);
            for r in (14..ch.saturating_sub(14)).step_by(2) {
                for c in 14..cw.saturating_sub(14) {
                    let fx = (x0 + c) as i32 + dx;
                    let fy = (y0 + r) as i32 + dy;
                    if fx < 0 || fy < 0 || fx as usize >= full.width() as usize || fy as usize >= full.height() as usize {
                        continue;
                    }
                    let a = &fb[fy as usize * fs + fx as usize * 4..][..3];
                    let b = &tb[(sy + r) * ts + (sx + c) * 4..][..3];
                    sum += a.iter().zip(b).map(|(p, q)| (*p as f64 - *q as f64).abs()).sum::<f64>();
                    n += 3.0;
                }
            }
            if n > 0.0 && sum / n < best.0 {
                best = (sum / n, dx, dy);
            }
        }
    }
    eprintln!(
        "DIAG offscreen: full={}x{} page_device_origin=({px},{py}) tex={}x{} crop={cw}x{ch} best_mean_abs_diff={:.3} at shift=({},{})",
        full.width(),
        full.height(),
        tex.width(),
        tex.height(),
        best.0,
        best.1,
        best.2
    );
    let sharp = |b: &[u8], st: usize, x: usize, y: usize| {
        let mut g = 0f64;
        for r in 1..ch - 1 {
            for c in 1..cw - 1 {
                let v = |cc: usize, rr: usize| b[(y + rr) * st + (x + cc) * 4 + 1] as f64;
                g += (v(c + 1, r) - v(c, r)).abs() + (v(c, r + 1) - v(c, r)).abs();
            }
        }
        g / ((cw * ch) as f64)
    };
    eprintln!("DIAG sharpness: screen={:.3} texture={:.3}", sharp(&fb, fs, x0, y0), sharp(&tb, ts, sx, sy));
    save_crop(&out.join("screen-crop.png"), &fb, fs, x0, y0, cw, ch);
    save_crop(&out.join("texture-crop.png"), &tb, ts, sx, sy, cw, ch);
}

fn fake_scroll(w: &Rc<Window>, smooth: bool, dy: f64, time: u32, stop: bool) {
    let Some(win) = w.view.window() else { return };
    let mut ev = gtk::gdk::Event::new(gtk::gdk::EventType::Scroll);
    unsafe {
        use glib::translate::*;
        let p = ToGlibPtrMut::<*mut gtk::gdk::ffi::GdkEvent>::to_glib_none_mut(&mut ev).0 as *mut gtk::gdk::ffi::GdkEventScroll;
        (*p).window = win.to_glib_full();
        (*p).send_event = 0;
        (*p).time = time;
        (*p).is_stop = stop as u32;
        (*p).x = 200.0;
        (*p).y = 200.0;
        (*p).direction = if smooth { gtk::gdk::ffi::GDK_SCROLL_SMOOTH } else { gtk::gdk::ffi::GDK_SCROLL_DOWN };
        (*p).delta_x = 0.0;
        (*p).delta_y = dy;
    }
    if let Some(dev) = gtk::gdk::Display::default().and_then(|d| d.default_seat()).and_then(|s| s.pointer()) {
        ev.set_device(Some(&dev));
        ev.set_source_device(Some(&dev));
    }
    let before = w.scrolled.vadjustment().value();
    gtk::main_do_event(&mut ev);
    let after = w.scrolled.vadjustment().value();
    let _ = (before, after);
}

/// Feeds synthetic scroll events through GTK and reports the resulting scroll (EVINCE_RS_SCROLLTEST).
pub fn scrolltest(w: &Rc<Window>) {
    let mut steps: Vec<Step> = vec![(1500, Box::new(|w| {
        gio::prelude::ActionGroupExt::change_action_state(&w.win, "continuous", &false.to_variant());
        gio::prelude::ActionGroupExt::change_action_state(&w.win, "sizing-mode", &"fit-page".to_variant());
        w.model.set_page(0);
    }))];
    for swipe in 0..2u32 {
        for i in 0..40u32 {
            steps.push((5, Box::new(move |w| fake_scroll(w, true, 2.0, 10_000 * (swipe + 1) + i * 8, false))));
        }
        steps.push((5, Box::new(move |w| fake_scroll(w, true, 0.0, 10_000 * (swipe + 1) + 400, true))));
        steps.push((800, Box::new(move |w| eprintln!("FLIPTEST after swipe {}: page={}", swipe + 1, w.model.page()))));
    }
    steps.push((300, Box::new(|w| w.win.close())));
    run(w.clone(), steps, 0);
}

/// Prints render and sidebar geometry, then closes (EVINCE_RS_DIAG).
pub fn diag(w: &Rc<Window>) {
    let steps: Vec<Step> = vec![
        (1500, Box::new(|w| {
            eprintln!("DIAG render: {}", w.view.debug_render());
            offscreen_check(w);
            diag_widths(w, "initial");
            w.paned.set_position(0);
        })),
        (600, Box::new(|w| {
            diag_widths(w, "after set_position(0)");
            if std::env::var_os("EVINCE_RS_DIAG_TREE").is_some() {
                let sb = w.sidebar();
                sb.set_current_page(super::sidebar::THUMBNAILS);
                eprintln!("DIAG ---- paned start tree");
                dump_tree(&w.paned.child1().unwrap(), 0);
                eprintln!("DIAG ---- sidebar tree (thumbnails shown)");
                dump_tree(sb.widget.upcast_ref(), 0);
                sb.set_current_page(super::sidebar::LINKS);
                eprintln!("DIAG ---- outline page");
                dump_tree(&sb.stack.child_by_name(super::sidebar::LINKS).unwrap(), 0);
            }
        })),
        (1500, Box::new(|w| w.win.close())),
    ];
    run(w.clone(), steps, 0);
}

/// Drives a scripted session when EVINCE_RS_SELFTEST is set (debug builds only).
pub fn start(w: &Rc<Window>) {
    let steps: Vec<Step> = vec![
        (1500, Box::new(|w| report(w, "loaded"))),
        (100, Box::new(|w| {
            act(w, "find");
            w.find.entry.set_text("findme");
        })),
        (1500, Box::new(|w| report(w, "find"))),
        (100, Box::new(|w| act(w, "escape"))),
        (300, Box::new(|w| report(w, "find-closed"))),
        (100, Box::new(|w| act(w, "go-next-page"))),
        (800, Box::new(|w| report(w, "next-page"))),
        (100, Box::new(|w| act(w, "zoom-in"))),
        (800, Box::new(|w| report(w, "zoom-in"))),
        (100, Box::new(|w| act(w, "rotate-right"))),
        (800, Box::new(|w| report(w, "rotated"))),
        (100, Box::new(|w| {
            act(w, "rotate-left");
            gio::prelude::ActionGroupExt::change_action_state(&w.win, "sizing-mode", &"fit-page".to_variant());
        })),
        (800, Box::new(|w| report(w, "fit-page"))),
        (100, Box::new(|w| gio::prelude::ActionGroupExt::change_action_state(&w.win, "dual-page", &true.to_variant()))),
        (800, Box::new(|w| report(w, "dual"))),
        (100, Box::new(|w| {
            gio::prelude::ActionGroupExt::change_action_state(&w.win, "dual-page", &false.to_variant());
            gio::prelude::ActionGroupExt::change_action_state(&w.win, "continuous", &false.to_variant());
        })),
        (800, Box::new(|w| report(w, "single"))),
        (100, Box::new(|w| {
            gio::prelude::ActionGroupExt::change_action_state(&w.win, "continuous", &true.to_variant());
            act(w, "select-all");
        })),
        (800, Box::new(|w| {
            report(w, "select-all");
            eprintln!("SELFTEST selected-text={:?}", w.view.selected_text().chars().take(60).collect::<String>());
        })),
        (100, Box::new(|w| act(w, "go-back-history"))),
        (600, Box::new(|w| report(w, "history-back"))),
        (100, Box::new(|w| act(w, "add-bookmark"))),
        (300, Box::new(|w| {
            eprintln!("SELFTEST metadata={}", w.metadata().is_some());
            let n = w.bookmarks.borrow().as_ref().map_or(0, |b| b.items().len());
            eprintln!("SELFTEST bookmarks={n}");
        })),
        (100, Box::new(|w| gio::prelude::ActionGroupExt::change_action_state(&w.win, "presentation", &true.to_variant()))),
        (800, Box::new(|w| report(w, "presentation"))),
        (100, Box::new(|w| act(w, "escape"))),
        (800, Box::new(|w| report(w, "presentation-closed"))),
        (1500, Box::new(|w| w.win.close())),
    ];
    run(w.clone(), steps, 0);
}

fn run(w: Rc<Window>, steps: Vec<Step>, i: usize) {
    if i >= steps.len() {
        return;
    }
    let delay = steps[i].0;
    glib::timeout_add_local_once(Duration::from_millis(delay), move || {
        (steps[i].1)(&w);
        run(w, steps, i + 1);
    });
}
