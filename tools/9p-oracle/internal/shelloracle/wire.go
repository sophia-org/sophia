// Package shelloracle judges bytes using the published KDL and file lifecycle
// contract. It deliberately imports no Sophia implementation or codec.
package shelloracle

import (
	"encoding/binary"
	"fmt"
	"io"
	"net"
	"time"
)

var le = binary.LittleEndian

func u16(b []byte, at int) uint16    { return le.Uint16(b[at:]) }
func u32(b []byte, at int) uint32    { return le.Uint32(b[at:]) }
func u64(b []byte, at int) uint64    { return le.Uint64(b[at:]) }
func p16(b []byte, at int, v uint16) { le.PutUint16(b[at:], v) }
func p32(b []byte, at int, v uint32) { le.PutUint32(b[at:], v) }
func p64(b []byte, at int, v uint64) { le.PutUint64(b[at:], v) }
func need(ok bool, format string, args ...any) {
	if !ok {
		panic(fmt.Errorf(format, args...))
	}
}
func must(err error) {
	if err != nil {
		panic(err)
	}
}

type response struct {
	kind byte
	body []byte
}
type request struct {
	kind  byte
	count uint32
}
type wire struct {
	conn     net.Conn
	msize    uint32
	next     uint16
	pending  map[uint16]request
	replies  map[uint16]response
	fragment bool
}

func dial(path string, msize uint32) *wire {
	c, err := net.DialTimeout("unix", path, 3*time.Second)
	must(err)
	w := &wire{conn: c, msize: msize, pending: map[uint16]request{}, replies: map[uint16]response{}}
	b := make([]byte, 14)
	p32(b, 0, msize)
	p16(b, 4, 8)
	copy(b[6:], "9P2000.L")
	w.sendTag(100, 65535, b)
	r := w.wait(65535)
	need(r.kind == 101 && len(r.body) == 14 && u16(r.body, 4) == 8 && string(r.body[6:]) == "9P2000.L", "version reply")
	w.msize = u32(r.body, 0)
	need(w.msize >= 4096 && w.msize <= msize, "negotiated msize")
	b = make([]byte, 16)
	p32(b, 0, 1)
	p32(b, 4, 0xffffffff)
	p32(b, 12, 0xffffffff)
	need(len(w.ok(104, b)) == 13, "attach shape")
	return w
}
func (w *wire) sendTag(kind byte, tag uint16, b []byte) {
	need(len(w.pending) < 16, "pending request bound")
	_, exists := w.pending[tag]
	need(!exists, "tag reused")
	need(len(b)+7 <= int(w.msize), "request exceeds msize")
	frame := make([]byte, len(b)+7)
	p32(frame, 0, uint32(len(frame)))
	frame[4] = kind
	p16(frame, 5, tag)
	copy(frame[7:], b)
	must(w.conn.SetDeadline(time.Now().Add(5 * time.Second)))
	for len(frame) > 0 {
		part := frame
		if w.fragment && len(part) > 3 {
			part = part[:3]
		}
		n, err := w.conn.Write(part)
		must(err)
		need(n > 0, "zero write")
		frame = frame[n:]
	}
	req := request{kind: kind}
	if kind == 116 || kind == 118 {
		req.count = u32(b, 12)
	}
	w.pending[tag] = req
}
func (w *wire) send(kind byte, b []byte) uint16 {
	for {
		w.next++
		if w.next == 65535 {
			w.next = 0
		}
		if _, ok := w.pending[w.next]; !ok {
			break
		}
	}
	w.sendTag(kind, w.next, b)
	return w.next
}
func (w *wire) receive() (uint16, response, error) {
	h := make([]byte, 7)
	must(w.conn.SetReadDeadline(time.Now().Add(5 * time.Second)))
	if _, err := io.ReadFull(w.conn, h); err != nil {
		return 0, response{}, err
	}
	n := u32(h, 0)
	if n < 7 || n > w.msize {
		return 0, response{}, fmt.Errorf("reply size %d", n)
	}
	tag := u16(h, 5)
	want, ok := w.pending[tag]
	if !ok {
		return 0, response{}, fmt.Errorf("unknown or flushed tag %d", tag)
	}
	if _, ok := w.replies[tag]; ok {
		return 0, response{}, fmt.Errorf("duplicate reply")
	}
	r := response{kind: h[4], body: make([]byte, n-7)}
	_, err := io.ReadFull(w.conn, r.body)
	if err != nil {
		return 0, r, err
	}
	if r.kind == 7 {
		if len(r.body) != 4 {
			return 0, r, fmt.Errorf("Rlerror length")
		}
	} else if r.kind != want.kind+1 {
		return 0, r, fmt.Errorf("reply kind %d for %d", r.kind, want.kind)
	} else if r.kind == 117 || r.kind == 119 {
		if len(r.body) < 4 || u32(r.body, 0) > want.count {
			return 0, r, fmt.Errorf("reply count exceeds request")
		}
		if (r.kind == 117 && uint64(u32(r.body, 0))+4 != uint64(len(r.body))) ||
			(r.kind == 119 && len(r.body) != 4) {
			return 0, r, fmt.Errorf("reply count/shape")
		}
	}
	return tag, r, nil
}
func (w *wire) wait(tag uint16) response {
	for {
		if r, ok := w.replies[tag]; ok {
			delete(w.replies, tag)
			delete(w.pending, tag)
			return r
		}
		t, r, err := w.receive()
		must(err)
		w.replies[t] = r
	}
}
func (w *wire) rpc(kind byte, b []byte) response { return w.wait(w.send(kind, b)) }
func (w *wire) ok(kind byte, b []byte) []byte {
	r := w.rpc(kind, b)
	need(r.kind != 7, "request %d errno %d", kind, errno(r))
	return r.body
}
func errno(r response) uint32 {
	if r.kind == 7 && len(r.body) == 4 {
		return u32(r.body, 0)
	}
	return 0
}
func (w *wire) walk(fid uint32, names ...string) {
	b := make([]byte, 10)
	p32(b, 0, 1)
	p32(b, 4, fid)
	p16(b, 8, uint16(len(names)))
	for _, name := range names {
		part := make([]byte, 2+len(name))
		p16(part, 0, uint16(len(name)))
		copy(part[2:], name)
		b = append(b, part...)
	}
	r := w.ok(110, b)
	need(len(r) == 2+13*len(names) && int(u16(r, 0)) == len(names), "walk shape")
}
func (w *wire) open(fid, mode uint32) response {
	b := make([]byte, 8)
	p32(b, 0, fid)
	p32(b, 4, mode)
	return w.rpc(12, b)
}
func (w *wire) file(fid, mode uint32, names ...string) uint64 {
	w.walk(fid, names...)
	r := w.open(fid, mode)
	need(r.kind == 13 && len(r.body) == 17, "open %v errno %d", names, errno(r))
	return u64(r.body, 5)
}
func (w *wire) clunk(fid uint32) {
	b := make([]byte, 4)
	p32(b, 0, fid)
	need(len(w.ok(120, b)) == 0, "clunk shape")
}
func readBody(fid uint32, offset uint64, count uint32) []byte {
	b := make([]byte, 16)
	p32(b, 0, fid)
	p64(b, 4, offset)
	p32(b, 12, count)
	return b
}
func (w *wire) read(fid uint32, off uint64, count uint32) response {
	return w.rpc(116, readBody(fid, off, count))
}
func data(r response) []byte {
	need(r.kind == 117 && len(r.body) >= 4, "read errno %d", errno(r))
	need(int(u32(r.body, 0)) == len(r.body)-4, "read count")
	return r.body[4:]
}
func (w *wire) write(fid uint32, off uint64, bytes []byte) response {
	return w.rpc(118, append(readBody(fid, off, uint32(len(bytes))), bytes...))
}
func wrote(r response) uint32 {
	need(r.kind == 119 && len(r.body) == 4, "write errno %d", errno(r))
	return u32(r.body, 0)
}
func (w *wire) flush(tag uint16) {
	b := make([]byte, 2)
	p16(b, 0, tag)
	need(len(w.ok(108, b)) == 0, "flush shape")
	delete(w.pending, tag)
	delete(w.replies, tag)
}
func (w *wire) getattr(fid uint32) []byte {
	b := make([]byte, 12)
	p32(b, 0, fid)
	p64(b, 4, ^uint64(0))
	r := w.ok(24, b)
	need(len(r) == 153, "getattr shape")
	return r
}
