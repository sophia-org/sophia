#![cfg(test)]
use super::*;

#[test]
fn counts_returned_bytes_not_requested_lengths() {
    let report = parse("1 write(3<UNIX-STREAM:[42]>, \"x\", 100) = 3\n1 read(3<UNIX-STREAM:[42]>, \"x\", 80) = 2\n1 write(3<UNIX-STREAM:[42]>, \"x\", 100) = -1 EAGAIN (Would block)\n1 write(1<pipe:[7]>, \"x\", 99) = 99\n1 +++ exited with 0 +++\n").unwrap();
    assert_eq!(report["tasks"][0]["written_bytes"], 3);
    assert_eq!(report["tasks"][0]["read_bytes"], 2);
    assert_eq!(report["tasks"][0]["calls"], 3);
}

#[test]
fn joins_interleaved_tasks_and_resumed_calls() {
    let report = parse("1 recvfrom(3<UNIX-STREAM:[42]> <unfinished ...>\n2 sendto(4<UNIX-STREAM:[43]>, \"x\", 3, 0, NULL, 0) = 3\n1 <... recvfrom resumed>, \"x\", 10, 0, NULL, NULL) = 3\n2 +++ exited with 0 +++\n1 +++ exited with 0 +++\n").unwrap();
    assert_eq!(report["tasks"][0]["read_bytes"], 3);
    assert_eq!(report["tasks"][1]["written_bytes"], 3);
}

#[test]
fn incomplete_or_ambiguous_traces_are_not_totals() {
    for trace in [
        "",
        "1 read(3<UNIX-STREAM:[1]> <unfinished ...>\n1 +++ killed by SIGKILL +++\n",
        "1 <... read resumed>, \"x\", 1) = 1\n",
        "1 read(3<UNIX-STREAM:[1]>, \"x\", 1) = 1\n",
        "1 read(3<UNIX-STREAM:[1]>, \"x\", 1) = ?\n1 +++ exited with 0 +++\n",
        "1 read(3<UNIX-STREAM:[1]> <unfinished ...>\n1 <... write resumed>) = 1\n",
    ] {
        assert!(parse(trace).is_err(), "accepted {trace}");
    }
}
