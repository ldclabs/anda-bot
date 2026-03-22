package api

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
)

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
	Role      MessageRole    `json:"role"`
	Content   MessageContent `json:"content"`
	Name      string         `json:"name,omitempty"`
	User      string         `json:"user,omitempty"`
	Timestamp *int64         `json:"timestamp,omitempty"`
}

type ContentPartType string

const (
	ContentPartText       ContentPartType = "Text"
	ContentPartReasoning  ContentPartType = "Reasoning"
	ContentPartFileData   ContentPartType = "FileData"
	ContentPartInlineData ContentPartType = "InlineData"
	ContentPartToolCall   ContentPartType = "ToolCall"
	ContentPartToolOutput ContentPartType = "ToolOutput"
	ContentPartAction     ContentPartType = "Action"
	ContentPartAny        ContentPartType = "Any"
)

type ContentPart interface {
	contentPartType() ContentPartType
}

type TextPart struct {
	Type ContentPartType `json:"type"`
	Text string          `json:"text"`
}

func (TextPart) contentPartType() ContentPartType { return ContentPartText }

type ReasoningPart struct {
	Type ContentPartType `json:"type"`
	Text string          `json:"text"`
}

func (ReasoningPart) contentPartType() ContentPartType { return ContentPartReasoning }

type FileDataPart struct {
	Type     ContentPartType `json:"type"`
	FileURI  string          `json:"fileUri"`
	MimeType *string         `json:"mimeType,omitempty"`
}

func (FileDataPart) contentPartType() ContentPartType { return ContentPartFileData }

type InlineDataPart struct {
	Type     ContentPartType `json:"type"`
	MimeType string          `json:"mimeType"`
	Data     any             `json:"data"`
}

func (InlineDataPart) contentPartType() ContentPartType { return ContentPartInlineData }

type ToolCallPart struct {
	Type   ContentPartType `json:"type"`
	Name   string          `json:"name"`
	Args   any             `json:"args"`
	CallID *string         `json:"callId,omitempty"`
}

func (ToolCallPart) contentPartType() ContentPartType { return ContentPartToolCall }

type ToolOutputPart struct {
	Type     ContentPartType `json:"type"`
	Name     string          `json:"name"`
	Output   any             `json:"output"`
	CallID   *string         `json:"callId,omitempty"`
	RemoteID *string         `json:"remoteId,omitempty"`
}

func (ToolOutputPart) contentPartType() ContentPartType { return ContentPartToolOutput }

type ActionPart struct {
	Type       ContentPartType `json:"type"`
	Name       string          `json:"name"`
	Payload    any             `json:"payload"`
	Recipients []string        `json:"recipients,omitempty"`
	Signature  *string         `json:"signature,omitempty"`
}

func (ActionPart) contentPartType() ContentPartType { return ContentPartAction }

type AnyPart struct {
	Raw json.RawMessage
}

func (AnyPart) contentPartType() ContentPartType { return ContentPartAny }

func (p AnyPart) MarshalJSON() ([]byte, error) {
	if len(p.Raw) > 0 {
		return p.Raw, nil
	}
	return json.Marshal(map[string]any{"type": string(ContentPartAny)})
}

type MessageContent []ContentPart

func NewTextContentPart(text string) ContentPart {
	return TextPart{Type: ContentPartText, Text: text}
}

func MessageContentFromText(text string) MessageContent {
	return MessageContent{NewTextContentPart(text)}
}

func parseContentPart(raw json.RawMessage) (ContentPart, error) {
	trimmed := bytes.TrimSpace(raw)
	if len(trimmed) == 0 {
		return AnyPart{Raw: append(json.RawMessage(nil), trimmed...)}, nil
	}

	if trimmed[0] == '"' {
		var text string
		if err := json.Unmarshal(trimmed, &text); err != nil {
			return nil, err
		}
		return TextPart{Type: ContentPartText, Text: text}, nil
	}

	if trimmed[0] != '{' {
		return AnyPart{Raw: append(json.RawMessage(nil), trimmed...)}, nil
	}

	var fields map[string]json.RawMessage
	if err := json.Unmarshal(trimmed, &fields); err != nil {
		return AnyPart{Raw: append(json.RawMessage(nil), trimmed...)}, nil
	}

	metaRaw, ok := fields["type"]
	if !ok {
		return AnyPart{Raw: append(json.RawMessage(nil), trimmed...)}, nil
	}

	var partType ContentPartType
	if err := json.Unmarshal(metaRaw, &partType); err != nil {
		return AnyPart{Raw: append(json.RawMessage(nil), trimmed...)}, nil
	}

	hasField := func(name string) bool {
		_, ok := fields[name]
		return ok
	}

	switch partType {
	case ContentPartText:
		if !hasField("text") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part TextPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartText
		return part, nil
	case ContentPartReasoning:
		if !hasField("text") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part ReasoningPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartReasoning
		return part, nil
	case ContentPartFileData:
		if !hasField("fileUri") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part FileDataPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartFileData
		return part, nil
	case ContentPartInlineData:
		if !hasField("mimeType") || !hasField("data") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part InlineDataPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartInlineData
		return part, nil
	case ContentPartToolCall:
		if !hasField("name") || !hasField("args") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part ToolCallPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartToolCall
		return part, nil
	case ContentPartToolOutput:
		if !hasField("name") || !hasField("output") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part ToolOutputPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartToolOutput
		return part, nil
	case ContentPartAction:
		if !hasField("name") || !hasField("payload") {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		var part ActionPart
		if err := json.Unmarshal(trimmed, &part); err != nil {
			return nil, fmt.Errorf("invalid ContentPart")
		}
		part.Type = ContentPartAction
		return part, nil
	default:
		return AnyPart{Raw: append(json.RawMessage(nil), trimmed...)}, nil
	}
}

func marshalContentPart(part ContentPart) ([]byte, error) {
	switch p := part.(type) {
	case TextPart:
		if p.Type == "" {
			p.Type = ContentPartText
		}
		return json.Marshal(p)
	case *TextPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartText
		}
		return json.Marshal(v)
	case ReasoningPart:
		if p.Type == "" {
			p.Type = ContentPartReasoning
		}
		return json.Marshal(p)
	case *ReasoningPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartReasoning
		}
		return json.Marshal(v)
	case FileDataPart:
		if p.Type == "" {
			p.Type = ContentPartFileData
		}
		return json.Marshal(p)
	case *FileDataPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartFileData
		}
		return json.Marshal(v)
	case InlineDataPart:
		if p.Type == "" {
			p.Type = ContentPartInlineData
		}
		return json.Marshal(p)
	case *InlineDataPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartInlineData
		}
		return json.Marshal(v)
	case ToolCallPart:
		if p.Type == "" {
			p.Type = ContentPartToolCall
		}
		return json.Marshal(p)
	case *ToolCallPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartToolCall
		}
		return json.Marshal(v)
	case ToolOutputPart:
		if p.Type == "" {
			p.Type = ContentPartToolOutput
		}
		return json.Marshal(p)
	case *ToolOutputPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartToolOutput
		}
		return json.Marshal(v)
	case ActionPart:
		if p.Type == "" {
			p.Type = ContentPartAction
		}
		return json.Marshal(p)
	case *ActionPart:
		if p == nil {
			return json.Marshal(nil)
		}
		v := *p
		if v.Type == "" {
			v.Type = ContentPartAction
		}
		return json.Marshal(v)
	case AnyPart:
		return json.Marshal(p)
	case *AnyPart:
		if p == nil {
			return json.Marshal(nil)
		}
		return json.Marshal(*p)
	default:
		return nil, fmt.Errorf("unsupported ContentPart type")
	}
}

func (c *MessageContent) UnmarshalJSON(data []byte) error {
	trimmed := bytes.TrimSpace(data)
	if len(trimmed) == 0 {
		*c = MessageContent{}
		return nil
	}

	if trimmed[0] == '"' {
		var text string
		if err := json.Unmarshal(trimmed, &text); err != nil {
			return err
		}
		*c = MessageContent{NewTextContentPart(text)}
		return nil
	}

	if trimmed[0] == '[' {
		var rawItems []json.RawMessage
		if err := json.Unmarshal(trimmed, &rawItems); err != nil {
			return err
		}
		items := make([]ContentPart, 0, len(rawItems))
		for _, raw := range rawItems {
			part, err := parseContentPart(raw)
			if err != nil {
				return err
			}
			items = append(items, part)
		}
		*c = MessageContent(items)
		return nil
	}

	return fmt.Errorf("message content must be a string or array")
}

func (c MessageContent) MarshalJSON() ([]byte, error) {
	if c == nil {
		return []byte("[]"), nil
	}
	rawItems := make([]json.RawMessage, 0, len(c))
	for _, part := range c {
		encoded, err := marshalContentPart(part)
		if err != nil {
			return nil, err
		}
		rawItems = append(rawItems, json.RawMessage(encoded))
	}
	return json.Marshal(rawItems)
}

func (c MessageContent) SizeBytes() int {
	total := 0
	allText := len(c) > 0

	for _, part := range c {
		switch p := part.(type) {
		case TextPart:
			total += len([]byte(p.Text))
			continue
		case *TextPart:
			if p != nil {
				total += len([]byte(p.Text))
				continue
			}
		case ReasoningPart:
			total += len([]byte(p.Text))
			continue
		case *ReasoningPart:
			if p != nil {
				total += len([]byte(p.Text))
				continue
			}
		}

		allText = false
		break
	}

	if allText {
		return total
	}

	b, err := json.Marshal(c)
	if err != nil {
		return 0
	}
	return len(b)
}

func (c MessageContent) Text() (string, bool) {
	texts := c.textParts()
	if len(texts) == 0 {
		return "", false
	}
	return strings.Join(texts, "\n"), true
}

func (c MessageContent) FirstText() (string, bool) {
	texts := c.textParts()
	if len(texts) == 0 {
		return "", false
	}
	return texts[0], true
}

func (c MessageContent) textParts() []string {
	texts := make([]string, 0, len(c))
	for _, part := range c {
		switch p := part.(type) {
		case TextPart:
			texts = append(texts, p.Text)
		case *TextPart:
			if p != nil {
				texts = append(texts, p.Text)
			}
		}
	}
	return texts
}

type FormationInput struct {
	Messages  []Message     `json:"messages"`
	Context   *InputContext `json:"context,omitempty"`
	Timestamp string        `json:"timestamp"`
}

type RecallInput struct {
	Query   string        `json:"query"`
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
	Scope     TokenScope `json:"scope"`
	Name      string     `json:"name"`
	ExpiresAt *int64     `json:"expires_at,omitempty"`
}

type RevokeSpaceTokenInput struct {
	Token string `json:"token"`
}

type UpdateSpaceInput struct {
	Name        *string `json:"name,omitempty"`
	Description *string `json:"description,omitempty"`
	Public      *bool   `json:"public,omitempty"`
}

type ModelConfig struct {
	Family   string `json:"family"` // "gemini", "anthropic", "openai", "deepseek", "mimo" etc.
	Model    string `json:"model"`
	APIBase  string `json:"api_base"`
	APIKey   string `json:"api_key"`
	Disabled *bool  `json:"disabled,omitempty"`
}

type RestartFormationInput struct {
	Conversation *uint64 `json:"conversation,omitempty"`
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
	Name      string     `json:"name"`
	Token     string     `json:"token"`
	Scope     TokenScope `json:"scope"`
	Usage     int        `json:"usage"`
	CreatedAt int64      `json:"created_at"`
	UpdatedAt int64      `json:"updated_at"`
	ExpiresAt *int64     `json:"expires_at,omitempty"`
}

type StorageStats map[string]any

type SpaceInfo struct {
	ID                     string        `json:"id"`
	Name                   string        `json:"name,omitempty"`
	Description            string        `json:"description,omitempty"`
	Owner                  string        `json:"owner"`
	DBStats                StorageStats  `json:"db_stats"`
	Concepts               int           `json:"concepts"`
	Propositions           int           `json:"propositions"`
	Conversations          int           `json:"conversations"`
	Public                 bool          `json:"public"`
	Tier                   SpaceTier     `json:"tier"`
	FormationUsage         Usage         `json:"formation_usage"`
	RecallUsage            Usage         `json:"recall_usage"`
	MaintenanceUsage       Usage         `json:"maintenance_usage"`
	FormationProcessedID   int64         `json:"formation_processed_id"`
	MaintenanceProcessedID int64         `json:"maintenance_processed_id"`
	MaintenanceAt          MaintenanceAt `json:"maintenance_at"`
}

type FormationStatus struct {
	ID                     string        `json:"id"`
	Concepts               int           `json:"concepts"`
	Propositions           int           `json:"propositions"`
	Conversations          int           `json:"conversations"`
	FormationProcessing    bool          `json:"formation_processing"`
	MaintenanceProcessing  bool          `json:"maintenance_processing"`
	FormationProcessedID   int64         `json:"formation_processed_id"`
	MaintenanceProcessedID int64         `json:"maintenance_processed_id"`
	MaintenanceAt          MaintenanceAt `json:"maintenance_at"`
}

type MaintenanceAt struct {
	Daydream int64 `json:"daydream"`
	Full     int64 `json:"full"`
	Quick    int64 `json:"quick"`
}

type Usage struct {
	InputTokens  int `json:"input_tokens,omitempty"`
	OutputTokens int `json:"output_tokens,omitempty"`
	Requests     int `json:"requests,omitempty"`
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
	Label            *string            `json:"label,omitempty"`
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
