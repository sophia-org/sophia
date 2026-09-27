package main

import (
	"flag"
	"fmt"
	"os"
	"sophia/tools/9p-oracle/internal/shelloracle"
)

func main() {
	root := flag.String("root", "", "private fixture directory")
	flag.Parse()
	if *root == "" {
		fmt.Fprintln(os.Stderr, "shell-oracle -root PATH")
		os.Exit(2)
	}
	b := make([]byte, 1)
	if _, err := os.Stdin.Read(b); err != nil || b[0] != 'G' {
		fmt.Fprintln(os.Stderr, "startup barrier")
		os.Exit(2)
	}
	if !shelloracle.Run(*root, os.Stdout, os.Stderr) {
		os.Exit(1)
	}
}
