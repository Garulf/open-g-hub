use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSize {
    Small,
    Medium,
    Large,
}

impl IconSize {
    fn png(self) -> &'static [u8] {
        match self {
            IconSize::Small => include_bytes!("../../../assets/icon-32.png"),
            IconSize::Medium => include_bytes!("../../../assets/icon-64.png"),
            IconSize::Large => include_bytes!("../../../assets/icon-256.png"),
        }
    }
}

/// Returns the unpremultiplied RGBA pixels, the width and the height.
pub fn rgba(size: IconSize) -> (Vec<u8>, u32, u32) {
    let mut decoder = png::Decoder::new(Cursor::new(size.png()));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().expect("embedded icon is a PNG");
    let mut pixels = vec![0; reader.output_buffer_size().expect("embedded icon fits")];
    let frame = reader
        .next_frame(&mut pixels)
        .expect("embedded icon decodes");
    assert_eq!(
        frame.color_type,
        png::ColorType::Rgba,
        "embedded icon is RGBA"
    );
    pixels.truncate(frame.buffer_size());
    (pixels, frame.width, frame.height)
}
