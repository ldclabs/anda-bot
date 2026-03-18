package cmd

import (
	"encoding/base64"
	"fmt"

	"github.com/ldclabs/cose/key"
	"github.com/ldclabs/cose/key/ed25519"
	"github.com/spf13/cobra"
)

var keygenCmd = &cobra.Command{
	Use:   "keygen",
	Short: "Generate a new Ed25519 key pair",
	Long: `Generate a random Ed25519 key pair and output the private key
and public key as base64url-encoded CBOR (COSE Key format).

Example:
  anda-cli keygen
  anda-cli keygen --json`,
	Run: func(cmd *cobra.Command, args []string) {
		privKey, err := ed25519.GenerateKey()
		if err != nil {
			exitError(fmt.Errorf("generate key: %w", err))
		}

		pubKey, err := ed25519.ToPublicKey(privKey)
		if err != nil {
			exitError(fmt.Errorf("derive public key: %w", err))
		}

		privCBOR, err := key.MarshalCBOR(privKey)
		if err != nil {
			exitError(fmt.Errorf("marshal private key: %w", err))
		}
		pubCBOR, err := key.MarshalCBOR(pubKey)
		if err != nil {
			exitError(fmt.Errorf("marshal public key: %w", err))
		}

		privB64 := base64.RawURLEncoding.EncodeToString(privCBOR)
		pubB64 := base64.RawURLEncoding.EncodeToString(pubCBOR)
		kid := privKey.Kid()

		outputJSON, _ := cmd.Flags().GetBool("json")
		if outputJSON {
			printJSON(map[string]string{
				"kid":         kid.Base64(),
				"private_key": privB64,
				"public_key":  pubB64,
			})
		} else {
			fmt.Printf("Kid:         %s\n", kid.Base64())
			fmt.Printf("Private Key: %s\n", privB64)
			fmt.Printf("Public Key:  %s\n", pubB64)
		}
	},
}

func init() {
	keygenCmd.Flags().Bool("json", false, "Output in JSON format")
	rootCmd.AddCommand(keygenCmd)
}
