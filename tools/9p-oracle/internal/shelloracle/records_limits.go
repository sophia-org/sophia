package shelloracle

func validateLimits(b []byte) {
	nonzero64(b, 0, 8, 16, 24)
	max64 := []uint64{4194304, 8388608, 16777216, 16777216, 67108864}
	for i, cap := range max64 {
		need(u64(b, 24+8*i) <= cap, "Limits u64 cap %d", i)
	}
	need(u64(b, 64) == 1 && u64(b, 72) == 0 && zero(b[260:]), "Limits masks/reserved")
	caps := []uint32{65536, 65488, 8192, 4096, 64, 4096, 4, 16, 16, 4, 1, 3, 8, 32, 64, 8192, 8, 2, 1, 8, 1, 16, 1, 64, 65536, 131072, 262144, 16, 512, 1024, 512, 50, 512, 32, 4, 1000, 2000, 500, 1000, 1000, 2000, 1000, 250, 2000, 120000}
	for i, cap := range caps {
		need(u32(b, 80+4*i) <= cap, "Limits cap offset %d", 80+4*i)
	}
	for _, at := range []int{88, 92, 96, 104, 108, 116, 128, 132, 152, 160, 188, 212, 216, 220, 224, 228, 232, 236, 240, 244, 248, 252, 256} {
		need(u32(b, at) != 0, "Limits zero offset %d", at)
	}
	need(u32(b, 168) == 1 && u32(b, 172) >= 1 && u32(b, 140) >= 40 && u32(b, 176) >= 1024, "Limits minimum")
	for _, p := range [][2]int{{100, 96}, {112, 116}, {148, 152}, {156, 160}, {184, 176}} {
		need(u32(b, p[0]) >= u32(b, p[1]), "Limits ordering")
	}
	need(uint64(u32(b, 180)) >= uint64(u32(b, 80))+24 && uint64(u32(b, 84)) >= uint64(u32(b, 88))*4 && uint64(u32(b, 84))+48 <= uint64(u32(b, 80)), "Limits byte relationships")
	for _, at := range []int{32, 40, 48} {
		need(u64(b, at) >= u64(b, 24), "Limits resource budgets")
	}
	need(u64(b, 56) >= u64(b, 32)+u64(b, 40)+u64(b, 48), "Limits session sum")
	need(u32(b, 200) <= u32(b, 192) && u32(b, 228) <= u32(b, 224), "Limits extent/timeout")
}
