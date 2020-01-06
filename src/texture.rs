// virtex/src/texture.rs

//! A sparse virtual texture.

use pathfinder_content::color::ColorF;
use pathfinder_geometry::vector::Vector2I;
use rand::{self, Rng};
use std::collections::VecDeque;
use std::fmt::{self, Debug, Formatter};
use std::mem;

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
    pub descriptor: Option<TileDescriptor>,
    pub status: TileCacheStatus,
}

pub struct VirtualTexture {
    pub(crate) cache: TileHashTable,
    lru: VecDeque<TileAddress>,
    tiles: Vec<TileCacheEntry>,
    next_free_tile: TileAddress,
    cache_texture_size: Vector2I,
    pub(crate) background_color: ColorF,
    tile_size: u32,
}

pub enum RequestResult {
    CacheFull,
    CacheHit(TileAddress),
    CachePending(TileAddress),
    CacheMiss(TileAddress),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileCacheStatus {
    Empty,
    Pending,
    Rasterized,
}

impl VirtualTexture {
    pub fn new(cache_texture_size: Vector2I,
               background_color: ColorF,
               tile_size: u32,
               initial_bucket_size: u32)
               -> VirtualTexture {
        let mut this = VirtualTexture {
            cache: TileHashTable::new(initial_bucket_size),
            lru: VecDeque::new(),
            tiles: vec![],
            next_free_tile: TileAddress(0),
            cache_texture_size,
            background_color,
            tile_size,
        };

        for address in 0..this.cache_size() {
            this.tiles.push(TileCacheEntry {
                address: TileAddress(address),
                descriptor: None,
                status: TileCacheStatus::Empty,
            });
        }

        this
    }

    pub fn request_tile(&mut self, tile_descriptor: TileDescriptor) -> RequestResult {
        // If already rasterized, just return it.
        if let Some(tile_address) = self.cache.get(tile_descriptor) {
            let lru_index = match self.lru.iter().enumerate().find(|(_, current_address)| {
                **current_address == tile_address
            }) {
                Some((lru_index, _)) => lru_index,
                None => {
                    panic!("Failed to find {:?}/{:?} in the LRU list!",
                           tile_descriptor,
                           tile_address)
                }
            };

            let removed_address = self.lru.remove(lru_index);
            debug_assert_eq!(removed_address, Some(tile_address));
            self.lru.push_front(tile_address);

            let tile = &self.tiles[tile_address.0 as usize];
            debug_assert_eq!(tile.descriptor, Some(tile_descriptor));
            match tile.status {
                TileCacheStatus::Empty => unreachable!(),
                TileCacheStatus::Pending => return RequestResult::CachePending(tile_address),
                TileCacheStatus::Rasterized => return RequestResult::CacheHit(tile_address),
            }
        }

        let tile_address = match self.get_next_free_tile() {
            None => return RequestResult::CacheFull,
            Some(tile_address) => tile_address,
        };

        {
            let tile = &mut self.tiles[tile_address.0 as usize];
            debug_assert!(tile.descriptor.is_none());
            debug_assert_eq!(tile.status, TileCacheStatus::Empty);
            tile.descriptor = Some(tile_descriptor);
            tile.status = TileCacheStatus::Pending;
        }

        self.cache.insert(tile_descriptor, tile_address);
        self.lru.push_front(tile_address);
        RequestResult::CacheMiss(tile_address)
    }

    fn get_next_free_tile(&mut self) -> Option<TileAddress> {
        let tile_address = self.next_free_tile;
        let cache_size = self.cache_size();
        if tile_address.0 < cache_size {
            self.next_free_tile.0 += 1;
            return Some(tile_address);
        }

        // This vector will only be used if an exceptionally large number of tiles are pending
        // rasterization.
        let mut pending_tile_addresses = vec![];

        let mut tile_address = None;
        loop {
            let candidate_address = match self.lru.pop_back() {
                None => break,
                Some(address_to_evict) => address_to_evict,
            };

            match self.tiles[candidate_address.0 as usize].status {
                TileCacheStatus::Empty | TileCacheStatus::Rasterized => {
                    tile_address = Some(candidate_address);
                    break;
                }
                TileCacheStatus::Pending => {}
            }

            pending_tile_addresses.push(candidate_address);
        }

        for pending_tile_address in pending_tile_addresses.into_iter() {
            self.lru.push_back(pending_tile_address);
        }

        let tile_address = match tile_address {
            None => return None,
            Some(tile_address) => tile_address,
        };

        let tile = &mut self.tiles[tile_address.0 as usize];
        match tile.status {
            TileCacheStatus::Empty => {}
            TileCacheStatus::Pending => unreachable!(),
            TileCacheStatus::Rasterized => {
                self.cache.remove(tile.descriptor.take().unwrap());
                tile.status = TileCacheStatus::Empty;
            }
        }

        Some(tile_address)
    }

    pub fn mark_as_rasterized(&mut self,
                              tile_address: TileAddress,
                              tile_descriptor: &TileDescriptor) {
        let mut tile = &mut self.tiles[tile_address.0 as usize];
        debug_assert_eq!(tile.descriptor, Some(*tile_descriptor));
        debug_assert_eq!(tile.status, TileCacheStatus::Pending);
        tile.status = TileCacheStatus::Rasterized;
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
    pub(crate) fn bucket_size(&self) -> usize {
        self.cache.subtables[0].buckets.len()
    }
}

pub(crate) struct TileHashTable {
    pub(crate) subtables: [TileHashSubtable; 2],
}

pub(crate) struct TileHashSubtable {
    pub(crate) buckets: Vec<Option<TileHashEntry>>,
    pub(crate) seed: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TileHashEntry {
    pub(crate) descriptor: TileDescriptor,
    pub(crate) address: TileAddress,
}

#[derive(Clone, Copy)]
pub(crate) enum TileHashInsertResult {
    Inserted,
    Replaced,
}

#[derive(Clone, Copy)]
enum TileHashSubinsertResult {
    Inserted,
    Replaced,
    Ejected(TileHashEntry),
}

impl TileHashTable {
    pub(crate) fn new(initial_bucket_size: u32) -> TileHashTable {
        let mut rng = rand::thread_rng();
        TileHashTable::with_seeds([rng.gen(), rng.gen()], initial_bucket_size)
    }

    pub(crate) fn with_seeds(seeds: [u32; 2], initial_bucket_size: u32) -> TileHashTable {
        TileHashTable {
            subtables: [
                TileHashSubtable::new(seeds[0], initial_bucket_size),
                TileHashSubtable::new(seeds[1], initial_bucket_size),
            ],
        }
    }

    pub(crate) fn get(&self, descriptor: TileDescriptor) -> Option<TileAddress> {
        for subtable in &self.subtables {
            if let Some(address) = subtable.get(descriptor) {
                return Some(address);
            }
        }
        None
    }

    pub(crate) fn insert(&mut self, descriptor: TileDescriptor, address: TileAddress)
                         -> TileHashInsertResult {
        debug!("insert({:?}, {:?})", descriptor, address);
        let bucket_size = self.subtables[0].buckets.len() as u32;
        let max_chain = 31 - bucket_size.leading_zeros();
        debug!("... max_chain={}", max_chain);

        let mut entry = TileHashEntry { descriptor, address };
        for _ in 0..max_chain {
            for subtable in &mut self.subtables {
                match subtable.insert(entry.descriptor, entry.address) {
                    TileHashSubinsertResult::Inserted => return TileHashInsertResult::Inserted,
                    TileHashSubinsertResult::Replaced => return TileHashInsertResult::Replaced,
                    TileHashSubinsertResult::Ejected(old_entry) => {
                        debug!("ejected! old_entry={:?}", old_entry);
                        entry = old_entry
                    }
                }
            }
        }

        // Give up and rehash.
        //
        // FIXME(pcwalton): If the load factor is less than 50%, don't increase the bucket size.
        self.rebuild(bucket_size * 2);
        self.insert(entry.descriptor, entry.address)
    }

    fn remove(&mut self, descriptor: TileDescriptor) -> Option<TileAddress> {
        for subtable in &mut self.subtables {
            if let Some(old_address) = subtable.remove(descriptor) {
                return Some(old_address);
            }
        }
        None
    }

    fn rebuild(&mut self, new_bucket_size: u32) {
        debug!("*** REBUILDING {} ***", new_bucket_size);
        let old_table = mem::replace(self, TileHashTable::new(new_bucket_size));
        for old_subtable in &old_table.subtables {
            for old_bucket in &old_subtable.buckets {
                if let Some(old_bucket) = old_bucket {
                    self.insert(old_bucket.descriptor, old_bucket.address);
                }
            }
        }
    }
}

impl TileHashSubtable {
    fn new(seed: u32, bucket_size: u32) -> TileHashSubtable {
        TileHashSubtable {
            buckets: vec![None; bucket_size as usize],
            seed,
        }
    }

    fn get(&self, descriptor: TileDescriptor) -> Option<TileAddress> {
        let bucket_index = descriptor.hash(self.seed) as usize % self.buckets.len();
        let bucket = &self.buckets[bucket_index];
        debug!("subtable get {:?} -> {:?} found? {:?}",
               descriptor,
               bucket_index,
               bucket.is_some());
        if let Some(ref bucket) = *bucket {
            if bucket.descriptor == descriptor {
                debug!("... matched!");
                return Some(bucket.address);
            }
        }
        None
    }

    fn insert(&mut self, descriptor: TileDescriptor, address: TileAddress)
              -> TileHashSubinsertResult {
        let bucket_index = descriptor.hash(self.seed) as usize % self.buckets.len();
        let bucket = &mut self.buckets[bucket_index];
        debug!("subtable insert {:?} -> {:?}", descriptor, bucket_index);
        match *bucket {
            None => {
                *bucket = Some(TileHashEntry { descriptor, address });
                TileHashSubinsertResult::Inserted
            }
            Some(ref mut bucket) if bucket.descriptor == descriptor => {
                bucket.address = address;
                TileHashSubinsertResult::Replaced
            }
            Some(ref mut bucket) => {
                let new_entry = TileHashEntry { descriptor, address };
                TileHashSubinsertResult::Ejected(mem::replace(bucket, new_entry))
            }
        }
    }

    fn remove(&mut self, descriptor: TileDescriptor) -> Option<TileAddress> {
        let bucket_index = descriptor.hash(self.seed) as usize % self.buckets.len();
        let bucket = &mut self.buckets[bucket_index];

        let found = match *bucket {
            None => false,
            Some(ref bucket) => bucket.descriptor == descriptor,
        };

        if found {
            Some(bucket.take().unwrap().address)
        } else {
            None
        }
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
