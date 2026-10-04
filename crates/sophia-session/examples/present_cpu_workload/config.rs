use super::Result;
use serde_json::{Value, json};

pub struct Config {
    pub mode: String,
    pub target: String,
    pub rate: u64,
    pub clients: usize,
    pub grace: u64,
    pub seconds: u64,
    pub output: String,
    pub sample_pid: Option<u32>,
    pub guest_process_accounting: bool,
}
impl Config {
    pub fn parse() -> Result<Self> {
        let mut c = Self {
            mode: "open".into(),
            target: "zero".into(),
            rate: 60,
            clients: 2,
            grace: 10,
            seconds: 60,
            output: String::new(),
            sample_pid: None,
            guest_process_accounting: false,
        };
        for arg in std::env::args().skip(1) {
            let (key, value) = arg.split_once('=').ok_or("arguments require --key=value")?;
            match key {
                "--mode" => c.mode = value.into(),
                "--target" => c.target = value.into(),
                "--rate" => c.rate = value.parse()?,
                "--clients" => c.clients = value.parse()?,
                "--grace" => c.grace = value.parse()?,
                "--seconds" => c.seconds = value.parse()?,
                "--output" => c.output = value.into(),
                "--guest-process-accounting" => c.guest_process_accounting = value.parse()?,
                "--sample-pid" => {
                    c.sample_pid = Some(if value == "parent" {
                        rustix::process::getppid()
                            .ok_or("no parent process")?
                            .as_raw_nonzero()
                            .get() as u32
                    } else {
                        value.parse()?
                    })
                }
                _ => return Err(format!("unknown argument {key}").into()),
            }
        }
        if !matches!(c.mode.as_str(), "open" | "closed")
            || !matches!(c.target.as_str(), "zero" | "next")
            || !(1..=240).contains(&c.rate)
            || !(1..=2).contains(&c.clients)
            || !(1..=120).contains(&c.seconds)
            || c.grace > 30
            || c.output.is_empty()
        {
            return Err("invalid workload bounds or missing --output".into());
        }
        Ok(c)
    }
    pub fn json(&self) -> Value {
        json!({"mode": self.mode, "target": self.target, "rate_per_window": self.rate,
            "clients": self.clients, "grace_seconds": self.grace, "seconds": self.seconds,
            "buffer_kind": "core_pixmap_cpu", "pool_per_window": 8, "options": 0,
            "divisor": 0, "remainder": 0, "sample_pid": self.sample_pid,
            "guest_process_accounting": self.guest_process_accounting})
    }
}
