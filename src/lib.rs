pub use chrono;

mod config;
mod crawl;
mod error;
mod parse;

pub use config::*;
pub use crawl::*;
pub use error::Error;
pub use parse::*;

pub const USER_AGENT: &str = "somes/2026.1.0";

pub fn http_client() -> Result<reqwest::Client, Error> {
    Ok(somes_scraper::http_client()?)
}

pub enum UrlOrSitemap {
    Url(String),
    Sitemap(Sitemap),
}

impl UrlOrSitemap {
    pub fn url(&self) -> &str {
        match self {
            UrlOrSitemap::Url(url) => url,
            UrlOrSitemap::Sitemap(sitemap) => &sitemap.url,
        }
    }
}
