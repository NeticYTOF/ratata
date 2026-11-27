use crate::error::RatataError;
use crate::http::{browser_headers, get_json, get_text};
use crate::model::Media;
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub duration: Option<f64>,
    pub artwork: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub video_id: String,
    pub title: String,
    pub duration: Option<f64>,
}

pub fn is_spotify(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("open.spotify.com") | Some("spotify.com")
    )
}

pub fn is_apple_music(url: &Url) -> bool {
    url.host_str() == Some("music.apple.com") || url.host_str() == Some("itunes.apple.com")
}

pub fn is_youtube_music(url: &Url) -> bool {
    url.host_str() == Some("music.youtube.com")
}

pub fn is_music_link(url: &Url) -> bool {
    is_spotify(url) || is_apple_music(url) || is_youtube_music(url)
}

pub fn apple_id(url: &Url) -> Option<String> {
    let from_query = url
        .query_pairs()
        .find(|(key, value)| key == "i" && !value.is_empty())
        .map(|(_, value)| value.into_owned());
    if let Some(id) = from_query {
        return Some(id);
    }
    url.path_segments()?
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.strip_prefix("id").unwrap_or(segment))
        .find(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_owned)
}

pub async fn metadata(url: &Url, client: &Client) -> Result<Vec<TrackMetadata>> {
    if is_spotify(url) {
        spotify_metadata(url, client).await
    } else if is_apple_music(url) {
        apple_metadata(url, client).await
    } else {
        Err(RatataError::UnsupportedUrl(url.to_string()).into())
    }
}

async fn spotify_metadata(url: &Url, client: &Client) -> Result<Vec<TrackMetadata>> {
    let oembed = format!(
        "https://open.spotify.com/oembed?url={}",
        percent_encoding::utf8_percent_encode(url.as_str(), percent_encoding::NON_ALPHANUMERIC)
    );
    let json = get_json(
        client,
        &oembed,
        &browser_headers("https://open.spotify.com/"),
    )
    .await?;
    let mut title = json["title"].as_str().unwrap_or("Spotify track").to_owned();
    let mut artist = json["author_name"]
        .as_str()
        .unwrap_or("Unknown artist")
        .to_owned();
    let mut artwork = json["thumbnail_url"].as_str().map(str::to_owned);
    let mut album = String::new();
    if let Some(track_id) = url
        .path_segments()
        .and_then(|mut segments| segments.find(|segment| segment.starts_with("track")))
        .and_then(|_| {
            url.path_segments()
                .and_then(|mut s| s.nth(1))
                .map(str::to_owned)
        })
    {
        let embed_url = format!("https://open.spotify.com/embed/track/{track_id}");
        if let Ok(page) = get_text(
            client,
            &embed_url,
            &browser_headers("https://open.spotify.com/"),
        )
        .await
        {
            let parsed = parse_spotify_embed(&page);
            if let Some(value) = &parsed.artist {
                artist = value.clone();
            } else if let Some(description) = &parsed.description {
                let (song, by) = split_title_artist(description, &artist);
                if by != artist {
                    artist = by;
                    title = song;
                }
            }
            if let Some(value) = &parsed.album {
                album = value.clone();
            }
            if let Some(value) = &parsed.title {
                title = value.clone();
            }
            artwork = parsed.artwork.or(artwork);
        }
    }
    let (title, artist) = split_title_artist(&title, &artist);
    Ok(vec![TrackMetadata {
        title,
        artist,
        album,
        year: None,
        track: None,
        duration: None,
        artwork,
        source: "Spotify".into(),
    }])
}

#[derive(Default)]
struct SpotifyEmbed {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    description: Option<String>,
    artwork: Option<String>,
}

fn parse_spotify_embed(html: &str) -> SpotifyEmbed {
    let capture = |pattern: &str| -> Option<String> {
        regex::Regex::new(pattern)
            .ok()?
            .captures(html)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim_matches('\\').to_owned())
    };
    let anchored = |anchor: &str| -> Option<String> {
        let start = html.find(anchor)?;
        let end = (start + 160).min(html.len());
        let slice = &html[start..end];
        regex::Regex::new(r#"name.{0,12}?([A-Za-z][A-Za-z0-9 .'-]{1,60})"#)
            .ok()?
            .captures(slice)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_owned())
    };
    let artist = capture(r#""artist":"([^"]+)""#)
        .or_else(|| capture(r#""artists".{0,80}?"name":"([^"]+)""#))
        .or_else(|| anchored("artist"));
    SpotifyEmbed {
        title: capture(r#"<meta property="og:title" content="([^"]+)""#),
        artist,
        album: capture(r#""album":"([^"]+)""#).or_else(|| anchored("album")),
        description: capture(r#"<meta property="og:description" content="([^"]+)""#),
        artwork: capture(r#"<meta property="og:image" content="([^"]+)""#),
    }
}

async fn apple_metadata(url: &Url, client: &Client) -> Result<Vec<TrackMetadata>> {
    let id = apple_id(url).context("Apple Music link has no track or album id")?;
    let endpoint = format!("https://itunes.apple.com/lookup?id={id}&entity=song&limit=200");
    let json = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        get_json(
            client,
            &endpoint,
            &browser_headers("https://music.apple.com/"),
        ),
    )
    .await
    .context("Apple Music lookup timed out")??;
    let tracks = parse_apple_lookup(&json);
    if tracks.is_empty() {
        return Err(anyhow::anyhow!("Apple Music has no track for id {id}"));
    }
    Ok(tracks)
}
pub async fn soundcloud_fallback_metadata(url: &Url, client: &Client) -> Result<TrackMetadata> {
    let oembed = format!(
        "https://soundcloud.com/oembed?format=json&url={}",
        percent_encoding::utf8_percent_encode(url.as_str(), percent_encoding::NON_ALPHANUMERIC)
    );
    let json = get_json(client, &oembed, &browser_headers("https://soundcloud.com/")).await?;
    let title = json["title"]
        .as_str()
        .unwrap_or("SoundCloud track")
        .to_owned();
    let author = json["author_name"]
        .as_str()
        .unwrap_or("Unknown artist")
        .to_owned();
    let (title, artist) = split_title_artist(&title, &author);
    Ok(TrackMetadata {
        title,
        artist,
        album: String::new(),
        year: None,
        track: None,
        duration: None,
        artwork: json["thumbnail_url"].as_str().map(str::to_owned),
        source: "SoundCloud".into(),
    })
}

pub fn parse_apple_lookup(json: &Value) -> Vec<TrackMetadata> {
    json["results"]
        .as_array()
        .map(|results| {
            results
                .iter()
                .filter(|item| {
                    item["wrapperType"].as_str() == Some("track")
                        || item["kind"].as_str() == Some("song")
                })
                .map(|item| TrackMetadata {
                    title: item["trackName"]
                        .as_str()
                        .unwrap_or("Unknown track")
                        .to_owned(),
                    artist: item["artistName"]
                        .as_str()
                        .unwrap_or("Unknown artist")
                        .to_owned(),
                    album: item["collectionName"].as_str().unwrap_or("").to_owned(),
                    year: item["releaseDate"]
                        .as_str()
                        .and_then(|value| value.get(0..4))
                        .and_then(|value| value.parse().ok()),
                    track: item["trackNumber"].as_u64().map(|value| value as u32),
                    duration: item["trackTimeMillis"]
                        .as_u64()
                        .map(|ms| ms as f64 / 1000.0),
                    artwork: item["artworkUrl100"]
                        .as_str()
                        .map(|value| value.replace("100x100bb", "600x600bb")),
                    source: "Apple Music".into(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn split_title_artist(title: &str, author: &str) -> (String, String) {
    if let Some((artist, song)) = title.split_once(" - ")
        && !artist.trim().is_empty()
        && !song.trim().is_empty()
    {
        return (song.trim().to_owned(), artist.trim().to_owned());
    }
    (title.trim().to_owned(), author.trim().to_owned())
}

pub fn match_score(track: &TrackMetadata, candidate: &Candidate) -> f64 {
    let title = normalize(&track.title);
    let candidate_title = normalize(&candidate.title);
    let artist = normalize(&track.artist);
    let mut score = 0.0;
    if candidate_title.contains(&title) || title.contains(&candidate_title) {
        score += 50.0;
    } else {
        let overlap = title
            .split_whitespace()
            .filter(|word| word.len() > 2 && candidate_title.contains(word))
            .count();
        score += overlap as f64 * 8.0;
    }
    if !artist.is_empty() && candidate_title.contains(&artist) {
        score += 25.0;
    }
    let lower = candidate.title.to_ascii_lowercase();
    if lower.contains("official audio") || lower.contains("topic") || lower.contains("vevo") {
        score += 15.0;
    }
    if lower.contains("cover") || lower.contains("live") || lower.contains("remix") {
        score -= 20.0;
    }
    if let (Some(want), Some(got)) = (track.duration, candidate.duration) {
        let delta = (want - got).abs();
        if delta <= 3.0 {
            score += 30.0;
        } else if delta <= 10.0 {
            score += 10.0;
        } else {
            score -= 20.0;
        }
    }
    score
}

fn normalize(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub async fn search_youtube(client: &Client, query: &str) -> Result<Vec<Candidate>> {
    let url = format!(
        "https://www.youtube.com/results?search_query={}",
        percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
    );
    let html = get_text(client, &url, &browser_headers("https://www.youtube.com/")).await?;
    Ok(parse_search(&html))
}

pub fn parse_search(html: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let id_pattern = match regex::Regex::new(r#"videoId":"([A-Za-z0-9_-]{11})""#) {
        Ok(pattern) => pattern,
        Err(_) => return candidates,
    };
    let title_pattern =
        match regex::Regex::new(r#""title":\{"runs":\[\{"text":"((?:\\.|[^"\\])*)""#) {
            Ok(pattern) => pattern,
            Err(_) => return candidates,
        };
    let duration_pattern = match regex::Regex::new(r#""lengthText":\{"simpleText":"(\d+):(\d+)""#) {
        Ok(pattern) => pattern,
        Err(_) => return candidates,
    };
    let ids: Vec<String> = id_pattern
        .captures_iter(html)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned()))
        .take(30)
        .collect();
    let titles: Vec<String> = title_pattern
        .captures_iter(html)
        .filter_map(|c| {
            c.get(1)
                .map(|m| m.as_str().replace("\\u0026", "&").replace("\\\"", "\""))
        })
        .take(30)
        .collect();
    let durations: Vec<Option<f64>> = duration_pattern
        .captures_iter(html)
        .filter_map(|c| {
            let minutes: f64 = c.get(1)?.as_str().parse().ok()?;
            let seconds: f64 = c.get(2)?.as_str().parse().ok()?;
            Some(Some(minutes * 60.0 + seconds))
        })
        .collect();
    for (index, id) in ids.into_iter().enumerate() {
        candidates.push(Candidate {
            video_id: id,
            title: titles.get(index).cloned().unwrap_or_default(),
            duration: durations.get(index).copied().flatten(),
        });
    }
    candidates
}

pub fn best_match<'a>(track: &TrackMetadata, candidates: &'a [Candidate]) -> Option<&'a Candidate> {
    candidates
        .iter()
        .max_by(|a, b| match_score(track, a).total_cmp(&match_score(track, b)))
}

pub async fn match_on_youtube(track: &TrackMetadata, client: &Client) -> Result<Media> {
    let query = format!("{} {} official audio", track.artist, track.title);
    let candidates = search_youtube(client, &query).await?;
    let best = best_match(track, &candidates)
        .context("no matching YouTube Music result for this track")?;
    let watch = format!("https://www.youtube.com/watch?v={}", best.video_id);
    let mut media =
        super::youtube::extract(&Url::parse(&watch)?, client, &crate::Config::default()).await?;
    media.platform = format!("{} → YT Music", track.source);
    media.title = format!("{} - {}", track.artist, track.title);
    media.author = Some(track.artist.clone());
    media.thumbnail = track.artwork.clone().or(media.thumbnail);
    media
        .formats
        .sort_by_key(|format| std::cmp::Reverse(format.bitrate.unwrap_or(format.quality())));
    Ok(media)
}

pub fn nearest_images(media: &Media, artwork: Option<&str>) -> Option<String> {
    artwork
        .map(str::to_owned)
        .or_else(|| media.thumbnail.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_music_urls() {
        assert!(is_spotify(
            &Url::parse("https://open.spotify.com/track/123").unwrap()
        ));
        assert!(is_apple_music(
            &Url::parse("https://music.apple.com/us/album/x/id123").unwrap()
        ));
        assert!(is_youtube_music(
            &Url::parse("https://music.youtube.com/watch?v=abc").unwrap()
        ));
        assert_eq!(
            apple_id(&Url::parse("https://music.apple.com/us/album/x/id123?i=456").unwrap())
                .as_deref(),
            Some("456")
        );
        assert_eq!(
            apple_id(
                &Url::parse("https://music.apple.com/us/song/never-gonna-give-you-up/1558534271")
                    .unwrap()
            )
            .as_deref(),
            Some("1558534271")
        );
    }

    #[test]
    fn parses_apple_lookup_results() {
        let json: Value = serde_json::from_str(
            r#"{"resultCount":1,"results":[{"wrapperType":"track","kind":"song","trackName":"Song","artistName":"Artist","collectionName":"Album","releaseDate":"2020-05-01","trackNumber":3,"trackTimeMillis":180000,"artworkUrl100":"https://image/100x100bb.jpg"}]}"#,
        )
        .unwrap();
        let tracks = parse_apple_lookup(&json);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].title, "Song");
        assert_eq!(tracks[0].year, Some(2020));
        assert_eq!(tracks[0].track, Some(3));
        assert_eq!(
            tracks[0].artwork.as_deref(),
            Some("https://image/600x600bb.jpg")
        );
    }

    #[test]
    fn scores_matches_with_duration_and_official_hint() {
        let track = TrackMetadata {
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            year: None,
            track: None,
            duration: Some(180.0),
            artwork: None,
            source: "Spotify".into(),
        };
        let good = Candidate {
            video_id: "a".into(),
            title: "Artist - Song (Official Audio)".into(),
            duration: Some(181.0),
        };
        let bad = Candidate {
            video_id: "b".into(),
            title: "Song cover".into(),
            duration: Some(240.0),
        };
        assert!(match_score(&track, &good) > match_score(&track, &bad));
        assert_eq!(best_match(&track, &[bad, good]).unwrap().video_id, "a");
    }

    #[test]
    fn parses_spotify_embed_metadata() {
        let html = r#"<meta property="og:title" content="Never Gonna Give You Up"><meta property="og:image" content="https://cover.jpg">{\"artist\":{\"name\":\"Rick Astley\"},\"album\":{\"name\":\"Whenever You Need Somebody\"}}"#;
        let parsed = parse_spotify_embed(html);
        assert_eq!(parsed.artist.as_deref(), Some("Rick Astley"));
        assert_eq!(parsed.album.as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!(parsed.artwork.as_deref(), Some("https://cover.jpg"));
    }
}
