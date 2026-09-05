use chrono::NaiveDate;
use sitemaps::{Crawler, Query};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = Query::new("derstandard").kind("archive").between(
        NaiveDate::from_ymd_opt(2025, 3, 15).unwrap(),
        NaiveDate::from_ymd_opt(2025, 8, 1).unwrap(),
    );
    let crawler = Crawler::builtin()?;
    let crawl = crawler.crawl(&query).await?;
    for url in crawl.urls() {
        println!("{url}");
    }
    eprintln!(
        "{} urls from {} sitemap files, {} failed, {} duplicates",
        crawl.len(),
        crawl.report.files_ok,
        crawl.report.files_failed,
        crawl.report.duplicates
    );
    for error in crawl.report.errors.iter().take(10) {
        eprintln!("failed: {error}");
    }
    Ok(())
}
