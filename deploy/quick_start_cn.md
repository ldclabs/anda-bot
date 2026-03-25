# Anda Hippocampus 快速开始（中文）

本文提供一条从 0 到可用的最小流程，包含：

- 安装 `anda-cli`（优先）
- 配置环境变量
- 启动服务（本地构建 或 Docker，本地文件存储）
- 生成 CWT
- 创建 Space
- 提交 Formation
- 执行 Recall
- 读取会话记录

## 0. 前置条件

- macOS / Linux
- 已安装：`git`、`jq`、`curl`
- 若使用本地构建：`rust`、`cargo`、`go`
- 若使用 Docker：`docker`
- 可用的大模型 API Key（例如 Gemini、MiniMax、小米 Mimo）

## 1. 工作目录
```bash
mkdir anda-brain
cd anda-brain
mkdir db
```

## 2. 安装 `anda-cli`（先完成）

二选一：从 Releases 下载，或本地构建。

### 方案 A：从 Releases 下载可执行文件（推荐）

仓库 Releases：

- https://github.com/ldclabs/anda-hippocampus/releases

下载与你系统匹配的 `anda-cli` 可执行文件后：

```bash
wget -O anda-cli https://github.com/ldclabs/anda-hippocampus/releases/download/v0.3.2/anda-cli-macos-arm64
chmod +x anda-cli
```

### 方案 B：本地构建 `anda-cli`

```bash
cd path/to/anda-hippocampus/anda-cli
go build -o anda-cli .
chmod +x anda-cli
mv anda-cli ../anda-brain/
cd ../anda-brain/
```

验证：

```bash
./anda-cli --help
```

## 3. 准备变量与密钥

### 3.1 生成 Ed25519 密钥（用于 CWT 签名）

```bash
./anda-cli keygen --json > keys.json
cat keys.json
```

### 3.2 配置通用环境变量

创建运行 Anda Hippocampus 需要的 `.env` 文件，内容示例如下：
```bash
LOG_LEVEL='info'
LISTEN_ADDR='0.0.0.0:8042'
SHARDING_IDX='0'
# 上一步生成的公钥，多个公钥用逗号分隔
ED25519_PUBKEYS='YOUR_ED25519_PUBKEYS'
# 可以替换成你自己的合法 Principal 文本（例如你已有的 principal id）。
MANAGERS="aaaaa-aa"
# 可替换为你使用的模型系列，例如 'gemini'、'openai'、'deepseek' 等
MODEL_FAMILY='anthropic'
MODEL_NAME='MiniMax-M2.7-highspeed'
MODEL_API_BASE='https://api.minimaxi.com/anthropic/v1'
MODEL_API_KEY='YOUR_MODEL_API_KEY'
```

## 4. 启动 Anda Hippocampus（本地文件存储）

二选一：本地构建运行，或 Docker 运行（支持远端拉取镜像）。

### 方案 A：本地构建运行

```bash
cd path/to/anda-hippocampus
cargo build -p anda_hippocampus --release
mv target/release/anda_hippocampus ../anda-brain/
cd ../anda-brain/

./anda_hippocampus local --db ./db
```

如果你不想本地编译，也可以在 Releases 页面下载对应系统的 `anda_hippocampus` 可执行程序：

- https://github.com/ldclabs/anda-hippocampus/releases

下载后同样使用 `local --db ./db` 启动即可。

### 方案 B：Docker 运行（本地文件存储）

#### B1. 直接拉取远端镜像（推荐）

在 Apple Silicon（M1/M2/M3）macOS 上，建议显式指定平台，避免出现
`requested image's platform (linux/amd64) does not match ...` 警告：

```bash
export DOCKER_PLATFORM=linux/amd64

docker pull --platform $DOCKER_PLATFORM ghcr.io/ldclabs/anda_hippocampus_amd64:latest

docker run --rm --platform $DOCKER_PLATFORM -p 8042:8042 \
	-v "$(pwd)/db:/app/db" \
  -v "$(pwd)/.env:/app/.env" \
	ghcr.io/ldclabs/anda_hippocampus_amd64:latest local --db /app/db
```

#### B2. 本地构建 Docker 镜像

```bash
# Apple Silicon 推荐构建 arm64 本地镜像
docker buildx build --platform linux/arm64 -f anda_hippocampus/Dockerfile -t anda_hippocampus:local --load .
```

### 4.2 验证服务可用

另开一个终端执行：


```bash
# 先设置环境变量
export ANDA_CWT_KEY="$(jq -r '.private_key' keys.json)"
export ANDA_BASE_URL='http://127.0.0.1:8042'
```

```bash
curl -s "$ANDA_BASE_URL/info" | jq .
```

## 5. 生成 CWT

### 5.1 生成管理员 Token（用于创建 Space）

```bash
export ANDA_TOKEN="$(./anda-cli cwt \
	--subject "aaaaa-aa" \
	--audience '*' \
	--scope '*' \
	--expiration 7200 \
	--json | jq -r '.token')"
```

## 6. 创建 Space

```bash
./anda-cli admin create-space \
	--user "aaaaa-aa" \
	--space-id "demo" \
	--tier 4
```

如果需要创建 space 的 CWT token：
```bash
./anda-cli cwt \
	--subject "aaaaa-aa" \
	--audience "demo" \
	--scope '*' \
	--expiration 7200 \
	--json | jq
```

创建 Space token 用于 Openclaw 或其它 agent 集成：
```bash
./anda-cli --space-id demo management add-token --scope "*" --name openclaw
```

查看在用的 Space tokens：
```bash
./anda-cli --space-id demo management list-tokens
```

撤销 Space token：
```bash
./anda-cli --space-id demo management revoke-token STxxx
```

查看 Space 信息：
```bash
./anda-cli --space-id demo info
```

## 7. 提交 Formation

```bash
# 这里使用了 ANDA_TOKEN 环境变量
./anda-cli --space-id demo \
	formation --messages '[
		{"role":"user","content":"我偏好深色模式，时区是 UTC+8。"},
		{"role":"assistant","content":"好的，我记住了你的偏好和时区。"}
	]'
```

Formation 是异步处理，可用 `formation-status` 或会话列表观察进度。
```bash
./anda-cli --space-id demo formation-status
```

查看完整 Formation 处理日志：
```bash
anda-cli --space-id demo conversations get 1
```

查看更多 Formation 相关命令：

```bash
anda-cli formation --help
```

## 8. 执行 Recall

Recall 是同步处理，等待最终结果返回：
```bash
./anda-cli --space-id demo \
	recall "这个用户有哪些偏好？"
```

查看完整 Recall 处理日志：
```bash
anda-cli --space-id demo conversations --collection recall get 1
```

## 9. 查看会话记录

### 9.2 列出会话

列出最近 10 条 Formation 会话记录：
```bash
./anda-cli --space-id demo \
	conversations list --limit 10
```

列出最近 10 条 Recall 会话记录：
```bash
./anda-cli --space-id demo \
	conversations --collection recall list --limit 10
```

列出最近 10 条 Maintenance 会话记录：
```bash
./anda-cli --space-id demo \
	conversations --collection maintenance list --limit 10
```

### 9.3 获取单条会话详情

将 `<conversation_id>` 替换为上一步返回的会话 ID：

```bash
./anda-cli --space-id demo \
	conversations get <conversation_id>
```



## 10. 集成

### 10.1 直接使用 HTTP API

1. 记忆：发送对话以进行记忆编码
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

2. 召回：在响应前查询记忆
```bash
curl -sX POST http://localhost:8042/v1/demo/recall \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "Where does this user work and what is their role?",
    "context": {"user": "user_123"}
  }'
```

### 10.2 与 OpenClaw 集成

安装提示词：
```txt
从 https://brain.anda.ai/SKILL.md 安装插件：
spaceId: your_space_id
spaceToken: your_space_token
baseUrl: "http://localhost:8042"
```
