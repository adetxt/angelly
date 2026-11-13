use std::{collections::HashMap, sync::Arc};

use gpui_kit::{point, px, size, App, Bounds, Corners, Pixels, Point, RenderImage, Size, Window};
use terminal::{Frame, ImageLayer, ImagePlacement};

use super::font::Metrics;

#[derive(Default)]
pub(super) struct ImageCache {
    textures: HashMap<(u32, u64), Arc<RenderImage>>,
}

impl ImageCache {
    pub fn update(&mut self, placements: &[ImagePlacement], window: &mut Window, cx: &mut App) {
        self.textures.retain(|key, texture| {
            let keep = placements.iter().any(|p| (p.image.id, p.image.generation) == *key);
            if !keep {
                cx.drop_image(texture.clone(), Some(window));
            }
            keep
        });
        for placement in placements {
            let image = &placement.image;
            self.textures.entry((image.id, image.generation)).or_insert_with(|| {
                let mut bgra = image.rgba.clone();
                for pixel in bgra.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                }
                let buffer = image::RgbaImage::from_raw(image.width, image.height, bgra)
                    .expect("terminal image dimensions match its decoded pixels");
                Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
            });
        }
    }

    pub fn clear(&mut self, cx: &mut App) {
        for (_, texture) in self.textures.drain() {
            cx.drop_image(texture, None);
        }
    }

    pub fn paint(
        &self,
        frame: &Frame,
        layer: ImageLayer,
        origin: Point<Pixels>,
        metrics: Metrics,
        window: &mut Window,
    ) {
        let viewport = Bounds::new(
            origin,
            size(
                metrics.cell_width * f32::from(frame.cols),
                metrics.cell_height * frame.rows.len() as f32,
            ),
        );
        for placement in frame.images.iter().filter(|p| p.layer == layer) {
            let Some(texture) = self.textures.get(&(placement.image.id, placement.image.generation)) else {
                continue;
            };
            let Some((destination, image_bounds)) = image_bounds(
                placement,
                origin,
                size(metrics.cell_width, metrics.cell_height),
                metrics.scale,
            ) else {
                continue;
            };
            if let Err(error) = window.paint_image(
                destination.intersect(&viewport),
                image_bounds,
                Corners::default(),
                texture.clone(),
                0,
                false,
            ) {
                eprintln!("terminal image paint failed: {error}");
            }
        }
    }
}

fn image_bounds(
    p: &ImagePlacement,
    origin: Point<Pixels>,
    cell: Size<Pixels>,
    scale: f32,
) -> Option<(Bounds<Pixels>, Bounds<Pixels>)> {
    let (sx, sy, sw, sh) = p.source;
    if sw == 0 || sh == 0 || p.size.0 == 0 || p.size.1 == 0 {
        return None;
    }
    let (w, h) = (p.size.0 as f32 / scale, p.size.1 as f32 / scale);
    let destination = Bounds::new(
        point(
            origin.x + cell.width * p.col as f32 + px(p.offset.0 as f32 / scale),
            origin.y + cell.height * p.row as f32 + px(p.offset.1 as f32 / scale),
        ),
        size(px(w), px(h)),
    );
    // Map the complete texture so that its source rectangle fills the destination.
    let image = Bounds::new(
        destination.origin - point(px(sx as f32 * w / sw as f32), px(sy as f32 * h / sh as f32)),
        size(
            px(p.image.width as f32 * w / sw as f32),
            px(p.image.height as f32 * h / sh as f32),
        ),
    );
    Some((destination, image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropped_images_keep_device_offsets_and_scale() {
        let mut p = ImagePlacement {
            image: Arc::new(terminal::Image {
                id: 1,
                generation: 1,
                width: 200,
                height: 100,
                rgba: Vec::new(),
            }),
            layer: ImageLayer::AboveText,
            z: 0,
            col: 2,
            row: -1,
            offset: (4, 6),
            size: (80, 40),
            source: (50, 25, 100, 50),
        };
        let origin = point(px(10.), px(20.));
        for scale in [1., 2.] {
            let cell = size(px(20. / scale), px(40. / scale));
            let (destination, image) = image_bounds(&p, origin, cell, scale).unwrap();
            assert_eq!(
                destination,
                Bounds::new(
                    origin + point(px(44. / scale), px(-34. / scale)),
                    size(px(80. / scale), px(40. / scale))
                )
            );
            assert_eq!(
                image,
                Bounds::new(
                    destination.origin - point(px(40. / scale), px(20. / scale)),
                    size(px(160. / scale), px(80. / scale))
                )
            );
        }
        p.source.2 = 0;
        assert!(image_bounds(&p, origin, size(px(10.), px(20.)), 2.).is_none());
    }
}
