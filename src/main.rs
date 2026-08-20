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
use iced_winit::core::renderer;
use iced_wgpu::{Engine, Renderer, wgpu};
use iced_winit::conversion;
use iced_winit::core::mouse;
use iced_winit::core::time::Instant;
use iced_winit::core::window;
use iced_winit::core::{Element, Event, Size, Theme};
use iced_winit::runtime::user_interface::{self, UserInterface};
use iced_winit::winit;
use winit::event::WindowEvent;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
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
        let _ = rendering_context.make_current();

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(ServoWaker(event_loop_proxy.clone())))
            .build();
        servo.setup_logging();

        let url = Url::parse("https://servo.org").expect("valid URL");
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

        event_loop.set_control_flow(ControlFlow::Wait);            *self = Self::Ready {
            window,
            device,
            queue,
            surface: Box::new(surface),
                format,
                renderer: Box::new(renderer),
                viewport: Box::new(viewport),
            cursor: mouse::Cursor::Unavailable,
            modifiers: ModifiersState::default(),
            cache: user_interface::Cache::new(),
            events: Vec::new(),
            resized: false,
            url_value: "https://servo.org".to_string(),
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
            queue,                surface,
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
                }

                // ---- Paint Servo content ----
                webview.paint();
                rendering_context.present();

                // ---- Draw Iced UI (URL bar) on top via wgpu ----
                match surface.get_current_texture() {
                    Ok(frame) => {
                        let view = frame
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        let mut encoder =
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: None,
                            });

                        // Clear to transparent
                        {
                            let _render_pass = encoder.begin_render_pass(
                                &wgpu::RenderPassDescriptor {
                                    label: Some("Iced overlay"),
                                    color_attachments: &[Some(
                                        wgpu::RenderPassColorAttachment {
                                            view: &view,
                                            resolve_target: None,
                                            ops: wgpu::Operations {
                                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                                    r: 0.0,
                                                    g: 0.0,
                                                    b: 0.0,
                                                    a: 0.0,
                                                }),
                                                store: wgpu::StoreOp::Store,
                                            },
                                            depth_slice: None,
                                        },
                                    )],
                                    depth_stencil_attachment: None,
                                    timestamp_writes: None,
                                    occlusion_query_set: None,
                                },
                            );
                        }

                        queue.submit([encoder.finish()]);

                        // ---- Draw Iced UI (URL bar) ----
                        let mut interface = UserInterface::build(
                            url_bar(url_value),
                            viewport.logical_size(),
                            mem::take(cache),
                            renderer,
                        );

                        let (state, _) = interface.update(
                            &[Event::Window(window::Event::RedrawRequested(
                                Instant::now(),
                            ))],
                            *cursor,
                            renderer,
                            &mut NullClipboard,
                            &mut Vec::new(),
                        );

                        if let user_interface::State::Updated {
                            mouse_interaction,
                            ..
                        } = state
                        {
                            if let Some(icon) =
                                conversion::mouse_interaction(mouse_interaction)
                            {
                                window.set_cursor(icon);
                                window.set_cursor_visible(true);
                            } else {
                                window.set_cursor_visible(false);
                            }
                        }

                        interface.draw(
                            renderer,
                            &Theme::Dark,
                            &renderer::Style::default(),
                            *cursor,
                        );

                        *cache = interface.into_cache();
                        renderer.present(None, frame.texture.format(), &view, viewport);
                        frame.present();
                    }
                    Err(wgpu::SurfaceError::Lost) => {
                        let size = window.inner_size();
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
                        window.request_redraw();
                    }
                    Err(e) => {
                        eprintln!("Surface error: {e:?}");
                    }
                }
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
                        *url_value = new_url;
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
