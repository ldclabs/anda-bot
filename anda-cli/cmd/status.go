package cmd

import (
	"github.com/spf13/cobra"
)

var statusCmd = &cobra.Command{
	Use:   "status",
	Short: "Get space status and statistics",
	Run: func(cmd *cobra.Command, args []string) {
		client := newClient()
		resp, err := client.GetStatus(cmd.Context())
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		printJSON(resp.Result)
	},
}

func init() {
	rootCmd.AddCommand(statusCmd)
}
