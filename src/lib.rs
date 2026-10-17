//! Notable comments:
//!
//! `disabled:` - features that have been implemented at some point, but are dormant for a later version. Usually for complex or optional features.
//!
//! `runtime:` - implementation relevant to runtimes (eg. animation logic, forward/inverse kinematics, etc).
//!
//! `iterable` - snippets that aren't automated/iterated, but probably should be.
//!
//! `todo:` - not important as of being written, but good to keep in mind.

use std::{
    io::{Seek, Write},
    path::PathBuf,
    sync::Mutex,
};

use egui_wgpu::wgpu::ExperimentalFeatures;
use shared::*;
use wgpu::{util::DeviceExt, BindGroupLayout, Buffer, InstanceDescriptor};

// native-only imports
#[cfg(not(target_arch = "wasm32"))]
mod native {
    pub use image::*;
    pub use std::fs;
    pub use std::io::Read;
    pub use std::process::{Command, Stdio};
    pub use std::time::Instant;
}
#[cfg(not(target_arch = "wasm32"))]
use native::*;
use zip::write::FullFileOptions;

#[cfg(target_arch = "wasm32")]
mod web {
    pub use wasm_bindgen::prelude::*;
    pub use web_sys::*;
    pub use web_time::Instant;
}
use std::sync::Arc;
#[cfg(target_arch = "wasm32")]
use web::*;

use image::GenericImageView;
use image::ImageEncoder;

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    window::{Theme, Window},
};

/// Keyternity's repository (releases, issues, translations).
pub const REPO_URL: &str = "https://github.com/inviti8/keyternity";

pub mod agent;
pub mod armature_window;
pub mod atlas_modal;
pub mod auto_mesh;
pub mod bind_pose;
pub mod bone_panel;
pub mod camera_ui;
pub mod dragonbones_export;
pub mod dragonbones_import;
pub mod editor;
pub mod envelope;
pub mod export_sequence;
pub mod ik;
pub mod export_modal;
pub mod file_reader;
pub mod keyframe_editor;
pub mod keyframe_panel;
pub mod layers;
pub mod mesh_tools;
pub mod modal;
pub mod navigation;
pub mod pen;
pub mod psd_import;
pub mod render_camera;
pub mod renderer;
pub mod settings_modal;
pub mod shapes;
pub mod shapes_panel;
pub mod shared;
pub mod skin;
pub mod startup_window;
pub mod timeline_rows;
pub mod timeline_view;
pub mod styles_modal;
pub mod topology;
pub mod ui;
pub mod utils;
pub mod warnings;
pub mod weights;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    pub fn getCanvasWidth() -> u32;
    pub fn getCanvasHeight() -> u32;
    pub fn getConfig() -> String;
    pub fn saveConfig(data_str: String);
    pub fn getUiSliderValue() -> f32;
    pub fn toggleElement(open: bool, id: String);
    pub fn isModalActive(id: String) -> bool;
    pub fn getEditInput() -> String;
    pub fn setEditInput(value: String);
    pub fn removeImage();
    pub fn getFile() -> Vec<u8>;
    pub fn getFileName() -> String;
    pub fn removeFile();
    pub fn loaded();
    pub fn focusEditInput();
    pub fn openDocumentation(docs_name: String, path: String);
    pub fn updateUiSlider();
    pub fn downloadSample(filename: String);
    pub fn openLink(url: String);
    pub fn isMobile() -> bool;
    pub fn clickFileInput(isImage: bool);
    pub fn hasElement(id: &str) -> bool;
    pub fn getImgName(idx: usize) -> String;
    pub fn hasLoadedAllImages() -> bool;
    pub fn downloadZip(zip: Vec<u8>, saving: String);
    pub fn downloadMp4(data: Vec<u8>, resX: f32, resY: f32, name: &str, fps: i32);
    pub fn downloadGif(resX: f32, resY: f32, name: &str, fps: i32);
    pub fn addGifFrame(frame: Vec<u8>);
    pub fn changeLang(filename: String);
    pub fn getLang() -> Vec<u8>;
    pub fn clearLang();
    pub fn ensureFFmpeg();
}

#[derive(Default)]
pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<BackendRenderer>,
    gui_state: Option<egui_winit::State>,
    last_render_time: Option<Instant>,
    #[cfg(target_arch = "wasm32")]
    pub renderer_receiver: Option<futures::channel::oneshot::Receiver<BackendRenderer>>,
    pub shared: shared::Shared,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        #[allow(unused_mut)]
        let mut attributes = Window::default_attributes();

        #[cfg(target_os = "windows")]
        {
            let file_bytes = include_bytes!("../assets/icon.png");
            let diffuse_image = image::load_from_memory(file_bytes).unwrap();
            let rgba = diffuse_image.to_rgba8();
            let pixels = rgba.as_bytes().to_vec();

            let icon = winit::window::Icon::from_rgba(pixels, rgba.width(), rgba.height()).unwrap();
            attributes.window_icon = Some(icon);
        }

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            #[rustfmt::skip]
            let canvas =
                window().unwrap().document().unwrap().get_element_by_id("canvas")
                .unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
            self.shared.camera.window = Vec2::new(canvas.width() as f32, canvas.height() as f32);
            attributes = attributes.with_canvas(Some(canvas));
        }

        if let Ok(window) = event_loop.create_window(attributes) {
            let first_window_handle = self.window.is_none();
            let window_handle = Arc::new(window);
            self.window = Some(window_handle.clone());

            if !first_window_handle {
                return;
            }

            let gui_context = egui::Context::default();

            // turn off egui kb zoom
            gui_context.options_mut(|op| {
                op.zoom_with_keyboard = false;
            });

            #[cfg(target_arch = "wasm32")]
            {
                gui_context.set_pixels_per_point(window_handle.scale_factor() as f32);
            }

            let viewport_id = gui_context.viewport_id();
            let gui_state = egui_winit::State::new(
                gui_context,
                viewport_id,
                &window_handle,
                Some(window_handle.scale_factor() as f32),
                Some(Theme::Dark),
                None,
            );

            #[cfg(not(target_arch = "wasm32"))]
            let (width, height) = (
                window_handle.inner_size().width,
                window_handle.inner_size().height,
            );

            #[cfg(not(target_arch = "wasm32"))]
            {
                let renderer = pollster::block_on(async move {
                    BackendRenderer::new(window_handle.clone(), width, height).await
                });
                self.renderer = Some(renderer);
            }

            #[cfg(target_arch = "wasm32")]
            {
                let (sender, receiver) = futures::channel::oneshot::channel();
                self.renderer_receiver = Some(receiver);
                let size = self.shared.camera.window.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let renderer =
                        BackendRenderer::new(window_handle.clone(), size.x as u32, size.y as u32)
                            .await;
                    if sender.send(renderer).is_err() {
                        log::error!("Failed to create and send renderer!");
                    }
                });
            }

            self.gui_state = Some(gui_state);
            self.last_render_time = Some(Instant::now());
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        #[cfg(target_arch = "wasm32")]
        {
            let mut renderer_received = false;
            if let Some(receiver) = self.renderer_receiver.as_mut() {
                if let Ok(Some(renderer)) = receiver.try_recv() {
                    self.renderer = Some(renderer);
                    renderer_received = true;
                }
            }
            if renderer_received {
                self.renderer_receiver = None;
            }
        }

        file_reader::read(
            &mut self.shared,
            &self.renderer,
            self.gui_state.as_ref().unwrap().egui_ctx(),
        );

        if self.shared.ui.scale <= 0. {
            self.shared.ui.scale = 1.;
        }

        let (Some(gui_state), Some(renderer), Some(window), Some(last_render_time)) = (
            self.gui_state.as_mut(),
            self.renderer.as_mut(),
            self.window.as_ref(),
            self.last_render_time.as_mut(),
        ) else {
            return;
        };

        // Receive gui window event
        if gui_state.on_window_event(window, &event).consumed {
            //return;
        }

        // If the gui didn't consume the event, handle it
        match event {
            WindowEvent::HoveredFile(_) => {
                self.shared.events.open_modal("drop_file", true);
            }
            WindowEvent::HoveredFileCancelled => {
                self.shared.ui.modal = false;
            }
            // process dragged and dropped files
            WindowEvent::DroppedFile(_path_buf) => {
                self.shared.ui.modal = false;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let raw_name = _path_buf.as_path().file_name().unwrap();
                    let name = raw_name.to_str().unwrap().to_string();
                    if !name.contains(".") {
                        return;
                    }
                    let ext = name.split('.').collect::<Vec<_>>()[1]
                        .to_string()
                        .to_lowercase();

                    if ext.contains("skf") || ext.contains("psd") {
                        // load .skf or .skfe file
                        *self.shared.ui.file_path.lock().unwrap() = vec![_path_buf];
                        *self.shared.ui.file_type.lock().unwrap() = 2;
                    } else {
                        // only load images if a style is selected in styles modal
                        if self.shared.selections.style_id != -1 {
                            self.shared.ui.dropped_file_path.push(_path_buf);
                            self.shared.ui.file_elapsed = Some(Instant::now());
                        } else {
                            self.shared.events.open_modal("img_unselected_style", false);
                        }
                    }
                }
            }
            WindowEvent::CloseRequested => {
                utils::attempt_exit(
                    &mut self.shared.undo_states,
                    &self.shared.config,
                    &mut self.shared.ui,
                );
            }
            WindowEvent::Focused(is_focused) => {
                let manager = &mut self.shared.input.hotkey_manager;
                let mod_q = &mut self.shared.input.mod_q;
                let mod_w = &mut self.shared.input.mod_w;
                if is_focused {
                    _ = manager.as_mut().unwrap().register(mod_q.unwrap());
                    _ = manager.as_mut().unwrap().register(mod_w.unwrap());
                } else {
                    let manager = &mut self.shared.input.hotkey_manager;
                    _ = manager.as_mut().unwrap().unregister(mod_q.unwrap());
                    _ = manager.as_mut().unwrap().unregister(mod_w.unwrap());
                }
            }
            #[allow(unused_mut)]
            WindowEvent::Resized(PhysicalSize {
                mut width,
                mut height,
            }) => {
                if width == 0 || height == 0 {
                    return;
                }
                self.shared.camera.window = Vec2::new(width as f32, height as f32);
                renderer.resize(width as u32, height as u32);
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                *last_render_time = now;

                // keep cursor in place, while dragging input
                if self.shared.ui.edited_dragging {
                    let mouse = self.shared.input.mouse_init.unwrap();
                    let _ = window
                        .set_cursor_position(winit::dpi::PhysicalPosition::new(mouse.x, mouse.y));
                }

                #[cfg(not(target_arch = "wasm32"))]
                {
                    let undo = &mut self.shared.undo_states;

                    // mark window as unsaved if prev_undo_actions doesn't match
                    if undo.prev_undo_actions != undo.undo_actions.len() {
                        self.shared.ui.changed_window_name = false;
                        undo.prev_undo_actions = undo.undo_actions.len();
                    }

                    if !self.shared.ui.changed_window_name {
                        // show loaded filename as window name
                        let file = if self.shared.ui.save_path == None {
                            "Keyternity".to_string()
                        } else {
                            let path = self.shared.ui.save_path.clone().unwrap();
                            let filename = path.as_path().file_name().unwrap();
                            filename.to_str().unwrap().to_string()
                        };

                        // show asterisk after name if unsaved
                        let undo = &self.shared.undo_states;
                        self.shared.ui.is_unsaved =
                            undo.unsaved_undo_actions != undo.undo_actions.len();
                        let asterisk = if self.shared.ui.is_unsaved { " *" } else { "" };

                        let title =
                            file + " - v" + &env!("CARGO_PKG_VERSION").to_string() + asterisk;
                        window.set_title(&title);
                        self.shared.ui.changed_window_name = true;
                    }
                }

                let input = gui_state.take_egui_input(&window);
                gui_state.egui_ctx().begin_pass(input);

                let s = &mut self.shared;
                #[rustfmt::skip]
                ui::process_inputs(
                    gui_state.egui_ctx(), &mut s.input, &mut s.ui, &s.config,
                    &mut s.edit_mode, &mut s.events, &s.camera, &s.selections, &mut s.armature
                );

                // the renderer's canvas labels, drawn by egui
                s.ui.envelope_labels = s.renderer.envelope_labels.clone();

                // ui logic handled in ui.rs
                #[rustfmt::skip]
                ui::draw(
                    gui_state.egui_ctx(), &mut s.ui, &mut s.input, &mut s.selections,
                    &mut s.config, &mut s.events, &mut s.edit_mode, &s.camera, &mut s.armature, &s.copy_buffer
                );

                let egui_winit::egui::FullOutput {
                    textures_delta,
                    shapes,
                    pixels_per_point,
                    platform_output,
                    ..
                } = gui_state.egui_ctx().end_pass();

                gui_state.handle_platform_output(window, platform_output);
                let paint_jobs = gui_state.egui_ctx().tessellate(shapes, pixels_per_point);

                let size = window.inner_size();
                let config = &renderer.gpu.surface_config;
                let screen_descriptor = egui_wgpu::ScreenDescriptor {
                    size_in_pixels: [config.width, config.height],
                    pixels_per_point,
                };

                if !self.shared.renderer.initialized_window && size.width != 0 && size.height != 0 {
                    renderer.resize(size.width as u32, size.height as u32);
                    self.shared.renderer.initialized_window = true;
                }

                self.shared.camera.window = Vec2::new(size.width as f32, size.height as f32);
                renderer.render_frame(
                    screen_descriptor,
                    paint_jobs,
                    textures_delta,
                    &mut self.shared,
                );

                #[cfg(target_arch = "wasm32")]
                {
                    self.shared.ui.scale = getUiSliderValue() * window.scale_factor() as f32;
                    self.shared.ui.mobile = isMobile();
                    self.shared.renderer.mobile = isMobile();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.shared.ui.scale =
                        self.shared.config.ui_scale * window.scale_factor() as f32;
                }
                gui_state
                    .egui_ctx()
                    .set_pixels_per_point(self.shared.ui.scale);
                self.shared.camera.px_scale = self.shared.ui.scale;

                // set hovering bone to -1 if neither the renderer nor UI have it
                if !self.shared.renderer.is_hovering_bone && !self.shared.ui.is_hovering_bone {
                    self.shared.selections.hovering_bone_id = -1;
                }

                // iterate and process all events for this frame
                while self.shared.events.events.len() > 0 {
                    // these events are processed here due to using WGPU features
                    let queue = Some(&self.renderer.as_ref().unwrap().gpu.queue);
                    let device = Some(&self.renderer.as_ref().unwrap().gpu.device);
                    let bgl = Some(&self.renderer.as_ref().unwrap().bind_group_layout);
                    let ctx = Some(gui_state.egui_ctx());

                    let shared = &mut self.shared;

                    if shared.events.events[0] == Events::CreateEmptyTexture {
                        let img = image::DynamicImage::new(1, 1, image::ColorType::Rgba8);
                        let style_id = shared.selections.style_id;
                        let armature = &mut shared.armature;
                        let dims = Vec2::new(1., 1.);
                        file_reader::add_texture(
                            img, style_id, dims, "", armature, queue, device, bgl, ctx,
                        );
                    } else if shared.events.events[0] == Events::TrimTexture {
                        for id in &shared.selections.tex_ids {
                            // get texture data of this selected texture
                            let style = shared.armature.sel_style(&shared.selections).unwrap();
                            let data_id = style.textures[*id as usize].data_id;
                            let tex_data = &mut shared.armature.tex_data[data_id as usize];

                            // trim transparent edges off
                            let img = file_reader::trim_transparent(&tex_data.image);

                            // replace this texture data with new, trimmed img
                            let dims =
                                Vec2::new(img.dimensions().0 as f32, img.dimensions().1 as f32);
                            let (bind_group, ui_img) =
                                file_reader::create_texture(&img, dims, queue, device, bgl, ctx);
                            tex_data.image = img;
                            tex_data.bind_group = bind_group;
                            tex_data.ui_img = ui_img;

                            // update texture size in armature as well
                            let style = shared.armature.sel_style_mut(&shared.selections).unwrap();
                            style.textures[*id as usize].size = dims;
                        }
                    }

                    // all other events go here
                    let s = &mut self.shared;
                    #[rustfmt::skip]
                    editor::iterate_events(
                        &s.input, &mut s.config, &mut s.events, &mut s.camera, &mut s.edit_mode, &mut s.selections,
                        &mut s.undo_states, &mut s.armature, &mut s.psd_armature, &mut s.copy_buffer, &mut s.ui, &mut s.renderer
                    );
                }
            }
            _ => (),
        }

        // read system shortcuts (defined in main.rs with global_hotkey)
        if let Ok(event) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv() {
            let pressing_w = event.id() == self.shared.input.mod_w.unwrap().id();
            let pressing_q = event.id() == self.shared.input.mod_q.unwrap().id();
            if (pressing_w || pressing_q) && self.shared.ui.can_quit {
                let shared = &mut self.shared;
                utils::attempt_exit(&mut shared.undo_states, &shared.config, &mut shared.ui);
            }
        }

        // if an exit was attemped, first show unsaved changes modal if appropriate
        if self.shared.ui.exiting {
            let undo = &self.shared.undo_states;
            if undo.unsaved_undo_actions != undo.undo_actions.len() {
                let str_del = self.shared.ui.loc("polar.unsaved").clone().to_string();
                let exiting = PolarId::Exiting;
                self.shared.events.open_polar_modal(exiting, str_del);
            } else {
                event_loop.exit();
            }
            self.shared.ui.exiting = false;
        }

        // exit has been confirmed - eviscerate and obliterate Keyternity
        if self.shared.ui.confirmed_exit {
            event_loop.exit();
        }

        window.request_redraw();
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BlitUniforms {
    magnification: f32,
    _pad: [f32; 7], // std140 alignment (important!)
}

pub struct BackendRenderer {
    gpu: Gpu,
    egui_renderer: egui_wgpu::Renderer,
    scene: Scene,
    bind_group_layout: BindGroupLayout,
    blit_bind_group_layout: BindGroupLayout,
    blit_buffer: Buffer,
}

impl BackendRenderer {
    pub async fn new(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Self {
        let gpu = Gpu::new_async(window, width, height).await;

        let egui_renderer = egui_wgpu::Renderer::new(
            &gpu.device,
            gpu.surface_config.format,
            egui_wgpu::RendererOptions {
                depth_stencil_format: None,
                msaa_samples: 1,
                ..Default::default()
            },
        );

        let bind_group_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                multisampled: false,
                                view_dimension: wgpu::TextureViewDimension::D2,
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                    label: Some("texture_bind_group_layout"),
                });

        let blit_bind_group_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                multisampled: false,
                                view_dimension: wgpu::TextureViewDimension::D2,
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                    label: Some("texture_bind_group_layout"),
                });

        let scene = Scene::new(&gpu.device, gpu.surface_format, &bind_group_layout);

        let blit_uniforms = BlitUniforms {
            magnification: 1.0,
            _pad: [0.0; 7],
        };

        let blit_uniform_buffer =
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Blit Uniform Buffer"),
                    contents: bytemuck::bytes_of(&blit_uniforms),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });

        Self {
            gpu,
            egui_renderer,
            scene,
            bind_group_layout,
            blit_bind_group_layout,
            blit_buffer: blit_uniform_buffer,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.gpu.resize(width, height);
    }

    pub fn render_frame(
        &mut self,
        screen_descriptor: egui_wgpu::ScreenDescriptor,
        paint_jobs: Vec<egui::epaint::ClippedPrimitive>,
        textures_delta: egui::TexturesDelta,
        shared: &mut shared::Shared,
    ) {
        if shared.camera.window == Vec2::new(0., 0.) {
            return;
        }

        let clear = wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color {
                r: shared.config.colors.background.r as f64 / 255.,
                g: shared.config.colors.background.g as f64 / 255.,
                b: shared.config.colors.background.b as f64 / 255.,
                a: 1.0,
            }),
            store: wgpu::StoreOp::Store,
        };

        for (id, image_delta) in &textures_delta.set {
            self.egui_renderer
                .update_texture(&self.gpu.device, &self.gpu.queue, *id, image_delta);
        }
        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        let desc = &wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        };
        let mut encoder = self.gpu.device.create_command_encoder(desc);
        encoder.insert_debug_marker("Render scene");

        #[rustfmt::skip]
        self.egui_renderer.update_buffers(&self.gpu.device, &self.gpu.queue, &mut encoder, &paint_jobs, &screen_descriptor);

        let surface_texture = self.gpu.surface.get_current_texture().unwrap();
        let surface_texture_view =
            surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor {
                    label: wgpu::Label::default(),
                    aspect: wgpu::TextureAspect::default(),
                    format: Some(self.gpu.surface_format),
                    dimension: None,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                    usage: None,
                });

        let uniforms = BlitUniforms {
            magnification: shared.config.pixel_magnification as f32,
            _pad: [0.0; 7],
        };
        let format;
        match self.gpu.surface_format {
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                format = wgpu::TextureFormat::Bgra8Unorm;
            }
            _ => format = self.gpu.surface_format,
        }
        let size = Vec2::new(
            (shared.camera.window.x as u32 / shared.config.pixel_magnification as u32) as f32,
            (shared.camera.window.y as u32 / shared.config.pixel_magnification as u32) as f32,
        );
        if size.x == 0. || size.y == 0. {
            return;
        }
        let blit_texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: size.x as u32,
                height: size.y as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            label: Some("Pixel Texture"),
            view_formats: &[],
        });
        let blit_view = blit_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut blit_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &blit_view,
                resolve_target: None,
                ops: clear,
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        blit_pass.set_pipeline(&self.scene.pipeline);
        self.skf_render(shared, &mut blit_pass.forget_lifetime());

        let sampler = self.gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let blit_bind_group = self
            .gpu
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.blit_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&blit_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.blit_buffer.as_entire_binding(),
                    },
                ],
                label: Some("LowRes Blit BindGroup"),
            });
        let mut final_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &surface_texture_view,
                resolve_target: None,
                ops: clear,
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        final_pass.set_pipeline(&self.scene.blit_pipeline);
        final_pass.set_bind_group(0, &blit_bind_group, &[]);
        final_pass.draw(0..3, 0..1);

        self.egui_renderer.render(
            &mut final_pass.forget_lifetime(),
            &paint_jobs,
            &screen_descriptor,
        );

        self.gpu
            .queue
            .write_buffer(&self.blit_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
    }

    fn skf_render(&mut self, shared: &mut Shared, render_pass: &mut wgpu::RenderPass) {
        self.init_buffers_and_bindgroups(&mut shared.renderer);
        self.check_export(shared);
        #[cfg(not(target_arch = "wasm32"))]
        self.step_sequence_export(shared);

        // import dropped files
        // this uses file_elapsed since winit does not buffer multiple dropped files
        if shared.ui.file_elapsed != None
            && shared.ui.file_elapsed.unwrap().elapsed().as_millis() > 150
        {
            *shared.ui.file_path.lock().unwrap() = shared.ui.dropped_file_path.clone();
            #[rustfmt::skip]
            let name = shared.ui.file_path.lock().unwrap()[0].as_path().file_name().unwrap().to_str().unwrap().to_string();
            let ext = name.split('.').collect::<Vec<_>>()[1]
                .to_string()
                .to_lowercase();
            if ext == "png" || ext == "webp" {
                *shared.ui.file_type.lock().unwrap() = 1;
            } else if ext.contains("skf") {
                *shared.ui.file_type.lock().unwrap() = 2;
            }
            shared.ui.file_elapsed = None;
            shared.ui.dropped_file_path = vec![];
        }

        if *shared.ui.save_finished.lock().unwrap() {
            shared.undo_states.unsaved_undo_actions = shared.undo_states.undo_actions.len();
            shared.undo_states.prev_undo_actions = shared.undo_states.undo_actions.len();
            shared.ui.changed_window_name = false;
            shared.ui.modal = false;
            shared.ui.can_quit = true;
            *shared.ui.save_finished.lock().unwrap() = false;
        } else if *shared.ui.export_finished.lock().unwrap() {
            shared.ui.modal = false;
            shared.ui.can_quit = true;
            *shared.ui.export_finished.lock().unwrap() = false;
        }

        let saving = shared.ui.saving.lock().unwrap().clone();
        let recording = saving == Saving::Spritesheet || saving == Saving::Video;
        if saving == Saving::FrameSequence {
            *shared.ui.saving.lock().unwrap() = Saving::None;
            #[cfg(not(target_arch = "wasm32"))]
            self.start_sequence_export(shared);
        } else if saving != Saving::None && !recording {
            // saving .skf or .skfe (exporting)

            #[cfg(target_arch = "wasm32")]
            {
                let saving_type = shared.ui.saving.lock().unwrap().clone();
                if saving_type == Saving::CustomPath || saving_type == Saving::Exporting {
                    #[rustfmt::skip]
                    utils::save_web(&rest_armature(shared), &shared.camera, &shared.edit_mode, saving_type);
                }
            }

            #[cfg(not(target_arch = "wasm32"))]
            if saving == Saving::DragonBones {
                self.save_dragonbones(shared);
            } else {
                self.save(shared);
            }
        } else if recording {
            // recording animations for images/videos
            shared.events.open_modal("exporting", true);
            #[rustfmt::skip]
            utils::render_spritesheets(&rest_armature(shared), &mut shared.ui, &shared.camera, &shared.config, self, &shared.renderer);
            *shared.ui.saving.lock().unwrap() = Saving::None;
            shared.ui.spritesheet_elapsed = Some(Instant::now());
            shared.ui.export_modal = false;
        }

        self.check_export_style(&shared.armature, &mut shared.ui);

        // animated bones will be used throughout the program
        // Pose Mode is for the setup pose; animations have their own keyframes
        let animating = shared.edit_mode.anim_open
            || shared
                .ui
                .is_animating(&shared.edit_mode, &shared.selections);
        if shared.edit_mode.pose_mode && animating {
            editor::exit_pose_mode(
                &mut shared.armature,
                &mut shared.edit_mode,
                &mut shared.undo_states,
            );
        }

        // the skinning agent's tool calls, queued by its MCP server
        #[cfg(not(target_arch = "wasm32"))]
        agent::server::poll(shared);

        // keep Bind Pose helpers consistent with the rest pose (no-op without helpers)
        let rest = shared
            .edit_mode
            .pose_mode
            .then_some(&shared.edit_mode.pose_snapshot);
        bind_pose::maintain_with_rest(&mut shared.armature, rest);
        // envelope-weighted meshes follow their inputs (no-op without any)
        skin::maintain(&mut shared.armature, shared.edit_mode.pose_mode);

        utils::animate_bones(&mut shared.armature, &shared.selections, &shared.edit_mode);
        shared.renderer.temp_bones = shared.armature.animated_bones.clone();

        // initialize non-mesh bone verts and indices
        for b in 0..shared.armature.bones.len() {
            let tex = shared.armature.anim_tex_of(shared.armature.bones[b].id);
            if tex != None && !shared.armature.bones[b].verts_edited {
                let size = tex.unwrap().size;

                let bone = &mut shared.armature.bones[b];
                (bone.vertices, bone.indices) = renderer::create_tex_rect(&size);
                let bone = &mut shared.armature.animated_bones[b];
                (bone.vertices, bone.indices) = renderer::create_tex_rect(&size);
                let bone = &mut shared.renderer.temp_bones[b];
                (bone.vertices, bone.indices) = renderer::create_tex_rect(&size);

                bone.verts_edited = false;
            }
        }

        // disable physics if current edited bone has physics properties.
        // this prevents the bone from being uncontrollable.
        let mut physics_active = false;
        if shared.edit_mode.is_moving || shared.edit_mode.is_scaling || shared.edit_mode.is_rotating
        {
            let bone = shared.armature.sel_bone(&shared.selections).unwrap();
            physics_active = shared.armature.has_physics(bone.id);
        }
        if !physics_active {
            shared.armature.bones = renderer::runtime_construction(
                &mut shared.renderer.temp_bones,
                &shared.armature.animated_bones,
                shared.armature.bones.clone(),
            );
        } else {
            renderer::construction(
                &mut shared.renderer.temp_bones,
                &shared.armature.animated_bones,
            );
        }

        // Pan and Zoom take the left button; the canvas sees it as released
        let navigating = shared.edit_mode.tool.navigates();
        let left = (shared.input.left_clicked, shared.input.left_pressed, shared.input.left_down);
        if navigating {
            #[rustfmt::skip]
            navigation::navigate(&shared.camera, &shared.input, &shared.edit_mode, &mut shared.renderer, &mut shared.events);
            shared.input.left_clicked = false;
            shared.input.left_pressed = false;
            shared.input.left_down = false;
        }

        // core rendering logic handled in renderer.rs
        #[rustfmt::skip]
        renderer::render(
            render_pass, &self.gpu.queue, &shared.camera, &shared.input, &mut shared.armature,
            &shared.config, &shared.edit_mode, &mut shared.selections, &mut shared.renderer, &mut shared.events,
        );
        if navigating {
            (shared.input.left_clicked, shared.input.left_pressed, shared.input.left_down) = left;
        }

        shared.ui.warnings = warnings::check_warnings(&shared.armature);
    }

    // processes style to export, if appropriate
    fn check_export_style(&self, armature: &Armature, shared_ui: &mut Ui) {
        let _path = shared_ui.export_style_path.lock().unwrap();
        let mut _web_buf: Vec<u8>;

        #[cfg(not(target_arch = "wasm32"))]
        let path_str = _path.to_str().unwrap().to_string();
        #[cfg(target_arch = "wasm32")]
        let path_str = " ";

        if shared_ui.export_style_id == usize::MAX || path_str == "" {
            return;
        }

        // initialize zip file (native/web)
        let mut zip;
        let options;
        #[cfg(not(target_arch = "wasm32"))]
        {
            zip = zip::ZipWriter::new(
                std::fs::File::create(_path.as_path().to_str().unwrap().to_string()).unwrap(),
            );
            options = zip::write::FullFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
        }
        #[cfg(target_arch = "wasm32")]
        {
            // create zip file
            _web_buf = vec![];
            let cursor = std::io::Cursor::new(&mut _web_buf);
            zip = zip::ZipWriter::new(cursor);
            options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
        }

        // go thru all textures of this style, and save them as png in the zip
        let style = &armature
            .styles
            .iter()
            .find(|s| s.id == shared_ui.export_style_id as i32)
            .unwrap();
        for tex in &style.textures {
            let tex_data = &armature.tex_data;
            let data = tex_data.iter().find(|t| t.id == tex.data_id);
            if data == None {
                continue;
            }

            // encode texture to png
            let img = &data.unwrap().image;
            let mut buf = vec![];
            let encoder = image::codecs::png::PngEncoder::new(&mut buf);
            let rgba8 = image::ExtendedColorType::Rgba8;
            encoder
                .write_image(&img.to_rgba8(), img.width(), img.height(), rgba8)
                .unwrap();

            // save texture into zip
            zip.start_file(format!("{}.png", tex.name), options.clone())
                .unwrap();
            zip.write(&buf).unwrap();
        }

        // save zip (native/web)
        #[cfg(not(target_arch = "wasm32"))]
        zip.finish().unwrap();
        #[cfg(target_arch = "wasm32")]
        {
            let bytes = zip.finish().unwrap().into_inner().to_vec();
            downloadZip(bytes, "Spritesheet".to_string());
        }

        // turn off exporting style
        shared_ui.export_style_id = usize::MAX;
    }

    fn check_export(&self, shared: &mut shared::Shared) {
        // if the spritesheet timer has initiaed, wait a little for all buffers to complete before saving them
        let elapsed = &mut shared.ui.spritesheet_elapsed;
        let duration_in_millis = 250;

        // keep timer still until all frames have been mapped
        let mut total_frames = 0;
        for sheet in &shared.ui.rendered_spritesheets {
            total_frames += sheet.len();
        }
        if *shared.ui.mapped_frames.lock().unwrap() < total_frames {
            *elapsed = Some(Instant::now());
        }

        if *elapsed != None && elapsed.unwrap().elapsed().as_millis() > duration_in_millis {
            if shared.ui.exporting_video_type != ExportVideoType::None {
                self.skf_export_videos(&shared.armature, &mut shared.ui);
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                self.skf_native_spritesheet(&shared.armature, &mut shared.ui);
                #[cfg(target_arch = "wasm32")]
                self.skf_web_spritesheet(&shared.armature, &mut shared.ui);
            }

            shared.ui.spritesheet_elapsed = None;
            shared.ui.modal = false;
            *shared.ui.mapped_frames.lock().unwrap() = 0;
        }
    }

    fn init_buffers_and_bindgroups(&self, renderer: &mut Renderer) {
        if renderer.meshframe_buffer.index == None {
            let max = 5000;
            renderer.bone_buffer.init(&self.gpu.device, max);
            renderer.prev_onion_buffer.init(&self.gpu.device, max);
            renderer.next_onion_buffer.init(&self.gpu.device, max);
            renderer.point_buffer.init(&self.gpu.device, max);
            renderer.kite_buffer.init(&self.gpu.device, max);
            renderer.attachment_line_buffer.init(&self.gpu.device, max);
            renderer.sel_bone_buffer.init(&self.gpu.device, max);
            renderer.gridline_buffer.init(&self.gpu.device, max);
            // meshes from Trace can be dense: each edge is 4 verts (8 with its outline)
            let mesh_max = 60000;
            renderer.meshframe_buffer.init(&self.gpu.device, mesh_max);
            renderer.edge_buffer.init(&self.gpu.device, mesh_max);
            renderer.wireframe_buffer.init(&self.gpu.device, mesh_max);
            renderer.wireframe_edge_buffer.init(&self.gpu.device, mesh_max);
            renderer.envelope_buffer.init(&self.gpu.device, mesh_max);
            renderer.envelope_edge_buffer.init(&self.gpu.device, max);
            renderer.paint_buffer.init(&self.gpu.device, mesh_max);
            renderer.ik_gizmo_buffer.init(&self.gpu.device, max);
            renderer.ik_link_buffer.init(&self.gpu.device, max);
            renderer.ring_buffer.init(&self.gpu.device, 100);
            renderer.selected_ring_buffer.init(&self.gpu.device, 100);
            renderer.rect_buffer.init(&self.gpu.device, max);
        }
        let bytes = include_bytes!("../assets/flow-kite.png");
        self.load_bindgroup(&mut renderer.flow_kite_bindgroup, bytes);
        let bytes = include_bytes!("../assets/circle.png");
        self.load_bindgroup(&mut renderer.circle_bindgroup, bytes);
        let bytes = include_bytes!("../assets/transform-ring.png");
        self.load_bindgroup(&mut renderer.ring_bindgroup, bytes);
        let bytes = include_bytes!("../assets/selected-ring.png");
        self.load_bindgroup(&mut renderer.selected_ring_bindgroup, bytes);

        if renderer.edge_pattern_bindgroups.is_none() {
            let pattern = |style| {
                let (px, len) = renderer::edge_pattern_pixels(style);
                renderer::create_pattern_bind_group(px, len, &self.gpu.queue, &self.gpu.device, &self.bind_group_layout)
            };
            renderer.edge_pattern_bindgroups = Some([pattern(EdgeStyle::Dotted), pattern(EdgeStyle::Dashed)]);
        }

        if renderer.generic_bindgroup == None {
            renderer.generic_bindgroup = Some(renderer::create_texture_bind_group(
                vec![255, 255, 255, 255],
                Vec2::new(1., 1.),
                &self.gpu.queue,
                &self.gpu.device,
                &self.bind_group_layout,
            ));
        }
    }

    fn load_bindgroup(&self, bindgroup: &mut Option<wgpu::BindGroup>, bytes: &[u8]) {
        if *bindgroup == None {
            let img = image::load_from_memory(bytes).unwrap();
            *bindgroup = Some(renderer::create_texture_bind_group(
                img.clone().into_rgba8().to_vec(),
                Vec2::new(img.width() as f32, img.height() as f32),
                &self.gpu.queue,
                &self.gpu.device,
                &self.bind_group_layout,
            ));
        }
    }

    /// Start a frame sequence or spritesheet export (docs/FRAME_SEQUENCES.md) to the
    /// chosen folder or zip. Asks first if frames leave the chosen camera, or if it
    /// would overwrite files; each answer re-runs this with that question settled.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_sequence_export(&self, shared: &mut Shared) {
        use export_sequence as es;
        let Some(dest) = shared.ui.file_path.lock().unwrap().first().cloned() else { return };
        let sheets_mode = shared.ui.sequence_sheets;
        let mut saved = shared.ui.sequence_opts.clone();
        // a fresh choice of where to export (Export Again sets this itself)
        saved.sync(&shared.armature);
        let opts = export_sequence::effective(&saved, sheets_mode);
        let fail = |shared: &mut Shared, msg: String| {
            shared.ui.sequence_overwrite_ok = false;
            shared.ui.sequence_framing_ok = false;
            editor::open_modal(&mut shared.ui, false, msg);
        };

        let armature = rest_armature(shared);
        let frames = match es::plan(&armature, &opts) {
            Ok(frames) => frames,
            Err(e) => return fail(shared, e),
        };
        let Some(camera) = es::export_camera(&armature, &opts) else {
            return fail(shared, "Nothing to render: the chosen animations have no visible art.".into());
        };
        let (_, out) = es::output_size(&camera, &opts);
        // render at up to twice the output size and shrink, or straight at a larger size (§5.1)
        let render = es::render_size(camera.px_size(), out);
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        if render.0 > max || render.1 > max {
            let msg = format!("The frames would be {}×{} px, over this GPU's limit of {max} px. Use a smaller output size or camera scale.", render.0, render.1);
            return fail(shared, msg);
        }
        let rig = shared
            .ui
            .save_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or("rig".to_string(), |s| es::sanitize(&s.to_string_lossy()));
        // a grid is laid out now; trimmed frames are packed once the writer has them all
        let grid = if sheets_mode && !opts.trim {
            match es::pack_sheets(&rig, &armature, &frames, out, &opts) {
                Ok(plan) => Some(plan),
                Err(e) => return fail(shared, e),
            }
        } else {
            None
        };
        if sheets_mode && opts.trim && (out.0 > opts.sheet_max || out.1 > opts.sheet_max) {
            let msg = format!("Each frame is up to {}×{} px, larger than the {} px sheet limit. Use a smaller output size or a larger sheet size.", out.0, out.1, opts.sheet_max);
            return fail(shared, msg);
        }
        // the data files' names (their contents come at the end)
        let extra_names: Vec<String> = es::extra_files(&rig, &armature, &camera, &opts, &frames, &es::Layout { sheets: grid.as_ref(), ..Default::default() })
            .into_iter()
            .map(|(n, _)| n)
            .collect();

        // frames leaving a chosen camera (an Auto camera fits them all)
        if opts.camera.is_some() && !shared.ui.sequence_framing_ok {
            let outside = es::outside_frames(&armature, &frames, &camera);
            if !outside.is_empty() {
                let msg = shared
                    .ui
                    .loc("polar.frames_outside")
                    .replace("$camera", &camera.name)
                    .replace("$frames", &outside.join("\n- "));
                shared.events.open_polar_modal(PolarId::FramesOutside, msg);
                return;
            }
        }

        let target = if opts.zip {
            let path = if dest.extension().is_none() { dest.with_extension("zip") } else { dest.clone() };
            match std::fs::File::create(&path) {
                Ok(file) => es::Target::Zip(zip::ZipWriter::new(file)),
                Err(e) => return fail(shared, format!("Couldn't create {}: {e}", path.display())),
            }
        } else {
            let ext = opts.format.ext();
            let mut paths: Vec<String> = match &grid {
                Some(plan) => plan.files.clone(),
                // packed sheets' names aren't known yet: the likely ones
                None if sheets_mode => vec![format!("{rig}.{ext}"), format!("{rig}_0.{ext}")],
                None => frames.iter().map(|f| f.path.clone()).collect(),
            };
            paths.extend(extra_names.iter().cloned());
            let existing = es::existing_files(&dest, &paths);
            if existing > 0 && !shared.ui.sequence_overwrite_ok {
                let msg = shared.ui.loc("polar.overwrite_frames").replace("$n", &existing.to_string());
                shared.events.open_polar_modal(PolarId::OverwriteFrames, msg);
                return;
            }
            es::Target::Folder(dest.clone())
        };
        shared.ui.sequence_overwrite_ok = false;
        shared.ui.sequence_framing_ok = false;

        let background = camera.background.unwrap_or(Color::new(0, 0, 0, 0));
        let follow = es::follow_offsets(&armature, &frames, &camera);
        let motion = es::root_motion(&camera, &opts, &follow);

        // the writer builds the data files once it knows the layout
        let light = Armature { animations: armature.animations.clone(), ..Default::default() };
        let (f_rig, f_cam, f_opts, f_frames) = (rig.clone(), camera.clone(), opts.clone(), frames.clone());
        let finish: es::Finisher = Box::new(move |layout: &es::Layout| {
            let layout = es::Layout { motion: motion.as_deref(), ..*layout };
            es::extra_files(&f_rig, &light, &f_cam, &f_opts, &f_frames, &layout)
        });
        let mode = match grid {
            Some(plan) => es::WriteMode::Grid(plan),
            None if sheets_mode => {
                let names = Armature { animations: armature.animations.clone(), ..Default::default() };
                let (p_rig, p_frames, p_opts) = (rig.clone(), frames.clone(), opts.clone());
                es::WriteMode::Packed(Box::new(move |sizes: &[(u32, u32)]| es::pack_trimmed(&p_rig, &names, &p_frames, sizes, &p_opts)))
            }
            None => es::WriteMode::Files,
        };
        let encoding = es::Encoding::new(&opts, camera.background);
        shared.sequence_job = Some(es::Job {
            rest_bones: armature.bones.clone(),
            armature,
            frames,
            next: 0,
            anim: usize::MAX,
            camera,
            follow,
            render,
            out,
            clear: background,
            writer: es::Writer::spawn(target, render, out, encoding, mode, finish),
            dest: dest.clone(),
            rig,
            sheets: sheets_mode,
        });
        // remembered for Export Again
        saved.last_dest = Some(dest.to_string_lossy().to_string());
        saved.last_sheets = sheets_mode;
        shared.ui.sequence_opts = saved;
        editor::open_modal(&mut shared.ui, true, "Exporting frames...".to_string());
    }

    /// A captured frame's pixels, row by row without the row padding, in the capture's
    /// own channel order (see `process_screenshot_raw`, which also swaps to RGBA).
    #[cfg(not(target_arch = "wasm32"))]
    fn read_rows(&self, buffer: &wgpu::Buffer, (width, height): (u32, u32)) -> Vec<u8> {
        let wait = wgpu::PollType::Wait { submission_index: None, timeout: None };
        let _ = self.gpu.device.poll(wait);
        let view = buffer.slice(..).get_mapped_range();
        let row = width as usize * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = (row + align - 1) / align * align;
        let mut pixels = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            pixels.extend_from_slice(&view[y * padded..y * padded + row]);
        }
        pixels
    }

    /// Render the next frames of an export and hand them to its writer.
    #[cfg(not(target_arch = "wasm32"))]
    fn step_sequence_export(&self, shared: &mut Shared) {
        // render while there's time this frame and the writer keeps up
        const BUDGET_MS: u128 = 60;
        let Some(job) = shared.sequence_job.as_mut() else { return };
        let (base_view, aspect) = job.camera.view();
        let size = Vec2::new(job.render.0 as f32, job.render.1 as f32);
        let mut mapped = Arc::new(Mutex::new(0));
        let started = Instant::now();
        let mut failed = false;

        while job.next < job.frames.len() && job.writer.has_room() && started.elapsed().as_millis() < BUDGET_MS {
            let planned = job.frames[job.next].clone();
            // each animation starts from the rest pose, so physics starts fresh
            if planned.anim != job.anim {
                job.armature.bones = job.rest_bones.clone();
                job.anim = planned.anim;
            }
            let arm = &mut job.armature;
            arm.bones = arm.animate(planned.anim, planned.frame, None);
            arm.animated_bones = arm.bones.clone();
            // non-mesh bones get their texture's rectangle
            for b in 0..arm.bones.len() {
                let tex = arm.anim_tex_of(arm.bones[b].id);
                if let (Some(tex), false) = (tex, arm.bones[b].verts_edited) {
                    let size = tex.size;
                    let (v, i) = renderer::create_tex_rect(&size);
                    (arm.bones[b].vertices, arm.bones[b].indices) = (v.clone(), i.clone());
                    (arm.animated_bones[b].vertices, arm.animated_bones[b].indices) = (v, i);
                    arm.animated_bones[b].verts_edited = false;
                }
            }
            let mut view = base_view.clone();
            view.pos += job.follow[job.next];
            let mut frames = vec![];
            #[rustfmt::skip]
            self.capture(size, arm, &view, aspect, &job.clear, &mut frames, &mut mapped, &shared.renderer);
            let frame = frames.pop().unwrap();
            let pixels = self.read_rows(&frame.buffer, job.render);
            drop(frame);
            let bgra = matches!(self.gpu.surface_format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
            job.next += 1;
            if !job.writer.send(export_sequence::RawFrame { path: planned.path, pixels, bgra }) {
                failed = true;
                break;
            }
        }
        if job.next >= job.frames.len() || failed {
            job.writer.close();
        }

        let total = job.frames.len();
        let written = job.writer.written.load(std::sync::atomic::Ordering::SeqCst);
        let Some(result) = job.writer.result() else {
            shared.ui.headline = format!("Exporting frames... {written} / {total}");
            return;
        };

        // the writer has finished (or failed)
        let job = shared.sequence_job.take().unwrap();
        let msg = match result {
            Ok(()) if written == total => {
                let sheet_count = job.writer.sheets.load(std::sync::atomic::Ordering::SeqCst);
                let on_sheets = match (job.sheets, sheet_count) {
                    (false, _) => String::new(),
                    (true, 1) => " on 1 spritesheet".to_string(),
                    (true, n) => format!(" on {n} spritesheets"),
                };
                let mut msg = format!(
                    "Exported {total} frames ({}×{} px){on_sheets} to:\n\n`{}`",
                    job.out.0,
                    job.out.1,
                    job.dest.display()
                );
                let opts = export_sequence::effective(&shared.ui.sequence_opts, job.sheets);
                msg += "\n\n";
                msg += &export_sequence::engine_notes(&job.rig, &job.camera, &opts, job.sheets.then_some(sheet_count));
                msg
            }
            Ok(()) => format!("The export stopped after {written} of {total} frames."),
            Err(e) => format!("The export stopped after {written} of {total} frames: {e}"),
        };
        editor::open_modal(&mut shared.ui, false, msg);
    }

    fn skf_export_videos(&self, armature: &Armature, shared_ui: &mut Ui) {
        let path;
        #[cfg(target_arch = "wasm32")]
        {
            path = PathBuf::new();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            path = shared_ui.file_path.lock().unwrap()[0].clone();

            // attempt to remove the existing dir, if it exists
            let can_proceed = if let Ok(exists) = std::fs::exists(&path) {
                if exists {
                    if let Err(e) = std::fs::remove_dir_all(&path) {
                        eprintln!("{}", e);
                        false
                    } else {
                        true
                    }
                } else {
                    true
                }
            } else {
                true
            };
            if !can_proceed {
                return;
            }
            let _ = std::fs::create_dir(&path).unwrap();
        }
        self.skf_pack_videos(armature, shared_ui, &path);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn skf_native_spritesheet(&self, armature: &Armature, shared_ui: &mut Ui) {
        let path = shared_ui.file_path.lock().unwrap()[0].clone();
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let options = zip::write::FullFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        self.skf_pack_sprites(armature, shared_ui, &mut zip, &options);

        _ = zip.finish();
    }

    #[cfg(target_arch = "wasm32")]
    fn skf_web_spritesheet(&self, armature: &Armature, shared_ui: &mut Ui) {
        let mut buf: Vec<u8> = Vec::new();
        let cursor = std::io::Cursor::new(&mut buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::FullFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        self.skf_pack_sprites(armature, shared_ui, &mut zip, &options);

        let bytes = zip.finish().unwrap().into_inner().to_vec();
        downloadZip(bytes, Saving::Spritesheet.to_string());
    }

    // sprite-packing stuff that applies to both native and web
    fn skf_pack_sprites<W: Write + Seek>(
        &self,
        armature: &Armature,
        shared_ui: &mut Ui,
        zip: &mut zip::ZipWriter<W>,
        options: &FullFileOptions,
    ) {
        if shared_ui.image_sequences {
            let bufs = utils::encode_sequence(armature, shared_ui, self);

            let mut buf_idx = 0;
            for a in 0..shared_ui.exporting_anims.len() {
                if !shared_ui.exporting_anims[a] {
                    continue;
                }
                zip.add_directory(armature.animations[a].name.clone(), options.clone())
                    .unwrap();
                for b in 0..bufs[buf_idx].len() {
                    let png_name =
                        armature.animations[a].name.to_string() + "/" + &b.to_string() + ".png";
                    zip.start_file(png_name, options.clone()).unwrap();
                    zip.write(&bufs[buf_idx][b]).unwrap();
                }
                buf_idx += 1;
            }
        } else {
            let bufs = utils::encode_spritesheets(armature, shared_ui, self);
            for b in 0..bufs.len() {
                let png_name = b.to_string() + ".png";
                zip.start_file(png_name, options.clone()).unwrap();
                zip.write(&bufs[b]).unwrap();
            }
        }
    }

    fn skf_pack_videos(&self, armature: &Armature, shared_ui: &mut Ui, path: &PathBuf) {
        #[rustfmt::skip]
        let ffmpeg_bin = if shared_ui.use_system_ffmpeg || cfg!(not(target_os = "windows")) {
            utils::system_ffmpeg()
        } else {
            utils::bin_path().join("ffmpeg.exe").to_str().unwrap().to_string()
        };

        let bufs = utils::encode_sequence(armature, shared_ui, self);
        let mut buf_idx = 0;
        let size = shared_ui.sprite_size;
        for a in 0..armature.animations.len() {
            let mut base = path.clone();
            if !shared_ui.exporting_anims[a] {
                continue;
            }
            base = base.join(armature.animations[a].name.clone());
            let mut _path_str = &base.to_str().unwrap().to_string();
            let fps = armature.animations[a].fps;
            #[cfg(target_arch = "wasm32")]
            {
                _path_str = &armature.animations[a].name;
            }
            shared_ui.custom_error = if shared_ui.exporting_video_type == ExportVideoType::Mp4 {
                Self::encode_video(bufs[buf_idx].clone(), fps, size, _path_str, &ffmpeg_bin)
            } else {
                Self::encode_gif(bufs[buf_idx].clone(), fps, size, _path_str, &ffmpeg_bin)
            };

            if shared_ui.custom_error != "" {}
            buf_idx += 1;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&mut self, shared: &mut Shared) {
        let mut save_path = "".to_string();
        let saving_type = shared.ui.saving.lock().unwrap().clone();
        match saving_type {
            Saving::CustomPath => {
                let path = &shared.ui.file_path.lock().unwrap()[0];
                save_path = path.as_path().to_str().unwrap().to_string();
                shared.ui.changed_window_name = false;
                shared.events.open_modal("saving", true);
                *shared.ui.save_finished.lock().unwrap() = false;
                shared.ui.save_path = Some(path.clone());
                shared.ui.can_quit = false;
                if !shared.ui.recent_file_paths.contains(&save_path) {
                    shared.ui.recent_file_paths.push(save_path.clone());
                }
            }
            Saving::Exporting => {
                let path = &shared.ui.file_path.lock().unwrap()[0];
                save_path = path.as_path().to_str().unwrap().to_string();
                shared.events.open_modal("exporting", true);
                *shared.ui.export_finished.lock().unwrap() = false;
                shared.ui.can_quit = false;
                utils::save_to_recent_files(shared.ui.recent_file_paths.clone());
            }
            Saving::Autosaving => {
                let dir_init = directories_next::ProjectDirs::from("com", "keyternity", "keyternity");
                let dir = dir_init.unwrap().data_dir().to_str().unwrap().to_string();
                save_path = dir + "/autosave.skf";
                let freq = shared.config.autosave_frequency as f32;
                let in_cooldown = shared.edit_mode.time - shared.last_autosave < freq;
                if in_cooldown {
                    *shared.ui.saving.lock().unwrap() = Saving::None;
                    return;
                }
                if !shared.ui.recent_file_paths.contains(&save_path) {
                    shared.ui.recent_file_paths.push(save_path.clone());
                }
                shared.last_autosave = shared.edit_mode.time;
            }
            _ => {}
        }

        utils::save_to_recent_files(shared.ui.recent_file_paths.clone());

        let mut frames = vec![];
        #[rustfmt::skip]
        self.take_screenshot(
            shared.screenshot_res, &mut shared.armature, &shared.camera, &shared.config.colors.background,
            &mut frames, &mut shared.ui.mapped_frames, &shared.renderer
        );
        let buffer = frames[0].buffer.clone();
        let screenshot_res = shared.screenshot_res;

        let mut armature = rest_armature(shared);
        let camera = shared.camera.clone();
        let mut edit_mode = shared.edit_mode.clone();
        // runtimes play Base only: exports bake the layers into it
        edit_mode.export_flatten_layers = *shared.ui.saving.lock().unwrap() == Saving::Exporting;
        let sequence_export = Some(shared.ui.sequence_opts.clone());

        // clear export options
        shared.edit_mode.export_bake_ik = false;
        shared.edit_mode.export_exclude_ik = false;
        shared.edit_mode.export_clear_color = Color::new(0, 0, 0, 0);
        shared.edit_mode.export_img_format = ExportImgFormat::PNG;

        let was_exporting = *shared.ui.saving.lock().unwrap() == Saving::Exporting;
        let autosaving = *shared.ui.saving.lock().unwrap() == Saving::Autosaving;
        *shared.ui.saving.lock().unwrap() = Saving::None;

        let save_finished = Arc::clone(&shared.ui.save_finished);
        let export_finished = Arc::clone(&shared.ui.export_finished);
        let device = self.gpu.device.clone();
        let surface_format = self.gpu.surface_format;
        std::thread::spawn(move || {
            let mut png_bufs = vec![];
            let mut sizes = vec![];

            if armature.styles.len() > 0 && armature.styles[0].textures.len() > 0 {
                (png_bufs, sizes) = utils::create_tex_sheet(&mut armature, &edit_mode);
            }

            let (armatures_json, editor_json) =
                utils::prepare_files(&armature, camera, sizes.clone(), &edit_mode, sequence_export);

            // create zip file
            let mut zip = zip::ZipWriter::new(std::fs::File::create(save_path.clone()).unwrap());

            let options = zip::write::FullFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            let thumb_buf =
                utils::process_screenshot(&buffer, &device, surface_format, screenshot_res);

            // save relevant files into the zip
            zip.start_file("armature.json", options.clone()).unwrap();
            zip.write(armatures_json.as_bytes()).unwrap();
            zip.start_file("editor.json", options.clone()).unwrap();
            zip.write(editor_json.as_bytes()).unwrap();
            zip.start_file("thumbnail.png", options.clone()).unwrap();
            zip.write(&thumb_buf).unwrap();
            zip.start_file("readme.md", options.clone()).unwrap();
            zip.write(include_bytes!("../assets/skf_readme.md"))
                .unwrap();
            let atlas_ext = match edit_mode.export_img_format {
                ExportImgFormat::PNG => ".png",
                ExportImgFormat::JPG => ".jpg",
            };
            for i in 0..png_bufs.len() {
                let atlas_name = format!("atlas{}{}", i.to_string(), atlas_ext);
                zip.start_file(atlas_name, options.clone()).unwrap();
                zip.write(&png_bufs[i]).unwrap();
            }

            zip.finish().unwrap();

            let _ = std::fs::copy(save_path.clone(), save_path + "~");

            // trigger saving modal if manually saving
            if !autosaving {
                if was_exporting {
                    *export_finished.lock().unwrap() = true;
                } else {
                    *save_finished.lock().unwrap() = true;
                }
            }
        });
    }

    /// Write `<base>_ske.json`, `<base>_tex.json` and `<base>_tex.png` next to the picked path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_dragonbones(&mut self, shared: &mut Shared) {
        *shared.ui.saving.lock().unwrap() = Saving::None;
        let path = shared.ui.file_path.lock().unwrap()[0].clone();
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("character");
        let base = dragonbones_export::base_name(file_name);

        let pad = shared.edit_mode.export_tex_padding;
        let armature = rest_armature(shared);
        let result = dragonbones_export::export(&armature, &base, pad).and_then(|files| {
            let write = |name: String, bytes: &[u8]| {
                fs::write(dir.join(&name), bytes).map_err(|e| format!("{}: {}", name, e))
            };
            write(format!("{}_ske.json", base), files.ske_json.as_bytes())?;
            write(format!("{}_tex.json", base), files.tex_json.as_bytes())?;
            write(format!("{}_tex.png", base), &files.tex_png)
        });

        if let Err(err) = result {
            shared.ui.custom_error = err;
            let str = shared.ui.loc("error_skf");
            editor::open_modal(&mut shared.ui, false, str);
        }
    }

    /// Render `armature` into a mapped buffer: square world units, cleared to
    /// `clear_color` with no alpha.
    pub fn take_screenshot(
        &self,
        screenshot_res: Vec2,
        armature: &mut Armature,
        camera: &Camera,
        clear_color: &Color,
        rendered_frames: &mut Vec<RenderedFrame>,
        mapped_frames: &mut Arc<Mutex<usize>>,
        renderer: &Renderer,
    ) {
        let clear = Color::new(clear_color.r, clear_color.g, clear_color.b, 0);
        #[rustfmt::skip]
        self.capture(screenshot_res, armature, camera, 1., &clear, rendered_frames, mapped_frames, renderer);
    }

    /// `take_screenshot` with x scaled by `aspect` (height / width, as
    /// `RenderCamera::view` gives it) and `clear_color`'s alpha kept.
    pub fn capture(
        &self,
        screenshot_res: Vec2,
        armature: &mut Armature,
        camera: &Camera,
        aspect: f32,
        clear_color: &Color,
        rendered_frames: &mut Vec<RenderedFrame>,
        mapped_frames: &mut Arc<Mutex<usize>>,
        renderer: &Renderer,
    ) {
        let width = screenshot_res.x as u32;
        let height = screenshot_res.y as u32;

        let format;
        match self.gpu.surface_format {
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                format = wgpu::TextureFormat::Bgra8Unorm;
            }
            _ => format = wgpu::TextureFormat::Rgba8Unorm,
        }
        let capture_texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            label: Some("Capture Texture"),
            view_formats: &[],
        });

        let capture_view = capture_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let device = &self.gpu.device;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        {
            let mut capture_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Capture Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &capture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color.r as f64 / 255.,
                            g: clear_color.g as f64 / 255.,
                            b: clear_color.b as f64 / 255.,
                            a: clear_color.a as f64 / 255.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            capture_pass.set_pipeline(&self.scene.pipeline);

            // render armature with for screenshot purposes
            let queue = &self.gpu.queue;
            renderer::render_screenshot(&mut capture_pass, armature, &camera, aspect, renderer, queue);
        }

        // pad screenshot width to a multiple of 256
        let bytes_per_pixel = 4;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u32;
        let padded_bytes_per_row = ((unpadded_bytes_per_row + align - 1) / align) * align;

        let buffer_size = (padded_bytes_per_row * height) as u64;
        let output_buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Readback Buffer"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &capture_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        rendered_frames.push(RenderedFrame {
            buffer: output_buffer.clone(),
            width,
            height,
        });

        let arc_mapped_frames = Arc::clone(mapped_frames);
        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        let buffer_slice = output_buffer.slice(..);
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            if let Ok(()) = result {
                *arc_mapped_frames.lock().unwrap() += 1;
            } else {
                println!("Failed to map buffer for read.");
            }
        });
    }

    fn encode_video(
        rendered_frames: Vec<Vec<u8>>,
        fps: i32,
        window: Vec2,
        path: &String,
        _ffmpeg_bin: &String,
    ) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        {
            #[rustfmt::skip]
            let mut child = Command::new(_ffmpeg_bin)
                .args(["-y", "-f", "rawvideo", "-pixel_format", "rgba", "-video_size", &format!("{}x{}", window.x, window.y), 
                    "-framerate", &fps.to_string(), "-i", "pipe:0", 
                    // disabled: manual encoder codec - not needed for now
                    // "-c:v", codec, 
                    "-pix_fmt", "yuv420p",
                    &format!("{}.mp4", path.to_string())])
                .stdin(Stdio::piped())
                .spawn();

            if let Err(e) = child {
                return format!("spawn ffmpeg: {}", e.to_string());
            }

            // process video frames
            let stdin = child.as_mut().unwrap().stdin.as_mut().unwrap();
            for frame in &rendered_frames {
                let rgb = image::load_from_memory(&frame).unwrap();
                if let Err(e) = stdin.write_all(&rgb.to_rgba8()) {
                    return format!("stdin: {}", e.to_string());
                }
            }

            drop(child.as_mut().unwrap().stdin.take());
            child.as_mut().unwrap().wait().unwrap();
        }

        #[cfg(target_arch = "wasm32")]
        {
            let mut raw_video = vec![];
            for frame in rendered_frames {
                let rgb = image::load_from_memory(&frame).unwrap();
                for chunk in rgb.to_rgba8().chunks_exact(4) {
                    raw_video.push(chunk[0]);
                    raw_video.push(chunk[1]);
                    raw_video.push(chunk[2]);
                }
            }
            let split: Vec<&str> = path.split('/').collect();
            downloadMp4(raw_video, window.x, window.y, split.last().unwrap(), fps);
        }

        "".to_string()
    }

    fn encode_gif(
        rendered_frames: Vec<Vec<u8>>,
        fps: i32,
        window: Vec2,
        path: &String,
        _ffmpeg_bin: &String,
    ) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            let split: Vec<&str> = path.split('/').collect();
            for frame in rendered_frames {
                addGifFrame(frame);
            }
            downloadGif(window.x, window.y, split.last().unwrap(), fps);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            #[rustfmt::skip]
            let mut child = Command::new(_ffmpeg_bin)
            .args(["-y", "-f", "rawvideo", "-pixel_format", "rgba", "-video_size", &format!("{}x{}", window.x, window.y), 
                "-framerate", &fps.to_string(), "-i", "pipe:0", "-filter_complex",
                "[0:v] fps=30,split [a][b]; \
                 [a] palettegen=stats_mode=diff [p]; \
                 [b][p] paletteuse=dither=sierra2_4a",
                "-loop", "0",
                &format!("{}.gif", path)
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn();

            if let Err(e) = child {
                return format!("spawn ffmpeg: {}", e.to_string());
            }

            let stdin = child.as_mut().unwrap().stdin.as_mut().unwrap();
            for frame in &rendered_frames {
                let rgb = image::load_from_memory(&frame).unwrap();
                if let Err(e) = stdin.write_all(&rgb.to_rgba8()) {
                    return "stdin: ".to_string() + &e.to_string();
                }
            }

            child.as_mut().unwrap().wait().unwrap();
            let _ = std::fs::remove_file("palette.png");
        }

        "".to_string()
    }
}

pub struct Gpu {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface_format: wgpu::TextureFormat,
}

impl Gpu {
    pub fn aspect_ratio(&self) -> f32 {
        self.surface_config.width as f32 / self.surface_config.height.max(1) as f32
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    pub async fn new_async(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Self {
        let surface: wgpu::Surface;
        #[allow(unused_mut, unused_assignments)]
        let mut instance = wgpu::Instance::new(&InstanceDescriptor::default());

        // force DX12 on Windows
        #[cfg(target_os = "windows")]
        {
            let backends = wgpu::Backend::Dx12;
            instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: backends.into(),
                ..Default::default()
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..Default::default()
            });
        }

        surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        let (device, queue) = {
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("WGPU Device"),
                    memory_hints: wgpu::MemoryHints::default(),
                    required_features: wgpu::Features::default(),
                    #[cfg(not(target_arch = "wasm32"))]
                    required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
                    #[cfg(all(target_arch = "wasm32"))]
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                    trace: wgpu::Trace::Off,
                    experimental_features: ExperimentalFeatures::disabled(),
                })
                .await
                .expect("Failed to request a device!")
        };

        let surface_capabilities = surface.get_capabilities(&adapter);

        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb()) // egui wants a non-srgb surface texture
            .unwrap_or(surface_capabilities.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        Self {
            surface,
            device,
            queue,
            surface_config,
            surface_format,
        }
    }
}

struct Scene {
    pub pipeline: wgpu::RenderPipeline,
    pub blit_pipeline: wgpu::RenderPipeline,
}

impl Scene {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        bind_group_layout: &BindGroupLayout,
    ) -> Self {
        let pipeline = Self::create_pipeline(device, surface_format, &bind_group_layout);
        let blit_pipeline = Self::create_blit_pipeline(device, surface_format);

        Self {
            pipeline,
            blit_pipeline,
        }
    }

    fn create_blit_pipeline(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Blit Bind Group Layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blit Pipeline Layout"),
            bind_group_layouts: &[&blit_bind_group_layout],
            push_constant_ranges: &[],
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Blit Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blit Pipeline"),
            layout: Some(&blit_pipeline_layout),

            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                buffers: &[], // fullscreen triangle, no vertex buffer
                compilation_options: Default::default(),
            },

            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),

            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },

            depth_stencil: None,

            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },

            multiview: None,
            cache: None,
        })
    }

    fn create_pipeline(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        bind_group_layout: &BindGroupLayout,
    ) -> wgpu::RenderPipeline {
        let shader_str = &String::from_utf8(include_bytes!("shader.wgsl").to_vec())
            .unwrap()
            .to_string();
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(shader_str)),
        });

        let attributes =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4].to_vec();
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GpuVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        };

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("test"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vertex_main"),
                buffers: &[vertex_layout],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
                unclipped_depth: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview: None,
            cache: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::shared::{AnimElement, Shared, Vec2};
    use crate::{armature_window, file_reader};

    fn init_shared() -> Shared {
        let mut shared = Shared::default();
        shared.armature.new_bone(-1);
        shared.armature.new_bone(-1);
        shared.armature.new_bone(-1);
        armature_window::drag_bone(&mut shared.armature, false, 2, 1);
        armature_window::drag_bone(&mut shared.armature, false, 1, 0);
        shared.ui.init_empty_loc();
        shared
    }

    // todo: add headless wgpu and egui to test those that depend on it

    #[test]
    fn import_skf() {
        let mut shared = init_shared();
        *shared.ui.file_name.lock().unwrap() = "./samples/skellington.skf".to_string();
        *shared.ui.import_contents.lock().unwrap() = vec![0];
        file_reader::read_import(&mut shared, None, None, None, None);
        assert_eq!(shared.armature.bones[0].name != "New Bone", true);
        assert_eq!(shared.armature.styles.len() > 0, true);
    }

    // check if exported skf is same as imported
    #[test]
    fn export_skf() {
        let mut shared = init_shared();
        *shared.ui.file_name.lock().unwrap() = "./samples/skellington.skf".to_string();
        *shared.ui.import_contents.lock().unwrap() = vec![0];
        file_reader::read_import(&mut shared, None, None, None, None);
        assert_eq!(shared.armature.bones[0].name != "New Bone", true);
        assert_eq!(shared.armature.styles.len() > 0, true);
    }

    #[test]
    fn import_psd() {
        let mut shared = init_shared();
        *shared.ui.file_name.lock().unwrap() = "./samples/skellington.psd".to_string();
        *shared.ui.import_contents.lock().unwrap() = vec![0];
        file_reader::read_import(&mut shared, None, None, None, None);
        assert_eq!(shared.armature.bones[0].name != "New Bone", true);
        assert_eq!(shared.armature.styles.len() > 0, true);
    }

    #[test]
    fn drag_bone_above() {
        let mut shared = init_shared();
        shared.armature.bones[0].name = "Bone0".to_string();
        shared.armature.bones[1].name = "Bone1".to_string();
        shared.armature.bones[2].name = "Bone2".to_string();
        armature_window::drag_bone(&mut shared, true, 2, 1);
        assert_eq!(shared.armature.bones[1].name, "Bone2");
    }

    #[test]
    fn drag_bone_directly() {
        let mut shared = init_shared();
        shared.armature.bones[0].name = "Bone0".to_string();
        shared.armature.bones[1].name = "Bone1".to_string();
        shared.armature.bones[2].name = "Bone2".to_string();
        armature_window::drag_bone(&mut shared, false, 2, 1);
        assert_eq!(shared.armature.bones[2].parent_id, 1);
    }
}

/// The armature as it should be saved/exported: with the rest pose, even mid Pose Mode.
fn rest_armature(shared: &Shared) -> Armature {
    let rest = shared
        .edit_mode
        .pose_mode
        .then_some(&shared.edit_mode.pose_snapshot);
    bind_pose::rest_armature(&shared.armature, rest)
}
