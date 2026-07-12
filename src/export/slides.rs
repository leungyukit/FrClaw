//! 单张 slide 的结构。

/// 一张 slide。
pub struct Slide {
    pub kind: SlideKind,
}

#[derive(Debug, Clone)]
pub enum SlideKind {
    /// 封面大标题
    Title {
        title: String,
        subtitle: Option<String>,
    },
    /// 标题 + bullet body
    Heading {
        title: String,
        body: Vec<String>,
    },
    /// section divider（整页大标题）
    Section {
        title: String,
    },
}

impl SlideKind {
    pub fn title(t: impl Into<String>) -> Self {
        SlideKind::Title { title: t.into(), subtitle: None }
    }
    pub fn title_sub(t: impl Into<String>, s: impl Into<String>) -> Self {
        SlideKind::Title { title: t.into(), subtitle: Some(s.into()) }
    }
    pub fn heading(t: impl Into<String>, body: Vec<String>) -> Self {
        SlideKind::Heading { title: t.into(), body }
    }
    pub fn section(t: impl Into<String>) -> Self {
        SlideKind::Section { title: t.into() }
    }
}
