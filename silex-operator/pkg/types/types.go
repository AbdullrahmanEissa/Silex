package types

type RoutePayload struct {
	Host string `json:"host"`
	IP   string `json:"ip"`
}

type TLSPayload struct {
	Host string `json:"host"`
	Cert string `json:"cert"`
	Key  string `json:"key"`
}
