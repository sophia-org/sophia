package shelloracle

import (
	"net"
	"testing"
)

func TestReplyBoundsBeforeAllocation(t *testing.T) {
	for _, size := range []uint32{0, 6, 4097, 0xffffffff} {
		client, server := net.Pipe()
		w := &wire{conn: client, msize: 4096, pending: map[uint16]request{1: {kind: 116, count: 1024}}, replies: map[uint16]response{}}
		done := make(chan struct{})
		go func() {
			defer close(done)
			defer server.Close()
			h := []byte{0, 0, 0, 0, 117, 1, 0}
			le.PutUint32(h, size)
			server.Write(h)
		}()
		if _, _, err := w.receive(); err == nil {
			t.Errorf("accepted size %d", size)
		}
		client.Close()
		<-done
	}
}
func TestUniqueCheckNames(t *testing.T) {
	seen := map[string]bool{}
	if len(Names) != 54 {
		t.Fatal(len(Names))
	}
	for _, name := range Names {
		if seen[name] {
			t.Fatal(name)
		}
		seen[name] = true
	}
}

func TestReplyShapeAndCount(t *testing.T) {
	for _, tc := range []struct {
		kind  byte
		count uint32
		body  []byte
		valid bool
	}{
		{117, 1, []byte{2, 0, 0, 0, 0, 0}, false},
		{117, 2, []byte{2, 0, 0, 0, 0}, false},
		{117, 2, []byte{1, 0, 0, 0, 0}, true},
		{119, 1, []byte{2, 0, 0, 0}, false},
		{119, 2, []byte{2, 0, 0, 0, 0}, false},
		{119, 2, []byte{1, 0, 0, 0}, true},
	} {
		client, server := net.Pipe()
		w := &wire{conn: client, msize: 4096, pending: map[uint16]request{1: {kind: tc.kind - 1, count: tc.count}}, replies: map[uint16]response{}}
		done := make(chan struct{})
		go func() {
			defer close(done)
			defer server.Close()
			frame := append([]byte{0, 0, 0, 0, tc.kind, 1, 0}, tc.body...)
			le.PutUint32(frame, uint32(len(frame)))
			server.Write(frame)
		}()
		_, _, err := w.receive()
		if (err == nil) != tc.valid {
			t.Errorf("kind %d count %d body %x: %v", tc.kind, tc.count, tc.body, err)
		}
		client.Close()
		<-done
	}
}
