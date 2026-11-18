use gpui_kit::{
    Context, SharedString, Window,
    component::{
        Disableable as _, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        checkbox::Checkbox,
        h_flex,
        input::{Editor, Input},
        scroll::ScrollableElement as _,
        v_flex,
    },
    div, hsla, prelude::*, px, relative,
};

use crate::mcp::installer::{McpClient, McpClientStatus};
use crate::net::updater::{CURRENT_VERSION, REPO_URL, UpdateState};
use crate::theme::{
    ACCENT_BLUE, BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, COLOR_GET,
    COLOR_POST, RADIUS_CONTROL, RADIUS_ROW, TEXT_MUTED, TEXT_PRIMARY, render_app_badge, solid,
};
use crate::ui::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsCategory {
    #[default]
    General,
    Network,
    WorkspaceGit,
    Mcp,
    Info,
}

impl SettingsCategory {
    pub const ALL: [Self; 5] = [
        Self::General,
        Self::Network,
        Self::WorkspaceGit,
        Self::Mcp,
        Self::Info,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Network => "Network & SSL",
            Self::WorkspaceGit => "Workspace & Git",
            Self::Mcp => "MCP Server",
            Self::Info => "Info",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::General => "Saving behavior and response formatting preferences.",
            Self::Network => "TLS verification, HTTP redirects, timeouts, and proxy defaults.",
            Self::WorkspaceGit => "Storage folder location and Git repository synchronization.",
            Self::Mcp => "Connect TexReq tools and collections to AI coding assistants.",
            Self::Info => "Application version details, repository links, and updates.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingConfirm {
    DeleteEnvironment { id: String, name: String },
    ClearHistory,
    DeleteFolder { id: String, name: String, count: usize },
    UnlinkFolder { id: String, name: String, path: String },
}

impl PendingConfirm {
    pub fn title(&self) -> &'static str {
        match self {
            Self::DeleteEnvironment { .. } => "Delete Environment",
            Self::ClearHistory => "Clear History",
            Self::DeleteFolder { .. } => "Delete Folder",
            Self::UnlinkFolder { .. } => "Unlink Linked Folder",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::DeleteEnvironment { name, .. } => format!(
                "Are you sure you want to delete \"{name}\"? All variables stored in this environment will be permanently removed."
            ),
            Self::ClearHistory => {
                "Are you sure you want to clear all request history? Unsaved requests in your history cannot be recovered.".to_string()
            }
            Self::DeleteFolder { name, count, .. } => format!(
                "Delete \"{name}\" and its {count} saved request(s)? This cannot be undone."
            ),
            Self::UnlinkFolder { name, path, .. } => format!(
                "Remove linked folder \"{name}\" from TexReq? Files on disk at \"{path}\" will not be deleted."
            ),
        }
    }

    pub fn confirm_label(&self) -> &'static str {
        match self {
            Self::DeleteEnvironment { .. } | Self::DeleteFolder { .. } => "Delete",
            Self::ClearHistory => "Clear History",
            Self::UnlinkFolder { .. } => "Unlink",
        }
    }
}

fn modal_backdrop(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .bg(hsla(0., 0., 0., 0.62))
        .occlude()
}

pub fn render_confirm_modal(
    action: &PendingConfirm,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    div()
        .id("confirm-dialog-root")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            modal_backdrop("confirm-dialog-backdrop").on_click(cx.listener(|this, _, _, cx| {
                this.dismiss_confirm(cx);
            })),
        )
        .child(
            v_flex()
                .id("confirm-dialog-card")
                .relative()
                .occlude()
                .w(px(440.))
                .max_w(relative(0.90))
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    h_flex()
                        .w_full()
                        .h(px(42.))
                        .px_4()
                        .items_center()
                        .justify_between()
                        .bg(solid(BG_BASE))
                        .border_b_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(action.title())),
                        )
                        .child(
                            Button::new("confirm-dialog-close")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.dismiss_confirm(cx);
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .p_4()
                        .gap_4()
                        .child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(action.message())),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("confirm-dialog-cancel")
                                        .with_size(Size::Small)
                                        .secondary()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.dismiss_confirm(cx);
                                        })),
                                )
                                .child(
                                    Button::new("confirm-dialog-accept")
                                        .with_size(Size::Small)
                                        .danger()
                                        .icon(IconName::Delete)
                                        .label(SharedString::from(action.confirm_label()))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.accept_confirm(window, cx);
                                        })),
                                ),
                        ),
                ),
        )
}

pub fn render_settings_modal(
    ws: &Workspace,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let active_cat = ws.settings_category;
    let update_available = matches!(ws.update_state, UpdateState::Available(_));

    div()
        .id("settings-modal-root")
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            // Dimmed backdrop layer; clicking anywhere outside the centered dialog closes Settings.
            modal_backdrop("settings-modal-backdrop").on_click(cx.listener(|this, _, _, cx| {
                this.close_settings(cx);
            })),
        )
        .child(
            v_flex()
                .id("settings-modal-card")
                .relative()
                .occlude()
                .w(px(820.))
                .h(px(570.))
                .max_w(relative(0.92))
                .max_h(relative(0.90))
                .bg(solid(BG_SURFACE))
                .border_1()
                .border_color(solid(BORDER))
                .rounded(px(RADIUS_ROW))
                .overflow_hidden()
                .child(
                    h_flex()
                        .w_full()
                        .h(px(44.))
                        .px_4()
                        .flex_shrink_0()
                        .items_center()
                        .justify_between()
                        .bg(solid(BG_BASE))
                        .border_b_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_semibold()
                                        .text_color(solid(TEXT_PRIMARY))
                                        .child("Settings"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(solid(TEXT_MUTED))
                                        .child(SharedString::from(active_cat.label())),
                                ),
                        )
                        .child(
                            Button::new("settings-modal-close")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::Close)
                                .tooltip("Close Settings (Esc)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close_settings(cx);
                                })),
                        ),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(render_category_rail(active_cat, update_available, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .h_full()
                                .overflow_y_scrollbar()
                                .child(
                                    v_flex()
                                        .w_full()
                                        .p_4()
                                        .gap_3()
                                        .child(
                                            v_flex()
                                                .gap_0p5()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_semibold()
                                                        .text_color(solid(TEXT_PRIMARY))
                                                        .child(SharedString::from(
                                                            active_cat.label(),
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(solid(TEXT_MUTED))
                                                        .child(SharedString::from(
                                                            active_cat.subtitle(),
                                                        )),
                                                ),
                                        )
                                        .child(match active_cat {
                                            SettingsCategory::General => {
                                                render_general_pane(ws, cx).into_any_element()
                                            }
                                            SettingsCategory::Network => {
                                                render_network_pane(ws, cx).into_any_element()
                                            }
                                            SettingsCategory::WorkspaceGit => {
                                                render_workspace_git_pane(ws, cx).into_any_element()
                                            }
                                            SettingsCategory::Mcp => {
                                                render_mcp_pane(ws, cx).into_any_element()
                                            }
                                            SettingsCategory::Info => {
                                                render_info_pane(ws, cx).into_any_element()
                                            }
                                        }),
                                ),
                        ),
                ),
        )
}

fn render_category_rail(
    active: SettingsCategory,
    update_available: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    v_flex()
        .w(px(196.))
        .h_full()
        .flex_shrink_0()
        .p_2p5()
        .gap_1()
        .bg(solid(BG_BASE))
        .border_r_1()
        .border_color(solid(BORDER_SUBTLE))
        .children(SettingsCategory::ALL.into_iter().enumerate().map(|(ix, cat)| {
            let selected = cat == active;
            let show_update_dot = cat == SettingsCategory::Info && update_available;
            div()
                .id(("settings-cat", ix))
                .w_full()
                .h(px(32.))
                .px_2p5()
                .rounded(px(RADIUS_CONTROL))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_between()
                .bg(if selected {
                    solid(BG_ELEVATED)
                } else {
                    gpui_kit::transparent_black()
                })
                .border_1()
                .border_color(if selected {
                    solid(BORDER)
                } else {
                    gpui_kit::transparent_black()
                })
                .hover(|s| s.bg(solid(BG_HOVER)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_settings_category(cat, window, cx);
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .w(px(3.))
                                .h(px(14.))
                                .rounded(px(2.))
                                .bg(if selected {
                                    solid(ACCENT_BLUE)
                                } else {
                                    gpui_kit::transparent_black()
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_medium()
                                .text_color(if selected {
                                    solid(TEXT_PRIMARY)
                                } else {
                                    solid(TEXT_MUTED)
                                })
                                .child(SharedString::from(cat.label())),
                        ),
                )
                .when(show_update_dot, |el| {
                    el.child(
                        div()
                            .w(px(7.))
                            .h(px(7.))
                            .rounded(px(4.))
                            .bg(solid(COLOR_GET)),
                    )
                })
        }))
}

fn section_card(title: &str) -> gpui_kit::Div {
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
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(title.to_string())),
        )
}

fn notice_box(text: String) -> impl IntoElement {
    div()
        .w_full()
        .p_2()
        .bg(solid(BG_SURFACE))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_CONTROL))
        .text_xs()
        .text_color(solid(TEXT_PRIMARY))
        .child(SharedString::from(text))
}

fn render_general_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex().w_full().gap_3().child(
        section_card("Editor & Persistence")
            .child(
                Checkbox::new("set-auto-save")
                    .checked(ws.settings.auto_save)
                    .label("Auto-save saved requests")
                    .on_change(cx.listener(|this, next, _, cx| {
                        this.set_auto_save(*next, cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("Automatically persists request edits when modifying saved items."),
            )
            .child(
                Checkbox::new("set-auto-format")
                    .checked(ws.settings.auto_format_json)
                    .label("Auto-format JSON responses")
                    .on_change(cx.listener(|this, next, _, cx| {
                        this.set_auto_format_json(*next, cx);
                    })),
            ),
    )
}

fn render_network_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex().w_full().gap_3().child(
        section_card("Default Request Behavior")
            .child(
                Checkbox::new("set-ssl-verify")
                    .checked(!ws.settings.default_accept_invalid_certs)
                    .label("Verify SSL/TLS certificates")
                    .on_change(cx.listener(|this, next, _, cx| {
                        this.set_global_ssl_verify(*next, cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(solid(TEXT_MUTED))
                    .child("Disable to trust self-signed or localhost certificates."),
            )
            .child(
                Checkbox::new("set-follow-redirects")
                    .checked(ws.settings.default_follow_redirects)
                    .label("Follow HTTP redirects by default")
                    .on_change(cx.listener(|this, next, _, cx| {
                        this.set_default_follow_redirects(*next, cx);
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child("Default timeout (s):"),
                    )
                    .child(
                        div()
                            .w(px(88.))
                            .child(Input::new(&ws.settings_timeout_input).with_size(Size::XSmall)),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child("Default HTTP/HTTPS proxy:"),
                    )
                    .child(Input::new(&ws.settings_proxy_input).with_size(Size::XSmall)),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("set-net-apply")
                        .with_size(Size::XSmall)
                        .secondary()
                        .label("Apply Defaults")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_default_network_inputs(window, cx);
                        })),
                ),
            )
            .when_some(ws.network_notice.clone(), |el, notice| {
                el.child(notice_box(notice))
            }),
    )
}

fn render_workspace_git_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let selected_linked = ws.active_git_linked_folder();
    let target_title = selected_linked
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "Personal Workspace".to_string());
    let target_path = ws.active_git_target_dir().to_string_lossy().to_string();
    let git = &ws.git_status;

    let linked_folders: Vec<_> = ws
        .collections
        .iter()
        .filter(|f| f.disk_path.is_some())
        .collect();

    let mut target_items = Vec::with_capacity(1 + linked_folders.len());
    let personal_selected = ws.git_target_folder_id.is_none();
    target_items.push(render_git_target_row(
        "git-target-personal",
        0,
        "Personal Workspace",
        &ws.store.workspace_dir().to_string_lossy(),
        None,
        personal_selected,
        None,
        cx,
    ));
    for (ix, folder) in linked_folders.iter().enumerate() {
        let fid = folder.id.clone();
        let selected = ws.git_target_folder_id.as_deref() == Some(fid.as_str());
        let path_str = folder
            .disk_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let status_info = ws.folder_git_statuses.get(&fid);
        target_items.push(render_git_target_row(
            "git-target-linked",
            ix + 1,
            &folder.name,
            &path_str,
            Some(fid),
            selected,
            status_info,
            cx,
        ));
    }

    v_flex()
        .w_full()
        .gap_3()
        .child(
            v_flex()
                .w_full()
                .p_3()
                .gap_2()
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(solid(TEXT_MUTED))
                                .child("Folders & Repositories"),
                        )
                        .child(
                            Button::new("set-link-folder")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::FolderOpen)
                                .label("Link Folder...")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.link_collection_folder_dialog(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Keep personal requests local without Git, or link disk folders that store their own collection.json and optional Git repo."),
                )
                .children(target_items),
        )
        .child(if let Some(folder) = selected_linked {
            let folder_id_unlink = folder.id.clone();
            section_card("Linked Collection Folder")
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Requests in this folder are stored independently in collection.json inside this path."),
                )
                .child(
                    div()
                        .px_2p5()
                        .py_1p5()
                        .bg(solid(BG_SURFACE))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_CONTROL))
                        .text_xs()
                        .text_color(solid(TEXT_PRIMARY))
                        .child(SharedString::from(target_path)),
                )
                .child(
                    h_flex().justify_end().child(
                        Button::new("set-unlink-folder")
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(IconName::Delete)
                            .label("Unlink Folder")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.request_remove_folder(&folder_id_unlink, cx);
                            })),
                    ),
                )
                .into_any_element()
        } else {
            section_card("Personal Workspace Directory")
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Collections and environments are stored here by default."),
                )
                .child(
                    div()
                        .px_2p5()
                        .py_1p5()
                        .bg(solid(BG_SURFACE))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_CONTROL))
                        .text_xs()
                        .text_color(solid(TEXT_PRIMARY))
                        .child(SharedString::from(target_path)),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(
                            Button::new("set-ws-pick")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::FolderOpen)
                                .label("Choose Folder...")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.pick_workspace_folder(window, cx);
                                })),
                        )
                        .child(
                            Button::new("set-ws-reset")
                                .with_size(Size::XSmall)
                                .ghost()
                                .label("Reset Default")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reset_workspace_folder(window, cx);
                                })),
                        ),
                )
                .into_any_element()
        })
        .child(
            section_card(&format!("Git Sync · {target_title}"))
                .when(!git.git_available, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child("Git CLI was not found in PATH."),
                    )
                })
                .when(git.git_available && !git.is_repo, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .child("This workspace is not a Git repository yet."),
                    )
                    .child(
                        h_flex().gap_1p5().child(
                            Button::new("set-git-init")
                                .with_size(Size::XSmall)
                                .primary()
                                .label("Initialize Git Repo")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.git_init_workspace(cx);
                                })),
                        ),
                    )
                })
                .when(git.git_available && git.is_repo, |el| {
                    el.child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .child(div().text_color(solid(TEXT_MUTED)).child("Branch"))
                            .child(
                                div()
                                    .font_semibold()
                                    .text_color(solid(COLOR_GET))
                                    .child(SharedString::from(git.branch.clone())),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .child(div().text_color(solid(TEXT_MUTED)).child("Changed files"))
                            .child(
                                div()
                                    .font_medium()
                                    .text_color(solid(TEXT_PRIMARY))
                                    .child(SharedString::from(git.changed_files.to_string())),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(solid(TEXT_MUTED))
                            .truncate()
                            .child(SharedString::from(format!(
                                "Remote: {}",
                                git.remote_url.as_deref().unwrap_or("none configured")
                            ))),
                    )
                    .when_some(git.last_commit.clone(), |col, msg| {
                        col.child(
                            div()
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .truncate()
                                .child(SharedString::from(format!("Last commit: {msg}"))),
                        )
                    })
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                div().flex_1().min_w_0().child(
                                    Input::new(&ws.git_commit_msg_input).with_size(Size::XSmall),
                                ),
                            )
                            .child(
                                Button::new("set-git-commit")
                                    .with_size(Size::XSmall)
                                    .primary()
                                    .label("Commit")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.git_commit_workspace(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .child(
                                Button::new("set-git-pull")
                                    .with_size(Size::XSmall)
                                    .secondary()
                                    .label("Pull")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.git_pull_workspace(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("set-git-push")
                                    .with_size(Size::XSmall)
                                    .secondary()
                                    .label("Push")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.git_push_workspace(cx);
                                    })),
                            )
                            .child(
                                Button::new("set-git-refresh")
                                    .with_size(Size::XSmall)
                                    .ghost()
                                    .icon(IconName::RefreshCw)
                                    .tooltip("Refresh Git status")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_git_status(cx);
                                    })),
                            ),
                    )
                })
                .when_some(ws.git_notice.clone(), |el, notice| el.child(notice_box(notice))),
        )
}

#[allow(clippy::too_many_arguments)]
fn render_git_target_row(
    id_prefix: &'static str,
    ix: usize,
    name: &str,
    path: &str,
    folder_id: Option<String>,
    selected: bool,
    status: Option<&crate::storage::git::GitStatusInfo>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let badge = match status {
        Some(info) if info.is_repo => {
            if info.changed_files > 0 {
                (
                    format!("{} *{}", info.branch, info.changed_files),
                    COLOR_POST,
                )
            } else {
                (info.branch.clone(), COLOR_GET)
            }
        }
        Some(_) => ("No Git".to_string(), TEXT_MUTED),
        None => ("Default".to_string(), TEXT_MUTED),
    };

    h_flex()
        .id((id_prefix, ix))
        .w_full()
        .px_2p5()
        .py_2()
        .gap_2()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .rounded(px(RADIUS_CONTROL))
        .bg(if selected {
            solid(BG_HOVER)
        } else {
            solid(BG_SURFACE)
        })
        .border_1()
        .border_color(if selected {
            solid(ACCENT_BLUE)
        } else {
            solid(BORDER_SUBTLE)
        })
        .hover(|s| s.bg(solid(BG_HOVER)))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.select_git_target(folder_id.clone(), cx);
        }))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_PRIMARY))
                        .truncate()
                        .child(SharedString::from(name.to_string())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .truncate()
                        .child(SharedString::from(path.to_string())),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .px_1p5()
                .py_0p5()
                .rounded(px(3.))
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .text_xs()
                .font_medium()
                .text_color(solid(badge.1))
                .child(SharedString::from(badge.0)),
        )
}

fn render_mcp_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let command_line = format!("{} --mcp", ws.mcp_command_path);

    v_flex()
        .w_full()
        .gap_3()
        .child(
            section_card("Built-in MCP Server (stdio)")
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Exposes TexReq collections, environments, HTTP/WS execution, and code snippet generators to AI coding agents."),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap_1p5()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .px_2p5()
                                .py_1p5()
                                .bg(solid(BG_SURFACE))
                                .border_1()
                                .border_color(solid(BORDER_SUBTLE))
                                .rounded(px(RADIUS_CONTROL))
                                .text_xs()
                                .text_color(solid(TEXT_PRIMARY))
                                .truncate()
                                .child(SharedString::from(command_line)),
                        )
                        .child(
                            Button::new("mcp-copy-cmd")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::Copy)
                                .label("Copy Cmd")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.copy_mcp_command(cx);
                                })),
                        ),
                )
                .child(
                    Checkbox::new("mcp-mask-secrets")
                        .checked(ws.settings.mcp_mask_secrets)
                        .label("Mask secrets (tokens, keys, cookies) in MCP responses")
                        .on_change(cx.listener(|this, next, _, cx| {
                            this.set_mcp_mask_secrets(*next, cx);
                        })),
                )
                .child(
                    Checkbox::new("mcp-read-only")
                        .checked(ws.settings.mcp_read_only)
                        .label("Read-only mode (block saving requests, folders, and variables)")
                        .on_change(cx.listener(|this, next, _, cx| {
                            this.set_mcp_read_only(*next, cx);
                        })),
                ),
        )
        .child(
            v_flex()
                .w_full()
                .p_3()
                .gap_2p5()
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(solid(TEXT_MUTED))
                                .child("One-Click Client Setup"),
                        )
                        .child(
                            Button::new("mcp-refresh-clients")
                                .with_size(Size::XSmall)
                                .ghost()
                                .icon(IconName::RefreshCw)
                                .tooltip("Re-scan client config files")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.refresh_mcp_state(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Automatically registers the \"texreq\" server entry while preserving existing servers."),
                )
                .children({
                    let mut client_rows = Vec::with_capacity(ws.mcp_statuses.len());
                    for (ix, st) in ws.mcp_statuses.iter().enumerate() {
                        client_rows.push(render_mcp_client_row(ix, st, cx));
                    }
                    client_rows
                })
                .when_some(ws.mcp_notice.clone(), |el, notice| {
                    el.child(notice_box(notice))
                }),
        )
        .child(
            v_flex()
                .w_full()
                .p_3()
                .gap_2()
                .bg(solid(BG_ELEVATED))
                .border_1()
                .border_color(solid(BORDER_SUBTLE))
                .rounded(px(RADIUS_ROW))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(solid(TEXT_MUTED))
                                .child("Generic MCP JSON"),
                        )
                        .child(
                            Button::new("mcp-copy-json")
                                .with_size(Size::XSmall)
                                .secondary()
                                .icon(IconName::Copy)
                                .label("Copy JSON")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.copy_mcp_json(cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child("Copy into Cursor, Windsurf, VS Code, Zed, or any custom MCP host config."),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(142.))
                        .bg(solid(BG_BASE))
                        .border_1()
                        .border_color(solid(BORDER_SUBTLE))
                        .rounded(px(RADIUS_CONTROL))
                        .overflow_hidden()
                        .child(
                            Editor::new(&ws.mcp_json_viewer)
                                .readonly(true)
                                .h(relative(1.))
                                .bordered(false),
                        ),
                ),
        )
}

fn render_mcp_client_row(
    ix: usize,
    status: &McpClientStatus,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let client = status.client;
    let path_label = status
        .config_path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "Home/config directory not found".to_string());

    let (badge_text, badge_color) = if status.installed && status.matches_command {
        ("Installed", COLOR_GET)
    } else if status.installed {
        ("Path outdated", COLOR_POST)
    } else {
        ("Not configured", TEXT_MUTED)
    };

    h_flex()
        .w_full()
        .p_2p5()
        .gap_2p5()
        .items_center()
        .justify_between()
        .bg(solid(BG_SURFACE))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_CONTROL))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(solid(TEXT_PRIMARY))
                                .child(SharedString::from(client.display_name())),
                        )
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(px(3.))
                                .bg(solid(BG_ELEVATED))
                                .text_xs()
                                .text_color(solid(TEXT_MUTED))
                                .child(SharedString::from(client.format_label())),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_medium()
                                .text_color(solid(badge_color))
                                .child(SharedString::from(badge_text)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .truncate()
                        .child(SharedString::from(path_label)),
                ),
        )
        .child(render_mcp_client_actions(ix, client, status, cx))
}

fn render_mcp_client_actions(
    ix: usize,
    client: McpClient,
    status: &McpClientStatus,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let actions = h_flex().gap_1p5().flex_shrink_0();
    if status.installed && status.matches_command {
        actions.child(
            Button::new(("mcp-remove", ix))
                .with_size(Size::XSmall)
                .ghost()
                .icon(IconName::Delete)
                .label("Remove")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.uninstall_mcp_client(client, cx);
                })),
        )
    } else if status.installed {
        actions
            .child(
                Button::new(("mcp-update", ix))
                    .with_size(Size::XSmall)
                    .secondary()
                    .label("Update")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.install_mcp_client(client, cx);
                    })),
            )
            .child(
                Button::new(("mcp-remove-stale", ix))
                    .with_size(Size::XSmall)
                    .ghost()
                    .icon(IconName::Delete)
                    .tooltip("Remove entry")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.uninstall_mcp_client(client, cx);
                    })),
            )
    } else {
        actions.child(
            Button::new(("mcp-install", ix))
                .with_size(Size::XSmall)
                .primary()
                .label("Install")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.install_mcp_client(client, cx);
                })),
        )
    }
}

fn render_info_pane(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    v_flex()
        .w_full()
        .gap_3()
        .child(
            section_card("About TexReq").child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_2p5()
                            .items_center()
                            .child(render_app_badge(36.))
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_semibold()
                                                    .text_color(solid(TEXT_PRIMARY))
                                                    .child("TexReq"),
                                            )
                                            .child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded(px(3.))
                                                    .bg(solid(BG_SURFACE))
                                                    .border_1()
                                                    .border_color(solid(BORDER_SUBTLE))
                                                    .text_xs()
                                                    .text_color(solid(TEXT_MUTED))
                                                    .child(SharedString::from(format!(
                                                        "v{CURRENT_VERSION}"
                                                    ))),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(solid(TEXT_MUTED))
                                            .child("Minimalist local-first GPU API client & MCP server."),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("info-open-github")
                            .with_size(Size::XSmall)
                            .secondary()
                            .label("GitHub")
                            .tooltip(REPO_URL)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_github_repo(cx);
                            })),
                    ),
            ),
        )
        .child(render_updates_card(ws, cx))
}

fn render_updates_card(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let state = &ws.update_state;
    let is_checking = matches!(state, UpdateState::Checking);

    v_flex()
        .w_full()
        .p_3()
        .gap_2p5()
        .bg(solid(BG_ELEVATED))
        .border_1()
        .border_color(solid(BORDER_SUBTLE))
        .rounded(px(RADIUS_ROW))
        .child(
            h_flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(TEXT_MUTED))
                        .child("Updates"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(solid(TEXT_MUTED))
                        .child(SharedString::from(format!("Current: v{CURRENT_VERSION}"))),
                ),
        )
        .child(match state {
            UpdateState::Idle => div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Check GitHub Releases for new versions.")
                .into_any_element(),
            UpdateState::Checking => div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child("Checking for updates...")
                .into_any_element(),
            UpdateState::UpToDate => div()
                .text_xs()
                .text_color(solid(COLOR_GET))
                .child("You are on the latest version.")
                .into_any_element(),
            UpdateState::Available(info) => v_flex()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(solid(COLOR_GET))
                        .child(SharedString::from(if info.published_at.is_empty() {
                            format!("Version {} is available", info.tag_name)
                        } else {
                            format!("Version {} available ({})", info.tag_name, info.published_at)
                        })),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .child(
                            Button::new("set-update-download")
                                .with_size(Size::XSmall)
                                .primary()
                                .icon(IconName::ArrowDown)
                                .label(SharedString::from(format!("Download {}", info.tag_name)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_update_download(cx);
                                })),
                        )
                        .child(
                            Button::new("set-update-notes")
                                .with_size(Size::XSmall)
                                .ghost()
                                .label("Release Notes")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_release_notes(cx);
                                })),
                        ),
                )
                .into_any_element(),
            UpdateState::Failed(err) => div()
                .text_xs()
                .text_color(solid(TEXT_MUTED))
                .child(SharedString::from(err.clone()))
                .into_any_element(),
        })
        .child(
            h_flex().justify_end().child(
                Button::new("set-check-updates")
                    .with_size(Size::XSmall)
                    .secondary()
                    .disabled(is_checking)
                    .icon(IconName::RefreshCw)
                    .label(if is_checking {
                        "Checking..."
                    } else {
                        "Check for Updates"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.check_for_updates(cx);
                    })),
            ),
        )
}
