package sync

import (
	"bytes"
	"encoding/json"
	"net/http"
	"time"

	"silex-operator/pkg/types"
)

var httpClient = &http.Client{
	Timeout: 2 * time.Second,
	Transport: &http.Transport{
		MaxIdleConns:        100,
		MaxIdleConnsPerHost: 100,
		IdleConnTimeout:     90 * time.Second,
	},
}

func SendRouteUpdate(payload types.RoutePayload) error {
	data, err := json.Marshal(payload)
	if err != nil {
		return err
	}

	req, err := http.NewRequest("POST", "http://127.0.0.1:9090", bytes.NewBuffer(data))
	if err != nil {
		return err
	}

	resp, err := httpClient.Do(req)
	if err == nil {
		resp.Body.Close()
	}
	return err
}
