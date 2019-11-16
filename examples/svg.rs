// virtex/examples/svg.rs

use euclid::default::{Point2D, Size2D};
use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gl::{GLBuffer, GLDevice, GLProgram, GLUniform, GLVersion};
use pathfinder_gl::{GLVertexArray, GLVertexAttr};
use pathfinder_gpu::resources::{FilesystemResourceLoader, ResourceLoader};
use pathfinder_gpu::{BlendState, BufferData, BufferTarget, BufferUploadMode, ClearOps, Device};
use pathfinder_gpu::{Primitive, RenderOptions, RenderState, RenderTarget, TextureFormat};
use pathfinder_gpu::{UniformData, VertexAttrClass, VertexAttrDescriptor, VertexAttrType};
use raqote::{DrawTarget, IntRect, SolidSource, Transform};
use resvg::{Options as ResvgOptions, ScreenSize};
use resvg::backend_raqote;
use resvg::usvg::{Options as UsvgOptions, Tree};
use std::env;
use std::slice;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use virtex::VirtualTexture;
use virtex::manager2d::VirtualTextureManager2D;
use winit::dpi::LogicalSize;
use winit::{DeviceEvent, Event, EventsLoop, KeyboardInput, ModifiersState, MouseScrollDelta};
use winit::{VirtualKeyCode, WindowBuilder, WindowEvent};

const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;

const CACHE_TILES_ACROSS: u32 = 16;
const CACHE_TILES_DOWN: u32 = 16;
const CACHE_TILE_COUNT: u32 = CACHE_TILES_ACROSS * CACHE_TILES_DOWN;
const TILE_SIZE: u32 = 256;
const TILE_BACKING_SIZE: u32 = 258;
const TILE_CACHE_WIDTH: u32 = CACHE_TILES_ACROSS * TILE_BACKING_SIZE;
const TILE_CACHE_HEIGHT: u32 = CACHE_TILES_DOWN * TILE_BACKING_SIZE;
const DEFAULT_GLOBAL_SCALE_FACTOR: f32 = 5.0;

static BACKGROUND_COLOR: SolidSource = SolidSource { r: 255, g: 255, b: 255, a: 255 };

static QUAD_VERTEX_POSITIONS: [u8; 8] = [0, 0, 1, 0, 0, 1, 1, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 2, 1, 3, 2];

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
    let svg_screen_size = ScreenSize::new(svg_size.x() as u32, svg_size.y() as u32).unwrap();

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
    let render_vertex_array = RenderVertexArray::new(&device, &resources);

    // Initialize the cache.
    let cache_texture_size = Vector2I::new(TILE_CACHE_WIDTH as i32, TILE_CACHE_HEIGHT as i32);
    let cache_texture = device.create_texture(TextureFormat::RGBA8, cache_texture_size);
    let mut cache_pixels =
        vec![0; cache_texture_size.x() as usize * cache_texture_size.y() as usize];
    let mut cache_draw_target =
        DrawTarget::new(TILE_BACKING_SIZE as i32, TILE_BACKING_SIZE as i32);

    // Initialize the virtual texture.
    let virtual_texture = VirtualTexture::new(svg_size, cache_texture_size, TILE_SIZE);
    let mut manager = VirtualTextureManager2D::new(virtual_texture, physical_window_size);

    let mut exit = false;
    let mut needed_tiles = vec![];

    while !exit {
        println!("--- begin frame ---");
        manager.request_needed_tiles(&mut needed_tiles);

        if !needed_tiles.is_empty() {
            for tile_cache_entry in needed_tiles.drain(..) {
                println!("rendering {:?}", tile_cache_entry);
                let descriptor = &tile_cache_entry.descriptor;
                let scene_offset = Vector2F::new(descriptor.x as f32,
                                                 descriptor.y as f32).scale(-(TILE_SIZE as f32));
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
                                                 &mut cache_draw_target);
                cache_draw_target.set_transform(&Transform::identity());

                let address = tile_cache_entry.address;
                let tile_offset = address.0.scale(TILE_BACKING_SIZE as i32);
                let tile_size = Vector2I::splat(TILE_BACKING_SIZE as i32);

                blit(&mut cache_pixels,
                     cache_texture_size.x() as usize,
                     RectI::new(tile_offset, tile_size),
                     cache_draw_target.get_data(),
                     TILE_BACKING_SIZE as usize,
                     Vector2I::default());
            }
            //cache_draw_target.write_png("cache.png").unwrap();
            unsafe {
                let cache_pixels: &[u8] = slice::from_raw_parts(cache_pixels.as_ptr() as *const u8,
                                                                cache_pixels.len() * 4);
                device.upload_to_texture(&cache_texture, cache_texture_size, cache_pixels);
            }
        }

        device.begin_commands();
        let mut cleared = false;

        // Render the two LODs in order.
        let current_scale = manager.current_scale();
        let current_lod = current_scale.log2();
        println!("current_lod = {}", current_lod);
        let current_lods = manager.current_lods();
        let high_lod_opacity = current_lod.fract();

        for (render_lod_index, &render_lod) in current_lods.iter().enumerate() {
            let opacity = if render_lod_index == 0 { 1.0 } else { high_lod_opacity };
            for tile_cache_entry in manager.texture.all_cached_tiles() {
                let descriptor = &tile_cache_entry.descriptor;
                if descriptor.lod != render_lod {
                    continue;
                }

                let mut tile_position = Vector2F::new(descriptor.x as f32, descriptor.y as f32);
                let tile_size = TILE_SIZE as f32 / (1 << render_lod) as f32;
                let tile_rect = RectF::new(tile_position, Vector2F::splat(1.0)).scale(tile_size);

                let tile_tex_origin = Vector2I::splat(1) +
                    tile_cache_entry.address.0.scale(TILE_BACKING_SIZE as i32);
                let tile_tex_size = Vector2I::splat(TILE_SIZE as i32);

                let cache_tex_scale = Vector2F::new(1.0 / TILE_CACHE_WIDTH as f32,
                                                    1.0 / TILE_CACHE_HEIGHT as f32);
                let tile_tex_rect =
                    RectI::new(tile_tex_origin, tile_tex_size).to_f32().scale_xy(cache_tex_scale);

                //println!("tile_tex_rect={:?}", tile_tex_rect);
                device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
                    target: &RenderTarget::Default,
                    program: &render_vertex_array.render_program.program,
                    vertex_array: &render_vertex_array.vertex_array,
                    primitive: Primitive::Triangles,
                    uniforms: &[
                        (&render_vertex_array.render_program.tile_rect_uniform,
                         UniformData::Vec4(tile_rect.0)),
                        (&render_vertex_array.render_program.tile_tex_rect_uniform,
                         UniformData::Vec4(tile_tex_rect.0)),
                        (&render_vertex_array.render_program.framebuffer_size_uniform,
                         UniformData::Vec2(physical_window_size.to_f32().0)),
                        (&render_vertex_array.render_program.transform_uniform,
                         UniformData::Mat2(manager.transform.matrix.0)),
                        (&render_vertex_array.render_program.translation_uniform,
                         UniformData::Vec2(manager.transform.vector.0)),
                        (&render_vertex_array.render_program.opacity_uniform,
                         UniformData::Float(opacity)),
                    ],
                    textures: &[&cache_texture],
                    viewport: RectI::new(Vector2I::splat(0), physical_window_size),
                    options: RenderOptions {
                        clear_ops: ClearOps {
                            color: if !cleared {
                                Some(ColorF::new(0.0, 0.0, 0.0, 1.0))
                            } else {
                                None
                            },
                            ..ClearOps::default()
                        },
                        blend: if render_lod_index == 0 {
                            BlendState::Off
                        } else {
                            BlendState::RGBOneAlphaOneMinusSrcAlpha
                        },
                        ..RenderOptions::default()
                    },
                });

                cleared = true;
            }
        }

        device.end_commands();

        let mut surface = surfman_device.unbind_surface_from_context(&mut context)
                                        .unwrap()
                                        .unwrap();
        surfman_device.present_surface(&mut context, &mut surface).unwrap();
        surfman_device.bind_surface_to_context(&mut context, surface).unwrap();

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

struct RenderVertexArray {
    render_program: RenderProgram,
    vertex_array: GLVertexArray,
    quad_vertex_positions_buffer: GLBuffer,
    quad_vertex_indices_buffer: GLBuffer,
} 

impl RenderVertexArray {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> RenderVertexArray {
        let render_program = RenderProgram::new(device, resources);
        let vertex_array = device.create_vertex_array();
        let quad_vertex_positions_buffer = device.create_buffer();
        device.allocate_buffer(&quad_vertex_positions_buffer,
                               BufferData::Memory(&QUAD_VERTEX_POSITIONS),
                               BufferTarget::Vertex,
                               BufferUploadMode::Static);
        let quad_vertex_indices_buffer = device.create_buffer();
        device.allocate_buffer(&quad_vertex_indices_buffer,
                               BufferData::Memory(&QUAD_VERTEX_INDICES),
                               BufferTarget::Index,
                               BufferUploadMode::Static);
        device.bind_buffer(&vertex_array, &quad_vertex_positions_buffer, BufferTarget::Vertex);
        device.bind_buffer(&vertex_array, &quad_vertex_indices_buffer, BufferTarget::Index);
        device.configure_vertex_attr(&vertex_array,
                                     &render_program.position_attribute,
                                     &VertexAttrDescriptor {
                                         size: 2,
                                         class: VertexAttrClass::Float,
                                         attr_type: VertexAttrType::U8,
                                         stride: 2,
                                         offset: 0,
                                         divisor: 0,
                                         buffer_index: 0,
                                     });
        RenderVertexArray {
            render_program,
            vertex_array,
            quad_vertex_positions_buffer,
            quad_vertex_indices_buffer,
        }
    }
}

struct RenderProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    tile_rect_uniform: GLUniform,
    tile_tex_rect_uniform: GLUniform,
    framebuffer_size_uniform: GLUniform,
    transform_uniform: GLUniform,
    translation_uniform: GLUniform,
    tile_cache_uniform: GLUniform,
    opacity_uniform: GLUniform,
}

impl RenderProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> RenderProgram {
        let program = device.create_program(resources, "render");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let tile_rect_uniform = device.get_uniform(&program, "TileRect");
        let tile_tex_rect_uniform = device.get_uniform(&program, "TileTexRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let tile_cache_uniform = device.get_uniform(&program, "TileCache");
        let opacity_uniform = device.get_uniform(&program, "Opacity");
        RenderProgram {
            program,
            position_attribute,
            tile_rect_uniform,
            tile_tex_rect_uniform,
            framebuffer_size_uniform,
            transform_uniform,
            translation_uniform,
            tile_cache_uniform,
            opacity_uniform,
        }
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

fn to_euclid_point_i32(point: Vector2I) -> Point2D<i32> {
    Point2D::new(point.x(), point.y())
}
