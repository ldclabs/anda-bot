package cmd

import (
	"github.com/spf13/cobra"
)

var infoCmd = &cobra.Command{
	Use:   "info",
	Short: "Get space information",
	Run: func(cmd *cobra.Command, args []string) {
		client := newClient()
		resp, err := client.GetSpaceInfo(cmd.Context())
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		printJSON(resp.Result)
	},
}

var formationStatusCmd = &cobra.Command{
	Use:   "formation-status",
	Short: "Get formation processing status",
	Run: func(cmd *cobra.Command, args []string) {
		client := newClient()
		resp, err := client.GetFormationStatus(cmd.Context())
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
	rootCmd.AddCommand(infoCmd)
	rootCmd.AddCommand(formationStatusCmd)
}
