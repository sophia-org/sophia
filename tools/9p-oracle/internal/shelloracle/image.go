package shelloracle

import (
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"golang.org/x/sys/unix"
)

// Offsets are hand-written from sophia-shell-files-v1.kdl and
// docs/sophia-image-channel.md; no Sophia codec is imported.
const (
	kindImageBegin     = 28672
	kindImageAdmitted  = 28673
	kindSourceReleased = 28674
	kindChannelReady   = 28675
	kindChannelClosed  = 28676
	kindImageLimits    = 28677
	kindImageFormats   = 28678
	sealedCapability   = 1 << 13
	contentGrantEpoch  = 1
)

func imageKind(kind uint16) bool { return kind >= kindImageBegin && kind <= kindImageFormats }

// imageClass admits image records only at r9: ImageBegin is a candidate,
// 0x7001-0x7004 are journal events and 0x7005-0x7006 are objects.
func imageClass(r record, revision uint16) bool {
	switch {
	case revision < 9 || !imageKind(r.kind):
		return false
	case r.kind == kindImageBegin:
		return r.submission != 0 && r.sequence == 0
	case r.kind >= kindImageLimits:
		return r.submission == 0 && r.sequence == 0
	default:
		return r.submission == 0 && r.sequence != 0
	}
}

// validateImage checks one r9 image record body against its KDL bounds.
func validateImage(r record) {
	b := r.body
	sizes := map[uint16]int{kindImageBegin: 96, kindImageAdmitted: 96, kindSourceReleased: 32, kindChannelReady: 16, kindChannelClosed: 16, kindImageLimits: 104}
	if r.kind != kindImageFormats {
		need(len(b) == sizes[r.kind], "image kind/size %d", r.kind)
	}
	switch r.kind {
	case kindImageBegin, kindImageAdmitted:
		validateMetadata(b)
	case kindSourceReleased:
		nonzero64(b, 0, 8, 16)
		need(u32(b, 24) >= 1 && u32(b, 24) <= 5 && u32(b, 28) == 0, "SourceReleased")
	case kindChannelReady:
		nonzero64(b, 0)
		need(u32(b, 8) >= 1 && u32(b, 8) <= 3 && u32(b, 12) == 0, "ChannelReady")
	case kindChannelClosed:
		nonzero64(b, 0)
		need(u32(b, 8) >= 1 && u32(b, 8) <= 4 && u32(b, 12) == 0, "ChannelClosed")
	case kindImageLimits:
		nonzero64(b, 0, 8, 16)
		offers := u32(b, 52)
		need(offers >= 1 && offers <= 4 && u32(b, 56) <= 4 && u32(b, 60) <= 4, "ImageLimits jobs")
		need(u32(b, 64) >= 1 && u32(b, 64) <= 16 && u32(b, 68) >= 1 && u32(b, 68) <= 256 && u32(b, 72) >= 1 && u32(b, 76) >= 1, "ImageLimits bounds")
		need(u32(b, 80) != 0 && u32(b, 84) != 0 && u32(b, 88) != 0 && u32(b, 92) != 0 && u64(b, 96) == 0, "ImageLimits timers")
		sealed := u64(b, 24) == 0 && u32(b, 56) == 0
		sealed = sealed || (u32(b, 56) >= 1 && u32(b, 56) <= offers && u64(b, 24) >= u64(b, 16))
		need(sealed, "ImageLimits sealed fields")
		dma := []uint64{u64(b, 32), u64(b, 40), uint64(u32(b, 48)), uint64(u32(b, 60))}
		need((dma[0] == 0 && dma[1] == 0 && dma[2] == 0 && dma[3] == 0) || (dma[0] > 0 && dma[1] > 0 && dma[2] > 0 && dma[3] > 0 && dma[3] <= uint64(offers)), "ImageLimits DMA-BUF fields")
	case kindImageFormats:
		need(len(b) >= 32, "ImageFormats prefix")
		nonzero64(b, 0, 8, 16)
		n := int(u32(b, 24))
		need(n <= 4096 && u32(b, 28) == 0 && len(b) == 32+40*n, "ImageFormats count/reserved")
		var last []byte
		for i := 0; i < n; i++ {
			row := b[32+40*i : 72+40*i]
			nonzero64(row, 0)
			need(u32(row, 16) != 0 && (u32(row, 20) == 1 || u32(row, 20) == 2) && u32(row, 32) != 0 && u32(row, 36) != 0, "ImageFormat row")
			// Rows are strictly ordered by (output, major, minor, fourcc, modifier).
			if last != nil {
				need(formatKey(last).less(formatKey(row)), "ImageFormat order")
			}
			last = row
		}
	}
}

type formatTuple [5]uint64

func formatKey(row []byte) formatTuple {
	return formatTuple{u64(row, 0), uint64(u32(row, 8)), uint64(u32(row, 12)), uint64(u32(row, 16)), u64(row, 24)}
}
func (a formatTuple) less(b formatTuple) bool {
	for i := range a {
		if a[i] != b[i] {
			return a[i] < b[i]
		}
	}
	return false
}
func validateMetadata(b []byte) {
	nonzero64(b, 0, 8, 16, 24, 32, 40, 48)
	kind := u32(b, 56)
	planes, fds := u16(b, 76), u16(b, 78)
	need(kind >= 1 && kind <= 2 && u32(b, 60) != 0 && u32(b, 64) >= 1 && u32(b, 68) >= 1, "image metadata kind/size")
	need(planes >= 1 && planes <= 4 && fds == planes && u64(b, 80) == uint64(u32(b, 64))*uint64(u32(b, 68))*4, "image metadata planes/bytes")
	if kind == 1 {
		need(planes == 1 && u32(b, 60) == 1 && u32(b, 72) == u32(b, 64)*4 && u64(b, 88) == 0, "sealed metadata")
	} else {
		need(u32(b, 72) == 0, "DMA-BUF stride")
	}
}

// imageMetadata is a tight premultiplied BGRA8 sealed source.
func imageMetadata(offer, resource, topology uint64, width, height uint32) []byte {
	b := make([]byte, 96)
	p64(b, 0, offer)
	p64(b, 8, resource)
	p64(b, 16, 1)
	p64(b, 24, contentGrantEpoch)
	p64(b, 32, 2)
	p64(b, 40, 1)
	p64(b, 48, topology)
	p32(b, 56, 1)
	p32(b, 60, 1)
	p32(b, 64, width)
	p32(b, 68, height)
	p32(b, 72, width*4)
	p16(b, 76, 1)
	p16(b, 78, 1)
	p64(b, 80, uint64(width)*uint64(height)*4)
	return b
}

type discovery struct {
	epoch, generation, limits, formats uint64
	endpoint                           string
	sourceKinds                        uint32
}

var discoveryKeys = []string{"role", "connection_epoch", "channel_generation", "endpoint", "source_kinds", "max_packet_bytes", "max_fds", "max_offers", "max_packets_per_turn", "hello_timeout_ms", "validation_timeout_ms", "validation_chunk_bytes", "capture_interest_timeout_ms", "formats_generation", "limits_generation"}
var canonical = regexp.MustCompile(`^(0|[1-9][0-9]*)$`)
var endpointName = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,64}$`)

// parseDiscovery accepts exactly the version 1 key order, each key once.
func parseDiscovery(text []byte) discovery {
	need(len(text) > 0 && len(text) <= 2048 && text[len(text)-1] == '\n', "image-channel size/final LF")
	lines := strings.Split(string(text[:len(text)-1]), "\n")
	need(len(lines) == 1+len(discoveryKeys) && lines[0] == "sophia-image-channel version=1", "image-channel header/lines")
	values := map[string]string{}
	for i, key := range discoveryKeys {
		name, value, ok := strings.Cut(lines[i+1], "=")
		need(ok && name == key, "image-channel key %d %q", i, lines[i+1])
		values[key] = value
	}
	number := func(key string, bits int) uint64 {
		need(canonical.MatchString(values[key]), "image-channel %s not canonical", key)
		v, err := strconv.ParseUint(values[key], 10, bits)
		must(err)
		return v
	}
	d := discovery{epoch: number("connection_epoch", 64), generation: number("channel_generation", 64), formats: number("formats_generation", 64), limits: number("limits_generation", 64), endpoint: values["endpoint"], sourceKinds: uint32(number("source_kinds", 32))}
	need(number("role", 16) == 1, "image-channel role")
	need(d.epoch != 0 && d.generation != 0 && d.formats != 0 && d.limits != 0, "image-channel zero generation")
	need(endpointName.MatchString(d.endpoint) && d.endpoint != "." && d.endpoint != "..", "image-channel endpoint")
	need(d.sourceKinds != 0 && d.sourceKinds&^3 == 0, "image-channel source kinds")
	need(number("max_packet_bytes", 32) == 200 && number("max_fds", 32) == 4, "image-channel packet/fd bounds")
	offers := number("max_offers", 32)
	need(offers >= 1 && offers <= 4, "image-channel offers")
	for _, key := range discoveryKeys[8:13] {
		need(number(key, 32) != 0, "image-channel %s", key)
	}
	return d
}
func (s *session) readFile(fid uint32, cap int) []byte {
	var raw []byte
	for {
		part := data(s.w.read(fid, uint64(len(raw)), 1024))
		if len(part) == 0 {
			return raw
		}
		need(len(raw)+len(part) <= cap, "file cap")
		raw = append(raw, part...)
	}
}
func (s *session) imageObject(name string, cap int) record {
	s.w.file(10, 0, name)
	raw := s.readFile(10, cap)
	s.w.clunk(10)
	e, err := decodeAt(raw, s.revision)
	must(err)
	need(e.epoch == s.epoch, "%s epoch", name)
	return e
}

// discover reads the three discovery files under the authenticated root.
func (s *session) discover() bool {
	s.w.file(10, 0, "image-channel")
	d := parseDiscovery(s.readFile(10, 2048))
	s.w.clunk(10)
	limits := s.imageObject("image-limits", 32+104)
	formats := s.imageObject("image-formats", 163904)
	s.discovery = d
	return d.epoch == s.epoch && d.sourceKinds&1 != 0 && limits.kind == kindImageLimits && formats.kind == kindImageFormats &&
		u64(limits.body, 0) == d.generation && u64(limits.body, 8) == d.limits &&
		u64(formats.body, 0) == d.generation && u64(formats.body, 8) == d.formats && u64(formats.body, 16) == d.limits
}
func packetHeader(total, kind, fds int, epoch, generation, offer uint64) []byte {
	b := make([]byte, total)
	p32(b, 0, uint32(total))
	p16(b, 4, 1)
	p16(b, 6, uint16(kind))
	p16(b, 8, 1)
	p16(b, 10, uint16(fds))
	p64(b, 16, epoch)
	p64(b, 24, generation)
	p64(b, 32, offer)
	return b
}

// hello connects the discovered endpoint beside the role socket.
func (s *session) hello() record {
	fd, err := unix.Socket(unix.AF_UNIX, unix.SOCK_SEQPACKET|unix.SOCK_CLOEXEC, 0)
	must(err)
	s.image = fd
	must(unix.Connect(fd, &unix.SockaddrUnix{Name: filepath.Join(filepath.Dir(s.endpoint), s.discovery.endpoint)}))
	n, err := unix.SendmsgN(fd, packetHeader(40, 1, 0, s.epoch, s.discovery.generation, 0), nil, nil, unix.MSG_NOSIGNAL)
	must(err)
	need(n == 40, "Hello send")
	e := s.expect(kindChannelReady)
	s.ack(e)
	return e
}

// memfd returns a source file holding bytes; sealed adds every required seal.
func memfd(bytes []byte, sealed bool) int {
	fd, err := unix.MemfdCreate("oracle-isb", unix.MFD_CLOEXEC|unix.MFD_ALLOW_SEALING)
	must(err)
	for off := 0; off < len(bytes); {
		n, err := unix.Write(fd, bytes[off:])
		must(err)
		off += n
	}
	if sealed {
		_, err = unix.FcntlInt(uintptr(fd), unix.F_ADD_SEALS, unix.F_SEAL_WRITE|unix.F_SEAL_GROW|unix.F_SEAL_SHRINK|unix.F_SEAL_SEAL)
		must(err)
	}
	return fd
}
func (s *session) source(metadata []byte, fd int) int {
	b := packetHeader(200, 2, 1, s.epoch, s.discovery.generation, u64(metadata, 0))
	copy(b[40:], metadata)
	p32(b, 136+8, u32(metadata, 72))
	n, err := unix.SendmsgN(s.image, b, unix.UnixRights(fd), nil, unix.MSG_NOSIGNAL)
	must(err)
	must(unix.Close(fd))
	return n
}

// admit submits ImageBegin and returns the exact ImageAdmitted record.
func (s *session) admit(metadata []byte) (record, record) {
	id := s.submit(kindImageBegin, metadata)
	custody := s.custody(id, kindImageBegin)
	s.ack(custody)
	e := s.expect(kindImageAdmitted)
	s.ack(e)
	return custody, e
}

// settle returns the offer's ResourceStatus and SourceReleased. The contract
// promises both but orders them on independent credits, so accept either order.
func (s *session) settle(metadata []byte) (status, release record) {
	for status.kind == 0 || release.kind == 0 {
		e := s.next()
		switch {
		case e.kind == 33 && status.kind == 0:
			need(u64(e.body, 24) == u64(metadata, 8) && u64(e.body, 32) == u64(metadata, 16), "image ResourceStatus identity")
			status = e
		case e.kind == kindSourceReleased && release.kind == 0:
			need(identical(e.body[:24], metadata[:24]), "SourceReleased identity")
			release = e
		default:
			need(false, "image settlement event %d", e.kind)
		}
		s.ack(e)
	}
	return status, release
}
func (s *session) nextOffer() uint64 {
	s.offers++
	return s.offers
}

// sealedResource transfers one accepted image through the channel. It is the
// only way pixels reach the export; no fixture seeds resources.
func sealedResource(s *session, resource, topology uint64) bool {
	if s.image < 0 {
		need(s.discover(), "image discovery")
		s.hello()
	}
	m := imageMetadata(s.nextOffer(), resource, topology, 2, 1)
	_, admitted := s.admit(m)
	sent := s.source(m, memfd([]byte{0, 0, 255, 255, 0, 128, 0, 128}, true))
	status, release := s.settle(m)
	return identical(admitted.body, m) && sent == 200 && u16(status.body, 40) == 2 && u32(release.body, 24) == 1
}

// smallResource supplies the 2x1 resource that role candidates present.
func smallResource(s *session, topology uint64) {
	need(sealedResource(s, 5, topology), "sealed resource 5")
}
