package shelloracle

import (
	"fmt"
	"io"
	"net"
	"testing"
	"time"
)

// A bounded scripted 9P peer: it answers each Tread on the events fid with
// the next scripted payload, then closes. No production endpoint is used.
func scriptedEvents(t *testing.T, reads [][]byte) (*session, func()) {
	t.Helper()
	client, server := net.Pipe()
	done := make(chan struct{})
	go func() {
		defer close(done)
		defer server.Close()
		_ = server.SetDeadline(time.Now().Add(5 * time.Second))
		for _, payload := range reads {
			h := make([]byte, 7)
			if _, err := io.ReadFull(server, h); err != nil {
				return
			}
			body := make([]byte, u32(h, 0)-7)
			if _, err := io.ReadFull(server, body); err != nil || h[4] != 116 {
				return
			}
			reply := make([]byte, 11, 11+len(payload))
			p32(reply, 0, uint32(11+len(payload)))
			reply[4] = 117
			p16(reply, 5, u16(h, 5))
			p32(reply, 7, uint32(len(payload)))
			if _, err := server.Write(append(reply, payload...)); err != nil {
				return
			}
		}
	}()
	w := &wire{conn: client, msize: 8192, pending: map[uint16]request{}, replies: map[uint16]response{}}
	return &session{w: w, epoch: 17}, func() {
		client.Close()
		<-done
	}
}

func event(kind uint16, body []byte, epoch, sequence uint64) []byte {
	b := inbound(kind, body)
	p64(b, 8, epoch)
	p64(b, 24, sequence)
	return b
}

func negotiatedAt(revision uint16, epoch uint64) []byte {
	b := validBodies()[16]
	p16(b, 0, revision)
	p64(b, 4, epoch)
	return b
}

func unavailable() []byte {
	b := validBodies()[33]
	p16(b, 40, 3)
	p16(b, 42, 13)
	return b
}

// readAll drives one readEvents per scripted read and reports a refusal
// instead of letting the oracle's panic escape the test.
func readAll(s *session, reads int) (err error) {
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("%v", p)
		}
	}()
	for i := 0; i < reads; i++ {
		s.readEvents()
	}
	return nil
}

func TestSessionRevisionFromNegotiated(t *testing.T) {
	both := func(revision uint16) []byte {
		return append(event(16, negotiatedAt(revision, 17), 17, 1), event(33, unavailable(), 17, 2)...)
	}
	cases := []struct {
		name  string
		reads [][]byte
		ok    bool
		want  uint16
	}{
		{"r9 one read", [][]byte{both(9)}, true, 9},
		{"r9 split reads", [][]byte{event(16, negotiatedAt(9, 17), 17, 1), event(33, unavailable(), 17, 2)}, true, 9},
		{"r8 one read", [][]byte{both(8)}, false, 8},
		{"r8 split reads", [][]byte{event(16, negotiatedAt(8, 17), 17, 1), event(33, unavailable(), 17, 2)}, false, 8},
		{"no Negotiated", [][]byte{event(33, unavailable(), 17, 1)}, false, 0},
	}
	for _, c := range cases {
		s, stop := scriptedEvents(t, c.reads)
		err := readAll(s, len(c.reads))
		stop()
		if (err == nil) != c.ok || s.revision != c.want {
			t.Errorf("%s: err %v revision %d", c.name, err, s.revision)
		}
		if c.ok && len(s.queue) != 2 {
			t.Errorf("%s: queued %d", c.name, len(s.queue))
		}
	}
}

// A Negotiated that fails epoch or sequence validation cannot select r9.
func TestInvalidNegotiatedLeavesRevision(t *testing.T) {
	for _, c := range []struct {
		name     string
		read     []byte
		sequence uint64
	}{
		{"foreign epoch", event(16, negotiatedAt(9, 18), 18, 6), 5},
		{"stale sequence", event(16, negotiatedAt(9, 17), 17, 5), 5},
	} {
		s, stop := scriptedEvents(t, [][]byte{c.read})
		s.revision, s.sequence = 6, c.sequence
		err := readAll(s, 1)
		stop()
		if err == nil || s.revision != 6 || s.sequence != c.sequence || len(s.queue) != 0 {
			t.Errorf("%s: err %v revision %d sequence %d queued %d", c.name, err, s.revision, s.sequence, len(s.queue))
		}
	}
}
