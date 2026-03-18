package api

// RpcError represents an API error.
type RpcError struct {
	Message string `json:"message"`
	Data    any    `json:"data,omitempty"`
}

func (e *RpcError) Error() string {
	return e.Message
}

// RpcResponse is the generic RPC envelope.
type RpcResponse[T any] struct {
	Result     *T        `json:"result,omitempty"`
	Error      *RpcError `json:"error,omitempty"`
	NextCursor string    `json:"next_cursor,omitempty"`
}

type TokenScope string

const (
	TokenScopeRead  TokenScope = "read"
	TokenScopeWrite TokenScope = "write"
	TokenScopeAll   TokenScope = "*"
)

type InputContext struct {
	User    string `json:"user,omitempty"`
	Agent   string `json:"agent,omitempty"`
	Session string `json:"session,omitempty"`
	Topic   string `json:"topic,omitempty"`
}

type MessageRole string

const (
	RoleSystem    MessageRole = "system"
	RoleUser      MessageRole = "user"
	RoleAssistant MessageRole = "assistant"
	RoleTool      MessageRole = "tool"
)

type Message struct {
	Role      MessageRole `json:"role"`
	Content   string      `json:"content"`
	Name      string      `json:"name,omitempty"`
	User      string      `json:"user,omitempty"`
	Timestamp *int64      `json:"timestamp,omitempty"`
}

type FormationInput struct {
	Messages  []Message     `json:"messages"`
	Context   *InputContext `json:"context,omitempty"`
	Timestamp string        `json:"timestamp"`
}

type RecallInput struct {
	Query   string       `json:"query"`
	Context *InputContext `json:"context,omitempty"`
}

type MaintenanceParameters struct {
	StaleEventThresholdDays *int     `json:"stale_event_threshold_days,omitempty"`
	ConfidenceDecayFactor   *float64 `json:"confidence_decay_factor,omitempty"`
	UnsortedMaxBacklog      *int     `json:"unsorted_max_backlog,omitempty"`
	OrphanMaxCount          *int     `json:"orphan_max_count,omitempty"`
}

type MaintenanceInput struct {
	Trigger    string                 `json:"trigger,omitempty"`
	Scope      string                 `json:"scope,omitempty"`
	Timestamp  string                 `json:"timestamp"`
	Parameters *MaintenanceParameters `json:"parameters,omitempty"`
}

type AddSpaceTokenInput struct {
	Scope TokenScope `json:"scope"`
}

type RevokeSpaceTokenInput struct {
	Token string `json:"token"`
}

type UpdateSpaceInput struct {
	Name        *string `json:"name,omitempty"`
	Description *string `json:"description,omitempty"`
	Public      *bool   `json:"public,omitempty"`
}

type CreateOrUpdateSpaceInput struct {
	User    string `json:"user"`
	SpaceID string `json:"space_id"`
	Tier    int    `json:"tier"`
}

type SpaceTier struct {
	Tier      int   `json:"tier"`
	UpdatedAt int64 `json:"updated_at"`
}

type SpaceToken struct {
	Token     string     `json:"token"`
	Scope     TokenScope `json:"scope"`
	Usage     int        `json:"usage"`
	CreatedAt int64      `json:"created_at"`
	UpdatedAt int64      `json:"updated_at"`
}

type StorageStats map[string]any

type SpaceStatus struct {
	ID            string       `json:"id"`
	Name          string       `json:"name,omitempty"`
	Description   string       `json:"description,omitempty"`
	Owner         string       `json:"owner"`
	DBStats       StorageStats `json:"db_stats"`
	Concepts      int          `json:"concepts"`
	Propositions  int          `json:"propositions"`
	Conversations int          `json:"conversations"`
	Public        bool         `json:"public"`
	Tier          SpaceTier    `json:"tier"`
}

type Usage struct {
	InputTokens  *int `json:"input_tokens,omitempty"`
	OutputTokens *int `json:"output_tokens,omitempty"`
	TotalTokens  *int `json:"total_tokens,omitempty"`
}

type AgentOutput struct {
	Content      string `json:"content"`
	Conversation *int   `json:"conversation,omitempty"`
	FailedReason string `json:"failed_reason,omitempty"`
	Usage        *Usage `json:"usage,omitempty"`
	Model        string `json:"model,omitempty"`
}

type ConversationStatus string

const (
	StatusSubmitted ConversationStatus = "submitted"
	StatusWorking   ConversationStatus = "working"
	StatusCompleted ConversationStatus = "completed"
	StatusFailed    ConversationStatus = "failed"
	StatusCancelled ConversationStatus = "cancelled"
)

type Conversation struct {
	ID               int                `json:"_id"`
	User             string             `json:"user"`
	Thread           string             `json:"thread,omitempty"`
	Messages         []Message          `json:"messages"`
	Resources        []any              `json:"resources"`
	Artifacts        []any              `json:"artifacts"`
	Status           ConversationStatus `json:"status"`
	FailedReason     *string            `json:"failed_reason,omitempty"`
	Period           int                `json:"period"`
	CreatedAt        int64              `json:"created_at"`
	UpdatedAt        int64              `json:"updated_at"`
	Usage            Usage              `json:"usage"`
	SteeringMessages []string           `json:"steering_messages,omitempty"`
	FollowUpMessages []string           `json:"follow_up_messages,omitempty"`
	Ancestors        []int              `json:"ancestors,omitempty"`
}

type ServiceInfo struct {
	Name        string `json:"name"`
	Version     string `json:"version"`
	Sharding    int    `json:"sharding"`
	Description string `json:"description"`
}
