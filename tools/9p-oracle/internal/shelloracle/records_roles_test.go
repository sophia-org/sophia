package shelloracle

import "testing"

func TestRoleCandidateValidationLayers(t *testing.T) {
	base := roleBodies()[267]
	withoutSurface := append(append([]byte{}, base[:108]...), base[172:]...)
	le.PutUint16(withoutSurface[98:], 0)
	withoutPlacement := append(append([]byte{}, base[:172]...), base[204:]...)
	le.PutUint16(withoutPlacement[100:], 0)
	mismatchedRows := append([]byte{}, base[:252]...)
	le.PutUint16(mismatchedRows[104:], 0)
	missingSelection := append([]byte{}, base...)
	le.PutUint16(missingSelection[96:], 2)
	duplicates := append(append([]byte{}, base[:252]...), base[204:252]...)
	duplicates = append(duplicates, 1, 0, 1, 0)
	le.PutUint16(duplicates[102:], 2)
	le.PutUint16(duplicates[104:], 2)
	for name, body := range map[string][]byte{"surface": withoutSurface, "placement": withoutPlacement,
		"row count": mismatchedRows, "selection": missingSelection, "duplicate slots": duplicates} {
		if _, err := decodeCandidateBytes(roleLiteral(267, body)); err != nil {
			t.Fatalf("%s bytes: %v", name, err)
		}
		if _, err := decode(roleLiteral(267, body)); err == nil {
			t.Fatalf("%s value accepted", name)
		}
	}
	// Target identity duplicates and overlaps require owner validation. Distinct
	// displayed slots keep this native value structurally valid.
	duplicates[len(duplicates)-2] = 2
	if _, err := decode(roleLiteral(267, duplicates)); err != nil {
		t.Fatal(err)
	}
	dock := append([]byte{}, roleBodies()[270][:88]...)
	for i := 80; i < 86; i++ {
		dock[i] = 0
	}
	if _, err := decode(roleLiteral(270, dock)); err != nil {
		t.Fatalf("dock owner counts: %v", err)
	}
	for _, kind := range []uint16{267, 270} {
		b := roleBodies()[kind]
		offset := 184
		if kind == 267 {
			offset = 204
		}
		le.PutUint32(b[offset+28:], 2147483647)
		if _, err := decodeCandidateBytes(roleLiteral(kind, b)); err == nil {
			t.Fatal("overflow accepted")
		}
	}
}

func TestRoleCandidateMaximumCounts(t *testing.T) {
	for _, kind := range []uint16{267, 270} {
		base := roleBodies()[kind]
		prefix, counts, ns, nt, nr := 88, 80, 8, 64, 0
		if kind == 267 {
			prefix, counts, ns, nt, nr = 108, 98, 1, 32, 32
		}
		b := append([]byte{}, base[:prefix]...)
		le.PutUint16(b[counts:], uint16(ns))
		le.PutUint16(b[counts+2:], 32)
		le.PutUint16(b[counts+4:], uint16(nt))
		if kind == 267 {
			le.PutUint16(b[104:], 32)
		}
		for i := 0; i < ns; i++ {
			b = append(b, base[prefix:prefix+64]...)
		}
		for i := 0; i < 32; i++ {
			b = append(b, base[prefix+64:prefix+96]...)
		}
		for i := 0; i < nt; i++ {
			b = append(b, base[prefix+96:prefix+144]...)
		}
		for i := 0; i < nr; i++ {
			b = append(b, byte(i+1), 0)
		}
		if _, err := decode(roleLiteral(kind, b)); err != nil {
			t.Fatalf("kind %d maximum: %v", kind, err)
		}
	}
}

// Independent KDL literals. They do not call encoders to create expected bytes.
func roleBodies() map[uint16][]byte {
	m := map[uint16][]byte{}
	for kind, n := range map[uint16]int{3: 688, 4: 152, 38: 64, 39: 112, 40: 116, 41: 398, 42: 136, 43: 36, 44: 132, 45: 36, 266: 92, 267: 254, 268: 132, 269: 132, 270: 232, 271: 128, 272: 56} {
		b := make([]byte, n)
		le.PutUint64(b, 7)
		switch kind {
		case 3:
			le.PutUint64(b[8:], 17)
			le.PutUint64(b[16:], 3)
			le.PutUint16(b[24:], 1)
			le.PutUint16(b[26:], 1)
			le.PutUint16(b[32:], 1)
			le.PutUint16(b[34:], 1)
			le.PutUint16(b[36:], 4)
			copy(b[40:], "Term")
			le.PutUint16(b[428:], 15)
			copy(b[432:], "registered:term")
		case 4:
			le.PutUint16(b[34:], 1)
			le.PutUint16(b[36:], 1)
			le.PutUint16(b[50:], 4)
			copy(b[54:], "tile")
			le.PutUint16(b[116:], 2)
			copy(b[120:], "ok")
		case 38, 39, 40, 41, 42, 43, 266, 268, 269:
			end := n
			if kind == 40 {
				end = 112
			}
			if kind == 41 {
				end = 136
			}
			if kind == 42 || kind == 268 || kind == 269 {
				end = 128
			}
			if kind == 43 {
				end = 32
			}
			if kind == 266 {
				end = 56
			}
			for at := 0; at < end; at += 8 {
				le.PutUint64(b[at:], 1)
			}
			le.PutUint64(b[8:], 17)
			switch kind {
			case 40:
				le.PutUint16(b[112:], 1)
			case 41:
				le.PutUint64(b[120:], 2)
				le.PutUint16(b[136:], 1)
				le.PutUint16(b[138:], 1)
				b[142] = 'a'
			case 42:
				le.PutUint16(b[128:], 1)
				le.PutUint16(b[130:], 1)
				le.PutUint16(b[132:], 1)
			case 43:
				le.PutUint16(b[32:], 1)
			case 266:
				le.PutUint16(b[72:], 1)
				le.PutUint16(b[74:], 1)
				le.PutUint32(b[76:], 64)
				le.PutUint32(b[80:], 32)
			case 268:
				le.PutUint16(b[128:], 1)
			case 269:
				le.PutUint16(b[128:], 1)
				le.PutUint16(b[130:], 1)
			}
		case 44, 271:
			for at := 0; at < 112; at += 8 {
				le.PutUint64(b[at:], 1)
			}
			le.PutUint16(b[112:], 1)
			le.PutUint64(b[120:], 3)
			if kind == 44 {
				le.PutUint16(b[128:], 1)
			}
		case 267, 270:
			prefix := 88
			for at := 0; at < 80; at += 8 {
				le.PutUint64(b[at:], 1)
			}
			if kind == 267 {
				prefix = 108
				le.PutUint64(b[80:], 3)
				le.PutUint64(b[88:], 1)
				le.PutUint16(b[96:], 1)
				le.PutUint16(b[98:], 1)
				le.PutUint16(b[100:], 1)
				le.PutUint16(b[102:], 1)
				le.PutUint16(b[104:], 1)
				le.PutUint16(b[252:], 1)
			} else {
				le.PutUint16(b[80:], 1)
				le.PutUint16(b[82:], 1)
				le.PutUint16(b[84:], 1)
			}
			s := b[prefix:]
			le.PutUint64(s, 1)
			le.PutUint64(s[8:], 1)
			le.PutUint64(s[16:], 1)
			le.PutUint16(s[24:], 1)
			if kind == 267 {
				le.PutUint16(s[24:], 3)
			}
			le.PutUint16(s[26:], 1)
			le.PutUint16(s[40:], 65535)
			le.PutUint64(s[64:], 5)
			le.PutUint64(s[72:], 1)
			t := s[96:]
			le.PutUint16(t[2:], 3)
			if kind == 267 {
				le.PutUint16(t[2:], 2)
			}
			le.PutUint64(t[4:], 1)
			le.PutUint64(t[12:], 1)
			le.PutUint64(t[20:], 1)
			le.PutUint32(t[36:], 2)
			le.PutUint32(t[40:], 1)
		}
		m[kind] = b
	}
	return m
}
func roleLiteral(kind uint16, b []byte) []byte {
	raw := inbound(kind, b)
	if kind >= 256 {
		le.PutUint64(raw[16:], 9)
		le.PutUint64(raw[24:], 0)
	}
	return raw
}
func TestRoleLiteralsAndTruncations(t *testing.T) {
	for kind, b := range roleBodies() {
		if _, err := decode(roleLiteral(kind, b)); err != nil {
			t.Fatalf("kind %d: %v", kind, err)
		}
		for n := 0; n < len(b); n++ {
			if _, err := decode(roleLiteral(kind, b[:n])); err == nil {
				t.Fatalf("kind %d truncated %d", kind, n)
			}
		}
		if _, err := decode(roleLiteral(kind, append(append([]byte{}, b...), 0))); err == nil {
			t.Fatalf("kind %d trailing byte", kind)
		}
		if kind >= 266 {
			raw, err := encodeRole(kind, 9, 17, b)
			if err != nil || !identical(raw, roleLiteral(kind, b)) {
				t.Fatalf("kind %d encode: %v", kind, err)
			}
		}
	}
}

func TestRoleScalarAndReservedControls(t *testing.T) {
	for _, c := range []struct {
		kind  uint16
		at    int
		value uint16
	}{
		{3, 24, 4097}, {3, 26, 2}, {3, 28, 1}, {3, 32, 0}, {3, 34, 2}, {3, 38, 1}, {3, 36, 129},
		{4, 32, 2}, {4, 34, 17}, {4, 36, 257}, {4, 38, 1}, {4, 50, 33}, {4, 52, 1},
		{38, 24, 0}, {39, 104, 0}, {40, 112, 0}, {40, 112, 13}, {40, 114, 1},
		{41, 136, 0}, {41, 136, 18}, {41, 138, 257}, {41, 140, 1},
		{42, 128, 0}, {42, 130, 0}, {42, 132, 0}, {42, 134, 1}, {43, 32, 0}, {43, 34, 1},
		{44, 112, 2}, {44, 114, 1}, {44, 128, 6}, {44, 130, 1}, {45, 32, 4},
		{266, 72, 0}, {266, 74, 5}, {266, 84, 513},
		{267, 98, 2}, {267, 100, 33}, {267, 102, 33}, {267, 104, 33}, {267, 106, 1},
		{268, 128, 0}, {268, 130, 1}, {269, 128, 3}, {269, 130, 4097},
		{270, 80, 9}, {270, 82, 33}, {270, 84, 65}, {270, 86, 1}, {271, 112, 0}, {272, 0, 0},
	} {
		b := roleBodies()[c.kind]
		le.PutUint16(b[c.at:], c.value)
		if _, err := decode(roleLiteral(c.kind, b)); err == nil {
			t.Errorf("kind %d offset %d value %d", c.kind, c.at, c.value)
		}
	}
}

func TestMaximumSnapshotRows(t *testing.T) {
	catalog := make([]byte, 32+4096*656)
	le.PutUint64(catalog, 1)
	le.PutUint64(catalog[8:], 17)
	le.PutUint64(catalog[16:], 1)
	le.PutUint16(catalog[24:], 4096)
	for i := 0; i < 4096; i++ {
		row := catalog[32+i*656:]
		le.PutUint16(row, uint16(i+1))
		le.PutUint16(row[4:], 1)
		row[8] = 'a'
	}
	if _, err := decode(roleLiteral(3, catalog)); err != nil {
		t.Fatal(err)
	}
	le.PutUint16(catalog[32+656:], 1)
	if _, err := decode(roleLiteral(3, catalog)); err == nil {
		t.Fatal("duplicate catalog slot")
	}
	indicators := make([]byte, 40+16*46+256*66)
	le.PutUint64(indicators, 1)
	le.PutUint16(indicators[34:], 16)
	le.PutUint16(indicators[36:], 256)
	if _, err := decode(roleLiteral(4, indicators)); err != nil {
		t.Fatal(err)
	}
	for _, kind := range []uint16{3, 4} {
		b := make([]byte, 32)
		le.PutUint64(b, 1)
		le.PutUint64(b[8:], 17)
		le.PutUint64(b[16:], 1)
		if kind == 4 {
			b = append(b, make([]byte, 8)...)
		}
		if _, err := decode(roleLiteral(kind, b)); err != nil {
			t.Fatal(err)
		}
	}
}

func TestTypedRoleRequestEncoders(t *testing.T) {
	literals := roleBodies()
	allocation := nativeAllocationBody(nativeAllocationRequest{
		transaction: 1, connection: 17, content: 1, opening: 1, output: 1, outputGeneration: 1,
		request: 1, operation: 1, edge: 1, width: 64, height: 32,
	})
	if !identical(allocation, literals[266]) {
		t.Fatal("native allocation literal")
	}
	in, err := decode(roleLiteral(41, literals[41]))
	if err != nil {
		t.Fatal(err)
	}
	ack := nativeInputAckBody(in, 1, 1)
	expected := append([]byte{}, literals[268]...)
	le.PutUint64(expected[120:], 2)
	if !identical(ack, expected) {
		t.Fatal("input ack literal")
	}
	activation := nativeActivateBody(literals[39], 1, 1, 1, 1, 1)
	if !identical(activation, literals[269]) {
		t.Fatal("native activation literal")
	}
	if !identical(indicatorActivateBody(7, 0, 0, 0, 0, 0, 0), literals[272]) {
		t.Fatal("indicator activation literal")
	}
}

func setLiteralText(field []byte, text []byte) {
	for i := range field {
		field[i] = 0
	}
	le.PutUint16(field, uint16(len(text)))
	copy(field[4:], text)
}
func TestRoleTextPolicies(t *testing.T) {
	for _, text := range [][]byte{{0}, {0x7f}, {0xc2, 0x85}, []byte("\u202a"), []byte("\u2069")} {
		catalog := roleBodies()[3]
		setLiteralText(catalog[36:168], text)
		if _, err := decode(roleLiteral(3, catalog)); err == nil {
			t.Errorf("catalog accepted control %x", text)
		}
		input := roleBodies()[41]
		setLiteralText(input[138:], text)
		if _, err := decode(roleLiteral(41, input)); err == nil {
			t.Errorf("input accepted control %x", text)
		}
		indicators := roleBodies()[4]
		setLiteralText(indicators[50:86], text)
		if _, err := decode(roleLiteral(4, indicators)); err != nil {
			t.Errorf("indicator refused valid UTF-8 %x: %v", text, err)
		}
	}
	for _, text := range [][]byte{{0xc0, 0x80}, {0xed, 0xa0, 0x80}, {0xf4, 0x90, 0x80, 0x80}, {0xe2, 0x82}, {0x80}} {
		for _, kind := range []uint16{3, 4, 41} {
			b := roleBodies()[kind]
			switch kind {
			case 3:
				setLiteralText(b[36:168], text)
			case 4:
				setLiteralText(b[50:86], text)
			case 41:
				setLiteralText(b[138:], text)
			}
			if _, err := decode(roleLiteral(kind, b)); err == nil {
				t.Errorf("kind %d accepted invalid UTF-8 %x", kind, text)
			}
		}
	}
	for _, identity := range []string{"registered:", "desktop:", "unknown:x", "registered:x\x00"} {
		b := roleBodies()[3]
		setLiteralText(b[428:688], []byte(identity))
		if _, err := decode(roleLiteral(3, b)); err == nil {
			t.Errorf("identity %q accepted", identity)
		}
	}
	b := roleBodies()[3]
	setLiteralText(b[36:168], []byte("é"))
	setLiteralText(b[428:688], []byte("desktop:é"))
	if _, err := decode(roleLiteral(3, b)); err != nil {
		t.Fatal(err)
	}
}
func TestNativeRevisionAndAllocationRules(t *testing.T) {
	for kind := uint16(1); kind <= 17; kind++ {
		b := roleBodies()[41]
		le.PutUint16(b[136:], kind)
		if kind != 1 {
			setLiteralText(b[138:], nil)
		}
		if kind == 17 {
			le.PutUint64(b[120:], 1)
		}
		if _, err := decode(roleLiteral(41, b)); err != nil {
			t.Fatalf("input kind %d: %v", kind, err)
		}
		if kind == 17 {
			le.PutUint64(b[120:], 2)
		} else {
			le.PutUint64(b[120:], 1)
		}
		if _, err := decode(roleLiteral(41, b)); err == nil {
			t.Fatalf("input kind %d revision accepted", kind)
		}
	}
	for _, kind := range []uint16{269, 42} {
		b := roleBodies()[kind]
		le.PutUint64(b[120:], 2)
		if _, err := decode(roleLiteral(kind, b)); err == nil {
			t.Fatalf("kind %d activation revision", kind)
		}
	}
	for op := uint16(1); op <= 3; op++ {
		b := roleBodies()[266]
		le.PutUint16(b[72:], op)
		if op != 1 {
			le.PutUint64(b[56:], 1)
			le.PutUint64(b[64:], 1)
		}
		if op == 3 {
			for i := 76; i < 92; i++ {
				b[i] = 0
			}
		}
		if _, err := decode(roleLiteral(266, b)); err != nil {
			t.Fatalf("allocation op %d: %v", op, err)
		}
		b[56] ^= 1
		if _, err := decode(roleLiteral(266, b)); err == nil {
			t.Fatalf("allocation op %d mixed pair", op)
		}
	}
	for _, at := range []int{76, 80, 84} {
		b := roleBodies()[266]
		le.PutUint16(b[72:], 3)
		le.PutUint64(b[56:], 1)
		le.PutUint64(b[64:], 1)
		for i := 76; i < 92; i++ {
			b[i] = 0
		}
		b[at] = 1
		if _, err := decode(roleLiteral(266, b)); err == nil {
			t.Fatalf("release geometry at %d", at)
		}
	}
}
