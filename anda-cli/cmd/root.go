package cmd

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/ldclabs/anda-hippocampus/anda-cli/api"
	"github.com/spf13/cobra"
)

var (
	baseURL string
	spaceID string
	token   string
)

func newClient() *api.Client {
	return api.NewClient(baseURL, spaceID, token)
}

func printJSON(v any) {
	data, err := json.MarshalIndent(v, "", "  ")
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}
	fmt.Println(string(data))
}

func exitError(err error) {
	fmt.Fprintf(os.Stderr, "Error: %v\n", err)
	os.Exit(1)
}

var rootCmd = &cobra.Command{
	Use:   "anda-cli",
	Short: "CLI tool for Anda Hippocampus API",
	Long:  "A command-line interface for interacting with the Anda Hippocampus memory service.",
}

func Execute() {
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}

func init() {
	rootCmd.PersistentFlags().StringVar(&baseURL, "base-url", envOrDefault("ANDA_BASE_URL", api.DefaultBaseURL), "API base URL (env: ANDA_BASE_URL)")
	rootCmd.PersistentFlags().StringVar(&spaceID, "space-id", os.Getenv("ANDA_SPACE_ID"), "Space ID (env: ANDA_SPACE_ID)")
	rootCmd.PersistentFlags().StringVar(&token, "token", os.Getenv("ANDA_TOKEN"), "Auth token (env: ANDA_TOKEN)")
}

func envOrDefault(key, defaultVal string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return defaultVal
}
