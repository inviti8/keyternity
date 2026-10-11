//! PSD flattening with layers that extend past the canvas, as GIMP leaves
//! them after cropping the canvas without cropping the layers.

/// A minimal 8-bit RGB PSD of `canvas` size with one raw (uncompressed)
/// layer at `(left, top)` whose red channel is `40 * x` for layer column x.
fn psd_bytes(canvas: (u32, u32), left: i32, top: i32, size: (u32, u32)) -> Vec<u8> {
    let (cw, ch) = canvas;
    let (lw, lh) = size;
    let mut b = vec![];

    // file header
    b.extend(b"8BPS");
    b.extend(1u16.to_be_bytes());
    b.extend([0u8; 6]);
    b.extend(3u16.to_be_bytes()); // channels
    b.extend(ch.to_be_bytes());
    b.extend(cw.to_be_bytes());
    b.extend(8u16.to_be_bytes()); // depth
    b.extend(3u16.to_be_bytes()); // RGB

    // color mode data, image resources
    b.extend(0u32.to_be_bytes());
    b.extend(0u32.to_be_bytes());

    // layer record
    let channel_len = 2 + lw * lh;
    let mut rec = vec![];
    rec.extend(top.to_be_bytes());
    rec.extend(left.to_be_bytes());
    rec.extend((top + lh as i32).to_be_bytes());
    rec.extend((left + lw as i32).to_be_bytes());
    rec.extend(4u16.to_be_bytes());
    for id in [-1i16, 0, 1, 2] {
        rec.extend(id.to_be_bytes());
        rec.extend(channel_len.to_be_bytes());
    }
    rec.extend(b"8BIMnorm");
    rec.extend([255, 0, 0, 0]); // opacity, clipping, flags (visible), filler
    let name = b"\x05layer\0\0"; // pascal string padded to 4 bytes
    rec.extend((8 + name.len() as u32).to_be_bytes());
    rec.extend(0u32.to_be_bytes()); // mask data
    rec.extend(0u32.to_be_bytes()); // blending ranges
    rec.extend(name);

    // channel image data: alpha, red, green, blue
    let mut chans = vec![];
    for c in 0..4 {
        chans.extend(0u16.to_be_bytes()); // raw
        for _y in 0..lh {
            for x in 0..lw {
                chans.push(match c {
                    0 => 255,
                    1 => (40 * x) as u8,
                    _ => 0,
                });
            }
        }
    }

    let mut layer_info = vec![];
    layer_info.extend(1i16.to_be_bytes());
    layer_info.extend(rec);
    layer_info.extend(chans);

    let mut lmi = vec![];
    lmi.extend((layer_info.len() as u32).to_be_bytes());
    lmi.extend(layer_info);
    lmi.extend(0u32.to_be_bytes()); // global mask info
    b.extend((lmi.len() as u32).to_be_bytes());
    b.extend(lmi);

    // composite image data
    b.extend(0u16.to_be_bytes());
    b.extend(vec![0u8; (cw * ch * 3) as usize]);
    b
}

fn flatten(bytes: &[u8]) -> (Vec<u8>, u32, u32, u32, u32) {
    let psd = psd::Psd::from_bytes(bytes).unwrap();
    psd.flatten_layers_rgba(&|_| true).unwrap()
}

#[test]
fn layer_larger_than_canvas_is_clipped() {
    // 6x3 layer at (0, 0) on a 4x2 canvas
    let (px, w, h, x, y) = flatten(&psd_bytes((4, 2), 0, 0, (6, 3)));
    assert_eq!((w, h, x, y), (4, 2, 0, 0));
    for row in 0..2 {
        for col in 0..4 {
            let i = (row * 4 + col) * 4;
            assert_eq!(&px[i..i + 4], &[40 * col as u8, 0, 0, 255], "pixel ({col}, {row})");
        }
    }
}

#[test]
fn layer_at_negative_offset_is_clipped() {
    // 6x3 layer at (-1, -1) on a 4x2 canvas: canvas x shows layer column x + 1
    let (px, w, h, x, y) = flatten(&psd_bytes((4, 2), -1, -1, (6, 3)));
    assert_eq!((w, h, x, y), (4, 2, 0, 0));
    for row in 0..2 {
        for col in 0..4 {
            let i = (row * 4 + col) * 4;
            assert_eq!(&px[i..i + 4], &[40 * (col + 1) as u8, 0, 0, 255], "pixel ({col}, {row})");
        }
    }
}

#[test]
fn layer_inside_canvas_keeps_its_full_size() {
    // 2x1 layer at (1, 1) on a 4x2 canvas: crop is exactly the layer
    let (px, w, h, x, y) = flatten(&psd_bytes((4, 2), 1, 1, (2, 1)));
    assert_eq!((w, h, x, y), (2, 1, 1, 1));
    assert_eq!(px, vec![0, 0, 0, 255, 40, 0, 0, 255]);
}
