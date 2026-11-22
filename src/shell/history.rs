const MAX_LENGTH: usize = 32;

/// Back/forward navigation history of visited pages (EvHistory).
#[derive(Default)]
pub struct History {
    list: Vec<usize>,
    current: Option<usize>,
    frozen: u32,
}

impl History {
    pub fn add_page(&mut self, page: usize) {
        if self.frozen > 0 {
            return;
        }
        if let Some(c) = self.current {
            if self.list[c] == page {
                return;
            }
            self.list.truncate(c + 1);
        }
        self.list.push(page);
        if self.list.len() > MAX_LENGTH {
            self.list.remove(0);
        }
        self.current = Some(self.list.len() - 1);
    }

    pub fn can_go_back(&self) -> bool {
        self.frozen == 0 && self.current.is_some_and(|c| c > 0)
    }

    pub fn can_go_forward(&self) -> bool {
        self.frozen == 0 && self.current.is_some_and(|c| c + 1 < self.list.len())
    }

    pub fn go_back(&mut self) -> Option<usize> {
        if !self.can_go_back() {
            return None;
        }
        let c = self.current? - 1;
        self.current = Some(c);
        Some(self.list[c])
    }

    pub fn go_forward(&mut self) -> Option<usize> {
        if !self.can_go_forward() {
            return None;
        }
        let c = self.current? + 1;
        self.current = Some(c);
        Some(self.list[c])
    }

    pub fn freeze(&mut self) {
        self.frozen += 1;
    }

    pub fn thaw(&mut self) {
        self.frozen = self.frozen.saturating_sub(1);
    }

    pub fn clear(&mut self) {
        self.list.clear();
        self.current = None;
    }

    pub fn back_list(&self) -> Vec<usize> {
        match self.current {
            Some(c) => self.list[..c].iter().rev().copied().collect(),
            None => vec![],
        }
    }

    pub fn forward_list(&self) -> Vec<usize> {
        match self.current {
            Some(c) => self.list[c + 1..].to_vec(),
            None => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_forward() {
        let mut h = History::default();
        h.add_page(0);
        h.add_page(5);
        h.add_page(9);
        assert_eq!(h.go_back(), Some(5));
        assert_eq!(h.go_back(), Some(0));
        assert!(!h.can_go_back());
        assert_eq!(h.go_forward(), Some(5));
        h.add_page(3);
        assert!(!h.can_go_forward());
        assert_eq!(h.back_list(), vec![5, 0]);
    }

    #[test]
    fn bounded() {
        let mut h = History::default();
        for i in 0..100 {
            h.add_page(i);
        }
        assert_eq!(h.back_list().len(), MAX_LENGTH - 1);
    }
}
