// virtex/examples/svg.rs

use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gl::{GLDevice, GLVersion};
use pathfinder_gpu::resources::FilesystemResourceLoader;
use pathfinder_gpu::{Device};
use raqote::{DrawTarget, SolidSource, Transform};
use resvg::{Options as ResvgOptions, ScreenSize};
use resvg::backend_raqote;
use resvg::usvg::{Options as UsvgOptions, Tree};
use std::env;
use std::slice;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use virtex::manager2d::VirtualTextureManager2D;
use virtex::renderer_simple::SimpleRenderer;
use virtex::{TileCacheEntry, VirtualTexture};
use winit::dpi::LogicalSize;
use winit::{DeviceEvent, Event, EventsLoop, KeyboardInput, ModifiersState, MouseScrollDelta};
use winit::{VirtualKeyCode, WindowBuilder, WindowEvent};

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;

const CACHE_TILES_ACROSS: u32 = 16;
const CACHE_TILES_DOWN: u32 = 16;
const TILE_SIZE: u32 = 256;
const TILE_BACKING_SIZE: u32 = 258;
const TILE_CACHE_WIDTH: u32 = CACHE_TILES_ACROSS * TILE_BACKING_SIZE;
const TILE_CACHE_HEIGHT: u32 = CACHE_TILES_DOWN * TILE_BACKING_SIZE;
const DEFAULT_GLOBAL_SCALE_FACTOR: f32 = 5.0;

static BACKGROUND_COLOR: SolidSource = SolidSource { r: 255, g: 255, b: 255, a: 255 };

static DEFAULT_SVG_PATH: &'static str = "resources/svg/Ghostscript_Tiger.svg";

fn main() {
    let svg_path = match env::args().nth(1) {
        Some(path) => path,
        None => DEFAULT_SVG_PATH.to_owned(),
    };

    let global_scale_factor: f32 = match env::args().nth(2) {
        None => DEFAULT_GLOBAL_SCALE_FACTOR,
        Some(factor) => factor.parse().unwrap(),
    };

    let svg_tree = Tree::from_file(&svg_path, &UsvgOptions::default()).unwrap();
    let svg_size = svg_tree.svg_node().size;
    let svg_size = Vector2I::new(svg_size.width().ceil() as i32, svg_size.height().ceil() as i32);

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

    // Initialize the cache.
    let cache_texture_size = Vector2I::new(TILE_CACHE_WIDTH as i32, TILE_CACHE_HEIGHT as i32);
    let mut cache_pixels =
        vec![0; cache_texture_size.x() as usize * cache_texture_size.y() as usize];
    let mut cache_draw_target = DrawTarget::new(TILE_BACKING_SIZE as i32,
                                                TILE_BACKING_SIZE as i32);

    // Initialize the virtual texture.
    let virtual_texture = VirtualTexture::new(svg_size, cache_texture_size, TILE_SIZE);
    let manager = VirtualTextureManager2D::new(virtual_texture, physical_window_size);
    let mut renderer = SimpleRenderer::new(&device, manager, &resources);

    let mut exit = false;
    let mut needed_tiles = vec![];

    while !exit {
        println!("--- begin frame ---");
        renderer.manager_mut().request_needed_tiles(&mut needed_tiles);
        rasterize_needed_tiles(&device,
                               &mut renderer,
                               global_scale_factor,
                               &mut cache_draw_target,
                               &mut cache_pixels,
                               &svg_tree,
                               &mut needed_tiles);

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

fn rasterize_needed_tiles(device: &GLDevice,
                          renderer: &mut SimpleRenderer<GLDevice>,
                          global_scale_factor: f32,
                          cache_draw_target: &mut DrawTarget,
                          cache_pixels: &mut [u32],
                          svg_tree: &Tree,
                          needed_tiles: &mut Vec<TileCacheEntry>) {
    if needed_tiles.is_empty() {
        return;
    }

    let cache_texture_size = Vector2I::new(TILE_CACHE_WIDTH as i32, TILE_CACHE_HEIGHT as i32);

    let svg_size = svg_tree.svg_node().size;
    let svg_size = Vector2I::new(svg_size.width().ceil() as i32, svg_size.height().ceil() as i32);
    let svg_screen_size = ScreenSize::new(svg_size.x() as u32, svg_size.y() as u32).unwrap();

    let tile_size = renderer.manager_mut().texture.tile_size();

    for tile_cache_entry in needed_tiles.drain(..) {
        println!("rendering {:?}, tile_size={}", tile_cache_entry, tile_size);
        let descriptor = &tile_cache_entry.descriptor;
        let scene_offset =
            Vector2F::new(descriptor.x as f32, descriptor.y as f32).scale(-(tile_size as f32));
        let scale = (1 << descriptor.lod) as f32 * global_scale_factor;

        let mut transform = Transform2F::default();
        transform = Transform2F::from_uniform_scale(scale) * transform;
        transform = Transform2F::from_translation(scene_offset) * transform;
        transform = Transform2F::from_translation(Vector2F::splat(1.0)) * transform;
        //transform = Transform2F::from_translation(tile_offset.to_f32()) * transform;

        println!("... transform={:?}", transform);
        cache_draw_target.set_transform(&Transform::row_major(transform.matrix.m11(),
                                                              transform.matrix.m21(),
                                                              transform.matrix.m12(),
                                                              transform.matrix.m22(),
                                                              transform.vector.x(),
                                                              transform.vector.y()));
        cache_draw_target.clear(BACKGROUND_COLOR);
        backend_raqote::render_to_canvas(&svg_tree,
                                         &ResvgOptions::default(),
                                         svg_screen_size,
                                         cache_draw_target);
        cache_draw_target.set_transform(&Transform::identity());

        let address = tile_cache_entry.address;
        let tile_rect = RectI::new(address.0, Vector2I::splat(1)).scale(TILE_BACKING_SIZE as i32);

        blit(cache_pixels,
             cache_texture_size.x() as usize,
             tile_rect,
             cache_draw_target.get_data(),
             TILE_BACKING_SIZE as usize,
             Vector2I::default());
    }
    //cache_draw_target.write_png("cache.png").unwrap();
    unsafe {
        let cache_pixels: &[u8] = slice::from_raw_parts(cache_pixels.as_ptr() as *const u8,
                                                        cache_pixels.len() * 4);
        device.upload_to_texture(&renderer.cache_texture(), cache_texture_size, cache_pixels);
    }
}

fn blit(dest: &mut [u32],
        dest_stride: usize,
        dest_rect: RectI,
        src: &[u32],
        src_stride: usize,
        src_origin: Vector2I) {
    for y in 0..dest_rect.size().y() {
        let dest_start = (dest_rect.origin().y() + y) as usize * dest_stride +
            dest_rect.origin().x() as usize;
        let src_start = (src_origin.y() + y) as usize * src_stride + src_origin.x() as usize;
        for x in 0..dest_rect.size().x() {
            let pixel = src[src_start + x as usize];
            dest[dest_start + x as usize] =
                (pixel & 0x00ff00ff).rotate_right(16) | (pixel & 0xff00ff00);
        }
    }
}