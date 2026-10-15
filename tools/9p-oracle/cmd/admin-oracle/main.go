// Independent administrative contract check. No Sophia codecs are imported.
// The side channel changes fixture state; all records come from the 9P socket.
package main

import (
	"bufio"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"os"
	"regexp"
	"strconv"
	"time"

	"github.com/hugelgupf/p9/linux"
	"github.com/hugelgupf/p9/p9"
)

func must(err error) {
	if err != nil {
		panic(err)
	}
}
func need(ok bool, message string) {
	if !ok {
		panic(message)
	}
}
func check(name string) { fmt.Printf("check %s ok\n", name) }
func open(root p9.File, name string, mode p9.OpenFlags) p9.File {
	_, f, err := root.Walk([]string{name})
	must(err)
	_, _, err = f.Open(mode)
	must(err)
	return f
}
func read(f p9.File) string {
	var data []byte
	for {
		// Deliberately split record fields across reads, including the counted name.
		b := make([]byte, 7)
		n, err := f.ReadAt(b, int64(len(data)))
		if err != nil && !errors.Is(err, io.EOF) {
			panic(err)
		}
		data = append(data, b[:n]...)
		need(len(data) <= 512, "snapshot bound")
		if n == 0 || errors.Is(err, io.EOF) {
			return string(data)
		}
	}
}
func snapshot(root p9.File, name string) string {
	f := open(root, name, p9.ReadOnly)
	defer f.Close()
	return read(f)
}
func phase(path, name string) {
	c, err := net.DialTimeout("unix", path, time.Second)
	must(err)
	defer c.Close()
	must(c.SetDeadline(time.Now().Add(3 * time.Second)))
	_, err = fmt.Fprintln(c, name)
	must(err)
	line, err := bufio.NewReader(io.LimitReader(c, 64)).ReadString('\n')
	must(err)
	need(line == "ok\n", "fixture phase refused")
}
func run(socket, control string) {
	c, err := net.DialTimeout("unix", socket, time.Second)
	must(err)
	must(c.SetDeadline(time.Now().Add(20 * time.Second)))
	client, err := p9.NewClient(c)
	must(err)
	defer client.Close()
	root, err := client.Attach("")
	must(err)
	info := snapshot(root, "info")
	pattern := regexp.MustCompile(`^v1 info (0|[1-9][0-9]*) (0|[1-9][0-9]*) ([1-9][0-9]*) 10000 2000 60000\n$`)
	fields := pattern.FindStringSubmatch(info)
	need(fields != nil, "info grammar")
	for _, field := range fields[1:] {
		_, err := strconv.ParseUint(field, 10, 64)
		must(err)
	}
	need(fields[1] != "0" || fields[2] != "0", "session identity")
	id := fields[3]
	need(snapshot(root, "commands") == "v1 catalog 7 1\nv1 command policy 10:focus next\n", "catalog")
	need(snapshot(root, "status") == "v1 ready "+id+"\n", "ready")
	check("discovery")
	for _, name := range []string{"info", "commands", "status", "ctl"} {
		_, f, err := root.Walk([]string{name})
		must(err)
		mode := p9.WriteOnly
		if name == "ctl" {
			mode = p9.ReadOnly
		}
		_, _, err = f.Open(mode)
		need(errors.Is(err, linux.EACCES), "mode refusal")
		must(f.Close())
	}
	check("modes")
	ctl := open(root, "ctl", p9.WriteOnly)
	invoke := func(request, generation int, name string) string {
		return fmt.Sprintf("v1 invoke %s %d %d policy %d:%s\n", id, request, generation, len(name), name)
	}
	write := func(line string) {
		n, err := ctl.WriteAt([]byte(line), 0)
		must(err)
		need(int(n) == len(line), "short write")
	}
	refuse := func(line string, offset int64) {
		_, err := ctl.WriteAt([]byte(line), offset)
		need(errors.Is(err, linux.EINVAL), "write must be refused")
	}
	line := invoke(1, 7, "focus next")
	refuse(line[:len(line)-1], 0)
	refuse(line+line, 0)
	refuse(line, 1)
	refuse("v1 invoke 0 1 7 policy 10:focus next\n", 0)
	refuse("v1 invoke "+id+" 01 7 policy 10:focus next\n", 0)
	need(snapshot(root, "status") == "v1 ready "+id+"\n", "refusal changed status")
	check("malformed")
	write(line)
	pending := open(root, "status", p9.ReadOnly)
	wantPending := fmt.Sprintf("v1 pending %s 1 7\n", id)
	need(read(pending) == wantPending, "pending identity")
	_, err = ctl.WriteAt([]byte(invoke(2, 7, "focus next")), 0)
	need(errors.Is(err, linux.EBUSY), "pending capacity")
	phase(control, "finish")
	want := fmt.Sprintf("v1 outcome %s 1 7 committed 0:\n", id)
	// Re-reading the retained fid must still return its original pending snapshot.
	need(read(pending) == wantPending, "snapshot changed")
	must(pending.Close())
	deadline := time.Now().Add(3 * time.Second)
	for snapshot(root, "status") != want {
		need(time.Now().Before(deadline), "settlement outcome")
		time.Sleep(time.Millisecond)
	}
	need(snapshot(root, "status") == want, "outcome consumed")
	check("commit-and-retention")
	refuse(line, 0)
	need(snapshot(root, "status") == want, "replay changed result")
	check("replay")
	phase(control, "advance")
	write(invoke(2, 7, "focus next"))
	need(snapshot(root, "status") == fmt.Sprintf("v1 outcome %s 2 7 stale 0:\n", id), "stale generation")
	need(snapshot(root, "commands") == "v1 catalog 8 1\nv1 command policy 10:focus next\n", "new catalog")
	write(invoke(3, 8, "absent"))
	need(snapshot(root, "status") == fmt.Sprintf("v1 outcome %s 3 8 rejected 0:\n", id), "unadvertised command")
	check("stale-and-unadvertised")
	retained := open(root, "commands", p9.ReadOnly)
	write(invoke(4, 8, "focus next"))
	phase(control, "revoke")
	_, err = retained.ReadAt(make([]byte, 7), 0)
	need(err != nil, "revoked fid remained readable")
	check("revocation")
	denied, err := net.DialTimeout("unix", socket, time.Second)
	must(err)
	defer denied.Close()
	must(denied.SetDeadline(time.Now().Add(2 * time.Second)))
	probe, err := p9.NewClient(denied)
	if err == nil {
		defer probe.Close()
		_, err = probe.Attach("")
	}
	need(err != nil, "excluded peer attached again")
	var timed net.Error
	need(!errors.As(err, &timed) || !timed.Timeout(), "denial was only a timeout")
	check("denied-reattach")
	fmt.Println("sophia_admin_oracle schema=1 status=pass checks=8")
}
func main() {
	socket := flag.String("socket", "", "private administrative socket")
	control := flag.String("control", "", "fixture control socket")
	flag.Parse()
	defer func() {
		if err := recover(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}()
	need(*socket != "" && *control != "", "socket and control required")
	run(*socket, *control)
}
