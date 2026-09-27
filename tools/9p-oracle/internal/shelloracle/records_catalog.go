package shelloracle

import (
	"bytes"
	"unicode/utf8"
)

// Text is represented by its length and bytes; padding is never part of the
// application value. The field's u16 reserved word must also be zero.
func roleText(b []byte, size int) []byte {
	need(len(b) == size+4, "text field size")
	n := int(u16(b, 0))
	need(n <= size && u16(b, 2) == 0 && zero(b[4+n:]), "text length/reserved/padding")
	need(utf8.Valid(b[4:4+n]), "text UTF-8")
	return b[4 : 4+n]
}

func launcherText(text []byte) {
	for _, c := range string(text) {
		need(!(c <= 0x1f || (c >= 0x7f && c <= 0x9f) ||
			(c >= 0x202a && c <= 0x202e) || (c >= 0x2066 && c <= 0x2069)), "launcher text control/bidi")
	}
}

func validateCatalog(b []byte) {
	need(len(b) >= 32, "Catalog prefix")
	nonzeroWords(b, 24)
	n := int(u16(b, 24))
	identities := u16(b, 26)
	need(n <= 4096 && identities <= 1 && zero(b[28:32]) && len(b) == 32+656*n, "Catalog count/length/reserved")
	var seen [4097]bool
	for i := 0; i < n; i++ {
		row := b[32+i*656 : 32+(i+1)*656]
		slot := u16(row, 0)
		need(slot >= 1 && slot <= 4096 && !seen[slot] && u16(row, 2) <= 1, "catalog slot/available")
		seen[slot] = true
		label := roleText(row[4:136], 128)
		need(len(label) > 0, "empty catalog label")
		launcherText(label)
		launcherText(roleText(row[136:396], 256))
		identity := roleText(row[396:656], 256)
		launcherText(identity)
		need((len(identity) > 0) == (identities == 1), "catalog identity disclosure")
		if identities == 1 {
			need((len(identity) > 11 && bytes.HasPrefix(identity, []byte("registered:"))) ||
				(len(identity) > 8 && bytes.HasPrefix(identity, []byte("desktop:"))), "catalog identity prefix/tail")
		}
	}
}
