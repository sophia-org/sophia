package shelloracle

import (
	"fmt"
	"io"
)

func (r *runner) roleFatal(name string, native bool) {
	s := r.connect(name, 4096)
	defer s.close()
	revision, mask, kind := uint16(8), uint64(0x11a2), uint16(270)
	if native {
		revision, mask, kind = 7, 0x9a0, 267
	}
	roleNegotiate(s, revision, mask, mask)
	s.phase("publish")
	roleObject(s, 3, 3)
	if native {
		s.phase("opening")
		s.ack(s.expect(38))
	}
	roleAllocation(s, native)
	smallResource(s)
	s.phase("hold-candidates")
	id := s.submit(kind, roleCandidate(native, 999, 1, 3, 1))
	s.ack(s.custody(id, kind))
	tag := s.w.send(116, readBody(2, s.offset, 1024))
	s.w.getattr(1)
	_, answered := s.w.replies[tag]
	need(!answered, "fatal role outcome before service")
	s.phase("release-candidates")
	need(errno(s.w.wait(tag)) == 116, "fatal role waiting read needs ESTALE")
	_, _, err := s.w.receive()
	need(err == io.EOF, "fatal role EOF: %v", err)
}
func (r *runner) roleCandidateControls(s *session, native bool, catalog, initialPermit uint64) uint64 {
	kind := uint16(270)
	if native {
		kind = 267
	}
	base := roleCandidate(native, 1, 1, catalog, 1)
	p64(base, 8, s.epoch)
	var invalid [][]byte
	if native {
		b := append(append([]byte{}, base[:108]...), base[172:]...)
		p16(b, 98, 0)
		invalid = append(invalid, b)
		b = append(append([]byte{}, base[:172]...), base[204:]...)
		p16(b, 100, 0)
		invalid = append(invalid, b)
		b = append([]byte{}, base[:252]...)
		p16(b, 104, 0)
		invalid = append(invalid, b)
		b = append([]byte{}, base...)
		p16(b, 96, 2)
		invalid = append(invalid, b)
		b = append(append([]byte{}, base[:252]...), base[204:252]...)
		b = append(b, 1, 0, 1, 0)
		p16(b, 102, 2)
		p16(b, 104, 2)
		invalid = append(invalid, b)
	} else {
		b := append([]byte{}, base...)
		p16(b, 112, 3)
		invalid = append(invalid, b)
	}
	s.phase("hold-candidates")
	for _, b := range invalid {
		raw := envelope(kind, s.nextID, s.epoch, b)
		s.stage(raw, 0)
		need(errno(s.submitRaw(raw)) == 22, "role value submit EINVAL")
		s.w.clunk(5)
		s.noEvents()
	}
	s.phase("release-candidates")
	cases := []string{"catalog", "missing-slot", "unavailable-slot", "duplicate-target", "overlap", "outside"}
	if native {
		cases = append(cases, "opening", "state")
	} else {
		cases = append(cases, "surface-count", "placement-count")
	}
	for i, name := range cases {
		fmt.Fprintf(r.diagnostics, "%s candidate control %s\n", s.fixture, name)
		generation := uint64(10 + i)
		// The value-refused submissions above leave the initial permit intact.
		permit := initialPermit
		if i > 0 {
			permit = rolePermit(s, uint64(10+i))
		}
		b := roleCandidate(native, permit, generation, catalog, 1)
		target, counts, catalogOffset := 184, 80, 72
		reason := uint16(3)
		if native {
			target, counts, catalogOffset = 204, 98, 80
		}
		switch name {
		case "catalog":
			p64(b, catalogOffset, catalog+100)
			reason = 1
		case "opening":
			p64(b, 72, 77)
			reason = 1
		case "state":
			p64(b, 88, 9)
			reason = 1
		case "missing-slot", "unavailable-slot":
			slot := uint64(4096)
			if name == "unavailable-slot" {
				slot = 3
			}
			p64(b, target+20, slot)
			if native {
				p16(b, 96, uint16(slot))
				p16(b, len(b)-2, uint16(slot))
			}
		case "duplicate-target", "overlap":
			end := target + 48
			b = append(append([]byte{}, b[:end]...), b[target:end]...)
			if name == "overlap" {
				p64(b, end+4, 2)
				p64(b, end+20, 2)
			} else {
				p32(b, end+28, 2)
			}
			p16(b, counts+4, 2)
			if native {
				b = append(b, 1, 0, 2, 0)
				p16(b, 104, 2)
			}
		case "outside":
			p32(b, target+28, 64)
		case "surface-count":
			b = append(append([]byte{}, b[:88]...), b[152:]...)
			p16(b, 80, 0)
		case "placement-count":
			b = append(append([]byte{}, b[:152]...), b[184:]...)
			p16(b, 82, 0)
		}
		s.phase("hold-candidates")
		id := s.submit(kind, b)
		s.ack(s.custody(id, kind))
		s.phase("release-candidates")
		roleOutcome(s, generation, 3, reason)
		s.phase("owner-drained")
		s.noEvents()
	}
	return uint64(10 + len(cases))
}
