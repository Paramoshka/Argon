package controller

import (
	"context"
	"crypto/sha256"
	"crypto/x509"
	"encoding/hex"
	"encoding/pem"
	"fmt"
	"net/netip"

	. "argon/internal/model"

	corev1 "k8s.io/api/core/v1"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/builder"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/predicate"
	gwapiv1 "sigs.k8s.io/gateway-api/apis/v1"
	gwapiv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gwapiv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

const (
	gatewayClassIndexKey    = "spec.gatewayClassName"
	httpRouteParentRefIndex = "spec.ParentRefs"
)

type ArgonConfigReconcilerGatewayAPI struct {
	client.Client
	GatewayClass string
	dnsResolver  DNSNamedResolver
}

func (r *ArgonConfigReconcilerGatewayAPI) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {

	gwList := &gwapiv1.GatewayList{}
	if err := r.Client.List(ctx, gwList, client.MatchingFields{
		gatewayClassIndexKey: r.GatewayClass,
	}); err != nil {
		return ctrl.Result{}, err
	}

	_, err := r.gatewayListToSnap(ctx, gwList)
	if err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

func SetupGatewayController(mgr ctrl.Manager, gatewayClassName string) error {

	if err := setupGatewayAPICache(mgr); err != nil {
		return err
	}

	reconciler := &ArgonConfigReconcilerGatewayAPI{
		Client:       mgr.GetClient(),
		GatewayClass: gatewayClassName,
		dnsResolver:  *NewDNSResolver(),
	}

	gatewayPredicate := predicate.NewPredicateFuncs(func(obj client.Object) bool {
		if gatewayClassName == "" {
			return true
		}

		gw, ok := obj.(*gwapiv1.Gateway)
		if !ok {
			return true
		}

		return string(gw.Spec.GatewayClassName) == gatewayClassName
	})

	gatewayClassPredicate := predicate.NewPredicateFuncs(func(obj client.Object) bool {
		if gatewayClassName == "" {
			return true
		}

		return obj.GetName() == gatewayClassName
	})

	ctrlBuilder := ctrl.NewControllerManagedBy(mgr).
		Named("gateway-controller").
		For(&gwapiv1.Gateway{}, builder.WithPredicates(gatewayPredicate))

	ctrlBuilder = ctrlBuilder.Watches(
		&gwapiv1.GatewayClass{},
		&handler.EnqueueRequestForObject{},
		builder.WithPredicates(gatewayClassPredicate),
	)

	watchedTypes := []client.Object{
		&gwapiv1.HTTPRoute{},
		&gwapiv1alpha2.TCPRoute{},
		&gwapiv1alpha2.TLSRoute{},
		&gwapiv1alpha2.UDPRoute{},
		&gwapiv1beta1.ReferenceGrant{},
	}

	for _, watchType := range watchedTypes {
		ctrlBuilder = ctrlBuilder.Watches(
			watchType,
			&handler.EnqueueRequestForObject{},
		)
	}

	return ctrlBuilder.Complete(reconciler)
}

func (r *ArgonConfigReconcilerGatewayAPI) gatewayListToSnap(ctx context.Context, gwList *gwapiv1.GatewayList) ([]GatewaySnapshot, error) {
	snapshots := make([]GatewaySnapshot, 0, len(gwList.Items))

	for gwi := range gwList.Items {
		gw := gwList.Items[gwi]
		if len(gw.Spec.Listeners) == 0 {
			return nil, fmt.Errorf("gateway %s has no listeners", gw.Name)
		}

		addresses, err := r.getAddressesFromGateway(ctx, &gw)
		if err != nil {
			return nil, err
		}

		gwSnap := GatewaySnapshot{
			Name:      gw.Name,
			Namespace: gw.Namespace,
			Addresses: addresses,
			Listeners: make([]ListenerSnapshot, 0, len(gw.Spec.Listeners)),
			TLS:       make([]TLSSecret, 0),
		}

		for _, listener := range gw.Spec.Listeners {
			listenerSnapshot, tlsBundles, err := r.parseListener(ctx, &gw, listener)
			if err != nil {
				return nil, err
			}
			gwSnap.Listeners = append(gwSnap.Listeners, listenerSnapshot)
			if len(tlsBundles) > 0 {
				gwSnap.TLS = append(gwSnap.TLS, tlsBundles...)
			}
		}

		snapshots = append(snapshots, gwSnap)
	}

	if len(snapshots) == 0 {
		return nil, fmt.Errorf("no gateways available for snapshot")
	}

	return snapshots, nil
}

func (r *ArgonConfigReconcilerGatewayAPI) getAddressesFromGateway(
	ctx context.Context,
	gw *gwapiv1.Gateway,
) ([]netip.Addr, error) {
	if gw == nil {
		return nil, fmt.Errorf("Gateway is nill, cannot parse addresses")
	}

	if len(gw.Status.Addresses) == 0 {
		return nil, fmt.Errorf("gateway %s has no addresses", gw.Name)
	}

	var ips []netip.Addr
	for _, addr := range gw.Status.Addresses {
		addrType := gwapiv1.IPAddressType
		if addr.Type != nil {
			addrType = *addr.Type
		}

		switch addrType {
		case gwapiv1.IPAddressType:
			ip, err := netip.ParseAddr(addr.Value)
			if err != nil {
				return nil, fmt.Errorf("gateway %s: invalid IP %q: %w", gw.Name, addr.Value, err)
			}
			ips = append(ips, ip)
		case gwapiv1.HostnameAddressType:
			resolved, err := r.dnsResolver.ResolveNamed(ctx, addr.Value)
			if err != nil {
				return nil, fmt.Errorf("gateway %s: hostname %q cannot be resolved: %w", gw.Name, addr.Value, err)
			}
			ips = append(ips, resolved...)
		case gwapiv1.NamedAddressType:
			resolved, err := r.dnsResolver.ResolveNamed(ctx, addr.Value)
			if err != nil {
				return nil, fmt.Errorf("gateway %s: named address %q cannot be resolved: %w", gw.Name, addr.Value, err)
			}
			ips = append(ips, resolved...)
		default:
			return nil, fmt.Errorf("gateway %s: unsupported address type %q", gw.Name, addrType)
		}
	}
	return ips, nil
}

func (r *ArgonConfigReconcilerGatewayAPI) parseListener(
	ctx context.Context,
	gw *gwapiv1.Gateway,
	listener gwapiv1.Listener,
) (ListenerSnapshot, []TLSSecret, error) {
	snapshot := ListenerSnapshot{
		Name:     string(listener.Name),
		Port:     int32(listener.Port),
		Protocol: listener.Protocol,
		Hostname: listener.Hostname,
	}
	// if listener.AllowedRoutes != nil { // TODO check allowed routes
	// 	snapshot.AllowedRoutes = *listener.AllowedRoutes
	// }

	var tlsBundles []TLSSecret

	if listener.TLS != nil {
		snapshot.TLSMode = listener.TLS.Mode
		if len(listener.TLS.CertificateRefs) > 0 {
			snapshot.Certificates = make([]gwapiv1.SecretObjectReference, len(listener.TLS.CertificateRefs))
			copy(snapshot.Certificates, listener.TLS.CertificateRefs)
		}

		if listener.TLS.Mode == nil || *listener.TLS.Mode == gwapiv1.TLSModeTerminate {
			var sni []string
			if listener.Hostname != nil {
				sni = append(sni, string(*listener.Hostname))
			}
			for _, ref := range listener.TLS.CertificateRefs {
				bundle, err := r.buildTLSSecretFromRef(ctx, gw.Namespace, ref, sni)
				if err != nil {
					return snapshot, tlsBundles, err
				}
				tlsBundles = append(tlsBundles, bundle)
			}
		}
	}

	return snapshot, tlsBundles, nil
}

func (r *ArgonConfigReconcilerGatewayAPI) buildTLSSecretFromRef(
	ctx context.Context,
	gatewayNamespace string,
	ref gwapiv1.SecretObjectReference,
	sni []string,
) (TLSSecret, error) {
	if ref.Kind != nil && *ref.Kind != "Secret" {
		return TLSSecret{}, fmt.Errorf("unsupported certificate ref kind %q for %s", *ref.Kind, ref.Name)
	}
	if ref.Group != nil && *ref.Group != "" {
		return TLSSecret{}, fmt.Errorf("unsupported certificate ref group %q for %s", *ref.Group, ref.Name)
	}

	namespace := gatewayNamespace
	if ref.Namespace != nil && len(*ref.Namespace) > 0 {
		namespace = string(*ref.Namespace)
	}

	var secret corev1.Secret
	if err := r.Get(ctx, client.ObjectKey{Namespace: namespace, Name: string(ref.Name)}, &secret); err != nil {
		return TLSSecret{}, err
	}

	crt := secret.Data["tls.crt"]
	key := secret.Data["tls.key"]
	if len(crt) == 0 || len(key) == 0 {
		return TLSSecret{}, fmt.Errorf("secret %s/%s missing tls.crt or tls.key", namespace, ref.Name)
	}

	block, _ := pem.Decode(crt)
	if block == nil {
		return TLSSecret{}, fmt.Errorf("failed to decode certificate for secret %s/%s", namespace, ref.Name)
	}

	certs, err := x509.ParseCertificates(block.Bytes)
	if err != nil || len(certs) == 0 {
		return TLSSecret{}, fmt.Errorf("failed to parse certificate for secret %s/%s: %w", namespace, ref.Name, err)
	}

	sum := sha256.Sum256(append(crt, key...))

	return TLSSecret{
		Name:         fmt.Sprintf("%s/%s", namespace, ref.Name),
		Sni:          sni,
		CertPem:      crt,
		KeyPem:       key,
		NotAfterUnix: certs[0].NotAfter,
		Version:      hex.EncodeToString(sum[:]),
	}, nil
}

func setupGatewayAPICache(mgr ctrl.Manager) error {
	if err := mgr.GetFieldIndexer().IndexField(
		context.Background(),
		&gwapiv1.Gateway{},
		gatewayClassIndexKey,
		func(obj client.Object) []string {
			gw := obj.(*gwapiv1.Gateway)
			if gw.Spec.GatewayClassName == "" {
				return []string{}
			}
			v := string(gw.Spec.GatewayClassName)
			return []string{v}
		},
	); err != nil {
		return err
	}

	return nil
}

func setupHTTPRouteCache(mgr ctrl.Manager) error {
	if err := mgr.GetFieldIndexer().IndexField(
		context.Background(),
		&gwapiv1.HTTPRoute{},
		httpRouteParentRefIndex,
		func(o client.Object) []string {
			httpRoute := o.(*gwapiv1.HTTPRoute)
			refs := httpRoute.Spec.ParentRefs
			if refs != nil && len(refs) == 0 {
				return nil
			}

			for i := range refs {
				groupOK := refs[i].Group == nil || *refs[i].Group == gwapiv1.Group(gwapiv1.GroupVersion.Group)
				kindOK := refs[i].Kind == nil || *refs[i].Kind == gwapiv1.Kind("Gateway")

				ns := httpRoute.Namespace
				if refs[i].Namespace != nil && *refs[i].Namespace != "" {
					ns = string(*refs[i].Namespace)
				}

				// TODO

			}

			return []string{""}
		},
	); err != nil {
		return err
	}
	return nil
}
