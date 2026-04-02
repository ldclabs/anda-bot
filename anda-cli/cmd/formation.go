package cmd

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

const maxMessageContentTokens = 100_000

var formationCmd = &cobra.Command{
	Use:   "formation",
	Short: "Submit a memory formation task",
	Long: `Submit conversation messages for memory encoding.

Messages are provided via --messages or stdin.
Input can be a JSON message array/object, or plain text.
Plain text is treated as one message: role="user", content=<text>.

Example:
  anda-cli formation --messages '[{"role":"user","content":"Hello"},{"role":"assistant","content":"Hi there!"}]'
  anda-cli formation --file ./message.txt
  echo '[{"role":"user","content":"Hello"}]' | anda-cli formation`,
	Run: func(cmd *cobra.Command, args []string) {
		messagesJSON, _ := cmd.Flags().GetString("messages")
		messagesFile, _ := cmd.Flags().GetString("file")
		batchDir, _ := cmd.Flags().GetString("batch-dir")
		batchFileName, _ := cmd.Flags().GetString("batch-file-name")
		batchExt, _ := cmd.Flags().GetString("batch-ext")
		batchReport, _ := cmd.Flags().GetString("batch-report")
		batchRetryFailed, _ := cmd.Flags().GetBool("batch-retry-failed")
		batchDryRun, _ := cmd.Flags().GetBool("batch-dry-run")
		contextUser, _ := cmd.Flags().GetString("context-user")
		contextAgent, _ := cmd.Flags().GetString("context-agent")
		contextSession, _ := cmd.Flags().GetString("context-session")
		contextTopic, _ := cmd.Flags().GetString("context-topic")

		ctx := buildInputContext(contextUser, contextAgent, contextSession, contextTopic)

		if batchDir != "" {
			if messagesJSON != "" || messagesFile != "" {
				exitError(fmt.Errorf("--batch-dir cannot be used with --messages or --file"))
			}

			client := newClient()
			err := runFileFormationBatch(cmd.Context(), client, fileFormationBatchOptions{
				RootDir:      batchDir,
				FileName:     batchFileName,
				Extension:    batchExt,
				ReportPath:   batchReport,
				RetryFailed:  batchRetryFailed,
				DryRun:       batchDryRun,
				InputContext: ctx,
			})
			if err != nil {
				exitError(err)
			}
			return
		}

		var messages []api.Message

		if messagesJSON != "" && messagesFile != "" {
			exitError(fmt.Errorf("--messages and --file cannot be used together"))
		}

		if messagesJSON != "" {
			var err error
			messages, err = parseMessagesInput(messagesJSON)
			if err != nil {
				exitError(fmt.Errorf("parse messages input: %w", err))
			}
		} else if messagesFile != "" {
			data, err := os.ReadFile(messagesFile)
			if err != nil {
				exitError(fmt.Errorf("read file %q: %w", messagesFile, err))
			}
			messages, err = parseMessagesInput(string(data))
			if err != nil {
				exitError(fmt.Errorf("parse file input: %w", err))
			}
		} else {
			stat, _ := os.Stdin.Stat()
			if (stat.Mode() & os.ModeCharDevice) == 0 {
				data, err := io.ReadAll(os.Stdin)
				if err != nil {
					exitError(fmt.Errorf("read stdin: %w", err))
				}
				messages, err = parseMessagesInput(string(data))
				if err != nil {
					exitError(fmt.Errorf("parse stdin messages: %w", err))
				}
			} else {
				exitError(fmt.Errorf("--messages or --file is required, or pipe input via stdin"))
			}
		}

		if err := validateMessageContentLength(messages); err != nil {
			exitError(err)
		}

		input := &api.FormationInput{
			Messages:  messages,
			Timestamp: time.Now().UTC().Format(time.RFC3339),
		}

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
			printJSON(resp.Result)
		}
	},
}

func parseMessagesInput(raw string) ([]api.Message, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil, fmt.Errorf("empty input")
	}

	var messages []api.Message
	if err := json.Unmarshal([]byte(raw), &messages); err == nil {
		if len(messages) == 0 {
			return nil, fmt.Errorf("messages cannot be empty")
		}
		return messages, nil
	}

	var single api.Message
	if err := json.Unmarshal([]byte(raw), &single); err == nil {
		return []api.Message{single}, nil
	}

	return []api.Message{{
		Role:    "user",
		Content: api.MessageContentFromText(raw),
	}}, nil
}

func validateMessageContentLength(messages []api.Message) error {
	for idx, message := range messages {
		contentTokens := message.Content.SizeBytes() / 3
		if contentTokens > maxMessageContentTokens {
			return fmt.Errorf("message[%d] content is %d tokens (estimated), exceeds %d-token limit", idx, contentTokens, maxMessageContentTokens)
		}
	}
	return nil
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
	formationCmd.Flags().String("messages", "", "Messages as JSON or plain text")
	formationCmd.Flags().String("file", "", "Read messages from file (JSON or plain text)")
	formationCmd.Flags().String("batch-dir", "", "Recursively submit files under the given directory")
	formationCmd.Flags().String("batch-file-name", "", "Submit files with exact filename match (case-insensitive), e.g. Skill.md")
	formationCmd.Flags().String("batch-ext", "", "Submit files by extension, e.g. .md or md")
	formationCmd.Flags().String("batch-report", "", "Batch checklist JSON path (default: <batch-dir>/.formation-batch-checklist.json)")
	formationCmd.Flags().Bool("batch-retry-failed", false, "Retry files previously marked as failed in checklist")
	formationCmd.Flags().Bool("batch-dry-run", false, "Dry run: scan and report matched files without submitting formation")
	formationCmd.Flags().String("context-user", "", "Context user")
	formationCmd.Flags().String("context-agent", "", "Context agent")
	formationCmd.Flags().String("context-session", "", "Context session")
	formationCmd.Flags().String("context-topic", "", "Context topic")
	rootCmd.AddCommand(formationCmd)
}
