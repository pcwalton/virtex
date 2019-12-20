// virtex/src/lib.rs

use pathfinder_geometry::vector::Vector2I;
use std::collections::VecDeque;
use std::collections::hash_map::HashMap;

pub mod manager2d;
pub mod renderer_advanced;
pub mod renderer_simple;

#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash, Debug)]
pub struct TileDescriptor {
    pub x: i32,
    pub y: i32,
    pub lod: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileAddress(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct TileCacheEntry {
    pub address: TileAddress,
    pub rasterized_descriptor: Option<TileDescriptor>,
    pub pending_descriptor: Option<TileDescriptor>,
}

pub struct VirtualTexture {
    cache: HashMap<TileDescriptor, TileAddress>,
    lru: VecDeque<TileAddress>,
    tiles: Vec<TileCacheEntry>,
    next_free_tile: TileAddress,
    #[allow(dead_code)]
    virtual_texture_size: Vector2I,
    cache_texture_size: Vector2I,
    tile_size: u32,
}

pub enum RequestResult {
    CacheFull,
    CacheHit(TileAddress),
    CachePending(TileAddress),
    CacheMiss(TileAddress),
}

impl VirtualTexture {
    pub fn new(virtual_texture_size: Vector2I, cache_texture_size: Vector2I, tile_size: u32)
               -> VirtualTexture {
        let mut this = VirtualTexture {
            cache: HashMap::new(),
            lru: VecDeque::new(),
            tiles: vec![],
            next_free_tile: TileAddress(0),
            virtual_texture_size,
            cache_texture_size,
            tile_size,
        };

        for address in 0..this.cache_size() {
            this.tiles.push(TileCacheEntry {
                address: TileAddress(address),
                rasterized_descriptor: None,
                pending_descriptor: None,
            });
        }

        this
    }

    pub fn request_tile(&mut self, tile_descriptor: &TileDescriptor) -> RequestResult {
        // If already rasterized, just return it.
        if let Some(&tile_address) = self.cache.get(tile_descriptor) {
            let lru_index = self.lru.iter().enumerate().find(|(_, current_address)| {
                **current_address == tile_address
            }).expect("Where's the address in the LRU list?").0;
            self.lru.remove(lru_index);
            self.lru.push_front(tile_address);

            let tile = &self.tiles[tile_address.0 as usize];
            if tile.rasterized_descriptor == Some(*tile_descriptor) {
                return RequestResult::CacheHit(tile_address);
            }
            debug_assert_eq!(tile.pending_descriptor, Some(*tile_descriptor));
            return RequestResult::CachePending(tile_address);
        }

        let mut tile_address = self.next_free_tile;
        if tile_address.0 < self.cache_size() {
            self.next_free_tile.0 += 1;
        } else {
            match self.lru.pop_back() {
                None => return RequestResult::CacheFull,
                Some(address_to_evict) => tile_address = address_to_evict,
            }
        }

        self.tiles[tile_address.0 as usize].pending_descriptor = Some(*tile_descriptor);
        self.cache.insert(*tile_descriptor, tile_address);
        self.lru.push_front(tile_address);
        RequestResult::CacheMiss(tile_address)
    }

    pub fn mark_as_rasterized(&mut self,
                              tile_address: TileAddress,
                              tile_descriptor: &TileDescriptor) {
        let mut tile = &mut self.tiles[tile_address.0 as usize];
        debug_assert_eq!(tile.pending_descriptor, Some(*tile_descriptor));
        if let Some(evicted_descriptor) = tile.rasterized_descriptor.take() {
            let old_address = self.cache.remove(&evicted_descriptor);
            debug_assert_eq!(old_address, Some(tile_address));
        }
        tile.rasterized_descriptor = tile.pending_descriptor.take();
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
    pub fn cache_size(&self) -> u32 {
        self.tile_texture_tiles_across() * self.tile_texture_tiles_down()
    }

    #[inline]
    fn tile_texture_tiles_across(&self) -> u32 {
        self.cache_texture_size.x() as u32 / self.tile_backing_size()
    }

    #[inline]
    fn tile_texture_tiles_down(&self) -> u32 {
        self.cache_texture_size.y() as u32 / self.tile_backing_size()
    }

    #[inline]
    pub fn tiles(&self) -> &[TileCacheEntry] {
        &self.tiles[..]
    }

    #[inline]
    pub fn address_to_tile_coords(&self, address: TileAddress) -> Vector2I {
        let tiles_across = self.tile_texture_tiles_across();
        Vector2I::new((address.0 % tiles_across) as i32, (address.0 / tiles_across) as i32)
    }
}
