use std::cmp::PartialEq;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::argon_config::{Endpoint, Snapshot};

#[derive(Clone, Debug)]
pub struct RouteRule {
    path: String,
    path_type: PathType,
    pub cluster: String,
    priority: i32,
}

#[derive(Clone, Debug)]
pub struct ClusterRule {
    name: String,
    /// "RoundRobin"...
    lb_policy: LBPolicy,
    endpoints: Vec<Endpoint>,
    timeout_ms: i32,
    retries: i32,
    rr_cursor: Arc<AtomicUsize>
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum PathType { Prefix, Exact }
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
enum LBPolicy { RoundRobin }

impl LBPolicy {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "RoundRobin" => Some(LBPolicy::RoundRobin),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct RouteTable {
    version: String,
    routes_by_host: HashMap<String, Arc<Vec<RouteRule>>>, // host name -> route_rule
    clusters: HashMap<String, Arc<ClusterRule>>, // cluster name -> cluster_rule
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
    pub fn new(
        snapshot: &Snapshot
    ) -> Self {
        // create hashMap clusters
        let mut clusters: HashMap<String, Arc<ClusterRule>> = HashMap::new();
        for cluster in &snapshot.clusters {
            if let Some(lb) = LBPolicy::parse(&cluster.lb_policy) {
                clusters.entry(cluster.name.to_ascii_lowercase())
                    .insert_entry(Arc::from(ClusterRule {
                        name: "".to_string(), // maybe remove ?
                        lb_policy: LBPolicy::RoundRobin,
                        endpoints: cluster.endpoints.clone(),
                        timeout_ms: cluster.timeout_ms,
                        retries: cluster.retries,
                        rr_cursor: Arc::new(AtomicUsize::new(0)),
                    }));
            }
        }

        // create hasMap routes
        let mut buckets: HashMap<String, Vec<RouteRule>> = HashMap::new();
        for r in &snapshot.routes {
            if let Some(pt) = PathType::parse(&r.path_type) {
                buckets.entry(r.host.to_ascii_lowercase())
                    .or_default()
                    .push(RouteRule {
                        path: r.path.clone(),
                        path_type: pt,
                        cluster: r.cluster.clone(),
                        priority: r.priority,
                    });
            }
        }

        // sorting in each bucket: priority, path.len, Exact, Prefvfix
        for v in buckets.values_mut() {
            v.sort_by(|a, b| {
                b.priority.cmp(&a.priority)
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
                // println!("For host {} found in path {:?}", host, route_rule);
                return Some(rule);
            }
        }

        if let Some(route_rule) = self.routes_by_host.get("") {
            if let Some(rule) = Self::match_in_bucket(route_rule, path) {
                return Some(rule);
            }
        }

        None
    }

    // get path if match
    fn match_in_bucket<'a>(rules: &'a [RouteRule], path: &str) -> Option<&'a RouteRule> {
        rules.iter().find(|r| match r.path_type {
            PathType::Exact => r.path == path,
            PathType::Prefix => path.starts_with(r.path.as_str()),
        })
    }

    // get endpoint by balance algorithm
    pub fn get_endpoint(&self, cluster_name: &str) -> Option<Endpoint> {
        let cluster = self.clusters.get(cluster_name)?;
        // println!("Cluster {} found in path {:?}", cluster_name, cluster);
        match cluster.lb_policy {
            LBPolicy::RoundRobin => self.round_robin(cluster),
            _ => Some(cluster.endpoints.first()?.clone()),
        }
    }

    // RoundRobin algorithm
    fn round_robin(&self, cluster: &ClusterRule) -> Option<Endpoint> {
        let len = cluster.endpoints.len();
        if len == 0 { return None; }
        let idx = cluster.rr_cursor.fetch_add(1, Ordering::Relaxed) % len;
        Some(cluster.endpoints[idx].clone())
    }
}