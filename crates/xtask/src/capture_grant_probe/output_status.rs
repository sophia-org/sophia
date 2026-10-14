//! Read-only status observations for an attended output-recovery window.
use super::*;

pub fn run(args: &[String], mut report: impl FnMut(&str)) -> Result<()> {
    let [seconds] = args else {
        return Err("output-status-probe SECONDS (1..=180)".into());
    };
    let seconds: u64 = seconds.parse()?;
    if !(1..=180).contains(&seconds) {
        return Err("duration must be 1..=180 seconds".into());
    }
    let mut admitted = authenticate(
        "roles/status",
        b"inspection",
        Instant::now() + Duration::from_secs(10),
    )?;
    let start = Instant::now();
    let until = start + Duration::from_secs(seconds);
    let mut previous = String::new();
    let mut last = String::new();
    let mut samples = 0;
    while Instant::now() < until {
        let deadline = Instant::now() + Duration::from_secs(2);
        let fid = open(
            &mut admitted.role,
            admitted.root,
            b"output-status",
            0,
            deadline,
        )?;
        let tag = admitted.role.read(fid, 0, 2049)?;
        let Reply::Read(bytes) = reply(&mut admitted.role, tag, deadline)? else {
            return Err("output status read refused".into());
        };
        if bytes.len() > 2048 {
            return Err("output status exceeds observation bound".into());
        }
        let tag = admitted.role.read(fid, bytes.len() as u64, 1)?;
        if !matches!(reply(&mut admitted.role, tag, deadline)?, Reply::Read(tail) if tail.is_empty())
        {
            return Err("output status has trailing bytes".into());
        }
        close(&mut admitted.role, fid, deadline)?;
        let text = std::str::from_utf8(&bytes)?;
        if !text.starts_with("sophia-output-status version=1 ")
            || !text.ends_with(" capture_authority=none\n")
            || text
                .trim_end_matches('\n')
                .bytes()
                .any(|b| b.is_ascii_control())
        {
            return Err("invalid output status envelope".into());
        }
        // Retirement counts change continuously. Emit structural changes plus
        // the final full observation rather than flooding the launch journal.
        let key = text
            .split_whitespace()
            .filter(|field| !field.starts_with("owner_retirements="))
            .collect::<Vec<_>>()
            .join(" ");
        last = format!(
            "output_status_probe elapsed_msec={} {}",
            start.elapsed().as_millis(),
            text.trim_end()
        );
        if key != previous {
            report(&last);
            previous = key;
        }
        samples += 1;
        std::thread::sleep(Duration::from_millis(100));
    }
    report(&last);
    report(&format!(
        "output_status_probe samples={samples} result=observed"
    ));
    Ok(())
}
