package shelloracle

import (
	"bufio"
	"bytes"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"time"
)

type session struct {
	w                               *wire
	runner                          *runner
	fixture                         string
	epoch, offset, sequence, nextID uint64
	queue                           []record
	last                            []byte
}

var apiLine = regexp.MustCompile(`^sophia-shell-files version=1 role=([a-z][a-z0-9_-]*) epoch=([1-9][0-9]*) fd_transfer=none\n$`)

func parseAPI(b []byte) (string, uint64, error) {
	m := apiLine.FindSubmatch(b)
	if m == nil {
		return "", 0, fmt.Errorf("invalid api line %q", b)
	}
	epoch, err := strconv.ParseUint(string(m[2]), 10, 64)
	if err != nil {
		return "", 0, err
	}
	return string(m[1]), epoch, nil
}
func (r *runner) connect(name string, msize uint32) *session {
	(&session{runner: r, fixture: name}).phase("start")
	endpoint, err := os.ReadFile(filepath.Join(r.root, name+".endpoint"))
	must(err)
	need(len(endpoint) > 0 && len(endpoint) < 256, "endpoint length")
	w := dial(string(endpoint), msize)
	s := &session{w: w, runner: r, fixture: name, nextID: 1}
	// Read the complete bounded line, including EOF, before emitting any records.
	w.file(10, 0, "api")
	var api []byte
	for {
		part := data(w.read(10, uint64(len(api)), 256))
		if len(part) == 0 {
			break
		}
		need(len(api)+len(part) <= 256, "api size")
		api = append(api, part...)
	}
	role, epoch, err := parseAPI(api)
	must(err)
	need(role == "bar", "fixture api role %q", role)
	s.epoch = epoch
	w.clunk(10)
	w.file(2, 0, "events")
	w.file(3, 1, "submit")
	w.file(4, 1, "ack")
	return s
}
func (s *session) close() { s.w.conn.Close() }
func (s *session) phase(op string) {
	c, err := net.DialTimeout("unix", filepath.Join(s.runner.root, "control.sock"), 3*time.Second)
	must(err)
	defer c.Close()
	must(c.SetDeadline(time.Now().Add(5 * time.Second)))
	_, err = fmt.Fprintf(c, "%s %s\n", s.fixture, op)
	must(err)
	line, err := bufio.NewReader(io.LimitReader(c, 256)).ReadString('\n')
	must(err)
	need(line == "ok\n", "fixture phase %s: %s", op, line)
}
func (s *session) encoded(kind uint16, id uint64, body []byte) []byte {
	b := append([]byte(nil), body...)
	if kind != 256 {
		at := 8
		if kind == 258 {
			at = 16
		}
		p64(b, at, s.epoch)
	}
	if kind >= 266 && kind <= 272 {
		raw, err := encodeRole(kind, id, s.epoch, b)
		must(err)
		return raw
	}
	return envelope(kind, id, s.epoch, b)
}
func (s *session) stage(raw []byte, split int) {
	s.w.file(5, 2, "transaction")
	if split <= 0 || split > int(s.w.msize)-23 {
		split = int(s.w.msize) - 23
	}
	for off := 0; off < len(raw); {
		end := off + split
		if end > len(raw) {
			end = len(raw)
		}
		n := wrote(s.w.write(5, uint64(off), raw[off:end]))
		need(n > 0 && int(n) <= end-off, "transaction short write")
		off += int(n)
	}
}
func (s *session) submitRaw(raw []byte) response {
	b := make([]byte, 24)
	p64(b, 0, s.epoch)
	p64(b, 8, u64(raw, 16))
	p32(b, 16, uint32(len(raw)))
	return s.w.write(3, 0, b)
}
func (s *session) submit(kind uint16, b []byte) uint64 {
	id := s.nextID
	s.nextID++
	s.last = s.encoded(kind, id, b)
	s.stage(s.last, 0)
	need(wrote(s.submitRaw(s.last)) == 24, "submit count")
	s.w.clunk(5)
	return id
}
func (s *session) readEvents() []byte {
	raw := data(s.w.read(2, s.offset, s.w.msize-11))
	need(len(raw) > 0, "empty events read")
	s.offset += uint64(len(raw))
	rest := raw
	for len(rest) > 0 {
		need(len(rest) >= 32, "partial event header")
		n := int(u32(rest, 0))
		need(n >= 32 && n <= len(rest), "partial event")
		e, err := decode(append([]byte(nil), rest[:n]...))
		must(err)
		need(e.kind >= 16 && e.epoch == s.epoch && e.sequence > s.sequence, "event epoch/sequence")
		if e.kind >= 32 {
			need(u64(e.body, 8) == s.epoch, "event grant epoch")
		}
		s.sequence = e.sequence
		s.queue = append(s.queue, e)
		need(len(s.queue) <= 256, "event queue bound")
		rest = rest[n:]
	}
	return raw
}
func (s *session) next() record {
	if len(s.queue) == 0 {
		s.readEvents()
	}
	e := s.queue[0]
	s.queue = s.queue[1:]
	return e
}
func (s *session) expect(kind uint16) record {
	e := s.next()
	need(e.kind == kind, "event kind %d want %d", e.kind, kind)
	return e
}
func (s *session) custody(id uint64, kind uint16) record {
	e := s.expect(18)
	need(u64(e.body, 0) == id && u16(e.body, 8) == kind, "Submitted correlation")
	return e
}
func (s *session) ack(e record) {
	b := make([]byte, 16)
	p64(b, 0, s.epoch)
	p64(b, 8, e.sequence)
	need(wrote(s.w.write(4, 0, b)) == 16, "ack count")
}
func (s *session) negotiate() record {
	id := s.submit(256, offer())
	s.ack(s.custody(id, 256))
	e := s.expect(16)
	need(u16(e.body, 0) == 6 && u64(e.body, 4) == s.epoch && u64(e.body, 12)&385 == 385 && u16(e.body, 26) == 1, "selected profile")
	s.ack(e)
	return e
}
func (s *session) object(fid uint32) record {
	var raw []byte
	for {
		part := data(s.w.read(fid, uint64(len(raw)), 1024))
		if len(part) == 0 {
			break
		}
		need(len(raw)+len(part) <= 1024, "object cap")
		raw = append(raw, part...)
	}
	e, err := decode(raw)
	must(err)
	need(e.epoch == s.epoch, "object epoch")
	return e
}
func (s *session) noEvents() {
	need(len(s.queue) == 0, "unexpected queued event")
	tag := s.w.send(116, readBody(2, s.offset, 1024))
	s.w.getattr(1)
	_, answered := s.w.replies[tag]
	need(!answered, "unexpected journal entry")
	s.w.flush(tag)
	s.w.getattr(1)
}
func (s *session) status(id uint64, state uint16) record {
	e := s.expect(33)
	need(u64(e.body, 24) == id && u16(e.body, 40) == state, "resource %d status %d", u64(e.body, 24), u16(e.body, 40))
	s.ack(e)
	return e
}
func identical(a, b []byte) bool { return bytes.Equal(a, b) }
