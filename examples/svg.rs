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
use raqote::{DrawTarget, IntRect, Transform};
use resvg::{Options as ResvgOptions, ScreenSize};
use resvg::backend_raqote;
use resvg::usvg::{Options as UsvgOptions, Tree};
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
const TILE_CACHE_WIDTH: u32 = CACHE_TILES_ACROSS * TILE_SIZE;
const TILE_CACHE_HEIGHT: u32 = CACHE_TILES_DOWN * TILE_SIZE;
const GLOBAL_SCALE_FACTOR: f32 = 5.0;

static QUAD_VERTEX_POSITIONS: [u8; 8] = [0, 0, 1, 0, 0, 1, 1, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 2, 1, 3, 2];

static DEFAULT_SVG_PATH: &'static str = "resources/svg/Ghostscript_Tiger.svg";

fn main() {
    let svg_tree = Tree::from_file(&DEFAULT_SVG_PATH, &UsvgOptions::default()).unwrap();
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
    let mut cache_draw_target = DrawTarget::new(cache_texture_size.x(), cache_texture_size.y());
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
                let scale = (1 << descriptor.lod) as f32 * GLOBAL_SCALE_FACTOR;

                let address = tile_cache_entry.address;
                let tile_offset = address.0.scale(TILE_SIZE as i32);
                let tile_max_point = tile_offset + Vector2I::splat(TILE_SIZE as i32);
                let tile_clip_rect = IntRect::new(to_euclid_point_i32(tile_offset),
                                                  to_euclid_point_i32(tile_max_point));

                let mut transform = Transform2F::default();
                transform = Transform2F::from_uniform_scale(scale) * transform;
                transform = Transform2F::from_translation(scene_offset) * transform;
                transform = Transform2F::from_translation(tile_offset.to_f32()) * transform;

                println!("... transform={:?}", transform);
                cache_draw_target.set_transform(&Transform::row_major(transform.matrix.m11(),
                                                                      transform.matrix.m21(),
                                                                      transform.matrix.m12(),
                                                                      transform.matrix.m22(),
                                                                      transform.vector.x(),
                                                                      transform.vector.y()));
                cache_draw_target.push_clip_rect(tile_clip_rect);
                backend_raqote::render_to_canvas(&svg_tree,
                                                 &ResvgOptions::default(),
                                                 svg_screen_size,
                                                 &mut cache_draw_target);
                cache_draw_target.pop_clip();
                cache_draw_target.set_transform(&Transform::identity());
            }
            //cache_draw_target.write_png("cache.png").unwrap();
            device.upload_to_texture(&cache_texture,
                                     cache_texture_size,
                                     cache_draw_target.get_data_u8_mut());
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

                let cache_tex_scale = Vector2F::new(1.0 / CACHE_TILES_ACROSS as f32,
                                                    1.0 / CACHE_TILES_DOWN as f32);
                let tile_tex_origin = tile_cache_entry.address.0.to_f32();
                let tile_tex_rect =
                    RectF::new(tile_tex_origin, Vector2F::splat(1.0)).scale_xy(cache_tex_scale);

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
                        manager.transform = manager.transform.scale(Vector2F::splat(1.05))
                    } else if delta.y < 0.0 {
                        manager.transform = manager.transform.scale(Vector2F::splat(0.95))
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

fn to_euclid_point_i32(point: Vector2I) -> Point2D<i32> {
    Point2D::new(point.x(), point.y())
}
