package model

import (
	"sort"
	"time"

	corev1 "k8s.io/api/core/v1"
	v1networking "k8s.io/api/networking/v1"
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

func (snap *Snapshot) Sort() {
	sort.Slice(snap.Routes, func(i, j int) bool {
		ri, rj := snap.Routes[i], snap.Routes[j]
		if ri.Priority != rj.Priority {
			return ri.Priority > rj.Priority
		}
		if ri.Host != rj.Host {
			return ri.Host < rj.Host
		}

		if ri.Path != rj.Path {
			return ri.Path < rj.Path
		}

		if ri.PathType != rj.PathType {
			return pathTypeRank(*ri.PathType) > pathTypeRank(*rj.PathType)
		}
		return ri.Cluster < rj.Cluster
	})

	sort.Slice(snap.Clusters, func(i, j int) bool {
		return snap.Clusters[i].Name < snap.Clusters[j].Name
	})

	sort.Slice(snap.TLS, func(i, j int) bool {
		return snap.TLS[i].Name < snap.TLS[j].Name
	})

}

func pathTypeRank(pt v1networking.PathType) int {
	switch pt {
	case v1networking.PathTypeExact:
		return 3
	case v1networking.PathTypePrefix:
		return 2
	case v1networking.PathTypeImplementationSpecific:
		return 1
	default:
		return 0
	}
}

func RoutePriority(path string, pt v1networking.PathType) int64 {
	const K = int64(1_000_000)
	return int64(pathTypeRank(pt))*K + int64(len(path))
}

// Route
type Route struct {
	Host     string                 `json:"host"`               // demo.local
	Path     string                 `json:"path"`               // "/api"
	PathType *v1networking.PathType `json:"pathType"`           // Prefix|Exact|Regex (Prefix/Exact)
	Cluster  string                 `json:"cluster"`            // name cluster
	Priority int                    `json:"priority,omitempty"` // for sort matches
}

// Cluster — logical backend (host+path or name service)
type Cluster struct {
	Name      string     `json:"name"`
	LBPolicy  LBPolicy   `json:"lbPolicy,omitempty"` // RR
	Endpoints []Endpoint `json:"endpoints"`
	// retries/timeouts/health-check
	TimeoutMs       int32            `json:"timeoutMs,omitempty"`
	Retries         int32            `json:"retries,omitempty"`
	BackendProtocol string           `json:"backendProtocol,omitempty"`
	LBAlgorithm     LBPolicy         `json:"lbAlgotihm,omitempty"`
	RewriteHeaders  []RewriteHeaders `json:"rwHeaders"`
}

type LBPolicy string

type RewriteHeaderMode string

const (
	LBRoundRobin   LBPolicy          = "RoundRobin"
	LBRandom       LBPolicy          = "Random"
	LBLeastConn    LBPolicy          = "LeastConn"
	RWHeaderAppend RewriteHeaderMode = "Append"
	RWHeaderSet    RewriteHeaderMode = "Set"
	RWHeaderRemove RewriteHeaderMode = "Remove"
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
	NotAfterUnix time.Time
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
	SNI  TLSSecret
}

type RewriteHeaders struct {
	Name  string            `json:"name"`
	Mode  RewriteHeaderMode `json:"mode"`
	Value string            `json:"value,omitempty"`
}

type TargetEndpoint struct {
	Port            int32
	Protocol        corev1.Protocol
	BackendProtocol string
	Addresses       []string
	PathType        *v1networking.PathType
	Retries         int32
	TimeoutMs       int32
	LBAlgorithm     LBPolicy
	RewriteHeaders  []RewriteHeaders
}
