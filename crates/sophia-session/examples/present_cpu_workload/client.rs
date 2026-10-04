use super::{Result, config::Config, now_usec};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        present::{self, ConnectionExt as _},
        randr::ConnectionExt as _,
        xfixes::ConnectionExt as _,
        xproto::{
            ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, ImageFormat, Rectangle,
            WindowClass,
        },
    },
    rust_connection::RustConnection,
};

pub struct Pending {
    pixmap: u32,
    sent: u64,
    measured: bool,
    complete: bool,
    idle: bool,
}
pub struct Client {
    pub conn: RustConnection,
    pub window: u32,
    pub initial: Value,
    pub free: VecDeque<u32>,
    pending: BTreeMap<u32, Pending>,
    serial: u32,
    pub last_msc: u64,
    pub offered: u64,
    pub sent: u64,
    pub completed: u64,
    pub idle: u64,
    pub starvation: u64,
    pub late_slots: u64,
    pub max_lateness_usec: u64,
    pub regressions: u64,
    pub unexpected: u64,
    pub modes: [u64; 4],
    pub latencies: Vec<u64>,
    pub clock_pairs: Vec<(u64, u64)>,
    update_region: u32,
    last_pixmap: Option<u32>,
    pixel_checks: u64,
    pool: Vec<u32>,
    next_pixmap: usize,
    last_parity: Option<usize>,
    unchanged_presents: u64,
}
impl Client {
    pub fn new(index: usize, config: &Config) -> Result<Self> {
        let (conn, screen_index) = x11rb::connect(None)?;
        let screen = &conn.setup().roots[screen_index];
        let root = screen.root;
        let depth = screen.root_depth;
        let visual = screen.root_visual;
        let version = conn.present_query_version(1, 2)?.reply()?;
        if version.major_version < 1 {
            return Err("Present unavailable".into());
        }
        let resources = conn.randr_get_screen_resources_current(root)?.reply()?;
        let mut heads = Vec::new();
        for crtc in resources.crtcs {
            let info = conn
                .randr_get_crtc_info(crtc, resources.config_timestamp)?
                .reply()?;
            if info.mode != 0 {
                heads.push((crtc, info.x, info.y, info.width, info.height));
            }
        }
        heads.sort();
        let (crtc, head_x, head_y, head_width, head_height) = *heads
            .iter()
            .find(|h| h.3 >= 704 && h.4 >= 304)
            .ok_or("no active CRTC fits the workload")?;
        let (width, height, inset) = if config.size == "head" {
            (head_width - 32, head_height - 32, 16)
        } else {
            (320, 240, 32)
        };
        // Keep the eight-pixmap pool bounded even on unusually large desktops.
        if u64::from(width) * u64::from(height) > 4096 * 4096 {
            return Err("workload exceeds 4096x4096 pixel bound".into());
        }
        let x = head_x;
        let y = head_y;
        let x = x
            .checked_add(inset + index as i16 * 352)
            .ok_or("position overflow")?;
        let y = y.checked_add(inset).ok_or("position overflow")?;
        let patch = Rectangle {
            x: 40,
            y: 40,
            width: 120,
            height: 120,
        };
        let full = Rectangle {
            x: 0,
            y: 0,
            width,
            height,
        };
        let window = conn.generate_id()?;
        conn.create_window(
            depth,
            window,
            root,
            x,
            y,
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(0x224466)
                .event_mask(EventMask::STRUCTURE_NOTIFY),
        )?
        .check()?;
        let update_region = if config.damage == "absent" {
            0
        } else {
            conn.xfixes_query_version(5, 0)?.reply()?;
            let region = conn.generate_id()?;
            conn.xfixes_create_region(
                region,
                &[if config.damage == "patch" {
                    patch
                } else {
                    full
                }],
            )?
            .check()?;
            region
        };
        let eid = conn.generate_id()?;
        conn.present_select_input(
            eid,
            window,
            present::EventMask::COMPLETE_NOTIFY | present::EventMask::IDLE_NOTIFY,
        )?
        .check()?;
        let gc = conn.generate_id()?;
        conn.create_gc(gc, window, &CreateGCAux::new())?.check()?;
        let mut free = VecDeque::new();
        for i in 0..8 {
            let pixmap = conn.generate_id()?;
            conn.create_pixmap(depth, pixmap, window, width, height)?
                .check()?;
            conn.change_gc(
                gc,
                &x11rb::protocol::xproto::ChangeGCAux::new().foreground(0x224466),
            )?;
            conn.poly_fill_rectangle(pixmap, gc, &[full])?;
            conn.change_gc(
                gc,
                &x11rb::protocol::xproto::ChangeGCAux::new().foreground(if i % 2 == 0 {
                    0xf0d060
                } else {
                    0x668822
                }),
            )?;
            conn.poly_fill_rectangle(pixmap, gc, &[patch])?;
            free.push_back(pixmap);
        }
        // A visible pattern also satisfies the Session's startup pixel-readiness check.
        conn.poly_fill_rectangle(window, gc, &[patch])?;
        conn.free_gc(gc)?;
        conn.map_window(window)?.check()?;
        conn.flush()?;
        let pool = free.iter().copied().collect();
        Ok(Self {
            conn,
            window,
            initial: json!({"window": window, "crtc": crtc, "x": x, "y": y,
                "width": width, "height": height, "depth": depth, "root": root,
                "head": {"x": head_x, "y": head_y, "width": head_width, "height": head_height},
                "output_layout": heads.iter().map(|h| json!({"x": h.1, "y": h.2,
                    "width": h.3, "height": h.4})).collect::<Vec<_>>(),
                "patch": {"x": patch.x, "y": patch.y, "width": patch.width, "height": patch.height}}),
            free,
            pending: BTreeMap::new(),
            serial: 0,
            last_msc: 0,
            offered: 0,
            sent: 0,
            completed: 0,
            idle: 0,
            starvation: 0,
            late_slots: 0,
            max_lateness_usec: 0,
            regressions: 0,
            unexpected: 0,
            modes: [0; 4],
            latencies: Vec::new(),
            clock_pairs: Vec::new(),
            update_region,
            last_pixmap: None,
            pixel_checks: 0,
            pool,
            next_pixmap: 0,
            last_parity: None,
            unchanged_presents: 0,
        })
    }
    pub fn geometry(&self) -> Result<Value> {
        let root = self.initial["root"].as_u64().unwrap() as u32;
        let g = self.conn.get_geometry(self.window)?.reply()?;
        let p = self
            .conn
            .translate_coordinates(self.window, root, 0, 0)?
            .reply()?;
        Ok(json!({"x": p.dst_x, "y": p.dst_y, "width": g.width, "height": g.height}))
    }
    pub fn outstanding(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn prime_measurement(&mut self) -> Result<()> {
        if self.outstanding() || self.free.len() != self.pool.len() {
            return Err("pixel sequence reset requires every pixmap to be idle".into());
        }
        // End grace with green; every arm then starts measurement with yellow.
        self.next_pixmap = self.pool.len() - 1;
        self.offer(false, false, 0, 1)
    }
    pub fn verify_pixels(&mut self) -> Result<()> {
        if self.outstanding() {
            return Err("pixel check requires Complete and Idle".into());
        }
        let pixmap = self.last_pixmap.ok_or("no presented pixmap to check")?;
        let width = self.initial["width"].as_u64().unwrap() as u16;
        let height = self.initial["height"].as_u64().unwrap() as u16;
        let image = self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                pixmap,
                0,
                0,
                width,
                height,
                0x00ff_ffff,
            )?
            .reply()?;
        let format = self
            .conn
            .setup()
            .pixmap_formats
            .iter()
            .find(|f| f.depth == image.depth);
        if image.depth != 24
            || format.is_none_or(|f| f.bits_per_pixel != 32)
            || image.data.len() != usize::from(width) * usize::from(height) * 4
        {
            return Err("source pixel check requires packed depth-24 RGB".into());
        }
        let little =
            self.conn.setup().image_byte_order == x11rb::protocol::xproto::ImageOrder::LSB_FIRST;
        for (index, pixel) in image.data.chunks_exact(4).enumerate() {
            let bytes = pixel.try_into().unwrap();
            let actual = if little {
                u32::from_le_bytes(bytes)
            } else {
                u32::from_be_bytes(bytes)
            };
            let x = index % usize::from(width);
            let y = index / usize::from(width);
            let expected = if (40..160).contains(&x) && (40..160).contains(&y) {
                if self.last_parity == Some(0) {
                    0xf0d060
                } else {
                    0x668822
                }
            } else {
                0x224466
            };
            if actual & 0x00ff_ffff != expected {
                return Err(format!(
                    "source pixel mismatch at {x},{y}: {actual:08x} != {expected:08x}"
                )
                .into());
            }
        }
        self.pixel_checks += 1;
        Ok(())
    }
    pub fn offer(
        &mut self,
        measured: bool,
        target_next: bool,
        lateness: u64,
        period: u64,
    ) -> Result<()> {
        if measured {
            self.offered += 1;
            self.late_slots += u64::from(lateness >= period);
            self.max_lateness_usec = self.max_lateness_usec.max(lateness);
        }
        let pixmap = self.pool[self.next_pixmap];
        let Some(free_index) = self.free.iter().position(|p| *p == pixmap) else {
            if measured {
                self.starvation += 1;
            }
            return Ok(());
        };
        self.free.remove(free_index);
        let parity = self.next_pixmap % 2;
        if measured && self.last_parity == Some(parity) {
            self.unchanged_presents += 1;
        }
        self.last_parity = Some(parity);
        self.next_pixmap = (self.next_pixmap + 1) % self.pool.len();
        self.serial = self.serial.checked_add(1).ok_or("serial exhausted")?;
        let sent = now_usec();
        self.conn.present_pixmap(
            self.window,
            pixmap,
            self.serial,
            0u32,
            // The first full Present initializes the window independently of
            // exposure/background semantics. Every later pixmap is identical
            // outside the patch, so all three damage descriptions are truthful.
            if self.serial == 1 {
                0
            } else {
                self.update_region
            },
            0,
            0,
            0u32,
            0u32,
            0u32,
            0,
            if target_next { self.last_msc + 1 } else { 0 },
            0,
            0,
            &[],
        )?;
        self.conn.flush()?;
        self.last_pixmap = Some(pixmap);
        self.pending.insert(
            self.serial,
            Pending {
                pixmap,
                sent,
                measured,
                complete: false,
                idle: false,
            },
        );
        if measured {
            self.sent += 1;
        }
        Ok(())
    }
    pub fn drain(&mut self) -> Result<()> {
        while let Some(event) = self.conn.poll_for_event()? {
            match event {
                Event::Error(error) => return Err(format!("X protocol error {error:?}").into()),
                Event::PresentCompleteNotify(event) if event.window == self.window => {
                    let Some(p) = self.pending.get_mut(&event.serial) else {
                        self.unexpected += 1;
                        continue;
                    };
                    if p.complete {
                        self.unexpected += 1;
                        continue;
                    }
                    p.complete = true;
                    if p.measured {
                        self.completed += 1;
                        self.regressions += u64::from(event.msc < self.last_msc);
                        self.latencies.push(now_usec().saturating_sub(p.sent));
                        self.clock_pairs.push((event.ust, event.msc));
                        let mode: u8 = event.mode.into();
                        if let Some(count) = self.modes.get_mut(usize::from(mode)) {
                            *count += 1;
                        } else {
                            self.unexpected += 1;
                        }
                    }
                    self.last_msc = event.msc;
                }
                Event::PresentIdleNotify(event) if event.window == self.window => {
                    let Some(p) = self.pending.get_mut(&event.serial) else {
                        self.unexpected += 1;
                        continue;
                    };
                    if p.idle || p.pixmap != event.pixmap {
                        self.unexpected += 1;
                        continue;
                    }
                    p.idle = true;
                    if p.measured {
                        self.idle += 1;
                    }
                    // Conservative reuse: this workload waits for both events.
                }
                _ => {}
            }
        }
        self.pending.retain(|_, p| {
            if p.complete && p.idle {
                self.free.push_back(p.pixmap);
                false
            } else {
                true
            }
        });
        Ok(())
    }
    pub fn report(&self, before: Value, after: Value) -> Value {
        json!({"initial": self.initial, "before": before, "after": after,
            "source_pixel_checks_before_grace": self.pixel_checks,
            "unchanged_presents": self.unchanged_presents,
            "offered": self.offered, "sent": self.sent, "completed": self.completed,
            "idle": self.idle, "starvation": self.starvation, "late_slots": self.late_slots,
            "max_lateness_usec": self.max_lateness_usec, "msc_regressions": self.regressions,
            "unexpected_events": self.unexpected, "modes_copy_flip_skip_suboptimal": self.modes,
            "send_to_complete_usec": self.latencies, "complete_ust_msc": self.clock_pairs,
            "outstanding_at_exit": self.pending.len()})
    }
}
