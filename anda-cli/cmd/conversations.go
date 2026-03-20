package cmd

import (
	"fmt"
	"strconv"

	"github.com/spf13/cobra"
)

var conversationsCmd = &cobra.Command{
	Use:   "conversations",
	Short: "Manage conversations",
}

var listConversationsCmd = &cobra.Command{
	Use:   "list",
	Short: "List conversations with pagination",
	Run: func(cmd *cobra.Command, args []string) {
		cursor, _ := cmd.Flags().GetString("cursor")
		limit, _ := cmd.Flags().GetInt("limit")
		collection, _ := cmd.Flags().GetString("collection")

		client := newClient()
		resp, err := client.ListConversations(cmd.Context(), cursor, limit, collection)
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}
		printJSON(resp.Result)
		if resp.NextCursor != "" {
			fmt.Fprintf(cmd.ErrOrStderr(), "\nNext cursor: %s\n", resp.NextCursor)
		}
	},
}

var getConversationCmd = &cobra.Command{
	Use:   "get <conversation_id>",
	Short: "Get a single conversation detail",
	Args:  cobra.ExactArgs(1),
	Run: func(cmd *cobra.Command, args []string) {
		id, err := strconv.Atoi(args[0])
		if err != nil {
			exitError(fmt.Errorf("invalid conversation ID: %w", err))
		}

		collection, _ := cmd.Flags().GetString("collection")

		client := newClient()
		resp, err := client.GetConversation(cmd.Context(), id, collection)
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
	listConversationsCmd.Flags().String("cursor", "", "Pagination cursor")
	listConversationsCmd.Flags().Int("limit", 0, "Number of conversations to return")
	listConversationsCmd.Flags().String("collection", "", "Collection name")

	getConversationCmd.Flags().String("collection", "", "Collection name, empty or 'recall'")

	conversationsCmd.AddCommand(listConversationsCmd)
	conversationsCmd.AddCommand(getConversationCmd)
	rootCmd.AddCommand(conversationsCmd)
}
