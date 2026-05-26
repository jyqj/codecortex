//! Infrastructure graph data model.
//! Kept minimal — fine-grained details go in `properties`.

use serde::{Deserialize, Serialize};

/// Infrastructure node kind — intentionally small enum.
/// Use `properties` for anything not covered here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfraKind {
    DockerImage,
    DockerStage,    // A named build stage (FROM ... AS name)
    DockerExpose,   // EXPOSE port
    ComposeService,
    K8sDeployment,
    K8sService,
    K8sIngress,
    K8sConfigMap,
    K8sSecret,
    K8sStatefulSet,
    K8sDaemonSet,
    K8sJob,
    K8sCronJob,
    K8sPvc,
    K8sServiceAccount,
    K8sNamespace,
    KustomizeOverlay,
    /// PubSub topic, Kafka topic, SNS topic
    MessageTopic,
    /// SQS queue, RabbitMQ queue, Cloud Tasks queue
    MessageQueue,
}

impl InfraKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DockerImage => "docker_image",
            Self::DockerStage => "docker_stage",
            Self::DockerExpose => "docker_expose",
            Self::ComposeService => "compose_service",
            Self::K8sDeployment => "k8s_deployment",
            Self::K8sService => "k8s_service",
            Self::K8sIngress => "k8s_ingress",
            Self::K8sConfigMap => "k8s_config_map",
            Self::K8sSecret => "k8s_secret",
            Self::K8sStatefulSet => "k8s_stateful_set",
            Self::K8sDaemonSet => "k8s_daemon_set",
            Self::K8sJob => "k8s_job",
            Self::K8sCronJob => "k8s_cron_job",
            Self::K8sPvc => "k8s_pvc",
            Self::K8sServiceAccount => "k8s_service_account",
            Self::K8sNamespace => "k8s_namespace",
            Self::KustomizeOverlay => "kustomize_overlay",
            Self::MessageTopic => "message_topic",
            Self::MessageQueue => "message_queue",
        }
    }
}

/// Infrastructure edge kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InfraEdgeKind {
    UsesImage,
    DependsOn,
    ExposesPort,
    RoutesTo,
    /// Topic/queue binds to an endpoint/route
    BindsTopic,
    /// Service consumes from a queue
    ConsumesQueue,
}

impl InfraEdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UsesImage => "uses_image",
            Self::DependsOn => "depends_on",
            Self::ExposesPort => "exposes_port",
            Self::RoutesTo => "routes_to",
            Self::BindsTopic => "binds_topic",
            Self::ConsumesQueue => "consumes_queue",
        }
    }
}

/// A node in the infrastructure graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraNode {
    pub node_id: String,
    pub file_path: String,
    pub kind: InfraKind,
    pub name: String,
    pub namespace: Option<String>,
    pub line: u32,
    pub end_line: Option<u32>,
    pub properties: serde_json::Value,
    /// Symbol UID this infra node is bound to (e.g., the entry point function for a K8s deployment)
    pub bound_symbol_uid: Option<String>,
    /// Confidence of the binding (0.0-1.0)
    pub binding_confidence: Option<f64>,
}

/// An edge in the infrastructure graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfraEdge {
    pub edge_id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub kind: InfraEdgeKind,
    pub confidence: f64,
    pub properties: serde_json::Value,
}
