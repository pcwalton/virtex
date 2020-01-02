// virtex/examples/svg.rs

#[macro_use]
extern crate log;

use env_logger;
use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gl::{GLBuffer, GLDevice, GLFramebuffer, GLProgram, GLUniform, GLVersion};
use pathfinder_gl::{GLVertexArray, GLVertexAttr};
use pathfinder_gpu::resources::{FilesystemResourceLoader, ResourceLoader};
use pathfinder_gpu::{BufferData, BufferTarget, BufferUploadMode, ClearOps, Device, Primitive};
use pathfinder_gpu::{RenderOptions, RenderState, RenderTarget, TextureFormat, UniformData};
use pathfinder_gpu::{VertexAttrClass, VertexAttrDescriptor, VertexAttrType};
use raqote::SolidSource;
use std::env;
use std::f32;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use virtex::VirtualTexture;
use virtex::manager::{TileRequest, VirtualTextureManager};
use virtex::renderer_advanced::{AdvancedRenderer, PrepareAdvancedUniforms, RenderAdvancedUniforms};
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

const DERIVATIVES_VIEWPORT_SCALE_FACTOR: i32 = 16;

static BACKGROUND_COLOR: SolidSource = SolidSource { r: 255, g: 255, b: 255, a: 255 };

static DEFAULT_SVG_PATH: &'static str = "resources/svg/Ghostscript_Tiger.svg";

static QUAD_VERTEX_POSITIONS: [u8; 8] = [0, 0, 1, 0, 0, 1, 1, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 2, 1, 3, 2];

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
    let thread_count = num_cpus::get_physical() as u32;
    let mut rasterizer_proxy = SVGRasterizerProxy::new(svg_path,
                                                       BACKGROUND_COLOR,
                                                       TILE_SIZE,
                                                       thread_count);
    let svg_size = rasterizer_proxy.wait_for_svg_to_load();

    // Initialize the virtual texture.
    // FIXME(pcwalton): Bad API. This should take the *number* of tiles across and down.
    let cache_texture_size = Vector2I::new(TILE_CACHE_WIDTH as i32, TILE_CACHE_HEIGHT as i32);
    let virtual_texture = VirtualTexture::new(cache_texture_size,
                                              TILE_SIZE,
                                              TILE_HASH_INITIAL_BUCKET_SIZE);

    // Initialize the virtual texture manger and renderer.
    let manager = VirtualTextureManager::new(virtual_texture, physical_window_size);
    let mut renderer = AdvancedRenderer::new(&device, manager, DERIVATIVES_VIEWPORT_SCALE_FACTOR);

    // Create the derivatives texture.
    let derivatives_texture = device.create_texture(TextureFormat::RGBA32F,
                                                    renderer.derivatives_viewport().size());
    let derivatives_framebuffer = device.create_framebuffer(derivatives_texture);

    // Initialize shaders and vertex arrays.
    let prepare_vertex_array = PrepareAdvancedVertexArray::new(&device, &resources);
    let render_vertex_array = RenderAdvancedVertexArray::new(&device, &resources);

    let mut exit = false;
    let mut needed_tiles = vec![];

    while !exit {
        debug!("--- begin frame ---");
        prepare(&mut renderer,
                &device,
                &prepare_vertex_array,
                &derivatives_framebuffer,
                svg_size,
                &mut needed_tiles);

        rasterizer_proxy.rasterize_needed_tiles(&device, &mut renderer, &mut needed_tiles);
        renderer.update_metadata(&device);
        render(&renderer, &device, &render_vertex_array, svg_size);

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

fn prepare(renderer: &mut AdvancedRenderer<GLDevice>,
           device: &GLDevice,
           prepare_vertex_array: &PrepareAdvancedVertexArray,
           derivatives_framebuffer: &GLFramebuffer,
           virtual_texture_size: Vector2I,
           needed_tiles: &mut Vec<TileRequest>) {
    let quad_rect = RectI::new(Vector2I::default(), virtual_texture_size).to_f32();
    let mut uniforms = vec![
        (&prepare_vertex_array.prepare_program.quad_rect_uniform, UniformData::Vec4(quad_rect.0)),
        (&prepare_vertex_array.prepare_program.framebuffer_size_uniform,
            UniformData::Vec2(renderer.manager().viewport_size().to_f32().0)),
        (&prepare_vertex_array.prepare_program.transform_uniform,
            UniformData::Mat2(renderer.manager().transform.matrix.0)),
        (&prepare_vertex_array.prepare_program.translation_uniform,
            UniformData::Vec2(renderer.manager().transform.vector.0)),
    ];
    renderer.push_prepare_uniforms(&prepare_vertex_array.prepare_program.virtex_uniforms,
                                   &mut uniforms);

    device.begin_commands();
    device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
        target: &RenderTarget::Framebuffer(derivatives_framebuffer),
        program: &prepare_vertex_array.prepare_program.program,
        vertex_array: &prepare_vertex_array.vertex_array,
        primitive: Primitive::Triangles,
        uniforms: &uniforms,
        textures: &[],
        viewport: renderer.derivatives_viewport(),
        options: RenderOptions {
            clear_ops: ClearOps {
                color: Some(ColorF::new(0.0, 0.0, 0.0, 0.0)),
                ..ClearOps::default()
            },
            ..RenderOptions::default()
        },
    });
    let texture_data = device.read_pixels(&RenderTarget::Framebuffer(derivatives_framebuffer),
                                          renderer.derivatives_viewport());
    device.end_commands();

    renderer.request_needed_tiles(&texture_data, needed_tiles);
}

fn render(renderer: &AdvancedRenderer<GLDevice>,
          device: &GLDevice,
          render_vertex_array: &RenderAdvancedVertexArray,
          virtual_texture_size: Vector2I) {
    let quad_rect = RectI::new(Vector2I::default(), virtual_texture_size).to_f32();
    let mut uniforms = vec![
        (&render_vertex_array.render_program.quad_rect_uniform,
            UniformData::Vec4(quad_rect.0)),
        (&render_vertex_array.render_program.framebuffer_size_uniform,
            UniformData::Vec2(renderer.manager().viewport_size().to_f32().0)),
        (&render_vertex_array.render_program.transform_uniform,
            UniformData::Mat2(renderer.manager().transform.matrix.0)),
        (&render_vertex_array.render_program.translation_uniform,
            UniformData::Vec2(renderer.manager().transform.vector.0)),
    ];
    let mut textures = vec![];
    renderer.push_render_uniforms(&render_vertex_array.render_program.virtex_uniforms,
                                  &mut uniforms,
                                  &mut textures);

    device.begin_commands();
    device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
        target: &RenderTarget::Default,
        program: &render_vertex_array.render_program.program,
        vertex_array: &render_vertex_array.vertex_array,
        primitive: Primitive::Triangles,
        uniforms: &uniforms,
        textures: &textures,
        viewport: RectI::new(Vector2I::splat(0), renderer.manager().viewport_size()),
        options: RenderOptions {
            clear_ops: ClearOps {
                color: Some(ColorF::new(0.0, 0.0, 0.0, 1.0)),
                ..ClearOps::default()
            },
            ..RenderOptions::default()
        },
    });
    device.end_commands();
}

struct PrepareAdvancedVertexArray {
    prepare_program: PrepareAdvancedProgram,
    vertex_array: GLVertexArray,
    #[allow(dead_code)]
    quad_vertex_positions_buffer: GLBuffer,
    #[allow(dead_code)]
    quad_vertex_indices_buffer: GLBuffer,
}

impl PrepareAdvancedVertexArray {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> PrepareAdvancedVertexArray {
        let prepare_program = PrepareAdvancedProgram::new(device, resources);
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
                                     &prepare_program.position_attribute,
                                     &VertexAttrDescriptor {
                                         size: 2,
                                         class: VertexAttrClass::Float,
                                         attr_type: VertexAttrType::U8,
                                         stride: 2,
                                         offset: 0,
                                         divisor: 0,
                                         buffer_index: 0,
                                     });
        PrepareAdvancedVertexArray {
            prepare_program,
            vertex_array,
            quad_vertex_positions_buffer,
            quad_vertex_indices_buffer,
        }
    }
}

struct PrepareAdvancedProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    quad_rect_uniform: GLUniform,
    framebuffer_size_uniform: GLUniform,
    transform_uniform: GLUniform,
    translation_uniform: GLUniform,
    virtex_uniforms: PrepareAdvancedUniforms<GLDevice>,
}

impl PrepareAdvancedProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> PrepareAdvancedProgram {
        let program = device.create_program_from_shader_names(resources,
                                                              "prepare_advanced",
                                                              "render_advanced",
                                                              "prepare_advanced");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let quad_rect_uniform = device.get_uniform(&program, "QuadRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let virtex_uniforms = PrepareAdvancedUniforms::new(device, &program);
        PrepareAdvancedProgram {
            program,
            position_attribute,
            quad_rect_uniform,
            framebuffer_size_uniform,
            transform_uniform,
            translation_uniform,
            virtex_uniforms,
        }
    }
}

struct RenderAdvancedVertexArray {
    render_program: RenderAdvancedProgram,
    vertex_array: GLVertexArray,
    #[allow(dead_code)]
    quad_vertex_positions_buffer: GLBuffer,
    #[allow(dead_code)]
    quad_vertex_indices_buffer: GLBuffer,
}

impl RenderAdvancedVertexArray {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> RenderAdvancedVertexArray {
        let render_program = RenderAdvancedProgram::new(device, resources);
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
        RenderAdvancedVertexArray {
            render_program,
            vertex_array,
            quad_vertex_positions_buffer,
            quad_vertex_indices_buffer,
        }
    }
}

struct RenderAdvancedProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    quad_rect_uniform: GLUniform,
    framebuffer_size_uniform: GLUniform,
    transform_uniform: GLUniform,
    translation_uniform: GLUniform,
    virtex_uniforms: RenderAdvancedUniforms<GLDevice>,
}

impl RenderAdvancedProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> RenderAdvancedProgram {
        let program = device.create_program(resources, "render_advanced");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let quad_rect_uniform = device.get_uniform(&program, "QuadRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let virtex_uniforms = RenderAdvancedUniforms::new(device, &program);
        RenderAdvancedProgram {
            program,
            position_attribute,
            quad_rect_uniform,
            framebuffer_size_uniform,
            transform_uniform,
            translation_uniform,
            virtex_uniforms,
        }
    }
}