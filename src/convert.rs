use std::path::Path;

#[derive(Debug)]
pub struct ImageRef {
    pub src: String,
    pub alt: String,
    pub sentinel: String,
}

#[derive(Debug, Default)]
pub struct Converted {
    pub markdown: String,
    pub images: Vec<ImageRef>,
}

fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_tag = false;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if in_tag {
            out.push(c);
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                }
                None => match c {
                    '>' => in_tag = false,
                    '\'' => quote = Some('\''),
                    '"' => quote = Some('"'),
                    _ => {}
                },
            }
            i += 1;
            continue;
        }
        if c == '<' {
            in_tag = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '&' {
            let limit = (i + 12).min(chars.len());
            if let Some(off) = chars[i + 1..limit].iter().position(|&ch| ch == ';') {
                let end = i + 1 + off;
                let entity: String = chars[i + 1..end].iter().collect();
                if let Some(repl) = decode_one(&entity) {
                    out.push_str(&repl);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn decode_one(entity: &str) -> Option<String> {
    let fixed: Option<&str> = match entity {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        "nbsp" => Some(" "),
        "copy" => Some("©"),
        "reg" => Some("®"),
        "trade" => Some("™"),
        "hellip" => Some("…"),
        "mdash" => Some("—"),
        "ndash" => Some("–"),
        "lsquo" => Some("’"),
        "rsquo" => Some("’"),
        "ldquo" => Some("“"),
        "rdquo" => Some("”"),
        _ => None,
    };
    if let Some(s) = fixed {
        return Some(s.to_string());
    }
    if let Some(hex) = entity
        .strip_prefix("#x")
        .or_else(|| entity.strip_prefix("#X"))
    {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return char::from_u32(v).map(|ch| ch.to_string());
    }
    if let Some(dec) = entity.strip_prefix('#') {
        let v: u32 = dec.parse().ok()?;
        return char::from_u32(v).map(|ch| ch.to_string());
    }
    None
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let q = rest.chars().next()?;
    if q != '"' && q != '\'' {
        let end = rest
            .char_indices()
            .find(|(_, c)| c.is_whitespace() || *c == '>')
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    let end = rest[1..].find(q)? + 1;
    Some(rest[1..end].to_string())
}

fn pre_br_to_newline(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<pre") {
        let (head, tail) = rest.split_at(start);
        match tail.find("</pre>") {
            Some(end) => {
                let block_len = end + "</pre>".len();
                let block = &tail[..block_len];
                let block = block
                    .replace("<br />", "\n")
                    .replace("<br/>", "\n")
                    .replace("<br>", "\n");
                out.push_str(head);
                out.push_str(&block);
                rest = &tail[block_len..];
            }
            None => {
                out.push_str(head);
                out.push_str(tail);
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn convert(html: &str, text_fallback: &str, images_dir: &Path, salt: u64) -> Converted {
    if html.trim().is_empty() {
        return Converted {
            markdown: text_fallback.to_string(),
            images: Vec::new(),
        };
    }
    let mut html = decode_entities(html);
    html = pre_br_to_newline(&html);
    // Extract image tags first so ImageRef.src keeps the original URL, which
    // lets resolve() recognize UpNote's localhost:9425 images. Local-path
    // rewriting for any remaining references happens afterwards.
    let mut images: Vec<ImageRef> = Vec::new();
    let mut out = String::with_capacity(html.len());
    let mut rest = html.as_str();
    while let Some(pos) = rest.find("<img") {
        let (head, tail) = rest.split_at(pos);
        let end = tail.find('>').map(|e| e + 1).unwrap_or(tail.len());
        let tag = &tail[..end];
        let src = attr(tag, "src").unwrap_or_default();
        let alt = attr(tag, "alt").unwrap_or_default();
        let idx = images.len();
        let sentinel = format!("xupnt{salt:010x}{idx:02x}x");
        images.push(ImageRef {
            src,
            alt,
            sentinel: sentinel.clone(),
        });
        out.push_str(head);
        out.push_str(&format!("\n<p>{sentinel}</p>\n"));
        rest = &tail[end..];
    }
    out.push_str(rest);
    let dir = format!("{}/", images_dir.to_string_lossy().replace('\\', "/"));
    let out = out.replace("http://localhost:9425/images/", &dir);
    let out = out.replace("http://localhost:9425/files/", &dir);
    let markdown = html2md::parse_html(&out);
    let markdown = annotate_code_blocks(&markdown);
    Converted {
        markdown: markdown.trim().to_string(),
        images,
    }
}

const SHELL_CMDS: &[&str] = &[
    "sudo",
    "systemctl",
    "apt",
    "apt-get",
    "dnf",
    "pacman",
    "brew",
    "journalctl",
    "chmod",
    "chown",
    "mkdir",
    "curl",
    "wget",
    "docker",
    "kubectl",
    "neofetch",
    "inxi",
    "ssh",
    "scp",
    "rsync",
    "mount",
    "umount",
    "useradd",
    "htop",
    "top",
    "git",
    "k3s",
    "crontab",
    "systemd",
];

fn guess_shell_lang(content: &str) -> Option<&'static str> {
    for raw in content.lines() {
        let l = raw.trim_start();
        if l.starts_with("$ ") || l.starts_with("#!") {
            return Some("bash");
        }
        if let Some(cmd) = l.split_whitespace().next()
            && SHELL_CMDS.contains(&cmd)
        {
            return Some("bash");
        }
    }
    None
}

fn annotate_code_blocks(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut out = String::with_capacity(md.len());
    let mut i = 0;
    let mut in_code = false;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if !in_code && trimmed == "```" {
                let mut content = String::new();
                let mut j = i + 1;
                let mut closed = false;
                while j < lines.len() {
                    let tl = lines[j].trim_start();
                    if tl.starts_with("```") {
                        closed = true;
                        break;
                    }
                    content.push_str(lines[j]);
                    content.push('\n');
                    j += 1;
                }
                if closed {
                    match guess_shell_lang(&content) {
                        Some(lang) => out.push_str(&format!("```{lang}")),
                        None => out.push_str("```"),
                    }
                    out.push('\n');
                    i += 1;
                    while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                        out.push_str(lines[i]);
                        out.push('\n');
                        i += 1;
                    }
                    if i < lines.len() {
                        out.push_str(lines[i]);
                        out.push('\n');
                        i += 1;
                    }
                    in_code = false;
                    continue;
                }
            }
            in_code = !in_code;
            out.push_str(line);
            out.push('\n');
            i += 1;
        } else {
            out.push_str(line);
            out.push('\n');
            i += 1;
        }
    }
    out
}
