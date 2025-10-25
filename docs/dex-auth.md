Dex + oauth2-proxy sample for Argon auth

This example deploys Dex and oauth2-proxy, then protects an example app behind Argon using the new auth annotations.

What you’ll get
- Namespace `oauth` with:
  - Dex (issuer: http://dex.oauth.svc.cluster.local:5556/dex)
  - oauth2-proxy (OIDC client for Dex)
- Example echo app + Argon Ingress with auth subrequest and redirect.

Prerequisites
- Argon controller/dataplane deployed and handling `ingressClassName: argon`.
- Local DNS/hosts for `echo.local` pointing to your cluster ingress IP (e.g., 127.0.0.1 for kind + port-forward).

1) Create oauth namespace (for Dex)
kubectl create namespace oauth || true

2) Prepare secrets for oauth2-proxy (in default namespace, same as Ingress/app)
- CLIENT_SECRET (random hex):
  CLIENT_SECRET=$(openssl rand -hex 32)
- COOKIE_SECRET (32B base64):
  COOKIE_SECRET=$(openssl rand -base64 32)
- Create the secret in default:
  kubectl -n default create secret generic oauth2-proxy \
    --from-literal=client-secret=${CLIENT_SECRET} \
    --from-literal=cookie-secret=${COOKIE_SECRET}

3) Prepare a bcrypt password for Dex static user
- Example using htpasswd (Apache utils):
  HASH=$(htpasswd -nbBC 10 user password | cut -d: -f2)
  echo ${HASH}
- Note: Dex expects bcrypt with $2a prefix; some tools use $2y — replace $2y with $2a if needed.

4) Apply Dex and oauth2-proxy
- Edit examples/dex/dex.yaml and:
  - Replace REPLACE_BCRYPT_HASH with your bcrypt hash.
  - Set staticClients.secret to the same value as ${CLIENT_SECRET} from step 2.
  kubectl apply -f examples/dex/dex.yaml
  kubectl apply -f examples/dex/oauth2-proxy.yaml

5) Deploy echo app + Argon Ingress with auth
kubectl apply -f examples/demo-ingress-auth.yaml

6) Test
- Open http://echo.local/
- You should be redirected to oauth2-proxy → Dex login. Use the static user:
  - Username: user@example.com (matches dex.yaml)
  - Password: the password for your generated hash (e.g., "password")
- After login, you will be redirected back to the app and headers like X-Auth-Request-User will be injected to upstream if configured.

Files
- examples/dex/dex.yaml — Dex namespace objects (ConfigMap, Deployment, Service)
- examples/dex/oauth2-proxy.yaml — oauth2-proxy Deployment + Service, uses secret `oauth2-proxy`
- examples/demo-ingress-auth.yaml — echo app + Argon Ingress with auth annotations and route for /oauth2/ to oauth2-proxy

Notes
- This sample uses HTTP for simplicity (cookie-secure=false). For production, terminate TLS and set cookie-secure=true and redirect/signin URLs to https.
- Sign-in redirect template uses http://$host/oauth2/start?rd=$escaped_request_uri. We route /oauth2/ to oauth2-proxy via a separate Argon Ingress rule to avoid loops and make the flow self-contained.
