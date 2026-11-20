//! Offset/limit pagination parameters and page envelope types.

/// Clamped pagination request parameters (`limit` within `1..=100`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PageParams {
    pub limit: u32,
    pub offset: u32,
}

impl PageParams {
    /// Minimum allowed page size.
    pub const MIN_LIMIT: u32 = 1;
    /// Maximum allowed page size.
    pub const MAX_LIMIT: u32 = 100;
    /// Default page size.
    pub const DEFAULT_LIMIT: u32 = 20;

    /// Constructs pagination parameters clamping `limit` into `1..=100`.
    #[must_use]
    pub const fn new(limit: u32, offset: u32) -> Self {
        let clamped_limit = if limit < Self::MIN_LIMIT {
            Self::MIN_LIMIT
        } else if limit > Self::MAX_LIMIT {
            Self::MAX_LIMIT
        } else {
            limit
        };
        Self {
            limit: clamped_limit,
            offset,
        }
    }
}

impl Default for PageParams {
    fn default() -> Self {
        Self {
            limit: Self::DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

/// Paginated response envelope containing a slice of items and total count metadata.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

impl<T> Paged<T> {
    /// Constructs a [`Paged<T>`] envelope from items, total count, and [`PageParams`].
    #[must_use]
    pub const fn new(items: Vec<T>, total: u64, params: PageParams) -> Self {
        Self {
            items,
            total,
            limit: params.limit,
            offset: params.offset,
        }
    }

    /// Returns `true` when additional items exist beyond `offset + items.len()`.
    #[must_use]
    pub fn has_more(&self) -> bool {
        let item_count = u64::try_from(self.items.len()).unwrap_or(u64::MAX);
        let covered = u64::from(self.offset).saturating_add(item_count);
        covered < self.total
    }
}

#[cfg(test)]
mod tests {
    use super::{PageParams, Paged};

    #[test]
    fn page_params_default_is_twenty_from_zero() {
        assert_eq!(
            PageParams::default(),
            PageParams {
                limit: 20,
                offset: 0,
            }
        );
    }

    #[test]
    fn page_params_clamps_out_of_bounds_limits() {
        assert_eq!(
            PageParams::new(0, 5),
            PageParams {
                limit: 1,
                offset: 5
            }
        );
        assert_eq!(
            PageParams::new(1, 0),
            PageParams {
                limit: 1,
                offset: 0
            }
        );
        assert_eq!(
            PageParams::new(50, 10),
            PageParams {
                limit: 50,
                offset: 10
            }
        );
        assert_eq!(
            PageParams::new(100, 0),
            PageParams {
                limit: 100,
                offset: 0
            }
        );
        assert_eq!(
            PageParams::new(500, 40),
            PageParams {
                limit: 100,
                offset: 40
            }
        );
    }

    #[test]
    fn paged_has_more_tracks_remaining_rows() {
        let first_page = Paged::new(vec!["a", "b"], 5, PageParams::new(2, 0));
        assert!(first_page.has_more());

        let final_page = Paged::new(vec!["e"], 5, PageParams::new(2, 4));
        assert!(!final_page.has_more());

        let empty_total = Paged::<u32>::new(Vec::new(), 0, PageParams::default());
        assert!(!empty_total.has_more());
    }
}
