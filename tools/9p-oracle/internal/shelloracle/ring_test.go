package shelloracle

import "testing"

func ringEventBytes(kind uint64) []byte {
	b := make([]byte, 120)
	p32(b, 0, 120)
	copy(b[4:8], "D4E1")
	for at, v := range map[int]uint64{8: kind, 16: 17, 24: 1, 32: 2, 40: 1, 48: 1, 56: 3} {
		p64(b, at, v)
	}
	if kind == 3 {
		p64(b, 104, 1)
	}
	if kind == 4 {
		for at, v := range map[int]uint64{64: 1, 80: 2, 88: 5, 96: 1} {
			p64(b, at, v)
		}
	}
	return b
}
func TestRingEventShapeAndRevision(t *testing.T) {
	for kind := uint64(1); kind <= 4; kind++ {
		b := ringEventBytes(kind)
		if r, err := decodeAt(b, 10); err != nil || r.kind != uint16(0x7100+kind) || r.epoch != 17 || r.sequence != 3 {
			t.Fatalf("kind %d: %+v %v", kind, r, err)
		}
		if _, err := decodeAt(b, 9); err == nil {
			t.Fatal("D4E1 accepted under r9")
		}
		for n := 0; n < len(b); n++ {
			if _, err := decodeAt(b[:n], 10); err == nil {
				t.Fatalf("truncation %d accepted", n)
			}
		}
	}
	for name, mutate := range map[string]func([]byte){
		"kind":            func(b []byte) { p64(b, 8, 5) },
		"epoch":           func(b []byte) { p64(b, 16, 0) },
		"sequence":        func(b []byte) { p64(b, 56, 0) },
		"slot":            func(b []byte) { p64(b, 72, 4) },
		"odd generation":  func(b []byte) { p64(b, 80, 3) },
		"resource":        func(b []byte) { p64(b, 88, 0) },
		"changed success": func(b []byte) { p64(b, 112, 1) },
		"reason width":    func(b []byte) { p64(b, 104, 65536) },
	} {
		b := ringEventBytes(4)
		mutate(b)
		if _, err := decodeAt(b, 10); err == nil {
			t.Fatalf("%s accepted", name)
		}
	}
}
func TestRingSharesJournalEpochAndSequence(t *testing.T) {
	for _, c := range []struct {
		name            string
		epoch, sequence uint64
		ok              bool
	}{
		{"valid", 17, 3, true}, {"foreign epoch", 18, 3, false}, {"replay", 17, 2, false},
	} {
		t.Run(c.name, func(t *testing.T) {
			b := ringEventBytes(1)
			p64(b, 16, c.epoch)
			p64(b, 56, c.sequence)
			s, done := scriptedEvents(t, [][]byte{b})
			defer done()
			s.revision = 10
			s.sequence = 2
			err := readAll(s, 1)
			if (err == nil) != c.ok {
				t.Fatalf("error %v", err)
			}
		})
	}
}
