package cmd

import (
	"fmt"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var recallCmd = &cobra.Command{
	Use:   "recall <query>",
	Short: "Recall memory via natural-language query",
	Long: `Query memory with natural language. The query should describe
what information you want to retrieve from memory.

Example:
  anda-cli recall "What are the user's preferences?"
  anda-cli recall --context-user u1 "What happened in the last meeting?"`,
	Args: cobra.ExactArgs(1),
	Run: func(cmd *cobra.Command, args []string) {
		contextUser, _ := cmd.Flags().GetString("context-user")
		contextAgent, _ := cmd.Flags().GetString("context-agent")
		contextSession, _ := cmd.Flags().GetString("context-session")
		contextTopic, _ := cmd.Flags().GetString("context-topic")

		input := &api.RecallInput{
			Query: args[0],
		}

		ctx := buildInputContext(contextUser, contextAgent, contextSession, contextTopic)
		if ctx != nil {
			input.Context = ctx
		}

		client := newClient()
		resp, err := client.Recall(cmd.Context(), input)
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
	recallCmd.Flags().String("context-user", "", "Context user")
	recallCmd.Flags().String("context-agent", "", "Context agent")
	recallCmd.Flags().String("context-session", "", "Context session")
	recallCmd.Flags().String("context-topic", "", "Context topic")
	rootCmd.AddCommand(recallCmd)
}
