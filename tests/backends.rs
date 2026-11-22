use evince_rs::document::*;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    assert!(p.exists(), "missing fixture {name}; run tests/make-fixtures.py");
    glib::filename_to_uri(&p, None).unwrap().to_string()
}

fn non_white(surf: &mut cairo::ImageSurface) -> usize {
    let stride = surf.stride() as usize;
    let (w, h) = (surf.width() as usize, surf.height() as usize);
    let d = surf.data().unwrap();
    let mut n = 0;
    for y in 0..h {
        for x in 0..w {
            let px = u32::from_ne_bytes(d[y * stride + x * 4..y * stride + x * 4 + 4].try_into().unwrap());
            if px & 0xffffff != 0xffffff {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn pdf_backend() {
    let mut l = load_document(&fixture("sample.pdf"), None).unwrap();
    let d = &mut l.doc;
    assert_eq!(d.n_pages(), 3);
    assert_eq!(d.page_size(0), (612.0, 792.0));
    let info = d.info();
    assert_eq!(info.title.as_deref(), Some("Sample Title"));
    assert_eq!(info.author.as_deref(), Some("Test Author"));
    assert!(info.format.unwrap().starts_with("PDF-"));

    let mut s = d.render(0, 0.5, 0).unwrap();
    assert_eq!((s.width(), s.height()), (306, 396));
    assert!(non_white(&mut s) > 100);
    let s = d.render(0, 0.5, 90).unwrap();
    assert_eq!((s.width(), s.height()), (396, 306));

    let text = d.page_text(1).unwrap();
    assert!(text.contains("findme"), "{text}");
    let layout = d.text_layout(0).unwrap();
    assert_eq!(layout.chars.len(), layout.rects.len());
    assert!(!layout.chars.is_empty());

    let hits = d.find(1, "FINDME", FindOptions::default());
    assert_eq!(hits.len(), 1);
    let r = hits[0].rects[0];
    assert!(r.y1 > 100.0 && r.y2 < 160.0, "find rect in top-left coordinates: {r:?}");
    assert_eq!(d.find(1, "FINDME", FindOptions { case_sensitive: true, whole_words: false }).len(), 0);
    assert_eq!(d.find(0, "findme", FindOptions::default()).len(), 0);

    let links = d.links(0);
    assert!(links.iter().any(|l| matches!(&l.data.action, Some(LinkAction::ExternalUri(u)) if u == "https://example.org/")));
    let internal = links
        .iter()
        .find_map(|l| match &l.data.action {
            Some(LinkAction::GotoDest(dest)) => Some(dest.clone()),
            _ => None,
        })
        .expect("internal link");
    let page = match internal {
        LinkDest::Named(n) => d.resolve_named_dest(&n).and_then(|d| d.page()),
        other => other.page(),
    };
    assert_eq!(page, Some(2));
    let ext = links.iter().find(|l| matches!(l.data.action, Some(LinkAction::ExternalUri(_)))).unwrap();
    assert!(ext.area.y1 > 170.0 && ext.area.y2 < 220.0, "{:?}", ext.area);

    let outline = d.outline();
    assert_eq!(outline.len(), 2);
    assert_eq!(outline[0].title, "Chapter 1");
    assert_eq!(outline[0].children[0].title, "Section 1.1");
    assert_eq!(d.page_label(0).as_deref(), Some("i"));

    let sel = d.selected_text(0, SelectionStyle::Line, Rect::new(80.0, 90.0, 80.0, 90.0));
    assert!(sel.contains("Hello World page 1"), "{sel:?}");
    assert!(!d.selection_region(0, SelectionStyle::Word, Rect::new(80.0, 90.0, 80.0, 90.0)).is_empty());

    assert!(!d.fonts().is_empty());
    assert!(d.attachments().is_empty());
}

#[test]
fn pdf_annotations_roundtrip() {
    let mut l = load_document(&fixture("sample.pdf"), None).unwrap();
    let d = &mut l.doc;
    assert!(d.annotations(0).is_empty());
    let note = d
        .add_annotation(0, &NewAnnot::Text { icon: TextIcon::Note }, Rect::new(400.0, 50.0, 424.0, 74.0), Color::rgb(1.0, 1.0, 0.0), "Me")
        .unwrap();
    assert!((note.area.y1 - 50.0).abs() < 0.01);
    let hl = d
        .add_annotation(
            0,
            &NewAnnot::TextMarkup(MarkupType::Highlight),
            Rect::new(72.0, 80.0, 300.0, 105.0),
            Color::rgb(1.0, 1.0, 0.0),
            "Me",
        )
        .unwrap();
    assert_eq!(hl.kind, AnnotKind::TextMarkup(MarkupType::Highlight));
    let mut n2 = note.clone();
    n2.contents = "Hello note".into();
    d.update_annotation(&n2);
    let annots = d.annotations(0);
    assert_eq!(annots.len(), 2);
    assert!(annots.iter().any(|a| a.contents == "Hello note" && a.label.as_deref() == Some("Me")));
    assert!(d.is_modified());

    let out = std::env::temp_dir().join(format!("evince-rs-annot-{}.pdf", std::process::id()));
    d.save(&out).unwrap();
    let uri = glib::filename_to_uri(&out, None).unwrap();
    let mut l2 = load_document(&uri, None).unwrap();
    let annots = l2.doc.annotations(0);
    assert_eq!(annots.len(), 2, "annotations persisted after save");
    assert!(annots.iter().any(|a| a.contents == "Hello note"));
    l2.doc.remove_annotation(0, annots[0].id);
    assert_eq!(l2.doc.annotations(0).len(), 1);
    let _ = std::fs::remove_file(out);
}

#[test]
fn gzipped_pdf() {
    let l = load_document(&fixture("sample.pdf.gz"), None).unwrap();
    assert_eq!(l.doc.n_pages(), 3);
    assert_eq!(l.temps.len(), 1);
    let tmp = l.temps[0].0.clone();
    assert!(tmp.exists());
    drop(l);
    assert!(!tmp.exists(), "temporary uncompressed copy removed");
}

#[test]
#[cfg(feature = "ps")]
fn ps_backend() {
    let mut l = load_document(&fixture("sample.ps"), None).unwrap();
    let d = &mut l.doc;
    assert_eq!(d.n_pages(), 2);
    let (w, h) = d.page_size(0);
    assert_eq!((w, h), (612.0, 792.0));
    let mut s = d.render(0, 0.5, 0).unwrap();
    assert_eq!((s.width(), s.height()), (306, 396));
    assert!(non_white(&mut s) > 100);
}

#[test]
fn comics_backend() {
    let mut l = load_document(&fixture("sample.cbz"), None).unwrap();
    let d = &mut l.doc;
    assert_eq!(d.n_pages(), 3, "junk and non-image entries skipped");
    assert_eq!(d.page_size(0), (300.0, 400.0), "pages sorted by name");
    assert_eq!(d.page_size(2), (400.0, 400.0));
    let mut s = d.render(1, 0.5, 0).unwrap();
    assert_eq!((s.width(), s.height()), (175, 200));
    assert!(non_white(&mut s) > 50);
    let s = d.render(1, 0.5, 270).unwrap();
    assert_eq!((s.width(), s.height()), (200, 175));
}

#[test]
fn tiff_backend() {
    let mut l = load_document(&fixture("sample.tiff"), None).unwrap();
    let d = &mut l.doc;
    assert_eq!(d.n_pages(), 2);
    assert_eq!(d.page_size(0), (300.0, 400.0));
    assert_eq!(d.page_size(1), (350.0, 400.0));
    let mut s = d.render(1, 1.0, 180).unwrap();
    assert_eq!((s.width(), s.height()), (350, 400));
    assert!(non_white(&mut s) > 100);
}

#[test]
#[cfg(feature = "djvu")]
fn djvu_backend() {
    let mut l = load_document(&fixture("sample.djvu"), None).unwrap();
    let d = &mut l.doc;
    assert_eq!(d.n_pages(), 2);
    let (w, h) = d.page_size(0);
    assert!((w - 300.0 * 72.0 / 100.0).abs() < 1.0 && (h - 400.0 * 72.0 / 100.0).abs() < 1.0, "{w}x{h}");
    let mut s = d.render(0, 1.0, 90).unwrap();
    assert_eq!((s.width(), s.height()), (288, 216));
    assert!(non_white(&mut s) > 100);
    assert!(d.outline().is_empty());
}

#[test]
fn unsupported_type() {
    let p = std::env::temp_dir().join(format!("evince-rs-plain-{}.txt", std::process::id()));
    std::fs::write(&p, "just text").unwrap();
    let uri = glib::filename_to_uri(&p, None).unwrap();
    match load_document(&uri, None) {
        Err(Error::UnsupportedType(_)) => {}
        Err(e) => panic!("unexpected error {e}"),
        Ok(_) => panic!("text file must not load"),
    }
    let _ = std::fs::remove_file(p);
}
