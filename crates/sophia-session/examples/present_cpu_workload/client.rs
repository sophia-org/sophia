use super::{Result, now_usec};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        present::{self, ConnectionExt as _},
        randr::ConnectionExt as _,
        xproto::{
            ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask, Rectangle, WindowClass,
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
}
impl Client {
    pub fn new(index: usize) -> Result<Self> {
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
            if info.mode != 0 && info.width >= 704 && info.height >= 304 {
                heads.push((crtc, info.x, info.y, info.width, info.height));
            }
        }
        heads.sort();
        let (crtc, x, y, _, _) = *heads.first().ok_or("no active CRTC fits both windows")?;
        let x = x
            .checked_add(32 + index as i16 * 352)
            .ok_or("position overflow")?;
        let y = y.checked_add(32).ok_or("position overflow")?;
        let window = conn.generate_id()?;
        conn.create_window(
            depth,
            window,
            root,
            x,
            y,
            320,
            240,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(0x224466)
                .event_mask(EventMask::STRUCTURE_NOTIFY),
        )?
        .check()?;
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
            conn.create_pixmap(depth, pixmap, window, 320, 240)?
                .check()?;
            conn.change_gc(
                gc,
                &x11rb::protocol::xproto::ChangeGCAux::new().foreground(if i % 2 == 0 {
                    0x224466
                } else {
                    0x668822
                }),
            )?;
            conn.poly_fill_rectangle(
                pixmap,
                gc,
                &[Rectangle {
                    x: 0,
                    y: 0,
                    width: 320,
                    height: 240,
                }],
            )?;
            conn.change_gc(
                gc,
                &x11rb::protocol::xproto::ChangeGCAux::new().foreground(0xf0d060),
            )?;
            conn.poly_fill_rectangle(
                pixmap,
                gc,
                &[Rectangle {
                    x: 40,
                    y: 40,
                    width: 120,
                    height: 120,
                }],
            )?;
            free.push_back(pixmap);
        }
        // A visible pattern also satisfies the Session's startup pixel-readiness check.
        conn.poly_fill_rectangle(
            window,
            gc,
            &[Rectangle {
                x: 40,
                y: 40,
                width: 120,
                height: 120,
            }],
        )?;
        conn.free_gc(gc)?;
        conn.map_window(window)?.check()?;
        conn.flush()?;
        Ok(Self {
            conn,
            window,
            initial: json!({"window": window, "crtc": crtc, "x": x, "y": y,
                "width": 320, "height": 240, "depth": depth, "root": root}),
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
        let Some(pixmap) = self.free.pop_front() else {
            if measured {
                self.starvation += 1;
            }
            return Ok(());
        };
        self.serial = self.serial.checked_add(1).ok_or("serial exhausted")?;
        let sent = now_usec();
        self.conn.present_pixmap(
            self.window,
            pixmap,
            self.serial,
            0u32,
            0u32,
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
            "offered": self.offered, "sent": self.sent, "completed": self.completed,
            "idle": self.idle, "starvation": self.starvation, "late_slots": self.late_slots,
            "max_lateness_usec": self.max_lateness_usec, "msc_regressions": self.regressions,
            "unexpected_events": self.unexpected, "modes_copy_flip_skip_suboptimal": self.modes,
            "send_to_complete_usec": self.latencies, "complete_ust_msc": self.clock_pairs,
            "outstanding_at_exit": self.pending.len()})
    }
}
