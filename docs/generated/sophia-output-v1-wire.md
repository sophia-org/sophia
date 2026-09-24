# sophia_output_v1 wire tables

Generated from `protocol/sophia-output-v1.kdl`; do not edit.

Experimental major 1 revision 1. [Normative lifecycle](../sophia-output-v1.md).

Fields occur in the listed order, packed and little endian. Offsets are relative to the
enclosing payload or record; variable terms are byte lengths, not sample offsets.

- limit `heads` = 16

- limit `groups` = 16

- limit `modes_per_head` = 128

- limit `heads_per_group` = 4

- limit `label_bytes` = 64

- capability `observe` = 1

- capability `configure` = 2

- outcome `validated` = 1

- outcome `committed` = 2

- outcome `stale` = 3

- outcome `rejected` = 4

- outcome `rolled_back` = 5

- outcome `failed` = 6

- reason `none` = 0

- reason `stale` = 1

- reason `preparation` = 2

- reason `apply` = 3

- reason `head_lost` = 4

- reason `first_presentation` = 5

- reason `rollback` = 6

- reason `invariant` = 7

- intent `validate_only` = 1

- intent `apply` = 2

- transform `normal` = 1

- transform `rotate90` = 2

- transform `rotate180` = 3

- transform `rotate270` = 4

- transform `flipped` = 5

- transform `flipped90` = 6

- transform `flipped180` = 7

- transform `flipped270` = 8

- vrr `disabled` = 1

- vrr `automatic` = 2

- vrr `always` = 3

- mapping `fit` = 1

- mapping `cover` = 2

- mapping `exact` = 3

- head-flag `connected` = 1

- head-flag `enabled` = 2

- head-flag `vrr_capable` = 4

## Mode

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `mode` | u64 |
| 8 | `width` | i32 |
| 12 | `height` | i32 |
| 16 | `refresh_millihz` | u32 |
| 20 | `preferred` | u16; enum=bool |
| 22 | `reserved` | u16; reserved=#true |

Total size: 24 bytes.

## Member

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `head` | u64 |
| 8 | `mapping` | u16; enum=mapping |
| 10 | `reserved` | u16; reserved=#true |

Total size: 12 bytes.

## Head

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `head` | u64 |
| 8 | `generation` | u64 |
| 16 | `flags` | u16; flags=head-flag |
| 18 | `transforms` | u16; flags=transform-set |
| 20 | `current_mode` | u64; zero=none |
| 28 | `label_len` | u16; max=64 |
| 30 | `mode_count` | u16; max=128 |
| 32 | `reserved` | u32; reserved=#true |
| 36 | `label` | utf8; length=label_len |
| 36 + label_len | `modes` | `mode_count` packed `Mode` records |

Total size: 36 + label_len + bytes(modes) bytes.

## GroupState

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `output` | u64 |
| 8 | `generation` | u64 |
| 16 | `x` | i32 |
| 20 | `y` | i32 |
| 24 | `width` | i32 |
| 28 | `height` | i32 |
| 32 | `member_count` | u16; max=4 |
| 34 | `reserved` | u16; reserved=#true |
| 36 | `members` | `member_count` packed `Member` records |

Total size: 36 + bytes(members) bytes.

## HeadTarget

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `head` | u64 |
| 8 | `head_generation` | u64 |
| 16 | `mode` | u64 |
| 24 | `transform` | u16; enum=transform |
| 26 | `vrr` | u16; enum=vrr |
| 28 | `reserved` | u32; reserved=#true |

Total size: 32 bytes.

## GroupProposal

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `output` | u64; zero=allocate |
| 8 | `x` | i32 |
| 12 | `y` | i32 |
| 16 | `width` | i32 |
| 20 | `height` | i32 |
| 24 | `member_count` | u16; max=4 |
| 26 | `reserved` | u16; reserved=#true |
| 28 | `members` | `member_count` packed `Member` records |

Total size: 28 + bytes(members) bytes.

## ClientHello

Kind 64; client-to-session; transaction `zero`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `minimum_revision` | u16 |
| 2 | `maximum_revision` | u16 |
| 4 | `capabilities` | u64 |

Total size: 12 bytes.

## ServerWelcome

Kind 65; session-to-client; transaction `zero`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `selected_revision` | u16 |
| 2 | `reserved` | u16; reserved=#true |
| 4 | `capabilities` | u64 |
| 12 | `connection_epoch` | u64 |
| 20 | `max_heads` | u16 |
| 22 | `max_groups` | u16 |
| 24 | `max_modes_per_head` | u16 |
| 26 | `max_heads_per_group` | u16 |

Total size: 28 bytes.

## Snapshot

Kind 66; session-to-client; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `connection_epoch` | u64 |
| 8 | `topology_epoch` | u64 |
| 16 | `primary_output` | u64 |
| 24 | `head_count` | u16; max=16 |
| 26 | `group_count` | u16; max=16 |
| 28 | `reserved` | u32; reserved=#true |
| 32 | `heads` | `head_count` packed `Head` records |
| 32 + bytes(heads) | `groups` | `group_count` packed `GroupState` records |

Total size: 32 + bytes(heads) + bytes(groups) bytes.

## Proposal

Kind 67; client-to-session; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `connection_epoch` | u64 |
| 8 | `base_topology_epoch` | u64 |
| 16 | `intent` | u16; enum=intent |
| 18 | `primary_group_index` | u16 |
| 20 | `head_count` | u16; max=16 |
| 22 | `group_count` | u16; max=16 |
| 24 | `heads` | `head_count` packed `HeadTarget` records |
| 24 + bytes(heads) | `groups` | `group_count` packed `GroupProposal` records |

Total size: 24 + bytes(heads) + bytes(groups) bytes.

## Outcome

Kind 68; session-to-client; transaction `required`.

| Offset | Field | Wire type |
| --- | --- | --- |
| 0 | `connection_epoch` | u64 |
| 8 | `topology_epoch` | u64 |
| 16 | `outcome` | u16; enum=outcome |
| 18 | `reason` | u16 |

Total size: 20 bytes.
