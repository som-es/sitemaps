use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use chrono::{Datelike, NaiveDate};
use futures::future::join_all;
use regex::Regex;

use crate::config::{url_date, Config, Provider, KIND_ARCHIVE, KIND_NEWS};
use crate::error::Error;
use crate::parse::{self, SitemapBody, SitemapEntry};
use crate::{Granularity, UrlOrSitemap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub start: Option<NaiveDate>,
    pub end: Option<NaiveDate>,
}

impl DateRange {
    pub fn between(start: NaiveDate, end: NaiveDate) -> Self {
        Self {
            start: Some(start),
            end: Some(end),
        }
    }

    pub fn since(start: NaiveDate) -> Self {
        Self {
            start: Some(start),
            end: None,
        }
    }

    pub fn until(end: NaiveDate) -> Self {
        Self {
            start: None,
            end: Some(end),
        }
    }

    pub fn month(year: i32, month: u32) -> Self {
        let start = NaiveDate::from_ymd_opt(year, month, 1);
        let (next_year, next_month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        let end =
            NaiveDate::from_ymd_opt(next_year, next_month, 1).and_then(|next| next.pred_opt());
        Self { start, end }
    }

    pub fn year(year: i32) -> Self {
        Self::month(year, 1).merge(Self::month(year, 12))
    }

    fn merge(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn contains(&self, date: NaiveDate) -> bool {
        self.start.map(|start| date >= start).unwrap_or(true)
            && self.end.map(|end| date <= end).unwrap_or(true)
    }
}

impl From<(NaiveDate, NaiveDate)> for DateRange {
    fn from(range: (NaiveDate, NaiveDate)) -> Self {
        Self::between(range.0, range.1)
    }
}

#[derive(Debug, Clone)]
pub struct Query {
    pub provider: String,
    pub kind: Option<String>,
    pub range: Option<DateRange>,
}

impl Query {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            kind: None,
            range: None,
        }
    }

    pub fn kind(mut self, kind: impl Into<String>) -> Self {
        self.kind = Some(kind.into());
        self
    }

    pub fn archive(mut self) -> Self {
        self.kind = Some(KIND_ARCHIVE.into());
        self
    }

    pub fn news(mut self) -> Self {
        self.kind = Some(KIND_NEWS.into());
        self
    }

    pub fn range(mut self, range: DateRange) -> Self {
        self.range = Some(range);
        self
    }

    pub fn between(self, start: NaiveDate, end: NaiveDate) -> Self {
        self.range(DateRange::between(start, end))
    }

    pub fn since(self, start: NaiveDate) -> Self {
        self.range(DateRange::since(start))
    }

    pub fn until(self, end: NaiveDate) -> Self {
        self.range(DateRange::until(end))
    }

    pub fn month(self, year: i32, month: u32) -> Self {
        self.range(DateRange::month(year, month))
    }

    pub fn year(self, year: i32) -> Self {
        self.range(DateRange::year(year))
    }
}

#[derive(Debug, Clone)]
enum Matcher {
    Contains(String),
    Regex(Regex),
}

impl Matcher {
    fn compile(pattern: &str) -> Self {
        match pattern.strip_prefix("regex:") {
            Some(expression) => Regex::new(expression)
                .map(Self::Regex)
                .unwrap_or_else(|_| Self::Contains(pattern.to_string())),
            None => Self::Contains(pattern.to_string()),
        }
    }

    fn is_match(&self, url: &str) -> bool {
        match self {
            Self::Contains(needle) => url.contains(needle.as_str()),
            Self::Regex(regex) => regex.is_match(url),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Matchers {
    by_provider: HashMap<String, Vec<Matcher>>,
}

impl Matchers {
    pub fn from_config(config: &Config) -> Self {
        Self {
            by_provider: config
                .providers
                .iter()
                .map(|provider| {
                    (
                        provider.id.to_lowercase(),
                        provider
                            .article_markers
                            .iter()
                            .map(|pattern| Matcher::compile(pattern))
                            .collect(),
                    )
                })
                .collect(),
        }
    }

    pub fn is_article(&self, provider: &Provider, url: &str) -> bool {
        match self.by_provider.get(&provider.id.to_lowercase()) {
            None => provider
                .article_markers
                .iter()
                .any(|pattern| Matcher::compile(pattern).is_match(url)),
            Some(markers) if markers.is_empty() => true,
            Some(markers) => markers.iter().any(|matcher| matcher.is_match(url)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CrawlReport {
    pub files_ok: usize,
    pub files_failed: usize,
    pub duplicates: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Crawl {
    pub entries: Vec<SitemapEntry>,
    pub report: CrawlReport,
}

impl Crawl {
    pub fn entries(&self) -> &[SitemapEntry] {
        &self.entries
    }

    pub fn urls(&self) -> Vec<&str> {
        self.entries
            .iter()
            .map(|entry| entry.loc.as_str())
            .collect()
    }

    pub fn into_entries(self) -> Vec<SitemapEntry> {
        self.entries
    }

    pub fn into_urls(self) -> Vec<String> {
        self.entries.into_iter().map(|entry| entry.loc).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Crawler {
    config: Config,
    client: reqwest::Client,
    matchers: Matchers,
    retries: u32,
    concurrency: usize,
    max_depth: usize,
    timeout: Duration,
}

impl Crawler {
    pub fn new(config: Config) -> Result<Self, Error> {
        let client = match config.user_agent.as_deref() {
            Some(user_agent) if user_agent != crate::USER_AGENT => reqwest::Client::builder()
                .user_agent(user_agent)
                .build()
                .map_err(|source| Error::Config(source.to_string()))?,
            _ => crate::http_client()?,
        };
        Ok(Self::with_client(config, client))
    }

    pub fn builtin() -> Result<Self, Error> {
        Self::new(Config::builtin())
    }

    pub fn with_client(config: Config, client: reqwest::Client) -> Self {
        Self {
            matchers: Matchers::from_config(&config),
            config,
            client,
            retries: 2,
            concurrency: 4,
            max_depth: 4,
            timeout: Duration::from_secs(120),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn is_article(&self, provider_id: &str, url: &str) -> bool {
        match self.config.provider(provider_id) {
            Some(provider) => self.matchers.is_article(provider, url),
            None => false,
        }
    }

    pub async fn urls(&self, provider_id: &str) -> Result<Vec<String>, Error> {
        Ok(self.crawl(&Query::new(provider_id)).await?.into_urls())
    }

    pub async fn entries(&self, provider_id: &str) -> Result<Vec<SitemapEntry>, Error> {
        Ok(self.crawl(&Query::new(provider_id)).await?.into_entries())
    }

    pub async fn crawl(&self, query: &Query) -> Result<Crawl, Error> {
        let provider = self
            .config
            .provider(&query.provider)
            .ok_or_else(|| Error::UnknownProvider(query.provider.clone()))?;

        let mut queue: VecDeque<(UrlOrSitemap, usize)> = provider
            .sitemaps(query.kind.as_deref())
            .into_iter()
            .cloned()
            .map(|sitemap| (UrlOrSitemap::Sitemap(sitemap), 0))
            .collect();
        let mut seen_files: HashSet<String> = queue
            .iter()
            .map(|(sitemap, _)| sitemap.url().to_string())
            .collect::<HashSet<String>>();
        let mut found: Vec<SitemapEntry> = Vec::new();
        let mut unique: HashSet<String> = HashSet::new();
        let mut report = CrawlReport::default();

        while !queue.is_empty() {
            let size = self.concurrency.min(queue.len());
            let batch: Vec<(UrlOrSitemap, usize)> = queue.drain(..size).collect();
            let bodies = join_all(batch.iter().map(|(url, _)| self.fetch(url.url()))).await;

            for ((url, depth), body) in batch.iter().zip(bodies) {
                let body = match body {
                    Ok(body) => body,
                    Err(error) => {
                        report.files_failed += 1;
                        report.errors.push(format!("{}: {error}", url.url()));
                        continue;
                    }
                };
                report.files_ok += 1;

                match body {
                    SitemapBody::Index(entries) => {
                        for entry in entries {
                            let range = match &url {
                                UrlOrSitemap::Sitemap(sitemap) => match sitemap.granularity {
                                    Granularity::Month | Granularity::Week => {
                                        query.range.clone().map(|mut range| {
                                            range.start = range
                                                .start
                                                .map(|start| start.with_day(1).unwrap_or(start));
                                            range
                                        })
                                    }
                                    _ => query.range,
                                },
                                _ => query.range,
                            };

                            if *depth >= self.max_depth
                                || !keep(url_date(&entry.loc), None, range.as_ref())
                            {
                                continue;
                            }
                            if seen_files.insert(entry.loc.clone()) {
                                queue.push_back((UrlOrSitemap::Url(entry.loc), depth + 1));
                            }
                        }
                    }
                    SitemapBody::Urls(entries) => {
                        let file_date = url_date(url.url());
                        for entry in entries {
                            if !keep(entry.date(), file_date, query.range.as_ref())
                                || !self.matchers.is_article(provider, &entry.loc)
                            {
                                continue;
                            }
                            if !unique.insert(entry.loc.clone()) {
                                report.duplicates += 1;
                                continue;
                            }
                            found.push(entry);
                        }
                    }
                }
            }
        }

        if report.files_ok == 0 && report.files_failed > 0 {
            return Err(Error::Empty {
                url: query.provider.clone(),
                attempts: self.retries + 1,
            });
        }

        Ok(Crawl {
            entries: found,
            report,
        })
    }

    async fn fetch(&self, url: &str) -> Result<SitemapBody, Error> {
        let attempts = self.retries + 1;
        let mut last_error = None;
        for attempt in 1..=attempts {
            match self.get(url).await {
                Ok(body) => return Ok(body),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < attempts {
                        tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| Error::Empty {
            url: url.to_string(),
            attempts,
        }))
    }

    async fn get(&self, url: &str) -> Result<SitemapBody, Error> {
        let response = self
            .client
            .get(url)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|source| Error::Http {
                url: url.to_string(),
                source,
            })?;
        let status = response.status();
        let body = response.bytes().await.map_err(|source| Error::Http {
            url: url.to_string(),
            source,
        })?;
        if !status.is_success() {
            return Err(Error::Status {
                url: url.to_string(),
                code: status.as_u16(),
            });
        }
        let parsed = parse::parse(url, &body)?;
        if parsed.entries().is_empty() {
            return Err(Error::Empty {
                url: url.to_string(),
                attempts: self.retries + 1,
            });
        }
        Ok(parsed)
    }
}

fn keep(
    file_date: Option<NaiveDate>,
    entry_date: Option<NaiveDate>,
    range: Option<&DateRange>,
) -> bool {
    match range {
        None => true,
        Some(range) => match file_date.or(entry_date) {
            Some(date) => range.contains(date),
            None => true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::parse::SitemapEntry;

    #[test]
    fn date_ranges_are_inclusive() {
        let start = NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        let range = DateRange::month(2026, 8);
        assert_eq!(range.start, Some(start));
        assert_eq!(range.end, Some(end));
        assert!(range.contains(end));
        assert!(!range.contains(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()));
        assert!(DateRange::year(2026).contains(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()));
        assert!(DateRange::since(start).contains(NaiveDate::from_ymd_opt(2099, 1, 1).unwrap()));
    }

    #[test]
    fn query_builders_set_provider_kind_and_range() {
        let query = Query::new("kurier")
            .archive()
            .month(2026, 8)
            .kind("archive");
        assert_eq!(query.provider, "kurier");
        assert_eq!(query.kind.as_deref(), Some("archive"));
        assert_eq!(query.range, Some(DateRange::month(2026, 8)));
    }

    #[test]
    fn matchers_support_substrings_and_regex() {
        let config = Config::builtin();
        let matchers = Matchers::from_config(&config);
        let derstandard = config.provider("derstandard").unwrap();
        let heute = config.provider("heute").unwrap();
        let diepresse = config.provider("diepresse").unwrap();
        assert!(matchers.is_article(derstandard, "https://www.derstandard.at/story/1/a"));
        assert!(!matchers.is_article(heute, "https://www.heute.at/wetter/d/wien/1"));
        assert!(matchers.is_article(diepresse, "https://www.diepresse.com/41232830/slug"));
        assert!(matchers.is_article(diepresse, "https://www.diepresse.com/311205/wien-energie"));
        assert!(!matchers.is_article(diepresse, "https://www.diepresse.com/autor/max-mustermann"));
        let kurier = config.provider("kurier").unwrap();
        assert!(matchers.is_article(kurier, "https://kurier.at/chronik/wien/a/403188541"));
        assert!(matchers.is_article(
            kurier,
            "https://kurier.at/sport/fussball/der-naechste-rekord/2.530.752"
        ));
        assert!(!matchers.is_article(kurier, "https://kurier.at/themen/wien"));
    }

    #[test]
    fn undated_child_sitemaps_are_always_crawled() {
        let range = DateRange::month(2026, 9);
        let shard = SitemapEntry {
            loc: "https://www.heute.at/storage/sitemaps/sitemap_0.xml".into(),
            lastmod: crate::parse::parse_timestamp("2025-12-30T17:56:41+01:00"),
            published: None,
            title: None,
        };
        assert!(keep(url_date(&shard.loc), None, Some(&range)));
    }

    #[test]
    fn file_dates_win_over_entry_timestamps() {
        let range = DateRange::year(2007);
        let entry = SitemapEntry {
            loc: "https://kurier.at/archiv/die-redaktion-im-burgenland/711.751".into(),
            lastmod: crate::parse::parse_timestamp("2023-01-19T14:15:05+01:00"),
            published: None,
            title: None,
        };
        let file_date = url_date("https://kurier.at/sitemaps_article_2007-10-29.xml");
        assert_eq!(file_date, NaiveDate::from_ymd_opt(2007, 10, 29));
        assert!(keep(file_date, entry.date(), Some(&range)));
        assert!(!keep(None, entry.date(), Some(&range)));
        assert!(keep(None, entry.date(), None));
    }

    #[tokio::test]
    async fn unknown_provider_is_an_error() {
        let crawler = Crawler::builtin().unwrap();
        let error = crawler.urls("nonexistent").await.unwrap_err();
        assert!(matches!(error, Error::UnknownProvider(_)));
    }

    #[tokio::test]
    async fn august_2026_urls_from_three_sites() {
        for id in ["derstandard", "kurier", "heute"] {
            let crawler = Crawler::builtin().unwrap();
            let query = Query::new(id).archive().month(2026, 8);
            let crawl = crawler.crawl(&query).await.unwrap();
            assert!(!crawl.is_empty(), "{id} yielded nothing");
            assert!(crawl.urls().iter().all(|url| crawler.is_article(id, url)));
        }
    }

    #[tokio::test]
    async fn test_all_providers_span_over_months() -> Result<(), Error> {
        let config = Config::builtin();
        let crawler = Crawler::new(config.clone())?;
        for provider in config.provider_ids() {
            let start = NaiveDate::from_ymd_opt(2025, 7, 29).unwrap();
            let end = NaiveDate::from_ymd_opt(2025, 8, 1).unwrap();
            let query = Query::new(provider).kind("archive").between(start, end);
            let crawl = crawler.crawl(&query).await?;
            let mut entries = crawl.entries().iter().collect::<Vec<_>>();
            entries.sort_by(|a, b| a.lastmod.cmp(&b.lastmod));
            let date = entries.first().as_ref().unwrap().date().unwrap();
            assert_eq!(date, start);
            let date = entries.last().as_ref().unwrap().date().unwrap();
            assert_eq!(date, end);
        }
        Ok(())
    }
}
