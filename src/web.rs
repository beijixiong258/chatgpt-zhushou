use crate::bridge::Request;
use serde::Deserialize;
use std::{cell::Cell, rc::Rc, sync::mpsc::Sender};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    core::{w, HSTRING, PWSTR},
    Win32::{Foundation::E_POINTER, UI::Shell::SHCreateMemStream},
};

const HOME: &str = "https://dialog.localhost/";
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Copy, Clone, Default, Deserialize)]
pub struct WindowState {
    pub dirty: bool,
    pub busy: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Incoming {
    State {
        #[serde(rename = "windowState")]
        state: WindowState,
    },
    Request(Request),
}

pub fn install(
    view: &ICoreWebView2,
    env: ICoreWebView2Environment,
    requests: Sender<Request>,
    state: Rc<Cell<WindowState>>,
) -> windows::core::Result<()> {
    let mut token = 0;
    unsafe {
        view.Settings()?.SetAreHostObjectsAllowed(false)?;
        view.Settings()?
            .SetAreDevToolsEnabled(cfg!(debug_assertions))?;
        view.Settings()?.SetAreDefaultContextMenusEnabled(false)?;
        view.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    args.SetCancel(CoTaskMemPWSTR::from(uri).to_string() != HOME)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.AddWebResourceRequestedFilter(
            w!("https://dialog.localhost/*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )?;
        view.add_WebResourceRequested(&WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
            let Some(args) = args else { return Ok(()) };
            let mut uri = PWSTR::null();
            args.Request()?.Uri(&mut uri)?;
            let uri = CoTaskMemPWSTR::from(uri).to_string();
            let (bytes, mime, status): (&[u8], _, _) = match uri.strip_prefix(HOME) {
                Some("") => (include_bytes!("../dist/index.html"), "text/html; charset=utf-8", 200),
                Some("app.js") => (include_bytes!("../dist/app.js"), "text/javascript; charset=utf-8", 200),
                Some("index.css") => (include_bytes!("../dist/index.css"), "text/css; charset=utf-8", 200),
                Some("white-dragon.jpg") => (include_bytes!("../dist/white-dragon.jpg"), "image/jpeg", 200),
                Some("128x128.png") => (include_bytes!("../dist/128x128.png"), "image/png", 200),
                _ => (b"Not found", "text/plain", 404),
            };
            let stream = SHCreateMemStream(Some(bytes)).ok_or(E_POINTER)?;
            let headers = HSTRING::from(format!("Content-Type: {mime}\r\nContent-Security-Policy: {CSP}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store"));
            let reason = if status == 200 { w!("OK") } else { w!("Not Found") };
            args.SetResponse(&env.CreateWebResourceResponse(&stream, status, reason, &headers)?)
        })), &mut token)?;
        view.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut source = PWSTR::null();
                args.Source(&mut source)?;
                if CoTaskMemPWSTR::from(source).to_string() != HOME {
                    return Ok(());
                }
                let mut message = PWSTR::null();
                args.WebMessageAsJson(&mut message)?;
                match serde_json::from_str(&CoTaskMemPWSTR::from(message).to_string()) {
                    Ok(Incoming::Request(request)) => {
                        let _ = requests.send(request);
                    }
                    Ok(Incoming::State { state: value }) => state.set(value),
                    Err(_) => (),
                }
                Ok(())
            })),
            &mut token,
        )?;
        view.Navigate(&HSTRING::from(HOME))
    }
}
