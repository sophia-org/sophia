package shelloracle

import (
	"encoding/hex"
	"testing"
)

// Fixture bytes are hand-authored from the KDL offsets. These helpers never
// call a production encoder, read another language's vectors or generate code.
func inbound(kind uint16, b []byte) []byte {
	h, _ := hex.DecodeString("0000000001000000110000000000000000000000000000000100000000000000")
	le.PutUint32(h, uint32(32+len(b)))
	le.PutUint16(h[6:], kind)
	if kind < 16 {
		h[24] = 0
	}
	return append(h, b...)
}
func minimalLimits() []byte {
	b := make([]byte, 264)
	for _, at := range []int{0, 8, 16, 24, 32, 40, 48, 64} {
		le.PutUint64(b[at:], 1)
	}
	le.PutUint64(b, 17)
	le.PutUint64(b[56:], 3)
	for at, v := range map[int]uint32{80: 52, 84: 4, 88: 1, 92: 1, 96: 1, 100: 1, 104: 1, 108: 1, 112: 1, 116: 1, 128: 1, 132: 1, 140: 40, 148: 1, 152: 1, 156: 1, 160: 1, 168: 1, 172: 1, 176: 1024, 180: 76, 184: 1024, 188: 1, 212: 1, 216: 1, 220: 1, 224: 1, 228: 1, 232: 1, 236: 1, 240: 1, 244: 1, 248: 1, 252: 1, 256: 1} {
		le.PutUint32(b[at:], v)
	}
	return b
}
func validBodies() map[uint16][]byte {
	bodies := map[uint16][]byte{1: minimalLimits()}
	// Full negotiated body: revision 6, epoch 17, required bits 0/7/8, limits.
	bodies[16], _ = hex.DecodeString("0600000011000000000000008101000000000000000000000000010000000000")
	bodies[17], _ = hex.DecodeString("01000000000000008001000000000000")
	bodies[18], _ = hex.DecodeString("02000000000000000101000000000000")
	bodies[19], _ = hex.DecodeString("020000000000000003000000000000004200000000000000")
	outputs := make([]byte, 80)
	for at, v := range map[int]uint64{0: 5, 8: 17, 16: 1, 24: 3, 40: 2, 48: 1, 72: 5} {
		le.PutUint64(outputs[at:], v)
	}
	for at, v := range map[int]uint32{32: 1, 56: 64, 60: 32, 64: 1, 68: 1} {
		le.PutUint32(outputs[at:], v)
	}
	bodies[2] = outputs
	for kind, n := range map[uint16]int{32: 168, 33: 56, 34: 42, 35: 76, 36: 72, 37: 120} {
		b := make([]byte, n)
		for _, at := range []int{0, 8, 16, 24, 32} {
			le.PutUint64(b[at:], 1)
		}
		le.PutUint64(b[8:], 17)
		switch kind {
		case 32:
			le.PutUint16(b[32:], 2)
			le.PutUint64(b[40:], 2)
			le.PutUint64(b[48:], 1)
		case 33:
			le.PutUint16(b[40:], 1)
		case 35:
			le.PutUint64(b[40:], 1)
			le.PutUint16(b[48:], 1)
		case 36:
			le.PutUint64(b[40:], 1)
			le.PutUint16(b[56:], 2)
			le.PutUint64(b[48:], 99)
			le.PutUint32(b[60:], 250)
			le.PutUint32(b[64:], 8192)
		case 37:
			for _, at := range []int{40, 48, 56, 64, 72, 80, 88, 96, 104} {
				le.PutUint64(b[at:], 1)
			}
			le.PutUint16(b[112:], 1)
		}
		bodies[kind] = b
	}
	return bodies
}
func TestEveryIncomingKindAndTruncatedBody(t *testing.T) {
	for kind, b := range validBodies() {
		if _, err := decode(inbound(kind, b)); err != nil {
			t.Fatalf("kind %d valid body: %v", kind, err)
		}
		for n := 0; n < len(b); n++ {
			if _, err := decode(inbound(kind, b[:n])); err == nil {
				t.Fatalf("kind %d truncated to %d accepted", kind, n)
			}
		}
		if _, err := decode(inbound(kind, append(append([]byte{}, b...), 0))); err == nil {
			t.Fatalf("kind %d trailing byte", kind)
		}
	}
}
func TestConditionalRules(t *testing.T) {
	cases := []struct {
		kind  uint16
		at    int
		value uint64
		width int
	}{
		{16, 4, 18, 8}, {16, 26, 2, 2}, {16, 2, 1, 2}, {17, 0, 5, 2}, {18, 8, 273, 2}, {19, 16, 0, 8},
		{32, 140, 65535, 2}, {32, 56, 1, 8}, {32, 24, 0, 8}, {32, 104, 1, 4},
		{33, 42, 1, 2}, {33, 40, 5, 2}, {33, 48, 4194305, 8}, {34, 40, 13, 2},
		{35, 52, 1, 8}, {35, 50, 1, 2}, {36, 56, 5, 2}, {36, 64, 8193, 4}, {36, 68, 1, 4},
		{37, 80, 0, 8}, {37, 112, 2, 2}, {37, 114, 1, 2}, {2, 64, 33, 4},
	}
	for _, c := range cases {
		b := validBodies()[c.kind]
		switch c.width {
		case 2:
			le.PutUint16(b[c.at:], uint16(c.value))
		case 4:
			le.PutUint32(b[c.at:], uint32(c.value))
		case 8:
			le.PutUint64(b[c.at:], c.value)
		}
		if _, err := decode(inbound(c.kind, b)); err == nil {
			t.Errorf("kind %d offset %d accepted", c.kind, c.at)
		}
	}
	// Invalidated geometry is deliberately meaningless, unlike rejected/released.
	permit := validBodies()[36]
	le.PutUint16(permit[56:], 1)
	le.PutUint32(permit[60:], 0)
	if _, err := decode(inbound(36, permit)); err == nil {
		t.Fatal("zero granted TTL")
	}
	outputs := validBodies()[2]
	le.PutUint32(outputs[64:], 2)
	le.PutUint32(outputs[68:], 2)
	if _, err := decode(inbound(2, outputs)); err == nil {
		t.Fatal("unreduced scale")
	}
	b := validBodies()[32]
	le.PutUint16(b[32:], 4)
	le.PutUint64(b[24:], 0)
	le.PutUint64(b[56:], 1)
	le.PutUint64(b[64:], 1)
	le.PutUint32(b[128:], 0xffffffff)
	le.PutUint32(b[136:], 0xffffffff)
	if _, err := decode(inbound(32, b)); err != nil {
		t.Fatal(err)
	}
	for state := uint16(2); state <= 4; state++ {
		permit := validBodies()[36]
		le.PutUint16(permit[56:], state)
		le.PutUint32(permit[60:], 0xffffffff)
		if _, err := decode(inbound(36, permit)); err != nil {
			t.Fatalf("meaningless TTL for state %d: %v", state, err)
		}
	}
}
func TestLimitsRelations(t *testing.T) {
	for _, change := range []struct {
		at    int
		value uint64
		width int
	}{
		{100, 0, 4}, {112, 0, 4}, {148, 0, 4}, {156, 0, 4}, {184, 1023, 4}, {180, 75, 4}, {84, 3, 4}, {80, 51, 4},
		{32, 0, 8}, {40, 0, 8}, {48, 0, 8}, {56, 2, 8}, {200, 1, 4}, {228, 2, 4}, {64, 0, 8}, {72, 1, 8},
		{176, 1023, 4}, {140, 39, 4}, {168, 0, 4}, {172, 0, 4}, {220, 0, 4}, {260, 1, 4},
	} {
		b := minimalLimits()
		if change.width == 4 {
			le.PutUint32(b[change.at:], uint32(change.value))
		} else {
			le.PutUint64(b[change.at:], change.value)
		}
		if _, err := decode(inbound(1, b)); err == nil {
			t.Errorf("Limits offset %d accepted", change.at)
		}
	}
}
func TestEnvelopeAndAPI(t *testing.T) {
	b := inbound(17, validBodies()[17])
	for _, at := range []int{0, 4, 8, 24} {
		bad := append([]byte{}, b...)
		for i := 0; i < 8 && at+i < len(bad); i++ {
			bad[at+i] = 0
		}
		if _, err := decode(bad); err == nil {
			t.Errorf("header %d accepted", at)
		}
	}
	good := "sophia-shell-files version=1 role=legacy epoch=17 fd_transfer=none\n"
	if _, epoch, err := parseAPI([]byte(good)); err != nil || epoch != 17 {
		t.Fatal(epoch, err)
	}
	for _, bad := range []string{"", good + "\n", " " + good, "sophia-shell-files version=1 role=legacy epoch=0 fd_transfer=none\n", "sophia-shell-files version=1 role=legacy epoch=017 fd_transfer=none\n", "sophia-shell-files version=1 role=legacy epoch=18446744073709551616 fd_transfer=none\n"} {
		if _, _, err := parseAPI([]byte(bad)); err == nil {
			t.Errorf("accepted api %q", bad)
		}
	}
}
