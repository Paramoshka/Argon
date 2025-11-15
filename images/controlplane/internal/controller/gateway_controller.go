package controller

import (
	"context"
	"fmt"
	"net"
	"net/netip"

	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/builder"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	"sigs.k8s.io/controller-runtime/pkg/predicate"
	gwapiv1 "sigs.k8s.io/gateway-api/apis/v1"
	gwapiv1alpha2 "sigs.k8s.io/gateway-api/apis/v1alpha2"
	gwapiv1beta1 "sigs.k8s.io/gateway-api/apis/v1beta1"
)

const gatewayClassIndexKey = "spec.gatewayClassName"

type ArgonConfigReconcilerGatewayAPI struct {
	client.Client
	GatewayClass string
}

func (r *ArgonConfigReconcilerGatewayAPI) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {

	gwList := &gwapiv1.GatewayList{}
	if err := r.Client.List(ctx, gwList, client.MatchingFields{
		gatewayClassIndexKey: r.GatewayClass,
	}); err != nil {
		return ctrl.Result{}, err
	}

	snap, err := gatewayListToSnap(gwList)

	return ctrl.Result{}, nil
}

func SetupGatewayController(mgr ctrl.Manager, gatewayClassName string) error {

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

	reconciler := &ArgonConfigReconcilerGatewayAPI{
		Client:       mgr.GetClient(),
		GatewayClass: gatewayClassName,
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

func gatewayListToSnap(gwList *gwapiv1.GatewayList) (any, error) {

	for gwi := range gwList.Items {
		gw := gwList.Items[gwi]
		ipsList, err := getAddressesFromGateway(*gw)
		if err != nil {
			continue
		}
	}

	return nil, nil
}

type NamedResolver interface {
	ResolveNamed(ctx context.Context, name string) ([]netip.Addr, error)
}

func getAddressesFromGateway(
	ctx context.Context,
	gw *gwapiv1.Gateway,
	dns *net.Resolver,
	named NamedResolver,
) ([]netip.Addr, error) {
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
			resolved, err := dns.LookupNetIP(ctx, "ip", addr.Value)
			if err != nil {
				return nil, fmt.Errorf("gateway %s: hostname %q cannot be resolved: %w", gw.Name, addr.Value, err)
			}
			ips = append(ips, resolved...)
		case gwapiv1.NamedAddressType:
			resolved, err := named.ResolveNamed(ctx, addr.Value)
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
