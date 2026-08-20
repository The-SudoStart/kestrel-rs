# Servo embedding notes

Filled in during Phase 0 (verified 2026-08-20).

## Crate versions (verified against crates.io)

| Crate | Version | Notes |
|-------|---------|-------|
| `servo` | **0.5.0** (2026-08-17) | Latest stable. Edition 2024, requires Rust ≥ 1.88.0. License: MPL-2.0. |
| `iced` | **0.14.0** (2025-12-07) | Latest stable. Elm-style GUI framework. |
| `winit` | **^0.30** | Used by both `iced_winit` (Iced) and Servo's dev deps/examples. Compatible. |
| `webrender_api` | **^0.70** | Servo's rendering API types. Used in embedding for `DevicePoint`, etc. |
| `euclid` | **^0.22** | Geometry types shared between Servo and winit. |

**Previous Cargo.toml assumed `servo = "0.1"` — this was 4 major versions behind.**

### Servo v0.5.0 feature flags

Default features: `baked-in-resources`, `bundled_freetype`, `clipboard`, `js_jit`.

Notable optional features:
- `media-gstreamer` — audio/video playback (requires GStreamer system libs)
- `webgpu` — WebGPU support
- `webxr` — WebXR/VR support
- `bluetooth` — Bluetooth API
- `vello` — alternative GPU renderer
- `tracing` — structured logging

We use default features for Phase 0.

## WebView API surface (as of v0.5.0)

The embedding API lives in the `servo` crate's root. Key types:

### Core types
- **`Servo`** — the engine instance. Created via `ServoBuilder`.
- **`ServoBuilder`** — configures and creates a `Servo` instance. Requires an `EventLoopWaker`.
- **`WebView`** — a single web view. Created via `WebViewBuilder`.
- **`WebViewBuilder`** — configures and creates a `WebView`. Requires `Servo` + `RenderingContext`.

### Traits to implement
- **`EventLoopWaker`** — bridges Servo's async needs into your event loop. Must implement `wake()` and `clone_box()`.
- **`WebViewDelegate`** — receives callbacks from the WebView. Minimum: `notify_new_frame_ready()`.
- **`ServoDelegate`** — (optional) receives engine-level notifications and error reporting.
- **`RenderingContext`** — abstraction over the rendering surface. We use `WindowRenderingContext`.

### RenderingContext options
1. **`WindowRenderingContext`** — renders to a native window (requires display + window handles). **This is what we use.**
2. **`OffscreenRenderingContext`** — renders to an offscreen buffer. Could be useful for Iced integration.
3. **`SoftwareRenderingContext`** — CPU-based rendering. Slow but portable.

### Embedding flow (from `winit_minimal.rs` example)

```
1. Create winit EventLoop
2. On Resumed:
   a. Create winit Window
   b. Create WindowRenderingContext(display_handle, window_handle, size)
   c. rendering_context.make_current()
   d. Create Servo via ServoBuilder (pass EventLoopWaker)
   e. Create WebView via WebViewBuilder (pass Servo + RenderingContext + URL + delegate)
3. Event loop:
   - On UserEvent/WakerEvent: servo.spin_event_loop()
   - On RedrawRequested: webview.paint() + rendering_context.present()
   - On MouseWheel: webview.notify_input_event(InputEvent::Wheel(...))
   - On Resized: webview.resize(new_size)
   - On CloseRequested: exit
```

### Key WebView methods
- `webview.paint()` — renders the current frame to the rendering context
- `webview.resize(size)` — notifies of window resize
- `webview.notify_input_event(event)` — forwards input (mouse, keyboard, wheel)
- `webview.load(url)` — navigates to a URL
- `webview.go_back()` / `webview.go_forward()` — navigation history

## Phase 0 proof-of-concept status

`src/main.rs` contains a minimal working proof-of-concept based on Servo's official
`winit_minimal.rs` example. It:
- Creates a winit window
- Initializes Servo with a `WindowRenderingContext`
- Loads servo.org in a WebView
- Handles redraw, resize, and scroll events

**Builds clean** with `cargo check`, `cargo clippy -- -D warnings`, zero warnings.

## The Iced integration challenge (Phase 1 blocker)

This is the **single biggest architectural question** for moving beyond Phase 0.

### The problem
- Servo's embedding API is tightly coupled to **winit** — it expects to own the window,
  manage the GL context, and render via WebRender directly to the surface.
- Iced **also** wraps winit internally via `iced_winit` — `iced::application()` creates
  and manages its own winit event loop and window.
- **Two libraries cannot both own the winit event loop.**

### Why this matters for kestrel-rs
Our ARCHITECTURE.md chose Iced for its Elm-style Model/Update/View architecture.
But Servo needs winit at the lowest level. We need to reconcile these.

### Possible approaches (for Phase 1 decision)

1. **Servo owns the window, Iced renders overlays** (recommended for now)
   - Let Servo manage the winit window and event loop (as in our PoC)
   - Use Iced's lower-level primitives (not `iced::application()`) to render
     UI elements (command palette, URL bar) on top of Servo's output
   - Challenge: Iced's rendering (wgpu/tiny-skia) and Servo's rendering (WebRender)
     both want the GL context — they can't both paint to the same surface

2. **Offscreen rendering bridge**
   - Use Servo's `OffscreenRenderingContext` to render web content to a texture/buffer
   - Display that texture in an Iced `canvas` widget
   - Challenge: performance overhead of the extra compositing step; may lose GPU acceleration

3. **Separate windows**
   - Servo renders in its own window; Iced renders UI in a separate overlay window
   - Hacky but functional; could work as a quick Phase 1 approach
   - Challenge: window management, focus handling, visual cohesion

4. **Replace Iced with custom UI layer**
   - Build the command palette and settings UI with raw winit + a simple widget toolkit
   - Abandon Iced entirely; keep the Elm-style architecture as a design pattern
   - Challenge: more code to write, lose Iced's widget library

### Recommendation
For Phase 0, we proved the embedding works with winit. For Phase 1, **option 1 or 3**
is most practical. Option 1 is architecturally cleanest but hardest. Option 3 is hacky
but gets a working browser shell fastest. Document the decision in ARCHITECTURE.md
before starting Phase 1.

## Gaps vs. Phase 1 requirements (ROADMAP.md)

| Phase 1 need | Servo API status | Gap? |
|---|---|---|
| Load URL, navigate | `webview.load(url)` ✓ | No |
| Back/forward | `webview.go_back()` / `go_forward()` ✓ | No |
| Reload | `webview.reload()` ✓ | No |
| Multi-tab | Can create multiple `WebView` instances ✓ | Need shell-level tab management |
| Keyboard input | `webview.notify_input_event()` ✓ | Need keymap layer |
| Command palette | Not in Servo — this is our UI layer | Iced integration needed |
| Settings (TOML) | Not in Servo — this is our config layer | Our `config` module |
| History | Not in Servo — need our own storage | New module needed |

## Workarounds / fork-for-patch assessment

**No fork needed for Phase 0.** The embedding API works as-is for proving the concept.

Potential Phase 1 issues:
- If Servo's `WebViewDelegate` doesn't expose enough navigation events (e.g., title changes,
  loading progress), we may need to request upstream API additions — but that's an API
  request, not a fork.
- If the GL context sharing between Servo and Iced proves impossible, that's an architectural
  fork point — but we should exhaust other approaches first.

## Runtime issues discovered

1. **Logger conflict**: `tracing_subscriber::fmt::init()` and `servo.setup_logging()`
   both try to set the global logger. Only one can be called. We let Servo handle
   logging and use `eprintln!` for our own output.

2. **TLS certificate verification fails**: `rustls_platform_verifier` can't verify
   HTTPS certificates in some environments (error: `CaUsedAsEndEntity`). The WebView
   is created and runs, but can't fetch HTTPS pages. This is an environment issue,
   not a code bug. For development, we could use `env_logger` with
   `RUST_LOG=servo=debug` for more details.

3. **SQLite storage warnings**: `ClientStorage` can't open its database in `/tmp`.
   Non-fatal — logged as warnings but doesn't affect functionality.

## Iced <-> Servo integration pattern (Issue #1 findings)

### Architecture that works

The integration uses the **low-level Iced crate path** (not `iced::application()`):

1. **winit** owns the window and event loop (our custom `ApplicationHandler`)
2. **Servo** renders web content via `WindowRenderingContext` (OpenGL/WebRender)
3. **wgpu** creates a rendering surface on the same window (Vulkan backend)
4. **Iced's `UserInterface`** renders the URL bar via wgpu on top

This is based on Iced's `integration` example but adapted for Servo coexistence.

### Key API signatures (iced 0.14, wgpu 27)

- `UserInterface::build(element, size, cache, renderer)` — builds UI
- `UserInterface::update(&mut self, events, cursor, renderer, clipboard, messages)` — processes events
- `Viewport::with_physical_size(size, scale_factor: f32)` — NOT a Scale struct, just f32
- `Renderer::new(engine, default_font, default_text_size)` — takes Font and Pixels, not Settings
- `wgpu::Surface::get_current_texture()` returns `Result<SurfaceTexture, SurfaceError>`
- `iced_winit::conversion::window_event()` converts winit events to Iced events
- `iced_winit::core::Clipboard` trait must be implemented (NullClipboard for now)

### Servo <-> Iced communication pattern

Navigation dispatch flow:
1. User types in Iced text_input, presses Enter
2. Iced `UserInterface::update()` produces `Message::UrlBarSubmitted`
3. In our message handler: `webview.load(url)` — calls Servo's navigation API
4. `servo.spin_event_loop()` — processes the navigation request
5. Servo fetches the page via its networking stack
6. `notify_new_frame_ready` callback triggers redraw
7. `webview.paint()` + `rendering_context.present()` renders new content

### What we learned

1. **Servo's GL context and wgpu can coexist on the same Wayland window.**
   Servo uses OpenGL (WebRender), wgpu uses Vulkan. The Wayland compositor handles both.
2. **Servo must render FIRST, then Iced draws on top.**
   Order: `webview.paint()` → `rendering_context.present()` → wgpu render pass → Iced draw → present.
3. **The URL bar text_input works** — typing, backspace, and Enter all function correctly
   through Iced's `UserInterface` event processing.
4. **Navigation dispatch works** — calling `webview.load(url)` from the Iced message
   handler successfully triggers Servo navigation.

### Remaining gaps

- Clipboard is stubbed (NullClipboard) — need real clipboard integration
- No mouse forwarding to Servo yet — keyboard events work but mouse clicks don't
  reach the WebView (need to forward winit mouse events to `webview.notify_input_event()`)
- TLS certificate verification fails in this environment (not a code issue)
- The URL bar renders but needs styling (transparent background, positioned at top)

## System dependencies discovered

- `libclang-dev` — required by `bindgen` (Servo uses FFI bindings). Install via
  `apt-get install -y libclang-dev` on Ubuntu/Debian.
- Rust stable toolchain ≥ 1.88.0 (we have 1.97.1).
- TLS: `rustls` with `aws-lc-rs` provider (required for Servo's networking).
- A display server (X11 or Wayland) is required to run the binary.

## note
Cargo.lock is gitignored here since this is a binary that's still pre-alpha;
switch to committing Cargo.lock once you're past Phase 0 and want reproducible builds.
