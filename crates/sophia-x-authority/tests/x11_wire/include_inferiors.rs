// A graphics context whose subwindow-mode is IncludeInferiors draws through
// the window's mapped inferiors instead of being clipped by them (t181).

const INCLUDE_INFERIORS_MASK: u32 = 1 << 15;

struct InferiorsFixture {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    order: XByteOrder,
    sequence: u16,
    presented: std::collections::BTreeMap<XResourceId, (i32, Vec<u8>)>,
    /// Every request's presentation updates, since each dispatch clears them.
    updates: Vec<sophia_x_authority::XAuthorityCpuBufferUpdate>,
}

impl InferiorsFixture {
    fn new() -> Self {
        Self {
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            order: XByteOrder::LittleEndian,
            sequence: 0,
            presented: std::collections::BTreeMap::new(),
            updates: Vec::new(),
        }
    }

    fn send(&mut self, ns: NamespaceId, op: u8, bytes: Vec<u8>) -> XDispatchResult {
        self.sequence += 1;
        let seq = self.sequence;
        let request = decode_x11_core_request(context(ns, u64::from(seq), self.order), &bytes).unwrap();
        let result = dispatch_x11_wire_request(
            dispatch_context(ns, seq, self.order, op),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        );
        self.updates.extend(self.runtime.take_cpu_buffer_updates());
        assert!(
            !result.outputs.iter().any(|output| matches!(output, XClientOutput::Error(_))),
            "no error: {:?}",
            result.outputs
        );
        result
    }

    fn gc(&mut self, ns: NamespaceId, gc: u32, drawable: u32, pixel: u32, include_inferiors: bool) {
        let order = self.order;
        self.send(ns, 55, create_gc_values_request(order, gc, drawable, 3, u32::MAX, pixel, 0, 0, 0));
        if include_inferiors {
            self.send(ns, 56, change_gc_request(order, gc, INCLUDE_INFERIORS_MASK, &[1]));
        }
    }

    fn fill(&mut self, ns: NamespaceId, drawable: u32, gc: u32, rect: (i16, i16, u16, u16)) {
        let order = self.order;
        self.send(ns, 70, poly_fill_rectangle_request(order, drawable, gc, &[rect]));
    }

    fn read(&mut self, ns: NamespaceId, drawable: u32, x: i16, y: i16) -> u32 {
        let order = self.order;
        match self.send(ns, 73, get_image_request(order, 2, drawable, x, y, 1, 1, u32::MAX)).outputs.as_slice() {
            [XClientOutput::Reply(XClientReply::GetImage { data, .. })] => {
                u32::from_le_bytes(data[..4].try_into().unwrap()) & 0x00ff_ffff
            }
            other => panic!("unexpected GetImage result: {other:?}"),
        }
    }

    fn on_screen(&mut self, toplevel: u32, x: i32, y: i32) -> u32 {
        let updates = std::mem::take(&mut self.updates);
        presented_pixel(&mut self.presented, updates, XResourceId::new(u64::from(toplevel), 1), x, y)
    }
}

#[test]
fn include_inferiors_draws_through_a_mapped_child() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5101);
    let order = fixture.order;
    fixture.send(ns, 1, create_window_request(order, 0x700001, 0, 0, 40, 40));
    fixture.send(ns, 1, create_window_request_with_parent(order, 0x700002, 0x700001, 10, 10, 10, 10));
    fixture.send(ns, 8, map_window_request(order, 0x700002));
    fixture.send(ns, 8, map_window_request(order, 0x700001));
    fixture.gc(ns, 0x700003, 0x700001, 0x0000_ff00, false);
    fixture.gc(ns, 0x700004, 0x700001, 0x0000_00ff, true);
    fixture.fill(ns, 0x700002, 0x700003, (0, 0, 10, 10));
    fixture.fill(ns, 0x700001, 0x700004, (5, 5, 10, 10));
    assert_eq!(fixture.read(ns, 0x700002, 2, 2), 0x0000_00ff, "the fill reaches the child");
    assert_eq!(fixture.read(ns, 0x700002, 7, 7), 0x0000_ff00, "and only where it lands");
    assert_eq!(fixture.read(ns, 0x700001, 12, 12), 0x0000_00ff, "the parent reads it through the child");
    assert_eq!(fixture.on_screen(0x700001, 12, 12), 0x0000_00ff, "the screen shows it over the child");
    assert_eq!(fixture.on_screen(0x700001, 17, 17), 0x0000_ff00, "and the child around it");
    assert_eq!(fixture.on_screen(0x700001, 7, 7), 0x0000_00ff, "and the parent beside it");
}

#[test]
fn include_inferiors_on_the_root_draws_through_the_namespaces_own_windows() {
    let mut fixture = InferiorsFixture::new();
    let ns = NamespaceId::from_raw(0x5102);
    let other = NamespaceId::from_raw(0x5103);
    let order = fixture.order;
    fixture.send(other, 1, create_window_request(order, 0x710001, 0, 0, 20, 20));
    fixture.send(other, 8, map_window_request(order, 0x710001));
    fixture.gc(other, 0x710002, 0x710001, 0x00ff_0000, false);
    fixture.fill(other, 0x710001, 0x710002, (0, 0, 20, 20));
    fixture.send(ns, 1, create_window_request(order, 0x700011, 0, 0, 20, 20));
    fixture.send(ns, 8, map_window_request(order, 0x700011));
    fixture.gc(ns, 0x700012, 0x700011, 0x0000_ff00, false);
    fixture.gc(ns, 0x700013, 0x20, 0x0000_00ff, true);
    fixture.fill(ns, 0x700011, 0x700012, (0, 0, 20, 20));
    fixture.fill(ns, 0x20, 0x700013, (5, 5, 5, 5));
    assert_eq!(fixture.read(ns, 0x700011, 6, 6), 0x0000_00ff, "the root fill reaches the toplevel");
    assert_eq!(fixture.read(ns, 0x700011, 15, 15), 0x0000_ff00, "and only where it lands");
    assert_eq!(fixture.read(ns, 0x20, 6, 6), 0x0000_00ff, "the root reads it back");
    assert_eq!(fixture.on_screen(0x700011, 6, 6), 0x0000_00ff, "the toplevel is presented with it");
    assert_eq!(fixture.read(other, 0x710001, 6, 6), 0x00ff_0000, "another namespace's window is untouched");
}
