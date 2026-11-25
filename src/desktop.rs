use crate::{
    bridge::Request,
    web::{self, WindowState},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    thread,
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::{E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            LibraryLoader::GetModuleHandleW,
        },
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
};

thread_local! {
    static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
    static STATE: Rc<Cell<WindowState>> = Rc::default();
}

pub fn start() {
    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
        .map_err(webview2_com::Error::from)
        .and_then(|_| {
            let result = run();
            unsafe { CoUninitialize() };
            result
        });
    if let Err(error) = result {
        let message = HSTRING::from(format!(
            "无法打开会话编辑器，请检查 Microsoft Edge WebView2 运行时。\n{error}"
        ));
        unsafe {
            MessageBoxW(None, &message, w!("chatgpt对话助手"), MB_OK | MB_ICONERROR);
        }
    }
}

fn controller(
    hwnd: HWND,
    env: &ICoreWebView2Environment,
) -> webview2_com::Result<ICoreWebView2Controller> {
    let env = env.clone();
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2ControllerCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe { Ok(env.CreateCoreWebView2Controller(hwnd, &handler)?) }),
        Box::new(move |status, controller| {
            status?;
            let _ = tx.send(controller);
            Ok(())
        }),
    )?;
    rx.recv().ok().flatten().ok_or_else(|| E_POINTER.into())
}

fn environment() -> webview2_com::Result<ICoreWebView2Environment> {
    let path = dirs::data_local_dir()
        .ok_or_else(|| webview2_com::Error::CallbackError("无法定位应用数据目录".into()))?
        .join("chatgpt-dialog-assistant")
        .join("WebView2");
    let folder = HSTRING::from(path.as_os_str());
    let (tx, rx) = mpsc::channel();
    CreateCoreWebView2EnvironmentCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            Ok(CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                &folder,
                None,
                &handler,
            )?)
        }),
        Box::new(move |status, env| {
            status?;
            let _ = tx.send(env);
            Ok(())
        }),
    )?;
    rx.recv().ok().flatten().ok_or_else(|| E_POINTER.into())
}

fn make_window() -> windows::core::Result<HWND> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let class = w!("ChatGPTHistoryWindow");
        let window_class = WNDCLASSW {
            hInstance: instance,
            hIcon: LoadIconW(Some(instance), PCWSTR(1 as *const u16))?,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: class,
            lpfnWndProc: Some(window_proc),
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err(windows::core::Error::from_win32());
        }
        let mut work = RECT::default();
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut work as *mut RECT).cast()),
            Default::default(),
        )?;
        let dpi = GetDpiForSystem() as i32;
        let width = (1120 * dpi / 96).min(work.right - work.left);
        let height = (730 * dpi / 96).min(work.bottom - work.top);
        CreateWindowExW(
            Default::default(),
            class,
            w!("chatgpt对话助手"),
            WS_OVERLAPPEDWINDOW,
            work.left + (work.right - work.left - width) / 2,
            work.top + (work.bottom - work.top - height) / 2,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
    }
}

fn run() -> webview2_com::Result<()> {
    let hwnd = make_window()?;
    let env = environment()?;
    let controller = controller(hwnd, &env)?;
    let view = unsafe { controller.CoreWebView2()? };
    let (requests, jobs) = mpsc::channel::<Request>();
    let (results, replies) = mpsc::channel();
    // COM stays on the window thread; file and database work is serialized on the worker.
    let address = hwnd.0 as usize;
    let worker = thread::spawn(move || {
        for request in jobs {
            if results.send(request.respond()).is_err() {
                break;
            }
            unsafe {
                let _ = PostMessageW(Some(HWND(address as _)), WM_APP, WPARAM(0), LPARAM(0));
            }
        }
    });
    web::install(&view, env, requests, STATE.with(Rc::clone))?;
    CONTROLLER.with(|slot| slot.replace(Some(controller.clone())));
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        controller.SetBounds(rect)?;
        controller.SetIsVisible(true)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut message = MSG::default();
        loop {
            let status = GetMessageW(&mut message, None, 0, 0).0;
            if status == -1 {
                return Err(windows::core::Error::from_win32().into());
            }
            if status == 0 {
                break;
            }
            for reply in replies.try_iter() {
                view.PostWebMessageAsJson(&HSTRING::from(reply))?;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        CONTROLLER.with(|slot| slot.take());
        controller.Close()?;
    }
    drop(view);
    drop(controller);
    let _ = worker.join();
    Ok(())
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let controller = || CONTROLLER.with(|slot| slot.borrow().clone());
    match msg {
        WM_SIZE => {
            if let Some(view) = controller() {
                let mut rect = RECT::default();
                if GetClientRect(hwnd, &mut rect).is_ok() {
                    let _ = view.SetBounds(rect);
                }
            }
        }
        WM_MOVE => {
            if let Some(view) = controller() {
                let _ = view.NotifyParentWindowPositionChanged();
            }
        }
        WM_SETFOCUS => {
            if let Some(view) = controller() {
                let _ = view.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
        WM_GETMINMAXINFO => {
            let bounds = &mut *(lp.0 as *mut MINMAXINFO);
            let dpi = GetDpiForWindow(hwnd) as i32;
            bounds.ptMinTrackSize.x = 780 * dpi / 96;
            bounds.ptMinTrackSize.y = 500 * dpi / 96;
        }
        WM_DPICHANGED => {
            let rect = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
        }
        WM_CLOSE => {
            let state = STATE.with(|s| s.get());
            if state.busy {
                return LRESULT(0);
            }
            if state.dirty
                && MessageBoxW(
                    Some(hwnd),
                    w!("放弃未保存的编辑并退出？"),
                    w!("chatgpt对话助手"),
                    MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
                ) != IDYES
            {
                return LRESULT(0);
            }
            let _ = DestroyWindow(hwnd);
        }
        WM_DESTROY => PostQuitMessage(0),
        _ => return DefWindowProcW(hwnd, msg, wp, lp),
    }
    LRESULT(0)
}
