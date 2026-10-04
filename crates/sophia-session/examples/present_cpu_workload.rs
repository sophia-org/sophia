//! Generic core-pixmap Present workload. Run as a new test Session's startup app.
//! Open mode has an absolute offered schedule; closed mode waits for Complete
//! AND Idle, with no rate cap. Outputs raw samples for independent analysis.
#[path = "present_cpu_workload/client.rs"]
mod client;
#[path = "present_cpu_workload/config.rs"]
mod config;
#[path = "present_cpu_workload/proc_sample.rs"]
mod proc_sample;
#[cfg(test)]
#[path = "../tests/support/present_cpu_workload.rs"]
mod tests;
use client::Client;
use config::Config;
use serde_json::json;
use std::{error::Error, fs, time::Duration};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn now_usec() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    t.tv_sec as u64 * 1_000_000 + t.tv_nsec as u64 / 1_000
}
fn wait(clients: &[Client], deadline: u64) -> Result<()> {
    let mut fds: Vec<_> = clients
        .iter()
        .map(|c| rustix::event::PollFd::new(c.conn.stream(), rustix::event::PollFlags::IN))
        .collect();
    let timeout = rustix::time::Timespec::try_from(Duration::from_micros(
        deadline.saturating_sub(now_usec()),
    ))?;
    match rustix::event::poll(&mut fds, Some(&timeout)) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
fn phase(clients: &mut [Client], c: &Config, seconds: u64, measured: bool) -> Result<(u64, u64)> {
    let start = now_usec();
    let end = start + seconds * 1_000_000;
    let mut slot = 0;
    loop {
        for client in &mut *clients {
            client.drain()?;
        }
        let now = now_usec();
        if now >= end {
            break;
        }
        if c.mode == "open" {
            // Advance by slot index, never by completion or the time of the last send.
            let due = start + slot * 1_000_000 / c.rate;
            if now >= due {
                for client in &mut *clients {
                    client.offer(measured, c.target == "next", now - due, 1_000_000 / c.rate)?;
                }
                slot += 1;
                continue;
            }
            wait(clients, due.min(end))?;
        } else {
            for client in &mut *clients {
                if !client.outstanding() {
                    client.offer(measured, c.target == "next", 0, 1)?;
                }
                if client.sent > 500_000 {
                    return Err("workload sample bound exceeded".into());
                }
            }
            wait(clients, end)?;
        }
    }
    Ok((start, now_usec()))
}
fn drain(clients: &mut [Client]) -> Result<()> {
    let deadline = now_usec() + 5_000_000;
    loop {
        for client in &mut *clients {
            client.drain()?;
        }
        if clients.iter().all(|c| !c.outstanding()) {
            return Ok(());
        }
        if now_usec() >= deadline {
            return Err("Complete/Idle drain exceeded five seconds".into());
        }
        wait(clients, deadline)?;
    }
}
fn run(c: &Config) -> Result<serde_json::Value> {
    let mut clients: Vec<_> = (0..c.clients)
        .map(|index| Client::new(index, c))
        .collect::<Result<_>>()?;
    // Check every source pixmap and one reuse before grace, outside CPU sampling.
    // Window GetImage reads core backing, not the separate Present raster;
    // renderer-payload equivalence is checked by present_damage_accounting.
    for _ in 0..9 {
        for client in &mut clients {
            client.offer(false, false, 0, 1)?;
        }
        drain(&mut clients)?;
        for client in &mut clients {
            client.verify_pixels()?;
        }
    }
    phase(&mut clients, c, c.grace, false)?;
    drain(&mut clients)?;
    for client in &mut clients {
        client.prime_measurement()?;
    }
    drain(&mut clients)?;
    let geometry_before = clients
        .iter()
        .map(Client::geometry)
        .collect::<Result<Vec<_>>>()?;
    let before = proc_sample::snapshot(c.sample_pid, c.guest_process_accounting)?;
    let (start, end) = phase(&mut clients, c, c.seconds, true)?;
    let after = proc_sample::snapshot(c.sample_pid, c.guest_process_accounting)?;
    let completed_at_end: Vec<_> = clients.iter().map(|c| c.completed).collect();
    drain(&mut clients)?;
    let windows = clients
        .iter()
        .zip(geometry_before)
        .map(|(client, before)| Ok(client.report(before, client.geometry()?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"schema": 2, "status": "complete", "config": c.json(),
        "measure_start_usec": start, "measure_end_usec": end,
        "proc_before": before, "proc_after": after, "windows": windows,
        "completed_at_measure_end": completed_at_end,
        "expected_offers_per_window": if c.mode == "open" { Some(c.seconds*c.rate) } else { None },
        "buffer_note": "core pixmaps; software Present, no DRI3 or DMA-BUF client buffers",
        "latency_note": "client send through receipt of Complete; includes socket delivery"}),
    )
}
fn main() {
    let result = Config::parse().and_then(|c| {
        // Create-new avoids overwriting a prior run, including a failed attempt.
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&c.output)?;
        let result = run(&c);
        let report = match &result {
            Ok(report) => report.clone(),
            Err(error) => {
                json!({"schema":1,"status":"failed","error":error.to_string(),"config":c.json()})
            }
        };
        serde_json::to_writer(&mut file, &report)?;
        result.map(|_| ())
    });
    match result {
        Ok(()) => println!("sophia_present_cpu schema=1 status=complete"),
        Err(e) => {
            eprintln!("present_cpu_workload: {e}");
            std::process::exit(1);
        }
    }
}
