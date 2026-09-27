package shelloracle

// Expected masks and semantics are from the admitted file contract, not the
// runtime fixture. Session launch/admission policy is scripted by the harness.
func roleNegotiate(s *session, revision uint16, mask, grant uint64) {
	b := make([]byte, 16)
	p16(b, 0, revision)
	p16(b, 2, revision)
	p64(b, 8, mask)
	id := s.submit(256, b)
	s.ack(s.custody(id, 256))
	e := s.expect(16)
	need(u16(e.body, 0) == revision && u64(e.body, 12) == grant && u16(e.body, 26) == 1, "role negotiation")
	s.ack(e)
	s.w.file(10, 0, "limits")
	need(s.object(10).kind == 1, "role limits")
	s.w.clunk(10)
}
func announced(s *session, kind uint16, generation uint64) record {
	e := s.expect(19)
	need(u16(e.body, 0) == kind && u64(e.body, 8) == generation, "object announcement")
	s.ack(e)
	return e
}
func roleObject(s *session, kind uint16, generation uint64) record {
	a := announced(s, kind, generation)
	name := "catalog"
	if kind == 4 {
		name = "indicators"
	}
	qid := s.w.file(10, 0, name)
	o := s.object(10)
	s.w.clunk(10)
	need(o.kind == kind && u64(o.body, 16) == generation && qid == u64(a.body, 16), "role object identity")
	return o
}
func roleAllocation(s *session, native bool) record {
	s.phase("outputs")
	announced(s, 2, 3)
	var id uint64
	if native {
		id = s.submit(266, nativeAllocationBody(nativeAllocationRequest{transaction: 40, connection: s.epoch, content: 1, opening: 7, output: 2, outputGeneration: 1, request: 1, operation: 1, edge: 1, width: 64, height: 32}))
		s.custody(id, 266)
	} else {
		id = s.submit(257, allocation(40, 1, 2))
		s.custody(id, 257)
	}
	e := s.expect(32)
	need(u64(e.body, 0) == 40 && u64(e.body, 24) == 1 && u16(e.body, 32) == 1 && u64(e.body, 56) == 1 && u32(e.body, 104) == 64 && u32(e.body, 108) == 32, "role allocation")
	if native {
		need(u32(e.body, 136) == 0, "native reservation")
	}
	s.ack(e)
	return e
}
func rolePermit(s *session, id uint64) uint64 {
	b := demand(90+id, id)
	p64(b, 40, 1)
	p64(b, 48, 1)
	n := s.submit(263, b)
	s.custody(n, 263)
	e := s.expect(36)
	need(u64(e.body, 40) == id && u16(e.body, 56) == 1 && u64(e.body, 48) != 0, "role permit")
	s.ack(e)
	return u64(e.body, 48)
}
func roleCandidate(native bool, permit, generation, catalog, state uint64) []byte {
	b := candidate(permit, generation)
	p64(b, 48, 3)
	return roleCandidateBody(b, native, 7, catalog, state, 1, []uint16{1})
}
func roleOutcome(s *session, generation uint64, kind, reason uint16) record {
	e := s.expect(35)
	need(u64(e.body, 24) == generation && u16(e.body, 48) == kind && u16(e.body, 50) == reason, "role candidate outcome gen=%d want=%d/%d got=%d/%d", generation, kind, reason, u16(e.body, 48), u16(e.body, 50))
	s.ack(e)
	return e
}
func rolePresent(s *session, generation uint64, native bool) record {
	s.phase("present")
	e := roleOutcome(s, generation, 2, 0)
	need(u64(e.body, 52) != 0, "presentation epoch")
	if !native {
		return e
	}
	f := s.expect(39)
	s.ack(f)
	return f
}
