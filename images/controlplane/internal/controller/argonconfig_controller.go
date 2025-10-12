/*
Copyright 2025.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

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
	"time"

	. "argon/internal/grpc"
	. "argon/internal/model"

	corev1 "k8s.io/api/core/v1"
	discoveryv1 "k8s.io/api/discovery/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/runtime"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/log"
)

// ArgonConfigReconciler reconciles a ArgonConfig object
type ArgonConfigReconciler struct {
	client.Client
	Scheme          *runtime.Scheme
	IngressClass    string
	lastVersion     string
	currentSnapshot Snapshot
	StreamHub       *StreamHub
}

const ingressClassIndexKey = "spec.ingressClassName"

func (r *ArgonConfigReconciler) Reconcile(ctx context.Context, _ ctrl.Request) (ctrl.Result, error) {

	var ingList networkingv1.IngressList
	if err := r.List(ctx, &ingList, client.MatchingFields{
		ingressClassIndexKey: r.IngressClass,
	}); err != nil {
		return ctrl.Result{}, err
	}

	targets, err := r.parseEndpoints(ctx, &ingList)
	if err != nil {
		return ctrl.Result{}, err
	}

	snap := r.ToSnapshot(targets)
	if snap.Version == r.lastVersion {
		return ctrl.Result{}, nil
	}

	r.lastVersion = snap.Version
	r.currentSnapshot = snap
	r.StreamHub.Broadcast(snap)

	return ctrl.Result{}, nil
}

// SetupWithManager sets up the controller with the Manager.
func (r *ArgonConfigReconciler) SetupWithManager(mgr ctrl.Manager) error {
	// index spec.ingressClassName
	if err := mgr.GetFieldIndexer().IndexField(
		context.Background(),
		&networkingv1.Ingress{},
		"spec.ingressClassName",
		func(raw client.Object) []string {
			ing := raw.(*networkingv1.Ingress)
			if ing.Spec.IngressClassName == nil {
				return []string{}
			}
			return []string{*ing.Spec.IngressClassName}
		},
	); err != nil {
		return err
	}

	return ctrl.NewControllerManagedBy(mgr).
		For(&networkingv1.Ingress{}).
		Named("ingress-controller").
		Complete(r)
}

func (r *ArgonConfigReconciler) parseEndpoints(ctx context.Context, ingList *networkingv1.IngressList) ([]TargetProxy, error) {
	var targetProxies []TargetProxy
	logger := log.FromContext(ctx) // ctrl-runtime logger

	for _, ing := range ingList.Items {
		logger.V(1).Info("processing ingress", "ns", ing.Namespace, "name", ing.Name)
		annotations := ing.GetAnnotations()

		backendProtocol := "h1"
		if _, exists := annotations[BACKEND_PROTOCOL_ANNOTATION]; exists {
			backendProtocol = annotations[BACKEND_PROTOCOL_ANNOTATION]
		}

		backendTimeout := 3000
		if _, exists := annotations[BACKEND_TIMEOUT_ANNOTATION]; exists {
			backendTimeout, _ = strconv.Atoi(annotations[BACKEND_TIMEOUT_ANNOTATION])
		}

		lbAlgorithm := LBRoundRobin
		if alg, exists := annotations[BACKEND_LB_ALGORITHM_ANNOTATION]; exists {
			switch LBPolicy(alg) {
			case LBLeastConn:
				lbAlgorithm = LBLeastConn
			case LBRoundRobin:
				lbAlgorithm = LBRoundRobin
			default:
				logger.Info("unknown lb algorithm annotation, falling back to RoundRobin", "value", alg)
				lbAlgorithm = LBRoundRobin
			}
		}

		backendRetries := 1 // todo make retries for backend

		// tls
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

		for _, rule := range ing.Spec.Rules {
			if rule.HTTP == nil {
				continue
			}

			target := TargetProxy{
				Host: rule.Host,
				Path: make(map[string]TargetEndpoint),
				SNI:  bundle,
			}

			for _, p := range rule.HTTP.Paths {
				be := p.Backend
				if be.Service == nil {
					continue
				}

				svcName := be.Service.Name

				var slices discoveryv1.EndpointSliceList
				if err := r.List(ctx, &slices,
					client.InNamespace(ing.Namespace),
					client.MatchingLabels{"kubernetes.io/service-name": svcName},
				); err != nil {
					continue
				}

				var allAddrs []string
				var chosenPort *int32
				var proto = corev1.ProtocolTCP
				portName, _ := resolveServicePortName(ctx, r.Client, ing.Namespace, svcName, &be)

				for _, slice := range slices.Items {

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
					BackendProtocol: backendProtocol,
					Retries:         int32(backendRetries),
					TimeoutMs:       int32(backendTimeout),
					LBAlgorithm:     lbAlgorithm,
				}
			}

			if len(target.Path) > 0 {
				targetProxies = append(targetProxies, target)
			}
		}
	}

	logger.Info("endpoints parsed", "targets", len(targetProxies))
	return targetProxies, nil
}

func (r *ArgonConfigReconciler) ToSnapshot(targets []TargetProxy) Snapshot {

	snap := Snapshot{
		ControllerID:       "argon.github.io/ingress",
		IngressClassName:   r.IngressClass,
		GeneratedAtUnixSec: time.Now().Unix(),
		ResourceVersions:   make(map[string]string),
		TLS:                make([]TLSSecret, 0),
	}

	for _, tp := range targets {
		if tp.SNI.Name != "" {
			snap.TLS = append(snap.TLS, tp.SNI)
		}

		for path, te := range tp.Path {
			clusterName := fmt.Sprintf("%s|%s", tp.Host, path)

			snap.Routes = append(snap.Routes, Route{
				Host:     tp.Host,
				Path:     path,
				PathType: te.PathType,
				Cluster:  clusterName,
				Priority: int(RoutePriority(path, *te.PathType)),
			})

			cluster := Cluster{
				Name:            clusterName,
				LBPolicy:        te.LBAlgorithm,
				Endpoints:       make([]Endpoint, 0, len(te.Addresses)),
				TimeoutMs:       te.TimeoutMs,
				Retries:         te.Retries,
				BackendProtocol: te.BackendProtocol,
			}
			for _, a := range te.Addresses {
				cluster.Endpoints = append(cluster.Endpoints, Endpoint{
					Address: a,
					Port:    te.Port,
					Weight:  1,
				})
			}

			snap.Clusters = append(snap.Clusters, cluster)
		}
	}

	snap.Sort() // determinism

	sum := sha256.Sum256([]byte(
		fmt.Sprintf("%+v%+v+%v", snap.Routes, snap.Clusters, snap.TLS),
	))

	snap.Version = hex.EncodeToString(sum[:])

	return snap
}

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
