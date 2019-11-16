// virtex/src/manager2d.rs

use crate::{RequestResult, TileCacheEntry, TileDescriptor, VirtualTexture};

use arrayvec::ArrayVec;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{Vector2F, Vector2I};

pub struct VirtualTextureManager2D {
    pub texture: VirtualTexture,
    pub transform: Transform2F,
    viewport_size: Vector2I,
}

impl VirtualTextureManager2D {
    #[inline]
    pub fn new(texture: VirtualTexture, viewport_size: Vector2I) -> VirtualTextureManager2D {
        VirtualTextureManager2D {
            texture,
            viewport_size,
            transform: Transform2F::default(),
        }
    }

    #[inline]
    pub fn current_scale(&self) -> f32 {
        f32::max(self.transform.m11(), self.transform.m22())
    }

    pub fn current_lods(&self) -> ArrayVec<[i32; 2]> {
        let scale = self.current_scale();
        let lower_lod = 31 - ((scale.floor() as u32).leading_zeros() as i32);

        let mut lods = ArrayVec::new();
        lods.push(lower_lod);
        if (1 << lower_lod) as f32 != scale {
            lods.push(lower_lod + 1);
        }

        lods
    }

    pub fn request_needed_tiles(&mut self, needed_tiles: &mut Vec<TileCacheEntry>) {
        let lods = self.current_lods();
        println!("lods={:?}", lods);
        for lod in lods {
            self.request_needed_tiles_for_lod(needed_tiles, lod);
        }
    }

    #[inline]
    pub fn viewport_size(&self) -> Vector2I {
        self.viewport_size
    }

    fn request_needed_tiles_for_lod(&mut self, needed_tiles: &mut Vec<TileCacheEntry>, lod: i32) {
        let viewport_rect = RectF::new(Vector2F::default(), self.viewport_size.to_f32());
        let transformed_viewport_rect = self.transform.inverse() * viewport_rect;
        let tile_size_inv = ((1 << lod) as f32) / self.texture.tile_size as f32;
        let tile_space_rect = transformed_viewport_rect.scale(tile_size_inv).round_out().to_i32();
        println!("tile space rect={:?}", tile_space_rect);
        for y in tile_space_rect.min_y()..tile_space_rect.max_y() {
            for x in tile_space_rect.min_x()..tile_space_rect.max_x() {
                let descriptor = TileDescriptor { x, y, lod };
                if let RequestResult::CacheMiss(address) = self.texture.request_tile(&descriptor) {
                    needed_tiles.push(TileCacheEntry { descriptor, address });
                }
            }
        }
    }
}
