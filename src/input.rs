//! 打开交来的文件：单独的一个 JSON，或整个数据下载的 zip。
//!
//! zip 里的条目只按名字读进内存；条目名从不拼成要写的路径。
//! 名字有 `..`、以 `/` 开头、含反斜杠或控制字符的条目当没有。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

use crate::error::{Error, HTML_EXPORT, NO_FILES, NOT_AN_EXPORT, TOO_LARGE};
use crate::limits::Limits;

/// 交来的单个文件在报错里叫什么。
pub const PLAIN: &str = "input";

/// 读进来的一个 JSON 文件。
pub struct JsonFile {
    /// 单个文件叫 `input`，zip 里的是条目名。
    pub name: String,
    pub bytes: Vec<u8>,
    /// 文件名（去掉扩展名和 `_<数字>` 后缀）：顶层是数组、没有键可认时靠它判断。
    pub stem: String,
}

/// 下载里认得的文件：名字（去掉扩展名和 `_<数字>` 后缀）。文件夹随年份变过，所以不看文件夹。
pub const KNOWN_STEMS: [&str; 3] = ["saved_posts", "saved_collections", "liked_posts"];

/// 去掉扩展名和 `_<数字>` 后缀的文件名。
fn stem_of(name: &str) -> String {
    let file = name.rsplit('/').next().unwrap_or(name);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    match stem.rsplit_once('_') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
            head.to_owned()
        }
        _ => stem.to_owned(),
    }
}

/// 名字能不能当 zip 里的一条正常条目：不会指到别处。
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
        && !name.split('/').any(|seg| seg == "..")
        && name.as_bytes().get(1) != Some(&b':')
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    head.iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| *b == b'<')
}

pub fn read(path: &Path, limits: &Limits) -> Result<Vec<JsonFile>, Error> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 4];
    let n = file.read(&mut magic)?;
    if n == 4 && magic.starts_with(b"PK") && matches!(&magic[2..], [3, 4] | [5, 6]) {
        return read_zip(file, limits);
    }
    let len = file.metadata()?.len();
    if len > limits.json_bytes {
        return Err(Error::unsupported(
            PLAIN,
            "(whole file)",
            format!(
                "the file is {len} bytes, over the {} byte limit",
                limits.json_bytes
            ),
            TOO_LARGE,
        ));
    }
    let mut bytes = magic[..n].to_vec();
    file.read_to_end(&mut bytes)?;
    if looks_like_html(&bytes) {
        return Err(Error::unsupported(
            PLAIN,
            "(whole file)",
            "the file is HTML, not JSON",
            HTML_EXPORT,
        ));
    }
    Ok(vec![JsonFile {
        name: PLAIN.into(),
        bytes,
        stem: String::new(),
    }])
}

fn read_zip(file: File, limits: &Limits) -> Result<Vec<JsonFile>, Error> {
    let mut archive = ZipArchive::new(file).map_err(|e| {
        Error::unsupported(
            PLAIN,
            "(zip)",
            format!("not a readable zip file: {e}"),
            NOT_AN_EXPORT,
        )
    })?;
    if archive.len() > limits.zip_entries {
        return Err(Error::unsupported(
            PLAIN,
            "(zip entries)",
            format!(
                "the zip has {} entries, over the {} limit",
                archive.len(),
                limits.zip_entries
            ),
            TOO_LARGE,
        ));
    }
    let mut json: Vec<(String, usize, String)> = Vec::new();
    let mut html = false;
    for i in 0..archive.len() {
        // 名字读不出来（不是 UTF-8）的条目当没有
        let Some(Ok(name)) = archive.name_for_index(i) else {
            continue;
        };
        if name.ends_with('/') || !safe_name(&name) {
            continue;
        }
        let stem = stem_of(&name);
        if !KNOWN_STEMS.contains(&stem.as_str()) {
            continue;
        }
        if name.ends_with(".json") {
            json.push((name.into_owned(), i, stem));
        } else if name.ends_with(".html") {
            html = true;
        }
    }
    if json.is_empty() {
        return Err(Error::unsupported(
            PLAIN,
            "(zip entries)",
            "no saved_posts, saved_collections or liked_posts JSON file in the zip",
            if html { HTML_EXPORT } else { NO_FILES },
        ));
    }
    json.sort();
    if json.len() > limits.files {
        return Err(Error::unsupported(
            PLAIN,
            "(zip entries)",
            format!(
                "{} matching files, over the {} limit",
                json.len(),
                limits.files
            ),
            TOO_LARGE,
        ));
    }
    let mut out = Vec::new();
    for (name, index, stem) in json {
        let entry = archive.by_index(index).map_err(|e| {
            Error::unsupported(
                &name,
                "(zip entry)",
                format!("cannot read the entry: {e}"),
                NOT_AN_EXPORT,
            )
        })?;
        let limit = limits.json_bytes;
        if entry.size() > limit {
            return Err(Error::unsupported(
                &name,
                "(zip entry)",
                format!(
                    "the entry is {} bytes, over the {limit} byte limit",
                    entry.size()
                ),
                TOO_LARGE,
            ));
        }
        let mut bytes = Vec::new();
        entry.take(limit + 1).read_to_end(&mut bytes).map_err(|e| {
            Error::unsupported(
                &name,
                "(zip entry)",
                format!("cannot read the entry: {e}"),
                NOT_AN_EXPORT,
            )
        })?;
        if bytes.len() as u64 > limit {
            return Err(Error::unsupported(
                &name,
                "(zip entry)",
                format!("the entry is over the {limit} byte limit when unpacked"),
                TOO_LARGE,
            ));
        }
        out.push(JsonFile { name, bytes, stem });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_and_names() {
        assert_eq!(
            stem_of("a/your_instagram_activity/saved/saved_posts.json"),
            "saved_posts"
        );
        assert_eq!(stem_of("likes/liked_posts_2.json"), "liked_posts");
        assert_eq!(stem_of("saved_collections.html"), "saved_collections");
        assert!(safe_name("a/b.json"));
        assert!(!safe_name("../saved_posts.json"));
        assert!(!safe_name("/saved_posts.json"));
        assert!(!safe_name("a\\saved_posts.json"));
        assert!(!safe_name("C:/saved_posts.json"));
    }
}
