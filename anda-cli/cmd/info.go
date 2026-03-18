package cmd

import (
	"github.com/spf13/cobra"
)

var infoCmd = &cobra.Command{
	Use:   "info",
	Short: "Get service information",
	Run: func(cmd *cobra.Command, args []string) {
		client := newClient()
		info, err := client.GetInfo(cmd.Context())
		if err != nil {
			exitError(err)
		}
		printJSON(info)
	},
}

func init() {
	rootCmd.AddCommand(infoCmd)
}
