// virtex/examples/svg.rs

#[macro_use]
extern crate log;

use env_logger;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gl::{GLDevice, GLVersion};
use pathfinder_gpu::resources::FilesystemResourceLoader;
use raqote::SolidSource;
use std::env;
use std::f32;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use virtex::VirtualTexture;
use virtex::manager::VirtualTextureManager;
use virtex::renderer_advanced::AdvancedRenderer;
use virtex::svg::SVGRasterizerProxy;
use winit::dpi::LogicalSize;
use winit::{DeviceEvent, Event, EventsLoop, KeyboardInput, ModifiersState, MouseScrollDelta};
use winit::{VirtualKeyCode, WindowBuilder, WindowEvent};

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;

const CACHE_TILES_ACROSS: u32 = 16;
const CACHE_TILES_DOWN: u32 = 16;
const TILE_SIZE: u32 = 256;
const TILE_BACKING_SIZE: u32 = 258;
const TILE_HASH_INITIAL_BUCKET_SIZE: u32 = 64;
const TILE_CACHE_WIDTH: u32 = CACHE_TILES_ACROSS * TILE_BACKING_SIZE;
const TILE_CACHE_HEIGHT: u32 = CACHE_TILES_DOWN * TILE_BACKING_SIZE;

static BACKGROUND_COLOR: SolidSource = SolidSource { r: 255, g: 255, b: 255, a: 255 };

static DEFAULT_SVG_PATH: &'static str = "resources/svg/Ghostscript_Tiger.svg";

fn main() {
    env_logger::init();

    let svg_path = match env::args().nth(1) {
        Some(path) => path,
        None => DEFAULT_SVG_PATH.to_owned(),
    };

    let mut event_loop = EventsLoop::new();
    let dpi = event_loop.get_primary_monitor().get_hidpi_factor() as f32;
    let logical_window_size = LogicalSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64);
    let physical_window_size =
        Vector2F::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32).scale(dpi).to_i32();
    let window = WindowBuilder::new().with_title("SVG example")
                                     .with_dimensions(logical_window_size)
                                     .build(&event_loop)
                                     .unwrap();
    window.show();

    let connection = Connection::from_winit_window(&window).unwrap();
    let native_widget = connection.create_native_widget_from_winit_window(&window).unwrap();
    let adapter = connection.create_low_power_adapter().unwrap();
    let mut surfman_device = connection.create_device(&adapter).unwrap();

    let context_attributes = ContextAttributes {
        version: SurfmanGLVersion::new(3, 3),
        flags: ContextAttributeFlags::ALPHA,
    };
    let context_descriptor = surfman_device.create_context_descriptor(&context_attributes)
                                           .unwrap();

    let surface_type = SurfaceType::Widget { native_widget };
    let mut context = surfman_device.create_context(&context_descriptor).unwrap();
    let surface = surfman_device.create_surface(&context, SurfaceAccess::GPUOnly, surface_type)
                                .unwrap();
    surfman_device.bind_surface_to_context(&mut context, surface).unwrap();
    surfman_device.make_context_current(&context).unwrap();

    gl::load_with(|symbol| surfman_device.get_proc_address(&context, symbol));

    let default_framebuffer_object = surfman_device.context_surface_info(&context)
                                                   .unwrap()
                                                   .unwrap()
                                                   .framebuffer_object;
    let device = GLDevice::new(GLVersion::GL3, default_framebuffer_object);
    let resources = FilesystemResourceLoader::locate();

    // Initialize the raster thread, and wait for the SVG to load.
    let mut rasterizer_proxy = SVGRasterizerProxy::new(svg_path, BACKGROUND_COLOR, TILE_SIZE);
    let svg_size = rasterizer_proxy.wait_for_svg_to_load();

    // Initialize the virtual texture.
    let cache_texture_size = Vector2I::new(TILE_CACHE_WIDTH as i32, TILE_CACHE_HEIGHT as i32);
    let virtual_texture = VirtualTexture::new(svg_size,
                                              cache_texture_size,
                                              TILE_SIZE,
                                              TILE_HASH_INITIAL_BUCKET_SIZE);

    // Initialize the virtual texture manger and renderer.
    let manager = VirtualTextureManager::new(virtual_texture, physical_window_size);
    let mut renderer = AdvancedRenderer::new(&device, manager, &resources);

    let mut exit = false;
    let mut needed_tiles = vec![];

    while !exit {
        debug!("--- begin frame ---");
        renderer.prepare(&device, &mut needed_tiles);
        rasterizer_proxy.rasterize_needed_tiles(&device, &mut renderer, &mut needed_tiles);

        renderer.render(&device);

        let mut surface = surfman_device.unbind_surface_from_context(&mut context)
                                        .unwrap()
                                        .unwrap();
        surfman_device.present_surface(&mut context, &mut surface).unwrap();
        surfman_device.bind_surface_to_context(&mut context, surface).unwrap();

        let manager = renderer.manager_mut();
        event_loop.poll_events(|event| {
            match event {
                Event::WindowEvent {
                    event: WindowEvent::MouseWheel {
                        delta: MouseScrollDelta::PixelDelta(delta),
                        modifiers: ModifiersState { ctrl: true, .. },
                        ..
                    },
                    ..
                } => {
                    if delta.y > 0.0 { 
                        manager.transform = manager.transform.scale(Vector2F::splat(1.025))
                    } else if delta.y < 0.0 {
                        manager.transform = manager.transform.scale(Vector2F::splat(0.975))
                    }
                }
                Event::WindowEvent {
                    event: WindowEvent::MouseWheel {
                        delta: MouseScrollDelta::PixelDelta(delta),
                        ..
                    },
                    ..
                } => {
                    let vector = Vector2F::new(delta.x as f32, delta.y as f32).scale(dpi);
                    manager.transform = manager.transform.translate(vector)
                }
                Event::WindowEvent { event: WindowEvent::Destroyed, .. } |
                Event::DeviceEvent {
                    event: DeviceEvent::Key(KeyboardInput {
                        virtual_keycode: Some(VirtualKeyCode::Escape),
                        ..
                    }),
                    ..
                } => exit = true,
                _ => {}
            }
        });
    }
}
