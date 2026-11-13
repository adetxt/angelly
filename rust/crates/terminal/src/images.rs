use std::{collections::HashMap, ffi::c_void, ptr, slice, sync::Arc};

use vt_sys as sys;

use crate::{ffi::*, Frame, Term};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageLayer {
    BelowBackground,
    BelowText,
    AboveText,
}

/// Decoded, unpremultiplied RGBA pixels owned independently of the terminal lock.
#[derive(Debug)]
pub struct Image {
    pub id: u32,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct ImagePlacement {
    pub image: Arc<Image>,
    pub layer: ImageLayer,
    pub z: i32,
    pub col: i32,
    pub row: i32,
    /// Offsets and dimensions are in device pixels; source is in image pixels.
    pub offset: (u32, u32),
    pub size: (u32, u32),
    pub source: (u32, u32, u32, u32),
}

struct ImageIterator(sys::GhosttyRenderStateImageIterator);

impl Drop for ImageIterator {
    fn drop(&mut self) {
        unsafe { sys::ghostty_render_state_image_iterator_free(self.0) };
    }
}

impl Term {
    pub(crate) fn update_images(&self, frame: &mut Frame) -> bool {
        use sys::GhosttyRenderStateData as D;
        use sys::GhosttyRenderStateImageData as I;

        let generation = state_get::<u64>(self.state, D::GHOSTTY_RENDER_STATE_DATA_IMAGE_PLACEMENTS_GENERATION);
        if generation == frame.images_generation {
            return false;
        }
        frame.images_generation = generation;
        let mut cached: HashMap<_, _> = frame
            .images
            .drain(..)
            .map(|p| ((p.image.id, p.image.generation), p.image))
            .collect();
        let Some(storage) = terminal_get::<sys::GhosttyKittyGraphics>(
            self.raw,
            sys::GhosttyTerminalData::GHOSTTY_TERMINAL_DATA_KITTY_GRAPHICS,
        ) else {
            return true;
        };
        unsafe {
            let mut iter = ImageIterator(ptr::null_mut());
            check(
                sys::ghostty_render_state_image_iterator_new(ptr::null(), &mut iter.0),
                "image_iterator_new",
            );
            check(
                sys::ghostty_render_state_get(
                    self.state,
                    D::GHOSTTY_RENDER_STATE_DATA_IMAGE_ITERATOR as _,
                    &mut iter.0 as *mut _ as *mut c_void,
                ),
                "image_iterator_get",
            );
            while sys::ghostty_render_state_image_iterator_next(iter.0) {
                let get = |key| {
                    let mut value = 0u32;
                    check(
                        sys::ghostty_render_state_image_get(iter.0, key, &mut value as *mut _ as *mut c_void),
                        "image_get",
                    );
                    value
                };
                if get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_PROTOCOL) as i32
                    != sys::GhosttyImageProtocol::GHOSTTY_IMAGE_PROTOCOL_KITTY
                {
                    continue;
                }
                let id = get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_ID);
                let mut image_generation = 0u64;
                check(
                    sys::ghostty_render_state_image_get(
                        iter.0,
                        I::GHOSTTY_RENDER_STATE_IMAGE_DATA_IMAGE_GENERATION,
                        &mut image_generation as *mut _ as *mut c_void,
                    ),
                    "image_generation",
                );
                let image = if let Some(image) = cached.get(&(id, image_generation)) {
                    image.clone()
                } else {
                    let Some(image) = read_image(storage, id, image_generation) else {
                        continue;
                    };
                    let image = Arc::new(image);
                    cached.insert((id, image_generation), image.clone());
                    image
                };
                let layer = match get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_LAYER) as i32 {
                    sys::GhosttyRenderStateImageLayer::GHOSTTY_RENDER_STATE_IMAGE_LAYER_BELOW_BACKGROUND => {
                        ImageLayer::BelowBackground
                    }
                    sys::GhosttyRenderStateImageLayer::GHOSTTY_RENDER_STATE_IMAGE_LAYER_BELOW_TEXT => {
                        ImageLayer::BelowText
                    }
                    _ => ImageLayer::AboveText,
                };
                frame.images.push(ImagePlacement {
                    image,
                    layer,
                    z: get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_Z) as i32,
                    col: get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_COL) as i32,
                    row: get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_VIEWPORT_ROW) as i32,
                    offset: (
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_X),
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_CELL_OFFSET_Y),
                    ),
                    size: (
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_WIDTH),
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_PIXEL_HEIGHT),
                    ),
                    source: (
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_X),
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_Y),
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_WIDTH),
                        get(I::GHOSTTY_RENDER_STATE_IMAGE_DATA_SOURCE_HEIGHT),
                    ),
                });
            }
        }
        frame.images.sort_by_key(|p| (p.layer, p.z));
        true
    }
}

unsafe fn read_image(storage: sys::GhosttyKittyGraphics, id: u32, generation: u64) -> Option<Image> {
    use sys::GhosttyKittyGraphicsImageData as D;
    use sys::GhosttyKittyImageFormat as F;
    let handle = sys::ghostty_kitty_graphics_image(storage, id);
    if handle.is_null() {
        return None;
    }
    let get = |key| {
        let mut value = 0u32;
        check(
            sys::ghostty_kitty_graphics_image_get(handle, key, &mut value as *mut _ as *mut c_void),
            "image_pixels_get",
        );
        value
    };
    let (width, height) = (
        get(D::GHOSTTY_KITTY_IMAGE_DATA_WIDTH),
        get(D::GHOSTTY_KITTY_IMAGE_DATA_HEIGHT),
    );
    let format = get(D::GHOSTTY_KITTY_IMAGE_DATA_FORMAT) as i32;
    let channels = match format {
        F::GHOSTTY_KITTY_IMAGE_FORMAT_RGB => 3,
        F::GHOSTTY_KITTY_IMAGE_FORMAT_RGBA => 4,
        F::GHOSTTY_KITTY_IMAGE_FORMAT_GRAY_ALPHA => 2,
        F::GHOSTTY_KITTY_IMAGE_FORMAT_GRAY => 1,
        _ => return None,
    };
    let pixels = (width as usize).checked_mul(height as usize)?;
    let expected = pixels.checked_mul(channels)?;
    let mut data: *const u8 = ptr::null();
    let mut len = 0usize;
    if sys::ghostty_kitty_graphics_image_get(
        handle,
        D::GHOSTTY_KITTY_IMAGE_DATA_DATA_PTR,
        &mut data as *mut _ as *mut c_void,
    ) != OK
        || sys::ghostty_kitty_graphics_image_get(
            handle,
            D::GHOSTTY_KITTY_IMAGE_DATA_DATA_LEN,
            &mut len as *mut _ as *mut c_void,
        ) != OK
        || data.is_null()
        || len != expected
        || pixels == 0
    {
        return None;
    }
    let mut rgba = Vec::with_capacity(pixels.checked_mul(4)?);
    for pixel in slice::from_raw_parts(data, len).chunks_exact(channels) {
        let color = match channels {
            4 => [pixel[0], pixel[1], pixel[2], pixel[3]],
            3 => [pixel[0], pixel[1], pixel[2], 255],
            2 => [pixel[0], pixel[0], pixel[0], pixel[1]],
            _ => [pixel[0], pixel[0], pixel[0], 255],
        };
        rgba.extend_from_slice(&color);
    }
    Some(Image {
        id,
        generation,
        width,
        height,
        rgba,
    })
}
