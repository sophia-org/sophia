fn sophia_present_pixmap_request(
    byte_order: XByteOrder,
    window: u32,
    pixmap: u32,
    damage: (i16, i16, u16, u16),
    previous_committed_generation: u64,
    timeout_msec: u32,
) -> Vec<u8> {
    let mut out = vec![
        X_SOPHIA_PRESENT_MAJOR_OPCODE,
        X_SOPHIA_PRESENT_PIXMAP_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 8);
    push_u32(&mut out, byte_order, window);
    push_u32(&mut out, byte_order, pixmap);
    push_i16(&mut out, byte_order, damage.0);
    push_i16(&mut out, byte_order, damage.1);
    push_u16(&mut out, byte_order, damage.2);
    push_u16(&mut out, byte_order, damage.3);
    push_u64(&mut out, byte_order, previous_committed_generation);
    push_u32(&mut out, byte_order, timeout_msec);
    out
}

fn mit_shm_query_version_request(byte_order: XByteOrder) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_QUERY_VERSION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 1);
    out
}

fn mit_shm_attach_request(
    byte_order: XByteOrder,
    segment: u32,
    shmid: u32,
    read_only: bool,
) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_ATTACH_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, segment);
    push_u32(&mut out, byte_order, shmid);
    out.push(u8::from(read_only));
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn mit_shm_attach_fd_request(byte_order: XByteOrder, segment: u32, read_only: bool) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_ATTACH_FD_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, segment);
    out.push(u8::from(read_only));
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn mit_shm_create_segment_request(
    byte_order: XByteOrder,
    segment: u32,
    size: u32,
    read_only: bool,
) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_CREATE_SEGMENT_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, segment);
    push_u32(&mut out, byte_order, size);
    out.push(u8::from(read_only));
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn mit_shm_get_image_request(
    byte_order: XByteOrder,
    drawable: u32,
    segment: u32,
    offset: u32,
) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_GET_IMAGE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 8);
    push_u32(&mut out, byte_order, drawable);
    push_i16(&mut out, byte_order, 3);
    push_i16(&mut out, byte_order, 5);
    push_u16(&mut out, byte_order, 32);
    push_u16(&mut out, byte_order, 24);
    push_u32(&mut out, byte_order, u32::MAX);
    out.push(2);
    out.extend_from_slice(&[0; 3]);
    push_u32(&mut out, byte_order, segment);
    push_u32(&mut out, byte_order, offset);
    out
}

fn mit_shm_detach_request(byte_order: XByteOrder, segment: u32) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_DETACH_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, segment);
    out
}

fn mit_shm_put_image_request(
    byte_order: XByteOrder,
    drawable: u32,
    gc: u32,
    segment: u32,
    offset: u32,
) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_PUT_IMAGE_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 10);
    push_u32(&mut out, byte_order, drawable);
    push_u32(&mut out, byte_order, gc);
    push_u16(&mut out, byte_order, 64);
    push_u16(&mut out, byte_order, 48);
    push_u16(&mut out, byte_order, 0);
    push_u16(&mut out, byte_order, 0);
    push_u16(&mut out, byte_order, 32);
    push_u16(&mut out, byte_order, 24);
    push_i16(&mut out, byte_order, 3);
    push_i16(&mut out, byte_order, 5);
    out.push(24);
    out.push(2);
    out.push(0);
    out.push(0);
    push_u32(&mut out, byte_order, segment);
    push_u32(&mut out, byte_order, offset);
    out
}

fn mit_shm_create_pixmap_request(
    byte_order: XByteOrder,
    pixmap: u32,
    drawable: u32,
    segment: u32,
    offset: u32,
) -> Vec<u8> {
    let mut out = vec![X_MIT_SHM_MAJOR_OPCODE, X_MIT_SHM_CREATE_PIXMAP_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 7);
    push_u32(&mut out, byte_order, pixmap);
    push_u32(&mut out, byte_order, drawable);
    push_u16(&mut out, byte_order, 64);
    push_u16(&mut out, byte_order, 48);
    out.push(24);
    out.extend_from_slice(&[0, 0, 0]);
    push_u32(&mut out, byte_order, segment);
    push_u32(&mut out, byte_order, offset);
    out
}

fn randr_query_version_request(
    byte_order: XByteOrder,
    major_version: u32,
    minor_version: u32,
) -> Vec<u8> {
    let mut out = vec![X_RANDR_MAJOR_OPCODE, X_RANDR_QUERY_VERSION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, major_version);
    push_u32(&mut out, byte_order, minor_version);
    out
}

fn extension_query_version_request(
    byte_order: XByteOrder,
    opcode: u8,
    major_version: u32,
    minor_version: u32,
) -> Vec<u8> {
    let mut out = vec![opcode, 0];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, major_version);
    push_u32(&mut out, byte_order, minor_version);
    out
}

fn dri3_open_request(byte_order: XByteOrder, drawable: u32, provider: u32) -> Vec<u8> {
    let mut out = vec![X_DRI3_MAJOR_OPCODE, X_DRI3_OPEN_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, drawable);
    push_u32(&mut out, byte_order, provider);
    out
}

fn dri3_get_supported_modifiers_request(
    byte_order: XByteOrder,
    window: u32,
    depth: u8,
    bits_per_pixel: u8,
) -> Vec<u8> {
    let mut out = vec![
        X_DRI3_MAJOR_OPCODE,
        X_DRI3_GET_SUPPORTED_MODIFIERS_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, window);
    out.push(depth);
    out.push(bits_per_pixel);
    out.extend_from_slice(&[0; 2]);
    out
}

#[allow(clippy::too_many_arguments)]
fn dri3_pixmap_from_buffer_request(
    byte_order: XByteOrder,
    pixmap: u32,
    drawable: u32,
    size_bytes: u32,
    width: u16,
    height: u16,
    stride: u16,
    depth: u8,
    bits_per_pixel: u8,
) -> Vec<u8> {
    let mut out = vec![X_DRI3_MAJOR_OPCODE, X_DRI3_PIXMAP_FROM_BUFFER_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 6);
    push_u32(&mut out, byte_order, pixmap);
    push_u32(&mut out, byte_order, drawable);
    push_u32(&mut out, byte_order, size_bytes);
    push_u16(&mut out, byte_order, width);
    push_u16(&mut out, byte_order, height);
    push_u16(&mut out, byte_order, stride);
    out.push(depth);
    out.push(bits_per_pixel);
    out
}

#[allow(clippy::too_many_arguments)]
fn dri3_pixmap_from_buffers_request(
    byte_order: XByteOrder,
    pixmap: u32,
    window: u32,
    num_buffers: u8,
    width: u16,
    height: u16,
    strides: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
    offsets: [u32; sophia_protocol::DMA_BUF_MAX_PLANES],
    depth: u8,
    bits_per_pixel: u8,
    modifier: u64,
) -> Vec<u8> {
    let mut out = vec![X_DRI3_MAJOR_OPCODE, X_DRI3_PIXMAP_FROM_BUFFERS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 16);
    push_u32(&mut out, byte_order, pixmap);
    push_u32(&mut out, byte_order, window);
    out.push(num_buffers);
    out.extend_from_slice(&[0; 3]);
    push_u16(&mut out, byte_order, width);
    push_u16(&mut out, byte_order, height);
    for (stride, offset) in strides.into_iter().zip(offsets) {
        push_u32(&mut out, byte_order, stride);
        push_u32(&mut out, byte_order, offset);
    }
    out.push(depth);
    out.push(bits_per_pixel);
    out.extend_from_slice(&[0; 2]);
    push_u64(&mut out, byte_order, modifier);
    out
}

fn dri3_buffers_from_pixmap_request(byte_order: XByteOrder, pixmap: u32) -> Vec<u8> {
    let mut out = vec![X_DRI3_MAJOR_OPCODE, X_DRI3_BUFFERS_FROM_PIXMAP_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, pixmap);
    out
}

fn dri3_buffer_from_pixmap_request(byte_order: XByteOrder, pixmap: u32) -> Vec<u8> {
    let mut out = vec![X_DRI3_MAJOR_OPCODE, X_DRI3_BUFFER_FROM_PIXMAP_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, pixmap);
    out
}

fn dri3_fence_from_fd_request(
    byte_order: XByteOrder,
    drawable: u32,
    fence: u32,
    initially_triggered: bool,
) -> Vec<u8> {
    let mut out = vec![X_DRI3_MAJOR_OPCODE, X_DRI3_FENCE_FROM_FD_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, drawable);
    push_u32(&mut out, byte_order, fence);
    out.push(u8::from(initially_triggered));
    out.extend_from_slice(&[0; 3]);
    out
}

fn xfixes_create_region_request(
    byte_order: XByteOrder,
    region: u32,
    rectangles: &[Rect],
) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_CREATE_REGION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (2 + rectangles.len() * 2) as u16);
    push_u32(&mut out, byte_order, region);
    for rectangle in rectangles {
        push_i16(&mut out, byte_order, rectangle.x as i16);
        push_i16(&mut out, byte_order, rectangle.y as i16);
        push_u16(&mut out, byte_order, rectangle.width as u16);
        push_u16(&mut out, byte_order, rectangle.height as u16);
    }
    out
}

fn xfixes_set_region_request(byte_order: XByteOrder, region: u32, rectangles: &[Rect]) -> Vec<u8> {
    let mut out = vec![X_XFIXES_MAJOR_OPCODE, X_XFIXES_SET_REGION_MINOR_OPCODE];
    push_u16(&mut out, byte_order, (2 + rectangles.len() * 2) as u16);
    push_u32(&mut out, byte_order, region);
    for rectangle in rectangles {
        push_i16(&mut out, byte_order, rectangle.x as i16);
        push_i16(&mut out, byte_order, rectangle.y as i16);
        push_u16(&mut out, byte_order, rectangle.width as u16);
        push_u16(&mut out, byte_order, rectangle.height as u16);
    }
    out
}

fn xfixes_select_selection_input_request(
    byte_order: XByteOrder,
    window: u32,
    selection: u32,
    event_mask: u32,
) -> Vec<u8> {
    let mut out = vec![
        X_XFIXES_MAJOR_OPCODE,
        X_XFIXES_SELECT_SELECTION_INPUT_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, window);
    push_u32(&mut out, byte_order, selection);
    push_u32(&mut out, byte_order, event_mask);
    out
}

fn randr_get_output_property_request(
    byte_order: XByteOrder,
    output: u32,
    property: u32,
    long_length: u32,
) -> Vec<u8> {
    let mut out = vec![
        X_RANDR_MAJOR_OPCODE,
        X_RANDR_GET_OUTPUT_PROPERTY_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 7);
    push_u32(&mut out, byte_order, output);
    push_u32(&mut out, byte_order, property);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, long_length);
    out.extend_from_slice(&[0, 0, 0, 0]);
    out
}

fn present_pixmap_request(
    byte_order: XByteOrder,
    window: XResourceId,
    pixmap: XResourceId,
    serial: u32,
) -> Vec<u8> {
    let mut out = vec![X_PRESENT_MAJOR_OPCODE, X_PRESENT_PIXMAP_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 18);
    push_u32(&mut out, byte_order, window.local.raw() as u32);
    push_u32(&mut out, byte_order, pixmap.local.raw() as u32);
    push_u32(&mut out, byte_order, serial);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u16(&mut out, byte_order, 0);
    push_u16(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u32(&mut out, byte_order, 0);
    push_u64(&mut out, byte_order, 0);
    push_u64(&mut out, byte_order, 0);
    push_u64(&mut out, byte_order, 0);
    out
}

fn present_select_input_request(
    byte_order: XByteOrder,
    event_id: u32,
    window: u32,
    event_mask: u32,
) -> Vec<u8> {
    let mut out = vec![X_PRESENT_MAJOR_OPCODE, X_PRESENT_SELECT_INPUT_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, event_id);
    push_u32(&mut out, byte_order, window);
    push_u32(&mut out, byte_order, event_mask);
    out
}

fn randr_select_input_request(byte_order: XByteOrder, window: u32, enable: u16) -> Vec<u8> {
    let mut out = vec![X_RANDR_MAJOR_OPCODE, X_RANDR_SELECT_INPUT_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, window);
    push_u16(&mut out, byte_order, enable);
    push_u16(&mut out, byte_order, 0);
    out
}

fn randr_get_monitors_request(byte_order: XByteOrder, window: u32, get_active: bool) -> Vec<u8> {
    let mut out = vec![X_RANDR_MAJOR_OPCODE, X_RANDR_GET_MONITORS_MINOR_OPCODE];
    push_u16(&mut out, byte_order, 3);
    push_u32(&mut out, byte_order, window);
    out.push(u8::from(get_active));
    out.extend_from_slice(&[0, 0, 0]);
    out
}

fn randr_window_request(byte_order: XByteOrder, minor_opcode: u8, window: u32) -> Vec<u8> {
    let mut out = vec![X_RANDR_MAJOR_OPCODE, minor_opcode];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, window);
    out
}

fn randr_crtc_request(byte_order: XByteOrder, minor_opcode: u8, crtc: u32) -> Vec<u8> {
    let mut out = vec![X_RANDR_MAJOR_OPCODE, minor_opcode];
    push_u16(&mut out, byte_order, 2);
    push_u32(&mut out, byte_order, crtc);
    out
}

fn query_extension_request(byte_order: XByteOrder, name: &str) -> Vec<u8> {
    let mut out = vec![98, 0];
    let len_units = (8 + padded_len_for_test(name.len())) / 4;
    push_u16(&mut out, byte_order, len_units as u16);
    push_u16(&mut out, byte_order, name.len() as u16);
    push_u16(&mut out, byte_order, 0);
    out.extend_from_slice(name.as_bytes());
    pad_to_four(&mut out);
    out
}

fn push_u16(out: &mut Vec<u8>, byte_order: XByteOrder, value: u16) {
    match byte_order {
        XByteOrder::LittleEndian => out.extend_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.extend_from_slice(&value.to_be_bytes()),
    }
}

fn push_i16(out: &mut Vec<u8>, byte_order: XByteOrder, value: i16) {
    match byte_order {
        XByteOrder::LittleEndian => out.extend_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.extend_from_slice(&value.to_be_bytes()),
    }
}

fn push_u32(out: &mut Vec<u8>, byte_order: XByteOrder, value: u32) {
    match byte_order {
        XByteOrder::LittleEndian => out.extend_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.extend_from_slice(&value.to_be_bytes()),
    }
}

fn push_u64(out: &mut Vec<u8>, byte_order: XByteOrder, value: u64) {
    match byte_order {
        XByteOrder::LittleEndian => out.extend_from_slice(&value.to_le_bytes()),
        XByteOrder::BigEndian => out.extend_from_slice(&value.to_be_bytes()),
    }
}

fn query_colors_request(byte_order: XByteOrder, colormap: u32, pixels: &[u32]) -> Vec<u8> {
    let mut out = vec![91, 0];
    let len_units = 2 + pixels.len();
    push_u16(&mut out, byte_order, len_units as u16);
    push_u32(&mut out, byte_order, colormap);
    for pixel in pixels {
        push_u32(&mut out, byte_order, *pixel);
    }
    out
}

fn create_colormap_request(
    byte_order: XByteOrder,
    colormap: u32,
    window: u32,
    visual: u32,
) -> Vec<u8> {
    let mut out = vec![78, 0];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, colormap);
    push_u32(&mut out, byte_order, window);
    push_u32(&mut out, byte_order, visual);
    out
}

fn alloc_color_request(
    byte_order: XByteOrder,
    colormap: u32,
    red: u16,
    green: u16,
    blue: u16,
) -> Vec<u8> {
    let mut out = vec![84, 0];
    push_u16(&mut out, byte_order, 4);
    push_u32(&mut out, byte_order, colormap);
    push_u16(&mut out, byte_order, red);
    push_u16(&mut out, byte_order, green);
    push_u16(&mut out, byte_order, blue);
    push_u16(&mut out, byte_order, 0);
    out
}

fn alloc_named_color_request(byte_order: XByteOrder, colormap: u32, name: &str) -> Vec<u8> {
    let mut out = vec![85, 0];
    let len_units = (12 + padded_len_for_test(name.len())) / 4;
    push_u16(&mut out, byte_order, len_units as u16);
    push_u32(&mut out, byte_order, colormap);
    push_u16(&mut out, byte_order, name.len() as u16);
    push_u16(&mut out, byte_order, 0);
    out.extend_from_slice(name.as_bytes());
    pad_to_four(&mut out);
    out
}

fn xkb_use_extension_request(
    byte_order: XByteOrder,
    wanted_major: u16,
    wanted_minor: u16,
) -> Vec<u8> {
    let mut out = vec![
        X_KEYBOARD_MAJOR_OPCODE,
        X_KEYBOARD_USE_EXTENSION_MINOR_OPCODE,
    ];
    push_u16(&mut out, byte_order, 2);
    push_u16(&mut out, byte_order, wanted_major);
    push_u16(&mut out, byte_order, wanted_minor);
    out
}

fn configure_window_request(
    byte_order: XByteOrder,
    window: u32,
    value_mask: u16,
    values: &[u32],
) -> Vec<u8> {
    let mut out = vec![12, 0];
    let len_units = 3 + values.len();
    push_u16(&mut out, byte_order, len_units as u16);
    push_u32(&mut out, byte_order, window);
    push_u16(&mut out, byte_order, value_mask);
    push_u16(&mut out, byte_order, 0);
    for value in values {
        push_u32(&mut out, byte_order, *value);
    }
    out
}

fn read_u16(byte_order: XByteOrder, bytes: &[u8]) -> u16 {
    match byte_order {
        XByteOrder::LittleEndian => u16::from_le_bytes(bytes.try_into().unwrap()),
        XByteOrder::BigEndian => u16::from_be_bytes(bytes.try_into().unwrap()),
    }
}

fn read_i16(byte_order: XByteOrder, bytes: &[u8]) -> i16 {
    match byte_order {
        XByteOrder::LittleEndian => i16::from_le_bytes(bytes.try_into().unwrap()),
        XByteOrder::BigEndian => i16::from_be_bytes(bytes.try_into().unwrap()),
    }
}

fn read_u32(byte_order: XByteOrder, bytes: &[u8]) -> u32 {
    match byte_order {
        XByteOrder::LittleEndian => u32::from_le_bytes(bytes.try_into().unwrap()),
        XByteOrder::BigEndian => u32::from_be_bytes(bytes.try_into().unwrap()),
    }
}

fn read_u64(byte_order: XByteOrder, bytes: &[u8]) -> u64 {
    match byte_order {
        XByteOrder::LittleEndian => u64::from_le_bytes(bytes.try_into().unwrap()),
        XByteOrder::BigEndian => u64::from_be_bytes(bytes.try_into().unwrap()),
    }
}

fn pad_to_four(out: &mut Vec<u8>) {
    out.resize(padded_len_for_test(out.len()), 0);
}

const fn padded_len_for_test(len: usize) -> usize {
    (len + 3) & !3
}

#[cfg(unix)]
fn wait_for_socket(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("timed out waiting for socket {}", path.display());
}

#[cfg(unix)]
fn read_setup_success(stream: &mut std::os::unix::net::UnixStream,
    byte_order: XByteOrder) {
    let _ = read_setup_resource_id_base(stream, byte_order);
}

/// How long a record read waits with nothing arriving before calling it a hang.
///
/// Generous on purpose. These sockets carry a timeout for one reason -- so a test
/// waiting on a record that will never come fails with a message instead of
/// hanging until the harness kills it. It is not a latency assertion, and the
/// second-scale values it replaced were being read as one: under a loaded machine
/// a threaded service can take longer than a second to deliver, and tests failed
/// for being slow rather than wrong.
///
/// Tests that prove *silence* need the opposite and set their own short window
/// inline, reading the socket directly rather than through the helpers here.
#[cfg(unix)]
const X_RECORD_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Fills `buffer` from a socket that carries a read timeout.
///
/// `read_exact` cannot be used on these streams. They all set `SO_RCVTIMEO`, and
/// when it expires part-way through a record `read_exact` returns `WouldBlock`
/// having already consumed an unspecified number of bytes -- the read fails *and*
/// leaves the stream mid-record, so every later read on it is garbage. Under load
/// a writer is slow enough for a record to straddle that boundary, which is the
/// whole of the flake: whichever test lost the race failed, and never the same one
/// twice.
///
/// The timeout is treated as an idle budget rather than a deadline for the record.
/// That is what it was there to express: a peer that has stopped talking is a
/// failure, a peer that is merely slow is not. Bytes that arrive reset it, so a
/// record split across the boundary is resumed rather than lost.
#[cfg(unix)]
fn fill_from_socket(stream: &mut std::os::unix::net::UnixStream,
    buffer: &mut [u8]) {
    use std::io::Read;

    let budget = stream
        .read_timeout()
        .expect("socket read timeout is readable")
        .unwrap_or(X_RECORD_READ_TIMEOUT);
    let mut filled = 0;
    let mut last_progress = std::time::Instant::now();
    while filled < buffer.len() {
        match stream.read(&mut buffer[filled..]) {
            Ok(0) => panic!(
                "peer closed mid-record after {filled} of {} bytes",
                buffer.len()
            ),
            Ok(read) => {
                filled += read;
                last_progress = std::time::Instant::now();
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                assert!(
                    last_progress.elapsed() < budget,
                    "no bytes for {budget:?} with {filled} of {} read",
                    buffer.len()
                );
            }
            Err(error) => panic!(
                "record read failed after {filled} of {} bytes: {error}",
                buffer.len()
            ),
        }
    }
}

#[cfg(unix)]
fn read_setup_resource_id_base(
    stream: &mut std::os::unix::net::UnixStream,
    byte_order: XByteOrder,
) -> u32 {
    let mut prefix = [0; X_SETUP_REPLY_PREFIX_LEN];
    fill_from_socket(stream, &mut prefix);
    assert_eq!(prefix[0], 1);
    let body_len = usize::from(read_u16(byte_order, &prefix[6..8])) * 4;
    let mut body = vec![0; body_len];
    fill_from_socket(stream, &mut body);
    read_u32(byte_order, &body[4..8])
}

#[cfg(unix)]
fn read_x_record(stream: &mut std::os::unix::net::UnixStream) -> [u8; 32] {
    let mut record = [0; 32];
    fill_from_socket(stream, &mut record);
    record
}

/// Connects to a listening authority, tolerating a listener that is still starting.
///
/// `wait_for_socket` waits for the path to appear, which is not the same as the
/// server having called `listen`, and is not the same as its backlog having room.
/// On a loaded machine a connect can arrive in the gap and be refused. Retrying is
/// the honest response: a refused connect at startup is a race, and only a connect
/// that keeps being refused is a failure.
#[cfg(unix)]
fn connect_x_socket(path: &std::path::Path) -> std::os::unix::net::UnixStream {
    let deadline = std::time::Instant::now() + X_RECORD_READ_TIMEOUT;
    loop {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(error) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "could not connect to {}: {error}",
                    path.display()
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }
}

/// Waits until everything already written on this connection has been processed.
///
/// X orders requests within a connection and says nothing about ordering between
/// them. A test that creates a window on one connection and then asks about it
/// from a second is asserting an ordering the protocol does not provide: under
/// load the second connection's request can reach the authority first and be
/// answered, correctly, with `BadWindow`.
///
/// A round trip is the only barrier available. `GetGeometry` is the cheapest one
/// that names a window, and its reply cannot arrive before the requests queued
/// ahead of it on the same connection have been handled.
#[cfg(unix)]
fn sync_x_connection(
    stream: &mut std::os::unix::net::UnixStream,
    byte_order: XByteOrder,
    window: u32,
) {
    use std::io::Write;

    stream
        .write_all(&resource_request(byte_order, 14, window))
        .unwrap();
    expect_x_reply(&read_x_reply(stream, byte_order), byte_order);
}

/// Asserts a record is a reply, and says what it actually was when it is not.
///
/// An X error is a 32-byte record whose first byte is zero, and every field that
/// identifies it -- which error, which resource, which request -- is in the bytes
/// after that. `assert_eq!(record[0], 1)` throws all of it away and reports
/// `left: 0, right: 1`, which says a record was not a reply without saying
/// anything about why.
#[cfg(unix)]
fn expect_x_reply(record: &[u8], byte_order: XByteOrder) {
    assert!(
        record[0] != 0,
        "expected a reply, got X error code {} for resource {:#010x} \
(major opcode {}, minor {}, sequence {})",
        record[1],
        read_u32(byte_order, &record[4..8]),
        record[10],
        read_u16(byte_order, &record[8..10]),
        read_u16(byte_order, &record[2..4]),
    );
    assert_eq!(record[0], 1, "expected a reply, got record type");
}

/// Reads one record of any class off the wire.
///
/// Every X record is 32 bytes, and exactly two classes carry more: a reply
/// (`0`-indexed byte zero of `1`) and a GenericEvent (`35`), which share the
/// extended length at bytes 4..8. Errors and core events do not -- those four
/// bytes are payload. Reading them as a length is how a 32-byte core event turned
/// into a demand for eight megabytes that never arrived.
///
/// So the class is read from the wire rather than assumed by the caller. A test
/// that receives a record it did not expect now fails on its own assertion about
/// `record[0]`, with the stream still framed, instead of desynchronising and
/// failing somewhere unrelated later.
#[cfg(unix)]
fn read_x_reply(stream: &mut std::os::unix::net::UnixStream,
    byte_order: XByteOrder) -> Vec<u8> {
    let mut record = vec![0; 32];
    fill_from_socket(stream, &mut record);
    // Bit 7 marks an event as sent by SendEvent and is not part of the class.
    let extended = matches!(record[0], 1) || record[0] & 0x7f == 35;
    if !extended {
        return record;
    }
    let body_len = usize::try_from(read_u32(byte_order, &record[4..8])).unwrap() * 4;
    record.resize(32 + body_len, 0);
    fill_from_socket(stream, &mut record[32..]);
    record
}
