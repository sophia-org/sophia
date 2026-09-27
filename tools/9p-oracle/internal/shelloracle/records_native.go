package shelloracle

import "fmt"

// Role encoders accept a complete independently constructed value body. The
// same KDL validator checks outgoing fields before any transaction is staged.
func encodeRole(kind uint16, id, epoch uint64, body []byte) ([]byte, error) {
	if kind < 266 || kind > 272 {
		return nil, fmt.Errorf("not a role candidate")
	}
	raw := envelope(kind, id, epoch, body)
	if _, err := decode(raw); err != nil {
		return nil, err
	}
	return raw, nil
}

func nonzeroWords(b []byte, end int) {
	for at := 0; at < end; at += 8 {
		nonzero64(b, at)
	}
}

func validateRole(r record) {
	b := r.body
	switch r.kind {
	case 3:
		validateCatalog(b)
	case 4:
		validateIndicators(b)
	case 38:
		need(len(b) == 64, "NativeOpening size")
		nonzeroWords(b, 64)
	case 39:
		need(len(b) == 112, "NativeFocus size")
		nonzeroWords(b, 112)
	case 40:
		need(len(b) == 116, "NativeFocusRevoked size")
		nonzeroWords(b, 112)
		need(u16(b, 112) >= 1 && u16(b, 112) <= 12 && zero(b[114:]), "focus revoke reason/reserved")
	case 41:
		validateNativeInput(b)
	case 42:
		need(len(b) == 136, "NativeActivationOutcome size")
		validateNativeActivation(b[:132])
		status, reason := u16(b, 132), u16(b, 134)
		need(status >= 1 && status <= 5 && reason <= 12 && (status == 1) == (reason == 0), "native outcome status/reason")
	case 43:
		need(len(b) == 36, "NativeClosed size")
		nonzeroWords(b, 32)
		need(u16(b, 32) >= 1 && u16(b, 32) <= 12 && zero(b[34:]), "native close reason/reserved")
	case 44:
		need(len(b) == 132, "CatalogActivationOutcome size")
		validateCatalogActivation(b[:128])
		need(u16(b, 128) >= 1 && u16(b, 128) <= 5 && u16(b, 130) == 0, "catalog outcome status/reason")
	case 45:
		need(len(b) == 36, "IndicatorActivationOutcome size")
		nonzero64(b, 0)
		need(u16(b, 32) <= 3, "indicator status")
	case 266:
		validateNativeAllocation(b)
	case 267, 270:
		validateRoleCandidate(r.kind, b)
	case 268:
		need(len(b) == 132, "NativeInputAck size")
		nonzeroWords(b, 128)
		need(u16(b, 128) >= 1 && u16(b, 128) <= 2 && zero(b[130:]), "input disposition/reserved")
	case 269:
		validateNativeActivation(b)
	case 271:
		validateCatalogActivation(b)
	case 272:
		need(len(b) == 56, "IndicatorActivate size")
		nonzero64(b, 0)
	default:
		need(false, "unknown role kind")
	}
}

func validateNativeActivation(b []byte) {
	need(len(b) == 132, "NativeActivate size")
	nonzeroWords(b, 128)
	need(u16(b, 128) >= 1 && u16(b, 128) <= 2 && u16(b, 130) >= 1 && u16(b, 130) <= 4096, "native cause/slot")
	need(u64(b, 120) == u64(b, 96), "activation revision")
}

func validateCatalogActivation(b []byte) {
	need(len(b) == 128, "CatalogActivate size")
	nonzeroWords(b, 112)
	nonzero64(b, 120)
	need(u64(b, 96) <= 4096 && u16(b, 112) == 1 && zero(b[114:120]), "catalog activation fields")
}
