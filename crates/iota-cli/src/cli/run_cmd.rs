use anyhow::Result;

use iota_core::acp;
use iota_core::config;
use iota_core::engine::IotaEngine;
use iota_core::resources::LocalResources;

pub(super) async fn run_direct(options: &acp::AcpRunOptions) -> Result<()> {
    let config = config::read_config()?;
    if options.multi_backend {
        // Run all backends and collect results.
        use tokio::spawn;
        let mut handles = Vec::new();
        for backend in acp::ALL_BACKENDS {
            let backend_name = backend.to_string();
            let config = config.clone();
            let cwd = options.cwd.clone();
            let mut engine = IotaEngine::create_session_with_resources(
                config,
                LocalResources::from_workspace(cwd.clone()),
                options.show_native,
                options.timeout_ms,
                None,
            );
            let prompt = options.prompt.clone();
            let timing = options.timing;
            handles.push(spawn(async move {
                let result = engine.run_with_timing(backend, cwd, &prompt).await;
                engine.shutdown().await;
                (backend_name, result, timing)
            }));
        }
        for handle in handles {
            let (backend_name, result, timing) = handle.await?;
            match result {
                Ok(output) => {
                    if timing {
                        super::print_route_timing(
                            "direct",
                            acp::AcpBackend::parse(&backend_name).unwrap_or(acp::AcpBackend::Codex),
                            Some(&output.timing),
                        );
                    }
                    let text = output.text;
                    if !text.is_empty() {
                        println!("[{}] {}", backend_name, text);
                    }
                }
                Err(e) => eprintln!("[{}] Error: {}", backend_name, e),
            }
        }
    } else {
        let mut engine = IotaEngine::create_session_with_resources(
            config,
            LocalResources::from_workspace(options.cwd.clone()),
            options.show_native,
            options.timeout_ms,
            None,
        );
        let result = engine
            .run_with_timing(options.backend, options.cwd.clone(), &options.prompt)
            .await;
        engine.shutdown().await;
        let output = result?;
        if options.log_events {
            for event in &output.events {
                eprintln!("{}", serde_json::to_string(event).unwrap_or_default());
            }
        }
        if options.timing {
            super::print_route_timing("direct", options.backend, Some(&output.timing));
        }
        let text = output.text;
        if !text.is_empty() {
            println!("{}", text);
        }
    }
    Ok(())
}
