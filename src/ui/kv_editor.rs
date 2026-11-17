use gpui_kit::{
    App, AppContext as _, Context, Entity, SharedString, Window,
    component::{
        IconName, Sizable as _, Size,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        h_flex,
        input::{Input, InputState},
        v_flex,
    },
    div,
    prelude::*,
    px,
};
use uuid::Uuid;

use crate::models::{CaptureRule, KeyValueRow};
use crate::theme::{
    BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW, TEXT_MUTED,
    TEXT_PRIMARY, solid,
};
use crate::ui::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KvTarget {
    Params,
    Headers,
    Form,
    Multipart,
    EnvVars,
}

pub struct UiKvRow {
    pub id: String,
    pub enabled: bool,
    pub key_input: Entity<InputState>,
    pub val_input: Entity<InputState>,
    pub is_file: bool,
}

impl UiKvRow {
    pub fn from_model(row: &KeyValueRow, window: &mut Window, cx: &mut App) -> Self {
        let key_val = row.key.clone();
        let val_val = row.value.clone();
        let key_input = cx.new(|cx| InputState::new(window, cx).placeholder("Key").default_value(key_val));
        let val_placeholder = if row.is_file {
            "File path..."
        } else {
            "Value"
        };
        let val_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(val_placeholder)
                .default_value(val_val)
        });
        Self {
            id: row.id.clone(),
            enabled: row.enabled,
            key_input,
            val_input,
            is_file: row.is_file,
        }
    }

    pub fn empty(is_file: bool, window: &mut Window, cx: &mut App) -> Self {
        Self::from_model(
            &KeyValueRow {
                id: Uuid::new_v4().to_string(),
                enabled: true,
                key: String::new(),
                value: String::new(),
                is_file,
            },
            window,
            cx,
        )
    }

    pub fn to_model(&self, cx: &App) -> KeyValueRow {
        KeyValueRow {
            id: self.id.clone(),
            enabled: self.enabled,
            key: self.key_input.read(cx).value().to_string(),
            value: self.val_input.read(cx).value().to_string(),
            is_file: self.is_file,
        }
    }
}

pub struct UiCaptureRow {
    pub id: String,
    pub enabled: bool,
    pub var_input: Entity<InputState>,
    pub path_input: Entity<InputState>,
}

impl UiCaptureRow {
    pub fn from_model(rule: &CaptureRule, window: &mut Window, cx: &mut App) -> Self {
        let var_name = rule.var_name.clone();
        let json_path = rule.json_path.clone();
        let var_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. token")
                .default_value(var_name)
        });
        let path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. $.access_token")
                .default_value(json_path)
        });
        Self {
            id: rule.id.clone(),
            enabled: rule.enabled,
            var_input,
            path_input,
        }
    }

    pub fn empty(window: &mut Window, cx: &mut App) -> Self {
        Self::from_model(&CaptureRule::new("", ""), window, cx)
    }

    pub fn to_model(&self, cx: &App) -> CaptureRule {
        CaptureRule {
            id: self.id.clone(),
            enabled: self.enabled,
            var_name: self.var_input.read(cx).value().to_string(),
            json_path: self.path_input.read(cx).value().to_string(),
        }
    }
}

pub fn render_kv_table(
    rows: &[UiKvRow],
    target: KvTarget,
    allow_file_mode: bool,
    key_header: &str,
    val_header: &str,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let prefix = match target {
        KvTarget::Params => "kv-params",
        KvTarget::Headers => "kv-headers",
        KvTarget::Form => "kv-form",
        KvTarget::Multipart => "kv-multi",
        KvTarget::EnvVars => "kv-env",
    };

    v_flex()
        .w_full()
        .gap_2()
        .child(
            // Single clean table containing header + rows without double borders
            v_flex()
                .w_full()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    h_flex()
                        .w_full()
                        .h(px(30.))
                        .px_3()
                        .gap_2()
                        .bg(solid(BG_ELEVATED))
                        .border_b_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .items_center()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child(div().w(px(22.)).child(""))
                        .child(div().flex_1().child(SharedString::from(key_header.to_string())))
                        .child(div().flex_1().child(SharedString::from(val_header.to_string())))
                        .child(div().w(px(if allow_file_mode { 112. } else { 28. }))),
                )
                .when(rows.is_empty(), |this| {
                    this.child(
                        div()
                            .px_3()
                            .py_3()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child("No rows yet. Click + Add Row to begin."),
                    )
                })
                .children(rows.iter().enumerate().map(|(ix, row)| {
                    let row_id = row.id.clone();
                    let row_id_del = row.id.clone();
                    let row_id_kind = row.id.clone();
                    let row_id_pick = row.id.clone();
                    let checked = row.enabled;
                    let is_file = row.is_file;
                    let is_last = ix + 1 == rows.len();

                    h_flex()
                        .id((prefix, ix))
                        .w_full()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .bg(solid(BG_SURFACE))
                        .hover(|s| s.bg(solid(BG_ELEVATED)))
                        .when(!is_last, |this| {
                            this.border_b_1().border_color(solid(BORDER_SUBTLE))
                        })
                        .items_center()
                        .child(
                            Checkbox::new((SharedString::from(format!("{prefix}-chk")), ix))
                                .checked(checked)
                                .on_change(cx.listener(move |this, next, window, cx| {
                                    this.kv_toggle(target, &row_id, *next, window, cx);
                                })),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&row.key_input)
                                    .with_size(Size::XSmall)
                                    .appearance(false),
                            ),
                        )
                        .child(div().w(px(1.)).h(px(16.)).bg(solid(BORDER_SUBTLE)))
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&row.val_input)
                                    .with_size(Size::XSmall)
                                    .appearance(false),
                            ),
                        )
                        .when(allow_file_mode, |this| {
                            this.child(
                                Button::new((SharedString::from(format!("{prefix}-type")), ix))
                                    .with_size(Size::XSmall)
                                    .secondary()
                                    .label(if is_file { "File" } else { "Text" })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.kv_toggle_file_mode(&row_id_kind, window, cx);
                                    })),
                            )
                            .when(is_file, |this| {
                                this.child(
                                    Button::new((SharedString::from(format!("{prefix}-pick")), ix))
                                        .with_size(Size::XSmall)
                                        .secondary()
                                        .icon(IconName::FolderOpen)
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.pick_file_for_multipart(&row_id_pick, window, cx);
                                        })),
                                )
                            })
                        })
                        .child(
                            Button::new((SharedString::from(format!("{prefix}-del")), ix))
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.kv_remove(target, &row_id_del, window, cx);
                                })),
                        )
                })),
        )
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new(SharedString::from(format!("{prefix}-add")))
                        .with_size(Size::XSmall)
                        .secondary()
                        .icon(IconName::Plus)
                        .label("Add Row")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.kv_add(target, false, window, cx);
                        })),
                )
                .when(allow_file_mode, |this| {
                    this.child(
                        Button::new(SharedString::from(format!("{prefix}-add-file")))
                            .with_size(Size::XSmall)
                            .secondary()
                            .icon(IconName::File)
                            .label("Add File")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.kv_add(target, true, window, cx);
                            })),
                    )
                }),
        )
}

pub fn render_capture_table(rows: &[UiCaptureRow], cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .px_3()
                .py_2()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Extract fields from the JSON response without JavaScript and save them automatically to the active Environment. Example: variable 'token' = path '$.access_token'."),
        )
        .child(
            v_flex()
                .w_full()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .when(rows.is_empty(), |this| {
                    this.child(
                        div()
                            .px_3()
                            .py_3()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child("No capture rules configured."),
                    )
                })
                .children(rows.iter().enumerate().map(|(ix, row)| {
                    let row_id = row.id.clone();
                    let row_id_del = row.id.clone();
                    let checked = row.enabled;
                    let is_last = ix + 1 == rows.len();

                    h_flex()
                        .id(("cap-row", ix))
                        .w_full()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .bg(solid(BG_SURFACE))
                        .hover(|s| s.bg(solid(BG_ELEVATED)))
                        .when(!is_last, |this| {
                            this.border_b_1().border_color(solid(BORDER_SUBTLE))
                        })
                        .items_center()
                        .child(
                            Checkbox::new(("cap-chk", ix))
                                .checked(checked)
                                .on_change(cx.listener(move |this, next, _, cx| {
                                    this.capture_toggle(&row_id, *next, cx);
                                })),
                        )
                        .child(
                            div().w(px(160.)).child(
                                Input::new(&row.var_input)
                                    .with_size(Size::XSmall)
                                    .appearance(false),
                            ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child("="),
                        )
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(&row.path_input)
                                    .with_size(Size::XSmall)
                                    .appearance(false),
                            ),
                        )
                        .child(
                            Button::new(("cap-del", ix))
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.capture_remove(&row_id_del, window, cx);
                                })),
                        )
                })),
        )
        .child(
            h_flex().child(
                Button::new("cap-add")
                    .with_size(Size::XSmall)
                    .secondary()
                    .icon(IconName::Plus)
                    .label("New Capture Rule")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.capture_add(window, cx);
                    })),
            ),
        )
}

pub fn render_flat_subtab(
    id: impl Into<gpui_kit::ElementId>,
    label: &str,
    active: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .px_2p5()
        .py_1()
        .rounded(px(RADIUS_CONTROL))
        .cursor_pointer()
        .bg(if active {
            solid(BG_HOVER)
        } else {
            gpui_kit::transparent_black()
        })
        .hover(|s| s.bg(solid(BG_ELEVATED)))
        .text_xs()
        .text_color(if active {
            solid(TEXT_PRIMARY)
        } else {
            solid(TEXT_MUTED)
        })
        .child(SharedString::from(label.to_string()))
}
