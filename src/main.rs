mod mcp;
mod sixng_cli;
mod texdump;
use memmap2::Mmap;
use rpf9::{audio, extract, model, report, texture};
use report::Report;
use std::{
    env,
    error::Error,
    fs::File,
    path::{Path, PathBuf},
    process::ExitCode,
};

const USAGE: &str = "\
usage: 6ixpack [-n] [-v] <file> [outdir]
       6ixpack audio   <args...>   AWC/ADAT audio: list, extract Opus, Ogg/WAV
       6ixpack texture <args...>   RAGE textures -> DDS/PNG
       6ixpack model   <args...>   RAGE drawables -> OBJ
       6ixpack convert <kind> ...   GTA V output: ytd, ydr, ybn, ymap, ytyp (--help)
       6ixpack fivem build ...      6m world -> FiveM map resource (terrain, buildings, water)
       6ixpack redm <args...>       RDR2 / RedM output: ytd, ydr, one 6m prop -> RedM resource (--help)
       6ixpack mcp                 stdio MCP server exposing all of the above as tools
       6ixpack bcpack-calibrate --game <dir> [--out <file>] [--rounds N]
                                   fit the BCPack BC7/BC6H preset tables to the
                                   install (long run); the decoders use the file
                                   when present, else the shipped v1 tables
       6ixpack colour-textures --game <dir> [--out <dir>]
                                   every colour texture (albedo / emissive,
                                   no normal or data maps) as a top-mip PNG
       6ixpack sixng   <args...>   SixNG: placements, vehicles, GLB export, names, ... (--help)

  -n, --list     report only, extract nothing
  --no-cache     ignore and rebuild the saved scan index (%LOCALAPPDATA%/rpf9/scan)
  -v, --verbose  add every archive's header fields to the report
  --only EXTS    extract only these types (comma list of type labels, e.g.
                 pso,xml); report.txt stays complete, index.tsv lists the
                 written files only
  --threads N    worker threads for scanning (default: all cores; any command)
  -h, --help     print this help

Extracts to out/<file stem> unless outdir is given. The directory must be empty.";

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let args = rpf9::par::take_threads(&args)?;
    match args.first().and_then(|a| a.to_str()) {
        Some("audio") => return audio::run(&args[1..]),
        Some("texture") => return texture::run(&args[1..]),
        Some("model") => return model::run(&args[1..]),
        Some("mcp") => return mcp::run(),
        Some("convert") => return rpf9::gta5::run(&args[1..]),
        Some("fivem") => return rpf9::gta5::fivem::run(&args[1..]),
        Some("redm") => return rpf9::rdr2::run(&args[1..]),
        Some("bcpack-calibrate") => return bcpack_calibrate(&args[1..]),
        Some("colour-textures") => return texdump::run(&args[1..]),
        Some("sixng") => {
            sixng_cli::main(&args[1..]);
            return Ok(());
        }
        _ => {}
    }
    let (mut paths, mut list, mut verbose, mut only) = (Vec::new(), false, false, None::<Vec<String>>);
    let mut use_cache = true;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--only") => {
                let exts = args.next().and_then(|v| v.into_string().ok()).ok_or("--only needs a type list")?;
                only = Some(exts.split(',').map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).collect());
            }
            Some("-n" | "--list") => list = true,
            Some("--no-cache") => use_cache = false,
            Some("-v" | "--verbose") => verbose = true,
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some(option) if option.starts_with('-') => {
                return Err(format!("unknown option {option}\n{USAGE}").into());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let [file, rest @ ..] = paths.as_slice() else {
        return Err(USAGE.into());
    };
    let out = match rest {
        [] => Path::new("out").join(file.file_stem().unwrap_or_default()),
        [out] => out.clone(),
        _ => return Err(USAGE.into()),
    };

    // SAFETY: the map is only read, and nothing is expected to write to an
    // archive while it is being inspected.
    let map = |file| unsafe { Mmap::map(&file) };
    let data = File::open(file).and_then(map).map_err(|e| format!("{}: {e}", file.display()))?;
    let scan = rpf9::scancache::scan(file, &data, use_cache);
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let report = Report { name: &name, scan: &scan };
    if verbose {
        print!("{report:#}");
    } else {
        print!("{report}");
    }
    if !list {
        let keep = |ext: &str| only.as_ref().is_none_or(|only| only.iter().any(|o| o == ext));
        extract::extract_only(&scan, &format!("{report:#}"), &out, keep)?;
        let kept = || scan.files.iter().filter(|file| keep(file.kind.label().0));
        let files = kept().count();
        let bytes: usize = kept().map(|file| file.size()).sum();
        println!("\nwrote {files} files ({}) to {}", report::size(bytes), out.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("6ixpack: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `6ixpack bcpack-calibrate`: fits the BC7 and BC6H preset tables to the
/// game's TTPF stores and writes them where the decoders look
/// (`sixng_core::bcpack_presets::path()`, or `--out`).
fn bcpack_calibrate(args: &[std::ffi::OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let (mut game, mut out, mut rounds) = (None, None, 5usize);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--game") => game = it.next().map(std::path::PathBuf::from),
            Some("--out") => out = it.next().map(std::path::PathBuf::from),
            Some("--rounds") => rounds = it.next().and_then(|v| v.to_str()?.parse().ok()).ok_or("--rounds needs a number")?,
            _ => return Err(format!("bcpack-calibrate: unknown argument {}", a.to_string_lossy()).into()),
        }
    }
    let game = game.ok_or("bcpack-calibrate needs --game <6m dir>")?;
    let out = out.unwrap_or_else(sixng_core::bcpack_presets::path);
    if rounds > 1 {
        // Decode-and-fit rounds (GTA6-Asset-Info 0fe110e: the BC6H refit
        // converges in about four): each round is a fresh process so the
        // decoded neighbour mips are decoded with the previous round's
        // tables; stop once a round changes nothing.
        let exe = std::env::current_exe()?;
        for r in 1..=rounds {
            println!("[bcpack-calibrate] round {r}/{rounds}");
            let before = std::fs::read(&out).ok();
            let st = std::process::Command::new(&exe).arg("bcpack-calibrate").arg("--game").arg(&game).arg("--out").arg(&out).arg("--rounds").arg("1").env("SIXNG_BCPACK_PRESETS", &out).status()?;
            if !st.success() {
                return Err(format!("bcpack-calibrate round {r} failed").into());
            }
            if std::fs::read(&out).ok() == before {
                println!("[bcpack-calibrate] converged after round {r}");
                break;
            }
        }
        return Ok(());
    }
    let set = sixng_core::ttpf::global(&game);
    if set.stores.is_empty() {
        return Err(format!("no TTPF stores under {}", game.display()).into());
    }
    let p = sixng_core::bcpack_fit::calibrate(set, &|m| println!("[bcpack-calibrate] {m}"));
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&out, p.to_bytes())?;
    println!("[bcpack-calibrate] wrote {}", out.display());
    Ok(())
}
