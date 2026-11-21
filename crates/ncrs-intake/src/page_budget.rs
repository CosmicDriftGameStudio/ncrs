//! Bounds the memory that the page images of one document may take together.

use crate::text::PageImage;

/// Grey bytes of all page images of one document together. A page is rendered
/// only while the pages before it are below this, so the peak is the budget
/// plus one page. An ordinary scan stays far below it.
pub const MAX_SCAN_TOTAL_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug)]
pub struct PageBudget {
    limit: usize,
    used: usize,
}

impl PageBudget {
    pub fn new(limit: usize) -> Self {
        Self { limit, used: 0 }
    }

    pub fn has_room(&self) -> bool {
        self.used < self.limit
    }

    pub fn spend(&mut self, page: &PageImage) {
        self.used = self.used.saturating_add(page.pixels.len());
    }
}

/// Produces the pages from `sources` in order until the budget is used up;
/// later sources are not even produced. The first error stops it.
pub fn collect_within_budget<S, E>(
    sources: impl IntoIterator<Item = S>,
    limit: usize,
    mut produce: impl FnMut(S) -> Result<PageImage, E>,
) -> Result<Vec<PageImage>, E> {
    let mut budget = PageBudget::new(limit);
    let mut pages = Vec::new();
    for source in sources {
        if !budget.has_room() {
            break;
        }
        let page = produce(source)?;
        budget.spend(&page);
        pages.push(page);
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(bytes: usize) -> PageImage {
        PageImage {
            width: u32::try_from(bytes).unwrap(),
            height: 1,
            pixels: vec![0; bytes],
        }
    }

    #[test]
    fn stops_producing_once_the_budget_is_used_up() {
        let mut produced = Vec::new();
        let pages = collect_within_budget(0..10, 25, |number| {
            produced.push(number);
            Ok::<_, ()>(page(10))
        })
        .unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(produced, [0, 1, 2]);
    }

    #[test]
    fn a_document_below_the_budget_keeps_every_page() {
        let pages = collect_within_budget(0..5, 1000, |_| Ok::<_, ()>(page(10))).unwrap();
        assert_eq!(pages.len(), 5);
    }

    #[test]
    fn one_page_larger_than_the_budget_is_still_read() {
        let pages = collect_within_budget(0..3, 5, |_| Ok::<_, ()>(page(100))).unwrap();
        assert_eq!(pages.len(), 1);
    }

    #[test]
    fn an_error_stops_the_collection() {
        let result = collect_within_budget(0..3, 1000, |number| {
            if number == 1 {
                Err("broken")
            } else {
                Ok(page(1))
            }
        });
        assert_eq!(result, Err("broken"));
    }
}
