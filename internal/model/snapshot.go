package model

import (
	"bytes"
	"encoding/hex"
	"github.com/golang/protobuf/ptypes/timestamp"
	corev1 "k8s.io/api/core/v1"
)

type Snapshot struct {
	// Meta
	Version            string            `json:"version"`            // hash
	ControllerID       string            `json:"controllerID"`       // argon.github.io/ingress
	IngressClassName   string            `json:"ingressClassName"`   // my-ingress-class
	GeneratedAtUnixSec int64             `json:"generatedAtUnixSec"` // timestamp
	ResourceVersions   map[string]string `json:"resourceVersions,omitempty"`
	// sample: key="ing:<ns>/<name>" -> value=ing.ResourceVersion
	//           key="svc:<ns>/<name>", "es:<ns>/<name>" etc

	// Routing
	Routes   []Route   `json:"routes"`   // host+path -> cluster
	Clusters []Cluster `json:"clusters"` // name cluster -> endpoints (ip:port)

	// TLS/GeoIP/Policies
	TLS      []TLSSecret   `json:"tls,omitempty"`
	GeoIP    *GeoIPRuntime `json:"geoip,omitempty"`
	Policies *Policies     `json:"policies,omitempty"`
}

// Route
type Route struct {
	Host     string   `json:"host"`               // demo.local
	Path     string   `json:"path"`               // "/api"
	PathType PathType `json:"pathType"`           // Prefix|Exact|Regex (на старте Prefix/Exact)
	Cluster  string   `json:"cluster"`            // name cluster
	Priority int      `json:"priority,omitempty"` // for sort matches
}

// PathType
type PathType string

const (
	PathPrefix PathType = "Prefix"
	PathExact  PathType = "Exact"
	PathRegex  PathType = "Regex" //
)

// Cluster — logical backend (host+path or name service)
type Cluster struct {
	Name      string     `json:"name"`
	LBPolicy  LBPolicy   `json:"lbPolicy,omitempty"` // RR
	Endpoints []Endpoint `json:"endpoints"`
	// retries/timeouts/health-check
	TimeoutMs int32 `json:"timeoutMs,omitempty"`
	Retries   int32 `json:"retries,omitempty"`
}

type LBPolicy string

const (
	LBRoundRobin LBPolicy = "RoundRobin"
	LBRandom     LBPolicy = "Random"
	LBLeastConn  LBPolicy = "LeastConnections"
)

// Endpoint — реальный upstream адрес
type Endpoint struct {
	Address string `json:"address"` // IP or hostname
	Port    int32  `json:"port"`
	Weight  int32  `json:"weight,omitempty"` // default 1
	// zone/region  hint for geo/priority LB
	Zone   string `json:"zone,omitempty"`
	Region string `json:"region,omitempty"`
}

type TLSSecret struct {
	Name         string
	Sni          []string
	CertPem      []byte
	KeyPem       []byte
	NotAfterUnix timestamp.Timestamp
	Version      string
}

type GeoIPRuntime struct {
	Enabled         bool   `json:"enabled"`
	Header          string `json:"header,omitempty"`          // X-Geo-Country
	FallbackCountry string `json:"fallbackCountry,omitempty"` // ZZ
}

type Policies struct {
	CountryRouting []CountryRoute `json:"countryRouting,omitempty"`
}

type CountryRoute struct {
	Country string `json:"country"` // "US"
	Cluster string `json:"cluster"`
}

type TargetProxy struct {
	Host string
	Path map[string]TargetEndpoint
}

type TargetEndpoint struct {
	Port      int32
	Protocol  corev1.Protocol
	Addresses []string
}
