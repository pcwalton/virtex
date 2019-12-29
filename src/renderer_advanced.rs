// virtex/src/renderer_advanced.rs

use crate::manager2d::{TileRequest, VirtualTextureManager2D};
use crate::{RequestResult, TileCacheEntry, TileDescriptor};

use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gpu::resources::ResourceLoader;
use pathfinder_gpu::{BufferData, BufferTarget, BufferUploadMode, ClearOps, Device, Primitive};
use pathfinder_gpu::{RenderOptions, RenderState, RenderTarget, TextureData, TextureDataRef};
use pathfinder_gpu::{TextureFormat, UniformData, VertexAttrClass};
use pathfinder_gpu::{VertexAttrDescriptor, VertexAttrType};
use pathfinder_simd::default::F32x2;
use std::i8;

static QUAD_VERTEX_POSITIONS: [u8; 8] = [0, 0, 1, 0, 0, 1, 1, 1];
static QUAD_VERTEX_INDICES: [u32; 6] = [0, 1, 2, 1, 3, 2];

const DERIVATIVES_VIEWPORT_SCALE_FACTOR: i32 = 16;

pub struct AdvancedRenderer<D> where D: Device {
    manager: VirtualTextureManager2D,
    prepare_vertex_array: PrepareAdvancedVertexArray<D>,
    render_vertex_array: RenderAdvancedVertexArray<D>,
    cache_texture: D::Texture,
    metadata_texture: D::Texture,
    derivatives_framebuffer: D::Framebuffer,
}

impl<D> AdvancedRenderer<D> where D: Device {
    pub fn new(device: &D, manager: VirtualTextureManager2D, resource_loader: &dyn ResourceLoader)
               -> AdvancedRenderer<D> {
        let cache_texture = device.create_texture(TextureFormat::RGBA8,
                                                  manager.texture.cache_texture_size());
        let metadata_texture_size = Vector2I::new(manager.texture.table_size() as i32, 4);
        let metadata_texture = device.create_texture(TextureFormat::RGBA32F,
                                                     metadata_texture_size);

        let viewport_size = manager.viewport_size();
        let derivatives_texture_size =
            Vector2I::new(viewport_size.x() / DERIVATIVES_VIEWPORT_SCALE_FACTOR,
                          viewport_size.y() / DERIVATIVES_VIEWPORT_SCALE_FACTOR);
        let derivatives_texture = device.create_texture(TextureFormat::RGBA32F,
                                                        derivatives_texture_size);
        let derivatives_framebuffer = device.create_framebuffer(derivatives_texture);

        let prepare_vertex_array = PrepareAdvancedVertexArray::new(device, resource_loader);
        let render_vertex_array = RenderAdvancedVertexArray::new(device, resource_loader);

        AdvancedRenderer {
            manager,
            prepare_vertex_array,
            render_vertex_array,
            cache_texture,
            metadata_texture,
            derivatives_framebuffer,
        }
    }

    #[inline]
    pub fn manager(&self) -> &VirtualTextureManager2D {
        &self.manager
    }

    #[inline]
    pub fn manager_mut(&mut self) -> &mut VirtualTextureManager2D {
        &mut self.manager
    }

    #[inline]
    pub fn cache_texture(&self) -> &D::Texture {
        &self.cache_texture
    }

    pub fn prepare(&mut self, device: &D, needed_tiles: &mut Vec<TileRequest>) {
        let quad_rect =
            RectI::new(Vector2I::default(), self.manager.texture.virtual_texture_size).to_f32();
        let tile_size = Vector2F::splat(self.manager.texture.tile_size() as f32);

        let viewport_size = self.manager.viewport_size();
        let derivatives_viewport_size =
            Vector2I::new(viewport_size.x() / DERIVATIVES_VIEWPORT_SCALE_FACTOR,
                          viewport_size.y() / DERIVATIVES_VIEWPORT_SCALE_FACTOR);
        let derivatives_viewport = RectI::new(Vector2I::default(), derivatives_viewport_size);

        device.begin_commands();
        device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
            target: &RenderTarget::Framebuffer(&self.derivatives_framebuffer),
            program: &self.prepare_vertex_array.prepare_program.program,
            vertex_array: &self.prepare_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            uniforms: &[
                (&self.prepare_vertex_array.prepare_program.quad_rect_uniform,
                 UniformData::Vec4(quad_rect.0)),
                (&self.prepare_vertex_array.prepare_program.framebuffer_size_uniform,
                 UniformData::Vec2(viewport_size.to_f32().0)),
                (&self.prepare_vertex_array.prepare_program.transform_uniform,
                 UniformData::Mat2(self.manager.transform.matrix.0)),
                (&self.prepare_vertex_array.prepare_program.translation_uniform,
                 UniformData::Vec2(self.manager.transform.vector.0)),
                (&self.prepare_vertex_array.prepare_program.tile_size_uniform,
                 UniformData::Vec2(tile_size.0)),
                (&self.prepare_vertex_array.prepare_program.viewport_scale_factor_uniform,
                 UniformData::Vec2(F32x2::splat(1.0 / DERIVATIVES_VIEWPORT_SCALE_FACTOR as f32))),
            ],
            textures: &[&self.metadata_texture, &self.cache_texture],
            viewport: derivatives_viewport,
            options: RenderOptions {
                clear_ops: ClearOps {
                    color: Some(ColorF::new(0.0, 0.0, 0.0, 0.0)),
                    ..ClearOps::default()
                },
                ..RenderOptions::default()
            },
        });
        let texture_data =
            device.read_pixels(&RenderTarget::Framebuffer(&self.derivatives_framebuffer),
                               derivatives_viewport);
        device.end_commands();

        let texture_data = match texture_data {
            TextureData::F32(ref data) => data,
            _ => panic!("Unexpected texture data type!"),
        };

        for pixel in texture_data.chunks(4) {
            if pixel[3] == 0.0 {
                continue;
            }

            let descriptor = TileDescriptor::new(Vector2I::new(pixel[0] as i32, pixel[1] as i32),
                                                 pixel[2] as i8);

            if let RequestResult::CacheMiss(address) = self.manager
                                                           .texture
                                                           .request_tile(descriptor) {
                println!("cache miss: {:?}", descriptor);
                needed_tiles.push(TileRequest { descriptor, address });
            }
        }
    }

    pub fn render(&mut self, device: &D) {
        // Pack and upload new metadata.
        let table_size = self.manager.texture.table_size();
        let metadata_texture_size = Vector2I::new(table_size as i32, 4);
        let metadata_stride = metadata_texture_size.x() as usize * 4;
        let mut metadata = vec![0.0; metadata_stride * metadata_texture_size.y() as usize];

        let cache_texture_size = self.manager.texture.cache_texture_size().to_f32();
        let cache_texture_scale = Vector2F::new(1.0 / cache_texture_size.x(),
                                                1.0 / cache_texture_size.y());

        let tile_size = self.manager.texture.tile_size() as f32;
        let tile_backing_size = self.manager.texture.tile_backing_size() as f32;
        let tiles = self.manager.texture.tiles();

        let (mut min_lod, mut max_lod) = (i8::MAX, i8::MIN);

        for (subtable_index, subtable) in self.manager.texture.cache.subtables.iter().enumerate() {
            for (bucket_index, &bucket) in subtable.buckets.iter().enumerate() {
                if bucket.is_empty() {
                    continue;
                }

                let tile_address = bucket.address;
                let tile_descriptor = match &tiles[tile_address.0 as usize].rasterized_descriptor {
                    None => continue,
                    Some(tile_descriptor) => tile_descriptor,
                };

                let tile_origin = self.manager
                                      .texture
                                      .address_to_tile_coords(tile_address)
                                      .to_f32()
                                      .scale(tile_backing_size);

                let tile_rect = RectF::new(tile_origin + Vector2F::splat(1.0),
                                        Vector2F::splat(tile_size)).scale_xy(cache_texture_scale);

                let tile_position = tile_descriptor.tile_position();

                let tile_lod = tile_descriptor.lod();
                min_lod = i8::min(min_lod, tile_lod);
                max_lod = i8::max(max_lod, tile_lod);

                let metadata_start_index = metadata_stride * (subtable_index * 2 + 0) +
                    bucket_index * 4;
                let rect_start_index = metadata_stride * (subtable_index * 2 + 1) +
                    bucket_index * 4;

                metadata[metadata_start_index + 0] = tile_position.x() as f32;
                metadata[metadata_start_index + 1] = tile_position.y() as f32;
                metadata[metadata_start_index + 2] = tile_lod as f32;
                metadata[rect_start_index + 0] = tile_rect.origin().x();
                metadata[rect_start_index + 1] = tile_rect.origin().y();
                metadata[rect_start_index + 2] = tile_rect.max_x();
                metadata[rect_start_index + 3] = tile_rect.max_y();
            }
        }

        device.upload_to_texture(&self.metadata_texture,
                                 RectI::new(Vector2I::default(), metadata_texture_size),
                                 TextureDataRef::F32(&metadata));

        let quad_rect =
            RectI::new(Vector2I::default(), self.manager.texture.virtual_texture_size).to_f32();
        let tile_size = Vector2F::splat(self.manager.texture.tile_size() as f32);
        println!("lod range=[{}, {}] quad_rect={:?} tile_size={:?}",
                 min_lod, max_lod, quad_rect, tile_size);

        device.begin_commands();
        device.draw_elements(QUAD_VERTEX_INDICES.len() as u32, &RenderState {
            target: &RenderTarget::Default,
            program: &self.render_vertex_array.render_program.program,
            vertex_array: &self.render_vertex_array.vertex_array,
            primitive: Primitive::Triangles,
            uniforms: &[
                (&self.render_vertex_array.render_program.quad_rect_uniform,
                 UniformData::Vec4(quad_rect.0)),
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
                (&self.render_vertex_array.render_program.cache_seed_a_uniform,
                 UniformData::Int(self.manager.texture.cache.subtables[0].seed as i32)),
                (&self.render_vertex_array.render_program.cache_seed_b_uniform,
                 UniformData::Int(self.manager.texture.cache.subtables[1].seed as i32)),
                (&self.render_vertex_array.render_program.cache_size_uniform,
                 UniformData::Int(table_size as i32)),
                (&self.render_vertex_array.render_program.tile_size_uniform,
                 UniformData::Vec2(tile_size.0)),
                (&self.render_vertex_array.render_program.lod_range_uniform,
                 UniformData::Vec2(F32x2::new(min_lod as f32, max_lod as f32))),
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

struct PrepareAdvancedVertexArray<D> where D: Device {
    prepare_program: PrepareAdvancedProgram<D>,
    vertex_array: D::VertexArray,
    #[allow(dead_code)]
    quad_vertex_positions_buffer: D::Buffer,
    #[allow(dead_code)]
    quad_vertex_indices_buffer: D::Buffer,
}

impl<D> PrepareAdvancedVertexArray<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> PrepareAdvancedVertexArray<D> {
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

struct PrepareAdvancedProgram<D> where D: Device {
    program: D::Program,
    position_attribute: D::VertexAttr,
    quad_rect_uniform: D::Uniform,
    framebuffer_size_uniform: D::Uniform,
    transform_uniform: D::Uniform,
    translation_uniform: D::Uniform,
    tile_size_uniform: D::Uniform,
    viewport_scale_factor_uniform: D::Uniform,
}

impl<D> PrepareAdvancedProgram<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> PrepareAdvancedProgram<D> {
        let program = device.create_program_from_shader_names(resources,
                                                              "prepare_advanced",
                                                              "render_advanced",
                                                              "prepare_advanced");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let quad_rect_uniform = device.get_uniform(&program, "QuadRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let viewport_scale_factor_uniform = device.get_uniform(&program, "ViewportScaleFactor");
        PrepareAdvancedProgram {
            program,
            position_attribute,
            quad_rect_uniform,
            framebuffer_size_uniform,
            transform_uniform,
            translation_uniform,
            tile_size_uniform,
            viewport_scale_factor_uniform,
        }
    }
}

struct RenderAdvancedProgram<D> where D: Device {
    program: D::Program,
    position_attribute: D::VertexAttr,
    quad_rect_uniform: D::Uniform,
    framebuffer_size_uniform: D::Uniform,
    transform_uniform: D::Uniform,
    translation_uniform: D::Uniform,
    metadata_uniform: D::Uniform,
    tile_cache_uniform: D::Uniform,
    cache_seed_a_uniform: D::Uniform,
    cache_seed_b_uniform: D::Uniform,
    cache_size_uniform: D::Uniform,
    tile_size_uniform: D::Uniform,
    lod_range_uniform: D::Uniform,
}

impl<D> RenderAdvancedProgram<D> where D: Device {
    fn new(device: &D, resources: &dyn ResourceLoader) -> RenderAdvancedProgram<D> {
        let program = device.create_program(resources, "render_advanced");
        let position_attribute = device.get_vertex_attr(&program, "Position").unwrap();
        let quad_rect_uniform = device.get_uniform(&program, "QuadRect");
        let framebuffer_size_uniform = device.get_uniform(&program, "FramebufferSize");
        let transform_uniform = device.get_uniform(&program, "Transform");
        let translation_uniform = device.get_uniform(&program, "Translation");
        let metadata_uniform = device.get_uniform(&program, "Metadata");
        let tile_cache_uniform = device.get_uniform(&program, "TileCache");
        let cache_seed_a_uniform = device.get_uniform(&program, "CacheSeedA");
        let cache_seed_b_uniform = device.get_uniform(&program, "CacheSeedB");
        let cache_size_uniform = device.get_uniform(&program, "CacheSize");
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let lod_range_uniform = device.get_uniform(&program, "LODRange");
        RenderAdvancedProgram {
            program,
            position_attribute,
            quad_rect_uniform,
            framebuffer_size_uniform,
            transform_uniform,
            translation_uniform,
            metadata_uniform,
            tile_cache_uniform,
            cache_seed_a_uniform,
            cache_seed_b_uniform,
            cache_size_uniform,
            tile_size_uniform,
            lod_range_uniform,
        }
    }
}
