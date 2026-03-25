package cmd

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var executeKIPReadonlyCmd = &cobra.Command{
	Use:   "execute-kip-readonly",
	Short: "Execute a read-only KIP request",
	Long: `Execute a KIP request in read-only mode.

Input JSON can be provided via --request, --file, or stdin.

Example:
  anda-cli --space-id my_space --token $TOKEN execute-kip-readonly \
		--request '{"commands":["DESCRIBE PRIMER"]}'

  anda-cli --space-id my_space --token $TOKEN execute-kip-readonly --file ./kip_request.json

  cat kip_request.json | anda-cli --space-id my_space --token $TOKEN execute-kip-readonly`,
	Args: cobra.NoArgs,
	Run: func(cmd *cobra.Command, args []string) {
		requestJSON, _ := cmd.Flags().GetString("request")
		requestFile, _ := cmd.Flags().GetString("file")

		if requestJSON != "" && requestFile != "" {
			exitError(fmt.Errorf("--request and --file cannot be used together"))
		}

		raw, err := readKIPRequestInput(requestJSON, requestFile)
		if err != nil {
			exitError(err)
		}

		var input api.KipRequest
		if err := json.Unmarshal(raw, &input); err != nil {
			exitError(fmt.Errorf("invalid request JSON: %w", err))
		}
		if len(input.Commands) == 0 {
			exitError(fmt.Errorf("invalid request JSON: commands cannot be empty"))
		}

		client := newClient()
		resp, err := client.ExecuteKIPReadonly(cmd.Context(), &input)
		if err != nil {
			exitError(err)
		}
		if resp.Error != nil {
			exitError(resp.Error)
		}

		printJSON(resp)
	},
}

func readKIPRequestInput(requestJSON, requestFile string) ([]byte, error) {
	if requestJSON != "" {
		return []byte(strings.TrimSpace(requestJSON)), nil
	}

	if requestFile != "" {
		data, err := os.ReadFile(requestFile)
		if err != nil {
			return nil, fmt.Errorf("read file %q: %w", requestFile, err)
		}
		return []byte(strings.TrimSpace(string(data))), nil
	}

	stat, _ := os.Stdin.Stat()
	if (stat.Mode() & os.ModeCharDevice) == 0 {
		data, err := io.ReadAll(os.Stdin)
		if err != nil {
			return nil, fmt.Errorf("read stdin: %w", err)
		}
		trimmed := strings.TrimSpace(string(data))
		if trimmed == "" {
			return nil, fmt.Errorf("empty stdin input")
		}
		return []byte(trimmed), nil
	}

	return nil, fmt.Errorf("--request or --file is required, or pipe JSON via stdin")
}

func init() {
	executeKIPReadonlyCmd.Flags().String("request", "", "KIP request JSON string")
	executeKIPReadonlyCmd.Flags().String("file", "", "Read KIP request JSON from file")
	rootCmd.AddCommand(executeKIPReadonlyCmd)
}
