use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Local;
use futures_channel::mpsc::unbounded;
use futures_util::StreamExt as _;
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyDownEvent,
    PathPromptOptions, SharedString, Window, WindowControlArea,
    component::{
        Icon, IconName, Sizable as _, Size, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        menu::{DropdownMenu as _, PopupMenuItem},
        resizable::{h_resizable, resizable_panel, v_resizable},
        v_flex,
    },
    div,
    prelude::*,
    px,
};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::models::{
    AppSettings, CollectionFolder, Environment, HistoryEntry, KeyValueRow, MAX_ENV_NAME_CHARS,
    RequestItem, RequestKind, StreamStatus, next_environment_name, parse_dotenv, truncate_chars,
};
use crate::net::WorkerEvent;
use crate::net::curl_parser::parse_curl;
use crate::net::http_client::{execute_http, run_sse_stream};
use crate::net::runtime::spawn_tokio;
use crate::net::snippets::{SnippetLang, generate_snippet};
use crate::mcp::installer::{
    McpClient, McpClientStatus, generic_mcp_json, inspect_all_clients, install_client,
    resolve_texreq_executable, uninstall_client,
};
use crate::net::updater::{self, REPO_URL, UpdateState};
use crate::net::ws_client::{WsCommand, run_ws_session};
use crate::storage::{LocalStore, git};
use crate::theme::{
    BG_BASE, BG_ELEVATED, BG_HOVER, BG_SURFACE, BORDER, BORDER_SUBTLE, COLOR_DELETE,
    RADIUS_CONTROL, TEXT_MUTED, TEXT_PRIMARY, badge_info, crisp_resize_handle_appearance,
    render_app_badge, solid,
};
use crate::ui::kv_editor::{KvTarget, UiCaptureRow, UiKvRow};
use crate::ui::request_pane::render_request_pane;
use crate::ui::response_pane::render_response_pane;
use crate::ui::right_drawer::render_right_rail_and_drawer;
use crate::ui::settings_modal::{
    PendingConfirm, SettingsCategory, render_confirm_modal, render_settings_modal,
};
use crate::ui::sidebar::render_sidebar;
use crate::ui::tab_session::{BodyKindUi, RequestSubTab, ResponseSubTab, TabSession, should_autoparse_curl};

gpui_kit::actions!(
    texreq,
    [
        SendRequest,
        NewTab,
        SaveRequest,
        FocusUrl,
        ToggleSidebar,
        FocusSearch,
        FormatJson,
        Quit
    ]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    Collections,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RightDrawerTab {
    Snippets,
    Variables,
    NetworkInfo,
    Git,
}

pub struct Workspace {
    pub focus_handle: FocusHandle,
    pub store: LocalStore,
    pub settings: AppSettings,
    pub git_status: git::GitStatusInfo,
    pub git_notice: Option<String>,
    pub collections: Vec<CollectionFolder>,
    pub history: Vec<HistoryEntry>,
    pub environments: Vec<Environment>,
    pub active_env_id: Option<String>,
    pub env_var_rows: Vec<UiKvRow>,

    pub tabs: Vec<TabSession>,
    pub active_tab_id: String,

    pub show_sidebar: bool,
    pub sidebar_mode: SidebarMode,
    pub split_horizontal: bool,
    pub active_drawer: Option<RightDrawerTab>,
    pub snippet_lang: SnippetLang,
    pub snippet_viewer: Entity<gpui_kit::component::input::EditorState>,

    pub search_input: Entity<gpui_kit::component::input::InputState>,
    pub settings_timeout_input: Entity<gpui_kit::component::input::InputState>,
    pub settings_proxy_input: Entity<gpui_kit::component::input::InputState>,
    pub git_commit_msg_input: Entity<gpui_kit::component::input::InputState>,
    pub update_state: UpdateState,
    pub folder_counter: usize,
    pub folder_git_statuses: HashMap<String, git::GitStatusInfo>,
    pub git_target_folder_id: Option<String>,
    pub git_details: git::GitRepoDetails,
    pub git_selected_change: Option<String>,
    pub git_diff_lines: Vec<git::GitDiffLine>,
    pub git_show_recent_commits: bool,
    pub env_name_input: Entity<gpui_kit::component::input::InputState>,
    pub pending_confirm: Option<PendingConfirm>,
    _env_name_sub: gpui_kit::Subscription,
    pub show_settings_modal: bool,
    pub settings_category: SettingsCategory,
    pub mcp_command_path: String,
    pub mcp_statuses: Vec<McpClientStatus>,
    pub mcp_notice: Option<String>,
    pub network_notice: Option<String>,
    pub mcp_json_viewer: Entity<gpui_kit::component::input::EditorState>,
    pub picker_open: bool,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (store, settings) = LocalStore::from_settings();
        let git_status = git::inspect_repo(store.workspace_dir());
        let collections = store.load_all_collections(&settings.linked_folders);
        let folder_git_statuses = compute_folder_git_statuses(&collections);
        let history = store.load_history();
        let environments = store.load_environments();
        let active_env_id: Option<String> = None;

        let env_var_rows: Vec<UiKvRow> = Vec::new();

        let initial_tab = if let Some(first_req) = collections.first().and_then(|f| f.requests.first()) {
            TabSession::from_request(first_req, Some(first_req.id.clone()), window, cx)
        } else {
            TabSession::from_request(&RequestItem::with_defaults(&settings), None, window, cx)
        };
        let active_tab_id = initial_tab.id.clone();

        let search_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("Search request or URL...")
        });
        let timeout_str = settings.default_timeout_secs.to_string();
        let settings_timeout_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("30")
                .default_value(timeout_str)
        });
        let proxy_str = settings.default_proxy_url.clone();
        let settings_proxy_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("http://127.0.0.1:8080")
                .default_value(proxy_str)
        });
        let git_commit_msg_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("Commit message (optional)")
        });
        let env_name_input = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder("Environment name")
        });
        let _env_name_sub = cx.subscribe_in(
            &env_name_input,
            window,
            |this, state, ev: &gpui_kit::component::input::InputEvent, window, cx| {
                match ev {
                    gpui_kit::component::input::InputEvent::Change => {
                        let raw = state.read(cx).value().to_string();
                        if raw.chars().count() > MAX_ENV_NAME_CHARS {
                            let clamped: String = raw.chars().take(MAX_ENV_NAME_CHARS).collect();
                            let st = state.clone();
                            window.defer(cx, move |window, cx| {
                                st.update(cx, |s, cx| s.set_value(clamped, window, cx));
                            });
                        }
                        let next_name: String =
                            raw.trim().chars().take(MAX_ENV_NAME_CHARS).collect();
                        if !next_name.is_empty()
                            && let Some(ref env_id) = this.active_env_id
                            && let Some(env) = this.environments.iter_mut().find(|e| &e.id == env_id)
                            && env.name != next_name
                        {
                            env.name = next_name;
                            cx.notify();
                        }
                    }
                    gpui_kit::component::input::InputEvent::PressEnter { .. }
                    | gpui_kit::component::input::InputEvent::Blur => {
                        this.save_active_env_vars_silent(cx);
                        cx.notify();
                    }
                    _ => {}
                }
            },
        );
        let snippet_viewer = cx.new(|cx| {
            gpui_kit::component::input::EditorState::new(window, cx)
                .language("bash")
                .line_number(true)
                .default_value("")
        });

        let focus_handle = cx.focus_handle();

        let folder_counter = collections.len();
        let mcp_command_path = resolve_texreq_executable();
        let mcp_statuses = inspect_all_clients(&mcp_command_path);
        let mcp_json_text = generic_mcp_json(&mcp_command_path);
        let mcp_json_viewer = cx.new(|cx| {
            gpui_kit::component::input::EditorState::new(window, cx)
                .language("json")
                .line_number(false)
                .default_value(mcp_json_text)
        });

        let mut ws = Self {
            focus_handle,
            store,
            settings,
            git_status,
            git_notice: None,
            collections,
            history,
            environments,
            active_env_id,
            env_var_rows,
            tabs: vec![initial_tab],
            active_tab_id,
            show_sidebar: true,
            sidebar_mode: SidebarMode::Collections,
            split_horizontal: false,
            active_drawer: None,
            snippet_lang: SnippetLang::Curl,
            snippet_viewer,
            search_input,
            settings_timeout_input,
            settings_proxy_input,
            git_commit_msg_input,
            update_state: UpdateState::Idle,
            folder_counter,
            folder_git_statuses,
            git_target_folder_id: None,
            git_details: git::GitRepoDetails::default(),
            git_selected_change: None,
            git_diff_lines: Vec::new(),
            git_show_recent_commits: false,
            env_name_input,
            pending_confirm: None,
            _env_name_sub,
            show_settings_modal: false,
            settings_category: SettingsCategory::General,
            mcp_command_path,
            mcp_statuses,
            mcp_notice: None,
            network_notice: None,
            mcp_json_viewer,
            picker_open: false,
        };

        ws.refresh_snippet_editor(window, cx);
        ws.run_update_check(false, cx);
        ws
    }

    pub fn active_tab(&self) -> Option<&TabSession> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut TabSession> {
        self.tabs.iter_mut().find(|t| t.id == self.active_tab_id)
    }

    pub fn active_environment(&self) -> Option<&Environment> {
        let id = self.active_env_id.as_ref()?;
        self.environments.iter().find(|e| &e.id == id)
    }

    pub fn resolved_env_map(&self, cx: &App) -> HashMap<String, String> {
        if self.active_env_id.is_none() {
            return HashMap::new();
        }
        let mut map = HashMap::new();
        for row in &self.env_var_rows {
            if row.enabled {
                let k = row.key_input.read(cx).value().trim().to_string();
                if !k.is_empty() {
                    map.insert(k, row.val_input.read(cx).value().to_string());
                }
            }
        }
        map
    }

    pub fn mark_tab_dirty(&mut self, tab_id: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn maybe_autoparse_curl_in_tab(
        &mut self,
        tab_id: &str,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !should_autoparse_curl(raw) {
            return false;
        }
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            if tab.ignore_next_url_change {
                tab.ignore_next_url_change = false;
                return false;
            }
            if let Ok(parsed) = parse_curl(raw) {
                tab.apply_request_model(&parsed, window, cx);
                tab.ignore_next_url_change = false;
                tab.dirty = true;
                self.refresh_snippet_editor(window, cx);
                cx.notify();
                return true;
            }
        }
        false
    }

    pub fn new_scratch_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        let tab = TabSession::from_request(&RequestItem::with_defaults(&self.settings), None, window, cx);
        let id = tab.id.clone();
        let url_input = tab.url_input.clone();
        self.tabs.push(tab);
        self.active_tab_id = id;
        url_input.update(cx, |s, cx| s.focus(window, cx));
        self.refresh_snippet_editor(window, cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        if let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) {
            let mut removed = self.tabs.remove(pos);
            if let Some(tx) = removed.cancel_tx.take() {
                let _ = tx.send(());
            }
            if let Some(ws_tx) = removed.ws_cmd_tx.take() {
                let _ = ws_tx.send(WsCommand::Disconnect);
            }
            if self.tabs.is_empty() {
                let fresh = TabSession::from_request(
                    &RequestItem::with_defaults(&self.settings),
                    None,
                    window,
                    cx,
                );
                self.active_tab_id = fresh.id.clone();
                self.tabs.push(fresh);
            } else if self.active_tab_id == tab_id {
                let next_ix = pos.min(self.tabs.len() - 1);
                self.active_tab_id = self.tabs[next_ix].id.clone();
            }
            self.refresh_snippet_editor(window, cx);
            cx.notify();
        }
    }

    pub fn select_tab(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        if self.tabs.iter().any(|t| t.id == tab_id) {
            self.active_tab_id = tab_id.to_string();
            self.refresh_snippet_editor(window, cx);
            cx.notify();
        }
    }

    pub fn open_saved_request(
        &mut self,
        req: &RequestItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing) = self
            .tabs
            .iter()
            .find(|t| t.saved_request_id.as_deref() == Some(req.id.as_str()))
        {
            self.active_tab_id = existing.id.clone();
            self.refresh_snippet_editor(window, cx);
            cx.notify();
            return;
        }
        let tab = TabSession::from_request(req, Some(req.id.clone()), window, cx);
        self.active_tab_id = tab.id.clone();
        self.tabs.push(tab);
        self.refresh_snippet_editor(window, cx);
        cx.notify();
    }

    pub fn open_history_request(
        &mut self,
        req: &RequestItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut copy = req.clone();
        copy.id = Uuid::new_v4().to_string();
        let tab = TabSession::from_request(&copy, None, window, cx);
        self.active_tab_id = tab.id.clone();
        self.tabs.push(tab);
        self.refresh_snippet_editor(window, cx);
        cx.notify();
    }

    pub fn create_folder(&mut self, cx: &mut Context<Self>) {
        self.folder_counter += 1;
        let folder = CollectionFolder::new(format!("Collection {}", self.folder_counter));
        self.collections.push(folder);
        self.store.save_all_collections(&self.collections);
        self.sidebar_mode = SidebarMode::Collections;
        self.show_sidebar = true;
        cx.notify();
    }

    /// Opens GPUI's non-blocking native path picker after the active click event finishes so OS
    /// modal loops never re-enter GPUI while `App`'s mutable `RefCell` borrow is held.
    fn pick_single_path(
        &mut self,
        files: bool,
        directories: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        on_picked: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if self.picker_open {
            return;
        }
        self.picker_open = true;

        cx.spawn_in(window, async move |this, cx| {
            let rx = cx
                .update(|_window, cx| {
                    cx.prompt_for_paths(PathPromptOptions {
                        files,
                        directories,
                        multiple: false,
                        prompt: None,
                    })
                })
                .ok();
            let res = match rx {
                Some(rx) => rx.await.ok(),
                None => None,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.picker_open = false;
                match res {
                    Some(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            on_picked(this, path, window, cx);
                        }
                    }
                    Some(Ok(None)) | None => {}
                    Some(Err(err)) => {
                        this.git_notice = Some(format!("Failed to open path dialog: {err}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    pub fn link_collection_folder_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_single_path(false, true, window, cx, |this, folder_dir, _window, cx| {
            this.apply_linked_collection_folder(folder_dir, cx);
        });
    }

    fn apply_linked_collection_folder(&mut self, folder_dir: PathBuf, cx: &mut Context<Self>) {
        let normalized = folder_dir.to_string_lossy().to_string();
        let canonical_target =
            std::fs::canonicalize(&folder_dir).unwrap_or_else(|_| folder_dir.clone());
        if let Some(existing) = self.collections.iter_mut().find(|f| {
            f.disk_path.as_ref().is_some_and(|p| {
                std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) == canonical_target
            })
        }) {
            existing.expanded = true;
            self.sidebar_mode = SidebarMode::Collections;
            self.show_sidebar = true;
            cx.notify();
            return;
        }

        match self.store.load_linked_folder(&folder_dir) {
            Ok(mut folder) => {
                folder.expanded = true;
                let fid = folder.id.clone();
                if let Some(dir) = folder.disk_path.as_deref() {
                    self.folder_git_statuses
                        .insert(fid.clone(), git::inspect_folder_repo(dir));
                }
                self.collections.push(folder);
                if !self.settings.linked_folders.iter().any(|p| p == &normalized) {
                    self.settings.linked_folders.push(normalized);
                    self.store.save_settings(&self.settings);
                }
                self.sidebar_mode = SidebarMode::Collections;
                self.show_sidebar = true;
                cx.notify();
            }
            Err(err) => {
                self.git_notice = Some(err);
                cx.notify();
            }
        }
    }

    pub fn make_folder_git_dialog(
        &mut self,
        folder_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fid = folder_id.to_string();
        self.pick_single_path(false, true, window, cx, move |this, folder_dir, _window, cx| {
            this.apply_make_folder_git(&fid, folder_dir, cx);
        });
    }

    fn apply_make_folder_git(
        &mut self,
        folder_id: &str,
        dir: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.autosave_saved_tabs(cx);
        let canonical_target = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        let already_linked = self.collections.iter().any(|f| {
            f.disk_path.as_ref().is_some_and(|p| {
                std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) == canonical_target
            })
        });
        if already_linked {
            self.git_notice = Some("That directory is already linked as a collection folder.".to_string());
            self.active_drawer = Some(RightDrawerTab::Git);
            self.refresh_all_git_statuses();
            cx.notify();
            return;
        }
        if dir.join("collection.json").exists() {
            self.git_notice = Some(
                "Chosen folder already contains collection.json. Use Link Folder to open it without overwriting."
                    .to_string(),
            );
            self.active_drawer = Some(RightDrawerTab::Git);
            self.refresh_all_git_statuses();
            cx.notify();
            return;
        }

        let Some(pos) = self
            .collections
            .iter()
            .position(|f| f.id == folder_id && f.disk_path.is_none())
        else {
            return;
        };

        self.collections[pos].disk_path = Some(dir.clone());
        self.collections[pos].expanded = true;
        if let Err(err) = self.store.save_linked_folder(&self.collections[pos]) {
            self.collections[pos].disk_path = None;
            self.git_notice = Some(err);
            self.active_drawer = Some(RightDrawerTab::Git);
            cx.notify();
            return;
        }

        self.store.save_all_collections(&self.collections);
        let normalized = dir.to_string_lossy().to_string();
        if !self.settings.linked_folders.iter().any(|p| p == &normalized) {
            self.settings.linked_folders.push(normalized);
            self.store.save_settings(&self.settings);
        }

        if !git::inspect_folder_repo(&dir).is_repo {
            match git::init_repo(&dir) {
                Ok(msg) => self.git_notice = Some(msg),
                Err(err) => self.git_notice = Some(format!("Folder linked, but git init failed: {err}")),
            }
        } else {
            git::ensure_workspace_gitignore(&dir);
            self.git_notice = Some(format!("Linked folder to Git repository at {}", dir.display()));
        }

        self.git_target_folder_id = Some(folder_id.to_string());
        self.active_drawer = Some(RightDrawerTab::Git);
        self.refresh_all_git_statuses();
        cx.notify();
    }

    pub fn toggle_folder(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        if let Some(folder) = self.collections.iter_mut().find(|f| f.id == folder_id) {
            folder.expanded = !folder.expanded;
            self.store.save_all_collections(&self.collections);
            cx.notify();
        }
    }

    pub fn request_remove_folder(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        let Some(folder) = self.collections.iter().find(|f| f.id == folder_id) else {
            return;
        };
        if let Some(path) = folder.disk_path.as_ref() {
            self.pending_confirm = Some(PendingConfirm::UnlinkFolder {
                id: folder.id.clone(),
                name: folder.name.clone(),
                path: path.to_string_lossy().to_string(),
            });
            cx.notify();
        } else if !folder.requests.is_empty() {
            self.pending_confirm = Some(PendingConfirm::DeleteFolder {
                id: folder.id.clone(),
                name: folder.name.clone(),
                count: folder.requests.len(),
            });
            cx.notify();
        } else {
            self.delete_folder(folder_id, cx);
        }
    }

    pub fn delete_folder(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        let removed_path = self
            .collections
            .iter()
            .find(|f| f.id == folder_id)
            .and_then(|f| f.disk_path.clone());
        self.collections.retain(|f| f.id != folder_id);
        self.folder_git_statuses.remove(folder_id);
        if let Some(disk_dir) = removed_path {
            let target_canon = std::fs::canonicalize(&disk_dir).unwrap_or(disk_dir);
            self.settings.linked_folders.retain(|raw| {
                let p = PathBuf::from(raw);
                let canon = std::fs::canonicalize(&p).unwrap_or(p);
                canon != target_canon
            });
            self.store.save_settings(&self.settings);
            if self.git_target_folder_id.as_deref() == Some(folder_id) {
                self.git_target_folder_id = None;
                self.refresh_all_git_statuses();
            }
        } else {
            self.store.save_all_collections(&self.collections);
        }
        cx.notify();
    }

    pub fn create_request_in_folder(
        &mut self,
        folder_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut req = RequestItem::with_defaults(&self.settings);
        req.name = "New Request".to_string();
        let req_id = req.id.clone();
        if let Some(folder) = self.collections.iter_mut().find(|f| f.id == folder_id) {
            folder.expanded = true;
            folder.requests.push(req.clone());
            self.store.save_all_collections(&self.collections);
        }
        self.refresh_folder_git_if_linked(folder_id);
        let tab = TabSession::from_request(&req, Some(req_id), window, cx);
        let url = tab.url_input.clone();
        self.active_tab_id = tab.id.clone();
        self.tabs.push(tab);
        url.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    pub fn delete_saved_request(&mut self, folder_id: &str, req_id: &str, cx: &mut Context<Self>) {
        if let Some(folder) = self.collections.iter_mut().find(|f| f.id == folder_id) {
            folder.requests.retain(|r| r.id != req_id);
            self.store.save_all_collections(&self.collections);
        }
        self.refresh_folder_git_if_linked(folder_id);
        for tab in &mut self.tabs {
            if tab.saved_request_id.as_deref() == Some(req_id) {
                tab.saved_request_id = None;
                tab.dirty = true;
            }
        }
        cx.notify();
    }

    pub fn save_active_tab(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let mut model = tab.sync_to_model(cx);
        let existing_id = tab.saved_request_id.clone();

        if self.collections.is_empty() {
            self.collections.push(CollectionFolder::new("General"));
        }

        let mut updated_folder_id: Option<String> = None;
        if let Some(ref sid) = existing_id {
            for folder in &mut self.collections {
                if let Some(slot) = folder.requests.iter_mut().find(|r| &r.id == sid) {
                    model.id = sid.clone();
                    *slot = model.clone();
                    updated_folder_id = Some(folder.id.clone());
                    break;
                }
            }
        }

        if updated_folder_id.is_none() {
            model.id = Uuid::new_v4().to_string();
            let first = &mut self.collections[0];
            first.expanded = true;
            first.requests.push(model.clone());
            updated_folder_id = Some(first.id.clone());
        }

        if let Some(tab_mut) = self.active_tab_mut() {
            tab_mut.saved_request_id = Some(model.id.clone());
            tab_mut.dirty = false;
        }

        self.store.save_all_collections(&self.collections);
        if let Some(fid) = updated_folder_id {
            self.refresh_folder_git_if_linked(&fid);
        }
        cx.notify();
    }

    pub fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.history.clear();
        self.store.save_history(&self.history);
        cx.notify();
    }

    pub fn set_active_request_kind(
        &mut self,
        kind: RequestKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_tab_mut() {
            if tab.kind != kind {
                if let Some(tx) = tab.cancel_tx.take() {
                    let _ = tx.send(());
                }
                if let Some(ws_tx) = tab.ws_cmd_tx.take() {
                    let _ = ws_tx.send(WsCommand::Disconnect);
                }
                tab.stream_status = StreamStatus::Disconnected;
                tab.kind = kind;
                tab.dirty = true;
                if kind == RequestKind::WebSocket {
                    tab.req_subtab = RequestSubTab::Body;
                }
            }
        }
        self.refresh_snippet_editor(window, cx);
        cx.notify();
    }

    pub fn set_request_subtab(&mut self, subtab: RequestSubTab, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.req_subtab = subtab;
            cx.notify();
        }
    }

    pub fn set_response_subtab(&mut self, subtab: ResponseSubTab, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.res_subtab = subtab;
            cx.notify();
        }
    }

    pub fn set_auth_kind(
        &mut self,
        auth_kind: crate::ui::tab_session::AuthKindUi,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_tab_mut() {
            tab.auth_kind = auth_kind;
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn set_apikey_location(&mut self, in_header: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.api_key_in_header = in_header;
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn set_body_kind(&mut self, body_kind: BodyKindUi, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.body_kind = body_kind;
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn set_raw_content_type(
        &mut self,
        ct: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_tab_mut() {
            tab.raw_content_type = ct.to_string();
            tab.dirty = true;
            let lang = if ct.contains("html") || ct.contains("xml") {
                "html"
            } else if ct.contains("yaml") {
                "yaml"
            } else {
                "text"
            };
            let current_text = tab.raw_body_editor.read(cx).value().to_string();
            tab.raw_body_editor = cx.new(|cx| {
                gpui_kit::component::input::EditorState::new(window, cx)
                    .language(lang)
                    .line_number(true)
                    .searchable(true)
                    .default_value(current_text)
            });
            cx.notify();
        }
    }

    pub fn format_active_json(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let raw = tab.body_json_editor.read(cx).value().to_string();
        match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(val) => {
                if let Ok(pretty) = serde_json::to_string_pretty(&val) {
                    if let Some(tab_mut) = self.active_tab_mut() {
                        tab_mut
                            .body_json_editor
                            .update(cx, |s, cx| s.replace_all(pretty, window, cx));
                        tab_mut.last_error = None;
                    }
                }
            }
            Err(e) => {
                if let Some(tab_mut) = self.active_tab_mut() {
                    tab_mut.last_error = Some(format!("Invalid JSON: {e}"));
                }
            }
        }
        cx.notify();
    }

    pub fn kv_toggle(
        &mut self,
        target: KvTarget,
        row_id: &str,
        enabled: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let list = match target {
            KvTarget::EnvVars => Some(&mut self.env_var_rows),
            _ => self.active_tab_mut().map(|t| match target {
                KvTarget::Params => &mut t.params,
                KvTarget::Headers => &mut t.headers,
                KvTarget::Form => &mut t.form_rows,
                KvTarget::Multipart => &mut t.multipart_rows,
                KvTarget::EnvVars => unreachable!(),
            }),
        };
        if let Some(rows) = list
            && let Some(row) = rows.iter_mut().find(|r| r.id == row_id)
        {
            row.enabled = enabled;
        }
        if target == KvTarget::EnvVars {
            self.save_active_env_vars(cx);
        } else if let Some(t) = self.active_tab_mut() {
            t.dirty = true;
        }
        cx.notify();
    }

    pub fn kv_add(
        &mut self,
        target: KvTarget,
        is_file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_row = UiKvRow::empty(is_file, window, cx);
        match target {
            KvTarget::EnvVars => self.env_var_rows.push(new_row),
            _ => {
                if let Some(t) = self.active_tab_mut() {
                    match target {
                        KvTarget::Params => t.params.push(new_row),
                        KvTarget::Headers => t.headers.push(new_row),
                        KvTarget::Form => t.form_rows.push(new_row),
                        KvTarget::Multipart => t.multipart_rows.push(new_row),
                        KvTarget::EnvVars => unreachable!(),
                    }
                    t.dirty = true;
                }
            }
        }
        cx.notify();
    }

    pub fn kv_remove(
        &mut self,
        target: KvTarget,
        row_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            KvTarget::EnvVars => {
                self.env_var_rows.retain(|r| r.id != row_id);
                self.save_active_env_vars(cx);
            }
            _ => {
                if let Some(t) = self.active_tab_mut() {
                    match target {
                        KvTarget::Params => t.params.retain(|r| r.id != row_id),
                        KvTarget::Headers => t.headers.retain(|r| r.id != row_id),
                        KvTarget::Form => t.form_rows.retain(|r| r.id != row_id),
                        KvTarget::Multipart => t.multipart_rows.retain(|r| r.id != row_id),
                        KvTarget::EnvVars => unreachable!(),
                    }
                    t.dirty = true;
                }
            }
        }
        cx.notify();
    }

    pub fn kv_toggle_file_mode(
        &mut self,
        row_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.active_tab_mut()
            && let Some(row) = tab.multipart_rows.iter_mut().find(|r| r.id == row_id)
        {
            row.is_file = !row.is_file;
            tab.dirty = true;
        }
        cx.notify();
    }

    pub fn pick_file_for_multipart(
        &mut self,
        row_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row_id = row_id.to_string();
        self.pick_single_path(true, false, window, cx, move |this, path, window, cx| {
            let p_str = path.to_string_lossy().to_string();
            if let Some(tab) = this.active_tab_mut()
                && let Some(row) = tab.multipart_rows.iter_mut().find(|r| r.id == row_id)
            {
                row.is_file = true;
                row.val_input
                    .update(cx, |s, cx| s.set_value(p_str, window, cx));
                tab.dirty = true;
            }
            cx.notify();
        });
    }

    pub fn pick_binary_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_single_path(true, false, window, cx, |this, path, window, cx| {
            let p_str = path.to_string_lossy().to_string();
            if let Some(tab) = this.active_tab_mut() {
                tab.binary_path_input
                    .update(cx, |s, cx| s.set_value(p_str, window, cx));
                tab.dirty = true;
            }
            cx.notify();
        });
    }

    pub fn capture_toggle(&mut self, row_id: &str, enabled: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut()
            && let Some(c) = tab.captures.iter_mut().find(|c| c.id == row_id)
        {
            c.enabled = enabled;
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn capture_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = UiCaptureRow::empty(window, cx);
        if let Some(tab) = self.active_tab_mut() {
            tab.captures.push(row);
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn capture_remove(&mut self, row_id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.captures.retain(|c| c.id != row_id);
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn set_proxy_preset(&mut self, proxy: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.proxy_input
                .update(cx, |s, cx| s.set_value(proxy.to_string(), window, cx));
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn set_follow_redirects(&mut self, follow: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.follow_redirects = follow;
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn set_accept_invalid_certs(&mut self, accept: bool, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.accept_invalid_certs = accept;
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn trigger_primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        self.refresh_snippet_editor(window, cx);
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.id.clone();
        let kind = tab.kind;
        let stream_status = tab.stream_status;
        let is_loading = tab.is_loading;

        if kind == RequestKind::WebSocket {
            match stream_status {
                StreamStatus::Disconnected => self.start_ws_connection(&tab_id, window, cx),
                StreamStatus::Connected => self.send_ws_message(cx),
                StreamStatus::Connecting => {}
            }
            return;
        }

        if kind == RequestKind::Sse {
            match stream_status {
                StreamStatus::Disconnected => self.start_sse_connection(&tab_id, window, cx),
                StreamStatus::Connecting | StreamStatus::Connected => {
                    self.disconnect_active_stream(window, cx);
                }
            }
            return;
        }

        if is_loading {
            self.cancel_active_http(cx);
            return;
        }

        self.send_http_request(&tab_id, window, cx);
    }

    fn send_http_request(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let vars = self.resolved_env_map(cx);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let req_model = tab.sync_to_model(cx);
        if req_model.url.trim().is_empty() {
            tab.last_error = Some("Please enter a request URL before sending.".to_string());
            cx.notify();
            return;
        }

        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        tab.is_loading = true;
        tab.last_error = None;
        tab.cancel_tx = Some(cancel_tx);

        let (tx, mut rx) = unbounded::<WorkerEvent>();
        let req_for_net = req_model.clone();
        let vars_for_net = vars.clone();
        spawn_tokio(async move {
            let outcome = tokio::select! {
                _ = cancel_rx => Err("Request canceled by user".to_string()),
                res = execute_http(req_for_net, vars_for_net) => res,
            };
            let _ = tx.unbounded_send(WorkerEvent::HttpFinished(outcome));
        });

        let target_tab_id = tab_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(ev) = rx.next().await {
                let ok = this.update_in(cx, |this, window, cx| {
                    this.handle_worker_event(&target_tab_id, ev, window, cx);
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    fn start_sse_connection(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let vars = self.resolved_env_map(cx);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let req_model = tab.sync_to_model(cx);
        if req_model.url.trim().is_empty() {
            tab.last_error = Some("Please enter an SSE URL to connect.".to_string());
            cx.notify();
            return;
        }

        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        tab.cancel_tx = Some(stop_tx);
        tab.last_error = None;
        tab.stream_status = StreamStatus::Connecting;

        let (tx, mut rx) = unbounded::<WorkerEvent>();
        spawn_tokio(run_sse_stream(req_model, vars, tx, stop_rx));

        let target_tab_id = tab_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(ev) = rx.next().await {
                let ok = this.update_in(cx, |this, window, cx| {
                    this.handle_worker_event(&target_tab_id, ev, window, cx);
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    fn start_ws_connection(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let vars = self.resolved_env_map(cx);
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let req_model = tab.sync_to_model(cx);
        if req_model.url.trim().is_empty() {
            tab.last_error = Some("Please enter a ws:// or wss:// URL to connect.".to_string());
            cx.notify();
            return;
        }

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<WsCommand>();
        tab.ws_cmd_tx = Some(cmd_tx);
        tab.last_error = None;
        tab.stream_status = StreamStatus::Connecting;

        let (tx, mut rx) = unbounded::<WorkerEvent>();
        spawn_tokio(run_ws_session(req_model, vars, cmd_rx, tx));

        let target_tab_id = tab_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(ev) = rx.next().await {
                let ok = this.update_in(cx, |this, window, cx| {
                    this.handle_worker_event(&target_tab_id, ev, window, cx);
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();

        cx.notify();
    }

    pub fn send_ws_message(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let msg = tab.ws_composer_editor.read(cx).value().to_string();
        if let Some(ref tx) = tab.ws_cmd_tx {
            let _ = tx.send(WsCommand::SendText(msg));
        }
    }

    pub fn load_ws_template(&mut self, tpl: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab() {
            tab.ws_composer_editor
                .update(cx, |s, cx| s.set_value(tpl.to_string(), window, cx));
            cx.notify();
        }
    }

    pub fn save_current_ws_template(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        let msg = tab.ws_composer_editor.read(cx).value().to_string();
        if !msg.trim().is_empty() && !tab.ws_templates.contains(&msg) {
            tab.ws_templates.push(msg);
            tab.dirty = true;
            cx.notify();
        }
    }

    pub fn disconnect_active_stream(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            if let Some(tx) = tab.cancel_tx.take() {
                let _ = tx.send(());
            }
            if let Some(ws_tx) = tab.ws_cmd_tx.take() {
                let _ = ws_tx.send(WsCommand::Disconnect);
            }
            tab.stream_status = StreamStatus::Disconnected;
            cx.notify();
        }
    }

    pub fn cancel_active_http(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut()
            && let Some(tx) = tab.cancel_tx.take()
        {
            let _ = tx.send(());
            tab.is_loading = false;
            cx.notify();
        }
    }

    pub fn clear_stream_log(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab_mut() {
            tab.stream_entries.clear();
            cx.notify();
        }
    }

    pub fn copy_stream_log(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab() {
            let text: Vec<String> = tab
                .stream_entries
                .iter()
                .map(|e| format!("[{}] {}", e.timestamp, e.payload))
                .collect();
            cx.write_to_clipboard(ClipboardItem::new_string(text.join("\n")));
            cx.notify();
        }
    }

    pub fn copy_active_response(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab()
            && let Some(ref resp) = tab.last_response
        {
            let text = match tab.res_subtab {
                ResponseSubTab::Raw => resp.raw_body.clone(),
                _ => resp.pretty_body.clone(),
            };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.notify();
        }
    }

    fn handle_worker_event(
        &mut self,
        tab_id: &str,
        ev: WorkerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ev {
            WorkerEvent::HttpFinished(result) => {
                let req_snapshot = self
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.sync_to_model(cx));

                let mut captured_vars: Vec<(String, String)> = Vec::new();
                let mut status_opt = None;
                let mut elapsed_opt = None;

                let auto_format = self.settings.auto_format_json;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.is_loading = false;
                    tab.cancel_tx = None;
                    match result {
                        Ok(resp) => {
                            status_opt = Some(resp.status);
                            elapsed_opt = Some(resp.elapsed_ms);
                            captured_vars = resp.captured_vars.clone();
                            if !auto_format && tab.res_subtab == ResponseSubTab::Pretty {
                                tab.res_subtab = ResponseSubTab::Raw;
                            }
                            tab.update_response_viewers(&resp, window, cx);
                            tab.last_response = Some(resp);
                            tab.last_error = None;
                        }
                        Err(err) => {
                            tab.last_error = Some(err);
                        }
                    }
                }

                if let Some(req) = req_snapshot {
                    self.history.insert(
                        0,
                        HistoryEntry {
                            id: Uuid::new_v4().to_string(),
                            timestamp: Local::now().format("%H:%M:%S").to_string(),
                            status: status_opt,
                            elapsed_ms: elapsed_opt,
                            request: req,
                        },
                    );
                    self.history.truncate(100);
                    self.store.save_history(&self.history);
                }

                if !captured_vars.is_empty() {
                    self.apply_captured_vars(captured_vars, window, cx);
                }
            }
            WorkerEvent::StreamStatus(st) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.stream_status = st;
                    if st == StreamStatus::Disconnected {
                        tab.cancel_tx = None;
                        tab.ws_cmd_tx = None;
                    }
                }
            }
            WorkerEvent::StreamMessage(entry) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.stream_entries.push(entry);
                    if tab.stream_entries.len() > 2000 {
                        let excess = tab.stream_entries.len() - 2000;
                        tab.stream_entries.drain(..excess);
                    }
                }
            }
            WorkerEvent::StreamError(err) => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.last_error = Some(err.clone());
                    tab.stream_entries.push(crate::models::StreamEntry::now(
                        crate::models::StreamDirection::System,
                        None,
                        err,
                    ));
                }
            }
        }
        cx.notify();
    }

    fn apply_captured_vars(
        &mut self,
        captured: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(env_id) = self.active_env_id.clone() else {
            return;
        };

        let mut names = Vec::new();
        for (k, v) in captured {
            names.push(k.clone());
            if let Some(existing) = self
                .env_var_rows
                .iter()
                .find(|r| r.key_input.read(cx).value().trim() == k)
            {
                existing
                    .val_input
                    .update(cx, |s, cx| s.set_value(v.clone(), window, cx));
            } else {
                self.env_var_rows
                    .push(UiKvRow::from_model(&KeyValueRow::new(k, v), window, cx));
            }
        }

        let updated_models: Vec<KeyValueRow> =
            self.env_var_rows.iter().map(|r| r.to_model(cx)).collect();
        if let Some(env) = self.environments.iter_mut().find(|e| e.id == env_id) {
            env.variables = updated_models;
        }
        self.store.save_environments(&self.environments);
        let _ = names;
    }

    fn sync_active_env_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected_name = self
            .active_env_id
            .as_ref()
            .and_then(|id| self.environments.iter().find(|e| &e.id == id))
            .map(|e| e.name.clone())
            .unwrap_or_default();
        self.env_name_input
            .update(cx, |s, cx| s.set_value(selected_name, window, cx));
        self.env_var_rows = self
            .active_env_id
            .as_ref()
            .and_then(|id| self.environments.iter().find(|e| &e.id == id))
            .map(|env| {
                env.variables
                    .iter()
                    .map(|r| UiKvRow::from_model(r, window, cx))
                    .collect()
            })
            .unwrap_or_default();
        self.refresh_snippet_editor(window, cx);
    }

    pub fn select_environment(
        &mut self,
        env_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.save_active_env_vars_silent(cx);
        self.active_env_id = env_id;
        self.sync_active_env_inputs(window, cx);
        cx.notify();
    }

    pub fn create_environment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_active_env_vars_silent(cx);
        let name = next_environment_name(&self.environments);
        let env = Environment::new(name, Vec::new());
        let new_id = env.id.clone();
        self.environments.push(env);
        self.store.save_environments(&self.environments);
        self.select_environment(Some(new_id), window, cx);
        self.active_drawer = Some(RightDrawerTab::Variables);
        cx.notify();
    }

    pub fn request_delete_active_environment(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.active_env_id.clone() {
            self.request_delete_environment(&id, cx);
        }
    }

    pub fn request_delete_environment(&mut self, env_id: &str, cx: &mut Context<Self>) {
        if let Some(env) = self.environments.iter().find(|e| e.id == env_id) {
            self.pending_confirm = Some(PendingConfirm::DeleteEnvironment {
                id: env.id.clone(),
                name: env.name.clone(),
            });
            cx.notify();
        }
    }

    pub fn request_clear_history(&mut self, cx: &mut Context<Self>) {
        if !self.history.is_empty() {
            self.pending_confirm = Some(PendingConfirm::ClearHistory);
            cx.notify();
        }
    }

    pub fn dismiss_confirm(&mut self, cx: &mut Context<Self>) {
        if self.pending_confirm.take().is_some() {
            cx.notify();
        }
    }

    pub fn accept_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.pending_confirm.take() {
            Some(PendingConfirm::DeleteEnvironment { id, .. }) => {
                self.delete_environment(&id, window, cx);
            }
            Some(PendingConfirm::ClearHistory) => {
                self.clear_history(cx);
            }
            Some(PendingConfirm::DeleteFolder { id, .. })
            | Some(PendingConfirm::UnlinkFolder { id, .. }) => {
                self.delete_folder(&id, cx);
            }
            Some(PendingConfirm::DiscardGitChange {
                repo_root,
                file,
                has_head,
            }) => {
                match git::discard_file_change(&repo_root, &file, has_head) {
                    Ok(msg) => {
                        self.git_notice = Some(msg);
                        self.reload_or_seed_workspace(window, cx);
                        self.sync_open_saved_tabs_after_reload(window, cx);
                    }
                    Err(err) => {
                        self.git_notice = Some(format!("Discard failed: {err}"));
                        self.refresh_all_git_statuses();
                    }
                }
                cx.notify();
            }
            None => {}
        }
    }

    fn sync_open_saved_tabs_after_reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut by_id: HashMap<String, RequestItem> = HashMap::new();
        for folder in &self.collections {
            for req in &folder.requests {
                by_id.insert(req.id.clone(), req.clone());
            }
        }
        let existing_tabs = std::mem::take(&mut self.tabs);
        for mut tab in existing_tabs {
            if let Some(sid) = tab.saved_request_id.clone() {
                if let Some(req) = by_id.get(&sid) {
                    let mut rebuilt = TabSession::from_request(req, Some(sid), window, cx);
                    rebuilt.id = tab.id;
                    self.tabs.push(rebuilt);
                    continue;
                }
                tab.saved_request_id = None;
                tab.dirty = true;
            }
            self.tabs.push(tab);
        }
        self.refresh_snippet_editor(window, cx);
    }

    pub fn delete_environment(
        &mut self,
        env_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let before = self.environments.len();
        self.environments.retain(|e| e.id != env_id);
        if self.environments.len() == before {
            return;
        }
        self.store.save_environments(&self.environments);
        if self.active_env_id.as_deref() == Some(env_id) {
            self.active_env_id = None;
            self.env_var_rows.clear();
            self.env_name_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.refresh_snippet_editor(window, cx);
        }
        cx.notify();
    }

    pub fn save_active_env_vars_silent(&mut self, cx: &App) {
        if let Some(ref env_id) = self.active_env_id {
            let models: Vec<KeyValueRow> =
                self.env_var_rows.iter().map(|r| r.to_model(cx)).collect();
            let edited_name: String = self
                .env_name_input
                .read(cx)
                .value()
                .trim()
                .chars()
                .take(MAX_ENV_NAME_CHARS)
                .collect();
            if let Some(env) = self.environments.iter_mut().find(|e| &e.id == env_id) {
                if !edited_name.is_empty() {
                    env.name = edited_name;
                }
                env.variables = models;
            }
            self.store.save_environments(&self.environments);
        }
    }

    pub fn save_active_env_vars(&mut self, cx: &mut Context<Self>) {
        self.save_active_env_vars_silent(cx);
        cx.notify();
    }

    pub fn import_dotenv_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_single_path(true, false, window, cx, Self::apply_dotenv_file);
    }

    fn apply_dotenv_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(content) = std::fs::read_to_string(&path) else {
            if let Some(tab) = self.active_tab_mut() {
                tab.last_error = Some("Failed to read .env file.".to_string());
            }
            cx.notify();
            return;
        };

        let parsed_rows = parse_dotenv(&content);
        if parsed_rows.is_empty() {
            if let Some(tab) = self.active_tab_mut() {
                tab.last_error = Some("No variables found in the selected .env file.".to_string());
            }
            cx.notify();
            return;
        }

        if self.active_env_id.is_none() {
            let env = Environment::new("Imported (.env)", parsed_rows);
            let id = env.id.clone();
            self.environments.push(env);
            self.store.save_environments(&self.environments);
            self.select_environment(Some(id), window, cx);
        } else {
            for r in parsed_rows {
                if let Some(existing) = self
                    .env_var_rows
                    .iter()
                    .find(|er| er.key_input.read(cx).value().trim() == r.key)
                {
                    existing
                        .val_input
                        .update(cx, |s, cx| s.set_value(r.value, window, cx));
                } else {
                    self.env_var_rows.push(UiKvRow::from_model(&r, window, cx));
                }
            }
            self.save_active_env_vars_silent(cx);
        }

        self.active_drawer = Some(RightDrawerTab::Variables);
        cx.notify();
    }

    pub fn toggle_drawer(
        &mut self,
        tab: RightDrawerTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_drawer == Some(tab) {
            self.active_drawer = None;
        } else {
            self.active_drawer = Some(tab);
            if tab == RightDrawerTab::Snippets {
                self.refresh_snippet_editor(window, cx);
            }
        }
        cx.notify();
    }

    pub fn toggle_drawer_no_window(&mut self, tab: RightDrawerTab, cx: &mut Context<Self>) {
        if self.active_drawer == Some(tab) {
            self.active_drawer = None;
        } else {
            self.active_drawer = Some(tab);
            if tab == RightDrawerTab::Git {
                self.autosave_saved_tabs(cx);
                self.pick_default_git_target_if_unset();
                self.refresh_all_git_statuses();
            }
        }
        cx.notify();
    }

    pub fn refresh_snippet_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vars = self.resolved_env_map(cx);
        let Some(tab) = self.active_tab() else {
            return;
        };
        let req = tab.sync_to_model(cx);
        let code = generate_snippet(&req, &vars, self.snippet_lang);
        let lang = self.snippet_lang.editor_language();
        self.snippet_viewer = cx.new(|cx| {
            gpui_kit::component::input::EditorState::new(window, cx)
                .language(lang)
                .line_number(true)
                .default_value(code)
        });
    }

    pub fn copy_snippet(&mut self, cx: &mut Context<Self>) {
        let code = self.snippet_viewer.read(cx).value().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(code));
        cx.notify();
    }

    fn autosave_saved_tabs(&mut self, cx: &App) {
        if !self.settings.auto_save {
            return;
        }
        let mut changed = false;
        for tab in &mut self.tabs {
            if !tab.dirty {
                continue;
            }
            let Some(sid) = tab.saved_request_id.clone() else {
                continue;
            };
            let mut model = tab.sync_to_model(cx);
            for folder in &mut self.collections {
                if let Some(slot) = folder.requests.iter_mut().find(|r| r.id == sid) {
                    model.id = sid.clone();
                    *slot = model;
                    tab.dirty = false;
                    changed = true;
                    break;
                }
            }
        }
        if changed {
            self.store.save_all_collections(&self.collections);
            self.refresh_all_git_statuses();
        }
    }

    pub fn set_auto_save(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.auto_save = enabled;
        self.store.save_settings(&self.settings);
        if enabled {
            self.autosave_saved_tabs(cx);
        }
        cx.notify();
    }

    pub fn set_auto_format_json(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.auto_format_json = enabled;
        self.store.save_settings(&self.settings);
        cx.notify();
    }

    pub fn set_global_ssl_verify(&mut self, verify: bool, cx: &mut Context<Self>) {
        let accept_invalid = !verify;
        self.settings.default_accept_invalid_certs = accept_invalid;
        self.store.save_settings(&self.settings);
        for tab in &mut self.tabs {
            tab.accept_invalid_certs = accept_invalid;
            if tab.saved_request_id.is_some() {
                tab.dirty = true;
            }
        }
        self.autosave_saved_tabs(cx);
        cx.notify();
    }

    pub fn set_default_follow_redirects(&mut self, follow: bool, cx: &mut Context<Self>) {
        self.settings.default_follow_redirects = follow;
        self.store.save_settings(&self.settings);
        cx.notify();
    }

    pub fn apply_default_network_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let timeout_raw = self.settings_timeout_input.read(cx).value().trim().to_string();
        let timeout_secs = timeout_raw.parse::<u64>().unwrap_or(30).clamp(1, 600);
        let proxy_url = self.settings_proxy_input.read(cx).value().trim().to_string();

        self.settings.default_timeout_secs = timeout_secs;
        self.settings.default_proxy_url = proxy_url;
        self.store.save_settings(&self.settings);

        let normalized_timeout = timeout_secs.to_string();
        self.settings_timeout_input
            .update(cx, |s, cx| s.set_value(normalized_timeout, window, cx));
        self.network_notice = Some("Default network settings updated.".to_string());
        cx.notify();
    }

    pub fn toggle_settings_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_settings_modal {
            self.close_settings(cx);
        } else {
            let category = self.settings_category;
            self.open_settings(category, window, cx);
        }
    }

    pub fn open_settings(
        &mut self,
        category: SettingsCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_settings_modal = true;
        self.settings_category = category;
        self.refresh_all_git_statuses();
        self.refresh_mcp_state(window, cx);
        cx.notify();
    }

    pub fn open_git_drawer(&mut self, folder_id: Option<String>, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        if let Some(fid) = folder_id {
            self.git_target_folder_id = Some(fid);
        } else {
            self.pick_default_git_target_if_unset();
        }
        self.git_selected_change = None;
        self.git_notice = None;
        self.active_drawer = Some(RightDrawerTab::Git);
        self.refresh_all_git_statuses();
        cx.notify();
    }

    pub fn open_folder_git_drawer(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        self.open_git_drawer(Some(folder_id.to_string()), cx);
    }

    pub fn init_folder_git_now(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        self.git_target_folder_id = Some(folder_id.to_string());
        self.git_selected_change = None;
        self.active_drawer = Some(RightDrawerTab::Git);
        self.git_init_workspace(cx);
    }

    fn pick_default_git_target_if_unset(&mut self) {
        if self.git_target_folder_id.is_some() {
            return;
        }
        if let Some(saved_id) = self
            .active_tab()
            .and_then(|t| t.saved_request_id.as_deref())
            && let Some(folder) = self.collections.iter().find(|f| {
                f.disk_path.is_some() && f.requests.iter().any(|r| r.id == saved_id)
            })
        {
            self.git_target_folder_id = Some(folder.id.clone());
            return;
        }
        let personal_is_repo = git::inspect_repo(self.store.workspace_dir()).is_repo;
        if !personal_is_repo {
            if let Some(folder) = self.collections.iter().find(|f| {
                f.disk_path.is_some()
                    && self
                        .folder_git_statuses
                        .get(&f.id)
                        .is_some_and(|s| s.is_repo)
            }) {
                self.git_target_folder_id = Some(folder.id.clone());
                return;
            }
            if let Some(folder) = self.collections.iter().find(|f| f.disk_path.is_some()) {
                self.git_target_folder_id = Some(folder.id.clone());
            }
        }
    }

    pub fn select_git_target(&mut self, folder_id: Option<String>, cx: &mut Context<Self>) {
        self.git_target_folder_id = folder_id;
        self.git_selected_change = None;
        self.git_notice = None;
        self.refresh_all_git_statuses();
        cx.notify();
    }

    pub fn select_git_change(&mut self, repo_path: String, cx: &mut Context<Self>) {
        self.git_selected_change = Some(repo_path);
        self.sync_selected_git_diff();
        cx.notify();
    }

    pub fn request_discard_git_change(&mut self, file: git::GitFileChange, cx: &mut Context<Self>) {
        let Some(repo_root) = self.git_details.repo_root.clone() else {
            return;
        };
        self.pending_confirm = Some(PendingConfirm::DiscardGitChange {
            repo_root,
            file,
            has_head: self.git_details.has_head,
        });
        cx.notify();
    }

    pub fn has_pending_git_changes(&self) -> bool {
        (self.git_status.is_repo && self.git_status.changed_files > 0)
            || self
                .folder_git_statuses
                .values()
                .any(|s| s.is_repo && s.changed_files > 0)
    }

    pub fn active_git_linked_folder(&self) -> Option<&CollectionFolder> {
        let fid = self.git_target_folder_id.as_deref()?;
        self.collections
            .iter()
            .find(|f| f.id == fid && f.disk_path.is_some())
    }

    pub fn active_git_target_dir(&self) -> PathBuf {
        self.active_git_linked_folder()
            .and_then(|f| f.disk_path.clone())
            .unwrap_or_else(|| self.store.workspace_dir().to_path_buf())
    }

    fn sync_selected_git_diff(&mut self) {
        let target = self
            .git_selected_change
            .as_deref()
            .and_then(|sel| self.git_details.changes.iter().find(|c| c.repo_path == sel))
            .or_else(|| self.git_details.changes.first())
            .cloned();

        match (target, self.git_details.repo_root.as_deref()) {
            (Some(file), Some(root)) => {
                self.git_selected_change = Some(file.repo_path.clone());
                self.git_diff_lines = git::read_file_diff(root, &file, self.git_details.has_head);
            }
            _ => {
                self.git_selected_change = None;
                self.git_diff_lines.clear();
            }
        }
    }

    fn refresh_active_git_details(&mut self) {
        let is_linked = self.active_git_linked_folder().is_some();
        let dir = self.active_git_target_dir();
        self.git_details = git::inspect_repo_details(&dir, is_linked);
        self.git_status = self.git_details.status.clone();
        self.sync_selected_git_diff();
    }

    fn refresh_folder_git_if_linked(&mut self, folder_id: &str) {
        if let Some(dir) = self
            .collections
            .iter()
            .find(|f| f.id == folder_id)
            .and_then(|f| f.disk_path.as_deref())
        {
            let status = git::inspect_folder_repo(dir);
            self.folder_git_statuses
                .insert(folder_id.to_string(), status.clone());
            if self.git_target_folder_id.as_deref() == Some(folder_id) {
                self.git_status = status;
            }
        }
        if self.active_drawer == Some(RightDrawerTab::Git) {
            self.refresh_active_git_details();
        }
    }

    fn refresh_all_git_statuses(&mut self) {
        self.folder_git_statuses = compute_folder_git_statuses(&self.collections);
        if let Some(fid) = self.git_target_folder_id.clone() {
            if let Some(status) = self.folder_git_statuses.get(&fid) {
                self.git_status = status.clone();
                if self.active_drawer == Some(RightDrawerTab::Git) {
                    self.refresh_active_git_details();
                }
                return;
            }
            self.git_target_folder_id = None;
        }
        if self.active_drawer == Some(RightDrawerTab::Git) {
            self.refresh_active_git_details();
        } else {
            self.git_status = git::inspect_repo(self.store.workspace_dir());
        }
    }

    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.show_settings_modal = false;
        cx.notify();
    }

    pub fn select_settings_category(
        &mut self,
        category: SettingsCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_category = category;
        if category == SettingsCategory::WorkspaceGit {
            self.refresh_all_git_statuses();
        } else if category == SettingsCategory::Mcp {
            self.refresh_mcp_state(window, cx);
        }
        cx.notify();
    }

    pub fn refresh_mcp_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mcp_command_path = resolve_texreq_executable();
        self.mcp_statuses = inspect_all_clients(&self.mcp_command_path);
        let json_text = generic_mcp_json(&self.mcp_command_path);
        self.mcp_json_viewer.update(cx, |state, cx| {
            state.set_value(json_text, window, cx);
        });
        cx.notify();
    }

    pub fn install_mcp_client(&mut self, client: McpClient, cx: &mut Context<Self>) {
        match install_client(client, &self.mcp_command_path) {
            Ok(msg) => {
                self.mcp_notice = Some(msg);
                self.mcp_statuses = inspect_all_clients(&self.mcp_command_path);
            }
            Err(err) => {
                self.mcp_notice = Some(err);
            }
        }
        cx.notify();
    }

    pub fn uninstall_mcp_client(&mut self, client: McpClient, cx: &mut Context<Self>) {
        match uninstall_client(client) {
            Ok(msg) => {
                self.mcp_notice = Some(msg);
                self.mcp_statuses = inspect_all_clients(&self.mcp_command_path);
            }
            Err(err) => {
                self.mcp_notice = Some(err);
            }
        }
        cx.notify();
    }

    pub fn copy_mcp_command(&mut self, cx: &mut Context<Self>) {
        let cmd = format!("{} --mcp", self.mcp_command_path);
        cx.write_to_clipboard(ClipboardItem::new_string(cmd));
        self.mcp_notice = Some("Copied MCP command to clipboard.".to_string());
        cx.notify();
    }

    pub fn copy_mcp_json(&mut self, cx: &mut Context<Self>) {
        let json = generic_mcp_json(&self.mcp_command_path);
        cx.write_to_clipboard(ClipboardItem::new_string(json));
        self.mcp_notice = Some("Copied generic MCP JSON to clipboard.".to_string());
        cx.notify();
    }

    pub fn set_mcp_mask_secrets(&mut self, mask: bool, cx: &mut Context<Self>) {
        self.settings.mcp_mask_secrets = mask;
        self.store.save_settings(&self.settings);
        cx.notify();
    }

    pub fn set_mcp_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.settings.mcp_read_only = read_only;
        self.store.save_settings(&self.settings);
        cx.notify();
    }

    pub fn open_github_repo(&mut self, _cx: &mut Context<Self>) {
        let _ = updater::open_url_in_browser(REPO_URL);
    }

    pub fn pick_workspace_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_single_path(false, true, window, cx, |this, folder, window, cx| {
            this.switch_workspace_dir(Some(folder), window, cx);
        });
    }

    pub fn reset_workspace_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.switch_workspace_dir(None, window, cx);
    }

    fn switch_workspace_dir(
        &mut self,
        custom_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.autosave_saved_tabs(cx);
        self.save_active_env_vars_silent(cx);

        self.settings.workspace_dir = custom_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        self.store.set_workspace_dir(custom_dir);
        self.store.save_settings(&self.settings);

        self.reload_or_seed_workspace(window, cx);
        self.refresh_all_git_statuses();
        self.git_notice = Some(format!(
            "Workspace set to {}",
            self.store.workspace_dir().to_string_lossy()
        ));
        cx.notify();
    }

    fn reload_or_seed_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.store.workspace_dir().to_path_buf();
        if dir.join("collections.json").exists() {
            self.collections = self.store.load_all_collections(&self.settings.linked_folders);
        } else {
            self.store.save_all_collections(&self.collections);
            self.collections = self.store.load_all_collections(&self.settings.linked_folders);
        }

        if dir.join("environments.toml").exists() {
            self.environments = self.store.load_environments();
            self.active_env_id = self
                .active_env_id
                .take()
                .filter(|id| self.environments.iter().any(|e| &e.id == id));
        } else {
            self.store.save_environments(&self.environments);
        }

        self.sync_active_env_inputs(window, cx);
        self.refresh_all_git_statuses();
    }

    pub fn refresh_git_status(&mut self, cx: &mut Context<Self>) {
        self.refresh_all_git_statuses();
        cx.notify();
    }

    pub fn git_init_workspace(&mut self, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        self.save_active_env_vars_silent(cx);
        self.store.save_all_collections(&self.collections);
        if self.active_git_linked_folder().is_none() {
            self.store.save_environments(&self.environments);
        }

        let target_dir = self.active_git_target_dir();
        match git::init_repo(&target_dir) {
            Ok(msg) => self.git_notice = Some(msg),
            Err(err) => self.git_notice = Some(format!("Git init failed: {err}")),
        }
        self.refresh_all_git_statuses();
        cx.notify();
    }

    pub fn git_commit_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        self.save_active_env_vars_silent(cx);
        self.store.save_all_collections(&self.collections);
        let is_linked = self.active_git_linked_folder().is_some();
        if !is_linked {
            self.store.save_environments(&self.environments);
        }

        let target_dir = self.active_git_target_dir();
        let message = self.git_commit_msg_input.read(cx).value().to_string();
        let outcome = if is_linked {
            git::commit_linked_folder(&target_dir, &message)
        } else {
            git::commit_workspace(&target_dir, &message)
        };
        match outcome {
            Ok(msg) => {
                self.git_commit_msg_input
                    .update(cx, |s, cx| s.set_value(String::new(), window, cx));
                self.git_notice = Some(msg);
            }
            Err(err) => {
                self.git_notice = Some(format!("Commit failed: {err}"));
            }
        }
        self.refresh_all_git_statuses();
        cx.notify();
    }

    pub fn git_pull_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.active_git_target_dir();
        let (tx, mut rx) = unbounded::<Result<String, String>>();
        self.git_notice = Some("Pulling from origin...".to_string());
        cx.notify();

        spawn_tokio(async move {
            let res = tokio::task::spawn_blocking(move || git::pull_repo(&dir))
                .await
                .unwrap_or_else(|e| Err(format!("Task error: {e}")));
            let _ = tx.unbounded_send(res);
        });

        cx.spawn_in(window, async move |this, cx| {
            if let Some(res) = rx.next().await {
                let _ = this.update_in(cx, |this, window, cx| {
                    match res {
                        Ok(msg) => {
                            this.reload_or_seed_workspace(window, cx);
                            this.sync_open_saved_tabs_after_reload(window, cx);
                            this.git_notice = Some(if msg.is_empty() {
                                "Pulled latest changes.".to_string()
                            } else {
                                msg
                            });
                        }
                        Err(err) => {
                            this.git_notice = Some(format!("Pull failed: {err}"));
                        }
                    }
                    this.refresh_all_git_statuses();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn git_push_workspace(&mut self, cx: &mut Context<Self>) {
        let dir = self.active_git_target_dir();
        let (tx, mut rx) = unbounded::<Result<String, String>>();
        self.git_notice = Some("Pushing to origin...".to_string());
        cx.notify();

        spawn_tokio(async move {
            let res = tokio::task::spawn_blocking(move || git::push_repo(&dir))
                .await
                .unwrap_or_else(|e| Err(format!("Task error: {e}")));
            let _ = tx.unbounded_send(res);
        });

        cx.spawn(async move |this, cx| {
            if let Some(res) = rx.next().await {
                let _ = this.update(cx, |this, cx| {
                    match res {
                        Ok(msg) => {
                            this.git_notice = Some(if msg.is_empty() {
                                "Pushed to remote.".to_string()
                            } else {
                                msg
                            });
                        }
                        Err(err) => {
                            this.git_notice = Some(format!("Push failed: {err}"));
                        }
                    }
                    this.refresh_all_git_statuses();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(self.update_state, UpdateState::Checking) {
            return;
        }
        self.run_update_check(true, cx);
    }

    fn run_update_check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if manual {
            self.update_state = UpdateState::Checking;
            cx.notify();
        }
        let (tx, mut rx) = unbounded::<Result<Option<updater::UpdateInfo>, String>>();
        spawn_tokio(async move {
            let outcome = updater::check_for_updates().await;
            let _ = tx.unbounded_send(outcome);
        });

        cx.spawn(async move |this, cx| {
            if let Some(res) = rx.next().await {
                let _ = this.update(cx, |this, cx| {
                    this.update_state = match res {
                        Ok(Some(info)) => UpdateState::Available(info),
                        Ok(None) => {
                            if manual {
                                UpdateState::UpToDate
                            } else {
                                UpdateState::Idle
                            }
                        }
                        Err(err) => {
                            if manual {
                                UpdateState::Failed(err)
                            } else {
                                UpdateState::Idle
                            }
                        }
                    };
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn open_update_download(&mut self, cx: &mut Context<Self>) {
        let url = match &self.update_state {
            UpdateState::Available(info) => info.download_url.clone(),
            _ => updater::RELEASES_PAGE_URL.to_string(),
        };
        if let Err(err) = updater::open_url_in_browser(&url) {
            self.update_state = UpdateState::Failed(err);
            cx.notify();
        }
    }

    pub fn open_release_notes(&mut self, cx: &mut Context<Self>) {
        let url = match &self.update_state {
            UpdateState::Available(info) => info.release_url.clone(),
            _ => updater::RELEASES_PAGE_URL.to_string(),
        };
        if let Err(err) = updater::open_url_in_browser(&url) {
            self.update_state = UpdateState::Failed(err);
            cx.notify();
        }
    }

    fn on_send_action(&mut self, _: &SendRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.trigger_primary_action(window, cx);
    }

    fn on_new_tab_action(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.new_scratch_tab(window, cx);
    }

    fn on_save_action(&mut self, _: &SaveRequest, _: &mut Window, cx: &mut Context<Self>) {
        self.save_active_tab(cx);
    }

    fn on_focus_url_action(&mut self, _: &FocusUrl, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.active_tab() {
            tab.url_input.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    fn on_toggle_sidebar_action(
        &mut self,
        _: &ToggleSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_sidebar = !self.show_sidebar;
        cx.notify();
    }

    fn on_focus_search_action(
        &mut self,
        _: &FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_sidebar = true;
        self.search_input.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    fn on_format_json_action(
        &mut self,
        _: &FormatJson,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.format_active_json(window, cx);
    }

    fn on_quit_action(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        self.autosave_saved_tabs(cx);
        self.save_active_env_vars_silent(cx);
        cx.quit();
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_confirm.is_some() && ev.keystroke.key == "escape" {
            self.dismiss_confirm(cx);
            return;
        }
        if self.show_settings_modal && ev.keystroke.key == "escape" {
            self.close_settings(cx);
            return;
        }
        if ev.keystroke.modifiers.control && ev.keystroke.key == "enter" {
            self.trigger_primary_action(window, cx);
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let active_env_name = self
            .active_environment()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "No Environment".to_string());
        let active_env_id = self.active_env_id.clone();
        let envs_snapshot: Vec<(String, String)> = self
            .environments
            .iter()
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect();

        let split_horizontal = self.split_horizontal;
        let show_sidebar = self.show_sidebar;
        let active_tab_id = self.active_tab_id.clone();
        let available_update = match &self.update_state {
            UpdateState::Available(info) => Some(info.clone()),
            _ => None,
        };

        let center_split = if split_horizontal {
            h_resizable("req-res-split-h")
                .with_handle_appearance(crisp_resize_handle_appearance())
                .child(resizable_panel().child(render_request_pane(self, window, cx)))
                .child(resizable_panel().child(render_response_pane(self, window, cx)))
                .into_any_element()
        } else {
            v_resizable("req-res-split-v")
                .with_handle_appearance(crisp_resize_handle_appearance())
                .child(resizable_panel().child(render_request_pane(self, window, cx)))
                .child(resizable_panel().child(render_response_pane(self, window, cx)))
                .into_any_element()
        };

        let center_and_rail = v_flex()
            .size_full()
            .overflow_hidden()
            .bg(solid(BG_BASE))
            .child(
                // Integrated Frameless Top Bar: Tabs + Drag Spacer + Toolbar + Min/Max/Close
                h_flex()
                    .w_full()
                    .h(px(40.))
                    .pl_2()
                    .gap_2()
                    .items_center()
                    .bg(solid(BG_BASE))
                    .border_b_1()
                    .border_color(solid(BORDER_SUBTLE))
                    .when(!show_sidebar, |bar| {
                        bar.child(render_app_badge(22.))
                    })
                    .child(
                        Button::new("top-toggle-sb")
                            .with_size(Size::XSmall)
                            .ghost()
                            .icon(if show_sidebar {
                                IconName::PanelLeftClose
                            } else {
                                IconName::PanelLeftOpen
                            })
                            .tooltip("Toggle Sidebar")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sidebar = !this.show_sidebar;
                                cx.notify();
                            })),
                    )
                    .child(
                        // Open tabs strip (fits tab content so remaining bar space stays draggable)
                        h_flex()
                            .id("top-tabs-strip")
                            .flex_initial()
                            .min_w_0()
                            .h_full()
                            .items_center()
                            .gap_1()
                            .overflow_x_scroll()
                            .children(self.tabs.iter().enumerate().map(|(ix, tab)| {
                                let tid = tab.id.clone();
                                let tid_close = tab.id.clone();
                                let is_active = tid == active_tab_id;
                                let model_body = match tab.body_kind {
                                    BodyKindUi::GraphQl => crate::models::BodyKind::GraphQl {
                                        query: String::new(),
                                        variables: String::new(),
                                    },
                                    _ => crate::models::BodyKind::None,
                                };
                                let (badge_lbl, badge_hex) = badge_info(tab.kind, &model_body);
                                let title = tab.tab_title(cx);
                                let dirty = tab.dirty;

                                h_flex()
                                    .id(("open-tab", ix))
                                    .h(px(30.))
                                    .px_2()
                                    .gap_1p5()
                                    .flex_shrink_0()
                                    .items_center()
                                    .rounded(px(RADIUS_CONTROL))
                                    .cursor_pointer()
                                    .bg(if is_active {
                                        solid(BG_ELEVATED)
                                    } else {
                                        solid(BG_SURFACE)
                                    })
                                    .border_1()
                                    .border_color(if is_active {
                                        solid(BORDER)
                                    } else {
                                        solid(BORDER_SUBTLE)
                                    })
                                    .hover(|s| s.bg(solid(BG_HOVER)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_tab(&tid, window, cx);
                                    }))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_bold()
                                            .text_color(solid(badge_hex))
                                            .child(badge_lbl),
                                    )
                                    .child(
                                        div()
                                            .max_w(px(140.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(if is_active {
                                                solid(TEXT_PRIMARY)
                                            } else {
                                                solid(TEXT_MUTED)
                                            })
                                            .child(SharedString::from(if dirty {
                                                format!("{title} *")
                                            } else {
                                                title
                                            })),
                                    )
                                    .child(
                                        Button::new(("close-tab", ix))
                                            .with_size(Size::XSmall)
                                            .ghost()
                                            .icon(IconName::Close)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.close_tab(&tid_close, window, cx);
                                            })),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(36.))
                            .h_full()
                            .window_control_area(WindowControlArea::Drag),
                    )
                    .child(
                        // Right-side global toolbar controls + integrated window controls
                        h_flex()
                            .flex_shrink_0()
                            .h_full()
                            .gap_1p5()
                            .items_center()
                            .when_some(available_update, |bar, info| {
                                bar.child(
                                    Button::new("top-update-pill")
                                        .with_size(Size::XSmall)
                                        .secondary()
                                        .icon(IconName::ArrowDown)
                                        .label(SharedString::from(format!("Update {}", info.tag_name)))
                                        .tooltip("New TexReq version available in Settings")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_settings(SettingsCategory::Info, window, cx);
                                        })),
                                )
                            })
                            .child(
                                Button::new("top-toggle-split")
                                    .with_size(Size::XSmall)
                                    .secondary()
                                    .icon(if split_horizontal {
                                        IconName::PanelBottom
                                    } else {
                                        IconName::PanelRight
                                    })
                                    .label(if split_horizontal {
                                        "Vertical"
                                    } else {
                                        "Side by Side"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.split_horizontal = !this.split_horizontal;
                                        cx.notify();
                                    })),
                            )
                            .child({
                                let top_env_label = truncate_chars(&active_env_name, 18);
                                let full_env_name = active_env_name.clone();
                                let show_tip = top_env_label != full_env_name;
                                Button::new("top-env-picker")
                                    .with_size(Size::XSmall)
                                    .secondary()
                                    .dropdown_caret(true)
                                    .max_w(px(180.))
                                    .label(SharedString::from(top_env_label))
                                    .when(show_tip, |btn| {
                                        btn.tooltip(SharedString::from(full_env_name))
                                    })
                                    .dropdown_menu(move |mut menu, _, _| {
                                        let v_none = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("No Environment")
                                                .checked(active_env_id.is_none())
                                                .on_click(move |_, window, app| {
                                                    v_none.update(app, |this, cx| {
                                                        this.select_environment(None, window, cx);
                                                    });
                                                }),
                                        );
                                        for (eid, ename) in &envs_snapshot {
                                            let v_env = view.clone();
                                            let id_clone = eid.clone();
                                            let is_cur =
                                                active_env_id.as_deref() == Some(eid.as_str());
                                            menu = menu.item(
                                                PopupMenuItem::new(truncate_chars(ename, 28))
                                                    .checked(is_cur)
                                                    .on_click(move |_, window, app| {
                                                        v_env.update(app, |this, cx| {
                                                            this.select_environment(
                                                                Some(id_clone.clone()),
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }),
                                            );
                                        }
                                        menu = menu.separator();
                                        let v_new = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("+ New Environment").on_click(
                                                move |_, window, app| {
                                                    v_new.update(app, |this, cx| {
                                                        this.create_environment(window, cx);
                                                    });
                                                },
                                            ),
                                        );
                                        let v_dot = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new("Import .env...").on_click(
                                                move |_, window, app| {
                                                    v_dot.update(app, |this, cx| {
                                                        this.import_dotenv_dialog(window, cx);
                                                    });
                                                },
                                            ),
                                        );
                                        if let Some(del_id) = active_env_id.clone() {
                                            let del_name = envs_snapshot
                                                .iter()
                                                .find(|(id, _)| id == &del_id)
                                                .map(|(_, name)| truncate_chars(name, 22))
                                                .unwrap_or_else(|| "Environment".to_string());
                                            let v_del = view.clone();
                                            menu = menu.separator();
                                            menu = menu.item(
                                                PopupMenuItem::element(move |_, _| {
                                                    h_flex()
                                                        .gap_1p5()
                                                        .items_center()
                                                        .text_color(solid(COLOR_DELETE))
                                                        .child(
                                                            Icon::new(IconName::Delete)
                                                                .size_3p5()
                                                                .text_color(solid(COLOR_DELETE)),
                                                        )
                                                        .child(SharedString::from(format!(
                                                            "Delete \"{del_name}\""
                                                        )))
                                                })
                                                .on_click(move |_, _, app| {
                                                    v_del.update(app, |this, cx| {
                                                        this.request_delete_environment(
                                                            &del_id, cx,
                                                        );
                                                    });
                                                }),
                                            );
                                        }
                                        menu
                                    })
                            })
                            .child(div().w(px(1.)).h(px(16.)).bg(solid(BORDER_SUBTLE)))
                            .child(render_window_controls(window)),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(div().flex_1().min_w_0().h_full().child(center_split))
                    .child(render_right_rail_and_drawer(self, window, cx)),
            );

        let main_area = div().flex_1().min_h_0().w_full().child(if show_sidebar {
            h_resizable("main-split")
                .with_handle_appearance(crisp_resize_handle_appearance())
                .child(
                    resizable_panel()
                        .size(px(260.))
                        .size_range(px(200.)..px(420.))
                        .fixed()
                        .child(render_sidebar(self, window, cx)),
                )
                .child(resizable_panel().child(center_and_rail))
                .into_any_element()
        } else {
            center_and_rail.into_any_element()
        });

        v_flex()
            .id("texreq-root")
            .key_context("TexReqWorkspace")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_send_action))
            .on_action(cx.listener(Self::on_new_tab_action))
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_focus_url_action))
            .on_action(cx.listener(Self::on_toggle_sidebar_action))
            .on_action(cx.listener(Self::on_focus_search_action))
            .on_action(cx.listener(Self::on_format_json_action))
            .on_action(cx.listener(Self::on_quit_action))
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .size_full()
            .bg(solid(BG_BASE))
            .text_color(solid(TEXT_PRIMARY))
            .child(main_area)
            .when(self.show_settings_modal, |root| {
                root.child(render_settings_modal(self, window, cx))
            })
            .when_some(self.pending_confirm.as_ref(), |root, action| {
                root.child(render_confirm_modal(action, cx))
            })
    }
}

fn render_window_controls(window: &Window) -> impl IntoElement {
    let max_icon = if window.is_maximized() {
        IconName::WindowRestore
    } else {
        IconName::WindowMaximize
    };

    h_flex()
        .h_full()
        .flex_shrink_0()
        .items_center()
        .child(window_control_btn(
            "win-min",
            IconName::WindowMinimize,
            WindowControlArea::Min,
            false,
        ))
        .child(window_control_btn(
            "win-max",
            max_icon,
            WindowControlArea::Max,
            false,
        ))
        .child(window_control_btn(
            "win-close",
            IconName::WindowClose,
            WindowControlArea::Close,
            true,
        ))
}

fn window_control_btn(
    id: &'static str,
    icon: IconName,
    area: WindowControlArea,
    is_close: bool,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(36.))
        .h_full()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(solid(TEXT_MUTED))
        .hover(move |s| {
            if is_close {
                s.bg(solid(COLOR_DELETE)).text_color(solid(0xffffff))
            } else {
                s.bg(solid(BG_HOVER)).text_color(solid(TEXT_PRIMARY))
            }
        })
        .window_control_area(area)
        .child(Icon::new(icon).small())
}

fn compute_folder_git_statuses(
    collections: &[CollectionFolder],
) -> HashMap<String, git::GitStatusInfo> {
    let mut map = HashMap::new();
    for folder in collections {
        if let Some(dir) = folder.disk_path.as_deref() {
            map.insert(folder.id.clone(), git::inspect_folder_repo(dir));
        }
    }
    map
}
