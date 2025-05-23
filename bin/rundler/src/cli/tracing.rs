// This file is part of Rundler.
//
// Rundler is free software: you can redistribute it and/or modify it under the
// terms of the GNU Lesser General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later version.
//
// Rundler is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
// without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
// See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with Rundler.
// If not, see https://www.gnu.org/licenses/.

use std::{io, time::Duration};

use opentelemetry::{global, trace::TracerProvider, KeyValue};
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use tonic::metadata::MetadataMap;
pub use tracing::*;
use tracing::{subscriber, subscriber::Interest, Metadata, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_log::LogTracer;
use tracing_subscriber::{layer::SubscriberExt, EnvFilter, FmtSubscriber, Layer};

use super::LogsArgs;

pub fn configure_logging(
    network: &Option<String>,
    config: &LogsArgs,
) -> anyhow::Result<WorkerGuard> {
    let (appender, guard) = if let Some(log_file) = &config.file {
        tracing_appender::non_blocking(tracing_appender::rolling::never(".", log_file))
    } else {
        tracing_appender::non_blocking(io::stdout())
    };

    let subscriber_builder = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(appender);

    if config.json {
        let subscriber = subscriber_builder
            .json()
            .finish()
            .with(TargetBlacklistLayer);
        if let Some(endpoint) = &config.otlp_grpc_endpoint {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_tonic()
                .with_endpoint(endpoint)
                .with_timeout(Duration::from_secs(3))
                .with_metadata(MetadataMap::new())
                .build()?;

            let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(
                    Resource::builder()
                        .with_service_name("rundler")
                        .with_attributes([KeyValue::new(
                            "network",
                            network.clone().unwrap_or_default(),
                        )])
                        .build(),
                )
                .build();

            global::set_tracer_provider(tracer_provider.clone());
            let tracer = tracer_provider.tracer("rundler-tracer");

            let otlp_subscriber =
                subscriber.with(tracing_opentelemetry::layer().with_tracer(tracer));
            subscriber::set_global_default(otlp_subscriber)?;
        } else {
            subscriber::set_global_default(subscriber)?;
        };
    } else {
        subscriber::set_global_default(
            subscriber_builder
                .pretty()
                .finish()
                .with(TargetBlacklistLayer),
        )?;
    }

    LogTracer::init()?;

    Ok(guard)
}

const BLACKLISTED_TARGETS: &[&str] = &["h2", "hyper", "tower::buffer"];

struct TargetBlacklistLayer;

impl<S: Subscriber> Layer<S> for TargetBlacklistLayer {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        let matches_blacklist = BLACKLISTED_TARGETS
            .iter()
            .any(|target| metadata.target().starts_with(target));
        if matches_blacklist {
            Interest::never()
        } else {
            Interest::always()
        }
    }
}
