//! 岛的页面元数据唯一来源：名称、设置页签、描述、展开高度与标题。
//!
//! `settings.txt` 里的 `page_order` / `visible_pages` 按这里的下标持久化，
//! 所以**不要重排**已有页面；新增页面应追加，并在 `Settings::load` 里补迁移。

use eframe::egui::Color32;

pub(crate) const MUSIC: usize = 0;
pub(crate) const FOCUS: usize = 1;
pub(crate) const WATER: usize = 2;
pub(crate) const SHELF: usize = 3;
pub(crate) const TODO: usize = 4;
pub(crate) const MAIL: usize = 5;
pub(crate) const QUOTA: usize = 6;
pub(crate) const CALENDAR: usize = 7;
pub(crate) const COUNT: usize = 8;

/// 常规展开面板高度；日历页更高，邮件详情另有单独高度。
pub(crate) const PANEL_H: f32 = 140.0;

pub(crate) struct Spec {
    pub name: &'static str,
    pub settings_tab: usize,
    pub description: &'static str,
    pub height: f32,
    /// 面板标题的（前缀，强调词，强调色）；音乐和日程不画标题。
    pub title: Option<(&'static str, &'static str, Color32)>,
}

pub(crate) const SPECS: [Spec; COUNT] = [
    Spec {
        name: "音乐",
        settings_tab: 1,
        description: "播放控制与同步歌词",
        height: PANEL_H,
        title: None,
    },
    Spec {
        name: "专注",
        settings_tab: 2,
        description: "番茄钟与休息计时",
        height: PANEL_H,
        title: Some(("番茄", "专注", crate::theme::RED_TOP)),
    },
    Spec {
        name: "饮水",
        settings_tab: 3,
        description: "饮水记录与提醒",
        height: PANEL_H,
        title: Some(("饮水", "提醒", crate::theme::BLUE)),
    },
    Spec {
        name: "置物架",
        settings_tab: 5,
        description: "临时收纳文件位置",
        height: PANEL_H,
        title: Some(("置物架", "", crate::theme::WHITE)),
    },
    Spec {
        name: "待办",
        settings_tab: 4,
        description: "任务清单与到期提醒",
        height: PANEL_H,
        title: Some(("待办", "清单", crate::theme::GREEN)),
    },
    Spec {
        name: "邮箱",
        settings_tab: 7,
        description: "邮件摘要与验证码",
        height: PANEL_H,
        title: Some(("邮箱", "", crate::theme::CYAN)),
    },
    Spec {
        name: "AI 额度",
        settings_tab: 11,
        description: "Agent 剩余额度和账号切换",
        height: PANEL_H,
        title: Some(("AI", "额度", crate::theme::SOFT_GREEN)),
    },
    Spec {
        name: "日程",
        settings_tab: 8,
        description: "月历、邮件邀请与工作日程",
        height: 216.0,
        title: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_cover_every_page() {
        assert_eq!(SPECS.len(), COUNT);
        for (index, spec) in SPECS.iter().enumerate() {
            assert!(!spec.name.is_empty(), "page {index}");
            assert!(!spec.description.is_empty(), "page {index}");
            assert!(spec.height >= PANEL_H, "page {index}");
            // 设置窗口当前有 12 个页签；新增页签时这里和设置分发要一起改。
            assert!(spec.settings_tab < 12, "page {index}");
        }
        assert_eq!(SPECS[MUSIC].name, "音乐");
        assert_eq!(SPECS[CALENDAR].name, "日程");
        assert!(SPECS[CALENDAR].height > SPECS[MUSIC].height);
    }
}
