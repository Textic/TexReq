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

use crate::net::snippets::{SnippetLang, build_resolved_headers, build_resolved_url};
use crate::theme::{
    ACCENT_BLUE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW,
    TEXT_MUTED, TEXT_PRIMARY, solid,
};
use crate::ui::kv_editor::{KvTarget, render_flat_subtab, render_kv_table};
use crate::ui::workspace::{RightDrawerTab, Workspace};

pub fn render_right_rail_and_drawer(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let active = ws.active_drawer;

    h_flex()
        .h_full()
        .flex_shrink_0()
        .when_some(active, |this, tab| {
            this.child(
                v_flex()
                    .w(px(360.))
                    .h_full()
                    .bg(solid(BG_SURFACE))
                    .border_l_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .overflow_hidden()
                    .child(match tab {
                        RightDrawerTab::Snippets => render_snippets_drawer(ws, cx).into_any_element(),
                        RightDrawerTab::Variables => render_variables_drawer(ws, cx).into_any_element(),
                        RightDrawerTab::NetworkInfo => render_network_info_drawer(ws, cx).into_any_element(),
                    }),
            )
        })
        .child(
            // Seamless vertical right rail separated by crisp 1px cut
            v_flex()
                .w(px(40.))
                .h_full()
                .flex_shrink_0()
                .py_2p5()
                .gap_1p5()
                .items_center()
                .bg(solid(BG_SURFACE))
                .border_l_1()
                .border_color(solid(BORDER_SUBTLE))
                .child(
                    rail_button(
                        "rail-snippets",
                        "</>",
                        active == Some(RightDrawerTab::Snippets),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_drawer(RightDrawerTab::Snippets, window, cx);
                    })),
                )
                .child(
                    rail_button(
                        "rail-vars",
                        "{x}",
                        active == Some(RightDrawerTab::Variables),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_drawer_no_window(RightDrawerTab::Variables, cx);
                    })),
                )
                .child(
                    rail_button(
                        "rail-net",
                        "ⓘ",
                        active == Some(RightDrawerTab::NetworkInfo),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_drawer_no_window(RightDrawerTab::NetworkInfo, cx);
                    })),
                ),
        )
}

fn rail_button(
    id: &'static str,
    symbol: &'static str,
    active: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .w(px(28.))
        .h(px(28.))
        .rounded(px(RADIUS_CONTROL))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .bg(if active {
            solid(BG_HOVER)
        } else {
            gpui_kit::transparent_black()
        })
        .hover(|s| s.bg(solid(BG_HOVER)))
        .text_xs()
        .font_bold()
        .text_color(if active {
            solid(ACCENT_BLUE)
        } else {
            solid(TEXT_MUTED)
        })
        .child(symbol)
}

fn render_snippets_drawer(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let current_lang = ws.snippet_lang;

    v_flex()
        .size_full()
        .p_3()
        .gap_2p5()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child("Code Snippets"),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("snip-refresh")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::RefreshCw)
                                .tooltip("Refresh snippet")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.refresh_snippet_editor(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("snip-copy")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::Copy)
                                .label("Copy")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.copy_snippet(cx);
                                })),
                        ),
                ),
        )
        .child(
            h_flex()
                .gap_1()
                .flex_wrap()
                .children(SnippetLang::ALL.iter().enumerate().map(|(ix, &lang)| {
                    render_flat_subtab(("snip-lang", ix), lang.label(), current_lang == lang)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.snippet_lang = lang;
                            this.refresh_snippet_editor(window, cx);
                            cx.notify();
                        }))
                })),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    Editor::new(&ws.snippet_viewer)
                        .readonly(true)
                        .h(relative(1.))
                        .bordered(false),
                ),
        )
}

fn render_variables_drawer(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let env_name = ws
        .active_environment()
        .map(|e| e.name.clone())
        .unwrap_or_else(|| "No active environment".to_string());

    v_flex()
        .size_full()
        .p_3()
        .gap_2p5()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child(SharedString::from(format!("Variables · {env_name}"))),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .flex_shrink_0()
                        .child(
                            Button::new("vars-import-env")
                                .with_size(Size::XSmall)
                                .secondary()
                                .label(".env")
                                .tooltip("Import .env file into active environment")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.import_dotenv_dialog(window, cx);
                                })),
                        )
                        .when(ws.active_env_id.is_some(), |el| {
                            el.child(
                                Button::new("vars-save")
                                    .with_size(Size::XSmall)
                                    .primary()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_active_env_vars(cx);
                                    })),
                            )
                            .child(
                                Button::new("vars-delete-env")
                                    .with_size(Size::XSmall)
                                    .danger()
                                    .icon(IconName::Delete)
                                    .label("Delete")
                                    .tooltip("Delete this environment")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_delete_active_environment(cx);
                                    })),
                            )
                        }),
                ),
        )
        .when(ws.active_env_id.is_some(), |el| {
            el.child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_medium()
                            .text_color(solid(TEXT_MUTED))
                            .child("Environment Name"),
                    )
                    .child(Input::new(&ws.env_name_input).with_size(Size::XSmall)),
            )
        })
        .child(
            div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Use {{key}} in URL, Params, Headers, Auth, or Body."),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(if ws.active_env_id.is_some() {
                    render_kv_table(
                        &ws.env_var_rows,
                        KvTarget::EnvVars,
                        false,
                        "Variable",
                        "Value",
                        cx,
                    )
                    .into_any_element()
                } else {
                    div()
                        .p_3()
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Select or create an environment above to edit variables.")
                        .into_any_element()
                }),
        )
}

fn render_network_info_drawer(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let vars = ws.resolved_env_map(cx);
    let tab = ws.active_tab();

    let (resolved_url, preview_headers, last_resp) = match tab {
        Some(t) => {
            let model = t.sync_to_model(cx);
            (
                build_resolved_url(&model, &vars),
                build_resolved_headers(&model, &vars),
                t.last_response.clone(),
            )
        }
        None => (String::new(), Vec::new(), None),
    };

    v_flex()
        .size_full()
        .p_3()
        .gap_2p5()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child("Network / TLS Inspector"),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .gap_2p5()
                        .child(info_card("Resolved URL", &resolved_url))
                        .when_some(last_resp.as_ref(), |this, resp| {
                            this.child(info_card("Protocol & TLS", &resp.tls_info))
                                .child(
                                    v_flex()
                                        .p_2p5()
                                        .gap_1()
                                        .bg(solid(BG_ELEVATED))
                                        .border_1()
                                        .border_color(solid(BORDER_SUBTLE))
                                        .rounded(px(RADIUS_ROW))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_semibold()
                                                .text_color(solid(TEXT_MUTED))
                                                .child("Timing Breakdown"),
                                        )
                                        .child(timing_row("TTFB (First byte)", resp.ttfb_ms))
                                        .child(timing_row("Body Download", resp.download_ms))
                                        .child(timing_row("Total", resp.elapsed_ms)),
                                )
                        })
                        .child(
                            v_flex()
                                .p_2p5()
                                .gap_1()
                                .bg(solid(BG_ELEVATED))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .rounded(px(RADIUS_ROW))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(solid(TEXT_MUTED))
                                        .child("Sent Headers"),
                                )
                                .children({
                                    let hdrs = last_resp
                                        .as_ref()
                                        .map(|r| r.sent_headers.clone())
                                        .unwrap_or(preview_headers);
                                    hdrs.into_iter().map(|(k, v)| {
                                        h_flex()
                                            .gap_1p5()
                                            .text_xs()
                                            .child(
                                                div()
                                                    .font_medium()
                                                    .text_color(solid(TEXT_PRIMARY))
                                                    .child(SharedString::from(format!("{k}:"))),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_color(solid(TEXT_MUTED))
                                                    .child(SharedString::from(v)),
                                            )
                                    })
                                }),
                        ),
                ),
        )
}

fn info_card(title: &str, value: &str) -> impl IntoElement {
    v_flex()
        .p_2p5()
        .gap_1()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(title.to_string())),
        )
        .child(
            div()
                .text_xs()
                .text_color(solid(TEXT_PRIMARY))
                .child(SharedString::from(value.to_string())),
        )
}

fn timing_row(label: &str, ms: u128) -> impl IntoElement {
    h_flex()
        .justify_between()
        .text_xs()
        .child(div().text_color(solid(TEXT_MUTED)).child(SharedString::from(label.to_string())))
        .child(
            div()
                .font_medium()
                .text_color(solid(TEXT_PRIMARY))
                .child(SharedString::from(format!("{ms} ms"))),
        )
}
