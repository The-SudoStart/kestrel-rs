// kestrel-rs — Iced URL bar composited into the Servo WebView window.
//
// Issue #1: Prove Iced UI and Servo content can coexist in the same window
// and communicate through a custom ApplicationHandler.
//
// Architecture:
//   1. winit owns the window and event loop (our custom ApplicationHandler)
//   2. Servo renders web content via WindowRenderingContext (OpenGL/WebRender)
//   3. wgpu provides a rendering surface for Iced UI overlay
//   4. Iced's UserInterface renders the URL bar (text_input widget)
//   5. Both coexist on the same window — Servo GL + wgpu/Vulkan
//
// Key learning: Servo's OffscreenRenderingContext requires a parent
// WindowRenderingContext, so Servo MUST own a GL context on the window.
// We test whether wgpu (Vulkan) can coexist alongside Servo's GL on the
// same Wayland window.

use std::error::Error;
use std::mem;
use std::rc::Rc;

use iced_wgpu::graphics::{Shell as WgpuShell, Viewport};
use iced_wgpu::{Engine, Renderer, wgpu};
use iced_winit::conversion;
use iced_winit::core::mouse;
use iced_winit::core::{Element, Event, Size, Theme};
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;
use winit::event::WindowEvent;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use servo::{
    Preferences, RenderingContext, Servo, ServoBuilder, WebView, WebViewBuilder,
    WindowRenderingContext,
};
use url::Url;

use std::sync::Arc;

// ---------------------------------------------------------------------------
// User events
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ServoWakeEvent;

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    UrlBarChanged(String),
    UrlBarSubmitted,
}

// ---------------------------------------------------------------------------
// Null clipboard (Iced requires a Clipboard impl)
// ---------------------------------------------------------------------------

struct NullClipboard;

impl iced_winit::core::Clipboard for NullClipboard {
    fn read(&self, _kind: iced_winit::core::clipboard::Kind) -> Option<String> {
        None
    }
    fn write(&mut self, _kind: iced_winit::core::clipboard::Kind, _contents: String) {}
}

// ---------------------------------------------------------------------------
// URL bar widget
// ---------------------------------------------------------------------------

fn url_bar<'a>(url_value: &'a str) -> Element<'a, Message, Theme, Renderer> {
    iced_widget::text_input("Enter URL…", url_value)
        .on_submit(Message::UrlBarSubmitted)
        .on_input(Message::UrlBarChanged)
        .width(iced_winit::core::Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Servo WebView delegate (minimal — just handle new frame notifications)
// ---------------------------------------------------------------------------

struct WebViewDelegate;

impl servo::WebViewDelegate for WebViewDelegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        // Redraw is handled by our winit event loop
    }
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
        let _ = self.0.send_event(ServoWakeEvent);
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
        // -- winit / wgpu --
        window: Arc<Window>,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: Box<wgpu::Surface<'static>>,
        format: wgpu::TextureFormat,
        // -- Iced --
        renderer: Box<Renderer>,
        viewport: Box<Viewport>,
        cursor: mouse::Cursor,
        modifiers: ModifiersState,
        cache: user_interface::Cache,
        events: Vec<Event>,
        resized: bool,
        // -- Application state --
        url_value: String,
        // -- Servo --
        servo: Servo,
        webview: WebView,
        rendering_context: Rc<WindowRenderingContext>,
    },
}

impl winit::application::ApplicationHandler<ServoWakeEvent> for Runner {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let Self::Loading { event_loop_proxy } = self else {
            return;
        };

        // ---- TLS (required by Servo networking) ----
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

        // ---- winit window ----
        let window = Arc::new(
            event_loop
                .create_window(winit::window::WindowAttributes::default())
                .expect("Create window"),
        );
        let physical_size = window.inner_size();

        // ---- Servo (WindowRenderingContext on the window) ----
        let display_handle = event_loop
            .display_handle()
            .expect("Failed to get display handle");
        let window_handle = window.window_handle().expect("Failed to get window handle");

        let rendering_context = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, physical_size)
                .expect("Could not create WindowRenderingContext"),
        );
        let _ = rendering_context.make_current();            // Use WebPKI roots instead of platform verifier to avoid
            // CaUsedAsEndEntity errors on some systems.
            let prefs = Preferences {
                network_use_webpki_roots: true,
                ..Preferences::default()
            };

            let servo = ServoBuilder::default()
                .event_loop_waker(Box::new(ServoWaker(event_loop_proxy.clone())))
                .preferences(prefs)
                .build();
        servo.setup_logging();            let url = Url::parse("https://servo.org").expect("valid URL");
        let webview = WebViewBuilder::new(&servo, rendering_context.clone())
            .url(url)                .hidpi_scale_factor(euclid::Scale::new(window.scale_factor() as f32))
                .delegate(Rc::new(WebViewDelegate))
            .build();

        // ---- wgpu surface (for Iced UI overlay) ----
        let viewport = Viewport::with_physical_size(
            Size::new(physical_size.width, physical_size.height),
            window.scale_factor() as f32,
        );

        let backend = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::VULKAN);
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: backend,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("Create wgpu surface");

        let (format, _adapter, device, queue) =
            futures::executor::block_on(async {
                let adapter = wgpu::util::initialize_adapter_from_env_or_default(
                    &instance,
                    Some(&surface),
                )
                .await
                .expect("Create adapter");
                let adapter_features = adapter.features();
                let capabilities = surface.get_capabilities(&adapter);
                let (device, queue) = adapter
                    .request_device(&wgpu::DeviceDescriptor {
                        label: None,
                        required_features: adapter_features & wgpu::Features::default(),
                        required_limits: wgpu::Limits::default(),
                        memory_hints: wgpu::MemoryHints::MemoryUsage,
                        trace: wgpu::Trace::Off,
                        experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    })
                    .await
                    .expect("Request device");
                (
                    capabilities
                        .formats
                        .iter()
                        .copied()
                        .find(wgpu::TextureFormat::is_srgb)
                        .or_else(|| capabilities.formats.first().copied())
                        .expect("Get preferred format"),
                    adapter,
                    device,
                    queue,
                )
            });

        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: physical_size.width,
                height: physical_size.height,
                present_mode: wgpu::PresentMode::AutoVsync,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );

        // ---- Iced renderer ----
        let renderer = {
            let engine = Engine::new(
                &_adapter,
                device.clone(),
                queue.clone(),
                format,
                None,
                WgpuShell::headless(),
            );
                Renderer::new(engine, iced_winit::core::Font::default(), iced_winit::core::Pixels(16.0))
        };

        event_loop.set_control_flow(ControlFlow::Wait);
        window.set_title("kestrel-rs — https://servo.org");

        *self = Self::Ready {
            window,
            device,
            #[allow(unused_variables)]
            queue,
            surface: Box::new(surface),
                format,
                renderer: Box::new(renderer),
                viewport: Box::new(viewport),
            cursor: mouse::Cursor::Unavailable,
            modifiers: ModifiersState::default(),
            cache: user_interface::Cache::new(),
            events: Vec::new(),
            resized: false,                url_value: "https://servo.org".to_string(),
            servo,
            webview,
            rendering_context,
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
            device,
            queue: _queue,
            surface,
                format,
                renderer,
                viewport,
            cursor,
            modifiers,
            cache,
            events,
            resized,
            url_value,
            servo,
            webview,
            rendering_context,
        } = self
        else {
            return;
        };

        // Let Servo process pending work
        servo.spin_event_loop();

        match &event {
            WindowEvent::RedrawRequested => {
                // ---- Resize ----
                if *resized {
                    let size = window.inner_size();
                    **viewport = Viewport::with_physical_size(
                        Size::new(size.width, size.height),
                        window.scale_factor() as f32,
                    );
                    surface.configure(
                        device,
                        &wgpu::SurfaceConfiguration {
                            format: *format,
                            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                            width: size.width,
                            height: size.height,
                            present_mode: wgpu::PresentMode::AutoVsync,
                            alpha_mode: wgpu::CompositeAlphaMode::Auto,
                            view_formats: vec![],
                            desired_maximum_frame_latency: 2,
                        },
                    );
                    rendering_context.resize(size);
                    *resized = false;
                }                // ---- Paint Servo content (GL only, no wgpu present) ----
                //
                // We ONLY present via Servo's GL context. The wgpu surface is
                // not presented because GL and Vulkan present to the same
                // window surface — presenting both causes flickering/blanking.
                //
                // The URL bar is still functional through event processing
                // (typing + Enter navigation), but not visually rendered on
                // screen. See docs/servo-embedding-notes.md for the pixel
                // readback approach needed to solve this.
                webview.paint();
                rendering_context.present();
            }

            WindowEvent::CursorMoved { position, .. } => {
                *cursor = mouse::Cursor::Available(conversion::cursor_position(
                    *position,
                    viewport.scale_factor(),
                ));
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                *modifiers = new_modifiers.state();
            }

            WindowEvent::Resized(_) => {
                *resized = true;
            }

            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            _ => {}
        }

        // ---- Convert winit event to Iced event ----
        if let Some(iced_event) =
            conversion::window_event(event, window.scale_factor() as f32, *modifiers)
        {
            events.push(iced_event);
        }

        // ---- Process pending Iced events ----
        if !events.is_empty() {
            let mut interface = UserInterface::build(
                url_bar(url_value),
                viewport.logical_size(),
                mem::take(cache),
                renderer,
            );

            let mut messages: Vec<Message> = Vec::new();
            let mut clipboard = NullClipboard;
            let _ = interface.update(
                events,
                *cursor,
                renderer,
                &mut clipboard,
                &mut messages,
            );
            events.clear();
            *cache = interface.into_cache();

            // ---- Process messages → dispatch to Servo ----
            for message in messages {
                match message {
                    Message::UrlBarChanged(new_url) => {
                        *url_value = new_url.clone();
                        // Show URL in title bar since we can't render the
                        // URL bar visually on top of Servo's GL context yet.
                        window.set_title(&format!("kestrel-rs — {new_url}"));
                    }
                    Message::UrlBarSubmitted => {
                        if let Ok(url) = Url::parse(url_value) {
                            eprintln!("kestrel-rs: navigating to {url}");
                            webview.load(url);
                            servo.spin_event_loop();
                        } else {
                            eprintln!("kestrel-rs: invalid URL: {url_value}");
                        }
                    }
                }
            }

            window.request_redraw();
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
