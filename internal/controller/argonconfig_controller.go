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
	"encoding/hex"
	"fmt"
	"slices"
	"sort"
	"time"

	. "argon.github.io/ingress/internal/grpc"
	. "argon.github.io/ingress/internal/model"
	corev1 "k8s.io/api/core/v1"
	discoveryv1 "k8s.io/api/discovery/v1"
	networkingv1 "k8s.io/api/networking/v1"
	"k8s.io/apimachinery/pkg/runtime"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
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

func (r *ArgonConfigReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {

	var ing networkingv1.Ingress
	if err := r.Get(ctx, req.NamespacedName, &ing); err != nil {
		return ctrl.Result{}, client.IgnoreNotFound(err)
	}

	if ing.Spec.IngressClassName != nil && *ing.Spec.IngressClassName != r.IngressClass {
		return ctrl.Result{}, nil
	}

	targets, err := r.parseEndpoints(ctx, &ing)
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

func (r *ArgonConfigReconciler) parseEndpoints(ctx context.Context, ing *networkingv1.Ingress) ([]TargetProxy, error) {
	var targetProxies []TargetProxy

	for _, rule := range ing.Spec.Rules {
		if rule.HTTP == nil {
			continue
		}

		target := TargetProxy{
			Host: rule.Host,
			Path: make(map[string]TargetEndpoint),
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
			var proto corev1.Protocol = corev1.ProtocolTCP

			for _, slice := range slices.Items {

				matched := matchSlicePort(slice, &be)
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
				Port:      *chosenPort,
				Protocol:  proto,
				Addresses: allAddrs,
			}
		}

		if len(target.Path) > 0 {
			targetProxies = append(targetProxies, target)
		}
	}

	return targetProxies, nil
}

func (r *ArgonConfigReconciler) ToSnapshot(targets []TargetProxy) Snapshot {
	snap := Snapshot{
		ControllerID:       "argon.github.io/ingress",
		IngressClassName:   r.IngressClass,
		GeneratedAtUnixSec: time.Now().Unix(),
		ResourceVersions:   make(map[string]string),
	}

	for _, tp := range targets {
		for path, te := range tp.Path {
			clusterName := fmt.Sprintf("%s|%s", tp.Host, path)

			snap.Routes = append(snap.Routes, Route{
				Host:     tp.Host,
				Path:     path,
				PathType: PathPrefix,
				Cluster:  clusterName,
				Priority: len(path),
			})

			cluster := Cluster{
				Name:      clusterName,
				LBPolicy:  LBRoundRobin,
				Endpoints: make([]Endpoint, 0, len(te.Addresses)),
				TimeoutMs: 5000,
				Retries:   1,
			}
			for _, a := range te.Addresses {
				cluster.Endpoints = append(cluster.Endpoints, Endpoint{
					Address: a,
					Port:    te.Port,
					Weight:  1,
				})
			}
			// детерминистика
			sort.Slice(cluster.Endpoints, func(i, j int) bool {
				if cluster.Endpoints[i].Address == cluster.Endpoints[j].Address {
					return cluster.Endpoints[i].Port < cluster.Endpoints[j].Port
				}
				return cluster.Endpoints[i].Address < cluster.Endpoints[j].Address
			})

			snap.Clusters = append(snap.Clusters, cluster)
		}
	}

	sort.Slice(snap.Routes, func(i, j int) bool {
		if snap.Routes[i].Host == snap.Routes[j].Host {
			if snap.Routes[i].Priority == snap.Routes[j].Priority {
				return snap.Routes[i].Path < snap.Routes[j].Path
			}
			return snap.Routes[i].Priority > snap.Routes[j].Priority
		}
		return snap.Routes[i].Host < snap.Routes[j].Host
	})
	sort.Slice(snap.Clusters, func(i, j int) bool { return snap.Clusters[i].Name < snap.Clusters[j].Name })

	sum := sha256.Sum256([]byte(
		fmt.Sprintf("%+v%+v", snap.Routes, snap.Clusters),
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

func matchSlicePort(slice discoveryv1.EndpointSlice, be *networkingv1.IngressBackend) *int32 {
	if be == nil || be.Service == nil {
		return nil
	}
	wantNum := be.Service.Port.Number
	wantName := be.Service.Port.Name

	for _, sp := range slice.Ports {
		if sp.Port == nil {
			continue
		}

		if wantNum != 0 && *sp.Port == wantNum {
			return sp.Port
		}

		if wantName != "" && sp.Name != nil && *sp.Name == wantName {
			return sp.Port
		}
	}

	return nil
}

func appendUnique(dst []string, src ...string) []string {
	for _, addr := range src {
		if !slices.Contains(dst, addr) {
			dst = append(dst, addr)
		}
	}

	return dst
}
