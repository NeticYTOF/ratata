use regex::Regex;
use url::Url;

#[derive(Debug, Clone)]
enum Op {
    Reverse,
    Slice(usize),
    Cut(usize),
    Swap(usize),
}

pub fn player_js_url(page: &str) -> Option<String> {
    for pattern in [
        r#""jsUrl":"([^"]+)""#,
        r#""player_js_url":"([^"]+)""#,
        r#""assets":{"js":"([^"]+)""#,
    ] {
        if let Some(capture) = Regex::new(pattern).ok()?.captures(page) {
            let raw = capture.get(1)?.as_str();
            let unescaped = raw.replace("\\u0026", "&").replace("\\/", "/");
            let absolute = if unescaped.starts_with("http") {
                unescaped
            } else {
                format!("https://www.youtube.com{unescaped}")
            };
            if Url::parse(&absolute).is_ok() {
                return Some(absolute);
            }
        }
    }
    None
}

/// Turns `signatureCipher` (or a query with `s=`) into a usable media URL.
pub fn decipher(js: &str, raw: &str) -> Option<String> {
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_owned());
    }
    let ciphered = if raw.contains("signatureCipher=") {
        let wrapper = Url::parse(&format!("https://youtube.com/?{raw}")).ok()?;
        wrapper
            .query_pairs()
            .find(|(k, _)| k == "signatureCipher")?
            .1
            .into_owned()
    } else {
        raw.to_owned()
    };
    let query = Url::parse(&format!("https://youtube.com/?{ciphered}")).ok()?;
    let pairs: Vec<(String, String)> = query
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let mut url = pairs.iter().find(|(k, _)| k == "url")?.1.clone();
    let signature = pairs.iter().find(|(k, _)| k == "s").map(|(_, v)| v.clone());
    let parameter = pairs
        .iter()
        .find(|(k, _)| k == "sp")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "signature".into());
    let Some(signature) = signature else {
        return Some(url);
    };
    let ops = signature_ops(js)?;
    let solved = apply(&signature, &ops);
    let mut parsed = Url::parse(&url).ok()?;
    parsed.query_pairs_mut().append_pair(&parameter, &solved);
    url = parsed.to_string();
    Some(url)
}

fn signature_ops(js: &str) -> Option<Vec<Op>> {
    let name = Regex::new(r#""sig"\s*:\s*"?([\w$]+)"?"#)
        .ok()?
        .captures(js)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_owned()));
    if let Some(name) = name
        && let Some(body) = function_body(js, &name)
        && let Some(ops) = parse_ops(&body)
    {
        return Some(ops);
    }
    for (name, body) in all_functions(js) {
        if (body.contains("split(\"\")") || body.contains("a.split"))
            && body.contains("join(\"\")")
            && let Some(ops) = parse_ops(&body)
        {
            let _ = name;
            return Some(ops);
        }
    }
    None
}

fn parse_ops(body: &str) -> Option<Vec<Op>> {
    let mut ops = Vec::new();
    if !body.contains("split(\"\")") && !body.contains("split('')") {
        return None;
    }
    let splice = Regex::new(r"\.splice\((\d+)").ok()?;
    let slice = Regex::new(r"\.slice\((\d+)").ok()?;
    let swap = Regex::new(r"\b[\w$]+\.swap\(a,(\d+)\)").ok()?;
    let helper_splice = Regex::new(r"\b[\w$]+\.splice\(a,(\d+)\)").ok()?;
    for statement in body.split(';') {
        let statement = statement.trim();
        if statement.contains("reverse()") {
            ops.push(Op::Reverse);
        } else if let Some(capture) = splice.captures(statement) {
            ops.push(Op::Cut(capture.get(1)?.as_str().parse().ok()?));
        } else if let Some(capture) = slice.captures(statement) {
            ops.push(Op::Slice(capture.get(1)?.as_str().parse().ok()?));
        } else if let Some(capture) = swap.captures(statement) {
            ops.push(Op::Swap(capture.get(1)?.as_str().parse().ok()?));
        } else if let Some(capture) = helper_splice.captures(statement) {
            ops.push(Op::Cut(capture.get(1)?.as_str().parse().ok()?));
        } else if statement.contains(".reverse(a)") {
            ops.push(Op::Reverse);
        }
    }
    if ops.is_empty() { None } else { Some(ops) }
}

fn apply(input: &str, ops: &[Op]) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    for op in ops {
        match op {
            Op::Reverse => chars.reverse(),
            Op::Slice(n) if *n < chars.len() => chars = chars[*n..].to_vec(),
            Op::Slice(_) => chars.clear(),
            Op::Cut(n) => {
                let n = (*n).min(chars.len());
                chars.drain(0..n);
            }
            Op::Swap(n) if !chars.is_empty() => {
                let index = *n % chars.len();
                chars.swap(0, index);
            }
            Op::Swap(_) => {}
        }
    }
    chars.into_iter().collect()
}

fn function_body(js: &str, name: &str) -> Option<String> {
    let needle = format!("function {name}(a){{");
    let start = js.find(&needle)? + needle.len() - 1;
    let bytes = js.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(js[start..start + offset + 1].to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

fn all_functions(js: &str) -> Vec<(String, String)> {
    let regex = Regex::new(r"function\s+([\w$]+)\(a\)\{").expect("static regex");
    regex
        .captures_iter(js)
        .filter_map(|c| {
            let name = c.get(1)?.as_str();
            function_body(js, name).map(|body| (name.to_owned(), body))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deciphers_signature_with_reverse_and_swap() {
        let js = r#"var Xa={swap:function(a,b){var c=a[0];a[0]=a[b%a.length];a[b%a.length]=c},reverse:function(a){return a.reverse()}};
        function dec(a){a=a.split("");Xa.reverse(a);Xa.swap(a,3);return a.join("")}"#;
        let cipher = format!(
            "s=abcdef&sp=sig&url={}",
            url::form_urlencoded::byte_serialize(b"https://media.example/video.mp4".as_slice())
                .collect::<String>()
        );
        let result = decipher(js, &cipher).unwrap();
        assert!(result.contains("sig=cedfba"));
        assert!(result.contains("video.mp4"));
    }

    #[test]
    fn returns_plain_url_untouched() {
        assert_eq!(
            decipher("", "https://media.example/v.mp4").unwrap(),
            "https://media.example/v.mp4"
        );
    }
}
