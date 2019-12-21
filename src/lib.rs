// virtex/src/lib.rs

use pathfinder_geometry::vector::Vector2I;
use rand::{self, Rng};
use std::collections::VecDeque;
use std::collections::hash_map::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::mem;

pub mod manager2d;
pub mod renderer_advanced;
pub mod renderer_simple;

pub const TILE_HASH_TABLE_BUCKET_SIZE: usize = 1024;

// 0123456789abcdef0123456789abcdef
// yyyyyyyyyyyyyxxxxxxxxxxxxxLlllll
// \_____ _____/\_____ _____/|\_ _/
//       V            V      |  V
//   Y position   X position | LOD
//                           V
//                      LOD sign bit
#[derive(Clone, Copy, PartialEq, PartialOrd, Eq, Ord, Hash)]
pub struct TileDescriptor(pub u32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileAddress(pub u32);

#[derive(Clone, Copy, Debug)]
pub struct TileCacheEntry {
    pub address: TileAddress,
    pub rasterized_descriptor: Option<TileDescriptor>,
    pub pending_descriptor: Option<TileDescriptor>,
}

pub struct VirtualTexture {
    pub(crate) cache: TileHashTable,
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
            cache: TileHashTable::new(),
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

    pub fn request_tile(&mut self, tile_descriptor: TileDescriptor) -> RequestResult {
        // If already rasterized, just return it.
        if let Some(tile_address) = self.cache.get(tile_descriptor) {
            let lru_index = self.lru.iter().enumerate().find(|(_, current_address)| {
                **current_address == tile_address
            }).expect("Where's the address in the LRU list?").0;
            self.lru.remove(lru_index);
            self.lru.push_front(tile_address);

            let tile = &self.tiles[tile_address.0 as usize];
            if tile.rasterized_descriptor == Some(tile_descriptor) {
                return RequestResult::CacheHit(tile_address);
            }
            debug_assert_eq!(tile.pending_descriptor, Some(tile_descriptor));
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

        self.tiles[tile_address.0 as usize].pending_descriptor = Some(tile_descriptor);
        self.cache.insert(tile_descriptor, tile_address);
        self.lru.push_front(tile_address);
        RequestResult::CacheMiss(tile_address)
    }

    pub fn mark_as_rasterized(&mut self,
                              tile_address: TileAddress,
                              tile_descriptor: &TileDescriptor) {
        let mut tile = &mut self.tiles[tile_address.0 as usize];
        debug_assert_eq!(tile.pending_descriptor, Some(*tile_descriptor));
        if let Some(evicted_descriptor) = tile.rasterized_descriptor.take() {
            let old_address = self.cache.remove(evicted_descriptor);
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

    #[inline]
    pub(crate) fn table_size(&self) -> usize {
        self.cache.subtables[0].buckets.len()
    }
}

pub(crate) struct TileHashTable {
    pub(crate) subtables: [TileHashSubtable; 2],
}

pub(crate) struct TileHashSubtable {
    pub(crate) buckets: [TileHashEntry; TILE_HASH_TABLE_BUCKET_SIZE],
    pub(crate) seed: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct TileHashEntry {
    pub(crate) descriptor: TileDescriptor,
    pub(crate) address: TileAddress,
}

#[derive(Clone, Copy)]
enum TileHashInsertResult {
    Inserted,
    Replaced,
    Ejected(TileHashEntry),
}

impl TileHashTable {
    fn new() -> TileHashTable {
        let mut rng = rand::thread_rng();
        TileHashTable {
            subtables: [TileHashSubtable::new(rng.gen()), TileHashSubtable::new(rng.gen())],
        }
    }

    fn get(&self, descriptor: TileDescriptor) -> Option<TileAddress> {
        for subtable in &self.subtables {
            if let Some(address) = subtable.get(descriptor) {
                return Some(address);
            }
        }
        None
    }

    fn insert(&mut self, descriptor: TileDescriptor, address: TileAddress)
              -> TileHashInsertResult {
        let mut entry = TileHashEntry { descriptor, address };
        for _ in 0..50 {
            for subtable in &mut self.subtables {
                match subtable.insert(descriptor, address) {
                    TileHashInsertResult::Inserted => return TileHashInsertResult::Inserted,
                    TileHashInsertResult::Replaced => return TileHashInsertResult::Replaced,
                    TileHashInsertResult::Ejected(old_entry) => entry = old_entry,
                }
            }
        }

        // FIXME(pcwalton): Give up and rehash!
        unimplemented!("Cuckoo hash reseeding")
    }

    fn remove(&mut self, descriptor: TileDescriptor) -> Option<TileAddress> {
        for subtable in &mut self.subtables {
            if let Some(old_address) = subtable.remove(descriptor) {
                return Some(old_address);
            }
        }
        None
    }
}

impl TileHashSubtable {
    fn new(seed: u32) -> TileHashSubtable {
        TileHashSubtable {
            buckets: [TileHashEntry::default(); TILE_HASH_TABLE_BUCKET_SIZE],
            seed,
        }
    }

    fn get(&self, descriptor: TileDescriptor) -> Option<TileAddress> {
        let bucket_index = descriptor.hash(self.seed) as usize % TILE_HASH_TABLE_BUCKET_SIZE;
        let bucket = &self.buckets[bucket_index];
        if !bucket.is_empty() && bucket.descriptor == descriptor {
            Some(bucket.address)
        } else {
            None
        }
    }

    fn insert(&mut self, descriptor: TileDescriptor, address: TileAddress)
              -> TileHashInsertResult {
        let bucket_index = descriptor.hash(self.seed) as usize % TILE_HASH_TABLE_BUCKET_SIZE;
        let mut bucket = &mut self.buckets[bucket_index];
        if bucket.is_empty() {
            *bucket = TileHashEntry { descriptor, address };
            TileHashInsertResult::Inserted
        } else if bucket.descriptor == descriptor {
            bucket.address = address;
            TileHashInsertResult::Replaced
        } else {
            let new_entry = TileHashEntry { descriptor, address };
            TileHashInsertResult::Ejected(mem::replace(bucket, new_entry))
        }
    }

    fn remove(&mut self, descriptor: TileDescriptor) -> Option<TileAddress> {
        let bucket_index = descriptor.hash(self.seed) as usize % TILE_HASH_TABLE_BUCKET_SIZE;
        let mut bucket = &mut self.buckets[bucket_index];
        if !bucket.is_empty() && bucket.descriptor == descriptor {
            let old_address = bucket.address;
            *bucket = TileHashEntry::default();
            Some(old_address)
        } else {
            None
        }
    }
}

impl Default for TileHashEntry {
    fn default() -> TileHashEntry {
        TileHashEntry { descriptor: TileDescriptor(!0), address: TileAddress::default() }
    }
}

impl TileHashEntry {
    #[inline]
    fn is_empty(&self) -> bool {
        self.address.is_empty()
    }
}

impl TileDescriptor {
    #[inline]
    pub fn new(tile_position: Vector2I, lod: i8) -> TileDescriptor {
        debug_assert!(tile_position.x() >= 0);
        debug_assert!(tile_position.y() >= 0);
        debug_assert!(tile_position.x() < 1 << 13);
        debug_assert!(tile_position.y() < 1 << 13);
        debug_assert!(lod >= -32 && lod < 32);
        TileDescriptor(((tile_position.y() as u32) << 19) |
                       ((tile_position.x() as u32) << 6) |
                       ((lod as u32) & 0x3f))
    }

    #[inline]
    pub fn tile_position(self) -> Vector2I {
        Vector2I::new(((self.0 >> 6) & ((1 << 13) - 1)) as i32, (self.0 >> 19) as i32)
    }

    #[inline]
    pub fn lod(self) -> i8 {
        // Sign-extend
        ((self.0 << 2) as i8) >> 2
    }

    #[inline]
    pub fn hash(self, seed: u32) -> u32 {
        let mut h = self.0;
        h ^= h >> 16;
        h = h.wrapping_mul(0x85ebca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2ae35);
        h ^= h >> 16;
        h ^ seed
    }
}

impl Debug for TileDescriptor {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "TileDescriptor({:?} @ {})", self.tile_position(), self.lod())
    }
}

impl Default for TileAddress {
    #[inline]
    fn default() -> TileAddress {
        TileAddress(!0)
    }
}

impl TileAddress {
    #[inline]
    fn is_empty(self) -> bool {
        self == TileAddress::default()
    }
}
