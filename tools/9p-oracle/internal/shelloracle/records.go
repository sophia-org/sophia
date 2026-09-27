package shelloracle

import "fmt"

// These offsets are hand-written from sophia-shell-files-v1.kdl, not from
// another encoder. Every bounds-dependent access follows a length check.
type record struct {
	kind                        uint16
	epoch, submission, sequence uint64
	body, bytes                 []byte
}

func zero(b []byte) bool {
	for _, v := range b {
		if v != 0 {
			return false
		}
	}
	return true
}
func nonzero64(b []byte, offsets ...int) {
	for _, at := range offsets {
		need(u64(b, at) != 0, "zero identity at %d", at)
	}
}
func pair(a, b uint64) bool { return (a == 0) == (b == 0) }
func scale(n, d uint32) bool {
	if n < 1 || n > 32 || d < 1 || d > 4 {
		return false
	}
	for d != 0 {
		n, d = d, n%d
	}
	return n == 1
}
func margins(b []byte) {
	for at := 0; at < len(b); at += 2 {
		v := int16(u16(b, at))
		need(v >= -512 && v <= 512, "margin")
	}
}
func decode(bytes []byte) (r record, err error) {
	return decodeRecord(bytes, true)
}

// Candidate negative controls can inspect byte-valid records independently
// of the family value validator which production submit also runs.
func decodeCandidateBytes(bytes []byte) (r record, err error) {
	if len(bytes) < 32 || (u16(bytes, 6) != 267 && u16(bytes, 6) != 270) {
		return r, fmt.Errorf("not a role candidate")
	}
	return decodeRecord(bytes, false)
}
func decodeRecord(bytes []byte, values bool) (r record, err error) {
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("record: %v", p)
		}
	}()
	need(len(bytes) >= 32 && len(bytes) <= 4194304 && int(u32(bytes, 0)) == len(bytes), "length")
	need(u16(bytes, 4) == 1 && u64(bytes, 8) != 0, "header")
	r = record{u16(bytes, 6), u64(bytes, 8), u64(bytes, 16), u64(bytes, 24), bytes[32:], bytes}
	need((r.submission == 0 && ((r.kind < 16 && r.sequence == 0) || (r.kind >= 16 && r.kind < 256 && r.sequence != 0))) || (r.kind >= 266 && r.kind <= 272 && r.submission != 0 && r.sequence == 0), "record class")
	if r.kind == 3 || r.kind == 4 || (r.kind >= 38 && r.kind <= 45) || (r.kind >= 266 && r.kind <= 272) {
		if !values && (r.kind == 267 || r.kind == 270) {
			validateRoleCandidateBytes(r.kind, r.body)
			return r, nil
		}
		validateRole(r)
		return r, nil
	}
	need(len(bytes) <= 65536, "base record cap")
	sizes := map[uint16]int{1: 264, 16: 32, 17: 16, 18: 16, 19: 24, 32: 168, 33: 56, 34: 42, 35: 76, 36: 72, 37: 120}
	b := r.body
	if r.kind != 2 {
		n, ok := sizes[r.kind]
		need(ok && len(b) == n, "kind/size %d", r.kind)
	}
	switch r.kind {
	case 1:
		validateLimits(b)
	case 2:
		need(len(b) >= 40, "Outputs prefix")
		nonzero64(b, 0, 8, 16, 24)
		n := int(u32(b, 32))
		need(n <= 16 && len(b) == 40+40*n && zero(b[36:40]), "Outputs count/reserved")
		seen := map[uint64]bool{}
		for i := 0; i < n; i++ {
			v := b[40+40*i : 80+40*i]
			nonzero64(v, 0, 8, 32)
			need(u32(v, 16) > 0 && u32(v, 20) > 0 && scale(u32(v, 24), u32(v, 28)), "output geometry")
			need(!seen[u64(v, 0)], "duplicate output")
			seen[u64(v, 0)] = true
		}
	case 16:
		need(u16(b, 0) > 0 && u64(b, 4) == r.epoch && zero(b[2:4]) && u16(b, 26) <= 1 && zero(b[28:32]), "Negotiated")
	case 17:
		need(u16(b, 0) >= 1 && u16(b, 0) <= 4 && zero(b[2:8]), "Refused")
	case 18:
		nonzero64(b, 0)
		need(u16(b, 8) >= 256 && u16(b, 8) <= 272 && zero(b[10:]), "Submitted")
	case 19:
		need(u16(b, 0) >= 1 && u16(b, 0) <= 4 && zero(b[2:8]) && u64(b, 16) != 0, "ObjectPublished")
	case 32:
		validateAllocation(b)
	case 33:
		nonzero64(b, 0, 8, 16, 24, 32)
		s := u16(b, 40)
		reason := u16(b, 42)
		need(s >= 1 && s <= 4 && reason <= 12 && (s > 2 || reason == 0) && u64(b, 48) <= 4194304, "ResourceStatus")
	case 34:
		nonzero64(b, 0, 8, 16, 24, 32)
		need(u16(b, 40) <= 12, "ResourceReleased")
	case 35:
		nonzero64(b, 0, 8, 16, 24, 32, 40)
		kind := u16(b, 48)
		reason := u16(b, 50)
		need(kind >= 1 && kind <= 4 && reason <= 12 && (kind > 2 || reason == 0) && ((kind == 2) == (u64(b, 52) != 0)), "CandidateOutcome")
	case 36:
		nonzero64(b, 0, 8, 16, 24, 32, 40)
		s := u16(b, 56)
		reason := u16(b, 58)
		need(s >= 1 && s <= 4 && reason <= 12 && u32(b, 64) <= 8192 && zero(b[68:]), "FramePermit")
		if s == 1 {
			need(u64(b, 48) > 0 && u32(b, 60) > 0 && u32(b, 60) <= 250 && u32(b, 64) > 0 && reason == 0, "granted permit")
		}
	case 37:
		nonzero64(b, 0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 104)
		kind := u16(b, 112)
		reason := u16(b, 114)
		need(kind >= 1 && kind <= 3 && reason <= 12 && zero(b[116:]), "Action")
		if kind == 1 {
			nonzero64(b, 80, 88, 96)
			need(reason == 0, "activation reason")
		}
		if kind == 2 {
			need(zero(b[80:104]) && reason == 0, "dismissal identity")
		}
	default:
		need(false, "unknown event/object")
	}
	return r, nil
}
func validateAllocation(b []byte) {
	nonzero64(b, 0, 8, 16, 40, 48)
	s := u16(b, 32)
	need(s >= 1 && s <= 4 && u16(b, 34) <= 12 && zero(b[36:40]) && zero(b[164:]), "allocation status/reserved")
	need((u64(b, 24) == 0) == (s == 4) && pair(u64(b, 56), u64(b, 64)) && pair(u64(b, 72), u64(b, 80)), "allocation identities")
	need(s == 2 || u64(b, 56) != 0, "allocation missing")
	margins(b[140:148])
	if s == 1 {
		need(u32(b, 136) <= 512, "extent")
		need(u16(b, 34) == 0 && u64(b, 88) > 0 && u32(b, 104) > 0 && u32(b, 108) > 0 && u32(b, 120) > 0 && u32(b, 124) > 0 && scale(u32(b, 128), u32(b, 132)), "granted geometry")
	}
	if s == 2 || s == 3 {
		need(zero(b[88:164]), "terminal geometry")
	}
}
func envelope(kind uint16, id, epoch uint64, b []byte) []byte {
	out := make([]byte, 32+len(b))
	p32(out, 0, uint32(len(out)))
	p16(out, 4, 1)
	p16(out, 6, kind)
	p64(out, 8, epoch)
	p64(out, 16, id)
	copy(out[32:], b)
	return out
}
func grantBody(n int, tx uint64) []byte {
	b := make([]byte, n)
	p64(b, 0, tx)
	p64(b, 8, 1)
	p64(b, 16, 1)
	return b
}
func offer() []byte {
	b := make([]byte, 16)
	p16(b, 0, 5)
	p16(b, 2, 6)
	p64(b, 8, 1|1<<7|1<<8)
	return b
}
func allocation(tx, id, output uint64) []byte {
	b := grantBody(128, tx)
	p64(b, 24, output)
	p64(b, 32, 1)
	p64(b, 40, id)
	p16(b, 48, 1)
	p16(b, 50, 1)
	p16(b, 52, 1)
	p32(b, 112, 64)
	p32(b, 116, 32)
	return b
}
func demand(tx, id uint64) []byte {
	b := grantBody(66, tx)
	p64(b, 24, 2)
	p64(b, 32, 1)
	p64(b, 56, id)
	p16(b, 64, 1)
	return b
}
func cancelDemand(tx, id, permit uint64) []byte {
	b := grantBody(56, tx)
	p64(b, 24, 2)
	p64(b, 32, 1)
	p64(b, 40, id)
	p64(b, 48, permit)
	return b
}
func resourceBegin(id uint64) []byte {
	b := make([]byte, 80)
	p64(b, 0, 70+id)
	p64(b, 16, 1)
	p64(b, 24, 1)
	p64(b, 32, id)
	p64(b, 40, 1)
	p32(b, 48, 2048)
	p32(b, 52, 8)
	p32(b, 56, 1)
	p32(b, 60, 1)
	p16(b, 64, 1)
	p32(b, 68, 2)
	p64(b, 72, 65536)
	return b
}
func resourceIdentity(n int, id, tx uint64) []byte {
	b := grantBody(n, tx)
	p64(b, 24, id)
	p64(b, 32, 1)
	return b
}
func candidate(permit, generation uint64) []byte {
	b := grantBody(80+64+32+48, 100+generation)
	p64(b, 24, generation)
	p64(b, 32, 2)
	p64(b, 40, 1)
	p64(b, 48, 4)
	p64(b, 56, permit)
	p64(b, 64, 4)
	p16(b, 72, 1)
	p16(b, 74, 1)
	p16(b, 76, 1)
	s := b[80:144]
	p64(s, 0, 1)
	p64(s, 8, 1)
	p64(s, 16, 5)
	p16(s, 24, 1)
	p16(s, 26, 1)
	p16(s, 40, 65535)
	p := b[144:176]
	p64(p, 0, 5)
	p64(p, 8, 1)
	t := b[176:]
	p16(t, 2, 1)
	p64(t, 4, 1)
	p64(t, 12, 1)
	p64(t, 20, 1)
	p32(t, 36, 2)
	p32(t, 40, 1)
	return b
}
