package shelloracle

func validateIndicators(b []byte) {
	need(len(b) >= 40, "Indicators prefix")
	nonzero64(b, 0)
	present := u16(b, 32)
	statuses := int(u16(b, 34))
	indicators := int(u16(b, 36))
	need(present <= 1 && (u64(b, 24) != 0) == (present == 1), "active output pairing")
	need(statuses <= 16 && indicators <= 256 && u16(b, 38) == 0 && len(b) == 40+46*statuses+66*indicators, "indicator counts/length/reserved")
	at := 40
	for i := 0; i < statuses; i++ {
		roleText(b[at+10:at+46], 32)
		at += 46
	}
	for i := 0; i < indicators; i++ {
		roleText(b[at+30:at+66], 32)
		at += 66
	}
}
