use gpui_kit::{
    App, AppContext as _, Context, Entity, Subscription, Window,
    component::input::{EditorState, InputEvent, InputState},
};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::models::{
    AuthConfig, BodyKind, HttpResponseData, RequestItem, RequestKind, StreamEntry, StreamStatus,
};
use crate::net::curl_parser::looks_like_curl;
use crate::net::ws_client::WsCommand;
use crate::ui::kv_editor::{UiCaptureRow, UiKvRow};
use crate::ui::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestSubTab {
    Params,
    Auth,
    Headers,
    Body,
    Captures,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSubTab {
    Pretty,
    Raw,
    Headers,
    Cookies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKindUi {
    None,
    Bearer,
    Basic,
    ApiKey,
}

impl AuthKindUi {
    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "No Auth",
            Self::Bearer => "Bearer",
            Self::Basic => "Basic",
            Self::ApiKey => "API Key",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKindUi {
    None,
    Json,
    GraphQl,
    FormUrlEncoded,
    Multipart,
    Raw,
    Binary,
}

impl BodyKindUi {
    pub const ALL: &'static [BodyKindUi] = &[
        Self::None,
        Self::Json,
        Self::GraphQl,
        Self::FormUrlEncoded,
        Self::Multipart,
        Self::Raw,
        Self::Binary,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Json => "JSON",
            Self::GraphQl => "GraphQL",
            Self::FormUrlEncoded => "Form",
            Self::Multipart => "Multipart",
            Self::Raw => "Raw",
            Self::Binary => "Binary",
        }
    }
}

pub struct TabSession {
    pub id: String,
    pub saved_request_id: Option<String>,
    pub dirty: bool,
    pub kind: RequestKind,
    pub req_subtab: RequestSubTab,
    pub res_subtab: ResponseSubTab,

    pub auth_kind: AuthKindUi,
    pub api_key_in_header: bool,
    pub body_kind: BodyKindUi,
    pub raw_content_type: String,
    pub accept_invalid_certs: bool,
    pub follow_redirects: bool,

    pub name_input: Entity<InputState>,
    pub url_input: Entity<InputState>,
    pub params: Vec<UiKvRow>,
    pub headers: Vec<UiKvRow>,
    pub form_rows: Vec<UiKvRow>,
    pub multipart_rows: Vec<UiKvRow>,
    pub captures: Vec<UiCaptureRow>,

    pub auth_bearer_input: Entity<InputState>,
    pub auth_user_input: Entity<InputState>,
    pub auth_pass_input: Entity<InputState>,
    pub auth_apikey_name_input: Entity<InputState>,
    pub auth_apikey_val_input: Entity<InputState>,

    pub body_json_editor: Entity<EditorState>,
    pub gql_query_editor: Entity<EditorState>,
    pub gql_vars_editor: Entity<EditorState>,
    pub raw_body_editor: Entity<EditorState>,
    pub binary_path_input: Entity<InputState>,

    pub proxy_input: Entity<InputState>,
    pub timeout_input: Entity<InputState>,

    pub ws_composer_editor: Entity<EditorState>,
    pub ws_templates: Vec<String>,
    pub stream_filter_input: Entity<InputState>,

    pub response_viewer: Entity<EditorState>,
    pub response_viewer_lang: String,
    pub response_raw_viewer: Entity<EditorState>,

    pub is_loading: bool,
    pub cancel_tx: Option<oneshot::Sender<()>>,
    pub ws_cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<WsCommand>>,
    pub stream_status: StreamStatus,
    pub stream_entries: Vec<StreamEntry>,
    pub last_response: Option<HttpResponseData>,
    pub last_error: Option<String>,

    pub ignore_next_url_change: bool,
    pub _subscriptions: Vec<Subscription>,
}

impl TabSession {
    pub fn from_request(
        item: &RequestItem,
        saved_request_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Self {
        let tab_id = Uuid::new_v4().to_string();

        let name_val = item.name.clone();
        let url_val = item.url.clone();
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Request name")
                .default_value(name_val)
        });
        let url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://api.example.com/v1/resource")
                .default_value(url_val)
        });

        let (auth_kind, bearer, user, pass, ak_name, ak_val, ak_in_header) = match &item.auth {
            AuthConfig::None => (AuthKindUi::None, "", "", "", "X-API-Key", "", true),
            AuthConfig::Bearer { token } => {
                (AuthKindUi::Bearer, token.as_str(), "", "", "X-API-Key", "", true)
            }
            AuthConfig::Basic { username, password } => (
                AuthKindUi::Basic,
                "",
                username.as_str(),
                password.as_str(),
                "X-API-Key",
                "",
                true,
            ),
            AuthConfig::ApiKey {
                key,
                value,
                in_header,
            } => (
                AuthKindUi::ApiKey,
                "",
                "",
                "",
                key.as_str(),
                value.as_str(),
                *in_header,
            ),
        };

        let b_str = bearer.to_string();
        let u_str = user.to_string();
        let p_str = pass.to_string();
        let akn_str = ak_name.to_string();
        let akv_str = ak_val.to_string();

        let auth_bearer_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("{{token}} or JWT...")
                .default_value(b_str)
        });
        let auth_user_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Username")
                .default_value(u_str)
        });
        let auth_pass_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .default_value(p_str)
        });
        let auth_apikey_name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Header or Query param")
                .default_value(akn_str)
        });
        let auth_apikey_val_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("API Key value")
                .default_value(akv_str)
        });

        let (
            body_kind,
            json_text,
            gql_q,
            gql_v,
            form_models,
            multi_models,
            raw_ct,
            raw_txt,
            bin_path,
        ) = match &item.body {
            BodyKind::None => (
                BodyKindUi::None,
                "{\n  \n}".to_string(),
                "query {\n  \n}".to_string(),
                "{}".to_string(),
                Vec::new(),
                Vec::new(),
                "text/plain".to_string(),
                String::new(),
                String::new(),
            ),
            BodyKind::Json(s) => (
                BodyKindUi::Json,
                s.clone(),
                "query {\n  \n}".to_string(),
                "{}".to_string(),
                Vec::new(),
                Vec::new(),
                "text/plain".to_string(),
                String::new(),
                String::new(),
            ),
            BodyKind::GraphQl { query, variables } => (
                BodyKindUi::GraphQl,
                "{\n  \n}".to_string(),
                query.clone(),
                variables.clone(),
                Vec::new(),
                Vec::new(),
                "text/plain".to_string(),
                String::new(),
                String::new(),
            ),
            BodyKind::FormUrlEncoded(rows) => (
                BodyKindUi::FormUrlEncoded,
                "{\n  \n}".to_string(),
                "query {\n  \n}".to_string(),
                "{}".to_string(),
                rows.clone(),
                Vec::new(),
                "text/plain".to_string(),
                String::new(),
                String::new(),
            ),
            BodyKind::Multipart(rows) => (
                BodyKindUi::Multipart,
                "{\n  \n}".to_string(),
                "query {\n  \n}".to_string(),
                "{}".to_string(),
                Vec::new(),
                rows.clone(),
                "text/plain".to_string(),
                String::new(),
                String::new(),
            ),
            BodyKind::Raw { content_type, text } => (
                BodyKindUi::Raw,
                "{\n  \n}".to_string(),
                "query {\n  \n}".to_string(),
                "{}".to_string(),
                Vec::new(),
                Vec::new(),
                content_type.clone(),
                text.clone(),
                String::new(),
            ),
            BodyKind::Binary { file_path } => (
                BodyKindUi::Binary,
                "{\n  \n}".to_string(),
                "query {\n  \n}".to_string(),
                "{}".to_string(),
                Vec::new(),
                Vec::new(),
                "text/plain".to_string(),
                String::new(),
                file_path.clone(),
            ),
        };

        let body_json_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
                .searchable(true)
                .default_value(json_text)
        });
        let gql_query_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("graphql")
                .line_number(true)
                .searchable(true)
                .default_value(gql_q)
        });
        let gql_vars_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
                .default_value(gql_v)
        });
        let raw_body_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .line_number(true)
                .searchable(true)
                .default_value(raw_txt)
        });
        let binary_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Binary file path...")
                .default_value(bin_path)
        });

        let proxy_val = item.network.proxy_url.clone();
        let timeout_val = item.network.timeout_secs.to_string();
        let proxy_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("e.g. http://127.0.0.1:8080 (TexMITM)")
                .default_value(proxy_val)
        });
        let timeout_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("30")
                .default_value(timeout_val)
        });

        let default_ws_msg = item
            .ws_templates
            .first()
            .cloned()
            .unwrap_or_else(|| "{\"ping\": true}".to_string());
        let ws_composer_editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
                .default_value(default_ws_msg)
        });
        let stream_filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter messages...")
        });

        let response_viewer = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("json")
                .line_number(true)
                .searchable(true)
                .default_value("")
        });
        let response_raw_viewer = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("text")
                .line_number(true)
                .searchable(true)
                .default_value("")
        });

        let params: Vec<UiKvRow> = item
            .params
            .iter()
            .map(|r| UiKvRow::from_model(r, window, cx))
            .collect();
        let headers: Vec<UiKvRow> = item
            .headers
            .iter()
            .map(|r| UiKvRow::from_model(r, window, cx))
            .collect();
        let form_rows: Vec<UiKvRow> = form_models
            .iter()
            .map(|r| UiKvRow::from_model(r, window, cx))
            .collect();
        let multipart_rows: Vec<UiKvRow> = multi_models
            .iter()
            .map(|r| UiKvRow::from_model(r, window, cx))
            .collect();
        let captures: Vec<UiCaptureRow> = item
            .captures
            .iter()
            .map(|c| UiCaptureRow::from_model(c, window, cx))
            .collect();

        let default_subtab = if item.kind == RequestKind::WebSocket {
            RequestSubTab::Body
        } else if !matches!(body_kind, BodyKindUi::None) {
            RequestSubTab::Body
        } else if !params.is_empty() {
            RequestSubTab::Params
        } else {
            RequestSubTab::Params
        };

        let tid_url = tab_id.clone();
        let sub_url = cx.subscribe_in(&url_input, window, move |this, state, ev: &InputEvent, window, cx| {
            match ev {
                InputEvent::Change => {
                    let val = state.read(cx).value().to_string();
                    let handled_curl = this.maybe_autoparse_curl_in_tab(&tid_url, &val, window, cx);
                    if !handled_curl {
                        this.mark_tab_dirty(&tid_url, cx);
                    }
                }
                InputEvent::PressEnter { .. } => {
                    if this.active_tab_id == tid_url {
                        this.trigger_primary_action(window, cx);
                    }
                }
                _ => {}
            }
        });

        let tid_name = tab_id.clone();
        let sub_name = cx.subscribe_in(&name_input, window, move |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                this.mark_tab_dirty(&tid_name, cx);
            }
        });

        Self {
            id: tab_id,
            saved_request_id,
            dirty: false,
            kind: item.kind,
            req_subtab: default_subtab,
            res_subtab: ResponseSubTab::Pretty,
            auth_kind,
            api_key_in_header: ak_in_header,
            body_kind,
            raw_content_type: raw_ct,
            accept_invalid_certs: item.network.accept_invalid_certs,
            follow_redirects: item.network.follow_redirects,
            name_input,
            url_input,
            params,
            headers,
            form_rows,
            multipart_rows,
            captures,
            auth_bearer_input,
            auth_user_input,
            auth_pass_input,
            auth_apikey_name_input,
            auth_apikey_val_input,
            body_json_editor,
            gql_query_editor,
            gql_vars_editor,
            raw_body_editor,
            binary_path_input,
            proxy_input,
            timeout_input,
            ws_composer_editor,
            ws_templates: item.ws_templates.clone(),
            stream_filter_input,
            response_viewer,
            response_viewer_lang: "json".to_string(),
            response_raw_viewer,
            is_loading: false,
            cancel_tx: None,
            ws_cmd_tx: None,
            stream_status: StreamStatus::Disconnected,
            stream_entries: Vec::new(),
            last_response: None,
            last_error: None,
            ignore_next_url_change: false,
            _subscriptions: vec![sub_url, sub_name],
        }
    }

    pub fn apply_request_model(&mut self, item: &RequestItem, window: &mut Window, cx: &mut App) {
        self.kind = item.kind;
        self.ignore_next_url_change = true;
        self.url_input
            .update(cx, |s, cx| s.set_value(item.url.clone(), window, cx));
        self.name_input
            .update(cx, |s, cx| s.set_value(item.name.clone(), window, cx));

        self.params = item
            .params
            .iter()
            .map(|r| UiKvRow::from_model(r, window, cx))
            .collect();
        self.headers = item
            .headers
            .iter()
            .map(|r| UiKvRow::from_model(r, window, cx))
            .collect();
        self.captures = item
            .captures
            .iter()
            .map(|c| UiCaptureRow::from_model(c, window, cx))
            .collect();

        match &item.auth {
            AuthConfig::None => self.auth_kind = AuthKindUi::None,
            AuthConfig::Bearer { token } => {
                self.auth_kind = AuthKindUi::Bearer;
                self.auth_bearer_input
                    .update(cx, |s, cx| s.set_value(token.clone(), window, cx));
            }
            AuthConfig::Basic { username, password } => {
                self.auth_kind = AuthKindUi::Basic;
                self.auth_user_input
                    .update(cx, |s, cx| s.set_value(username.clone(), window, cx));
                self.auth_pass_input
                    .update(cx, |s, cx| s.set_value(password.clone(), window, cx));
            }
            AuthConfig::ApiKey {
                key,
                value,
                in_header,
            } => {
                self.auth_kind = AuthKindUi::ApiKey;
                self.api_key_in_header = *in_header;
                self.auth_apikey_name_input
                    .update(cx, |s, cx| s.set_value(key.clone(), window, cx));
                self.auth_apikey_val_input
                    .update(cx, |s, cx| s.set_value(value.clone(), window, cx));
            }
        }

        match &item.body {
            BodyKind::None => self.body_kind = BodyKindUi::None,
            BodyKind::Json(txt) => {
                self.body_kind = BodyKindUi::Json;
                self.body_json_editor
                    .update(cx, |s, cx| s.set_value(txt.clone(), window, cx));
                self.req_subtab = RequestSubTab::Body;
            }
            BodyKind::GraphQl { query, variables } => {
                self.body_kind = BodyKindUi::GraphQl;
                self.gql_query_editor
                    .update(cx, |s, cx| s.set_value(query.clone(), window, cx));
                self.gql_vars_editor
                    .update(cx, |s, cx| s.set_value(variables.clone(), window, cx));
                self.req_subtab = RequestSubTab::Body;
            }
            BodyKind::FormUrlEncoded(rows) => {
                self.body_kind = BodyKindUi::FormUrlEncoded;
                self.form_rows = rows
                    .iter()
                    .map(|r| UiKvRow::from_model(r, window, cx))
                    .collect();
                self.req_subtab = RequestSubTab::Body;
            }
            BodyKind::Multipart(rows) => {
                self.body_kind = BodyKindUi::Multipart;
                self.multipart_rows = rows
                    .iter()
                    .map(|r| UiKvRow::from_model(r, window, cx))
                    .collect();
                self.req_subtab = RequestSubTab::Body;
            }
            BodyKind::Raw { content_type, text } => {
                self.body_kind = BodyKindUi::Raw;
                self.raw_content_type = content_type.clone();
                self.raw_body_editor
                    .update(cx, |s, cx| s.set_value(text.clone(), window, cx));
                self.req_subtab = RequestSubTab::Body;
            }
            BodyKind::Binary { file_path } => {
                self.body_kind = BodyKindUi::Binary;
                self.binary_path_input
                    .update(cx, |s, cx| s.set_value(file_path.clone(), window, cx));
                self.req_subtab = RequestSubTab::Body;
            }
        }

        self.accept_invalid_certs = item.network.accept_invalid_certs;
        self.follow_redirects = item.network.follow_redirects;
        self.proxy_input
            .update(cx, |s, cx| s.set_value(item.network.proxy_url.clone(), window, cx));
        self.timeout_input.update(cx, |s, cx| {
            s.set_value(item.network.timeout_secs.to_string(), window, cx)
        });
    }

    pub fn sync_to_model(&self, cx: &App) -> RequestItem {
        let auth = match self.auth_kind {
            AuthKindUi::None => AuthConfig::None,
            AuthKindUi::Bearer => AuthConfig::Bearer {
                token: self.auth_bearer_input.read(cx).value().to_string(),
            },
            AuthKindUi::Basic => AuthConfig::Basic {
                username: self.auth_user_input.read(cx).value().to_string(),
                password: self.auth_pass_input.read(cx).value().to_string(),
            },
            AuthKindUi::ApiKey => AuthConfig::ApiKey {
                key: self.auth_apikey_name_input.read(cx).value().to_string(),
                value: self.auth_apikey_val_input.read(cx).value().to_string(),
                in_header: self.api_key_in_header,
            },
        };

        let body = match self.body_kind {
            BodyKindUi::None => BodyKind::None,
            BodyKindUi::Json => BodyKind::Json(self.body_json_editor.read(cx).value().to_string()),
            BodyKindUi::GraphQl => BodyKind::GraphQl {
                query: self.gql_query_editor.read(cx).value().to_string(),
                variables: self.gql_vars_editor.read(cx).value().to_string(),
            },
            BodyKindUi::FormUrlEncoded => BodyKind::FormUrlEncoded(
                self.form_rows.iter().map(|r| r.to_model(cx)).collect(),
            ),
            BodyKindUi::Multipart => BodyKind::Multipart(
                self.multipart_rows.iter().map(|r| r.to_model(cx)).collect(),
            ),
            BodyKindUi::Raw => BodyKind::Raw {
                content_type: self.raw_content_type.clone(),
                text: self.raw_body_editor.read(cx).value().to_string(),
            },
            BodyKindUi::Binary => BodyKind::Binary {
                file_path: self.binary_path_input.read(cx).value().to_string(),
            },
        };

        let timeout_secs = self
            .timeout_input
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .unwrap_or(30)
            .max(1);

        let name = self.name_input.read(cx).value().trim().to_string();
        RequestItem {
            id: self
                .saved_request_id
                .clone()
                .unwrap_or_else(|| self.id.clone()),
            name: if name.is_empty() {
                "New Request".to_string()
            } else {
                name
            },
            kind: self.kind,
            url: self.url_input.read(cx).value().to_string(),
            params: self.params.iter().map(|r| r.to_model(cx)).collect(),
            headers: self.headers.iter().map(|r| r.to_model(cx)).collect(),
            auth,
            body,
            captures: self.captures.iter().map(|c| c.to_model(cx)).collect(),
            network: crate::models::NetworkOptions {
                proxy_url: self.proxy_input.read(cx).value().to_string(),
                accept_invalid_certs: self.accept_invalid_certs,
                follow_redirects: self.follow_redirects,
                timeout_secs,
            },
            ws_templates: self.ws_templates.clone(),
        }
    }

    pub fn update_response_viewers(
        &mut self,
        resp: &HttpResponseData,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.response_viewer_lang != resp.detected_language {
            let lang = resp.detected_language.clone();
            let text = resp.pretty_body.clone();
            self.response_viewer = cx.new(|cx| {
                EditorState::new(window, cx)
                    .language(lang.clone())
                    .line_number(true)
                    .searchable(true)
                    .default_value(text)
            });
            self.response_viewer_lang = lang;
        } else {
            let text = resp.pretty_body.clone();
            self.response_viewer
                .update(cx, |s, cx| s.set_value(text, window, cx));
        }

        let raw = resp.raw_body.clone();
        self.response_raw_viewer
            .update(cx, |s, cx| s.set_value(raw, window, cx));
    }

    pub fn tab_title(&self, cx: &App) -> String {
        let name = self.name_input.read(cx).value().trim().to_string();
        if !name.is_empty() && name != "New Request" {
            return name;
        }
        let url = self.url_input.read(cx).value().trim().to_string();
        if url.is_empty() {
            "New Request".to_string()
        } else {
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .or_else(|| url.strip_prefix("wss://"))
                .or_else(|| url.strip_prefix("ws://"))
                .unwrap_or(&url)
                .chars()
                .take(26)
                .collect()
        }
    }
}

pub fn should_autoparse_curl(input: &str) -> bool {
    looks_like_curl(input) && (input.contains("://") || input.contains(" -"))
}
