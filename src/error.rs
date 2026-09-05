use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("http error for {url}: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("xml error in {url}: {message}")]
    Parse { url: String, message: String },
    #[error("no sitemap content at {url} after {attempts} attempts")]
    Empty { url: String, attempts: u32 },
    #[error("sitemap {url} returned http status {code}")]
    Status { url: String, code: u16 },
    #[error("io error: {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("config error: {0}")]
    Config(String),
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("client error: {0}")]
    Client(#[from] reqwest::Error),
}
