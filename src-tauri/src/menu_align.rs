//! 托盘菜单文本测量（macOS）。
//!
//! muda 0.19 没有暴露 NSMenuItem 的原生句柄，无法设置 attributedTitle 或右制表位，
//! NSMenuItem.badge 也未穿透到 Tauri 层。因此「值右对齐」只能退化为：
//! 用菜单真实字体量出每行文本宽度，再在名称与值之间补空白，把值的右边界推到
//! 菜单最宽一行的末尾。空白优先用普通空格（3.58pt），余量用 thin/hair space
//! （1.75pt / 0.84pt）收尾，误差 < 0.9pt。

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSFont, NSFontAttributeName, NSStringDrawing};
use objc2_foundation::{NSAttributedStringKey, NSDictionary, NSString};

/// 以系统菜单字体测量字符串宽度。构造一次、复用 N 次，避免每行重复查字体。
pub(crate) struct MenuTextMeasurer {
    #[allow(dead_code)] // 持有字体以保证 attributes 字典里的引用始终有效
    font: Retained<NSFont>,
    attrs: Retained<NSDictionary<NSAttributedStringKey, AnyObject>>,
}

impl MenuTextMeasurer {
    /// 字体查询失败时返回 None（调用方退化为不右对齐，保持原样）。
    pub(crate) fn new() -> Option<Self> {
        // SAFETY: NSFont 为不可变对象；NSStringDrawing 的测量会自建排版上下文，
        // 不共享 state，可在后台线程调用。
        unsafe {
            let font = NSFont::menuFontOfSize(0.0);
            let font_ref: &AnyObject = &*(Retained::as_ptr(&font) as *const AnyObject);
            let attrs = NSDictionary::from_slices(&[NSFontAttributeName], &[font_ref]);
            Some(Self { font, attrs })
        }
    }

    pub(crate) fn width(&self, text: &str) -> f64 {
        let s = NSString::from_str(text);
        // SAFETY: attrs 只含 NSFontAttributeName -> NSFont，类型与文档一致。
        let size = unsafe { s.sizeWithAttributes(Some(&self.attrs)) };
        size.width
    }
}
