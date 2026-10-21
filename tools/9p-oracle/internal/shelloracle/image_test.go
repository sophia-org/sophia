package shelloracle

import (
	"strings"
	"testing"
)

func imageEvent(kind uint16, body []byte) []byte {
	raw := envelope(kind, 0, 17, body)
	p64(raw, 24, 5)
	return raw
}

func TestImageRecordsRequireRevision9(t *testing.T) {
	m := imageMetadata(1, 5, 3, 2, 1)
	for _, raw := range [][]byte{imageEvent(kindImageAdmitted, m), envelope(kindImageBegin, 3, 17, m)} {
		if _, err := decodeAt(raw, 8); err == nil {
			t.Fatalf("kind %d accepted below r9", u16(raw, 6))
		}
		if _, err := decodeAt(raw, 9); err != nil {
			t.Fatalf("kind %d at r9: %v", u16(raw, 6), err)
		}
	}
	submitted := make([]byte, 16)
	p64(submitted, 0, 3)
	p16(submitted, 8, kindImageBegin)
	if _, err := decodeAt(imageEvent(18, submitted), 8); err == nil {
		t.Fatal("ImageBegin custody accepted below r9")
	}
	if _, err := decodeAt(imageEvent(18, submitted), 9); err != nil {
		t.Fatal(err)
	}
}

func TestImageMetadataBounds(t *testing.T) {
	for name, mutate := range map[string]func([]byte){
		"stride":   func(b []byte) { p32(b, 72, 4) },
		"bytes":    func(b []byte) { p64(b, 80, 4) },
		"planes":   func(b []byte) { p16(b, 76, 2) },
		"modifier": func(b []byte) { p64(b, 88, 1) },
		"epoch":    func(b []byte) { p64(b, 24, 0) },
	} {
		m := imageMetadata(1, 5, 3, 2, 1)
		mutate(m)
		if _, err := decodeAt(imageEvent(kindImageAdmitted, m), 9); err == nil {
			t.Fatalf("%s accepted", name)
		}
	}
	released := make([]byte, 32)
	nonzeroIdentity := imageMetadata(1, 5, 3, 2, 1)[:24]
	copy(released, nonzeroIdentity)
	p32(released, 24, 6)
	if _, err := decodeAt(imageEvent(kindSourceReleased, released), 9); err == nil {
		t.Fatal("disposition 6 accepted")
	}
}

// ImageBegin keeps resource_id @8; ordinary candidates take the epoch there.
func TestEncodedLeavesImageBeginIdentity(t *testing.T) {
	s := &session{epoch: 17}
	m := imageMetadata(1, 5, 3, 2, 1)
	raw := s.encoded(kindImageBegin, 3, m)
	if u64(raw, 32+8) != 5 || u64(raw, 32+24) != contentGrantEpoch {
		t.Fatalf("ImageBegin rewritten: resource %d epoch %d", u64(raw, 32+8), u64(raw, 32+24))
	}
	if u64(s.encoded(263, 3, demand(90, 1)), 32+8) != 17 {
		t.Fatal("candidate epoch not written")
	}
}

func TestDiscoveryOrderIsExact(t *testing.T) {
	good := "sophia-image-channel version=1\nrole=1\nconnection_epoch=17\nchannel_generation=2\nendpoint=image.sock\nsource_kinds=4\nmax_packet_bytes=200\nmax_fds=4\nmax_offers=4\nmax_packets_per_turn=8\nhello_timeout_ms=2000\nvalidation_timeout_ms=2000\nvalidation_chunk_bytes=65536\ncapture_interest_timeout_ms=500\nformats_generation=1\nlimits_generation=1\n"
	if d := parseDiscovery([]byte(good)); d.epoch != 17 || d.generation != 2 || d.endpoint != "image.sock" {
		t.Fatalf("parsed %+v", d)
	}
	for name, bad := range map[string]string{
		"sealed":  replaceOnce(good, "source_kinds=4", "source_kinds=1"),
		"mixed":   replaceOnce(good, "source_kinds=4", "source_kinds=5"),
		"order":   swapLines(good, 2, 3),
		"leading": replaceOnce(good, "channel_generation=2", "channel_generation=02"),
		"packet":  replaceOnce(good, "max_packet_bytes=200", "max_packet_bytes=199"),
		"path":    replaceOnce(good, "endpoint=image.sock", "endpoint=.."),
		"final":   good[:len(good)-1],
	} {
		func() {
			defer func() { _ = recover() }()
			parseDiscovery([]byte(bad))
			t.Fatalf("%s accepted", name)
		}()
	}
}

func swapLines(s string, a, b int) string {
	lines := strings.Split(s, "\n")
	lines[a], lines[b] = lines[b], lines[a]
	return strings.Join(lines, "\n")
}
func replaceOnce(s, old, new string) string { return strings.Replace(s, old, new, 1) }
