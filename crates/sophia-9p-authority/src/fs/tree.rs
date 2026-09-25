//! Synthetic directory tree and fid tracking.

use std::collections::HashMap;

use sophia_protocol::geometry::Size;
use sophia_protocol::ids::SurfaceId;

use super::node::NodeKind;
use crate::types::{Fid, OpenMode, Qid};

#[derive(Clone, Debug)]
pub struct WindowState {
    pub surface_id: SurfaceId,
    pub size: Size,
    pub title: String,
    pub buffer: Vec<u8>,
    pub mouse_queue: Vec<u8>,
    pub kbd_queue: Vec<u8>,
    pub version: u32,
}

impl WindowState {
    pub fn new(surface_id: SurfaceId) -> Self {
        let default_size = Size {
            width: 800,
            height: 600,
        };
        let buffer_len = (default_size.width * default_size.height * 4) as usize;
        Self {
            surface_id,
            size: default_size,
            title: String::new(),
            buffer: vec![0u8; buffer_len],
            mouse_queue: Vec::new(),
            kbd_queue: Vec::new(),
            version: 1,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SyntheticTree {
    next_index: u32,
    windows: HashMap<u32, WindowState>,
    fids: HashMap<Fid, (NodeKind, Option<OpenMode>)>,
}

impl SyntheticTree {
    pub fn new() -> Self {
        Self {
            next_index: 1,
            windows: HashMap::new(),
            fids: HashMap::new(),
        }
    }

    pub fn attach(&mut self, fid: Fid) -> Result<Qid, String> {
        let root = NodeKind::Root;
        let qid = root.to_qid(1);
        self.fids.insert(fid, (root, None));
        Ok(qid)
    }

    pub fn allocate_window(&mut self) -> SurfaceId {
        let index = self.next_index;
        self.next_index += 1;
        let surface_id = SurfaceId::new(index, 1);
        self.windows.insert(index, WindowState::new(surface_id));
        surface_id
    }

    pub fn get_window(&self, index: u32) -> Option<&WindowState> {
        self.windows.get(&index)
    }

    pub fn get_window_mut(&mut self, index: u32) -> Option<&mut WindowState> {
        self.windows.get_mut(&index)
    }

    pub fn walk(&mut self, fid: Fid, newfid: Fid, wnames: &[String]) -> Result<Vec<Qid>, String> {
        let (current_node, _) = self
            .fids
            .get(&fid)
            .copied()
            .ok_or_else(|| "Unknown fid".to_string())?;

        let mut current = current_node;
        let mut qids = Vec::with_capacity(wnames.len());

        for name in wnames {
            let next = match (current, name.as_str()) {
                (NodeKind::Root, "new") => NodeKind::New,
                (NodeKind::Root, id_str) => {
                    let index: u32 = id_str
                        .parse()
                        .map_err(|_| format!("Directory not found: {}", id_str))?;
                    if !self.windows.contains_key(&index) {
                        return Err(format!("Window {} does not exist", index));
                    }
                    NodeKind::WindowDir(SurfaceId::new(index, 1))
                }
                (NodeKind::WindowDir(id), "ctl") => NodeKind::Ctl(id),
                (NodeKind::WindowDir(id), "data") => NodeKind::Data(id),
                (NodeKind::WindowDir(id), "refresh") => NodeKind::Refresh(id),
                (NodeKind::WindowDir(id), "mouse") => NodeKind::Mouse(id),
                (NodeKind::WindowDir(id), "kbd") => NodeKind::Kbd(id),
                (NodeKind::WindowDir(id), "text") => NodeKind::Text(id),
                (NodeKind::WindowDir(_), "..") => NodeKind::Root,
                _ => return Err(format!("Path element not found: {}", name)),
            };
            qids.push(next.to_qid(1));
            current = next;
        }

        self.fids.insert(newfid, (current, None));
        Ok(qids)
    }

    pub fn open(&mut self, fid: Fid, mode_val: u8) -> Result<(Qid, u32), String> {
        let (node, _) = self
            .fids
            .get(&fid)
            .copied()
            .ok_or_else(|| "Unknown fid".to_string())?;

        let mode = OpenMode::from_u8(mode_val).ok_or_else(|| "Invalid open mode".to_string())?;
        self.fids.insert(fid, (node, Some(mode)));

        let qid = node.to_qid(1);
        let iounit = 8192;
        Ok((qid, iounit))
    }

    pub fn clunk(&mut self, fid: Fid) -> Result<(), String> {
        self.fids.remove(&fid);
        Ok(())
    }

    pub fn read(&mut self, fid: Fid, offset: u64, count: u32) -> Result<Vec<u8>, String> {
        let (node, _) = self
            .fids
            .get(&fid)
            .copied()
            .ok_or_else(|| "Unknown fid".to_string())?;

        match node {
            NodeKind::New => {
                let id = self.allocate_window();
                let output = format!("{}\n", id.index());
                let bytes = output.as_bytes();
                if offset as usize >= bytes.len() {
                    return Ok(Vec::new());
                }
                let end = (offset as usize + count as usize).min(bytes.len());
                Ok(bytes[offset as usize..end].to_vec())
            }
            NodeKind::Ctl(id) => {
                let win = self
                    .windows
                    .get(&id.index())
                    .ok_or_else(|| "Window not found".to_string())?;
                let output = format!(
                    "{} {} 0 0 24 100 presented\n",
                    win.size.width, win.size.height
                );
                let bytes = output.as_bytes();
                if offset as usize >= bytes.len() {
                    return Ok(Vec::new());
                }
                let end = (offset as usize + count as usize).min(bytes.len());
                Ok(bytes[offset as usize..end].to_vec())
            }
            NodeKind::Mouse(id) => {
                let win = self
                    .windows
                    .get_mut(&id.index())
                    .ok_or_else(|| "Window not found".to_string())?;
                let count = count as usize;
                let available = win.mouse_queue.len();
                let take = count.min(available);
                let data: Vec<u8> = win.mouse_queue.drain(..take).collect();
                Ok(data)
            }
            NodeKind::Kbd(id) => {
                let win = self
                    .windows
                    .get_mut(&id.index())
                    .ok_or_else(|| "Window not found".to_string())?;
                let count = count as usize;
                let available = win.kbd_queue.len();
                let take = count.min(available);
                let data: Vec<u8> = win.kbd_queue.drain(..take).collect();
                Ok(data)
            }
            _ => Ok(Vec::new()),
        }
    }

    pub fn write(&mut self, fid: Fid, offset: u64, data: &[u8]) -> Result<u32, String> {
        let (node, _) = self
            .fids
            .get(&fid)
            .copied()
            .ok_or_else(|| "Unknown fid".to_string())?;

        match node {
            NodeKind::Ctl(id) => {
                let text = std::str::from_utf8(data).map_err(|_| "Invalid UTF-8 in ctl command")?;
                let win = self
                    .windows
                    .get_mut(&id.index())
                    .ok_or_else(|| "Window not found".to_string())?;
                for line in text.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.is_empty() {
                        continue;
                    }
                    match parts[0] {
                        "size" if parts.len() >= 3 => {
                            if let (Ok(w), Ok(h)) =
                                (parts[1].parse::<i32>(), parts[2].parse::<i32>())
                            {
                                win.size = Size {
                                    width: w,
                                    height: h,
                                };
                                win.buffer.resize((w * h * 4).max(0) as usize, 0);
                            }
                        }
                        "title" if parts.len() >= 2 => {
                            win.title = parts[1..].join(" ");
                        }
                        _ => {}
                    }
                }
                Ok(data.len() as u32)
            }
            NodeKind::Data(id) => {
                let win = self
                    .windows
                    .get_mut(&id.index())
                    .ok_or_else(|| "Window not found".to_string())?;
                let start = offset as usize;
                let end = start + data.len();
                if end > win.buffer.len() {
                    win.buffer.resize(end, 0);
                }
                win.buffer[start..end].copy_from_slice(data);
                Ok(data.len() as u32)
            }
            _ => Err("File not writable".to_string()),
        }
    }
}
