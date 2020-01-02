// virtex/examples/cloth.rs

use env_logger;
use num_cpus;
use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform3d::Transform4F;
use pathfinder_geometry::vector::{Vector2F, Vector2I, Vector3F, Vector4F};
use pathfinder_gl::{GLBuffer, GLDevice, GLProgram, GLUniform, GLVersion};
use pathfinder_gl::{GLVertexArray, GLVertexAttr};
use pathfinder_gpu::resources::{FilesystemResourceLoader, ResourceLoader};
use pathfinder_gpu::{BlendFunc, BlendOp, BlendState, BufferData, BufferTarget, BufferUploadMode};
use pathfinder_gpu::{ClearOps, DepthFunc, DepthState, Device, Primitive, RenderOptions};
use pathfinder_gpu::{RenderState, RenderTarget, TextureFormat, TextureDataRef, UniformData};
use pathfinder_gpu::{VertexAttrClass, VertexAttrDescriptor, VertexAttrType};
use pathfinder_simd::default::F32x2;
use raqote::SolidSource;
use std::env;
use std::f32::consts::FRAC_PI_2;
use std::mem;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use virtex::manager::VirtualTextureManager;
use virtex::renderer_advanced::{AdvancedRenderer, PrepareAdvancedUniforms, RenderAdvancedUniforms};
use virtex::svg::SVGRasterizerProxy;
use virtex::texture::VirtualTexture;
use winit::dpi::LogicalSize;
use winit::{DeviceEvent, Event, EventsLoop, KeyboardInput, VirtualKeyCode};
use winit::{WindowBuilder, WindowEvent};

static DEFAULT_SVG_PATH: &'static str = "resources/svg/Ghostscript_Tiger.svg";

const WINDOW_WIDTH:  i32 = 800;
const WINDOW_HEIGHT: i32 = 600;

const INITIAL_CAMERA_DISTANCE: f32 = 15.0;

const CAMERA_ROTATION_SPEED:    f32 = 0.01;
const CAMERA_TRANSLATION_SPEED: f32 = 0.1;

const MESH_PATCHES_ACROSS:  i32 = 20;
const MESH_PATCHES_DOWN:    i32 = 20;
const MESH_PATCH_COUNT:     i32 = MESH_PATCHES_ACROSS * MESH_PATCHES_DOWN;
const MESH_VERTICES_ACROSS: i32 = MESH_PATCHES_ACROSS + 1;
const MESH_VERTICES_DOWN:   i32 = MESH_PATCHES_DOWN + 1;
const MESH_VERTEX_COUNT:    i32 = MESH_VERTICES_ACROSS * MESH_VERTICES_DOWN;
const MESH_INDEX_COUNT:     i32 = MESH_PATCH_COUNT * 6;
const MESH_CENTER_X:        f32 = MESH_PATCHES_ACROSS as f32 * 0.5;
const MESH_CENTER_Y:        f32 = MESH_PATCHES_DOWN as f32 * 0.5;

const GRAVITY:     f32 = 0.001;
const STIFFNESS:   f32 = 0.04;
const MAX_STRETCH: f32 = 0.1;

const WIND_VARIATION_SPEED: f32 = 0.02;
const WIND_SPEED:           f32 = 0.001;

const UPDATE_ITERATIONS: u32 = 50;
const FIXUP_ITERATIONS:  u32 = 1;

const DEBUG_POSITION_SCALE: f32 = 0.2;
const DEBUG_VIEWPORT_SCALE: i32 = 5;

const TILE_SIZE: u32 = 256;
// FIXME(pcwalton): Don't hardcode this.
const TILE_BACKING_SIZE: u32 = 258;
const CACHE_TILES_ACROSS: u32 = 16;
const CACHE_TILES_DOWN: u32 = 16;
const TILE_CACHE_WIDTH: u32 = CACHE_TILES_ACROSS * TILE_BACKING_SIZE;
const TILE_CACHE_HEIGHT: u32 = CACHE_TILES_DOWN * TILE_BACKING_SIZE;
const TILE_HASH_INITIAL_BUCKET_SIZE: u32 = 64;

const DERIVATIVES_VIEWPORT_SCALE_FACTOR: i32 = 16;

static BACKGROUND_COLOR: SolidSource = SolidSource { r: 255, g: 255, b: 255, a: 255 };

static QUAD_VERTEX_POSITIONS: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
static QUAD_INDICES:          [u32; 6] = [0, 1, 2, 1, 3, 2];

/*
static NEIGHBOR_OFFSET_VECTORS: [[f32; 2]; 8] = [
    [-1.0, -1.0], [ 0.0, -1.0], [ 1.0, -1.0],
    [-1.0,  0.0],               [ 1.0,  0.0],
    [-1.0,  1.0], [ 0.0,  1.0], [ 1.0,  1.0],
];
*/
static NEIGHBOR_OFFSET_VECTORS: [[f32; 2]; 4] = [
                  [ 0.0, -1.0],
    [-1.0,  0.0],               [ 1.0,  0.0],
                  [ 0.0,  1.0],
];

const DRAW_LINES: bool = false;

#[repr(C)]
struct ClothRenderVertex {
    position: Vector2F,
}

fn main() {
    env_logger::init();

    // Get the SVG path.
    let svg_path = match env::args().nth(1) {
        Some(path) => path,
        None => DEFAULT_SVG_PATH.to_owned(),
    };

    // Set up the window.
    let mut event_loop = EventsLoop::new();
    let dpi = event_loop.get_primary_monitor().get_hidpi_factor() as f32;
    let logical_window_size = LogicalSize::new(WINDOW_WIDTH as f64, WINDOW_HEIGHT as f64);
    let physical_window_size =
        Vector2F::new(WINDOW_WIDTH as f32, WINDOW_HEIGHT as f32).scale(dpi).to_i32();
    let window = WindowBuilder::new().with_title("Cloth example")
                                     .with_dimensions(logical_window_size)
                                     .build(&event_loop)
                                     .unwrap();
    window.show();

    // Create a window surface using `surfman`.
    let connection = Connection::from_winit_window(&window).unwrap();
    let native_widget = connection.create_native_widget_from_winit_window(&window).unwrap();
    let adapter = connection.create_low_power_adapter().unwrap();
    let mut surfman_device = connection.create_device(&adapter).unwrap();

    // Create a `surfman` context descriptor.
    let context_attributes = ContextAttributes {
        version: SurfmanGLVersion::new(3, 3),
        flags: ContextAttributeFlags::ALPHA | ContextAttributeFlags::DEPTH,
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

    // Create the vertex position LUT textures.
    let vertex_positions = [0.0; MESH_VERTEX_COUNT as usize * 4];
    let vertex_position_texture_size = Vector2I::new(MESH_VERTICES_ACROSS, MESH_VERTICES_DOWN);
    let vertex_position_texture =
        device.create_texture_from_data(TextureFormat::RGBA32F,
                                        vertex_position_texture_size,
                                        TextureDataRef::F32(&vertex_positions));
    let last_vertex_position_texture =
        device.create_texture_from_data(TextureFormat::RGBA32F,
                                        vertex_position_texture_size,
                                        TextureDataRef::F32(&vertex_positions));

    // Create the vertex position framebuffers.
    let mut vertex_position_framebuffer = device.create_framebuffer(vertex_position_texture);
    let mut last_vertex_position_framebuffer =
        device.create_framebuffer(last_vertex_position_texture);

    // Create the cloth programs.
    let cloth_update_program = ClothUpdateProgram::new(&device, &resources);
    let cloth_fixup_program = ClothFixupProgram::new(&device, &resources);
    let cloth_debug_lut_program = ClothDebugLUTProgram::new(&device, &resources);
    let cloth_render_prepare_program = ClothRenderPrepareProgram::new(&device, &resources);
    let cloth_render_draw_program = ClothRenderDrawProgram::new(&device, &resources);

    // Create the cloth render vertex buffer.
    let mut cloth_render_vertices = Vec::with_capacity(MESH_VERTEX_COUNT as usize);
    for y in 0..MESH_VERTICES_DOWN {
        for x in 0..MESH_VERTICES_ACROSS {
            cloth_render_vertices.push(ClothRenderVertex {
                position: Vector2F::new(x as f32, y as f32),
            })
        }
    }

    // Initialize the cloth render indices.
    let mut cloth_render_indices: Vec<u32> = Vec::with_capacity(MESH_INDEX_COUNT as usize);
    for y in 0..MESH_PATCHES_DOWN {
        for x in 0..MESH_PATCHES_ACROSS {
            let upper_left = (x + MESH_VERTICES_ACROSS * y)       as u32;
            let lower_left = (x + MESH_VERTICES_ACROSS * (y + 1)) as u32;
            let (upper_right, lower_right) = (upper_left + 1, lower_left + 1);
            if !DRAW_LINES {
                cloth_render_indices.extend_from_slice(&[
                    upper_left,  upper_right, lower_left,
                    upper_right, lower_right, lower_left,
                ]);
            } else {
                cloth_render_indices.extend_from_slice(&[
                    upper_left,  upper_right,
                    upper_right, lower_right,
                    lower_right, lower_left,
                    lower_left,  upper_left,
                ]);
            }
        }
    }

    // Set up the cloth update vertex buffers.
    let cloth_update_vertex_buffer = device.create_buffer();
    let cloth_update_index_buffer = device.create_buffer();
    device.allocate_buffer(&cloth_update_vertex_buffer,
                           BufferData::Memory(&QUAD_VERTEX_POSITIONS),
                           BufferTarget::Vertex,
                           BufferUploadMode::Static);
    device.allocate_buffer(&cloth_update_index_buffer,
                           BufferData::Memory(&QUAD_INDICES),
                           BufferTarget::Index,
                           BufferUploadMode::Static);

    // Set up the cloth render vertex buffers.
    let cloth_render_vertex_buffer = device.create_buffer();
    let cloth_render_index_buffer = device.create_buffer();
    device.allocate_buffer(&cloth_render_vertex_buffer,
                           BufferData::Memory(&cloth_render_vertices),
                           BufferTarget::Vertex,
                           BufferUploadMode::Static);
    device.allocate_buffer(&cloth_render_index_buffer,
                           BufferData::Memory(&cloth_render_indices),
                           BufferTarget::Index,
                           BufferUploadMode::Static);

    // Create the cloth update vertex array.
    let cloth_update_vertex_array = device.create_vertex_array();
    device.bind_buffer(&cloth_update_vertex_array,
                       &cloth_update_vertex_buffer,
                       BufferTarget::Vertex);
    device.bind_buffer(&cloth_update_vertex_array,
                       &cloth_update_index_buffer,
                       BufferTarget::Index);
    let quad_vertex_attr_descriptor = VertexAttrDescriptor {
        size: 2,
        class: VertexAttrClass::Float,
        attr_type: VertexAttrType::F32,
        stride: 4 * 2,
        offset: 0,
        divisor: 0,
        buffer_index: 0,
    };
    device.configure_vertex_attr(&cloth_update_vertex_array,
                                 &cloth_update_program.position_attribute,
                                 &quad_vertex_attr_descriptor);

    // Create the cloth fixup vertex array.
    let cloth_fixup_vertex_array = device.create_vertex_array();
    device.bind_buffer(&cloth_fixup_vertex_array,
                       &cloth_update_vertex_buffer,
                       BufferTarget::Vertex);
    device.bind_buffer(&cloth_fixup_vertex_array,
                       &cloth_update_index_buffer,
                       BufferTarget::Index);
    let quad_vertex_attr_descriptor = VertexAttrDescriptor {
        size: 2,
        class: VertexAttrClass::Float,
        attr_type: VertexAttrType::F32,
        stride: 4 * 2,
        offset: 0,
        divisor: 0,
        buffer_index: 0,
    };
    device.configure_vertex_attr(&cloth_fixup_vertex_array,
                                 &cloth_fixup_program.position_attribute,
                                 &quad_vertex_attr_descriptor);

    // Create the cloth debug LUT vertex array.
    let cloth_debug_lut_vertex_array = device.create_vertex_array();
    device.bind_buffer(&cloth_debug_lut_vertex_array,
                       &cloth_update_vertex_buffer,
                       BufferTarget::Vertex);
    device.bind_buffer(&cloth_debug_lut_vertex_array,
                       &cloth_update_index_buffer,
                       BufferTarget::Index);
    device.configure_vertex_attr(&cloth_debug_lut_vertex_array,
                                 &cloth_debug_lut_program.position_attribute,
                                 &quad_vertex_attr_descriptor);

    // Create the cloth render prepare and draw vertex arrays.
    let cloth_render_prepare_vertex_array =
        cloth_render_prepare_program.cloth_render_program_info
                                    .create_vertex_array(&device,
                                                         &cloth_render_vertex_buffer,
                                                         &cloth_render_index_buffer);
    let cloth_render_draw_vertex_array =
        cloth_render_draw_program.cloth_render_program_info
                                 .create_vertex_array(&device,
                                                      &cloth_render_vertex_buffer,
                                                      &cloth_render_index_buffer);

    // Create the virtual texture.
    let virtual_texture_cache_size = Vector2I::new(TILE_CACHE_WIDTH as i32,
                                                   TILE_CACHE_HEIGHT as i32);
    let virtual_texture = VirtualTexture::new(virtual_texture_cache_size,
                                              TILE_SIZE,
                                              TILE_HASH_INITIAL_BUCKET_SIZE);
    let virtual_texture_manager = VirtualTextureManager::new(virtual_texture,
                                                             physical_window_size);
    let mut virtual_texture_renderer = AdvancedRenderer::new(&device,
                                                             virtual_texture_manager,
                                                             DERIVATIVES_VIEWPORT_SCALE_FACTOR);

    // Create the derivatives texture.
    // FIXME(pcwalton): The library should automatically do this internally, I think?
    let derivatives_texture =
        device.create_texture(TextureFormat::RGBA32F,
                              virtual_texture_renderer.derivatives_viewport().size());
    let derivatives_framebuffer = device.create_framebuffer(derivatives_texture);

    // Create the SVG rasterizer.
    let thread_count = num_cpus::get_physical() as u32;
    let mut svg_rasterizer_proxy = SVGRasterizerProxy::new(svg_path,
                                                           BACKGROUND_COLOR,
                                                           TILE_SIZE,
                                                           thread_count);
    let svg_size = svg_rasterizer_proxy.wait_for_svg_to_load();

    // Enter the main loop.
    let mut needed_tiles = vec![];
    let (mut camera_angle, mut camera_distance) = (FRAC_PI_2, INITIAL_CAMERA_DISTANCE);
    let mut time = 0.0;
    let mut exit = false;
    while !exit {
        // Calculate view.
        let aspect = WINDOW_WIDTH as f32 / WINDOW_HEIGHT as f32;
        let camera_position = Vector3F::new(f32::cos(camera_angle),
                                            0.0,
                                            f32::sin(camera_angle)).scale(camera_distance);

        let mut transform = Transform4F::from_perspective(FRAC_PI_2, aspect, 0.1, 1000.0);
        transform = transform * Transform4F::looking_at(camera_position,
                                                        Vector3F::default(),
                                                        Vector3F::new(0.0, 1.0, 0.0));
        transform = transform *
            Transform4F::from_translation(Vector4F::new(-MESH_CENTER_X, -MESH_CENTER_Y, 0.0, 1.0));

        // Start commands.
        device.begin_commands();

        let wind_speed = f32::sin(time * WIND_VARIATION_SPEED) * WIND_SPEED;
        let mut global_force = Vector3F::new(0.0, -GRAVITY, 0.0);
        global_force += Vector3F::new(0.0, 0.0, 1.0).scale(wind_speed);

        for _ in 0..UPDATE_ITERATIONS {
            // Update the cloth.
            device.draw_elements(QUAD_INDICES.len() as u32, &RenderState {
                target: &RenderTarget::Framebuffer(&last_vertex_position_framebuffer),
                program: &cloth_update_program.program,
                vertex_array: &cloth_update_vertex_array,
                primitive: Primitive::Triangles,
                uniforms: &[
                    (&cloth_update_program.global_force_uniform,
                    UniformData::Vec4(global_force.0)),
                    (&cloth_update_program.stiffness_uniform, UniformData::Float(STIFFNESS)),
                    (&cloth_update_program.last_positions_uniform, UniformData::TextureUnit(0)),
                    (&cloth_update_program.framebuffer_size_uniform,
                    UniformData::Vec2(vertex_position_texture_size.to_f32().0)),
                ],
                textures: &[device.framebuffer_texture(&vertex_position_framebuffer)],
                viewport: RectI::new(Vector2I::splat(0), vertex_position_texture_size),
                options: RenderOptions {
                    blend: Some(BlendState {
                        func: BlendFunc::RGBOneAlphaOne,
                        op: BlendOp::Subtract,
                    }),
                    ..RenderOptions::default()
                }
            });

            // Swap the two vertex position framebuffers.
            mem::swap(&mut last_vertex_position_framebuffer, &mut vertex_position_framebuffer);

            // Fix up the cloth so it doesn't explode.
            for _ in 0..FIXUP_ITERATIONS {
                for neighbor_offset_vector in &NEIGHBOR_OFFSET_VECTORS {
                    device.draw_elements(QUAD_INDICES.len() as u32, &RenderState {
                        target: &RenderTarget::Framebuffer(&last_vertex_position_framebuffer),
                        program: &cloth_fixup_program.program,
                        vertex_array: &cloth_fixup_vertex_array,
                        primitive: Primitive::Triangles,
                        uniforms: &[
                            (&cloth_fixup_program.last_positions_uniform,
                             UniformData::TextureUnit(0)),
                            (&cloth_fixup_program.framebuffer_size_uniform,
                            UniformData::Vec2(vertex_position_texture_size.to_f32().0)),
                            (&cloth_fixup_program.neighbor_offset_uniform,
                            UniformData::Vec2(F32x2::new(neighbor_offset_vector[0],
                                                         neighbor_offset_vector[1]))),
                            (&cloth_fixup_program.max_stretch_uniform,
                            UniformData::Float(MAX_STRETCH)),
                        ],
                        textures: &[device.framebuffer_texture(&vertex_position_framebuffer)],
                        viewport: RectI::new(Vector2I::splat(0), vertex_position_texture_size),
                        options: RenderOptions { blend: None, ..RenderOptions::default() },
                    });

                    // Swap the two vertex position framebuffers again.
                    mem::swap(&mut last_vertex_position_framebuffer,
                              &mut vertex_position_framebuffer);
                }
            }
        }

        // Prepare to draw the cloth.
        let mut uniforms = vec![
            (&cloth_render_prepare_program.cloth_render_program_info.transform_uniform,
             UniformData::Mat4([transform.c0, transform.c1, transform.c2, transform.c3])),
            (&cloth_render_prepare_program.cloth_render_program_info.texture_size_uniform,
             UniformData::Vec2(svg_size.to_f32().0)),
            (&cloth_render_prepare_program.cloth_render_program_info.vertex_positions_uniform,
             UniformData::TextureUnit(0)),
            (&cloth_render_prepare_program.cloth_render_program_info.vertex_positions_size_uniform,
             UniformData::Vec2(vertex_position_texture_size.to_f32().0)),
        ];
        virtual_texture_renderer.push_prepare_uniforms(
            &cloth_render_prepare_program.virtex_uniforms,
            &mut uniforms);
        device.draw_elements(cloth_render_indices.len() as u32, &RenderState {
            target: &RenderTarget::Framebuffer(&derivatives_framebuffer),
            program: &cloth_render_prepare_program.program,
            vertex_array: &cloth_render_prepare_vertex_array,
            primitive: if DRAW_LINES { Primitive::Lines } else { Primitive::Triangles },
            uniforms: &uniforms,
            textures: &[device.framebuffer_texture(&vertex_position_framebuffer)],
            viewport: virtual_texture_renderer.derivatives_viewport(),
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(0.0, 0.0, 0.0, 0.0)),
                    ..ClearOps::default()
                },
                depth: None,
                ..RenderOptions::default()
            }
        });
        let texture_data = device.read_pixels(&RenderTarget::Framebuffer(&derivatives_framebuffer),
                                              virtual_texture_renderer.derivatives_viewport());
        device.end_commands();

        // Determine which tiles we need to rasterize, and rasterize them.
        virtual_texture_renderer.request_needed_tiles(&texture_data, &mut needed_tiles);
        svg_rasterizer_proxy.rasterize_needed_tiles(&device,
                                                    &mut virtual_texture_renderer,
                                                    &mut needed_tiles);

        // Update metadata in preparation to draw the cloth.
        virtual_texture_renderer.update_metadata(&device);

        // Draw the cloth.
        device.begin_commands();
        let mut uniforms = vec![
            (&cloth_render_draw_program.cloth_render_program_info.transform_uniform,
             UniformData::Mat4([transform.c0, transform.c1, transform.c2, transform.c3])),
            (&cloth_render_draw_program.cloth_render_program_info.texture_size_uniform,
             UniformData::Vec2(svg_size.to_f32().0)),
            (&cloth_render_draw_program.cloth_render_program_info.vertex_positions_uniform,
             UniformData::TextureUnit(0)),
            (&cloth_render_draw_program.cloth_render_program_info.vertex_positions_size_uniform,
             UniformData::Vec2(vertex_position_texture_size.to_f32().0)),
        ];
        let mut textures = vec![device.framebuffer_texture(&vertex_position_framebuffer)];
        virtual_texture_renderer.push_render_uniforms(
            &cloth_render_draw_program.virtex_uniforms,
            &mut uniforms,
            &mut textures);
        device.draw_elements(cloth_render_indices.len() as u32, &RenderState {
            target: &RenderTarget::Default,
            program: &cloth_render_draw_program.program,
            vertex_array: &cloth_render_draw_vertex_array,
            primitive: if DRAW_LINES { Primitive::Lines } else { Primitive::Triangles },
            uniforms: &uniforms,
            textures: &textures,
            viewport: RectI::new(Vector2I::splat(0), physical_window_size),
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(0.5, 0.5, 0.5, 1.0)),
                    depth: Some(1.0),
                    ..ClearOps::default()
                },
                depth: Some(DepthState { func: DepthFunc::Less, write: true }),
                ..RenderOptions::default()
            }
        });

        // Draw the debug LUT visualization.
        device.draw_elements(QUAD_INDICES.len() as u32, &RenderState {
            target: &RenderTarget::Default,
            program: &cloth_debug_lut_program.program,
            vertex_array: &cloth_debug_lut_vertex_array,
            primitive: Primitive::Triangles,
            uniforms: &[
                (&cloth_debug_lut_program.positions_uniform, UniformData::TextureUnit(0)),
                (&cloth_debug_lut_program.scale_uniform, UniformData::Float(DEBUG_POSITION_SCALE)),
            ],
            textures: &[device.framebuffer_texture(&vertex_position_framebuffer)],
            viewport: RectI::new(Vector2I::splat(0),
                                 vertex_position_texture_size.scale(DEBUG_VIEWPORT_SCALE)),
            options: RenderOptions::default(),
        });

        // Submit commands.
        device.end_commands();

        let mut surface = surfman_device.unbind_surface_from_context(&mut context)
                                        .unwrap()
                                        .unwrap();
        surfman_device.present_surface(&mut context, &mut surface).unwrap();
        surfman_device.bind_surface_to_context(&mut context, surface).unwrap();

        event_loop.poll_events(|event| {
            match event {
                Event::WindowEvent { event: WindowEvent::Destroyed, .. } |
                Event::DeviceEvent {
                    event: DeviceEvent::Key(KeyboardInput {
                        virtual_keycode: Some(VirtualKeyCode::Escape),
                        ..
                    }),
                    ..
                } => exit = true,
                Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta: (delta_x, delta_y) },
                    ..
                } => {
                    camera_angle += delta_x as f32 * CAMERA_ROTATION_SPEED;
                    camera_distance += delta_y as f32 * CAMERA_TRANSLATION_SPEED;
                }
                _ => {}
            }
        });

        time += 1.0;
    }
}

struct ClothUpdateProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    last_positions_uniform: GLUniform,
    framebuffer_size_uniform: GLUniform,
    global_force_uniform: GLUniform,
    stiffness_uniform: GLUniform,
}

impl ClothUpdateProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothUpdateProgram {
        let program = device.create_program(resources, "cloth_update");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let last_positions_uniform = device.get_uniform(&program, "LastPositions");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let global_force_uniform = device.get_uniform(&program, "GlobalForce");
        let stiffness_uniform = device.get_uniform(&program, "Stiffness");
        ClothUpdateProgram {
            program,
            position_attribute,
            last_positions_uniform,
            framebuffer_size_uniform,
            global_force_uniform,
            stiffness_uniform,
        }
    }
}

struct ClothFixupProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    last_positions_uniform: GLUniform,
    framebuffer_size_uniform: GLUniform,
    neighbor_offset_uniform: GLUniform,
    max_stretch_uniform: GLUniform,
}

impl ClothFixupProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothFixupProgram {
        let program = device.create_program_from_shader_names(resources,
                                                              "cloth_fixup",
                                                              "cloth_update",
                                                              "cloth_fixup");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let last_positions_uniform = device.get_uniform(&program, "LastPositions");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let neighbor_offset_uniform = device.get_uniform(&program, "NeighborOffset");
        let max_stretch_uniform = device.get_uniform(&program, "MaxStretch");
        ClothFixupProgram {
            program,
            position_attribute,
            last_positions_uniform,
            framebuffer_size_uniform,
            neighbor_offset_uniform,
            max_stretch_uniform,
        }
    }
}

struct ClothDebugLUTProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    positions_uniform: GLUniform,
    scale_uniform: GLUniform,
}

impl ClothDebugLUTProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothDebugLUTProgram {
        let program = device.create_program_from_shader_names(resources,
                                                              "cloth_debug_lut",
                                                              "cloth_update",
                                                              "cloth_debug_lut");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let positions_uniform = device.get_uniform(&program, "Positions");
        let scale_uniform = device.get_uniform(&program, "PositionScale");
        ClothDebugLUTProgram { program, position_attribute, positions_uniform, scale_uniform }
    }
}

struct ClothRenderPrepareProgram {
    program: GLProgram,
    cloth_render_program_info: ClothRenderProgramInfo,
    virtex_uniforms: PrepareAdvancedUniforms<GLDevice>,
}

impl ClothRenderPrepareProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothRenderPrepareProgram {
        let program = device.create_program_from_shader_names(resources,
                                                              "cloth_render_prepare",
                                                              "cloth_render",
                                                              "prepare_advanced");
        let cloth_render_program_info = ClothRenderProgramInfo::new(device, &program);
        let virtex_uniforms = PrepareAdvancedUniforms::new(device, &program);
        ClothRenderPrepareProgram { program, cloth_render_program_info, virtex_uniforms }
    }
}

struct ClothRenderDrawProgram {
    program: GLProgram,
    cloth_render_program_info: ClothRenderProgramInfo,
    virtex_uniforms: RenderAdvancedUniforms<GLDevice>,
}

impl ClothRenderDrawProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothRenderDrawProgram {
        let program = device.create_program_from_shader_names(resources,
                                                              "cloth_render_draw",
                                                              "cloth_render",
                                                              "render_advanced");
        let cloth_render_program_info = ClothRenderProgramInfo::new(device, &program);
        let virtex_uniforms = RenderAdvancedUniforms::new(device, &program);
        ClothRenderDrawProgram { program, cloth_render_program_info, virtex_uniforms }
    }
}

struct ClothRenderProgramInfo {
    position_attribute: GLVertexAttr,
    transform_uniform: GLUniform,
    texture_size_uniform: GLUniform,
    vertex_positions_uniform: GLUniform,
    vertex_positions_size_uniform: GLUniform,
}

impl ClothRenderProgramInfo {
    fn new(device: &GLDevice, program: &GLProgram) -> ClothRenderProgramInfo {
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let transform_uniform = device.get_uniform(&program, "Transform");
        let texture_size_uniform = device.get_uniform(&program, "TextureSize");
        let vertex_positions_uniform = device.get_uniform(&program, "VertexPositions");
        let vertex_positions_size_uniform = device.get_uniform(&program, "VertexPositionsSize");
        ClothRenderProgramInfo {
            position_attribute,
            transform_uniform,
            texture_size_uniform,
            vertex_positions_uniform,
            vertex_positions_size_uniform,
        }
    }

    fn create_vertex_array(&self,
                           device: &GLDevice,
                           cloth_render_vertex_buffer: &GLBuffer,
                           cloth_render_index_buffer: &GLBuffer)
                           -> GLVertexArray {
        let cloth_render_vertex_array = device.create_vertex_array();
        device.bind_buffer(&cloth_render_vertex_array,
                        &cloth_render_vertex_buffer,
                        BufferTarget::Vertex);
        device.bind_buffer(&cloth_render_vertex_array,
                        &cloth_render_index_buffer,
                        BufferTarget::Index);
        device.configure_vertex_attr(&cloth_render_vertex_array,
                                     &self.position_attribute,
                                     &VertexAttrDescriptor {
                                        size: 2,
                                        class: VertexAttrClass::Float,
                                        attr_type: VertexAttrType::F32,
                                        stride: 4 * 2,
                                        offset: 0,
                                        divisor: 0,
                                        buffer_index: 0,
                                     });
        cloth_render_vertex_array
    }
}
