// virtex/examples/cloth.rs

use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform3d::Transform4F;
use pathfinder_geometry::vector::{Vector2F, Vector2I, Vector3F, Vector4F};
use pathfinder_gl::{GLDevice, GLProgram, GLUniform, GLVersion, GLVertexAttr};
use pathfinder_gpu::resources::{FilesystemResourceLoader, ResourceLoader};
use pathfinder_gpu::{BlendFunc, BlendOp, BlendState, BufferData, BufferTarget, BufferUploadMode};
use pathfinder_gpu::{ClearOps, Device, Primitive, RenderOptions, RenderState, RenderTarget};
use pathfinder_gpu::{TextureFormat, TextureDataRef, UniformData, VertexAttrClass};
use pathfinder_gpu::{VertexAttrDescriptor, VertexAttrType};
use pathfinder_simd::default::F32x4;
use std::f32::consts::FRAC_PI_2;
use std::mem;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use winit::dpi::LogicalSize;
use winit::{DeviceEvent, Event, EventsLoop, KeyboardInput, VirtualKeyCode};
use winit::{WindowBuilder, WindowEvent};

const WINDOW_WIDTH:  i32 = 800;
const WINDOW_HEIGHT: i32 = 600;

const INITIAL_CAMERA_DISTANCE: f32 = 75.0;

const CAMERA_ROTATION_SPEED:    f32 = 0.01;
const CAMERA_TRANSLATION_SPEED: f32 = 0.1;

const MESH_PATCHES_ACROSS:  i32 = 50;
const MESH_PATCHES_DOWN:    i32 = 50;
const MESH_PATCH_COUNT:     i32 = MESH_PATCHES_ACROSS * MESH_PATCHES_DOWN;
const MESH_VERTICES_ACROSS: i32 = MESH_PATCHES_ACROSS + 1;
const MESH_VERTICES_DOWN:   i32 = MESH_PATCHES_DOWN + 1;
const MESH_VERTEX_COUNT:    i32 = MESH_VERTICES_ACROSS * MESH_VERTICES_DOWN;
// Uncomment for triangles:
//const MESH_INDEX_COUNT:     i32 = MESH_PATCH_COUNT * 6;
const MESH_INDEX_COUNT:     i32 = MESH_PATCH_COUNT * 8;
const MESH_CENTER_X:        f32 = MESH_PATCHES_ACROSS as f32 * 0.5;
const MESH_CENTER_Y:        f32 = MESH_PATCHES_DOWN as f32 * 0.5;

const GRAVITY: f32 = 0.003;
const SPRING:  f32 = -0.2;

const DEBUG_POSITION_SCALE: f32 = 0.1;
const DEBUG_VIEWPORT_SCALE: i32 = 5;

static QUAD_VERTEX_POSITIONS: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
static QUAD_INDICES:          [u32; 6] = [0, 1, 2, 1, 3, 2];

#[repr(C)]
struct ClothRenderVertex {
    position: Vector2F,
}

fn main() {
    // Initialization boilerplate.

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
    let cloth_debug_lut_program = ClothDebugLUTProgram::new(&device, &resources);
    let cloth_render_program = ClothRenderProgram::new(&device, &resources);

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
            // Uncomment for filled rects:
            /*
            cloth_render_indices.extend_from_slice(&[
                upper_left,  upper_right, lower_left,
                upper_right, lower_right, lower_left,
            ]);
            */
            cloth_render_indices.extend_from_slice(&[
                upper_left,  upper_right,
                upper_right, lower_right,
                lower_right, lower_left,
                lower_left,  upper_left,
            ]);
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

    // Create the cloth render vertex array.
    let cloth_render_vertex_array = device.create_vertex_array();
    device.bind_buffer(&cloth_render_vertex_array,
                       &cloth_render_vertex_buffer,
                       BufferTarget::Vertex);
    device.bind_buffer(&cloth_render_vertex_array,
                       &cloth_render_index_buffer,
                       BufferTarget::Index);
    device.configure_vertex_attr(&cloth_render_vertex_array,
                                 &cloth_render_program.position_attribute,
                                 &VertexAttrDescriptor {
                                    size: 2,
                                    class: VertexAttrClass::Float,
                                    attr_type: VertexAttrType::F32,
                                    stride: 4 * 2,
                                    offset: 0,
                                    divisor: 0,
                                    buffer_index: 0,
                                 });

    // Enter the main loop.
    let (mut camera_angle, mut camera_distance) = (FRAC_PI_2, INITIAL_CAMERA_DISTANCE);
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

        // Update the cloth.
        device.draw_elements(QUAD_INDICES.len() as u32, &RenderState {
            target: &RenderTarget::Framebuffer(&last_vertex_position_framebuffer),
            program: &cloth_update_program.program,
            vertex_array: &cloth_update_vertex_array,
            primitive: Primitive::Triangles,
            uniforms: &[
                (&cloth_update_program.gravity_uniform,
                 UniformData::Vec4(F32x4::new(0.0, -GRAVITY, 0.0, 0.0))),
                (&cloth_update_program.spring_uniform, UniformData::Float(SPRING)),
                (&cloth_update_program.last_vertex_positions_uniform,
                 UniformData::TextureUnit(0)),
                (&cloth_update_program.framebuffer_size_uniform,
                 UniformData::Vec2(vertex_position_texture_size.to_f32().0)),
            ],
            textures: &[device.framebuffer_texture(&vertex_position_framebuffer)],
            viewport: RectI::new(Vector2I::splat(0), vertex_position_texture_size),
            options: RenderOptions {
                blend: Some(BlendState { func: BlendFunc::RGBOneAlphaOne, op: BlendOp::Subtract }),
                ..RenderOptions::default()
            }
        });

        // Swap the two vertex position framebuffers.
        mem::swap(&mut last_vertex_position_framebuffer, &mut vertex_position_framebuffer);

        // Render the cloth.
        device.draw_elements(MESH_INDEX_COUNT as u32, &RenderState {
            target: &RenderTarget::Default,
            program: &cloth_render_program.program,
            vertex_array: &cloth_render_vertex_array,
            primitive: Primitive::Lines,
            uniforms: &[
                (&cloth_render_program.transform_uniform,
                 UniformData::Mat4([transform.c0, transform.c1, transform.c2, transform.c3])),
                (&cloth_render_program.vertex_positions_uniform,
                 UniformData::TextureUnit(0)),
                (&cloth_render_program.vertex_positions_size_uniform,
                 UniformData::Vec2(vertex_position_texture_size.to_f32().0)),
            ],
            textures: &[device.framebuffer_texture(&vertex_position_framebuffer)],
            viewport: RectI::new(Vector2I::splat(0), physical_window_size),
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(0.0, 0.0, 0.0, 1.0)),
                    ..ClearOps::default()
                },
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
    }
}

struct ClothUpdateProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    last_vertex_positions_uniform: GLUniform,
    framebuffer_size_uniform: GLUniform,
    gravity_uniform: GLUniform,
    spring_uniform: GLUniform,
}

impl ClothUpdateProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothUpdateProgram {
        let program = device.create_program(resources, "cloth_update");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let last_vertex_positions_uniform = device.get_uniform(&program, "LastVertexPositions");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let gravity_uniform = device.get_uniform(&program, "Gravity");
        let spring_uniform = device.get_uniform(&program, "Spring");
        ClothUpdateProgram {
            program,
            position_attribute,
            last_vertex_positions_uniform,
            framebuffer_size_uniform,
            gravity_uniform,
            spring_uniform,
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
        let scale_uniform = device.get_uniform(&program, "Scale");
        ClothDebugLUTProgram { program, position_attribute, positions_uniform, scale_uniform }
    }
}

struct ClothRenderProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    transform_uniform: GLUniform,
    vertex_positions_uniform: GLUniform,
    vertex_positions_size_uniform: GLUniform,
}

impl ClothRenderProgram {
    fn new(device: &GLDevice, resources: &dyn ResourceLoader) -> ClothRenderProgram {
        let program = device.create_program(resources, "cloth_render");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let transform_uniform = device.get_uniform(&program, "Transform");
        let vertex_positions_uniform = device.get_uniform(&program, "VertexPositions");
        let vertex_positions_size_uniform = device.get_uniform(&program, "VertexPositionsSize");
        ClothRenderProgram {
            program,
            position_attribute,
            transform_uniform,
            vertex_positions_uniform,
            vertex_positions_size_uniform,
        }
    }
}
