//! CLI harness for the AT-SPI2 focus/inject proof-of-concept.
//!
//! Subcommands:
//!   watch                   Print every focus change (Ctrl-C to exit).
//!   context [max_chars]     Print a FocusContext snapshot after a short
//!                           settling delay so you can switch focus first.
//!   inject <text>           Insert <text> at the caret of the focused widget
//!                           (after a 3s delay so you can focus it).
//!   demo                    Watch focus for 3s, dump context, then inject
//!                           a marker string.

mod context;

use std::time::Duration;

use anyhow::{bail, Context, Result};
use context::FocusTracker;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_else(|| "demo".to_string());

    let tracker = FocusTracker::connect().await?;
    tracker.start().await?;

    match cmd.as_str() {
        "watch" => run_watch(&tracker).await,
        "context" => {
            let max = args
                .next()
                .map(|s| s.parse::<i32>())
                .transpose()
                .context("max_chars must be an integer")?
                .unwrap_or(500);
            run_context(&tracker, max).await
        }
        "inject" => {
            let text: String = args.collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                bail!("usage: atspi-context inject <text...>");
            }
            run_inject(&tracker, &text).await
        }
        "demo" => run_demo(&tracker).await,
        other => bail!(
            "unknown subcommand `{other}`; expected one of: watch | context | inject | demo"
        ),
    }
}

async fn run_watch(tracker: &FocusTracker) -> Result<()> {
    eprintln!("Watching focus changes — switch windows / click text fields. Ctrl-C to quit.");
    let mut last_path = String::new();
    loop {
        if let Some(obj) = tracker.focused_ref().await {
            let path = obj.path().to_string();
            if path != last_path {
                last_path = path.clone();
                let bus = obj.name().map(|n| n.to_string()).unwrap_or_default();
                println!("focus → bus={bus} path={path}");
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn run_context(tracker: &FocusTracker, max_chars: i32) -> Result<()> {
    eprintln!("Waiting 3s — click into a text field now…");
    tokio::time::sleep(Duration::from_secs(3)).await;
    match tracker.get_focused_context(max_chars).await {
        Some(ctx) => {
            println!("app:        {}", ctx.app_name);
            println!("role:       {}", ctx.role_name);
            println!("offset:     {}", ctx.cursor_offset);
            println!("is_code:    {}", ctx.is_code_context);
            println!("--- surrounding text (len={}): ---", ctx.surrounding_text.len());
            println!("{}", ctx.surrounding_text);
            println!("--- end ---");
        }
        None => println!("(no focused widget tracked yet)"),
    }
    Ok(())
}

async fn run_inject(tracker: &FocusTracker, text: &str) -> Result<()> {
    eprintln!("Waiting 3s — click into an editable field now…");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let ok = tracker.inject_text(text).await?;
    println!("inject_text → {ok}");
    Ok(())
}

async fn run_demo(tracker: &FocusTracker) -> Result<()> {
    eprintln!("Demo: focus a text field within 3s…");
    tokio::time::sleep(Duration::from_secs(3)).await;
    if let Some(ctx) = tracker.get_focused_context(200).await {
        println!("--- context before inject ---");
        println!("app={}  role={}  offset={}  is_code={}",
            ctx.app_name, ctx.role_name, ctx.cursor_offset, ctx.is_code_context);
        println!("text-before-caret: {:?}", ctx.surrounding_text);
    } else {
        println!("(no focused widget yet — make sure at-spi2-registryd is running and a11y is enabled)");
        return Ok(());
    }
    let marker = "[voxctr-rust-poc] ";
    let ok = tracker.inject_text(marker).await?;
    println!("--- inject ---");
    println!("inserted {:?} → success={ok}", marker);
    if let Some(ctx) = tracker.get_focused_context(200).await {
        println!("--- context after inject ---");
        println!("offset={}  text-before-caret={:?}", ctx.cursor_offset, ctx.surrounding_text);
    }
    Ok(())
}
