use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use std::sync::OnceLock;

pub struct IotaMetrics {
    pub execution_count: Counter<u64>,
    pub prompt_queued: UpDownCounter<i64>,
    pub token_usage_count: Counter<u64>,
    pub token_input: Counter<u64>,
    pub token_output: Counter<u64>,
    pub token_total: Counter<u64>,
    pub prompt_duration: Histogram<f64>,
    pub init_duration: Histogram<f64>,
    /// Writes that failed against an auxiliary (non-authoritative) store and
    /// were downgraded to a `degraded` event instead of failing the request.
    pub storage_degraded: Counter<u64>,
    /// Time spent waiting to acquire a pooled SQLite connection.
    pub db_lock_wait: Histogram<f64>,
    /// Statements whose execution exceeded the slow-query threshold.
    pub db_slow_query: Histogram<f64>,
    /// Reads served by the writer connection because no reader was available.
    pub db_reader_fallback: Counter<u64>,
    /// Engines evicted from the daemon's `EnginePool` to respect its bounds.
    pub pool_eviction: Counter<u64>,
    /// Turns that waited for their workspace's execution slot.
    pub pool_queue_wait: Histogram<f64>,
    /// Engines currently held by the daemon engine pool.
    pub pool_size: opentelemetry::metrics::Gauge<u64>,
    /// Estimated tokens spent per `<iota-context>` section.
    pub context_section_tokens: Histogram<f64>,
    /// Sections that had to leave content out to fit their budget.
    pub context_section_trimmed: Counter<u64>,
    /// Estimated tokens spent on the whole injected capsule.
    pub context_total_tokens: Histogram<f64>,
    /// Kanban sync requests rejected before doing any work.
    pub sync_rejection: Counter<u64>,
    /// `iota-fun` executions terminated for exceeding a resource limit.
    pub sandbox_limit: Counter<u64>,
}

static METRICS: OnceLock<IotaMetrics> = OnceLock::new();

pub fn get() -> &'static IotaMetrics {
    METRICS.get_or_init(|| {
        let meter = global::meter("iota");
        let buckets = vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];
        let token_buckets = vec![
            16.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0,
        ];

        IotaMetrics {
            execution_count: meter
                .u64_counter("iota.execution.count")
                .with_unit("{execution}")
                .with_description("Total execution count by status")
                .build(),
            prompt_queued: meter
                .i64_up_down_counter("iota.prompt.queued")
                .with_unit("{prompt}")
                .with_description("Queued prompts")
                .build(),
            token_usage_count: meter
                .u64_counter("iota.token.usage.count")
                .with_unit("{event}")
                .with_description("Token usage event count")
                .build(),
            token_input: meter
                .u64_counter("iota.token.input")
                .with_unit("{token}")
                .with_description("Input tokens consumed")
                .build(),
            token_output: meter
                .u64_counter("iota.token.output")
                .with_unit("{token}")
                .with_description("Output tokens produced")
                .build(),
            token_total: meter
                .u64_counter("iota.token.total")
                .with_unit("{token}")
                .with_description("Total tokens")
                .build(),
            prompt_duration: meter
                .f64_histogram("iota.prompt.duration")
                .with_unit("s")
                .with_description("Prompt processing duration")
                .with_boundaries(buckets.clone())
                .build(),
            init_duration: meter
                .f64_histogram("iota.init.duration")
                .with_unit("s")
                .with_description("ACP initialization duration")
                .with_boundaries(buckets.clone())
                .build(),
            storage_degraded: meter
                .u64_counter("iota.storage.degraded")
                .with_unit("{event}")
                .with_description("Auxiliary store writes downgraded to a degraded event")
                .build(),
            db_lock_wait: meter
                .f64_histogram("iota.db.lock_wait")
                .with_unit("s")
                .with_description("Time waiting for a pooled SQLite connection")
                .with_boundaries(buckets.clone())
                .build(),
            db_slow_query: meter
                .f64_histogram("iota.db.slow_query")
                .with_unit("s")
                .with_description("SQLite statements exceeding the slow-query threshold")
                .with_boundaries(buckets.clone())
                .build(),
            db_reader_fallback: meter
                .u64_counter("iota.db.reader_fallback")
                .with_unit("{read}")
                .with_description(
                    "Reads served by the writer because no reader connection was available",
                )
                .build(),
            pool_eviction: meter
                .u64_counter("iota.pool.eviction")
                .with_unit("{engine}")
                .with_description("Engines evicted from the daemon engine pool")
                .build(),
            pool_queue_wait: meter
                .f64_histogram("iota.pool.queue_wait")
                .with_unit("s")
                .with_description("Time a turn waited for its workspace execution slot")
                .with_boundaries(buckets.clone())
                .build(),
            pool_size: meter
                .u64_gauge("iota.pool.size")
                .with_unit("{engine}")
                .with_description("Engines currently held by the daemon engine pool")
                .build(),
            context_section_tokens: meter
                .f64_histogram("iota.context.section_tokens")
                .with_unit("{token}")
                .with_description("Estimated tokens per injected context section")
                .with_boundaries(token_buckets.clone())
                .build(),
            context_section_trimmed: meter
                .u64_counter("iota.context.section_trimmed")
                .with_unit("{section}")
                .with_description("Context sections that dropped content to fit their budget")
                .build(),
            context_total_tokens: meter
                .f64_histogram("iota.context.total_tokens")
                .with_unit("{token}")
                .with_description("Estimated tokens of the whole injected context capsule")
                .with_boundaries(token_buckets)
                .build(),
            sync_rejection: meter
                .u64_counter("iota.sync.rejection")
                .with_unit("{request}")
                .with_description("Kanban sync requests rejected before processing")
                .build(),
            sandbox_limit: meter
                .u64_counter("iota.sandbox.limit")
                .with_unit("{execution}")
                .with_description("iota-fun executions stopped by a resource limit")
                .build(),
        }
    })
}

impl IotaMetrics {
    /// Records a write downgraded against an auxiliary store.
    pub fn record_storage_degraded(&self, store: &str, category: &str) {
        self.storage_degraded.add(
            1,
            &[
                opentelemetry::KeyValue::new("store", store.to_string()),
                opentelemetry::KeyValue::new("category", category.to_string()),
            ],
        );
    }

    pub fn record_db_lock_wait(&self, seconds: f64, label: &str, kind: &str) {
        self.db_lock_wait.record(
            seconds,
            &[
                opentelemetry::KeyValue::new("statement", label.to_string()),
                opentelemetry::KeyValue::new("kind", kind.to_string()),
            ],
        );
    }

    pub fn record_db_slow_query(&self, seconds: f64, label: &str, kind: &str) {
        self.db_slow_query.record(
            seconds,
            &[
                opentelemetry::KeyValue::new("statement", label.to_string()),
                opentelemetry::KeyValue::new("kind", kind.to_string()),
            ],
        );
    }

    /// Records a read that fell back to the writer connection.
    pub fn record_db_reader_fallback(&self, reason: &str) {
        self.db_reader_fallback.add(
            1,
            &[opentelemetry::KeyValue::new("reason", reason.to_string())],
        );
    }

    pub fn record_pool_eviction(&self, reason: &str) {
        self.pool_eviction.add(
            1,
            &[opentelemetry::KeyValue::new("reason", reason.to_string())],
        );
    }

    pub fn record_pool_queue_wait(&self, seconds: f64) {
        self.pool_queue_wait.record(seconds, &[]);
    }

    pub fn record_pool_size(&self, engines: usize) {
        self.pool_size.record(engines as u64, &[]);
    }

    /// Records the token cost of one composed context section.
    pub fn record_context_section_tokens(&self, section: &str, tokens: usize, trimmed: bool) {
        let attrs = [opentelemetry::KeyValue::new("section", section.to_string())];
        self.context_section_tokens.record(tokens as f64, &attrs);
        if trimmed {
            self.context_section_trimmed.add(1, &attrs);
        }
    }

    /// Records the token cost of a whole composed capsule.
    pub fn record_context_total_tokens(&self, tokens: usize) {
        self.context_total_tokens.record(tokens as f64, &[]);
    }

    pub fn record_sync_rejection(&self, reason: &str) {
        self.sync_rejection.add(
            1,
            &[opentelemetry::KeyValue::new("reason", reason.to_string())],
        );
    }

    pub fn record_sandbox_limit(&self, limit: &str) {
        self.sandbox_limit.add(
            1,
            &[opentelemetry::KeyValue::new("limit", limit.to_string())],
        );
    }
}
