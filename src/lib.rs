// virtex/src/lib.rs

use pathfinder_geometry::vector::Vector2I;
use std::collections::VecDeque;
use std::collections::hash_map::HashMap;

pub mod manager2d;
pub mod renderer_simple;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, Debug)]
pub struct TileDescriptor {
    pub x: i32,
    pub y: i32,
    pub lod: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct TileAddress(pub Vector2I);

#[derive(Clone, Copy, Debug)]
pub struct TileCacheEntry {
    pub descriptor: TileDescriptor,
    pub address: TileAddress,
}

pub struct VirtualTexture {
    cache: HashMap<TileDescriptor, TileAddress>,
    lru: VecDeque<TileDescriptor>,
    free_tile_addresses: Vec<TileAddress>,
    #[allow(dead_code)]
    virtual_texture_size: Vector2I,
    cache_texture_size: Vector2I,
    tile_size: u32,
}

pub enum RequestResult {
    CacheFull,
    CacheHit(TileAddress),
    CacheMiss(TileAddress),
}

impl VirtualTexture {
    pub fn new(virtual_texture_size: Vector2I, cache_texture_size: Vector2I, tile_size: u32)
               -> VirtualTexture {
        let mut this = VirtualTexture {
            cache: HashMap::new(),
            lru: VecDeque::new(),
            free_tile_addresses: vec![],
            virtual_texture_size,
            cache_texture_size,
            tile_size,
        };

        let tiles_down = this.tile_texture_tiles_down() as i32;
        let tiles_across = this.tile_texture_tiles_across() as i32;
        for tile_y in 0..tiles_down {
            for tile_x in 0..tiles_across {
                this.free_tile_addresses.push(TileAddress(Vector2I::new(tile_x, tile_y)));
            }
        }

        this
    }

    pub fn request_tile(&mut self, tile_descriptor: &TileDescriptor) -> RequestResult {
        if let Some(&tile_address) = self.cache.get(tile_descriptor) {
            let lru_index = self.lru.iter().enumerate().find(|(_, current_descriptor)| {
                *current_descriptor == tile_descriptor
            }).expect("Where's the descriptor in the LRU list?").0;
            self.lru.remove(lru_index);
            self.lru.push_front(*tile_descriptor);
            return RequestResult::CacheHit(tile_address);
        }

        if self.free_tile_addresses.is_empty() {
            let descriptor_to_evict = match self.lru.pop_back() {
                None => return RequestResult::CacheFull,
                Some(descriptor_to_evict) => descriptor_to_evict,
            };
            let tile_address_to_evict =
                self.cache
                    .remove(&descriptor_to_evict)
                    .expect("Where's the descriptor in the cache?");
            self.free_tile_addresses.push(tile_address_to_evict);
        }

        let tile_address = match self.free_tile_addresses.pop() {
            None => return RequestResult::CacheFull,
            Some(tile_address) => tile_address,
        };
        self.cache.insert(*tile_descriptor, tile_address);
        self.lru.push_front(*tile_descriptor);
        RequestResult::CacheMiss(tile_address)
    }

    #[inline]
    pub fn tile_size(&self) -> u32 {
        self.tile_size
    }

    #[inline]
    pub fn tile_backing_size(&self) -> u32 {
        self.tile_size + 2
    }

    #[inline]
    pub fn cache_texture_size(&self) -> Vector2I {
        self.cache_texture_size
    }

    #[inline]
    fn tile_texture_tiles_across(&self) -> u32 {
        self.cache_texture_size.x() as u32 / self.tile_backing_size()
    }

    #[inline]
    fn tile_texture_tiles_down(&self) -> u32 {
        self.cache_texture_size.y() as u32 / self.tile_backing_size()
    }

    pub fn all_cached_tiles(&self) -> Vec<TileCacheEntry> {
        self.cache
            .iter()
            .map(|(&descriptor, &address)| TileCacheEntry { descriptor, address })
            .collect()
    }
}
