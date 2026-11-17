use std::rc::Rc;

use gpui_kit::{
    App, Axis, Hsla,
    base::{ResizeHandleContext, ResizeHandleRenderer, ResizeHandleState},
    component::{Theme, ThemeMode},
    div,
    prelude::*,
    px, rgb,
};

use crate::models::{BodyKind, HttpMethod, RequestKind};

pub const BG_BASE: u32 = 0x11141b;
pub const BG_SURFACE: u32 = 0x141821;
pub const BG_ELEVATED: u32 = 0x191e29;
pub const BG_HOVER: u32 = 0x222938;
pub const BORDER: u32 = 0x283042;
pub const BORDER_SUBTLE: u32 = 0x1e2432;

pub const TEXT_PRIMARY: u32 = 0xe5e7eb;
pub const TEXT_MUTED: u32 = 0x94a3b8;

pub const ACCENT_BLUE: u32 = 0x3b82f6;
pub const ACCENT_BLUE_HOVER: u32 = 0x2563eb;
pub const ACCENT_BLUE_ACTIVE: u32 = 0x1d4ed8;

pub const COLOR_GET: u32 = 0x22c55e;
pub const COLOR_POST: u32 = 0xeab308;
pub const COLOR_PUT: u32 = 0x3b82f6;
pub const COLOR_PATCH: u32 = 0xa855f7;
pub const COLOR_DELETE: u32 = 0xef4444;
pub const COLOR_DELETE_HOVER: u32 = 0xdc2626;
pub const COLOR_HEAD_OPT: u32 = 0x06b6d4;
pub const COLOR_STREAM: u32 = 0x14b8a6;
pub const COLOR_STREAM_HOVER: u32 = 0x0d9488;
pub const COLOR_GRAPHQL: u32 = 0xec4899;

pub const RADIUS_ROW: f32 = 6.;
pub const RADIUS_CONTROL: f32 = 4.;
pub const RADIUS_CIRCLE: f32 = 999.;

#[inline]
pub fn solid(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Crisp flat 1px panel cut divider at rest; highlights cleanly when hovered or dragged.
pub fn crisp_resize_handle_appearance() -> ResizeHandleRenderer {
    Rc::new(|handle: &ResizeHandleContext, _, _| {
        let axis = handle.axis();
        let (color, thick) = match handle.state() {
            ResizeHandleState::Idle => (solid(BORDER_SUBTLE), false),
            ResizeHandleState::Hovered => (solid(0x475569), true),
            ResizeHandleState::Pressed | ResizeHandleState::Dragging => (solid(ACCENT_BLUE), true),
        };

        Some(
            div()
                .flex_none()
                .bg(color)
                .map(|d| match (axis, thick) {
                    (Axis::Horizontal, false) => d.w(px(1.)).h_full(),
                    (Axis::Horizontal, true) => d.w(px(2.)).h_full(),
                    (Axis::Vertical, false) => d.h(px(1.)).w_full(),
                    (Axis::Vertical, true) => d.h(px(2.)).w_full(),
                })
                .into_any_element(),
        )
    })
}

/// Applies the Strict Flat Dark palette: dark mode by default, zero gradients,
/// zero drop shadows, crisp edge-to-edge section cuts and subtle 4-6px controls.
pub fn apply_flat_dark_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.shadow = false;
        theme.focus_ring = false;
        theme.radius = px(RADIUS_CONTROL);
        theme.radius_lg = px(RADIUS_ROW);
        theme.font_size = px(13.);
        theme.mono_font_size = px(12.5);

        let bg_base = solid(BG_BASE);
        let bg_surface = solid(BG_SURFACE);
        let bg_elevated = solid(BG_ELEVATED);
        let bg_hover = solid(BG_HOVER);
        let border = solid(BORDER);
        let fg = solid(TEXT_PRIMARY);
        let fg_muted = solid(TEXT_MUTED);
        let primary = solid(ACCENT_BLUE);
        let primary_hover = solid(ACCENT_BLUE_HOVER);
        let primary_active = solid(ACCENT_BLUE_ACTIVE);
        let danger = solid(COLOR_DELETE);
        let danger_hover = solid(COLOR_DELETE_HOVER);
        let success = solid(COLOR_GET);
        let warning = solid(COLOR_POST);
        let info = solid(COLOR_STREAM);

        let c = &mut theme.colors;
        c.background = bg_base;
        c.foreground = fg;
        c.border = border;
        c.input = border;
        c.caret = primary;
        c.ring = primary;
        c.selection = solid(0x1e3a5f);
        c.drag_border = primary;
        c.drop_target = bg_hover;

        c.muted = bg_elevated;
        c.muted_foreground = fg_muted;
        c.accent = bg_hover;
        c.accent_foreground = fg;

        c.primary = primary;
        c.primary_hover = primary_hover;
        c.primary_active = primary_active;
        c.primary_foreground = solid(0xffffff);

        c.secondary = bg_elevated;
        c.secondary_hover = bg_hover;
        c.secondary_active = bg_hover;
        c.secondary_foreground = fg;

        c.button = bg_elevated;
        c.button_hover = bg_hover;
        c.button_active = bg_hover;
        c.button_foreground = fg;

        c.button_primary = primary;
        c.button_primary_hover = primary_hover;
        c.button_primary_active = primary_active;
        c.button_primary_foreground = solid(0xffffff);

        c.button_secondary = bg_elevated;
        c.button_secondary_hover = bg_hover;
        c.button_secondary_active = bg_hover;
        c.button_secondary_foreground = fg;

        c.danger = danger;
        c.danger_hover = danger_hover;
        c.danger_active = danger_hover;
        c.danger_foreground = solid(0xffffff);
        c.button_danger = danger;
        c.button_danger_hover = danger_hover;
        c.button_danger_active = danger_hover;
        c.button_danger_foreground = solid(0xffffff);

        c.success = success;
        c.success_hover = solid(0x16a34a);
        c.success_active = solid(0x15803d);
        c.success_foreground = solid(0x090b0e);
        c.button_success = success;
        c.button_success_hover = solid(0x16a34a);
        c.button_success_active = solid(0x15803d);
        c.button_success_foreground = solid(0x090b0e);

        c.warning = warning;
        c.warning_hover = solid(0xca8a04);
        c.warning_active = solid(0xa16207);
        c.warning_foreground = solid(0x090b0e);
        c.button_warning = warning;
        c.button_warning_hover = solid(0xca8a04);
        c.button_warning_active = solid(0xa16207);
        c.button_warning_foreground = solid(0x090b0e);

        c.info = info;
        c.info_hover = solid(COLOR_STREAM_HOVER);
        c.info_active = solid(0x0f766e);
        c.info_foreground = solid(0x090b0e);
        c.button_info = info;
        c.button_info_hover = solid(COLOR_STREAM_HOVER);
        c.button_info_active = solid(0x0f766e);
        c.button_info_foreground = solid(0x090b0e);

        c.popover = bg_elevated;
        c.popover_foreground = fg;

        c.sidebar = bg_surface;
        c.sidebar_foreground = fg;
        c.sidebar_border = border;
        c.sidebar_accent = bg_hover;
        c.sidebar_accent_foreground = fg;
        c.sidebar_primary = primary;
        c.sidebar_primary_foreground = solid(0xffffff);

        c.list = bg_surface;
        c.list_even = bg_surface;
        c.list_head = bg_elevated;
        c.list_hover = bg_hover;
        c.list_active = bg_hover;
        c.list_active_border = primary;
        c.list_highlight_foreground = primary;

        c.tab_bar = bg_surface;
        c.tab_bar_segmented = bg_surface;
        c.tab = bg_surface;
        c.tab_foreground = fg_muted;
        c.tab_active = bg_base;
        c.tab_active_foreground = fg;

        c.table = bg_base;
        c.table_even = bg_surface;
        c.table_head = bg_surface;
        c.table_head_foreground = fg_muted;
        c.table_foot = bg_surface;
        c.table_foot_foreground = fg_muted;
        c.table_hover = bg_hover;
        c.table_active = bg_hover;
        c.table_active_border = primary;
        c.table_row_border = solid(BORDER_SUBTLE);

        c.title_bar = bg_surface;
        c.title_bar_border = solid(BORDER_SUBTLE);
        c.status_bar = bg_surface;
        c.status_bar_border = solid(BORDER_SUBTLE);

        c.scrollbar = bg_base;
        c.scrollbar_thumb = border;
        c.scrollbar_thumb_hover = solid(0x3a4358);

        c.group_box = bg_surface;
        c.group_box_foreground = fg;
        c.switch = bg_hover;
        c.switch_thumb = fg;
        c.link = primary;
        c.link_hover = primary_hover;
        c.link_active = primary_active;
    });
}

/// Returns `(short_label, solid_hex_color)` for request badges in sidebar, tabs, and method pills.
pub fn badge_info(kind: RequestKind, body: &BodyKind) -> (&'static str, u32) {
    match kind {
        RequestKind::WebSocket => ("WS", COLOR_STREAM),
        RequestKind::Sse => ("SSE", COLOR_STREAM),
        RequestKind::Http(method) => {
            if matches!(body, BodyKind::GraphQl { .. }) {
                return ("GQL", COLOR_GRAPHQL);
            }
            match method {
                HttpMethod::Get => ("GET", COLOR_GET),
                HttpMethod::Post => ("POST", COLOR_POST),
                HttpMethod::Put => ("PUT", COLOR_PUT),
                HttpMethod::Patch => ("PATCH", COLOR_PATCH),
                HttpMethod::Delete => ("DEL", COLOR_DELETE),
                HttpMethod::Head => ("HEAD", COLOR_HEAD_OPT),
                HttpMethod::Options => ("OPT", COLOR_HEAD_OPT),
            }
        }
    }
}

pub fn status_color(status: u16) -> u32 {
    match status {
        200..=299 => COLOR_GET,
        300..=399 => ACCENT_BLUE,
        400..=499 => COLOR_POST,
        500..=599 => COLOR_DELETE,
        _ => TEXT_MUTED,
    }
}
