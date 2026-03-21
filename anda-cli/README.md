# Anda CLI

A command-line tool for interacting with the [Anda Hippocampus](https://brain.anda.ai) memory service API.

## Install

```bash
go install github.com/ldclabs/anda-hippocampus/anda-cli@latest
```

Or build from source:

```bash
cd anda-cli
go build -o anda-cli .
```

## Configuration

Configuration can be provided via flags or environment variables:

| Flag         | Env Variable    | Description  | Default                 |
| ------------ | --------------- | ------------ | ----------------------- |
| `--base-url` | `ANDA_BASE_URL` | API base URL | `https://brain.anda.ai` |
| `--space-id` | `ANDA_SPACE_ID` | Space ID     |                         |
| `--token`    | `ANDA_TOKEN`    | Auth token   |                         |

**CWT command flags:**

| Flag         | Env Variable        | Description                          | Default |
| ------------ | ------------------- | ------------------------------------ | ------- |
| `--key`      | `ANDA_CWT_KEY`      | Ed25519 private key (base64url CBOR) |         |
| `--subject`  | `ANDA_CWT_SUBJECT`  | Subject claim - user/principal ID    |         |
| `--audience` | `ANDA_CWT_AUDIENCE` | Audience claim - space ID or `*`     |         |
| `--scope`    | `ANDA_CWT_SCOPE`    | Scope claim: read, write or `*`      | `read`  |
| `--issuer`   | `ANDA_CWT_ISSUER`   | Issuer claim                         |         |

## Commands

### Key Generation & CWT

```bash
# Generate a new Ed25519 key pair (base64url-encoded COSE Key)
anda-cli keygen
anda-cli keygen --json

# Create a CWT (CBOR Web Token) signed with Ed25519 private key
anda-cli cwt --key <base64url_private_key> --subject <user_id> --audience <space_id> --scope write

# Create a CWT with wildcard audience and 2-hour expiration
anda-cli cwt --key <base64url_private_key> --subject <user_id> --audience "*" --scope "*" --expiration 7200
```

### Service

```bash
# Get service information
anda-cli info
```

### Memory Operations

```bash
# Submit memory formation
anda-cli --space-id my_space --token $TOKEN formation \
  --messages '[{"role":"user","content":"Hello"},{"role":"assistant","content":"Hi!"}]'

# Submit memory formation with plain text (--messages)
anda-cli --space-id my_space --token $TOKEN formation \
  --messages 'Hello, this is a plain text memory.'

# Submit memory formation from file (JSON or plain text)
anda-cli --space-id my_space --token $TOKEN formation \
  --file ./message.txt

# Or pipe from stdin
echo '[{"role":"user","content":"Hello"}]' | \
  anda-cli --space-id my_space --token $TOKEN formation

# Or pipe plain text from stdin
echo 'Hello from stdin plain text' | \
  anda-cli --space-id my_space --token $TOKEN formation

# Batch submit files by exact filename (recursive)
anda-cli --space-id my_space --token $TOKEN formation \
  --batch-dir ./docs \
  --batch-file-name Skill.md

# Batch submit files by extension (recursive)
anda-cli --space-id my_space --token $TOKEN formation \
  --batch-dir ./docs \
  --batch-ext .md

# Resume from checklist and retry only previously failed files
anda-cli --space-id my_space --token $TOKEN formation \
  --batch-dir ./docs \
  --batch-ext .md \
  --batch-retry-failed

# Use a custom checklist path
anda-cli --space-id my_space --token $TOKEN formation \
  --batch-dir ./docs \
  --batch-file-name Skill.md \
  --batch-report ./tmp/formation-batch-checklist.json

# Dry run: only scan and print matched files, no formation submission
anda-cli --space-id my_space --token $TOKEN formation \
  --batch-dir ./docs \
  --batch-ext .md \
  --batch-dry-run

# Recall memory
anda-cli --space-id my_space --token $TOKEN recall "What are the user's preferences?"

# Recall with context
anda-cli --space-id my_space --token $TOKEN recall \
  --context-user u1 "What happened in the last meeting?"

# Trigger maintenance
anda-cli --space-id my_space --token $TOKEN maintenance
anda-cli --space-id my_space --token $TOKEN maintenance --trigger on_demand --scope full
```

### Space Status & Conversations

```bash
# Get space status
anda-cli --space-id my_space --token $TOKEN status

# List conversations
anda-cli --space-id my_space --token $TOKEN conversations list --limit 10

# Get a specific conversation
anda-cli --space-id my_space --token $TOKEN conversations get 42
```

### Space Management (requires CWT auth)

```bash
# List space tokens
anda-cli --space-id my_space --token $CWT_TOKEN management list-tokens

# Add a space token
anda-cli --space-id my_space --token $CWT_TOKEN management add-token --scope write

# Revoke a space token
anda-cli --space-id my_space --token $CWT_TOKEN management revoke-token ST_xxx

# Update space info
anda-cli --space-id my_space --token $CWT_TOKEN management update-space --name "My Space" --public

# Restart formation for a conversation
anda-cli --space-id my_space --token $CWT_TOKEN management restart-formation --conversation 42

# Update BYOK configuration
anda-cli --space-id my_space --token $CWT_TOKEN management update-byok \
  --family anthropic \
  --model claude-opus-4-6 \
  --api-base https://api.anthropic.com/v1 \
  --api-key sk-xxx
```

### Admin (requires platform admin auth)

```bash
# Create a space
anda-cli --token $ADMIN_TOKEN admin create-space --user owner_id --space-id new_space --tier 1

# Update space tier
anda-cli --token $ADMIN_TOKEN admin update-tier --user owner_id --space-id my_space --tier 2
```
