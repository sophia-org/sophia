//! The instrumented comparison executable cannot become an unbounded desktop.
pub(crate) fn require_bounded_run(args: &[String]) -> Result<(), &'static str> {
    let mut values = args
        .iter()
        .filter_map(|arg| arg.strip_prefix("--max-runtime-ms="));
    let valid = values.next().is_some_and(|value| {
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && value
                .parse::<u64>()
                .is_ok_and(|v| (1..=60_000).contains(&v))
    });
    if !valid || values.next().is_some() {
        return Err("comparison counters require exactly one --max-runtime-ms=1..60000");
    }
    Ok(())
}
