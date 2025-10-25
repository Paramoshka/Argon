use dashmap::DashMap;
use http::HeaderName;
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::argon_config::{AuthConfig, Endpoint, HeaderRewrite, Snapshot};
use tracing::warn;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RouteRule {
    path: String,
    path_type: PathType,
    pub cluster: String,
    priority: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EndpointKey {
    address: String,
    port: i32,
    index: usize,
}

#[derive(Clone, Debug)]
pub struct SelectedEndpoint {
    pub endpoint: Endpoint,
    pub counter: Option<Arc<AtomicUsize>>,
}

#[derive(Clone, Debug)]
pub struct AuthConfigDex {
    pub url: Option<String>,
    pub signin: Option<String>,
    pub response_headers: Arc<Vec<String>>, // names to copy from auth response
    pub skip_paths: Arc<Vec<String>>,       // prefixes to skip auth
    pub cookie_name: Option<String>,
}

impl AuthConfigDex {
    pub fn from_pb(pb: &AuthConfig) -> Self {
        let url = if pb.url.trim().is_empty() {
            None
        } else {
            Some(pb.url.clone())
        };
        let signin = if pb.signin.trim().is_empty() {
            None
        } else {
            Some(pb.signin.clone())
        };
        let cookie_name = if pb.cookie_name.trim().is_empty() {
            None
        } else {
            Some(pb.cookie_name.clone())
        };

        // Normalize lists: trim, drop empties, dedup while preserving order
        fn normalize_list(items: &[String]) -> Vec<String> {
            let mut out = Vec::with_capacity(items.len());
            for s in items {
                let v = s.trim();
                if v.is_empty() {
                    continue;
                }
                if !out
                    .iter()
                    .any(|x: &String| x.as_str().eq_ignore_ascii_case(v))
                {
                    out.push(v.to_string());
                }
            }
            out
        }

        let response_headers = Arc::new(normalize_list(&pb.response_headers));
        let skip_paths = Arc::new(normalize_list(&pb.skip_paths));

        AuthConfigDex {
            url,
            signin,
            response_headers,
            skip_paths,
            cookie_name,
        }
    }
}

fn build_auth_runtime(auth: Option<&AuthConfig>) -> Option<Arc<AuthConfigDex>> {
    let Some(pb) = auth else { return None };
    // If everything is empty, don't attach auth
    let has_any = !pb.url.trim().is_empty()
        || !pb.signin.trim().is_empty()
        || !pb.response_headers.is_empty()
        || !pb.skip_paths.is_empty()
        || !pb.cookie_name.trim().is_empty();
    if !has_any {
        return None;
    }
    Some(Arc::new(AuthConfigDex::from_pb(pb)))
}

#[derive(Clone, Debug)]
pub struct ClusterRule {
    name: String,
    /// "RoundRobin"...
    lb_policy: LBPolicy,
    endpoints: Vec<Endpoint>,
    pub timeout_ms: i32,
    pub retries: i32,
    pub backend_protocol: BackendProtocol,
    pub request_headers: Arc<Vec<HeaderRewriteRule>>,
    pub backend_tls_insecure_skip_verify: bool,
    rr_cursor: Arc<AtomicUsize>,
    least_conn_cursor: Arc<DashMap<EndpointKey, Arc<AtomicUsize>>>,
    pub auth: Option<Arc<AuthConfigDex>>,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
enum PathType {
    Prefix,
    Exact,
}
impl PathType {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "Prefix" => Some(PathType::Prefix),
            "Exact" => Some(PathType::Exact),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum LBPolicy {
    RoundRobin,
    LeastConn,
}

impl LBPolicy {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "RoundRobin" => Some(LBPolicy::RoundRobin),
            "LeastConn" => Some(LBPolicy::LeastConn),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendProtocol {
    H1,
    H2,
    H1Ssl,
    H2Ssl,
}

impl BackendProtocol {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "h1" => Some(BackendProtocol::H1),
            "h2" => Some(BackendProtocol::H2),
            "h1-ssl" => Some(BackendProtocol::H1Ssl),
            "h2-ssl" => Some(BackendProtocol::H2Ssl),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderRewriteMode {
    Set,
    Append,
    Remove,
}

impl HeaderRewriteMode {
    fn parse(mode: &str) -> Option<Self> {
        match mode.to_ascii_lowercase().as_str() {
            "set" => Some(HeaderRewriteMode::Set),
            "append" => Some(HeaderRewriteMode::Append),
            "remove" => Some(HeaderRewriteMode::Remove),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderRewriteRule {
    pub name: HeaderName,
    pub value: Option<String>,
    pub mode: HeaderRewriteMode,
}

#[derive(Clone)]
pub struct RouteTable {
    version: String,
    routes_by_host: HashMap<String, Arc<Vec<RouteRule>>>, // host name -> route_rule
    clusters: HashMap<String, Arc<ClusterRule>>,          // cluster name -> cluster_rule
}

impl Default for RouteTable {
    fn default() -> Self {
        RouteTable {
            version: "".to_string(),
            routes_by_host: Default::default(),
            clusters: Default::default(),
        }
    }
}

impl RouteTable {
    // new create sorted hasMap route table for fast routing
    pub fn new(snapshot: &Snapshot) -> Self {
        // create hashMap clusters
        let mut clusters: HashMap<String, Arc<ClusterRule>> = HashMap::new();
        for cluster in &snapshot.clusters {
            let bp = BackendProtocol::parse(&cluster.backend_protocol)
                .unwrap_or_else(|| BackendProtocol::H1);

            if let Some(lb) = LBPolicy::parse(&cluster.lb_policy) {
                let counters = EndpointKey::build_map(&cluster.endpoints);
                let request_headers = build_header_rewrites(&cluster.request_headers);
                let auth = build_auth_runtime(cluster.auth.as_ref());
                clusters
                    .entry(cluster.name.to_ascii_lowercase())
                    .insert_entry(Arc::from(ClusterRule {
                        name: "".to_string(), // maybe remove ?
                        lb_policy: lb,
                        endpoints: cluster.endpoints.clone(),
                        timeout_ms: cluster.timeout_ms,
                        retries: cluster.retries,
                        rr_cursor: Arc::new(AtomicUsize::new(0)),
                        least_conn_cursor: counters,
                        backend_protocol: bp,
                        request_headers,
                        backend_tls_insecure_skip_verify: cluster.backend_tls_insecure_skip_verify,
                        auth,
                    }));
            }
        }

        // create hasMap routes
        let mut buckets: HashMap<String, Vec<RouteRule>> = HashMap::new();
        for r in &snapshot.routes {
            if let Some(pt) = PathType::parse(&r.path_type) {
                buckets
                    .entry(r.host.to_ascii_lowercase())
                    .or_default()
                    .push(RouteRule {
                        path: r.path.clone(),
                        path_type: pt,
                        cluster: r.cluster.clone(),
                        priority: r.priority,
                    });
            }
        }

        // sorting in each bucket: priority, path.len, Exact, Prefix
        for v in buckets.values_mut() {
            v.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then(b.path.len().cmp(&a.path.len()))
                    .then_with(|| match (a.path_type, b.path_type) {
                        (PathType::Exact, PathType::Prefix) => std::cmp::Ordering::Less,
                        (PathType::Prefix, PathType::Exact) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    })
            });
        }

        // convert type routes
        let routes_by_host = buckets.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();

        RouteTable {
            version: snapshot.version.clone(),
            routes_by_host: routes_by_host,
            clusters: clusters,
        }
    }

    // get rule for host
    pub fn choose_route<'a>(&'a self, host: &str, path: &str) -> Option<&'a RouteRule> {
        if let Some(route_rule) = self.routes_by_host.get(host) {
            if let Some(rule) = Self::match_in_bucket(route_rule, path) {
                return Some(rule);
            }
        }

        if let Some(route_rule) = self.routes_by_host.get("") {
            if let Some(rule) = Self::match_in_bucket(route_rule, path) {
                return Some(rule);
            }
        }

        // println!("For host {} found in path {:?}", host, self.routes_by_host);

        None
    }

    // get path if match
    // todo make Prefix in more precision and add Implemented
    fn match_in_bucket<'a>(rules: &'a [RouteRule], path: &str) -> Option<&'a RouteRule> {
        rules.iter().find(|r| match r.path_type {
            PathType::Exact => r.path == path,
            PathType::Prefix => path.starts_with(r.path.as_str()),
        })
    }

    // get endpoint by balance algorithm
    pub fn get_endpoint(&self, cluster_name: &str) -> Option<SelectedEndpoint> {
        let cluster = self.clusters.get(cluster_name)?;
        match cluster.lb_policy {
            LBPolicy::RoundRobin => self.round_robin(cluster),
            LBPolicy::LeastConn => self.least_conn(cluster),
        }
    }

    // get cluster rule timeouts, retries etc...
    pub fn get_cluster_rules(&self, cluster_name: &str) -> Option<Arc<ClusterRule>> {
        let cluster = self.clusters.get(cluster_name)?;
        Some(cluster.clone())
    }

    // RoundRobin algorithm
    fn round_robin(&self, cluster: &ClusterRule) -> Option<SelectedEndpoint> {
        let len = cluster.endpoints.len();
        if len == 0 {
            return None;
        }
        let idx = cluster.rr_cursor.fetch_add(1, Ordering::Relaxed) % len;
        let endpoint = cluster.endpoints[idx].clone();
        let counter = cluster.counter_for_index(idx);
        Some(SelectedEndpoint { endpoint, counter })
    }

    // LeastConn algorithm
    fn least_conn(&self, cluster: &ClusterRule) -> Option<SelectedEndpoint> {
        let len = cluster.endpoints.len();
        if len == 0 {
            return None;
        }

        let min = cluster
            .least_conn_cursor
            .as_ref()
            .iter()
            .min_by_key(|entry| entry.value().load(Ordering::Relaxed));

        if let Some(entry) = min {
            let idx = entry.key().index;
            if let Some(endpoint) = cluster.endpoints.get(idx).cloned() {
                let counter = Some(entry.value().clone());
                return Some(SelectedEndpoint { endpoint, counter });
            }
        }

        self.round_robin(cluster)
    }
}

impl ClusterRule {
    fn counter_for_index(&self, idx: usize) -> Option<Arc<AtomicUsize>> {
        let endpoint = self.endpoints.get(idx)?;
        let key = EndpointKey::from_endpoint(idx, endpoint);
        self.least_conn_cursor
            .get(&key)
            .map(|entry| entry.value().clone())
    }
}

impl EndpointKey {
    fn from_endpoint(index: usize, endpoint: &Endpoint) -> Self {
        Self {
            address: endpoint.address.clone(),
            port: endpoint.port,
            index,
        }
    }

    fn build_map(endpoints: &[Endpoint]) -> Arc<DashMap<EndpointKey, Arc<AtomicUsize>>> {
        let counters = DashMap::with_capacity(endpoints.len());
        for (index, endpoint) in endpoints.iter().enumerate() {
            let key = EndpointKey::from_endpoint(index, endpoint);
            counters.insert(key, Arc::new(AtomicUsize::new(0)));
        }
        Arc::new(counters)
    }
}

fn build_header_rewrites(items: &[HeaderRewrite]) -> Arc<Vec<HeaderRewriteRule>> {
    let mut rewrites = Vec::with_capacity(items.len());
    for item in items {
        let name = item.name.trim();
        if name.is_empty() {
            warn!("ignoring request header rewrite with empty name");
            continue;
        }

        let Some(mode) = HeaderRewriteMode::parse(item.mode.as_str()) else {
            warn!(mode = %item.mode, header = %item.name, "ignoring unsupported header rewrite mode");
            continue;
        };

        let value = if matches!(mode, HeaderRewriteMode::Remove) {
            None
        } else {
            Some(item.value.clone())
        };

        let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) else {
            warn!(header = %item.name, "ignoring header rewrite with invalid name");
            continue;
        };

        rewrites.push(HeaderRewriteRule {
            name: header_name,
            value,
            mode,
        });
    }
    Arc::new(rewrites)
}
