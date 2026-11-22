use evince_rs::document::factory;
use gio::prelude::*;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const THUMBNAIL_SIZE: i32 = 128;
const USAGE: &str = "Usage:\n  evince-rs-thumbnailer [OPTION…] <input> <output>\n\n- Evince RS Thumbnailer\n\nHelp Options:\n  -h, --help        Show help options\n\nApplication Options:\n  -s, --size=SIZE   \n  -l, --no-limit    Don't limit the thumbnailing time to 15 seconds\n";

static FINISHED: AtomicBool = AtomicBool::new(true);

fn local_uri(file: &gio::File) -> Option<String> {
    if file.has_uri_scheme("trash") || file.has_uri_scheme("recent") {
        let info = file
            .query_info(gio::FILE_ATTRIBUTE_STANDARD_TARGET_URI, gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
            .ok()?;
        return info.attribute_string(gio::FILE_ATTRIBUTE_STANDARD_TARGET_URI).map(|s| s.to_string());
    }
    Some(file.uri().to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut size = THUMBNAIL_SIZE;
    let mut limit = true;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if a == "-h" || a == "--help" {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        } else if a == "-l" || a == "--no-limit" {
            limit = false;
        } else if let Some(v) = a.strip_prefix("--size=").or_else(|| a.strip_prefix("-s").filter(|v| !v.is_empty())) {
            match v.parse() {
                Ok(n) => size = n,
                Err(_) => {
                    eprintln!("Cannot parse integer value “{v}” for --size");
                    return ExitCode::from(255);
                }
            }
        } else if a == "-s" || a == "--size" {
            let Some(v) = args.get(i) else {
                eprintln!("Missing argument for {a}");
                return ExitCode::from(255);
            };
            i += 1;
            match v.parse() {
                Ok(n) => size = n,
                Err(_) => {
                    eprintln!("Cannot parse integer value “{v}” for --size");
                    return ExitCode::from(255);
                }
            }
        } else {
            files.push(a.clone());
        }
    }
    if files.len() < 2 {
        print!("{USAGE}");
        return ExitCode::from(255);
    }
    if size < 1 {
        eprintln!("Size cannot be smaller than 1 pixel");
        return ExitCode::from(255);
    }
    let input = files[0].clone();
    let output = files[1].clone();
    let file = gio::File::for_commandline_arg(&input);
    let Some(uri) = local_uri(&file) else { return ExitCode::from(254) };
    let mut loaded = match factory::load_document(&uri, None) {
        Ok(l) => l,
        Err(evince_rs::document::Error::Encrypted) => return ExitCode::from(254),
        Err(e) => {
            eprintln!("Error loading document: {e}");
            return ExitCode::from(254);
        }
    };
    if limit {
        FINISHED.store(false, Ordering::SeqCst);
        let name = input.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(15));
            if !FINISHED.load(Ordering::SeqCst) {
                eprintln!("evince-rs-thumbnailer couldn't process file: '{name}'\nReason: Took too much time to process.");
                std::process::exit(0);
            }
        });
    }
    let doc = &mut loaded.doc;
    if doc.n_pages() == 0 {
        return ExitCode::from(254);
    }
    let (w, h) = doc.page_size(0);
    let scale = size as f64 / w.max(h).max(1.0);
    let Some(surface) = doc.render(0, scale, 0) else { return ExitCode::from(254) };
    let out = match std::fs::File::create(&output) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error saving thumbnail: {e}");
            return ExitCode::from(254);
        }
    };
    let mut out = std::io::BufWriter::new(out);
    let r = surface.write_to_png(&mut out);
    FINISHED.store(true, Ordering::SeqCst);
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error saving thumbnail: {e}");
            let _ = std::fs::remove_file(&output);
            ExitCode::from(254)
        }
    }
}
