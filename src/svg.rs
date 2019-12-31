// virtex/src/svg.rs

use crate::manager::TileRequest;
use crate::renderer_advanced::AdvancedRenderer;

use crossbeam_channel::{Receiver, Sender};
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gpu::{Device, TextureDataRef};
use raqote::{DrawTarget, SolidSource, Transform};
use resvg::backend_raqote;
use resvg::usvg::{Options as UsvgOptions, Tree};
use resvg::{Options as ResvgOptions, ScreenSize};
use std::panic::{self, AssertUnwindSafe};
use std::thread::{self, JoinHandle};

pub struct SVGRasterizerProxy {
    main_to_rasterizer_sender: Sender<MainToRasterizerMsg>,
    rasterizer_to_main_receiver: Receiver<RasterizerToMainMsg>,
    #[allow(dead_code)]
    thread: JoinHandle<()>,
}

struct SVGRasterizerThread {
    rasterizer_to_main_sender: Sender<RasterizerToMainMsg>,
    main_to_rasterizer_receiver: Receiver<MainToRasterizerMsg>,
    svg_path: String,
    background_color: SolidSource,
    tile_size: u32,
}

impl SVGRasterizerProxy {
    pub fn new(svg_path: String, background_color: SolidSource, tile_size: u32)
               -> SVGRasterizerProxy {
        let (main_to_rasterizer_sender,
             main_to_rasterizer_receiver) = crossbeam_channel::unbounded();
        let (rasterizer_to_main_sender,
             rasterizer_to_main_receiver) = crossbeam_channel::unbounded();
        let thread = thread::spawn(move || {
            SVGRasterizerThread {
                rasterizer_to_main_sender,
                main_to_rasterizer_receiver,
                svg_path,
                background_color,
                tile_size,
            }.run()
        });
        SVGRasterizerProxy { main_to_rasterizer_sender, rasterizer_to_main_receiver, thread }
    }

    /// Waits for the SVG to load and returns its size.
    ///
    /// This must only be called once, immediately after loading the SVG.
    pub fn wait_for_svg_to_load(&mut self) -> Vector2I {
        match self.rasterizer_to_main_receiver.recv().unwrap() {
            RasterizerToMainMsg::SVGLoaded { size } => size,
            RasterizerToMainMsg::TileRasterized { .. } => {
                panic!("Called `wait_for_svg_to_load` at an unexpected time!")
            }
        }
    }

    pub fn rasterize_needed_tiles<D>(&mut self,
                                     device: &D,
                                     renderer: &mut AdvancedRenderer<D>,
                                     needed_tiles: &mut Vec<TileRequest>)
                                     where D: Device {
        if !needed_tiles.is_empty() {
            for tile_cache_entry in needed_tiles.drain(..) {
                let tile_origin = renderer.manager()
                                        .texture
                                        .address_to_tile_coords(tile_cache_entry.address);
                self.main_to_rasterizer_sender
                    .send(MainToRasterizerMsg { tile_request: tile_cache_entry, tile_origin })
                    .unwrap();
            }
        }

        let tile_backing_size = renderer.manager().texture.tile_backing_size() as i32;
        while let Ok(msg) = self.rasterizer_to_main_receiver.try_recv() {
            let (tile_request, tile_origin, new_tile_pixels) = match msg {
                RasterizerToMainMsg::SVGLoaded { .. } => unreachable!(),
                RasterizerToMainMsg::TileRasterized {
                    tile_request,
                    tile_origin,
                    new_tile_pixels,
                } => (tile_request, tile_origin, new_tile_pixels),
            };

            renderer.manager_mut().texture.mark_as_rasterized(tile_request.address,
                                                            &tile_request.descriptor);

            let cache_texture_rect =
                RectI::new(tile_origin, Vector2I::splat(1)).scale(tile_backing_size);
            device.upload_to_texture(&renderer.cache_texture(),
                                    cache_texture_rect,
                                    TextureDataRef::U8(&new_tile_pixels));

            debug!("marking {:?}/{:?} as rasterized!",
                tile_request.address,
                tile_request.descriptor);
        }
    }
}

fn blit(dest: &mut [u8],
        dest_stride: usize,
        dest_rect: RectI,
        src: &[u32],
        src_stride: usize,
        src_origin: Vector2I) {
    for y in 0..dest_rect.size().y() {
        let dest_start = (dest_rect.origin().y() + y) as usize * dest_stride +
            dest_rect.origin().x() as usize * 4;
        let src_start = (src_origin.y() + y) as usize * src_stride + src_origin.x() as usize;
        for x in 0..dest_rect.size().x() {
            let pixel = src[src_start + x as usize];
            let dest_offset = dest_start + x as usize * 4;
            dest[dest_offset + 0] = ((pixel >> 16) & 0xff) as u8;
            dest[dest_offset + 1] = ((pixel >> 8) & 0xff) as u8;
            dest[dest_offset + 2] = (pixel & 0xff) as u8;
            dest[dest_offset + 3] = ((pixel >> 24) & 0xff) as u8;
        }
    }
}

struct MainToRasterizerMsg {
    tile_request: TileRequest,
    tile_origin: Vector2I,
}

enum RasterizerToMainMsg {
    SVGLoaded {
        size: Vector2I,
    },
    TileRasterized {
        tile_request: TileRequest,
        tile_origin: Vector2I,
        new_tile_pixels: Vec<u8>,
    },
}

impl SVGRasterizerThread {
    fn run(&mut self) {
        // Load the SVG.
        let svg_tree = Tree::from_file(&self.svg_path, &UsvgOptions::default()).unwrap();

        let svg_size = svg_tree.svg_node().size;
        let svg_size = Vector2I::new(svg_size.width().ceil() as i32,
                                     svg_size.height().ceil() as i32);
        let svg_screen_size = ScreenSize::new(svg_size.x() as u32, svg_size.y() as u32).unwrap();

        self.rasterizer_to_main_sender
            .send(RasterizerToMainMsg::SVGLoaded { size: svg_size })
            .unwrap();

        // Initialize the cache.
        let tile_backing_size = (self.tile_size + 2) as i32;
        let mut cache_draw_target = DrawTarget::new(tile_backing_size, tile_backing_size);

        while let Ok(msg) = self.main_to_rasterizer_receiver.recv() {
            debug!("rendering {:?}, tile_size={}", msg.tile_request, self.tile_size);
            let mut cache_pixels =
                vec![0; tile_backing_size as usize * tile_backing_size as usize * 4];

            if let Err(_) = panic::catch_unwind(AssertUnwindSafe(|| {
                let descriptor = &msg.tile_request.descriptor;
                let scene_offset = descriptor.tile_position()
                                             .to_f32()
                                             .scale(-(self.tile_size as f32));
                let scale = f32::exp2(descriptor.lod() as f32);

                let mut transform = Transform2F::default();
                transform = Transform2F::from_uniform_scale(scale) * transform;
                transform = Transform2F::from_translation(scene_offset) * transform;
                transform = Transform2F::from_translation(Vector2F::splat(1.0)) * transform;

                cache_draw_target.set_transform(&Transform::row_major(transform.matrix.m11(),
                                                                      transform.matrix.m21(),
                                                                      transform.matrix.m12(),
                                                                      transform.matrix.m22(),
                                                                      transform.vector.x(),
                                                                      transform.vector.y()));
                cache_draw_target.clear(self.background_color);

                backend_raqote::render_to_canvas(&svg_tree,
                                                &ResvgOptions::default(),
                                                svg_screen_size,
                                                &mut cache_draw_target);

                cache_draw_target.set_transform(&Transform::identity());

                blit(&mut cache_pixels,
                    tile_backing_size as usize * 4,
                    RectI::new(Vector2I::default(), Vector2I::splat(tile_backing_size)),
                    cache_draw_target.get_data(),
                    tile_backing_size as usize,
                    Vector2I::default());
            })) {
                error!("rendering {:?} panicked!", msg.tile_request);
                cache_draw_target = DrawTarget::new(tile_backing_size, tile_backing_size);
            }

            self.rasterizer_to_main_sender.send(RasterizerToMainMsg::TileRasterized {
                tile_request: msg.tile_request,
                tile_origin: msg.tile_origin,
                new_tile_pixels: cache_pixels,
            }).unwrap();
        }
    }
}

