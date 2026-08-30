//! `forge serve`: the queue for this project, with the MCP tools on the
//! same port.
//!
//! One process holds `out/serve/daemon.lock`, one listener on `127.0.0.1`,
//! one endpoint file naming the port and the bearer token, and one worker
//! behind the card lease. A second `forge serve` in the same project exits
//! naming the first.
//!
//! `/mcp` is nested onto the daemon's own router, so the tools an agent
//! drives over HTTP are the same router `forge mcp` serves over stdio, over
//! the same queue. Same tools, same instructions text, same frames,
//! whichever door the client came through.

use std::sync::Arc;

use forge_library::Project;
use forge_serve::{JobStore, LocalQueue, LocalQueueOptions, daemon, discovery};

use crate::cli::ServeArgs;
use crate::outcome::{Failure, Outcome};

/// Run, stop or describe the daemon.
pub(crate) fn run(project: &Project, args: &ServeArgs) -> Outcome {
    if args.stop {
        return stop(project);
    }
    if args.status {
        return status(project);
    }
    serve(project, args)
}

/// End the daemon serving this project.
pub(crate) fn stop(project: &Project) -> Outcome {
    let Some(remote) = discovery::find(project) else {
        println!("no daemon is serving {}", project.root.display());
        return Ok(());
    };
    remote
        .stop()
        .map_err(|e| Failure::failed(format!("the daemon would not stop: {e}")))?;
    println!("stopped the daemon on {}", remote.url());
    Ok(())
}

/// Say whether a daemon is up and what it is doing.
fn status(project: &Project) -> Outcome {
    let store = JobStore::open(&project.root).map_err(|e| Failure::failed(e.to_string()))?;
    let Some(remote) = discovery::find(project) else {
        println!("daemon    down for {}", project.root.display());
        if store.daemon().is_some() {
            println!(
                "note      out/serve/daemon.json named a process that is not there; it was removed"
            );
        }
        println!("note      every forge gen still runs, in the process that asks for it");
        return Ok(());
    };
    let health = remote
        .health(std::time::Duration::from_secs(2))
        .map_err(|e| Failure::failed(e.to_string()))?;
    println!("daemon    up on {}", remote.url());
    if let Some(pid) = health.get("pid") {
        println!("pid       {pid}");
    }
    if let Some(started) = health.get("started").and_then(serde_json::Value::as_str) {
        println!("started   {started}");
    }
    crate::commands::jobs::print_queue(project)
}

/// Serve until stopped.
fn serve(project: &Project, args: &ServeArgs) -> Outcome {
    let store = JobStore::open(&project.root).map_err(|e| Failure::failed(e.to_string()))?;
    // The lock before the listener: a second daemon must not take a port
    // and then find out it is the second.
    let lock = daemon::DaemonLock::take(&store).map_err(|e| Failure::refused(e.to_string()))?;

    let queue = LocalQueue::open(
        project,
        LocalQueueOptions {
            forge: std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("forge")),
            ..LocalQueueOptions::default()
        },
    )
    .map_err(|e| Failure::failed(e.to_string()))?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Failure::failed(format!("cannot start the async runtime: {e}")))?;

    let foreground = args.foreground || std::io::IsTerminal::is_terminal(&std::io::stdout());
    let idle_exit = args.idle_exit;
    let serve_mcp = !args.no_mcp;
    let port = args.port;
    let project_root = project.root.clone();
    let mcp_config = if serve_mcp {
        Some(
            forge_mcp::Config::for_project(project.clone())
                .map_err(|e| Failure::refused(e.to_string()))?,
        )
    } else {
        None
    };

    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| Failure::failed(format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
        let address = listener
            .local_addr()
            .map_err(|e| Failure::failed(format!("the listener has no address: {e}")))?;
        let facts = daemon::announce(
            &store,
            &project_root,
            address,
            &daemon::token(),
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|e| Failure::failed(e.to_string()))?;

        let mut app = forge_serve::http::router(daemon::state(Arc::clone(&queue), &facts));
        if let Some(config) = mcp_config {
            // The router does not move and is not duplicated: forge_mcp
            // hands back its own tools as a tower service, holding the same
            // Arc<dyn Queue> the daemon's worker is draining.
            app = app.nest_service(
                "/mcp",
                forge_mcp::http_service(config, Arc::clone(&queue) as Arc<dyn forge_serve::Queue>),
            );
        }

        eprintln!(
            "forge serve {} on {} for {}{}",
            env!("CARGO_PKG_VERSION"),
            facts.url,
            project_root.display(),
            if serve_mcp { ", MCP at /mcp" } else { "" }
        );
        if !foreground {
            eprintln!("out/serve/daemon.json holds the port and the token (mode 0600)");
        }

        let idle = idle_watch(Arc::clone(&queue), idle_exit);
        let result = tokio::select! {
            served = axum::serve(listener, app).into_future() => served
                .map_err(|e| Failure::failed(format!("the daemon stopped: {e}"))),
            () = shutdown() => {
                eprintln!("forge serve: stopping");
                Ok(())
            }
            () = idle => {
                eprintln!("forge serve: idle, stopping");
                Ok(())
            }
        };
        queue.stop();
        drop(lock);
        result
    })
}

/// Resolve when the queue has been empty for `seconds`, or never.
async fn idle_watch(queue: Arc<LocalQueue>, seconds: Option<u64>) {
    let Some(seconds) = seconds else {
        std::future::pending::<()>().await;
        return;
    };
    let mut idle_since = std::time::Instant::now();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let busy = forge_serve::Queue::status(queue.as_ref()).is_ok_and(|status| {
            status.queue.queued + status.queue.blocked + status.queue.running > 0
        });
        if busy {
            idle_since = std::time::Instant::now();
        } else if idle_since.elapsed().as_secs() >= seconds {
            return;
        }
    }
}

/// Resolve on ^C or SIGTERM.
async fn shutdown() {
    let interrupt = tokio::signal::ctrl_c();
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = interrupt.await;
        return;
    };
    tokio::select! {
        _ = interrupt => {}
        _ = terminate.recv() => {}
    }
}
