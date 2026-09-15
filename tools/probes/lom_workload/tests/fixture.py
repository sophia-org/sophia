"""Synthetic complete causal transcript; no application, GPU or socket execution."""

from records import HOST, CLIENT, SHUTDOWN_INVENTORY


def transcript():
    host = {name: [] for name in HOST}
    client = {name: [] for name in CLIENT}
    active = {1: 0, 2: 0}
    current = {}
    generation = 1

    def publication(revision):
        for output in (1, 2):
            for slot in (0, 1):
                host["sophia_shell_indicator_state"].append(dict(
                    connection_epoch=1, indicator_generation=revision, output=output,
                    indicator=slot + 1, action=10 * output + slot, slot=slot,
                    state_bits=int(slot == active[output]), entries=4,
                ))

    def presentation(output, revision, when):
        nonlocal generation
        candidate = dict(connection_epoch=1, content_grant_epoch=2, output=output,
                         candidate_generation=generation, presentation_epoch=100 + generation,
                         indicator_generation=revision, width=100, height=24, bytes=9600,
                         checksum="0123456789abcdef", status="presented")
        client["lom_panel_candidate"].append(candidate)
        current[output] = candidate
        host["sophia_shell_native_binding"].append(dict(
            connection_epoch=1, content_grant_epoch=2, output=output,
            candidate_generation=generation, native_owner=1, native_frame=generation,
            head=output, target_generation=1, heads=1, mode_refresh_millihz=60000,
        ))
        host["sophia_shell_native_completion"].append(dict(
            output=output, native_owner=1, native_frame=generation, heads=1,
            monotonic_usec=when, timestamp_source="kernel", missing_kernel_timestamp="0",
        ))
        generation += 1

    publication(1)
    for output in (1, 2):
        presentation(output, 1, 1_000_000)
    for index in range(40):
        output = 1 + index % 2
        target = 1 - active[output]
        action = 10 * output + target
        when = 11_000_000 + index * 1_400_000
        old = current[output]
        issue = dict(connection_epoch=1, content_grant_epoch=2, event_id=index + 1,
                     output=output, candidate_generation=old["candidate_generation"],
                     presentation_epoch=old["presentation_epoch"], target_id=target + 1,
                     target_generation=1, action=action, monotonic_usec=when,
                     status="issued", disposition="0")
        host["sophia_shell_action_receipt"].append(issue)
        host["sophia_shell_action_receipt"].append({
            **issue, "status": "acknowledged", "disposition": "1", "monotonic_usec": when + 10_000,
        })
        host["sophia_shell_action_cause"].append(dict(
            connection_epoch=1, event_id=index + 1, output=output, action=action,
            activation_serial=index + 100, policy_connection_epoch=7, admission="Admitted",
        ))
        host["sophia_shell_action_policy"].append(dict(
            policy_connection_epoch=7, activation_serial=index + 100, action=action,
            transaction=index + 200, request_id=index + 300,
            indicator_generation=index + 2, outcome="Committed",
        ))
        active[output] = target
        publication(index + 2)
        for output in (1, 2):
            presentation(output, index + 2, when + 50_000)
    for output in (1, 2):
        presentation(output, 41, 72_000_000)
    host["sophia_shell_content_shutdown"].append(dict(
        connection_epoch=1, content_grant_epoch=2, monotonic_usec=90_000_000,
        settled_candidates=1, status="quiescent", workers_joined="1",
        **dict.fromkeys(SHUTDOWN_INVENTORY.split(), 0),
    ))
    return host, client


def encode(records):
    return "\n".join(
        f"{name} schema=1 " + " ".join(f"{field}={value}" for field, value in row.items())
        for name, rows in records.items() for row in rows
    ) + "\n"


def limits():
    # Test values only; not a recovered or approved operator workload budget.
    return dict(warmup_usec=10_000_000, duration_usec=60_000_000, actions_per_output=20,
                output_count=2, ack_p95_usec=50_000, ack_max_usec=100_000,
                native_p95_usec=150_000, native_max_usec=300_000)
