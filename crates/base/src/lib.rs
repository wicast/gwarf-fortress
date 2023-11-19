pub mod asset;
pub mod camera;
pub mod texture;

use std::time::Duration;

use as_any::{AsAny, Downcast};
use camera::{Camera, CameraController, CameraUniform};
use env_logger::Env;
use image::ImageError;
use snafu::{Backtrace, Snafu};
use texture::Texture;
use typed_builder::TypedBuilder;
use wgpu::{util::DeviceExt, Backend, Backends, InstanceFlags};
use winit::{
    event::{
        DeviceEvent, ElementState, Event, KeyboardInput, MouseButton, VirtualKeyCode, WindowEvent,
    },
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

pub use bytemuck;
pub use env_logger;
pub use glam;
pub use image;
pub use snafu;
pub use wgpu;
pub use winit;

type GetConfigFn = fn() -> (wgpu::Backends, wgpu::Features);
type InitFn = fn(base_state: &mut BaseState) -> Result<(), Error>;
type TickFn = fn(base_state: &mut BaseState, dt: Duration) -> Result<(), Error>;
type RenderFn = fn(base_state: &mut BaseState, dt: Duration) -> Result<(), Error>;
type ResizeFn =
    fn(base_state: &mut BaseState, new_size: winit::dpi::PhysicalSize<u32>) -> Result<(), Error>;

pub trait StateDynObj: AsAny {}

pub fn downcast_mut<OT: 'static>(state: &mut Box<dyn StateDynObj>) -> Option<&mut OT> {
    state.as_mut().downcast_mut::<OT>()
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
    GLTFErr { source: asset::gltf::Error },
    NoneErr { backtrace: Backtrace },
    ImageLoadErr { source: ImageError },
    SurfaceErr { source: wgpu::SurfaceError },
}

pub struct BaseState {
    pub surface: wgpu::Surface,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub window: Window,
    pub depth: Texture,

    pub camera: Camera,
    pub camera_controller: CameraController,
    pub camera_uniform: CameraUniform,
    pub camera_buffer: wgpu::Buffer,
    pub camera_bind_group_layout: wgpu::BindGroupLayout,
    pub camera_bind_group: wgpu::BindGroup,

    pub mouse_pressed: bool,

    pub extra_state: Option<Box<dyn StateDynObj>>,
    pub render_fn: Option<RenderFn>,
    pub tick_fn: Option<TickFn>,
    pub resize_fn: Option<ResizeFn>,
}

impl BaseState {
    // Creating some of the wgpu types requires async code
    //TODO error
    fn new(window: Window, app: &App) -> Self {
        let (backends, features) = app.config.map_or((wgpu::Backends::all(), wgpu::Features::empty()), |i|i);

        let size = window.inner_size();

        // The instance is a handle to our GPU
        // Backends::all => Vulkan + Metal + DX12 + Browser WebGPU
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            dx12_shader_compiler: Default::default(),
            flags: InstanceFlags::debugging(),
            gles_minor_version: Default::default(),
        });

        // # Safety
        //
        // The surface needs to live as long as the window that created it.
        // State owns the window so this should be safe.
        let surface = unsafe { instance.create_surface(&window) }.unwrap();

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                features,
                // WebGL doesn't support all of wgpu's features, so if
                // we're building for the web we'll have to disable some.
                limits: if cfg!(target_arch = "wasm32") {
                    wgpu::Limits::downlevel_webgl2_defaults()
                } else {
                    wgpu::Limits::default()
                },
                label: None,
            },
            None, // Trace path
        ))
        .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an sRGB surface texture. Using a different
        // one will result all the colors coming out darker. If you want to support non
        // sRGB surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let projection = camera::Projection::new(config.width, config.height, 45.0, 0.1, 100.0);
        let camera = Camera::new((0.0, 5.0, 10.0), -90.0, -20.0, projection);
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera);
        //TODO the camera sensitivity is different when using cgmath
        let camera_controller = camera::CameraController::new(4.0, 10.0);

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("camera_bind_group_layout"),
            });
        let camera_bind_group: wgpu::BindGroup =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &camera_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                }],
                label: Some("camera_bind_group"),
            });

        let depth_texture =
            texture::Texture::create_depth_texture(&device, &config, "depth_texture", false);

        let mut base_state = Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            render_fn: app.render_fn,
            tick_fn: app.tick_fn,
            extra_state: Default::default(),
            camera,
            camera_controller,
            camera_uniform,
            mouse_pressed: false,
            camera_bind_group,
            camera_buffer,
            camera_bind_group_layout,
            depth: depth_texture,
            resize_fn: app.resize_fn,
        };
        //TODO deal with error
        let init_result = app.init_fn.map_or(Ok(()), |f| f(&mut base_state));
        init_result.unwrap();
        base_state
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.camera.proj.resize(new_size.width, new_size.height);
        }
        self.depth = texture::Texture::create_depth_texture(
            &self.device,
            &self.config,
            "depth_texture",
            false,
        );

        if let Some(f) = self.resize_fn {
            f(self, new_size).unwrap();
        }
    }

    fn input(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        virtual_keycode: Some(key),
                        state,
                        ..
                    },
                ..
            } => self.camera_controller.process_keyboard(*key, *state),
            WindowEvent::MouseWheel { delta, .. } => {
                self.camera_controller.process_scroll(delta);
                true
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => {
                self.mouse_pressed = *state == ElementState::Pressed;
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, dt: Duration) -> Result<(), Error> {
        self.camera_controller.update_camera(&mut self.camera, dt);
        self.camera_uniform.update_view_proj(&self.camera);
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
        if let Some(tick_fn) = self.tick_fn {
            tick_fn(self, dt)
        } else {
            Ok(())
        }
    }

    fn render(&mut self, dt: Duration) -> Result<(), Error> {
        if let Some(render_fn) = self.render_fn {
            render_fn(self, dt)
        } else {
            Ok(())
        }
    }
}

#[derive(TypedBuilder)]
pub struct App {
    #[builder(default, setter(strip_option))]
    config: Option<(wgpu::Backends, wgpu::Features)>,
    #[builder(default, setter(strip_option))]
    init_fn: Option<InitFn>,
    #[builder(default, setter(strip_option))]
    tick_fn: Option<TickFn>,
    #[builder(default, setter(strip_option))]
    render_fn: Option<RenderFn>,
    #[builder(default, setter(strip_option))]
    resize_fn: Option<ResizeFn>,
}

impl App {
    pub fn run(&mut self) {
        env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new().build(&event_loop).unwrap();
        let mut last_render_time = std::time::Instant::now();

        let mut state = BaseState::new(window, &self);
        event_loop.run(move |event, _, control_flow| {
            match event {
                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion{ delta, },
                    .. // We're not using device_id currently
                } => if state.mouse_pressed {
                    //TODO deal all events in one place
                    state.camera_controller.process_mouse(delta.0, delta.1)
                }
                Event::WindowEvent {
                    ref event,
                    window_id,
                } if window_id == state.window().id() => {
                    if !state.input(event) {
                        match event {
                            WindowEvent::CloseRequested
                            | WindowEvent::KeyboardInput {
                                input:
                                    KeyboardInput {
                                        state: ElementState::Pressed,
                                        virtual_keycode: Some(VirtualKeyCode::Escape),
                                        ..
                                    },
                                ..
                            } => *control_flow = ControlFlow::Exit,
                            WindowEvent::Resized(physical_size) => {
                                state.resize(*physical_size);
                            }
                            WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                                state.resize(**new_inner_size);
                            }
                            _ => {}
                        }
                    }
                }
                Event::RedrawRequested(window_id) if window_id == state.window().id() => {
                    let now = std::time::Instant::now();
                    let dt = now - last_render_time;
                    last_render_time = now;
                    state.tick(dt).unwrap();
                    match state.render(dt) {
                        Ok(_) => {},
                        Err(err) => {
                            match err {
                                Error::SurfaceErr { source } => {
                                    match source {
                                        // Reconfigure the surface if lost
                                        wgpu::SurfaceError::Lost => state.resize(state.size),
                                        // The system is out of memory, we should probably quit
                                        wgpu::SurfaceError::OutOfMemory => *control_flow = ControlFlow::Exit,
                                        // All other errors
                                        e => eprintln!("{:?}", e),
                                    }
                                },
                                e => {
                                    eprintln!("{:?}", e);
                                }
                            }
                        },
                    }
                }
                Event::MainEventsCleared => {
                    // RedrawRequested will only trigger once, unless we manually
                    // request it.
                    state.window().request_redraw();
                }
                _ => {}
            }
        })
    }
}

//TODO make a application builder
// pub async fn run(
//     config_fn: GetConfigFn,
//     init_fn: InitFn,
//     tick_fn: TickFn,
//     render_fn: RenderFn,
//     resize_fn: Option<ResizeFn>,
// ) {
//     env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
//     let event_loop = EventLoop::new();
//     let window = WindowBuilder::new().build(&event_loop).unwrap();
//     let mut last_render_time = std::time::Instant::now();

//     let mut state = BaseState::new(window, config_fn, init_fn, tick_fn, render_fn, resize_fn).await;
//     event_loop.run(move |event, _, control_flow| {
//         match event {
//             Event::DeviceEvent {
//                 event: DeviceEvent::MouseMotion{ delta, },
//                 .. // We're not using device_id currently
//             } => if state.mouse_pressed {
//                 //TODO deal all events in one place
//                 state.camera_controller.process_mouse(delta.0, delta.1)
//             }
//             Event::WindowEvent {
//                 ref event,
//                 window_id,
//             } if window_id == state.window().id() => {
//                 if !state.input(event) {
//                     match event {
//                         WindowEvent::CloseRequested
//                         | WindowEvent::KeyboardInput {
//                             input:
//                                 KeyboardInput {
//                                     state: ElementState::Pressed,
//                                     virtual_keycode: Some(VirtualKeyCode::Escape),
//                                     ..
//                                 },
//                             ..
//                         } => *control_flow = ControlFlow::Exit,
//                         WindowEvent::Resized(physical_size) => {
//                             state.resize(*physical_size);
//                         }
//                         WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
//                             state.resize(**new_inner_size);
//                         }
//                         _ => {}
//                     }
//                 }
//             }
//             Event::RedrawRequested(window_id) if window_id == state.window().id() => {
//                 let now = std::time::Instant::now();
//                 let dt = now - last_render_time;
//                 last_render_time = now;
//                 state.tick(dt).unwrap();
//                 match state.render(dt) {
//                     Ok(_) => {},
//                     Err(err) => {
//                         match err {
//                             Error::SurfaceErr { source } => {
//                                 match source {
//                                     // Reconfigure the surface if lost
//                                     wgpu::SurfaceError::Lost => state.resize(state.size),
//                                     // The system is out of memory, we should probably quit
//                                     wgpu::SurfaceError::OutOfMemory => *control_flow = ControlFlow::Exit,
//                                     // All other errors
//                                     e => eprintln!("{:?}", e),
//                                 }
//                             },
//                             e => {
//                                 eprintln!("{:?}", e);
//                             }
//                         }
//                     },
//                 }
//             }
//             Event::MainEventsCleared => {
//                 // RedrawRequested will only trigger once, unless we manually
//                 // request it.
//                 state.window().request_redraw();
//             }
//             _ => {}
//         }
//     });
// }
