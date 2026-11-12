//! usuyo 的 Instagram 获取插件：读你向 Instagram 申请下载的 JSON 数据（整个 zip，或其中的
//! `saved_posts.json`、`saved_collections.json`、`liked_posts.json`），把收藏和赞过的帖子列成链接条目。不联网。
//!
//! - [`cli`]：子命令和信封，入口只调它。
//! - `input`：打开交来的文件（json 或 zip），按名字读 zip 里的条目，不解压到磁盘。
//! - [`export`]：认几种文件形状，列出帖子（`list`）。
//! - [`text`]：把被当成 Latin-1 读的 UTF-8 文字改回来。

pub mod cli;
mod error;
pub mod export;
mod input;
mod limits;
pub mod text;

pub use error::{Error, Lang};
pub use limits::Limits;
