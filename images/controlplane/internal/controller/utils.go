package controller

import (
	"context"
	"fmt"
	"slices"
	"strings"

	. "argon/internal/model"

	corev1 "k8s.io/api/core/v1"
	discoveryv1 "k8s.io/api/discovery/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/util/yaml"
	"sigs.k8s.io/controller-runtime/pkg/client"
)

func portProtocol(slice discoveryv1.EndpointSlice, matched *int32) corev1.Protocol {
	for _, sp := range slice.Ports {
		if sp.Port == nil || matched == nil || *sp.Port != *matched {
			continue
		}

		if sp.Protocol != nil {
			return *sp.Protocol
		}
	}

	return corev1.ProtocolTCP
}

func appendUnique(dst []string, src ...string) []string {
	for _, addr := range src {
		if !slices.Contains(dst, addr) {
			dst = append(dst, addr)
		}
	}

	return dst
}

func resolveServicePortName(ctx context.Context, c client.Client, ns, svcName string, be *networkingv1.IngressBackend) (string, error) {
	if be == nil || be.Service == nil {
		return "", nil
	}
	var svc corev1.Service
	if err := c.Get(ctx, client.ObjectKey{Namespace: ns, Name: svcName}, &svc); err != nil {
		return "", err
	}

	if be.Service.Port.Name != "" {
		return be.Service.Port.Name, nil
	}

	if num := be.Service.Port.Number; num != 0 {
		for _, p := range svc.Spec.Ports {
			if p.Port == num {
				return p.Name, nil
			}
		}
	}
	return "", nil
}

func matchSlicePortByName(slice discoveryv1.EndpointSlice, portName string) *int32 {
	for _, sp := range slice.Ports {
		if sp.Port == nil {
			continue
		}
		if portName == "" {
			return sp.Port
		}
		if sp.Name != nil && *sp.Name == portName {
			return sp.Port
		}
	}
	return nil
}

func getRequestHeaders(reqHeadersRaw string) ([]RewriteHeaders, error) {
	var reqheaders []RewriteHeaders
	if err := yaml.Unmarshal([]byte(reqHeadersRaw), &reqheaders); err != nil {
		return nil, fmt.Errorf("failed to parse %s: %w", REQUEST_HEADERS_ANNOTATION, err)
	}

	for i := range reqheaders {
		mode := strings.TrimSpace(string(reqheaders[i].Mode))
		switch {
		case strings.EqualFold(mode, string(RWHeaderSet)):
			reqheaders[i].Mode = RWHeaderSet
		case strings.EqualFold(mode, string(RWHeaderAppend)):
			reqheaders[i].Mode = RWHeaderAppend
		case strings.EqualFold(mode, string(RWHeaderRemove)):
			reqheaders[i].Mode = RWHeaderRemove
		default:
			return nil, fmt.Errorf("unsupported rewrite header mode %q", mode)
		}
	}

	return reqheaders, nil
}
