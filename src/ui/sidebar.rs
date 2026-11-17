use gpui_kit::{
    Context, SharedString, Window, WindowControlArea,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::Input,
        scroll::ScrollableElement as _,
        v_flex,
    },
    div,
    prelude::*,
    px,
};

use crate::net::updater::UpdateState;
use crate::theme::{
    BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER_SUBTLE, COLOR_DELETE, RADIUS_CONTROL, RADIUS_ROW,
    TEXT_MUTED, TEXT_PRIMARY, badge_info, full_hit_input, render_app_badge, solid, status_color,
};
use crate::ui::workspace::{SidebarMode, Workspace};

pub fn render_sidebar(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let query = ws.search_input.read(cx).value().trim().to_ascii_lowercase();
    let mode = ws.sidebar_mode;
    let settings_active = ws.show_settings_modal;
    let update_available = matches!(ws.update_state, UpdateState::Available(_));
    let active_saved_id = ws
        .active_tab()
        .and_then(|t| t.saved_request_id.clone());

    v_flex()
        .size_full()
        .overflow_hidden()
        .bg(solid(BG_SURFACE))
        .child(
            // Seamless sidebar header with circular app mark (drag handle on brand text)
            h_flex()
                .w_full()
                .h(px(40.))
                .px_3()
                .items_center()
                .justify_between()
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .gap_2()
                        .items_center()
                        .window_control_area(WindowControlArea::Drag)
                        .child(render_app_badge(22.))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(solid(TEXT_PRIMARY))
                                .child("TexReq"),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("sb-new-req")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Plus)
                                .tooltip("New Request")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.new_scratch_tab(window, cx);
                                })),
                        )
                        .child(
                            Button::new("sb-new-folder")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Folder)
                                .tooltip("New Folder")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.create_folder(cx);
                                })),
                        )
                        .child({
                            let btn = Button::new("sb-settings")
                                .with_size(Size::XSmall)
                                .icon(IconName::Settings)
                                .tooltip(if update_available {
                                    "Settings (Update available)"
                                } else {
                                    "Settings"
                                })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_settings_modal(window, cx);
                                }));
                            if settings_active {
                                btn.secondary()
                            } else {
                                btn.ghost()
                            }
                        }),
                ),
        )
        .child(
            // Clean rectangular search & segmented mode switch
            v_flex()
                .px_2p5()
                .pb_2p5()
                .gap_2()
                .child(
                    h_flex()
                        .w_full()
                        .h(px(30.))
                        .px_2p5()
                        .gap_1p5()
                        .items_center()
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .child(
                            Icon::new(IconName::Search)
                                .size_3p5()
                                .text_color(solid(TEXT_MUTED)),
                        )
                        .child(
                            full_hit_input(
                                "sb-search-input-slot",
                                &ws.search_input,
                                Input::new(&ws.search_input)
                                    .with_size(Size::XSmall)
                                    .appearance(false)
                                    .cleanable(true),
                            )
                            .flex_1()
                            .min_w_0(),
                        ),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_1()
                        .p(px(3.))
                        .bg(solid(BG_ELEVATED))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_ROW))
                        .child(
                            div()
                                .id("sb-mode-collections")
                                .flex_1()
                                .py_1()
                                .rounded(px(RADIUS_CONTROL))
                                .cursor_pointer()
                                .text_center()
                                .text_xs()
                                .bg(if mode == SidebarMode::Collections {
                                    solid(BG_HOVER)
                                } else {
                                    gpui_kit::transparent_black()
                                })
                                .text_color(if mode == SidebarMode::Collections {
                                    solid(TEXT_PRIMARY)
                                } else {
                                    solid(TEXT_MUTED)
                                })
                                .hover(|s| s.bg(solid(BG_HOVER)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sidebar_mode = SidebarMode::Collections;
                                    cx.notify();
                                }))
                                .child("Collections"),
                        )
                        .child(
                            div()
                                .id("sb-mode-history")
                                .flex_1()
                                .py_1()
                                .rounded(px(RADIUS_CONTROL))
                                .cursor_pointer()
                                .text_center()
                                .text_xs()
                                .bg(if mode == SidebarMode::History {
                                    solid(BG_HOVER)
                                } else {
                                    gpui_kit::transparent_black()
                                })
                                .text_color(if mode == SidebarMode::History {
                                    solid(TEXT_PRIMARY)
                                } else {
                                    solid(TEXT_MUTED)
                                })
                                .hover(|s| s.bg(solid(BG_HOVER)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sidebar_mode = SidebarMode::History;
                                    cx.notify();
                                }))
                                .child(SharedString::from(format!(
                                    "History ({})",
                                    ws.history.len()
                                ))),
                        ),
                ),
        )
        .child(
            // Scrollable tree or history list
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_y_scrollbar()
                .child(match mode {
                    SidebarMode::Collections => render_collections_list(
                        ws,
                        &query,
                        active_saved_id.as_deref(),
                        cx,
                    )
                    .into_any_element(),
                    SidebarMode::History => render_history_list(ws, &query, cx).into_any_element(),
                }),
        )
}

fn render_collections_list(
    ws: &Workspace,
    query: &str,
    active_saved_id: Option<&str>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    v_flex()
        .w_full()
        .px_2()
        .pb_2()
        .gap_1()
        .when(ws.collections.is_empty(), |this| {
            this.child(
                div()
                    .p_3()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("No folders yet. Create a folder or save a request."),
            )
        })
        .children(ws.collections.iter().enumerate().map(|(f_ix, folder)| {
            let folder_id = folder.id.clone();
            let folder_id_add = folder.id.clone();
            let folder_id_del = folder.id.clone();
            let expanded = folder.expanded || !query.is_empty();

            let matching_requests: Vec<_> = folder
                .requests
                .iter()
                .filter(|r| {
                    query.is_empty()
                        || r.name.to_ascii_lowercase().contains(query)
                        || r.url.to_ascii_lowercase().contains(query)
                })
                .collect();

            if !query.is_empty()
                && matching_requests.is_empty()
                && !folder.name.to_ascii_lowercase().contains(query)
            {
                return div().into_any_element();
            }

            v_flex()
                .w_full()
                .gap_0p5()
                .child(
                    h_flex()
                        .id(("folder-row", f_ix))
                        .w_full()
                        .h(px(30.))
                        .px_2()
                        .gap_1p5()
                        .rounded(px(RADIUS_ROW))
                        .items_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(solid(BG_ELEVATED)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_folder(&folder_id, cx);
                        }))
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size_3p5()
                            .text_color(solid(TEXT_MUTED)),
                        )
                        .child(
                            Icon::new(if expanded {
                                IconName::FolderOpen
                            } else {
                                IconName::FolderClosed
                            })
                            .size_3p5()
                            .text_color(solid(TEXT_MUTED)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .font_medium()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(folder.name.clone())),
                        )
                        .child(
                            Button::new(("folder-add-req", f_ix))
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Plus)
                                .tooltip("New request in this folder")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.create_request_in_folder(&folder_id_add, window, cx);
                                })),
                        )
                        .child(
                            Button::new(("folder-del", f_ix))
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .tooltip("Delete folder")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.delete_folder(&folder_id_del, cx);
                                })),
                        ),
                )
                .when(expanded, |this| {
                    this.child(
                        v_flex()
                            .pl_3()
                            .gap_0p5()
                            .when(matching_requests.is_empty(), |col| {
                                col.child(
                                    div()
                                        .px_2p5()
                                        .py_1()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child("Empty folder"),
                                )
                            })
                            .children(matching_requests.into_iter().enumerate().map(
                                |(r_ix, req)| {
                                    let req_clone = req.clone();
                                    let req_id_del = req.id.clone();
                                    let folder_id_for_del = folder.id.clone();
                                    let (badge_lbl, badge_hex) = badge_info(req.kind, &req.body);
                                    let is_active = active_saved_id == Some(req.id.as_str());

                                    h_flex()
                                        .id((SharedString::from(format!("col-req-{f_ix}")), r_ix))
                                        .w_full()
                                        .h(px(30.))
                                        .px_2()
                                        .gap_1p5()
                                        .rounded(px(RADIUS_ROW))
                                        .items_center()
                                        .cursor_pointer()
                                        .bg(if is_active {
                                            solid(BG_HOVER)
                                        } else {
                                            gpui_kit::transparent_black()
                                        })
                                        .hover(|s| s.bg(solid(BG_ELEVATED)))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_saved_request(&req_clone, window, cx);
                                        }))
                                        .child(
                                            div()
                                                .w(px(46.))
                                                .flex_shrink_0()
                                                .text_xs()
                                                .font_bold()
                                                .text_color(solid(badge_hex))
                                                .child(badge_lbl),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .truncate()
                                                .text_xs()
                                                .text_color(solid(TEXT_PRIMARY))
                                                .child(SharedString::from(req.name.clone())),
                                        )
                                        .child(
                                            Button::new((
                                                SharedString::from(format!("col-req-del-{f_ix}")),
                                                r_ix,
                                            ))
                                            .with_size(Size::XSmall)
                                            .ghost()
                                            .icon(IconName::Close)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.delete_saved_request(
                                                    &folder_id_for_del,
                                                    &req_id_del,
                                                    cx,
                                                );
                                            })),
                                        )
                                },
                            )),
                    )
                })
                .into_any_element()
        }))
}

fn render_history_list(
    ws: &Workspace,
    query: &str,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let entries: Vec<_> = ws
        .history
        .iter()
        .filter(|h| {
            query.is_empty()
                || h.request.name.to_ascii_lowercase().contains(query)
                || h.request.url.to_ascii_lowercase().contains(query)
        })
        .collect();

    v_flex()
        .w_full()
        .px_2()
        .pb_2()
        .gap_1()
        .when(!ws.history.is_empty(), |this| {
            this.child(
                h_flex().w_full().justify_end().pb_1().child(
                    Button::new("hist-clear")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::Delete)
                        .label("Clear")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.clear_history(cx);
                        })),
                ),
            )
        })
        .when(entries.is_empty(), |this| {
            this.child(
                div()
                    .p_3()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("No requests in history yet."),
            )
        })
        .children(entries.into_iter().enumerate().map(|(ix, entry)| {
            let req_clone = entry.request.clone();
            let (badge_lbl, badge_hex) = badge_info(entry.request.kind, &entry.request.body);
            let status_txt = entry
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "ERR".to_string());
            let status_hex = entry.status.map(status_color).unwrap_or(COLOR_DELETE);

            v_flex()
                .id(("hist-item", ix))
                .w_full()
                .px_2p5()
                .py_2()
                .gap_0p5()
                .rounded(px(RADIUS_ROW))
                .cursor_pointer()
                .bg(solid(BG_ELEVATED))
                .hover(|s| s.bg(solid(BG_HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_history_request(&req_clone, window, cx);
                }))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_1p5()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_bold()
                                        .text_color(solid(badge_hex))
                                        .child(badge_lbl),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_semibold()
                                        .text_color(solid(status_hex))
                                        .child(status_txt),
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
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(solid(TEXT_PRIMARY))
                        .child(SharedString::from(if entry.request.url.is_empty() {
                            entry.request.name.clone()
                        } else {
                            entry.request.url.clone()
                        })),
                )
        }))
}
