//! 交来的文件不可信：各处读多少、数多少都有上限。测试用小的上限，运行用 [`Limits::default`]。

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// zip 里最多多少个条目。
    pub zip_entries: usize,
    /// 一个 JSON 文件最多多少字节（zip 里的看解压后读到的字节）。
    pub json_bytes: u64,
    /// 一次最多读几个 JSON 文件。
    pub files: usize,
    /// 列出的条目最多多少条。
    pub items: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            zip_entries: 500_000,
            json_bytes: 128 << 20,
            files: 32,
            items: 1_000_000,
        }
    }
}
