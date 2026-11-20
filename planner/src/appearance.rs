// SPDX-License-Identifier: Apache-2.0
//! Desktop typography and semantic colors for the planner workspace.

use std::{path::Path, sync::Arc};

use eframe::egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, RichText,
    Stroke, TextStyle, Theme, Visuals, vec2,
};

pub const INDIGO: Color32 = Color32::from_rgb(99, 102, 241);
pub const PRIMARY: Color32 = Color32::from_rgb(79, 70, 229);
pub const BUTTON_TEXT: Color32 = Color32::from_rgb(250, 250, 250);
pub const LOW_IMPACT: Color32 = Color32::from_rgb(53, 218, 167);
pub const ZONE: Color32 = Color32::from_rgb(113, 83, 153);

pub fn install(ctx: &Context) {
    for theme in [Theme::Light, Theme::Dark] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = palette(theme);
        style.spacing.item_spacing = vec2(8.0, 8.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.spacing.interact_size = vec2(32.0, 30.0);
        style.spacing.text_edit_width = 200.0;
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.0));
        ctx.set_style_of(theme, style);
    }
    ctx.set_theme(Theme::Light);
    ctx.set_fonts(fonts());
}

fn palette(theme: Theme) -> Visuals {
    let (page, surface, muted, text, border, hover, selected, link, error) = match theme {
        Theme::Light => (
            Color32::from_gray(250),
            Color32::WHITE,
            Color32::from_gray(107),
            Color32::from_gray(10),
            Color32::from_rgb(232, 232, 236),
            Color32::from_rgb(244, 244, 248),
            Color32::from_rgb(235, 235, 255),
            PRIMARY,
            Color32::from_rgb(177, 38, 38),
        ),
        Theme::Dark => (
            Color32::from_rgb(20, 20, 24),
            Color32::from_rgb(27, 27, 33),
            Color32::from_rgb(173, 173, 187),
            Color32::from_rgb(243, 243, 247),
            Color32::from_rgb(60, 60, 72),
            Color32::from_rgb(41, 41, 51),
            Color32::from_rgb(48, 44, 89),
            Color32::from_rgb(173, 180, 255),
            Color32::from_rgb(255, 152, 152),
        ),
    };
    let mut visuals = match theme {
        Theme::Light => Visuals::light(),
        Theme::Dark => Visuals::dark(),
    };
    visuals.panel_fill = page;
    visuals.window_fill = surface;
    visuals.extreme_bg_color = surface;
    visuals.faint_bg_color = hover;
    visuals.hyperlink_color = link;
    visuals.error_fg_color = error;
    visuals.selection.bg_fill = selected;
    visuals.selection.stroke = Stroke::new(1.0_f32, link);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
    visuals.widgets.noninteractive.fg_stroke.color = text;
    visuals.widgets.noninteractive.bg_fill = surface;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(6);
        widget.fg_stroke.color = text;
        widget.bg_fill = surface;
        widget.weak_bg_fill = surface;
        widget.bg_stroke = Stroke::new(1.0_f32, border);
        widget.expansion = 0.0;
    }
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.bg_stroke.color = muted;
    visuals.widgets.active.bg_stroke.color = INDIGO;
    visuals.widgets.active.bg_fill = selected;
    visuals.widgets.active.weak_bg_fill = selected;
    visuals.widgets.open.bg_fill = hover;
    visuals.widgets.open.weak_bg_fill = hover;
    visuals.widgets.inactive.fg_stroke.width = 1.0;
    visuals
}

pub fn muted(visuals: &Visuals) -> Color32 {
    if visuals.dark_mode {
        Color32::from_rgb(173, 173, 187)
    } else {
        Color32::from_gray(107)
    }
}

pub fn display(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, display_family()))
        .extra_letter_spacing(-size * 0.035)
}

fn display_family() -> FontFamily {
    FontFamily::Name(Arc::from("display"))
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "DM Sans",
            include_bytes!("../fonts/DMSans-Regular.ttf").as_slice(),
        ),
        (
            "DM Sans Bold",
            include_bytes!("../fonts/DMSans-Bold.ttf").as_slice(),
        ),
        (
            "JetBrains Mono",
            include_bytes!("../fonts/JetBrainsMono-Regular.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), FontData::from_static(bytes).into());
    }
    let body = fonts.families.entry(FontFamily::Proportional).or_default();
    body.insert(0, "DM Sans".into());
    let mut headings = body.clone();
    headings.insert(0, "DM Sans Bold".into());
    if let Some(bytes) =
        std::env::var_os("SILENT_PLANNER_DISPLAY_FONT").and_then(|path| std::fs::read(path).ok())
    {
        fonts
            .font_data
            .insert("General Sans".into(), FontData::from_owned(bytes).into());
        headings.insert(0, "General Sans".into());
    }
    let candidates = [
        "/usr/share/fonts/google-noto-sans-cjk-vf-fonts/NotoSansCJK-VF.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "C:/Windows/Fonts/malgun.ttf",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ];
    if let Some(bytes) = candidates
        .iter()
        .find_map(|path| std::fs::read(Path::new(path)).ok())
    {
        let fallback = "system place names";
        fonts
            .font_data
            .insert(fallback.into(), FontData::from_owned(bytes).into());
        body.push(fallback.into());
        headings.push(fallback.into());
    }
    fonts.families.insert(display_family(), headings);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrains Mono".into());
    fonts
}

#[cfg(test)]
mod tests {
    use eframe::egui::Rgba;

    use super::*;

    fn contrast(foreground: Color32, background: Color32) -> f32 {
        let luminance = |color| {
            let rgba = Rgba::from(color);
            0.2126 * rgba.r() + 0.7152 * rgba.g() + 0.0722 * rgba.b()
        };
        let first = luminance(foreground);
        let second = luminance(background);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn text_and_controls_meet_aa_contrast_in_both_appearances() {
        for theme in [Theme::Light, Theme::Dark] {
            let visuals = palette(theme);
            for background in [visuals.panel_fill, visuals.window_fill] {
                for foreground in [
                    visuals.text_color(),
                    muted(&visuals),
                    visuals.hyperlink_color,
                    visuals.error_fg_color,
                ] {
                    assert!(contrast(foreground, background) >= 4.5);
                }
            }
            assert!(contrast(visuals.hyperlink_color, visuals.selection.bg_fill) >= 4.5);
        }
        assert!(contrast(BUTTON_TEXT, PRIMARY) >= 4.5);
    }
}
