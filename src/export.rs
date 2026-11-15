//! 读 Instagram 数据下载（JSON 形式）里的收藏和赞过，列成链接条目（协议 §3.8、§3.9）。
//!
//! 条目的几种写法照 meizuflux/scrollback（`src/import/interactions.ts`，MIT）：
//! - 新的 `label_values`：`{"timestamp", "label_values": [{"label": "URL", "value": 链接}, {"title": "Owner", "dict": …}]}`；
//! - `string_list_data`：`{"title": 作者, "string_list_data": [{"href", "timestamp"}]}`（赞过）；
//! - `string_map_data`：`{"title": 作者, "string_map_data": {"Saved on": {"href", "timestamp"}}}`（收藏）。
//!
//! 顶层的键：`saved_saved_media`、`saved_saved_collections`（收藏，`Favorite`）、`likes_media_likes`（赞过，`Like`）；
//! 也有顶层直接是数组的，动作靠文件名判断。键的名字不挑具体哪一个字段，只认条目里带 `href` 的。
//! 每一处写法的哪个字段读不出，都在 [`item_link`] 一处。

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use usuyo_plugin_kit::protocol::{
    Access, Author, Evidence, Identity, ListAction, ListItem, ListPage, PlatformId, Rfc3339,
};

use crate::error::{Error, NOT_AN_EXPORT};
use crate::input::{self, JsonFile};
use crate::limits::Limits;
use crate::text::repair;

/// 一页最多多少条。
pub const PAGE: usize = 500;

/// 顶层的键对应的动作。
fn action_of_key(key: &str) -> Option<ListAction> {
    match key {
        "saved_saved_media" | "saved_saved_collections" => Some(ListAction::Favorite),
        "likes_media_likes" => Some(ListAction::Like),
        _ => None,
    }
}

fn action_of_stem(stem: &str) -> Option<ListAction> {
    match stem {
        "saved_posts" | "saved_collections" => Some(ListAction::Favorite),
        "liked_posts" => Some(ListAction::Like),
        _ => None,
    }
}

/// 一个条目读出来的：链接、时间、作者。
struct Link {
    href: String,
    timestamp: Option<i64>,
    owner: Option<String>,
}

fn string<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str().filter(|s| !s.is_empty())
}

/// 链接和时间可能在哪几处：照上面三种写法找，先找到的算。
fn item_link(item: &Value) -> Option<Link> {
    // label_values：{"label": "URL", "value": …}
    let labelled = item.get("label_values").and_then(Value::as_array);
    let label = |name: &str| {
        labelled?
            .iter()
            .find(|e| e.get("label").and_then(Value::as_str) == Some(name))
            .and_then(|e| string(e, "value").or_else(|| string(e, "href")))
    };
    // string_list_data[]、string_map_data.*：带 href 的那一项
    let datum = item
        .get("string_list_data")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|d| string(d, "href").is_some()))
        .or_else(|| {
            item.get("string_map_data")?
                .as_object()?
                .values()
                .find(|d| string(d, "href").is_some())
        });
    let href = label("URL").or_else(|| datum.and_then(|d| string(d, "href")))?;
    let timestamp = item
        .get("timestamp")
        .and_then(Value::as_i64)
        .or_else(|| datum?.get("timestamp")?.as_i64());
    let owner = labelled
        .and_then(|a| {
            a.iter()
                .find(|e| e.get("title").and_then(Value::as_str) == Some("Owner"))?
                .get("dict")?
                .as_array()?
                .iter()
                .find_map(|d| {
                    d.get("dict")?
                        .as_array()?
                        .iter()
                        .find(|e| e.get("label").and_then(Value::as_str) == Some("Username"))
                        .and_then(|e| string(e, "value"))
                })
        })
        .or_else(|| label("Username"))
        .or_else(|| string(item, "title"));
    Some(Link {
        href: href.to_owned(),
        timestamp,
        owner: owner.map(repair),
    })
}

/// 帖子链接里的短码：`/p/<码>/`、`/reel/<码>/`、`/reels/<码>/`、`/tv/<码>/`，前面可以有用户名。
/// 主页、故事一类的链接回 `None`。短码区分大小写，`[A-Za-z0-9_-]`。
fn shortcode(href: &str) -> Option<String> {
    let url = url::Url::parse(href).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    if host != "instagram.com" && !host.ends_with(".instagram.com") {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let code = match segments.as_slice() {
        ["p" | "tv" | "reel" | "reels", code, ..] => *code,
        [_, "p" | "tv" | "reel" | "reels", code, ..] => *code,
        _ => return None,
    };
    (code.len() <= 64
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then(|| code.to_owned())
}

fn username(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 30
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
}

/// 秒数，或毫秒（Instagram 的几个文件两种都出现过）。
fn time(timestamp: i64) -> Option<Rfc3339> {
    let secs = if timestamp > 100_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    jiff::Timestamp::from_second(secs)
        .ok()
        .map(Rfc3339::from_timestamp)
}

struct Found {
    action: ListAction,
    code: String,
    timestamp: Option<i64>,
    owner: Option<String>,
}

/// 一个文件里的条目。读不出来的不算错，只数着；整个文件的条目一个都读不出才算错。
fn read_file(file: &JsonFile, out: &mut Vec<Found>) -> Result<(), Error> {
    let bytes = file
        .bytes
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(&file.bytes);
    if bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<') {
        return Err(Error::unsupported(
            &file.name,
            "(whole file)",
            "the file is HTML, not JSON",
            crate::error::HTML_EXPORT,
        ));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|e| {
        Error::unsupported(
            &file.name,
            "(whole file)",
            format!("not valid JSON: {e}"),
            NOT_AN_EXPORT,
        )
    })?;
    // 要读的数组：顶层的键，或顶层本身
    let mut arrays: Vec<(String, ListAction, &Vec<Value>)> = Vec::new();
    match &value {
        Value::Object(map) => {
            for (key, v) in map {
                if let (Some(action), Value::Array(a)) = (action_of_key(key), v) {
                    arrays.push((key.clone(), action, a));
                }
            }
        }
        Value::Array(a) => {
            if let Some(action) = action_of_stem(&file.stem) {
                arrays.push(("(top level)".into(), action, a));
            }
        }
        _ => {}
    }
    if arrays.is_empty() {
        return Err(Error::unsupported(
            &file.name,
            "saved_saved_media, saved_saved_collections, likes_media_likes",
            "none of the keys of a saved or liked file is present",
            NOT_AN_EXPORT,
        ));
    }
    for (key, action, items) in arrays {
        let (mut no_link, mut not_post) = (0usize, 0usize);
        let before = out.len();
        for item in items {
            let Some(link) = item_link(item) else {
                no_link += 1;
                continue;
            };
            let Some(code) = shortcode(&link.href) else {
                not_post += 1;
                continue;
            };
            out.push(Found {
                action,
                code,
                timestamp: link.timestamp,
                owner: link.owner,
            });
        }
        if no_link + not_post > 0 {
            eprintln!(
                "{}: {key}: {no_link} entries without a link, {not_post} links that are not posts were skipped",
                file.name
            );
        }
        // 收藏夹的文件里有收藏夹自己的条目（没有链接），一个收藏夹里没有帖子时整个文件都没有链接，不算形状变了
        if out.len() == before && no_link > 0 && not_post == 0 && key != "saved_saved_collections" {
            return Err(Error::changed(
                &file.name,
                &format!(
                    "{key}[].(label_values URL | string_list_data[].href | string_map_data.*.href)"
                ),
                format!("none of the {} entries has a link", items.len()),
            ));
        }
    }
    Ok(())
}

fn items(files: &[JsonFile], limits: &Limits) -> Result<Vec<ListItem>, Error> {
    let mut found = Vec::new();
    for file in files {
        read_file(file, &mut found)?;
        if found.len() > limits.items {
            return Err(Error::unsupported(
                &file.name,
                "(entries)",
                format!("more than {} entries", limits.items),
                crate::error::TOO_LARGE,
            ));
        }
    }
    // 同一个动作里同一个帖子只留一条，留最新的时间；作者取第一个有的
    let mut by_post: BTreeMap<(u8, String), Found> = BTreeMap::new();
    for f in found {
        let key = (f.action as u8, f.code.clone());
        match by_post.get_mut(&key) {
            None => {
                by_post.insert(key, f);
            }
            Some(old) => {
                if f.timestamp > old.timestamp {
                    old.timestamp = f.timestamp;
                }
                if old.owner.is_none() {
                    old.owner = f.owner;
                }
            }
        }
    }
    let mut all: Vec<Found> = by_post.into_values().collect();
    // 新的在前；没有时间的排在最后。时间相同按动作和短码排，翻页时顺序才不变
    all.sort_by(|a, b| {
        (Reverse(a.timestamp), a.action as u8, &a.code).cmp(&(
            Reverse(b.timestamp),
            b.action as u8,
            &b.code,
        ))
    });
    Ok(all.into_iter().map(to_item).collect())
}

fn to_item(f: Found) -> ListItem {
    let url = format!("https://www.instagram.com/p/{}/", f.code);
    ListItem {
        url: Some(url.clone()),
        reference: None,
        identity: PlatformId::try_from("instagram")
            .ok()
            .map(|platform| Identity::Platform {
                platform,
                content_id: f.code.clone(),
                url: Some(url),
            }),
        title: None,
        author: f.owner.map(|name| Author {
            url: username(&name).then(|| format!("https://www.instagram.com/{name}/")),
            name,
            platform_id: None,
        }),
        published_at: None,
        action: Some(f.action),
        acted_at: f.timestamp.and_then(time),
    }
}

/// 列一页。`cursor` 是从第几条开始（插件自己的不透明字符串）。
pub fn list(
    source: &str,
    input: Option<&Path>,
    cursor: Option<&str>,
    limits: &Limits,
) -> Result<ListPage, Error> {
    if !source.starts_with("file:") {
        return Err(Error::InvalidArgs(format!(
            "source `{source}` is not a file import"
        )));
    }
    let input = input
        .ok_or_else(|| Error::InvalidArgs("USUYO_INPUT is not set for a file import".into()))?;
    let start = match cursor {
        None => 0,
        Some(c) => c
            .parse::<usize>()
            .map_err(|_| Error::InvalidArgs(format!("cursor `{c}` is not ours")))?,
    };
    let fetched_at = Rfc3339::from_timestamp(jiff::Timestamp::now());
    let all = items(&input::read(input, limits)?, limits)?;
    let end = start.saturating_add(PAGE).min(all.len());
    Ok(ListPage {
        items: all.get(start..end).unwrap_or_default().to_vec(),
        next_cursor: (end < all.len()).then(|| end.to_string()),
        evidence: Evidence {
            fetched_at,
            access: Access::OfficialExport,
            site: None,
            upstream: None,
            status: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn shortcodes() {
        let code = |u: &str| shortcode(u);
        assert_eq!(
            code("https://www.instagram.com/p/Cx1-_aB/"),
            Some("Cx1-_aB".into())
        );
        assert_eq!(
            code("https://instagram.com/reel/AbC123/?igsh=x"),
            Some("AbC123".into())
        );
        assert_eq!(
            code("https://www.instagram.com/someone/p/AbC123/"),
            Some("AbC123".into())
        );
        assert_eq!(
            code("https://www.instagram.com/tv/AbC123"),
            Some("AbC123".into())
        );
        assert_eq!(code("https://www.instagram.com/someone/"), None);
        assert_eq!(code("https://www.instagram.com/stories/someone/123/"), None);
        assert_eq!(code("https://example.com/p/AbC123/"), None);
        assert_eq!(code("https://www.instagram.com/p/a%20b/"), None);
        assert_eq!(code("not a url"), None);
    }

    #[test]
    fn the_three_shapes() {
        let list = json!({"title": "kaya", "string_list_data": [{"href": "https://www.instagram.com/p/A1/", "timestamp": 1700000000}]});
        let l = item_link(&list).unwrap();
        assert_eq!(
            (l.href.as_str(), l.timestamp, l.owner.as_deref()),
            (
                "https://www.instagram.com/p/A1/",
                Some(1_700_000_000),
                Some("kaya")
            )
        );
        let map = json!({"title": "zed", "string_map_data": {"Saved on": {"href": "https://www.instagram.com/p/B2/", "timestamp": 1700000001, "value": "Saved on"}}});
        let m = item_link(&map).unwrap();
        assert_eq!(
            (m.href.as_str(), m.timestamp),
            ("https://www.instagram.com/p/B2/", Some(1_700_000_001))
        );
        let label = json!({"timestamp": 1700000002, "label_values": [
            {"label": "URL", "value": "https://www.instagram.com/reel/C3/"},
            {"title": "Owner", "dict": [{"dict": [{"label": "Username", "value": "nia"}]}]}]});
        let n = item_link(&label).unwrap();
        assert_eq!(
            (n.href.as_str(), n.timestamp, n.owner.as_deref()),
            (
                "https://www.instagram.com/reel/C3/",
                Some(1_700_000_002),
                Some("nia")
            )
        );
        // 没有链接的（收藏夹自己的条目）
        assert!(
            item_link(&json!({"title": "Collection", "string_map_data": {"Name": {"value": "x"}}}))
                .is_none()
        );
    }

    #[test]
    fn times_in_seconds_or_milliseconds() {
        assert_eq!(
            time(1_700_000_000).unwrap().as_str(),
            "2023-11-14T22:13:20Z"
        );
        assert_eq!(
            time(1_700_000_000_000).unwrap().as_str(),
            "2023-11-14T22:13:20Z"
        );
    }
}
