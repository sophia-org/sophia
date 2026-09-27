package shelloracle

func (r *runner) custodyScenario() {
	s := r.connect("custody", 4096)
	defer s.close()
	s.negotiate()
	start := s.offset
	id := s.nextID
	s.nextID++
	raw := s.encoded(257, id, allocation(500, 1, 99))
	s.stage(raw, 0)
	need(wrote(s.submitRaw(raw)) == 24, "initial custody submit")
	firstBytes := s.readEvents()
	repeated := data(s.w.read(2, start, uint32(len(firstBytes))))
	r.check(42, identical(firstBytes, repeated))
	s.custody(id, 257)
	result := s.expect(32)
	r.check(41, u64(result.body, 0) == 500 && s.offset > start && len(s.queue) == 0)
	need(wrote(s.submitRaw(raw)) == 24, "identical retry")
	s.noEvents()
	r.check(37, true)
	r.check(44, errno(s.w.read(2, s.offset+1, 1024)) == 22)
	tag := s.w.send(116, readBody(2, s.offset, 1024))
	s.w.getattr(1)
	_, ready := s.w.replies[tag]
	r.check(45, !ready)
	s.w.flush(tag)
	s.ack(result)
	s.w.clunk(5)
	r.check(43, errno(s.w.read(2, start, 1024)) == 116)
	s.stage(raw, 0)
	r.check(38, errno(s.submitRaw(raw)) == 114)
	s.w.clunk(5)
	s.noEvents()
	var last record
	// Fill the contract's 192 ordinary journal positions with real snapshot
	// publications. Reading them without acknowledgement retains that history.
	for i := 0; i < 192; i++ {
		s.phase("pressure")
		last = s.expect(19)
	}
	raw = s.encoded(257, s.nextID, allocation(600, 10, 99))
	s.stage(raw, 0)
	need(errno(s.submitRaw(raw)) == 11, "full journal must refuse custody")
	s.noEvents()
	r.check(39, true)
	s.ack(last)
	need(wrote(s.submitRaw(raw)) == 24, "retry after credit release")
	s.w.clunk(5)
	s.custody(s.nextID, 257)
	last = s.expect(32)
	need(u64(last.body, 0) == u64(raw, 32), "retry transaction correlation")
	s.ack(last)
	s.noEvents()
	r.check(40, true)
}
