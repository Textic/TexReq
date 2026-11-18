use gpui_kit::{
    AnyElement, Context, SharedString, Window,
    assets::IconName as AssetIconName,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        input::{Editor, Input},
        menu::{DropdownMenu as _, PopupMenuItem},
        scroll::ScrollableElement as _,
        v_flex,
    },
    div,
    prelude::*,
    px, relative,
};

use crate::net::snippets::{SnippetLang, build_resolved_headers, build_resolved_url};
use crate::storage::git::{DiffLineKind, GitChangeKind};
use crate::theme::{
    ACCENT_BLUE, BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, COLOR_DELETE,
    COLOR_GET, COLOR_POST, RADIUS_CONTROL, RADIUS_ROW, TEXT_MUTED, TEXT_PRIMARY, solid,
};
use crate::ui::kv_editor::{KvTarget, render_flat_subtab, render_kv_table};
use crate::ui::settings_modal::SettingsCategory;
use crate::ui::workspace::{RightDrawerTab, Workspace};

pub fn render_right_rail_and_drawer(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let active = ws.active_drawer;
    let drawer_width = if active == Some(RightDrawerTab::Git) {
        420.
    } else {
        360.
    };
    let has_git_changes = ws.has_pending_git_changes();

    h_flex()
        .h_full()
        .flex_shrink_0()
        .when_some(active, |this, tab| {
            this.child(
                v_flex()
                    .w(px(drawer_width))
                    .h_full()
                    .bg(solid(BG_SURFACE))
                    .border_l_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .overflow_hidden()
                    .child(match tab {
                        RightDrawerTab::Snippets => render_snippets_drawer(ws, cx).into_any_element(),
                        RightDrawerTab::Variables => render_variables_drawer(ws, cx).into_any_element(),
                        RightDrawerTab::NetworkInfo => render_network_info_drawer(ws, cx).into_any_element(),
                        RightDrawerTab::Git => render_git_drawer(ws, cx).into_any_element(),
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
                )
                .child(
                    rail_icon_button(
                        "rail-git",
                        Icon::new(AssetIconName::GitBranch).size_4().into_any_element(),
                        active == Some(RightDrawerTab::Git),
                        has_git_changes,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_drawer_no_window(RightDrawerTab::Git, cx);
                    })),
                ),
        )
}

fn rail_button_shell(
    id: &'static str,
    active: bool,
    content: impl IntoElement,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .relative()
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
        .child(content)
}

fn rail_button(
    id: &'static str,
    symbol: &'static str,
    active: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    rail_button_shell(id, active, symbol)
}

fn rail_icon_button(
    id: &'static str,
    icon: AnyElement,
    active: bool,
    show_dot: bool,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    rail_button_shell(id, active, icon).when(show_dot, |el| {
        el.child(
            div()
                .absolute()
                .top(px(4.))
                .right(px(4.))
                .w(px(6.))
                .h(px(6.))
                .rounded_full()
                .bg(solid(COLOR_GET)),
        )
    })
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

fn change_kind_color(kind: GitChangeKind) -> u32 {
    match kind {
        GitChangeKind::Added | GitChangeKind::Untracked => COLOR_GET,
        GitChangeKind::Modified | GitChangeKind::Renamed => COLOR_POST,
        GitChangeKind::Deleted => COLOR_DELETE,
    }
}

fn diff_line_colors(kind: DiffLineKind) -> (Option<u32>, u32) {
    match kind {
        DiffLineKind::Added => (Some(0x14261c), COLOR_GET),
        DiffLineKind::Removed => (Some(0x2a161b), COLOR_DELETE),
        DiffLineKind::Hunk => (Some(0x182438), ACCENT_BLUE),
        DiffLineKind::Meta => (None, TEXT_MUTED),
        DiffLineKind::Context => (None, TEXT_PRIMARY),
    }
}

fn render_git_drawer(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let selected_linked = ws.active_git_linked_folder();
    let target_name = selected_linked
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "Personal Workspace".to_string());
    let target_path = ws.active_git_target_dir().to_string_lossy().to_string();
    let details = &ws.git_details;
    let git = &details.status;

    v_flex()
        .size_full()
        .p_3()
        .gap_2p5()
        .child(render_git_drawer_header(git.is_repo, &git.branch, cx))
        .child(render_git_target_selector(ws, &target_name, &target_path, cx))
        .when(!git.git_available, |el| {
            el.child(muted_notice_card(
                "Git CLI was not found in PATH. Install Git to track changes.",
            ))
        })
        .when(git.git_available && !git.is_repo, |el| {
            el.child(render_git_uninitialized_card(
                &target_name,
                selected_linked.is_some(),
                cx,
            ))
            .when_some(ws.git_notice.clone(), |col, msg| {
                col.child(muted_notice_card(&msg))
            })
        })
        .when(git.git_available && git.is_repo, |el| {
            el.child(render_git_meta_card(ws, details, cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .child(
                            v_flex()
                                .w_full()
                                .gap_2p5()
                                .when(!details.request_changes.is_empty(), |col| {
                                    col.child(render_request_changes_card(&details.request_changes))
                                })
                                .child(render_changed_files_card(ws, details, cx))
                                .when(!ws.git_diff_lines.is_empty(), |col| {
                                    col.child(render_diff_viewer_card(ws))
                                }),
                        ),
                )
                .child(render_git_footer(ws, cx))
        })
}

fn render_git_drawer_header(
    is_repo: bool,
    branch: &str,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            h_flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .child(
                    Icon::new(AssetIconName::GitBranch)
                        .size_3p5()
                        .text_color(solid(if is_repo { COLOR_GET } else { TEXT_MUTED })),
                )
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .child("Git"),
                )
                .when(is_repo && !branch.is_empty(), |row| {
                    row.child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(3.))
                            .bg(solid(BG_ELEVATED))
                            .border_1()
                            .border_color(solid(BORDER_SUBTLE))
                            .text_xs()
                            .text_color(solid(COLOR_GET))
                            .child(SharedString::from(branch.to_string())),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_1()
                .flex_shrink_0()
                .child(
                    Button::new("git-drawer-link-folder")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::FolderOpen)
                        .tooltip("Link collection folder from disk")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.link_collection_folder_dialog(window, cx);
                        })),
                )
                .child(
                    Button::new("git-drawer-refresh")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip("Refresh Git status and diff")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.refresh_git_status(cx);
                        })),
                )
                .child(
                    Button::new("git-drawer-settings")
                        .with_size(Size::XSmall)
                        .ghost()
                        .icon(IconName::Settings)
                        .tooltip("Open Workspace & Git settings")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_settings(SettingsCategory::WorkspaceGit, window, cx);
                        })),
                ),
        )
}

fn render_git_target_selector(
    ws: &Workspace,
    target_name: &str,
    target_path: &str,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let view = cx.entity().clone();
    let active_target = ws.git_target_folder_id.clone();
    let personal_status = crate::storage::git::inspect_repo(ws.store.workspace_dir());
    let linked_items: Vec<(String, String, Option<crate::storage::git::GitStatusInfo>)> = ws
        .collections
        .iter()
        .filter(|f| f.disk_path.is_some())
        .map(|f| {
            (
                f.id.clone(),
                f.name.clone(),
                ws.folder_git_statuses.get(&f.id).cloned(),
            )
        })
        .collect();

    v_flex()
        .w_full()
        .gap_1()
        .p_2()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Repository"),
                )
                .child(
                    Button::new("git-target-dropdown")
                        .with_size(Size::XSmall)
                        .secondary()
                        .dropdown_caret(true)
                        .max_w(px(250.))
                        .label(SharedString::from(target_name.to_string()))
                        .dropdown_menu(move |mut menu, _, _| {
                            let personal_label = if personal_status.is_repo {
                                if personal_status.changed_files > 0 {
                                    format!(
                                        "Personal Workspace ({} *{})",
                                        personal_status.branch, personal_status.changed_files
                                    )
                                } else {
                                    format!("Personal Workspace ({})", personal_status.branch)
                                }
                            } else {
                                "Personal Workspace (No Git)".to_string()
                            };
                            let v_personal = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(personal_label)
                                    .checked(active_target.is_none())
                                    .on_click(move |_, _, app| {
                                        v_personal.update(app, |this, cx| {
                                            this.select_git_target(None, cx);
                                        });
                                    }),
                            );
                            if !linked_items.is_empty() {
                                menu = menu.separator();
                            }
                            for (fid, fname, status_opt) in &linked_items {
                                let label = match status_opt {
                                    Some(info) if info.is_repo => {
                                        if info.changed_files > 0 {
                                            format!("{fname} ({} *{})", info.branch, info.changed_files)
                                        } else {
                                            format!("{fname} ({})", info.branch)
                                        }
                                    }
                                    _ => format!("{fname} (No Git)"),
                                };
                                let v_linked = view.clone();
                                let fid_clone = fid.clone();
                                let selected = active_target.as_deref() == Some(fid.as_str());
                                menu = menu.item(
                                    PopupMenuItem::new(label)
                                        .checked(selected)
                                        .on_click(move |_, _, app| {
                                            v_linked.update(app, |this, cx| {
                                                this.select_git_target(Some(fid_clone.clone()), cx);
                                            });
                                        }),
                                );
                            }
                            menu
                        }),
                ),
        )
        .child(
            div()
                .w_full()
                .truncate()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(target_path.to_string())),
        )
}

fn render_git_uninitialized_card(
    target_name: &str,
    is_linked: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let hint = if is_linked {
        "Initialize Git in this linked folder to track collection.json changes, view diffs, and commit."
    } else {
        "Personal Workspace tracks collections.json & environments.toml. You can also click the Git folder button on any collection or link a folder on disk."
    };

    v_flex()
        .w_full()
        .p_3()
        .gap_2p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_PRIMARY))
                .child(SharedString::from(format!("{target_name} is not a Git repository"))),
        )
        .child(div().text_xs().text_color(solid(TEXT_MUTED)).child(hint))
        .child(
            h_flex()
                .gap_1p5()
                .flex_wrap()
                .child(
                    Button::new("git-drawer-init")
                        .with_size(Size::XSmall)
                        .primary()
                        .icon(AssetIconName::GitBranch)
                        .label("Initialize Git Repo")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.git_init_workspace(cx);
                        })),
                )
                .when(!is_linked, |row| {
                    row.child(
                        Button::new("git-drawer-link-instead")
                            .with_size(Size::XSmall)
                            .secondary()
                            .icon(IconName::FolderOpen)
                            .label("Link Folder...")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.link_collection_folder_dialog(window, cx);
                            })),
                    )
                }),
        )
}

fn render_git_meta_card(
    ws: &Workspace,
    details: &crate::storage::git::GitRepoDetails,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let remote_text = details
        .status
        .remote_url
        .as_deref()
        .unwrap_or("No remote configured");
    let show_commits = ws.git_show_recent_commits;

    v_flex()
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
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child(SharedString::from(format!("Remote: {remote_text}"))),
                )
                .when(!details.recent_commits.is_empty(), |row| {
                    row.child(
                        Button::new("git-toggle-commits")
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(AssetIconName::GitCommitVertical)
                            .label(SharedString::from(format!(
                                "History ({})",
                                details.recent_commits.len()
                            )))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.git_show_recent_commits = !this.git_show_recent_commits;
                                cx.notify();
                            })),
                    )
                }),
        )
        .when(!show_commits, |col| {
            col.when_some(details.status.last_commit.clone(), |col, msg| {
                col.child(
                    div()
                        .w_full()
                        .truncate()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child(SharedString::from(format!("Last: {msg}"))),
                )
            })
        })
        .when(show_commits && !details.recent_commits.is_empty(), |col| {
            col.child(
                v_flex()
                    .w_full()
                    .pt_1()
                    .gap_1()
                    .border_t_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .children(details.recent_commits.iter().map(|c| {
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_family("Consolas")
                                    .text_color(solid(ACCENT_BLUE))
                                    .child(SharedString::from(c.hash.clone())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(solid(TEXT_PRIMARY))
                                    .child(SharedString::from(c.summary.clone())),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(solid(TEXT_MUTED))
                                    .child(SharedString::from(c.relative_time.clone())),
                            )
                    })),
            )
        })
}

fn render_request_changes_card(items: &[crate::storage::git::RequestDiffSummaryItem]) -> impl IntoElement {
    v_flex()
        .w_full()
        .p_2p5()
        .gap_1p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(format!(
                    "Endpoint Changes ({})",
                    items.len()
                ))),
        )
        .children(items.iter().map(|item| {
            let color = change_kind_color(item.kind);
            let sign = match item.kind {
                GitChangeKind::Added | GitChangeKind::Untracked => "+",
                GitChangeKind::Modified | GitChangeKind::Renamed => "~",
                GitChangeKind::Deleted => "-",
            };
            v_flex()
                .w_full()
                .gap_0p5()
                .px_2()
                .py_1()
                .bg(solid(BG_SURFACE))
                .rounded(px(RADIUS_CONTROL))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_1p5()
                        .text_xs()
                        .child(
                            div()
                                .font_bold()
                                .text_color(solid(color))
                                .child(sign),
                        )
                        .child(
                            div()
                                .font_semibold()
                                .text_color(solid(color))
                                .child(SharedString::from(item.method_label.clone())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_medium()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(item.name.clone())),
                        ),
                )
                .when(!item.detail.is_empty(), |row| {
                    row.child(
                        div()
                            .w_full()
                            .truncate()
                            .pl_3()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child(SharedString::from(item.detail.clone())),
                    )
                })
        }))
}

fn render_changed_files_card(
    ws: &Workspace,
    details: &crate::storage::git::GitRepoDetails,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let selected_path = ws.git_selected_change.as_deref();

    v_flex()
        .w_full()
        .p_2p5()
        .gap_1p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            div()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(format!(
                    "Changed Files ({})",
                    details.changes.len()
                ))),
        )
        .when(details.changes.is_empty(), |col| {
            col.child(
                div()
                    .py_1()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("Working tree clean — no uncommitted changes."),
            )
        })
        .children(details.changes.iter().enumerate().map(|(ix, file)| {
            let is_selected = selected_path == Some(file.repo_path.as_str());
            let badge_color = change_kind_color(file.kind);
            let repo_path_select = file.repo_path.clone();
            let file_discard = file.clone();
            let can_discard = details.has_head || file.kind == GitChangeKind::Untracked;
            let discard_tip = if file.kind == GitChangeKind::Untracked {
                "Delete untracked file"
            } else {
                "Discard file changes"
            };

            h_flex()
                .id(("git-change-file", ix))
                .w_full()
                .px_2()
                .py_1()
                .gap_1p5()
                .items_center()
                .rounded(px(RADIUS_CONTROL))
                .cursor_pointer()
                .bg(if is_selected {
                    solid(BG_HOVER)
                } else {
                    solid(BG_SURFACE)
                })
                .border_1()
                .border_color(if is_selected {
                    solid(ACCENT_BLUE)
                } else {
                    solid(BORDER_SUBTLE)
                })
                .hover(|s| s.bg(solid(BG_HOVER)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_git_change(repo_path_select.clone(), cx);
                }))
                .child(
                    div()
                        .w(px(22.))
                        .flex_shrink_0()
                        .text_xs()
                        .font_bold()
                        .text_color(solid(badge_color))
                        .child(file.kind.badge()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(solid(TEXT_PRIMARY))
                        .child(SharedString::from(file.display_path.clone())),
                )
                .when(can_discard, |row| {
                    row.child(
                        Button::new(("git-discard-file", ix))
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(AssetIconName::Undo)
                            .tooltip(discard_tip)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.request_discard_git_change(file_discard.clone(), cx);
                            })),
                    )
                })
        }))
}

fn render_diff_viewer_card(ws: &Workspace) -> impl IntoElement {
    let selected_label = ws
        .git_details
        .changes
        .iter()
        .find(|c| Some(c.repo_path.as_str()) == ws.git_selected_change.as_deref())
        .map(|c| c.display_path.as_str())
        .unwrap_or("Diff");

    v_flex()
        .w_full()
        .p_2p5()
        .gap_1p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            div()
                .w_full()
                .truncate()
                .text_xs()
                .font_semibold()
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(format!("Diff · {selected_label}"))),
        )
        .child(
            v_flex()
                .w_full()
                .p_1p5()
                .gap_0p5()
                .bg(solid(BG_BASE))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_CONTROL))
                .children(ws.git_diff_lines.iter().map(|line| {
                    let (bg_opt, fg_hex) = diff_line_colors(line.kind);
                    div()
                        .w_full()
                        .px_1p5()
                        .py_0p5()
                        .rounded(px(2.))
                        .when_some(bg_opt, |el, hex| el.bg(solid(hex)))
                        .font_family("Consolas")
                        .text_xs()
                        .text_color(solid(fg_hex))
                        .child(SharedString::from(line.text.clone()))
                })),
        )
}

fn render_git_footer(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex()
        .w_full()
        .pt_2p5()
        .gap_2()
        .border_t_1()
        .border_color(solid(BORDER_SUBTLE))
        .child(Input::new(&ws.git_commit_msg_input).with_size(Size::XSmall))
        .child(
            h_flex()
                .w_full()
                .gap_1p5()
                .child(
                    Button::new("git-drawer-commit")
                        .with_size(Size::XSmall)
                        .primary()
                        .label("Commit")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.git_commit_workspace(window, cx);
                        })),
                )
                .child(
                    Button::new("git-drawer-pull")
                        .with_size(Size::XSmall)
                        .secondary()
                        .label("Pull")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.git_pull_workspace(window, cx);
                        })),
                )
                .child(
                    Button::new("git-drawer-push")
                        .with_size(Size::XSmall)
                        .secondary()
                        .label("Push")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.git_push_workspace(cx);
                        })),
                ),
        )
        .when_some(ws.git_notice.clone(), |col, msg| {
            col.child(muted_notice_card(&msg))
        })
}

fn muted_notice_card(message: &str) -> impl IntoElement {
    div()
        .w_full()
        .px_2p5()
        .py_1p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_CONTROL))
        .text_xs()
        .text_color(solid(TEXT_MUTED))
        .child(SharedString::from(message.to_string()))
}
