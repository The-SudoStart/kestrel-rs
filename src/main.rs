// kestrel-rs — Servo WebView with keyboard-driven URL navigation.
//
// Architecture:
//   1. winit owns the window and event loop (our custom ApplicationHandler)
//   2. Servo renders web content via WindowRenderingContext (OpenGL/WebRender)
//   3. Raw keyboard input drives URL navigation (type URL, press Enter)
//   4. Current URL shown in window title as visual feedback
//
// Why no Iced rendering: Servo's GL present() and wgpu's frame.present()
// both write to the same window surface. Presenting both causes flickering.
// The URL bar is functional through raw keyboard capture but not visually
// rendered on screen. See docs/servo-embedding-notes.md for the pixel
// readback approach needed to solve this.

use std::error::Error;
use std::rc::Rc;

use winit::event::WindowEvent;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use servo::{
    RenderingContext, Servo, ServoBuilder, WebView, WebViewBuilder, WindowRenderingContext,
};
use url::Url;

use std::sync::Arc;

// ---------------------------------------------------------------------------
// User events
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ServoWakeEvent;

// ---------------------------------------------------------------------------
// Servo event loop waker
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ServoWaker(winit::event_loop::EventLoopProxy<ServoWakeEvent>);

impl servo::EventLoopWaker for ServoWaker {
    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.0.send_event(ServoWakeEvent);
    }
}

// ---------------------------------------------------------------------------
// WebView delegate
// ---------------------------------------------------------------------------

struct WebViewDelegate;

impl servo::WebViewDelegate for WebViewDelegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        // Redraw is handled by our winit event loop
    }
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

enum Runner {
    Loading {
        event_loop_proxy: winit::event_loop::EventLoopProxy<ServoWakeEvent>,
    },
    Ready {
        window: Arc<Window>,
        servo: Servo,
        webview: WebView,
        rendering_context: Rc<WindowRenderingContext>,
        url_value: String,
    },
}

impl winit::application::ApplicationHandler<ServoWakeEvent> for Runner {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Self::Loading { event_loop_proxy } = self else {
            return;
        };

        // TLS provider (required by Servo networking)
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // Window
        let window = Arc::new(
            event_loop
                .create_window(winit::window::WindowAttributes::default())
                .expect("Create window"),
        );
        let physical_size = window.inner_size();

        // Servo rendering context
        let display_handle = event_loop
            .display_handle()
            .expect("Failed to get display handle");
        let window_handle = window.window_handle().expect("Failed to get window handle");

        let rendering_context = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, physical_size)
                .expect("Could not create WindowRenderingContext"),
        );
        let _ = rendering_context.make_current();

        // Servo engine
        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(ServoWaker(event_loop_proxy.clone())))
            .preferences(servo::Preferences {
                network_use_webpki_roots: true,
                ..servo::Preferences::default()
            })
            .build();
        servo.setup_logging();

        // Initial page
        let initial_url = "https://servo.org";
        let url = Url::parse(initial_url).expect("valid URL");
        let webview = WebViewBuilder::new(&servo, rendering_context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(window.scale_factor() as f32))            .delegate(Rc::new(WebViewDelegate))
                .build();

        event_loop.set_control_flow(ControlFlow::Wait);
        window.set_title(&format!("kestrel-rs — {initial_url}"));

        *self = Self::Ready {
            window,
            servo,
            webview,
            rendering_context,
            url_value: initial_url.to_string(),
        };
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        _event: ServoWakeEvent,
    ) {
        if let Self::Ready { servo, .. } = self {
            servo.spin_event_loop();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Self::Ready {
            window,
            servo,
            webview,
            rendering_context,
            url_value,
        } = self
        else {
            return;
        };

        // Let Servo process pending work
        servo.spin_event_loop();

        match &event {
            WindowEvent::RedrawRequested => {
                webview.paint();
                rendering_context.present();
            }

            WindowEvent::Resized(new_size) => {
                rendering_context.resize(*new_size);
            }

            // ---- Keyboard input: raw capture for URL bar ----
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                match &event.logical_key {
                    Key::Named(NamedKey::Enter) => {
                        if url_value.is_empty() {
                            return;
                        }
                        match Url::parse(url_value) {
                            Ok(url) => {
                                eprintln!("kestrel-rs: navigating to {url}");
                                webview.load(url);
                                servo.spin_event_loop();
                            }
                            Err(_) => {
                                // Try prepending https://
                                let with_scheme = format!("https://{url_value}");
                                if let Ok(url) = Url::parse(&with_scheme) {
                                    eprintln!("kestrel-rs: navigating to {url}");
                                    url_value.clone_from(&with_scheme);
                                    webview.load(url);
                                    servo.spin_event_loop();
                                } else {
                                    eprintln!("kestrel-rs: invalid URL: {url_value}");
                                }
                            }
                        }
                        window.set_title(&format!("kestrel-rs — {url_value}"));
                    }
                    Key::Named(NamedKey::Backspace) => {
                        url_value.pop();
                        window.set_title(&format!("kestrel-rs — {url_value}"));
                    }
                    Key::Named(NamedKey::Escape) => {
                        url_value.clear();
                        window.set_title("kestrel-rs");
                    }
                    Key::Character(c) => {
                        url_value.push_str(c.as_str());
                        window.set_title(&format!("kestrel-rs — {url_value}"));
                    }
                    _ => {}
                }
            }

            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::<ServoWakeEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let mut runner = Runner::Loading {
        event_loop_proxy: proxy,
    };
    event_loop.run_app(&mut runner)?;
    Ok(())
}
