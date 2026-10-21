package shelloracle

import "golang.org/x/sys/unix"

// Independently encoded from sophia-owned-ring-v1.kdl. Synthetic kind numbers
// are internal dispatch only; D4E1 has its own framing on the shared journal.
const (
	ringAdmitted = 0x7101
	ringReady    = 0x7102
	ringRefused  = 0x7103
	ringReleased = 0x7104
	ringCapacity = 512
	ringExtent   = 8192
)

func ringEventKind(k uint16) bool { return k >= ringAdmitted && k <= ringReleased }
func decodeRingEvent(b []byte, revision uint16) record {
	need(revision == 10 && len(b) == 120 && u32(b, 0) == 120 && string(b[4:8]) == "D4E1", "ring event framing/revision")
	k := u64(b, 8)
	need(k >= 1 && k <= 4, "ring event kind")
	nonzero64(b, 16, 24, 32, 40, 48, 56)
	reason, changed := u64(b, 104), u64(b, 112)
	need(reason <= 65535 && changed <= 1, "ring reason/changed")
	if k == 4 {
		nonzero64(b, 64, 80, 88, 96)
		need(u64(b, 72) <= 3 && u64(b, 80)%2 == 0 && (changed == 0 || reason != 0), "slot release")
	} else {
		need(zero(b[64:104]) && changed == 0, "ring non-slot fields")
		need((k == 3 && reason != 0) || (k != 3 && reason == 0), "ring setup reason")
	}
	return record{kind: uint16(0x7100 + k), epoch: u64(b, 16), sequence: u64(b, 56), body: b, bytes: b}
}
func (s *session) ringControl(size int, kind uint64) []byte {
	b := make([]byte, size)
	p32(b, 0, uint32(size))
	copy(b[4:8], "D4C1")
	for at, v := range map[int]uint64{8: kind, 16: s.epoch, 24: contentGrantEpoch, 32: s.discovery.generation, 40: 1, 48: 1} {
		p64(b, at, v)
	}
	return b
}
func (s *session) ringEvent() record {
	e := s.next()
	need(ringEventKind(e.kind), "expected ring event, got %d", e.kind)
	need(u64(e.body, 24) == contentGrantEpoch && u64(e.body, 32) == s.discovery.generation && u64(e.body, 40) == 1 && u64(e.body, 48) == 1, "ring event identity")
	s.ack(e)
	return e
}
func (s *session) registerRing(unsealed, wrongSize bool) record {
	need(!s.ringRegistered, "duplicate registration")
	s.w.file(29, 1, "owned-ring-v1")
	registration := s.ringControl(72, 1)
	p64(registration, 56, 1)
	p64(registration, 64, ringCapacity)
	need(wrote(s.w.write(29, 0, registration)) == 72, "Register write")
	need(s.ringEvent().kind == ringAdmitted, "RingAdmitted")
	fd, err := unix.MemfdCreate("oracle-ring", unix.MFD_CLOEXEC|unix.MFD_ALLOW_SEALING)
	must(err)
	s.ringFD = fd
	size := int64(ringExtent)
	if wrongSize {
		size -= 4
	}
	must(unix.Ftruncate(fd, size))
	if !unsealed {
		_, err = unix.FcntlInt(uintptr(fd), unix.F_ADD_SEALS, unix.F_SEAL_GROW|unix.F_SEAL_SHRINK)
		must(err)
	}
	source := append([]byte(nil), registration...)
	copy(source[4:8], "D4F1")
	n, err := unix.SendmsgN(s.image, source, unix.UnixRights(fd), nil, unix.MSG_NOSIGNAL)
	must(err)
	need(n == 72, "Source send")
	var e record
	if unsealed {
		e = s.expect(kindChannelClosed)
		need(u64(e.body, 0) == s.discovery.generation && u32(e.body, 8) == 2, "unsealed source protocol fault")
		s.ack(e)
	} else {
		e = s.ringEvent()
	}
	s.ringRegistered = e.kind == ringReady
	return e
}
func (s *session) ringUpload(resource, topology uint64, width, height uint32, pixels []byte) []byte {
	need(s.ringRegistered && !s.ringBusy && len(pixels) == int(width)*int(height)*4 && len(pixels) <= ringCapacity, "ring pixels")
	s.offers++
	b := s.ringControl(152, 2)
	for at, v := range map[int]uint64{56: s.offers, 72: s.offers * 2, 80: resource, 88: 1, 96: 1, 104: 1, 112: 2, 120: 1, 128: topology, 136: uint64(width), 144: uint64(height)} {
		p64(b, at, v)
	}
	n, err := unix.Pwrite(s.ringFD, b[72:80], 0)
	must(err)
	need(n == 8, "generation write")
	n, err = unix.Pwrite(s.ringFD, pixels, 4096)
	must(err)
	need(n == len(pixels), "pixel write")
	need(wrote(s.w.write(29, 0, b)) == 152, "Offer write")
	s.ringBusy = true
	return b
}
func (s *session) settleRing(offer []byte) (record, record) {
	status := s.expect(33)
	need(u64(status.body, 0) == u64(offer, 56) && u64(status.body, 16) == contentGrantEpoch && u64(status.body, 24) == u64(offer, 80) && u64(status.body, 32) == u64(offer, 88), "ring status identity")
	s.ack(status)
	release := s.ringEvent()
	need(release.kind == ringReleased && release.sequence > status.sequence, "final status before SlotReleased")
	need(u64(release.body, 64) == u64(offer, 56) && u64(release.body, 72) == u64(offer, 64) && u64(release.body, 80) == u64(offer, 72) && u64(release.body, 88) == u64(offer, 80) && u64(release.body, 96) == u64(offer, 88), "exact SlotReleased")
	need(u64(release.body, 104) == uint64(u16(status.body, 42)) && u64(release.body, 112) == 0, "release reason")
	s.ringBusy = false
	return status, release
}
func ringResource(s *session, resource, topology uint64) bool {
	if s.image < 0 {
		need(s.discover(), "image discovery")
		s.hello()
	}
	if !s.ringRegistered {
		need(s.registerRing(false, false).kind == ringReady, "ring ready")
	}
	offer := s.ringUpload(resource, topology, 2, 1, []byte{0, 0, 255, 255, 0, 128, 0, 128})
	status, _ := s.settleRing(offer)
	// Rewriting a released slot must not alter the retained resource that the
	// role fixture subsequently renders and compares byte for byte.
	n, err := unix.Pwrite(s.ringFD, make([]byte, 8), 4096)
	must(err)
	need(n == 8, "released slot rewrite")
	return u16(status.body, 40) == 2
}
func (r *runner) badRingSource(name string, unsealed, wrongSize bool) bool {
	s := r.connect(name, 4096)
	defer s.close()
	s.negotiate()
	need(s.discover(), "bad source discovery")
	s.hello()
	refusal := s.registerRing(unsealed, wrongSize)
	s.noEvents()
	if unsealed {
		return refusal.kind == kindChannelClosed && u32(refusal.body, 8) == 2
	}
	return refusal.kind == ringRefused && u64(refusal.body, 104) != 0
}
