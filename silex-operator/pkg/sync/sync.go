package sync

import (
	"bytes"
	"encoding/json"
	"net/http"
	"time"

	"silex-operator/pkg/types"
)

func SendRouteUpdate(payload types.RoutePayload) error {
	data, _ := json.Marshal(payload)
	req, _ := http.NewRequest("POST", "http://127.0.0.1:9090/route", bytes.NewBuffer(data))
	client := &http.Client{Timeout: 2 * time.Second}
	resp, err := client.Do(req)
	if err == nil {
		resp.Body.Close()
	}
	return err
}

func SendTLSUpdate(payload types.TLSPayload) error {
	data, _ := json.Marshal(payload)
	req, _ := http.NewRequest("POST", "http://127.0.0.1:9090/tls", bytes.NewBuffer(data))
	client := &http.Client{Timeout: 2 * time.Second}
	resp, err := client.Do(req)
	if err == nil {
		resp.Body.Close()
	}
	return err
}
