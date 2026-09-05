use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::Error;

pub const BUILTIN_CONFIG: &str = include_str!("../providers.json");

pub const KIND_ARCHIVE: &str = "archive";
pub const KIND_NEWS: &str = "news";

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    #[default]
    None,
    Month,
    Week,
    Day,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sitemap {
    pub kind: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Granularity::is_none")]
    pub granularity: Granularity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

impl Sitemap {
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind.eq_ignore_ascii_case(kind)
    }
}

impl Granularity {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub article_markers: Vec<String>,
    pub sitemaps: Vec<Sitemap>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl Provider {
    pub fn sitemaps(&self, kind: Option<&str>) -> Vec<&Sitemap> {
        self.sitemaps
            .iter()
            .filter(|sitemap| !sitemap.disabled)
            .filter(|sitemap| kind.map(|kind| sitemap.is_kind(kind)).unwrap_or(true))
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub providers: Vec<Provider>,
}

impl Config {
    pub fn builtin() -> Self {
        Self::from_json(BUILTIN_CONFIG).expect("built-in providers.json must be valid")
    }

    pub fn from_json(json: &str) -> Result<Self, Error> {
        serde_json::from_str(json).map_err(|error| Error::Config(error.to_string()))
    }

    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self).map_err(|error| Error::Config(error.to_string()))
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&text)
    }

    pub fn save(&self, path: impl AsRef<std::path::Path>) -> Result<(), Error> {
        let path = path.as_ref();
        let text = self.to_json()?;
        std::fs::write(path, text).map_err(|source| Error::Io {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers
            .iter()
            .find(|provider| provider.id.eq_ignore_ascii_case(id))
    }

    pub fn provider_mut(&mut self, id: &str) -> Option<&mut Provider> {
        self.providers
            .iter_mut()
            .find(|provider| provider.id.eq_ignore_ascii_case(id))
    }

    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect()
    }

    pub fn add_provider(&mut self, provider: Provider) {
        self.remove_provider(&provider.id);
        self.providers.push(provider);
    }

    pub fn remove_provider(&mut self, id: &str) -> bool {
        let before = self.providers.len();
        self.providers
            .retain(|provider| !provider.id.eq_ignore_ascii_case(id));
        self.providers.len() != before
    }
}

fn digit_runs(text: &str) -> Vec<String> {
    let mut runs = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else {
            if !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

pub fn url_date(url: &str) -> Option<NaiveDate> {
    let segments: Vec<&str> = url
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let tail = match segments.len() {
        0 => return None,
        1 => segments[0].to_string(),
        _ => format!(
            "{}-{}",
            segments[segments.len() - 2],
            segments[segments.len() - 1]
        ),
    };
    let runs = digit_runs(&tail);
    let year_index = runs.iter().position(|run| {
        run.len() == 4
            && run
                .parse::<u32>()
                .map(|year| (1987..=2100).contains(&year))
                .unwrap_or(false)
    })?;
    let year: i32 = runs[year_index].parse().ok()?;
    let month: u32 = runs.get(year_index + 1)?.parse().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let day = runs
        .get(year_index + 2)
        .filter(|run| run.len() == 2)
        .and_then(|run| run.parse::<u32>().ok())
        .unwrap_or(1);
    NaiveDate::from_ymd_opt(year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_config_is_valid() {
        let config = Config::builtin();
        assert_eq!(config.providers.len(), 7);
        for id in [
            "derstandard",
            "profil",
            "kurier",
            "diepresse",
            "oe24",
            "salzburger-nachrichten",
            "heute",
        ] {
            assert!(config.provider(id).is_some(), "missing provider {id}");
        }
    }

    #[test]
    fn json_round_trip() {
        let config = Config::builtin();
        let text = config.to_json().unwrap();
        assert_eq!(Config::from_json(&text).unwrap(), config);
    }

    #[test]
    fn providers_can_be_added_and_removed() {
        let mut config = Config::builtin();
        assert!(config.remove_provider("heute"));
        assert_eq!(config.providers.len(), 6);
        assert!(config.provider("heute").is_none());
        config.add_provider(Provider {
            id: "heute".into(),
            name: "Heute".into(),
            origin: "https://www.heute.at".into(),
            article_markers: vec!["/s/".into()],
            sitemaps: vec![Sitemap {
                kind: KIND_ARCHIVE.into(),
                url: "https://www.heute.at/storage/sitemaps/sitemap.xml".into(),
                granularity: Granularity::None,
                coverage: None,
                notes: None,
                disabled: false,
            }],
            notes: None,
        });
        assert_eq!(config.providers.len(), 7);
        assert!(config.provider("Heute").is_some());
    }

    #[test]
    fn article_markers_are_provider_specific() {
        let config = Config::builtin();
        let matchers = crate::Matchers::from_config(&config);
        let derstandard = config.provider("derstandard").unwrap();
        let heute = config.provider("heute").unwrap();
        assert!(matchers.is_article(derstandard, "https://www.derstandard.at/story/1/foo"));
        assert!(matchers.is_article(
            derstandard,
            "https://www.derstandard.at/jetzt/livebericht/1/foo"
        ));
        assert!(!matchers.is_article(derstandard, "https://www.derstandard.at/themen/wien"));
        assert!(matchers.is_article(heute, "https://www.heute.at/s/foo-120213897"));
        assert!(!matchers.is_article(heute, "https://www.heute.at/wetter/d/wien/1"));
    }

    #[test]
    fn sitemap_kinds_can_be_selected_and_sources_disabled() {
        let config = Config::builtin();
        let kurier = config.provider("kurier").unwrap();
        assert_eq!(kurier.sitemaps(None).len(), 4);
        assert_eq!(kurier.sitemaps(Some(KIND_NEWS)).len(), 1);
        assert_eq!(kurier.sitemaps(Some("recipe")).len(), 1);
        assert_eq!(kurier.sitemaps(Some("nothing")).len(), 0);
    }

    #[test]
    fn dates_come_from_child_sitemap_urls() {
        assert_eq!(
            url_date("https://www.derstandard.at/sitemaps/sitemap-2026-09.xml"),
            NaiveDate::from_ymd_opt(2026, 9, 1)
        );
        assert_eq!(
            url_date("https://www.profil.at/sitemaps_article_2014-11-24.xml"),
            NaiveDate::from_ymd_opt(2014, 11, 24)
        );
        assert_eq!(
            url_date("https://www.diepresse.com/sitemap/2026/09"),
            NaiveDate::from_ymd_opt(2026, 9, 1)
        );
        assert_eq!(
            url_date("https://www.sn.at/article-2026_08-3.sitemap.xml"),
            NaiveDate::from_ymd_opt(2026, 8, 1)
        );
        assert_eq!(
            url_date("https://www.heute.at/storage/sitemaps/sitemap_0.xml"),
            None
        );
    }
}
