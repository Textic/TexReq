#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mcp;
mod models;
mod net;
mod storage;
mod theme;
mod ui;

use std::borrow::Cow;

use gpui_kit::{assets::Assets, *};

use crate::ui::workspace::{
    FocusSearch, FocusUrl, FormatJson, NewTab, Quit, SaveRequest, SendRequest, ToggleSidebar,
    Workspace,
};

struct TexReqAssets;

impl AssetSource for TexReqAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if path == "icons/texreq.png" {
            return Ok(Some(Cow::Borrowed(include_bytes!("../assets/texreq.png"))));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        Assets.list(path)
    }
}

fn main() {
    if std::env::args().skip(1).any(|arg| arg == "--mcp" || arg == "mcp") {
        if let Err(err) = mcp::server::run_stdio_server() {
            eprintln!("TexReq MCP server error: {err}");
            std::process::exit(1);
        }
        return;
    }

    gpui_kit::application().with_assets(TexReqAssets).run(|cx| {
        gpui_kit::init(cx);
        theme::apply_flat_dark_theme(cx);

        cx.bind_keys([
            KeyBinding::new("ctrl-enter", SendRequest, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-n", NewTab, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-s", SaveRequest, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-l", FocusUrl, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-b", ToggleSidebar, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-k", FocusSearch, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-shift-f", FormatJson, Some("TexReqWorkspace")),
            KeyBinding::new("ctrl-q", Quit, Some("TexReqWorkspace")),
        ]);

        let bounds = WindowBounds::centered(size(px(1360.), px(860.)), cx);
        let options = WindowOptions {
            window_bounds: Some(bounds),
            titlebar: Some(TitlebarOptions {
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(px(9.), px(9.))),
            }),
            app_owns_titlebar_drag: true,
            ..Default::default()
        };

        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(window, cx))
        })
        .expect("failed to open TexReq window");
    });
}
