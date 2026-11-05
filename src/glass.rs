//! 玻璃材质的数值，逐条取自 oyenza-studio（MIT）。
//!
//! 对应关系见 `CHANGES-v0.5.5.md` 的对照表：
//! - `crates/poni-core/src/glass.rs`：烟熏不透明度、最小/最大着色、对比度目标、
//!   厚度、阴影；
//! - `crates/poni/src/glass.rs`：霜层模糊、rim/glow 强度、饱和度；
//! - `crates/poni/res/glass.hlsl`：`dark = mat * (1 - tint)`、rim 与 glow 的叠加方式；
//! - `crates/poni-core/src/look.rs`：玻璃底色与描边。
//!
//! 这里用 egui 网格近似「烟熏玻璃」：半透明深色底、顶部柔光、rim/glow、
//! 霜边和色散；没有桌面模糊采样时用 `dark_tint(None)` 的兜底着色。

use eframe::egui::Color32;

/// 看不到屏幕时的烟熏玻璃不透明度（oyenza `glass.rs:21`）。
pub const SMOKED_OPACITY: f32 = 0.82;
/// 深色玻璃的着色范围（`crates/poni/src/glass.rs:673`）。
pub const TINT_MIN: f32 = 0.34;
pub const TINT_MAX: f32 = 0.85;
/// 文字要达到的对比度（`poni-core/src/glass.rs:173`）。
pub const TEXT_CONTRAST: f32 = 4.8;
/// 玻璃最薄/最厚的高度（DIP，`poni-core/src/glass.rs:25-26`）。
const THIN_H: f32 = 37.0;
const THICK_H: f32 = 220.0;
/// 边缘高光与内发光的强度（`crates/poni/src/glass.rs:693-694`）。
pub const RIM_STRENGTH: f32 = 0.80;
pub const RIM_FACING_MIN: f32 = 0.30;
pub const GLOW_STRENGTH: f32 = 0.09;
/// 两层霜的模糊半径，DIP（`crates/poni/src/glass.rs:29-30`）。
pub const FROST_LO: f32 = 3.0;
pub const FROST_HI: f32 = 9.0;
/// 颜色饱和度与色散（`crates/poni/src/glass.rs:699-700, 725`）。
pub const SATURATION: f32 = 1.45;
pub const DISPERSION: f32 = 0.03;
/// 深色玻璃的底色与描边（`poni-core/src/look.rs:139, 142`）。
pub const GROUND_DARK: (u8, u8, u8) = (0x1C, 0x1C, 0x1E);
pub const BORDER_ALPHA: f32 = 0.10;

/// 一帧的玻璃材质。绘制层只读这里，避免各处自行解释常量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub ground: (u8, u8, u8),
    pub tint: f32,
    pub opacity: f32,
    pub saturation: f32,
    pub rim: f32,
    pub glow: f32,
    pub border: f32,
    pub frost_lo: f32,
    pub frost_hi: f32,
    pub dispersion: f32,
    pub shadow: f32,
    pub thick: f32,
}

/// 按当前高度、桌面色和忙碌程度生成材质。
pub fn material(height: f32, backdrop: Option<[f32; 3]>, busy: f32) -> Material {
    let thick = thickness(height);
    let tint = dark_tint(backdrop);
    // 收起的小条更实，展开面板允许透出一点桌面；与烟熏玻璃的观感一致。
    let opacity = (SMOKED_OPACITY + (1.0 - SMOKED_OPACITY) * (1.0 - thick) * 0.5).clamp(0.0, 1.0);
    Material {
        ground: GROUND_DARK,
        tint,
        opacity,
        saturation: SATURATION,
        rim: (rim_alpha() * (0.62 + 0.38 * thick)).clamp(0.0, 1.0),
        glow: (GLOW_STRENGTH * (0.60 + 0.40 * thick)).clamp(0.0, 0.5),
        border: BORDER_ALPHA,
        frost_lo: FROST_LO,
        frost_hi: FROST_HI,
        dispersion: DISPERSION,
        shadow: shadow_alpha(busy, thick),
        thick,
    }
}

fn ground_rgba(m: &Material, lift: f32) -> [f32; 4] {
    let [r, g, b] = [m.ground.0, m.ground.1, m.ground.2].map(|c| c as f32 / 255.0);
    let mean = (r + g + b) / 3.0;
    let saturated = |c: f32| (mean + (c - mean) * m.saturation).clamp(0.0, 1.0);
    let dim = 1.0 - m.tint;
    [(saturated(r) * dim + lift).clamp(0.0, 1.0), (saturated(g) * dim + lift).clamp(0.0, 1.0), (saturated(b) * dim + lift).clamp(0.0, 1.0), m.opacity]
}

/// 玻璃底色：地面色去饱和、按 tint 混黑，再乘烟熏不透明度。
pub fn ground_color(m: &Material) -> Color32 {
    let [r, g, b, a] = ground_rgba(m, 0.0);
    Color32::from_rgba_unmultiplied((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8, (a * 255.0).round() as u8)
}

/// 顶部柔光：同一底色抬亮 `glow`，填充时从上到下渐变回底色。
pub fn sheen_color(m: &Material) -> Color32 {
    let [r, g, b, a] = ground_rgba(m, m.glow * 0.55);
    Color32::from_rgba_unmultiplied((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8, (a * 255.0).round() as u8)
}

/// 边缘高光：rim + border，色散让高光略偏暖。
pub fn rim_color(m: &Material, facing: f32) -> Color32 {
    let alpha = ((m.rim + m.border) * facing).clamp(0.0, 1.0);
    let warm = (m.dispersion * 255.0 * 0.7).round() as u8;
    Color32::from_rgba_unmultiplied(255, 255u8.saturating_sub(warm / 2), 255u8.saturating_sub(warm), (alpha * 255.0).round() as u8)
}

/// 内发光：贴着轮廓向内淡出。
pub fn glow_color(m: &Material) -> Color32 {
    Color32::from_white_alpha((m.glow * 255.0).round() as u8)
}

/// 霜边：比 glow 更宽、更淡的一层。
pub fn frost_color(m: &Material) -> Color32 {
    Color32::from_white_alpha((m.glow * 0.22 * 255.0).round() as u8)
}

/// 玻璃厚度：0 = 收起的小药丸，1 = 展开的面板。
pub fn thickness(height: f32) -> f32 {
    ((height - THIN_H) / (THICK_H - THIN_H)).clamp(0.0, 1.0)
}

/// 阴影强度（`poni-core/src/glass.rs:238-240`）。
pub fn shadow_alpha(busy: f32, thick: f32) -> f32 {
    (0.12 + 0.06 * thick + 1.1 * busy).clamp(0.12, 0.34)
}

/// WCAG 相对亮度（`poni-core/src/glass.rs:120-123`）。
pub fn rel_luminance(c: [f32; 3]) -> f32 {
    fn lin(v: f32) -> f32 {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG 对比度，1..21。
pub fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (la, lb) = (rel_luminance(a), rel_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// 让 `ink` 在「底色 `material` 混入 `toward`」上至少达到 `target` 对比度，
/// 取满足条件的最小混合比例；范围是 `min..=max`（`poni-core/src/glass.rs:139-157`）。
pub fn tint_for_contrast(
    material: [f32; 3],
    ink: [f32; 3],
    toward: [f32; 3],
    min: f32,
    max: f32,
    target: f32,
) -> f32 {
    let ok = |t: f32| contrast(mix3(material, toward, t), ink) >= target;
    if ok(min) {
        return min;
    }
    if !ok(max) {
        return max;
    }
    let (mut lo, mut hi) = (min, max);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// 深色玻璃在给定背景上的着色量。没有背景样本时用 oyenza 的兜底值 `0.34`
/// （`crates/poni/src/glass.rs:678`）。
pub fn dark_tint(backdrop: Option<[f32; 3]>) -> f32 {
    match backdrop {
        Some(b) => tint_for_contrast(b, [1.0; 3], [0.0; 3], TINT_MIN, TINT_MAX, TEXT_CONTRAST),
        None => TINT_MIN,
    }
}

/// 边缘高光的基础强度：rim 0.80 × 背光面 0.30（`glass.hlsl:149`）。
pub fn rim_alpha() -> f32 {
    RIM_STRENGTH * RIM_FACING_MIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast([1.0; 3], [0.0; 3]) - 21.0).abs() < 0.01);
        assert!((contrast([0.5; 3], [0.5; 3]) - 1.0).abs() < 1e-5);
        let g = 0x76 as f32 / 255.0;
        assert!((contrast([g; 3], [1.0; 3]) - 4.54).abs() < 0.02);
    }

    #[test]
    fn tint_stops_where_text_becomes_readable() {
        let orange = [0.98, 0.70, 0.45];
        let t = tint_for_contrast(orange, [1.0; 3], [0.0; 3], TINT_MIN, TINT_MAX, 4.5);
        assert!(t > TINT_MIN && t < TINT_MAX, "{t}");
        assert!(contrast(mix3(orange, [0.0; 3], t), [1.0; 3]) >= 4.5);
        assert!(contrast(mix3(orange, [0.0; 3], t - 0.02), [1.0; 3]) < 4.5);
        // 深色背景：最小着色就够
        assert_eq!(tint_for_contrast([0.10, 0.10, 0.12], [1.0; 3], [0.0; 3], TINT_MIN, TINT_MAX, 4.5), TINT_MIN);
        // 白底：着色抬到刚够 4.8:1，而不是无脑拉满
        let white = dark_tint(Some([1.0; 3]));
        assert!(white > TINT_MIN && white < TINT_MAX, "{white}");
        assert!(contrast(mix3([1.0; 3], [0.0; 3], white), [1.0; 3]) >= TEXT_CONTRAST);
    }

    #[test]
    fn dark_tint_falls_back_to_oyenza_minimum() {
        assert_eq!(dark_tint(None), TINT_MIN);
        // 深色页面按最小着色，浅色页面抬到能读
        assert!(dark_tint(Some([0.12, 0.12, 0.14])) < dark_tint(Some([0.9, 0.9, 0.9])));
    }

    #[test]
    fn busy_backdrops_get_deeper_shadows() {
        assert!(shadow_alpha(0.0, 0.0) < shadow_alpha(0.1, 0.0));
        assert!(shadow_alpha(0.0, 0.0) < shadow_alpha(0.0, 1.0));
        assert_eq!(shadow_alpha(5.0, 1.0), 0.34);
        assert!((shadow_alpha(0.0, 0.0) - 0.12).abs() < 1e-6);
    }

    #[test]
    fn thickness_and_rim_follow_oyenza() {
        assert_eq!(thickness(37.0), 0.0);
        assert_eq!(thickness(220.0), 1.0);
        assert!(thickness(140.0) > 0.5 && thickness(140.0) < 0.6);
        assert!((rim_alpha() - 0.24).abs() < 1e-6);
    }

    #[test]
    fn material_wires_every_glass_layer() {
        let thin = material(30.0, None, 0.0);
        let thick = material(216.0, Some([1.0; 3]), 0.05);
        assert!(thin.thick < 0.2 && thick.thick > 0.95);
        assert_eq!(thin.tint, TINT_MIN, "没有背景样本时用烟熏兜底");
        assert!(thick.tint > TINT_MIN && thick.tint <= TINT_MAX, "亮背景要加深着色");
        assert!(thin.opacity > thick.opacity, "收起小条应比展开面板更实");
        assert!(thin.opacity >= SMOKED_OPACITY);
        assert!(thin.rim > 0.0 && thick.rim > thin.rim);
        assert!(thin.glow > 0.0 && thick.glow >= thin.glow);
        assert!(thin.shadow >= 0.12 && thick.shadow > thin.shadow);
        assert_eq!(thin.border, BORDER_ALPHA);
        assert_eq!(thin.frost_lo, FROST_LO);
        assert_eq!(thin.frost_hi, FROST_HI);
        assert_eq!(thin.dispersion, DISPERSION);
    }

    #[test]
    fn material_colors_follow_tint_sheen_and_facing() {
        let dark = material(140.0, Some([0.05; 3]), 0.0);
        let bright = material(140.0, Some([1.0; 3]), 0.0);
        assert!(ground_color(&bright).r() < ground_color(&dark).r(), "亮背景上的玻璃应更暗");
        assert!(ground_color(&dark).a() >= (SMOKED_OPACITY * 255.0 * 0.99) as u8);
        assert!(sheen_color(&dark).r() > ground_color(&dark).r(), "顶部柔光应比底色亮");
        let rim = rim_color(&dark, 1.0);
        assert!(rim.a() > rim_color(&dark, RIM_FACING_MIN).a(), "背光面 rim 应更弱");
        assert_ne!(rim, Color32::WHITE, "色散应让边缘略偏暖");
        assert!(glow_color(&dark).a() > frost_color(&dark).a(), "霜边应比内发光更淡");
    }
}
