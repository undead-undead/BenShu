use async_trait::async_trait;
use benshu_infra::{SafetyLevel, Tool, ToolDefinition};
use benshu_sensory::SensoryHub;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::task;

// ==================== 统一数据实体 ====================
#[derive(Debug, Serialize, Clone)]
pub struct WindowInfo {
    pub title: String,
    pub owner: String,
    pub is_active: bool,
    pub id: Option<u64>,
}

// ==================== Windows 实现（内存安全+稳定） ====================
#[cfg(target_os = "windows")]
mod win_impl {
    use super::WindowInfo;
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowTextW, IsWindowVisible,
    };

    struct EnumContext {
        windows: Vec<WindowInfo>,
        active_hwnd: HWND,
    }

    pub fn get_active_info() -> Option<WindowInfo> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0 == 0 {
            return None;
        }

        let mut buf = [0u16; 512];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };

        Some(WindowInfo {
            title: if len > 0 {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                "Unknown Window".to_string()
            },
            owner: "Windows Application".to_string(),
            is_active: true,
            id: Some(hwnd.0 as u64),
        })
    }

    pub fn list_windows() -> Vec<WindowInfo> {
        let active_hwnd = unsafe { GetForegroundWindow() };
        let mut ctx = EnumContext {
            windows: Vec::new(),
            active_hwnd,
        };

        let ctx_ptr = &mut ctx as *mut EnumContext;
        unsafe {
            let _ = EnumWindows(Some(enum_callback), LPARAM(ctx_ptr as isize));
        }

        ctx.windows
            .into_iter()
            .filter(|w| !w.title.trim().is_empty())
            .collect()
    }

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if lparam.0 == 0 {
            return BOOL(1);
        }

        let ctx = &mut *(lparam.0 as *mut EnumContext);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }

        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len > 0 {
            ctx.windows.push(WindowInfo {
                title: String::from_utf16_lossy(&buf[..len as usize]),
                owner: "Windows Process".to_string(),
                is_active: hwnd == ctx.active_hwnd,
                id: Some(hwnd.0 as u64),
            });
        }

        BOOL(1)
    }
}

// ==================== macOS 实现（精准匹配+功能完整） ====================
#[cfg(target_os = "macos")]
mod mac_impl {
    use super::WindowInfo;
    use core_foundation::{
        array::CFArray, base::TCFType, dictionary::CFDictionary, number::CFNumber, string::CFString,
    };
    use core_graphics::window::{
        kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
        CGWindowListCopyWindowInfo,
    };

    const KEY_WINDOW_NAME: &str = "kCGWindowName";
    const KEY_OWNER_NAME: &str = "kCGWindowOwnerName";
    const KEY_WINDOW_NUMBER: &str = "kCGWindowNumber";
    const KEY_LAYER: &str = "kCGWindowLayer";
    const KEY_IS_ONSCREEN: &str = "kCGWindowIsOnscreen";

    pub fn get_active_info() -> Option<WindowInfo> {
        let all_windows = list_all();
        all_windows.into_iter().find(|w| w.is_active)
    }

    pub fn list_all() -> Vec<WindowInfo> {
        unsafe {
            let list_ref = CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
                kCGNullWindowID,
            );
            if list_ref.is_null() {
                return vec![];
            }

            let array = CFArray::<CFDictionary>::wrap_under_create_rule(list_ref);
            let mut results = Vec::new();
            let mut active_idx_in_results = None;

            for dict in array.iter() {
                let layer = get_dict_i64(&dict, KEY_LAYER).unwrap_or(-1);
                if layer != 0 {
                    continue;
                }

                let is_onscreen = get_dict_bool(&dict, KEY_IS_ONSCREEN).unwrap_or(true);
                if !is_onscreen {
                    continue;
                }

                let owner = get_dict_string(&dict, KEY_OWNER_NAME)
                    .unwrap_or_else(|| "Unknown App".to_string());
                let title = get_dict_string(&dict, KEY_WINDOW_NAME).unwrap_or_default();
                let id = get_dict_i64(&dict, KEY_WINDOW_NUMBER).map(|v| v as u64);

                let win = WindowInfo {
                    title: if title.is_empty() {
                        "Untitled Window".to_string()
                    } else {
                        title
                    },
                    owner,
                    is_active: false,
                    id,
                };

                if active_idx_in_results.is_none() {
                    active_idx_in_results = Some(results.len());
                }
                results.push(win);
            }

            if let Some(idx) = active_idx_in_results {
                if let Some(win) = results.get_mut(idx) {
                    win.is_active = true;
                }
            }

            results
        }
    }

    fn get_dict_string(dict: &CFDictionary, key: &str) -> Option<String> {
        unsafe {
            let cf_key = CFString::from_static_string(key);
            dict.find(cf_key.as_CFTypeRef().cast())
                .map(|v| CFString::wrap_under_get_rule(v.cast()).to_string())
        }
    }

    fn get_dict_i64(dict: &CFDictionary, key: &str) -> Option<i64> {
        unsafe {
            let cf_key = CFString::from_static_string(key);
            dict.find(cf_key.as_CFTypeRef().cast())
                .and_then(|v| CFNumber::wrap_under_get_rule(v.cast()).to_i64())
        }
    }

    fn get_dict_bool(dict: &CFDictionary, key: &str) -> Option<bool> {
        unsafe {
            let cf_key = CFString::from_static_string(key);
            dict.find(cf_key.as_CFTypeRef().cast())
                .map(|v| v != core_foundation::base::kCFBooleanFalse.cast())
        }
    }
}

// ==================== Linux 实现（内存安全+精准） ====================
#[cfg(target_os = "linux")]
mod linux_impl {
    use super::WindowInfo;
    use std::ffi::CStr;
    use std::os::raw::c_ulong;
    use std::ptr;
    use x11_dl::xlib::{Display, Window, Xlib};

    pub fn get_active_info() -> Option<WindowInfo> {
        let xlib = Xlib::open().ok()?;
        unsafe {
            let display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return None;
            }

            let root = (xlib.XDefaultRootWindow)(display);
            let active_id = fetch_active_id(&xlib, display, root);

            let mut info = None;
            if active_id != 0 {
                if let Some(title) = fetch_window_title(&xlib, display, active_id) {
                    info = Some(WindowInfo {
                        title,
                        owner: "X11 Application".to_string(),
                        is_active: true,
                        id: Some(active_id as u64),
                    });
                }
            }

            (xlib.XCloseDisplay)(display);
            info
        }
    }

    pub fn list_windows() -> Vec<WindowInfo> {
        let xlib = match Xlib::open() {
            Ok(x) => x,
            Err(_) => return vec![],
        };

        unsafe {
            let display = (xlib.XOpenDisplay)(ptr::null());
            if display.is_null() {
                return vec![];
            }

            let root = (xlib.XDefaultRootWindow)(display);
            let active_id = fetch_active_id(&xlib, display, root);

            let mut root_ret = 0;
            let mut parent_ret = 0;
            let mut children = ptr::null_mut::<c_ulong>();
            let mut nchildren = 0;
            let mut result = Vec::new();

            if (xlib.XQueryTree)(
                display,
                root,
                &mut root_ret,
                &mut parent_ret,
                &mut children,
                &mut nchildren,
            ) != 0
                && !children.is_null()
            {
                for i in 0..nchildren {
                    let win = *children.add(i as usize);

                    let mut attr = std::mem::zeroed();
                    if (xlib.XGetWindowAttributes)(display, win, &mut attr) != 0
                        && attr.map_state == 2
                    {
                        if let Some(title) = fetch_window_title(&xlib, display, win) {
                            result.push(WindowInfo {
                                title,
                                owner: "Linux App".to_string(),
                                is_active: win == active_id,
                                id: Some(win as u64),
                            });
                        }
                    }
                }
                (xlib.XFree)(children as *mut _);
            }

            (xlib.XCloseDisplay)(display);
            result
        }
    }

    unsafe fn fetch_active_id(xlib: &Xlib, display: *mut Display, root: Window) -> Window {
        let atom = (xlib.XInternAtom)(display, b"_NET_ACTIVE_WINDOW\0".as_ptr() as _, 0);
        let mut prop = ptr::null_mut::<u8>();
        let mut nitems = 0;
        let mut actual_type = 0;
        let mut actual_format = 0;
        let mut bytes_after = 0;

        // XA_WINDOW is constant 33
        const XA_WINDOW: c_ulong = 33;

        let result = (xlib.XGetWindowProperty)(
            display,
            root,
            atom,
            0,
            1,
            0,
            XA_WINDOW,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );

        let mut active_win = 0;
        if result == 0 && !prop.is_null() && nitems > 0 {
            active_win = *(prop as *const Window);
            (xlib.XFree)(prop as *mut _);
        }

        active_win
    }

    unsafe fn fetch_window_title(
        xlib: &Xlib,
        display: *mut Display,
        win: Window,
    ) -> Option<String> {
        let mut name = ptr::null_mut();
        let result = (xlib.XFetchName)(display, win, &mut name);

        if result != 0 && !name.is_null() {
            let title = CStr::from_ptr(name).to_string_lossy().into_owned();
            (xlib.XFree)(name as *mut _);
            if !title.trim().is_empty() {
                return Some(title);
            }
        }

        None
    }
}

// ==================== 工具核心 ====================
pub struct DesktopSenseTool {
    pub sensory: Arc<SensoryHub>,
}

#[derive(Debug, Deserialize)]
struct DesktopArgs {
    action: String,
}

#[async_trait]
impl Tool for DesktopSenseTool {
    fn name(&self) -> String {
        "desktop_sense".to_string()
    }

    async fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Production-grade native desktop awareness tool. Detects windows and focus state across Windows/macOS/Linux.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list_windows", "get_active"],
                        "description": "Action to perform: list visible windows or get the currently active window"
                    }
                },
                "required": ["action"]
            }),
            parameters_ts: None,
            is_binary: false,
            is_verified: true,
            usage_guidelines: Some("Use to detect current active applications and orient agent workflow.".to_string()),
            safety_level: SafetyLevel::Yellow,
        }
    }

    async fn call(&self, arguments: &str) -> anyhow::Result<String> {
        let args: DesktopArgs = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("Failed to parse arguments: {}", e))?;

        let current_os = std::env::consts::OS.to_string();

        let join_result = task::spawn_blocking(move || match args.action.as_str() {
            "list_windows" => {
                #[cfg(target_os = "windows")]
                {
                    Ok(win_impl::list_windows())
                }
                #[cfg(target_os = "macos")]
                {
                    Ok(mac_impl::list_all())
                }
                #[cfg(target_os = "linux")]
                {
                    Ok(linux_impl::list_windows())
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                {
                    Err(anyhow::anyhow!("Unsupported OS: {}", current_os))
                }
            }
            "get_active" => {
                #[cfg(target_os = "windows")]
                {
                    Ok(win_impl::get_active_info().into_iter().collect::<Vec<_>>())
                }
                #[cfg(target_os = "macos")]
                {
                    Ok(mac_impl::get_active_info().into_iter().collect::<Vec<_>>())
                }
                #[cfg(target_os = "linux")]
                {
                    Ok(linux_impl::get_active_info()
                        .into_iter()
                        .collect::<Vec<_>>())
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                {
                    Err(anyhow::anyhow!("Unsupported OS: {}", current_os))
                }
            }
            other => Err(anyhow::anyhow!("Unknown action '{}'", other)),
        })
        .await;

        let result = match join_result {
            Ok(inner_res) => inner_res?,
            Err(e) => return Err(anyhow::anyhow!("Desktop sensing task failed: {}", e)),
        };

        if result.is_empty() {
            return Ok(format!("No visible windows detected on {}.", current_os));
        }

        let mut output = format!("### Desktop Observation (OS: {}) ###\n", current_os);
        for window in result {
            let focus_tag = if window.is_active {
                " **[ACTIVE FOCUS]**"
            } else {
                ""
            };
            let window_id = window.id.unwrap_or(0);
            output.push_str(&format!(
                "- [{}] **{}** (ID: {}){}\n",
                window.owner, window.title, window_id, focus_tag
            ));
        }

        Ok(output)
    }
}
