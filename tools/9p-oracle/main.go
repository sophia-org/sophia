// Command 9p-oracle is the independent conformance oracle for Sophia's
// 9P2000.L core (crates/sophia-9p). It is not built from, and shares no code
// with, the Rust implementation.
//
// Two scenarios run against a server serving the C1 static test export:
//
//   - client: the pinned third-party client, github.com/hugelgupf/p9 v0.4.1
//     (Apache-2.0), performs version, attach, walk, open, read, readdir,
//     write, getattr and clunk, and observes the server's errors.
//   - raw: frames written here from the 9P2000.L specification
//     (diod protocol.md) exercise what that client never sends: flush,
//     malformed and oversize frames, tag rules and version edge cases.
//
// Each check prints one line. The last line is the verdict:
//
//	sophia_9p_oracle schema=1 status=pass|fail checks=N failed=M
package main

import (
	"bytes"
	"encoding/binary"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"time"

	"github.com/hugelgupf/p9/linux"
	"github.com/hugelgupf/p9/p9"
)

const (
	notag   = 0xffff
	nofid   = 0xffffffff
	infoLen = 70000
)

type oracle struct {
	socket string
	checks int
	failed int
}

func (o *oracle) check(name string, err error) {
	o.checks++
	if err != nil {
		o.failed++
		fmt.Printf("check %s FAIL: %v\n", name, err)
		return
	}
	fmt.Printf("check %s ok\n", name)
}

func wantErrno(err error, want linux.Errno) error {
	if !errors.Is(err, want) {
		return fmt.Errorf("error %v, want %v", err, want)
	}
	return nil
}

func infoByte(offset int) byte { return byte(offset % 251) }

func main() {
	socket := flag.String("socket", "", "server socket path")
	flag.Parse()
	if *socket == "" {
		fmt.Fprintln(os.Stderr, "usage: 9p-oracle -socket PATH")
		os.Exit(2)
	}
	o := &oracle{socket: *socket}
	o.clientScenario()
	o.rawScenario()
	status := "pass"
	if o.failed != 0 || o.checks == 0 {
		status = "fail"
	}
	fmt.Printf("sophia_9p_oracle schema=1 status=%s checks=%d failed=%d\n", status, o.checks, o.failed)
	if status != "pass" {
		os.Exit(1)
	}
}

// ---- client scenario: the pinned third-party client ----

func (o *oracle) clientScenario() {
	conn, err := net.Dial("unix", o.socket)
	if err != nil {
		o.check("client/dial", err)
		return
	}
	conn.SetDeadline(time.Now().Add(20 * time.Second))
	client, err := p9.NewClient(conn)
	o.check("client/version", err)
	if err != nil {
		return
	}
	defer client.Close()

	root, err := client.Attach("")
	o.check("client/attach", err)
	if err != nil {
		return
	}
	_, err = client.Attach("wm")
	o.check("client/attach-unknown-export", wantErrno(err, linux.ENOENT))

	rootQID, _, rootAttr, err := root.GetAttr(p9.AttrMaskAll)
	o.check("client/getattr-root", func() error {
		if err != nil {
			return err
		}
		if rootQID.Type != p9.TypeDir || uint32(rootAttr.Mode) != 0o040555 {
			return fmt.Errorf("root qid %v mode %o", rootQID, rootAttr.Mode)
		}
		return nil
	}())

	o.check("client/walk-open-read", func() error {
		qids, leaf, err := root.Walk([]string{"dir", "leaf"})
		if err != nil {
			return err
		}
		if len(qids) != 2 || qids[0].Type != p9.TypeDir || qids[1].Type != p9.TypeRegular {
			return fmt.Errorf("qids %v", qids)
		}
		_, iounit, err := leaf.Open(p9.ReadOnly)
		if err != nil {
			return err
		}
		if iounit != 65536-24 {
			return fmt.Errorf("iounit %d", iounit)
		}
		buffer := make([]byte, 64)
		n, err := leaf.ReadAt(buffer, 0)
		if err != nil || !bytes.Equal(buffer[:n], []byte{0, 1, 2, 3}) {
			return fmt.Errorf("read %v %v", buffer[:n], err)
		}
		if _, _, err := leaf.Walk(nil); !errors.Is(err, linux.EBADF) {
			return fmt.Errorf("walk from an open fid: %v, want EBADF", err)
		}
		return leaf.Close()
	}())

	o.check("client/large-read", func() error {
		_, info, err := root.Walk([]string{"info"})
		if err != nil {
			return err
		}
		defer info.Close()
		_, _, attr, err := info.GetAttr(p9.AttrMaskAll)
		if err != nil {
			return err
		}
		if uint32(attr.Mode) != 0o100444 || attr.Size != infoLen {
			return fmt.Errorf("info mode %o size %d", attr.Mode, attr.Size)
		}
		if _, _, err := info.Open(p9.ReadOnly); err != nil {
			return err
		}
		buffer := make([]byte, infoLen+100)
		n, err := info.ReadAt(buffer, 0)
		if n != infoLen || (err != nil && err != io.EOF) {
			return fmt.Errorf("read %d bytes: %v", n, err)
		}
		for i := 0; i < n; i++ {
			if buffer[i] != infoByte(i) {
				return fmt.Errorf("byte %d is %d", i, buffer[i])
			}
		}
		return nil
	}())

	o.check("client/parent-of-root-is-root", func() error {
		qids, up, err := root.Walk([]string{"..", "dir", ".."})
		if err != nil {
			return err
		}
		defer up.Close()
		if len(qids) != 3 || qids[0] != rootQID || qids[2] != rootQID {
			return fmt.Errorf("qids %v, root %v", qids, rootQID)
		}
		return nil
	}())

	o.check("client/walk-first-name-missing", func() error {
		_, _, err := root.Walk([]string{"missing", "leaf"})
		return wantErrno(err, linux.ENOENT)
	}())

	// The client keeps a file for a partial walk; the server created no fid,
	// so the file names nothing.
	o.check("client/partial-walk-creates-no-fid", func() error {
		qids, partial, err := root.Walk([]string{"dir", "missing"})
		if err != nil {
			return err
		}
		if len(qids) != 1 || qids[0].Type != p9.TypeDir {
			return fmt.Errorf("qids %v", qids)
		}
		if _, _, _, err := partial.GetAttr(p9.AttrMaskAll); !errors.Is(err, linux.EBADF) {
			return fmt.Errorf("getattr on the partial walk's fid: %v, want EBADF", err)
		}
		return wantErrno(partial.Close(), linux.EBADF)
	}())

	o.check("client/owner-refuses-hidden", func() error {
		_, _, err := root.Walk([]string{"hidden"})
		return wantErrno(err, linux.EACCES)
	}())

	o.check("client/write-sink", func() error {
		_, sink, err := root.Walk([]string{"sink"})
		if err != nil {
			return err
		}
		defer sink.Close()
		if _, _, err := sink.Open(p9.WriteOnly); err != nil {
			return err
		}
		n, err := sink.WriteAt([]byte("hello"), 0)
		if err != nil || n != 5 {
			return fmt.Errorf("wrote %d: %v", n, err)
		}
		_, reader, err := root.Walk([]string{"sink"})
		if err != nil {
			return err
		}
		defer reader.Close()
		_, _, err = reader.Open(p9.ReadOnly)
		return wantErrno(err, linux.EACCES)
	}())

	o.check("client/directory-not-writable", func() error {
		_, dir, err := root.Walk([]string{"dir"})
		if err != nil {
			return err
		}
		defer dir.Close()
		_, _, err = dir.Open(p9.WriteOnly)
		return wantErrno(err, linux.EISDIR)
	}())

	o.check("client/readdir-lists-root", func() error {
		_, dir, err := root.Walk(nil)
		if err != nil {
			return err
		}
		defer dir.Close()
		if _, _, err := dir.Open(p9.ReadOnly); err != nil {
			return err
		}
		entries, err := dir.Readdir(0, 1024)
		if err != nil {
			return err
		}
		// hidden is refused by the owner, so it is not listed. The type is
		// the Linux d_type: 8 a regular file, 4 a directory.
		want := []struct {
			name  string
			qid   p9.QIDType
			dtype uint8
		}{
			{"info", p9.TypeRegular, 8},
			{"sink", p9.TypeRegular, 8},
			{"events", p9.TypeRegular, 8},
			{"dir", p9.TypeDir, 4},
		}
		if len(entries) != len(want) {
			return fmt.Errorf("entries %v", entries)
		}
		for i, w := range want {
			e := entries[i]
			if e.Name != w.name || e.QID.Type != w.qid || uint8(e.Type) != w.dtype || e.Offset != uint64(i+1) {
				return fmt.Errorf("entry %d is %+v, want %+v at offset %d", i, e, w, i+1)
			}
		}
		qids, walked, err := root.Walk([]string{"dir"})
		if err != nil {
			return err
		}
		walked.Close()
		if qids[0] != entries[3].QID {
			return fmt.Errorf("dir walks to %v, listed as %v", qids[0], entries[3].QID)
		}
		end, err := dir.Readdir(entries[3].Offset, 1024)
		if err != nil || len(end) != 0 {
			return fmt.Errorf("after the last entry: %v %v", end, err)
		}
		return nil
	}())

	o.check("client/readdir-resumes-at-an-offset", func() error {
		_, dir, err := root.Walk(nil)
		if err != nil {
			return err
		}
		defer dir.Close()
		if _, _, err := dir.Open(p9.ReadOnly); err != nil {
			return err
		}
		// One entry for "info" is 24 + 4 bytes.
		first, err := dir.Readdir(0, 28)
		if err != nil || len(first) != 1 || first[0].Name != "info" {
			return fmt.Errorf("first page %v %v", first, err)
		}
		rest, err := dir.Readdir(first[0].Offset, 1024)
		if err != nil || len(rest) != 3 || rest[0].Name != "sink" || rest[2].Name != "dir" {
			return fmt.Errorf("rest %v %v", rest, err)
		}
		return nil
	}())

	o.check("client/clunk-root", root.Close())
}

// ---- raw scenario: frames written from the specification ----

type raw struct {
	conn net.Conn
}

func (o *oracle) dial() (*raw, error) {
	conn, err := net.Dial("unix", o.socket)
	if err != nil {
		return nil, err
	}
	conn.SetDeadline(time.Now().Add(5 * time.Second))
	return &raw{conn: conn}, nil
}

type body struct{ bytes.Buffer }

func (b *body) u8(v uint8) *body   { b.WriteByte(v); return b }
func (b *body) u16(v uint16) *body { binary.Write(&b.Buffer, binary.LittleEndian, v); return b }
func (b *body) u32(v uint32) *body { binary.Write(&b.Buffer, binary.LittleEndian, v); return b }
func (b *body) u64(v uint64) *body { binary.Write(&b.Buffer, binary.LittleEndian, v); return b }
func (b *body) str(s string) *body { b.u16(uint16(len(s))); b.WriteString(s); return b }

func frame(kind uint8, tag uint16, b *body) []byte {
	payload := b.Bytes()
	out := make([]byte, 7, 7+len(payload))
	binary.LittleEndian.PutUint32(out, uint32(7+len(payload)))
	out[4] = kind
	binary.LittleEndian.PutUint16(out[5:], tag)
	return append(out, payload...)
}

type reply struct {
	kind uint8
	tag  uint16
	body []byte
}

func (r *raw) send(f []byte) error {
	_, err := r.conn.Write(f)
	return err
}

func (r *raw) recv() (reply, error) {
	var size [4]byte
	if _, err := io.ReadFull(r.conn, size[:]); err != nil {
		return reply{}, err
	}
	n := binary.LittleEndian.Uint32(size[:])
	if n < 7 {
		return reply{}, fmt.Errorf("reply size %d", n)
	}
	rest := make([]byte, n-4)
	if _, err := io.ReadFull(r.conn, rest); err != nil {
		return reply{}, err
	}
	return reply{kind: rest[0], tag: binary.LittleEndian.Uint16(rest[1:3]), body: rest[3:]}, nil
}

// closed reports whether the server ended the connection without replying.
func (r *raw) closed() error {
	var b [1]byte
	n, err := r.conn.Read(b[:])
	if n == 0 && err == io.EOF {
		return nil
	}
	return fmt.Errorf("connection still open (read %d, %v)", n, err)
}

func (r reply) errno() (uint32, bool) {
	if r.kind != 7 || len(r.body) != 4 {
		return 0, false
	}
	return binary.LittleEndian.Uint32(r.body), true
}

func (r *raw) expectErrno(tag uint16, want uint32) error {
	got, err := r.recv()
	if err != nil {
		return err
	}
	if e, ok := got.errno(); !ok || e != want || got.tag != tag {
		return fmt.Errorf("reply type %d tag %d body %v, want Rlerror %d tag %d", got.kind, got.tag, got.body, want, tag)
	}
	return nil
}

func (r *raw) expectKind(tag uint16, kind uint8) (reply, error) {
	got, err := r.recv()
	if err != nil {
		return got, err
	}
	if got.kind != kind || got.tag != tag {
		return got, fmt.Errorf("reply type %d tag %d body %v, want type %d tag %d", got.kind, got.tag, got.body, kind, tag)
	}
	return got, nil
}

func tversion(tag uint16, msize uint32, version string) []byte {
	return frame(100, tag, new(body).u32(msize).str(version))
}

// version negotiates; the Rversion must be exactly the one given.
func (r *raw) version(msize uint32, offered, want string, wantMsize uint32) error {
	if err := r.send(tversion(notag, msize, offered)); err != nil {
		return err
	}
	got, err := r.expectKind(notag, 101)
	if err != nil {
		return err
	}
	expected := new(body).u32(wantMsize).str(want).Bytes()
	if !bytes.Equal(got.body, expected) {
		return fmt.Errorf("Rversion body %q, want %q", got.body, expected)
	}
	return nil
}

// session: versioned at 8192, fid 0 attached, fid 1 the events file opened.
func (o *oracle) session() (*raw, error) {
	r, err := o.dial()
	if err != nil {
		return nil, err
	}
	steps := []struct {
		request []byte
		kind    uint8
	}{
		{frame(104, 1, new(body).u32(0).u32(nofid).str("").str("").u32(nofid)), 105},
		{frame(110, 2, new(body).u32(0).u32(1).u16(1).str("events")), 111},
		{frame(12, 3, new(body).u32(1).u32(0)), 13},
	}
	if err := r.version(8192, "9P2000.L", "9P2000.L", 8192); err != nil {
		return nil, err
	}
	for _, step := range steps {
		if err := r.send(step.request); err != nil {
			return nil, err
		}
		if _, err := r.expectKind(binary.LittleEndian.Uint16(step.request[5:7]), step.kind); err != nil {
			return nil, err
		}
	}
	return r, nil
}

func tread(tag uint16, fid uint32, count uint32) []byte {
	return frame(116, tag, new(body).u32(fid).u64(0).u32(count))
}

func treaddir(tag uint16, fid uint32, offset uint64, count uint32) []byte {
	return frame(40, tag, new(body).u32(fid).u64(offset).u32(count))
}

func tgetattr(tag uint16, fid uint32) []byte {
	return frame(24, tag, new(body).u32(fid).u64(0x7ff))
}

func (o *oracle) rawScenario() {
	versions := []struct {
		name, offered, want string
		msize, wantMsize    uint32
	}{
		{"raw/version-exact", "9P2000.L", "9P2000.L", 1 << 20, 65536},
		{"raw/version-google-offer-gets-plain-dialect", "9P2000.L.Google.7", "9P2000.L", 8192, 8192},
		{"raw/version-classic-unknown", "9P2000", "unknown", 8192, 8192},
		{"raw/version-dotu-unknown", "9P2000.u", "unknown", 8192, 8192},
		{"raw/version-other-suffix-unknown", "9P2000.L.foo", "unknown", 8192, 8192},
	}
	for _, v := range versions {
		o.check(v.name, func() error {
			r, err := o.dial()
			if err != nil {
				return err
			}
			defer r.conn.Close()
			return r.version(v.msize, v.offered, v.want, v.wantMsize)
		}())
	}

	o.check("raw/version-msize-below-minimum", func() error {
		r, err := o.dial()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(tversion(notag, 4095, "9P2000.L")); err != nil {
			return err
		}
		return r.expectErrno(notag, 22)
	}())

	fatal := []struct {
		name      string
		versioned bool
		request   []byte
	}{
		{"raw/request-before-version-closes", false, frame(120, 1, new(body).u32(0))},
		{"raw/notag-request-closes", true, frame(120, notag, new(body).u32(0))},
		{"raw/short-size-closes", true, []byte{6, 0, 0, 0, 120, 1, 0}},
		{"raw/oversize-frame-closes", true, []byte{0x01, 0x20, 0, 0, 120, 1, 0}},
	}
	for _, f := range fatal {
		o.check(f.name, func() error {
			r, err := o.dial()
			if err != nil {
				return err
			}
			defer r.conn.Close()
			if f.versioned {
				if err := r.version(8192, "9P2000.L", "9P2000.L", 8192); err != nil {
					return err
				}
			}
			if err := r.send(f.request); err != nil {
				return err
			}
			return r.closed()
		}())
	}

	answered := []struct {
		name    string
		request []byte
		errno   uint32
	}{
		{"raw/trailing-bytes-eproto", frame(120, 5, new(body).u32(0).u8(0)), 71},
		{"raw/short-body-eproto", frame(120, 5, new(body).u16(0)), 71},
		{"raw/walk-of-seventeen-einval", func() []byte {
			b := new(body).u32(0).u32(9).u16(17)
			for i := 0; i < 17; i++ {
				b.str("dir")
			}
			return frame(110, 5, b)
		}(), 22},
		{"raw/readdir-unopened-ebadf", treaddir(5, 0, 0, 64), 9},
		{"raw/readdir-file-enotdir", treaddir(5, 1, 0, 64), 20},
		{"raw/auth-eopnotsupp", frame(102, 5, new(body).u32(7).str("").str("").u32(nofid)), 95},
		{"raw/unassigned-type-enosys", frame(200, 5, new(body)), 38},
		{"raw/attach-with-afid-einval", frame(104, 5, new(body).u32(8).u32(3).str("").str("").u32(nofid)), 22},
		{"raw/attach-used-fid-ebadf", frame(104, 5, new(body).u32(0).u32(nofid).str("").str("").u32(nofid)), 9},
		{"raw/read-unopened-ebadf", tread(5, 0, 10), 9},
	}
	for _, a := range answered {
		o.check(a.name, func() error {
			r, err := o.session()
			if err != nil {
				return err
			}
			defer r.conn.Close()
			if err := r.send(a.request); err != nil {
				return err
			}
			if err := r.expectErrno(5, a.errno); err != nil {
				return err
			}
			// The connection is still usable.
			if err := r.send(tgetattr(6, 0)); err != nil {
				return err
			}
			_, err = r.expectKind(6, 25)
			return err
		}())
	}

	// dir opened as fid 2 for listing.
	openDir := func() (*raw, error) {
		r, err := o.session()
		if err != nil {
			return nil, err
		}
		if err := r.send(frame(110, 7, new(body).u32(0).u32(2).u16(1).str("dir"))); err != nil {
			return nil, err
		}
		if _, err := r.expectKind(7, 111); err != nil {
			return nil, err
		}
		if err := r.send(frame(12, 8, new(body).u32(2).u32(0))); err != nil {
			return nil, err
		}
		if _, err := r.expectKind(8, 13); err != nil {
			return nil, err
		}
		return r, nil
	}

	o.check("raw/readdir-entry-layout", func() error {
		r, err := openDir()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(treaddir(9, 2, 0, 64)); err != nil {
			return err
		}
		got, err := r.expectKind(9, 41)
		if err != nil {
			return err
		}
		// count[4], then qid[13] offset[8] type[1] name[s] for "leaf".
		b := got.body
		if len(b) != 4+24+4 || binary.LittleEndian.Uint32(b) != 28 {
			return fmt.Errorf("Rreaddir body %v", b)
		}
		e := b[4:]
		if e[0] != 0 || binary.LittleEndian.Uint64(e[13:21]) != 1 || e[21] != 8 ||
			binary.LittleEndian.Uint16(e[22:24]) != 4 || string(e[24:]) != "leaf" {
			return fmt.Errorf("entry %v", e)
		}
		if err := r.send(treaddir(10, 2, 1, 64)); err != nil {
			return err
		}
		end, err := r.expectKind(10, 41)
		if err != nil {
			return err
		}
		if !bytes.Equal(end.body, []byte{0, 0, 0, 0}) {
			return fmt.Errorf("after the last entry: %v", end.body)
		}
		return nil
	}())

	o.check("raw/readdir-first-entry-too-large-einval", func() error {
		r, err := openDir()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(treaddir(9, 2, 0, 27)); err != nil {
			return err
		}
		return r.expectErrno(9, 22)
	}())

	o.check("raw/flushed-read-is-never-answered", func() error {
		r, err := o.session()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(tread(9, 1, 64)); err != nil {
			return err
		}
		if err := r.send(frame(108, 10, new(body).u16(9))); err != nil {
			return err
		}
		if _, err := r.expectKind(10, 109); err != nil {
			return fmt.Errorf("first reply after the flush: %w", err)
		}
		if err := r.send(tgetattr(11, 0)); err != nil {
			return err
		}
		_, err = r.expectKind(11, 25)
		return err
	}())

	o.check("raw/zero-count-read-answers-at-once", func() error {
		r, err := o.session()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(tread(9, 1, 0)); err != nil {
			return err
		}
		got, err := r.expectKind(9, 117)
		if err != nil {
			return err
		}
		if !bytes.Equal(got.body, []byte{0, 0, 0, 0}) {
			return fmt.Errorf("Rread body %v, want count 0", got.body)
		}
		return nil
	}())

	o.check("raw/flush-of-unknown-tag-answered", func() error {
		r, err := o.session()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(frame(108, 10, new(body).u16(77))); err != nil {
			return err
		}
		_, err = r.expectKind(10, 109)
		return err
	}())

	o.check("raw/duplicate-waiting-tag-closes", func() error {
		r, err := o.session()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(tread(9, 1, 64)); err != nil {
			return err
		}
		if err := r.send(tgetattr(9, 0)); err != nil {
			return err
		}
		return r.closed()
	}())

	o.check("raw/clunk-answers-waiting-reads-first", func() error {
		r, err := o.session()
		if err != nil {
			return err
		}
		defer r.conn.Close()
		if err := r.send(tread(9, 1, 64)); err != nil {
			return err
		}
		if err := r.send(frame(120, 12, new(body).u32(1))); err != nil {
			return err
		}
		if err := r.expectErrno(9, 9); err != nil {
			return err
		}
		_, err = r.expectKind(12, 121)
		return err
	}())
}
