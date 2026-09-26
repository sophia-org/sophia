# sophia_control_v1 wire tables

Generated from `protocol/sophia-control-v1.kdl`; do not edit.

Experimental major 1, revision 2 (revision 1 remains negotiable). [Normative semantics](../sophia-control-v1.md).

All offsets are payload-relative; integers are little endian, with no alignment padding.

- owner `policy` = 1

- owner `session` = 2

- completion `policy-commit` = 1

- completion `session-settlement` = 2

- outcome `committed` = 1

- outcome `completed` = 2

- outcome `unchanged` = 3

- outcome `rejected` = 4

- outcome `stale` = 5

- outcome `denied` = 6

- outcome `unavailable` = 7

- outcome `overloaded` = 8

- outcome `timed-out` = 9

- outcome `indeterminate` = 10

- error `malformed` = 1

- error `sequence` = 2

- error `revision` = 3

- error `features` = 4

## ClientHello

Kind 128; client-to-session; transaction `zero`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `minimum_revision` | u16 (2 bytes) |
| 2 | `maximum_revision` | u16 (2 bytes) |
| 4 | `required_features` | u64 (8 bytes) |

Payload size: 12 bytes.

## ServerWelcome

Kind 129; session-to-client; transaction `zero`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `selected_revision` | u16 (2 bytes) |
| 2 | `reserved` | u16 (2 bytes); must be zero |
| 4 | `session_id_low` | u64 (8 bytes) |
| 12 | `session_id_high` | u64 (8 bytes) |
| 20 | `connection_id` | u64 (8 bytes) |
| 28 | `features` | u64 (8 bytes) |
| 36 | `max_payload` | u32 (4 bytes) |
| 40 | `max_commands` | u16 (2 bytes) |
| 42 | `max_name_bytes` | u16 (2 bytes) |
| 44 | `command_timeout_ms` | u32 (4 bytes) |
| 48 | `frame_timeout_ms` | u32 (4 bytes) |
| 52 | `idle_timeout_ms` | u32 (4 bytes) |

Payload size: 56 bytes.

## CommandsRequest

Kind 130; client-to-session; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |

Payload size: 0 bytes.

## CommandsReply

Kind 131; session-to-client; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `catalog_generation` | u64 (8 bytes) |
| 8 | `entry_count` | u16 (2 bytes) |
| 10 | `reserved` | u16 (2 bytes); must be zero |
| 12 | `entries` | `entry_count` entries, max 259 |

Entry offsets:

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `owner` | u16 (2 bytes) |
| 2 | `completion` | u16 (2 bytes) |
| 4 | `name_len` | u16 (2 bytes) |
| 6 | `reserved` | u16 (2 bytes); must be zero |
| 8 | `name` | u8 (128 bytes) |

Payload size: 12 + 136 × `entry_count` bytes.

## Invoke

Kind 132; client-to-session; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `catalog_generation` | u64 (8 bytes) |
| 8 | `owner` | u16 (2 bytes) |
| 10 | `name_len` | u16 (2 bytes) |
| 12 | `name` | u8 (128 bytes) |

Payload size: 140 bytes.

## CommandOutcome

Kind 133; session-to-client; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `catalog_generation` | u64 (8 bytes) |
| 8 | `outcome` | u16 (2 bytes) |
| 10 | `detail_len` | u16 (2 bytes) |
| 12 | `detail` | u8 (256 bytes) |

Payload size: 268 bytes.

## ProtocolError

Kind 134; session-to-client; transaction `zero-or-offending`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `error` | u16 (2 bytes) |
| 2 | `reserved` | u16 (2 bytes); must be zero |

Payload size: 4 bytes.
