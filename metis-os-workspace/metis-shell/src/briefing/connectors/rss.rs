use thiserror::Error;

#[derive(Debug, Error)]
pub enum RssError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("parse failed: {0}")]
    Parse(String),
}

pub async fn fetch_headlines(feed_url: &str) -> Result<Vec<String>, RssError> {
    let body = reqwest::get(feed_url).await?.text().await?;
    parse_titles(&body)
}

fn parse_titles(xml: &str) -> Result<Vec<String>, RssError> {
    let mut titles = Vec::new();
    for chunk in xml.split("<item>").skip(1) {
        if let Some(title) = extract_tag(chunk, "title") {
            titles.push(title);
        }
        if titles.len() >= 8 {
            break;
        }
    }
    if titles.is_empty() {
        for chunk in xml.split("<entry>").skip(1) {
            if let Some(title) = extract_tag(chunk, "title") {
                titles.push(title);
            }
            if titles.len() >= 8 {
                break;
            }
        }
    }
    if titles.is_empty() {
        return Err(RssError::Parse("no items found".into()));
    }
    Ok(titles)
}

fn extract_tag(chunk: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = chunk.find(&open)? + open.len();
    let end = chunk[start..].find(&close)? + start;
    let raw = &chunk[start..end];
    Some(decode_xml(raw.trim()))
}

fn decode_xml(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
