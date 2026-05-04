package main

import "github.com/toantran292/tncli/internal/commands"

func main() {
	commands.Version = version
	execute()
}
