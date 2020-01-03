// virtex/src/tests.rs

//! Unit tests.

use crate::texture::{TileAddress, TileDescriptor, TileHashTable};

use env_logger;
use quickcheck::{self, Arbitrary, Gen};

impl Arbitrary for TileDescriptor {
    fn arbitrary<G>(generator: &mut G) -> TileDescriptor where G: Gen {
        TileDescriptor(u32::arbitrary(generator))
    }
    fn shrink(&self) -> Box<dyn Iterator<Item = TileDescriptor>> {
        Box::new((self.0).shrink().map(TileDescriptor))
    }
}

impl Arbitrary for TileAddress {
    fn arbitrary<G>(generator: &mut G) -> TileAddress where G: Gen {
        TileAddress(u32::arbitrary(generator))
    }
    fn shrink(&self) -> Box<dyn Iterator<Item = TileAddress>> {
        Box::new((self.0).shrink().map(TileAddress))
    }
}

fn init() {
    drop(env_logger::builder().is_test(true).try_init());
}

#[test]
fn test_tile_hash() {
    init();

    quickcheck::quickcheck(check_tile_hash as fn(Vec<(TileDescriptor, TileAddress)>,
                                                 (u32, u32),
                                                 u32));

    fn check_tile_hash(entries_to_insert: Vec<(TileDescriptor, TileAddress)>,
                       seeds: (u32, u32),
                       initial_bucket_size: u32) {
        if (initial_bucket_size as usize) < entries_to_insert.len() * 2 {
            return;
        }

        debug!("*** begin check_tile_hash({:?}, {:?}, {})",
               entries_to_insert,
               seeds,
               initial_bucket_size);

        let mut tile_hash_table = TileHashTable::with_seeds([seeds.0, seeds.1],
                                                            initial_bucket_size);
        for &(tile_descriptor, tile_address) in &entries_to_insert {
            tile_hash_table.insert(tile_descriptor, tile_address);
        }
        for &(tile_descriptor, _) in &entries_to_insert {
            debug!("checking for {:?} presence", tile_descriptor);
            assert!(tile_hash_table.get(tile_descriptor).is_some());
        }
    }
}
