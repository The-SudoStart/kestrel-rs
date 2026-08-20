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

mod ui;

use servo::{
    RenderingContext, Servo, ServoBuilder, WebView, WebViewBuilder, WindowRenderingContext,
};
use url::Url;

use std::sync::Arc;

// ---------------------------------------------------------------------------
// User events
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum ServoWakeEvent {
    Wake,
    NewFrame,
}

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
        let _ = self.0.send_event(ServoWakeEvent::Wake);
    }
}

// ---------------------------------------------------------------------------
// WebView delegate
// ---------------------------------------------------------------------------

struct WebViewDelegate {
    proxy: winit::event_loop::EventLoopProxy<ServoWakeEvent>,
}

impl servo::WebViewDelegate for WebViewDelegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        let _ = self.proxy.send_event(ServoWakeEvent::NewFrame);
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
        dummy_window: Arc<Window>,
        servo: Servo,
        webview: WebView,
        rendering_context: Rc<WindowRenderingContext>,
        url_value: String,
        iced_integration: ui::IcedIntegration,
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

        // Create a dummy invisible window for Servo to own the EGL context
        // This avoids Wayland surface conflicts with WGPU on the main window.
        let dummy_window = event_loop
            .create_window(winit::window::WindowAttributes::default().with_visible(false))
            .expect("Create dummy window");
        let dummy_window = Arc::new(dummy_window);

        let display_handle = dummy_window.display_handle().unwrap();
        let window_handle = dummy_window.window_handle().unwrap();

        let rendering_context = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, window.inner_size())
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
        let mut webview = WebViewBuilder::new(&servo, rendering_context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(window.scale_factor() as f32))
            .delegate(Rc::new(WebViewDelegate { proxy: event_loop_proxy.clone() }))
            .build();

        event_loop.set_control_flow(ControlFlow::Wait);
        window.set_title(&format!("kestrel-rs — {initial_url}"));

        let mut iced_integration = pollster::block_on(ui::IcedIntegration::new(window.clone()));
        iced_integration.state.url_value = initial_url.to_string();

        *self = Self::Ready {
            window,
            dummy_window,
            servo,
            webview,
            rendering_context,
            url_value: initial_url.to_string(),
            iced_integration,
        };
    }

    fn user_event(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        event: ServoWakeEvent,
    ) {
        if let Self::Ready { servo, window, .. } = self {
            match event {
                ServoWakeEvent::Wake => {
                    servo.spin_event_loop();
                }
                ServoWakeEvent::NewFrame => {
                    window.request_redraw();
                }
            }
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
            dummy_window,
            servo,
            webview,
            rendering_context,
            url_value,
            iced_integration,
        } = self
        else {
            return;
        };

        // Let Servo process pending work
        servo.spin_event_loop();

        match &event {
            WindowEvent::RedrawRequested => {
                webview.paint();
                
                // Read pixels from Servo's GL framebuffer
                let physical_size = window.inner_size();
                let width = physical_size.width;
                let height = physical_size.height;
                
                if width > 0 && height > 0 {
                    let gl = rendering_context.glow_gl_api();
                    let mut pixels = vec![0u8; (width * height * 4) as usize];
                    unsafe {
                        use glow::HasContext;
                        gl.read_pixels(
                            0,
                            0,
                            width as i32,
                            height as i32,
                            glow::RGBA,
                            glow::UNSIGNED_BYTE,
                            glow::PixelPackData::Slice(Some(&mut pixels)),
                        );
                    }
                    
                    // OpenGL reads pixels bottom-up, we need to flip them vertically
                    let row_bytes = (width * 4) as usize;
                    for i in 0..(height as usize / 2) {
                        let top_idx = i * row_bytes;
                        let bot_idx = (height as usize - 1 - i) * row_bytes;
                        for j in 0..row_bytes {
                            pixels.swap(top_idx + j, bot_idx + j);
                        }
                    }
                    
                    // Update the image in the iced UI state
                    let handle = iced_widget::image::Handle::from_rgba(width, height, pixels);
                    iced_integration.state.servo_pixels = Some(handle);
                }
                
                // Present using Iced (wgpu)
                iced_integration.render(window);
            }

            WindowEvent::Resized(new_size) => {
                let _ = dummy_window.request_inner_size(*new_size);
                webview.resize(*new_size);
                rendering_context.resize(*new_size);
                iced_integration.resize(new_size.width, new_size.height);
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
                        iced_integration.state.url_value = url_value.clone();
                        window.request_redraw();
                    }
                    Key::Named(NamedKey::Backspace) => {
                        url_value.pop();
                        window.set_title(&format!("kestrel-rs — {url_value}"));
                        iced_integration.state.url_value = url_value.clone();
                        window.request_redraw();
                    }
                    Key::Named(NamedKey::Escape) => {
                        url_value.clear();
                        window.set_title("kestrel-rs");
                        iced_integration.state.url_value = url_value.clone();
                        window.request_redraw();
                    }
                    Key::Character(c) => {
                        url_value.push_str(c.as_str());
                        window.set_title(&format!("kestrel-rs — {url_value}"));
                        iced_integration.state.url_value = url_value.clone();
                        window.request_redraw();
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
