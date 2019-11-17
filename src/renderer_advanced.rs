// virtex/src/renderer_advanced.rs

use crate::manager2d::VirtualTextureManager2D;

use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gpu::resources::ResourceLoader;
use pathfinder_gpu::{BufferData, BufferTarget, BufferUploadMode, ClearOps, Device, Primitive, RenderOptions, RenderState, RenderTarget, TextureFormat, UniformData, VertexAttrClass, VertexAttrDescriptor, VertexAttrType};
use std::slice;

static QUAD_VERTEX_POSITIONS: [u8; 8] = [0, 0, 1, 0, 0, 1, 1, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 2, 1, 3, 2];

pub struct AdvancedRenderer<D> where D: Device {
    manager: VirtualTextureManager2D,
    render_vertex_array: RenderAdvancedVertexArray<D>,
    cache_texture: D::Texture,
    metadata_texture: D::Texture,
}

impl<D> AdvancedRenderer<D> where D: Device {
    pub fn new(device: &D, manager: VirtualTextureManager2D, resource_loader: &dyn ResourceLoader)
               -> AdvancedRenderer<D> {
        let cache_texture = device.create_texture(TextureFormat::RGBA8,
                                                  manager.texture.cache_texture_size());
        let metadata_texture_size = Vector2I::new(manager.texture.cache_size() as i32, 2);
        let metadata_texture = device.create_texture(TextureFormat::RGBA32F,
                                                     metadata_texture_size);
        let render_vertex_array = RenderAdvancedVertexArray::new(device, resource_loader);
        AdvancedRenderer { manager, render_vertex_array, cache_texture, metadata_texture }
    }

    #[inline]
    pub fn manager_mut(&mut self) -> &mut VirtualTextureManager2D {
        &mut self.manager
    }

    #[inline]
    pub fn cache_texture(&self) -> &D::Texture {
        &self.cache_texture
    }

    pub fn render(&mut self, device: &D) {
        // Pack and upload new metadata.
        let cache_size = self.manager.texture.cache_size() as i32;
        let metadata_texture_size = Vector2I::new(cache_size, 2);
        let metadata_stride = metadata_texture_size.x() as usize * 4;
        let mut metadata = vec![0.0; metadata_stride * metadata_texture_size.y() as usize];
        let cache_texture_scale =
            Vector2F::new(1.0 / self.manager.texture.tile_texture_tiles_across() as f32,
                          1.0 / self.manager.texture.tile_texture_tiles_down() as f32);
        for (cache_index, tile_descriptor) in self.manager.texture.lru.iter().enumerate() {
            let tile_address = self.manager.texture.cache.get(&tile_descriptor).unwrap();
            let tile_rect =
                RectI::new(tile_address.0, Vector2I::splat(1)).to_f32()
                                                              .scale_xy(cache_texture_scale);
            metadata[metadata_stride * 0 + cache_index * 4 + 0] = tile_descriptor.x as f32;
            metadata[metadata_stride * 0 + cache_index * 4 + 1] = tile_descriptor.y as f32;
            metadata[metadata_stride * 1 + cache_index * 4 + 0] = tile_rect.origin().x();
            metadata[metadata_stride * 1 + cache_index * 4 + 1] = tile_rect.origin().y();
            metadata[metadata_stride * 1 + cache_index * 4 + 2] = tile_rect.max_x();
            metadata[metadata_stride * 1 + cache_index * 4 + 3] = tile_rect.max_y();
        }

        unsafe {
            let metadata_texels: &[u8] = slice::from_raw_parts(metadata.as_ptr() as *const u8,
                                                            metadata.len() * 4 * 4);
            device.upload_to_texture(&self.metadata_texture,
                                     metadata_texture_size,
                                     metadata_texels);
        }

        let quad_rect =
            RectI::new(Vector2I::default(), self.manager.texture.virtual_texture_size).to_f32();
        let quad_tex_scale = quad_rect.size().scale(1.0 / self.manager.texture.tile_size as f32);
        println!("quad_rect={:?} quad_tex_scale={:?}", quad_rect, quad_tex_scale);

        device.begin_commands();
        device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
            target: &RenderTarget::Default,
            program: &self.render_vertex_array.render_program.program,
            vertex_array: &self.render_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            uniforms: &[
                (&self.render_vertex_array.render_program.quad_rect_uniform,
                 UniformData::Vec4(quad_rect.0)),
                (&self.render_vertex_array.render_program.quad_tex_scale_uniform,
                 UniformData::Vec2(quad_tex_scale.0)),
                (&self.render_vertex_array.render_program.framebuffer_size_uniform,
                 UniformData::Vec2(self.manager.viewport_size().to_f32().0)),
                (&self.render_vertex_array.render_program.transform_uniform,
                 UniformData::Mat2(self.manager.transform.matrix.0)),
                (&self.render_vertex_array.render_program.translation_uniform,
                 UniformData::Vec2(self.manager.transform.vector.0)),
                (&self.render_vertex_array.render_program.metadata_uniform,
                 UniformData::TextureUnit(0)),
                (&self.render_vertex_array.render_program.tile_cache_uniform,
                 UniformData::TextureUnit(1)),
                (&self.render_vertex_array.render_program.cache_size_uniform,
                 UniformData::Int(cache_size)),
            ],
            textures: &[&self.metadata_texture, &self.cache_texture],
            viewport: RectI::new(Vector2I::splat(0), self.manager.viewport_size()),
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
}

struct RenderAdvancedVertexArray<D> where D: Device {
    render_program: RenderAdvancedProgram<D>,
    vertex_array: D::VertexArray,
    #[allow(dead_code)]
    quad_vertex_positions_buffer: D::Buffer,
    #[allow(dead_code)]
    quad_vertex_indices_buffer: D::Buffer,
}

impl<D> RenderAdvancedVertexArray<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> RenderAdvancedVertexArray<D> {
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

struct RenderAdvancedProgram<D> where D: Device {
    program: D::Program,
    position_attribute: D::VertexAttr,
    quad_rect_uniform: D::Uniform,
    quad_tex_scale_uniform: D::Uniform,
    framebuffer_size_uniform: D::Uniform,
    transform_uniform: D::Uniform,
    translation_uniform: D::Uniform,
    metadata_uniform: D::Uniform,
    tile_cache_uniform: D::Uniform,
    cache_size_uniform: D::Uniform,
}

impl<D> RenderAdvancedProgram<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> RenderAdvancedProgram<D> {
        let program = device.create_program(resources, "render_advanced");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let quad_rect_uniform = device.get_uniform(&program, "QuadRect");
        let quad_tex_scale_uniform = device.get_uniform(&program, "QuadTexScale");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let metadata_uniform = device.get_uniform(&program, "Metadata");
        let tile_cache_uniform = device.get_uniform(&program, "TileCache");
        let cache_size_uniform = device.get_uniform(&program, "CacheSize");
        RenderAdvancedProgram {
            program,
            position_attribute,
            quad_rect_uniform,
            quad_tex_scale_uniform,
            framebuffer_size_uniform,
            transform_uniform,
            translation_uniform,
            metadata_uniform,
            tile_cache_uniform,
            cache_size_uniform,
        }
    }
}
