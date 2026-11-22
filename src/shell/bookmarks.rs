use super::metadata::Metadata;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub page: u32,
    pub title: String,
}

/// User bookmarks persisted in the "bookmarks" metadata key as a GVariant a(us).
pub struct Bookmarks {
    metadata: Rc<Metadata>,
    items: Vec<Bookmark>,
}

impl Bookmarks {
    pub fn new(metadata: Rc<Metadata>) -> Bookmarks {
        let mut items = Vec::new();
        if let Some(s) = metadata.get_string("bookmarks").filter(|s| !s.is_empty()) {
            if let Ok(v) = glib::Variant::parse(Some(glib::VariantTy::new("a(us)").unwrap()), &s) {
                for child in v.iter() {
                    if let Some((page, title)) = child.get::<(u32, String)>() {
                        items.push(Bookmark { page, title });
                    }
                }
            }
        }
        items.sort_by_key(|b| b.page);
        Bookmarks { metadata, items }
    }

    pub fn items(&self) -> &[Bookmark] {
        &self.items
    }

    pub fn has_page(&self, page: u32) -> bool {
        self.items.iter().any(|b| b.page == page)
    }

    fn save(&self) {
        if self.items.is_empty() {
            self.metadata.set_string("bookmarks", "");
            return;
        }
        let v: Vec<(u32, String)> = self.items.iter().map(|b| (b.page, b.title.clone())).collect();
        self.metadata.set_string("bookmarks", &glib::Variant::from(v).print(false));
    }

    pub fn add(&mut self, bm: Bookmark) {
        if self.has_page(bm.page) {
            return;
        }
        self.items.push(bm);
        self.items.sort_by_key(|b| b.page);
        self.save();
    }

    pub fn delete(&mut self, page: u32) {
        self.items.retain(|b| b.page != page);
        self.save();
    }

    pub fn rename(&mut self, page: u32, title: &str) {
        if let Some(b) = self.items.iter_mut().find(|b| b.page == page) {
            b.title = title.to_string();
        }
        self.save();
    }
}
