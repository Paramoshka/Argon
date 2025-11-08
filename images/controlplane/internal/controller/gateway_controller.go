package controller

import (
	"context"

	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/handler"
	gwapiv1 "sigs.k8s.io/gateway-api/apis/v1"
)

type ArgonConfigReconcilerGatewayAPI struct {
	client.Client
}

func (r *ArgonConfigReconcilerGatewayAPI) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	return ctrl.Result{}, nil
}

func SetupGatewayController(mgr ctrl.Manager) error {
	reconciler := &ArgonConfigReconcilerGatewayAPI{
		Client: mgr.GetClient(),
	}

	ctrlBuilder := ctrl.NewControllerManagedBy(mgr).
		For(&gwapiv1.Gateway{})

	watchedTypes := []client.Object{
		&gwapiv1.GatewayClass{},
		&gwapiv1.HTTPRoute{},
		&gwapiv1.TCPRoute{},
		&gwapiv1.TLSRoute{},
		&gwapiv1.UDPRoute{},
		&gwapiv1.ReferenceGrant{},
	}

	for _, watchType := range watchedTypes {
		ctrlBuilder.Watches(watchType,
			&handler.EnqueueRequestForObject{},
		)
	}

	return ctrlBuilder.Complete(reconciler)
}
