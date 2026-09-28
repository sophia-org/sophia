//! Complete descriptor candidates; there is no socket fragment framing.
use super::*;

impl Wire for ShellV1CandidateEntry {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.slot.put(bytes);
        0u16.put(bytes);
        self.generation.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let slot = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        Ok(Self {
            slot,
            generation: u64::take(cursor)?,
        })
    }
}

impl Wire for ShellV1Candidate {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.snapshot_generation.put(bytes);
        self.candidate_generation.put(bytes);
        self.output.put(bytes);
        u16::from(self.visible).put(bytes);
        let (edge, thickness): (u16, u16) = self.reservation.map_or((0, 0), |r| {
            (
                match r.edge {
                    ShellV1ReservationEdge::Top => 1,
                    ShellV1ReservationEdge::Bottom => 2,
                    ShellV1ReservationEdge::Left => 3,
                    ShellV1ReservationEdge::Right => 4,
                },
                r.thickness_px,
            )
        });
        edge.put(bytes);
        thickness.put(bytes);
        self.selected_slot.unwrap_or(0).put(bytes);
        (self.entries.len() as u16).put(bytes);
        0u16.put(bytes);
        for entry in &self.entries {
            entry.put(bytes);
        }
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let snapshot_generation = u64::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let output = OutputId::take(cursor)?;
        let visible = boolean(cursor, "descriptor visible")?;
        let edge = u16::take(cursor)?;
        let thickness_px = u16::take(cursor)?;
        let reservation = match edge {
            0 if thickness_px == 0 => None,
            1..=4 => Some(ShellV1WorkAreaReservation {
                edge: match edge {
                    1 => ShellV1ReservationEdge::Top,
                    2 => ShellV1ReservationEdge::Bottom,
                    3 => ShellV1ReservationEdge::Left,
                    _ => ShellV1ReservationEdge::Right,
                },
                thickness_px,
            }),
            _ => return Err(ValueError::InvalidRecord("descriptor reservation")),
        };
        let selected = u16::take(cursor)?;
        let count = table_count(cursor, SOPHIA_SHELL_MAX_DESCRIPTORS)?;
        reserved::<u16>(cursor)?;
        let entries = rows(cursor, count)?;
        Ok(Self {
            connection_epoch,
            snapshot_generation,
            candidate_generation,
            output,
            visible,
            selected_slot: if selected == 0 { None } else { Some(selected) },
            reservation,
            entries,
        })
    }
}

impl Wire for ShellTabCandidate {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.snapshot_generation.put(bytes);
        self.candidate_generation.put(bytes);
        (self.groups.len() as u16).put(bytes);
        0u16.put(bytes);
        for group in &self.groups {
            group.put(bytes);
        }
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let snapshot_generation = u64::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let count = table_count(cursor, SOPHIA_SHELL_MAX_TAB_GROUPS)?;
        reserved::<u16>(cursor)?;
        Ok(Self {
            connection_epoch,
            snapshot_generation,
            candidate_generation,
            groups: rows(cursor, count)?,
        })
    }
}

impl Wire for ShellReferenceStyle {
    fn put(&self, bytes: &mut Vec<u8>) {
        for field in [
            self.body_size,
            self.title_size,
            self.padding,
            self.row_gap,
            self.key_gap,
            self.column_gap,
            self.border,
            self.margin,
            self.columns,
            0,
        ] {
            field.put(bytes);
        }
        for color in self.colors {
            color.put(bytes);
        }
        put_text_padded(bytes, &self.title, 128);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let body_size = u16::take(cursor)?;
        let title_size = u16::take(cursor)?;
        let padding = u16::take(cursor)?;
        let row_gap = u16::take(cursor)?;
        let key_gap = u16::take(cursor)?;
        let column_gap = u16::take(cursor)?;
        let border = u16::take(cursor)?;
        let margin = u16::take(cursor)?;
        let columns = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        let mut colors = [0; 6];
        for color in &mut colors {
            *color = u32::take(cursor)?;
        }
        let title = take_text_padded(cursor, 128)?;
        Ok(Self {
            body_size,
            title_size,
            padding,
            row_gap,
            key_gap,
            column_gap,
            border,
            margin,
            columns,
            colors,
            title,
        })
    }
}
impl Wire for ShellReferenceEntry {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.slot.put(bytes);
        0u16.put(bytes);
        put_text_padded(bytes, &self.key, 64);
        put_text_padded(bytes, &self.label, 128);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let slot = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        Ok(Self {
            slot,
            key: take_text_padded(cursor, 64)?,
            label: take_text_padded(cursor, 128)?,
        })
    }
}
impl Wire for ShellReferenceCandidate {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.catalog_generation.put(bytes);
        self.request_generation.put(bytes);
        self.candidate_generation.put(bytes);
        self.output.put(bytes);
        u16::from(self.visible).put(bytes);
        self.page.put(bytes);
        (self.entries.len() as u16).put(bytes);
        0u16.put(bytes);
        self.style.put(bytes);
        for entry in &self.entries {
            entry.put(bytes);
        }
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let catalog_generation = u64::take(cursor)?;
        let request_generation = u64::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let output = OutputId::take(cursor)?;
        let visible = boolean(cursor, "reference visible")?;
        let page = u16::take(cursor)?;
        let count = table_count(cursor, SOPHIA_SHELL_MAX_SHORTCUTS)?;
        reserved::<u16>(cursor)?;
        let style = ShellReferenceStyle::take(cursor)?;
        let entries = rows(cursor, count)?;
        Ok(Self {
            connection_epoch,
            catalog_generation,
            request_generation,
            candidate_generation,
            output,
            visible,
            page,
            style,
            entries,
        })
    }
}

impl Wire for ShellLauncherCandidate {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.catalog_generation.put(bytes);
        self.request_generation.put(bytes);
        self.candidate_generation.put(bytes);
        self.output.put(bytes);
        u16::from(self.visible).put(bytes);
        self.selected.put(bytes);
        (self.entries.len() as u16).put(bytes);
        self.font_size.put(bytes);
        for color in self.colors {
            color.put(bytes);
        }
        for slot in &self.entries {
            slot.put(bytes);
        }
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let catalog_generation = u64::take(cursor)?;
        let request_generation = u64::take(cursor)?;
        let candidate_generation = u64::take(cursor)?;
        let output = OutputId::take(cursor)?;
        let visible = boolean(cursor, "launcher visible")?;
        let selected = u16::take(cursor)?;
        let count = table_count(cursor, SOPHIA_SHELL_MAX_LAUNCHER_ROWS)?;
        let font_size = u16::take(cursor)?;
        let mut colors = [0; 4];
        for color in &mut colors {
            *color = u32::take(cursor)?;
        }
        let entries = rows(cursor, count)?;
        Ok(Self {
            connection_epoch,
            catalog_generation,
            request_generation,
            candidate_generation,
            output,
            visible,
            selected,
            entries,
            font_size,
            colors,
        })
    }
}

value_codec!(
    encode_shell_descriptor_candidate_value,
    decode_shell_descriptor_candidate_value,
    ShellV1Candidate,
    validate_shell_descriptor_candidate
);
value_codec!(
    encode_shell_tab_candidate_value,
    decode_shell_tab_candidate_value,
    ShellTabCandidate,
    validate_shell_tab_candidate
);
value_codec!(
    encode_shell_reference_candidate_value,
    decode_shell_reference_candidate_value,
    ShellReferenceCandidate,
    validate_shell_reference_candidate
);
value_codec!(
    encode_shell_launcher_candidate_value,
    decode_shell_launcher_candidate_value,
    ShellLauncherCandidate,
    validate_shell_launcher_candidate
);
