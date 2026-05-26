//! Broker pattern detection for async message queue identification.
//!
//! Matches library identifiers (import paths, module names, object names)
//! against known async broker patterns to classify call_kind and broker_type.

/// Result of broker pattern matching.
pub struct BrokerMatch {
    /// `"async"` for message brokers, `"http"` for HTTP clients, `"grpc"` for gRPC.
    pub call_kind: &'static str,
    /// Broker identifier: `"kafka"`, `"rabbitmq"`, `"pubsub"`, etc.
    pub broker_type: &'static str,
}

/// Pattern entry: (substring to match, broker_type).
struct BrokerPattern {
    substring: &'static str,
    broker_type: &'static str,
}

/// Import-path / qualified-name patterns (case-insensitive substring match).
static IMPORT_PATTERNS: &[BrokerPattern] = &[
    // Google Cloud
    BrokerPattern {
        substring: "cloud_tasks",
        broker_type: "cloud_tasks",
    },
    BrokerPattern {
        substring: "cloudtasks",
        broker_type: "cloud_tasks",
    },
    BrokerPattern {
        substring: "cloud.pubsub",
        broker_type: "pubsub",
    },
    BrokerPattern {
        substring: "pubsub",
        broker_type: "pubsub",
    },
    // AWS
    BrokerPattern {
        substring: "@aws-sdk/client-sqs",
        broker_type: "sqs",
    },
    BrokerPattern {
        substring: "aws-sdk-go/service/sqs",
        broker_type: "sqs",
    },
    BrokerPattern {
        substring: "@aws-sdk/client-sns",
        broker_type: "sns",
    },
    BrokerPattern {
        substring: "aws-sdk-go/service/sns",
        broker_type: "sns",
    },
    BrokerPattern {
        substring: "eventbridge",
        broker_type: "eventbridge",
    },
    BrokerPattern {
        substring: "stepfunctions",
        broker_type: "stepfunctions",
    },
    // Kafka
    BrokerPattern {
        substring: "kafkajs",
        broker_type: "kafka",
    },
    BrokerPattern {
        substring: "rdkafka",
        broker_type: "kafka",
    },
    BrokerPattern {
        substring: "confluent.kafka",
        broker_type: "kafka",
    },
    BrokerPattern {
        substring: "sarama",
        broker_type: "kafka",
    },
    BrokerPattern {
        substring: "kafka",
        broker_type: "kafka",
    },
    // RabbitMQ / AMQP
    BrokerPattern {
        substring: "amqplib",
        broker_type: "rabbitmq",
    },
    BrokerPattern {
        substring: "amqp",
        broker_type: "rabbitmq",
    },
    BrokerPattern {
        substring: "rabbitmq",
        broker_type: "rabbitmq",
    },
    BrokerPattern {
        substring: "lapin",
        broker_type: "rabbitmq",
    },
    BrokerPattern {
        substring: "masstransit",
        broker_type: "rabbitmq",
    },
    BrokerPattern {
        substring: "pika",
        broker_type: "rabbitmq",
    },
    // Python task queues
    BrokerPattern {
        substring: "celery",
        broker_type: "celery",
    },
    BrokerPattern {
        substring: "dramatiq",
        broker_type: "celery",
    },
    BrokerPattern {
        substring: "huey",
        broker_type: "celery",
    },
    BrokerPattern {
        substring: "python-rq",
        broker_type: "rq",
    },
    BrokerPattern {
        substring: "rq.job",
        broker_type: "rq",
    },
    BrokerPattern {
        substring: "rq.worker",
        broker_type: "rq",
    },
    BrokerPattern {
        substring: "rq.queue",
        broker_type: "rq",
    },
    // Node.js task queues
    BrokerPattern {
        substring: "bullmq",
        broker_type: "bullmq",
    },
    BrokerPattern {
        substring: "bull",
        broker_type: "bullmq",
    },
    // Ruby / .NET task queues
    BrokerPattern {
        substring: "sidekiq",
        broker_type: "sidekiq",
    },
    BrokerPattern {
        substring: "resque",
        broker_type: "sidekiq",
    },
    BrokerPattern {
        substring: "goodjob",
        broker_type: "sidekiq",
    },
    BrokerPattern {
        substring: "hangfire",
        broker_type: "hangfire",
    },
    // Go task queue
    BrokerPattern {
        substring: "asynq",
        broker_type: "asynq",
    },
    // Temporal
    BrokerPattern {
        substring: "@temporalio",
        broker_type: "temporal",
    },
    BrokerPattern {
        substring: "temporalio",
        broker_type: "temporal",
    },
    BrokerPattern {
        substring: "temporal.client",
        broker_type: "temporal",
    },
    // Inngest
    BrokerPattern {
        substring: "inngest",
        broker_type: "inngest",
    },
    // NATS
    BrokerPattern {
        substring: "nats.go",
        broker_type: "nats",
    },
    BrokerPattern {
        substring: "nats",
        broker_type: "nats",
    },
    // Redis streams
    BrokerPattern {
        substring: "ioredis",
        broker_type: "redis",
    },
    // Elixir / Erlang
    BrokerPattern {
        substring: "oban",
        broker_type: "oban",
    },
    BrokerPattern {
        substring: "broadway",
        broker_type: "broadway",
    },
    BrokerPattern {
        substring: "genstage",
        broker_type: "broadway",
    },
    BrokerPattern {
        substring: "phoenix.pubsub",
        broker_type: "phoenix_pubsub",
    },
    // Scala / JVM
    BrokerPattern {
        substring: "alpakka",
        broker_type: "alpakka",
    },
];

/// Object/receiver-name patterns (case-insensitive substring match).
/// These are shorter identifiers typically seen as variable/object names in code,
/// e.g. `producer.send()`, `channel.basic_publish()`.
static OBJECT_PATTERNS: &[BrokerPattern] = &[
    // Kafka
    BrokerPattern {
        substring: "kafkaproducer",
        broker_type: "kafka",
    },
    BrokerPattern {
        substring: "kafkaconsumer",
        broker_type: "kafka",
    },
    BrokerPattern {
        substring: "kafka",
        broker_type: "kafka",
    },
    // RabbitMQ / AMQP
    BrokerPattern {
        substring: "rabbitmq",
        broker_type: "rabbitmq",
    },
    BrokerPattern {
        substring: "amqp",
        broker_type: "rabbitmq",
    },
    // Google Cloud
    BrokerPattern {
        substring: "pubsub",
        broker_type: "pubsub",
    },
    BrokerPattern {
        substring: "cloudtasks",
        broker_type: "cloud_tasks",
    },
    BrokerPattern {
        substring: "cloud_tasks",
        broker_type: "cloud_tasks",
    },
    // AWS
    BrokerPattern {
        substring: "sqsclient",
        broker_type: "sqs",
    },
    BrokerPattern {
        substring: "snsclient",
        broker_type: "sns",
    },
    BrokerPattern {
        substring: "eventbridge",
        broker_type: "eventbridge",
    },
    // Task queues
    BrokerPattern {
        substring: "celery",
        broker_type: "celery",
    },
    BrokerPattern {
        substring: "dramatiq",
        broker_type: "celery",
    },
    BrokerPattern {
        substring: "bullmq",
        broker_type: "bullmq",
    },
    BrokerPattern {
        substring: "bull",
        broker_type: "bullmq",
    },
    BrokerPattern {
        substring: "sidekiq",
        broker_type: "sidekiq",
    },
    BrokerPattern {
        substring: "asynq",
        broker_type: "asynq",
    },
    // Temporal
    BrokerPattern {
        substring: "temporal",
        broker_type: "temporal",
    },
    // Inngest
    BrokerPattern {
        substring: "inngest",
        broker_type: "inngest",
    },
    // NATS
    BrokerPattern {
        substring: "nats",
        broker_type: "nats",
    },
    // Redis
    BrokerPattern {
        substring: "ioredis",
        broker_type: "redis",
    },
];

/// Method names that indicate an async broker operation (publish / consume side).
static ASYNC_METHODS: &[&str] = &[
    "publish",
    "send",
    "send_message",
    "sendmessage",
    "produce",
    "dispatch",
    "enqueue",
    "push",
    "subscribe",
    "consume",
    "receive",
    "poll",
    "listen",
    // Python-specific
    "basic_publish",
    "basic_consume",
    "apply_async",
    "delay",
    "send_task",
];

/// Method names that indicate a plain HTTP call (used to disambiguate
/// when an object could be either broker or HTTP client).
static HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "head", "options", "request", "fetch",
];

/// Match an import path or qualified name against known broker patterns.
///
/// Uses case-insensitive substring matching (mirroring CBM's `lib_pattern_t`).
/// Returns `Some(BrokerMatch)` if the input matches a known async broker.
pub fn match_broker(import_or_qn: &str) -> Option<BrokerMatch> {
    let lower = import_or_qn.to_lowercase();
    for pat in IMPORT_PATTERNS {
        if lower.contains(pat.substring) {
            return Some(BrokerMatch {
                call_kind: "async",
                broker_type: pat.broker_type,
            });
        }
    }
    None
}

/// Match an object/receiver name against known broker patterns.
///
/// Used when we have `obj.method()` and want to check if `obj` is a broker client.
/// Only returns a match when the object name is **not** a common false-positive
/// (e.g. "channel" alone is too generic — we require an async method name in that case
/// via the caller combining this with `method_call_kind`).
pub fn match_broker_object(obj_name: &str) -> Option<BrokerMatch> {
    let lower = obj_name.to_lowercase();

    // Skip very generic single-word names that would cause false positives
    // unless they clearly identify a broker.
    if lower == "channel" || lower == "client" || lower == "conn" || lower == "connection" {
        return None;
    }

    for pat in OBJECT_PATTERNS {
        if lower.contains(pat.substring) {
            return Some(BrokerMatch {
                call_kind: "async",
                broker_type: pat.broker_type,
            });
        }
    }
    None
}

/// Given a method name, determine whether it indicates an async broker operation
/// or a plain HTTP call.
///
/// Returns `"async"` for broker methods, `"http"` for HTTP verbs, or `"unknown"`.
pub fn method_call_kind(method_name: &str) -> &'static str {
    let lower = method_name.to_lowercase();
    if ASYNC_METHODS.iter().any(|&m| lower == m) {
        return "async";
    }
    if HTTP_METHODS.iter().any(|&m| lower == m) {
        return "http";
    }
    "unknown"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_kafka_import() {
        let m = match_broker("kafkajs").unwrap();
        assert_eq!(m.broker_type, "kafka");
        assert_eq!(m.call_kind, "async");
    }

    #[test]
    fn match_aws_sqs_import() {
        let m = match_broker("@aws-sdk/client-sqs").unwrap();
        assert_eq!(m.broker_type, "sqs");
    }

    #[test]
    fn match_rabbitmq_via_pika() {
        let m = match_broker("pika").unwrap();
        assert_eq!(m.broker_type, "rabbitmq");
    }

    #[test]
    fn match_celery_import() {
        let m = match_broker("celery").unwrap();
        assert_eq!(m.broker_type, "celery");
    }

    #[test]
    fn match_pubsub_import() {
        let m = match_broker("google.cloud.pubsub_v1").unwrap();
        assert_eq!(m.broker_type, "pubsub");
    }

    #[test]
    fn match_temporal_import() {
        let m = match_broker("@temporalio/client").unwrap();
        assert_eq!(m.broker_type, "temporal");
    }

    #[test]
    fn no_match_for_http_library() {
        assert!(match_broker("axios").is_none());
        assert!(match_broker("requests").is_none());
    }

    #[test]
    fn match_broker_object_kafka() {
        let m = match_broker_object("kafkaProducer").unwrap();
        assert_eq!(m.broker_type, "kafka");
    }

    #[test]
    fn no_match_generic_client() {
        assert!(match_broker_object("client").is_none());
        assert!(match_broker_object("channel").is_none());
    }

    #[test]
    fn method_kinds() {
        assert_eq!(method_call_kind("publish"), "async");
        assert_eq!(method_call_kind("send"), "async");
        assert_eq!(method_call_kind("basic_publish"), "async");
        assert_eq!(method_call_kind("get"), "http");
        assert_eq!(method_call_kind("post"), "http");
        assert_eq!(method_call_kind("someMethod"), "unknown");
    }

    // --- rq false-positive regression tests ---

    #[test]
    fn no_match_rq_false_positives() {
        // These should NOT match "rq" broker
        assert!(match_broker("jquery").is_none());
        assert!(match_broker("requirements").is_none());
        assert!(match_broker("torque").is_none());
    }

    #[test]
    fn match_rq_precise_patterns() {
        // These should match the precise rq patterns
        let m = match_broker("python-rq").unwrap();
        assert_eq!(m.broker_type, "rq");
        let m = match_broker("rq.job").unwrap();
        assert_eq!(m.broker_type, "rq");
        let m = match_broker("rq.worker").unwrap();
        assert_eq!(m.broker_type, "rq");
        let m = match_broker("rq.queue").unwrap();
        assert_eq!(m.broker_type, "rq");
    }

    // --- channel false-positive regression tests ---

    #[test]
    fn no_match_channel_false_positives() {
        // "channel" pattern removed from OBJECT_PATTERNS to avoid false positives;
        // bare "channel" was already blocked, but "myChannel", "channelManager" etc.
        // would still match via substring.  Genuine rabbitmq objects are caught by
        // the "amqp" / "rabbitmq" patterns instead.
        assert!(match_broker_object("channel").is_none());
        assert!(match_broker_object("myChannel").is_none());
        assert!(match_broker_object("channelManager").is_none());
        assert!(match_broker_object("dataChannel").is_none());
    }
}
