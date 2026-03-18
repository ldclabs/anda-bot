package main

import (
	_ "github.com/joho/godotenv/autoload"
	"github.com/ldclabs/anda-hippocampus/anda-cli/cmd"
)

func main() {
	cmd.Execute()
}
