// virtex/examples/svg.rs

#[macro_use]
extern crate log;

use env_logger;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gl::{GLDevice, GLVersion};
use pathfinder_gpu::resources::FilesystemResourceLoader;
use pathfinder_gpu::{Device, TextureDataRef};
use raqote::{DrawTarget, SolidSource, Transform};
use resvg::{Options as ResvgOptions, ScreenSize};
use resvg::backend_raqote;
use resvg::usvg::{Options as UsvgOptions, Tree};
use std::env;
use std::f32;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use virtex::VirtualTexture;
use virtex::manager2d::{TileRequest, VirtualTextureManager2D};
use virtex::renderer_advanced::AdvancedRenderer;
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

    // Initialize the raster thread.
    let (mut main_to_rasterizer_sender, main_to_rasterizer_receiver) = mpsc::channel();
    let (rasterizer_to_main_sender, mut rasterizer_to_main_receiver) = mpsc::channel();
    let _raster_thread = thread::spawn(move || {
        rasterizer_thread(rasterizer_to_main_sender, main_to_rasterizer_receiver, svg_path);
    });

    // Wait for the SVG to be loaded.
    let svg_size = match rasterizer_to_main_receiver.recv().unwrap() {
        RasterizerToMainMsg::SVGLoaded { size } => size,
        RasterizerToMainMsg::TileRasterized { .. } => unreachable!(),
    };

    // Initialize the virtual texture.
    let cache_texture_size = Vector2I::new(TILE_CACHE_WIDTH as i32, TILE_CACHE_HEIGHT as i32);
    let virtual_texture = VirtualTexture::new(svg_size,
                                              cache_texture_size,
                                              TILE_SIZE,
                                              TILE_HASH_INITIAL_BUCKET_SIZE);

    // Initialize the virtual texture manger and renderer.
    let manager = VirtualTextureManager2D::new(virtual_texture, physical_window_size);
    let mut renderer = AdvancedRenderer::new(&device, manager, &resources);

    let mut exit = false;
    let mut needed_tiles = vec![];

    while !exit {
        debug!("--- begin frame ---");
        //renderer.manager_mut().request_needed_tiles(&mut needed_tiles);
        renderer.prepare(&device, &mut needed_tiles);
        rasterize_needed_tiles(&device,
                               &mut renderer,
                               &mut needed_tiles,
                               &mut main_to_rasterizer_sender,
                               &mut rasterizer_to_main_receiver);

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
                          renderer: &mut AdvancedRenderer<GLDevice>,
                          needed_tiles: &mut Vec<TileRequest>,
                          sender: &mut Sender<MainToRasterizerMsg>,
                          receiver: &mut Receiver<RasterizerToMainMsg>) {
    if !needed_tiles.is_empty() {
        for tile_cache_entry in needed_tiles.drain(..) {
            let tile_origin = renderer.manager()
                                    .texture
                                    .address_to_tile_coords(tile_cache_entry.address);
            sender.send(MainToRasterizerMsg { tile_request: tile_cache_entry, tile_origin })
                  .unwrap();
        }
    }

    // FIXME(pcwalton): Squash multiple upload-to-texture operations.
    while let Ok(msg) = receiver.try_recv() {
        let (tile_request, tile_origin, new_tile_pixels) = match msg {
            RasterizerToMainMsg::SVGLoaded { .. } => unreachable!(),
            RasterizerToMainMsg::TileRasterized { tile_request, tile_origin, new_tile_pixels } => {
                (tile_request, tile_origin, new_tile_pixels)
            }
        };

        renderer.manager_mut().texture.mark_as_rasterized(tile_request.address,
                                                          &tile_request.descriptor);

        let cache_texture_rect =
            RectI::new(tile_origin, Vector2I::splat(1)).scale(TILE_BACKING_SIZE as i32);
        device.upload_to_texture(&renderer.cache_texture(),
                                 cache_texture_rect,
                                 TextureDataRef::U8(&new_tile_pixels));

        debug!("marking {:?}/{:?} as rasterized!",
               tile_request.address,
               tile_request.descriptor);
    }
}

fn blit(dest: &mut [u8],
        dest_stride: usize,
        dest_rect: RectI,
        src: &[u32],
        src_stride: usize,
        src_origin: Vector2I) {
    for y in 0..dest_rect.size().y() {
        let dest_start = (dest_rect.origin().y() + y) as usize * dest_stride +
            dest_rect.origin().x() as usize * 4;
        let src_start = (src_origin.y() + y) as usize * src_stride + src_origin.x() as usize;
        for x in 0..dest_rect.size().x() {
            let pixel = src[src_start + x as usize];
            let dest_offset = dest_start + x as usize * 4;
            dest[dest_offset + 0] = ((pixel >> 16) & 0xff) as u8;
            dest[dest_offset + 1] = ((pixel >> 8) & 0xff) as u8;
            dest[dest_offset + 2] = (pixel & 0xff) as u8;
            dest[dest_offset + 3] = ((pixel >> 24) & 0xff) as u8;
        }
    }
}

struct MainToRasterizerMsg {
    tile_request: TileRequest,
    tile_origin: Vector2I,
}

enum RasterizerToMainMsg {
    SVGLoaded {
        size: Vector2I,
    },
    TileRasterized {
        tile_request: TileRequest,
        tile_origin: Vector2I,
        new_tile_pixels: Vec<u8>,
    },
}

fn rasterizer_thread(sender: Sender<RasterizerToMainMsg>,
                     receiver: Receiver<MainToRasterizerMsg>,
                     svg_path: String) {
    // Load the SVG.
    let svg_tree = Tree::from_file(&svg_path, &UsvgOptions::default()).unwrap();

    let svg_size = svg_tree.svg_node().size;
    let svg_size = Vector2I::new(svg_size.width().ceil() as i32, svg_size.height().ceil() as i32);
    let svg_screen_size = ScreenSize::new(svg_size.x() as u32, svg_size.y() as u32).unwrap();

    sender.send(RasterizerToMainMsg::SVGLoaded { size: svg_size }).unwrap();

    // Initialize the cache.
    let mut cache_draw_target = DrawTarget::new(TILE_BACKING_SIZE as i32,
                                                TILE_BACKING_SIZE as i32);

    while let Ok(msg) = receiver.recv() {
        debug!("rendering {:?}, tile_size={}", msg.tile_request, TILE_SIZE);
        let mut cache_pixels =
            vec![0; TILE_BACKING_SIZE as usize * TILE_BACKING_SIZE as usize * 4];

        if let Err(_) = panic::catch_unwind(AssertUnwindSafe(|| {
            let descriptor = &msg.tile_request.descriptor;
            let scene_offset = descriptor.tile_position().to_f32().scale(-(TILE_SIZE as f32));
            let scale = f32::exp2(descriptor.lod() as f32);

            let mut transform = Transform2F::default();
            transform = Transform2F::from_uniform_scale(scale) * transform;
            transform = Transform2F::from_translation(scene_offset) * transform;
            transform = Transform2F::from_translation(Vector2F::splat(1.0)) * transform;

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
                                            &mut cache_draw_target);

            cache_draw_target.set_transform(&Transform::identity());

            blit(&mut cache_pixels,
                 TILE_BACKING_SIZE as usize * 4,
                 RectI::new(Vector2I::default(), Vector2I::splat(TILE_BACKING_SIZE as i32)),
                 cache_draw_target.get_data(),
                 TILE_BACKING_SIZE as usize,
                 Vector2I::default());
        })) {
            error!("rendering {:?} panicked!", msg.tile_request);
            cache_draw_target = DrawTarget::new(TILE_BACKING_SIZE as i32,
                                                TILE_BACKING_SIZE as i32);
        }

        sender.send(RasterizerToMainMsg::TileRasterized {
            tile_request: msg.tile_request,
            tile_origin: msg.tile_origin,
            new_tile_pixels: cache_pixels,
        }).unwrap();
    }
}
