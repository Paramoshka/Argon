package model

import (
	"net/netip"

	gwapiv1 "sigs.k8s.io/gateway-api/apis/v1"
)

type GatewaySnapshot struct {
	Name      string
	Namespace string
	Addresses []netip.Addr
	Listeners []ListenerSnapshot
	TLS       []TLSSecret
}

type ListenerSnapshot struct {
	Name          string
	Port          int32
	Protocol      gwapiv1.ProtocolType
	Hostname      *gwapiv1.Hostname
	TLSMode       *gwapiv1.TLSModeType
	Certificates  []gwapiv1.SecretObjectReference
	AllowedRoutes gwapiv1.AllowedRoutes
}
