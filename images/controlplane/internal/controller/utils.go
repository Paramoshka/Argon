package controller

import (
	"context"
	"crypto/sha256"
	"crypto/x509"
	"encoding/hex"
	"encoding/pem"
	"fmt"
	"slices"
	"sort"
	"strconv"
	"strings"

	. "argon/internal/model"

	corev1 "k8s.io/api/core/v1"
	discoveryv1 "k8s.io/api/discovery/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/util/yaml"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/log"
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

func parseAnnotations(annotations map[string]string) *TargetEndpoint {

	te := &TargetEndpoint{}

	backendProtocol := "h1"
	if _, exists := annotations[BACKEND_PROTOCOL_ANNOTATION]; exists {
		backendProtocol = annotations[BACKEND_PROTOCOL_ANNOTATION]
	}
	te.BackendProtocol = backendProtocol

	backendTimeout := 3000
	if _, exists := annotations[BACKEND_TIMEOUT_ANNOTATION]; exists {
		backendTimeout, _ = strconv.Atoi(annotations[BACKEND_TIMEOUT_ANNOTATION])
	}
	te.TimeoutMs = int32(backendTimeout)

	var reqheaders []RewriteHeaders
	if rawReqHeaders, ok := annotations[REQUEST_HEADERS_ANNOTATION]; ok {
		if parsed, err := getRequestHeaders(rawReqHeaders); err == nil {
			reqheaders = parsed
		}
	}
	te.RewriteHeaders = reqheaders

	lbAlgorithm := LBRoundRobin
	if alg, exists := annotations[BACKEND_LB_ALGORITHM_ANNOTATION]; exists {
		switch LBPolicy(alg) {
		case LBLeastConn:
			lbAlgorithm = LBLeastConn
		case LBRoundRobin:
			lbAlgorithm = LBRoundRobin
		default:
			lbAlgorithm = LBRoundRobin
		}
	}
	te.LBAlgorithm = lbAlgorithm

	// Auth annotations
	var auth *AuthConfig
	if rawURL, ok := annotations[AUTH_URL_ANNOTATION]; ok && strings.TrimSpace(rawURL) != "" {
		if auth == nil {
			auth = &AuthConfig{}
		}
		auth.URL = strings.TrimSpace(rawURL)
	}
	if rawSignin, ok := annotations[AUTH_SIGNIN_ANNOTATION]; ok && strings.TrimSpace(rawSignin) != "" {
		if auth == nil {
			auth = &AuthConfig{}
		}
		auth.Signin = strings.TrimSpace(rawSignin)
	}
	if rawResp, ok := annotations[AUTH_RESPONSE_HEADERS_ANNOTATION]; ok && strings.TrimSpace(rawResp) != "" {
		if auth == nil {
			auth = &AuthConfig{}
		}
		auth.ResponseHeaders = parseCSVList(rawResp)
	}
	if rawSkip, ok := annotations[AUTH_SKIP_PATHS_ANNOTATION]; ok && strings.TrimSpace(rawSkip) != "" {
		if auth == nil {
			auth = &AuthConfig{}
		}
		auth.SkipPaths = parseCSVList(rawSkip)
	}
	if rawCookie, ok := annotations[AUTH_COOKIE_NAME_ANNOTATION]; ok && strings.TrimSpace(rawCookie) != "" {
		if auth == nil {
			auth = &AuthConfig{}
		}
		auth.CookieName = strings.TrimSpace(rawCookie)
	}
	te.Auth = auth

	return te
}

// buildTargetFromRule converts a single Ingress rule into a TargetProxy using EndpointSlices.
// It discovers backend endpoints and fills TargetEndpoint fields from base template.
func buildTargetFromRule(
	ctx context.Context,
	c client.Client,
	namespace string,
	rule networkingv1.IngressRule,
	base TargetEndpoint,
	bundle TLSSecret,
) TargetProxy {
	target := TargetProxy{
		Host: rule.Host,
		Path: make(map[string]TargetEndpoint),
		SNI:  bundle,
	}

	if rule.HTTP == nil {
		return target
	}

	for _, p := range rule.HTTP.Paths {
		be := p.Backend
		if be.Service == nil {
			continue
		}

		svcName := be.Service.Name

		var slicesList discoveryv1.EndpointSliceList
		if err := c.List(ctx, &slicesList,
			client.InNamespace(namespace),
			client.MatchingLabels{"kubernetes.io/service-name": svcName},
		); err != nil {
			continue
		}

		var allAddrs []string
		var chosenPort *int32
		var proto = corev1.ProtocolTCP
		portName, _ := resolveServicePortName(ctx, c, namespace, svcName, &be)

		for _, slice := range slicesList.Items {
			matched := matchSlicePortByName(slice, portName)
			if matched == nil {
				continue
			}
			if chosenPort == nil {
				chosenPort = matched
				proto = portProtocol(slice, matched)
			}
			if *matched != *chosenPort {
				continue
			}

			for _, ep := range slice.Endpoints {
				if ep.Conditions.Ready != nil && !*ep.Conditions.Ready {
					continue
				}
				allAddrs = appendUnique(allAddrs, ep.Addresses...)
			}
		}

		if chosenPort == nil || len(allAddrs) == 0 {
			continue
		}

		sort.Strings(allAddrs)

		target.Path[p.Path] = TargetEndpoint{
			Port:            *chosenPort,
			Protocol:        proto,
			Addresses:       allAddrs,
			PathType:        p.PathType,
			BackendProtocol: base.BackendProtocol,
			Retries:         base.Retries,
			TimeoutMs:       base.TimeoutMs,
			LBAlgorithm:     base.LBAlgorithm,
			RewriteHeaders:  base.RewriteHeaders,
			Auth:            base.Auth,
		}
	}

	return target
}

// parseCSVList splits a comma-separated list, trims whitespace, and removes empties/duplicates.
func parseCSVList(s string) []string {
	parts := strings.Split(s, ",")
	out := make([]string, 0, len(parts))
	seen := map[string]struct{}{}
	for _, p := range parts {
		v := strings.TrimSpace(p)
		if v == "" {
			continue
		}
		if _, ok := seen[v]; ok {
			continue
		}
		seen[v] = struct{}{}
		out = append(out, v)
	}
	return out
}

// buildTLSBundle extracts TLS Secret bundle for an Ingress.
// It iterates over ing.Spec.TLS and returns the last successfully parsed bundle.
// If nothing valid is found, returns a zero-value TLSSecret.
func (r *ArgonConfigReconciler) buildTLSBundle(ctx context.Context, ing networkingv1.Ingress) TLSSecret {
	logger := log.FromContext(ctx) // ctrl-runtime logger

	var bundle TLSSecret

	for _, tls := range ing.Spec.TLS {
		if tls.SecretName == "" || len(tls.Hosts) == 0 {
			continue
		}

		var secret corev1.Secret
		if err := r.Get(ctx, client.ObjectKey{Name: tls.SecretName, Namespace: ing.Namespace}, &secret); err != nil {
			logger.Error(err, "get TLS secret failed", "ns", ing.Namespace, "secret", tls.SecretName)
			continue
		}

		crt := secret.Data["tls.crt"]
		key := secret.Data["tls.key"]

		if len(crt) == 0 || len(key) == 0 {
			logger.Info("TLS secret missing tls.crt or tls.key", "ns", ing.Namespace, "secret", tls.SecretName)
			continue
		}

		sum := sha256.Sum256(append(crt, key...))

		block, _ := pem.Decode(crt)
		if block == nil {
			logger.Info("failed to PEM-decode tls.crt", "ns", ing.Namespace, "secret", tls.SecretName)
			continue
		}

		certs, err := x509.ParseCertificates(block.Bytes)
		if err != nil || len(certs) == 0 {
			logger.Error(err, "parse certificate failed", "ns", ing.Namespace, "secret", tls.SecretName)
			continue
		}
		notAfter := certs[0].NotAfter

		bundle = TLSSecret{
			Name:         fmt.Sprintf("%s/%s", ing.Namespace, tls.SecretName),
			Sni:          tls.Hosts,
			CertPem:      crt,
			KeyPem:       key,
			NotAfterUnix: notAfter,
			Version:      hex.EncodeToString(sum[:]),
		}

		logger.V(1).Info("TLS bundle prepared", "secret", bundle.Name, "hosts", bundle.Sni, "notAfter", notAfter)
	}

	return bundle
}
