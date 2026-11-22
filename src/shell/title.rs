const BAD_EXTENSIONS: &[(&str, &str)] = &[
    ("ps", ".dvi"),
    ("pdf", ".doc"),
    ("pdf", ".dvi"),
    ("pdf", ".indd"),
    ("pdf", ".rtf"),
];

const BAD_PREFIXES: &[(&str, &str)] = &[("pdf", "Microsoft Word - "), ("pdf", "Microsoft PowerPoint - ")];

fn backend_of(mime: &str) -> &'static str {
    if mime.contains("pdf") {
        "pdf"
    } else if mime.contains("postscript") {
        "ps"
    } else {
        ""
    }
}

/// Removes misleading extensions/prefixes some producers put in titles.
pub fn sanitize_title(title: &str, mime: &str) -> String {
    let backend = backend_of(mime);
    let mut t = title.to_string();
    for (b, ext) in BAD_EXTENSIONS {
        if *b == backend && t.to_ascii_lowercase().ends_with(&ext.to_ascii_lowercase()) {
            t.truncate(t.len() - ext.len());
        }
    }
    for (b, pre) in BAD_PREFIXES {
        if *b == backend && t.to_ascii_lowercase().starts_with(&pre.to_ascii_lowercase()) {
            t = t[pre.len()..].to_string();
        }
    }
    t
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TitleKind {
    Document,
    Password,
    Recent,
}

pub struct Titles {
    pub window: String,
    pub header: String,
    pub subtitle: Option<String>,
}

pub fn compute(kind: TitleKind, filename: Option<&str>, doc_title: Option<&str>, ltr: bool) -> Titles {
    if kind == TitleKind::Recent {
        return Titles { window: "Recent Documents".into(), header: "Evince RS".into(), subtitle: None };
    }
    let doc_title = doc_title.map(str::trim).filter(|t| !t.is_empty());
    let (title, header, sub) = match (doc_title, filename) {
        (Some(t), Some(f)) => {
            let s = if ltr { format!("{f} — {t}") } else { format!("{t} — {f}") };
            (s.replace('\n', " "), Some(t.to_string()), Some(f.to_string()))
        }
        (_, Some(f)) => (f.to_string(), None, None),
        _ => ("Evince RS".to_string(), None, None),
    };
    match kind {
        TitleKind::Password => {
            let w = if ltr { format!("{title} — Password Required") } else { format!("Password Required — {title}") };
            Titles { window: w, header: "Password Required".into(), subtitle: Some(title) }
        }
        _ => match (header, sub) {
            (Some(h), Some(s)) => Titles { window: title, header: h, subtitle: Some(s) },
            _ => Titles { window: title.clone(), header: title, subtitle: None },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        let t = compute(TitleKind::Document, Some("a.pdf"), Some("Doc"), true);
        assert_eq!(t.window, "a.pdf — Doc");
        assert_eq!(t.header, "Doc");
        let t = compute(TitleKind::Document, Some("a.pdf"), None, true);
        assert_eq!(t.window, "a.pdf");
        let t = compute(TitleKind::Password, Some("a.pdf"), None, true);
        assert_eq!(t.window, "a.pdf — Password Required");
        assert_eq!(sanitize_title("Report.doc", "application/pdf"), "Report");
        assert_eq!(sanitize_title("Microsoft Word - Report", "application/pdf"), "Report");
    }
}
