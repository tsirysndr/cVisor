// Package cvisor is a Go SDK for the cVisor sandbox runtime.
//
// It has two layers:
//
//   - Client / RemoteSandbox: a pure-Go client for the cVisor daemon's GraphQL
//     HTTP API. It uses only the standard library and works on any OS (macOS
//     included) — this is the primary, portable API.
//   - Sandbox: an optional in-process native runtime bound to libcvisor via
//     cgo. It is Linux-only (build tag "linux && cgo"); on other platforms the
//     native constructor returns a clear "Linux-only" error.
package cvisor

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
)

// Client is a thin client for the cVisor daemon's GraphQL endpoint.
type Client struct {
	url   string
	token string
	http  *http.Client
}

// Option configures a Client.
type Option func(*Client)

// WithHTTPClient overrides the underlying *http.Client (e.g. to set timeouts).
func WithHTTPClient(h *http.Client) Option {
	return func(c *Client) { c.http = h }
}

// NewClient builds a Client for the daemon at url (e.g.
// "http://127.0.0.1:8080/graphql") authenticating with the bearer token.
func NewClient(url, token string, opts ...Option) *Client {
	c := &Client{url: url, token: token, http: http.DefaultClient}
	for _, o := range opts {
		o(c)
	}
	return c
}

// GraphQLError is returned when the daemon replies with a non-empty errors array.
type GraphQLError struct {
	Messages []string
}

func (e *GraphQLError) Error() string {
	if len(e.Messages) == 0 {
		return "graphql error"
	}
	out := e.Messages[0]
	for _, m := range e.Messages[1:] {
		out += "; " + m
	}
	return out
}

type gqlRequest struct {
	Query     string         `json:"query"`
	Variables map[string]any `json:"variables"`
}

type gqlError struct {
	Message string `json:"message"`
}

type gqlResponse struct {
	Data   json.RawMessage `json:"data"`
	Errors []gqlError      `json:"errors"`
}

// Query runs a GraphQL query document and returns the raw `data` object.
func (c *Client) Query(ctx context.Context, doc string, vars map[string]any) (json.RawMessage, error) {
	return c.execute(ctx, doc, vars)
}

// Mutate runs a GraphQL mutation document and returns the raw `data` object.
func (c *Client) Mutate(ctx context.Context, doc string, vars map[string]any) (json.RawMessage, error) {
	return c.execute(ctx, doc, vars)
}

func (c *Client) execute(ctx context.Context, doc string, vars map[string]any) (json.RawMessage, error) {
	if vars == nil {
		vars = map[string]any{}
	}
	body, err := json.Marshal(gqlRequest{Query: doc, Variables: vars})
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.url, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.token)

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	var parsed gqlResponse
	if err := json.NewDecoder(resp.Body).Decode(&parsed); err != nil {
		if resp.StatusCode < 200 || resp.StatusCode >= 300 {
			return nil, fmt.Errorf("cvisor: HTTP %d from daemon", resp.StatusCode)
		}
		return nil, fmt.Errorf("cvisor: decode response: %w", err)
	}
	if len(parsed.Errors) > 0 {
		msgs := make([]string, len(parsed.Errors))
		for i, e := range parsed.Errors {
			msgs[i] = e.Message
		}
		return nil, &GraphQLError{Messages: msgs}
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("cvisor: HTTP %d from daemon", resp.StatusCode)
	}
	return parsed.Data, nil
}
