# sitemaps

Rust library that turns the sitemaps of Austrian news sites into **lists of article
URLs and nothing else**. It never fetches an article page, it only reads sitemap XML.

Providers (derStandard, profil, Kurier, Die Presse, oe24, Salzburger Nachrichten, Heute)
are data, not code: everything lives in [`providers.json`](providers.json), so adding,
disabling or removing a site or a single sitemap is a JSON edit.

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

Date ranges are inclusive and work on two levels: child sitemaps whose URL carries a
date (`sitemap-2026-08.xml`, `sitemaps_article_2026-08-31.xml`, `/sitemap/2026/09`,
`article-2026_08-3.sitemap.xml`) are skipped wholesale, and entries inside a fetched
urlset are filtered by their `news:publication_date` or `lastmod`.

```rust
Query::new("diepresse").archive().month(2026, 8);
Query::new("sn").archive().year(2025);
Query::new("heute").between(start, end);
Query::new("profil").since(date);
Query::new("kurier").news();
Query::new("kurier").kind("recipe");
Query::new("kurier");
```

### How date filtering works

For an entry **the date of the sitemap file wins**. If the file that yielded an entry is
dated, that date decides whether the entry is in range and the entry timestamp is only a
fallback for undated files (the Heute shards). This is deliberate: Kurier stamps every
entry with a modification date, a 2007 article carries `2023-01-19`, so entry timestamps
alone silently drop most of an archive. Child sitemaps are filtered by the date in their
URL only, a child without a date in its URL is always crawled, because index `lastmod`
values lie: Heute stamps its shards `2025-12` while serving current articles in them.
Entries whose date cannot be determined at all are kept, an unknown date never hides a
URL. Weekly files are named after the start of their week, so a `month()` query on
profil or Kurier is off by up to seven days at the boundaries.

The last one selects every enabled sitemap of a provider. `Crawl` returns
`Vec<SitemapEntry>` (`loc`, `lastmod`, `news:publication_date`, `news:title`, with
`date()` preferring the publication date) plus a `CrawlReport` with `files_ok`,
`files_failed`, `duplicates` and
per-url errors, so a slow provider degrades into a report instead of an error.
`urls()` and `into_urls()` are shorthands for the locs of those entries.
Non-article URLs (channel pages, tickers, weather, author pages) are filtered out with
the `article_markers` of the provider. Plain substrings are used as-is, a marker with
the prefix `regex:` is compiled once per crawl.

## Config file

```jsonc
{
  "version": 1,
  "verified": "2026-09-04",
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
sitemap with `"disabled": true` is kept in the file but never crawled. Kinds are free
form strings, `archive` and `news` are only conventions (`KIND_ARCHIVE`, `KIND_NEWS`).

The HTTP client comes from `somes_scraper::http_client()`, so requests use the same
`User-Agent` (`somes/2026.1.0`) as the somes scraper. Set `user_agent` in the config to
override it.

## Verified sitemap layout (2026-09-04)

| Site | Archive index | Children | Range | URLs/file | ≈ total |
|---|---|---|---|---|---|
| derStandard | `/sitemaps/sitemap.xml` | 353 monthly | 1997‑01 → now | 4–6 k | ~1.6 M |
| profil | `/sitemaps_article.xml` | 614 weekly | 2014‑11‑24 → now | ~45 | ~27 k |
| Kurier | `/sitemaps_article.xml` | 968 weekly | 2007‑10‑29 → now | ~880 | ~850 k |
| Die Presse | `/sitemap` | 235 monthly `/sitemap/YYYY/MM` | 2007‑02 → now | 4–12 k | ~1 M |
| oe24 | `/sitemap-article.xml` | 237 monthly | 2007‑01 → now | 2.6–4.7 k | ~700 k |
| SN | `/portal-artikel.sitemap.xml` | 550 monthly chunks | 2002‑03 → now | ~1.3 k | ~700 k |
| Heute | `/storage/sitemaps/sitemap.xml` | 16 flat shards | ~2003 → now | 50 000 | 766 566 (counted) |

Google-News feeds (about the last two days):
`derstandard.at/sitemaps/news.xml`, `profil.at/sitemaps_google_news.xml`,
`kurier.at/sitemaps_google_news.xml`, `diepresse.com/news-sitemap`,
`oe24.at/sitemap-news.xml`, `sn.at/news-artikel.sitemap.xml`. Heute has none.

## Gotchas baked into the config

- **SN is nested**: `index.sitemap.xml` lists only the news feeds, the archive sits in
  `portal-artikel.sitemap.xml`, which is itself a sitemap index.
- **Die Presse** children have no `.xml` suffix (`/sitemap/2026/09`).
- **Heute** has no dated filenames and its index `lastmod` values are stale, so date
  filters fall back to the `lastmod` of the entries inside each shard.
- **oe24** `sitemap-news.xml`, `sitemap-video.xml` and the current month file regularly
  hang instead of answering, raise `Crawler::timeout` and `Crawler::retries` for it.
- **Empty but HTTP 200** responses mark the beginning of the profil and Kurier
  archives, they are treated as retryable failures and end up in `report.errors`.
- **Kurier** entry timestamps are modification dates, and pre‑2015 ids are dotted
  (`/2.530.752`), which is why its article marker accepts digits and dots.
- **oe24** publishes the current month file late, `sitemap-article-2026-09.xml` was 404
  while `sitemap-article-2026-08.xml` answered fine.
- `robots.txt` of derStandard, Die Presse and Heute blocks AI crawlers such as `GPTBot`,
  `ClaudeBot`, `CCBot`, `Google-Extended` and `PerplexityBot`.

## License

AGPL-3.0-only, see [LICENSE](LICENSE).
# sitemaps
