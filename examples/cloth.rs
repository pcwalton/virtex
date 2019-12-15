// virtex/examples/cloth.rs

use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::transform3d::Transform4F;
use pathfinder_geometry::vector::{Vector2F, Vector2I, Vector3F};
use pathfinder_gl::{GLDevice, GLUniform, GLVersion, GLVertexAttr};
use pathfinder_gpu::resources::FilesystemResourceLoader;
use pathfinder_gpu::{BufferData, BufferTarget, BufferUploadMode, Device, RenderState};
use raqote::{DrawTarget, SolidSource, Transform};
use resvg::{Options as ResvgOptions, ScreenSize};
use resvg::backend_raqote;
use resvg::usvg::{Options as UsvgOptions, Tree};
use std::env;
use std::slice;
use surfman::{Connection, ContextAttributeFlags, ContextAttributes, GLVersion as SurfmanGLVersion};
use surfman::{SurfaceAccess, SurfaceType};
use winit::dpi::LogicalSize;
use winit::{DeviceEvent, Event, EventsLoop, KeyboardInput, ModifiersState, MouseScrollDelta};
use winit::{VirtualKeyCode, WindowBuilder, WindowEvent};

#[repr(C)]
struct ClothVertex {
    position: Vector3F,
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

    // Create the cloth program.
    let cloth_program = ClothProgram::new(device, resources);

    // Create the cloth vertex buffer.
    let cloth_vertices: [ClothVertex; 3] = [
        ClothVertex { position: Vector3F::new( 0.0,  3.0, 0.0) },
        ClothVertex { position: Vector3F::new( 3.0, -3.0, 0.0) },
        ClothVertex { position: Vector3F::new(-3.0, -3.0, 0.0) },
    ];
    let cloth_indices: [u32; 3] = [0, 1, 2];
    let cloth_vertex_buffer = device.create_buffer();
    let cloth_index_buffer = device.create_buffer();
    device.allocate_buffer(&cloth_vertex_buffer,
                           BufferData::Memory(&cloth_vertices),
                           BufferTarget::Vertex,
                           BufferUploadMode::Static);
    device.allocate_buffer(&cloth_index_buffer,
                           BufferData::Memory(&cloth_indices),
                           BufferTarget::Index,
                           BufferUploadMode::Static);

    // Enter the main loop.
    let mut exit = false;
    let mut cleared = false;
    while !exit {
        device.begin_commands();
        device.draw_elements(3, &RenderState {
            target: &RenderTarget::Default,
            program: &self.cloth_program.program,
            vertex_array: &self.cloth_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            uniforms: &[
                (&self.cloth_program.transform_uniform, UniformData::Mat4(Transform4F::default())),
            ],
            textures: &[],
            viewport: RectI::new(Vector2I::splat(0), Vector2I::new(WINDOW_WIDTH, WINDOW_HEIGHT)),
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: if !cleared {
                        Some(ColorF::new(0.0, 0.0, 0.0, 1.0))
                    } else {
                        None
                    },
                    ..ClearOps::default()
                },
                ..RenderOptions::default()
            }
        });
        device.end_commands();

        cleared = true;

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
                _ => {}
            }
        });
    }
}

struct ClothProgram {
    program: GLProgram,
    position_attribute: GLVertexAttr,
    transform_uniform: GLUniform,
}

impl ClothProgram {
    fn new(device: &D, resources: &dyn ResourceLoader) -> RenderSimpleProgram<D> {
        let program = device.create_program(resources, "cloth");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let transform_uniform = device.get_uniform(&program, "Transform");
        ClothProgram {
            program,
            position_attribute,
            transform_uniform,
        }
    }
}
