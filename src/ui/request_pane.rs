use gpui_kit::{
    Context, SharedString, Window,
    component::{
        Disableable as _, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonRounded, ButtonVariants as _},
        checkbox::Checkbox,
        h_flex,
        input::{Editor, Input},
        menu::{DropdownMenu as _, PopupMenuItem},
        resizable::{h_resizable, resizable_panel},
        scroll::ScrollableElement as _,
        v_flex,
    },
    div,
    prelude::*,
    px, relative,
};

use crate::models::{HttpMethod, RequestKind, StreamStatus};
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, RADIUS_CONTROL, RADIUS_ROW,
    TEXT_MUTED, TEXT_PRIMARY, badge_info, crisp_resize_handle_appearance, full_hit_input, solid,
};
use crate::ui::kv_editor::{KvTarget, render_capture_table, render_flat_subtab, render_kv_table};
use crate::ui::tab_session::{AuthKindUi, BodyKindUi, RequestSubTab};
use crate::ui::workspace::Workspace;

pub fn render_request_pane(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let Some(tab) = ws.active_tab() else {
        return div().size_full().bg(solid(BG_BASE)).into_any_element();
    };

    let (method_lbl, method_hex) = badge_info(tab.kind, &crate::models::BodyKind::None);
    let view = cx.entity().clone();
    let current_kind = tab.kind;
    let is_realtime = tab.kind.is_realtime();
    let is_ws = tab.kind == RequestKind::WebSocket;
    let is_loading = tab.is_loading;
    let stream_status = tab.stream_status;

    let params_cnt = tab.params.len();
    let headers_cnt = tab.headers.len();
    let captures_cnt = tab.captures.len();

    v_flex()
        .size_full()
        .overflow_hidden()
        .bg(solid(BG_BASE))
        .p_3()
        .gap_2p5()
        .child(
            // Lightweight borderless title + Save button
            h_flex()
                .w_full()
                .h(px(28.))
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    full_hit_input(
                        "req-name-input-slot",
                        &tab.name_input,
                        Input::new(&tab.name_input)
                            .with_size(Size::XSmall)
                            .appearance(false),
                    )
                    .flex_1()
                    .min_w_0()
                    .px_1()
                    .rounded(px(RADIUS_CONTROL))
                    .hover(|s| s.bg(solid(BG_SURFACE))),
                )
                .child(
                    Button::new("req-save-btn")
                        .with_size(Size::XSmall)
                        .secondary()
                        .icon(IconName::Inbox)
                        .label("Save")
                        .tooltip("Save to collection")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_active_tab(cx);
                        })),
                ),
        )
        .child(
            // Clean unified Request URL bar (subtle 6px rectangle, no circles)
            h_flex()
                .w_full()
                .h(px(40.))
                .items_center()
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    // Method / Protocol Dropdown
                    div()
                        .h_full()
                        .flex_shrink_0()
                        .child(
                            Button::new("req-method-dropdown")
                                .with_size(Size::Small)
                                .ghost()
                                .h(px(38.))
                                .px_3()
                                .rounded(ButtonRounded::None)
                                .rounded_tl(px(RADIUS_ROW - 1.))
                                .rounded_bl(px(RADIUS_ROW - 1.))
                                .dropdown_caret(true)
                                .child(
                                    div()
                                        .w(px(62.))
                                        .text_xs()
                                        .font_bold()
                                        .text_color(solid(method_hex))
                                        .child(method_lbl),
                                )
                                .dropdown_menu(move |mut menu, _, _| {
                                    for &m in HttpMethod::ALL {
                                        let v = view.clone();
                                        let target_kind = RequestKind::Http(m);
                                        menu = menu.item(
                                            PopupMenuItem::new(m.as_str())
                                                .checked(current_kind == target_kind)
                                                .on_click(move |_, window, app| {
                                                    v.update(app, |this, cx| {
                                                        this.set_active_request_kind(
                                                            target_kind,
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                }),
                                        );
                                    }
                                    menu = menu.separator();
                                    let v_ws = view.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new("WebSocket (WS)")
                                            .checked(current_kind == RequestKind::WebSocket)
                                            .on_click(move |_, window, app| {
                                                v_ws.update(app, |this, cx| {
                                                    this.set_active_request_kind(
                                                        RequestKind::WebSocket,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    );
                                    let v_sse = view.clone();
                                    menu.item(
                                        PopupMenuItem::new("SSE Stream (EventSource)")
                                            .checked(current_kind == RequestKind::Sse)
                                            .on_click(move |_, window, app| {
                                                v_sse.update(app, |this, cx| {
                                                    this.set_active_request_kind(
                                                        RequestKind::Sse,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                }),
                        ),
                )
                .child(div().w(px(1.)).h(px(20.)).flex_shrink_0().bg(solid(BORDER)))
                .child(
                    full_hit_input(
                        "req-url-input-slot",
                        &tab.url_input,
                        Input::new(&tab.url_input)
                            .with_size(Size::Small)
                            .appearance(false),
                    )
                    .flex_1()
                    .min_w_0(),
                )
                .child(url_bar_action_button(if is_realtime {
                    match stream_status {
                        StreamStatus::Disconnected => Button::new("req-stream-connect")
                            .with_size(Size::Small)
                            .info()
                            .icon(IconName::Play)
                            .label("Connect")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.trigger_primary_action(window, cx);
                            })),
                        StreamStatus::Connecting => Button::new("req-stream-connecting")
                            .with_size(Size::Small)
                            .warning()
                            .icon(IconName::Pause)
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.disconnect_active_stream(window, cx);
                            })),
                        StreamStatus::Connected => Button::new("req-stream-disconnect")
                            .with_size(Size::Small)
                            .danger()
                            .icon(IconName::Square)
                            .label("Disconnect")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.disconnect_active_stream(window, cx);
                            })),
                    }
                } else if is_loading {
                    Button::new("req-cancel-btn")
                        .with_size(Size::Small)
                        .danger()
                        .icon(IconName::Square)
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cancel_active_http(cx);
                        }))
                } else {
                    Button::new("req-send-btn")
                        .with_size(Size::Small)
                        .primary()
                        .icon(IconName::Play)
                        .label("Send")
                        .tooltip("Send request")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.trigger_primary_action(window, cx);
                        }))
                })),
        )
        .child(
            // Sub-tabs bar
            h_flex()
                .w_full()
                .gap_1p5()
                .pb_1p5()
                .border_b_1()
                .border_color(solid(BORDER_SUBTLE))
                .when(is_ws, |this| {
                    this.child(
                        render_flat_subtab(
                            "rtab-ws-msg",
                            "WS Message",
                            tab.req_subtab == RequestSubTab::Body,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_request_subtab(RequestSubTab::Body, cx);
                        })),
                    )
                })
                .when(!is_realtime, |this| {
                    this.child(
                        render_flat_subtab(
                            "rtab-body",
                            &format!("Body · {}", tab.body_kind.label()),
                            tab.req_subtab == RequestSubTab::Body,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_request_subtab(RequestSubTab::Body, cx);
                        })),
                    )
                })
                .child(
                    render_flat_subtab(
                        "rtab-params",
                        &format!("Params ({params_cnt})"),
                        tab.req_subtab == RequestSubTab::Params,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_request_subtab(RequestSubTab::Params, cx);
                    })),
                )
                .child(
                    render_flat_subtab(
                        "rtab-auth",
                        &format!("Auth · {}", tab.auth_kind.label()),
                        tab.req_subtab == RequestSubTab::Auth,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_request_subtab(RequestSubTab::Auth, cx);
                    })),
                )
                .child(
                    render_flat_subtab(
                        "rtab-headers",
                        &format!("Headers ({headers_cnt})"),
                        tab.req_subtab == RequestSubTab::Headers,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_request_subtab(RequestSubTab::Headers, cx);
                    })),
                )
                .when(!is_realtime, |this| {
                    this.child(
                        render_flat_subtab(
                            "rtab-captures",
                            &format!("Captures ({captures_cnt})"),
                            tab.req_subtab == RequestSubTab::Captures,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.set_request_subtab(RequestSubTab::Captures, cx);
                        })),
                    )
                })
                .child(
                    render_flat_subtab(
                        "rtab-network",
                        "Network",
                        tab.req_subtab == RequestSubTab::Network,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.set_request_subtab(RequestSubTab::Network, cx);
                    })),
                ),
        )
        .child(
            // Sub-tab Content Area
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(match tab.req_subtab {
                    RequestSubTab::Params => div()
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(render_kv_table(
                            &tab.params,
                            KvTarget::Params,
                            false,
                            "Query Param",
                            "Value",
                            cx,
                        ))
                        .into_any_element(),
                    RequestSubTab::Headers => div()
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(render_kv_table(
                            &tab.headers,
                            KvTarget::Headers,
                            false,
                            "Header",
                            "Value",
                            cx,
                        ))
                        .into_any_element(),
                    RequestSubTab::Auth => render_auth_panel(ws, cx).into_any_element(),
                    RequestSubTab::Body => {
                        if is_ws {
                            render_ws_composer_panel(ws, cx).into_any_element()
                        } else {
                            render_body_panel(ws, cx).into_any_element()
                        }
                    }
                    RequestSubTab::Captures => div()
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(render_capture_table(&tab.captures, cx))
                        .into_any_element(),
                    RequestSubTab::Network => render_network_panel(ws, cx).into_any_element(),
                }),
        )
        .into_any_element()
}

fn render_auth_panel(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let tab = ws.active_tab().unwrap();
    let kind = tab.auth_kind;
    let in_header = tab.api_key_in_header;

    v_flex()
        .size_full()
        .gap_3()
        .overflow_y_scrollbar()
        .child(
            h_flex()
                .gap_1p5()
                .children(
                    [
                        AuthKindUi::None,
                        AuthKindUi::Bearer,
                        AuthKindUi::Basic,
                        AuthKindUi::ApiKey,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(ix, ak)| {
                        render_flat_subtab(("auth-kind", ix), ak.label(), kind == ak).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.set_auth_kind(ak, cx);
                            }),
                        )
                    }),
                ),
        )
        .child(match kind {
            AuthKindUi::None => div()
                .p_3()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("This request does not send automatic authentication credentials.")
                .into_any_element(),
            AuthKindUi::Bearer => v_flex()
                .p_3()
                .gap_2()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Bearer Token (sent as header Authorization: Bearer <token>)"),
                )
                .child(Input::new(&tab.auth_bearer_input).with_size(Size::Small))
                .into_any_element(),
            AuthKindUi::Basic => v_flex()
                .p_3()
                .gap_2()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Basic Auth (Base64 encoded as Authorization: Basic ...)"),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&tab.auth_user_input).with_size(Size::Small)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&tab.auth_pass_input).with_size(Size::Small)),
                        ),
                )
                .into_any_element(),
            AuthKindUi::ApiKey => v_flex()
                .p_3()
                .gap_2()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div().flex_1().child(
                                Input::new(&tab.auth_apikey_name_input).with_size(Size::Small),
                            ),
                        )
                        .child(
                            div().flex_1().child(
                                Input::new(&tab.auth_apikey_val_input).with_size(Size::Small),
                            ),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            render_flat_subtab("ak-loc-hdr", "Send in Header", in_header)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_apikey_location(true, cx);
                                })),
                        )
                        .child(
                            render_flat_subtab("ak-loc-qry", "Send in Query Param", !in_header)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_apikey_location(false, cx);
                                })),
                        ),
                )
                .into_any_element(),
        })
}

fn render_body_panel(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let tab = ws.active_tab().unwrap();
    let bk = tab.body_kind;

    v_flex()
        .size_full()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_2()
                .child(
                    h_flex()
                        .gap_1()
                        .children(BodyKindUi::ALL.iter().enumerate().map(|(ix, &kind)| {
                            render_flat_subtab(("body-kind", ix), kind.label(), bk == kind)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_body_kind(kind, cx);
                                }))
                        })),
                )
                .when(bk == BodyKindUi::Json, |this| {
                    this.child(
                        Button::new("body-format-json")
                            .with_size(Size::XSmall)
                            .secondary()
                            .label("Format JSON")
                            .tooltip("Format JSON")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.format_active_json(window, cx);
                            })),
                    )
                }),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(match bk {
                    BodyKindUi::None => div()
                        .p_3()
                        .bg(solid(BG_SURFACE))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("This request has no body.")
                        .into_any_element(),
                    BodyKindUi::Json => div()
                        .size_full()
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .overflow_hidden()
                        .child(Editor::new(&tab.body_json_editor).h(relative(1.)).bordered(false))
                        .into_any_element(),
                    BodyKindUi::GraphQl => h_resizable("gql-split")
                        .with_handle_appearance(crisp_resize_handle_appearance())
                        .child(
                            resizable_panel().child(
                                v_flex()
                                    .size_full()
                                    .overflow_hidden()
                                    .gap_1()
                                    .pr_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(solid(TEXT_MUTED))
                                            .child("Query / Mutation"),
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
                                                Editor::new(&tab.gql_query_editor)
                                                    .h(relative(1.))
                                                    .bordered(false),
                                            ),
                                    ),
                            ),
                        )
                        .child(
                            resizable_panel().size(px(280.)).child(
                                v_flex()
                                    .size_full()
                                    .overflow_hidden()
                                    .gap_1()
                                    .pl_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(solid(TEXT_MUTED))
                                            .child("Variables (JSON)"),
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
                                                Editor::new(&tab.gql_vars_editor)
                                                    .h(relative(1.))
                                                    .bordered(false),
                                            ),
                                    ),
                            ),
                        )
                        .into_any_element(),
                    BodyKindUi::FormUrlEncoded => div()
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(render_kv_table(
                            &tab.form_rows,
                            KvTarget::Form,
                            false,
                            "Form Field",
                            "Value",
                            cx,
                        ))
                        .into_any_element(),
                    BodyKindUi::Multipart => div()
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(render_kv_table(
                            &tab.multipart_rows,
                            KvTarget::Multipart,
                            true,
                            "Multipart Field",
                            "Value / File",
                            cx,
                        ))
                        .into_any_element(),
                    BodyKindUi::Raw => v_flex()
                        .size_full()
                        .gap_1p5()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child("Content-Type:"),
                                )
                                .children(
                                    ["text/plain", "application/xml", "text/html", "application/yaml"]
                                        .into_iter()
                                        .enumerate()
                                        .map(|(ix, ct)| {
                                            render_flat_subtab(
                                                ("raw-ct", ix),
                                                ct,
                                                tab.raw_content_type == ct,
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_raw_content_type(ct, window, cx);
                                            }))
                                        }),
                                ),
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
                                    Editor::new(&tab.raw_body_editor)
                                        .h(relative(1.))
                                        .bordered(false),
                                ),
                        )
                        .into_any_element(),
                    BodyKindUi::Binary => h_flex()
                        .p_3()
                        .gap_2()
                        .bg(solid(BG_SURFACE))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&tab.binary_path_input).with_size(Size::Small)),
                        )
                        .child(
                            Button::new("pick-binary-file")
                                .with_size(Size::Small)
                                .secondary()
                                .icon(IconName::FolderOpen)
                                .label("Choose file...")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_binary_file(window, cx);
                                })),
                        )
                        .into_any_element(),
                }),
        )
}

fn render_ws_composer_panel(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let tab = ws.active_tab().unwrap();
    let connected = tab.stream_status == StreamStatus::Connected;

    v_flex()
        .size_full()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_2()
                .child(
                    h_flex()
                        .gap_1()
                        .flex_wrap()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child("Templates:"),
                        )
                        .children(tab.ws_templates.iter().enumerate().map(|(ix, tpl)| {
                            let tpl_val = tpl.clone();
                            let short: String = tpl.chars().take(24).collect();
                            div()
                                .id(("ws-tpl", ix))
                                .px_2()
                                .py_0p5()
                                .bg(solid(BG_ELEVATED))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .rounded(px(RADIUS_CONTROL))
                                .cursor_pointer()
                                .hover(|s| s.bg(solid(BG_HOVER)))
                                .text_xs()
                                .text_color(solid(TEXT_PRIMARY))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.load_ws_template(&tpl_val, window, cx);
                                }))
                                .child(SharedString::from(short))
                        })),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(
                            Button::new("ws-save-tpl")
                                .with_size(Size::XSmall)
                                .secondary()
                                .label("Save template")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.save_current_ws_template(cx);
                                })),
                        )
                        .child(
                            Button::new("ws-send-msg")
                                .with_size(Size::XSmall)
                                .primary()
                                .disabled(!connected)
                                .icon(IconName::Play)
                                .label("Send message")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.send_ws_message(cx);
                                })),
                        ),
                ),
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
                    Editor::new(&tab.ws_composer_editor)
                        .h(relative(1.))
                        .bordered(false),
                ),
        )
}

fn render_network_panel(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let tab = ws.active_tab().unwrap();
    let accept_certs = tab.accept_invalid_certs;
    let follow_redirs = tab.follow_redirects;

    v_flex()
        .size_full()
        .gap_3()
        .overflow_y_scrollbar()
        .child(
            v_flex()
                .p_3()
                .gap_2()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child("HTTP / HTTPS Proxy"),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&tab.proxy_input).with_size(Size::Small)),
                        )
                        .child(
                            Button::new("net-proxy-clear")
                                .with_size(Size::Small)
                                .ghost()
                                .label("Clear")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.set_proxy_preset("", window, cx);
                                })),
                        ),
                ),
        )
        .child(
            v_flex()
                .p_3()
                .gap_2p5()
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child("Timeout (seconds):"),
                        )
                        .child(
                            div()
                                .w(px(90.))
                                .child(Input::new(&tab.timeout_input).with_size(Size::XSmall)),
                        ),
                )
                .child(
                    Checkbox::new("net-follow-redirects")
                        .checked(follow_redirs)
                        .label("Follow HTTP redirects automatically")
                        .on_change(cx.listener(|this, next, _, cx| {
                            this.set_follow_redirects(*next, cx);
                        })),
                )
                .child(
                    Checkbox::new("net-accept-invalid-certs")
                        .checked(accept_certs)
                        .label("Allow invalid / self-signed SSL certificates")
                        .on_change(cx.listener(|this, next, _, cx| {
                            this.set_accept_invalid_certs(*next, cx);
                        })),
                ),
        )
}

/// Styles the right-most action button inside the unified URL bar so it sits
/// flush against the top, right, and bottom inner edges of the outer border.
fn url_bar_action_button(btn: Button) -> Button {
    btn.h_full()
        .px_4()
        .flex_shrink_0()
        .rounded(ButtonRounded::None)
        .rounded_tr(px(RADIUS_ROW - 1.))
        .rounded_br(px(RADIUS_ROW - 1.))
}
