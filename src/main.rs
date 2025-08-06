use eframe::egui;

mod app;
mod ime;
mod terminal;
mod utils;

use app::TerminalApp;

// macOS 전용 둥근 창 설정
#[cfg(target_os = "macos")]
fn round_window_corners(ns_view: *mut std::os::raw::c_void) {
    use cocoa::appkit::NSView;
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSAutoreleasePool;
    use objc::runtime::YES;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        let pool = NSAutoreleasePool::new(nil);

        let view: id = ns_view as id;

        // view에 layer 생성
        let _: () = msg_send![view, setWantsLayer: YES];

        // layer 가져오기
        let layer: id = msg_send![view, layer];

        // 둥근 모서리 설정
        let _: () = msg_send![layer, setMasksToBounds: YES];
        let _: () = msg_send![layer, setCornerRadius: 12.0f64];

        pool.drain();
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_decorations(false)
            .with_resizable(true) // Make window resizable
            .with_transparent(true) // Enable transparency
            .with_window_level(egui::WindowLevel::Normal)
            .with_title("WTerm - 터미널"), // Window title
        ..Default::default()
    };

    let _result = eframe::run_native(
        "WTerm",
        options,
        Box::new(|cc| {
            // macOS에서 윈도우를 둥글게 만들기
            #[cfg(target_os = "macos")]
            {
                use raw_window_handle::{HasRawWindowHandle, HasWindowHandle};

                if let Ok(window_handle) = cc.window_handle() {
                    if let Ok(raw_handle) = window_handle.raw_window_handle() {
                        if let raw_window_handle::RawWindowHandle::AppKit(handle) = raw_handle {
                            round_window_corners(
                                handle.ns_view.as_ptr() as *mut std::os::raw::c_void
                            );
                        }
                    }
                }
            }

            Ok(Box::new(
                TerminalApp::new(cc).expect("Failed to create terminal app"),
            ))
        }),
    );
}
