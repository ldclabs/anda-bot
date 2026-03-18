package cmd

import (
	"fmt"
	"time"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var maintenanceCmd = &cobra.Command{
	Use:   "maintenance",
	Short: "Trigger maintenance (sleep/consolidation)",
	Long: `Trigger a maintenance task for memory consolidation.

Example:
  anda-cli maintenance
  anda-cli maintenance --trigger on_demand --scope full`,
	Run: func(cmd *cobra.Command, args []string) {
		trigger, _ := cmd.Flags().GetString("trigger")
		scope, _ := cmd.Flags().GetString("scope")

		input := &api.MaintenanceInput{
			Timestamp: time.Now().UTC().Format(time.RFC3339),
		}
		if trigger != "" {
			input.Trigger = trigger
		}
		if scope != "" {
			input.Scope = scope
		}

		client := newClient()
		resp, err := client.Maintenance(cmd.Context(), input)
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		if resp.Result != nil {
			fmt.Println(resp.Result.Content)
		}
	},
}

func init() {
	maintenanceCmd.Flags().String("trigger", "", "Trigger type: scheduled, threshold, on_demand")
	maintenanceCmd.Flags().String("scope", "", "Scope: full, quick")
	rootCmd.AddCommand(maintenanceCmd)
}
