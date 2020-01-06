// virtex/src/svg.rs

use crate::manager::TileRequest;
use crate::renderer_advanced::AdvancedRenderer;
use crate::stack::ConcurrentStack;
use crate::texture::TileDescriptor;

use cairo::{Context, Format, ImageSurface, Matrix};
use crossbeam_channel::{Receiver, Sender};
use pathfinder_content::color::ColorF;
use pathfinder_geometry::rect::RectI;
use pathfinder_geometry::transform2d::Transform2F;
use pathfinder_geometry::vector::{Vector2F, Vector2I};
use pathfinder_gpu::{Device, TextureDataRef};
use resvg::backend_cairo;
use resvg::usvg::{Options as UsvgOptions, Tree};
use resvg::{Options as ResvgOptions, ScreenSize};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub struct SVGRasterizerProxy {
    rasterization_stack: Arc<ConcurrentStack<TileRasterRequest>>,
    rasterized_tile_receiver: Receiver<RasterizedTile>,
    svg_size_receiver: Receiver<Vector2I>,
    #[allow(dead_code)]
    threads: Vec<JoinHandle<()>>,
}

struct SVGRasterizerThread {
    rasterized_tile_sender: Sender<RasterizedTile>,
    rasterization_stack: Arc<ConcurrentStack<TileRasterRequest>>,
    svg_size_sender: Option<Sender<Vector2I>>,
    svg_path: String,
    background_color: ColorF,
    tile_size: u32,
}

impl SVGRasterizerProxy {
    pub fn new(svg_path: String, background_color: ColorF, tile_size: u32, thread_count: u32)
               -> SVGRasterizerProxy {
        let (rasterized_tile_sender, rasterized_tile_receiver) = crossbeam_channel::unbounded();
        let (svg_size_sender, svg_size_receiver) = crossbeam_channel::unbounded();
        let mut svg_size_sender = Some(svg_size_sender);
        let rasterization_stack = Arc::new(ConcurrentStack::new());
        let mut threads = vec![];
        for _ in 0..thread_count {
            // FIXME(pcwalton): Can we only load the SVG once?
            let svg_path_for_thread = svg_path.clone();
            let rasterization_stack_for_thread = rasterization_stack.clone();
            let rasterized_tile_sender_for_thread = rasterized_tile_sender.clone();
            let svg_size_sender_for_thread = svg_size_sender.take();
            threads.push(thread::spawn(move || {
                SVGRasterizerThread {
                    rasterization_stack: rasterization_stack_for_thread,
                    rasterized_tile_sender: rasterized_tile_sender_for_thread,
                    svg_path: svg_path_for_thread,
                    svg_size_sender: svg_size_sender_for_thread,
                    background_color,
                    tile_size,
                }.run()
            }));
        }
        SVGRasterizerProxy {
            rasterization_stack,
            rasterized_tile_receiver,
            svg_size_receiver,
            threads,
        }
    }

    /// Waits for the SVG to load and returns its size.
    ///
    /// This must only be called once, immediately after loading the SVG.
    pub fn wait_for_svg_to_load(&mut self) -> Vector2I {
        self.svg_size_receiver.recv().unwrap()
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
                self.rasterization_stack.push(TileRasterRequest {
                    tile_request: tile_cache_entry,
                    tile_origin,
                });
            }
        }

        let tile_backing_size = renderer.manager().texture.tile_backing_size() as i32;
        while let Ok(msg) = self.rasterized_tile_receiver.try_recv() {
            let RasterizedTile { tile_request, tile_origin, new_tile_pixels } = msg;

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
        src: &[u8],
        src_stride: usize,
        src_origin: Vector2I) {
    for y in 0..dest_rect.size().y() {
        let dest_start = (dest_rect.origin().y() + y) as usize * dest_stride +
            dest_rect.origin().x() as usize * 4;
        let src_start = (src_origin.y() + y) as usize * src_stride + src_origin.x() as usize * 4;
        for x in 0..dest_rect.size().x() {
            let dest_offset = dest_start + x as usize * 4;
            let src_offset = src_start + x as usize * 4;
            dest[dest_offset + 0] = src[src_offset + 2];
            dest[dest_offset + 1] = src[src_offset + 1];
            dest[dest_offset + 2] = src[src_offset + 0];
            dest[dest_offset + 3] = src[src_offset + 3];
        }
    }
}

struct TileRasterRequest {
    tile_request: TileRequest,
    tile_origin: Vector2I,
}

struct RasterizedTile {
    tile_request: TileRequest,
    tile_origin: Vector2I,
    new_tile_pixels: Vec<u8>,
}

impl SVGRasterizerThread {
    fn run(&mut self) {
        // Load the SVG.
        let svg_tree = Tree::from_file(&self.svg_path, &UsvgOptions::default()).unwrap();

        let svg_size = svg_tree.svg_node().size;
        let svg_size = Vector2I::new(svg_size.width().ceil() as i32,
                                     svg_size.height().ceil() as i32);
        let svg_screen_size = ScreenSize::new(svg_size.x() as u32, svg_size.y() as u32).unwrap();

        if let Some(ref svg_size_sender) = self.svg_size_sender {
            svg_size_sender.send(svg_size).unwrap();
        }

        // Initialize the cache.
        let tile_backing_size = (self.tile_size + 2) as i32;
        let mut cache_surface = ImageSurface::create(Format::ARgb32,
                                                     tile_backing_size,
                                                     tile_backing_size).unwrap();

        loop {
            let msg = self.rasterization_stack.pop();
            debug!("rendering {:?}, tile_size={}", msg.tile_request, self.tile_size);
            let mut cache_pixels =
                vec![0; tile_backing_size as usize * tile_backing_size as usize * 4];

            {
                let mut cache_draw_target = Context::new(&cache_surface);
                let transform = transform_for_tile_descriptor(&msg.tile_request.descriptor,
                                                              self.tile_size);

                cache_draw_target.transform(Matrix::new(transform.matrix.m11() as f64,
                                                        transform.matrix.m21() as f64,
                                                        transform.matrix.m12() as f64,
                                                        transform.matrix.m22() as f64,
                                                        transform.vector.x() as f64,
                                                        transform.vector.y() as f64));
                cache_draw_target.set_source_rgb(self.background_color.r() as f64,
                                                 self.background_color.g() as f64,
                                                 self.background_color.b() as f64);
                cache_draw_target.paint();

                backend_cairo::render_to_canvas(&svg_tree,
                                                &ResvgOptions::default(),
                                                svg_screen_size,
                                                &mut cache_draw_target);

                cache_draw_target.transform(Matrix::identity());
            }

            blit(&mut cache_pixels,
                 tile_backing_size as usize * 4,
                 RectI::new(Vector2I::default(), Vector2I::splat(tile_backing_size)),
                 &*cache_surface.get_data().unwrap(),
                 tile_backing_size as usize * 4,
                 Vector2I::default());

            self.rasterized_tile_sender.send(RasterizedTile {
                tile_request: msg.tile_request,
                tile_origin: msg.tile_origin,
                new_tile_pixels: cache_pixels,
            }).unwrap();
        }
    }
}

fn transform_for_tile_descriptor(descriptor: &TileDescriptor, tile_size: u32) -> Transform2F {
    let scene_offset = descriptor.tile_position().to_f32().scale(-(tile_size as f32));
    let scale = f32::exp2(descriptor.lod() as f32);

    let mut transform = Transform2F::default();
    transform = Transform2F::from_uniform_scale(scale) * transform;
    transform = Transform2F::from_translation(scene_offset) * transform;
    transform = Transform2F::from_translation(Vector2F::splat(1.0)) * transform;
    transform
}
