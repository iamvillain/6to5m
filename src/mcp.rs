//! `6ixpack mcp`: a stdio MCP server (JSON-RPC 2.0, one message per line).
//! Each tool runs this same executable as a child process with the given
//! arguments, so command output never mixes with the protocol stream.

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::process::Command;

const MAX_OUT: usize = 200_000;

const TOOLS: &[(&str, &str, &str)] = &[
    ("extract", "", "Scan an RPF9 archive and extract its payloads: args like [\"-n\", \"levels.rpf\"] or [\"levels.rpf\", \"outdir\"]. Pass [\"--help\"] for options."),
    ("audio", "audio", "AWC/ADAT audio: list banks, extract Opus, Ogg or WAV. Pass [\"--help\"] for options."),
    ("texture", "texture", "RAGE textures and TTPF stores to DDS/PNG. Pass [\"--help\"] for options."),
    ("model", "model", "RAGE drawables to OBJ. Pass [\"--help\"] for options."),
    ("convert", "convert", "Convert to GTA V formats: ytd (DDS dir), ydr/ybn (OBJ), ymap/ytyp (JSON). Pass [] for usage."),
    ("sixng", "sixng", "SixNG commands: placements, vehicles, GLB export, ttpf, names, interiors, movies, ... Pass [\"--help\"] (or [\"<command>\", \"--help\"]) for the list."),
    ("bcpack_calibrate", "bcpack-calibrate", "Fit the BCPack BC7/BC6H preset tables to the install's TTPF stores and write the calibrated table file the decoders load (else they use the shipped v1 tables): args [\"--game\", \"<6m dir>\"], optional [\"--out\", \"<file>\"]. Long run."),
    ("colour_textures", "colour-textures", "Export every GTA VI colour texture (albedo, detail, emissive; no normal, height or data maps) as top-mip PNGs: args [\"--game\", \"<6m dir>\", \"--out\", \"<dir>\"]. Long run."),
    ("redm", "redm", "RDR2 / RedM output (RSC8): [\"convert\", \"ytd\", \"<dds-dir>\", \"<out.ytd>\"], [\"convert\", \"ydr\", \"<in.obj>\", \"<out.ydr>\"], [\"prop\", \"<6m name>\", \"--game\", \"<dir>\", \"--out\", \"<resource>\", \"--work\", \"<dir>\"] (one GTA VI prop -> RedM resource; --tint-row <n> --as <model> for a colour variant), [\"prop\", \"<6m name>\", \"--tints\", \"--game\", \"<dir>\"] (list tint rows), [\"prop\", \"--list\", \"<pattern>\"], [\"check\", \"<file>\"]. Pass [] for usage."),
    ("fivem", "fivem", "Build a ready FiveM resource from GTA VI: args like [\"build\", \"--game\", \"<dir>\", \"--out\", \"<res>\", \"--bbox\", \"x0,y0,x1,y1\"]. Pass [\"--help\"] for options."),
];

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            send(&mut out, json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}))?;
            continue;
        };
        let Some(id) = msg.get("id").cloned() else { continue }; // notification
        let method = msg["method"].as_str().unwrap_or("");
        let reply = match method {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "6ixpack", "version": env!("CARGO_PKG_VERSION")}
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": TOOLS.iter().map(|(name, _, desc)| json!({
                "name": name,
                "description": desc,
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "args": {"type": "array", "items": {"type": "string"}, "description": "Command-line arguments"},
                        "threads": {"type": "integer", "description": "Worker threads (default: all cores)"}
                    },
                    "required": ["args"]
                }
            })).collect::<Vec<_>>()})),
            "tools/call" => call(&msg["params"], &mut out),
            _ => Err((-32601, format!("unknown method {method}"))),
        };
        let resp = match reply {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        };
        send(&mut out, resp)?;
    }
    Ok(())
}

fn send(out: &mut impl Write, v: Value) -> std::io::Result<()> {
    writeln!(out, "{v}")?;
    out.flush()
}

/// Runs a tool. With `_meta.progressToken`, the child's progress lines
/// (`{"event":"progress","stage","done","total","pct"}` on stderr, as from
/// `fivem build` and `sixng index`) are forwarded as
/// `notifications/progress` while it runs and left out of the output.
fn call(params: &Value, out: &mut impl Write) -> Result<Value, (i64, String)> {
    let name = params["name"].as_str().unwrap_or("");
    let (_, sub, _) = TOOLS.iter().find(|t| t.0 == name).ok_or((-32602, format!("unknown tool {name}")))?;
    let args: Vec<String> = params["arguments"]["args"]
        .as_array()
        .ok_or((-32602, "args must be an array of strings".to_string()))?
        .iter()
        .map(|a| a.as_str().map(str::to_string).ok_or((-32602, "args must be strings".to_string())))
        .collect::<Result<_, _>>()?;
    let exe = std::env::current_exe().map_err(|e| (-32603, e.to_string()))?;
    let mut cmd = Command::new(exe);
    if let Some(n) = params["arguments"]["threads"].as_u64() {
        cmd.arg("--threads").arg(n.to_string());
    }
    if !sub.is_empty() {
        cmd.arg(sub);
    }
    use std::process::Stdio;
    let token = params["_meta"]["progressToken"].clone();
    let mut child = cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| (-32603, e.to_string()))?;
    let mut so = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(&mut so, &mut b);
        b
    });
    let mut err = String::new();
    // progress must increase per notification while stages restart at 0:
    // a running count, with the stage's own numbers in the message
    let mut n = 0u64;
    for line in std::io::BufReader::new(child.stderr.take().unwrap()).lines().map_while(Result::ok) {
        let ev = serde_json::from_str::<Value>(&line).ok().filter(|v| v["event"] == "progress");
        match ev {
            Some(v) if !token.is_null() => {
                n += 1;
                let msg = format!("{} {}/{} ({}%)", v["stage"].as_str().unwrap_or(""), v["done"], v["total"], v["pct"]);
                let _ = send(out, json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progressToken": token, "progress": n, "message": msg}}));
            }
            Some(_) => {}
            None => {
                err.push_str(&line);
                err.push('\n');
            }
        }
    }
    let status = child.wait().map_err(|e| (-32603, e.to_string()))?;
    let stdout = reader.join().unwrap_or_default();
    let mut text = String::from_utf8_lossy(&stdout).into_owned();
    if !err.trim().is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&err);
    }
    if text.len() > MAX_OUT {
        let cut = (0..=MAX_OUT).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
        text.truncate(cut);
        text.push_str("\n[output truncated]");
    }
    Ok(json!({"content": [{"type": "text", "text": text}], "isError": !status.success()}))
}
