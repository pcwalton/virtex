// virtex/src/renderer_advanced.rs

use crate::manager::{TileRequest, VirtualTextureManager};
use crate::texture::{RequestResult, TileDescriptor};

use pathfinder_geometry::rect::{RectF, RectI};
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gpu::{Device, TextureData, TextureDataRef, TextureFormat, UniformData};
use pathfinder_simd::default::F32x2;
use std::i8;

pub struct AdvancedRenderer<D> where D: Device {
    manager: VirtualTextureManager,
    cache_texture: D::Texture,
    metadata_texture: D::Texture,
    derivatives_viewport_scale_factor: i32,
    min_lod: i8,
    max_lod: i8,
}

impl<D> AdvancedRenderer<D> where D: Device {
    pub fn new(device: &D, manager: VirtualTextureManager, derivatives_viewport_scale_factor: i32)
               -> AdvancedRenderer<D> {
        let cache_texture = device.create_texture(TextureFormat::RGBA8,
                                                  manager.texture.cache_texture_size());

        let metadata_texture_size = Vector2I::new(manager.texture.bucket_size() as i32, 4);
        let metadata_texture = device.create_texture(TextureFormat::RGBA32F,
                                                     metadata_texture_size);

        AdvancedRenderer {
            manager,
            cache_texture,
            metadata_texture,
            derivatives_viewport_scale_factor,
            min_lod: i8::MAX,
            max_lod: i8::MIN,
         }
    }

    #[inline]
    pub fn manager(&self) -> &VirtualTextureManager {
        &self.manager
    }

    #[inline]
    pub fn manager_mut(&mut self) -> &mut VirtualTextureManager {
        &mut self.manager
    }

    #[inline]
    pub fn cache_texture(&self) -> &D::Texture {
        &self.cache_texture
    }

    pub fn push_prepare_uniforms<'a, 'b>(&self,
                                         prepare_uniforms: &'a PrepareAdvancedUniforms<D>,
                                         uniforms: &'b mut Vec<(&'a D::Uniform, UniformData)>) {
        let viewport_scale_factor = 1.0 / self.derivatives_viewport_scale_factor as f32;
        uniforms.push((&prepare_uniforms.tile_size_uniform,
                       UniformData::Vec2(F32x2::splat(self.manager.texture.tile_size() as f32))));
        uniforms.push((&prepare_uniforms.viewport_scale_factor_uniform,
                       UniformData::Vec2(F32x2::splat(viewport_scale_factor))));
    }

    pub fn request_needed_tiles(&mut self,
                                derivatives_texture_data: &TextureData,
                                needed_tiles: &mut Vec<TileRequest>) {
        let texture_data = match *derivatives_texture_data {
            TextureData::F32(ref data) => data,
            _ => panic!("Unexpected texture data type!"),
        };

        for pixel in texture_data.chunks(4) {
            if pixel[3] == 0.0 {
                continue;
            }

            let tile_origin = Vector2I::new(pixel[0] as i32, pixel[1] as i32);
            if tile_origin.x() < 0 || tile_origin.y() < 0 {
                continue;
            }

            let descriptor = TileDescriptor::new(tile_origin, pixel[2] as i8);
            if let RequestResult::CacheMiss(address) = self.manager
                                                           .texture
                                                           .request_tile(descriptor) {
                debug!("cache miss: {:?}", descriptor);
                needed_tiles.push(TileRequest { descriptor, address });
            }
        }
    }

    pub fn update_metadata(&mut self, device: &D) {
        // Pack and upload new metadata.

        // Resize the metadata texture if necessary.
        let bucket_size = self.manager.texture.bucket_size();
        let metadata_texture_size = Vector2I::new(bucket_size as i32, 4);
        if device.texture_size(&self.metadata_texture) != metadata_texture_size {
            self.metadata_texture = device.create_texture(TextureFormat::RGBA32F,
                                                          metadata_texture_size);
        }

        // Allocate new data for the metadata texture storage.
        let metadata_stride = metadata_texture_size.x() as usize * 4;
        let mut metadata = vec![0.0; metadata_stride * metadata_texture_size.y() as usize];

        let cache_texture_size = self.manager.texture.cache_texture_size().to_f32();
        let cache_texture_scale = Vector2F::new(1.0 / cache_texture_size.x(),
                                                1.0 / cache_texture_size.y());

        let tile_size = self.manager.texture.tile_size() as f32;
        let tile_backing_size = self.manager.texture.tile_backing_size() as f32;
        let tiles = self.manager.texture.tiles();

        self.min_lod = i8::MAX;
        self.max_lod = i8::MIN;

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

                let tile_rect =
                    RectF::new(tile_origin + Vector2F::splat(1.0),
                               Vector2F::splat(tile_size)).scale_xy(cache_texture_scale);

                let tile_position = tile_descriptor.tile_position();

                let tile_lod = tile_descriptor.lod();
                self.min_lod = i8::min(self.min_lod, tile_lod);
                self.max_lod = i8::max(self.max_lod, tile_lod);

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
    }

    pub fn push_render_uniforms<'a, 'b, 'c>(&'a self,
                                            render_uniforms: &'a RenderAdvancedUniforms<D>,
                                            uniforms: &'b mut Vec<(&'a D::Uniform, UniformData)>,
                                            textures: &'c mut Vec<&'a D::Texture>) {
        let tile_size = Vector2F::splat(self.manager.texture.tile_size() as f32);
        trace!("lod range=[{}, {}] tile_size={:?}", self.min_lod, self.max_lod, tile_size);

        uniforms.push((&render_uniforms.metadata_uniform,
                       UniformData::TextureUnit(textures.len() as u32)));
        textures.push(&self.metadata_texture);
        uniforms.push((&render_uniforms.tile_cache_uniform,
                       UniformData::TextureUnit(textures.len() as u32)));
        textures.push(&self.cache_texture);
        uniforms.push((&render_uniforms.cache_seed_a_uniform,
                       UniformData::Int(self.manager.texture.cache.subtables[0].seed as i32)));
        uniforms.push((&render_uniforms.cache_seed_b_uniform,
                       UniformData::Int(self.manager.texture.cache.subtables[1].seed as i32)));
        uniforms.push((&render_uniforms.cache_size_uniform,
                       UniformData::Int(self.manager.texture.bucket_size() as i32)));
        uniforms.push((&render_uniforms.tile_size_uniform,
                       UniformData::Vec2(F32x2::splat(self.manager.texture.tile_size() as f32))));
        uniforms.push((&render_uniforms.lod_range_uniform,
                       UniformData::Vec2(F32x2::new(self.min_lod as f32, self.max_lod as f32))));
    }

    pub fn derivatives_viewport(&self) -> RectI {
        let viewport_size = self.manager.viewport_size();
        let derivatives_viewport_size =
            Vector2I::new(viewport_size.x() / self.derivatives_viewport_scale_factor,
                          viewport_size.y() / self.derivatives_viewport_scale_factor);
        RectI::new(Vector2I::default(), derivatives_viewport_size)
    }
}

pub struct PrepareAdvancedUniforms<D> where D: Device {
    tile_size_uniform: D::Uniform,
    viewport_scale_factor_uniform: D::Uniform,
}

impl<D> PrepareAdvancedUniforms<D> where D: Device {
    pub fn new(device: &D, program: &D::Program) -> PrepareAdvancedUniforms<D> {
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let viewport_scale_factor_uniform = device.get_uniform(&program, "ViewportScaleFactor");
        PrepareAdvancedUniforms { tile_size_uniform, viewport_scale_factor_uniform }
    }
}

pub struct RenderAdvancedUniforms<D> where D: Device {
    metadata_uniform: D::Uniform,
    tile_cache_uniform: D::Uniform,
    cache_seed_a_uniform: D::Uniform,
    cache_seed_b_uniform: D::Uniform,
    cache_size_uniform: D::Uniform,
    tile_size_uniform: D::Uniform,
    lod_range_uniform: D::Uniform,
}

impl<D> RenderAdvancedUniforms<D> where D: Device {
    pub fn new(device: &D, program: &D::Program) -> RenderAdvancedUniforms<D> {
        let metadata_uniform = device.get_uniform(&program, "Metadata");
        let tile_cache_uniform = device.get_uniform(&program, "TileCache");
        let cache_seed_a_uniform = device.get_uniform(&program, "CacheSeedA");
        let cache_seed_b_uniform = device.get_uniform(&program, "CacheSeedB");
        let cache_size_uniform = device.get_uniform(&program, "CacheSize");
        let tile_size_uniform = device.get_uniform(&program, "TileSize");
        let lod_range_uniform = device.get_uniform(&program, "LODRange");
        RenderAdvancedUniforms {
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
