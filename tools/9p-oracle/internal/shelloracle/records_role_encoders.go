package shelloracle

type nativeAllocationRequest struct {
	transaction, connection, content, opening, output, outputGeneration uint64
	request, prior, priorGeneration                                     uint64
	operation, edge                                                     uint16
	width, height                                                       uint32
	margins                                                             [4]int16
}

func nativeAllocationBody(v nativeAllocationRequest) []byte {
	b := make([]byte, 92)
	for i, value := range []uint64{v.transaction, v.connection, v.content, v.opening, v.output, v.outputGeneration, v.request, v.prior, v.priorGeneration} {
		p64(b, i*8, value)
	}
	p16(b, 72, v.operation)
	p16(b, 74, v.edge)
	p32(b, 76, v.width)
	p32(b, 80, v.height)
	for i, value := range v.margins {
		p16(b, 84+2*i, uint16(value))
	}
	return b
}
func nativeInputAckBody(input record, transaction uint64, disposition uint16) []byte {
	need(input.kind == 41 && len(input.body) == 398, "input ack source")
	b := make([]byte, 132)
	copy(b, input.body[:128])
	p64(b, 0, transaction)
	p16(b, 128, disposition)
	return b
}
func nativeActivateBody(binding []byte, transaction, event, state uint64, cause, slot uint16) []byte {
	need(len(binding) == 112, "activation binding size")
	b := make([]byte, 132)
	copy(b, binding)
	p64(b, 0, transaction)
	p64(b, 112, event)
	p64(b, 120, state)
	p16(b, 128, cause)
	p16(b, 130, slot)
	return b
}
func catalogActivateBody(action record, transaction, generation uint64) []byte {
	need(action.kind == 37 && len(action.body) == 120, "catalog action source")
	b := make([]byte, 128)
	copy(b, action.body)
	p64(b, 0, transaction)
	p64(b, 120, generation)
	return b
}
func indicatorActivateBody(transaction, epoch, generation, output, indicator, action, event uint64) []byte {
	b := make([]byte, 56)
	for i, value := range []uint64{transaction, epoch, generation, output, indicator, action, event} {
		p64(b, i*8, value)
	}
	return b
}

// Extend an independently built base candidate with role metadata. The row
// shape is unchanged; role/action-kind values are selected explicitly here.
func roleCandidateBody(base []byte, native bool, opening, catalog, state uint64, selected uint16, rows []uint16) []byte {
	need(len(base) >= 80, "base candidate prefix")
	ns, np, nt := int(u16(base, 72)), int(u16(base, 74)), int(u16(base, 76))
	need(ns <= 8 && np <= 32 && nt <= 64 && len(base) == 80+ns*64+np*32+nt*48, "base candidate rows")
	need(len(rows) <= 32, "native rows")
	prefix := 88
	if native {
		prefix = 108
	}
	n := prefix + len(base) - 80
	if native {
		n += 2 * len(rows)
	}
	b := make([]byte, n)
	copy(b, base[:72])
	if native {
		p64(b, 72, opening)
		p64(b, 80, catalog)
		p64(b, 88, state)
		p16(b, 96, selected)
		p16(b, 98, uint16(ns))
		p16(b, 100, uint16(np))
		p16(b, 102, uint16(nt))
		p16(b, 104, uint16(len(rows)))
	} else {
		p64(b, 72, catalog)
		p16(b, 80, uint16(ns))
		p16(b, 82, uint16(np))
		p16(b, 84, uint16(nt))
	}
	copy(b[prefix:], base[80:])
	for i := 0; i < ns; i++ {
		if native {
			p16(b, prefix+64*i+24, 3)
		}
	}
	at := prefix + ns*64 + np*32
	for i := 0; i < nt; i++ {
		kind := uint16(3)
		if native {
			kind = 2
		}
		p16(b, at+48*i+2, kind)
	}
	if native {
		for i, slot := range rows {
			p16(b, at+48*nt+2*i, slot)
		}
	}
	return b
}
