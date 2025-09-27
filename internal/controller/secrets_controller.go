package controller

import (
	"argon.github.io/ingress/internal/grpc"
	"context"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/builder"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	"sigs.k8s.io/controller-runtime/pkg/predicate"
)

const nameGRPCSecret = "grpc-tls"

type CertsReconciler struct {
	client          client.Client
	bundle          *grpc.Bundle
	targetName      string
	targetNamespace string
}

func (c *CertsReconciler) Reconcile(ctx context.Context, req ctrl.Request) (ctrl.Result, error) {
	if req.Name != c.targetName || req.Namespace != c.targetNamespace {
		return ctrl.Result{}, nil
	}

	if err := c.createCertsSecret(ctx, c.targetNamespace); err != nil {
		return ctrl.Result{}, err
	}

	return ctrl.Result{}, nil
}

func SetupCertController(mgr ctrl.Manager, bundle *grpc.Bundle, namespace string) error {
	r := &CertsReconciler{
		client:          mgr.GetClient(),
		bundle:          bundle,
		targetName:      nameGRPCSecret,
		targetNamespace: namespace,
	}

	onlyOurSecret := predicate.NewPredicateFuncs(func(o client.Object) bool {
		return o.GetName() == nameGRPCSecret && o.GetNamespace() == namespace
	})

	return ctrl.NewControllerManagedBy(mgr).
		For(&corev1.Secret{}, builder.WithPredicates(onlyOurSecret)).
		Complete(r)
}

func (c *CertsReconciler) createCertsSecret(ctx context.Context, namespace string) error {
	secret := &corev1.Secret{
		ObjectMeta: metav1.ObjectMeta{
			Name:      nameGRPCSecret,
			Namespace: namespace,
		},
	}

	_, err := controllerutil.CreateOrUpdate(ctx, c.client, secret, func() error {
		secret.Type = corev1.SecretTypeTLS // "kubernetes.io/tls"

		if secret.Data == nil {
			secret.Data = make(map[string][]byte, 2)
		}

		secret.Data[corev1.TLSCertKey] = c.bundle.ServerCertPEM      // "tls.crt"
		secret.Data[corev1.TLSPrivateKeyKey] = c.bundle.ServerKeyPEM // "tls.key"

		// if secret.Labels == nil { secret.Labels = map[string]string{} }
		// secret.Labels["app.kubernetes.io/managed-by"] = "your-controller"

		// return controllerutil.SetControllerReference(c.owner, secret, c.scheme)

		return nil
	})

	return err
}
