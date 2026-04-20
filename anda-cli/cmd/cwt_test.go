package cmd

import (
	"bytes"
	"encoding/base64"
	"os"
	"path/filepath"
	"testing"

	"github.com/ldclabs/cose/key"
	"github.com/ldclabs/cose/key/ed25519"
)

func TestReadPrivateKeyData(t *testing.T) {
	privKey, err := ed25519.GenerateKey()
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}

	privCBOR, err := key.MarshalCBOR(privKey)
	if err != nil {
		t.Fatalf("marshal key: %v", err)
	}

	base64URLKey := base64.RawURLEncoding.EncodeToString(privCBOR)
	base64Key := base64.StdEncoding.EncodeToString(privCBOR)

	tests := []struct {
		name  string
		input func(t *testing.T) string
	}{
		{
			name: "direct base64url",
			input: func(t *testing.T) string {
				return base64URLKey
			},
		},
		{
			name: "file path with base64",
			input: func(t *testing.T) string {
				path := filepath.Join(t.TempDir(), "private_key.txt")
				if err := os.WriteFile(path, []byte(base64Key+"\n"), 0o644); err != nil {
					t.Fatalf("write key file: %v", err)
				}
				return path
			},
		},
		{
			name: "at file with wrapped base64url",
			input: func(t *testing.T) string {
				path := filepath.Join(t.TempDir(), "private_key_wrapped.txt")
				wrapped := base64URLKey[:len(base64URLKey)/2] + "\n" + base64URLKey[len(base64URLKey)/2:]
				if err := os.WriteFile(path, []byte(wrapped), 0o644); err != nil {
					t.Fatalf("write wrapped key file: %v", err)
				}
				return "@" + path
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := readPrivateKeyData(tt.input(t))
			if err != nil {
				t.Fatalf("readPrivateKeyData returned error: %v", err)
			}
			if !bytes.Equal(got, privCBOR) {
				t.Fatalf("decoded key mismatch")
			}
		})
	}
}

func TestReadPrivateKeyData_EmptyAtFilePath(t *testing.T) {
	if _, err := readPrivateKeyData("@"); err == nil {
		t.Fatalf("expected error for empty @file path")
	}
}
