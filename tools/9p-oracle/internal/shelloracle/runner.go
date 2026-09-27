package shelloracle

import (
	"fmt"
	"io"
)

// Names are the evidence contract, not an optional count threshold.
var Names = []string{
	"negotiation/api-accepted", "negotiation/policy-refused", "negotiation/second", "negotiation/allocation-before", "negotiation/candidate-before",
	"objects/limits", "objects/outputs", "objects/announcement", "objects/getattr", "objects/second-pin", "objects/old-pin", "objects/fresh-qid", "objects/fresh-generation",
	"allocation/granted", "allocation/rejected", "allocation/geometry",
	"upload/admitted", "upload/split", "upload/msize", "upload/short", "upload/end", "upload/cancel", "upload/clunk", "upload/ended-stale", "upload/cancelled-stale", "upload/successor",
	"candidate/permit", "candidate/custody", "candidate/prepared", "candidate/presented", "candidate/missing-revokes", "candidate/cancel", "candidate/cancelled-revokes",
	"action/identity", "action/echo",
	"custody/before-outcome", "custody/retry", "custody/watermark", "custody/eagain-empty", "custody/eagain-retry", "journal/offset", "journal/reread", "journal/floor", "journal/past-tail", "journal/pending", "custody/domain-id",
	"malformed/length", "malformed/reserved", "malformed/count", "malformed/kind",
	"stream/fragmented", "stream/staging", "stream/flush", "stream/no-late-reply",
}

type runner struct {
	root             string
	out, diagnostics io.Writer
	seen             map[string]bool
	failed           int
}

func (r *runner) check(id int, ok bool) {
	name := Names[id-1]
	need(!r.seen[name], "duplicate check %s", name)
	r.seen[name] = true
	if !ok {
		fmt.Fprintf(r.out, "check %s FAIL\n", name)
		panic(fmt.Errorf("check %s", name))
	}
	fmt.Fprintf(r.out, "check %s ok\n", name)
}
func (r *runner) scenario(name string, f func()) {
	defer func() {
		if p := recover(); p != nil {
			r.failed++
			fmt.Fprintf(r.diagnostics, "scenario %s: %v\n", name, p)
		}
	}()
	f()
}
func Run(root string, out, diagnostics io.Writer) bool {
	r := &runner{root: root, out: out, diagnostics: diagnostics, seen: map[string]bool{}}
	r.scenario("main", r.mainScenario)
	r.scenario("refused", r.refusedScenario)
	r.scenario("custody", r.custodyScenario)
	r.scenario("malformed", r.malformedScenario)
	r.scenario("stream", r.streamScenario)
	r.scenario("missing", func() { r.revocationScenario("missing", false, 31) })
	r.scenario("cancelled", func() { r.revocationScenario("cancelled", true, 33) })
	pass := len(r.seen) == len(Names) && r.failed == 0
	status := "fail"
	if pass {
		status = "pass"
	}
	if !pass && r.failed == 0 {
		r.failed++
	}
	fmt.Fprintf(out, "sophia_shell_files_oracle schema=1 status=%s checks=%d failed=%d\n", status, len(r.seen), r.failed)
	return pass
}
