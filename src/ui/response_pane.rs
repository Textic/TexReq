use gpui_kit::{
    Context, SharedString, Window,
    component::{
        IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{Editor, Input},
        scroll::ScrollableElement as _,
        v_flex,
    },
    div,
    prelude::*,
    px, relative,
};

use crate::models::{StreamDirection, StreamStatus, format_bytes};
use crate::theme::{
    ACCENT_BLUE, BG_BASE, BG_ELEVATED, BG_SURFACE, BORDER_SUBTLE, COLOR_DELETE, COLOR_GET,
    COLOR_POST, COLOR_STREAM, RADIUS_CONTROL, RADIUS_ROW, TEXT_MUTED, TEXT_PRIMARY,
    full_hit_input, solid, status_color,
};
use crate::ui::kv_editor::render_flat_subtab;
use crate::ui::tab_session::ResponseSubTab;
use crate::ui::workspace::Workspace;

pub fn render_response_pane(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let Some(tab) = ws.active_tab() else {
        return div().size_full().bg(solid(BG_BASE)).into_any_element();
    };

    if tab.kind.is_realtime() {
        return render_realtime_stream_pane(ws, cx).into_any_element();
    }

    if tab.is_loading {
        return v_flex()
            .size_full()
            .overflow_hidden()
            .bg(solid(BG_BASE))
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(solid(TEXT_PRIMARY))
                    .child("Executing HTTP request..."),
            )
            .child(
                Button::new("res-cancel-loading")
                    .with_size(Size::Small)
                    .danger()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cancel_active_http(cx);
                    })),
            )
            .into_any_element();
    }

    if let Some(ref err) = tab.last_error {
        return v_flex()
            .size_full()
            .overflow_hidden()
            .bg(solid(BG_BASE))
            .p_3()
            .child(
                v_flex()
                    .p_3()
                    .gap_1p5()
                    .bg(solid(BG_ELEVATED))
                    .border_1()
                    .border_color(solid(COLOR_DELETE))
                    .rounded(px(RADIUS_ROW))
                    .child(
                        div()
                            .text_xs()
                            .font_bold()
                            .text_color(solid(COLOR_DELETE))
                            .child("Request failed"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_PRIMARY))
                            .child(SharedString::from(err.clone())),
                    ),
            )
            .into_any_element();
    }

    let Some(ref resp) = tab.last_response else {
        return v_flex()
            .size_full()
            .overflow_hidden()
            .bg(solid(BG_BASE))
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(solid(TEXT_PRIMARY))
                    .child("Ready to send"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("Send a request to inspect the response here."),
            )
            .into_any_element();
    };

    let status_hex = status_color(resp.status);
    let status_label = if resp.status_text.is_empty() {
        format!("{}", resp.status)
    } else {
        format!("{} {}", resp.status, resp.status_text)
    };
    let time_label = format!("{} ms", resp.elapsed_ms);
    let size_label = format_bytes(resp.size_bytes);
    let hdr_cnt = resp.headers.len();
    let cookie_cnt = resp.cookies.len();
    let captured_summary = if resp.captured_vars.is_empty() {
        None
    } else {
        let names: Vec<&str> = resp.captured_vars.iter().map(|(k, _)| k.as_str()).collect();
        Some(format!("Captured: {}", names.join(", ")))
    };

    v_flex()
        .size_full()
        .overflow_hidden()
        .bg(solid(BG_BASE))
        .p_3()
        .gap_2p5()
        .child(
            // Status + Metrics + Response Subtabs
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .pb_1p5()
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            render_flat_subtab(
                                "restab-pretty",
                                "Pretty",
                                tab.res_subtab == ResponseSubTab::Pretty,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_response_subtab(ResponseSubTab::Pretty, cx);
                            })),
                        )
                        .child(
                            render_flat_subtab(
                                "restab-raw",
                                "Raw",
                                tab.res_subtab == ResponseSubTab::Raw,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_response_subtab(ResponseSubTab::Raw, cx);
                            })),
                        )
                        .child(
                            render_flat_subtab(
                                "restab-headers",
                                &format!("Headers ({hdr_cnt})"),
                                tab.res_subtab == ResponseSubTab::Headers,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_response_subtab(ResponseSubTab::Headers, cx);
                            })),
                        )
                        .child(
                            render_flat_subtab(
                                "restab-cookies",
                                &format!("Cookies ({cookie_cnt})"),
                                tab.res_subtab == ResponseSubTab::Cookies,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_response_subtab(ResponseSubTab::Cookies, cx);
                            })),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .when_some(captured_summary, |this, summary| {
                            this.child(
                                div()
                                    .px_2()
                                    .py_0p5()
                                    .bg(solid(BG_ELEVATED))
                                    .border_1()
                                    .border_color(solid(COLOR_STREAM))
                                    .rounded(px(RADIUS_CONTROL))
                                    .text_xs()
                                    .text_color(solid(COLOR_STREAM))
                                    .child(SharedString::from(summary)),
                            )
                        })
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .bg(solid(BG_ELEVATED))
                                .border_1()
                                .border_color(solid(status_hex))
                                .rounded(px(RADIUS_CONTROL))
                                .text_xs()
                                .font_bold()
                                .text_color(solid(status_hex))
                                .child(SharedString::from(status_label)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child(SharedString::from(time_label)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child(SharedString::from(size_label)),
                        )
                        .child(
                            Button::new("res-copy-btn")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::Copy)
                                .label("Copy")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.copy_active_response(cx);
                                })),
                        ),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(match tab.res_subtab {
                    ResponseSubTab::Pretty => div()
                        .size_full()
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .overflow_hidden()
                        .child(
                            Editor::new(&tab.response_viewer)
                                .readonly(true)
                                .h(relative(1.))
                                .bordered(false),
                        )
                        .into_any_element(),
                    ResponseSubTab::Raw => div()
                        .size_full()
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .overflow_hidden()
                        .child(
                            Editor::new(&tab.response_raw_viewer)
                                .readonly(true)
                                .h(relative(1.))
                                .bordered(false),
                        )
                        .into_any_element(),
                    ResponseSubTab::Headers => {
                        render_pair_table(&resp.headers, "Header", "Value").into_any_element()
                    }
                    ResponseSubTab::Cookies => {
                        if resp.cookies.is_empty() {
                            div()
                                .p_3()
                                .bg(solid(BG_SURFACE))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .rounded(px(RADIUS_ROW))
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child("The response did not include cookies (Set-Cookie).")
                                .into_any_element()
                        } else {
                            render_pair_table(&resp.cookies, "Cookie", "Value").into_any_element()
                        }
                    }
                }),
        )
        .into_any_element()
}

fn render_pair_table(pairs: &[(String, String)], col1: &str, col2: &str) -> impl IntoElement {
    v_flex()
        .size_full()
        .bg(solid(BG_SURFACE))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .overflow_hidden()
        .child(
            h_flex()
                .w_full()
                .px_3()
                .py_1p5()
                .bg(solid(BG_ELEVATED))
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_MUTED))
                .child(div().w(px(240.)).child(SharedString::from(col1.to_string())))
                .child(div().flex_1().child(SharedString::from(col2.to_string()))),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .children(pairs.iter().enumerate().map(|(ix, (k, v))| {
                            h_flex()
                                .id(("resp-pair", ix))
                                .w_full()
                                .px_3()
                                .py_1p5()
                                .border_b_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .text_xs()
                                .child(
                                    div()
                                        .w(px(240.))
                                        .pr_2()
                                        .font_medium()
                                        .text_color(solid(TEXT_PRIMARY))
                                        .child(SharedString::from(k.clone())),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_color(solid(TEXT_MUTED))
                                        .child(SharedString::from(v.clone())),
                                )
                        })),
                ),
        )
}

fn render_realtime_stream_pane(
    ws: &Workspace,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let tab = ws.active_tab().unwrap();
    let status_bar = match tab.stream_status {
        StreamStatus::Connected => COLOR_GET,
        StreamStatus::Connecting => COLOR_POST,
        StreamStatus::Disconnected => TEXT_MUTED,
    };
    let filter = tab
        .stream_filter_input
        .read(cx)
        .value()
        .trim()
        .to_ascii_lowercase();

    let entries: Vec<_> = tab
        .stream_entries
        .iter()
        .rev()
        .filter(|e| {
            filter.is_empty()
                || e.payload.to_ascii_lowercase().contains(&filter)
                || e.event_name
                    .as_ref()
                    .is_some_and(|n| n.to_ascii_lowercase().contains(&filter))
        })
        .collect();

    v_flex()
        .size_full()
        .overflow_hidden()
        .bg(solid(BG_BASE))
        .p_3()
        .gap_2p5()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .pb_1p5()
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .w(px(3.))
                                .h(px(12.))
                                .rounded(px(1.))
                                .bg(solid(status_bar)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(tab.stream_status.label()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child(SharedString::from(format!(
                                    "{} messages",
                                    tab.stream_entries.len()
                                ))),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            full_hit_input(
                                "stream-filter-slot",
                                &tab.stream_filter_input,
                                Input::new(&tab.stream_filter_input)
                                    .with_size(Size::XSmall)
                                    .appearance(false),
                            )
                            .w(px(200.))
                            .h(px(28.))
                            .px_2()
                            .bg(solid(BG_ELEVATED))
                            .border_1()
                            .border_color(solid(BORDER_SUBTLE))
                            .rounded(px(RADIUS_ROW)),
                        )
                        .child(
                            Button::new("stream-clear")
                                .with_size(Size::XSmall)
                                .secondary()
                                .label("Clear")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_stream_log(cx);
                                })),
                        )
                        .child(
                            Button::new("stream-copy")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::Copy)
                                .label("Copy")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.copy_stream_log(cx);
                                })),
                        ),
                ),
        )
        .when_some(tab.last_error.clone(), |this, err| {
            this.child(
                div()
                    .p_2p5()
                    .bg(solid(BG_ELEVATED))
                    .border_1()
                    .border_color(solid(COLOR_DELETE))
                    .rounded(px(RADIUS_ROW))
                    .text_xs()
                    .text_color(solid(COLOR_DELETE))
                    .child(SharedString::from(err)),
            )
        })
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .p_2()
                        .gap_1p5()
                        .when(entries.is_empty(), |col| {
                            col.child(
                                div()
                                    .p_3()
                                    .text_xs()
                                    .text_color(solid(TEXT_MUTED))
                                    .child("Connect the stream to view live messages here."),
                            )
                        })
                        .children(entries.into_iter().enumerate().map(|(ix, entry)| {
                            let (dir_lbl, dir_hex) = match entry.direction {
                                StreamDirection::Outgoing => ("↑ SENT", ACCENT_BLUE),
                                StreamDirection::Incoming => ("↓ RECEIVED", COLOR_GET),
                                StreamDirection::System => ("• SYSTEM", TEXT_MUTED),
                            };

                            v_flex()
                                .id(("stream-msg", ix))
                                .w_full()
                                .p_2p5()
                                .gap_1p5()
                                .bg(solid(BG_ELEVATED))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .rounded(px(RADIUS_ROW))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .justify_between()
                                        .items_center()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_bold()
                                                        .text_color(solid(dir_hex))
                                                        .child(dir_lbl),
                                                )
                                                .when_some(
                                                    entry.event_name.clone(),
                                                    |row, ev_name| {
                                                        row.child(
                                                            div()
                                                                .px_1p5()
                                                                .py_0p5()
                                                                .bg(solid(BG_SURFACE))
                                                                .border_1()
                                                                .border_color(solid(BORDER_SUBTLE))
                                                                .rounded(px(RADIUS_CONTROL))
                                                                .text_xs()
                                                                .text_color(solid(COLOR_STREAM))
                                                                .child(SharedString::from(ev_name)),
                                                        )
                                                    },
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(solid(TEXT_MUTED))
                                                .child(SharedString::from(entry.timestamp.clone())),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(solid(TEXT_PRIMARY))
                                        .child(SharedString::from(entry.payload.clone())),
                                )
                        })),
                ),
        )
        .into_any_element()
}
