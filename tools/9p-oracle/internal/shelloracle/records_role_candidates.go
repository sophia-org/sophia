package shelloracle

func validateRoleCandidateBytes(kind uint16, b []byte) {
	native := kind == 267
	prefix, counts := 88, 80
	if native {
		prefix, counts = 108, 98
	}
	need(len(b) >= prefix && len(b)+32 <= 8192, "role candidate size")
	nonzeroWords(b, 80)
	ns, np, nt := int(u16(b, counts)), int(u16(b, counts+2)), int(u16(b, counts+4))
	nr := 0
	if native {
		nonzero64(b, 80, 88)
		nr = int(u16(b, 104))
		need(ns <= 1 && nt <= 32 && nr <= 32 && u16(b, 96) <= 4096 && u16(b, 106) == 0, "native candidate fields")
	} else {
		need(ns <= 8 && nt <= 64 && u16(b, 86) == 0, "catalog candidate fields")
	}
	need(np <= 32 && len(b) == prefix+64*ns+32*np+48*nt+2*nr, "role candidate row lengths")
	at := prefix
	for i := 0; i < ns; i++ {
		s := b[at : at+64]
		at += 64
		nonzeroWords(s, 24)
		role := uint16(1)
		if native {
			role = 3
		}
		need(u16(s, 24) == role && u16(s, 26) >= 1 && u16(s, 26) <= 4, "role surface")
		margins(s[28:36])
		need(u32(s, 36) <= 512 && (!native || u32(s, 36) == 0) && u16(s, 40) == 65535 && zero(s[42:]), "panel surface geometry")
	}
	for i := 0; i < np; i++ {
		p := b[at : at+32]
		at += 32
		nonzeroWords(p, 16)
		need(u16(p, 16) <= 7 && (!native || u16(p, 16) == 0) && u16(p, 18) == 0 && int32(u32(p, 20)) >= 0 && int32(u32(p, 24)) >= 0 && zero(p[28:]), "role placement")
	}
	for i := 0; i < nt; i++ {
		t := b[at : at+48]
		at += 48
		nonzero64(t, 4, 12, 20)
		action := uint16(3)
		if native {
			action = 2
		}
		need(u16(t, 0) <= 7 && (!native || u16(t, 0) == 0) && u16(t, 2) == action && u64(t, 20) <= 4096, "role target identity")
		need(int32(u32(t, 28)) >= 0 && int32(u32(t, 32)) >= 0 && u32(t, 36) > 0 && u32(t, 40) > 0 && zero(t[44:]), "role target geometry")
		need(uint64(u32(t, 28))+uint64(u32(t, 36)) <= 2147483647 &&
			uint64(u32(t, 32))+uint64(u32(t, 40)) <= 2147483647, "target rectangle overflow")
	}
	for i := 0; i < nr; i++ {
		slot := u16(b, at+2*i)
		need(slot >= 1 && slot <= 4096, "displayed slot")
	}
}

func validateRoleCandidate(kind uint16, b []byte) {
	validateRoleCandidateBytes(kind, b)
	if kind == 270 {
		return
	} // One surface and nonempty placements are owner checks for dock.
	ns, np, nt, nr := int(u16(b, 98)), int(u16(b, 100)), int(u16(b, 102)), int(u16(b, 104))
	need(ns == 1 && np >= 1 && nt == nr, "native candidate counts")
	selected := u16(b, 96)
	need((selected == 0) == (nr == 0), "native selection presence")
	at := 108 + 64*ns + 32*np + 48*nt
	var seen [4097]bool
	for i := 0; i < nr; i++ {
		slot := u16(b, at+2*i)
		need(!seen[slot], "duplicate displayed slot")
		seen[slot] = true
	}
	need(selected == 0 || seen[selected], "selected slot missing")
	// Catalog availability, target triple uniqueness/overlap and current
	// opening/catalog/state require the separate owner validation path.
}

func validateNativeAllocation(b []byte) {
	need(len(b) == 92, "native allocation size")
	nonzeroWords(b, 56)
	need(u16(b, 72) >= 1 && u16(b, 72) <= 3 && u16(b, 74) >= 1 && u16(b, 74) <= 4, "native allocation operation/edge")
	margins(b[84:92])
	prior, gen, op := u64(b, 56), u64(b, 64), u16(b, 72)
	need(pair(prior, gen) && ((op == 1) == (prior == 0)), "native prior identity")
	if op == 3 {
		need(zero(b[76:92]), "native release geometry")
	} else {
		need(u32(b, 76) > 0 && u32(b, 80) > 0, "native desired dimensions")
	}
}
func validateNativeInput(b []byte) {
	need(len(b) == 398, "native input size")
	nonzeroWords(b, 136)
	kind := u16(b, 136)
	need(kind >= 1 && kind <= 17, "native input kind")
	text := roleText(b[138:], 256)
	need((kind == 1) == (len(text) > 0), "native input text shape")
	launcherText(text)
	if kind == 17 {
		need(u64(b, 120) == u64(b, 96), "Accept revision")
	} else {
		need(u64(b, 120) > u64(b, 96), "editing input revision")
	}
}
