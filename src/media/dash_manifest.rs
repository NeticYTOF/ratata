use crate::error::RatataError;
use anyhow::Result;
use regex::Regex;
use url::Url;

#[derive(Clone, Debug)]
pub struct Representation {
    pub id: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub base_url: Option<String>,
}

pub fn select_representation(mpd: &str) -> Result<Representation> {
    let regex = Regex::new(r#"(?s)<Representation\b([^>]*)>(.*?)</Representation>"#)?;
    let mut best: Option<Representation> = None;
    for capture in regex.captures_iter(mpd) {
        let attrs = capture.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = capture.get(2).map(|m| m.as_str()).unwrap_or("");
        if attribute(attrs, "mimeType")
            .is_some_and(|m| m.starts_with("audio") || m.starts_with("text"))
        {
            continue;
        }
        let rep = Representation {
            id: attribute(attrs, "id").unwrap_or_else(|| "0".into()),
            bandwidth: attribute(attrs, "bandwidth")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            width: attribute(attrs, "width")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            height: attribute(attrs, "height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            base_url: capture_base_url(body),
        };
        let score = (rep.bandwidth, rep.width * rep.height);
        if best
            .as_ref()
            .is_none_or(|b| score > (b.bandwidth, b.width * b.height))
        {
            best = Some(rep);
        }
    }
    best.ok_or_else(|| RatataError::NoFormats("DASH MPD".into()).into())
}

pub fn segment_template_for(mpd: &str, representation_id: &str) -> Option<String> {
    let duration = attribute(mpd, "mediaPresentationDuration")
        .map(|v| format!("mediaPresentationDuration=\"{v}\" "))
        .unwrap_or_default();
    let sets = Regex::new(r"(?s)<AdaptationSet\b[^>]*>(.*?)</AdaptationSet>").ok()?;
    for set in sets.captures_iter(mpd) {
        let body = set.get(1)?.as_str();
        if body.contains(&format!("id=\"{representation_id}\""))
            && let Some(template) = template_block(body)
        {
            return Some(format!("{duration}{template}"));
        }
    }
    template_block(mpd).map(|template| format!("{duration}{template}"))
}

fn template_block(input: &str) -> Option<String> {
    let paired = Regex::new(r"(?s)<SegmentTemplate\b([^>]*)>(.*?)</SegmentTemplate>").ok()?;
    if let Some(capture) = paired.captures(input) {
        return Some(format!(
            "{} {}",
            capture.get(1)?.as_str(),
            capture.get(2)?.as_str()
        ));
    }
    let self_closing = Regex::new(r"<SegmentTemplate\b([^>]*)/>").ok()?;
    self_closing.captures(input).map(|c| c[1].to_owned())
}

fn capture_base_url(body: &str) -> Option<String> {
    let regex = Regex::new(r"(?s)<BaseURL\b[^>]*>(.*?)</BaseURL>").ok()?;
    regex
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn timeline_times(template: &str) -> Vec<u64> {
    let Some(timeline) = Regex::new(r"(?s)<SegmentTimeline>(.*?)</SegmentTimeline>")
        .ok()
        .and_then(|r| r.captures(template).map(|c| c[1].to_owned()))
    else {
        return Vec::new();
    };
    let entries = Regex::new(r"<S\b([^>]*)/>").expect("static regex");
    let mut times = Vec::new();
    let mut cursor = 0u64;
    for entry in entries.captures_iter(&timeline) {
        let Some(attrs) = entry.get(1).map(|m| m.as_str()) else {
            continue;
        };
        let start = attribute(attrs, "t")
            .and_then(|v| v.parse().ok())
            .unwrap_or(cursor);
        let duration = attribute(attrs, "d")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let repeat = attribute(attrs, "r")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
            .min(100_000);
        if duration == 0 {
            continue;
        }
        for index in 0..=repeat {
            times.push(start + index * duration);
        }
        cursor = start + (repeat + 1) * duration;
    }
    times
}

pub fn segment_urls(
    template: &str,
    base_url: &str,
    rep: &Representation,
) -> Result<(Option<String>, Vec<String>)> {
    if template.contains("<SegmentURL") {
        let urls = Regex::new(r#"<SegmentURL\b[^>]*\bmedia="([^"]+)""#)?
            .captures_iter(template)
            .map(|c| resolve(base_url, &c[1]))
            .collect::<Result<Vec<_>>>()?;
        let init = Regex::new(r#"<Initialization\b[^>]*\bsourceURL="([^"]+)""#)
            .ok()
            .and_then(|r| r.captures(template))
            .map(|c| resolve(base_url, &c[1]))
            .transpose()?;
        return Ok((init, urls));
    }
    let media = attribute(template, "media").unwrap_or_else(|| "$Number$.m4s".into());
    let init_template = attribute(template, "initialization");
    let start = attribute(template, "startNumber")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);
    let times = timeline_times(template);
    let count = if media.contains("$Time$") && !times.is_empty() {
        times.len() as u64
    } else {
        segment_count(template)
    };
    let expand = |value: &str, number: u64, time: u64| -> String {
        value
            .replace("$RepresentationID$", &rep.id)
            .replace("$Bandwidth$", &rep.bandwidth.to_string())
            .replace("$Number%05d$", &format!("{number:05}"))
            .replace("$Number%04d$", &format!("{number:04}"))
            .replace("$Number$", &number.to_string())
            .replace("$Time$", &time.to_string())
    };
    let init = init_template
        .map(|t| resolve(base_url, &expand(&t, start, 0)))
        .transpose()?;
    let urls = (0..count)
        .map(|index| {
            let time = times.get(index as usize).copied().unwrap_or(0);
            resolve(base_url, &expand(&media, start + index, time))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((init, urls))
}

fn segment_count(template: &str) -> u64 {
    if let Some(timeline) = Regex::new(r"(?s)<SegmentTimeline>(.*?)</SegmentTimeline>")
        .ok()
        .and_then(|r| r.captures(template))
    {
        let total: u64 = Regex::new(r"<S\b[^>]*/>")
            .expect("static regex")
            .captures_iter(timeline.get(1).map(|m| m.as_str()).unwrap_or_default())
            .filter_map(|c| {
                let element = c.get(0)?.as_str();
                Some(
                    attribute(element, "r")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1,
                )
            })
            .sum();
        if total > 0 {
            return total;
        }
    }
    let duration = attribute(template, "duration")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(4.0);
    let timescale = attribute(template, "timescale")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0);
    let total = attribute(template, "mediaPresentationDuration")
        .map(|v| iso_duration(&v))
        .unwrap_or(0.0);
    ((total * timescale) / duration).ceil() as u64
}

fn iso_duration(value: &str) -> f64 {
    let regex = Regex::new(r"PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?").expect("static regex");
    let Some(captures) = regex.captures(value) else {
        return 0.0;
    };
    let hours: f64 = captures
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let minutes: f64 = captures
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let seconds: f64 = captures
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    hours * 3600.0 + minutes * 60.0 + seconds
}

fn attribute(input: &str, name: &str) -> Option<String> {
    let regex = Regex::new(&format!(r#"\b{name}="([^"]+)""#)).ok()?;
    regex.captures(input).map(|c| c[1].to_owned())
}

fn resolve(base: &str, uri: &str) -> Result<String> {
    Ok(Url::parse(base)?.join(uri)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_segment_template() {
        let mpd = r#"<MPD mediaPresentationDuration="PT20S"><Period><AdaptationSet><SegmentTemplate timescale="1" duration="4" media="seg-$Number$.m4s" initialization="init-$RepresentationID$.mp4" /><Representation id="720p" bandwidth="2000000" width="1280" height="720"><BaseURL>v/</BaseURL></Representation></AdaptationSet></Period></MPD>"#;
        let rep = select_representation(mpd).unwrap();
        let template = segment_template_for(mpd, &rep.id).unwrap();
        let (init, urls) =
            segment_urls(&template, "https://cdn.example/mpd/manifest.mpd", &rep).unwrap();
        assert_eq!(
            init.as_deref(),
            Some("https://cdn.example/mpd/init-720p.mp4")
        );
        assert_eq!(urls.len(), 5);
        assert_eq!(urls[0], "https://cdn.example/mpd/seg-1.m4s");
    }

    #[test]
    fn expands_time_timeline() {
        let template = r#"timescale="600" initialization="init-$RepresentationID$.dash" media="video-$RepresentationID$-$Time$.dash"><SegmentTimeline><S t="0" d="2400" r="1"/><S d="1200"/></SegmentTimeline>"#;
        let rep = Representation {
            id: "video_401000".into(),
            bandwidth: 401000,
            width: 224,
            height: 100,
            base_url: None,
        };
        let (init, urls) = segment_urls(template, "https://cdn.example/mpd.mpd", &rep).unwrap();
        assert_eq!(urls.len(), 3);
        assert_eq!(urls[0], "https://cdn.example/video-video_401000-0.dash");
        assert_eq!(urls[2], "https://cdn.example/video-video_401000-4800.dash");
        assert_eq!(
            init.as_deref(),
            Some("https://cdn.example/init-video_401000.dash")
        );
    }
}
