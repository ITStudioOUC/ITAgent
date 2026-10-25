//! 事件正文预览的截断。
//!
//! 正文本身只进内容寻址存储，库里只留两段预览：头与尾。默认头 4 KiB 加尾 1 KiB，依据是
//! "报错通常在尾部，上下文通常在头部"（ADR 0018 第九节参数表）。这两个数字是默认值而不是
//! 不变量，因此收进 [`PreviewLimit`]，由调用方传入，只在 [`PreviewLimit::defaults`] 一处出现。
//!
//! 截断按字节切，但必须回退到字符边界：按字节切会切开多字节字符，留下非法 UTF-8。判据是
//! "头加尾"这个总预算：正文不超过总预算时整段留下、不切两段；超过才切。

/// 预览预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewLimit {
    head: usize,
    tail: usize,
}

impl PreviewLimit {
    /// ADR 0018 第九节参数表的默认值：头 4 KiB、尾 1 KiB。
    pub fn defaults() -> Self {
        Self { head: 4096, tail: 1024 }
    }

    /// 头预算（字节）。
    pub fn head(&self) -> usize {
        self.head
    }

    /// 尾预算（字节）。
    pub fn tail(&self) -> usize {
        self.tail
    }

    /// 头加尾的总预算。正文不超过它时不截断。
    pub fn total(&self) -> usize {
        self.head + self.tail
    }
}

/// 把正文截成头尾两段预览。
///
/// # 步骤
/// 1. 正文不超过总预算时，整段进头、尾为空，不截断。
/// 2. 超过时，头取前 `head` 字节、尾取后 `tail` 字节，各自回退到字符边界。
///
/// # 边界与前提
/// 回退方向：头向内收（字节数只减不增），尾向后移（起点只增不减）。两段都不超过各自预算。
/// 判据是头加尾的总预算，不是两段各自的上限。
///
/// # 副作用
/// 无。纯函数。
///
/// # 返回值
/// (头, 尾)。不截断时尾为 `None`。
pub fn truncate(body: &str, limit: &PreviewLimit) -> (String, Option<String>) {
    if body.len() <= limit.total() {
        return (body.to_owned(), None);
    }
    let head = head_cut(body, limit.head);
    let tail = tail_cut(body, limit.tail);
    (head.to_owned(), Some(tail.to_owned()))
}

/// 取正文前 `budget` 字节，回退到字符边界。
fn head_cut(body: &str, budget: usize) -> &str {
    if body.len() <= budget {
        return body;
    }
    let mut n = budget;
    while n > 0 && !body.is_char_boundary(n) {
        n -= 1;
    }
    &body[..n]
}

/// 取正文后 `budget` 字节，回退到字符边界。
fn tail_cut(body: &str, budget: usize) -> &str {
    if body.len() <= budget {
        return body;
    }
    let mut n = body.len() - budget;
    while !body.is_char_boundary(n) {
        n += 1;
    }
    &body[n..]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 不超过总预算时整段留下，尾为空。
    #[test]
    fn short_body_not_truncated() {
        let limit = PreviewLimit::defaults();
        let body = "a".repeat(5120);
        let (head, tail) = truncate(&body, &limit);
        assert_eq!(head, body);
        assert!(tail.is_none(), "不超过预算时尾应当为空");
    }

    /// 超过总预算时切两段，头 4096 尾 1024，中间省略。
    #[test]
    fn long_body_cut_to_head_and_tail() {
        let limit = PreviewLimit::defaults();
        let body = "a".repeat(6000);
        let (head, tail) = truncate(&body, &limit);
        assert_eq!(head.len(), 4096, "头应当是前 4096 字节");
        assert_eq!(head, &body[..4096]);
        let tail = tail.expect("超长应当有尾");
        assert_eq!(tail.len(), 1024, "尾应当是最后 1024 字节");
        assert_eq!(tail, &body[6000 - 1024..]);
    }

    /// 恰好等于总预算不截断，加一字节截断。
    #[test]
    fn boundary_at_total_budget() {
        let limit = PreviewLimit::defaults();
        let at = "a".repeat(5120);
        let (head, tail) = truncate(&at, &limit);
        assert_eq!(head, at, "恰好 5120 字节不截断");
        assert!(tail.is_none(), "恰好 5120 字节尾为空");

        let over = "a".repeat(5121);
        let (head, tail) = truncate(&over, &limit);
        assert_eq!(head.len(), 4096, "5121 字节头为 4096");
        assert_eq!(tail.as_ref().unwrap().len(), 1024, "5121 字节尾为 1024");
    }

    /// 多字节字符不被切开：全中文正文，头尾各自是合法 UTF-8 且长度不超过预算。
    #[test]
    fn multibyte_chars_not_split() {
        let limit = PreviewLimit::defaults();
        // 每字 3 字节，4096 不是 3 的倍数，首段只能退到 4095 字节。
        let body = "中".repeat(3000); // 9000 字节
        let (head, tail) = truncate(&body, &limit);
        assert_eq!(head.len(), 4095, "首段应当退到 4095 字节");
        assert_eq!(head, &body[..4095], "头应当是正文的头部");
        let tail = tail.expect("超长应当有尾");
        assert_eq!(tail.len(), 1023, "尾段应当是 1023 字节");
        assert_eq!(tail, &body[7977..], "尾应当是正文的尾部");
    }
}
