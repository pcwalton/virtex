// virtex/src/renderer_simple.rs

use crate::manager2d::VirtualTextureManager2D;

use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gpu::resources::ResourceLoader;
use pathfinder_gpu::{BlendState, BufferData, BufferTarget, BufferUploadMode, ClearOps, Device};
use pathfinder_gpu::{Primitive, RenderOptions, RenderState, RenderTarget, TextureFormat};
use pathfinder_gpu::{UniformData, VertexAttrClass, VertexAttrDescriptor, VertexAttrType};

static QUAD_VERTEX_POSITIONS: [u8; 8] = [0, 0, 1, 0, 0, 1, 1, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 2, 1, 3, 2];

pub struct SimpleRenderer<D> where D: Device {
    manager: VirtualTextureManager2D,
    render_vertex_array: RenderSimpleVertexArray<D>,
    cache_texture: D::Texture,
}

impl<D> SimpleRenderer<D> where D: Device {
    pub fn new(device: &D, manager: VirtualTextureManager2D, resource_loader: &dyn ResourceLoader)
               -> SimpleRenderer<D> {
        let cache_texture = device.create_texture(TextureFormat::RGBA8,
                                                  manager.texture.cache_texture_size());
        let render_vertex_array = RenderSimpleVertexArray::new(device, resource_loader);
        SimpleRenderer { manager, render_vertex_array, cache_texture }
    }

    pub fn render(&mut self, device: &D) {
        let tile_size = self.manager.texture.tile_size();
        let tile_backing_size = self.manager.texture.tile_backing_size();

        device.begin_commands();
        let mut cleared = false;

        // Render the two LODs in order.
        let current_scale = self.manager.current_scale();
        let current_lod = current_scale.log2();
        println!("current_lod = {}", current_lod);
        let current_lods = self.manager.current_lods();
        let high_lod_opacity = current_lod.fract();

        for (render_lod_index, &render_lod) in current_lods.iter().enumerate() {
            let opacity = if render_lod_index == 0 { 1.0 } else { high_lod_opacity };
            for tile_cache_entry in self.manager.texture.all_cached_tiles() {
                let descriptor = &tile_cache_entry.descriptor;
                if descriptor.lod != render_lod {
                    continue;
                }

                let tile_position = Vector2F::new(descriptor.x as f32, descriptor.y as f32);
                let scaled_tile_size = tile_size as f32 / (1 << render_lod) as f32;
                let tile_rect = RectF::new(tile_position,
                                           Vector2F::splat(1.0)).scale(scaled_tile_size);

                let tile_tex_origin = Vector2I::splat(1) +
                    tile_cache_entry.address.0.scale(tile_backing_size as i32);
                let tile_tex_size = Vector2I::splat(tile_size as i32);

                let cache_tex_size = self.manager.texture.cache_texture_size();
                let cache_tex_scale = Vector2F::new(1.0 / cache_tex_size.x() as f32,
                                                    1.0 / cache_tex_size.y() as f32);
                let tile_tex_rect =
                    RectI::new(tile_tex_origin, tile_tex_size).to_f32().scale_xy(cache_tex_scale);

                //println!("tile_tex_rect={:?}", tile_tex_rect);
                device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
                    target: &RenderTarget::Default,
                    program: &self.render_vertex_array.render_program.program,
                    vertex_array: &self.render_vertex_array.vertex_array,
                    primitive: Primitive::Triangles,
                    uniforms: &[
                        (&self.render_vertex_array.render_program.tile_rect_uniform,
                         UniformData::Vec4(tile_rect.0)),
                        (&self.render_vertex_array.render_program.tile_tex_rect_uniform,
                         UniformData::Vec4(tile_tex_rect.0)),
                        (&self.render_vertex_array.render_program.framebuffer_size_uniform,
                         UniformData::Vec2(self.manager.viewport_size().to_f32().0)),
                        (&self.render_vertex_array.render_program.transform_uniform,
                         UniformData::Mat2(self.manager.transform.matrix.0)),
                        (&self.render_vertex_array.render_program.translation_uniform,
                         UniformData::Vec2(self.manager.transform.vector.0)),
                        (&self.render_vertex_array.render_program.opacity_uniform,
                         UniformData::Float(opacity)),
                        (&self.render_vertex_array.render_program.tile_cache_uniform,
                         UniformData::TextureUnit(0)),
                    ],
                    textures: &[&self.cache_texture],
                    viewport: RectI::new(Vector2I::splat(0), self.manager.viewport_size()),
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
    }

    #[inline]
    pub fn manager_mut(&mut self) -> &mut VirtualTextureManager2D {
        &mut self.manager
    }

    #[inline]
    pub fn cache_texture(&self) -> &D::Texture {
        &self.cache_texture
    }
}

struct RenderSimpleVertexArray<D> where D: Device {
    render_program: RenderSimpleProgram<D>,
    vertex_array: D::VertexArray,
    #[allow(dead_code)]
    quad_vertex_positions_buffer: D::Buffer,
    #[allow(dead_code)]
    quad_vertex_indices_buffer: D::Buffer,
}

impl<D> RenderSimpleVertexArray<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> RenderSimpleVertexArray<D> {
        let render_program = RenderSimpleProgram::new(device, resources);
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
        RenderSimpleVertexArray {
            render_program,
            vertex_array,
            quad_vertex_positions_buffer,
            quad_vertex_indices_buffer,
        }
    }
}

struct RenderSimpleProgram<D> where D: Device {
    program: D::Program,
    position_attribute: D::VertexAttr,
    tile_rect_uniform: D::Uniform,
    tile_tex_rect_uniform: D::Uniform,
    framebuffer_size_uniform: D::Uniform,
    transform_uniform: D::Uniform,
    translation_uniform: D::Uniform,
    tile_cache_uniform: D::Uniform,
    opacity_uniform: D::Uniform,
}

impl<D> RenderSimpleProgram<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> RenderSimpleProgram<D> {
        let program = device.create_program(resources, "render_simple");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let tile_rect_uniform = device.get_uniform(&program, "TileRect");
        let tile_tex_rect_uniform = device.get_uniform(&program, "TileTexRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let tile_cache_uniform = device.get_uniform(&program, "TileCache");
        let opacity_uniform = device.get_uniform(&program, "Opacity");
        RenderSimpleProgram {
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
