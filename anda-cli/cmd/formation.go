package cmd

import (
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var formationCmd = &cobra.Command{
	Use:   "formation",
	Short: "Submit a memory formation task",
	Long: `Submit conversation messages for memory encoding.

Messages are provided as a JSON array via --messages or stdin.
Each message must have "role" and "content" fields.

Example:
  anda-cli formation --messages '[{"role":"user","content":"Hello"},{"role":"assistant","content":"Hi there!"}]'
  echo '[{"role":"user","content":"Hello"}]' | anda-cli formation`,
	Run: func(cmd *cobra.Command, args []string) {
		messagesJSON, _ := cmd.Flags().GetString("messages")
		contextUser, _ := cmd.Flags().GetString("context-user")
		contextAgent, _ := cmd.Flags().GetString("context-agent")
		contextSession, _ := cmd.Flags().GetString("context-session")
		contextTopic, _ := cmd.Flags().GetString("context-topic")

		var messages []api.Message

		if messagesJSON == "" {
			stat, _ := os.Stdin.Stat()
			if (stat.Mode() & os.ModeCharDevice) == 0 {
				if err := json.NewDecoder(os.Stdin).Decode(&messages); err != nil {
					exitError(fmt.Errorf("parse stdin messages: %w", err))
				}
			} else {
				exitError(fmt.Errorf("--messages is required or pipe JSON via stdin"))
			}
		} else {
			if err := json.Unmarshal([]byte(messagesJSON), &messages); err != nil {
				exitError(fmt.Errorf("parse messages JSON: %w", err))
			}
		}

		input := &api.FormationInput{
			Messages:  messages,
			Timestamp: time.Now().UTC().Format(time.RFC3339),
		}

		ctx := buildInputContext(contextUser, contextAgent, contextSession, contextTopic)
		if ctx != nil {
			input.Context = ctx
		}

		client := newClient()
		resp, err := client.Formation(cmd.Context(), input)
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

func buildInputContext(user, agent, session, topic string) *api.InputContext {
	if user == "" && agent == "" && session == "" && topic == "" {
		return nil
	}
	return &api.InputContext{
		User:    user,
		Agent:   agent,
		Session: session,
		Topic:   topic,
	}
}

func init() {
	formationCmd.Flags().String("messages", "", "Messages as JSON array")
	formationCmd.Flags().String("context-user", "", "Context user")
	formationCmd.Flags().String("context-agent", "", "Context agent")
	formationCmd.Flags().String("context-session", "", "Context session")
	formationCmd.Flags().String("context-topic", "", "Context topic")
	rootCmd.AddCommand(formationCmd)
}
