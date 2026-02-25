//! summit-ctl — command-line interface for the Summit daemon.

use anyhow::{Context, Result};

mod cmd;

const DEFAULT_PORT: u16 = 9001;

fn print_usage() {
    println!("Usage: summit-ctl [--port <port>] <command>");
    println!();
    println!("Daemon");
    println!("  shutdown                        Gracefully shut down the daemon");
    println!("  status                          Sessions, cache, and peer summary");
    println!("  services                        Show enabled/disabled services");
    println!();
    println!("Peers & Sessions");
    println!("  peers                           List discovered peers with trust status");
    println!("  sessions drop <id>              Drop a specific session");
    println!("  sessions inspect <id>           Show detailed session info");
    println!();
    println!("Trust");
    println!("  trust list                      Show trust rules");
    println!("  trust add <pubkey>              Trust a peer (flushes buffered chunks)");
    println!("  trust block <pubkey>            Block a peer");
    println!("  trust pending                   Untrusted peers with buffered chunks");
    println!();
    println!("File Transfer");
    println!("  send <file>                     Broadcast file to all trusted peers");
    println!("  send <file> --peer <pubkey>     Send file to specific peer");
    println!("  send <file> --session <id>      Send file to specific session");
    println!("  files                           List received and in-progress files");
    println!();
    println!("Messaging");
    println!("  messages <pubkey>               List messages from a peer");
    println!("  messages send <pubkey> <text>   Send a text message to a peer");
    println!();
    println!("Compute");
    println!("  compute tasks                   List all compute tasks");
    println!("  compute tasks <pubkey>          List compute tasks from a specific peer");
    println!("  compute submit <pubkey> -- <cmd>  Submit a shell command to a peer");
    println!("  compute submit <pubkey> <json>    Submit a JSON task payload");
    println!();
    println!("Cache & Schema");
    println!("  cache                           Show cache statistics");
    println!("  cache clear                     Clear the chunk cache");
    println!("  schema list                     List all known schemas");
    println!();
    println!(
        "Options:\n  --port <port>                   API port (default: {})",
        DEFAULT_PORT
    );
    println!();
    println!("Examples:");
    println!("  summit-ctl status");
    println!("  summit-ctl services");
    println!("  summit-ctl trust add 5c8c7d3c9eff6572...");
    println!("  summit-ctl send document.pdf");
    println!("  summit-ctl send photo.jpg --peer 99b1db0b1849c7f8...");
    println!("  summit-ctl messages send 99b1db0b... 'hello world'");
    println!("  summit-ctl compute submit 99b1db0b... -- uname -a");
    println!("  summit-ctl compute submit 99b1db0b... -- hostnamectl > info.txt");
    println!("  summit-ctl compute tasks");
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Parse --port option
    let mut port = DEFAULT_PORT;
    let mut remaining: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--port" {
            i += 1;
            port = args
                .get(i)
                .context("--port requires a value")?
                .parse()
                .context("--port must be a number")?;
        } else {
            remaining.push(args[i].clone());
        }
        i += 1;
    }

    let remaining_refs: Vec<&str> = remaining.iter().map(|s| s.as_str()).collect();

    // Handle send command with optional targeting
    if remaining_refs.first() == Some(&"send") && remaining_refs.len() >= 2 {
        let path = remaining_refs[1];
        let mut target_peer = None;
        let mut target_session = None;

        let mut i = 2;
        while i < remaining_refs.len() {
            match remaining_refs[i] {
                "--peer" => {
                    i += 1;
                    target_peer = remaining_refs.get(i).copied();
                }
                "--session" => {
                    i += 1;
                    target_session = remaining_refs.get(i).copied();
                }
                _ => {
                    anyhow::bail!("Unknown option: {}", remaining_refs[i]);
                }
            }
            i += 1;
        }

        return cmd::files::cmd_send(port, path, target_peer, target_session).await;
    }

    // Handle: compute submit <pubkey> -- <shell command...>
    if remaining_refs.len() >= 4
        && remaining_refs[0] == "compute"
        && remaining_refs[1] == "submit"
        && let Some(sep) = remaining_refs.iter().position(|s| *s == "--")
    {
        let to = remaining_refs[2];
        let shell_cmd = remaining[sep + 1..].join(" ");
        let payload = serde_json::json!({ "run": shell_cmd }).to_string();
        return cmd::compute::cmd_compute_submit(port, to, &payload).await;
    }

    match remaining_refs.as_slice() {
        ["shutdown"] => cmd::status::cmd_shutdown(port).await,
        ["status"] | [] => cmd::status::cmd_status(port).await,
        ["services"] => cmd::status::cmd_services(port).await,
        ["peers"] => cmd::status::cmd_peers(port).await,
        ["sessions", "drop", id] => cmd::sessions::cmd_session_drop(port, id).await,
        ["sessions", "inspect", id] => cmd::sessions::cmd_session_inspect(port, id).await,
        ["cache"] => cmd::status::cmd_cache(port).await,
        ["cache", "clear"] => cmd::status::cmd_cache_clear(port).await,
        ["files"] => cmd::files::cmd_files(port).await,
        ["trust", "list"] | ["trust"] => cmd::trust::cmd_trust_list(port).await,
        ["trust", "add", pubkey] => cmd::trust::cmd_trust_add(port, pubkey).await,
        ["trust", "block", pubkey] => cmd::trust::cmd_trust_block(port, pubkey).await,
        ["trust", "pending"] => cmd::trust::cmd_trust_pending(port).await,
        ["messages", peer] => cmd::messages::cmd_messages(port, peer).await,
        ["messages", "send", to, text] => cmd::messages::cmd_messages_send(port, to, text).await,
        ["compute", "tasks"] => cmd::compute::cmd_compute_tasks_all(port).await,
        ["compute", "tasks", peer] => cmd::compute::cmd_compute_tasks(port, peer).await,
        ["compute", "submit", to, payload] => {
            cmd::compute::cmd_compute_submit(port, to, payload).await
        }
        ["schema", "list"] | ["schema"] => cmd::status::cmd_schema_list(port).await,
        ["help"] | ["--help"] | ["-h"] => {
            print_usage();
            Ok(())
        }
        other => {
            eprintln!("Unknown command: {}", other.join(" "));
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }
}
