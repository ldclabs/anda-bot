package cmd

import (
	"fmt"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var adminCmd = &cobra.Command{
	Use:   "admin",
	Short: "Admin operations (requires platform admin auth)",
}

var createSpaceCmd = &cobra.Command{
	Use:   "create-space",
	Short: "Create a space",
	Run: func(cmd *cobra.Command, args []string) {
		user, _ := cmd.Flags().GetString("user")
		sid, _ := cmd.Flags().GetString("space-id")
		tier, _ := cmd.Flags().GetInt("tier")

		if user == "" || sid == "" {
			exitError(fmt.Errorf("--user and --space-id are required"))
		}

		input := &api.CreateOrUpdateSpaceInput{
			User:    user,
			SpaceID: sid,
			Tier:    tier,
		}

		client := newClient()
		resp, err := client.CreateSpace(cmd.Context(), input)
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		printJSON(resp.Result)
	},
}

var updateTierCmd = &cobra.Command{
	Use:   "update-tier",
	Short: "Update space tier",
	Run: func(cmd *cobra.Command, args []string) {
		user, _ := cmd.Flags().GetString("user")
		sid, _ := cmd.Flags().GetString("space-id")
		tier, _ := cmd.Flags().GetInt("tier")

		if user == "" || sid == "" {
			exitError(fmt.Errorf("--user and --space-id are required"))
		}

		input := &api.CreateOrUpdateSpaceInput{
			User:    user,
			SpaceID: sid,
			Tier:    tier,
		}

		client := newClient()
		resp, err := client.UpdateSpaceTier(cmd.Context(), sid, input)
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
	createSpaceCmd.Flags().String("user", "", "Owner user ID (required)")
	createSpaceCmd.Flags().String("space-id", "", "Space ID (required)")
	createSpaceCmd.Flags().Int("tier", 0, "Space tier")

	updateTierCmd.Flags().String("user", "", "Owner user ID (required)")
	updateTierCmd.Flags().String("space-id", "", "Space ID (required)")
	updateTierCmd.Flags().Int("tier", 0, "Space tier")

	adminCmd.AddCommand(createSpaceCmd)
	adminCmd.AddCommand(updateTierCmd)
	rootCmd.AddCommand(adminCmd)
}
