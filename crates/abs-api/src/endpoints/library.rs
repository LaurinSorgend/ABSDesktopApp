use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::Raw;
use crate::client::AbsClient;
use crate::error::Result;
use crate::models::{
    Authors, Collection, Libraries, Library, LibraryItem, Page, SearchResults, Series, Shelf,
};

/// Paging, sorting and filtering for `GET /api/libraries/:id/items`.
#[derive(Debug, Clone, Default)]
pub struct ItemsQuery {
    /// Items per page; 0 returns all items.
    pub limit: u32,
    pub page: u32,
    /// Sort key such as `media.metadata.title` or `addedAt`.
    pub sort: Option<String>,
    pub desc: bool,
    pub filter: Option<ItemFilter>,
    pub collapse_series: bool,
}

/// A library filter such as authors, series, genres, tags or progress.
#[derive(Debug, Clone)]
pub struct ItemFilter {
    pub group: String,
    pub value: String,
}

impl ItemFilter {
    pub fn new(group: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            group: group.into(),
            value: value.into(),
        }
    }

    /// The `group.base64(value)` form the server expects.
    pub fn encode(&self) -> String {
        format!("{}.{}", self.group, STANDARD.encode(&self.value))
    }
}

impl ItemsQuery {
    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = vec![
            ("minified", "1".to_owned()),
            ("limit", self.limit.to_string()),
            ("page", self.page.to_string()),
        ];
        if let Some(sort) = &self.sort {
            pairs.push(("sort", sort.clone()));
        }
        if self.desc {
            pairs.push(("desc", "1".to_owned()));
        }
        if let Some(filter) = &self.filter {
            pairs.push(("filter", filter.encode()));
        }
        if self.collapse_series {
            pairs.push(("collapseseries", "1".to_owned()));
        }
        pairs
    }
}

impl AbsClient {
    /// `GET /api/libraries`: the libraries this user can access.
    pub async fn libraries(&self) -> Result<Vec<Raw<Library>>> {
        let list: Libraries = self
            .send_json(self.http().get(self.url("api/libraries")?))
            .await?;
        Ok(list.libraries)
    }

    /// `GET /api/libraries/:id/items`, minified.
    pub async fn library_items(
        &self,
        library_id: &str,
        query: &ItemsQuery,
    ) -> Result<Page<Raw<LibraryItem>>> {
        let url = self.url(&format!("api/libraries/{library_id}/items"))?;
        self.send_json(self.http().get(url).query(&query.pairs()))
            .await
    }

    /// `GET /api/libraries/:id/series`.
    pub async fn library_series(
        &self,
        library_id: &str,
        limit: u32,
        page: u32,
    ) -> Result<Page<Series>> {
        self.paged(library_id, "series", limit, page).await
    }

    /// `GET /api/libraries/:id/collections`.
    pub async fn library_collections(
        &self,
        library_id: &str,
        limit: u32,
        page: u32,
    ) -> Result<Page<Collection>> {
        self.paged(library_id, "collections", limit, page).await
    }

    /// `GET /api/libraries/:id/authors`.
    pub async fn library_authors(&self, library_id: &str) -> Result<Authors> {
        let url = self.url(&format!("api/libraries/{library_id}/authors"))?;
        self.send_json(self.http().get(url)).await
    }

    /// `GET /api/libraries/:id/search`.
    pub async fn search(&self, library_id: &str, text: &str, limit: u32) -> Result<SearchResults> {
        let url = self.url(&format!("api/libraries/{library_id}/search"))?;
        let query = [("q", text.to_owned()), ("limit", limit.to_string())];
        self.send_json(self.http().get(url).query(&query)).await
    }

    /// `GET /api/libraries/:id/personalized`: the home page shelves.
    pub async fn personalized(&self, library_id: &str, limit: u32) -> Result<Vec<Shelf>> {
        let url = self.url(&format!("api/libraries/{library_id}/personalized"))?;
        self.send_json(self.http().get(url).query(&[("limit", limit)]))
            .await
    }

    async fn paged<T: serde::de::DeserializeOwned>(
        &self,
        library_id: &str,
        list: &str,
        limit: u32,
        page: u32,
    ) -> Result<Page<T>> {
        let url = self.url(&format!("api/libraries/{library_id}/{list}"))?;
        let query = [("limit", limit), ("page", page)];
        self.send_json(self.http().get(url).query(&query)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_filter_value() {
        let filter = ItemFilter::new("authors", "a1b2");
        assert_eq!(filter.encode(), "authors.YTFiMg==");
    }

    #[test]
    fn query_pairs_skip_unset_options() {
        let query = ItemsQuery {
            limit: 50,
            ..ItemsQuery::default()
        };
        let keys: Vec<_> = query.pairs().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["minified", "limit", "page"]);
    }
}
