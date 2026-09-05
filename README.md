# sitemaps

Rust library that turns the sitemaps of Austrian news sites into **lists of news
article URLs and nothing else**. It never fetches an article page, it only reads
sitemap XML, and it drops everything that is not a news article: section pages,
tickers, podcasts, clips and paid content.

Providers (derStandard, profil, Kurier, Die Presse, oe24 and Heute) are data, not code:
everything lives in [`providers.json`](providers.json), so adding, disabling or removing
a site or a single sitemap is a JSON edit.

## Usage

```rust
use sitemaps::{Crawler, Config, Query};

let crawler = Crawler::new(Config::builtin())?;

let urls = crawler.urls("kurier").await?;

let august = crawler.crawl(&Query::new("derstandard").archive().month(2026, 8)).await?;
for entry in august.entries() {
    println!("{} {:?}", entry.loc, entry.date());
}
```

`Crawl` carries `Vec<SitemapEntry>` with `loc`, `lastmod`, `published` and `title`, plus
a `CrawlReport` with `files_ok`, `files_failed`, `duplicates` and per-url errors, so a
slow provider degrades into a report instead of an error. `urls()` and `into_urls()`
reduce it to plain URLs.

## Date ranges

```rust
Query::new("diepresse").archive().month(2026, 8);
Query::new("derstandard").archive().year(2025);
Query::new("heute").between(start, end);
Query::new("profil").since(date);
Query::new("kurier").news();
Query::new("kurier").kind("archive");
Query::new("kurier");
```

Ranges are inclusive and are applied on two levels. Child sitemaps whose URL carries a
date (`sitemap-2026-08.xml`, `sitemaps_article_2026-08-31.xml`, `/sitemap/2026/09`) are
skipped before they are fetched. Entries inside a fetched urlset are filtered by
`news:publication_date`, falling back to `lastmod`, and only when an entry has no
timestamp at all does the date of the sitemap file decide. An entry whose date cannot be
determined is kept, an unknown date never hides a URL.

A source with `"granularity": "month"` or `"week"` widens the lower bound to the first
day of that month before child sitemaps are matched. Without it, `between(2025-07-29,
2025-08-01)` would never match the `2025-07` or `2025-08` file of derStandard, because
those files are dated `2025-07-01` and `2025-08-01`.

## Example

```bash
cargo run --release --example urls
```

## Config file

```jsonc
{
  "version": 1,
  "verified": "2026-09-05",
  "user_agent": "somes/2026.1.0",
  "providers": [
    {
      "id": "derstandard",
      "name": "derStandard.at",
      "origin": "https://www.derstandard.at",
      "article_markers": ["/story/", "/jetzt/livebericht/"],
      "sitemaps": [
        { "kind": "archive", "url": "https://www.derstandard.at/sitemaps/sitemap.xml",
          "granularity": "month", "coverage": "1997-01 until now" }
      ]
    }
  ]
}
```

`Config::load`/`Config::save` read and write that file, `Config::builtin()` embeds the
shipped `providers.json`, and `add_provider`/`remove_provider` change it at runtime. A
sitemap with `"disabled": true` is kept in the file but never crawled, and
`Provider::has_sources()`/`Config::active_provider_ids()` report the providers that
actually have something to crawl. Kinds are free-form strings, `archive` and `news` are
conventions (`KIND_ARCHIVE`, `KIND_NEWS`).

`article_markers` decide what counts as a news article. A plain entry is a substring, an
entry with the prefix `regex:` is a regular expression compiled once per crawl, and an
entry with the prefix `!` excludes a url. Kurier uses that to keep podcast episodes,
clips and paid content out of the result:

```jsonc
"article_markers": ["regex:/[0-9][0-9.]{2,}$", "!/podcasts/", "!/video/", "!/cm/", "!/trend-hub/"]
```

The HTTP client comes from `somes_scraper::http_client()`, so requests use the same
`User-Agent` (`somes/2026.1.0`) as the somes scraper. Set `user_agent` in the config to
override it.

## Verified sitemap layout (2026-09-05)

| Site | Archive index | Children | Range | URLs/file | ≈ total |
|---|---|---|---|---|---|
| derStandard | `/sitemaps/sitemap.xml` | 353 monthly | 1997‑01 → now | 4–6 k | ~1.6 M |
| profil | `/sitemaps_article.xml` | 614 weekly | 2014‑11‑24 → now | ~45 | ~27 k |
| Kurier | `/sitemaps_article.xml` | 968 weekly | 2007‑10‑29 → now | ~880 | ~850 k |
| Die Presse | `/sitemap` | 235 monthly `/sitemap/YYYY/MM` | 2007‑02 → now | 4–12 k | ~1 M |
| oe24 | `/sitemap-article.xml` | 237 monthly | 2007‑01 → now | 2.6–4.7 k | ~700 k |
| Heute | `/storage/sitemaps/sitemap.xml` | 16 flat shards | ~2003 → now | 50 000 | 766 566 (counted) |

Google-News feeds (about the last 48 hours): `derstandard.at/sitemaps/news.xml`,
`profil.at/sitemaps_google_news.xml`, `kurier.at/sitemaps_google_news.xml`,
`diepresse.com/news-sitemap`, `oe24.at/sitemap-news.xml`. Heute has none.

## ORF.at has no sitemaps

ORF publishes neither a sitemap nor an RSS feed for its news articles. `robots.txt`
declares no `Sitemap:` line on orf.at, news.orf.at, the landesseiten (kaernten, ooe,
steiermark, noe, burgenland, …), tvthek, audiothek, science, oe1 and fm4, and
`/sitemap.xml` is 404 on all of them. The only two sitemaps in the whole ORF network,
`on.orf.at/sitemap.xml` (9 urls) and `fm4.orf.at/sitemap.xml` (10 urls), list static
pages such as `/kontakt` and `/barrierefrei` and not a single story. Stories exist only
as `https://orf.at/stories/<id>/` and are listed solely by the storyserver JSON API,
which this crate does not speak. The `orf` provider is therefore configured with markers
and **no sources**; `crawler.urls("orf")` returns an empty list instead of guessing.

## Gotchas baked into the config

- **ORF.at is not a provider.** ORF publishes no sitemap and no RSS feed for its news
  articles: no `Sitemap:` line in `robots.txt` on orf.at, news.orf.at, the landesseiten,
  tvthek, audiothek, science, oe1 or fm4, `/sitemap.xml` is 404 on all of them, and the
  two sitemaps that do exist in the ORF network (`on.orf.at`, `fm4.orf.at`) list static
  pages only. Stories are reachable only through the storyserver JSON API.

- **Die Presse** children have no `.xml` suffix (`/sitemap/2026/09`).
- **Heute** has no dated filenames and its index `lastmod` values are stale, so its
  shards are always crawled and date filters use the `lastmod` of the entries inside.
- **oe24** `sitemap-news.xml` regularly hangs and the current month file can be 404,
  raise `Crawler::timeout` and `Crawler::retries` for it.
- **Empty but HTTP 200** responses mark the beginning of the profil and Kurier archives,
  they are treated as retryable failures and end up in `report.errors`.
- **Kurier** entry timestamps are modification dates, its pre‑2015 ids are dotted
  (`/2.530.752`), and its Google‑News feed keeps items for weeks.
- `robots.txt` of derStandard, Die Presse and Heute blocks AI crawlers such as
  `GPTBot`, `ClaudeBot`, `CCBot`, `Google-Extended` and `PerplexityBot`.

## License

AGPL-3.0-only, see [LICENSE](LICENSE).
