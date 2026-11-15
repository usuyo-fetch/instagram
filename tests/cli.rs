//! 全部离线：列只读本机的文件，错误在读文件时就答。前半起插件进程（清空环境，代理指向关着的端口），
//! 后半（文件太大）用小的上限直接调库函数，不必真造几百 MB。

mod common;

use usuyo_plugin::{CallLimits, CallOptions, HostConfig, ListRequest, PluginHost, discover};

use std::path::{Path, PathBuf};

use common::{download_zip, envelope, fixture, lone_file, plugin, zip_of};
use serde_json::{Value, json};
use usuyo_fetch_instagram::{Error, Limits, export};
use usuyo_plugin::protocol::{
    Access, Account, ListOrder, ListPage, Login, PluginKind, PluginManifest,
};

fn manifest() -> PluginManifest {
    let text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("usuyo-plugin.yaml"),
    )
    .unwrap();
    let parsed = PluginManifest::from_yaml(&text).unwrap();
    assert!(
        parsed.unknown_fields.is_empty(),
        "{:?}",
        parsed.unknown_fields
    );
    parsed.value
}

#[test]
fn the_manifest_declares_the_export_method() {
    let m = manifest();
    let PluginKind::Fetch(spec) = &m.kind else {
        panic!("not a fetch plugin");
    };
    assert_eq!(
        spec.platforms
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>(),
        ["instagram"]
    );
    assert_eq!(spec.methods.len(), 1);
    let method = spec.method("export").unwrap();
    assert!(method.built);
    assert_eq!(
        (method.access, method.login, method.account),
        (Access::OfficialExport, Login::None, Account::None)
    );
    let list = method.capabilities.list.as_ref().unwrap();
    assert_eq!(list.order, ListOrder::NewestFirst);
    assert!(list.files.iter().any(|f| f == "saved_posts.json") && list.reads.is_some());
    assert!(method.capabilities.fetch.is_none());
    assert!(spec.match_patterns.is_empty());
}

fn list_page(input: &Path, cursor: Option<&str>) -> Value {
    let mut args = vec!["list", "file:download.zip", "--method", "export"];
    if let Some(c) = cursor {
        args.extend(["--cursor", c]);
    }
    let out = plugin(&args, Some(input));
    assert!(out.status.success(), "{out:?}");
    envelope(&out)
}

fn rows(value: &Value) -> Vec<(String, String, Option<String>, Option<String>)> {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["url"].as_str().unwrap().to_owned(),
                i["action"].as_str().unwrap().to_owned(),
                i["author"]["name"].as_str().map(str::to_owned),
                i["actedAt"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn post(code: &str) -> String {
    format!("https://www.instagram.com/p/{code}/")
}

fn row(
    code: &str,
    action: &str,
    author: &str,
    at: Option<&str>,
) -> (String, String, Option<String>, Option<String>) {
    (
        post(code),
        action.into(),
        Some(author.into()),
        at.map(str::to_owned),
    )
}

#[test]
fn lists_saved_collections_and_liked_posts_from_the_download() {
    let dir = tempfile::tempdir().unwrap();
    let zip = download_zip(dir.path());
    let value = list_page(&zip, None);
    assert_eq!(value["ok"], true);
    let mut page = value.clone();
    page.as_object_mut().unwrap().remove("ok");
    let parsed: ListPage = serde_json::from_value(page).unwrap();
    assert_eq!(parsed.next_cursor, None);
    // 新的在前；同一个帖子在收藏和收藏夹里各一条，留较新的时间；没有时间的在最后；
    // 主页链接和收藏夹自己的条目不算；乱码的作者名改回来了
    assert_eq!(
        rows(&value),
        [
            row(
                "DEMOlike0001",
                "like",
                "kayacollins2",
                Some("2026-09-22T18:00:00Z")
            ),
            row(
                "DEMOsaved01",
                "favorite",
                "café_roasters",
                Some("2026-09-21T14:13:20Z")
            ),
            row(
                "DEMOsaved03",
                "favorite",
                "你好",
                Some("2026-09-20T10:26:40Z")
            ),
            row(
                "DEMOcoll001",
                "favorite",
                "bean_lab",
                Some("2026-09-19T06:40:00Z")
            ),
            row(
                "DEMOsaved02",
                "favorite",
                "plain_user",
                Some("2026-09-18T16:46:40Z")
            ),
            row(
                "DEMOlike0002",
                "like",
                "maría_92",
                Some("2026-09-16T23:06:40Z")
            ),
            row("DEMOsaved04", "favorite", "no_time_user", None),
        ]
    );
    let first = &value["items"][0];
    assert_eq!(
        first["identity"],
        json!({"platform": "instagram", "contentId": "DEMOlike0001", "url": post("DEMOlike0001")})
    );
    assert_eq!(
        first["author"]["url"],
        "https://www.instagram.com/kayacollins2/"
    );
    // 链接条目：没有 ref，没有标题
    assert!(first.get("ref").is_none() && first.get("title").is_none());
    // 作者名不是用户名的写法（中文）：不给主页链接
    assert!(value["items"][2]["author"].get("url").is_none());
    assert_eq!(value["evidence"]["access"], "official-export");
}

#[test]
fn the_older_folder_names_and_a_root_folder_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("old.zip");
    zip_of(
        &zip,
        &[
            ("saved/saved_posts.json", &fixture("saved_posts.json")),
            ("likes/liked_posts.json", &fixture("liked_posts.json")),
        ],
    );
    let urls: Vec<String> = rows(&list_page(&zip, None))
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(urls.len(), 6);
    assert_eq!(urls[0], post("DEMOlike0001"));
    // 只有赞过的，也行
    let likes = dir.path().join("likes.zip");
    zip_of(
        &likes,
        &[("a/b/likes/liked_posts_1.json", &fixture("liked_posts.json"))],
    );
    assert_eq!(rows(&list_page(&likes, None)).len(), 2);
}

#[test]
fn each_file_alone_and_both_item_shapes() {
    let dir = tempfile::tempdir().unwrap();
    for (name, expect) in [
        ("saved_posts.json", 4),
        ("saved_collections.json", 2),
        ("liked_posts.json", 2),
        ("liked_posts_label_values.json", 1),
    ] {
        let path = lone_file(dir.path(), "input", &fixture(name));
        assert_eq!(rows(&list_page(&path, None)).len(), expect, "{name}");
    }
    let path = lone_file(
        dir.path(),
        "input",
        &fixture("liked_posts_label_values.json"),
    );
    assert_eq!(
        rows(&list_page(&path, None)),
        [row(
            "DEMOlabel001",
            "like",
            "owner_one",
            Some("2026-09-23T21:46:40Z")
        )]
    );
    // 顶层直接是数组：动作靠 zip 里的文件名；单个文件没有名字可看，不认
    let bare = json!([{"title": "u", "string_list_data": [{"href": "https://www.instagram.com/p/BARE1/", "timestamp": 1790000000}]}]);
    let zip = dir.path().join("bare.zip");
    zip_of(
        &zip,
        &[("likes/liked_posts.json", bare.to_string().as_bytes())],
    );
    assert_eq!(rows(&list_page(&zip, None))[0].1, "like");
    let lone = lone_file(dir.path(), "input", bare.to_string().as_bytes());
    let v = envelope(&plugin(
        &["list", "file:x", "--method", "export"],
        Some(&lone),
    ));
    assert_eq!(v["error"]["code"], "unsupported-input");
}

#[test]
fn text_is_repaired_only_when_it_was_mis_encoded() {
    let dir = tempfile::tempdir().unwrap();
    // 真正的 Latin-1 文字（José）不动；修过的（café）改回来；已经是对的 UTF-8（你好）不动
    let file = json!({"likes_media_likes": [
        {"title": "Jos\u{e9}", "string_list_data": [{"href": "https://www.instagram.com/p/L1/", "timestamp": 1790000003}]},
        {"title": "caf\u{c3}\u{a9}", "string_list_data": [{"href": "https://www.instagram.com/p/L2/", "timestamp": 1790000002}]},
        {"title": "你好", "string_list_data": [{"href": "https://www.instagram.com/p/L3/", "timestamp": 1790000001}]}
    ]});
    let path = lone_file(dir.path(), "input", file.to_string().as_bytes());
    let names: Vec<String> = rows(&list_page(&path, None))
        .into_iter()
        .map(|r| r.2.unwrap())
        .collect();
    assert_eq!(names, ["Jos\u{e9}", "café", "你好"]);
}

#[test]
fn list_pages_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let items: Vec<Value> = (1..=1200)
        .map(|n| json!({"title": "u", "string_list_data": [{"href": format!("https://www.instagram.com/p/P{n}/"), "timestamp": 1_790_000_000 + n}]}))
        .collect();
    let path = lone_file(
        dir.path(),
        "input",
        json!({"likes_media_likes": items}).to_string().as_bytes(),
    );
    let mut cursor: Option<String> = None;
    let mut urls = Vec::new();
    let mut pages = 0;
    loop {
        let value = list_page(&path, cursor.as_deref());
        pages += 1;
        urls.extend(
            value["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["url"].as_str().unwrap().to_owned()),
        );
        match value["nextCursor"].as_str() {
            Some(c) => cursor = Some(c.to_owned()),
            None => break,
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(urls.len(), 1200);
    assert_eq!(urls[0], post("P1200"));
    assert_eq!(urls[1199], post("P1"));
}

fn failure(path: &Path) -> Value {
    let out = plugin(&["list", "file:x", "--method", "export"], Some(path));
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    envelope(&out)
}

fn detail(v: &Value) -> &str {
    v["error"]["detail"].as_str().unwrap()
}

#[test]
fn the_html_form_is_answered_with_a_hint_to_ask_for_json() {
    let dir = tempfile::tempdir().unwrap();
    let html = lone_file(
        dir.path(),
        "input",
        b"<!DOCTYPE html><html><body>saved</body></html>",
    );
    let v = failure(&html);
    assert_eq!(v["error"]["code"], "unsupported-input");
    assert!(v["error"]["hint"].as_str().unwrap().contains("JSON"));
    let zip = dir.path().join("html.zip");
    zip_of(
        &zip,
        &[
            (
                "your_instagram_activity/saved/saved_posts.html",
                b"<html></html>",
            ),
            (
                "your_instagram_activity/likes/liked_posts.html",
                b"<html></html>",
            ),
        ],
    );
    let v = failure(&zip);
    assert_eq!(v["error"]["code"], "unsupported-input");
    assert!(v["error"]["hint"].as_str().unwrap().contains("JSON"));
    // 英文界面
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_usuyo-fetch-instagram"))
        .env_clear()
        .env("USUYO_LOCALE", "en-US")
        .env("USUYO_INPUT", &zip)
        .args(["list", "file:x", "--method", "export"])
        .output()
        .unwrap();
    assert!(
        envelope(&out)["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("JSON format")
    );
}

#[test]
fn malformed_files_are_answered_with_the_file_and_field() {
    let dir = tempfile::tempdir().unwrap();
    let bad = |bytes: &[u8]| failure(&lone_file(dir.path(), "input", bytes));
    let v = bad(b"hello");
    assert_eq!(v["error"]["code"], "unsupported-input");
    assert!(detail(&v).contains("file: input"));
    let v = bad(b"{\"likes_media_likes\": [{\"title\": ");
    assert_eq!(v["error"]["code"], "unsupported-input");
    let v = bad(b"{\"something_else\": []}");
    assert_eq!(v["error"]["code"], "unsupported-input");
    assert!(detail(&v).contains("likes_media_likes"));
    let v = bad(b"42");
    assert_eq!(v["error"]["code"], "unsupported-input");
    // 认得出键，条目却一个都找不到链接：字段变了
    let v = bad(
        json!({"likes_media_likes": [{"title": "a", "something": 1}, {"title": "b"}]})
            .to_string()
            .as_bytes(),
    );
    assert_eq!(v["error"]["code"], "upstream-changed");
    assert!(detail(&v).contains("file: input") && detail(&v).contains("likes_media_likes[]"));
    // 全是别的链接（主页）不是形状变了，是没有帖子
    let v = envelope(&plugin(
        &["list", "file:x", "--method", "export"],
        Some(&lone_file(dir.path(), "input", json!({"likes_media_likes": [{"title": "a", "string_list_data": [{"href": "https://www.instagram.com/a/", "timestamp": 1}]}]}).to_string().as_bytes())),
    ));
    assert_eq!(v["items"], json!([]));
    // zip 里：没有认得的文件、坏掉的 zip、文件里是坏的
    let none = dir.path().join("none.zip");
    zip_of(&none, &[("readme.txt", b"hi")]);
    let v = failure(&none);
    assert_eq!(v["error"]["code"], "unsupported-input");
    assert!(v["error"]["hint"].as_str().unwrap().contains("已保存"));
    let broken = dir.path().join("broken.zip");
    let mut bytes = std::fs::read(download_zip(dir.path())).unwrap();
    bytes.truncate(bytes.len() / 2);
    std::fs::write(&broken, bytes).unwrap();
    assert_eq!(failure(&broken)["error"]["code"], "unsupported-input");
    let inner = dir.path().join("inner.zip");
    zip_of(
        &inner,
        &[(
            "saved/saved_posts.json",
            b"{\"saved_saved_media\": [{\"title\": \"a\"}]}",
        )],
    );
    let v = failure(&inner);
    assert_eq!(v["error"]["code"], "upstream-changed");
    assert!(
        detail(&v).contains("file: saved/saved_posts.json")
            && detail(&v).contains("saved_saved_media[]")
    );
}

#[test]
fn a_zip_with_hostile_entry_names_writes_nothing_and_reads_nothing_outside() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let hostile = json!({"likes_media_likes": [{"title": "evil", "string_list_data": [{"href": "https://www.instagram.com/p/EVIL1/", "timestamp": 1}]}]}).to_string();
    let zip = dir.path().join("hostile.zip");
    zip_of(
        &zip,
        &[
            ("../../liked_posts.json", hostile.as_bytes()),
            ("/tmp/usuyo-escape/liked_posts.json", hostile.as_bytes()),
            ("..\\liked_posts.json", hostile.as_bytes()),
            ("a/../../liked_posts.json", hostile.as_bytes()),
            ("likes/liked_posts.json", &fixture("liked_posts.json")),
        ],
    );
    let urls: Vec<String> = rows(&list_page(&zip, None))
        .into_iter()
        .map(|r| r.0)
        .collect();
    assert_eq!(urls, [post("DEMOlike0001"), post("DEMOlike0002")]);
    // 只有名字有问题的：当没有
    let only = dir.path().join("only.zip");
    zip_of(&only, &[("../liked_posts.json", hostile.as_bytes())]);
    assert_eq!(failure(&only)["error"]["code"], "unsupported-input");
    assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0);
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["hostile.zip", "only.zip", "work"]);
    assert!(!Path::new("/tmp/usuyo-escape").exists());
}

#[test]
fn health_canonicalize_and_other_calls_send_no_request() {
    let v = envelope(&plugin(&["health"], None));
    assert_eq!(
        (v["ok"].clone(), v["ready"].clone(), v["protocol"].clone()),
        (json!(true), json!(true), json!(1))
    );
    assert_eq!(
        envelope(&plugin(&["health", "--probe"], None))["ready"],
        true
    );
    assert_eq!(
        envelope(&plugin(
            &["canonicalize", "https://www.instagram.com/p/AbC/"],
            None
        )),
        json!({"ok": true, "match": false})
    );
    let dir = tempfile::tempdir().unwrap();
    let out_arg = dir.path().to_str().unwrap();
    let v = envelope(&plugin(
        &["fetch", "x", "--out", out_arg, "--method", "export"],
        None,
    ));
    assert_eq!(v["error"]["code"], "invalid-args");
    let zip = download_zip(dir.path());
    let v = envelope(&plugin(
        &["list", "instagram:saved", "--method", "export"],
        Some(&zip),
    ));
    assert_eq!(v["error"]["code"], "invalid-args");
    let v = envelope(&plugin(&["list", "file:x", "--method", "export"], None));
    assert_eq!(v["error"]["code"], "invalid-args");
    let v = envelope(&plugin(
        &["list", "file:x", "--method", "export", "--cursor", "abc"],
        Some(&zip),
    ));
    assert_eq!(v["error"]["code"], "invalid-args");
    assert_eq!(plugin(&["unfavorite", "x"], None).status.code(), Some(2));
}

fn small() -> Limits {
    Limits {
        zip_entries: 4,
        json_bytes: 300,
        files: 2,
        items: 3,
    }
}

fn unsupported_detail(e: Error) -> String {
    match e {
        Error::Unsupported { detail, .. } => detail,
        other => panic!("{other:?}"),
    }
}

#[test]
fn files_over_the_limits_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    // 单个文件太大
    let big = lone_file(dir.path(), "input", &fixture("saved_posts.json"));
    let e = export::list("file:x", Some(&big), None, &small()).unwrap_err();
    assert!(unsupported_detail(e).contains("over the 300 byte limit"));
    // zip 里的条目太大
    let zip = dir.path().join("big.zip");
    zip_of(
        &zip,
        &[("saved/saved_posts.json", &fixture("saved_posts.json"))],
    );
    let e = export::list("file:x", Some(&zip), None, &small()).unwrap_err();
    assert!(unsupported_detail(e).contains("file: saved/saved_posts.json"));
    // 条目太多
    let many = dir.path().join("many.zip");
    zip_of(
        &many,
        &[
            ("a", b"1"),
            ("b", b"1"),
            ("c", b"1"),
            ("d", b"1"),
            ("e", b"1"),
        ],
    );
    let e = export::list("file:x", Some(&many), None, &small()).unwrap_err();
    assert!(unsupported_detail(e).contains("5 entries"));
    // 认得的文件太多
    let files = dir.path().join("files.zip");
    zip_of(
        &files,
        &[
            ("a/saved_posts.json", b"{}"),
            ("b/saved_posts.json", b"{}"),
            ("c/saved_posts.json", b"{}"),
        ],
    );
    let e = export::list("file:x", Some(&files), None, &small()).unwrap_err();
    assert!(unsupported_detail(e).contains("3 matching files"));
    // 列出的条目太多
    let items: Vec<Value> = (1..=4)
        .map(|n| json!({"title": "u", "string_list_data": [{"href": format!("https://www.instagram.com/p/P{n}/")}]}))
        .collect();
    let path = lone_file(
        dir.path(),
        "input",
        json!({"likes_media_likes": items}).to_string().as_bytes(),
    );
    let limits = Limits {
        json_bytes: 1 << 20,
        ..small()
    };
    let e = export::list("file:x", Some(&path), None, &limits).unwrap_err();
    assert!(unsupported_detail(e).contains("more than 3 entries"));
}

// ---- 清单经宿主装上，列（和取）经宿主调一次：输出要过宿主的 schema 和路径检查。
// 文件按宿主的方式交来：副本固定叫 `input.<扩展名>`，路径放在 `USUYO_INPUT`，来源是 `file:<显示名>`。

const HOST_BIN: &str = env!("CARGO_BIN_EXE_usuyo-fetch-instagram");

/// 清单装成宿主认的样子：`<root>/usuyo-fetch/instagram/<version>/`。
fn host() -> (tempfile::TempDir, PluginHost) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("plugins/usuyo-fetch/instagram/0.1.0");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("usuyo-plugin.yaml");
    std::fs::copy(manifest, dir.join("usuyo-plugin.yaml")).unwrap();
    std::fs::copy(HOST_BIN, dir.join("usuyo-fetch-instagram")).unwrap();
    let found = discover(&tmp.path().join("plugins")).unwrap();
    assert!(found.rejected.is_empty(), "{:?}", found.rejected);
    let host = PluginHost::new(
        HostConfig {
            work_dir: tmp.path().join("work"),
            cache_dir: tmp.path().join("cache"),
            locale: usuyo_plugin::Locale::new("zh-CN"),
            limits: CallLimits::default(),
        },
        found.fetch,
    );
    (tmp, host)
}

#[tokio::test]
async fn the_manifest_loads_and_a_file_is_listed_through_the_host() {
    let (tmp, host) = host();
    let plugin = &host.plugins()[0];
    assert_eq!(plugin.id().as_str(), "usuyo-fetch/instagram");
    let method = plugin.spec.method("export").unwrap();
    let list = method.capabilities.list.as_ref().unwrap();
    assert!(!list.files.is_empty() && list.reads.is_some());
    // 副本固定叫 input.zip
    let made = download_zip(tmp.path());
    let input = tmp.path().join("input.zip");
    std::fs::copy(made, &input).unwrap();
    let page = host
        .list(
            plugin,
            ListRequest {
                method: "export".into(),
                source: "file:download.zip".into(),
                cursor: None,
                since: None,
                input_file: Some(input.clone()),
            },
            &CallOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 7);
    assert!(
        page.items
            .iter()
            .all(|i| i.url.is_some() || i.reference.is_some())
    );
    let health = host.health(plugin, &CallOptions::default()).await.unwrap();
    assert!(health.ready);
}
