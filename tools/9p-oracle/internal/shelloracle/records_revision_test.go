package shelloracle

import "testing"

// Shell r9 admits ResourceStatus Rejected (status 3) reason 13,
// image_channel_unavailable. Nothing else widens, at any revision.
func TestResourceStatusReason13OnlyAtR9Rejected(t *testing.T) {
	status := func(s, reason uint16) []byte {
		b := validBodies()[33]
		le.PutUint16(b[40:], s)
		le.PutUint16(b[42:], reason)
		return inbound(33, b)
	}
	if _, err := decode(status(3, 12)); err != nil {
		t.Fatalf("reason 12 below r9: %v", err)
	}
	if _, err := decode(status(3, 13)); err == nil {
		t.Fatal("reason 13 accepted without a revision")
	}
	if _, err := decodeAt(status(3, 13), 8); err == nil {
		t.Fatal("reason 13 accepted at r8")
	}
	r, err := decodeAt(status(3, 13), 9)
	if err != nil || u16(r.body, 42) != 13 {
		t.Fatalf("reason 13 refused at r9: %v", err)
	}
	for _, c := range []struct{ s, reason uint16 }{{1, 13}, {2, 13}, {4, 13}, {3, 14}} {
		if _, err := decodeAt(status(c.s, c.reason), 9); err == nil {
			t.Errorf("status %d reason %d accepted at r9", c.s, c.reason)
		}
	}
}

func TestOtherReasonsKeepTheirBoundAtR9(t *testing.T) {
	cases := []struct {
		kind uint16
		at   int
	}{{34, 40}, {35, 50}, {36, 58}, {37, 114}}
	for _, c := range cases {
		b := validBodies()[c.kind]
		if c.kind == 35 {
			le.PutUint16(b[48:], 3)
		}
		if c.kind == 36 {
			le.PutUint16(b[56:], 2)
		}
		if c.kind == 37 {
			le.PutUint16(b[112:], 3)
		}
		le.PutUint16(b[c.at:], 12)
		if _, err := decodeAt(inbound(c.kind, b), 9); err != nil {
			t.Errorf("kind %d reason 12 refused at r9: %v", c.kind, err)
		}
		le.PutUint16(b[c.at:], 13)
		if _, err := decodeAt(inbound(c.kind, b), 9); err == nil {
			t.Errorf("kind %d reason 13 accepted at r9", c.kind)
		}
	}
}
