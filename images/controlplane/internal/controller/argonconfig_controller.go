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
    "time"

    . "argon/internal/grpc"
    . "argon/internal/model"

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

		targetEndpoint := parseAnnotations(annotations)
		targetEndpoint.Retries = 1 // TODO

		// tls
		bundle := r.buildTLSBundle(ctx, ing)

		for _, rule := range ing.Spec.Rules {
			// Build target proxy for this rule using helper
			t := buildTargetFromRule(ctx, r.Client, ing.Namespace, rule, *targetEndpoint, bundle)
			if len(t.Path) > 0 {
				targetProxies = append(targetProxies, t)
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
				RewriteHeaders:  te.RewriteHeaders,
				Auth:            te.Auth,
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
