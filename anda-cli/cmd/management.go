package cmd

import (
	"fmt"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var managementCmd = &cobra.Command{
	Use:   "management",
	Short: "Space management operations (requires CWT auth)",
}

var listTokensCmd = &cobra.Command{
	Use:   "list-tokens",
	Short: "List space tokens",
	Run: func(cmd *cobra.Command, args []string) {
		client := newClient()
		resp, err := client.ListSpaceTokens(cmd.Context())
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		printJSON(resp.Result)
	},
}

var addTokenCmd = &cobra.Command{
	Use:   "add-token",
	Short: "Add a space token",
	Run: func(cmd *cobra.Command, args []string) {
		scope, _ := cmd.Flags().GetString("scope")
		name, _ := cmd.Flags().GetString("name")
		if scope != "read" && scope != "write" && scope != "*" {
			exitError(fmt.Errorf("invalid scope: %s", scope))
		}
		if name == "" {
			exitError(fmt.Errorf("--name is required"))
		}

		client := newClient()
		resp, err := client.AddSpaceToken(cmd.Context(), &api.AddSpaceTokenInput{
			Scope: api.TokenScope(scope),
			Name:  name,
		})
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		printJSON(resp.Result)
	},
}

var revokeTokenCmd = &cobra.Command{
	Use:   "revoke-token <token>",
	Short: "Revoke a space token",
	Args:  cobra.ExactArgs(1),
	Run: func(cmd *cobra.Command, args []string) {
		client := newClient()
		resp, err := client.RevokeSpaceToken(cmd.Context(), args[0])
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		fmt.Printf("Revoked: %v\n", *resp.Result)
	},
}

var updateSpaceCmd = &cobra.Command{
	Use:   "update-space",
	Short: "Update space information",
	Run: func(cmd *cobra.Command, args []string) {
		input := &api.UpdateSpaceInput{}
		hasField := false

		if cmd.Flags().Changed("name") {
			v, _ := cmd.Flags().GetString("name")
			input.Name = &v
			hasField = true
		}
		if cmd.Flags().Changed("description") {
			v, _ := cmd.Flags().GetString("description")
			input.Description = &v
			hasField = true
		}
		if cmd.Flags().Changed("public") {
			v, _ := cmd.Flags().GetBool("public")
			input.Public = &v
			hasField = true
		}

		if !hasField {
			exitError(fmt.Errorf("at least one of --name, --description, or --public is required"))
		}

		client := newClient()
		resp, err := client.UpdateSpace(cmd.Context(), input)
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		fmt.Println("Space updated successfully")
	},
}

var restartFormationCmd = &cobra.Command{
	Use:   "restart-formation",
	Short: "Restart a formation task (manager only)",
	Run: func(cmd *cobra.Command, args []string) {
		input := &api.RestartFormationInput{}

		v, _ := cmd.Flags().GetUint64("conversation")
		if v == 0 {
			exitError(fmt.Errorf("--conversation is required"))
		}

		input.Conversation = &v
		client := newClient()
		resp, err := client.RestartFormation(cmd.Context(), input)
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		fmt.Println("Formation restarted successfully")
	},
}

func init() {
	addTokenCmd.Flags().String("name", "", "Token name (required)")
	addTokenCmd.Flags().String("scope", "*", "Token scope: read, write, *")
	restartFormationCmd.Flags().Uint64("conversation", 0, "Conversation ID")

	updateSpaceCmd.Flags().String("name", "", "Space name")
	updateSpaceCmd.Flags().String("description", "", "Space description")
	updateSpaceCmd.Flags().Bool("public", false, "Whether space is public")

	managementCmd.AddCommand(listTokensCmd)
	managementCmd.AddCommand(addTokenCmd)
	managementCmd.AddCommand(revokeTokenCmd)
	managementCmd.AddCommand(updateSpaceCmd)
	managementCmd.AddCommand(restartFormationCmd)
	rootCmd.AddCommand(managementCmd)
}
