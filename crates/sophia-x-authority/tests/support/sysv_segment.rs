use std::process::Command;

pub struct Segment(pub u32);

impl Segment {
    pub fn new(value: u8) -> Self {
        let output = Command::new("ipcmk")
            .args(["-M", "4096"])
            .env("LC_ALL", "C")
            .output()
            .expect("util-linux ipcmk creates the SysV fixture");
        assert!(output.status.success(), "{output:?}");
        let id = String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .last()
            .unwrap()
            .parse()
            .unwrap();
        let segment = Self(id);
        sophia_sysv_shm::write_bytes(id, 0, &[value; 16]).unwrap();
        segment
    }
}

impl Drop for Segment {
    fn drop(&mut self) {
        let _ = Command::new("ipcrm")
            .args(["-m", &self.0.to_string()])
            .status();
    }
}
