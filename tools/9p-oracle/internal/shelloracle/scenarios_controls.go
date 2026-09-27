package shelloracle

import "io"

func (r *runner) refusedScenario() {
	s := r.connect("refused", 4096)
	defer s.close()
	id := s.submit(256, offer())
	s.custody(id, 256)
	e := s.expect(17)
	need(u16(e.body, 0) == 1 && u64(e.body, 8) == 384, "policy refusal reason/bits")
	s.ack(e)
	// Unservable offers must not be mislabeled as policy Refused. This attach
	// is separately revoked and therefore cannot share the accepted fixture.
	u := r.connect("unservable", 4096)
	defer u.close()
	b := offer()
	p64(b, 8, 384)
	raw := u.encoded(256, 1, b)
	u.stage(raw, 0)
	tag := u.w.send(116, readBody(2, u.offset, 4096-11))
	u.w.getattr(1)
	reply := u.submitRaw(raw)
	need(reply.kind == 119 || errno(reply) == 116, "unservable submit")
	noRefused := true
	resp := u.w.wait(tag)
	if errno(resp) != 116 {
		bytes := data(resp)
		need(len(bytes) > 0, "unservable empty events")
		u.offset += uint64(len(bytes))
		for len(bytes) > 0 {
			need(len(bytes) >= 32, "unservable event header")
			n := int(u32(bytes, 0))
			need(n >= 32 && n <= len(bytes), "unservable event length")
			event, err := decode(bytes[:n])
			must(err)
			need(event.epoch == u.epoch && event.sequence > u.sequence, "unservable event identity")
			u.sequence = event.sequence
			if event.kind == 17 {
				noRefused = false
			} else {
				need(event.kind == 18 && u64(event.body, 0) == 1 && u16(event.body, 8) == 256, "unservable unexpected event")
			}
			bytes = bytes[n:]
		}
	}
	_, _, err := u.w.receive()
	need(err == io.EOF, "unservable attach did not close cleanly: %v", err)
	r.check(2, noRefused)
}
func (r *runner) malformedScenario() {
	s := r.connect("malformed", 4096)
	defer s.close()
	s.negotiate()
	for i := 0; i < 4; i++ {
		raw := s.encoded(262, s.nextID, candidate(1, 1))
		switch i {
		case 0:
			p32(raw, 0, uint32(len(raw)+1))
		case 1:
			raw[32+78] = 1
		case 2:
			p16(raw, 32+72, 9)
		case 3:
			p16(raw, 6, 65535)
		}
		s.stage(raw, 0)
		need(errno(s.submitRaw(raw)) == 22, "malformed submit errno")
		s.w.clunk(5)
		s.noEvents()
		r.check(47+i, true)
	}
}
func (r *runner) streamScenario() {
	s := r.connect("stream", 4096)
	defer s.close()
	s.w.fragment = true
	s.negotiate()
	r.check(51, true)
	raw := s.encoded(257, s.nextID, allocation(40, 1, 99))
	id := s.nextID
	s.nextID++
	s.stage(raw, 7)
	need(wrote(s.submitRaw(raw)) == 24, "fragmented staged submit")
	s.w.clunk(5)
	s.custody(id, 257)
	e := s.expect(32)
	s.ack(e)
	r.check(52, u64(e.body, 0) == 40)
	tag := s.w.send(116, readBody(2, s.offset, 1024))
	s.w.getattr(1)
	_, answered := s.w.replies[tag]
	need(!answered, "pending read answered before publication")
	s.w.flush(tag)
	r.check(53, true)
	s.phase("publish")
	e = s.expect(19)
	s.ack(e)
	s.w.getattr(1)
	r.check(54, e.kind == 19)
}
func (r *runner) revocationScenario(name string, cancelled bool, check int) {
	s := r.connect(name, 4096)
	defer s.close()
	s.negotiate()
	s.phase("republish")
	s.ack(s.expect(19))
	id := s.submit(257, allocation(40, 1, 2))
	s.custody(id, 257)
	s.ack(s.expect(32))
	smallResource(s)
	permit := uint64(999)
	if cancelled {
		id = s.submit(263, demand(90, 1))
		s.custody(id, 263)
		e := s.expect(36)
		permit = u64(e.body, 48)
		s.ack(e)
		id = s.submit(264, cancelDemand(91, 1, permit))
		s.custody(id, 264)
		e = s.expect(36)
		need(u16(e.body, 56) == 3 && u16(e.body, 58) == 11, "cancel before revocation")
		s.ack(e)
	}
	// Pause semantic service until the client has observed custody. This proves
	// custody separately from the owner error that causes revocation.
	s.phase("hold-candidates")
	id = s.submit(262, candidate(permit, 1))
	s.ack(s.custody(id, 262))
	tag := s.w.send(116, readBody(2, s.offset, 1024))
	s.w.getattr(1)
	_, answered := s.w.replies[tag]
	need(!answered, "unexpected CandidateOutcome")
	s.phase("release-candidates")
	reply := s.w.wait(tag)
	need(errno(reply) == 116, "waiting read not fenced on revocation")
	_, _, err := s.w.receive()
	r.check(check, err == io.EOF)
}
