package api

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const (
	DefaultBaseURL = "https://brain.anda.ai"
	DefaultTimeout = 120 * time.Second
)

// Client is the HTTP client for the Anda Hippocampus API.
type Client struct {
	BaseURL    string
	SpaceID    string
	Token      string
	HTTPClient *http.Client
}

// NewClient creates a new API client.
func NewClient(baseURL, spaceID, token string) *Client {
	baseURL = strings.TrimRight(baseURL, "/")
	return &Client{
		BaseURL: baseURL,
		SpaceID: spaceID,
		Token:   token,
		HTTPClient: &http.Client{
			Timeout: DefaultTimeout,
		},
	}
}

func (c *Client) spacePath(path string) string {
	return fmt.Sprintf("/v1/%s%s", url.PathEscape(c.SpaceID), path)
}

func (c *Client) doJSON(ctx context.Context, method, path string, body any) ([]byte, error) {
	var reqBody io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, fmt.Errorf("marshal request: %w", err)
		}
		reqBody = bytes.NewReader(data)
	}

	reqURL := c.BaseURL + path
	req, err := http.NewRequestWithContext(ctx, method, reqURL, reqBody)
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}

	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	req.Header.Set("Accept", "application/json")
	if c.Token != "" {
		req.Header.Set("Authorization", "Bearer "+c.Token)
	}

	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("http request: %w", err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}

	if resp.StatusCode >= 400 {
		var rpcErr RpcError
		if json.Unmarshal(respBody, &rpcErr) == nil && rpcErr.Message != "" {
			return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, rpcErr.Message)
		}
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(respBody))
	}

	return respBody, nil
}

// GetInfo returns service information.
func (c *Client) GetInfo(ctx context.Context) (*ServiceInfo, error) {
	data, err := c.doJSON(ctx, http.MethodGet, "/info", nil)
	if err != nil {
		return nil, err
	}
	var info ServiceInfo
	if err := json.Unmarshal(data, &info); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &info, nil
}

// Formation submits a memory formation task.
func (c *Client) Formation(ctx context.Context, input *FormationInput) (*RpcResponse[AgentOutput], error) {
	data, err := c.doJSON(ctx, http.MethodPost, c.spacePath("/formation"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[AgentOutput]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// Recall queries memory with natural language.
func (c *Client) Recall(ctx context.Context, input *RecallInput) (*RpcResponse[AgentOutput], error) {
	data, err := c.doJSON(ctx, http.MethodPost, c.spacePath("/recall"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[AgentOutput]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// Maintenance triggers maintenance task.
func (c *Client) Maintenance(ctx context.Context, input *MaintenanceInput) (*RpcResponse[AgentOutput], error) {
	data, err := c.doJSON(ctx, http.MethodPost, c.spacePath("/maintenance"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[AgentOutput]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// GetStatus returns space status.
func (c *Client) GetStatus(ctx context.Context) (*RpcResponse[SpaceInfo], error) {
	data, err := c.doJSON(ctx, http.MethodGet, c.spacePath("/info"), nil)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[SpaceInfo]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// GetConversation returns a single conversation.
func (c *Client) GetConversation(ctx context.Context, conversationID int, collection string) (*RpcResponse[Conversation], error) {
	path := fmt.Sprintf("%s/conversations/%d", c.spacePath(""), conversationID)
	params := url.Values{}
	if collection != "" {
		params.Set("collection", collection)
	}
	if len(params) > 0 {
		path += "?" + params.Encode()
	}
	data, err := c.doJSON(ctx, http.MethodGet, path, nil)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[Conversation]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// ListConversations lists conversations with pagination.
func (c *Client) ListConversations(ctx context.Context, cursor string, limit int, collection string) (*RpcResponse[[]Conversation], error) {
	path := c.spacePath("/conversations")
	params := url.Values{}
	if cursor != "" {
		params.Set("cursor", cursor)
	}
	if limit > 0 {
		params.Set("limit", fmt.Sprintf("%d", limit))
	}
	if collection != "" {
		params.Set("collection", collection)
	}
	if len(params) > 0 {
		path += "?" + params.Encode()
	}

	data, err := c.doJSON(ctx, http.MethodGet, path, nil)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[[]Conversation]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// ListSpaceTokens lists space tokens (management).
func (c *Client) ListSpaceTokens(ctx context.Context) (*RpcResponse[[]SpaceToken], error) {
	data, err := c.doJSON(ctx, http.MethodGet, c.spacePath("/management/space_tokens"), nil)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[[]SpaceToken]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// AddSpaceToken adds a space token (management).
func (c *Client) AddSpaceToken(ctx context.Context, input *AddSpaceTokenInput) (*RpcResponse[SpaceToken], error) {
	data, err := c.doJSON(ctx, http.MethodPost, c.spacePath("/management/add_space_token"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[SpaceToken]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// RevokeSpaceToken revokes a space token (management).
func (c *Client) RevokeSpaceToken(ctx context.Context, token string) (*RpcResponse[bool], error) {
	input := RevokeSpaceTokenInput{Token: token}
	data, err := c.doJSON(ctx, http.MethodPost, c.spacePath("/management/revoke_space_token"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[bool]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// UpdateSpace updates space information (management).
func (c *Client) UpdateSpace(ctx context.Context, input *UpdateSpaceInput) (*RpcResponse[bool], error) {
	data, err := c.doJSON(ctx, http.MethodPatch, c.spacePath("/management/update_space"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[bool]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

func (c *Client) RestartFormation(ctx context.Context, input *RestartFormationInput) (*RpcResponse[bool], error) {
	data, err := c.doJSON(ctx, http.MethodPatch, c.spacePath("/management/restart_formation"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[bool]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

func (c *Client) GetBYOK(ctx context.Context) (*RpcResponse[ModelConfig], error) {
	data, err := c.doJSON(ctx, http.MethodGet, c.spacePath("/management/space_byok"), nil)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[ModelConfig]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

func (c *Client) UpdateBYOK(ctx context.Context, input *ModelConfig) (*RpcResponse[bool], error) {
	data, err := c.doJSON(ctx, http.MethodPatch, c.spacePath("/management/space_byok"), input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[bool]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// CreateSpace creates a space (admin).
func (c *Client) CreateSpace(ctx context.Context, input *CreateOrUpdateSpaceInput) (*RpcResponse[SpaceInfo], error) {
	data, err := c.doJSON(ctx, http.MethodPost, "/admin/create_space", input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[SpaceInfo]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}

// UpdateSpaceTier updates space tier (admin).
func (c *Client) UpdateSpaceTier(ctx context.Context, spaceID string, input *CreateOrUpdateSpaceInput) (*RpcResponse[SpaceTier], error) {
	path := fmt.Sprintf("/admin/%s/update_space_tier", url.PathEscape(spaceID))
	data, err := c.doJSON(ctx, http.MethodPost, path, input)
	if err != nil {
		return nil, err
	}
	var resp RpcResponse[SpaceTier]
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("decode response: %w", err)
	}
	return &resp, nil
}
