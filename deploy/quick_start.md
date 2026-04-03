# Anda Hippocampus Quick Start

This guide provides a minimal flow from zero to a usable state, including:

- Installing `anda-cli` (Priority)
- Configuring environment variables
- Starting the service (Local Build or Docker, Local File Storage)
- Generating a CWT
- Creating a Space
- Submitting Formation
- Executing Recall
- Reading conversation logs

## 0. Prerequisites

- macOS / Linux
- Installed tools: `git`, `jq`, `curl`
- If building from source: `rust`, `cargo`, `go`
- If using Docker: `docker`
- A valid LLM API Key (e.g., Gemini, MiniMax, Xiaomi Mimo)

## 1. Working Directory
```bash
mkdir anda-brain
cd anda-brain
mkdir db
```

## 2. Install `anda-cli` (Complete this first)

Choose one: Download from Releases, or build from source.

### Option A: Download the executable from Releases (Recommended)

Repository Releases:

- https://github.com/ldclabs/anda-hippocampus/releases

After downloading the `anda-cli` executable that matches your system:

```bash
wget -O anda-cli https://github.com/ldclabs/anda-hippocampus/releases/download/v0.3.2/anda-cli-macos-arm64
chmod +x anda-cli
```

### Option B: Build `anda-cli` locally

```bash
cd path/to/anda-hippocampus/anda-cli
go build -o anda-cli .
chmod +x anda-cli
mv anda-cli ../anda-brain/
cd ../anda-brain/
```

Verify the installation:

```bash
./anda-cli --help
```

## 3. Prepare Variables and Keys

### 3.1 Generate Ed25519 Key (for CWT signing)

```bash
./anda-cli keygen --json > keys.json
cat keys.json
```

### 3.2 Configure General Environment Variables

Create a `.env` file required to run Anda Hippocampus. Example content:
```bash
LOG_LEVEL='info'
LISTEN_ADDR='0.0.0.0:8042'
SHARDING_IDX='0'
# The public key generated in the previous step. Separate multiple public keys with commas.
ED25519_PUBKEYS='YOUR_ED25519_PUBKEYS'
# You can replace this with your own valid Principal text (e.g., your existing principal id).
MANAGERS="aaaaa-aa"
# Can be replaced with the model family you are using, e.g., 'gemini', 'openai', 'deepseek', etc.
MODEL_FAMILY='anthropic'
MODEL_NAME='MiniMax-M2.7-highspeed'
MODEL_API_BASE='https://api.minimaxi.com/anthropic/v1'
MODEL_API_KEY='YOUR_MODEL_API_KEY'
```

## 4. Start Anda Hippocampus (Local File Storage)

Choose one: Run via local build, or run via Docker (supports pulling remote images).

### Option A: Run via local build

```bash
cd path/to/anda-hippocampus
cargo build -p anda_hippocampus --release
mv target/release/anda_hippocampus ../anda-brain/
cd ../anda-brain/

./anda_hippocampus local --db ./db
```

If you prefer not to compile locally, you can also download the corresponding `anda_hippocampus` executable for your system from the Releases page:

- https://github.com/ldclabs/anda-hippocampus/releases

After downloading, you can start it similarly using `local --db ./db`.

### Option B: Run via Docker (Local file storage)

#### B1. Pull the remote image directly (Recommended)

On Apple Silicon (M1/M2/M3) macOS, it is recommended to explicitly specify the platform to avoid the `requested image's platform (linux/amd64) does not match ...` warning:

```bash
export DOCKER_PLATFORM=linux/amd64

docker pull --platform $DOCKER_PLATFORM ghcr.io/ldclabs/anda_hippocampus_amd64:latest

docker run --rm --platform $DOCKER_PLATFORM -p 8042:8042 \
	-v "$(pwd)/db:/app/db" \
  -v "$(pwd)/.env:/app/.env" \
	ghcr.io/ldclabs/anda_hippocampus_amd64:latest local --db /app/db
```

#### B2. Build the Docker image locally

```bash
# Building an arm64 local image is recommended for Apple Silicon
docker buildx build --platform linux/arm64 -f anda_hippocampus/Dockerfile -t anda_hippocampus:local --load .
```

### 4.2 Verify service availability

Open another terminal and execute:

```bash
# Set environment variables first
export ANDA_CWT_KEY="$(jq -r '.private_key' keys.json)"
export ANDA_BASE_URL='http://127.0.0.1:8042'
```

```bash
curl -s "$ANDA_BASE_URL/info" | jq .
```

## 5. Generate CWT

### 5.1 Generate an Admin Token (for creating a Space)

```bash
export ANDA_TOKEN="$(./anda-cli cwt \
	--subject "aaaaa-aa" \
	--audience '*' \
	--scope '*' \
	--expiration 7200 \
	--json | jq -r '.token')"
```

## 6. Create a Space

```bash
./anda-cli admin create-space \
	--user "aaaaa-aa" \
	--space-id "demo" \
	--tier 4
```

If you need to create a CWT token for the space:
```bash
./anda-cli cwt \
	--subject "aaaaa-aa" \
	--audience "demo" \
	--scope '*' \
	--expiration 7200 \
	--json | jq
```

Create a Space token for Openclaw or other agent integrations:
```bash
./anda-cli --space-id demo management add-token --scope "*" --name openclaw
```

View active Space tokens:
```bash
./anda-cli --space-id demo management list-tokens
```

Revoke a Space token:
```bash
./anda-cli --space-id demo management revoke-token STxxx
```

View Space info:
```bash
./anda-cli --space-id demo info
```

## 7. Submit Formation

```bash
# The ANDA_TOKEN environment variable is used here
./anda-cli --space-id demo \
	formation --messages '[
		{"role":"user","content":"I prefer dark mode, and my timezone is UTC+8."},
		{"role":"assistant","content":"Got it, I have noted your preference and timezone."}
	]'
```

Formation is processed asynchronously. You can monitor the progress using `formation-status` or by checking the conversation list.
```bash
./anda-cli --space-id demo formation-status
```

View complete Formation processing logs:
```bash
anda-cli --space-id demo conversations get 1
```

View more Formation-related commands:

```bash
anda-cli formation --help
```

## 8. Execute Recall

Recall is processed synchronously and will wait for the final result to return:
```bash
./anda-cli --space-id demo \
	recall "What are the preferences of this user?"
```

View complete Recall processing logs:
```bash
anda-cli --space-id demo conversations --collection recall get 1
```

## 9. View Conversation Logs

### 9.1 List Conversations

List the latest 10 Formation conversation records:
```bash
./anda-cli --space-id demo \
	conversations list --limit 10
```

List the latest 10 Recall conversation records:
```bash
./anda-cli --space-id demo \
	conversations --collection recall list --limit 10
```

List the latest 10 Maintenance conversation records:
```bash
./anda-cli --space-id demo \
	conversations --collection maintenance list --limit 10
```

### 9.2 Get Details of a Single Conversation

Replace `<conversation_id>` with the conversation ID returned in the previous step:

```bash
./anda-cli --space-id demo \
	conversations get <conversation_id>
```

## 10. Integration

### 10.1 Direct use of the HTTP API

1. Memorize (Formation): Send conversations to encode memories
```bash
curl -sX POST http://localhost:8042/v1/demo/formation \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "I work at Acme Corp as a senior engineer."},
      {"role": "assistant", "content": "Nice to meet you! Noted that you are a senior engineer at Acme Corp."}
    ],
    "context": {"user": "user_123", "agent": "onboarding_bot"},
    "timestamp": "2026-03-25T10:30:00Z"
  }'
```

2. Recall: Query memories before generating a response
```bash
curl -sX POST http://localhost:8042/v1/demo/recall \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "Where does this user work and what is their role?",
    "context": {"user": "user_123"}
  }'
```

### 10.2 Integration with OpenClaw

Installation Prompt:
```txt
Install the plugin from https://brain.anda.ai/SKILL.md:
spaceId: your_space_id
spaceToken: your_space_token
baseUrl: "http://localhost:8042"
```