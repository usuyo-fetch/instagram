//! Instagram 的 JSON 把 UTF-8 的字节当成 Latin-1 字符写出来（`ð\u009f\u0098\u0080` 本该是一个表情）。
//! 读回来要把每个字符当一个字节，再按 UTF-8 解一次。

/// 改回来。只在每个字符都不超过 U+00FF、且这些字节合起来是合法的 UTF-8 时才改；
/// 真正的 Latin-1 文字（`café`）凑不成合法的 UTF-8，原样不动。
pub fn repair(text: &str) -> String {
    if text.is_ascii() {
        return text.to_owned();
    }
    let bytes: Option<Vec<u8>> = text
        .chars()
        .map(|c| u8::try_from(u32::from(c)).ok())
        .collect();
    bytes
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::repair;

    #[test]
    fn repairs_mis_encoded_text() {
        // "é" 是 C3 A9，表情 😀 是 F0 9F 98 80，"你" 是 E4 BD A0
        assert_eq!(repair("caf\u{c3}\u{a9}"), "café");
        assert_eq!(repair("\u{f0}\u{9f}\u{98}\u{80}"), "😀");
        assert_eq!(repair("\u{e4}\u{bd}\u{a0}\u{e5}\u{a5}\u{bd}"), "你好");
        assert_eq!(repair("mar\u{c3}\u{ad}a_92"), "mar\u{ed}a_92");
    }

    #[test]
    fn leaves_everything_else_alone() {
        assert_eq!(repair("plain_name.1"), "plain_name.1");
        // 真正的 Latin-1：é 单独出现不是合法 UTF-8
        assert_eq!(repair("café"), "café");
        assert_eq!(repair("Jos\u{e9}"), "Jos\u{e9}");
        // 已经是正确的 UTF-8 文字：有超过 U+00FF 的字符，不动
        assert_eq!(repair("你好"), "你好");
        assert_eq!(repair("😀"), "😀");
        // 修了一次的不会再修第二次
        assert_eq!(repair(&repair("caf\u{c3}\u{a9}")), "café");
    }
}
