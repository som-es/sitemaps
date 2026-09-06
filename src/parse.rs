use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::hash::{Hash, Hasher};

use crate::Error;

#[derive(Debug, Clone, Default, PartialOrd, Ord)]
pub struct SitemapEntry {
    pub loc: String,
    pub lastmod: Option<DateTime<Utc>>,
    pub published: Option<DateTime<Utc>>,
    pub title: Option<String>,
}

impl PartialEq for SitemapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.loc == other.loc && self.lastmod == other.lastmod && self.published == other.published
    }
}

impl Eq for SitemapEntry {}

impl Hash for SitemapEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.loc.hash(state);
        self.lastmod.hash(state);
        self.published.hash(state);
    }
}

impl SitemapEntry {
    pub fn date(&self) -> Option<chrono::NaiveDate> {
        self.published
            .or(self.lastmod)
            .map(|timestamp| timestamp.date_naive())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SitemapBody {
    Index(Vec<SitemapEntry>),
    Urls(Vec<SitemapEntry>),
}

impl SitemapBody {
    pub fn is_index(&self) -> bool {
        matches!(self, Self::Index(_))
    }

    pub fn entries(&self) -> &[SitemapEntry] {
        match self {
            Self::Index(entries) | Self::Urls(entries) => entries,
        }
    }

    pub fn locs(&self) -> impl Iterator<Item = &str> {
        self.entries().iter().map(|entry| entry.loc.as_str())
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Loc,
    Lastmod,
    Published,
    Title,
}

pub fn parse(url: &str, xml: &[u8]) -> Result<SitemapBody, Error> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut buffer = Vec::new();
    let mut index = Vec::new();
    let mut urls = Vec::new();
    let mut entry = SitemapEntry::default();
    let mut text = String::new();
    let mut field: Option<Field> = None;

    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                match name.as_str() {
                    "sitemap" | "url" => entry = SitemapEntry::default(),
                    "loc" => field = Some(Field::Loc),
                    "lastmod" => field = Some(Field::Lastmod),
                    "news:publication_date" => field = Some(Field::Published),
                    "news:title" => field = Some(Field::Title),
                    _ => {}
                }
            }
            Ok(Event::Text(event)) => {
                if field.is_some() {
                    if let Ok(content) = event.xml10_content() {
                        if let Ok(unescaped) = unescape(&content) {
                            text.push_str(&unescaped);
                        }
                    }
                }
            }
            Ok(Event::CData(event)) => {
                if field.is_some() {
                    text.push_str(&String::from_utf8_lossy(&event.into_inner()));
                }
            }
            Ok(Event::End(event)) => {
                let name = String::from_utf8_lossy(event.name().as_ref()).into_owned();
                match name.as_str() {
                    "loc" | "lastmod" | "news:publication_date" | "news:title" => {
                        match field {
                            Some(Field::Loc) => entry.loc = text.trim().to_string(),
                            Some(Field::Lastmod) => entry.lastmod = parse_timestamp(&text),
                            Some(Field::Published) => entry.published = parse_timestamp(&text),
                            Some(Field::Title) => {
                                entry.title =
                                    Some(text.trim().to_string()).filter(|title| !title.is_empty())
                            }
                            None => {}
                        }
                        text.clear();
                        field = None;
                    }
                    "sitemap" => index.push(std::mem::take(&mut entry)),
                    "url" => urls.push(std::mem::take(&mut entry)),
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(source) => {
                return Err(Error::Parse {
                    url: url.to_string(),
                    message: source.to_string(),
                })
            }
            _ => {}
        }
    }

    if !index.is_empty() {
        return Ok(SitemapBody::Index(index));
    }
    Ok(SitemapBody::Urls(urls))
}

pub fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(text) {
        return Some(timestamp.with_timezone(&Utc));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format) {
            return Utc.from_utc_datetime(&naive).into();
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0)?).into();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <sitemap><loc>https://www.profil.at/sitemaps_article_2014-11-24.xml</loc>
    <lastmod>2009-09-03T23:16:08+02:00</lastmod></sitemap>
  <sitemap><loc>https://www.profil.at/sitemaps_article_2014-11-17.xml</loc></sitemap>
</sitemapindex>"#;

    const URLS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:news="http://www.google.com/schemas/sitemap-news/0.9" xmlns:image="http://www.google.com/schemas/sitemap-image/1.1">
  <url><loc>https://www.sn.at/wirtschaft/welt/vw-streicht-jobs-art-671165</loc>
    <news:news><news:publication><news:name>Salzburger Nachrichten</news:name></news:publication>
      <news:publication_date>2026-09-04T17:36:00+02:00</news:publication_date>
      <news:title><![CDATA[VW streicht von 50.000 Jobs Hälfte in Deutschland]]></news:title>
    </news:news>
    <image:image><image:loc>https://www.sn.at/images/vw.jpg</image:loc></image:image>
  </url>
</urlset>"#;

    #[test]
    fn parses_sitemap_index() {
        let body = parse(
            "https://www.profil.at/sitemaps_article.xml",
            INDEX.as_bytes(),
        )
        .unwrap();
        assert!(body.is_index());
        assert_eq!(body.locs().collect::<Vec<&str>>().len(), 2);
        assert_eq!(
            body.entries()[0].lastmod,
            parse_timestamp("2009-09-03T21:16:08Z")
        );
        assert_eq!(body.entries()[1].lastmod, None);
    }

    #[test]
    fn parses_news_urls_and_ignores_image_locs() {
        let body = parse(
            "https://www.sn.at/news-artikel.sitemap.xml",
            URLS.as_bytes(),
        )
        .unwrap();
        assert!(!body.is_index());
        let entry = &body.entries()[0];
        assert_eq!(
            entry.loc,
            "https://www.sn.at/wirtschaft/welt/vw-streicht-jobs-art-671165"
        );
        assert_eq!(
            entry.title.as_deref(),
            Some("VW streicht von 50.000 Jobs Hälfte in Deutschland")
        );
        assert_eq!(
            entry.date(),
            Some(NaiveDate::from_ymd_opt(2026, 9, 4).unwrap())
        );
    }

    #[test]
    fn timestamps_are_parsed_leniently() {
        assert_eq!(
            parse_timestamp("2026-09-04"),
            parse_timestamp("2026-09-04T00:00:00Z")
        );
        assert_eq!(
            parse_timestamp("2026-09-04T07:36:00+02:00"),
            parse_timestamp("2026-09-04T05:36:00Z")
        );
        assert_eq!(parse_timestamp("nope"), None);
    }

    #[test]
    fn empty_body_yields_no_entries() {
        let body = parse(
            "https://www.profil.at/sitemaps_article_2014-11-17.xml",
            URLS.len().to_string().as_bytes(),
        )
        .unwrap();
        assert_eq!(body.entries().len(), 0);
    }
}
